use std::collections::HashMap;

use anyhow::{Context, anyhow};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self as m, Config, quantized_model};
use candle_transformers::quantized_var_builder;
use tokenizers::Tokenizer;

use super::languages::LANGUAGES;
use super::mel;
use super::model;
use crate::models::ModelPaths;
use crate::types::{Recognizer, Transcript};

const PRIOR_FLOOR: f32 = 0.01;
/// Whisper reads at most 224 tokens of earlier text; a list of names needs far fewer.
const HINT_TOKENS: usize = 120;
/// The most tokens decoding is started from. 12 s of fast talk is about 80.
const PREFIX_TOKENS: usize = 160;

enum Model {
    Normal(Box<model::Whisper>),
    Quantized(Box<quantized_model::Whisper>),
}

impl Model {
    fn encoder_forward(&mut self, x: &Tensor, flush: bool) -> candle_core::Result<Tensor> {
        match self {
            Self::Normal(m) => m.encoder.forward(x, flush),
            Self::Quantized(m) => m.encoder.forward(x, flush),
        }
    }

    fn decoder_forward(
        &mut self,
        x: &Tensor,
        xa: &Tensor,
        flush: bool,
    ) -> candle_core::Result<Tensor> {
        match self {
            Self::Normal(m) => m.decoder.forward(x, xa, flush),
            Self::Quantized(m) => m.decoder.forward(x, xa, flush),
        }
    }

    fn final_linear(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        match self {
            Self::Normal(m) => m.decoder.final_linear(x),
            Self::Quantized(m) => m.decoder.final_linear(x),
        }
    }

    fn reset_kv_cache(&mut self) {
        match self {
            Self::Normal(m) => m.reset_kv_cache(),
            Self::Quantized(m) => m.reset_kv_cache(),
        }
    }
}

pub struct WhisperEngine {
    /// Encode only the real audio length plus a margin instead of 30 s (see `mel::Mel::tensor`).
    pub trim: bool,
    model: Model,
    tokenizer: Tokenizer,
    mel: mel::Mel,
    device: Device,
    /// What the weights are loaded as, and so what the audio features have to be.
    dtype: DType,
    sot: u32,
    transcribe: u32,
    no_timestamps: u32,
    eot: u32,
    no_speech: u32,
    blank: Option<u32>,
    /// (code, token id) for every language the tokenizer knows.
    langs: Vec<(&'static str, u32)>,
    /// -inf for config.suppress_tokens and every special token after EOT (timestamps, no-speech, ...).
    suppress: Vec<f32>,
    sot_prev: Option<u32>,
    /// Words to expect, as text and tokens. Whisper is told they came before the audio, which
    /// steers what it hears and how it spells it.
    hint: (String, Vec<u32>),
    /// What the next audio begins with, as text and tokens: decoding goes on from there.
    prefix: (String, Vec<u32>),
}

impl WhisperEngine {
    pub fn load(paths: &ModelPaths, device: &Device) -> anyhow::Result<Self> {
        let config: Config = serde_json::from_str(
            &std::fs::read_to_string(&paths.config).context("reading whisper config")?,
        )?;
        let tokenizer =
            Tokenizer::from_file(&paths.tokenizer).map_err(|e| anyhow!("tokenizer: {e}"))?;
        // Half precision on CUDA: the weights are stored that way, and as f32 the largest model
        // takes twice the graphics memory (3.2 GB) for the same captions.
        let dtype = if device.is_cuda() && !paths.quantized {
            DType::F16
        } else {
            m::DTYPE
        };
        let model = if paths.quantized {
            let vb = quantized_var_builder::VarBuilder::from_gguf(&paths.weights, device)?;
            Model::Quantized(Box::new(quantized_model::Whisper::load(
                &vb,
                config.clone(),
            )?))
        } else {
            // SAFETY: the weights file is only read; it is not modified while mapped.
            let vb =
                unsafe { VarBuilder::from_mmaped_safetensors(&[&paths.weights], dtype, device)? };
            Model::Normal(Box::new(model::Whisper::load(&vb, config.clone())?))
        };

        let token = |s: &str| {
            tokenizer
                .token_to_id(s)
                .ok_or_else(|| anyhow!("missing token {s}"))
        };
        let sot = token(m::SOT_TOKEN)?;
        let transcribe = token(m::TRANSCRIBE_TOKEN)?;
        let no_timestamps = token(m::NO_TIMESTAMPS_TOKEN)?;
        let eot = token(m::EOT_TOKEN)?;
        let no_speech = m::NO_SPEECH_TOKENS
            .iter()
            .find_map(|t| tokenizer.token_to_id(t))
            .ok_or_else(|| anyhow!("missing no-speech token"))?;
        let blank = tokenizer.token_to_id("Ġ");
        let langs = LANGUAGES
            .iter()
            .chain(&["yue"])
            .filter_map(|&l| tokenizer.token_to_id(&format!("<|{l}|>")).map(|id| (l, id)))
            .collect();

        let mut suppress = vec![0f32; config.vocab_size];
        for &t in &config.suppress_tokens {
            if let Some(v) = suppress.get_mut(t as usize) {
                *v = f32::NEG_INFINITY;
            }
        }
        for v in suppress.iter_mut().skip(eot as usize + 1) {
            *v = f32::NEG_INFINITY;
        }

        Ok(Self {
            sot_prev: tokenizer.token_to_id("<|startofprev|>"),
            hint: Default::default(),
            prefix: Default::default(),
            trim: true,
            mel: mel::Mel::new(config.num_mel_bins, device)?,
            model,
            tokenizer,
            device: device.clone(),
            dtype,
            sot,
            transcribe,
            no_timestamps,
            eot,
            no_speech,
            blank,
            langs,
            suppress,
        })
    }

    fn last_logits(
        &mut self,
        tokens: &[u32],
        audio: &Tensor,
        flush: bool,
    ) -> anyhow::Result<Vec<f32>> {
        let t = Tensor::new(tokens, &self.device)?.unsqueeze(0)?;
        let ys = self.model.decoder_forward(&t, audio, flush)?;
        let n = ys.dim(1)?;
        let logits = self
            .model
            .final_linear(&ys.i((..1, n - 1..))?)?
            .i(0)?
            .i(0)?;
        Ok(logits.to_dtype(DType::F32)?.to_vec1()?)
    }

    fn lang_id(&self, code: &str) -> Option<u32> {
        let base = code.split(['-', '_']).next().unwrap_or(code);
        self.langs
            .iter()
            .find(|(l, _)| *l == code || *l == base)
            .map(|&(_, id)| id)
    }
}

impl Recognizer for WhisperEngine {
    fn set_hint(&mut self, hint: &str) {
        let hint = hint.trim();
        if hint == self.hint.0 {
            return;
        }
        let mut tokens = Vec::new();
        let list = hint.trim_end_matches(['.', ',']);
        if !list.is_empty()
            && let Ok(e) = self.tokenizer.encode(format!(" {list}"), false)
            && let Ok(stop) = self.tokenizer.encode(".", false)
        {
            // Whisper writes the way what came before was written: after a bare list of names
            // it leaves out full stops, which captions are cut at.
            tokens.extend(e.get_ids().iter().take(HINT_TOKENS));
            tokens.extend(stop.get_ids());
        }
        self.hint = (hint.to_string(), tokens);
    }

    fn set_prefix(&mut self, prefix: &str) {
        let prefix = prefix.trim();
        if prefix == self.prefix.0 {
            return;
        }
        let tokens = match self.tokenizer.encode(format!(" {prefix}"), false) {
            Ok(e) if !prefix.is_empty() => e.get_ids().to_vec(),
            _ => Vec::new(),
        };
        self.prefix = (prefix.to_string(), tokens);
    }

    fn transcribe(
        &mut self,
        audio: &[f32],
        language: Option<&str>,
        prior: &HashMap<String, f32>,
    ) -> anyhow::Result<Transcript> {
        let t0 = std::time::Instant::now();
        let mel = self.mel.tensor(audio, self.trim)?.to_dtype(self.dtype)?;
        self.model.reset_kv_cache();
        let features = self.model.encoder_forward(&mel, true)?;

        // Step 0: [SOT] only. Gives no-speech prob, language confidence and the language.
        let logits = self.last_logits(&[self.sot], &features, true)?;
        let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let sum: f32 = logits.iter().map(|v| (v - max).exp()).sum();
        let no_speech = (logits[self.no_speech as usize] - max).exp() / sum;
        let p: Vec<f32> = self
            .langs
            .iter()
            .map(|&(_, id)| (logits[id as usize] - max).exp())
            .collect();
        let z: f32 = p.iter().sum();
        let mut sorted = p.clone();
        sorted.sort_by(|a, b| b.total_cmp(a));
        let lang_conf = (sorted[0] + sorted.get(1).copied().unwrap_or(0.0)) / z;
        let lang = match language.and_then(|l| self.lang_id(l)) {
            Some(id) => id,
            None => {
                let score = |k: usize| {
                    p[k].ln()
                        + prior
                            .get(self.langs[k].0)
                            .copied()
                            .unwrap_or(PRIOR_FLOOR)
                            .ln()
                };
                let best = (0..p.len())
                    .max_by(|&a, &b| score(a).total_cmp(&score(b)))
                    .unwrap_or(0);
                self.langs[best].1
            }
        };
        let lang_code = self
            .langs
            .iter()
            .find(|(_, id)| *id == lang)
            .map_or("en", |(l, _)| l);

        let mut tokens = Vec::new();
        if let Some(p) = self.sot_prev.filter(|_| !self.hint.1.is_empty()) {
            tokens.push(p);
            tokens.extend(&self.hint.1);
        }
        tokens.extend([self.sot, lang, self.transcribe, self.no_timestamps]);
        let prompt_len = tokens.len();
        // Half of what the decoder can hold at most: the rest is for what follows.
        let forced = if self.prefix.1.len() <= PREFIX_TOKENS {
            self.prefix.1.len()
        } else {
            0
        };
        tokens.extend(&self.prefix.1[..forced]);
        let mut logprob = 0f32;
        let t1 = std::time::Instant::now();
        let secs = audio.len() as f32 / m::SAMPLE_RATE as f32;
        let max_new = 128.min((secs * 7.0).ceil() as usize + 8);
        for i in 0..max_new {
            let mut logits = self.last_logits(&tokens, &features, false)?;
            for (l, s) in logits.iter_mut().zip(&self.suppress) {
                *l += s;
            }
            if i == 0 && forced == 0 {
                logits[self.eot as usize] = f32::NEG_INFINITY;
                if let Some(b) = self.blank {
                    logits[b as usize] = f32::NEG_INFINITY;
                }
            }
            let next = logits
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(self.eot, |(i, _)| i as u32);
            if next == self.eot {
                break;
            }
            let max = logits[next as usize];
            logprob -= logits.iter().map(|v| (v - max).exp()).sum::<f32>().ln();
            tokens.push(next);
            if let Some(n) = repeated_tail(&tokens[prompt_len..]) {
                tokens.truncate(tokens.len() - n);
                break;
            }
        }
        let period = loop_period(&tokens[prompt_len..]);
        tokens.truncate(prompt_len + period);
        let text = self
            .tokenizer
            .decode(&tokens[prompt_len..], true)
            .map_err(|e| anyhow!("decode: {e}"))?;

        // What decoding started from counts as certain.
        let n = (tokens.len() - prompt_len).max(1) as f32;
        log::debug!(
            "asr {secs:.1}s enc {}ms dec {}ms/{n} ({forced} given) logprob {:.2} conf {lang_conf:.2}: {}",
            (t1 - t0).as_millis(),
            t1.elapsed().as_millis(),
            logprob / n,
            text.trim()
        );

        Ok(Transcript {
            text: text.trim().to_string(),
            language: lang_code.to_string(),
            no_speech,
            lang_conf,
            avg_logprob: logprob / n,
        })
    }
}

/// Length of a run of 3+ tokens that the sequence just repeated back to back (a decoding loop).
fn repeated_tail(t: &[u32]) -> Option<usize> {
    (3..=t.len() / 2).find(|&n| t[t.len() - n..] == t[t.len() - 2 * n..t.len() - n])
}

/// Whisper loops back to its first word when it runs out of audio ("A B C. A B"): the
/// shortest period (3+ tokens) of the whole sequence, or its length when it does not repeat.
fn loop_period(t: &[u32]) -> usize {
    (3..t.len().saturating_sub(2))
        .find(|&p| t[p..] == t[..t.len() - p])
        .unwrap_or(t.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn repeated_tail() {
        use super::repeated_tail;
        assert_eq!(repeated_tail(&[1, 2, 3, 4, 2, 3, 4]), Some(3));
        assert_eq!(repeated_tail(&[9, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5]), Some(5));
        assert_eq!(repeated_tail(&[1, 1, 1, 1]), None);
        assert_eq!(repeated_tail(&[1, 2, 3, 4, 5, 6]), None);
    }

    #[test]
    fn loop_period() {
        use super::loop_period;
        assert_eq!(loop_period(&[1, 2, 3, 4, 1, 2, 3]), 4);
        assert_eq!(loop_period(&[1, 2, 3, 1, 2, 3, 1, 2]), 3);
        assert_eq!(loop_period(&[1, 2, 3, 4, 1, 2]), 6);
        assert_eq!(loop_period(&[1, 2, 3, 4, 5]), 5);
        assert_eq!(loop_period(&[]), 0);
    }
}

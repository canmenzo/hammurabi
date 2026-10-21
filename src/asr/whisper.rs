use std::collections::HashMap;

use anyhow::{Context, anyhow};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self as m, Config, model, quantized_model};
use candle_transformers::quantized_var_builder;
use tokenizers::Tokenizer;

use super::languages::LANGUAGES;
use super::mel;
use crate::models::ModelPaths;
use crate::types::{Recognizer, Transcript};

const PRIOR_FLOOR: f32 = 0.01;

enum Model {
    Normal(model::Whisper),
    Quantized(quantized_model::Whisper),
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
    model: Model,
    config: Config,
    tokenizer: Tokenizer,
    filters: Vec<f32>,
    device: Device,
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
}

impl WhisperEngine {
    pub fn load(paths: &ModelPaths, device: &Device) -> anyhow::Result<Self> {
        let config: Config = serde_json::from_str(
            &std::fs::read_to_string(&paths.config).context("reading whisper config")?,
        )?;
        let tokenizer =
            Tokenizer::from_file(&paths.tokenizer).map_err(|e| anyhow!("tokenizer: {e}"))?;
        let model = if paths.quantized {
            let vb = quantized_var_builder::VarBuilder::from_gguf(&paths.weights, device)?;
            Model::Quantized(quantized_model::Whisper::load(&vb, config.clone())?)
        } else {
            // SAFETY: the weights file is only read; it is not modified while mapped.
            let vb = unsafe {
                VarBuilder::from_mmaped_safetensors(&[&paths.weights], m::DTYPE, device)?
            };
            Model::Normal(model::Whisper::load(&vb, config.clone())?)
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
            filters: mel::filters(config.num_mel_bins)?,
            model,
            config,
            tokenizer,
            device: device.clone(),
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
    fn transcribe(
        &mut self,
        audio: &[f32],
        language: Option<&str>,
        prior: &HashMap<String, f32>,
    ) -> anyhow::Result<Transcript> {
        let mel = mel::mel_tensor(&self.config, audio, &self.filters, &self.device)?;
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

        let mut tokens = vec![self.sot, lang, self.transcribe, self.no_timestamps];
        let prompt_len = tokens.len();
        let secs = audio.len() as f32 / m::SAMPLE_RATE as f32;
        let max_new = 128.min((secs * 7.0).ceil() as usize + 8);
        for i in 0..max_new {
            let mut logits = self.last_logits(&tokens, &features, false)?;
            for (l, s) in logits.iter_mut().zip(&self.suppress) {
                *l += s;
            }
            if i == 0 {
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
            tokens.push(next);
        }
        let text = self
            .tokenizer
            .decode(&tokens[prompt_len..], true)
            .map_err(|e| anyhow!("decode: {e}"))?;

        Ok(Transcript {
            text: text.trim().to_string(),
            language: lang_code.to_string(),
            no_speech,
            lang_conf,
        })
    }
}

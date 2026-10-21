use anyhow::{Context, anyhow};
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_t5::{Config, T5ForConditionalGeneration, VarBuilder};
use tokenizers::Tokenizer;

use crate::models::ModelPaths;
use crate::types::Translate;

/// MADLAD-400 3B (quantized T5). Input is `<2xx> text`; context is ignored.
pub struct Madlad {
    model: T5ForConditionalGeneration,
    tokenizer: Tokenizer,
    device: Device,
    start: u32,
    eos: u32,
}

impl Madlad {
    pub fn load(paths: &ModelPaths, device: &Device) -> anyhow::Result<Self> {
        let config: Config = serde_json::from_slice(&std::fs::read(&paths.config)?)?;
        let tokenizer = Tokenizer::from_file(&paths.tokenizer).map_err(|e| anyhow!("{e}"))?;
        let vb = VarBuilder::from_gguf(&paths.weights, device)?;
        let model = T5ForConditionalGeneration::load(vb, &config)?;
        Ok(Self {
            model,
            tokenizer,
            device: device.clone(),
            start: config.decoder_start_token_id.unwrap_or(config.pad_token_id) as u32,
            eos: config.eos_token_id as u32,
        })
    }

    /// `zh-Hant` → `<2zh_Hant>`, falling back to `<2zh>` when the vocab lacks it.
    fn lang_token(&self, target: &str) -> anyhow::Result<String> {
        let full = format!("<2{}>", target.replace('-', "_"));
        if self.tokenizer.token_to_id(&full).is_some() {
            return Ok(full);
        }
        let base = format!("<2{}>", target.split(['-', '_']).next().unwrap_or(target));
        self.tokenizer
            .token_to_id(&base)
            .map(|_| base)
            .with_context(|| format!("MADLAD has no language token for {target}"))
    }

    /// Greedy decode; returns the translation and the number of generated tokens.
    pub fn translate_counted(
        &mut self,
        text: &str,
        target: &str,
    ) -> anyhow::Result<(String, usize)> {
        let prompt = format!("{} {}", self.lang_token(target)?, text.trim());
        let input = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow!("{e}"))?;
        let input_ids = input.get_ids();
        let max_new = (input_ids.len() * 2 + 16).min(256);

        self.model.clear_kv_cache();
        let encoded = self
            .model
            .encode(&Tensor::new(input_ids, &self.device)?.unsqueeze(0)?)?;
        let mut out = vec![self.start];
        for _ in 0..max_new {
            // KV cache: only the newest token is fed after the first step.
            let last = [*out.last().unwrap()];
            let ids = Tensor::new(&last, &self.device)?.unsqueeze(0)?;
            let logits = self.model.decode(&ids, &encoded)?.squeeze(0)?;
            let next = logits.argmax(0)?.to_scalar::<u32>()?;
            if next == self.eos {
                break;
            }
            out.push(next);
        }
        let n = out.len() - 1;
        let text = self
            .tokenizer
            .decode(&out[1..], true)
            .map_err(|e| anyhow!("{e}"))?;
        Ok((join_cjk(text.trim()), n))
    }
}

/// MADLAD emits "你好嗎 ， 我的朋友 ？": drop spaces touching CJK or fullwidth chars,
/// but keep them between Hangul words.
fn join_cjk(text: &str) -> String {
    let cjk = |c: char| matches!(c, '\u{3000}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}' | '\u{FF00}'..='\u{FFEF}');
    let hangul = |c: char| ('\u{AC00}'..='\u{D7AF}').contains(&c);
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len())
        .filter(|&i| {
            let c = chars[i];
            if !c.is_whitespace() || i == 0 || i + 1 == chars.len() {
                return true;
            }
            let (a, b) = (chars[i - 1], chars[i + 1]);
            !(cjk(a) || cjk(b)) || (hangul(a) && hangul(b))
        })
        .map(|i| chars[i])
        .collect()
}

impl Translate for Madlad {
    fn translate(
        &mut self,
        text: &str,
        _source: &str,
        target: &str,
        _context: &[(String, String)],
    ) -> anyhow::Result<String> {
        Ok(self.translate_counted(text, target)?.0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn join_cjk() {
        assert_eq!(
            super::join_cjk("你好嗎 ， 我的朋友 ？"),
            "你好嗎，我的朋友？"
        );
        assert_eq!(super::join_cjk("Hello, world"), "Hello, world");
        assert_eq!(super::join_cjk("안녕하세요 친구 ？"), "안녕하세요 친구？");
    }
}

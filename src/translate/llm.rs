use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};

use crate::settings::lang_name;
use crate::types::Translate;

const USER_AGENT: &str = "OpenAI File Downloader, XaiImageApiFetch/1.0";

/// OpenAI-compatible chat endpoint, e.g. Ollama `http://localhost:11434/v1`.
pub struct LlmTranslator {
    base_url: String,
    model: String,
    agent: ureq::Agent,
}

impl LlmTranslator {
    pub fn new(base_url: &str, model: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            agent,
        }
    }

    /// Checks the endpoint is reachable.
    pub fn ping(&self) -> anyhow::Result<()> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build()
            .into();
        let url = &self.base_url;
        let mut resp = agent
            .get(&format!("{url}/models"))
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| {
                log::warn!("LLM ping {url}: {e}");
                anyhow::anyhow!(
                    "can't reach the local LLM at {url}, is Ollama or LM Studio running?"
                )
            })?;
        anyhow::ensure!(
            resp.status().is_success(),
            "the local LLM at {url} returned {}",
            resp.status()
        );
        // OpenAI-style `{"data": [{"id": ...}]}`; servers that list nothing are trusted.
        let models: Vec<String> = resp
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|b| serde_json::from_str::<Value>(&b).ok())
            .and_then(|v| v["data"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect();
        anyhow::ensure!(
            models.is_empty()
                || models
                    .iter()
                    .any(|m| *m == self.model || *m == format!("{}:latest", self.model)),
            "the local LLM has no model named {} (it has: {})",
            self.model,
            models.join(", ")
        );
        Ok(())
    }
}

impl Translate for LlmTranslator {
    fn translate(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
    ) -> anyhow::Result<String> {
        let (src, tgt) = (lang_name(source), lang_name(target));
        let system = format!(
            "You are a subtitle translator. Translate only the last line from {src} to {tgt}. \
             Earlier lines are prior subtitles with their translations, for consistency. \
             Output only the translation of the last line, with no quotes, notes or explanations."
        );
        let mut messages = vec![json!({"role": "system", "content": system})];
        for (orig, trans) in context {
            messages.push(json!({"role": "user", "content": orig}));
            messages.push(json!({"role": "assistant", "content": trans}));
        }
        messages.push(json!({"role": "user", "content": text}));

        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
            "temperature": 0.2,
        });
        let mut resp = self
            .agent
            .post(&format!("{}/chat/completions", self.base_url))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .context("LLM request failed")?;
        let status = resp.status();
        let raw = resp
            .body_mut()
            .read_to_string()
            .context("LLM response unreadable")?;
        anyhow::ensure!(status.is_success(), "LLM returned {status}: {raw}");
        let v: Value = serde_json::from_str(&raw).context("LLM response is not JSON")?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .context("LLM response has no content")?;
        Ok(clean(content))
    }
}

fn clean(s: &str) -> String {
    let mut s = s.to_string();
    while let Some(start) = s.find("<think>") {
        match s[start..].find("</think>") {
            Some(end) => s.replace_range(start..start + end + "</think>".len(), ""),
            None => {
                s.truncate(start);
                break;
            }
        }
    }
    if let Some(end) = s.rfind("</think>") {
        s.drain(..end + "</think>".len());
    }
    let quotes = ['"', '\'', '“', '”', '‘', '’', '«', '»'];
    s.trim().trim_matches(&quotes[..]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_and_quotes() {
        assert_eq!(
            clean("<think>\nhmm\n</think>\n\n\"Merhaba dünya\"\n"),
            "Merhaba dünya"
        );
        assert_eq!(clean("  “Hola”  "), "Hola");
        assert_eq!(clean("reasoning</think>Hi"), "Hi");
        assert_eq!(clean("plain"), "plain");
    }

    #[test]
    #[ignore = "needs local Ollama with a model, set HAMMURABI_LLM_MODEL"]
    fn ollama_translates() {
        let model = std::env::var("HAMMURABI_LLM_MODEL").unwrap_or_else(|_| "qwen2.5:1.5b".into());
        let mut t = LlmTranslator::new("http://localhost:11434/v1", &model);
        t.ping().unwrap();
        let out = t
            .translate("Good morning, how are you?", "en", "tr", &[])
            .unwrap();
        println!("{out}");
        assert!(!out.is_empty());
    }
}

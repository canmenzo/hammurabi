#![allow(dead_code, unused_variables)]

use crate::types::Translate;

/// OpenAI-compatible chat endpoint, e.g. Ollama `http://localhost:11434/v1`.
pub struct LlmTranslator {}

impl LlmTranslator {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {}
    }

    /// Checks the endpoint is reachable.
    pub fn ping(&self) -> anyhow::Result<()> {
        anyhow::bail!("unimplemented")
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
        anyhow::bail!("unimplemented")
    }
}

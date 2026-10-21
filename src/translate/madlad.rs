#![allow(dead_code, unused_variables)]

use candle_core::Device;

use crate::models::ModelPaths;
use crate::types::Translate;

pub struct Madlad {}

impl Madlad {
    pub fn load(paths: &ModelPaths, device: &Device) -> anyhow::Result<Self> {
        anyhow::bail!("unimplemented")
    }
}

impl Translate for Madlad {
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

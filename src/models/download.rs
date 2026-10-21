#![allow(dead_code, unused_variables)]

use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Downloads `huggingface.co/{repo}/resolve/{rev}/{file}` to `dest` via `dest.part` with Range resume.
/// `progress(done, total)`; returns an error when `cancel` is set.
pub fn fetch(
    repo: &str,
    rev: &str,
    file: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    anyhow::bail!("unimplemented")
}

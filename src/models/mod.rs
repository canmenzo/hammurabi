pub mod download;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::settings::ModelChoice;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelId {
    WhisperTiny,
    WhisperBase,
    WhisperSmall,
    WhisperTurbo,
    Madlad3bQ4k,
}

impl ModelId {
    pub const ALL: [ModelId; 5] = [
        Self::WhisperTiny,
        Self::WhisperBase,
        Self::WhisperSmall,
        Self::WhisperTurbo,
        Self::Madlad3bQ4k,
    ];

    /// `Auto` = small on CUDA/Metal, base on CPU.
    pub fn whisper(choice: ModelChoice, gpu: bool) -> ModelId {
        match choice {
            ModelChoice::Auto if gpu => Self::WhisperSmall,
            ModelChoice::Auto => Self::WhisperBase,
            ModelChoice::Tiny => Self::WhisperTiny,
            ModelChoice::Base => Self::WhisperBase,
            ModelChoice::Small => Self::WhisperSmall,
            ModelChoice::Turbo => Self::WhisperTurbo,
        }
    }
}

pub struct ModelSpec {
    pub repo: &'static str,
    pub revision: &'static str,
    /// Always `[weights, config, tokenizer]`.
    pub files: [&'static str; 3],
    pub approx_bytes: u64,
    pub label: &'static str,
}

#[derive(Clone, Debug)]
pub struct ModelPaths {
    pub weights: PathBuf,
    pub config: PathBuf,
    pub tokenizer: PathBuf,
    /// Weights are GGUF (quantized) rather than safetensors.
    pub quantized: bool,
}

const MB: u64 = 1_000_000;

pub fn spec(id: ModelId) -> &'static ModelSpec {
    const SAFETENSORS: [&str; 3] = ["model.safetensors", "config.json", "tokenizer.json"];
    // The q80 GGUF tiny (lmz/candle-whisper) ran slower than base on CPU.
    static TINY: ModelSpec = ModelSpec {
        repo: "openai/whisper-tiny",
        revision: "169d4a4341b33bc18d8881c4b69c2e104e1cc0af",
        files: SAFETENSORS,
        approx_bytes: 151 * MB,
        label: "Tiny",
    };
    static BASE: ModelSpec = ModelSpec {
        repo: "openai/whisper-base",
        revision: "e37978b90ca9030d5170a5c07aadb050351a65bb",
        files: SAFETENSORS,
        approx_bytes: 290 * MB,
        label: "Base",
    };
    static SMALL: ModelSpec = ModelSpec {
        repo: "openai/whisper-small",
        revision: "973afd24965f72e36ca33b3055d56a652f456b4d",
        files: SAFETENSORS,
        approx_bytes: 967 * MB,
        label: "Small",
    };
    static TURBO: ModelSpec = ModelSpec {
        repo: "openai/whisper-large-v3-turbo",
        revision: "41f01f3fe87f28c78e2fbf8b568835947dd65ed9",
        files: SAFETENSORS,
        approx_bytes: 1_620 * MB,
        label: "Turbo",
    };
    static MADLAD: ModelSpec = ModelSpec {
        repo: "jbochi/madlad400-3b-mt",
        revision: "bb45f1851bf13a0b8f4fcd9ecadf2cdb7cf22439",
        files: ["model-q4k.gguf", "config.json", "tokenizer.json"],
        approx_bytes: 1_670 * MB,
        label: "MADLAD-400 3B",
    };
    match id {
        ModelId::WhisperTiny => &TINY,
        ModelId::WhisperBase => &BASE,
        ModelId::WhisperSmall => &SMALL,
        ModelId::WhisperTurbo => &TURBO,
        ModelId::Madlad3bQ4k => &MADLAD,
    }
}

fn model_dir(id: ModelId) -> anyhow::Result<PathBuf> {
    Ok(crate::paths::models_dir()?.join(spec(id).repo.replace('/', "--")))
}

fn paths_in(id: ModelId, dir: &Path) -> ModelPaths {
    let [w, c, t] = spec(id).files;
    ModelPaths {
        weights: dir.join(w),
        config: dir.join(c),
        tokenizer: dir.join(t),
        quantized: w.ends_with(".gguf"),
    }
}

/// Cache layout: `models_dir()/<repo with '/' -> '--'>/<file>`.
pub fn is_cached(id: ModelId) -> bool {
    model_dir(id).is_ok_and(|d| {
        let p = paths_in(id, &d);
        [p.weights, p.config, p.tokenizer]
            .iter()
            .all(|f| f.is_file())
    })
}

/// Downloads missing files; `progress` reports cumulative bytes over all files, at most every
/// 200 ms. Concurrent calls for the same model (worker + Download button) wait for each other.
pub fn ensure(
    id: ModelId,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<ModelPaths> {
    static BUSY: Mutex<Vec<ModelId>> = Mutex::new(Vec::new());
    loop {
        let mut busy = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        if !busy.contains(&id) {
            busy.push(id);
            break;
        }
        drop(busy);
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("download cancelled");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let (mut last, mut downloaded) = (None::<Instant>, None);
    let r = ensure_locked(
        id,
        &mut |done, total| {
            downloaded = Some(done);
            if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(200)) {
                last = Some(Instant::now());
                progress(done, total);
            }
        },
        cancel,
    );
    // A final done == total tells listeners the download is over.
    if let (Ok(_), Some(d)) = (&r, downloaded) {
        progress(d, Some(d));
    }
    BUSY.lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|&m| m != id);
    r
}

fn ensure_locked(
    id: ModelId,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<ModelPaths> {
    let spec = spec(id);
    let paths = paths_in(id, &model_dir(id)?);
    let mut base = 0;
    for (name, dest) in spec
        .files
        .iter()
        .zip([&paths.weights, &paths.config, &paths.tokenizer])
    {
        if dest.is_file() {
            base += std::fs::metadata(dest).map_or(0, |m| m.len());
            continue;
        }
        let mut size = 0;
        download::fetch(
            spec.repo,
            spec.revision,
            name,
            dest,
            &mut |d, t| {
                size = d;
                progress(base + d, Some(spec.approx_bytes.max(base + t.unwrap_or(d))));
            },
            cancel,
        )?;
        base += size;
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_pinned() {
        for id in ModelId::ALL {
            let r = spec(id).revision;
            assert!(
                r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()),
                "{r}"
            );
        }
    }

    #[test]
    fn layout_and_quantized() {
        let p = paths_in(ModelId::Madlad3bQ4k, Path::new("x"));
        assert!(p.quantized && p.weights.ends_with("model-q4k.gguf"));
        assert!(!paths_in(ModelId::WhisperTiny, Path::new("x")).quantized);
        assert!(
            model_dir(ModelId::WhisperBase)
                .unwrap()
                .ends_with("openai--whisper-base")
        );
    }
}

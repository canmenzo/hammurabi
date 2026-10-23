pub mod download;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::settings::ModelChoice;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelId {
    WhisperTiny,
    WhisperBase,
    WhisperSmall,
    WhisperTurbo,
    Madlad3bQ4k,
    /// Supertonic 3 neural voices for Read aloud.
    Supertonic,
    /// OmniVoice for Read aloud in the speaker's voice. Non-commercial weights.
    #[cfg(omnivoice)]
    OmniVoice,
}

impl ModelId {
    pub const ALL: &[ModelId] = &[
        Self::WhisperTiny,
        Self::WhisperBase,
        Self::WhisperSmall,
        Self::WhisperTurbo,
        Self::Madlad3bQ4k,
        Self::Supertonic,
        #[cfg(omnivoice)]
        Self::OmniVoice,
    ];

    /// `Auto` = turbo on CUDA/Metal (faster than small there), base on CPU.
    pub fn whisper(choice: ModelChoice, gpu: bool) -> ModelId {
        match choice {
            ModelChoice::Auto if gpu => Self::WhisperTurbo,
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
    /// Whisper and MADLAD: `[weights, config, tokenizer]`.
    pub files: &'static [&'static str],
    /// sha256 of each file, checked after download.
    pub sha256: &'static [&'static str],
    /// Tried first for one of `files`, with `repo` as the fallback.
    pub mirror: Option<Mirror>,
    pub approx_bytes: u64,
    pub label: &'static str,
}

pub struct Mirror {
    pub file: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub sha256: &'static str,
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
    const SAFETENSORS: &[&str] = &["model.safetensors", "config.json", "tokenizer.json"];
    const WHISPER_TOKENIZER: &str =
        "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566";
    // The q80 GGUF tiny (lmz/candle-whisper) ran slower than base on CPU.
    static TINY: ModelSpec = ModelSpec {
        repo: "openai/whisper-tiny",
        revision: "169d4a4341b33bc18d8881c4b69c2e104e1cc0af",
        files: SAFETENSORS,
        sha256: &[
            "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395",
            "ffdccec4f3211f4c63310f2b7098f309fe70f3952cedc5e4d11e43f5b2379b98",
            WHISPER_TOKENIZER,
        ],
        mirror: None,
        approx_bytes: 151 * MB,
        label: "Tiny",
    };
    static BASE: ModelSpec = ModelSpec {
        repo: "openai/whisper-base",
        revision: "e37978b90ca9030d5170a5c07aadb050351a65bb",
        files: SAFETENSORS,
        sha256: &[
            "07cadb9f25677c8d50df603e66a98fbd842cce45047139baeb16e6219a1e807b",
            "a153c53883a6799b6f056b4a8d1a515c9926d03994682ba88a7616618d7da0c1",
            WHISPER_TOKENIZER,
        ],
        mirror: None,
        approx_bytes: 290 * MB,
        label: "Base",
    };
    static SMALL: ModelSpec = ModelSpec {
        repo: "openai/whisper-small",
        revision: "973afd24965f72e36ca33b3055d56a652f456b4d",
        files: SAFETENSORS,
        sha256: &[
            "1d7734884874f1a1513ed9aa760a4f8e97aaa02fd6d93a3a85d27b2ae9ca596b",
            "e6a2b489da1b5aed65a8eb8d1e7466fa867ad5643a8bc138ba708bd56b2875c4",
            WHISPER_TOKENIZER,
        ],
        mirror: None,
        approx_bytes: 967 * MB,
        label: "Small",
    };
    static TURBO: ModelSpec = ModelSpec {
        repo: "openai/whisper-large-v3-turbo",
        revision: "41f01f3fe87f28c78e2fbf8b568835947dd65ed9",
        files: SAFETENSORS,
        sha256: &[
            "542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1",
            "c5b526b3e3cd64cd8940dabb45e8ba726629e22d8ed389c29b552f9140daf04a",
            "297b13372ac43916285644fb9687add3cc62ee2a1adb60da3dc25cc94c1871fd",
        ],
        mirror: None,
        approx_bytes: 1_620 * MB,
        label: "Turbo",
    };
    static MADLAD: ModelSpec = ModelSpec {
        repo: "jbochi/madlad400-3b-mt",
        revision: "bb45f1851bf13a0b8f4fcd9ecadf2cdb7cf22439",
        files: &["model-q4k.gguf", "config.json", "tokenizer.json"],
        sha256: &[
            "ea6e5531a3e95213c7f0635988d119e078a655c09306e47851e15d4c0c3f9c37",
            "cad399cab799b99409a6ec2d90d72552257c2bb752861261d2016691e0643e7c",
            "a2799ccc696b752ba00c34f58726bfe253a04921ceb6cfc620400f560474790b",
        ],
        mirror: None,
        approx_bytes: 1_670 * MB,
        label: "MADLAD-400 3B",
    };
    // Babil's catalog: the official files, but the vector estimator quantized to int8 (a quarter
    // of the size, same speech) with the official full-size one as the fallback.
    static SUPERTONIC: ModelSpec = ModelSpec {
        repo: "supertone-oss-archive/supertonic-3",
        revision: "aafc6e32416a594460b32413efc49d7fe4ce6d46",
        files: &[
            "onnx/tts.json",
            "onnx/unicode_indexer.json",
            "onnx/duration_predictor.onnx",
            "onnx/text_encoder.onnx",
            "onnx/vector_estimator.onnx",
            "onnx/vocoder.onnx",
            "voice_styles/F1.json",
            "voice_styles/F2.json",
            "voice_styles/F3.json",
            "voice_styles/F4.json",
            "voice_styles/F5.json",
            "voice_styles/M1.json",
            "voice_styles/M2.json",
            "voice_styles/M3.json",
            "voice_styles/M4.json",
            "voice_styles/M5.json",
        ],
        sha256: &[
            "42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09",
            "9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f",
            "c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db",
            "c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff",
            "883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c",
            "085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba",
            "bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2",
            "7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6",
            "12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab",
            "c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b",
            "45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d",
            "e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b",
            "b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50",
            "ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b",
            "ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303",
            "dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2",
        ],
        mirror: Some(Mirror {
            file: "onnx/vector_estimator.onnx",
            repo: "payam1394/traxlate-supertonic-3-q8ve",
            revision: "83bd582620fcb82dac18b39d21febf706638cf31",
            sha256: "073575704bf0b6ca0714eb6509029f324b794d5bd6dbdb27dccbbc5939abc167",
        }),
        approx_bytes: 210 * MB,
        label: "Supertonic 3",
    };
    // CC-BY-NC-4.0 (k2-fsa/OmniVoice), GGUF conversion for omnivoice.cpp.
    #[cfg(omnivoice)]
    static OMNIVOICE: ModelSpec = ModelSpec {
        repo: "Serveurperso/OmniVoice-GGUF",
        revision: "017094167b5c9ed565a5076ac9b3b93c5ecf5c73",
        files: &[crate::tts::omnivoice::MODEL, crate::tts::omnivoice::CODEC],
        sha256: &[
            "2882d887921798aea13d45236556bdf8012842ab6f8cd2690943eead6289f298",
            "75204fa566a8e30984e7a1066da6557184c9fd099c8f1bc0cb5b9415edfec255",
        ],
        mirror: None,
        approx_bytes: 945 * MB,
        label: "OmniVoice",
    };
    match id {
        ModelId::WhisperTiny => &TINY,
        ModelId::WhisperBase => &BASE,
        ModelId::WhisperSmall => &SMALL,
        ModelId::WhisperTurbo => &TURBO,
        ModelId::Madlad3bQ4k => &MADLAD,
        ModelId::Supertonic => &SUPERTONIC,
        #[cfg(omnivoice)]
        ModelId::OmniVoice => &OMNIVOICE,
    }
}

/// Cache layout: `models_dir()/<repo with '/' -> '--'>/<file>`.
pub fn model_dir(id: ModelId) -> anyhow::Result<PathBuf> {
    Ok(crate::paths::models_dir()?.join(spec(id).repo.replace('/', "--")))
}

/// The first three files as weights, config and tokenizer (the directory where a pack has fewer).
fn paths_in(id: ModelId, dir: &Path) -> ModelPaths {
    let file = |i: usize| dir.join(spec(id).files.get(i).copied().unwrap_or_default());
    ModelPaths {
        weights: file(0),
        config: file(1),
        tokenizer: file(2),
        quantized: spec(id).files[0].ends_with(".gguf"),
    }
}

pub fn is_cached(id: ModelId) -> bool {
    model_dir(id).is_ok_and(|d| spec(id).files.iter().all(|f| d.join(f).is_file()))
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

/// Deletes cached files of `id` whose sha256 does not match; true when one was deleted.
pub fn remove_corrupt(id: ModelId) -> bool {
    let Ok(dir) = model_dir(id) else {
        return false;
    };
    let spec = spec(id);
    let mut removed = false;
    for (name, &sha) in spec.files.iter().zip(spec.sha256) {
        let f = dir.join(name);
        let mirror = spec.mirror.as_ref().filter(|m| m.file == *name);
        if f.is_file()
            && download::sha256_file(&f)
                .is_ok_and(|h| h != sha && mirror.is_none_or(|m| h != m.sha256))
            && std::fs::remove_file(&f).is_ok()
        {
            log::warn!("deleted damaged model file {}", f.display());
            removed = true;
        }
    }
    removed
}

/// `ensure`, then `load`. When loading fails because a cached file is damaged, that file is
/// downloaded again and loading is retried once.
pub fn ensure_and_load<T>(
    id: ModelId,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
    load: impl Fn(&ModelPaths) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let paths = ensure(id, progress, cancel)?;
    match load(&paths) {
        Err(e) if remove_corrupt(id) => {
            log::warn!("{} failed to load: {e:#}", spec(id).label);
            load(&ensure(id, progress, cancel)?)
        }
        r => r,
    }
}

fn ensure_locked(
    id: ModelId,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<ModelPaths> {
    let spec = spec(id);
    let dir = model_dir(id)?;
    let mut base = 0;
    for (name, &sha) in spec.files.iter().zip(spec.sha256) {
        let dest = dir.join(name);
        if dest.is_file() {
            base += std::fs::metadata(&dest).map_or(0, |m| m.len());
            continue;
        }
        let mut size = 0;
        let mut fetch = |repo, rev, sha| {
            download::fetch(
                repo,
                rev,
                name,
                sha,
                &dest,
                &mut |d, t| {
                    size = d;
                    progress(base + d, Some(spec.approx_bytes.max(base + t.unwrap_or(d))));
                },
                cancel,
            )
        };
        let r = match spec.mirror.as_ref().filter(|m| m.file == *name) {
            Some(m) => fetch(m.repo, m.revision, m.sha256).or_else(|e| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(e);
                }
                log::warn!("{name}: {e:#}; trying {}", spec.repo);
                // The mirror's partial file is not a prefix of the official one.
                let mut part = dest.clone().into_os_string();
                part.push(".part");
                let _ = std::fs::remove_file(part);
                fetch(spec.repo, spec.revision, sha)
            }),
            None => fetch(spec.repo, spec.revision, sha),
        };
        r.with_context(|| format!("couldn't download {}", spec.label))?;
        base += size;
    }
    Ok(paths_in(id, &dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_pinned() {
        for &id in ModelId::ALL {
            let s = spec(id);
            for r in [s.revision]
                .into_iter()
                .chain(s.mirror.as_ref().map(|m| m.revision))
            {
                assert!(
                    r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{r}"
                );
            }
            assert_eq!(s.files.len(), s.sha256.len());
            assert!(s.mirror.as_ref().is_none_or(|m| s.files.contains(&m.file)));
            for h in s.sha256.iter().chain(s.mirror.as_ref().map(|m| &m.sha256)) {
                assert!(
                    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{h}"
                );
            }
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

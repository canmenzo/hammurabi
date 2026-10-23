//! Speaker's voice: OmniVoice (k2-fsa) on the GPU through omnivoice.cpp and GGML's Vulkan backend,
//! reading captions in the voice of whoever is being captioned. `cfg(omnivoice)` (Windows builds
//! with the `omnivoice` feature); the C shim is src/tts/omnivoice_shim.c.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use anyhow::bail;

use crate::settings::lang_name;

pub const SAMPLE_RATE: u32 = 24_000;
/// MaskGIT steps; OmniVoice's default is 32, which takes twice as long.
pub const STEPS: i32 = 16;
const SEED: u64 = 42;
/// Codec frames per second.
const FPS: f32 = 25.0;
/// Cloned speech follows the reference's pace; it may run at most this much longer than
/// OmniVoice's own voice would read the same text.
const MAX_SLOWER: f32 = 1.1;
/// The first Vulkan GPU.
const DEVICE: &str = "Vulkan0";
pub const MODEL: &str = "omnivoice-base-Q8_0.gguf";
pub const CODEC: &str = "omnivoice-tokenizer-Q8_0.gguf";

unsafe extern "C" {
    fn hm_ov_error() -> *const c_char;
    fn hm_ov_init(model: *const c_char, codec: *const c_char, device: *const c_char)
    -> *mut c_void;
    fn hm_ov_extract_ref(
        ctx: *mut c_void,
        pcm24k: *const f32,
        n: c_int,
        text: *const c_char,
    ) -> *mut c_void;
    fn hm_ov_ref_frames(r: *const c_void) -> c_int;
    fn hm_ov_free_ref(r: *mut c_void);
    fn hm_ov_synth(
        ctx: *mut c_void,
        r: *const c_void,
        text: *const c_char,
        lang: *const c_char,
        steps: c_int,
        seed: u64,
        seconds: f32,
        out: *mut *mut f32,
        n: *mut c_int,
    ) -> c_int;
    fn hm_ov_free_audio(samples: *mut f32, n: c_int);
    fn hm_ov_free(ctx: *mut c_void);
}

/// omnivoice.cpp's log lines (ov_log), called from any thread.
#[unsafe(no_mangle)]
extern "C" fn hammurabi_ov_log(level: c_int, msg: *const c_char) {
    let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    match level {
        2.. => log::warn!("omnivoice: {}", msg.trim_end()),
        _ => log::debug!("omnivoice: {}", msg.trim_end()),
    }
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(hm_ov_error()) }
        .to_string_lossy()
        .into_owned()
}

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap()
}

/// OmniVoice's name for caption language `code`; "" lets it tell from the text.
pub fn language(code: &str) -> &str {
    match code {
        "ar" => "Standard Arabic",
        "zh-Hant" => "Chinese",
        c if lang_name(c) != c => lang_name(c),
        _ => "",
    }
}

/// Letter weights of OmniVoice's duration rule (src/duration-estimator.h), roughly: only used to
/// compare paces, so marks and rarer scripts count as letters.
fn weight(text: &str) -> f32 {
    text.chars()
        .map(|c| match c as u32 {
            _ if c.is_whitespace() => 0.2,
            _ if c.is_numeric() => 3.5,
            _ if !c.is_alphabetic() => 0.5,
            0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000.. => 3.0,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => 2.5,
            0x3040..=0x30FF => 2.2,
            0x0900..=0x0DFF | 0x1000..=0x109F | 0x1780..=0x17FF => 1.8,
            0x0590..=0x06FF | 0x0750..=0x077F | 0x0E00..=0x0EFF | 0xFB1D..=0xFEFF => 1.5,
            _ => 1.0,
        })
        .sum()
}

/// Frames OmniVoice gives `text` when the reference reads `ref_weight` in `ref_frames`.
fn frames(text: &str, ref_weight: f32, ref_frames: f32) -> f32 {
    let est = weight(text) * ref_frames / ref_weight;
    if est < 50.0 {
        50.0 * (est / 50.0).powf(1.0 / 3.0)
    } else {
        est
    }
}

/// Seconds to force for `text` read in a voice learned from `ref_frames` of `ref_text`, or 0 to
/// keep the reference's pace when it is not much slower than OmniVoice's own voice.
fn forced_seconds(text: &str, ref_text: &str, ref_frames: i32) -> f32 {
    // Without a reference OmniVoice paces text like "Nice to meet you." in 1 s.
    let own = frames(text, weight("Nice to meet you."), FPS);
    let cloned = frames(text, weight(ref_text), ref_frames as f32);
    if cloned > own * MAX_SLOWER {
        own * MAX_SLOWER / FPS
    } else {
        0.0
    }
}

struct VoiceRef {
    ptr: *mut c_void,
    text: String,
    frames: i32,
}

impl Drop for VoiceRef {
    fn drop(&mut self) {
        unsafe { hm_ov_free_ref(self.ptr) }
    }
}

/// One OmniVoice context; use it from one thread only.
pub struct OmniVoice {
    ctx: *mut c_void,
    voice: Option<VoiceRef>,
}

impl OmniVoice {
    /// Loads the pack downloaded to `dir` (`models::ModelId::OmniVoice`) onto the GPU and warms it
    /// up: the first synthesis on a new install compiles Vulkan shaders (about 18 s).
    pub fn load(dir: &Path) -> anyhow::Result<OmniVoice> {
        let path = |f: &str| cstr(&dir.join(f).to_string_lossy());
        let (model, codec, device) = (path(MODEL), path(CODEC), cstr(DEVICE));
        let ctx = unsafe { hm_ov_init(model.as_ptr(), codec.as_ptr(), device.as_ptr()) };
        if ctx.is_null() {
            bail!("it did not start on the GPU ({DEVICE}): {}", last_error());
        }
        let mut ov = OmniVoice { ctx, voice: None };
        ov.synthesize("Hello.", "en", STEPS)?;
        Ok(ov)
    }

    pub fn has_voice(&self) -> bool {
        self.voice.is_some()
    }

    /// Reads in OmniVoice's own voice again.
    pub fn forget(&mut self) {
        self.voice = None;
    }

    /// Learns the voice in `pcm` (mono at `rate`, about 3 to 10 s) that says `text`.
    pub fn learn(&mut self, pcm: &[f32], rate: u32, text: &str) -> anyhow::Result<()> {
        let pcm = if rate == SAMPLE_RATE {
            pcm.to_vec()
        } else {
            super::supertonic::resample(pcm, rate, SAMPLE_RATE)?
        };
        let text_c = cstr(text);
        let ptr = unsafe {
            hm_ov_extract_ref(self.ctx, pcm.as_ptr(), pcm.len() as c_int, text_c.as_ptr())
        };
        if ptr.is_null() {
            bail!("it could not learn the voice: {}", last_error());
        }
        let frames = unsafe { hm_ov_ref_frames(ptr) };
        self.voice = Some(VoiceRef {
            ptr,
            text: text.into(),
            frames,
        });
        Ok(())
    }

    /// 24 kHz mono speech for `text` in caption language `lang`, in the learned voice (OmniVoice's
    /// own voice before one is learned).
    pub fn synthesize(&mut self, text: &str, lang: &str, steps: i32) -> anyhow::Result<Vec<f32>> {
        let (r, seconds) = match &self.voice {
            Some(v) => (v.ptr.cast_const(), forced_seconds(text, &v.text, v.frames)),
            None => (ptr::null(), 0.0),
        };
        let (text, lang) = (cstr(text), cstr(language(lang)));
        let (mut out, mut n) = (ptr::null_mut(), 0);
        let rc = unsafe {
            hm_ov_synth(
                self.ctx,
                r,
                text.as_ptr(),
                lang.as_ptr(),
                steps,
                SEED,
                seconds,
                &mut out,
                &mut n,
            )
        };
        if rc != 0 {
            bail!("synthesis failed ({rc}): {}", last_error());
        }
        let pcm = unsafe { std::slice::from_raw_parts(out, n as usize) }.to_vec();
        unsafe { hm_ov_free_audio(out, n) };
        Ok(pcm)
    }
}

impl Drop for OmniVoice {
    fn drop(&mut self) {
        self.voice = None;
        unsafe { hm_ov_free(self.ctx) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_known_to_omnivoice() {
        let table = include_str!("../../third_party/omnivoice.cpp/src/lang-map.h");
        for c in crate::settings::TARGET_LANGS
            .iter()
            .chain(crate::settings::SOURCE_LANGS.iter())
        {
            let name = language(c).to_lowercase();
            assert!(table.contains(&format!("{{ \"{name}\",")), "{c}: {name}");
        }
        assert_eq!(language("tr"), "Turkish");
        assert_eq!(language("sw"), "");
    }

    #[test]
    fn slow_voices_keep_pace() {
        let text = "Bu cümle konuşanın kendi sesiyle okunuyor.";
        let own = frames(text, weight("Nice to meet you."), FPS);
        assert!((own / FPS - 2.6).abs() < 0.3, "{own}");
        // A reference read at the own voice's pace keeps its own length.
        let r = "Merhaba! Altyazıları bu sesle okuyacağım.";
        let normal = (weight(r) / weight("Nice to meet you.") * FPS) as i32;
        assert_eq!(forced_seconds(text, r, normal), 0.0);
        // One read twice as slowly is held to 10% longer than the own voice.
        let s = forced_seconds(text, r, normal * 2);
        assert!((s - own * MAX_SLOWER / FPS).abs() < 1e-3, "{s}");
        assert_eq!(weight("ab 1."), 2.0 + 0.2 + 3.5 + 0.5);
        assert_eq!(weight("日本"), 6.0);
    }
}

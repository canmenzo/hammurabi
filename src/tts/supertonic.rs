//! Supertonic 3 neural voices on ONNX Runtime (CPU). Ported from Babil's src/tts/supertonic.js,
//! itself from Supertonic's web/helper.js (MIT, Copyright (c) 2025 Supertone Inc.).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use ort::session::Session;
use ort::value::Tensor;
use regex::Regex;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::POLL;

pub const LANGS: [&str; 22] = [
    "tr", "en", "es", "fr", "de", "it", "pt", "ru", "ar", "hi", "ja", "ko", "id", "vi", "nl", "pl",
    "uk", "el", "sv", "ro", "cs", "hu",
];
const STYLES: [&str; 10] = ["F1", "F2", "F3", "F4", "F5", "M1", "M2", "M3", "M4", "M5"];
/// Flow-matching steps of the vector estimator.
const STEPS: usize = 4;

const REPLACE: [(&str, &str); 20] = [
    ("–", "-"),
    ("‑", "-"),
    ("\u{2014}", "-"),
    ("_", " "),
    ("“", "\""),
    ("”", "\""),
    ("‘", "'"),
    ("’", "'"),
    ("´", "'"),
    ("`", "'"),
    ("[", " "),
    ("]", " "),
    ("|", " "),
    ("/", " "),
    ("#", " "),
    ("→", " "),
    ("←", " "),
    ("@", " at "),
    ("e.g.,", "for example, "),
    ("i.e.,", "that is, "),
];

fn preprocess(text: &str, lang: &str) -> String {
    static CLEAN: LazyLock<[(Regex, &str); 6]> = LazyLock::new(|| {
        [
            (r"[\x{1F600}-\x{1F64F}\x{1F300}-\x{1F5FF}\x{1F680}-\x{1F6FF}\x{1F700}-\x{1F77F}\x{1F780}-\x{1F7FF}\x{1F800}-\x{1F8FF}\x{1F900}-\x{1F9FF}\x{1FA00}-\x{1FA6F}\x{1FA70}-\x{1FAFF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}\x{1F1E6}-\x{1F1FF}]+", ""),
            (r"[♥☆♡©\\]", ""),
            (r" ([,.!?;:'])", "$1"),
            (r#""{2,}"#, "\""),
            (r"'{2,}", "'"),
            (r"\s+", " "),
        ]
        .map(|(re, to)| (Regex::new(re).unwrap(), to))
    });
    let mut t: String = text.nfkd().collect();
    t = CLEAN[0].0.replace_all(&t, "").into_owned();
    for (k, v) in REPLACE {
        t = t.replace(k, v);
    }
    for (re, to) in &CLEAN[1..] {
        t = re.replace_all(&t, *to).into_owned();
    }
    let mut t = t.trim().to_string();
    if !t.ends_with([
        '.', '!', '?', ';', ':', ',', '\'', '"', ')', ']', '}', '…', '。', '」', '』', '】', '〉',
        '》', '›', '»',
    ]) {
        t.push('.');
    }
    format!("<{lang}>{t}</{lang}>")
}

/// Splits after sentence punctuation, except after common abbreviations and initials.
fn sentences(para: &str) -> Vec<&str> {
    const ABBREV: [&str; 19] = [
        "Mr.", "Mrs.", "Ms.", "Dr.", "Prof.", "Sr.", "Jr.", "Ph.D.", "etc.", "e.g.", "i.e.", "vs.",
        "Inc.", "Ltd.", "Co.", "Corp.", "St.", "Ave.", "Blvd.",
    ];
    static END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]\s+").unwrap());
    static INITIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z]\.$").unwrap());
    let mut out = Vec::new();
    let mut start = 0;
    for m in END.find_iter(para) {
        let head = &para[..m.start() + 1];
        if ABBREV.iter().any(|a| head.ends_with(a)) || INITIAL.is_match(head) {
            continue;
        }
        out.push(&para[start..m.start() + 1]);
        start = m.end();
    }
    out.push(&para[start..]);
    out
}

/// Paragraphs, then sentences joined up to `max` characters.
fn chunks(text: &str, max: usize) -> Vec<String> {
    static PARA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n+").unwrap());
    let len = |s: &str| s.encode_utf16().count();
    let mut out = Vec::new();
    for para in PARA.split(text.trim()) {
        let mut cur = String::new();
        for s in sentences(para.trim()) {
            if !cur.is_empty() && len(&cur) + len(s) + 1 > max {
                out.push(std::mem::replace(&mut cur, s.into()));
            } else {
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(s);
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

fn style_tensor(v: &Value) -> anyhow::Result<Tensor<f32>> {
    fn flatten(v: &Value, out: &mut Vec<f32>) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| flatten(x, out)),
            x => out.push(x.as_f64().unwrap_or(0.0) as f32),
        }
    }
    let dims: Vec<usize> = serde_json::from_value(v["dims"].clone())?;
    let mut data = Vec::new();
    flatten(&v["data"], &mut data);
    Ok(Tensor::from_array((dims, data))?)
}

pub struct Supertonic {
    dp: Session,
    enc: Session,
    est: Session,
    voc: Session,
    indexer: Vec<i64>,
    /// (style_ttl, style_dp) by style name.
    styles: HashMap<&'static str, (Tensor<f32>, Tensor<f32>)>,
    pub sample_rate: u32,
    chunk_size: usize,
    dim: usize,
}

impl Supertonic {
    /// Loads the pack downloaded to `dir` (`models::ModelId::Supertonic`).
    pub fn load(dir: &Path) -> anyhow::Result<Supertonic> {
        let json = |name: &str| -> anyhow::Result<Value> {
            let p = dir.join(name);
            let bytes = std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?;
            Ok(serde_json::from_slice(&bytes)?)
        };
        let cfg = json("onnx/tts.json")?;
        let num = |v: &Value| v.as_u64().context("tts.json is missing a value");
        let compress = num(&cfg["ttl"]["chunk_compress_factor"])? as usize;
        let env = ort::init().with_name("hammurabi").build()?;
        let session = |name: &str| -> anyhow::Result<Session> {
            Ok(Session::builder(&env)?.commit_from_file(dir.join("onnx").join(name))?)
        };
        let mut styles = HashMap::new();
        for s in STYLES {
            let v = json(&format!("voice_styles/{s}.json"))?;
            styles.insert(
                s,
                (
                    style_tensor(&v["style_ttl"])?,
                    style_tensor(&v["style_dp"])?,
                ),
            );
        }
        Ok(Supertonic {
            dp: session("duration_predictor.onnx")?,
            enc: session("text_encoder.onnx")?,
            est: session("vector_estimator.onnx")?,
            voc: session("vocoder.onnx")?,
            indexer: serde_json::from_value(json("onnx/unicode_indexer.json")?)?,
            styles,
            sample_rate: num(&cfg["ae"]["sample_rate"])? as u32,
            chunk_size: num(&cfg["ae"]["base_chunk_size"])? as usize * compress,
            dim: num(&cfg["ttl"]["latent_dim"])? as usize * compress,
        })
    }

    fn infer(
        &mut self,
        text: &str,
        lang: &str,
        style: &str,
        speed: f32,
    ) -> anyhow::Result<Vec<f32>> {
        let s = preprocess(text, lang);
        let ids: Vec<i64> = s
            .chars()
            .map(|c| self.indexer.get(c as usize).copied().unwrap_or(-1))
            .collect();
        let n = ids.len();
        let (ttl, dp) = &self.styles[style];
        let text_ids = Tensor::from_array(([1, n], ids))?;
        let text_mask = Tensor::from_array(([1, 1, n], vec![1f32; n]))?;
        let out = self.dp.run(ort::inputs! {
            "text_ids" => &text_ids,
            "style_dp" => dp,
            "text_mask" => &text_mask,
        })?;
        let duration = out["duration"].try_extract_tensor::<f32>()?.1[0];
        let samples =
            (f64::from(duration) / f64::from(speed) * f64::from(self.sample_rate)) as usize;
        let enc = self.enc.run(ort::inputs! {
            "text_ids" => &text_ids,
            "style_ttl" => ttl,
            "text_mask" => &text_mask,
        })?;
        let text_emb = &enc["text_emb"];

        let len = samples.div_ceil(self.chunk_size);
        let mut xt: Vec<f32> = (0..self.dim * len)
            .map(|_| {
                let u1 = rand::random::<f64>().max(0.0001);
                let u2 = rand::random::<f64>();
                ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
            })
            .collect();
        let latent_mask = Tensor::from_array(([1, 1, len], vec![1f32; len]))?;
        let total_step = Tensor::from_array(([1], vec![STEPS as f32]))?;
        for step in 0..STEPS {
            let out = self.est.run(ort::inputs! {
                "noisy_latent" => Tensor::from_array(([1, self.dim, len], xt))?,
                "text_emb" => text_emb,
                "style_ttl" => ttl,
                "latent_mask" => &latent_mask,
                "text_mask" => &text_mask,
                "current_step" => Tensor::from_array(([1], vec![step as f32]))?,
                "total_step" => &total_step,
            })?;
            xt = out["denoised_latent"]
                .try_extract_tensor::<f32>()?
                .1
                .to_vec();
        }
        let out = self.voc.run(ort::inputs! {
            "latent" => Tensor::from_array(([1, self.dim, len], xt))?,
        })?;
        let wav = out["wav_tts"].try_extract_tensor::<f32>()?.1;
        Ok(wav[..samples.min(wav.len())].to_vec())
    }

    /// Mono samples at `sample_rate` for `text` in `lang` (one of `LANGS`) with voice `style`
    /// ("F1".."M5"). `speed` is final: 1.05 is Supertonic's normal.
    pub fn synthesize(
        &mut self,
        text: &str,
        lang: &str,
        style: &str,
        speed: f32,
    ) -> anyhow::Result<Vec<f32>> {
        let max = if matches!(lang, "ja" | "ko") {
            120
        } else {
            300
        };
        let gap = (0.3 * self.sample_rate as f32) as usize;
        let mut pcm = Vec::new();
        for (i, c) in chunks(text, max).iter().enumerate() {
            if i > 0 {
                pcm.resize(pcm.len() + gap, 0.0);
            }
            pcm.extend(self.infer(c, lang, style, speed)?);
        }
        // Supertonic now and then overshoots; scale down instead of clipping.
        let peak = pcm.iter().fold(0f32, |m, x| m.max(x.abs()));
        if peak > 1.0 {
            pcm.iter_mut().for_each(|x| *x /= peak);
        }
        Ok(pcm)
    }
}

pub(super) fn resample(pcm: &[f32], from: u32, to: u32) -> anyhow::Result<Vec<f32>> {
    let mut r = Fft::<f32>::new(from as usize, to as usize, 1024, 1, FixedSync::Both)?;
    let mut out = vec![0.0; r.process_all_needed_output_len(pcm.len())];
    let n = out.len();
    let input = InterleavedSlice::new(pcm, 1, pcm.len())?;
    let mut output = InterleavedSlice::new_mut(&mut out, 1, n)?;
    let (_, written) = r.process_all_into_buffer(&input, &mut output, pcm.len(), None)?;
    out.truncate(written);
    Ok(out)
}

fn build<T>(
    device: &Device,
    cfg: StreamConfig,
    pcm: Vec<f32>,
    pos: Arc<AtomicUsize>,
) -> anyhow::Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = cfg.channels.max(1) as usize;
    Ok(device.build_output_stream(
        cfg,
        move |data: &mut [T], _: &_| {
            let mut i = pos.load(Ordering::Relaxed);
            for frame in data.chunks_mut(channels) {
                frame.fill(T::from_sample(pcm.get(i).copied().unwrap_or(0.0)));
                i += 1;
            }
            pos.store(i.min(pcm.len()), Ordering::Relaxed);
        },
        // Core Audio reports an xrun as the stream starts.
        |e: cpal::Error| match e.kind() {
            cpal::ErrorKind::Xrun => log::debug!("speech output: {e}"),
            _ => log::warn!("speech output: {e}"),
        },
        None,
    )?)
}

/// Plays mono `pcm` at `rate` on the default output device; returns when it has played or
/// `cancel` is set.
pub fn play(pcm: &[f32], rate: u32, cancel: &AtomicBool) -> anyhow::Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no audio output device")?;
    let config = device.default_output_config()?;
    let out_rate = config.sample_rate();
    let pcm = if out_rate == rate {
        pcm.to_vec()
    } else {
        resample(pcm, rate, out_rate)?
    };
    let len = pcm.len();
    let pos = Arc::new(AtomicUsize::new(0));
    let cfg = config.config();
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, cfg, pcm, pos.clone()),
        SampleFormat::F64 => build::<f64>(&device, cfg, pcm, pos.clone()),
        SampleFormat::I16 => build::<i16>(&device, cfg, pcm, pos.clone()),
        SampleFormat::I32 => build::<i32>(&device, cfg, pcm, pos.clone()),
        SampleFormat::U16 => build::<u16>(&device, cfg, pcm, pos.clone()),
        f => anyhow::bail!("unsupported output sample format {f} on {device}"),
    }?;
    stream.play()?;
    // A device that stops pulling data ends playback a little after the audio's length.
    let deadline = Instant::now() + Duration::from_secs_f64(len as f64 / f64::from(out_rate) + 2.0);
    while pos.load(Ordering::Relaxed) < len && Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    // The last buffer handed over is still in the device.
    std::thread::sleep(Duration::from_millis(150));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocesses_like_babil() {
        assert_eq!(preprocess("Hello  world", "en"), "<en>Hello world.</en>");
        assert_eq!(preprocess("Wait , what ?", "en"), "<en>Wait, what?</en>");
        assert_eq!(
            preprocess("“Hi” \u{2014} e.g., you@home 😀", "en"),
            "<en>\"Hi\" - for example, you at home.</en>"
        );
        // NFKD: ş is s plus a combining cedilla.
        assert_eq!(preprocess("Ateş", "tr"), "<tr>Ates\u{327}.</tr>");
    }

    #[test]
    fn splits_sentences() {
        assert_eq!(
            sentences("Dr. Smith is here. He met J. Doe! Fine?"),
            ["Dr. Smith is here.", "He met J. Doe!", "Fine?"]
        );
        assert_eq!(chunks("One. Two.\n\nThree.", 300), ["One. Two.", "Three."]);
        assert_eq!(chunks("Aaaa. Bbbb. Cccc.", 11), ["Aaaa. Bbbb.", "Cccc."]);
        assert!(chunks("  ", 300).is_empty());
    }

    #[test]
    fn resamples() {
        let pcm = vec![0.5f32; 44_100];
        let out = resample(&pcm, 44_100, 48_000).unwrap();
        assert!((out.len() as i64 - 48_000).abs() < 10, "{}", out.len());
        assert!((out[24_000] - 0.5).abs() < 0.01);
    }
}

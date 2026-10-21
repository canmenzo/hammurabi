//! cargo run --release --example transcribe_wav -- <file.wav> [--model tiny|base|small|turbo] [--lang xx]
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{Context, bail};
use hammurabi::asr::whisper::WhisperEngine;
use hammurabi::models::{self, ModelId};
use hammurabi::settings::ModelChoice;
use hammurabi::types::{Recognizer, SAMPLE_RATE};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut file, mut choice, mut lang) = (None, ModelChoice::Auto, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => {
                choice = match args.next().as_deref() {
                    Some("tiny") => ModelChoice::Tiny,
                    Some("base") => ModelChoice::Base,
                    Some("small") => ModelChoice::Small,
                    Some("turbo") => ModelChoice::Turbo,
                    m => bail!("unknown model {m:?}"),
                }
            }
            "--lang" => lang = args.next(),
            _ => file = Some(a),
        }
    }
    let file = file
        .context("usage: transcribe_wav <file.wav> [--model tiny|base|small|turbo] [--lang xx]")?;

    let mut reader = hound::WavReader::open(&file)?;
    let spec = reader.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let ch = spec.channels as usize;
    let mono: Vec<f32> = raw
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect();
    // Naive linear resample; fine for a test tool.
    let pcm: Vec<f32> = if spec.sample_rate == SAMPLE_RATE {
        mono
    } else {
        let ratio = spec.sample_rate as f64 / SAMPLE_RATE as f64;
        let n = (mono.len() as f64 / ratio) as usize;
        (0..n)
            .map(|i| {
                let x = i as f64 * ratio;
                let (j, f) = (x as usize, (x.fract()) as f32);
                let a = mono[j];
                let b = *mono.get(j + 1).unwrap_or(&a);
                a + (b - a) * f
            })
            .collect()
    };

    let (device, label) = hammurabi::device::best_device(true);
    let id = ModelId::whisper(choice, !device.is_cpu());
    let paths = models::ensure(
        id,
        &mut |d, t| {
            eprint!(
                "\rdownloading {} MB / {:?} MB",
                d / 1_000_000,
                t.map(|t| t / 1_000_000)
            )
        },
        &AtomicBool::new(false),
    )?;
    let t0 = Instant::now();
    let mut engine = WhisperEngine::load(&paths, &device)?;
    println!(
        "model {:?} on {label}, loaded in {} ms",
        id,
        t0.elapsed().as_millis()
    );

    let prior = HashMap::new();
    for (i, chunk) in pcm.chunks(5 * SAMPLE_RATE as usize).enumerate() {
        let t0 = Instant::now();
        let t = engine.transcribe(chunk, lang.as_deref(), &prior)?;
        println!(
            "[{:>3}s] {:?}  lang={} no_speech={:.3} lang_conf={:.3} ms={}",
            i * 5,
            t.text,
            t.language,
            t.no_speech,
            t.lang_conf,
            t0.elapsed().as_millis()
        );
    }
    Ok(())
}

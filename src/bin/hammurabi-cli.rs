//! Command-line tools: list devices, record, caption a WAV file through the real pipeline, download models.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use crossbeam_channel::RecvTimeoutError;
use hammurabi::asr::whisper::WhisperEngine;
use hammurabi::audio::{AudioCapture, Frames16k, list_devices};
use hammurabi::models::{self, ModelId, ModelPaths};
use hammurabi::pipeline::{self, Engine};
use hammurabi::settings::{ModelChoice, Settings, TranslatorChoice};
use hammurabi::translate::{llm::LlmTranslator, madlad::Madlad};
use hammurabi::types::{
    AudioChunk, AudioSource, CaptionUpdate, EngineCommand, EngineEvent, EngineState, FRAME_SAMPLES,
    Recognizer, SAMPLE_RATE, SourceKind, Transcript, Translate,
};

#[derive(Parser)]
#[command(
    name = "hammurabi-cli",
    version,
    about = "Hammurabi command-line tools"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List audio devices and whether system audio can be captured.
    Devices,
    /// Record 16 kHz mono WAV through the capture path.
    Record {
        #[arg(long, value_enum, default_value = "system")]
        source: Source,
        #[arg(long, default_value_t = 10)]
        secs: u64,
        /// Microphone device id (see `devices`).
        #[arg(long)]
        input: Option<String>,
        /// Playback device id to capture system audio from (see `devices`).
        #[arg(long)]
        output: Option<String>,
        out: PathBuf,
    },
    /// Caption a WAV file through the caption pipeline.
    Transcribe {
        input: PathBuf,
        #[arg(long, value_enum, default_value = "auto")]
        model: Model,
        /// Spoken language code; auto-detected when omitted.
        #[arg(long)]
        lang: Option<String>,
        /// Translate captions to this language code.
        #[arg(long)]
        translate: Option<String>,
        /// Translate with an OpenAI-compatible endpoint (e.g. http://localhost:11434/v1) instead of MADLAD.
        #[arg(long)]
        llm: Option<String>,
        #[arg(long, default_value = "qwen2.5:3b")]
        llm_model: String,
        /// Feed the audio at 1x through the real engine and its worker threads.
        #[arg(long)]
        realtime: bool,
        /// With --realtime, feed audio right away instead of waiting for the models (like the
        /// app's first run).
        #[arg(long)]
        start_now: bool,
        /// Encode Whisper's full 30 s window instead of trimming it to the audio.
        #[arg(long)]
        no_trim: bool,
        /// Force the CPU (non-realtime mode).
        #[arg(long)]
        cpu: bool,
        /// Also print live (unfinished) lines.
        #[arg(long)]
        live: bool,
    },
    /// Caption system audio through Windows Live Captions (it must be set up once in Windows).
    #[cfg(all(windows, feature = "win-livecaptions"))]
    LiveCaptions {
        #[arg(long, default_value_t = 30)]
        secs: u64,
        /// Translate captions to this language code.
        #[arg(long)]
        translate: Option<String>,
        /// Also print live (unfinished) lines.
        #[arg(long)]
        live: bool,
    },
    /// Download a model into the cache.
    Download {
        #[arg(value_enum)]
        model: DownloadModel,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Source {
    System,
    Mic,
    Both,
}

#[derive(Clone, Copy, ValueEnum)]
enum Model {
    Auto,
    Tiny,
    Base,
    Small,
    Turbo,
}

#[derive(Clone, Copy, ValueEnum)]
enum DownloadModel {
    Tiny,
    Base,
    Small,
    Turbo,
    Madlad,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    match Cli::parse().cmd {
        Cmd::Devices => devices(),
        Cmd::Record {
            source,
            secs,
            input,
            output,
            out,
        } => record(source, secs, input.as_deref(), output.as_deref(), &out),
        Cmd::Transcribe {
            input,
            model,
            lang,
            translate,
            llm,
            llm_model,
            realtime,
            start_now,
            no_trim,
            cpu,
            live,
        } => {
            let model = match model {
                Model::Auto => ModelChoice::Auto,
                Model::Tiny => ModelChoice::Tiny,
                Model::Base => ModelChoice::Base,
                Model::Small => ModelChoice::Small,
                Model::Turbo => ModelChoice::Turbo,
            };
            let settings = Settings {
                model,
                spoken_language: lang,
                trim_audio: !no_trim,
                translate: translate.is_some(),
                target_language: translate.unwrap_or_else(|| "en".into()),
                translator: match llm {
                    Some(base_url) => TranslatorChoice::Llm {
                        base_url,
                        model: llm_model,
                    },
                    None => TranslatorChoice::Builtin,
                },
                ..Settings::default()
            };
            let (rate, pcm) = read_wav(&input)?;
            if realtime {
                transcribe_realtime(rate, pcm, settings, live, start_now)
            } else {
                transcribe_sim(rate, pcm, &settings, cpu, live)
            }
        }
        #[cfg(all(windows, feature = "win-livecaptions"))]
        Cmd::LiveCaptions {
            secs,
            translate,
            live,
        } => live_captions(secs, translate, live),
        Cmd::Download { model } => {
            let id = match model {
                DownloadModel::Tiny => ModelId::WhisperTiny,
                DownloadModel::Base => ModelId::WhisperBase,
                DownloadModel::Small => ModelId::WhisperSmall,
                DownloadModel::Turbo => ModelId::WhisperTurbo,
                DownloadModel::Madlad => ModelId::Madlad3bQ4k,
            };
            let paths = ensure(id)?;
            println!(
                "{}",
                paths.weights.parent().unwrap_or(Path::new("")).display()
            );
            Ok(())
        }
    }
}

fn devices() -> anyhow::Result<()> {
    let d = list_devices();
    for (label, devs) in [("Inputs", &d.inputs), ("Outputs", &d.outputs)] {
        println!("{label}:");
        for dev in devs {
            let mark = if dev.is_default { "*" } else { " " };
            println!(" {mark} {}  [{}]", dev.name, dev.id);
        }
    }
    match d.system_audio {
        Ok(()) => println!("System audio: available"),
        Err(e) => println!("System audio: unavailable: {e}"),
    }
    Ok(())
}

fn record(
    source: Source,
    secs: u64,
    input: Option<&str>,
    output: Option<&str>,
    out: &Path,
) -> anyhow::Result<()> {
    let source = match source {
        Source::System => AudioSource::System,
        Source::Mic => AudioSource::Microphone,
        Source::Both => AudioSource::Both,
    };
    let (tx, rx) = crossbeam_channel::bounded(64);
    let capture = AudioCapture::start(source, input, output, tx)?;
    for note in &capture.notes {
        eprintln!("note: {note}");
    }
    let mut wav = hound::WavWriter::create(out, wav_spec())?;
    let mut frames = Frames16k::new();
    let mut buf = Vec::new();
    let total = secs as usize * 10;
    let (mut written, mut sumsq) = (0, 0.0f64);
    let mut last_audio = Instant::now();
    while written < total {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                last_audio = Instant::now();
                frames.push(chunk, &mut buf);
            }
            // Loopback delivers nothing while nothing plays; pad like the engine does.
            Err(RecvTimeoutError::Timeout) if last_audio.elapsed() > Duration::from_millis(300) => {
                frames.push_silence(&mut buf)
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => anyhow::bail!("capture stopped"),
        }
        for frame in buf.drain(..) {
            if written == total {
                break;
            }
            for &s in &frame {
                wav.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
                sumsq += (s * s) as f64;
            }
            written += 1;
            if written % 10 == 0 {
                let rms = (sumsq / (10 * FRAME_SAMPLES) as f64).sqrt();
                eprintln!("{:>4}s  rms {rms:.4}", written / 10);
                sumsq = 0.0;
            }
        }
    }
    capture.stop();
    wav.finalize()?;
    println!("wrote {}", out.display());
    Ok(())
}

fn wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

/// Mono f32 at the file's own rate.
fn read_wav(path: &Path) -> anyhow::Result<(u32, Vec<f32>)> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
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
    let mono = raw
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((spec.sample_rate, mono))
}

fn ensure(id: ModelId) -> anyhow::Result<ModelPaths> {
    let label = models::spec(id).label;
    let mut shown = false;
    let paths = models::ensure(
        id,
        &mut |done, total| {
            shown = true;
            let mb = |b: u64| b / 1_000_000;
            match total {
                Some(t) => eprint!("\rDownloading {label}: {} / {} MB   ", mb(done), mb(t)),
                None => eprint!("\rDownloading {label}: {} MB   ", mb(done)),
            }
        },
        &AtomicBool::new(false),
    )?;
    if shown {
        eprintln!();
    }
    Ok(paths)
}

#[derive(Default)]
struct Timing {
    calls: u32,
    total: Duration,
    max: Duration,
}

impl Timing {
    fn add(&mut self, d: Duration) {
        self.calls += 1;
        self.total += d;
        self.max = self.max.max(d);
    }

    fn summary(&self) -> String {
        let avg = self.total.as_millis() / self.calls.max(1) as u128;
        format!(
            "{} calls, avg {avg} ms, max {} ms, total {:.1} s",
            self.calls,
            self.max.as_millis(),
            self.total.as_secs_f32()
        )
    }
}

struct Timed<T> {
    inner: T,
    timing: Timing,
}

impl<T: Recognizer> Recognizer for Timed<T> {
    fn transcribe(
        &mut self,
        audio: &[f32],
        language: Option<&str>,
        prior: &HashMap<String, f32>,
    ) -> anyhow::Result<Transcript> {
        let t = Instant::now();
        let r = self.inner.transcribe(audio, language, prior);
        self.timing.add(t.elapsed());
        r
    }
}

impl Translate for Timed<Box<dyn Translate>> {
    fn translate(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
    ) -> anyhow::Result<String> {
        let t = Instant::now();
        let r = self.inner.translate(text, source, target, context);
        self.timing.add(t.elapsed());
        r
    }
}

fn frames_16k(rate: u32, pcm: Vec<f32>) -> Vec<Vec<f32>> {
    let mut frames = Frames16k::new();
    let mut out = Vec::new();
    frames.push(
        AudioChunk {
            source: SourceKind::System,
            samples: pcm,
            sample_rate: rate,
        },
        &mut out,
    );
    // Push the resampler's tail out.
    for _ in 0..3 {
        frames.push_silence(&mut out);
    }
    out
}

fn print_caption(c: &CaptionUpdate, at: Option<f32>, live: bool) {
    let t = at.map(|s| format!("[{s:6.1}s] ")).unwrap_or_default();
    match c {
        CaptionUpdate::Final {
            id,
            original,
            translated: None,
            lang,
        } => println!("{t}#{id} ({lang}) {original}"),
        CaptionUpdate::Final {
            id,
            translated: Some(tr),
            ..
        } => println!("{t}#{id}   => {tr}"),
        CaptionUpdate::Live {
            original,
            translated,
            ..
        } if live && !original.is_empty() => match translated {
            Some(tr) => println!("{t}   live: {original} => {tr}"),
            None => println!("{t}   live: {original}"),
        },
        _ => {}
    }
}

fn transcribe_sim(
    rate: u32,
    pcm: Vec<f32>,
    settings: &Settings,
    cpu: bool,
    live: bool,
) -> anyhow::Result<()> {
    let secs = pcm.len() as f32 / rate as f32;
    let (device, label) = hammurabi::device::best_device(!cpu);
    let id = ModelId::whisper(settings.model, !device.is_cpu());
    let paths = ensure(id)?;
    let t = Instant::now();
    let mut engine = WhisperEngine::load(&paths, &device)?;
    engine.trim = settings.trim_audio;
    eprintln!(
        "Whisper {} on {label}, loaded in {} ms, trim {}",
        models::spec(id).label,
        t.elapsed().as_millis(),
        if engine.trim { "on" } else { "off" }
    );
    let mut rec = Timed {
        inner: engine,
        timing: Timing::default(),
    };
    let mut tr = if settings.translate {
        let inner: Box<dyn Translate> = match &settings.translator {
            TranslatorChoice::Builtin => {
                let paths = ensure(ModelId::Madlad3bQ4k)?;
                let t = Instant::now();
                let m = Madlad::load(&paths, &device)?;
                eprintln!(
                    "MADLAD on {label}, loaded in {} ms",
                    t.elapsed().as_millis()
                );
                Box::new(m)
            }
            TranslatorChoice::Llm { base_url, model } => {
                let l = LlmTranslator::new(base_url, model);
                l.ping()?;
                Box::new(l)
            }
        };
        Some(Timed {
            inner,
            timing: Timing::default(),
        })
    } else {
        None
    };

    let t = Instant::now();
    let events = pipeline::simulate(
        frames_16k(rate, pcm),
        &mut rec,
        tr.as_mut().map(|t| t as &mut dyn Translate),
        settings,
    );
    let wall = t.elapsed();
    for e in &events {
        match e {
            EngineEvent::Caption(c) => print_caption(c, None, live),
            EngineEvent::Status(s) => eprintln!("status: {}", s.text),
            _ => {}
        }
    }
    eprintln!("---");
    eprintln!(
        "audio {secs:.1} s, processed in {:.1} s ({:.2}x realtime)",
        wall.as_secs_f32(),
        wall.as_secs_f32() / secs
    );
    eprintln!("ASR: {}", rec.timing.summary());
    if let Some(tr) = &tr {
        eprintln!("translation: {}", tr.timing.summary());
    }
    Ok(())
}

fn transcribe_realtime(
    rate: u32,
    pcm: Vec<f32>,
    settings: Settings,
    live: bool,
    start_now: bool,
) -> anyhow::Result<()> {
    let (engine, feed) = Engine::spawn_fed(Arc::new(|| {}));
    let wants_tr = settings.translate;
    engine.send(EngineCommand::Start(settings));

    // Wait for the speech model (and the translator) so timing reflects steady state.
    let (mut asr_ready, mut tr_ready) = (start_now, !wants_tr || start_now);
    let mut asr_ms = Timing::default();
    let started = Instant::now();
    while !(asr_ready && tr_ready) {
        let Some(e) = engine.recv_timeout(Duration::from_secs(600)) else {
            anyhow::bail!("timed out waiting for the models");
        };
        match e {
            EngineEvent::State(EngineState::Running) => asr_ready = true,
            EngineEvent::State(EngineState::Error) => anyhow::bail!("engine error"),
            EngineEvent::Status(s) => {
                eprintln!("status: {}", s.text);
                tr_ready |= s.text.starts_with("Translation");
            }
            _ => {}
        }
    }
    eprintln!("models ready in {:.1} s", started.elapsed().as_secs_f32());

    let chunk = rate as usize / 10;
    let secs = pcm.len() as f32 / rate as f32;
    let t0 = Instant::now();
    let mut shown = HashMap::new();
    let mut handle = |e: EngineEvent, t0: Instant| {
        let at = t0.elapsed().as_secs_f32();
        match e {
            EngineEvent::Caption(c) => print_caption(&c, Some(at), live),
            EngineEvent::Status(s) => eprintln!("[{at:6.1}s] status: {}", s.text),
            EngineEvent::State(s) => eprintln!("[{at:6.1}s] state: {s:?}"),
            EngineEvent::Stats { asr_ms: ms, .. } => asr_ms.add(Duration::from_millis(ms as u64)),
            EngineEvent::Progress(p) => {
                // One line per 10% of each download.
                let step = p.total.map_or(0, |t| p.done * 10 / t.max(1));
                if shown.insert((p.model, format!("{:?}", p.phase)), step) != Some(step) {
                    let label = models::spec(p.model).label;
                    eprintln!("[{at:6.1}s] {:?} {label}: {}0%", p.phase, step);
                }
            }
        }
    };
    for (i, c) in pcm.chunks(chunk).enumerate() {
        let _ = feed.send(AudioChunk {
            source: SourceKind::System,
            samples: c.to_vec(),
            sample_rate: rate,
        });
        let next = t0 + Duration::from_millis(100 * (i as u64 + 1));
        while let Some(e) = engine.recv_timeout(next.saturating_duration_since(Instant::now())) {
            handle(e, t0);
        }
    }
    // Stop feeding (the engine pads silence) and wait until captions settle.
    while let Some(e) = engine.recv_timeout(Duration::from_secs(5)) {
        handle(e, t0);
        if t0.elapsed().as_secs_f32() > secs + 120.0 {
            break;
        }
    }
    engine.shutdown(Duration::from_secs(2));
    eprintln!("---");
    eprintln!("audio {secs:.1} s fed at 1x");
    eprintln!("ASR: {}", asr_ms.summary());
    Ok(())
}

#[cfg(all(windows, feature = "win-livecaptions"))]
fn live_captions(secs: u64, translate: Option<String>, live: bool) -> anyhow::Result<()> {
    let engine = Engine::spawn(Arc::new(|| {}));
    engine.send(EngineCommand::Start(Settings {
        asr_backend: hammurabi::settings::AsrBackend::WindowsLiveCaptions,
        translate: translate.is_some(),
        target_language: translate.unwrap_or_else(|| "en".into()),
        ..Settings::default()
    }));
    let t0 = Instant::now();
    let end = t0 + Duration::from_secs(secs);
    while let Some(e) = engine.recv_timeout(end.saturating_duration_since(Instant::now())) {
        let at = Some(t0.elapsed().as_secs_f32());
        match e {
            EngineEvent::Caption(c) => print_caption(&c, at, live),
            EngineEvent::Status(s) => eprintln!("status: {}", s.text),
            EngineEvent::State(s) => eprintln!("state: {s:?}"),
            _ => {}
        }
    }
    engine.send(EngineCommand::Stop);
    engine.shutdown(Duration::from_secs(3));
    Ok(())
}

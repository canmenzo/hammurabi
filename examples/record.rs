//! Records 16 kHz mono WAV through the capture + Frames16k path and prints per-second RMS.
//! `cargo run --release --example record -- --source system --secs 10 out.wav`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use crossbeam_channel::RecvTimeoutError;
use hammurabi::audio::{AudioCapture, Frames16k, list_devices};
use hammurabi::types::{AudioSource, FRAME_SAMPLES, SAMPLE_RATE};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "system", value_parser = ["system", "mic", "both"])]
    source: String,
    #[arg(long, default_value_t = 10)]
    secs: u64,
    /// Microphone device id (see --list).
    #[arg(long)]
    input: Option<String>,
    /// Output device id to capture system audio from (see --list).
    #[arg(long)]
    output: Option<String>,
    /// List devices and exit.
    #[arg(long)]
    list: bool,
    #[arg(default_value = "out.wav")]
    out: PathBuf,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    if args.list {
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
        return Ok(());
    }

    let source = match args.source.as_str() {
        "system" => AudioSource::System,
        "mic" => AudioSource::Microphone,
        _ => AudioSource::Both,
    };
    let (tx, rx) = crossbeam_channel::bounded(64);
    let capture = AudioCapture::start(source, args.input.as_deref(), args.output.as_deref(), tx)?;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(&args.out, spec)?;
    let mut frames = Frames16k::new();
    let mut out = Vec::new();
    let total = args.secs as usize * 10;
    let (mut written, mut sumsq) = (0, 0.0f64);
    let mut last_audio = Instant::now();

    while written < total {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                last_audio = Instant::now();
                frames.push(chunk, &mut out);
            }
            // Loopback delivers nothing while nothing plays; pad like the engine does.
            Err(RecvTimeoutError::Timeout) if last_audio.elapsed() > Duration::from_millis(300) => {
                frames.push_silence(&mut out)
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => anyhow::bail!("capture stopped"),
        }
        for frame in out.drain(..) {
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
                println!("{:>4}s  rms {rms:.4}", written / 10);
                sumsq = 0.0;
            }
        }
    }
    capture.stop();
    wav.finalize()?;
    println!("wrote {}", args.out.display());
    Ok(())
}

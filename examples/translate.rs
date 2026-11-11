//! `cargo run --release --example translate -- --to tr "How are you, my friend?"`
//!
//! `--natural` uses the Natural translator; `--lines file.jsonl` translates a test set of
//! `{"id", "en", "context": [earlier lines]}` records the way captions are: each line after its
//! earlier ones, with their own translations as context. Prints `{"id", "out", "ms"}` per record.
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use clap::Parser;
use hammurabi::models::{self, ModelId, ModelPaths};
use hammurabi::translate::{apply_glossary, madlad::Madlad};
use hammurabi::types::Translate;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "tr")]
    to: String,
    #[arg(long, default_value = "en")]
    from: String,
    /// Force the CPU even on GPU builds.
    #[arg(long)]
    cpu: bool,
    /// The Natural translator instead of MADLAD.
    #[arg(long)]
    natural: bool,
    /// With --natural, its smaller model.
    #[arg(long)]
    light: bool,
    /// Names to keep as they are (Natural only).
    #[arg(long, default_value = "")]
    hint: String,
    #[arg(long)]
    lines: Option<std::path::PathBuf>,
    text: Vec<String>,
}

fn paths(id: ModelId) -> anyhow::Result<ModelPaths> {
    let mut last = 0;
    let progress = &mut |done: u64, total: Option<u64>| {
        if done >= last + 50_000_000 {
            last = done;
            eprintln!(
                "downloading {} / {:?} MB",
                done / 1_000_000,
                total.map(|t| t / 1_000_000)
            );
        }
    };
    models::ensure(id, progress, &AtomicBool::new(false))
}

#[cfg(windows)]
fn natural(light: bool) -> anyhow::Result<Box<dyn Translate>> {
    hammurabi::translate::natural(paths(ModelId::natural(light))?.weights)
}

#[cfg(not(windows))]
fn natural(_: bool) -> anyhow::Result<Box<dyn Translate>> {
    anyhow::bail!("the Natural translator runs on Windows only")
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();
    if args.natural {
        let t = Instant::now();
        let mut tr = natural(args.light)?;
        eprintln!("loaded in {:.2?}", t.elapsed());
        tr.set_hint(&args.hint);
        let mut say = |text: &str, context: &[(String, String)]| {
            let t = Instant::now();
            let out = tr
                .translate(text, &args.from, &args.to, context)
                .unwrap_or_else(|e| format!("ERROR {e:#}"));
            (out, t.elapsed().as_millis())
        };
        if let Some(file) = &args.lines {
            for line in std::fs::read_to_string(file)?.lines() {
                let rec: serde_json::Value = serde_json::from_str(line)?;
                let mut context = Vec::new();
                for c in rec["context"].as_array().into_iter().flatten() {
                    let c = c.as_str().unwrap_or_default().to_string();
                    let out = say(&c, &context).0;
                    context.push((c, out));
                }
                let (out, ms) = say(rec["en"].as_str().unwrap_or_default(), &context);
                println!(
                    "{}",
                    serde_json::json!({"id": rec["id"], "out": out, "ms": ms})
                );
            }
        }
        for text in &args.text {
            let (out, ms) = say(text, &[]);
            println!("{out}");
            eprintln!("{ms} ms");
        }
        return Ok(());
    }

    let paths = paths(ModelId::Madlad3bQ4k)?;
    let (device, label) = hammurabi::device::best_device(!args.cpu);
    let t = Instant::now();
    let mut m = Madlad::load(&paths, &device)?;
    eprintln!("loaded on {label} in {:.2?}", t.elapsed());

    if let Some(file) = &args.lines {
        for line in std::fs::read_to_string(file)?.lines() {
            let rec: serde_json::Value = serde_json::from_str(line)?;
            let t = Instant::now();
            let src = apply_glossary(rec["en"].as_str().unwrap_or_default());
            let out = m.translate_counted(&src, &args.to)?.0;
            let ms = t.elapsed().as_millis();
            println!(
                "{}",
                serde_json::json!({"id": rec["id"], "out": out, "ms": ms})
            );
        }
        return Ok(());
    }
    let texts = if args.text.is_empty() {
        vec!["How are you, my friend?".to_string()]
    } else {
        args.text
    };
    for text in texts {
        let src = if args.from == "en" {
            apply_glossary(&text)
        } else {
            text
        };
        let t = Instant::now();
        let (out, n) = m.translate_counted(&src, &args.to)?;
        let secs = t.elapsed().as_secs_f64();
        println!("{out}");
        eprintln!(
            "{n} tokens in {:.0} ms ({:.1} tokens/s)",
            secs * 1000.0,
            n as f64 / secs
        );
    }
    Ok(())
}

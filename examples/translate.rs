//! `cargo run --release --example translate -- --to tr "How are you, my friend?"`
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use clap::Parser;
use hammurabi::models::{self, ModelId, ModelPaths};
use hammurabi::translate::{apply_glossary, madlad::Madlad};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "tr")]
    to: String,
    #[arg(long, default_value = "en")]
    from: String,
    /// Force the CPU even on GPU builds.
    #[arg(long)]
    cpu: bool,
    text: Vec<String>,
}

fn paths() -> anyhow::Result<ModelPaths> {
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
    models::ensure(ModelId::Madlad3bQ4k, progress, &AtomicBool::new(false))
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();
    let paths = paths()?;
    let (device, label) = hammurabi::device::best_device(!args.cpu);
    let t = Instant::now();
    let mut m = Madlad::load(&paths, &device)?;
    eprintln!("loaded on {label} in {:.2?}", t.elapsed());

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

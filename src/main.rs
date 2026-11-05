#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use hammurabi::{config, paths, ui};
use ui::instance::{self, Signal};

const LOG_CAP: u64 = 5_000_000;

/// hammurabi.log, rotated to hammurabi.old.log at `LOG_CAP`; every line is also copied to stderr.
struct LogFile {
    path: PathBuf,
    file: File,
    len: u64,
}

impl LogFile {
    fn open() -> anyhow::Result<Self> {
        let dir = paths::log_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("hammurabi.log");
        let file = File::options().create(true).append(true).open(&path)?;
        let len = file.metadata()?.len();
        Ok(Self { path, file, len })
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        std::fs::rename(&self.path, self.path.with_extension("old.log"))?;
        self.file = File::create(&self.path)?;
        self.len = 0;
        Ok(())
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if self.len >= LOG_CAP {
            self.rotate()?;
        }
        self.file.write_all(buf)?;
        self.len += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn main() -> eframe::Result {
    let mut logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    match LogFile::open() {
        Ok(f) => logger.target(env_logger::Target::Pipe(Box::new(f))).init(),
        Err(e) => {
            logger.init();
            log::warn!("no log file: {e:#}");
        }
    }
    // This is a GUI app, so without this a panic would leave no trace at all.
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::force_capture();
        log::error!("{info}\n{trace}");
    }));
    log::info!("Hammurabi {} starting", env!("CARGO_PKG_VERSION"));
    let has_arg = |name: &str| std::env::args().any(|a| a == name);
    let lock = single_instance::SingleInstance::new("dev.menzo.hammurabi");
    if lock.as_ref().is_ok_and(|i| !i.is_single()) {
        log::info!("Hammurabi is already running");
        let signal = if has_arg("--quit") {
            Some(Signal::Quit)
        } else {
            (!has_arg(ui::BACKGROUND_ARG)).then_some(Signal::Show)
        };
        if let Some(Err(e)) = signal.map(instance::send) {
            log::warn!("could not reach the running copy: {e:#}");
        }
        return Ok(());
    }
    if has_arg("--quit") {
        return Ok(());
    }
    let settings = config::load();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: ui::overlay::viewport_builder(&settings).with_visible(false),
        event_loop_builder: Some(Box::new(ui::platform::configure_event_loop)),
        ..Default::default()
    };
    eframe::run_native(
        "Hammurabi",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc, settings)))),
    )
}

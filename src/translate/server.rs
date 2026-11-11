//! llama.cpp's server as a child process, for the translator that runs a language model:
//! downloaded on first use, started hidden on a local port, and gone when Hammurabi is.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::models::download;

/// The Vulkan build, which runs on any GPU.
const BUILD: &str = "b12290";
const ARCHIVE_SHA256: &str = "62837f67d9950d40f3200f6a577bb8f82e59cb7eabe3db35a7ef3d2b4956d8b7";
const EXE: &str = "llama-server.exe";
/// A model of several GB takes a while to load from a cold disk.
const START_TIMEOUT: Duration = Duration::from_secs(180);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Server {
    child: Child,
    port: u16,
}

/// Downloads and unpacks the server on first use; returns its executable.
fn ensure(
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<PathBuf> {
    let models = crate::paths::models_dir()?;
    let dir = models.join(format!("llama.cpp-{BUILD}"));
    let exe = dir.join(EXE);
    if exe.is_file() {
        return Ok(exe);
    }
    let zip = models.join(format!("llama.cpp-{BUILD}.zip"));
    let url = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{BUILD}/llama-{BUILD}-bin-win-vulkan-x64.zip"
    );
    download::fetch_url(&url, ARCHIVE_SHA256, &zip, download::IDLE, progress, cancel)
        .context("couldn't download llama.cpp")?;
    // Unpacked beside its place and moved there whole, so a half-unpacked copy is never run.
    let tmp = models.join(format!("llama.cpp-{BUILD}.tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    // Windows' own tar reads zip files; one from elsewhere on PATH may not.
    let tar = Path::new(&std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32")
        .join("tar.exe");
    let status = Command::new(tar)
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&tmp)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("unpacking llama.cpp")?;
    anyhow::ensure!(
        status.success() && tmp.join(EXE).is_file(),
        "unpacking llama.cpp failed"
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir)?;
    let _ = std::fs::remove_file(zip);
    Ok(exe)
}

/// The child ends with this process, however this process ends: it is put in a job that Windows
/// closes, killing what is in it, when the last handle to it goes.
fn tie_to_us(child: &Child) -> windows::core::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    static JOB: OnceLock<usize> = OnceLock::new();
    // SAFETY: plain calls on handles that are live; `info` outlives the call that reads it.
    unsafe {
        let job = match JOB.get() {
            Some(&j) => HANDLE(j as *mut _),
            None => {
                let job = CreateJobObjectW(None, windows::core::PCWSTR::null())?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
                let _ = JOB.set(job.0 as usize);
                job
            }
        };
        AssignProcessToJobObject(job, HANDLE(child.as_raw_handle()))
    }
}

impl Server {
    /// Starts the server with the GGUF `model` fully on the GPU, and waits until it answers.
    /// `progress` reports the download of the server itself, the first time.
    pub fn start(
        model: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> anyhow::Result<Server> {
        let exe = ensure(progress, cancel)?;
        // Free now, and nothing else is likely to take it in the next moment.
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))?
            .local_addr()?
            .port();
        let log = crate::paths::log_dir()?.join("llama-server.log");
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let out = std::fs::File::create(&log)?;
        let child = Command::new(&exe)
            .arg("-m")
            .arg(model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            // Everything on the GPU, one request at a time, no thinking aloud. `--swa-full` lets
            // Gemma, whose attention slides, keep the unchanging start of the prompt: without it
            // every line reads the whole prompt again (467 ms against 62 ms).
            .args(["-ngl", "999", "-c", "2048", "-np", "1", "-fa", "on"])
            .args(["--swa-full", "--cache-ram", "0"])
            .args(["--jinja", "--reasoning", "off", "--no-webui"])
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("starting llama.cpp")?;
        if let Err(e) = tie_to_us(&child) {
            log::warn!("llama.cpp may outlive a crash: {e}");
        }
        let mut server = Server { child, port };
        server.wait_ready(cancel, &log)?;
        Ok(server)
    }

    fn wait_ready(&mut self, cancel: &AtomicBool, log: &Path) -> anyhow::Result<()> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build()
            .into();
        let health = format!("http://127.0.0.1:{}/health", self.port);
        let started = Instant::now();
        loop {
            anyhow::ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
            if let Some(status) = self.child.try_wait()? {
                // The reason is in the last lines it wrote.
                let text = std::fs::read_to_string(log).unwrap_or_default();
                let tail: Vec<&str> = text.lines().rev().take(3).collect();
                log::error!("llama.cpp stopped ({status}): {tail:?}");
                anyhow::bail!("the language model could not start, see llama-server.log");
            }
            // 503 while the model loads.
            if agent
                .get(&health)
                .call()
                .is_ok_and(|r| r.status().is_success())
            {
                log::info!("llama.cpp ready in {:?}", started.elapsed());
                return Ok(());
            }
            anyhow::ensure!(
                started.elapsed() < START_TIMEOUT,
                "the language model took too long to start"
            );
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

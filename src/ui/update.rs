//! Asks GitHub for the latest release; failures (offline, rate limit, no release yet) are only logged.
//! A copy installed by the Windows setup program downloads the new setup program in the background
//! and runs it silently once captions are off; other copies only announce the release.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Redirects to the latest release's page; unlike the API it has no per-IP hourly limit.
const LATEST: &str = "https://github.com/canmenzo/hammurabi/releases/latest";
const USER_AGENT: &str = "OpenAI File Downloader, XaiImageApiFetch/1.0";
pub const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Without the leading "v".
    pub version: String,
    pub url: String,
}

/// Where the update check is, shown in Settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// Version and percent done.
    Downloading(String, u8),
    /// The verified setup program, installed once captions are off.
    Ready(Release, PathBuf),
    /// A newer release this copy does not install itself: an unzipped copy, or the download failed.
    Available(Release),
    Failed,
}

/// Checks on a background thread, downloading the new setup program when this copy can install
/// it, and reports each step.
pub fn spawn(report: impl Fn(Status) + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("hm-update".into())
        .spawn(move || {
            report(Status::Checking);
            let status = match latest() {
                Ok(r) if newer(&r.version, env!("CARGO_PKG_VERSION")) => {
                    match setup_asset().filter(|_| installed()) {
                        Some(asset) => {
                            let mut shown = None;
                            let got = download(&r.version, asset, &mut |done, total| {
                                let pct = total.map(|t| (done * 100 / t.max(1)).min(100) as u8);
                                if pct != shown {
                                    shown = pct;
                                    report(Status::Downloading(
                                        r.version.clone(),
                                        pct.unwrap_or(0),
                                    ));
                                }
                            });
                            match got {
                                Ok(path) => Status::Ready(r, path),
                                Err(e) => {
                                    log::warn!("update download failed: {e:#}");
                                    Status::Available(r)
                                }
                            }
                        }
                        None => Status::Available(r),
                    }
                }
                Ok(r) => {
                    log::info!("up to date (latest release v{})", r.version);
                    if let Ok(dir) = crate::paths::updates_dir() {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    Status::UpToDate
                }
                Err(e) => {
                    log::info!("update check failed: {e:#}");
                    Status::Failed
                }
            };
            report(status);
        });
}

/// The release asset of the setup program for this build.
fn setup_asset() -> Option<&'static str> {
    if !cfg!(windows) {
        None
    } else if cfg!(feature = "cuda") {
        Some("hammurabi-windows-cuda-setup.exe")
    } else if cfg!(feature = "omnivoice") {
        Some("hammurabi-windows-gpu-setup.exe")
    } else {
        Some("hammurabi-windows-x64-setup.exe")
    }
}

/// Installed by the setup program, whose uninstaller sits beside the exe; an unzipped copy is not.
fn installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("unins000.exe").is_file()))
        .unwrap_or(false)
}

/// Fetches the setup program of `version` into the updates folder, checked against the
/// release's SHA256SUMS.txt; older downloads there are removed.
fn download(
    version: &str,
    asset: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> anyhow::Result<PathBuf> {
    let dir = crate::paths::updates_dir()?;
    let dest = dir.join(format!("hammurabi-{version}-setup.exe"));
    let part = dir.join(format!("hammurabi-{version}-setup.exe.part"));
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if e.path() != dest && e.path() != part {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let base = format!("https://github.com/canmenzo/hammurabi/releases/download/v{version}");
    let sums = agent()
        .get(format!("{base}/SHA256SUMS.txt"))
        .call()?
        .body_mut()
        .read_to_string()?;
    let sha256 = checksum(&sums, asset)
        .ok_or_else(|| anyhow::anyhow!("{asset} is not in SHA256SUMS.txt"))?;
    if !(dest.is_file()
        && crate::models::download::sha256_file(&dest)?.eq_ignore_ascii_case(sha256))
    {
        crate::models::download::fetch_url(
            &format!("{base}/{asset}"),
            sha256,
            &dest,
            crate::models::download::IDLE,
            progress,
            &AtomicBool::new(false),
        )?;
    }
    log::info!("update v{version} downloaded to {}", dest.display());
    Ok(dest)
}

/// The hash `sha256sum` printed for `file`.
fn checksum<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|l| {
        let (hash, name) = l.split_once(char::is_whitespace)?;
        (name.trim_start_matches([' ', '*']) == file).then_some(hash)
    })
}

/// Runs the setup program with only its progress window. It quits this copy, installs over it
/// and starts the new version (see hammurabi.iss), in the tray unless `show` reopens Settings.
pub fn install(installer: &Path, show: bool) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(installer);
    let mut log = std::ffi::OsString::from("/LOG=");
    log.push(installer.with_extension("log"));
    cmd.arg(log).args([
        "/SILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/SP-",
        if show {
            "/HMRELAUNCH="
        } else {
            "/HMRELAUNCH=--background"
        },
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Out of any job the app sits in, so closing it does not end the setup program.
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS);
        // A job that forbids breaking away refuses the flag.
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        cmd.creation_flags(DETACHED_PROCESS);
    }
    cmd.spawn()?;
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

fn latest() -> anyhow::Result<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let resp = agent.get(LATEST).call()?;
    let url = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("no redirect from {LATEST} (HTTP {})", resp.status()))?;
    let tag = url
        .strip_prefix("https://github.com/canmenzo/hammurabi/releases/tag/")
        .ok_or_else(|| anyhow::anyhow!("unexpected release url {url}"))?;
    Ok(Release {
        version: tag.trim_start_matches('v').into(),
        url: url.into(),
    })
}

/// (major, minor, patch); pre-release and build suffixes are ignored.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.split(['-', '+']).next()?.split('.').map(str::parse);
    let mut next = || parts.next()?.ok();
    Some((next()?, next()?, next()?))
}

fn newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver() {
        assert!(newer("0.1.1", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.0"));
        assert!(newer("0.2.0-beta.1", "0.1.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert!(!newer("garbage", "0.1.0"));
        assert!(!newer("0.2", "0.1.0"));
    }

    #[test]
    #[ignore = "downloads a release from GitHub"]
    fn downloads_setup() {
        let path = download("0.7.3", "hammurabi-windows-x64-setup.exe", &mut |_, _| {}).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn finds_checksum() {
        let sums = "aa  hammurabi-windows-x64.zip
bb  hammurabi-windows-x64-setup.exe
cc *hammurabi-windows-cuda-setup.exe
";
        assert_eq!(
            checksum(sums, "hammurabi-windows-x64-setup.exe"),
            Some("bb")
        );
        assert_eq!(
            checksum(sums, "hammurabi-windows-cuda-setup.exe"),
            Some("cc")
        );
        assert_eq!(checksum(sums, "hammurabi-windows-gpu-setup.exe"), None);
    }
}

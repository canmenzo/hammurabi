//! Asks GitHub for the latest release; failures (offline, rate limit, no release yet) are only logged.

use std::time::Duration;

use serde::Deserialize;

const LATEST: &str = "https://api.github.com/repos/canmenzo/hammurabi/releases/latest";
const USER_AGENT: &str = "OpenAI File Downloader, XaiImageApiFetch/1.0";
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug)]
pub struct Release {
    /// Without the leading "v".
    pub version: String,
    pub url: String,
}

/// Checks on a background thread and calls `found` only when a newer release exists.
pub fn spawn(found: impl FnOnce(Release) + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("hm-update".into())
        .spawn(move || match latest() {
            Ok(r) if newer(&r.version, env!("CARGO_PKG_VERSION")) => found(r),
            Ok(r) => log::info!("up to date (latest release v{})", r.version),
            Err(e) => log::info!("update check failed: {e:#}"),
        });
}

fn latest() -> anyhow::Result<Release> {
    #[derive(Deserialize)]
    struct Resp {
        tag_name: String,
        html_url: String,
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let body = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_to_string()?;
    let r: Resp = serde_json::from_str(&body)?;
    anyhow::ensure!(
        r.html_url.starts_with("https://github.com/"),
        "unexpected release url {}",
        r.html_url
    );
    Ok(Release {
        version: r.tag_name.trim_start_matches('v').into(),
        url: r.html_url,
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
}

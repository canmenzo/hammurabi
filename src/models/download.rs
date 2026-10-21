use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, bail};

const USER_AGENT: &str = "OpenAI File Downloader, XaiImageApiFetch/1.0";

/// Downloads `huggingface.co/{repo}/resolve/{rev}/{file}` to `dest` via `dest.part` with Range resume.
/// `progress(done, total)`; returns an error when `cancel` is set.
pub fn fetch(
    repo: &str,
    rev: &str,
    file: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    fetch_url(
        &format!("https://huggingface.co/{repo}/resolve/{rev}/{file}"),
        dest,
        progress,
        cancel,
    )
}

fn fetch_url(
    url: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .build()
        .into();

    let mut have = std::fs::metadata(&part).map_or(0, |m| m.len());
    let mut resp = request(&agent, url, have)?;
    if resp.status().as_u16() == 416 {
        // .part is stale or already full-length: restart from scratch.
        have = 0;
        let _ = std::fs::remove_file(&part);
        resp = request(&agent, url, 0)?;
    }
    let status = resp.status().as_u16();
    if status != 200 && status != 206 {
        bail!("download {url}: HTTP {status}");
    }
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let total = if status == 206 {
        header("content-range").and_then(|v| v.rsplit('/').next()?.parse::<u64>().ok())
    } else {
        have = 0;
        header("content-length").and_then(|v| v.parse::<u64>().ok())
    };

    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(have > 0)
        .truncate(have == 0)
        .open(&part)?;
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = have;
    progress(done, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("download cancelled");
        }
        let n = body.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    drop(out);
    if let Some(t) = total
        && done != t
    {
        bail!("download truncated: {done} of {t} bytes (will resume)");
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

fn request(
    agent: &ureq::Agent,
    url: &str,
    from: u64,
) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
    let mut req = agent.get(url);
    if from > 0 {
        req = req.header("Range", format!("bytes={from}-"));
    }
    req.call().with_context(|| format!("GET {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://huggingface.co/openai/whisper-tiny/resolve/169d4a4341b33bc18d8881c4b69c2e104e1cc0af/tokenizer.json";

    #[test]
    #[ignore = "network"]
    fn resume_after_cancel() {
        let dir = std::env::temp_dir().join(format!("hammurabi-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("a").join("tokenizer.json");
        let part = dir.join("a").join("tokenizer.json.part");

        let cancel = AtomicBool::new(false);
        let err = fetch_url(
            URL,
            &dest,
            &mut |d, _| {
                if d >= 200_000 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert!(err.is_err() && !dest.exists());
        let partial = std::fs::metadata(&part).unwrap().len();
        assert!(partial >= 200_000, "partial {partial}");

        cancel.store(false, Ordering::Relaxed);
        let mut first = None;
        fetch_url(
            URL,
            &dest,
            &mut |d, t| {
                first.get_or_insert((d, t));
            },
            &cancel,
        )
        .unwrap();
        let (start, total) = first.unwrap();
        assert_eq!(start, partial, "did not resume");
        assert!(!part.exists());
        let resumed = std::fs::read(&dest).unwrap();
        assert_eq!(Some(resumed.len() as u64), total);

        let fresh = dir.join("b.json");
        fetch_url(URL, &fresh, &mut |_, _| {}, &cancel).unwrap();
        assert_eq!(resumed, std::fs::read(&fresh).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
}

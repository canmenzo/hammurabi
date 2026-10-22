use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{anyhow, bail};
use sha2::{Digest, Sha256};

const USER_AGENT: &str = "OpenAI File Downloader, XaiImageApiFetch/1.0";
/// ureq has no idle read timeout, so each request reads for at most this long and the download
/// continues with a new Range request; a stalled connection then fails instead of hanging.
const SLICE: Duration = Duration::from_secs(30);

/// Downloads `huggingface.co/{repo}/resolve/{rev}/{file}` to `dest` via `dest.part` with Range resume,
/// checking the result against `sha256`. `progress(done, total)`; returns an error when `cancel` is set.
pub fn fetch(
    repo: &str,
    rev: &str,
    file: &str,
    sha256: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    fetch_url(
        &format!("https://huggingface.co/{repo}/resolve/{rev}/{file}"),
        sha256,
        dest,
        SLICE,
        progress,
        cancel,
    )
}

fn fetch_url(
    url: &str,
    sha256: &str,
    dest: &Path,
    slice: Duration,
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
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(slice))
        .build()
        .into();

    loop {
        let before = std::fs::metadata(&part).map_or(0, |m| m.len());
        match fetch_slice(&agent, url, &part, progress, cancel) {
            Ok(true) => break,
            Ok(false) => {
                // Interrupted: carry on while the last request made progress.
                if std::fs::metadata(&part).map_or(0, |m| m.len()) <= before {
                    bail!(
                        "the download from huggingface.co stalled, check your internet connection"
                    );
                }
            }
            Err(e) => return Err(e),
        }
    }
    let actual = sha256_file(&part)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&part);
        log::warn!("{url}: sha256 {actual}, expected {sha256}");
        bail!("the downloaded file was damaged and has been deleted");
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

/// One request from the end of `part`; Ok(true) once the file is complete, Ok(false) when the
/// body ended early or timed out.
fn fetch_slice(
    agent: &ureq::Agent,
    url: &str,
    part: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> anyhow::Result<bool> {
    let mut have = std::fs::metadata(part).map_or(0, |m| m.len());
    let mut resp = request(agent, url, have)?;
    if resp.status().as_u16() == 416 {
        // .part is stale or already full-length: restart from scratch.
        have = 0;
        let _ = std::fs::remove_file(part);
        resp = request(agent, url, 0)?;
    }
    let status = resp.status().as_u16();
    if status != 200 && status != 206 {
        log::warn!("GET {url}: HTTP {status}");
        if status >= 500 {
            bail!("huggingface.co is having problems (HTTP {status}), try again later");
        }
        bail!("huggingface.co returned HTTP {status}");
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
        .open(part)?;
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = have;
    progress(done, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("download cancelled");
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                log::info!("GET {url}: interrupted at {done} bytes: {e}");
                return Ok(false);
            }
        };
        out.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    Ok(total.is_none_or(|t| done == t))
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
    req.call().map_err(|e| {
        log::warn!("GET {url}: {e}");
        anyhow!("can't reach huggingface.co ({e}), check your internet connection")
    })
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    const URL: &str = "https://huggingface.co/openai/whisper-tiny/resolve/169d4a4341b33bc18d8881c4b69c2e104e1cc0af/tokenizer.json";
    const URL_SHA: &str = "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566";

    enum Reply {
        Status(u16),
        /// Send this many bytes of the requested range, then close (None = all of it).
        Send(Option<usize>),
        /// Send this many bytes, then hold the connection open without sending more.
        Stall(usize),
    }

    /// HTTP server on localhost that answers connection `i` with `reply(i)`, honouring Range.
    fn serve(body: Vec<u8>, reply: impl Fn(usize) -> Reply + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for (i, conn) in listener.incoming().enumerate() {
                let mut conn = conn.unwrap();
                let mut from = 0;
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let lower = line.to_ascii_lowercase();
                    if let Some(r) = lower.strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                    if line.trim().is_empty() {
                        break;
                    }
                }
                let rest = &body[from..];
                let (head, n, stall) = match reply(i) {
                    Reply::Status(s) => {
                        let _ = write!(conn, "HTTP/1.1 {s} X\r\ncontent-length: 0\r\n\r\n");
                        continue;
                    }
                    Reply::Send(n) => (from, n.unwrap_or(rest.len()), false),
                    Reply::Stall(n) => (from, n, true),
                };
                let status = if head > 0 {
                    format!(
                        "206 Partial Content\r\ncontent-range: bytes {head}-{}/{}",
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    "200 OK".into()
                };
                let _ = write!(
                    conn,
                    "HTTP/1.1 {status}\r\ncontent-length: {}\r\n\r\n",
                    rest.len()
                );
                let _ = conn.write_all(&rest[..n.min(rest.len())]);
                let _ = conn.flush();
                if stall {
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        });
        url
    }

    fn body() -> (Vec<u8>, String) {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let sha = format!("{:x}", Sha256::digest(&body));
        (body, sha)
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hammurabi-dl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn get(url: &str, sha: &str, dest: &Path) -> anyhow::Result<()> {
        let slice = Duration::from_secs(1);
        fetch_url(
            url,
            sha,
            dest,
            slice,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
    }

    #[test]
    fn resumes_after_cut_and_stall() {
        let (body, sha) = body();
        let url = serve(body.clone(), |i| match i {
            0 => Reply::Send(Some(100_000)),
            1 => Reply::Stall(100_000),
            _ => Reply::Send(None),
        });
        let dest = dir("resume").join("f.bin");
        get(&url, &sha, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[test]
    fn http_errors_and_offline() {
        let (body, sha) = body();
        let d = dir("errors");
        let url = serve(body, |i| Reply::Status(if i == 0 { 404 } else { 503 }));
        let e = get(&url, &sha, &d.join("a")).unwrap_err().to_string();
        assert!(e.contains("HTTP 404"), "{e}");
        let e = get(&url, &sha, &d.join("a")).unwrap_err().to_string();
        assert!(
            e.contains("HTTP 503") && e.contains("try again later"),
            "{e}"
        );
        assert!(!d.join("a").exists());

        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let e = get(&format!("http://127.0.0.1:{port}/x"), &sha, &d.join("b"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("can't reach"), "{e}");
    }

    #[test]
    fn stalled_without_progress_fails() {
        let (body, sha) = body();
        let url = serve(body, |_| Reply::Stall(0));
        let e = get(&url, &sha, &dir("stall").join("f")).unwrap_err();
        assert!(e.to_string().contains("stalled"), "{e}");
    }

    #[test]
    fn damaged_download_is_deleted() {
        let (mut body, sha) = body();
        body[1234] ^= 1;
        let url = serve(body, |_| Reply::Send(None));
        let dest = dir("damaged").join("f");
        let e = get(&url, &sha, &dest).unwrap_err();
        assert!(e.to_string().contains("damaged"), "{e}");
        assert!(!dest.exists() && !dest.with_extension("part").exists());
        // The retry downloads it again from scratch.
        assert!(get(&url, &sha, &dest).is_err());
    }

    #[test]
    #[ignore = "network"]
    fn resume_after_cancel() {
        let dir = dir("hf");
        let dest = dir.join("a").join("tokenizer.json");
        let part = dir.join("a").join("tokenizer.json.part");

        let cancel = AtomicBool::new(false);
        let err = fetch_url(
            URL,
            URL_SHA,
            &dest,
            SLICE,
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
            URL_SHA,
            &dest,
            SLICE,
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
        let _ = std::fs::remove_dir_all(dir);
    }
}

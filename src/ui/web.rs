//! Captions as a web page at `http://127.0.0.1:PORT`, for a streaming app's browser source: the
//! overlay window is not part of a captured game or capture card, a browser source is. The page
//! listens to `/events` (server-sent events) and draws what the overlay draws, without statuses.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::settings::Settings;
use crate::types::CaptionUpdate;

pub const PORT: u16 = 7460;
const PAGE: &str = include_str!("../../assets/captions.html");

#[derive(Default)]
struct Shared {
    pages: Vec<TcpStream>,
    /// Sent to a page when it connects.
    style: String,
}

pub struct Web {
    shared: Arc<Mutex<Shared>>,
}

impl Web {
    pub fn spawn() -> std::io::Result<Web> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, PORT))?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let s = shared.clone();
        std::thread::Builder::new()
            .name("hm-web".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    if let Err(e) = serve(stream, &s) {
                        log::debug!("captions page request: {e}");
                    }
                }
            })?;
        Ok(Web { shared })
    }

    pub fn url() -> String {
        format!("http://127.0.0.1:{PORT}")
    }

    fn send(&self, event: &serde_json::Value, style: bool) {
        let event = event.to_string();
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        s.pages.retain_mut(|p| write_event(p, &event).is_ok());
        if style {
            s.style = event;
        }
    }

    pub fn style(&self, s: &Settings) {
        let c = &s.caption;
        let style = json!({
            "t": "style",
            "trans_size": c.trans_size,
            "orig_size": c.orig_size,
            "trans_color": c.trans_color,
            "orig_color": c.orig_color,
            "bg": c.bg_opacity,
            "show_original": c.show_original,
            "original_first": c.original_first,
            "cues": c.cues,
            "hold": c.hold_secs,
            "edge": format!("{:?}", c.edge),
        });
        self.send(&style, true);
    }

    pub fn caption(&self, c: &CaptionUpdate, target: &str) {
        let event = match c {
            CaptionUpdate::Final {
                id,
                original,
                translated,
                lang,
            } => {
                json!({"t": "final", "id": id, "o": original, "tr": translated, "same": lang == target})
            }
            CaptionUpdate::Live {
                original,
                translated,
                lang,
            } => json!({
                "t": "live",
                "o": original,
                "tr": translated,
                "same": lang.as_deref() == Some(target),
            }),
            CaptionUpdate::Clear => json!({"t": "clear"}),
        };
        self.send(&event, false);
    }
}

fn write_event(page: &mut TcpStream, event: &str) -> std::io::Result<()> {
    page.write_all(format!("data: {event}\n\n").as_bytes())
}

fn serve(mut stream: TcpStream, shared: &Mutex<Shared>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    // A page that stopped reading must not hold up the captions.
    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let path = request.split(' ').nth(1).unwrap_or("/");
    if path.starts_with("/events") {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n",
        )?;
        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
        if !s.style.is_empty() {
            write_event(&mut stream, &s.style)?;
        }
        s.pages.push(stream);
        return Ok(());
    }
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
        PAGE.len()
    )
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn serves_the_page_and_streams_captions() {
        // The port is fixed, so this is the only test that binds it.
        let web = Web::spawn().unwrap();
        web.style(&Settings::default());
        let get = |path: &str| {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, PORT)).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            s
        };
        let mut page = String::new();
        get("/").read_to_string(&mut page).unwrap();
        assert!(page.starts_with("HTTP/1.1 200 OK") && page.contains("EventSource"));

        let mut events = BufReader::new(get("/events"));
        let mut next = || {
            let mut line = String::new();
            while !line.starts_with("data: ") {
                line.clear();
                events.read_line(&mut line).unwrap();
            }
            serde_json::from_str::<serde_json::Value>(&line[6..]).unwrap()
        };
        assert_eq!(next()["t"], "style");
        // The page is registered once the style went out.
        web.caption(
            &CaptionUpdate::Final {
                id: 3,
                original: "What's up, \"homie\"?".into(),
                translated: Some("N'aber kanka?".into()),
                lang: "en".into(),
            },
            "tr",
        );
        let e = next();
        assert_eq!(
            (e["t"].as_str(), e["id"].as_u64()),
            (Some("final"), Some(3))
        );
        assert_eq!(e["tr"], "N'aber kanka?");
        assert_eq!(e["same"], false);
    }
}

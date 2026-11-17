//! Captions as a web page at `http://127.0.0.1:PORT`, for a streaming app's browser source: the
//! overlay window is not part of a captured game or capture card, a browser source is. The page
//! listens to `/events` (server-sent events) and draws what the overlay draws, without statuses.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::screen::Spot;
use crate::settings::Settings;
use crate::types::CaptionUpdate;

pub const PORT: u16 = 7460;
const PAGE: &str = include_str!("../../assets/captions.html");

#[derive(Default)]
struct Shared {
    pages: Vec<TcpStream>,
    /// Sent to a page when it connects.
    style: String,
    cover: String,
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
            "cover": s.covers(),
        });
        self.send(&style, true);
    }

    /// The cover over the subtitle read off the screen (`OverlayState::covered`), when it
    /// changed. The page is taken to show the whole picture read, as a game's capture does.
    pub fn cover(&self, cover: Option<(Spot, Option<&str>)>) {
        let event = match cover {
            Some((s, text)) => {
                let a = s.area;
                json!({"t": "cover", "box": [a.left, a.top, a.right, a.bottom], "row": s.row, "tr": text})
            }
            None => json!({"t": "cover"}),
        }
        .to_string();
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        if s.cover != event {
            s.pages.retain_mut(|p| write_event(p, &event).is_ok());
            s.cover = event;
        }
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
        for event in [&s.style, &s.cover] {
            if !event.is_empty() {
                write_event(&mut stream, event)?;
            }
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

        // A cover: where, and the translation only; empty while it is not in.
        let spot = Spot {
            area: crate::screen::Area {
                left: 0.375,
                top: 0.875,
                right: 0.625,
                bottom: 0.9375,
            },
            row: 0.03125,
        };
        web.cover(Some((spot, None)));
        let e = next();
        assert_eq!(e["t"], "cover");
        assert_eq!(e["box"], json!([0.375, 0.875, 0.625, 0.9375]));
        assert_eq!(e["row"], 0.03125);
        assert!(e["tr"].is_null());
        // Sent when it changes only.
        web.cover(Some((spot, None)));
        web.cover(Some((spot, Some("N'aber kanka?"))));
        assert_eq!(next()["tr"], "N'aber kanka?");
        // A page that connects later has it at once, after the style.
        let mut late = BufReader::new(get("/events"));
        let mut lines = Vec::new();
        while lines.len() < 2 {
            let mut line = String::new();
            late.read_line(&mut line).unwrap();
            if let Some(data) = line.strip_prefix("data: ") {
                lines.push(serde_json::from_str::<serde_json::Value>(data).unwrap());
            }
        }
        assert_eq!(
            (&lines[0]["t"], &lines[1]["t"]),
            (&json!("style"), &json!("cover"))
        );
        assert_eq!(lines[1]["tr"], "N'aber kanka?");
        web.cover(None);
        assert_eq!(next(), json!({"t": "cover"}));
    }
}

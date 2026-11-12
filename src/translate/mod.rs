pub mod llm;
pub mod madlad;
#[cfg(windows)]
pub mod server;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use regex::Regex;

use crate::models::{self, ModelId};
use crate::settings::TranslatorChoice;
use crate::types::{Progress, ProgressPhase, Translate, TranslateRequest, TranslatorEvent};

/// A caption that has waited this long for its turn is answered untranslated: by the time it
/// was translated, later ones would be late too.
const MAX_WAIT: Duration = Duration::from_secs(4);
/// This many failed translations in a row pause the translator for `RETRY_AFTER`.
const MAX_FAILURES: u32 = 3;
const RETRY_AFTER: Duration = Duration::from_secs(15);

/// Thread "hm-tr": ensures MADLAD or pings the LLM endpoint, then serves requests.
pub struct TranslatorWorker {
    pub events: Receiver<TranslatorEvent>,
    tx: Sender<TranslateRequest>,
    cancel: Arc<AtomicBool>,
}

impl TranslatorWorker {
    pub fn spawn(choice: TranslatorChoice) -> TranslatorWorker {
        let (ev_tx, events) = crossbeam_channel::unbounded();
        let (tx, rx) = crossbeam_channel::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::Builder::new()
            .name("hm-tr".into())
            .spawn(move || run(choice, rx, ev_tx, &c))
            .expect("spawn hm-tr");
        TranslatorWorker { events, tx, cancel }
    }

    pub fn submit(&self, req: TranslateRequest) {
        let _ = self.tx.send(req);
    }
}

impl Drop for TranslatorWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn run(
    choice: TranslatorChoice,
    rx: Receiver<TranslateRequest>,
    ev: Sender<TranslatorEvent>,
    cancel: &AtomicBool,
) {
    let tr = match load(choice, &ev, cancel) {
        Ok(t) => {
            let _ = ev.send(TranslatorEvent::Ready);
            Some(t)
        }
        Err(e) => {
            log::error!("translator load failed: {e:#}");
            let _ = ev.send(TranslatorEvent::Error(format!("{e:#}")));
            None
        }
    };
    serve(tr, rx, ev);
}

fn serve(
    mut tr: Option<Box<dyn Translate>>,
    rx: Receiver<TranslateRequest>,
    ev: Sender<TranslatorEvent>,
) {
    let mut failures = 0;
    let mut paused_until = None;
    // Without a translator every request still gets an (empty) answer.
    while let Ok(req) = rx.recv() {
        let text = if req.source == req.target {
            Some(req.text)
        } else if req.at.elapsed() > MAX_WAIT {
            log::info!("translation backlog, skipped #{}", req.id);
            None
        } else if paused_until.is_some_and(|t| Instant::now() < t) {
            None
        } else if let Some(t) = tr.as_mut() {
            t.set_hint(&req.hint);
            match t.translate(&req.text, &req.source, &req.target, &req.context) {
                Ok(text) => {
                    failures = 0;
                    Some(text)
                }
                Err(e) => {
                    log::warn!("translate failed: {e:#}");
                    failures += 1;
                    if failures == MAX_FAILURES {
                        failures = 0;
                        paused_until = Some(Instant::now() + RETRY_AFTER);
                        let _ = ev.send(TranslatorEvent::Trouble(format!("{e:#}")));
                    }
                    None
                }
            }
        } else {
            None
        };
        if ev
            .send(TranslatorEvent::Result { id: req.id, text })
            .is_err()
        {
            break;
        }
    }
}

fn load(
    choice: TranslatorChoice,
    ev: &Sender<TranslatorEvent>,
    cancel: &AtomicBool,
) -> anyhow::Result<Box<dyn Translate>> {
    match choice {
        TranslatorChoice::Builtin => {
            let model = ModelId::Madlad3bQ4k;
            let progress = |phase, done, total| {
                let _ = ev.send(TranslatorEvent::Progress(Progress {
                    model,
                    phase,
                    done,
                    total,
                }));
            };
            let (device, label) = crate::device::best_device(true);
            let m = models::ensure_and_load(
                model,
                &mut |done, total| progress(ProgressPhase::Downloading, done, total),
                cancel,
                |paths| {
                    progress(ProgressPhase::Loading, 0, None);
                    let t = std::time::Instant::now();
                    let m = madlad::Madlad::load(paths, &device)?;
                    log::info!("MADLAD loaded on {label} in {:?}", t.elapsed());
                    Ok(m)
                },
            )?;
            Ok(Box::new(m))
        }
        #[cfg(windows)]
        TranslatorChoice::Natural { light } => {
            let model = ModelId::natural(light);
            let progress = |phase, done, total| {
                let _ = ev.send(TranslatorEvent::Progress(Progress {
                    model,
                    phase,
                    done,
                    total,
                }));
            };
            let paths = models::ensure(
                model,
                &mut |done, total| progress(ProgressPhase::Downloading, done, total),
                cancel,
            )?;
            progress(ProgressPhase::Loading, 0, None);
            Ok(Box::new(Natural::start(paths.weights, cancel)?))
        }
        #[cfg(not(windows))]
        TranslatorChoice::Natural { .. } => {
            anyhow::bail!("the Natural translator runs on Windows only for now")
        }
        TranslatorChoice::Llm { base_url, model } => {
            let llm = llm::LlmTranslator::new(&base_url, &model);
            llm.ping()?;
            Ok(Box::new(llm))
        }
    }
}

/// The LLM translator on a llama.cpp server of our own, which is started again if it dies.
#[cfg(windows)]
struct Natural {
    model: std::path::PathBuf,
    server: server::Server,
    llm: llm::LlmTranslator,
    hint: String,
}

#[cfg(windows)]
impl Natural {
    fn start(model: std::path::PathBuf, cancel: &AtomicBool) -> anyhow::Result<Self> {
        let server = server::Server::start(&model, &mut |_, _| {}, cancel)?;
        let llm = llm::LlmTranslator::own(&server.base_url());
        let t = Instant::now();
        llm.warm_up()?;
        log::info!("language model warmed up in {:?}", t.elapsed());
        Ok(Self {
            model,
            server,
            llm,
            hint: String::new(),
        })
    }
}

/// The Natural translator over its downloaded `model`, for tools.
#[cfg(windows)]
pub fn natural(model: std::path::PathBuf) -> anyhow::Result<Box<dyn Translate>> {
    Ok(Box::new(Natural::start(model, &AtomicBool::new(false))?))
}

#[cfg(windows)]
impl Translate for Natural {
    fn set_hint(&mut self, hint: &str) {
        self.hint = hint.to_string();
    }

    fn translate(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
    ) -> anyhow::Result<String> {
        self.llm.set_hint(&self.hint);
        let r = self.llm.translate(text, source, target, context);
        if r.is_err() && !self.server.alive() {
            log::error!("llama.cpp stopped, starting it again");
            *self = Self::start(self.model.clone(), &AtomicBool::new(false))?;
        }
        r
    }
}

/// Babil's EN glossary: steers streaming slang to words MT models translate well.
static EN_GLOSSARY: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
    [
        (r"(?i)\bstreamers\b", "broadcasters"),
        (r"(?i)\bstreamer\b", "broadcaster"),
        (r"(?i)\bstreaming\b", "broadcasting"),
        (r"(?i)\bstreams\b", "broadcasts"),
        (r"(?i)\bstream(ed)?\b", "broadcast"),
        (r"(?i)\bsubs\b", "subscribers"),
        (r"(?i)\bsubbed\b", "subscribed"),
        (r"(?i)\bguys\b", "friends"),
    ]
    .into_iter()
    .map(|(re, to)| (Regex::new(re).unwrap(), to))
    .collect()
});

pub fn apply_glossary(text: &str) -> String {
    EN_GLOSSARY.iter().fold(text.to_string(), |t, (re, to)| {
        re.replace_all(&t, *to).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glossary() {
        assert_eq!(
            apply_glossary("Thanks guys, the Streamer streamed for subs"),
            "Thanks friends, the broadcaster broadcast for subscribers"
        );
        assert_eq!(
            apply_glossary("mainstream upstreams"),
            "mainstream upstreams"
        );
    }

    #[test]
    #[ignore = "needs the MADLAD download (1.67 GB)"]
    fn madlad_worker() {
        let w = TranslatorWorker::spawn(TranslatorChoice::Builtin);
        w.submit(TranslateRequest {
            id: 7,
            text: "Thanks guys, how are you?".into(),
            source: "en".into(),
            target: "tr".into(),
            context: vec![],
            hint: String::new(),
            at: Instant::now(),
        });
        let mut ready = false;
        loop {
            match w.events.recv().unwrap() {
                TranslatorEvent::Ready => ready = true,
                TranslatorEvent::Result { id, text } => {
                    assert!(ready);
                    assert_eq!(id, 7);
                    println!("{text:?}");
                    assert!(text.unwrap().to_lowercase().contains("arkadaş"));
                    break;
                }
                TranslatorEvent::Error(e) | TranslatorEvent::Trouble(e) => panic!("{e}"),
                TranslatorEvent::Progress(_) => {}
            }
        }
    }

    #[test]
    fn passthrough_and_no_translator() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev) = crossbeam_channel::unbounded();
        let llm = TranslatorChoice::Llm {
            base_url: "http://127.0.0.1:9".into(),
            model: "x".into(),
        };
        let h = std::thread::spawn(move || run(llm, rx, ev_tx, &AtomicBool::new(false)));
        for (id, target) in [(1, "en"), (2, "tr")] {
            tx.send(TranslateRequest {
                id,
                text: "hi".into(),
                source: "en".into(),
                target: target.into(),
                context: vec![],
                hint: String::new(),
                at: Instant::now(),
            })
            .unwrap();
        }
        drop(tx);
        h.join().unwrap();
        let results: Vec<_> = ev
            .iter()
            .filter_map(|e| match e {
                TranslatorEvent::Result { id, text } => Some((id, text)),
                _ => None,
            })
            .collect();
        assert_eq!(results, vec![(1, Some("hi".into())), (2, None)]);
    }

    struct Fake(Vec<anyhow::Result<String>>);

    impl Translate for Fake {
        fn translate(
            &mut self,
            _: &str,
            _: &str,
            _: &str,
            _: &[(String, String)],
        ) -> anyhow::Result<String> {
            self.0.remove(0)
        }
    }

    fn req(id: u64) -> TranslateRequest {
        TranslateRequest {
            id,
            text: "hi".into(),
            source: "en".into(),
            target: "tr".into(),
            context: vec![],
            hint: String::new(),
            at: Instant::now(),
        }
    }

    #[test]
    fn stale_requests_are_skipped_and_failures_pause() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev) = crossbeam_channel::unbounded();
        // 2 that have waited too long are skipped, the 3 after them translated.
        for id in 1..=5 {
            let mut r = req(id);
            if id <= 2 {
                r.at -= MAX_WAIT + Duration::from_secs(1);
            }
            tx.send(r).unwrap();
        }
        drop(tx);
        let fake = Fake((0..3).map(|i| Ok(format!("t{i}"))).collect());
        serve(Some(Box::new(fake)), rx, ev_tx);
        let got: Vec<_> = ev.try_iter().collect();
        assert!(matches!(
            &got[..],
            [
                TranslatorEvent::Result { id: 1, text: None },
                TranslatorEvent::Result { id: 2, text: None },
                TranslatorEvent::Result {
                    id: 3,
                    text: Some(_)
                },
                TranslatorEvent::Result {
                    id: 4,
                    text: Some(_)
                },
                TranslatorEvent::Result {
                    id: 5,
                    text: Some(_)
                },
            ]
        ));

        // 3 failures in a row report trouble, then requests are answered without trying for a while.
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev) = crossbeam_channel::unbounded();
        let h = std::thread::spawn(move || {
            let fake = Fake((0..3).map(|_| Err(anyhow::anyhow!("down"))).collect());
            serve(Some(Box::new(fake)), rx, ev_tx)
        });
        for id in 1..=4 {
            tx.send(req(id)).unwrap();
            ev.recv().unwrap();
        }
        drop(tx);
        h.join().unwrap();
        let rest: Vec<_> = ev.try_iter().collect();
        assert!(matches!(
            &rest[..],
            [TranslatorEvent::Result { id: 4, text: None }]
        ));
    }
}

pub mod llm;
pub mod madlad;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use crossbeam_channel::{Receiver, Sender};
use regex::Regex;

use crate::models::{self, ModelId};
use crate::settings::TranslatorChoice;
use crate::types::{Progress, ProgressPhase, Translate, TranslateRequest, TranslatorEvent};

/// Requests waiting beyond this are answered untranslated.
const MAX_QUEUE: usize = 3;
/// This many failed translations in a row disable the translator until the next Start.
const MAX_FAILURES: u32 = 3;

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
    // Without a translator every request still gets an (empty) answer.
    while let Ok(req) = rx.recv() {
        let text = if req.source == req.target {
            Some(req.text)
        } else if rx.len() >= MAX_QUEUE {
            // Falling behind: skip the oldest so translations stay current (the caption
            // is already shown untranslated).
            log::info!("translation backlog, skipped #{}", req.id);
            None
        } else if let Some(t) = tr.as_mut() {
            match t.translate(&req.text, &req.source, &req.target, &req.context) {
                Ok(text) => {
                    failures = 0;
                    Some(text)
                }
                Err(e) => {
                    log::warn!("translate failed: {e:#}");
                    failures += 1;
                    if failures == MAX_FAILURES {
                        let _ = ev.send(TranslatorEvent::Error(format!("{e:#}")));
                        tr = None;
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
        TranslatorChoice::Llm { base_url, model } => {
            let llm = llm::LlmTranslator::new(&base_url, &model);
            llm.ping()?;
            Ok(Box::new(llm))
        }
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
                TranslatorEvent::Error(e) => panic!("{e}"),
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
        }
    }

    #[test]
    fn backlog_skips_oldest_and_failures_disable() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev) = crossbeam_channel::unbounded();
        // 5 queued at once: the 2 oldest are skipped, the other 3 translated.
        for id in 1..=5 {
            tx.send(req(id)).unwrap();
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

        // 3 failures in a row report an error, then requests are answered without trying.
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

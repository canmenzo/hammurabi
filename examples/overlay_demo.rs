use std::time::{Duration, Instant};

use eframe::egui::{self, Key, ViewportCommand};
use hammurabi::settings::Settings;
use hammurabi::types::{CaptionUpdate, Status, StatusLevel};
use hammurabi::ui::overlay::{OverlayState, layout_for_monitor, viewport_builder};

const LINES: [(&str, &str); 4] = [
    ("Welcome back to the channel, today we are looking at something new.", "Kanala tekrar hos geldiniz, bugun yeni bir seye bakiyoruz."),
    ("The results were far better than anyone had expected.", "Sonuclar herkesin bekledigi her seyden cok daha iyiydi."),
    ("Let me show you how this works in practice.", "Bunun pratikte nasil calistigini size gostereyim."),
    ("Thanks for watching, see you next time!", "Izledigin icin tesekkurler, bir sonraki videoda gorusuruz!"),
];

enum Step {
    Live(&'static str),
    Final(u64, bool),
    Status,
    Move(bool),
}

struct Demo {
    settings: Settings,
    overlay: OverlayState,
    start: Instant,
    script: Vec<(f32, Step)>,
    next: usize,
    move_mode: bool,
    placed: bool,
}

impl Demo {
    fn new() -> Self {
        let mut script = vec![(0.5, Step::Status)];
        for (i, (orig, _)) in LINES.iter().enumerate() {
            let t = 1.0 + i as f32 * 3.0;
            script.push((t, Step::Live(orig)));
            script.push((t + 1.2, Step::Final(i as u64, false)));
            script.push((t + 1.8, Step::Final(i as u64, true)));
        }
        script.push((14.0, Step::Move(true)));
        script.push((20.0, Step::Move(false)));
        Self {
            settings: Settings { target_language: "tr".into(), ..Settings::default() },
            overlay: OverlayState::default(),
            start: Instant::now(),
            script,
            next: 0,
            move_mode: false,
            placed: false,
        }
    }

    fn set_move(&mut self, ctx: &egui::Context, on: bool) {
        self.move_mode = on;
        ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(!on));
    }
}

impl eframe::App for Demo {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let t = now.duration_since(self.start).as_secs_f32();
        while self.next < self.script.len() && self.script[self.next].0 <= t {
            match self.script[self.next].1 {
                Step::Live(o) => self.overlay.apply(
                    CaptionUpdate::Live { original: o.into(), translated: None, lang: Some("en".into()) },
                    &self.settings,
                    now,
                ),
                Step::Final(i, translated) => {
                    let (o, tr) = LINES[i as usize];
                    if !translated {
                        self.overlay.apply(CaptionUpdate::Live { original: String::new(), translated: None, lang: None }, &self.settings, now);
                    }
                    self.overlay.apply(
                        CaptionUpdate::Final {
                            id: i,
                            original: o.into(),
                            translated: translated.then(|| tr.into()),
                            lang: "en".into(),
                        },
                        &self.settings,
                        now,
                    );
                }
                Step::Status => self.overlay.set_status(
                    Status { level: StatusLevel::Info, text: "Downloading translation model: 42%".into(), ttl: Some(Duration::from_secs(4)) },
                    now,
                ),
                Step::Move(on) => self.set_move(ctx, on),
            }
            self.next += 1;
        }
        if ctx.input(|i| i.key_pressed(Key::M)) {
            self.set_move(ctx, !self.move_mode);
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.placed {
            if let Some(m) = ui.ctx().input(|i| i.viewport().monitor_size) {
                let (pos, size) = layout_for_monitor(m, &self.settings.caption, self.settings.overlay_pos);
                ui.ctx().send_viewport_cmd(ViewportCommand::OuterPosition(pos));
                ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(size));
                self.placed = true;
            }
        }
        let style = self.settings.caption.clone();
        let r = self.overlay.paint(ui, &style, self.move_mode);
        if r.start_drag {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if r.finish_move {
            self.set_move(&ui.ctx().clone(), false);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}

fn main() -> eframe::Result {
    let settings = Settings::default();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: viewport_builder(&settings).with_visible(true),
        ..Default::default()
    };
    eframe::run_native("Hammurabi overlay demo", options, Box::new(|_| Ok(Box::new(Demo::new()))))
}

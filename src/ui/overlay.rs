use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Galley, Pos2, Rect, Shape, Stroke, Ui,
    Vec2, ViewportBuilder, vec2,
};

use crate::settings::{CaptionStyle, Edge, Settings};
use crate::types::{CaptionUpdate, Status, StatusLevel};

const FADE: Duration = Duration::from_millis(250);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 8.0;
const BLOCK_GAP: f32 = 6.0;
const LINE_GAP: f32 = 2.0;
const MOVE_HINT: &str = "Drag to move \u{b7} double-click to finish";

struct Line {
    original: String,
    translated: Option<String>,
    /// Source language equals the target, so the original is the caption.
    same_lang: bool,
}

impl Line {
    fn new(original: String, translated: Option<String>, lang: Option<&str>, target: &str) -> Self {
        Self {
            original,
            translated: translated.filter(|t| !t.trim().is_empty()),
            same_lang: lang.is_some_and(|l| l == target),
        }
    }

    fn chars(&self) -> usize {
        let t = self.translated.as_deref().map_or(0, |t| t.chars().count());
        t.max(self.original.chars().count())
    }

    /// (text, is_translation_style) in display order.
    fn lines(&self, style: &CaptionStyle) -> Vec<(&str, bool)> {
        let mut out = Vec::with_capacity(2);
        match &self.translated {
            Some(t) => {
                out.push((t.as_str(), true));
                if style.show_original && !self.same_lang && !self.original.is_empty() {
                    out.push((self.original.as_str(), false));
                }
            }
            None if self.same_lang || !style.show_original => {
                out.push((self.original.as_str(), true))
            }
            None => out.push((self.original.as_str(), false)),
        }
        if style.original_first {
            out.reverse();
        }
        out
    }
}

struct Cue {
    id: u64,
    line: Line,
    shown: Instant,
    hold: Duration,
}

impl Cue {
    fn hold_end(&self) -> Instant {
        self.shown + self.hold
    }
}

struct StatusPill {
    level: StatusLevel,
    text: String,
    expires: Option<Instant>,
}

#[derive(Default)]
pub struct OverlayState {
    cues: Vec<Cue>,
    live: Option<Line>,
    status: Option<StatusPill>,
    hold_secs: f32,
    last_press: Option<Instant>,
}

#[derive(Default)]
pub struct OverlayResponse {
    /// Primary press in move mode: caller sends `ViewportCommand::StartDrag`.
    pub start_drag: bool,
    /// Double-click in move mode: caller leaves move mode and saves the position.
    pub finish_move: bool,
}

impl OverlayState {
    pub fn apply(&mut self, update: CaptionUpdate, settings: &Settings, now: Instant) {
        let target = settings.target_language.as_str();
        self.hold_secs = settings.caption.hold_secs;
        match update {
            CaptionUpdate::Final {
                id,
                original,
                translated,
                lang,
            } => {
                let line = Line::new(original, translated, Some(&lang), target);
                let hold = Duration::from_secs_f32(
                    self.hold_secs.max(line.chars() as f32 / 15.0).max(0.0),
                );
                if let Some(c) = self.cues.iter_mut().find(|c| c.id == id) {
                    c.line = line;
                    c.hold = hold;
                } else {
                    self.cues.push(Cue {
                        id,
                        line,
                        shown: now,
                        hold,
                    });
                    let keep = settings.caption.cues.max(1) as usize;
                    let drop = self.cues.len().saturating_sub(keep);
                    self.cues.drain(..drop);
                }
            }
            CaptionUpdate::Live {
                original,
                translated,
                lang,
            } => {
                self.live = (!original.trim().is_empty())
                    .then(|| Line::new(original, translated, lang.as_deref(), target));
            }
            CaptionUpdate::Clear => {
                self.cues.clear();
                self.live = None;
            }
        }
    }

    pub fn set_status(&mut self, status: Status, now: Instant) {
        self.status = Some(StatusPill {
            level: status.level,
            text: status.text,
            expires: status.ttl.map(|t| now + t),
        });
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// Next time a cue or status expires or fades; the caller schedules a repaint for it.
    pub fn next_deadline(&self) -> Option<Instant> {
        let now = Instant::now();
        let cues = self.cues.iter().map(|c| {
            if now < c.hold_end() {
                c.hold_end()
            } else {
                c.hold_end() + FADE
            }
        });
        let status = self.status.as_ref().and_then(|s| s.expires);
        cues.chain(status).min()
    }

    pub fn paint(&mut self, ui: &mut Ui, style: &CaptionStyle, move_mode: bool) -> OverlayResponse {
        let now = Instant::now();
        self.cues.retain(|c| now < c.hold_end() + FADE);
        if self
            .status
            .as_ref()
            .is_some_and(|s| s.expires.is_some_and(|e| now >= e))
        {
            self.status = None;
        }

        let rect = ui.max_rect();
        let painter = ui.painter().clone();
        let wrap = (rect.width() - 2.0 * PAD_X - 8.0).max(50.0);
        let mut y = rect.bottom() - 4.0;
        let mut fading = false;

        let mut blocks: Vec<(&Line, f32)> = Vec::new();
        let skip = self.cues.len().saturating_sub(style.cues.max(1) as usize);
        for c in &self.cues[skip..] {
            let left = (c.hold_end() + FADE).saturating_duration_since(now);
            let alpha = if left < FADE {
                fading = true;
                left.as_secs_f32() / FADE.as_secs_f32()
            } else {
                1.0
            };
            blocks.push((&c.line, alpha));
        }
        if let Some(l) = &self.live {
            blocks.push((l, 1.0));
        }

        let mut draw = |lines: Vec<(Arc<Galley>, Color32)>, bg: Color32, alpha: f32| {
            let w = lines.iter().map(|(g, _)| g.size().x).fold(0.0, f32::max);
            let h = lines.iter().map(|(g, _)| g.size().y).sum::<f32>()
                + LINE_GAP * lines.len().saturating_sub(1) as f32;
            let size = vec2(w + 2.0 * PAD_X, h + 2.0 * PAD_Y);
            let r = Rect::from_center_size(Pos2::new(rect.center().x, y - size.y / 2.0), size);
            painter.rect_filled(r, CornerRadius::same(10), bg.gamma_multiply(alpha));
            let mut ty = r.top() + PAD_Y;
            for (g, color) in lines {
                let pos = Pos2::new(rect.center().x - g.size().x / 2.0, ty);
                paint_text(
                    &painter,
                    pos,
                    &g,
                    color.gamma_multiply(alpha),
                    style.edge,
                    alpha,
                );
                ty += g.size().y + LINE_GAP;
            }
            y -= size.y + BLOCK_GAP;
        };

        let bg = Color32::from_black_alpha((style.bg_opacity.clamp(0.0, 1.0) * 255.0) as u8);

        if let Some(s) = &self.status {
            let color = match s.level {
                StatusLevel::Info => Color32::WHITE,
                StatusLevel::Warning => Color32::from_rgb(255, 200, 80),
                StatusLevel::Error => Color32::from_rgb(255, 120, 110),
            };
            let g = painter.layout(
                s.text.clone(),
                FontId::proportional(18.0),
                Color32::WHITE,
                wrap,
            );
            draw(vec![(g, color)], Color32::from_black_alpha(215), 1.0);
        }

        let trans_color = parse_color(&style.trans_color);
        let orig_color = parse_color(&style.orig_color);
        for (line, alpha) in blocks.into_iter().rev() {
            let lines = line
                .lines(style)
                .into_iter()
                .map(|(text, is_trans)| {
                    let (size, color) = if is_trans {
                        (style.trans_size, trans_color)
                    } else {
                        (style.orig_size, orig_color)
                    };
                    let g = painter.layout(
                        text.to_owned(),
                        FontId::proportional(size),
                        Color32::WHITE,
                        wrap,
                    );
                    (g, color)
                })
                .collect();
            draw(lines, bg, alpha);
        }

        if fading {
            ui.ctx().request_repaint();
        } else if let Some(d) = self.next_deadline() {
            ui.ctx()
                .request_repaint_after(d.saturating_duration_since(now));
        }

        let mut resp = OverlayResponse::default();
        if move_mode {
            let frame = rect.shrink(2.0);
            let path = [
                frame.left_top(),
                frame.right_top(),
                frame.right_bottom(),
                frame.left_bottom(),
                frame.left_top(),
            ];
            let stroke = Stroke::new(2.0, Color32::from_rgb(255, 224, 102));
            painter.extend(Shape::dashed_line(&path, stroke, 10.0, 6.0));
            let g = painter.layout_no_wrap(
                MOVE_HINT.into(),
                FontId::proportional(16.0),
                Color32::WHITE,
            );
            let hint = Align2::CENTER_TOP.anchor_size(
                frame.center_top() + vec2(0.0, 8.0),
                g.size() + vec2(20.0, 8.0),
            );
            painter.rect_filled(hint, CornerRadius::same(8), Color32::from_black_alpha(200));
            painter.galley(hint.center() - g.size() / 2.0, g, Color32::WHITE);

            ui.ctx().set_cursor_icon(CursorIcon::Grab);
            let pressed = ui.input(|i| {
                i.pointer.primary_pressed()
                    && i.pointer.latest_pos().is_some_and(|p| rect.contains(p))
            });
            if pressed {
                if self
                    .last_press
                    .is_some_and(|t| now.duration_since(t) < DOUBLE_CLICK)
                {
                    resp.finish_move = true;
                    self.last_press = None;
                } else {
                    resp.start_drag = true;
                    self.last_press = Some(now);
                }
            }
        } else {
            self.last_press = None;
        }
        resp
    }

    pub fn is_empty(&self) -> bool {
        let now = Instant::now();
        self.live.is_none()
            && !self.has_status()
            && !self.cues.iter().any(|c| now < c.hold_end() + FADE)
    }

    pub fn has_status(&self) -> bool {
        let now = Instant::now();
        self.status
            .as_ref()
            .is_some_and(|s| s.expires.is_none_or(|e| now < e))
    }
}

fn paint_text(
    p: &egui::Painter,
    pos: Pos2,
    g: &Arc<Galley>,
    color: Color32,
    edge: Edge,
    alpha: f32,
) {
    let black = Color32::from_black_alpha(230).gamma_multiply(alpha);
    match edge {
        Edge::None => {}
        Edge::Shadow => p.galley_with_override_text_color(pos + vec2(2.0, 2.0), g.clone(), black),
        Edge::Outline => {
            for (dx, dy) in [
                (-1.5, 0.0),
                (1.5, 0.0),
                (0.0, -1.5),
                (0.0, 1.5),
                (-1.1, -1.1),
                (1.1, -1.1),
                (-1.1, 1.1),
                (1.1, 1.1),
            ] {
                p.galley_with_override_text_color(pos + vec2(dx, dy), g.clone(), black);
            }
        }
    }
    p.galley_with_override_text_color(pos, g.clone(), color);
}

fn parse_color(s: &str) -> Color32 {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let rgba = match h.len() {
        6 => byte(0)
            .zip(byte(2))
            .zip(byte(4))
            .map(|((r, g), b)| (r, g, b, 255)),
        8 => byte(0)
            .zip(byte(2))
            .zip(byte(4))
            .zip(byte(6))
            .map(|(((r, g), b), a)| (r, g, b, a)),
        _ => None,
    };
    rgba.map_or(Color32::WHITE, |(r, g, b, a)| {
        Color32::from_rgba_unmultiplied(r, g, b, a)
    })
}

/// Root viewport: borderless, transparent, on top, click-through, no taskbar entry, hidden.
pub fn viewport_builder(settings: &Settings) -> ViewportBuilder {
    // Real monitor size is unknown until the window exists; the app re-lays it out then.
    let (pos, size) = layout_for_monitor(
        vec2(1920.0, 1080.0),
        &settings.caption,
        settings.overlay_pos,
    );
    ViewportBuilder::default()
        .with_title("Hammurabi")
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_taskbar(false)
        .with_resizable(false)
        .with_has_shadow(false)
        .with_active(false)
        .with_visible(false)
        .with_position(pos)
        .with_inner_size(size)
}

/// Outer position and inner size (logical px) for a monitor of `monitor` size.
/// `saved` is `Settings::overlay_pos`, which wins over the default bottom-centre spot.
pub fn layout_for_monitor(
    monitor: Vec2,
    style: &CaptionStyle,
    saved: Option<[f32; 2]>,
) -> (Pos2, Vec2) {
    let block = (style.trans_size + style.orig_size) * 1.25 + 2.0 * PAD_Y + LINE_GAP + BLOCK_GAP;
    let size = vec2(monitor.x * 0.8, block * (style.cues.max(1) + 1) as f32);
    let pos = match saved {
        Some([x, y]) => Pos2::new(x, y),
        None => Pos2::new((monitor.x - size.x) / 2.0, monitor.y * 0.92 - size.y),
    };
    (pos, size)
}

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, FontId, Galley, Pos2, Rect, Ui, Vec2,
    ViewportBuilder, pos2, vec2,
};

use crate::screen::{Area, Place, Spot};
use crate::settings::{CaptionStyle, Edge, Settings};
use crate::types::{CaptionUpdate, Status, StatusLevel};

const FADE: Duration = Duration::from_millis(250);
const FADE_IN: Duration = Duration::from_millis(120);
/// How often a fade is painted. Asking for the next frame at once leaves the pace to the driver,
/// which without vertical sync paints thousands of frames a second.
const FADE_FRAME: Duration = Duration::from_millis(16);
pub const SAMPLE_ORIGINAL: &str = "This is how captions will look.";
/// How long an error stays over the game; Settings keeps showing it.
const ERROR_TTL: Duration = Duration::from_secs(20);
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 8.0;
const BLOCK_GAP: f32 = 6.0;
const LINE_GAP: f32 = 2.0;
// The room a cover leaves around the subtitle across and above and below, in heights of the
// subtitle's row, for the letters' shadows and outlines. Its letters are as large as any
// translated text.
const COVER_PAD_X: f32 = 0.5;
const COVER_PAD_Y: f32 = 0.3;
/// A subtitle that comes this long after the cover was last up starts with an empty one, not
/// with the translation of the one before.
const COVER_GAP: Duration = Duration::from_millis(500);

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

/// Subtitles read off the screen, covered by their translations (`Settings::covers`).
#[derive(Default)]
struct Cover {
    place: Place,
    /// The subtitle covered last, which the cover stays over while its translation holds.
    spot: Option<Spot>,
    /// When the subtitle on the screen went.
    gone: Option<Instant>,
    /// The translation in the cover: its caption, its text, and until when it holds.
    text: Option<(u64, String, Instant)>,
}

#[derive(Default)]
pub struct OverlayState {
    cues: Vec<Cue>,
    live: Option<Line>,
    status: Option<StatusPill>,
    hold_secs: f32,
    /// The bubbles painted last, in window coordinates.
    bubbles: Vec<Rect>,
    cover: Cover,
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
                // Covering, a caption comes once its translation is in: one without a
                // translation has none coming, and shows as it was read.
                if settings.covers() && self.cover.text.as_ref().is_none_or(|t| t.0 <= id) {
                    let text = line.translated.as_ref().unwrap_or(&line.original);
                    self.cover.text = Some((id, text.clone(), now + hold));
                }
                if let Some(c) = self.cues.iter_mut().find(|c| c.id == id) {
                    let mut line = line;
                    if line.original != c.line.original {
                        // The caption grew: it stays up as long as a new one would, and keeps
                        // the translation it has until the one for all of it comes.
                        c.hold = now.saturating_duration_since(c.shown) + hold;
                        if line.translated.is_none() {
                            line.translated = c.line.translated.take();
                        }
                    }
                    c.line = line;
                } else if line.translated.is_some() && self.cues.last().is_some_and(|c| c.id > id) {
                    // A translation that comes after its caption left: newer ones are up already.
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
                self.cover = Cover::default();
            }
        }
    }

    /// Where subtitles are read off the screen now.
    pub fn place(&mut self, place: Place, now: Instant) {
        let c = &mut self.cover;
        match place.subtitle {
            Some(spot) => {
                // When the cover went: once the subtitle went and its translation stopped holding.
                let went = c.gone.map(|g| c.text.as_ref().map_or(g, |t| g.max(t.2)));
                if went.is_some_and(|w| now.saturating_duration_since(w) >= COVER_GAP) {
                    c.text = None;
                }
                c.gone = None;
                c.spot = Some(spot);
            }
            None => c.gone = c.gone.or(Some(now)),
        }
        c.place = place;
    }

    /// The picture subtitles are read in, on the desktop in pixels, while there is one.
    pub fn picture(&self) -> Option<[i32; 4]> {
        self.cover.place.picture
    }

    /// The cover, while the subtitle stands or its translation holds: over what, and the
    /// translation in it. Never the subtitle itself: empty until its translation is in.
    pub fn covered(&self, now: Instant) -> Option<(Spot, Option<&str>)> {
        let c = &self.cover;
        let held = c.text.as_ref().is_some_and(|t| now < t.2);
        let spot = c.place.subtitle.or(c.spot.filter(|_| held))?;
        Some((spot, c.text.as_ref().map(|t| t.1.as_str())))
    }

    pub fn set_status(&mut self, status: Status, now: Instant) {
        // An info line ("Running: ...") must not hide a warning the user has not had time to read.
        if matches!(status.level, StatusLevel::Info)
            && self.status.as_ref().is_some_and(|s| {
                !matches!(s.level, StatusLevel::Info) && s.expires.is_none_or(|e| e > now)
            })
        {
            return;
        }
        self.status = Some(StatusPill {
            level: status.level,
            text: status.text,
            expires: status
                .ttl
                .or(matches!(status.level, StatusLevel::Error).then_some(ERROR_TTL))
                .map(|t| now + t),
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
        let cover = self.cover.text.as_ref().map(|t| t.2).filter(|&t| now < t);
        cues.chain(status).chain(cover).min()
    }

    pub fn bubbles(&self) -> &[Rect] {
        &self.bubbles
    }

    /// `sample`: a translation to show as a sample caption while nothing else is on screen.
    /// `cover`: the window stands over this band of the picture subtitles are read in, and
    /// covers them in place of captions. Returns whether a bubble was just pressed, which
    /// starts dragging the window.
    pub fn paint(
        &mut self,
        ui: &mut Ui,
        style: &CaptionStyle,
        sample: Option<&str>,
        cover: Option<Area>,
    ) -> bool {
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
        let mut bubbles = Vec::new();

        let sample = sample.filter(|_| self.quiet()).map(|t| Line {
            original: SAMPLE_ORIGINAL.into(),
            translated: Some(t.into()),
            same_lang: false,
        });
        let trans_color = parse_color(&style.trans_color);
        if let Some(band) = cover
            && let Some((spot, text)) = self.covered(now)
        {
            let r = in_window(rect, band, spot.area);
            let row = spot.row / (band.bottom - band.top) * rect.height();
            let pad = vec2(COVER_PAD_X, COVER_PAD_Y) * row;
            let mut job = LayoutJob::simple(
                text.unwrap_or_default().to_owned(),
                FontId::proportional(style.trans_size),
                Color32::WHITE,
                (0.9 * rect.width()).max(r.width()),
            );
            job.halign = Align::Center;
            let g = painter.layout_job(job);
            // Over the subtitle and the room around it, and as large as its translation needs,
            // growing up from the subtitle's bottom as subtitles do.
            let size = (r.size() + 2.0 * pad).max(g.size() + 2.0 * pad);
            let b = Rect::from_min_size(
                pos2(r.center().x - size.x / 2.0, r.bottom() + pad.y - size.y),
                size,
            );
            painter.rect_filled(b, CornerRadius::same(4), Color32::BLACK);
            let at = pos2(b.center().x, b.center().y - g.size().y / 2.0);
            paint_text(&painter, at, &g, trans_color, style.edge, 1.0);
        }

        let mut blocks: Vec<(&Line, f32)> = Vec::new();
        let skip = self.cues.len().saturating_sub(style.cues.max(1) as usize);
        for c in self.cues[skip..].iter().filter(|_| cover.is_none()) {
            let left = (c.hold_end() + FADE).saturating_duration_since(now);
            let age = now.saturating_duration_since(c.shown);
            let mut alpha = 1.0;
            if left < FADE {
                fading = true;
                alpha = left.as_secs_f32() / FADE.as_secs_f32();
            }
            if age < FADE_IN {
                fading = true;
                alpha = f32::min(alpha, age.as_secs_f32() / FADE_IN.as_secs_f32());
            }
            blocks.push((&c.line, alpha));
        }
        let live = self.live.as_ref().filter(|_| cover.is_none());
        if let Some(l) = live.or(sample.as_ref()) {
            blocks.push((l, 1.0));
        }

        // Blocks go up from the bottom, newest first. The window is sized for one-line captions,
        // so one that wraps, or a status, leaves no room for the oldest: it is left out, and so
        // is every block above it. The status and the newest caption are `keep`: drawn even
        // when they do not fit, with their top cut.
        let mut full = false;
        let mut draw = |lines: Vec<(Arc<Galley>, Color32)>, bg: Color32, alpha: f32, keep: bool| {
            let w = lines.iter().map(|(g, _)| g.size().x).fold(0.0, f32::max);
            let h = lines.iter().map(|(g, _)| g.size().y).sum::<f32>()
                + LINE_GAP * lines.len().saturating_sub(1) as f32;
            let size = vec2(w + 2.0 * PAD_X, h + 2.0 * PAD_Y);
            full = full || (!keep && y - size.y < rect.top());
            if full {
                return;
            }
            let r = Rect::from_center_size(Pos2::new(rect.center().x, y - size.y / 2.0), size);
            painter.rect_filled(r, CornerRadius::same(10), bg.gamma_multiply(alpha));
            bubbles.push(r);
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
            draw(vec![(g, color)], Color32::from_black_alpha(215), 1.0, true);
        }

        let orig_color = parse_color(&style.orig_color);
        for (i, (line, alpha)) in blocks.into_iter().rev().enumerate() {
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
            draw(lines, bg, alpha, i == 0);
        }

        if fading {
            ui.ctx().request_repaint_after(FADE_FRAME);
        } else if let Some(d) = self.next_deadline() {
            ui.ctx()
                .request_repaint_after(d.saturating_duration_since(now));
        }

        self.bubbles = bubbles;
        let hovered = ui
            .input(|i| i.pointer.latest_pos())
            .is_some_and(|p| self.bubbles.iter().any(|r| r.contains(p)));
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        hovered && ui.input(|i| i.pointer.primary_pressed())
    }

    pub fn is_empty(&self) -> bool {
        let now = Instant::now();
        self.live.is_none()
            && !self.has_status()
            && !self.cues.iter().any(|c| now < c.hold_end() + FADE)
    }

    /// No caption is up, so the sample stands in for one.
    pub fn quiet(&self) -> bool {
        self.cues.is_empty() && self.live.is_none()
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

/// Where `area` of a picture is in a window that stands over `band` of it, both as fractions of
/// the picture: the window's size in points holds its scale.
fn in_window(window: Rect, band: Area, area: Area) -> Rect {
    let x = |f: f32| window.left() + (f - band.left) / (band.right - band.left) * window.width();
    let y = |f: f32| window.top() + (f - band.top) / (band.bottom - band.top) * window.height();
    Rect::from_min_max(
        pos2(x(area.left), y(area.top)),
        pos2(x(area.right), y(area.bottom)),
    )
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
        .with_title(crate::screen::CAPTIONS_WINDOW)
        .with_icon(super::icons::window_icon())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_captions_and_a_status_stay_inside_the_window() {
        let settings = Settings::default();
        let (_, size) = layout_for_monitor(vec2(1920.0, 1080.0), &settings.caption, None);
        let window = Rect::from_min_size(Pos2::ZERO, size);
        let long = "so we were going down the highway and the cops were right behind us ".repeat(4);
        let mut overlay = OverlayState::default();
        let now = Instant::now();
        for id in 0..2 {
            let c = CaptionUpdate::Final {
                id,
                original: long.clone(),
                translated: Some(long.clone()),
                lang: "en".into(),
            };
            overlay.apply(c, &settings, now);
        }
        let live = CaptionUpdate::Live {
            original: long.clone(),
            translated: Some(long.clone()),
            lang: Some("en".into()),
        };
        overlay.apply(live, &settings, now);
        let status = Status {
            level: StatusLevel::Warning,
            text: "Translation unavailable".into(),
            ttl: None,
        };
        overlay.set_status(status, now);

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(window),
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            overlay.paint(ui, &settings.caption, None, None);
        })
        .drop_without_applying_deltas();

        // The status, then the newest caption, which may be cut at the top; what else is drawn
        // is whole inside the window.
        let bubbles = overlay.bubbles();
        assert!((2..4).contains(&bubbles.len()), "{bubbles:?}");
        let whole = |b: &Rect| window.contains_rect(*b);
        assert!(
            whole(&bubbles[0]) && bubbles[2..].iter().all(whole),
            "{bubbles:?}"
        );
        let newest = bubbles[1];
        assert!(
            newest.bottom() <= bubbles[0].top()
                && newest.left() >= window.left()
                && newest.right() <= window.right(),
            "{bubbles:?}"
        );
    }

    #[test]
    fn a_part_of_the_picture_is_found_in_the_window_at_any_scale() {
        // A 2560 by 1440 picture; the window stands over its band, 2048 by 360 pixels.
        let band = Area {
            left: 0.1,
            top: 0.75,
            right: 0.9,
            bottom: 1.0,
        };
        let spot = Area {
            left: 0.4,
            top: 0.85,
            right: 0.6,
            bottom: 0.9,
        };
        for scale in [1.0, 1.25, 1.5] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(2048.0, 360.0) / scale);
            let r = in_window(window, band, spot);
            let want = Rect::from_min_max(pos2(768.0, 144.0), pos2(1280.0, 216.0));
            assert!(
                (r.min - want.min / scale).length() < 0.01
                    && (r.max - want.max / scale).length() < 0.01,
                "{scale}: {r:?}"
            );
        }
    }

    #[test]
    fn a_cover_shows_translations_only_and_follows_the_subtitle() {
        let settings = Settings {
            asr_backend: crate::settings::AsrBackend::ScreenText,
            translate: true,
            target_language: "tr".into(),
            ..Settings::default()
        };
        let spot = |left: f32| Spot {
            area: Area {
                left,
                top: 0.85,
                right: left + 0.2,
                bottom: 0.9,
            },
            row: 0.03,
        };
        let at = |picture: bool, s: Option<Spot>| Place {
            picture: picture.then_some([0, 0, 1920, 1080]),
            subtitle: s,
        };
        let final_ = |id, original: &str, translated: Option<&str>| CaptionUpdate::Final {
            id,
            original: original.into(),
            translated: translated.map(String::from),
            lang: "en".into(),
        };
        let secs = Duration::from_secs_f32;
        let t0 = Instant::now();
        let mut o = OverlayState::default();
        // Up as soon as the subtitle is, empty until its translation is in.
        o.place(at(true, Some(spot(0.4))), t0);
        assert_eq!(o.covered(t0), Some((spot(0.4), None)));
        o.apply(
            final_(1, "Hello there.", Some("Merhaba.")),
            &settings,
            t0 + secs(0.5),
        );
        assert_eq!(
            o.covered(t0 + secs(0.5)),
            Some((spot(0.4), Some("Merhaba.")))
        );
        // It follows the subtitle, and stays where it was while the translation holds.
        o.place(at(true, Some(spot(0.3))), t0 + secs(0.6));
        o.place(at(true, None), t0 + secs(1.0));
        assert_eq!(
            o.covered(t0 + secs(4.0)),
            Some((spot(0.3), Some("Merhaba.")))
        );
        assert_eq!(o.covered(t0 + secs(4.6)), None);
        // The next subtitle soon after has the last translation until its own is in,
        o.place(at(true, Some(spot(0.4))), t0 + secs(4.8));
        assert_eq!(
            o.covered(t0 + secs(4.8)),
            Some((spot(0.4), Some("Merhaba.")))
        );
        // which is the subtitle as read when none is coming.
        o.apply(final_(2, "Yo.", None), &settings, t0 + secs(5.5));
        assert_eq!(o.covered(t0 + secs(5.5)), Some((spot(0.4), Some("Yo."))));
        // One long after starts empty.
        o.place(at(true, None), t0 + secs(6.0));
        o.place(at(true, Some(spot(0.4))), t0 + secs(12.0));
        assert_eq!(o.covered(t0 + secs(12.0)), Some((spot(0.4), None)));

        // Captions are not drawn as well; the cover is.
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 300.0));
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(window),
            ..Default::default()
        };
        let band = Area {
            left: 0.0,
            top: 0.75,
            right: 1.0,
            bottom: 1.0,
        };
        let output = ctx.run_ui(input, |ui| {
            o.paint(ui, &settings.caption, None, Some(band));
        });
        assert!(o.bubbles().is_empty());
        assert!(!output.shapes.is_empty());
        output.drop_without_applying_deltas();
    }

    #[test]
    fn an_error_leaves_the_overlay_after_a_while() {
        let error = || Status {
            level: StatusLevel::Error,
            text: "Capture failed".into(),
            ttl: None,
        };
        let mut overlay = OverlayState::default();
        overlay.set_status(error(), Instant::now());
        assert!(overlay.has_status());
        overlay.set_status(error(), Instant::now() - ERROR_TTL);
        assert!(!overlay.has_status());
    }
}

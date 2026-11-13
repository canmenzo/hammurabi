//! The band as the text recognition reads it best: a subtitle's letters as black ink on white
//! and nothing else. A subtitle is white, thin and stands still, so a pixel is ink when it is
//! white, has been for as many pictures as the subtitle was found in, and lies in a stroke;
//! sky, walls and headlights are too wide, and what moves is too young.

use super::reader::{Block, C_TOL, EXT_TOL, H_MAX, H_MIN, Line, key};

/// A pixel is white now when its darkest channel is at least this,
const LO: u8 = 235;
/// and has lasted while it stayed at least this.
const ALO: u8 = 215;
/// Ink runs one pixel out from the white ones into those at least this bright: none at `GLO`,
/// full at `HI`, which keeps the letters' soft edges.
const GLO: u8 = 170;
const HI: u8 = 235;
/// White that is wider and taller than this part of the picture's width is not a stroke.
const STROKE: f32 = 1.0 / 320.0;
/// The most pictures a pixel has to have lasted (0.8 s at ten a second).
const K_MAX: u8 = 8;
/// The subtitle changed when less than this share of its pixels (of at least `COLLAPSE_MIN`)
/// lasted into a picture.
/// Bright pixels in thin strokes a subtitle has to have for its whiteness to be told.
const WHITE_OF: usize = 100;
const COLLAPSE: f32 = 0.5;
const COLLAPSE_MIN: usize = 40;
/// Pictures without a subtitle before where it stood is forgotten.
const MISS_MAX: u32 = 2;
/// A coloured word (the place or person an objective names): its brightest channel at least
/// `COL_LO` and that much above its darkest.
const COL_LO: u8 = 160;
const COL_SAT: u8 = 45;
/// Colours are let in between these parts of the width, on the rows of a line that looks cut
/// short by one: off centre, but within `COL_EDGE` of the middle, read the same twice.
const ZONE_X: (f32, f32) = (0.15, 0.85);
const COL_EDGE: f32 = 0.15;
/// Pictures they stay let in without coloured letters being found there:
const ZONE_TTL: u8 = 3;
/// at least this many, and this share of the row's.
const COL_KEEP_MIN: usize = 30;
const COL_KEEP_FRAC: f32 = 0.1;
/// Ink of a coloured pixel by its brightest channel: none and full.
const COL_INK: (u8, u8) = (130, 200);

pub struct Prep {
    w: usize,
    h: usize,
    stroke: usize,
    /// Pictures in a row each pixel has been nearly white, bright enough for ink, and nearly
    /// white or coloured.
    age_w: Vec<u8>,
    age_g: Vec<u8>,
    age_x: Vec<u8>,
    /// The last picture's letter pixels, and each pixel's darkest channel.
    letters: Vec<bool>,
    dark: Vec<u8>,
    /// A subtitle was found lately, in this many pictures in a row,
    alive: bool,
    seen: u8,
    miss: u32,
    /// from here to here, in this box (x0, y0, x1, y1).
    ext: Option<(f32, f32)>,
    around: Option<[usize; 4]>,
    /// Rows where colours are let in, and for how many more pictures.
    zone: Option<(usize, usize, u8)>,
    /// The last picture's line that looked cut short by a coloured word.
    cut: Option<(Vec<char>, f32, f32)>,
    /// The subtitle's pixels went away in the picture prepared last: what stands there now is
    /// another one, however alike it reads.
    pub changed: bool,
    grey: Vec<u8>,
}

impl Prep {
    pub fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            w,
            h,
            stroke: ((STROKE * w as f32).round() as usize).max(3),
            age_w: vec![0; n],
            age_g: vec![0; n],
            age_x: vec![0; n],
            letters: vec![false; n],
            dark: vec![0; n],
            alive: false,
            seen: 0,
            miss: 0,
            ext: None,
            around: None,
            zone: None,
            cut: None,
            changed: false,
            grey: vec![255; n],
        }
    }

    /// `bgra`: the band, four bytes a pixel. Gives it as one byte a pixel, black text on white.
    pub fn prepare(&mut self, bgra: &[u8]) -> &[u8] {
        let (w, n) = (self.w, self.w * self.h);
        let (mut dark, mut bright) = (vec![0u8; n], vec![0u8; n]);
        for (i, p) in bgra.as_chunks::<4>().0.iter().take(n).enumerate() {
            let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            dark[i] = lo;
            bright[i] = hi;
            let run = |age: &mut u8, on: bool| *age = if on { age.saturating_add(1) } else { 0 };
            run(&mut self.age_w[i], lo >= ALO);
            run(&mut self.age_g[i], lo >= GLO);
            run(
                &mut self.age_x[i],
                lo >= ALO || (hi >= COL_LO && hi - lo >= COL_SAT),
            );
        }
        let mut age = self.age_w.clone();
        let mut white: Vec<bool> = dark.iter().map(|&v| v >= LO).collect();
        let zone = self.zone.map(|(y0, y1, _)| {
            let (xa, xb) = (
                (ZONE_X.0 * w as f32) as usize,
                (ZONE_X.1 * w as f32) as usize,
            );
            (y0..y1).flat_map(move |y| y * w + xa..y * w + xb)
        });
        for i in zone.clone().into_iter().flatten() {
            age[i] = self.age_x[i];
            white[i] |= self.age_x[i] > 0;
        }
        let mut need = if self.alive {
            K_MAX.min(self.seen.saturating_add(1))
        } else {
            1
        };
        self.changed = false;
        if need > 1
            && let Some([x0, y0, x1, y1]) = self.around
        {
            let (mut was, mut still) = (0, 0);
            for i in (y0..y1).flat_map(|y| y * w + x0..y * w + x1) {
                was += usize::from(self.letters[i]);
                still += usize::from(self.letters[i] && age[i] >= need);
            }
            if was >= COLLAPSE_MIN && (still as f32) < COLLAPSE * was as f32 {
                need = 1;
                self.seen = 0;
                self.changed = true;
            }
        }
        let mut seed: Vec<bool> = (0..n).map(|i| age[i] >= need && white[i]).collect();
        strokes(&mut seed, w, self.h, self.stroke);
        let near = grown(&seed, w, self.h);
        let ramp = |v: u8, lo: u8, hi: u8| {
            ((i32::from(v) - i32::from(lo)) * 255 / i32::from(hi - lo)).clamp(0, 255) as u8
        };
        for i in 0..n {
            let ink = if near[i] && self.age_g[i] >= need {
                ramp(dark[i], GLO, HI)
            } else {
                0
            };
            self.grey[i] = 255 - ink;
        }
        for i in zone.into_iter().flatten() {
            if near[i] && self.age_x[i] >= need && dark[i] < ALO {
                self.grey[i] = self.grey[i].min(255 - ramp(bright[i], COL_INK.0, COL_INK.1));
            }
        }
        self.letters = seed;
        self.dark = dark;
        &self.grey
    }

    /// How white the letters of a subtitle read in the last picture are: of the pixels in
    /// its rows that lie in thin strokes at least `GLO` bright, the share that is white.
    /// Most of a white subtitle's; a fifth of one in thin, off-white or coloured letters,
    /// which this picture is not made for. None where there is too little to tell.
    pub fn whiteness(&self, block: &Block) -> Option<f32> {
        let (mut white, mut bright) = (0, 0);
        for r in &block.rows {
            let (x0, y0) = (r.x0.max(0.0) as usize, r.y0.max(0.0) as usize);
            let x1 = (r.x1.max(0.0) as usize + 1).min(self.w);
            let y1 = (r.y1.max(0.0) as usize + 1).min(self.h);
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let (w, h) = (x1 - x0, y1 - y0);
            for (level, count) in [(LO, &mut white), (GLO, &mut bright)] {
                let mut thin: Vec<bool> = (0..w * h)
                    .map(|i| self.dark[(y0 + i / w) * self.w + x0 + i % w] >= level)
                    .collect();
                strokes(&mut thin, w, h, self.stroke);
                *count += thin.iter().filter(|&&t| t).count();
            }
        }
        (bright >= WHITE_OF).then(|| white as f32 / bright as f32)
    }

    /// What was read in the picture prepared last: the subtitle found in it and all its lines.
    pub fn feedback(&mut self, block: Option<&Block>, lines: &[Line]) {
        let width = self.w as f32;
        match block {
            None => {
                self.miss += 1;
                if self.miss > MISS_MAX {
                    (self.alive, self.seen, self.ext, self.around) = (false, 0, None, None);
                }
            }
            Some(b) => {
                self.miss = 0;
                let y0 = b.rows.iter().map(|r| r.y0).fold(f32::MAX, f32::min);
                let y1 = b.rows.iter().map(|r| r.y1).fold(0.0, f32::max);
                let same = self.alive
                    && self.ext.is_some_and(|(x0, x1)| {
                        (b.x0 - x0).abs() <= EXT_TOL * b.h && (b.x1 - x1).abs() <= EXT_TOL * b.h
                    });
                self.seen = if same { self.seen.saturating_add(1) } else { 1 };
                self.alive = true;
                self.ext = Some((b.x0, b.x1));
                self.around = Some([
                    (b.x0 - b.h).max(0.0) as usize,
                    (y0 - 0.3 * b.h).max(0.0) as usize,
                    ((b.x1 + b.h).max(0.0) as usize).min(self.w),
                    ((y1 + 0.3 * b.h).max(0.0) as usize).min(self.h),
                ]);
            }
        }
        if let Some((y0, y1, mut left)) = self.zone.take() {
            left -= 1;
            // A row of the subtitle there that does hold coloured letters keeps them let in.
            for r in block.iter().flat_map(|b| &b.rows) {
                if r.y0 >= y1 as f32 || r.y1 <= y0 as f32 {
                    continue;
                }
                let (mut all, mut coloured) = (0, 0);
                for y in r.y0.max(0.0) as usize..(r.y1.max(0.0) as usize + 1).min(self.h) {
                    for x in r.x0.max(0.0) as usize..(r.x1.max(0.0) as usize + 1).min(self.w) {
                        let i = y * self.w + x;
                        all += usize::from(self.letters[i]);
                        coloured += usize::from(self.letters[i] && self.age_w[i] == 0);
                    }
                }
                if coloured >= COL_KEEP_MIN && coloured as f32 >= COL_KEEP_FRAC * all as f32 {
                    left = ZONE_TTL;
                }
            }
            if left > 0 {
                self.zone = Some((y0, y1, left));
            }
        }
        let mut cut = None;
        for l in lines {
            let (h, mid) = (l.y1 - l.y0, width / 2.0);
            if !(H_MIN * width..=H_MAX * width).contains(&h)
                || key(&l.text).is_empty()
                || ((l.x0 + l.x1) / 2.0 - mid).abs() <= C_TOL * width
            {
                continue;
            }
            let edge = if l.x0 <= mid && mid <= l.x1 {
                0.0
            } else {
                (l.x0 - mid).abs().min((l.x1 - mid).abs())
            };
            if edge <= COL_EDGE * width {
                cut = Some((l, key(&l.text)));
            }
        }
        if let (Some((l, k)), Some((was, x0, x1))) = (&cut, &self.cut)
            && k == was
            && (l.x0 - x0).abs() <= 4.0
            && (l.x1 - x1).abs() <= 4.0
        {
            let h = l.y1 - l.y0;
            self.zone = Some((
                (l.y0 - 0.25 * h).max(0.0) as usize,
                ((l.y1 + 0.25 * h) as usize).min(self.h),
                ZONE_TTL,
            ));
        }
        self.cut = cut.map(|(l, k)| (k, l.x0, l.x1));
    }
}

/// Takes out what lies in a run longer than `max` both across and down: a letter's stroke is
/// thin one way at least.
fn strokes(seed: &mut [bool], w: usize, h: usize, max: usize) {
    let mut wide = vec![false; seed.len()];
    for row in seed.chunks_exact(w).enumerate() {
        let (y, row) = row;
        let mut x = 0;
        while x < w {
            let start = x;
            while x < w && row[x] {
                x += 1;
            }
            if x - start > max {
                wide[y * w + start..y * w + x].fill(true);
            }
            x += 1;
        }
    }
    for x in 0..w {
        let mut y = 0;
        while y < h {
            let start = y;
            while y < h && seed[y * w + x] {
                y += 1;
            }
            if y - start > max {
                for i in (start..y).map(|y| y * w + x) {
                    seed[i] &= !wide[i];
                }
            }
            y += 1;
        }
    }
}

/// Each pixel with the eight around it.
fn grown(m: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut across = m.to_vec();
    for i in (0..m.len()).filter(|&i| m[i]) {
        if i % w > 0 {
            across[i - 1] = true;
        }
        if i % w + 1 < w {
            across[i + 1] = true;
        }
    }
    let mut out = across.clone();
    for i in (0..m.len()).filter(|&i| across[i]) {
        if i >= w {
            out[i - w] = true;
        }
        if i / w + 1 < h {
            out[i + w] = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey picture with white drawn into it by `white(x, y)`.
    fn picture(w: usize, h: usize, white: impl Fn(usize, usize) -> bool) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let v = if white(i % w, i / w) { 255 } else { 90 };
                [v, v, v, 255]
            })
            .collect()
    }

    #[test]
    fn strokes_are_ink_and_a_white_wall_is_not() {
        let (w, h) = (640, 40);
        // A stroke three pixels thick, and a block of white twenty by twenty.
        let bgra = picture(w, h, |x, y| {
            ((100..103).contains(&x) && (5..35).contains(&y))
                || ((300..320).contains(&x) && (10..30).contains(&y))
        });
        let mut prep = Prep::new(w, h);
        let grey = prep.prepare(&bgra);
        assert_eq!(grey[20 * w + 101], 0);
        assert_eq!(grey[20 * w + 310], 255);
        assert_eq!(grey[20 * w + 50], 255);
    }

    #[test]
    fn what_has_not_lasted_is_not_ink_while_a_subtitle_stands() {
        let (w, h) = (640, 40);
        let sub = |x: usize, y: usize| (300..340).contains(&x) && (20..22).contains(&y);
        let block = Block {
            text: "sub".into(),
            rows: vec![Line {
                text: "sub".into(),
                x0: 300.0,
                y0: 16.0,
                x1: 340.0,
                y1: 26.0,
                words: vec![("sub".into(), 300.0, 340.0)],
            }],
            x0: 300.0,
            x1: 340.0,
            h: 10.0,
        };
        let mut prep = Prep::new(w, h);
        for _ in 0..3 {
            prep.prepare(&picture(w, h, sub));
            prep.feedback(Some(&block), &[]);
        }
        // A glint next to it, there in this picture only.
        let glint = |x: usize, y: usize| sub(x, y) || ((342..344).contains(&x) && y == 21);
        let grey = prep.prepare(&picture(w, h, glint));
        assert_eq!(grey[21 * w + 320], 0);
        assert_eq!(grey[21 * w + 343], 255);
        assert!(!prep.changed);
        // The subtitle gives way to another.
        let next = |x: usize, y: usize| (200..260).contains(&x) && (20..22).contains(&y);
        let ink = prep.prepare(&picture(w, h, next))[21 * w + 230];
        assert!(prep.changed);
        assert_eq!(ink, 0);
    }
}

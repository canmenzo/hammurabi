//! A picture at another size, by Lanczos resampling. A subtitle in a small window has strokes
//! a pixel or two wide whose white the smoother ways of enlarging dull below what counts as
//! white; this one keeps it.

/// How many pixels to each side one is made of, before widening for a smaller picture.
const LOBES: f32 = 3.0;

/// Along one axis, for each pixel of the new size: the first old pixel it is made of, and the
/// weights of that one and the `n - 1` after it.
struct Taps {
    first: Vec<isize>,
    weights: Vec<f32>,
    n: usize,
}

impl Taps {
    fn new(from: usize, to: usize) -> Self {
        let step = from as f32 / to as f32;
        let wide = step.max(1.0);
        let n = (2.0 * LOBES * wide).ceil() as usize + 1;
        let (mut first, mut weights) = (Vec::with_capacity(to), Vec::with_capacity(to * n));
        for o in 0..to {
            let centre = (o as f32 + 0.5) * step - 0.5;
            let start = (centre - LOBES * wide).ceil();
            let at = weights.len();
            weights.extend((0..n).map(|k| lanczos((start + k as f32 - centre) / wide)));
            let sum: f32 = weights[at..].iter().sum();
            weights[at..].iter_mut().for_each(|w| *w /= sum);
            first.push(start as isize);
        }
        Self { first, weights, n }
    }

    /// The old pixels (kept within `len`) and weights the new pixel `o` is made of.
    fn of(&self, o: usize, len: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        let first = self.first[o];
        self.weights[o * self.n..(o + 1) * self.n]
            .iter()
            .enumerate()
            .map(move |(k, w)| ((first + k as isize).clamp(0, len as isize - 1) as usize, *w))
    }
}

fn lanczos(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        return 1.0;
    }
    if x.abs() >= LOBES {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    LOBES * px.sin() * (px / LOBES).sin() / (px * px)
}

pub struct Resize {
    from: (usize, usize),
    to: (usize, usize),
    across: Taps,
    down: Taps,
    /// The picture at its new width and old height, three numbers a pixel.
    wide: Vec<f32>,
    out: Vec<u8>,
}

impl Resize {
    /// From and to (width, height).
    pub fn new(from: (usize, usize), to: (usize, usize)) -> Self {
        Self {
            from,
            to,
            across: Taps::new(from.0, to.0),
            down: Taps::new(from.1, to.1),
            wide: vec![0.0; to.0 * from.1 * 3],
            out: vec![255; to.0 * to.1 * 4],
        }
    }

    /// `bgra`: four bytes a pixel, as is what it gives.
    pub fn run(&mut self, bgra: &[u8]) -> &[u8] {
        let ((fw, fh), (tw, th)) = (self.from, self.to);
        for y in 0..fh {
            let row = &bgra[y * fw * 4..(y + 1) * fw * 4];
            for x in 0..tw {
                let mut sum = [0f32; 3];
                for (i, w) in self.across.of(x, fw) {
                    for (s, v) in sum.iter_mut().zip(&row[i * 4..]) {
                        *s += w * f32::from(*v);
                    }
                }
                self.wide[(y * tw + x) * 3..][..3].copy_from_slice(&sum);
            }
        }
        for y in 0..th {
            let out = &mut self.out[y * tw * 4..(y + 1) * tw * 4];
            let mut sums = vec![0f32; tw * 3];
            for (i, w) in self.down.of(y, fh) {
                let row = &self.wide[i * tw * 3..(i + 1) * tw * 3];
                for (s, v) in sums.iter_mut().zip(row) {
                    *s += w * v;
                }
            }
            for (px, s) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(sums.as_chunks::<3>().0)
            {
                for (p, s) in px.iter_mut().zip(s) {
                    *p = s.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        &self.out
    }
}

/// A picture `k` times smaller each way, a pixel the mean of the `k` by `k` it stands for:
/// blue, green, red in, red, green, blue out. Gives its width and height with it.
#[cfg(any(windows, test))]
pub fn shrink(bgra: &[u8], width: usize, height: usize, k: usize) -> (usize, usize, Vec<u8>) {
    let (w, h) = (width / k, height / k);
    let n = (k * k) as u32;
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for row in 0..k {
                let at = ((y * k + row) * width + x * k) * 4;
                for px in bgra[at..at + k * 4].as_chunks::<4>().0 {
                    for (s, v) in sum.iter_mut().zip(px) {
                        *s += u32::from(*v);
                    }
                }
            }
            out.extend([sum[2] / n, sum[1] / n, sum[0] / n, 255].map(|v| v as u8));
        }
    }
    (w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smaller_picture_is_the_mean_of_its_pixels() {
        // Four by two: a blue half that is half lit, and a red half.
        let px = |x: usize, y: usize| match (x < 2, y) {
            (true, 0) => [200, 0, 0, 255],
            (true, _) => [100, 0, 0, 255],
            _ => [0, 0, 60, 255],
        };
        let bgra: Vec<u8> = (0..8).flat_map(|i| px(i % 4, i / 4)).collect();
        assert_eq!(
            shrink(&bgra, 4, 2, 2),
            (2, 1, vec![0, 0, 150, 255, 60, 0, 0, 255])
        );
    }

    #[test]
    fn a_thin_white_stroke_stays_white_when_enlarged() {
        // A stroke two pixels wide on dark grey, half again as large.
        let (w, h) = (40, 20);
        let bgra: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = if (20..22).contains(&(i % w)) { 255 } else { 60 };
                [v, v, v, 255]
            })
            .collect();
        let mut resize = Resize::new((w, h), (60, 30));
        let out = resize.run(&bgra);
        assert_eq!(out.len(), 60 * 30 * 4);
        let row = &out[15 * 60 * 4..16 * 60 * 4];
        assert!(row[31 * 4] >= 235, "{}", row[31 * 4]);
        assert!(row[10 * 4].abs_diff(60) <= 2);
    }
}

//! From pictures of the band to subtitles: the picture is prepared (`prepare`), read by the
//! text recognition, the lines that are the subtitle are picked out of what it read, and the
//! readings of one subtitle over time are made into one text, given on once.

use super::prepare::Prep;
use super::resize::Resize;

/// A line of text as it stands in the band, in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Its words, each with its left and right edge.
    pub words: Vec<(String, f32, f32)>,
}

/// The subtitle in one picture: one row, or two or three stacked.
#[derive(Clone, Debug)]
pub struct Block {
    pub text: String,
    pub rows: Vec<Line>,
    pub x0: f32,
    pub x1: f32,
    /// The height of its tallest row.
    pub h: f32,
}

// Lengths that are parts of the picture's width.
/// How tall a subtitle's row is: between these.
pub(super) const H_MIN: f32 = 0.006;
pub(super) const H_MAX: f32 = 0.030;
/// A subtitle's middle is this near the picture's: credits, a street name, a radar and help
/// text stand to a side.
pub(super) const C_TOL: f32 = 0.020;
// Lengths in row heights.
/// Pieces the recognition split one row into are joined across a gap up to this,
const JOIN_GAP: f32 = 2.5;
/// and rows stack into one subtitle across one up to this.
const ROW_GAP: f32 = 1.0;
/// A reading is of the subtitle that stands when both its ends are this near that one's
pub(super) const EXT_TOL: f32 = 1.5;
/// and it is this alike, letter for letter (0 to 1).
const SAME_SIM: f32 = 0.5;
/// Two readings have the same word when it starts this near in both.
const WORD_TOL: f32 = 0.4;

/// Readings a word has to be the same in before it is taken as read right.
const N_AGREE: usize = 2;
/// Seconds without a reading that end a subtitle.
const GAP_S: f64 = 0.45;
/// A row given on this lately is not given on again: when it reads this alike,
const REPEAT_S: f64 = 2.5;
const REPEAT_SIM: f32 = 0.9;
/// or is the beginning or the end of that row, at least this much of it, with at most this
/// share of its letters wrong.
const PIECE_MIN: f32 = 0.5;
const PIECE_ERR: f32 = 0.15;
/// The readings words are mended from: the last so many.
const MEND_READS: usize = 12;
/// What is put in a misread word's place has to cover this much of it.
const MEND_COVER: f32 = 0.7;
/// A word the best reading lacks is put back when this share of the readings has it.
const LIVE_FRAC: f32 = 0.34;
/// A subtitle whose words do not all agree is given on anyway after this many seconds and
/// readings, when at least this share of them does.
const LATE_S: f64 = 0.6;
const LATE_N: usize = 4;
const LATE_FRAC: f32 = 0.5;
/// Readings kept of one subtitle.
const READS_MAX: usize = 64;

/// The width the band is read at. What counts as white and how the text recognition reads
/// were set on pictures this wide; a smaller window's band is enlarged to it first, which
/// takes the words read wrong at 1280 wide from a third to a ninth.
const WIDTH: usize = 1920;
/// A band within this share of that width is read as it is.
const WIDTH_TOL: f32 = 0.06;

pub struct Reader {
    /// The size the band is read at.
    width: usize,
    height: usize,
    resize: Option<Resize>,
    prep: Prep,
    tracker: Tracker,
}

impl Reader {
    /// For a band this many pixels wide and high, read ten times a second.
    pub fn new(width: usize, height: usize) -> Self {
        let scale = WIDTH as f32 / width as f32;
        let to = if (scale - 1.0).abs() <= WIDTH_TOL {
            (width, height)
        } else {
            (WIDTH, ((height as f32 * scale).round() as usize).max(1))
        };
        Self {
            width: to.0,
            height: to.1,
            resize: (to != (width, height)).then(|| Resize::new((width, height), to)),
            prep: Prep::new(to.0, to.1),
            tracker: Tracker::default(),
        }
    }

    /// The width and height of the picture `prepare` gives.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The band (four bytes a pixel: blue, green, red, alpha) as the picture the text
    /// recognition is to read: one byte a pixel.
    pub fn prepare(&mut self, bgra: &[u8]) -> &[u8] {
        match &mut self.resize {
            Some(resize) => self.prep.prepare(resize.run(bgra)),
            None => self.prep.prepare(bgra),
        }
    }

    /// What was read in that picture, `t` seconds in. Gives a subtitle once it is sure of it,
    /// with the time it was first seen.
    pub fn read(&mut self, t: f64, lines: &[Line]) -> Option<(f64, String)> {
        let block = pick(lines, self.width as f32);
        self.prep.feedback(block.as_ref(), lines);
        self.tracker.feed(t, block, self.prep.changed)
    }
}

/// Letters and digits only, lower case: what readings are compared on.
pub(super) fn key(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Edits that make one the other.
fn lev(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = diagonal + usize::from(ca != cb);
            diagonal = row[j + 1];
            row[j + 1] = cost.min(row[j] + 1).min(diagonal + 1);
        }
    }
    row[b.len()]
}

/// How alike two keys are: 1 the same, 0 nothing in common or one of them empty.
fn sim(a: &[char], b: &[char]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    1.0 - lev(a, b) as f32 / a.len().max(b.len()) as f32
}

/// Which of the lines read in a band `w` wide are the subtitle: the lowest centred row and
/// the centred rows right above it.
fn pick(lines: &[Line], w: f32) -> Option<Block> {
    let mut lines: Vec<&Line> = lines
        .iter()
        .filter(|l| (H_MIN * w..=H_MAX * w).contains(&(l.y1 - l.y0)) && !key(&l.text).is_empty())
        .collect();
    lines.sort_by(|a, b| (a.y0 + a.y1).total_cmp(&(b.y0 + b.y1)));
    let mut rows: Vec<Line> = Vec::new();
    for l in lines {
        if let Some(r) = rows.last_mut() {
            let (hr, hl) = (r.y1 - r.y0, l.y1 - l.y0);
            let gap = JOIN_GAP * hr.max(hl);
            if ((r.y0 + r.y1) / 2.0 - (l.y0 + l.y1) / 2.0).abs() < 0.5 * hr.min(hl)
                && l.x0 - r.x1 < gap
                && r.x0 - l.x1 < gap
            {
                let (a, b) = if r.x0 <= l.x0 { (&*r, l) } else { (l, &*r) };
                *r = Line {
                    text: format!("{} {}", a.text, b.text),
                    x0: a.x0,
                    y0: a.y0.min(b.y0),
                    x1: a.x1.max(b.x1),
                    y1: a.y1.max(b.y1),
                    words: [a.words.as_slice(), b.words.as_slice()].concat(),
                };
                continue;
            }
        }
        rows.push(l.clone());
    }
    rows.retain(|r| ((r.x0 + r.x1) / 2.0 - w / 2.0).abs() <= C_TOL * w);
    let mut block = vec![rows.pop()?];
    while let Some(r) = rows.pop() {
        let top = &block[0];
        if top.y0 - r.y1 >= ROW_GAP * (top.y1 - top.y0) {
            break;
        }
        block.insert(0, r);
    }
    Some(Block {
        text: block
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        x0: block.iter().map(|r| r.x0).fold(f32::MAX, f32::min),
        x1: block.iter().map(|r| r.x1).fold(0.0, f32::max),
        h: block.iter().map(|r| r.y1 - r.y0).fold(0.0, f32::max),
        rows: block,
    })
}

/// The subtitle that stands: its readings since it appeared.
struct Track {
    t0: f64,
    last: f64,
    reads: Vec<Block>,
    /// The reading seen most often, which the next ones are held against.
    best: usize,
    emitted: bool,
}

/// A row given on lately, where it stood, and whether its pixels have gone since.
struct Done {
    text: String,
    t: f64,
    x0: f32,
    x1: f32,
    gone: bool,
}

/// One subtitle at a time. It is given on once, when its words agree; the rows given on lately
/// are kept, so that one that stays while another joins it, or comes back after the picture
/// lost it, is not given on twice.
#[derive(Default)]
struct Tracker {
    cur: Option<Track>,
    done: Vec<Done>,
}

impl Tracker {
    /// `block`: the subtitle picked in the picture at `t` seconds. `changed`: the pixels of
    /// the one before went away in it.
    fn feed(&mut self, t: f64, block: Option<Block>, changed: bool) -> Option<(f64, String)> {
        if changed {
            self.cur = None;
            self.done.iter_mut().for_each(|d| d.gone = true);
        }
        if self.cur.as_ref().is_some_and(|c| t - c.last > GAP_S) {
            self.cur = None;
        }
        let block = block?;
        let cur = match &mut self.cur {
            Some(c) if c.same(&block) => {
                if c.emitted || c.reads.len() == READS_MAX {
                    // Only the best and the last are looked at from here on.
                    let old = if c.emitted {
                        c.reads.len() - 1
                    } else {
                        usize::from(c.best == 0)
                    };
                    if old != c.best {
                        c.reads.remove(old);
                        c.best -= usize::from(old < c.best);
                    }
                }
                c.reads.push(block);
                c.last = t;
                c
            }
            cur => cur.insert(Track {
                t0: t,
                last: t,
                reads: vec![block],
                best: 0,
                emitted: false,
            }),
        };
        if cur.emitted {
            let block = cur.reads.last()?;
            for r in &block.rows {
                known(&mut self.done, &r.text, (r.x0, r.x1), block.h, t, t);
            }
            return None;
        }
        let rows = cur.choose(t)?;
        cur.emitted = true;
        self.done.retain(|d| t - d.t <= REPEAT_S);
        let block = cur.reads.last()?;
        let mut fresh = Vec::new();
        for (i, text) in rows.into_iter().enumerate() {
            let r = &block.rows[i.min(block.rows.len() - 1)];
            if !known(&mut self.done, &text, (r.x0, r.x1), block.h, cur.t0, t) {
                self.done.push(Done {
                    text: text.clone(),
                    t,
                    x0: r.x0,
                    x1: r.x1,
                    gone: false,
                });
                fresh.push(text);
            }
        }
        (!fresh.is_empty()).then(|| (cur.t0, apostrophes(&commas(&fresh.join(" ")))))
    }
}

impl Track {
    /// Is this a reading of the subtitle that stands: as many rows, the same ends, alike?
    fn same(&self, b: &Block) -> bool {
        let (best, tol) = (&self.reads[self.best], EXT_TOL * b.h);
        let alike = |o: &Block| sim(&key(&b.text), &key(&o.text));
        b.rows.len() == best.rows.len()
            && (b.x0 - best.x0).abs() <= tol
            && (b.x1 - best.x1).abs() <= tol
            && self
                .reads
                .last()
                .is_some_and(|last| alike(best).max(alike(last)) >= SAME_SIM)
    }

    /// The rows to give on now, or None to wait for more readings.
    fn choose(&mut self, t: f64) -> Option<Vec<String>> {
        let count = |b: &Block| self.reads.iter().filter(|o| o.text == b.text).count();
        let mut best = 0;
        for i in 1..self.reads.len() {
            if count(&self.reads[i]) > count(&self.reads[best]) {
                best = i;
            }
        }
        self.best = best;
        let n = self.reads.len();
        let (rows, sure, growing) = mend(&self.reads[n.saturating_sub(MEND_READS)..])?;
        let late = t - self.t0 >= LATE_S && n >= LATE_N && sure >= LATE_FRAC;
        ((sure >= 1.0 && !growing) || late).then_some(rows)
    }
}

/// A full stop after a letter and before a small one is a comma the recognition took for
/// one, which it does to every third: "Man. get your ass in there."
fn commas(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let comma = |i: usize| {
        c[i] == '.'
            && i > 0
            && c[i - 1].is_alphabetic()
            && c.get(i + 1) == Some(&' ')
            && c.get(i + 2).is_some_and(|n| n.is_lowercase())
    };
    (0..c.len())
        .map(|i| if comma(i) { ',' } else { c[i] })
        .collect()
}

/// An apostrophe the recognition took for a letter, in the words it does that to: "goinl",
/// "11m", "Ilma".
fn apostrophes(text: &str) -> String {
    let word = |word: &str| {
        let end = word.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
        let (w, rest) = word.split_at(end);
        match w {
            "11m" | "1m" | "Ilm" => format!("I'm{rest}"),
            "11ma" | "Ilma" => format!("I'ma{rest}"),
            _ if w.len() > 4 && w.ends_with("inl") => format!("{}'{rest}", &w[..w.len() - 1]),
            _ => word.to_string(),
        }
    };
    text.split(' ').map(word).collect::<Vec<_>>().join(" ")
}

/// Was this row given on lately: the same text, its beginning or its end, or roughly the same
/// text in the same place? Then it counts as seen again at `t`.
fn known(done: &mut [Done], text: &str, (x0, x1): (f32, f32), h: f32, t0: f64, t: f64) -> bool {
    let a = key(text);
    for d in done.iter_mut().filter(|d| t0 - d.t <= REPEAT_S) {
        let b = key(&d.text);
        let s = sim(&a, &b);
        let piece = b.len() > a.len()
            && a.len() as f32 >= PIECE_MIN * b.len() as f32
            && lev(&a, &b[..a.len()]).min(lev(&a, &b[b.len() - a.len()..])) as f32
                <= PIECE_ERR * a.len() as f32;
        let there = (x0 - d.x0).abs() <= EXT_TOL * h && (x1 - d.x1).abs() <= EXT_TOL * h;
        if s >= REPEAT_SIM || piece || (!d.gone && s >= SAME_SIM && there) {
            d.t = t;
            return true;
        }
    }
    false
}

/// A word in one reading: its row's middle, its edges, and in how many readings a word with
/// the same text starts where it does.
struct Word<'a> {
    y: f32,
    x0: f32,
    x1: f32,
    text: &'a str,
    n: usize,
}

/// A word to stand in the text: its edges, its text and how many readings have it.
type Piece<'a> = (f32, f32, &'a str, usize);

/// One text out of the readings of a subtitle, each of which may have some word wrong. A word
/// is read right when `N_AGREE` readings have it in the same place. The reading with the
/// largest share of such words is taken; each other word of it gives way to the right words
/// other readings have in its place, and right words it lacks are put in. Gives the rows, the
/// share of the words that are read right, and whether the newest reading has a word where no
/// word has been read twice: over a bright background a subtitle's first readings lack words,
/// which come as what moves behind them is told from what stands.
fn mend(reads: &[Block]) -> Option<(Vec<String>, f32, bool)> {
    let h = reads.last()?.h;
    let tol = WORD_TOL * h;
    let mut words: Vec<Word> = Vec::new();
    for r in reads.iter().flat_map(|b| &b.rows) {
        let y = (r.y0 + r.y1) / 2.0;
        words.extend(r.words.iter().map(|(text, x0, x1)| Word {
            y,
            x0: *x0,
            x1: *x1,
            text,
            n: 0,
        }));
    }
    let same = |a: &Word, b: &Word| {
        a.text == b.text && (a.x0 - b.x0).abs() <= tol && (a.y - b.y).abs() < 0.5 * h
    };
    let counts: Vec<usize> = words
        .iter()
        .map(|w| words.iter().filter(|o| same(o, w)).count())
        .collect();
    for (w, n) in words.iter_mut().zip(counts) {
        w.n = n;
    }
    // The base reading and where its words start.
    let (mut base, mut at) = (None, 0);
    for b in reads {
        let n: usize = b.rows.iter().map(|r| r.words.len()).sum();
        let ws = &words[at..at + n];
        at += n;
        if n == 0 {
            continue;
        }
        let right = ws.iter().filter(|w| w.n >= N_AGREE).count() as f32 / n as f32;
        let score = (right, ws.iter().map(|w| w.n).sum::<usize>());
        if base.is_none_or(|(_, _, best)| score > best) {
            base = Some((b, ws, score));
        }
    }
    let (b, ws, _) = base?;
    let good: Vec<&Word> = words.iter().filter(|w| w.n >= N_AGREE).collect();
    let put_back = (N_AGREE as f32).max(LIVE_FRAC * reads.len() as f32);
    let newest: usize = reads.last()?.rows.iter().map(|r| r.words.len()).sum();
    let growing = words[words.len() - newest..].iter().any(|w| {
        let over =
            |g: &&Word| (g.y - w.y).abs() < 0.5 * h && g.x0 < w.x1 - tol && w.x0 < g.x1 - tol;
        w.n < N_AGREE && !good.iter().any(over)
    });
    fn piece<'a>(w: &Word<'a>) -> Piece<'a> {
        (w.x0, w.x1, w.text, w.n)
    }
    let order = |a: &Piece, b: &Piece| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1));
    let (mut out, mut k, mut open) = (Vec::new(), 0, 0);
    for r in &b.rows {
        let y = (r.y0 + r.y1) / 2.0;
        let mut line = Vec::new();
        for w in &ws[k..k + r.words.len()] {
            if w.n >= N_AGREE {
                line.push(piece(w));
                continue;
            }
            // The right words of other readings within this word's span.
            let mut sub: Vec<Piece> = Vec::new();
            for g in good.iter().filter(|g| (g.y - y).abs() < 0.5 * h) {
                let mid = (g.x0 + g.x1) / 2.0;
                if (w.x0 - tol..=w.x1 + tol).contains(&mid)
                    && g.x0 >= w.x0 - 2.0 * tol
                    && g.x1 <= w.x1 + 2.0 * tol
                    && !sub
                        .iter()
                        .any(|s| s.2 == g.text && (s.0 - g.x0).abs() <= tol)
                {
                    sub.push(piece(g));
                }
            }
            sub.sort_by(order);
            // Of two that overlap, the one more readings have stays.
            let mut keep: Vec<Piece> = Vec::new();
            for s in sub {
                match keep.last_mut() {
                    Some(last) if s.0 < last.1 - tol => {
                        if s.3 > last.3 {
                            *last = s;
                        }
                    }
                    _ => keep.push(s),
                }
            }
            let cover: f32 = keep.iter().map(|s| s.1.min(w.x1) - s.0.max(w.x0)).sum();
            if !keep.is_empty() && cover >= MEND_COVER * (w.x1 - w.x0) {
                line.extend(keep);
            } else {
                line.push(piece(w));
                open += 1;
            }
        }
        k += r.words.len();
        line.sort_by(order);
        let known = line.len();
        for g in good.iter().filter(|g| (g.y - y).abs() < 0.5 * h) {
            let mid = (g.x0 + g.x1) / 2.0;
            let has = line.iter().any(|l| {
                (l.0 - tol..=l.1 + tol).contains(&mid)
                    || (g.x0 - tol..=g.x1 + tol).contains(&((l.0 + l.1) / 2.0))
            });
            let far =
                known > 0 && (g.x1 < line[0].0 - 3.0 * h || g.x0 > line[known - 1].1 + 3.0 * h);
            if g.n as f32 >= put_back && !has && !far {
                line.push(piece(g));
            }
        }
        line.sort_by(order);
        out.push(line.iter().map(|l| l.2).collect::<Vec<_>>().join(" "));
    }
    Some((out, 1.0 - open as f32 / ws.len() as f32, growing))
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1920.0;

    /// A line as the recognition gives it, 14 pixels a letter, centred unless `x0` is given.
    fn line_at(text: &str, y: f32, x0: Option<f32>) -> Line {
        let w = 14.0 * text.chars().count() as f32;
        let x0 = x0.unwrap_or((W - w) / 2.0);
        let mut x = x0;
        let words = text
            .split(' ')
            .map(|word| {
                let from = x;
                x += 14.0 * (word.chars().count() + 1) as f32;
                (word.to_string(), from, x - 14.0)
            })
            .collect();
        Line {
            text: text.into(),
            x0,
            y0: y,
            x1: x0 + w,
            y1: y + 30.0,
            words,
        }
    }

    fn line(text: &str) -> Line {
        line_at(text, 188.0, None)
    }

    fn picked(lines: &[Line]) -> Option<String> {
        pick(lines, W).map(|b| b.text)
    }

    /// The subtitles given on for pictures of (seconds, lines, changed), with their times in
    /// tenths of a second.
    fn run(pictures: &[(f64, Vec<Line>, bool)]) -> Vec<(i64, i64, String)> {
        let mut tracker = Tracker::default();
        let tenths = |s: f64| (s * 10.0).round() as i64;
        pictures
            .iter()
            .filter_map(|(t, lines, changed)| {
                let (seen, text) = tracker.feed(*t, pick(lines, W), *changed)?;
                Some((tenths(seen), tenths(*t), text))
            })
            .collect()
    }

    const A: &str = "Man, would you come on?";
    const B: &str = "No, homie, I cannot.";

    #[test]
    fn the_centred_line_is_the_subtitle() {
        let credits = line_at("MICHAEL KANE", 60.0, Some(1246.0));
        let street = line_at("Del Perro", 188.0, Some(1700.0));
        assert_eq!(
            picked(&[line("Hello there."), credits, street]),
            Some("Hello there.".into())
        );
        let mut title = line("BIG TITLE");
        title.y1 = title.y0 + 70.0;
        assert_eq!(picked(&[title]), None);
        assert_eq!(
            picked(&[line_at("far above", 20.0, None), line("the subtitle")]),
            Some("the subtitle".into())
        );
    }

    #[test]
    fn rows_and_pieces_of_a_row_are_one_subtitle() {
        assert_eq!(
            picked(&[line_at("first row of two", 148.0, None), line("second row")]),
            Some("first row of two second row".into())
        );
        let halves = [
            line_at("left half", 188.0, Some(820.0)),
            line_at("right half", 188.0, Some(960.0)),
        ];
        assert_eq!(picked(&halves), Some("left half right half".into()));
    }

    #[test]
    fn a_subtitle_is_given_once_at_its_second_reading() {
        let stays = [
            (0.0, vec![line(A)], false),
            (0.2, vec![line(A)], false),
            (0.4, vec![line(A)], false),
        ];
        assert_eq!(run(&stays), [(0, 2, A.to_string())]);
        // The picture loses it for a while: not again.
        let back = [
            (0.0, vec![line(A)], false),
            (0.2, vec![line(A)], false),
            (0.4, vec![], false),
            (0.6, vec![], false),
            (0.8, vec![], false),
            (1.0, vec![line(A)], false),
            (1.2, vec![line(A)], false),
        ];
        assert_eq!(run(&back), [(0, 2, A.to_string())]);
        // Long after, the same words are a subtitle again.
        let again = [
            (0.0, vec![line(A)], false),
            (0.2, vec![line(A)], false),
            (3.5, vec![line(A)], false),
            (3.7, vec![line(A)], false),
        ];
        assert_eq!(
            run(&again),
            [(0, 2, A.to_string()), (35, 37, A.to_string())]
        );
    }

    #[test]
    fn the_next_subtitle_is_given_and_one_that_stays_is_not() {
        let next = [
            (0.0, vec![line(A)], false),
            (0.2, vec![line(A)], false),
            (0.4, vec![line(B)], true),
            (0.6, vec![line(B)], false),
        ];
        assert_eq!(run(&next), [(0, 2, A.to_string()), (4, 6, B.to_string())]);
        let joined = || vec![line_at(A, 148.0, None), line(B)];
        let stays = [
            (0.0, vec![line(A)], false),
            (0.2, vec![line(A)], false),
            (0.4, joined(), false),
            (0.6, joined(), false),
        ];
        assert_eq!(run(&stays), [(0, 2, A.to_string()), (4, 6, B.to_string())]);
    }

    #[test]
    fn alike_in_the_same_place_is_another_when_the_pixels_went() {
        let (c1, c2) = ("Down Grove Street.", "Go to Grove Street.");
        let pictures = [
            (0.0, vec![line(c1)], false),
            (0.2, vec![line(c1)], false),
            (0.4, vec![line(c2)], true),
            (0.6, vec![line(c2)], false),
        ];
        assert_eq!(
            run(&pictures),
            [(0, 2, c1.to_string()), (4, 6, c2.to_string())]
        );
    }

    #[test]
    fn a_subtitle_still_coming_out_is_waited_for() {
        // Its middle stands over something bright and is read two pictures later.
        let (whole, ends) = ("Don't act like you know, homie.", "Don't act like homie.");
        let short = || {
            let mut l = line(whole);
            l.text = ends.into();
            l.words.drain(3..5);
            vec![l]
        };
        let pictures = [
            (0.0, short(), false),
            (0.1, short(), false),
            (0.2, vec![line(whole)], false),
            (0.3, vec![line(whole)], false),
        ];
        assert_eq!(run(&pictures), [(0, 1, ends.to_string())]);
        let pictures = [
            (0.0, short(), false),
            (0.1, vec![line(whole)], false),
            (0.2, vec![line(whole)], false),
        ];
        assert_eq!(run(&pictures), [(0, 2, whole.to_string())]);
    }

    #[test]
    fn a_full_stop_before_a_small_letter_is_a_comma() {
        assert_eq!(commas("Man. get in. Now."), "Man, get in. Now.");
        assert_eq!(commas("I... mean... no. I do."), "I... mean... no. I do.");
    }

    #[test]
    fn an_apostrophe_read_as_a_letter_is_one_again() {
        assert_eq!(
            apostrophes("Keep bustinl, loc! 11m serious. Ilma go, girl."),
            "Keep bustin', loc! I'm serious. I'ma go, girl."
        );
    }

    #[test]
    fn readings_that_each_have_a_word_wrong_are_mended() {
        let pictures = [
            (0.0, vec![line("I say become a doctor,")], false),
            (0.2, vec![line("I say becorne a doctor,")], false),
            (0.4, vec![line("l say become a doctor,")], false),
        ];
        assert_eq!(
            run(&pictures),
            [(0, 4, "I say become a doctor,".to_string())]
        );
    }
}

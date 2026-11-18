//! From pictures of the band to subtitles: the picture is prepared (`prepare`), read by the
//! text recognition, the lines that are the subtitle are picked out of what it read, and the
//! readings of one subtitle over time are made into one text, given on once.

use std::collections::VecDeque;

use super::prepare::Prep;
use super::resize::Resize;
use super::{Area, Spot};

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
/// Letters less white than this (`Prep::whiteness`) are read in the plain picture: white
/// subtitles measure 0.45 to 1, a game's thin off-white ones 0.1 to 0.25.
const WHITE_MIN: f32 = 0.35;
/// Outlined letters are read over ground at least this bright (`Prep::ground`): 76 to 115 in
/// daylight behind Red Dead Redemption's, 34 to 71 in its dark scenes, which the plain picture
/// reads.
pub(super) const GROUND_MIN: u8 = 74;
/// and when this share of their words reads like words (`words_alike`).
const WORDS_ALIKE: f32 = 0.9;

// Captions that grow word by word from a left edge, as a video site's automatic ones do.
/// A row is one of those once it has gained a word this many times: up to `GROW_WORDS` after
/// the words it had, which stand as they were, and not in the middle of the picture, where a
/// subtitle that stands is and reads now with a word more, now with one less. A row that reads
/// with fewer words starts its count again. And it has to have moved up a row since, as one
/// does when the next begins under it: text typed at a left edge grows too, but stays.
const GROW_MIN: u32 = 3;
const GROW_WORDS: usize = 3;
/// and rows are looked for at the left edge it grew from for this long after the last one.
const ROLL_KEEP_S: f64 = 8.0;
/// The lowest row is given on as it is when no word has joined it for this long,
const ROLL_PAUSE_S: f64 = 1.2;
/// and what was not given on of a row that went away, after this long.
const ROLL_GONE_S: f64 = 0.5;
/// A row is kept this long after it was last seen, for what was not given on of it to be given
/// once it went away. A reading is not of a row unseen for longer than `ROLL_GONE_S`.
const ROLL_DONE_S: f64 = 12.0;
/// A reading is of a row when one starts with the other, but for this share of the letters,
const ROLL_ERR: f32 = 0.15;
/// and a reading that lacks the row's end has at least this much of it.
const ROLL_PART: f32 = 0.6;
/// What stands further than this from a row's end, in row heights, is not of it: a word in
/// the picture beside the captions. Unless it stands level with the row, as its own words do:
/// then across this much, the hole a word struck out of the captions leaves.
const ROLL_GAP: f32 = 0.8;
const ROLL_LEVEL_GAP: f32 = 3.5;
/// Level is within this much of a row's height, at its top and its bottom: the pieces of a row
/// that is moving up are read a few pixels apart.
const LEVEL: f32 = 0.3;
/// A row's first word begins this near the left edge the captions grow from, in row heights.
const EDGE_TOL: f32 = 0.3;

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
    /// The band as it is, one byte a pixel.
    plain: Vec<u8>,
    /// How white the letters read lately were (`Prep::whiteness`); None before any were.
    white: Option<f32>,
    /// When a reading other than of outlined letters last had a subtitle.
    other: f64,
    /// and when one of outlined letters last did.
    edged: f64,
    tracker: Tracker,
    roller: Roller,
    /// What the subtitle being read covers.
    spot: Option<Bounds>,
}

/// What rows of text cover, in pixels: left, top, right and bottom, and the height of the
/// tallest.
type Bounds = ([f32; 4], f32);

fn bounds<'a>(rows: impl IntoIterator<Item = &'a Line>) -> Option<Bounds> {
    rows.into_iter().fold(None, |b, r| {
        let ([x0, y0, x1, y1], h) = b.unwrap_or(([f32::MAX, f32::MAX, 0.0, 0.0], 0.0));
        Some((
            [x0.min(r.x0), y0.min(r.y0), x1.max(r.x1), y1.max(r.y1)],
            h.max(r.y1 - r.y0),
        ))
    })
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
            plain: vec![0; to.0 * to.1],
            white: None,
            other: f64::MIN,
            edged: f64::MIN,
            tracker: Tracker::default(),
            roller: Roller::default(),
            spot: None,
        }
    }

    /// The subtitle being read, as fractions of the pictures `prepare` gives. None while there
    /// is none, and while one was read in a single picture: that may be something else.
    pub fn spot(&self) -> Option<Spot> {
        let ([x0, y0, x1, y1], h) = self.spot?;
        let (w, ht) = (self.width as f32, self.height as f32);
        Some(Spot {
            area: Area {
                left: x0 / w,
                top: y0 / ht,
                right: x1 / w,
                bottom: y1 / ht,
            },
            row: h / ht,
        })
    }

    /// The width and height of the pictures `prepare` gives.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The band (four bytes a pixel: blue, green, red, alpha) as the two pictures the text
    /// recognition is to read, one byte a pixel: prepared, with only what is white, thin and
    /// still as ink, and as it is. The first tells white subtitles from a bright scene behind
    /// them. The second reads what the first is not made for: thin, off-white or coloured
    /// letters, which games that use them set on a dark ground.
    pub fn prepare(&mut self, bgra: &[u8]) -> (&[u8], &[u8]) {
        let bgra = match &mut self.resize {
            Some(resize) => resize.run(bgra),
            None => bgra,
        };
        for (grey, p) in self.plain.iter_mut().zip(bgra.as_chunks::<4>().0) {
            let [b, g, r] = [p[0], p[1], p[2]].map(u32::from);
            *grey = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
        }
        (self.prep.prepare(bgra), &self.plain)
    }

    /// What was read in those two pictures, `t` seconds in. Gives a subtitle once it is sure
    /// of it, with the time it was first seen.
    pub fn read(&mut self, t: f64, lines: &[Line], plain: &[Line]) -> Option<(f64, String)> {
        let width = self.width as f32;
        // Captions that grow word by word are followed by themselves: they are not centred,
        // and a row of them is not a subtitle until it is whole.
        let rolled = rows(plain, width, ROLL_GAP, ROLL_LEVEL_GAP);
        if self.roller.feed(t, &rolled, width) {
            // What the tracker had begun is the roller's, or old by the time it lets go.
            self.tracker.cur = None;
            self.spot = self
                .roller
                .edge
                .and_then(|(x, h, _)| bounds(rolled.iter().filter(|r| begins_at(r, x, h))));
            return self.roller.out.pop_front();
        }
        let block = pick(lines, width);
        self.prep.feedback(block.as_ref(), lines);
        let plain = pick(plain, width);
        if let Some(now) = plain.as_ref().and_then(|b| self.prep.whiteness(b)) {
            self.white = Some(self.white.map_or(now, |was| 0.8 * was + 0.2 * now));
        }
        // The prepared picture's reading where the letters are white, the plain one's else;
        // and outlined letters where the plain picture lost them over bright ground. Those
        // are read only when no other reading has had a subtitle for a while, and only when
        // they read like words: else they are not read at all.
        let edged = block.as_ref().is_some_and(|b| self.prep.outlined(b));
        if plain.is_some() || (block.is_some() && !edged) {
            self.other = t;
        }
        let edged = edged
            && block.as_ref().is_some_and(|b| {
                plain.is_none()
                    && t - self.other > GAP_S
                    && self.prep.ground(b) >= GROUND_MIN
                    && words_alike(&b.text)
            });
        let block = block.filter(|b| edged || !self.prep.outlined(b));
        let block = if self.white.is_some_and(|w| w < WHITE_MIN) && !edged {
            plain
        } else {
            block
        };
        if edged {
            self.edged = t;
        }
        let out = self.tracker.feed(t, block, self.prep.changed);
        // What was mended out of readings of outlined letters has to read like words too.
        let out = out.filter(|(_, text)| t - self.edged > 2.0 * GAP_S || words_alike(text));
        // A subtitle that goes on in another reading keeps its cover.
        self.spot = self
            .tracker
            .cur
            .as_ref()
            .filter(|c| c.reads.len() >= 2 || self.spot.is_some())
            .map(|c| c.bounds);
        out
    }
}

/// Does a reading of outlined letters read like words, `WORDS_ALIKE` of them? Read where only
/// part of the letters came out, they have signs in them ("h$ppy", "lot}:") and digits among
/// letters ("10t", "f0f"), marks after a word that no sentence ends with ("tiil!:"), and
/// letters other than English ones ("fißt"): it is set for English subtitles only.
fn words_alike(text: &str) -> bool {
    let (mut all, mut alike) = (0, 0);
    for word in text.split_whitespace() {
        let marks = |c: char| ".,?!:;'\"-".contains(c);
        let end = word.trim_end_matches(marks);
        let after = &word[end.len()..];
        let word = end.trim_start_matches(marks);
        let digits = word.chars().any(|c| c.is_ascii_digit());
        all += 1;
        alike += usize::from(
            word.chars()
                .all(|c| c.is_ascii_alphanumeric() || ".,'’-".contains(c))
                && (!digits || word.chars().all(|c| c.is_ascii_digit()))
                && (after.len() < 2
                    || after.chars().all(|c| c == '.')
                    || ["?!", "!?", "?\"", "!\"", ".\"", ",\"", "--", "'.", "',"].contains(&after)),
        );
    }
    all > 0 && alike as f32 >= WORDS_ALIKE * all as f32
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

/// The rows of text in a band `w` wide that are as tall as a subtitle's, top to bottom: the
/// pieces the recognition split a row into are one again, across a gap of up to `join` row
/// heights, or `level` of them where the pieces stand level.
fn rows(lines: &[Line], w: f32, join: f32, level: f32) -> Vec<Line> {
    let mut lines: Vec<&Line> = lines
        .iter()
        .filter(|l| {
            // A subtitle has a word of letters: a lone digit, a dash or a number plate
            // ("FC1988") is something else on the screen.
            (H_MIN * w..=H_MAX * w).contains(&(l.y1 - l.y0))
                && l.text.split_whitespace().any(|word| {
                    let word = word.trim_matches(|c: char| !c.is_alphanumeric());
                    !word.is_empty() && !word.chars().any(|c| c.is_numeric())
                })
        })
        .collect();
    lines.sort_by(|a, b| (a.y0 + a.y1).total_cmp(&(b.y0 + b.y1)));
    let mut rows: Vec<Line> = Vec::new();
    for l in lines {
        if let Some(r) = rows.last_mut() {
            let (hr, hl) = (r.y1 - r.y0, l.y1 - l.y0);
            let even = (r.y0 - l.y0).abs().max((r.y1 - l.y1).abs()) <= LEVEL * hr.min(hl);
            let gap = if even { level } else { join } * hr.max(hl);
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
    rows
}

/// Which of the lines read in a band `w` wide are the subtitle: the lowest centred row and
/// the centred rows right above it.
fn pick(lines: &[Line], w: f32) -> Option<Block> {
    let mut rows = rows(lines, w, JOIN_GAP, JOIN_GAP);
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

/// A row of captions that grow word by word.
struct Rolled {
    key: Vec<char>,
    text: String,
    x0: f32,
    h: f32,
    /// When it was first seen, last gained a word and was last seen.
    born: f64,
    grew: f64,
    seen: f64,
    /// How often it gained a word, and how many readings on end had it as it reads now.
    grown: u32,
    same: u32,
    /// How many of its words were given on.
    given: usize,
    /// How many rows stood under it when it was last seen, and whether a row has begun under
    /// it since it was first seen.
    up: usize,
    rose: bool,
}

impl Rolled {
    /// Is `key`, read at `x0` and `up` rows from the lowest, this row: as it is, with more
    /// words after it, or with its end not read? A row only ever moves up, so one that
    /// begins like a row above it is another. A new row begins at the bottom, so above it
    /// even the first word or two of a row are that row.
    fn is(&self, key: &[char], x0: f32, up: usize) -> bool {
        let n = self.key.len().min(key.len());
        (x0 - self.x0).abs() <= 1.5 * self.h.max(H_MIN * WIDTH as f32)
            && (key == self.key.as_slice()
                || (up >= self.up
                    && (up > 0 || n as f32 >= ROLL_PART * self.key.len() as f32)
                    && lev(&self.key[..n], &key[..n]) as f32 <= (ROLL_ERR * n as f32).floor()))
    }

    /// Its words that were not given on yet, as given on now.
    fn rest(&mut self) -> Option<(f64, String)> {
        let words: Vec<&str> = words(&self.text).collect();
        let fresh = words.get(self.given..).filter(|w| !w.is_empty())?.join(" ");
        self.given = words.len();
        Some((self.born, apostrophes(&commas(&fresh))))
    }
}

/// A row's text and where it begins, without what the recognition read before the left edge
/// at `x` the captions grow from: a mark, or a letter it made of the picture beside them.
fn from_edge(line: &Line, x: f32, h: f32) -> (String, f32) {
    let first = line
        .words
        .iter()
        .take(3)
        .position(|&(_, x0, _)| (x0 - x).abs() <= EDGE_TOL * h)
        .unwrap_or(0);
    let words: Vec<&str> = line.words[first..].iter().map(|w| w.0.as_str()).collect();
    match first {
        0 => (line.text.clone(), line.x0),
        _ => (words.join(" "), line.words[first].1),
    }
}

/// Does a row begin at `x`, give or take `h`? One of its first words does then: a mark the
/// recognition read before the row (a dot, a dash) stands left of where it begins.
fn begins_at(line: &Line, x: f32, h: f32) -> bool {
    line.words
        .iter()
        .take(3)
        .any(|&(_, x0, _)| (x0 - x).abs() <= h)
}

/// The words of a row: what has no letter or digit in it is a mark (">>" for another speaker).
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
}

/// Follows captions that grow word by word from a left edge and move up a row when the next
/// one begins. Such a row is told by its growing and moving up; from then on the rows at its
/// left edge are the captions, for as long as words join them. A row is given on when the next
/// one has begun under it, or when no word has joined it for a while, and then only its words
/// that were not given on before.
#[derive(Default)]
struct Roller {
    rows: Vec<Rolled>,
    /// The left edge the captions grow from, how tall they are, and when one was last there.
    edge: Option<(f32, f32, f64)>,
    out: VecDeque<(f64, String)>,
}

impl Roller {
    /// `rows`: what `rows()` gave for the picture at `t` seconds, `w` wide. True while there
    /// are such captions; what is to be given on of them is in `out` then.
    fn feed(&mut self, t: f64, rows: &[Line], w: f32) -> bool {
        self.rows.retain(|r| t - r.seen <= ROLL_DONE_S);
        if self.edge.is_some_and(|(.., at)| t - at > ROLL_KEEP_S) {
            self.edge = None;
            // Taking over again has to be earned again.
            for r in &mut self.rows {
                r.grown = 0;
                r.rose = false;
            }
        }
        let edge = self.edge;
        let rows: Vec<&Line> = rows
            .iter()
            .filter(|r| edge.is_none_or(|(x, h, _)| begins_at(r, x, h)))
            .collect();
        // Which row of `self.rows` each is, top to bottom: a row is taken once, by the
        // reading that is most of it.
        let mut taken: Vec<usize> = Vec::new();
        for (i, line) in rows.iter().enumerate() {
            let (text, x0) = match self.edge {
                Some((x, h, _)) => from_edge(line, x, h),
                None => (line.text.clone(), line.x0),
            };
            let (key, up) = (key(&text), rows.len() - 1 - i);
            // Of two rows that begin alike, the one nearer to it in length.
            let old = (0..self.rows.len())
                .filter(|j| !taken.contains(j))
                .filter(|&j| t - self.rows[j].seen <= ROLL_GONE_S)
                .filter(|&j| self.rows[j].is(&key, x0, up))
                .min_by_key(|&j| self.rows[j].key.len().abs_diff(key.len()));
            let j = match old {
                Some(j) => {
                    let r = &mut self.rows[j];
                    r.seen = t;
                    r.rose |= up > r.up;
                    r.up = up;
                    // A row's first word may be a low one.
                    r.h = r.h.max(line.y1 - line.y0);
                    // A reading that lacks the row's end leaves it as it was.
                    if key.len() + 2 >= r.key.len() {
                        let (was, now): (Vec<&str>, Vec<&str>) =
                            (words(&r.text).collect(), words(&text).collect());
                        if now.len() > was.len() {
                            let middle = ((line.x0 + line.x1) / 2.0 - w / 2.0).abs() <= C_TOL * w;
                            let joined =
                                now.len() - was.len() <= GROW_WORDS && now.starts_with(&was);
                            r.grown += u32::from(joined && !middle);
                            r.grew = t;
                            r.same = 0;
                        } else {
                            if now.len() < was.len() {
                                r.grown = 0;
                            }
                            if key != r.key {
                                // Marks that come and go in its readings do not make it another.
                                r.same = 0;
                            }
                        }
                        r.same += 1;
                        r.text = text;
                        r.key = key;
                    }
                    j
                }
                None => {
                    self.rows.push(Rolled {
                        key,
                        text,
                        x0,
                        h: line.y1 - line.y0,
                        born: t,
                        grew: t,
                        seen: t,
                        grown: 0,
                        same: 1,
                        given: 0,
                        up,
                        rose: false,
                    });
                    self.rows.len() - 1
                }
            };
            taken.push(j);
            let r = &mut self.rows[j];
            if self.edge.is_none() && r.grown >= GROW_MIN && r.rose {
                log::debug!("screen: captions grow from {:.0}: {:?}", r.x0, r.text);
                self.edge = Some((r.x0, r.h, t));
            }
            let Some(edge) = &mut self.edge else {
                continue;
            };
            if !begins_at(line, edge.0, edge.1) {
                continue;
            }
            edge.1 = edge.1.max(r.h);
            // A row that stands at the edge and does not grow, a label, does not keep it.
            if t - r.grew <= ROLL_KEEP_S {
                edge.2 = t;
            }
            // Whole when another row stands under it, or nothing has joined it for a while.
            let whole = i + 1 < rows.len() || t - r.grew >= ROLL_PAUSE_S;
            if whole && r.same >= 2 {
                self.out.extend(r.rest());
            }
        }
        if let Some(edge) = self.edge {
            // What went away before it was given on: at once when another row stood under it,
            // which made it whole; else once it has been gone a while.
            for r in &mut self.rows {
                let there = (r.x0 - edge.0).abs() <= 1.5 * edge.1;
                let gone = t - r.seen > ROLL_GONE_S || (r.up > 0 && r.seen < t);
                if gone && r.same >= 2 && there {
                    self.out.extend(r.rest());
                }
            }
        }
        // In the order they were first seen: rows seen before the captions were told apart
        // are given on together once they are.
        self.out
            .make_contiguous()
            .sort_by(|a, b| a.0.total_cmp(&b.0));
        self.edge.is_some() || !self.out.is_empty()
    }
}

/// The subtitle that stands: its readings since it appeared.
struct Track {
    t0: f64,
    last: f64,
    reads: Vec<Block>,
    /// What all its readings cover.
    bounds: Bounds,
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
        let mut ended = None;
        if changed {
            ended = self.end(t);
            self.done.iter_mut().for_each(|d| d.gone = true);
        }
        if self.cur.as_ref().is_some_and(|c| t - c.last > GAP_S) {
            ended = ended.or(self.end(t));
        }
        let Some(block) = block else {
            return ended;
        };
        let cur = match &mut self.cur {
            // Another reading of the subtitle that stands. One that reads or lies otherwise
            // takes its place: most often the same subtitle, come out further.
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
                if let Some(([x0, y0, x1, y1], h)) = bounds(&block.rows) {
                    let ([a0, b0, a1, b1], g) = c.bounds;
                    c.bounds = ([a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)], g.max(h));
                }
                c.reads.push(block);
                c.last = t;
                c
            }
            cur => cur.insert(Track {
                t0: t,
                last: t,
                bounds: bounds(&block.rows).unwrap_or_default(),
                reads: vec![block],
                best: 0,
                emitted: false,
            }),
        };
        if cur.emitted {
            if let Some(block) = cur.reads.last() {
                for r in &block.rows {
                    known(&mut self.done, &r.text, (r.x0, r.x1), block.h, t, t);
                }
            }
            return ended;
        }
        let Some(rows) = cur.choose(t) else {
            return ended;
        };
        cur.emitted = true;
        give(&mut self.done, cur, rows, t)
    }

    /// The subtitle that stood is gone: its pixels went, or nothing was read for a while. One
    /// that was up too briefly for its words to agree,
    /// but was read `LATE_N - 1` times with `LATE_FRAC` of them agreeing, is given on now:
    /// a word of it may be wrong, which a reader forgives sooner than a line that is missing.
    fn end(&mut self, t: f64) -> Option<(f64, String)> {
        let cur = self.cur.take()?;
        let n = cur.reads.len();
        if cur.emitted || n + 1 < LATE_N {
            return None;
        }
        let (rows, sure, _) = mend(&cur.reads[n.saturating_sub(MEND_READS)..])?;
        (sure >= LATE_FRAC)
            .then(|| give(&mut self.done, &cur, rows, t))
            .flatten()
    }
}

/// The rows of a subtitle to give on at `t`: those not given on lately, as one text with the
/// time the subtitle was first seen.
fn give(done: &mut Vec<Done>, cur: &Track, rows: Vec<String>, t: f64) -> Option<(f64, String)> {
    done.retain(|d| t - d.t <= REPEAT_S);
    let block = cur.reads.last()?;
    let mut fresh = Vec::new();
    for (i, text) in rows.into_iter().enumerate() {
        let r = &block.rows[i.min(block.rows.len() - 1)];
        if !known(done, &text, (r.x0, r.x1), block.h, cur.t0, t) {
            done.push(Done {
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
    fn a_subtitle_gone_before_its_words_agreed_is_given_when_it_goes() {
        // Its first word is read differently each time, and the next subtitle comes.
        let pictures = [
            (0.0, vec![line("Whet fucking thing?")], false),
            (0.1, vec![line("What fucking thing?")], false),
            (0.2, vec![line("Vmat fucking thing?")], false),
            (0.3, vec![line(B)], true),
            (0.4, vec![line(B)], false),
        ];
        assert_eq!(
            run(&pictures),
            [
                (0, 3, "Whet fucking thing?".to_string()),
                (3, 4, B.to_string())
            ]
        );
    }

    /// What a roller gives on for pictures of (seconds, the rows read in them).
    fn rolled(pictures: &[(f64, Vec<Line>)]) -> Vec<String> {
        let mut roller = Roller::default();
        let mut out = Vec::new();
        for (t, lines) in pictures {
            roller.feed(*t, &rows(lines, W, ROLL_GAP, ROLL_LEVEL_GAP), W);
            out.extend(roller.out.drain(..).map(|(_, text)| text));
        }
        out
    }

    /// Pictures a tenth of a second apart of captions that grow a word every fifth of one, two
    /// rows at most, each from the left edge at 300.
    fn rolling(lines: &[&str]) -> Vec<(f64, Vec<Line>)> {
        let mut pictures = Vec::new();
        let mut t = 0.0;
        for (i, text) in lines.iter().enumerate() {
            let all: Vec<&str> = text.split(' ').collect();
            for n in 1..=all.len() {
                for _ in 0..2 {
                    let mut rows = Vec::new();
                    if i > 0 {
                        rows.push(line_at(lines[i - 1], 150.0, Some(300.0)));
                    }
                    rows.push(line_at(&all[..n].join(" "), 190.0, Some(300.0)));
                    pictures.push((t, rows));
                    t += 0.1;
                }
            }
        }
        // The last row stands for two seconds more.
        for _ in 0..20 {
            let i = lines.len() - 1;
            let mut rows = vec![line_at(lines[i], 190.0, Some(300.0))];
            if i > 0 {
                rows.insert(0, line_at(lines[i - 1], 150.0, Some(300.0)));
            }
            pictures.push((t, rows));
            t += 0.1;
        }
        pictures
    }

    #[test]
    fn captions_that_grow_word_by_word_are_given_row_by_row() {
        let lines = [
            ">> I would not like to see it as",
            "a neighbour of theirs and so I",
            "went to the meeting",
        ];
        assert_eq!(
            rolled(&rolling(&lines)),
            [
                "I would not like to see it as",
                "a neighbour of theirs and so I",
                "went to the meeting",
            ]
        );
    }

    #[test]
    fn a_row_given_at_a_pause_goes_on_with_its_other_words_only() {
        // Three words, a pause of two seconds, then two more and the next row.
        let mut pictures = Vec::new();
        let at = |n: usize| n as f64 / 10.0;
        let row = |text: &str, y: f32| line_at(text, y, Some(300.0));
        for (n, text) in ["so", "so I", "so I", "so I went", "so I went"]
            .iter()
            .enumerate()
        {
            pictures.push((
                at(n),
                vec![row("it was a long day and", 150.0), row(text, 190.0)],
            ));
        }
        for n in 5..25 {
            pictures.push((
                at(n),
                vec![row("it was a long day and", 150.0), row("so I went", 190.0)],
            ));
        }
        for n in 25..30 {
            pictures.push((
                at(n),
                vec![
                    row("it was a long day and", 150.0),
                    row("so I went home early", 190.0),
                ],
            ));
        }
        for n in 30..36 {
            pictures.push((
                at(n),
                vec![row("so I went home early", 150.0), row("that", 190.0)],
            ));
        }
        // The first row grew before these pictures and is followed once the next begins under
        // it: else the row under it would have to grow thrice and move up.
        let mut before = vec![
            (-0.4, vec![row("it was", 190.0)]),
            (-0.3, vec![row("it was a", 190.0)]),
        ];
        before.push((-0.2, vec![row("it was a long", 190.0)]));
        before.push((-0.1, vec![row("it was a long day and", 190.0)]));
        before.extend(pictures);
        assert_eq!(
            rolled(&before),
            ["it was a long day and", "so I went", "home early"]
        );
    }

    #[test]
    fn a_row_read_in_pieces_while_it_moves_up_is_given_once() {
        // The lower row grows, the next begins under it, and for two pictures only the first
        // words of the row that moved up are read, a few pixels off its place.
        let row = |text: &str, y: f32| line_at(text, y, Some(300.0));
        // The upper row was the lower one before.
        let mut pictures = vec![(-0.1, vec![row("buying a new car", 190.0)])];
        let mut t = 0.0;
        for text in [
            "why",
            "why the",
            "why the am",
            "why the am i",
            "why the am i the only one",
        ] {
            for _ in 0..2 {
                pictures.push((t, vec![row("buying a new car", 150.0), row(text, 190.0)]));
                t += 0.1;
            }
        }
        pictures.push((t, vec![row("why the", 156.0), row("this", 196.0)]));
        pictures.push((t + 0.1, vec![row("why the", 158.0), row("this", 198.0)]));
        for n in 2u32..30 {
            let lower = ["this", "this cause", "this cause we", "this cause we got"]
                [(n as usize / 4).min(3)];
            pictures.push((
                t + f64::from(n) / 10.0,
                vec![row("why the am i the only one", 150.0), row(lower, 190.0)],
            ));
        }
        assert_eq!(
            rolled(&pictures),
            [
                "buying a new car",
                "why the am i the only one",
                "this cause we got"
            ]
        );
    }

    #[test]
    fn a_subtitle_read_with_a_word_more_and_a_word_less_does_not_grow() {
        // The recognition splits and joins a word of a row that stands, a dozen times.
        let mut roller = Roller::default();
        for n in 0..40 {
            let text = if n % 3 == 0 {
                "You are a racist and I don t like you"
            } else {
                "You are a racist and I dont like you"
            };
            assert!(!roller.feed(
                f64::from(n) / 10.0,
                &rows(&[line(text)], W, ROLL_GAP, ROLL_LEVEL_GAP),
                W
            ));
        }
        // Off centre too: the words it had do not stand as they were.
        let mut roller = Roller::default();
        for n in 0..40 {
            let text = if n % 3 == 0 {
                "You are a racist and I don t like you"
            } else {
                "You are a racist and I dont like you"
            };
            let lines = [line_at(text, 188.0, Some(300.0))];
            assert!(!roller.feed(
                f64::from(n) / 10.0,
                &rows(&lines, W, ROLL_GAP, ROLL_LEVEL_GAP),
                W
            ));
        }
    }

    #[test]
    fn subtitles_that_stand_are_not_captions_that_grow() {
        // A centred subtitle, and another under it later: neither gains a word.
        let mut roller = Roller::default();
        for n in 0..40 {
            let lines = if n < 20 {
                vec![line(A)]
            } else {
                vec![line(A), line_at(B, 230.0, None)]
            };
            assert!(!roller.feed(
                f64::from(n) / 10.0,
                &rows(&lines, W, ROLL_GAP, ROLL_LEVEL_GAP),
                W
            ));
        }
        assert!(roller.out.is_empty());
    }

    #[test]
    fn text_typed_at_a_left_edge_is_not_captions_that_grow() {
        // A row gains a word three times, and no row begins under it.
        let typed = ["so", "so I", "so I went", "so I went home"];
        let mut roller = Roller::default();
        for n in 0..40u32 {
            let text = typed[(n as usize / 4).min(typed.len() - 1)];
            let lines = [line_at(text, 190.0, Some(300.0))];
            assert!(!roller.feed(
                f64::from(n) / 10.0,
                &rows(&lines, W, ROLL_GAP, ROLL_LEVEL_GAP),
                W
            ));
        }
        assert!(roller.out.is_empty());
    }

    #[test]
    fn the_cover_is_what_the_rows_of_the_subtitle_read_cover() {
        let mut reader = Reader::new(1920, 300);
        let two = vec![line_at(A, 148.0, None), line(B)];
        reader.read(0.0, &two, &two);
        // Read once, it may be something else.
        assert_eq!(reader.spot(), None);
        reader.read(0.1, &two, &two);
        // A is the wider row: 23 letters of 14 pixels, centred; B's bottom is the lowest.
        let spot = reader.spot().unwrap();
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let Area {
            left,
            top,
            right,
            bottom,
        } = spot.area;
        assert!(
            near(left, 799.0 / 1920.0)
                && near(right, 1121.0 / 1920.0)
                && near(top, 148.0 / 300.0)
                && near(bottom, 218.0 / 300.0)
                && near(spot.row, 0.1),
            "{spot:?}"
        );
        // A picture without it does not end it, one long after does.
        reader.read(0.2, &[], &[]);
        assert_eq!(reader.spot(), Some(spot));
        reader.read(0.8, &[], &[]);
        assert_eq!(reader.spot(), None);
    }

    #[test]
    fn outlined_letters_read_in_part_are_not_words() {
        assert!(words_alike(
            "Says.he got 8 telegram from some Blackwater big bugs askin' for guide."
        ));
        assert!(words_alike("Well, I'm not - \"Stupid\" is the word."));
        assert!(!words_alike(
            "I'm h$ppy Kith my current tight 10K. Mi\" Yitcf8flne."
        ));
        assert!(!words_alike("I uspct *toln thea broken."));
        assert!(!words_alike("I don't tiiil!: san reach ie ston before."));
        assert!(!words_alike("fi\u{df}t impression herd to"));
        assert!(words_alike("What?! No... \"Never.\""));
        assert!(!words_alike("Th8t's right."));
        assert!(!words_alike(""));
    }

    #[test]
    fn a_full_stop_before_a_small_letter_is_a_comma() {
        assert_eq!(commas("Man. get in. Now."), "Man, get in. Now.");
        assert_eq!(commas("I... mean... no. I do."), "I... mean... no. I do.");
    }

    #[test]
    fn a_number_plate_is_not_a_subtitle() {
        assert_eq!(picked(&[line("FC1988")]), None);
        assert_eq!(picked(&[line("- 7")]), None);
        assert_eq!(
            picked(&[line("I... I got 5 of 'em.")]).as_deref(),
            Some("I... I got 5 of 'em.")
        );
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

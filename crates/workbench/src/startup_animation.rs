//! The startup animation's timeline and geometry (`SPEC-0355` v2 behaviour 23): Ferris IDEA's own
//! brand sequence, drawn from the brand's vector paths while start-up work still runs. Nothing
//! here draws; [`crate::splash_view`] paints the [`Frame`] this module computes for a moment.
//!
//! All lengths are units of the horizontal logo (`idecraft-horizontal.svg`, 109 × 32, generated
//! by `scripts/brand-assets.py`): the symbol occupies 0–32 with the crab's cells from 2 to 30, the
//! wordmark group is translated by 32.5 and its first glyph starts at 34.5, 4.5 units after the
//! crab's right edge.

use std::sync::OnceLock;
use std::time::Duration;

/// The crab is built on its grid until here (step 1).
pub const SYMBOL_END: Duration = Duration::from_millis(450);
/// The cursor bar appears at [`SYMBOL_END`], is off during this span (its one blink) and on again
/// after it (step 2).
pub const BLINK_OFF: (Duration, Duration) =
    (Duration::from_millis(520), Duration::from_millis(580));
/// The symbol starts moving left here (step 3)…
pub const MOVE_START: Duration = Duration::from_millis(650);
/// …and is in its place in the horizontal logo here.
pub const MOVE_END: Duration = Duration::from_millis(900);
/// The mask over the wordmark starts opening here…
pub const REVEAL_START: Duration = Duration::from_millis(720);
/// …and shows all of it here; then the horizontal logo rests (step 4).
pub const REVEAL_END: Duration = Duration::from_millis(1100);
/// The splash never stays longer than this in all, its fade included.
pub const CAP: Duration = Duration::from_millis(1500);
/// The fade at the end of the animation, once the first window is ready (or at the cap).
pub const FADE: Duration = Duration::from_millis(150);

/// Width of the horizontal logo.
pub const LOGO_WIDTH: f32 = 109.0;
/// Height of the horizontal logo.
pub const LOGO_HEIGHT: f32 = 32.0;
/// Where the wordmark group starts (`translate(32.5, 0)`).
pub const WORDMARK_X: f32 = 32.5;
/// Where the wordmark's first glyph starts inside its group.
pub const WORDMARK_INSET: f32 = 2.0;
/// The symbol's vertical axis, which the crab's rows grow out from.
pub const SYMBOL_AXIS: f32 = LOGO_HEIGHT / 2.0;
/// How far right of its final place the symbol starts: centred in the logo's width.
pub const START_SHIFT: f32 = LOGO_WIDTH / 2.0 - LOGO_HEIGHT / 2.0;

/// How the splash shows, chosen once at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The animated sequence, then the resting logo.
    Animated,
    /// The final horizontal logo, still (the system asks to reduce motion).
    Still,
    /// Nothing (the setting is off, or the brand paths could not be read).
    Off,
}

impl Mode {
    /// The mode for the setting `appearance.startup_animation`, the system's Reduce Motion and
    /// whether the brand's paths were read.
    pub fn choose(setting_on: bool, reduce_motion: bool, brand_read: bool) -> Self {
        match (setting_on && brand_read, reduce_motion) {
            (false, _) => Mode::Off,
            (true, true) => Mode::Still,
            (true, false) => Mode::Animated,
        }
    }

    /// The fade at the end: none for the still logo.
    pub fn fade(self) -> Duration {
        match self {
            Mode::Animated => FADE,
            Mode::Still | Mode::Off => Duration::ZERO,
        }
    }
}

/// What the splash shows at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// How much of the crab is built, 0 to 1.
    pub symbol: f32,
    /// Whether the cursor bar shows.
    pub cursor: bool,
    /// How far right of its final place the symbol stands.
    pub shift: f32,
    /// The part of the logo's width the mask over the wordmark lets through (`left, right`); the
    /// left edge follows the symbol, so the wordmark never shows behind it; nothing shows while
    /// `right <= left`.
    pub mask: (f32, f32),
    /// Opacity of the whole splash, 1 until the fade.
    pub opacity: f32,
}

impl Frame {
    /// The resting horizontal logo.
    pub const FINAL: Frame = Frame {
        symbol: 1.0,
        cursor: true,
        shift: 0.0,
        mask: (WORDMARK_X, LOGO_WIDTH),
        opacity: 1.0,
    };
}

/// When the splash closes: the fade starts once the first window is ready (`ready`, from the
/// splash's start) or at the latest so that it ends at [`CAP`]. `None` for [`Mode::Off`].
pub fn closes_at(mode: Mode, ready: Option<Duration>) -> Option<Duration> {
    if mode == Mode::Off {
        return None;
    }
    let latest = CAP - mode.fade();
    let fade_from = ready.map_or(latest, |r| r.min(latest));
    Some(fade_from + mode.fade())
}

/// The frame at `elapsed` since the splash opened, with the first window ready at `ready`;
/// `None` once the splash is closed (and always for [`Mode::Off`]).
pub fn frame(mode: Mode, elapsed: Duration, ready: Option<Duration>) -> Option<Frame> {
    let end = closes_at(mode, ready)?;
    if elapsed >= end {
        return None;
    }
    let fade = mode.fade();
    let opacity = if fade.is_zero() {
        1.0
    } else {
        let fade_from = end - fade;
        1.0 - progress(elapsed, fade_from, end)
    };
    let mut f = match mode {
        Mode::Still => Frame::FINAL,
        Mode::Animated => animated(elapsed),
        Mode::Off => return None,
    };
    f.opacity = opacity;
    Some(f)
}

fn animated(t: Duration) -> Frame {
    let symbol = ease_in_out(progress(t, Duration::ZERO, SYMBOL_END));
    let cursor = t >= SYMBOL_END && !(t >= BLINK_OFF.0 && t < BLINK_OFF.1);
    let shift = START_SHIFT * (1.0 - ease_out(progress(t, MOVE_START, MOVE_END)));
    let reveal = ease_in_out(progress(t, REVEAL_START, REVEAL_END));
    let left = WORDMARK_X + shift;
    let right = WORDMARK_X + WORDMARK_INSET + reveal * (LOGO_WIDTH - WORDMARK_X - WORDMARK_INSET);
    Frame {
        symbol,
        cursor,
        shift,
        mask: (left, right),
        opacity: 1.0,
    }
}

/// Where `t` stands between `from` and `to`, 0 to 1.
fn progress(t: Duration, from: Duration, to: Duration) -> f32 {
    if t <= from {
        0.0
    } else if t >= to {
        1.0
    } else {
        (t - from).as_secs_f32() / (to - from).as_secs_f32()
    }
}

fn ease_in_out(p: f32) -> f32 {
    if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) / 2.0
    }
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p).powi(3)
}

/// The crab's cells built so far (`progress` 0 to 1), in symbol units (`x0, y0, x1, y1`): the rows
/// are built one after the other from the top, each in an equal share of the progress, and a row
/// grows outwards from the symbol's axis to both sides at once; complete, they are exactly the
/// master's cells.
pub fn symbol_rects(cells: &[[f32; 4]], progress: f32) -> Vec<[f32; 4]> {
    let mut rows: Vec<f32> = Vec::new();
    for c in cells {
        if !rows.contains(&c[1]) {
            rows.push(c[1]);
        }
    }
    rows.sort_by(f32::total_cmp);
    let built = progress.clamp(0.0, 1.0) * rows.len() as f32;
    let mut out = Vec::new();
    for (i, y) in rows.iter().enumerate() {
        let part = (built - i as f32).clamp(0.0, 1.0);
        if part <= 0.0 {
            break;
        }
        let row: Vec<&[f32; 4]> = cells.iter().filter(|c| c[1] == *y).collect();
        let reach = row
            .iter()
            .map(|c| (SYMBOL_AXIS - c[0]).max(c[2] - SYMBOL_AXIS))
            .fold(0.0f32, f32::max);
        let (lo, hi) = if part >= 1.0 {
            (f32::MIN, f32::MAX)
        } else {
            (SYMBOL_AXIS - part * reach, SYMBOL_AXIS + part * reach)
        };
        for &&[x0, y0, x1, y1] in &row {
            let (l, r) = (x0.max(lo), x1.min(hi));
            if l < r {
                out.push([l, y0, r, y1]);
            }
        }
    }
    out
}

/// The cells of an outline made of axis-aligned rectangles, one per contour (`x0, y0, x1, y1`);
/// `None` if a contour is anything else.
fn cells_of(shape: &Shape) -> Option<Vec<[f32; 4]>> {
    let mut out = Vec::new();
    let mut pts: Vec<Pt> = Vec::new();
    for seg in shape.segs.iter().chain([&Seg::Close]) {
        match *seg {
            Seg::Move(p) | Seg::Line(p) => pts.push(p),
            Seg::Cubic(..) => return None,
            Seg::Close if pts.is_empty() => {}
            Seg::Close => {
                if pts.first() == pts.last() && pts.len() > 1 {
                    pts.pop();
                }
                let xs: Vec<f32> = pts.iter().map(|p| p.0).collect();
                let ys: Vec<f32> = pts.iter().map(|p| p.1).collect();
                let b = [
                    xs.iter().copied().fold(f32::MAX, f32::min),
                    ys.iter().copied().fold(f32::MAX, f32::min),
                    xs.iter().copied().fold(f32::MIN, f32::max),
                    ys.iter().copied().fold(f32::MIN, f32::max),
                ];
                let corner = |&(x, y): &Pt| (x == b[0] || x == b[2]) && (y == b[1] || y == b[3]);
                if pts.len() != 4 || !pts.iter().all(corner) || b[0] >= b[2] || b[1] >= b[3] {
                    return None;
                }
                out.push(b);
                pts.clear();
            }
        }
    }
    Some(out)
}

/// A point in logo units.
pub type Pt = (f32, f32);

/// One segment of an outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// Start a new contour.
    Move(Pt),
    /// A straight line.
    Line(Pt),
    /// A cubic Bézier curve: control points, then the end point.
    Cubic(Pt, Pt, Pt),
    /// Close the contour.
    Close,
}

/// A filled outline of the brand with its colour (`0xRRGGBB`); contours fill even-odd.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The outline.
    pub segs: Vec<Seg>,
    /// The fill colour.
    pub color: u32,
}

impl Shape {
    /// The smallest rectangle around the outline's points (`x0, y0, x1, y1`).
    pub fn bounds(&self) -> [f32; 4] {
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut add = |(x, y): Pt| {
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        };
        for s in &self.segs {
            match *s {
                Seg::Move(p) | Seg::Line(p) => add(p),
                Seg::Cubic(a, c, p) => {
                    add(a);
                    add(c);
                    add(p);
                }
                Seg::Close => {}
            }
        }
        b
    }
}

/// The brand as the splash draws it, read from the embedded vector masters.
#[derive(Debug, Clone, PartialEq)]
pub struct Brand {
    /// `background-primary`, from `idecraft-symbol-dark.svg`.
    pub background: u32,
    /// The crab's outline and colour (`foreground-primary`).
    pub symbol: Shape,
    /// The crab's cells, one rectangle per run of its grid, in the master's order (top to bottom).
    pub cells: Vec<[f32; 4]>,
    /// The cursor bar (`accent-primary`).
    pub cursor: Shape,
    /// The wordmark's outlines ("Ferris", "IDEA") in logo units, the group's translation applied.
    pub wordmark: Vec<Shape>,
}

/// The brand read once from [`ferris_idea_ui::brand`]; `None` when the masters cannot be read, and
/// then nothing is shown.
pub fn brand() -> Option<&'static Brand> {
    static BRAND: OnceLock<Option<Brand>> = OnceLock::new();
    BRAND
        .get_or_init(|| {
            read_brand(
                ferris_idea_ui::brand::HORIZONTAL_LOGO_SVG,
                ferris_idea_ui::brand::SYMBOL_DARK_SVG,
            )
        })
        .as_ref()
}

/// Reads the brand from the horizontal logo and the dark symbol (which gives the background).
pub fn read_brand(horizontal: &str, symbol_dark: &str) -> Option<Brand> {
    let shapes = svg_shapes(horizontal)?;
    let background = svg_tags(symbol_dark)
        .find(|t| t.starts_with("rect"))
        .and_then(|t| attr(t, "fill"))
        .and_then(color)?;
    let mut it = shapes.into_iter();
    let (symbol, _) = it.next()?;
    let (cursor, _) = it.next()?;
    let cells = cells_of(&symbol)?;
    let wordmark: Vec<Shape> = it
        .filter(|(_, dx)| *dx == WORDMARK_X)
        .map(|(s, _)| s)
        .collect();
    (wordmark.len() == 2).then_some(Brand {
        background,
        symbol,
        cells,
        cursor,
        wordmark,
    })
}

/// The `d` attributes of the paths of an SVG, in order (tests compare the masters).
pub fn svg_path_data(svg: &str) -> Vec<String> {
    svg_tags(svg)
        .filter(|t| t.starts_with("path"))
        .filter_map(|t| attr(t, "d").map(str::to_owned))
        .collect()
}

/// The filled paths of `svg` with the horizontal translation of their group applied (and returned
/// beside each shape); `None` if a path cannot be read.
fn svg_shapes(svg: &str) -> Option<Vec<(Shape, f32)>> {
    let mut dx = vec![0.0f32];
    let mut out = Vec::new();
    for tag in svg_tags(svg) {
        if tag.starts_with("g ") || tag == "g" {
            let x = attr(tag, "transform")
                .and_then(|t| t.trim().strip_prefix("translate("))
                .and_then(|t| t.split([',', ' ', ')']).find(|s| !s.is_empty()))
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(0.0);
            dx.push(dx.last().copied().unwrap_or(0.0) + x);
        } else if tag.starts_with("/g") {
            dx.pop();
        } else if tag.starts_with("path") {
            let shift = dx.last().copied().unwrap_or(0.0);
            let segs = path_segments(attr(tag, "d")?, shift)?;
            let color = attr(tag, "fill").and_then(color)?;
            out.push((Shape { segs, color }, shift));
        }
    }
    Some(out)
}

/// The tags of an SVG without their angle brackets.
fn svg_tags(svg: &str) -> impl Iterator<Item = &str> {
    svg.split('<').skip(1).filter_map(|s| {
        s.split_once('>')
            .map(|(t, _)| t.trim_end_matches('/').trim())
    })
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    tag[at..].split_once('"').map(|(v, _)| v)
}

fn color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok())?
}

/// The segments of SVG path data with absolute commands (`M L H V C Z`, the ones the brand's
/// masters use), shifted by `dx`; `None` for anything else.
pub fn path_segments(d: &str, dx: f32) -> Option<Vec<Seg>> {
    let mut tokens = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, tokens: &mut Vec<Tok>| -> Option<()> {
        if !num.is_empty() {
            tokens.push(Tok::Num(num.parse().ok()?));
            num.clear();
        }
        Some(())
    };
    for ch in d.chars() {
        match ch {
            '0'..='9' | '.' => {
                // A second point starts a new number ("1.5.5").
                if ch == '.' && num.contains('.') {
                    flush(&mut num, &mut tokens)?;
                }
                num.push(ch);
            }
            '-' => {
                flush(&mut num, &mut tokens)?;
                num.push(ch);
            }
            ' ' | ',' | '\n' | '\t' | '\r' => flush(&mut num, &mut tokens)?,
            c if c.is_ascii_alphabetic() => {
                flush(&mut num, &mut tokens)?;
                tokens.push(Tok::Cmd(c));
            }
            _ => return None,
        }
    }
    flush(&mut num, &mut tokens)?;

    let mut segs = Vec::new();
    let mut at = (0.0f32, 0.0f32);
    let mut start = at;
    let mut i = 0;
    let mut cmd = None;
    let take = |i: &mut usize, n: usize| -> Option<Vec<f32>> {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            match tokens.get(*i)? {
                Tok::Num(x) => v.push(*x),
                Tok::Cmd(_) => return None,
            }
            *i += 1;
        }
        Some(v)
    };
    while i < tokens.len() {
        if let Tok::Cmd(c) = tokens[i] {
            cmd = Some(c);
            i += 1;
            if c == 'Z' {
                segs.push(Seg::Close);
                at = start;
                continue;
            }
        }
        match cmd? {
            'M' => {
                let v = take(&mut i, 2)?;
                at = (v[0], v[1]);
                start = at;
                segs.push(Seg::Move(at));
                // Further pairs after M are lines.
                cmd = Some('L');
            }
            'L' => {
                let v = take(&mut i, 2)?;
                at = (v[0], v[1]);
                segs.push(Seg::Line(at));
            }
            'H' => {
                at.0 = take(&mut i, 1)?[0];
                segs.push(Seg::Line(at));
            }
            'V' => {
                at.1 = take(&mut i, 1)?[0];
                segs.push(Seg::Line(at));
            }
            'C' => {
                let v = take(&mut i, 6)?;
                at = (v[4], v[5]);
                segs.push(Seg::Cubic((v[0], v[1]), (v[2], v[3]), at));
            }
            _ => return None,
        }
    }
    let shift = |(x, y): Pt| (x + dx, y);
    Some(
        segs.into_iter()
            .map(|s| match s {
                Seg::Move(p) => Seg::Move(shift(p)),
                Seg::Line(p) => Seg::Line(shift(p)),
                Seg::Cubic(a, b, p) => Seg::Cubic(shift(a), shift(b), shift(p)),
                Seg::Close => Seg::Close,
            })
            .collect(),
    )
}

#[derive(Debug, Clone, Copy)]
enum Tok {
    Num(f32),
    Cmd(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn at(n: u64) -> Frame {
        frame(Mode::Animated, ms(n), None).expect("shown")
    }

    /// SPEC-0355 v2 AK-7: the storyboard's steps at their times.
    #[test]
    fn the_timeline_follows_the_storyboard() {
        // Step 1: the crab is built from nothing to all its cells by 0.45 s; no cursor yet, the
        // symbol centred, no wordmark.
        let start = at(0);
        assert_eq!(start.symbol, 0.0);
        assert!(!start.cursor);
        assert_eq!(start.shift, START_SHIFT);
        assert!(start.mask.1 <= start.mask.0, "no wordmark");
        let mid = at(225);
        assert!(mid.symbol > 0.3 && mid.symbol < 0.7, "{mid:?}");
        assert_eq!(at(450).symbol, 1.0);
        let mut last = 0.0;
        for t in (0..=450).step_by(15) {
            let b = at(t).symbol;
            assert!(b >= last, "the crab is only built up");
            last = b;
        }
        // Step 2: the cursor appears and blinks once.
        assert!(at(460).cursor);
        assert!(!at(550).cursor, "the blink");
        assert!(at(600).cursor);
        assert_eq!(
            at(640).shift,
            START_SHIFT,
            "the symbol waits for the cursor"
        );
        // Step 3: the symbol moves left while the mask opens left to right.
        let moving = at(800);
        assert!(moving.shift > 0.0 && moving.shift < START_SHIFT);
        assert_eq!(at(900).shift, 0.0);
        let mut right = 0.0;
        for t in (650..=1100).step_by(10) {
            let f = at(t);
            assert!(f.mask.1 >= right, "the mask only opens rightwards");
            right = f.mask.1;
            assert!(
                f.mask.0 >= WORDMARK_X + f.shift - 1e-3,
                "never behind the symbol"
            );
        }
        // Step 4: the horizontal logo rests.
        assert_eq!(at(1100), Frame::FINAL);
        assert_eq!(at(1300), Frame::FINAL);
    }

    /// The crab is built row by row from the top, each row outwards from the axis, and ends as
    /// exactly the master's cells.
    #[test]
    fn the_crab_is_built_row_by_row_and_ends_as_the_master() {
        let brand = brand().expect("the masters are read");
        let cells = &brand.cells;
        assert!(symbol_rects(cells, 0.0).is_empty());
        // The first row (the tips of the claws) grows from the axis to both sides at once.
        let first = symbol_rects(cells, 0.02);
        assert!(first.iter().all(|c| c[1] == 6.5), "{first:?}");
        let half = symbol_rects(cells, 0.5);
        let rows = |r: &[[f32; 4]]| {
            let mut ys: Vec<f32> = r.iter().map(|c| c[1]).collect();
            ys.dedup();
            ys.len()
        };
        assert!((9..=10).contains(&rows(&half)), "about half the 19 rows");
        assert!(half.iter().all(|c| c[1] < 16.5), "from the top down");
        let mut built = 0usize;
        for p in 0..=100 {
            let n: usize = symbol_rects(cells, p as f32 / 100.0)
                .iter()
                .map(|c| ((c[2] - c[0]) * 1000.0) as usize)
                .sum();
            assert!(n >= built, "the crab is only built up");
            built = n;
        }
        // Mirror image while building: as much left of the axis as right of it.
        for r in symbol_rects(cells, 0.37) {
            let mirror = [
                2.0 * SYMBOL_AXIS - r[2],
                r[1],
                2.0 * SYMBOL_AXIS - r[0],
                r[3],
            ];
            assert!(symbol_rects(cells, 0.37).contains(&mirror), "{r:?}");
        }
        // Complete, it is the master: 19 rows of cells between 2 and 30, 6.5 and 25.5.
        assert_eq!(&symbol_rects(cells, 1.0), cells);
        assert_eq!(rows(cells), 19);
        assert_eq!(brand.symbol.bounds(), [2.0, 6.5, 30.0, 25.5]);
        let union: f32 = cells.iter().map(|c| (c[2] - c[0]) * (c[3] - c[1])).sum();
        assert_eq!(union, 166.0, "the cells of the grid crab");
        // A curved or slanted outline is not a crab of cells.
        let curved = Shape {
            segs: path_segments("M0 0C1 1 2 2 3 3Z", 0.0).unwrap(),
            color: 0,
        };
        assert_eq!(cells_of(&curved), None);
        let slanted = Shape {
            segs: path_segments("M0 0L2 0L3 2L0 2Z", 0.0).unwrap(),
            color: 0,
        };
        assert_eq!(cells_of(&slanted), None);
    }

    /// SPEC-0355 v2 AK-7: the cap holds whatever the first window does, and the fade starts when
    /// it is ready.
    #[test]
    fn the_splash_ends_when_the_first_window_is_ready_and_never_after_the_cap() {
        // Ready at 0.3 s: fades for 150 ms, then closed.
        assert_eq!(closes_at(Mode::Animated, Some(ms(300))), Some(ms(450)));
        assert_eq!(
            frame(Mode::Animated, ms(299), Some(ms(300)))
                .unwrap()
                .opacity,
            1.0
        );
        let fading = frame(Mode::Animated, ms(375), Some(ms(300))).unwrap();
        assert!((fading.opacity - 0.5).abs() < 0.01, "{fading:?}");
        assert_eq!(frame(Mode::Animated, ms(450), Some(ms(300))), None);
        // Never ready, or ready late: closed at 1.5 s, the fade included.
        assert_eq!(closes_at(Mode::Animated, None), Some(CAP));
        assert_eq!(closes_at(Mode::Animated, Some(ms(5000))), Some(CAP));
        assert!(frame(Mode::Animated, ms(1400), None).unwrap().opacity < 1.0);
        assert_eq!(frame(Mode::Animated, CAP, None), None);
        assert_eq!(frame(Mode::Animated, ms(10_000), Some(ms(2000))), None);
        // Ready at once: only the fade.
        assert_eq!(closes_at(Mode::Animated, Some(Duration::ZERO)), Some(FADE));
        for t in (0..3000).step_by(7) {
            for ready in [
                None,
                Some(ms(0)),
                Some(ms(700)),
                Some(ms(1400)),
                Some(ms(9000)),
            ] {
                if frame(Mode::Animated, ms(t), ready).is_some() {
                    assert!(ms(t) < CAP);
                }
                if frame(Mode::Still, ms(t), ready).is_some() {
                    assert!(ms(t) < CAP);
                }
            }
        }
    }

    /// SPEC-0355 v2 AK-7: Reduce Motion shows the final logo still, as long as start-up runs; the
    /// setting switched off (or no brand) shows nothing.
    #[test]
    fn reduce_motion_and_the_setting() {
        assert_eq!(Mode::choose(true, false, true), Mode::Animated);
        assert_eq!(Mode::choose(true, true, true), Mode::Still);
        assert_eq!(Mode::choose(false, false, true), Mode::Off);
        assert_eq!(Mode::choose(false, true, true), Mode::Off);
        assert_eq!(Mode::choose(true, false, false), Mode::Off);
        for t in [0, 10, 500, 1000, 1499] {
            assert_eq!(frame(Mode::Still, ms(t), None), Some(Frame::FINAL));
            assert_eq!(frame(Mode::Off, ms(t), None), None);
        }
        assert_eq!(
            closes_at(Mode::Still, Some(ms(200))),
            Some(ms(200)),
            "no fade"
        );
        assert_eq!(frame(Mode::Still, ms(200), Some(ms(200))), None);
        assert_eq!(closes_at(Mode::Still, None), Some(CAP));
        assert_eq!(closes_at(Mode::Off, None), None);
    }

    /// The masters are read: the horizontal layout puts the wordmark 4.5 units after the crab,
    /// its paths are the wordmark's own, and the colours are the brand tokens.
    #[test]
    fn the_brand_is_read_from_the_masters() {
        let b = brand().expect("read");
        assert_eq!(b.background, 0x2E3440);
        assert_eq!(b.symbol.color, 0xECEFF4);
        assert_eq!(b.cursor.color, 0x88C0D0);
        assert_eq!(b.cursor.bounds(), [15.5, 16.5, 16.5, 19.5]);
        assert_eq!(b.wordmark.len(), 2);
        assert_eq!(b.wordmark[0].color, 0xECEFF4, "Ferris");
        assert_eq!(b.wordmark[1].color, 0x88C0D0, "IDEA");
        let ferris = b.wordmark[0].bounds();
        assert_eq!(ferris[0], WORDMARK_X + WORDMARK_INSET);
        assert_eq!(ferris[0] - b.symbol.bounds()[2], 4.5, "4.5 units apart");
        let idea = b.wordmark[1].bounds();
        assert!(idea[0] > ferris[2]);
        assert!(
            (LOGO_WIDTH - idea[2] - WORDMARK_INSET).abs() < 0.01,
            "{idea:?}"
        );
        // Cap height 10, from 11 to the baseline at 21, centred on the crab.
        assert_eq!([idea[1], idea[3]], [11.0, 21.0]);
        // The horizontal logo's wordmark is the wordmark master's.
        let horizontal = svg_path_data(ferris_idea_ui::brand::HORIZONTAL_LOGO_SVG);
        let wordmark = svg_path_data(ferris_idea_ui::brand::WORDMARK_SVG);
        assert_eq!(horizontal[2..], wordmark[..]);
        assert!(
            ferris_idea_ui::brand::HORIZONTAL_LOGO_SVG.contains("viewBox=\"0 0 109 32\""),
            "the logo's size"
        );
        // Unknown commands are refused rather than guessed.
        assert_eq!(path_segments("m1 1l2 2", 0.0), None);
        // No trace of the old Ferris IDEA masters: the bracket and the "IDE" / "Craft" glyphs.
        for svg in [
            ferris_idea_ui::brand::HORIZONTAL_LOGO_SVG,
            ferris_idea_ui::brand::WORDMARK_SVG,
            ferris_idea_ui::brand::SYMBOL_DARK_SVG,
        ] {
            assert!(
                !svg.contains("M28 4H4V28H28V23H10V9H28V4Z"),
                "the old bracket"
            );
            assert!(!svg.contains("M4 6H8V26H4Z"), "the old \"IDE\"");
        }
        assert_eq!(
            read_brand("<svg></svg>", ferris_idea_ui::brand::SYMBOL_DARK_SVG),
            None
        );
        assert_eq!(
            path_segments("M1 2H3V4C5 6 7 8 9 10Z", 1.0),
            Some(vec![
                Seg::Move((2.0, 2.0)),
                Seg::Line((4.0, 2.0)),
                Seg::Line((4.0, 4.0)),
                Seg::Cubic((6.0, 6.0), (8.0, 8.0), (10.0, 10.0)),
                Seg::Close
            ])
        );
    }
}

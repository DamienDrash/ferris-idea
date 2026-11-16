//! Three-way text merge for the merge editor (`SPEC-0037`).

use serde::Serialize;
use similar::DiffOp;

/// How a changed chunk came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChunkKind {
    /// Unchanged on both sides.
    Stable,
    /// Changed by us only.
    Ours,
    /// Changed by them only.
    Theirs,
    /// Changed identically by both.
    Both,
    /// Changed differently by both.
    Conflict,
}

/// How a conflict was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "take", content = "text")]
pub enum Resolution {
    /// Our lines.
    Ours,
    /// Their lines.
    Theirs,
    /// Our lines followed by theirs.
    OursThenTheirs,
    /// Their lines followed by ours.
    TheirsThenOurs,
    /// The base lines.
    Base,
    /// Hand-written text.
    Custom(String),
}

/// One region of the merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Chunk {
    /// Classification.
    pub kind: ChunkKind,
    /// Base lines (with line endings).
    pub base: Vec<String>,
    /// Our lines.
    pub ours: Vec<String>,
    /// Their lines.
    pub theirs: Vec<String>,
    /// Resolution of a conflict, if any.
    pub resolution: Option<Resolution>,
}

impl Chunk {
    fn stable(lines: Vec<String>) -> Self {
        Self {
            kind: ChunkKind::Stable,
            base: lines.clone(),
            ours: lines.clone(),
            theirs: lines,
            resolution: None,
        }
    }

    /// The lines this chunk contributes to the result, `None` for an open conflict.
    pub fn result(&self) -> Option<String> {
        let join = |l: &[String]| l.concat();
        Some(match (&self.kind, &self.resolution) {
            (_, Some(Resolution::Ours)) => join(&self.ours),
            (_, Some(Resolution::Theirs)) => join(&self.theirs),
            (_, Some(Resolution::Base)) => join(&self.base),
            (_, Some(Resolution::OursThenTheirs)) => {
                with_break(join(&self.ours)) + &join(&self.theirs)
            }
            (_, Some(Resolution::TheirsThenOurs)) => {
                with_break(join(&self.theirs)) + &join(&self.ours)
            }
            (_, Some(Resolution::Custom(text))) => text.clone(),
            (ChunkKind::Stable | ChunkKind::Ours | ChunkKind::Both, None) => join(&self.ours),
            (ChunkKind::Theirs, None) => join(&self.theirs),
            (ChunkKind::Conflict, None) => return None,
        })
    }
}

/// Appends a line break unless the text is empty or already ends with one.
fn with_break(mut text: String) -> String {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Conflict marker layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkerStyle {
    /// Ours and theirs.
    #[default]
    Merge,
    /// Ours, base and theirs.
    Diff3,
}

/// Names written after the conflict markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels {
    /// After `<<<<<<<`.
    pub ours: String,
    /// After `|||||||`.
    pub base: String,
    /// After `>>>>>>>`.
    pub theirs: String,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            ours: "ours".into(),
            base: "base".into(),
            theirs: "theirs".into(),
        }
    }
}

/// A three-way merge of one text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Merge3 {
    /// All regions in order.
    pub chunks: Vec<Chunk>,
    /// Whether the first line of the base ends with CR LF, `None` when it cannot be told (v3
    /// behaviour 9).
    #[serde(skip)]
    base_crlf: Option<bool>,
}

/// Whether line `i` of `lines` ends with CR LF, as far as that tells the file's line ends: the last
/// line without a line break is judged by the line before it, and an empty text or a single line
/// without a break tells nothing (v3 behaviour 9).
fn line_crlf(lines: &[&str], i: usize) -> Option<bool> {
    let line = lines.get(i.min(lines.len().checked_sub(1)?))?;
    if line.ends_with('\n') {
        return Some(line.ends_with("\r\n"));
    }
    let before = lines.get(lines.len().checked_sub(2)?)?;
    Some(before.ends_with("\r\n"))
}

fn lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_owned).collect()
}

/// For each line of `old`, the line of `new` it is kept as, if unchanged. Diffs the lines as
/// [`lines`] cuts them (only at `\n`), so the indices fit them even when a text holds a lone `\r`.
fn kept(old: &[String], new: &[String]) -> Vec<Option<usize>> {
    let mut map = vec![None; old.len()];
    let (old, new): (Vec<&str>, Vec<&str>) = (
        old.iter().map(String::as_str).collect(),
        new.iter().map(String::as_str).collect(),
    );
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &old, &new) {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        {
            for k in 0..len {
                map[old_index + k] = Some(new_index + k);
            }
        }
    }
    map
}

impl Merge3 {
    /// Merges `ours` and `theirs`, both derived from `base` (behaviours 1–3).
    pub fn new(base: &str, ours: &str, theirs: &str) -> Self {
        Self::build(base, ours, theirs, true)
    }

    /// Like [`Merge3::new`] without trimming conflicts, as Git does for the diff3 style.
    pub fn untrimmed(base: &str, ours: &str, theirs: &str) -> Self {
        Self::build(base, ours, theirs, false)
    }

    fn build(base: &str, ours: &str, theirs: &str, zealous: bool) -> Self {
        let (b, o, t) = (lines(base), lines(ours), lines(theirs));
        let om = kept(&b, &o);
        let tm = kept(&b, &t);
        let mut chunks: Vec<Chunk> = Vec::new();
        let (mut i, mut oi, mut ti) = (0, 0, 0);
        let mut stable: Vec<String> = Vec::new();
        while i < b.len() || oi < o.len() || ti < t.len() {
            if i < b.len() && om[i] == Some(oi) && tm[i] == Some(ti) {
                stable.push(b[i].clone());
                (i, oi, ti) = (i + 1, oi + 1, ti + 1);
                continue;
            }
            // Next base line kept by both sides, past the current positions.
            let next = (i..b.len()).find_map(|k| match (om[k], tm[k]) {
                (Some(x), Some(y)) if x >= oi && y >= ti => Some((k, x, y)),
                _ => None,
            });
            let (ni, no, nt) = next.unwrap_or((b.len(), o.len(), t.len()));
            let (cb, co, ct) = (&b[i..ni], &o[oi..no], &t[ti..nt]);
            let kind = if co == ct {
                ChunkKind::Both
            } else if co == cb {
                ChunkKind::Theirs
            } else if ct == cb {
                ChunkKind::Ours
            } else {
                ChunkKind::Conflict
            };
            if kind == ChunkKind::Conflict {
                // Zealous: common leading and trailing lines are not part of the conflict.
                let head = if zealous {
                    co.iter().zip(ct).take_while(|(a, b)| a == b).count()
                } else {
                    0
                };
                let tail = if !zealous {
                    0
                } else {
                    co[head..]
                        .iter()
                        .rev()
                        .zip(ct[head..].iter().rev())
                        .take_while(|(a, b)| a == b)
                        .count()
                };
                stable.extend_from_slice(&co[..head]);
                if !stable.is_empty() {
                    chunks.push(Chunk::stable(std::mem::take(&mut stable)));
                }
                chunks.push(Chunk {
                    kind,
                    base: cb.to_vec(),
                    ours: co[head..co.len() - tail].to_vec(),
                    theirs: ct[head..ct.len() - tail].to_vec(),
                    resolution: None,
                });
                stable.extend_from_slice(&co[co.len() - tail..]);
            } else {
                if !stable.is_empty() {
                    chunks.push(Chunk::stable(std::mem::take(&mut stable)));
                }
                chunks.push(Chunk {
                    kind,
                    base: cb.to_vec(),
                    ours: co.to_vec(),
                    theirs: ct.to_vec(),
                    resolution: None,
                });
            }
            (i, oi, ti) = (ni, no, nt);
        }
        if !stable.is_empty() {
            chunks.push(Chunk::stable(stable));
        }
        let b: Vec<&str> = b.iter().map(String::as_str).collect();
        Self {
            chunks,
            base_crlf: line_crlf(&b, 0),
        }
    }

    /// A merge of given regions, for a file that is not merged line by line (`SPEC-0101` v6
    /// behaviour 12).
    pub fn from_chunks(chunks: Vec<Chunk>) -> Self {
        Self {
            chunks,
            base_crlf: None,
        }
    }

    /// The line end of the markers of the open conflict `index`: CR LF when the line before the
    /// conflict (the first line when it starts the file) ends so in ours and in theirs, with the
    /// base's first line deciding where a side cannot tell (v3 behaviour 9).
    fn marker_break(&self, index: usize) -> &'static str {
        let side = |pick: fn(&Chunk) -> &Vec<String>| {
            let before: usize = self.chunks[..index].iter().map(|c| pick(c).len()).sum();
            let all: Vec<&str> = self
                .chunks
                .iter()
                .flat_map(|c| pick(c).iter().map(String::as_str))
                .collect();
            line_crlf(&all, before.saturating_sub(1))
        };
        let mut crlf = side(|c| &c.ours);
        if crlf != Some(false) {
            crlf = side(|c| &c.theirs);
        }
        if crlf != Some(false) {
            crlf = self.base_crlf;
        }
        if crlf == Some(true) { "\r\n" } else { "\n" }
    }

    /// Indices of the conflict chunks.
    pub fn conflicts(&self) -> Vec<usize> {
        (0..self.chunks.len())
            .filter(|&i| self.chunks[i].kind == ChunkKind::Conflict)
            .collect()
    }

    /// Number of conflicts without a resolution.
    pub fn open_conflicts(&self) -> usize {
        self.chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Conflict && c.resolution.is_none())
            .count()
    }

    /// Resolves (or with `None` reopens) chunk `index`; returns false for an unknown index.
    pub fn resolve(&mut self, index: usize, resolution: Option<Resolution>) -> bool {
        match self.chunks.get_mut(index) {
            Some(chunk) => {
                chunk.resolution = resolution;
                true
            }
            None => false,
        }
    }

    /// Number of open conflicts that are simple (v2 behaviour 8).
    pub fn simple_conflicts(&self) -> usize {
        self.chunks
            .iter()
            .filter(|c| {
                c.kind == ChunkKind::Conflict
                    && c.resolution.is_none()
                    && simple_resolution(c).is_some()
            })
            .count()
    }

    /// The word-by-word merge of conflict chunk `index` when it is simple (v2 behaviour 8), whether
    /// it is resolved or not; `None` for any other chunk.
    pub fn simple_text(&self, index: usize) -> Option<String> {
        self.chunks
            .get(index)
            .filter(|c| c.kind == ChunkKind::Conflict)
            .and_then(simple_resolution)
    }

    /// Resolves every open simple conflict with the word-by-word merge (v2 behaviour 8); returns
    /// how many were resolved.
    pub fn resolve_simple(&mut self) -> usize {
        let mut n = 0;
        for chunk in &mut self.chunks {
            if chunk.kind == ChunkKind::Conflict
                && chunk.resolution.is_none()
                && let Some(text) = simple_resolution(chunk)
            {
                chunk.resolution = Some(Resolution::Custom(text));
                n += 1;
            }
        }
        n
    }

    /// The merged text; open conflicts get markers (behaviour 5) that end as the file's lines do
    /// (v3 behaviour 9).
    pub fn text(&self, labels: &Labels, style: MarkerStyle) -> String {
        let mut out = String::new();
        for (index, chunk) in self.chunks.iter().enumerate() {
            if let Some(text) = chunk.result() {
                out.push_str(&text);
                continue;
            }
            let eol = self.marker_break(index);
            let body = |out: &mut String, lines: &[String]| {
                let text = lines.concat();
                out.push_str(&text);
                if !text.is_empty() && !text.ends_with('\n') {
                    out.push_str(eol);
                }
            };
            out.push_str(&format!("<<<<<<< {}{eol}", labels.ours));
            body(&mut out, &chunk.ours);
            if style == MarkerStyle::Diff3 {
                out.push_str(&format!("||||||| {}{eol}", labels.base));
                body(&mut out, &chunk.base);
            }
            out.push_str(&format!("======={eol}"));
            body(&mut out, &chunk.theirs);
            out.push_str(&format!(">>>>>>> {}{eol}", labels.theirs));
        }
        out
    }
}

/// Whether `text` still holds a conflict as the markers write it: a line starting with `<<<<<<<`
/// and a space or the line end, later a line that is exactly `=======`, later a line starting with
/// `>>>>>>>` and a space or the line end; a CR before the line end is ignored (v3 behaviour 10).
pub fn has_conflict_markers(text: &str) -> bool {
    let opens = |line: &str, marker: &str| {
        line.strip_prefix(marker)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    };
    let mut found = 0;
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let next = match found {
            0 => opens(line, "<<<<<<<"),
            1 => line == "=======",
            _ => opens(line, ">>>>>>>"),
        };
        if next {
            found += 1;
            if found == 3 {
                return true;
            }
        }
    }
    false
}

/// Tokens for the word-by-word merge: runs of letters, digits and `_`; runs of spaces and tabs; a
/// line break; any other single character (v2 behaviour 8).
fn tokens(text: &str) -> Vec<&str> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Blank,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c == ' ' || c == '\t' {
            Class::Blank
        } else {
            Class::Other
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let k = class(c);
        let mut end = i + c.len_utf8();
        if k != Class::Other {
            while let Some(&(j, d)) = chars.peek() {
                if class(d) != k {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
        }
        debug_assert_eq!(start, i);
        out.push(&text[i..end]);
        start = end;
    }
    out
}

/// A change of one side: base tokens `start..end` replaced by `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

/// The changes turning `base` into `side`, neighbouring changes joined.
fn edits(base: &[&str], side: &[&str]) -> Vec<Edit> {
    let mut out: Vec<Edit> = Vec::new();
    let mut joined = false;
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, base, side) {
        if let DiffOp::Equal { .. } = op {
            joined = false;
            continue;
        }
        let (old, new) = (op.old_range(), op.new_range());
        let text = side[new].concat();
        match out.last_mut() {
            Some(last) if joined => {
                last.end = old.end;
                last.text.push_str(&text);
            }
            _ => out.push(Edit {
                start: old.start,
                end: old.end,
                text,
            }),
        }
        joined = true;
    }
    out
}

/// The word-by-word merge of a conflict when ours and theirs change different words, with an
/// unchanged token between, or overlap identically; `None` otherwise (v2 behaviour 8).
fn simple_resolution(chunk: &Chunk) -> Option<String> {
    let (base, ours, theirs) = (
        chunk.base.concat(),
        chunk.ours.concat(),
        chunk.theirs.concat(),
    );
    let b = tokens(&base);
    let mut all = edits(&b, &tokens(&ours));
    for e in edits(&b, &tokens(&theirs)) {
        if all.contains(&e) {
            continue;
        }
        if all.iter().any(|a| a.start <= e.end && e.start <= a.end) {
            return None;
        }
        all.push(e);
    }
    all.sort_by_key(|e| (e.start, e.end));
    let mut out = String::new();
    let mut pos = 0;
    for e in &all {
        out.push_str(&b[pos..e.start].concat());
        out.push_str(&e.text);
        pos = e.end;
    }
    out.push_str(&b[pos..].concat());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(m: &Merge3) -> Vec<ChunkKind> {
        m.chunks.iter().map(|c| c.kind).collect()
    }

    /// M8.2 fuzz finding: a lone `\r` made the line diff cut more lines than the merge indexed.
    #[test]
    fn lone_carriage_returns() {
        let labels = Labels {
            ours: "o".into(),
            base: "b".into(),
            theirs: "t".into(),
        };
        let base = "{\u{8b}?\r8 ";
        let side = "\r8 ";
        for m in [
            Merge3::new(base, side, "\r8 |"),
            Merge3::new("a\rb\rc\n", "a\rB\rc\n", "a\rb\rC\n"),
            Merge3::untrimmed("\n\u{1}=======\r\n", "\n\u{1}=======\r=\r\n", "x\r"),
        ] {
            let _ = m.text(&labels, MarkerStyle::Merge);
        }
        let m = Merge3::new("a\rb\n", "a\rb\n", "a\rc\n");
        assert_eq!(m.open_conflicts(), 0);
        assert_eq!(m.text(&labels, MarkerStyle::Merge), "a\rc\n");
    }

    #[test]
    fn classification() {
        use ChunkKind::*;
        let base = "a\nb\nc\nd\ne\n";
        let m = Merge3::new(base, "a\nB\nc\nd\ne\n", "a\nb\nc\nD\ne\n");
        assert_eq!(kinds(&m), [Stable, Ours, Stable, Theirs, Stable]);
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "a\nB\nc\nD\ne\n"
        );

        let m = Merge3::new(base, "a\nX\nc\nd\ne\n", "a\nX\nc\nd\ne\n");
        assert_eq!(kinds(&m), [Stable, Both, Stable]);

        let m = Merge3::new(base, "a\nO\nc\nd\ne\n", "a\nT\nc\nd\ne\n");
        assert_eq!(kinds(&m), [Stable, Conflict, Stable]);
        assert_eq!(m.open_conflicts(), 1);

        let m = Merge3::new(base, "start\na\nb\nc\nd\ne\n", "a\nb\nc\nd\ne\nend\n");
        assert_eq!(kinds(&m), [Ours, Stable, Theirs]);
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "start\na\nb\nc\nd\ne\nend\n"
        );

        let m = Merge3::new(base, "a\nc\nd\ne\n", base);
        assert_eq!(kinds(&m), [Stable, Ours, Stable]);
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "a\nc\nd\ne\n"
        );

        let m = Merge3::new("", "x\n", "y\n");
        assert_eq!(kinds(&m), [Conflict]);
    }

    #[test]
    fn zealous_trimming() {
        let m = Merge3::new("a\nb\nc\n", "a\nsame\nO\nend\nc\n", "a\nsame\nT\nend\nc\n");
        let c = &m.chunks[m.conflicts()[0]];
        assert_eq!(
            (c.ours.concat(), c.theirs.concat()),
            ("O\n".into(), "T\n".into())
        );
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "a\nsame\n<<<<<<< ours\nO\n=======\nT\n>>>>>>> theirs\nend\nc\n"
        );
    }

    #[test]
    fn resolving() {
        let mut m = Merge3::new("a\nb\nc\n", "a\nO\nc\n", "a\nT\nc\n");
        let i = m.conflicts()[0];
        let labels = Labels::default();
        for (resolution, expected) in [
            (Resolution::Ours, "a\nO\nc\n"),
            (Resolution::Theirs, "a\nT\nc\n"),
            (Resolution::OursThenTheirs, "a\nO\nT\nc\n"),
            (Resolution::TheirsThenOurs, "a\nT\nO\nc\n"),
            (Resolution::Base, "a\nb\nc\n"),
            (Resolution::Custom("mine\n".into()), "a\nmine\nc\n"),
        ] {
            assert!(m.resolve(i, Some(resolution)));
            assert_eq!(m.open_conflicts(), 0);
            assert_eq!(m.text(&labels, MarkerStyle::Merge), expected);
        }
        assert!(m.resolve(i, None));
        assert_eq!(m.open_conflicts(), 1);
        assert!(!m.resolve(99, None));
    }

    #[test]
    fn missing_final_newline() {
        let m = Merge3::new("a\nb", "A\nb", "a\nb");
        assert_eq!(m.text(&Labels::default(), MarkerStyle::Merge), "A\nb");
        let m = Merge3::new("a", "o", "t");
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "<<<<<<< ours\no\n=======\nt\n>>>>>>> theirs\n"
        );
    }

    #[test]
    fn simple_conflicts_merge_word_by_word() {
        // Different words of one line: simple.
        let mut m = Merge3::new(
            "let total = price * count;\n",
            "let sum = price * count;\n",
            "let total = price * amount;\n",
        );
        assert_eq!(m.open_conflicts(), 1);
        assert_eq!(m.simple_conflicts(), 1);
        assert_eq!(m.resolve_simple(), 1);
        assert_eq!(m.open_conflicts(), 0);
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "let sum = price * amount;\n"
        );
        // Neighbouring words without an unchanged token between: not simple.
        let m = Merge3::new("a.b\n", "x.b\n", "a.y\n");
        assert_eq!(m.simple_conflicts(), 1, "`.` stays unchanged between");
        let mut m = Merge3::new("ab cd\n", "xy cd\n", "ab-cd\n");
        assert_eq!(
            m.simple_conflicts(),
            0,
            "the blank is changed by theirs, touching ours"
        );
        assert_eq!(m.resolve_simple(), 0);
        // Same word changed differently: not simple.
        let m = Merge3::new("a = 1\n", "a = 2\n", "a = 3\n");
        assert_eq!(m.simple_conflicts(), 0);
        // Insertions at the same place differ: not simple; identical overlap merges.
        let m = Merge3::new("f(a)\n", "f(a, b)\n", "f(a, c)\n");
        assert_eq!(m.simple_conflicts(), 0);
        let mut m = Merge3::new("f(a) g\n", "f(a, b) h\n", "f(a, b) g!\n");
        assert_eq!(m.resolve_simple(), 0, "h and g! touch");
        let mut m = Merge3::new("one two three\n", "one 2 three 3\n", "one 2 three\n");
        assert_eq!(
            m.resolve_simple(),
            1,
            "identical change plus one side's own"
        );
        assert_eq!(
            m.text(&Labels::default(), MarkerStyle::Merge),
            "one 2 three 3\n"
        );
        let mut m = Merge3::new("x one two\n", "y one 2\n", "x one 2\n");
        assert_eq!(m.resolve_simple(), 1);
        assert_eq!(m.text(&Labels::default(), MarkerStyle::Merge), "y one 2\n");
    }

    /// SPEC-0037 AK-8 (v3).
    #[test]
    fn left_over_markers() {
        let open = Merge3::new("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n");
        for style in [MarkerStyle::Merge, MarkerStyle::Diff3] {
            assert!(has_conflict_markers(&open.text(&Labels::default(), style)));
        }
        assert!(has_conflict_markers(
            "x\r\n<<<<<<< HEAD\r\na\r\n=======\r\nb\r\n>>>>>>> topic\r\n"
        ));
        assert!(has_conflict_markers("<<<<<<<\n=======\n>>>>>>>"));
        // A heading underline, a quoted marker, the wrong order and longer runs are none.
        assert!(!has_conflict_markers("Title\n=======\ntext\n"));
        assert!(!has_conflict_markers("<<<<<<< HEAD\nonly the start\n"));
        assert!(!has_conflict_markers(">>>>>>> b\n=======\n<<<<<<< a\n"));
        assert!(!has_conflict_markers("<<<<<<<< a\n=======\n>>>>>>>> b\n"));
        assert!(!has_conflict_markers(
            "  <<<<<<< a\n  =======\n  >>>>>>> b\n"
        ));
        assert!(!has_conflict_markers("<<<<<<< a\n======= x\n>>>>>>> b\n"));
        let mut resolved = open;
        resolved.resolve(1, Some(Resolution::OursThenTheirs));
        assert!(!has_conflict_markers(
            &resolved.text(&Labels::default(), MarkerStyle::Merge)
        ));
    }
}

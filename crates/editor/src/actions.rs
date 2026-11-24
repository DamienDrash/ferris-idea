//! Editing actions on lines, comments, indentation, brackets and line breaks (`SPEC-0056`).
//!
//! Every action applies to all carets and is one undo step.

use std::ops::Range;

use crate::buffer::{Position, TextBuffer};
use crate::selection::{Caret, Carets};

/// How line comments are written (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentStyle {
    /// Line comment token, e.g. `//`, `#`, `--`.
    pub token: String,
    /// Put the token at column 0 (the reference's default) instead of at the block's indentation.
    pub first_column: bool,
    /// Insert a space after the token.
    pub add_space: bool,
}

impl CommentStyle {
    /// `token` at the first column without a space; a token that ends with a space (`# `) is
    /// written with that space and removed with or without it (`SPEC-0004` v12 behaviour 23).
    pub fn new(token: &str) -> Self {
        let trimmed = token.trim_end_matches(' ');
        Self {
            token: trimmed.to_owned(),
            first_column: true,
            add_space: trimmed.len() < token.len() && !trimmed.is_empty(),
        }
    }
}

/// Bracket and quote pairs closed automatically (behaviour 7).
pub const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];

pub(crate) fn line_start(buf: &TextBuffer, line: usize) -> usize {
    if line >= buf.len_lines() {
        return buf.len_chars();
    }
    buf.position_to_offset(Position::new(line, 0))
        .unwrap_or(buf.len_chars())
}

pub(crate) fn line_of(buf: &TextBuffer, offset: usize) -> usize {
    buf.offset_to_position(offset.min(buf.len_chars()))
        .map(|p| p.line)
        .unwrap_or(0)
}

pub(crate) fn line_text(buf: &TextBuffer, line: usize) -> String {
    buf.line(line).unwrap_or_default()
}

/// Lines a caret covers; a selection ending at column 0 does not include that line.
fn caret_lines(buf: &TextBuffer, c: &Caret) -> Range<usize> {
    let r = c.range();
    let first = line_of(buf, r.start);
    let mut last = line_of(buf, r.end);
    if !c.is_empty() && last > first && line_start(buf, last) == r.end {
        last -= 1;
    }
    first..last + 1
}

/// Merged, sorted line blocks of all carets.
fn blocks(buf: &TextBuffer, carets: &Carets) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut all: Vec<Range<usize>> = carets
        .carets()
        .iter()
        .map(|c| caret_lines(buf, c))
        .collect();
    all.sort_by_key(|r| r.start);
    for r in all {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// Applies non-overlapping edits (sorted by start) as one undo step.
pub(crate) fn apply(buf: &mut TextBuffer, edits: &[(Range<usize>, String)]) {
    buf.begin_group();
    for (range, text) in edits.iter().rev() {
        buf.replace(range.clone(), text)
            .expect("edit ranges lie inside the buffer");
    }
    buf.end_group();
}

/// Where `offset` ends up after `edits` (sorted, non-overlapping); offsets inside a replaced range
/// move to its end unless `stick_start`.
pub(crate) fn map_offset(
    offset: usize,
    edits: &[(Range<usize>, String)],
    stick_start: bool,
) -> usize {
    let mut shift: isize = 0;
    for (range, text) in edits {
        let len = text.chars().count() as isize;
        if offset < range.start || (offset == range.start && (stick_start || range.is_empty())) {
            break;
        }
        if offset < range.end {
            return (range.start as isize + shift + if stick_start { 0 } else { len }) as usize;
        }
        shift += len - range.len() as isize;
    }
    (offset as isize + shift) as usize
}

fn remap(carets: &Carets, edits: &[(Range<usize>, String)]) -> Carets {
    let mut next = carets.clone();
    next.moved_to(
        carets
            .carets()
            .iter()
            .map(|c| {
                Caret::select(
                    map_offset(c.anchor, edits, false),
                    map_offset(c.head, edits, false),
                )
            })
            .collect(),
    );
    next
}

fn line_break(buf: &TextBuffer) -> &'static str {
    buf.line_ending().as_str()
}

/// Duplicates each selection after itself, or each caret's line below it; carets move to the copy
/// (behaviour 1).
pub fn duplicate(buf: &mut TextBuffer, carets: &mut Carets) {
    let nl = line_break(buf);
    let mut edits = Vec::new();
    let mut moved = Vec::new();
    for c in carets.carets() {
        if c.is_empty() {
            let line = line_of(buf, c.head);
            let start = line_start(buf, line);
            let text = line_text(buf, line);
            let column = c.head - start;
            let end = start + text.chars().count();
            let insert = format!("{nl}{text}");
            edits.push((end..end, insert));
            moved.push((end, column, true));
        } else {
            let r = c.range();
            let text = buf.slice(r.clone()).unwrap_or_default();
            edits.push((r.end..r.end, text));
            moved.push((r.end, r.len(), false));
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    edits.dedup_by_key(|(r, _)| r.start);
    apply(buf, &edits);
    let mut out = Vec::new();
    let nl_len = nl.chars().count();
    for (at, n, line) in moved {
        let base = map_offset(at, &edits, true);
        out.push(if line {
            Caret::at(base + nl_len + n)
        } else {
            Caret::select(base, base + n)
        });
    }
    carets.moved_to(out);
}

/// Deletes the lines the carets are on (behaviour 2).
pub fn delete_lines(buf: &mut TextBuffer, carets: &mut Carets) {
    let edits: Vec<(Range<usize>, String)> = blocks(buf, carets)
        .into_iter()
        .map(|b| {
            let mut start = line_start(buf, b.start);
            let mut end = line_start(buf, b.end);
            if b.end >= buf.len_lines() && start > 0 {
                // Last line: remove the break before it instead.
                end = buf.len_chars();
                start -= line_break(buf).chars().count().min(start);
            }
            (start..end, String::new())
        })
        .collect();
    let columns: Vec<(usize, usize)> = carets
        .carets()
        .iter()
        .map(|c| (c.head, c.head - line_start(buf, line_of(buf, c.head))))
        .collect();
    apply(buf, &edits);
    let out = columns
        .into_iter()
        .map(|(head, column)| {
            let at = map_offset(head, &edits, true);
            let line = line_of(buf, at);
            let start = line_start(buf, line);
            Caret::at(start + column.min(line_text(buf, line).chars().count()))
        })
        .collect();
    carets.moved_to(out);
}

/// Moves the lines the carets are on one line up or down, keeping carets and selections on them
/// (behaviour 3). Blocks at the document edge stay.
pub fn move_lines(buf: &mut TextBuffer, carets: &mut Carets, up: bool) {
    let all = blocks(buf, carets);
    let lines = buf.len_lines();
    if all
        .iter()
        .any(|b| if up { b.start == 0 } else { b.end >= lines })
    {
        return;
    }
    let mut text: Vec<String> = (0..lines).map(|l| line_text(buf, l)).collect();
    for b in if up {
        all.clone()
    } else {
        all.iter().rev().cloned().collect()
    } {
        if up {
            let moved = text.remove(b.start - 1);
            text.insert(b.end - 1, moved);
        } else {
            let moved = text.remove(b.end);
            text.insert(b.start, moved);
        }
    }
    let delta: isize = if up { -1 } else { 1 };
    let positions: Vec<(Position, Position)> = carets
        .carets()
        .iter()
        .map(|c| {
            let p = |o: usize| {
                let pos = buf.offset_to_position(o).unwrap_or_default();
                Position::new((pos.line as isize + delta) as usize, pos.column)
            };
            (p(c.anchor), p(c.head))
        })
        .collect();
    let nl = line_break(buf);
    let joined = text.join(nl);
    let edits = vec![(0..buf.len_chars(), joined)];
    apply(buf, &edits);
    let out = positions
        .into_iter()
        .map(|(a, h)| {
            Caret::select(
                buf.position_to_offset(a).unwrap_or(0),
                buf.position_to_offset(h).unwrap_or(0),
            )
        })
        .collect();
    carets.moved_to(out);
}

/// Joins each caret's line (or each selected block) with the next line: the line break and the
/// next line's indentation become one space, or nothing before a closing bracket (behaviour 4).
pub fn join_lines(buf: &mut TextBuffer, carets: &mut Carets) {
    let mut edits = Vec::new();
    for b in blocks(buf, carets) {
        let last = if b.end - b.start > 1 {
            b.end - 1
        } else {
            b.end
        };
        for line in b.start..last.max(b.start + 1) {
            if line + 1 >= buf.len_lines() {
                break;
            }
            let text = line_text(buf, line);
            let trimmed_end = text.trim_end().chars().count();
            let start = line_start(buf, line) + trimmed_end;
            let next = line_text(buf, line + 1);
            let indent = next.chars().take_while(|c| c.is_whitespace()).count();
            let end = line_start(buf, line + 1) + indent;
            let first = next.chars().nth(indent);
            let ends_open = matches!(text.trim_end().chars().last(), Some('(' | '['));
            let sep = if trimmed_end == 0
                || ends_open
                || first.is_none()
                || matches!(first, Some(')' | ']' | '}' | ',' | ';' | '.'))
            {
                ""
            } else {
                " "
            };
            edits.push((start..end, sep.to_owned()));
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    let caret_at: Vec<usize> = edits.iter().map(|(r, _)| r.start).collect();
    apply(buf, &edits);
    let out = caret_at
        .into_iter()
        .map(|at| Caret::at(map_offset(at, &edits, true)))
        .collect();
    carets.moved_to(out);
}

/// Comments the carets' lines, or uncomments them when every non-blank line is commented
/// (behaviour 5).
pub fn toggle_line_comment(buf: &mut TextBuffer, carets: &mut Carets, style: &CommentStyle) {
    let token = style.token.as_str();
    let mut edits = Vec::new();
    for b in blocks(buf, carets) {
        let lines: Vec<(usize, String)> =
            (b.start..b.end).map(|l| (l, line_text(buf, l))).collect();
        let non_blank: Vec<&(usize, String)> =
            lines.iter().filter(|(_, t)| !t.trim().is_empty()).collect();
        let commented = !non_blank.is_empty()
            && non_blank
                .iter()
                .all(|(_, t)| t.trim_start().starts_with(token));
        if commented {
            for (line, text) in &non_blank {
                let indent = text.chars().take_while(|c| c.is_whitespace()).count();
                let start = line_start(buf, *line) + indent;
                let mut n = token.chars().count();
                if style.add_space && text.chars().nth(indent + n) == Some(' ') {
                    n += 1;
                }
                edits.push((start..start + n, String::new()));
            }
        } else {
            let column = if style.first_column {
                0
            } else {
                non_blank
                    .iter()
                    .map(|(_, t)| t.chars().take_while(|c| c.is_whitespace()).count())
                    .min()
                    .unwrap_or(0)
            };
            let insert = if style.add_space {
                format!("{token} ")
            } else {
                token.to_owned()
            };
            let targets: Vec<&(usize, String)> = if non_blank.is_empty() {
                lines.iter().collect()
            } else {
                non_blank
            };
            for (line, _) in targets {
                let at = line_start(buf, *line) + column;
                edits.push((at..at, insert.clone()));
            }
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    let next = remap(carets, &edits);
    apply(buf, &edits);
    *carets = next;
}

/// Indents (or unindents) the carets' lines by one `unit` (behaviour 6). Unindent removes one unit
/// or, if there is less, the leading whitespace up to its width.
pub fn indent_lines(buf: &mut TextBuffer, carets: &mut Carets, unit: &str, outdent: bool) {
    let mut edits = Vec::new();
    let width = unit.chars().count();
    for b in blocks(buf, carets) {
        for line in b {
            let text = line_text(buf, line);
            let start = line_start(buf, line);
            if outdent {
                let n = if text.starts_with(unit) {
                    width
                } else if text.starts_with('\t') {
                    1
                } else {
                    text.chars().take(width).take_while(|c| *c == ' ').count()
                };
                if n > 0 {
                    edits.push((start..start + n, String::new()));
                }
            } else if !text.trim().is_empty() {
                edits.push((start..start, unit.to_owned()));
            }
        }
    }
    let next = remap(carets, &edits);
    apply(buf, &edits);
    *carets = next;
}

/// Options for [`smart_newline`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewlineOptions {
    /// One indentation step.
    pub unit: String,
    /// Characters after which the next line is indented one more step (e.g. `{`, `(`, `[`, `:`).
    pub indent_after: Vec<char>,
}

/// Breaks the line at each caret, keeping the indentation; after an opening bracket (or another
/// `indent_after` character) the new line gets one more step, and a closing bracket right after the
/// caret moves to its own line (behaviour 8).
pub fn smart_newline(buf: &mut TextBuffer, carets: &mut Carets, options: &NewlineOptions) {
    let nl = line_break(buf);
    let mut edits = Vec::new();
    let mut targets = Vec::new();
    for c in carets.carets() {
        let r = c.range();
        let line = line_of(buf, r.start);
        let start = line_start(buf, line);
        let text = line_text(buf, line);
        let indent: String = text
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let before: String = text.chars().take(r.start - start).collect();
        let after: String = text.chars().skip(r.end - start).collect();
        let last = before.trim_end().chars().last();
        let deeper = last.is_some_and(|l| options.indent_after.contains(&l));
        let closer = after.trim_start().chars().next();
        let pair = deeper
            && matches!(
                (last, closer),
                (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']'))
            );
        // Trailing whitespace before the caret and leading whitespace after it are dropped, except
        // inside the leading indentation: there the whitespace before the caret stays on the upper
        // line (SPEC-0056 v2).
        let cut_before = if before.trim().is_empty() {
            0
        } else {
            before.chars().count() - before.trim_end().chars().count()
        };
        let cut_after = after.chars().count() - after.trim_start().chars().count();
        let range = (r.start - cut_before)..(r.end + cut_after);
        let inner = if deeper {
            format!("{indent}{}", options.unit)
        } else {
            indent.clone()
        };
        let mut insert = format!("{nl}{inner}");
        let caret_in = range.start + insert.chars().count();
        if pair {
            insert.push_str(&format!("{nl}{indent}"));
        }
        targets.push((range.start, caret_in - range.start));
        edits.push((range, insert));
    }
    edits.sort_by_key(|(r, _)| r.start);
    apply(buf, &edits);
    let out = targets
        .into_iter()
        .map(|(at, n)| Caret::at(map_offset(at, &edits, true) + n))
        .collect();
    carets.moved_to(out);
}

fn closer_of(c: char) -> Option<char> {
    PAIRS.iter().find(|(o, _)| *o == c).map(|(_, cl)| *cl)
}

/// Types one character with bracket and quote handling (behaviour 7): an opener inserts its closer
/// when followed by whitespace, a closer or the line end; with a selection it surrounds it; typing a
/// closer that is already next to the caret steps over it.
pub fn type_char(buf: &mut TextBuffer, carets: &mut Carets, ch: char) {
    let len = buf.len_chars();
    let mut edits = Vec::new();
    let mut result = Vec::new();
    for c in carets.carets() {
        let r = c.range();
        let next = (r.end < len)
            .then(|| {
                buf.slice(r.end..r.end + 1)
                    .unwrap_or_default()
                    .chars()
                    .next()
            })
            .flatten();
        let prev = (r.start > 0)
            .then(|| {
                buf.slice(r.start - 1..r.start)
                    .unwrap_or_default()
                    .chars()
                    .next()
            })
            .flatten();
        let is_closer = PAIRS.iter().any(|(_, cl)| *cl == ch);
        if c.is_empty() && is_closer && next == Some(ch) {
            // Step over.
            result.push((r.start, 1usize, 0usize, false));
            continue;
        }
        if let Some(closer) = closer_of(ch) {
            let quote = ch == closer;
            if !c.is_empty() {
                let text = buf.slice(r.clone()).unwrap_or_default();
                edits.push((r.clone(), format!("{ch}{text}{closer}")));
                result.push((r.start, 1, text.chars().count(), true));
                continue;
            }
            let free_after =
                next.is_none_or(|n| n.is_whitespace() || PAIRS.iter().any(|(_, cl)| *cl == n));
            let word_before = prev.is_some_and(|p| p.is_alphanumeric() || p == '_');
            // `{` gets its closer on Enter instead (SPEC-0056 v3 behaviour 18).
            if free_after && !(quote && word_before) && ch != '{' {
                edits.push((r.clone(), format!("{ch}{closer}")));
                result.push((r.start, 1, 0, false));
                continue;
            }
        }
        edits.push((r.clone(), ch.to_string()));
        result.push((r.start, 1, 0, false));
    }
    let edit_starts: Vec<usize> = edits.iter().map(|(r, _)| r.start).collect();
    let mut sorted = edits.clone();
    sorted.sort_by_key(|(r, _)| r.start);
    apply(buf, &sorted);
    let out = result
        .into_iter()
        .map(|(at, step, sel, selection)| {
            let base = if edit_starts.contains(&at) {
                map_offset(at, &sorted, true)
            } else {
                map_offset(at, &sorted, false)
            };
            if selection {
                Caret::select(base + step, base + step + sel)
            } else {
                Caret::at(base + step)
            }
        })
        .collect();
    carets.moved_to(out);
}

/// Backspace that also removes the closer of an empty pair, e.g. `(|)` → `|` (behaviour 7).
pub fn backspace_pair(buf: &mut TextBuffer, carets: &mut Carets) {
    let len = buf.len_chars();
    let pairs: Vec<bool> = carets
        .carets()
        .iter()
        .map(|c| {
            c.is_empty() && c.head > 0 && c.head < len && {
                let two = buf.slice(c.head - 1..c.head + 1).unwrap_or_default();
                let mut it = two.chars();
                matches!((it.next(), it.next()), (Some(o), Some(cl)) if closer_of(o) == Some(cl))
            }
        })
        .collect();
    if !pairs.iter().any(|p| *p) {
        carets.backspace(buf);
        return;
    }
    let edits: Vec<(Range<usize>, String)> = carets
        .carets()
        .iter()
        .zip(&pairs)
        .map(|(c, pair)| {
            let r = if !c.is_empty() {
                c.range()
            } else if *pair {
                c.head - 1..c.head + 1
            } else {
                c.head.saturating_sub(1)..c.head
            };
            (r, String::new())
        })
        .collect();
    let starts: Vec<usize> = edits.iter().map(|(r, _)| r.start).collect();
    apply(buf, &edits);
    let out = starts
        .into_iter()
        .map(|s| Caret::at(map_offset(s, &edits, true)))
        .collect();
    carets.moved_to(out);
}

/// Offset of the bracket matching the one at `offset` (or just before it), skipping brackets inside
/// string literals on the way (behaviour 9).
pub fn matching_bracket(buf: &TextBuffer, offset: usize) -> Option<usize> {
    let text: Vec<char> = buf.text().chars().collect();
    let at = |i: usize| text.get(i).copied();
    let find = |i: usize| -> Option<usize> {
        let c = at(i)?;
        let (open, close, forward) = match c {
            '(' => ('(', ')', true),
            '[' => ('[', ']', true),
            '{' => ('{', '}', true),
            ')' => ('(', ')', false),
            ']' => ('[', ']', false),
            '}' => ('{', '}', false),
            _ => return None,
        };
        let mut depth = 0i32;
        let mut quote: Option<char> = None;
        let mut j = i as isize;
        while j >= 0 && (j as usize) < text.len() {
            let ch = text[j as usize];
            let escaped = j > 0 && text[j as usize - 1] == '\\';
            if let Some(q) = quote {
                if ch == q && !escaped {
                    quote = None;
                }
            } else if (ch == '"' || ch == '\'') && !escaped && j as usize != i {
                quote = Some(ch);
            } else if ch == open {
                depth += if forward { 1 } else { -1 };
            } else if ch == close {
                depth += if forward { -1 } else { 1 };
            }
            if depth == 0 {
                return Some(j as usize);
            }
            j += if forward { 1 } else { -1 };
        }
        None
    };
    find(offset).or_else(|| offset.checked_sub(1).and_then(find))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(text: &str) -> (TextBuffer, Carets) {
        // `|` marks a caret, `[`…`]` is not used to keep brackets free for tests: `«` and `»` mark a selection.
        let mut out = String::new();
        let mut carets = Vec::new();
        let mut sel_start = None;
        for c in text.chars() {
            match c {
                '|' => carets.push(Caret::at(out.chars().count())),
                '«' => sel_start = Some(out.chars().count()),
                '»' => carets.push(Caret::select(
                    sel_start.take().unwrap(),
                    out.chars().count(),
                )),
                _ => out.push(c),
            }
        }
        (TextBuffer::from_text(&out), Carets::from_carets(carets))
    }

    fn show(buf: &TextBuffer, carets: &Carets) -> String {
        let text: Vec<char> = buf.text().chars().collect();
        let mut out = String::new();
        for i in 0..=text.len() {
            for c in carets.carets() {
                if c.is_empty() && c.head == i {
                    out.push('|');
                } else if !c.is_empty() && c.range().start == i {
                    out.push('«');
                } else if !c.is_empty() && c.range().end == i {
                    out.push('»');
                }
            }
            if let Some(ch) = text.get(i) {
                out.push(*ch);
            }
        }
        out
    }

    fn check(input: &str, action: impl Fn(&mut TextBuffer, &mut Carets), expected: &str) {
        let (mut buf, mut carets) = setup(input);
        let before = buf.text();
        action(&mut buf, &mut carets);
        assert_eq!(show(&buf, &carets), expected, "from {input:?}");
        // One undo step restores the original text (an action that changed nothing leaves none).
        if buf.text() != before {
            assert!(buf.undo());
            assert_eq!(buf.text(), before);
        }
    }

    #[test]
    fn duplicate_lines_and_selections() {
        check("a|b\nc", duplicate, "ab\na|b\nc");
        check("x «ab» y", duplicate, "x ab«ab» y");
        check("one|\ntwo|", duplicate, "one\none|\ntwo\ntwo|");
    }

    #[test]
    fn delete_and_move_lines() {
        check("a\nb|b\nc", delete_lines, "a\nc|");
        check("a\nb\nc|c", delete_lines, "a\nb|");
        check("a\nb|\nc", |b, c| move_lines(b, c, true), "b|\na\nc");
        check("a\nb|\nc", |b, c| move_lines(b, c, false), "a\nc\nb|");
        check("«a\nb»\nc", |b, c| move_lines(b, c, false), "c\n«a\nb»");
    }

    #[test]
    fn join() {
        check("foo(|\n    a,\n    b\n)", join_lines, "foo(|a,\n    b\n)");
        check("x = 1|\n   + 2", join_lines, "x = 1| + 2");
        check("call(a|\n)", join_lines, "call(a|)");
    }

    #[test]
    fn comments() {
        let slash = CommentStyle::new("//");
        check(
            "    a|\n    b",
            |b, c| toggle_line_comment(b, c, &slash),
            "//    a|\n    b",
        );
        check(
            "«    a\n\n    b»",
            |b, c| toggle_line_comment(b, c, &slash),
            "«//    a\n\n//    b»",
        );
        check(
            "//    a|",
            |b, c| toggle_line_comment(b, c, &slash),
            "    a|",
        );
        let indented = CommentStyle {
            token: "#".into(),
            first_column: false,
            add_space: true,
        };
        check(
            "«  a\n    b»",
            |b, c| toggle_line_comment(b, c, &indented),
            "«  # a\n  #   b»",
        );
        check(
            "  # a|",
            |b, c| toggle_line_comment(b, c, &indented),
            "  a|",
        );
        // Mixed block: comments everything.
        check(
            "«//a\nb»",
            |b, c| toggle_line_comment(b, c, &slash),
            "«////a\n//b»",
        );
        // SPEC-0004 v12 AK-20: a token with a trailing space, as a Makefile's `# ` (OBS-0090 L2).
        let make = CommentStyle::new("# ");
        assert!(make.add_space && make.token == "#" && make.first_column);
        check("al|l:", |b, c| toggle_line_comment(b, c, &make), "# al|l:");
        check("# al|l:", |b, c| toggle_line_comment(b, c, &make), "al|l:");
        check("#al|l:", |b, c| toggle_line_comment(b, c, &make), "al|l:");
        // An ignore file's `#` has no space.
        let ignore = CommentStyle::new("#");
        check(
            "*.lo|g",
            |b, c| toggle_line_comment(b, c, &ignore),
            "#*.lo|g",
        );
        check(
            "#*.lo|g",
            |b, c| toggle_line_comment(b, c, &ignore),
            "*.lo|g",
        );
    }

    #[test]
    fn indentation() {
        check(
            "«a\n\nb»",
            |b, c| indent_lines(b, c, "    ", false),
            "«    a\n\n    b»",
        );
        check("    a|", |b, c| indent_lines(b, c, "    ", true), "a|");
        check("  a|", |b, c| indent_lines(b, c, "    ", true), "a|");
        check("\ta|", |b, c| indent_lines(b, c, "    ", true), "a|");
    }

    #[test]
    fn newline() {
        let o = NewlineOptions {
            unit: "    ".into(),
            indent_after: vec!['{', '(', '[', ':'],
        };
        check(
            "    x = 1|",
            |b, c| smart_newline(b, c, &o),
            "    x = 1\n    |",
        );
        check(
            "fn f() {|}",
            |b, c| smart_newline(b, c, &o),
            "fn f() {\n    |\n}",
        );
        check(
            "  if x:|",
            |b, c| smart_newline(b, c, &o),
            "  if x:\n      |",
        );
        check("a |  b", |b, c| smart_newline(b, c, &o), "a\n|b");
    }

    #[test]
    fn newline_inside_the_indentation() {
        let o = NewlineOptions {
            unit: "    ".into(),
            indent_after: vec!['{', '(', '[', ':'],
        };
        // Caret at column 9 (before `return`): eight spaces stay above, same column below.
        check(
            "{\n        |return x;\n}",
            |b, c| smart_newline(b, c, &o),
            "{\n        \n        |return x;\n}",
        );
        // Caret at column 5: four spaces stay above, the full indentation below.
        check(
            "{\n    |    return x;\n}",
            |b, c| smart_newline(b, c, &o),
            "{\n    \n        |return x;\n}",
        );
        // At column 1 nothing stays above.
        check("|    x", |b, c| smart_newline(b, c, &o), "\n    |x");
    }

    #[test]
    fn pairs() {
        check("f|", |b, c| type_char(b, c, '('), "f(|)");
        check("f(|)", |b, c| type_char(b, c, ')'), "f()|");
        check("|x", |b, c| type_char(b, c, '('), "(|x");
        check("«ab»", |b, c| type_char(b, c, '"'), "\"«ab»\"");
        check("don|", |b, c| type_char(b, c, '\''), "don'|");
        check("(|)", backspace_pair, "|");
        check("(a|)", backspace_pair, "(|)");
    }

    #[test]
    fn brackets() {
        let buf = TextBuffer::from_text("f(a, \")\", [b]) {}");
        assert_eq!(matching_bracket(&buf, 1), Some(13));
        assert_eq!(matching_bracket(&buf, 14), Some(1));
        assert_eq!(matching_bracket(&buf, 10), Some(12));
        assert_eq!(matching_bracket(&buf, 16), Some(15));
        assert_eq!(matching_bracket(&buf, 3), None);
    }
}

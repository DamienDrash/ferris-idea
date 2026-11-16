//! Language-aware editing helpers of `SPEC-0056` v3 and `SPEC-0057` v3: moving statements, block and
//! Markdown comments, the proper indentation of a line, re-indenting pasted text, the declared name
//! of a Java declaration and the brace balance of a document.
//!
//! They look at brackets and keywords line by line, not at a parse tree, so they work for every
//! language with braces and stay fast on large files.

use std::ops::Range;

use crate::actions::{apply, line_of, line_start, line_text};
use crate::buffer::{Position, TextBuffer};
use crate::selection::{Caret, Carets};

/// Language ids (as the editor is told them) whose blocks are written with braces.
const BRACE_LANGUAGES: &[&str] = &[
    "java",
    "kotlin",
    "javascript",
    "typescript",
    "javascriptreact",
    "typescriptreact",
    "c",
    "cpp",
    "go",
    "rust",
    "php",
    "groovy",
    "scala",
    "swift",
    "css",
    "scss",
    "less",
    "json",
    "csharp",
];

/// Keywords that open the block of a control statement (behaviour 13).
const CONTROL: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "try",
    "catch",
    "finally",
    "switch",
    "synchronized",
];

/// Whether `language` writes its blocks with braces.
pub fn has_braces(language: Option<&str>) -> bool {
    language.is_some_and(|l| BRACE_LANGUAGES.contains(&l))
}

fn indent_width(text: &str) -> usize {
    text.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

fn indent_of(text: &str) -> &str {
    let n: usize = text
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(char::len_utf8)
        .sum();
    &text[..n]
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Opening and closing brackets of `text` outside string and char literals and `//` comments.
pub fn bracket_counts(text: &str, open: char, close: char) -> (usize, usize) {
    let (mut o, mut c) = (0, 0);
    let mut quote: Option<char> = None;
    let mut prev = '\0';
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == q && prev != '\\' {
                quote = None;
            }
            prev = if prev == '\\' && ch == '\\' { '\0' } else { ch };
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '/' if chars.peek() == Some(&'/') => break,
            _ if ch == open => o += 1,
            _ if ch == close => c += 1,
            _ => {}
        }
        prev = ch;
    }
    (o, c)
}

/// Whether the document has more `{` than `}`, outside strings and comments (behaviour 18).
pub fn braces_open(text: &str) -> bool {
    let mut depth: isize = 0;
    let mut in_block = false;
    for line in text.lines() {
        let mut rest = line;
        if in_block {
            match rest.find("*/") {
                Some(i) => {
                    rest = &rest[i + 2..];
                    in_block = false;
                }
                None => continue,
            }
        }
        // Block comments that start on this line hide the rest up to their end.
        let mut visible = String::new();
        while let Some(i) = rest.find("/*") {
            visible.push_str(&rest[..i]);
            match rest[i + 2..].find("*/") {
                Some(j) => rest = &rest[i + 2 + j + 2..],
                None => {
                    in_block = true;
                    rest = "";
                }
            }
        }
        visible.push_str(rest);
        let (o, c) = bracket_counts(&visible, '{', '}');
        depth += o as isize - c as isize;
    }
    depth > 0
}

/// The proper indentation of `line` (behaviour 16): the previous non-blank line's indentation, one
/// `unit` more after an opening bracket at its end, one unit less when `line` starts with a closer.
pub fn proper_indent(buf: &TextBuffer, line: usize, unit: &str) -> String {
    let Some(prev) = (0..line).rev().find(|l| !blank(&line_text(buf, *l))) else {
        return String::new();
    };
    indent_after(
        indent_of(&line_text(buf, prev)).to_owned(),
        &line_text(buf, prev),
        &line_text(buf, line),
        unit,
    )
}

/// `indent`, the indentation the line `prev_text` stands for, as the line `here` below it needs
/// it: one `unit` more after an opening bracket at the end of `prev_text`, one less when `here`
/// starts with a closer.
fn indent_after(mut indent: String, prev_text: &str, here: &str, unit: &str) -> String {
    if matches!(prev_text.trim_end().chars().last(), Some('{' | '(' | '[')) {
        indent.push_str(unit);
    }
    if matches!(here.trim_start().chars().next(), Some('}' | ')' | ']')) {
        if indent.ends_with(unit) {
            indent.truncate(indent.len() - unit.len());
        } else {
            indent.pop();
        }
    }
    indent
}

/// The indentation of the line `here` under the non-blank line `prev` for Auto-Indent Lines
/// (`SPEC-0057` v6 behaviour 26): the proper indentation of [`proper_indent`], with the lines of
/// a block comment one space in from its `/*` and the line after its `*/` back out.
fn auto_indent_of(prev: &str, here: &str, unit: &str) -> String {
    let (p, h) = (prev.trim(), here.trim_start());
    let indent = indent_of(prev).to_owned();
    let opens = p.starts_with("/*") && !p.contains("*/");
    let goes_on = p.starts_with('*') && !p.ends_with("*/");
    if h.starts_with('*') && opens {
        return indent + " ";
    }
    if h.starts_with('*') && goes_on {
        return indent;
    }
    if p.starts_with('*') && p.ends_with("*/") {
        let out = indent.strip_suffix(' ').unwrap_or(&indent).to_owned();
        return indent_after(out, "", here, unit);
    }
    indent_after(indent, prev, here, unit)
}

/// Auto-Indent Lines (`SPEC-0057` v6 behaviour 26): the edits that give the lines `lines` their
/// indentation, from top to bottom, each line seeing those above as they will be. Blank lines and
/// lines that stand right get no edit; a first non-blank line of the text has no indentation.
pub fn auto_indent(
    buf: &TextBuffer,
    lines: std::ops::RangeInclusive<usize>,
    unit: &str,
) -> Vec<(Range<usize>, String)> {
    let first = *lines.start();
    let mut prev = (0..first)
        .rev()
        .map(|l| line_text(buf, l))
        .find(|t| !blank(t));
    let mut edits = Vec::new();
    for line in lines {
        let text = line_text(buf, line);
        if blank(&text) {
            continue;
        }
        let old = indent_of(&text);
        let new = match &prev {
            Some(prev) => auto_indent_of(prev, &text, unit),
            None => String::new(),
        };
        if new != old {
            let start = line_start(buf, line);
            edits.push((start..start + old.chars().count(), new.clone()));
        }
        prev = Some(format!("{new}{}", &text[old.len()..]));
    }
    edits
}

/// Whether the trimmed line starts a control statement's block.
fn control(text: &str) -> bool {
    let t = text.trim_start().trim_start_matches('}').trim_start();
    CONTROL.iter().any(|k| {
        t.strip_prefix(k)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

fn opens_block(text: &str) -> bool {
    text.trim_end().ends_with('{')
}

fn closes_block(text: &str) -> bool {
    text.trim_start().starts_with('}')
}

/// The line that closes the block opened on `line` (or `line` when it does not close).
fn block_end(lines: &[String], line: usize) -> usize {
    let mut depth = 0isize;
    for (i, text) in lines.iter().enumerate().skip(line) {
        let (o, c) = bracket_counts(text, '{', '}');
        depth += o as isize - c as isize;
        if depth <= 0 && i > line {
            return i;
        }
    }
    line
}

/// The line that opens the block closed on `line`.
fn block_start(lines: &[String], line: usize) -> usize {
    let mut depth = 0isize;
    for i in (0..=line).rev() {
        let (o, c) = bracket_counts(&lines[i], '{', '}');
        depth += c as isize - o as isize;
        if depth <= 0 && i < line {
            return i;
        }
    }
    line
}

/// `lines` with their indentation replaced: the first line's indentation becomes `to`, the others
/// keep their offset to it.
fn reindent(lines: &[String], to: &str) -> Vec<String> {
    let base = lines.first().map_or(0, |l| indent_width(l));
    let to_width = to.chars().count();
    lines
        .iter()
        .map(|l| {
            if blank(l) {
                return String::new();
            }
            let w = indent_width(l);
            let target = (to_width + w).saturating_sub(base);
            let pad = if to.contains('\t') && target >= to_width {
                format!("{to}{}", " ".repeat(target - to_width))
            } else {
                " ".repeat(target)
            };
            format!("{pad}{}", l.trim_start())
        })
        .collect()
}

/// Moves the statement at the primary caret up or down (`SPEC-0056` v3 behaviour 13). Returns
/// `false` when nothing moved.
pub fn move_statement(buf: &mut TextBuffer, carets: &mut Carets, up: bool, unit: &str) -> bool {
    let lines: Vec<String> = (0..buf.len_lines()).map(|l| line_text(buf, l)).collect();
    if lines.is_empty() {
        return false;
    }
    let primary = carets.primary();
    let r = primary.range();
    let s = line_of(buf, r.start);
    let mut e = line_of(buf, r.end);
    if e > s && line_start(buf, e) == r.end {
        e -= 1;
    }
    if opens_block(&lines[e]) && !closes_block(&lines[s]) {
        e = block_end(&lines, e);
    }
    let unit_lines: Vec<String> = lines[s..=e].to_vec();
    let single = s == e;
    let i_u = indent_of(&lines[s]).to_owned();
    // (lines before the unit's new place, unit lines as placed, lines after)
    let plan: Option<(Vec<String>, Vec<String>, Vec<String>)> = if up {
        let mut p = s;
        while p > 0 && blank(&lines[p - 1]) {
            p -= 1;
        }
        if p == 0 {
            return false;
        }
        let p = p - 1;
        let gap: Vec<String> = lines[p + 1..s].to_vec();
        let rest: Vec<String> = lines[e + 1..].to_vec();
        if opens_block(&lines[p]) && indent_width(&lines[p]) < i_u.chars().count() {
            if !control(&lines[p]) {
                return false;
            }
            let to = indent_of(&lines[p]).to_owned();
            let mut after = lines[p..s].to_vec();
            after.extend(rest);
            Some((lines[..p].to_vec(), reindent(&unit_lines, &to), after))
        } else if closes_block(&lines[p]) && indent_width(&lines[p]) == i_u.chars().count() {
            let o = block_start(&lines, p);
            if single && control(&lines[o]) {
                let to = format!("{i_u}{unit}");
                let mut after = lines[p..s].to_vec();
                after.extend(rest);
                Some((lines[..p].to_vec(), reindent(&unit_lines, &to), after))
            } else {
                let mut after = gap;
                after.extend(lines[o..=p].iter().cloned());
                after.extend(rest);
                Some((lines[..o].to_vec(), unit_lines.clone(), after))
            }
        } else {
            let mut after = gap;
            after.push(lines[p].clone());
            after.extend(rest);
            Some((lines[..p].to_vec(), unit_lines.clone(), after))
        }
    } else {
        let mut n = e + 1;
        while n < lines.len() && blank(&lines[n]) {
            n += 1;
        }
        if n >= lines.len() {
            return false;
        }
        let gap: Vec<String> = lines[e + 1..n].to_vec();
        let before = lines[..s].to_vec();
        if closes_block(&lines[n]) && indent_width(&lines[n]) < i_u.chars().count() {
            let o = block_start(&lines, n);
            if !control(&lines[o]) {
                return false;
            }
            let to = indent_of(&lines[n]).to_owned();
            let mut head = before;
            head.extend(gap);
            head.push(lines[n].clone());
            Some((head, reindent(&unit_lines, &to), lines[n + 1..].to_vec()))
        } else if opens_block(&lines[n]) && indent_width(&lines[n]) == i_u.chars().count() {
            let c = block_end(&lines, n);
            if single && control(&lines[n]) {
                let to = format!("{i_u}{unit}");
                let mut head = before;
                head.extend(gap);
                head.push(lines[n].clone());
                Some((head, reindent(&unit_lines, &to), lines[n + 1..].to_vec()))
            } else {
                let mut head = before;
                head.extend(lines[n..=c].iter().cloned());
                head.extend(gap);
                Some((head, unit_lines.clone(), lines[c + 1..].to_vec()))
            }
        } else {
            let mut head = before;
            head.push(lines[n].clone());
            head.extend(gap);
            Some((head, unit_lines.clone(), lines[n + 1..].to_vec()))
        }
    };
    let Some((head, placed, tail)) = plan else {
        return false;
    };
    let new_start = head.len();
    let shift = indent_width(&placed[0]) as isize - indent_width(&unit_lines[0]) as isize;
    let mut all = head;
    all.extend(placed);
    all.extend(tail);
    let nl = buf.line_ending().as_str();
    let text = all.join(nl);
    let positions: Vec<(Position, Position)> = carets
        .carets()
        .iter()
        .map(|c| {
            let map = |o: usize| {
                let p = buf.offset_to_position(o).unwrap_or_default();
                let line = (p.line.clamp(s, e) - s) + new_start;
                let column = (p.column as isize + shift).max(0) as usize;
                Position::new(line, column)
            };
            (map(c.anchor), map(c.head))
        })
        .collect();
    let edits = vec![(0..buf.len_chars(), text)];
    apply(buf, &edits);
    let out = positions
        .into_iter()
        .map(|(a, h)| {
            let at = |p: Position| {
                let len = line_text(buf, p.line).chars().count();
                line_start(buf, p.line) + p.column.min(len)
            };
            Caret::select(at(a), at(h))
        })
        .collect();
    carets.moved_to(out);
    true
}

/// Join of a `{` line with a block of one statement: the range from the brace to the end of the
/// closing line and its replacement (`SPEC-0056` v3 behaviour 14).
pub fn unwrap_one_line_block(buf: &TextBuffer, line: usize) -> Option<(Range<usize>, String)> {
    if line + 2 >= buf.len_lines() {
        return None;
    }
    let text = line_text(buf, line);
    let body = line_text(buf, line + 1);
    let close = line_text(buf, line + 2);
    if !text.trim_end().ends_with('{')
        || blank(&body)
        || body.trim_end().ends_with('{')
        || body.trim_start().starts_with('}')
        || close.trim() != "}"
    {
        return None;
    }
    let brace = text.trim_end().chars().count() - 1;
    let head = text.chars().take(brace).collect::<String>();
    let start = line_start(buf, line) + head.trim_end().chars().count();
    let end = line_start(buf, line + 2) + close.chars().count();
    Some((start..end, format!(" {}", body.trim())))
}

/// Line comment for languages with only block comments (Markdown, HTML, XML): wraps each non-blank
/// line of every caret block in `open … close`, or unwraps them when all are wrapped (behaviour 17).
pub fn toggle_wrapped_lines(buf: &mut TextBuffer, carets: &mut Carets, open: &str, close: &str) {
    let mut lines: Vec<usize> = Vec::new();
    for c in carets.carets() {
        let r = c.range();
        let first = line_of(buf, r.start);
        let mut last = line_of(buf, r.end);
        if !c.is_empty() && last > first && line_start(buf, last) == r.end {
            last -= 1;
        }
        lines.extend(first..=last);
    }
    lines.sort_unstable();
    lines.dedup();
    let texts: Vec<(usize, String)> = lines
        .iter()
        .map(|&l| (l, line_text(buf, l)))
        .filter(|(_, t)| !blank(t))
        .collect();
    if texts.is_empty() {
        return;
    }
    let wrapped = texts.iter().all(|(_, t)| {
        let t = t.trim();
        t.starts_with(open) && t.ends_with(close) && t.len() >= open.len() + close.len()
    });
    let mut edits = Vec::new();
    for (l, t) in &texts {
        let start = line_start(buf, *l);
        let range = start..start + t.chars().count();
        let indent = indent_of(t);
        let inner = t.trim();
        let new = if wrapped {
            let body = &inner[open.len()..inner.len() - close.len()];
            let body = body.strip_prefix(' ').unwrap_or(body);
            let body = body.strip_suffix(' ').unwrap_or(body);
            format!("{indent}{body}")
        } else {
            format!("{indent}{open} {inner} {close}")
        };
        edits.push((range, new));
    }
    let heads: Vec<(usize, usize)> = carets
        .carets()
        .iter()
        .map(|c| {
            let p = buf.offset_to_position(c.head).unwrap_or_default();
            (p.line, p.column)
        })
        .collect();
    apply(buf, &edits);
    let delta = if wrapped {
        -((open.chars().count() + 1) as isize)
    } else {
        (open.chars().count() + 1) as isize
    };
    let out = heads
        .into_iter()
        .map(|(line, column)| {
            let len = line_text(buf, line).chars().count();
            let col = (column as isize + delta).clamp(0, len as isize) as usize;
            Caret::at(line_start(buf, line) + col)
        })
        .collect();
    carets.moved_to(out);
}

/// Block comment toggle at each caret (behaviour 16).
pub fn toggle_block_comment(buf: &mut TextBuffer, carets: &mut Carets, open: &str, close: &str) {
    let (ol, cl) = (open.chars().count(), close.chars().count());
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    // Per caret: its place in the old text, how far behind it the caret goes, selection length.
    let mut result: Vec<(usize, usize, Option<usize>)> = Vec::new();
    for c in carets.carets() {
        let r = c.range();
        if !c.is_empty() {
            let text = buf.slice(r.clone()).unwrap_or_default();
            let t = text.trim();
            if t.starts_with(open) && t.ends_with(close) && t.len() >= open.len() + close.len() {
                let lead = text.chars().count() - text.trim_start().chars().count();
                let inner_start = r.start + lead;
                let inner_end = inner_start + t.chars().count();
                edits.push((inner_start..inner_start + ol, String::new()));
                edits.push((inner_end - cl..inner_end, String::new()));
                result.push((r.start, 0, Some(r.len() - ol - cl)));
            } else {
                edits.push((r.clone(), format!("{open}{text}{close}")));
                result.push((r.start, 0, Some(r.len() + ol + cl)));
            }
            continue;
        }
        let line = line_of(buf, c.head);
        let start = line_start(buf, line);
        let text: Vec<char> = line_text(buf, line).chars().collect();
        let col = c.head - start;
        let open_chars: Vec<char> = open.chars().collect();
        let close_chars: Vec<char> = close.chars().collect();
        let o = (0..=text.len().saturating_sub(ol))
            .rev()
            .find(|&i| i + ol <= col && text[i..].starts_with(&open_chars));
        let e = o.and_then(|o| {
            (o + ol..=text.len().saturating_sub(cl))
                .find(|&i| text[i..].starts_with(&close_chars))
                .filter(|&e| e >= col)
        });
        match (o, e) {
            (Some(o), Some(e)) => {
                edits.push((start + o..start + o + ol, String::new()));
                edits.push((start + e..start + e + cl, String::new()));
                result.push((c.head, 0, None));
            }
            _ => {
                edits.push((c.head..c.head, format!("{open}{close}")));
                result.push((c.head, ol, None));
            }
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    let mapped: Vec<Caret> = result
        .iter()
        .map(|&(at, behind, sel)| {
            let shift: isize = edits
                .iter()
                .filter(|(r, _)| r.end <= at && !(r.is_empty() && r.start == at))
                .map(|(r, t)| t.chars().count() as isize - r.len() as isize)
                .sum();
            let a = (at as isize + shift) as usize + behind;
            match sel {
                Some(n) => Caret::select(a, a + n),
                None => Caret::at(a),
            }
        })
        .collect();
    apply(buf, &edits);
    carets.moved_to(mapped);
}

/// The column (0-based, in characters) of the name a Java declaration line declares, if the line
/// is `[modifiers] Type[<…>][[]] name = …;` or `… name;` (`SPEC-0056` v3 behaviour 12).
pub fn declared_name_column(line: &str) -> Option<usize> {
    const MODIFIERS: &[&str] = &[
        "final",
        "static",
        "private",
        "public",
        "protected",
        "transient",
        "volatile",
    ];
    const NOT_TYPES: &[&str] = &[
        "return", "throw", "new", "else", "case", "yield", "assert", "break", "continue",
        "package", "import", "goto", "do", "if", "for", "while", "switch", "try", "catch",
    ];
    let chars: Vec<char> = line.chars().collect();
    let mut i = indent_width(line);
    let ident = |i: &mut usize| -> Option<String> {
        let start = *i;
        if !chars
            .get(*i)
            .is_some_and(|c| c.is_alphabetic() || *c == '_' || *c == '$')
        {
            return None;
        }
        while chars
            .get(*i)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '$' || *c == '.')
        {
            *i += 1;
        }
        Some(chars[start..*i].iter().collect())
    };
    let skip_ws = |i: &mut usize| {
        while chars.get(*i).is_some_and(|c| c.is_whitespace()) {
            *i += 1;
        }
    };
    let mut ty = ident(&mut i)?;
    while MODIFIERS.contains(&ty.as_str()) {
        skip_ws(&mut i);
        ty = ident(&mut i)?;
    }
    if NOT_TYPES.contains(&ty.as_str()) {
        return None;
    }
    if chars.get(i) == Some(&'<') {
        let mut depth = 0;
        while let Some(c) = chars.get(i) {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                ';' | '=' | '(' | ')' => return None,
                _ => {}
            }
            i += 1;
        }
    }
    while chars.get(i) == Some(&'[') && chars.get(i + 1) == Some(&']') {
        i += 2;
    }
    let before = i;
    skip_ws(&mut i);
    if i == before {
        return None;
    }
    let column = i;
    ident(&mut i)?;
    if chars[column..i].contains(&'.') {
        return None;
    }
    skip_ws(&mut i);
    matches!(chars.get(i), Some('=' | ';')).then_some(column)
}

/// `text` re-indented for a paste at the start of a line whose proper indentation is `target`
/// (`SPEC-0057` v3 behaviour 19).
pub fn reindent_paste(text: &str, target: &str, unit: &str, braces: bool) -> String {
    let normalized = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let unit_width = unit.chars().count().max(1);
    let base = lines.first().map_or(0, |l| indent_width(l));
    let mut depth: isize = 0;
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let content = line.trim_start();
        if content.is_empty() {
            out.push(String::new());
            continue;
        }
        let width = if braces {
            let leading = content
                .chars()
                .take_while(|c| matches!(c, '}' | ')' | ']'))
                .count() as isize;
            let level = (depth - leading).max(0) as usize;
            let (o1, c1) = bracket_counts(content, '{', '}');
            let (o2, c2) = bracket_counts(content, '(', ')');
            let (o3, c3) = bracket_counts(content, '[', ']');
            depth = (depth + (o1 + o2 + o3) as isize - (c1 + c2 + c3) as isize).max(0);
            // Javadoc and block comment continuation lines keep their one-space offset.
            let star = usize::from(content.starts_with('*') && i > 0);
            level * unit_width + star
        } else if i == 0 {
            0
        } else {
            indent_width(line).saturating_sub(base)
        };
        let pad = " ".repeat(width);
        out.push(format!("{target}{pad}{content}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_names() {
        assert_eq!(
            declared_name_column("        String a = \"Hello, \" + name;"),
            Some(15)
        );
        assert_eq!(declared_name_column("        int total = 0;"), Some(12));
        assert_eq!(
            declared_name_column("    private final List<String> xs;"),
            Some(31)
        );
        assert_eq!(declared_name_column("    int[] v = {1};"), Some(10));
        assert_eq!(declared_name_column("        return b;"), None);
        assert_eq!(declared_name_column("        total += x;"), None);
        assert_eq!(declared_name_column("        foo(a, b);"), None);
        assert_eq!(declared_name_column("        a.b = 3;"), None);
    }

    #[test]
    fn brace_balance_ignores_strings_and_comments() {
        assert!(braces_open("class A {\n  void f() {\n"));
        assert!(!braces_open("class A {\n}\n"));
        assert!(!braces_open("String s = \"{\";\n// {\n/* { */\n"));
    }

    #[test]
    fn pasted_lines_get_the_target_indentation() {
        assert_eq!(
            reindent_paste("if (x) {\nreturn x;\n}", "        ", "    ", true),
            "        if (x) {\n            return x;\n        }"
        );
        assert_eq!(
            reindent_paste("def f():\n    return 1\n", "    ", "    ", false),
            "    def f():\n        return 1\n"
        );
        assert_eq!(
            reindent_paste("/**\n * doc\n */", "    ", "    ", true),
            "    /**\n     * doc\n     */"
        );
    }

    /// `text` after Auto-Indent Lines on all its lines with `unit`.
    fn indented(text: &str, unit: &str) -> String {
        let mut buf = TextBuffer::from_text(text);
        let edits = auto_indent(&buf, 0..=buf.len_lines() - 1, unit);
        if !edits.is_empty() {
            apply(&mut buf, &edits);
        }
        buf.text()
    }

    /// SPEC-0057 v6 AK-21: every line under the brackets above it, each seeing the lines above
    /// as they are by then.
    #[test]
    fn auto_indent_follows_the_brackets() {
        let unit = "    ";
        // Nested blocks, closing brackets, a call over several lines, a blank line kept.
        assert_eq!(
            indented(
                "class A {\nvoid f() {\nif (x) {\ny(\na,\nb\n);\n}\n\n   \nz();\n}\n}\n",
                unit
            ),
            "class A {\n    void f() {\n        if (x) {\n            y(\n                a,\n                b\n            );\n        }\n\n   \n        z();\n    }\n}\n"
        );
        // Too far in comes back; lines that stand right give no edit.
        assert_eq!(
            indented("a {\n            b;\n        }\n", unit),
            "a {\n    b;\n}\n"
        );
        let right = "a {\n    b;\n}\n";
        assert!(auto_indent(&TextBuffer::from_text(right), 0..=3, unit).is_empty());
        // The first non-blank line of the text has no indentation.
        assert_eq!(indented("\n   a {\nb;\n", unit), "\na {\n    b;\n");
        // A tab as the unit.
        assert_eq!(
            indented("a {\nb {\nc;\n}\n}\n", "\t"),
            "a {\n\tb {\n\t\tc;\n\t}\n}\n"
        );
        // Lines of a block comment stand one space in, the line after it back out.
        assert_eq!(
            indented(
                "a {\n/**\n* Text.\n   * More.\n*/\nint x;\n/* one */\nint y;\n}\n",
                unit
            ),
            "a {\n    /**\n     * Text.\n     * More.\n     */\n    int x;\n    /* one */\n    int y;\n}\n"
        );
        // `\r\n` stays; only the indentation changes.
        assert_eq!(
            indented("a {\r\nb;\r\n  }\r\n", unit),
            "a {\r\n    b;\r\n}\r\n"
        );
        // Text behind the indentation is left alone, also outside ASCII.
        assert_eq!(
            indented("a {\n\t  s = \"ä𝄞\";\t// x\n}\n", unit),
            "a {\n    s = \"ä𝄞\";\t// x\n}\n"
        );
        // A range of lines: the lines before it are taken as they are, those after stay.
        let buf = TextBuffer::from_text("  a {\nb;\nc;\n");
        let edits = auto_indent(&buf, 1..=1, unit);
        assert_eq!(edits, [(6..6, "      ".to_owned())]);
    }
}

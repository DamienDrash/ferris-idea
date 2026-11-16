//! IDECraft's own Markdown formatter (`SPEC-0043` v6 behaviour 12): one blank line between
//! blocks (after a setext heading, around ATX headings, tables and fenced code blocks, between
//! lists with different markers), the columns of pipe tables aligned with a delimiter row of
//! dashes, and runs of spaces in top-level paragraphs reduced to one — nothing else. Code blocks,
//! front matter, HTML blocks, block quotes and list items are passed over. Before a result is
//! handed out it is checked: it differs from the source only in whitespace and dashes, and
//! formatting it again changes nothing.

/// A line without its `\r`.
fn content(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

fn blank(line: &str) -> bool {
    content(line).trim().is_empty()
}

/// The line without up to three leading spaces; `None` when it is indented deeper (an indented
/// code block, or the text of a list item) or with a tab.
fn block_start(line: &str) -> Option<&str> {
    let text = content(line);
    let rest = text.trim_start_matches(' ');
    (text.len() - rest.len() <= 3 && !rest.starts_with('\t')).then_some(rest)
}

/// `#` to `######` followed by a space, a tab or the end of the line.
fn heading(line: &str) -> bool {
    let Some(rest) = block_start(line) else {
        return false;
    };
    let level = rest.len() - rest.trim_start_matches('#').len();
    (1..=6).contains(&level) && matches!(rest.as_bytes().get(level), None | Some(b' ' | b'\t'))
}

/// The fence character and length when the line opens a fenced code block.
fn fence(line: &str) -> Option<(char, usize)> {
    let rest = block_start(line)?;
    let c = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = rest.len() - rest.trim_start_matches(c).len();
    (n >= 3 && !(c == '`' && rest[n..].contains('`'))).then_some((c, n))
}

/// The text that ends the HTML block the line opens: `-->` for a comment, the closing tag for
/// elements whose content is kept verbatim, an empty text (the next blank line) for any other
/// tag; `None` when the line opens no HTML block.
fn html_block(line: &str) -> Option<String> {
    let rest = block_start(line)?.strip_prefix('<')?;
    if rest.starts_with("!--") {
        return Some("-->".into());
    }
    let lower = rest.to_ascii_lowercase();
    for tag in ["pre", "script", "style", "textarea"] {
        if lower.strip_prefix(tag).is_some_and(|after| {
            after.is_empty() || after.starts_with(|c: char| c == '>' || c.is_whitespace())
        }) {
            return Some(format!("</{tag}>"));
        }
    }
    rest.starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
        .then(String::new)
}

/// The cells of a table row that starts and ends with `|`, trimmed; `None` for any other line.
/// `\|` is a pipe inside a cell.
fn cells(line: &str) -> Option<Vec<&str>> {
    let row = block_start(line)?.trim_end();
    let inner = row.strip_prefix('|')?.strip_suffix('|')?;
    if inner.ends_with('\\') {
        return None;
    }
    let bytes = inner.as_bytes();
    let (mut out, mut start, mut i) = (Vec::new(), 0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if bytes.get(i + 1) == Some(&b'|') => i += 2,
            b'|' => {
                out.push(inner[start..i].trim());
                start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    out.push(inner[start..].trim());
    Some(out)
}

/// How the row under a table's header aligns a column.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Default,
    Left,
    Right,
    Center,
}

fn alignment(cell: &str) -> Option<Align> {
    let (left, right) = (cell.starts_with(':'), cell.len() > 1 && cell.ends_with(':'));
    let dashes = &cell[usize::from(left)..cell.len() - usize::from(right)];
    (!dashes.is_empty() && dashes.bytes().all(|b| b == b'-')).then_some(match (left, right) {
        (true, true) => Align::Center,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        (false, false) => Align::Default,
    })
}

/// The table that starts at `lines[at]` with its columns aligned, one text per line; `None` when
/// no table starts there or its rows do not all have the header's number of cells.
fn table(lines: &[&str], at: usize) -> Option<Vec<String>> {
    let header = cells(lines[at])?;
    let aligns: Vec<Align> = cells(lines.get(at + 1)?)?
        .iter()
        .map(|c| alignment(c))
        .collect::<Option<_>>()?;
    if aligns.len() != header.len() {
        return None;
    }
    let mut rows = vec![header];
    for line in &lines[at + 2..] {
        match cells(line) {
            Some(row) => rows.push(row),
            None => break,
        }
    }
    if rows.iter().any(|r| r.len() != aligns.len()) {
        return None;
    }
    let width = |s: &str| s.chars().count();
    let widths: Vec<usize> = (0..aligns.len())
        .map(|c| rows.iter().map(|r| width(r[c])).max().unwrap_or(0).max(1))
        .collect();
    // A row is written with a space on each side of a cell, the delimiter row without.
    let line_of = |source: &str, cells: Vec<String>, gap: &str| {
        let text = content(source);
        let indent = &text[..text.len() - text.trim_start().len()];
        let end = if source.ends_with('\r') { "\r" } else { "" };
        let between = format!("{gap}|{gap}");
        format!("{indent}|{gap}{}{gap}|{end}", cells.join(&between))
    };
    let mut out = Vec::with_capacity(rows.len() + 1);
    for (r, row) in rows.iter().enumerate() {
        let padded = row
            .iter()
            .zip(&widths)
            .zip(&aligns)
            .map(|((cell, w), align)| {
                let gap = w - width(cell);
                let before = match align {
                    Align::Right => gap,
                    Align::Center => gap / 2,
                    _ => 0,
                };
                format!("{}{cell}{}", " ".repeat(before), " ".repeat(gap - before))
            })
            .collect();
        // The header is `lines[at]`, the rows follow the row of dashes.
        out.push(line_of(lines[at + r + usize::from(r > 0)], padded, " "));
        if r == 0 {
            let dashes = widths
                .iter()
                .zip(&aligns)
                .map(|(w, align)| match align {
                    Align::Default => "-".repeat(w + 2),
                    Align::Left => format!(":{}", "-".repeat(w + 1)),
                    Align::Right => format!("{}:", "-".repeat(w + 1)),
                    Align::Center => format!(":{}:", "-".repeat(*w)),
                })
                .collect();
            out.push(line_of(lines[at + 1], dashes, ""));
        }
    }
    Some(out)
}

/// Leading spaces of a line; a tab reaches the next multiple of four.
fn indent(line: &str) -> usize {
    let text = content(line);
    let spaces = text.len() - text.trim_start_matches(' ').len();
    if text[spaces..].starts_with('\t') {
        (spaces / 4 + 1) * 4
    } else {
        spaces
    }
}

/// Three or more `-`, `*` or `_` with nothing but spaces between them.
fn thematic_break(line: &str) -> bool {
    let Some(rest) = block_start(line) else {
        return false;
    };
    let Some(mark) = rest.chars().next().filter(|c| matches!(c, '-' | '*' | '_')) else {
        return false;
    };
    rest.chars().filter(|c| *c == mark).count() >= 3
        && rest.chars().all(|c| c == mark || c == ' ' || c == '\t')
}

/// A line of `=` or of `-`: under the text of a paragraph it makes that text a heading.
fn setext_underline(line: &str) -> bool {
    block_start(line).map(str::trim_end).is_some_and(|rest| {
        !rest.is_empty() && (rest.bytes().all(|b| b == b'=') || rest.bytes().all(|b| b == b'-'))
    })
}

fn quote(line: &str) -> bool {
    block_start(line).is_some_and(|rest| rest.starts_with('>'))
}

/// Only pipes, colons, dashes and spaces, with a dash: what stands under a table's header.
fn delimiter_like(line: &str) -> bool {
    let text = content(line);
    text.contains('-')
        && text
            .chars()
            .all(|c| matches!(c, '|' | ':' | '-' | ' ' | '\t'))
}

/// The start of a list item.
struct Item {
    /// The bullet character, or the `.` or `)` after the number: another marker, another list.
    marker: u8,
    /// The column the item's text starts in: lines indented that far belong to the item.
    content: usize,
    /// Nothing follows the marker.
    empty: bool,
    /// A bullet, or the number 1: only these may end a paragraph without a blank line.
    first: bool,
}

fn list_item(line: &str) -> Option<Item> {
    let text = content(line);
    let lead = text.len() - text.trim_start_matches(' ').len();
    let rest = &text[lead..];
    let bytes = rest.as_bytes();
    let (marker, width, first) = match *bytes.first()? {
        bullet @ (b'-' | b'*' | b'+') => (bullet, 1, true),
        b'0'..=b'9' => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            let delimiter = *bytes.get(digits)?;
            if digits > 9 || !matches!(delimiter, b'.' | b')') {
                return None;
            }
            (delimiter, digits + 1, &rest[..digits] == "1")
        }
        _ => return None,
    };
    let after = &rest[width..];
    if !(after.is_empty() || after.starts_with([' ', '\t'])) {
        return None;
    }
    let empty = after.trim().is_empty();
    let spaces = after.len() - after.trim_start_matches(' ').len();
    let gap = if empty || !(1..=4).contains(&spaces) {
        1
    } else {
        spaces
    };
    Some(Item {
        marker,
        content: lead + width + gap,
        empty,
        first,
    })
}

/// The line starts a block that is never the continuation of a line of text.
fn starts_block(line: &str) -> bool {
    fence(line).is_some()
        || heading(line)
        || thematic_break(line)
        || html_block(line).is_some()
        || quote(line)
}

/// What a line belongs to, as far as blank lines between blocks go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    /// Paragraph, quote, HTML, indented code, front matter, blank line.
    Other,
    Atx,
    Setext,
    Table,
    Fence,
    /// A list with its marker.
    List(u8),
}

/// Whether a blank line belongs between two blocks that follow each other.
fn separated(above: Block, below: Block) -> bool {
    match (above, below) {
        (Block::Setext | Block::Atx | Block::Table | Block::Fence, _)
        | (_, Block::Atx | Block::Table | Block::Fence) => true,
        (Block::List(a), Block::List(b)) => a != b,
        _ => false,
    }
}

struct Line {
    text: String,
    block: Block,
    /// Lines of one block share it.
    id: usize,
}

#[derive(Default)]
struct Doc {
    lines: Vec<Line>,
    blocks: usize,
}

impl Doc {
    /// The id of a new block.
    fn block(&mut self) -> usize {
        self.blocks += 1;
        self.blocks
    }

    fn push(&mut self, text: impl Into<String>, block: Block, id: usize) {
        self.lines.push(Line {
            text: text.into(),
            block,
            id,
        });
    }
}

/// The list or quote whose lines are being read.
#[derive(Clone, Copy)]
enum Open {
    List {
        marker: u8,
        content: usize,
        id: usize,
        /// A blank line was the last line: text without indentation ends the list.
        blank: bool,
    },
    Quote {
        id: usize,
    },
}

/// For every byte of a paragraph whether it stands where spaces mean something: in a code span,
/// between `<` and `>` on one line, in the parentheses of a link.
fn kept_spaces(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let ticks = |at: usize| bytes[at..].iter().take_while(|b| **b == b'`').count();
    let mut kept = vec![false; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => {
                let n = ticks(i);
                let mut j = i + n;
                let mut close = None;
                while j < bytes.len() {
                    if bytes[j] == b'`' {
                        let m = ticks(j);
                        if m == n {
                            close = Some(j + m);
                            break;
                        }
                        j += m;
                    } else {
                        j += 1;
                    }
                }
                match close {
                    Some(end) => {
                        kept[i..end].fill(true);
                        i = end;
                    }
                    None => i += n,
                }
            }
            b'<' => {
                let stop = bytes[i..]
                    .iter()
                    .position(|b| matches!(b, b'>' | b'\n'))
                    .filter(|p| bytes[i + p] == b'>');
                match stop {
                    Some(p) => {
                        kept[i..=i + p].fill(true);
                        i += p + 1;
                    }
                    None => i += 1,
                }
            }
            b']' if bytes.get(i + 1) == Some(&b'(') => {
                let (mut depth, mut j) = (0usize, i + 1);
                while j < bytes.len() && bytes[j] != b'\n' {
                    match bytes[j] {
                        b'\\' if bytes.get(j + 1).is_some_and(|b| *b != b'\n') => j += 1,
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                    if depth == 0 {
                        break;
                    }
                }
                let end = j.min(bytes.len());
                kept[i..end].fill(true);
                i = end;
            }
            _ => i += 1,
        }
    }
    kept
}

/// The lines of a paragraph with every run of spaces between two words reduced to one. The
/// indentation, the spaces at the end of a line and what [`kept_spaces`] names stay, and so
/// does a line that starts a link reference definition.
fn single_spaces(lines: &[&str]) -> Vec<String> {
    let texts: Vec<&str> = lines.iter().map(|l| content(l)).collect();
    let kept = kept_spaces(&texts.join("\n"));
    let mut out = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for (line, text) in lines.iter().zip(&texts) {
        let end = text.trim_end().len();
        let start = (text.len() - text.trim_start().len()).min(end);
        let definition = text[start..].starts_with('[') && text.contains("]:");
        let bytes = text.as_bytes();
        let mut new = String::with_capacity(line.len());
        new.push_str(&text[..start]);
        let mut k = start;
        while k < end {
            let run = bytes[k..end].iter().take_while(|b| **b == b' ').count();
            if run >= 2 && !definition && !kept[offset + k] {
                new.push(' ');
                k += run;
            } else {
                let len = run.max(text[k..].chars().next().map_or(1, char::len_utf8));
                new.push_str(&text[k..k + len]);
                k += len;
            }
        }
        new.push_str(&line[end..]);
        out.push(new);
        offset += text.len() + 1;
    }
    out
}

/// `text` with one blank line between the blocks behaviour 12 names, aligned table columns and
/// single spaces in paragraphs; an error (nothing to change then) when the result would differ
/// from `text` in more than whitespace and dashes or would not be stable.
pub fn format_markdown(text: &str) -> Result<String, String> {
    checked(text, layout(text))
}

/// What may not change: everything but whitespace and the dashes under a table's header.
fn substance(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

/// The promise, checked on every result: same text, and nothing left to do.
fn checked(source: &str, out: String) -> Result<String, String> {
    if substance(source) != substance(&out) || layout(&out) != out {
        return Err(
            "it would have changed more than whitespace here, so it left the file as it is (please report the file)"
                .to_owned(),
        );
    }
    Ok(out)
}

fn layout(text: &str) -> String {
    let (body, final_break) = match text.strip_suffix('\n') {
        Some(body) => (body, true),
        None => (text, false),
    };
    if body.is_empty() {
        return text.to_owned();
    }
    let lines: Vec<&str> = body.split('\n').collect();
    let empty = if text.contains("\r\n") { "\r" } else { "" };
    let mut doc = Doc::default();
    let mut open: Option<Open> = None;
    let mut i = 0;
    // Front matter: `---` on the first line to the next `---` or `...`.
    if content(lines[0]) == "---"
        && let Some(end) = lines[1..]
            .iter()
            .position(|l| matches!(content(l), "---" | "..."))
    {
        let id = doc.block();
        for line in &lines[..end + 2] {
            doc.push(*line, Block::Other, id);
        }
        i = end + 2;
    }
    while i < lines.len() {
        let line = lines[i];
        if blank(line) {
            match &mut open {
                Some(Open::List { blank, .. }) => *blank = true,
                _ => open = None,
            }
            let id = doc.block();
            doc.push(line, Block::Other, id);
            i += 1;
            continue;
        }
        // A list goes on with lines indented to its text, with further items, and with text
        // that continues an item; another marker starts another list.
        if let Some(Open::List {
            marker,
            content: text_column,
            id,
            blank: after_blank,
        }) = open
        {
            let goes_on = if indent(line) >= text_column {
                Some((marker, text_column, id))
            } else if starts_block(line) {
                None
            } else if let Some(item) = list_item(line) {
                let id = if item.marker == marker {
                    id
                } else {
                    doc.block()
                };
                Some((item.marker, item.content, id))
            } else if after_blank {
                None
            } else {
                Some((marker, text_column, id))
            };
            if let Some((marker, content, id)) = goes_on {
                open = Some(Open::List {
                    marker,
                    content,
                    id,
                    blank: false,
                });
                doc.push(line, Block::List(marker), id);
                i += 1;
                continue;
            }
            open = None;
        }
        // A quote goes on with quoted lines and with text that continues one.
        if let Some(Open::Quote { id }) = open {
            let item = list_item(line).is_some() && block_start(line).is_some();
            if quote(line) || !(starts_block(line) || item) {
                doc.push(line, Block::Other, id);
                i += 1;
                continue;
            }
            open = None;
        }
        let id = doc.block();
        if let Some((c, n)) = fence(line) {
            let close = lines[i + 1..].iter().position(|l| {
                block_start(l).is_some_and(|rest| {
                    let run = rest.len() - rest.trim_start_matches(c).len();
                    run >= n && rest[run..].trim().is_empty()
                })
            });
            let end = close.map_or(lines.len(), |p| i + p + 2);
            for kept in &lines[i..end] {
                doc.push(*kept, Block::Fence, id);
            }
            i = end;
        } else if let Some(end) = html_block(line) {
            let close = if end.is_empty() {
                lines[i..].iter().position(|l| blank(l)).map(|p| i + p)
            } else {
                lines[i..]
                    .iter()
                    .position(|l| l.to_ascii_lowercase().contains(&end))
                    .map(|p| i + p + 1)
            };
            let end = close.unwrap_or(lines.len()).max(i + 1);
            for kept in &lines[i..end] {
                doc.push(*kept, Block::Other, id);
            }
            i = end;
        } else if heading(line) {
            doc.push(line, Block::Atx, id);
            i += 1;
        } else if let Some(rows) = table(&lines, i) {
            i += rows.len();
            for row in rows {
                doc.push(row, Block::Table, id);
            }
        } else if thematic_break(line) {
            doc.push(line, Block::Other, id);
            i += 1;
        } else if quote(line) {
            open = Some(Open::Quote { id });
            doc.push(line, Block::Other, id);
            i += 1;
        } else if let Some(item) = list_item(line).filter(|_| block_start(line).is_some()) {
            open = Some(Open::List {
                marker: item.marker,
                content: item.content,
                id,
                blank: false,
            });
            doc.push(line, Block::List(item.marker), id);
            i += 1;
        } else if block_start(line).is_none() {
            // Indented code.
            doc.push(line, Block::Other, id);
            i += 1;
        } else {
            i = paragraph(&lines, i, id, &mut doc);
        }
    }
    let mut result: Vec<&str> = Vec::with_capacity(doc.lines.len() + 8);
    for (k, line) in doc.lines.iter().enumerate() {
        if let Some(above) = k.checked_sub(1).map(|k| &doc.lines[k])
            && above.id != line.id
            && !blank(&above.text)
            && !blank(&line.text)
            && separated(above.block, line.block)
        {
            result.push(empty);
        }
        result.push(&line.text);
    }
    let mut formatted = result.join("\n");
    if final_break {
        formatted.push('\n');
    }
    formatted
}

/// Reads the paragraph that starts at `lines[from]` into `doc` and returns the index after it:
/// a setext heading when a line of `=` or `-` ends it (kept as written), text with single
/// spaces otherwise. Lines that may be a table this formatter does not lay out are kept as
/// written up to the next blank line or block.
fn paragraph(lines: &[&str], from: usize, id: usize, doc: &mut Doc) -> usize {
    let mut end = from + 1;
    let mut block = Block::Other;
    let mut as_written = false;
    while end < lines.len() {
        let line = lines[end];
        if blank(line) {
            break;
        }
        if delimiter_like(line) && content(lines[end - 1]).contains('|') {
            as_written = true;
            end += 1;
            while end < lines.len() && !blank(lines[end]) && !starts_block(lines[end]) {
                end += 1;
            }
            break;
        }
        if setext_underline(line) {
            (block, as_written) = (Block::Setext, true);
            end += 1;
            break;
        }
        let item = list_item(line)
            .is_some_and(|item| block_start(line).is_some() && !item.empty && item.first);
        if starts_block(line) || item || table(lines, end).is_some() {
            break;
        }
        end += 1;
    }
    if as_written {
        for line in &lines[from..end] {
            doc.push(*line, block, id);
        }
    } else {
        for line in single_spaces(&lines[from..end]) {
            doc.push(line, block, id);
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formatted(text: &str) -> String {
        let out = format_markdown(text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(
            format_markdown(&out).as_deref(),
            Ok(out.as_str()),
            "idempotent"
        );
        assert_eq!(substance(text), substance(&out), "substance");
        out
    }

    /// SPEC-0043/AK-11: blank lines around headings, none added where one is.
    #[test]
    fn headings_get_blank_lines() {
        assert_eq!(
            formatted("# Title\ntext\n## Two\n### Three\n\nmore\n#### Last"),
            "# Title\n\ntext\n\n## Two\n\n### Three\n\nmore\n\n#### Last"
        );
        assert_eq!(formatted("intro\n#\nx\n"), "intro\n\n#\n\nx\n");
        // No heading: no space after the marks, seven marks, deep indentation, a quote, a list.
        let none = "#tag\n####### seven\n    # code\n> # quoted\n- # item\n";
        assert_eq!(formatted(none), none);
        assert_eq!(formatted(""), "");
        assert_eq!(formatted("\n"), "\n");
    }

    /// AK-11: table columns are padded to the widest cell; the delimiter row is filled with
    /// dashes and keeps its colons (v6, OBS-0036 FM6b).
    #[test]
    fn tables_are_aligned() {
        assert_eq!(
            formatted(
                "|Name|Count|Note|Mid|\n|---|--:|:-|:-:|\n|apple|1|a \\| b|x|\n| kiwi  |  1000 ||yyyy|\nafter\n"
            ),
            "| Name  | Count | Note   | Mid  |\n\
             |-------|------:|:-------|:----:|\n\
             | apple |     1 | a \\| b |  x   |\n\
             | kiwi  |  1000 |        | yyyy |\n\
             \n\
             after\n"
        );
        // A column of one character and an empty one; the text of a cell keeps its spaces.
        assert_eq!(
            formatted("|a||c  d|\n|:-:|-|-:|\n|1|||\n"),
            "| a |   | c  d |\n|:-:|---|-----:|\n| 1 |   |      |\n"
        );
        // Rows of another length, a row without the outer pipes, no row of dashes, a line with
        // a pipe over dashes: untouched.
        for same in [
            "| a | b |\n|---|---|\n| 1 |\n",
            "a | b\n--|--\n1  | 2\n",
            "| a | b |\n| 1 | 2 |\n",
            "| a  b\n---\n| 1  2\nmore  text\n",
        ] {
            assert_eq!(formatted(same), same);
        }
        assert_eq!(
            formatted("  | a |\r\n  |-|\r\n  | long |\r\n# H\r\nx\r\n"),
            "  | a    |\r\n  |------|\r\n  | long |\r\n\r\n# H\r\n\r\nx\r\n"
        );
    }

    /// AK-11: code blocks, front matter and HTML blocks are passed over.
    #[test]
    fn code_and_front_matter_stay() {
        let same = "---\n# no heading\ntitle: x\n---\n\n```sh\n# comment\n|a|b|\n|-|-|\n```\n\n~~~~\n# x\n~~~\n# still code\n~~~~\n\n<!--\n# hidden\n-->\n\n<pre>\n# pre\n\n|a|\n|-|\n</pre>\n\n<div>\n# in  html\n</div>\n\n    indented   code\n    # no heading\n";
        assert_eq!(formatted(same), same);
        // An unclosed fence takes the rest.
        let open = "text\n\n```\n# x\n\n|a|\n|-|\nb  c\n";
        assert_eq!(formatted(open), open);
        assert_eq!(
            formatted("```\ncode\n```\n# After\n"),
            "```\ncode\n```\n\n# After\n"
        );
    }

    /// AK-11 (v6, OBS-0036 FM6b): one blank line after a setext heading, around tables and
    /// fenced code blocks and between lists with different markers; none where one stands.
    #[test]
    fn blocks_are_separated() {
        assert_eq!(
            formatted("Title\n=====\ntext\nTwo\nlines\n---\n* a\n"),
            "Title\n=====\n\ntext\nTwo\nlines\n---\n\n* a\n"
        );
        assert_eq!(
            formatted("* a\n* b\n  * n\n+ c\n- d\n1. e\n2. f\n1) g\n"),
            "* a\n* b\n  * n\n\n+ c\n\n- d\n\n1. e\n2. f\n\n1) g\n"
        );
        assert_eq!(
            formatted("text\n|a|\n|-|\n|1|\nmore\n"),
            "text\n\n| a |\n|---|\n| 1 |\n\nmore\n"
        );
        assert_eq!(
            formatted("text\n```\ncode\n\n# x\n```\nmore\n- a\n~~~\nx\n~~~\n- b\n```\n"),
            "text\n\n```\ncode\n\n# x\n```\n\nmore\n- a\n\n~~~\nx\n~~~\n\n- b\n\n```\n"
        );
        // Blank lines that stand already stay as many as they are; other blocks get none.
        for same in [
            "# A\n\n\ntext\n\n```\nx\n```\n\n| a |\n|---|\n\nT\n=\n\n- a\n\n+ b\n",
            "text\n- a\n> q\n***\ntext\n1. a\n\n\n\nend\n",
        ] {
            assert_eq!(formatted(same), same);
        }
    }

    /// AK-11 (v6): nothing is inserted or changed inside a list or a quote, also in the lines
    /// that continue them without indentation.
    #[test]
    fn lists_and_quotes_stay() {
        for same in [
            "- a\n  # in item\n  | x |\n  |-|\n  ```\n  code\n  ```\n- b\nlazy   text\n===\n| x |\n|-|\n\n  still   item\n\n1. one\n\n   para   in item\n2. two\n",
            "> a\n> # h\n> | x |\n> |-|\nlazy  text\n===\n- item  x\n",
            "10. ten\n    * nested\n    + other\n\n    text\n11. eleven\n",
            "\t- tab\n* * *\n- - -\n-\n",
        ] {
            assert_eq!(formatted(same), same);
        }
        // What is not indented to the item's text ends the list.
        assert_eq!(
            formatted("- a\n# H\n- b\n\ntext  x\n===\nmore\n"),
            "- a\n\n# H\n\n- b\n\ntext  x\n===\n\nmore\n"
        );
    }

    /// AK-11 (v6, OBS-0036 FM6b): several spaces between words of a paragraph become one;
    /// indentation, trailing spaces, code spans, tags, link targets and definitions stay.
    #[test]
    fn spaces_in_paragraphs() {
        for (source, expected) in [
            (
                "Some   text   with   extra   spaces.\n",
                "Some text with extra spaces.\n",
            ),
            (
                "  lead   kept  \nnext  line\\\n\ttab\t\tkept  x\n    continued   line\n",
                "  lead kept  \nnext line\\\n\ttab\t\tkept x\n    continued line\n",
            ),
            (
                "a  `b  c`  d ``e  `  f``  g `open  h\n",
                "a `b  c` d ``e  `  f`` g `open h\n",
            ),
            ("x `a  b\nc  d` e  f\n", "x `a  b\nc  d` e f\n"),
            ("a \\`b  c\\` d\n", "a \\`b c\\` d\n"),
            (
                "see  <a  href=\"x  y\">t  u</a>  and  [l  m](<u  v>  \"t  t\")  end  ![i](a  (b  c)  d)  x\n",
                "see <a  href=\"x  y\">t u</a> and [l m](<u  v>  \"t  t\") end ![i](a  (b  c)  d) x\n",
            ),
            ("a  <  b  [c](d  e\nf  g\n", "a < b [c](d  e\nf g\n"),
            (
                "[ref]:  <my  url>  \"a  title\"\ntext  after\n",
                "[ref]:  <my  url>  \"a  title\"\ntext after\n",
            ),
            ("a  b  \r\nc  d\r\n", "a b  \r\nc d\r\n"),
            ("ä  ö  ü\n", "ä ö ü\n"),
        ] {
            assert_eq!(formatted(source), expected);
        }
        // Headings, list items, quotes and code keep their spaces.
        let same = "# a  b\n\nT  x\n===\n\n- a  b\n\n> c  d\n\n    code  x\n";
        assert_eq!(formatted(same), same);
    }

    /// AK-11 (v6): the `notes.md` of OBS-0036 as the observation describes its result.
    #[test]
    fn the_observed_file() {
        assert_eq!(
            formatted(
                "Notes\n=====\n* first item\n* second item\n    * nested item\n+ other bullet\n1. one\n1. two\n\n| name | value |\n|---|---|\n| a | 1 |\n| long name | 22 |\nSome   text   with   extra   spaces.\n## Heading without blank lines\n```\ncode\n```\n"
            ),
            "Notes\n\
             =====\n\
             \n\
             * first item\n\
             * second item\n    \
                 * nested item\n\
             \n\
             + other bullet\n\
             \n\
             1. one\n\
             1. two\n\
             \n\
             | name      | value |\n\
             |-----------|-------|\n\
             | a         | 1     |\n\
             | long name | 22    |\n\
             \n\
             Some text with extra spaces.\n\
             \n\
             ## Heading without blank lines\n\
             \n\
             ```\n\
             code\n\
             ```\n"
        );
    }

    /// AK-11 (v6): every Markdown file of the repository's `docs/` folder is formatted without
    /// a refusal, keeps its text and is stable after one run.
    #[test]
    fn the_repository_s_markdown_files_keep_their_text() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut folders = vec![root];
        let (mut files, mut changed) = (0, 0);
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "md") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let out = match format_markdown(&text) {
                        Ok(out) => out,
                        Err(e) => panic!("{}: {e}", path.display()),
                    };
                    assert_eq!(substance(&text), substance(&out), "{}", path.display());
                    assert!(out.lines().count() >= text.lines().count());
                    files += 1;
                    changed += usize::from(out != text);
                }
            }
        }
        assert!(
            files >= 100 && changed > 0,
            "{files} files, {changed} changed"
        );
    }

    /// AK-11 (v6): a result that differs in more than whitespace and dashes, or that a second
    /// run would change, is refused.
    #[test]
    fn an_unsafe_result_is_refused() {
        assert_eq!(checked("# a\n", "# a\n".into()).as_deref(), Ok("# a\n"));
        for (source, out) in [
            ("# a\n", "# b\n"),
            ("x\n# a\n", "x\n# a\n"),
            ("a  b\n", "ab c\n"),
        ] {
            let e = checked(source, out.into()).unwrap_err();
            assert!(
                e.starts_with("it would have changed more than whitespace"),
                "{e}"
            );
        }
    }
}

//! Small editing commands (`SPEC-0332`): Sort Lines and Reverse Lines, Transpose, a caret at the
//! end of every selected line, indentation converted to spaces or tabs.
//!
//! The rules are those of the open platform's actions of the same ids (intellij-community,
//! `docs/legal/PROVENANCE.md`: logic template); the code is IDECraft's own, on char offsets and
//! with every line break left as it is (behaviour 2).

use std::ops::Range;

use crate::actions::{apply, line_of, line_start, line_text};
use crate::buffer::TextBuffer;
use crate::selection::{Caret, Carets};

/// The new order of the lines (behaviour 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrder {
    /// Ascending by UTF-16 code units, equal lines in their old order.
    Sorted,
    /// The last line first.
    Reversed,
}

/// Offset after the last character of `line`, before its line break.
fn line_end(buf: &TextBuffer, line: usize) -> usize {
    line_start(buf, line) + line_text(buf, line).chars().count()
}

/// Puts `new` in the place of the primary caret; the other carets stay.
fn replace_primary(carets: &mut Carets, new: Caret) {
    let old = carets.primary();
    let mut list = carets.carets().to_vec();
    if let Some(i) = list.iter().position(|c| *c == old) {
        list[i] = new;
    }
    carets.moved_to(list);
}

/// Sort Lines and Reverse Lines (behaviours 3–5): reorders the lines of the primary caret's
/// selection, or all lines without one. Returns `false`, having changed nothing, when these are
/// fewer than two lines.
pub fn permute_lines(buf: &mut TextBuffer, carets: &mut Carets, order: LineOrder) -> bool {
    let primary = carets.primary();
    let selected = !primary.is_empty();
    let range = if selected {
        primary.range()
    } else {
        0..buf.len_chars()
    };
    let first = line_of(buf, range.start);
    let mut last = line_of(buf, range.end);
    // A last line on whose first column the range ends is left out.
    if range.end == line_start(buf, last) {
        if last == 0 {
            return false;
        }
        last -= 1;
    }
    if first >= last {
        return false;
    }
    let texts: Vec<String> = (first..=last).map(|l| line_text(buf, l)).collect();
    // `from[slot]` is the line whose text goes to `slot`.
    let mut from: Vec<usize> = (0..texts.len()).collect();
    match order {
        LineOrder::Sorted => {
            from.sort_by(|a, b| texts[*a].encode_utf16().cmp(texts[*b].encode_utf16()));
        }
        LineOrder::Reversed => from.reverse(),
    }
    let caret_line = line_of(buf, primary.head);
    let column = primary.head - line_start(buf, caret_line);
    // Only the texts change places; every line break stays where it is.
    let edits: Vec<(Range<usize>, String)> = from
        .iter()
        .enumerate()
        .filter(|(slot, line)| texts[**line] != texts[*slot])
        .map(|(slot, line)| {
            let start = line_start(buf, first + slot);
            (
                start..start + texts[slot].chars().count(),
                texts[*line].clone(),
            )
        })
        .collect();
    if !edits.is_empty() {
        apply(buf, &edits);
    }
    let moved = if selected {
        let end = if last + 1 < buf.len_lines() {
            line_start(buf, last + 1)
        } else {
            buf.len_chars()
        };
        Caret::select(line_start(buf, first), end)
    } else if (first..=last).contains(&caret_line) {
        let slot = from
            .iter()
            .position(|line| *line == caret_line - first)
            .unwrap_or(caret_line - first);
        Caret::at(line_start(buf, first + slot) + column)
    } else {
        primary
    };
    replace_primary(carets, moved);
    true
}

/// Start of the character before `at` (`at` > 0); a `\r\n` is one character (behaviour 2).
fn unit_before(buf: &TextBuffer, at: usize) -> usize {
    if at >= 2 && buf.char_at(at - 1) == Some('\n') && buf.char_at(at - 2) == Some('\r') {
        at - 2
    } else {
        at - 1
    }
}

/// Swaps the characters around a caret without selection at `at` (behaviour 6); returns where the
/// caret is afterwards.
fn swap_at(buf: &mut TextBuffer, at: usize) -> usize {
    if at == 0 {
        return at;
    }
    let before = unit_before(buf, at);
    let (start, middle, end, caret) = if at < line_end(buf, line_of(buf, at)) {
        // Before the end of the line: the character before and the one after, caret behind both.
        (before, at, at + 1, at + 1)
    } else {
        // At the end of the line: the two characters before the caret, which stays.
        if before == 0 {
            return at;
        }
        (unit_before(buf, before), before, at, at)
    };
    let a = buf.slice(start..middle).unwrap_or_default();
    let b = buf.slice(middle..end).unwrap_or_default();
    if a != b {
        let _ = buf.replace(start..end, &format!("{b}{a}"));
    }
    caret
}

/// Transpose (behaviours 6–8): without any selection, swaps the characters around every caret;
/// with a selection at every one of several carets, rotates the selected texts; else nothing.
pub fn transpose(buf: &mut TextBuffer, carets: &mut Carets) {
    let list = carets.carets().to_vec();
    if list.iter().all(Caret::is_empty) {
        // In the order of the text; a swap keeps the length, so later offsets stay valid.
        buf.begin_group();
        let moved = list
            .iter()
            .map(|c| Caret::at(swap_at(buf, c.head)))
            .collect();
        buf.end_group();
        carets.moved_to(moved);
    } else if list.len() > 1 && list.iter().all(|c| !c.is_empty()) {
        let texts: Vec<String> = list
            .iter()
            .map(|c| buf.slice(c.range()).unwrap_or_default())
            .collect();
        let n = list.len();
        // Every selection gets the text of the one before it, the first the last one's.
        let edits: Vec<(Range<usize>, String)> = (0..n)
            .map(|i| (list[i].range(), texts[(i + n - 1) % n].clone()))
            .collect();
        let changed: Vec<(Range<usize>, String)> = edits
            .iter()
            .zip(&texts)
            .filter(|((_, new), old)| new != *old)
            .map(|(e, _)| e.clone())
            .collect();
        if !changed.is_empty() {
            apply(buf, &changed);
        }
        let mut shift = 0isize;
        let moved = edits
            .iter()
            .map(|(range, text)| {
                let start = (range.start as isize + shift) as usize;
                let len = text.chars().count();
                shift += len as isize - range.len() as isize;
                Caret::select(start, start + len)
            })
            .collect();
        carets.moved_to(moved);
    }
}

/// Add Carets to Ends of Selected Lines (behaviour 9): every caret becomes one caret at the end of
/// each line its selection touches.
pub fn carets_at_line_ends(buf: &TextBuffer, carets: &mut Carets) {
    let primary = carets.primary();
    let mut list = Vec::new();
    let mut primary_at = None;
    for c in carets.carets() {
        let range = c.range();
        let first = line_of(buf, range.start);
        let mut last = line_of(buf, range.end);
        if last > first && range.end == line_start(buf, last) {
            last -= 1;
        }
        let ends: Vec<usize> = (first..=last).map(|l| line_end(buf, l)).collect();
        if *c == primary {
            // The caret at the end of its selection leaves the last line's caret primary.
            primary_at = if c.head == range.start {
                ends.first().copied()
            } else {
                ends.last().copied()
            };
        }
        list.extend(ends.into_iter().map(Caret::at));
    }
    let mut next = Carets::from_carets(list);
    if let Some(at) = primary_at {
        // Merges with the caret that is there and makes it primary.
        next.add_caret(at);
    }
    *carets = next;
}

/// Columns of an indentation of spaces and tabs with tabs `tab_width` columns apart
/// (behaviour 11).
pub fn indent_width(indent: &str, tab_width: usize) -> usize {
    let tab = tab_width.max(1);
    indent.chars().fold(0, |columns, c| match c {
        '\t' => (columns / tab + 1) * tab,
        _ => columns + 1,
    })
}

/// An indentation of `columns` columns: spaces, or as many tabs as fit and the rest as spaces.
pub fn build_indent(columns: usize, tab_width: usize, tabs: bool) -> String {
    let tab = tab_width.max(1);
    if tabs {
        format!(
            "{}{}",
            "\t".repeat(columns / tab),
            " ".repeat(columns % tab)
        )
    } else {
        " ".repeat(columns)
    }
}

/// Where `offset` is after `edits` (sorted, not overlapping): an offset at the start of a replaced
/// range stays there, one inside it goes to the end of the replacement.
pub(crate) fn through(offset: usize, edits: &[(Range<usize>, String)]) -> usize {
    let mut shift = 0isize;
    for (range, text) in edits {
        if offset <= range.start {
            break;
        }
        let len = text.chars().count() as isize;
        if offset < range.end {
            return (range.start as isize + shift + len) as usize;
        }
        shift += len - range.len() as isize;
    }
    (offset as isize + shift) as usize
}

/// Convert Indents (behaviours 10–12): rewrites the indentation of the lines the primary caret's
/// selection touches, or of all lines without one, as spaces or (`tabs`) as tabs. Every line is
/// measured from its first column (IMP-0403). Returns the number of lines changed.
pub fn convert_indents(
    buf: &mut TextBuffer,
    carets: &mut Carets,
    tab_width: usize,
    tabs: bool,
) -> usize {
    let primary = carets.primary();
    let range = if primary.is_empty() {
        0..buf.len_chars()
    } else {
        primary.range()
    };
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for line in line_of(buf, range.start)..=line_of(buf, range.end) {
        let text = line_text(buf, line);
        let indent: String = text
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        let columns = indent_width(&indent, tab_width);
        if columns == 0 {
            continue;
        }
        let new = build_indent(columns, tab_width, tabs);
        if new != indent {
            let start = line_start(buf, line);
            edits.push((start..start + indent.chars().count(), new));
        }
    }
    if edits.is_empty() {
        return 0;
    }
    apply(buf, &edits);
    let moved = carets
        .carets()
        .iter()
        .map(|c| Caret::select(through(c.anchor, &edits), through(c.head, &edits)))
        .collect();
    carets.moved_to(moved);
    edits.len()
}

/// What the status bar says after Convert Indents changed `lines` lines (behaviour 12).
pub fn indents_message(lines: usize, tabs: bool) -> String {
    match lines {
        0 if tabs => "Indentation already uses tabs".to_owned(),
        0 => "Indentation already uses spaces".to_owned(),
        1 => "Indentation converted in 1 line".to_owned(),
        n => format!("Indentation converted in {n} lines"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{EditorState, perform};

    /// Buffer and carets from text with `|` marking carets and `[`…`]` selections (head at `]`).
    fn parse(marked: &str) -> (TextBuffer, Carets) {
        let mut text = String::new();
        let mut list = Vec::new();
        let mut anchor = None;
        for c in marked.chars() {
            let at = text.chars().count();
            match c {
                '|' => list.push(Caret::at(at)),
                '[' => anchor = Some(at),
                ']' => list.push(Caret::select(anchor.take().expect("open"), at)),
                _ => text.push(c),
            }
        }
        (TextBuffer::from_text(&text), Carets::from_carets(list))
    }

    /// Text with the carets marked like [`parse`].
    fn show(buf: &TextBuffer, carets: &Carets) -> String {
        let text: Vec<char> = buf.text().chars().collect();
        let mut out = String::new();
        for i in 0..=text.len() {
            for c in carets.carets() {
                if c.is_empty() && c.head == i {
                    out.push('|');
                } else if !c.is_empty() && c.range().end == i {
                    out.push(']');
                }
            }
            for c in carets.carets() {
                if !c.is_empty() && c.range().start == i {
                    out.push('[');
                }
            }
            if let Some(ch) = text.get(i) {
                out.push(*ch);
            }
        }
        out
    }

    fn permuted(marked: &str, order: LineOrder) -> (bool, String) {
        let (mut buf, mut carets) = parse(marked);
        let done = permute_lines(&mut buf, &mut carets, order);
        (done, show(&buf, &carets))
    }

    fn sorted(marked: &str) -> String {
        let (done, text) = permuted(marked, LineOrder::Sorted);
        assert!(done, "{marked:?}");
        text
    }

    #[test]
    fn sort_selected_lines() {
        assert_eq!(sorted("[b\na\nc\n]d\n"), "[a\nb\nc\n]d\n");
        // The last selected line is the last line of the text: the selection ends with the text.
        assert_eq!(sorted("z\n[b\na]"), "z\n[a\nb]");
        // Only the primary caret's selection is sorted; another caret keeps its offset.
        let (mut buf, mut carets) = parse("|z\ny\nb\na\n");
        carets.add_selection(4, 8);
        assert!(permute_lines(&mut buf, &mut carets, LineOrder::Sorted));
        assert_eq!(show(&buf, &carets), "|z\ny\n[a\nb\n]");
    }

    #[test]
    fn sort_selection_ending_at_a_line_start() {
        // From the middle of a line to the middle of the next: both lines as a whole.
        assert_eq!(sorted("x\nc[c\nb]b\na\n"), "x\n[bb\ncc\n]a\n");
        // Ending at the start of `a`'s line: that line is not taken.
        assert_eq!(sorted("[c\nb\n]a\n"), "[b\nc\n]a\n");
    }

    #[test]
    fn sort_whole_text_with_and_without_final_line_break() {
        // The empty line after the final line break is not sorted to the front.
        assert_eq!(sorted("b\n|a\nc\n"), "|a\nb\nc\n");
        assert_eq!(sorted("b\na|"), "a|\nb");
        // A blank line inside the text is a line like any other.
        assert_eq!(sorted("b\n\na|\n"), "\na|\nb\n");
    }

    #[test]
    fn sort_one_line_changes_nothing() {
        for marked in ["on|e\n", "|", "one|", "[ab]c\nd\n", "[ab\n]c\n"] {
            let (mut buf, mut carets) = parse(marked);
            assert!(!permute_lines(&mut buf, &mut carets, LineOrder::Sorted));
            assert!(!permute_lines(&mut buf, &mut carets, LineOrder::Reversed));
            assert_eq!(show(&buf, &carets), marked);
            assert_eq!(buf.top_step(), None, "{marked:?}");
        }
        // Lines already in order: no undo step.
        let (mut buf, mut carets) = parse("a\n|b\n");
        assert!(permute_lines(&mut buf, &mut carets, LineOrder::Sorted));
        assert_eq!(buf.top_step(), None);
    }

    #[test]
    fn sort_keeps_crlf() {
        assert_eq!(sorted("b\r\na\r\n|"), "a\r\nb\r\n|");
        // Every line break stays where it is, also in a file that mixes them.
        assert_eq!(sorted("c\r\nb\na|"), "a|\r\nb\nc");
        assert_eq!(sorted("[b\r\na\r\n]c\r\n"), "[a\r\nb\r\n]c\r\n");
    }

    #[test]
    fn sort_order_is_utf16_and_case_sensitive() {
        // Capitals before small letters, a prefix before the longer line, no numbers.
        assert_eq!(sorted("b\nab\na\nB\n9\n10|"), "10|\n9\nB\na\nab\nb");
        // U+1F600 is written with surrogates (D83D …) and so stands before U+FF5E, which a
        // comparison by code points would put first.
        assert_eq!(
            sorted("\u{ff5e}\n\u{1f600}\n\u{e9}\nz|"),
            "z|\n\u{e9}\n\u{1f600}\n\u{ff5e}"
        );
    }

    #[test]
    fn sort_caret_follows_its_line() {
        assert_eq!(sorted("c\na\nb|b"), "a\nb|b\nc");
        // Equal lines keep their order: the caret's `a` is the second one.
        assert_eq!(sorted("b\na\n|a"), "a\n|a\nb");
    }

    #[test]
    fn reverse_lines() {
        let reversed = |m: &str| permuted(m, LineOrder::Reversed);
        assert_eq!(reversed("[1\n2\n3]"), (true, "[3\n2\n1]".into()));
        assert_eq!(reversed("1\n2\n3\n|"), (true, "3\n2\n1\n|".into()));
        assert_eq!(reversed("1|1\n2\n3"), (true, "3\n2\n1|1".into()));
        assert_eq!(
            reversed("0\n[1\r\n22\n]3"),
            (true, "0\n[22\r\n1\n]3".into())
        );
    }

    #[test]
    fn reverse_is_one_undo_step() {
        let (mut buf, mut carets) = parse("1\n2\n3\n4|");
        assert!(permute_lines(&mut buf, &mut carets, LineOrder::Reversed));
        assert_eq!(buf.text(), "4\n3\n2\n1");
        assert!(buf.undo());
        assert_eq!(buf.text(), "1\n2\n3\n4");
        assert!(!buf.undo());
    }

    fn transposed(marked: &str) -> String {
        let (mut buf, mut carets) = parse(marked);
        transpose(&mut buf, &mut carets);
        show(&buf, &carets)
    }

    #[test]
    fn transpose_in_a_line() {
        assert_eq!(transposed("a|bc"), "ba|c");
        assert_eq!(transposed("ab|c"), "acb|");
        assert_eq!(transposed("x\na|b\n"), "x\nba|\n");
        // Equal characters: the caret moves, no undo step.
        let (mut buf, mut carets) = parse("a|a");
        transpose(&mut buf, &mut carets);
        assert_eq!(show(&buf, &carets), "aa|");
        assert_eq!(buf.top_step(), None);
    }

    #[test]
    fn transpose_at_the_end_of_a_line() {
        assert_eq!(transposed("abc|"), "acb|");
        assert_eq!(transposed("abc|\nd"), "acb|\nd");
        assert_eq!(transposed("a|"), "a|");
        // One undo step.
        let (mut buf, mut carets) = parse("abc|");
        transpose(&mut buf, &mut carets);
        assert!(buf.undo());
        assert_eq!(buf.text(), "abc");
        assert!(!buf.undo());
    }

    #[test]
    fn transpose_at_the_start_of_text_and_line() {
        assert_eq!(transposed("|ab"), "|ab");
        assert_eq!(transposed("|"), "|");
        // The character before the caret is the line break.
        assert_eq!(transposed("ab\n|cd"), "abc\n|d");
    }

    #[test]
    fn transpose_on_an_empty_line() {
        assert_eq!(transposed("ab\n|\ncd"), "a\nb|\ncd");
        assert_eq!(transposed("\n|"), "\n|");
    }

    #[test]
    fn transpose_with_crlf() {
        assert_eq!(transposed("abc|\r\nd"), "acb|\r\nd");
        // The line break moves as a whole.
        assert_eq!(transposed("ab\r\n|cd"), "abc\r\n|d");
        assert_eq!(transposed("ab\r\n|\r\ncd"), "a\r\nb|\r\ncd");
    }

    #[test]
    fn transpose_non_ascii() {
        assert_eq!(transposed("\u{e9}|\u{fc}"), "\u{fc}\u{e9}|");
        // A letter outside the Basic Multilingual Plane is one character.
        assert_eq!(transposed("a\u{1f600}|b"), "ab\u{1f600}|");
        assert_eq!(transposed("\u{1f600}\u{1f601}|"), "\u{1f601}\u{1f600}|");
    }

    #[test]
    fn transpose_several_carets() {
        assert_eq!(transposed("a|bc d|ef"), "ba|c ed|f");
        // One after the other on the text as it then is.
        assert_eq!(transposed("a|b|c"), "bc|a|");
        let (mut buf, mut carets) = parse("a|bc d|ef");
        transpose(&mut buf, &mut carets);
        assert!(buf.undo());
        assert_eq!(buf.text(), "abc def");
        assert!(!buf.undo());
    }

    #[test]
    fn transpose_rotates_selections() {
        assert_eq!(transposed("[one] [two] [three]"), "[three] [one] [two]");
        assert_eq!(
            transposed("[a]\n[\u{1f600}\u{1f600}]"),
            "[\u{1f600}\u{1f600}]\n[a]"
        );
        let (mut buf, mut carets) = parse("[x] [x]");
        transpose(&mut buf, &mut carets);
        assert_eq!(buf.top_step(), None, "equal texts are not replaced");
    }

    #[test]
    fn transpose_mixed_carets_and_one_selection_do_nothing() {
        for marked in ["[ab]c", "[ab] c|d", "a|b [cd]"] {
            let (mut buf, mut carets) = parse(marked);
            transpose(&mut buf, &mut carets);
            assert_eq!(show(&buf, &carets), marked);
            assert_eq!(buf.top_step(), None);
        }
    }

    fn line_ends(marked: &str) -> String {
        let (buf, mut carets) = parse(marked);
        carets_at_line_ends(&buf, &mut carets);
        show(&buf, &carets)
    }

    #[test]
    fn carets_on_selected_lines() {
        assert_eq!(line_ends("a[b\ncd\ne]f\ng"), "ab|\ncd|\nef|\ng");
        assert_eq!(line_ends("[\n\nx]y"), "|\n|\nxy|");
    }

    #[test]
    fn carets_selection_ending_at_a_line_start() {
        assert_eq!(line_ends("[ab\ncd\n]ef"), "ab|\ncd|\nef");
        // A caret at the start of a line goes to that line's end.
        assert_eq!(line_ends("ab\n[]cd"), "ab\ncd|");
    }

    #[test]
    fn carets_without_selection() {
        assert_eq!(line_ends("a|b\ncd"), "ab|\ncd");
        assert_eq!(line_ends("|"), "|");
    }

    #[test]
    fn carets_several_carets() {
        assert_eq!(line_ends("[a\nb]\nc\n|dd"), "a|\nb|\nc\ndd|");
        // Two carets in one line become one.
        assert_eq!(line_ends("|a|b"), "ab|");
    }

    #[test]
    fn carets_last_line_and_crlf() {
        assert_eq!(line_ends("[a\r\nbc]"), "a|\r\nbc|");
        assert_eq!(line_ends("[a\r\nbc\r\n]"), "a|\r\nbc|\r\n");
    }

    #[test]
    fn carets_primary() {
        // The caret at the end of its selection: the last line's caret is primary.
        let (buf, mut carets) = parse("[ab\ncd\nef]");
        carets_at_line_ends(&buf, &mut carets);
        assert_eq!(carets.carets().len(), 3);
        assert_eq!(carets.primary().head, 8);
        // The caret at the start of its selection: the first line's.
        let mut carets = Carets::from_carets(vec![Caret::select(8, 0)]);
        carets_at_line_ends(&buf, &mut carets);
        assert_eq!(carets.carets().len(), 3);
        assert_eq!(carets.primary().head, 2);
    }

    fn converted(marked: &str, tab_width: usize, tabs: bool) -> (usize, String) {
        let (mut buf, mut carets) = parse(marked);
        let n = convert_indents(&mut buf, &mut carets, tab_width, tabs);
        (n, show(&buf, &carets))
    }

    #[test]
    fn indents_mixed_to_spaces() {
        assert_eq!(
            converted("|\tx\n  \ty\n    z\n", 4, false),
            (2, "|    x\n    y\n    z\n".into())
        );
        // A tab after three spaces still ends at column 4; after four it ends at 8.
        assert_eq!(converted("|   \tx", 4, false), (1, "|    x".into()));
        assert_eq!(converted("|    \tx", 4, false), (1, "|        x".into()));
    }

    #[test]
    fn indents_to_tabs() {
        assert_eq!(
            converted("|      x\n\t  y\n  z\n", 4, true),
            (1, "|\t  x\n\t  y\n  z\n".into())
        );
        assert_eq!(converted("|        x", 4, true), (1, "|\t\tx".into()));
        // Spaces before a tab are absorbed by it.
        assert_eq!(converted("| \t x", 4, true), (1, "|\t x".into()));
    }

    #[test]
    fn indents_tab_after_text_stays() {
        assert_eq!(
            converted("|\tx\ty  z", 4, false),
            (1, "|    x\ty  z".into())
        );
        assert_eq!(converted("|x\ty", 4, false), (0, "|x\ty".into()));
        assert_eq!(converted("|    x    y", 4, true), (1, "|\tx    y".into()));
    }

    #[test]
    fn indents_blank_lines() {
        assert_eq!(
            converted("|\t\n \t \n\nx", 4, false),
            (2, "|    \n     \n\nx".into())
        );
    }

    #[test]
    fn indents_selection() {
        // The line the selection ends on is converted too, also from its first column.
        assert_eq!(
            converted("\ta\n[\tb\n]\tc\n\td", 4, false),
            (2, "\ta\n[    b\n]    c\n\td".into())
        );
        assert_eq!(
            converted("\ta\n\tb[b]\n\tc", 4, false),
            (1, "\ta\n    b[b]\n\tc".into())
        );
    }

    #[test]
    fn indents_selection_inside_an_indentation() {
        // IMP-0403: the eight columns are measured from the start of the line, not from the
        // start of the selection (which would leave one tab).
        assert_eq!(converted("    [    x\n]", 4, true), (1, "\t\t[x\n]".into()));
        assert_eq!(converted("\t[\tx]", 4, false), (1, "        [x]".into()));
    }

    #[test]
    fn indents_crlf() {
        assert_eq!(
            converted("|\tx\r\n\ty\r\n", 4, false),
            (2, "|    x\r\n    y\r\n".into())
        );
        assert_eq!(
            converted("    x\r\n[    y\r\n]", 4, true),
            (1, "    x\r\n[\ty\r\n]".into())
        );
    }

    #[test]
    fn indents_count_and_carets() {
        let (mut buf, mut carets) = parse("\tfoo|\n\tbar");
        assert_eq!(convert_indents(&mut buf, &mut carets, 4, false), 2);
        assert_eq!(show(&buf, &carets), "    foo|\n    bar");
        // Again: nothing to change, no second undo step.
        assert_eq!(convert_indents(&mut buf, &mut carets, 4, false), 0);
        assert!(buf.undo());
        assert_eq!(buf.text(), "\tfoo\n\tbar");
        assert!(!buf.undo());
        assert_eq!(indents_message(0, false), "Indentation already uses spaces");
        assert_eq!(indents_message(0, true), "Indentation already uses tabs");
        assert_eq!(indents_message(1, true), "Indentation converted in 1 line");
        assert_eq!(
            indents_message(3, false),
            "Indentation converted in 3 lines"
        );
    }

    #[test]
    fn indents_tab_widths() {
        assert_eq!(converted("|\t\tx", 2, false), (1, "|    x".into()));
        assert_eq!(converted("|        x", 8, true), (1, "|\tx".into()));
        assert_eq!(converted("|    x", 8, true), (0, "|    x".into()));
        assert_eq!(converted("|\tx", 8, false), (1, "|        x".into()));
        // A tab width of 0 counts as 1.
        assert_eq!(converted("|\t\tx", 0, false), (1, "|  x".into()));
        assert_eq!(indent_width(" \t ", 4), 5);
        assert_eq!(build_indent(5, 4, true), "\t ");
    }

    #[test]
    fn commands_run_through_perform() {
        let mut s = EditorState::new("b\na");
        assert!(perform(&mut s, "EditorSortLines"));
        assert_eq!(s.buffer.text(), "a\nb");
        assert_eq!(s.message, None);
        assert!(perform(&mut s, "EditorReverseLines"));
        assert_eq!(s.buffer.text(), "b\na");
        // Each is one undo step.
        assert!(perform(&mut s, "$Undo"));
        assert_eq!(s.buffer.text(), "a\nb");
        assert!(perform(&mut s, "$Undo"));
        assert_eq!(s.buffer.text(), "b\na");

        let mut s = EditorState::new("one");
        assert!(perform(&mut s, "EditorSortLines"));
        assert_eq!(
            s.message.as_deref(),
            Some("Sort Lines: fewer than two lines")
        );
        assert!(perform(&mut s, "EditorReverseLines"));
        assert_eq!(
            s.message.as_deref(),
            Some("Reverse Lines: fewer than two lines")
        );
        assert_eq!(s.buffer.top_step(), None);
        // The next command takes the message away.
        assert!(perform(&mut s, "EditorTextEnd"));
        assert_eq!(s.message, None);
        assert!(perform(&mut s, "EditorTranspose"));
        assert_eq!(s.buffer.text(), "oen");

        let mut s = EditorState::new("    x\n  y");
        s.tab_width = 2;
        assert!(perform(&mut s, "ConvertIndentsToTabs"));
        assert_eq!(s.buffer.text(), "\t\tx\n\ty");
        assert_eq!(
            s.message.as_deref(),
            Some("Indentation converted in 2 lines")
        );
        assert!(perform(&mut s, "ConvertIndentsToTabs"));
        assert_eq!(s.message.as_deref(), Some("Indentation already uses tabs"));
        assert!(perform(&mut s, "ConvertIndentsToSpaces"));
        assert_eq!(s.buffer.text(), "    x\n  y");

        let mut s = EditorState::new("ab\ncd");
        assert!(perform(&mut s, "$SelectAll"));
        assert!(perform(&mut s, "EditorAddCaretPerSelectedLine"));
        let heads: Vec<usize> = s.carets.carets().iter().map(|c| c.head).collect();
        assert_eq!(heads, [2, 5]);
        assert_eq!(s.buffer.top_step(), None);
    }
}

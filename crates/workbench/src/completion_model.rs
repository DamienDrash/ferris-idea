//! Completion list without toolkit (`SPEC-0072` behaviours 2–5).

use idecraft_core::{fuzzy_match, rank};
use idecraft_languages::insight::CompletionItem;

/// Whether `c` belongs to an identifier.
fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `typed` is still a name the open list narrows by (behaviours 2–3): letters, digits and
/// `_`; for class names and ids in an HTML value also `-` (`css_names`, v15 behaviour 52).
pub fn is_typed_name(typed: &str, css_names: bool) -> bool {
    typed
        .chars()
        .all(|c| is_ident(c) || (css_names && c == '-'))
}

/// Column (chars) where the identifier ending at `column` starts in `line` (behaviour 2).
pub fn word_start(line: &str, column: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut at = column.min(chars.len());
    while at > 0 && is_ident(chars[at - 1]) {
        at -= 1;
    }
    at
}

/// Text of an LSP snippet without its tab stops: `${1:x}` → `x`, `$1`/`$0` → nothing, `\$` → `$`
/// (behaviour 4).
pub fn snippet_text(snippet: &str) -> String {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '$' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                // `${n:default}` or `${n}`: keep the default text.
                let mut j = i + 2;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let mut depth = 1;
                let start = if j < chars.len() && chars[j] == ':' {
                    j + 1
                } else {
                    j
                };
                let mut k = start;
                while k < chars.len() {
                    match chars[k] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                out.push_str(&snippet_text(
                    &chars[start..k.min(chars.len())].iter().collect::<String>(),
                ));
                i = k + 1;
            }
            '$' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// A snippet expanded for the editor (`SPEC-0072` v7 behaviour 18): its text and the tab stops in
/// the order Tab visits them (`$1`, `$2`, …, `$0` last), each a group of char ranges in the text
/// (a stop written twice is edited in all places at once).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expanded {
    /// The text to insert.
    pub text: String,
    /// Tab stops in visiting order; ranges are char offsets into `text`.
    pub stops: Vec<Vec<std::ops::Range<usize>>>,
}

/// Expands an LSP snippet: placeholders keep their default text (choices their first entry),
/// variables such as `$TM_SELECTED_TEXT` their default or nothing, escapes their character; every
/// line after the first starts with `line_indent` (the indentation of the line the snippet goes
/// into) and each tab in the snippet becomes `indent_unit` (behaviour 18).
pub fn expand_snippet(snippet: &str, line_indent: &str, indent_unit: &str) -> Expanded {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out: Vec<char> = Vec::new();
    // (stop number, start, end) in `out`.
    let mut found: Vec<(u32, usize, usize)> = Vec::new();
    expand_into(&chars, line_indent, indent_unit, &mut out, &mut found);
    let mut numbers: Vec<u32> = found.iter().map(|f| f.0).collect();
    numbers.sort_unstable();
    numbers.dedup();
    // `$0` is visited last.
    if numbers.first() == Some(&0) {
        numbers.rotate_left(1);
    }
    let stops = numbers
        .iter()
        .map(|n| {
            found
                .iter()
                .filter(|f| f.0 == *n)
                .map(|f| f.1..f.2)
                .collect()
        })
        .collect();
    Expanded {
        text: out.into_iter().collect(),
        stops,
    }
}

/// A tab stop that names a new variable (`SPEC-0072` v8 behaviour 32): what it holds and where the
/// names come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameStop {
    /// The value itself (`int length = s.length()`) or one element (`for (String name : names)`).
    pub kind: idecraft_languages::names::NameKind,
    /// The expression the variable gets (after `=`, `:`, `of` or `in`), if any.
    pub expression: Option<String>,
    /// The declared type before the name, if any (`int`, `String`).
    pub type_name: Option<String>,
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Whether the stop at char range `stop` of the expanded `text` names a new variable, and if so its
/// expression and type: an identifier placeholder followed by `=`, `:`, `of` or `in`, after a
/// type, `const`, `let` or `var` (behaviour 32).
pub fn name_stop(text: &str, stop: &std::ops::Range<usize>) -> Option<NameStop> {
    use idecraft_languages::names::NameKind;
    let chars: Vec<char> = text.chars().collect();
    let name: String = chars.get(stop.clone())?.iter().collect();
    if name.is_empty()
        || !name.chars().all(is_name_char)
        || name.starts_with(|c: char| c.is_ascii_digit() || c.is_uppercase())
    {
        return None;
    }
    let line_start = chars[..stop.start]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |i| i + 1);
    let line_end = chars[stop.end..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(chars.len(), |i| stop.end + i);
    let before: String = chars[line_start..stop.start].iter().collect();
    let after: String = chars[stop.end..line_end].iter().collect();
    let before_t = before.trim_end();
    if before_t.len() == before.len() {
        // No space before the name: not a declaration.
        return None;
    }
    let previous: String = before_t
        .chars()
        .rev()
        .take_while(|c| is_name_char(*c) || matches!(c, '<' | '>' | '[' | ']' | ',' | '.' | '?'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if !previous.chars().any(is_name_char)
        || matches!(previous.as_str(), "return" | "new" | "throw" | "case")
    {
        return None;
    }
    let keyword = matches!(previous.as_str(), "const" | "let" | "var");
    let type_name = (!keyword).then(|| previous.clone());
    let rest = after.trim_start();
    let for_each = before.contains("for (") || before.contains("for(");
    let loop_end = |e: &str| e.trim().trim_end_matches(['{', ' ', ')']).trim().to_owned();
    let (kind, expression) = if let Some(e) = rest.strip_prefix(':') {
        if for_each {
            (NameKind::Element, loop_end(e))
        } else {
            // A type annotation (`const x: T = e`).
            let value = e.split_once('=').map_or("", |(_, v)| v);
            (
                NameKind::Value,
                value.trim().trim_end_matches(';').trim().to_owned(),
            )
        }
    } else if let Some(e) = rest.strip_prefix("of ") {
        (NameKind::Element, loop_end(e))
    } else if let Some(e) = rest.strip_prefix("in ") {
        // `for (… in x)`: keys, no elements to name.
        (NameKind::Value, loop_end(e))
    } else if rest.starts_with('=') && !rest.starts_with("==") {
        let e = rest[1..].split(';').next().unwrap_or_default();
        (NameKind::Value, e.trim().to_owned())
    } else {
        return None;
    };
    Some(NameStop {
        kind,
        expression: (!expression.is_empty()).then_some(expression),
        type_name,
    })
}

/// The suggested names at a name stop, best first; the stop's own text comes first unless it is the
/// collection's name with a digit (jdtls `names2`), which the best suggestion replaces (v8
/// behaviour 32).
pub fn stop_names(stop: &NameStop, current: &str) -> Vec<String> {
    let mut names = idecraft_languages::names::suggest(
        stop.kind,
        stop.expression.as_deref(),
        stop.type_name.as_deref(),
    );
    if !server_placeholder(stop, current) {
        names.retain(|n| n != current);
        names.insert(0, current.to_owned());
    }
    names
}

/// Whether `current` is the server's stand-in name for an element: the collection's own name with
/// a digit (`names2`).
pub fn server_placeholder(stop: &NameStop, current: &str) -> bool {
    let stem = current.trim_end_matches(|c: char| c.is_ascii_digit());
    !stem.is_empty()
        && stem.len() < current.len()
        && stop
            .expression
            .as_deref()
            .is_some_and(|e| e.trim_end_matches("()").ends_with(stem))
}

/// Puts `text` in place of every range of stop group `group` of `expanded`, moving the other
/// stops (v8 behaviour 32: the server's `names2` becomes `name`). Stops never overlap.
pub fn replace_stop(expanded: &mut Expanded, group: usize, text: &str) {
    let Some(ranges) = expanded.stops.get(group).cloned() else {
        return;
    };
    let mut sorted = ranges;
    sorted.sort_by_key(|r| r.start);
    let new_len = text.chars().count();
    let mut chars: Vec<char> = expanded.text.chars().collect();
    for r in sorted.iter().rev() {
        chars.splice(r.clone(), text.chars());
    }
    // How far a position moves: by every replaced range that ends at or before it.
    let moved = |p: usize| -> usize {
        let d: isize = sorted
            .iter()
            .filter(|r| r.end <= p && r.start < p)
            .map(|r| new_len as isize - r.len() as isize)
            .sum();
        (p as isize + d).max(0) as usize
    };
    for (g, stops) in expanded.stops.iter_mut().enumerate() {
        for r in stops.iter_mut() {
            if g == group {
                let start = moved(r.start);
                *r = start..start + new_len;
            } else {
                *r = moved(r.start)..moved(r.end);
            }
        }
    }
    expanded.text = chars.into_iter().collect();
}

/// Tab visits the declared name before its type (v8 behaviour 33): when the first stop is a
/// single type placeholder directly followed by a space and the stop of the name (jdtls `fori`:
/// `${1:int} ${2:i}`), the two change places.
pub fn name_first(expanded: &mut Expanded) {
    let (Some(first), Some(second)) = (expanded.stops.first(), expanded.stops.get(1)) else {
        return;
    };
    let ([ty], Some(name)) = (first.as_slice(), second.iter().min_by_key(|r| r.start)) else {
        return;
    };
    let chars: Vec<char> = expanded.text.chars().collect();
    let type_text: String = chars
        .get(ty.clone())
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    if name.start == ty.end + 1
        && chars.get(ty.end) == Some(&' ')
        && !type_text.is_empty()
        && type_text
            .chars()
            .all(|c| is_name_char(c) || matches!(c, '<' | '>' | '[' | ']' | ','))
        && name_stop(&expanded.text, name).is_some()
    {
        expanded.stops.swap(0, 1);
    }
}

/// The index of the `}` that closes a `${` whose body starts at `from`.
fn closing_brace(chars: &[char], from: usize) -> usize {
    let mut depth = 1;
    let mut k = from;
    while k < chars.len() {
        match chars[k] {
            '\\' => k += 1,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
        k += 1;
    }
    chars.len()
}

fn number(digits: &[char]) -> u32 {
    digits.iter().collect::<String>().parse().unwrap_or(0)
}

fn expand_into(
    chars: &[char],
    line_indent: &str,
    indent_unit: &str,
    out: &mut Vec<char>,
    found: &mut Vec<(u32, usize, usize)>,
) {
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '\n' => {
                out.push('\n');
                out.extend(line_indent.chars());
                i += 1;
            }
            '\t' => {
                out.extend(indent_unit.chars());
                i += 1;
            }
            '$' if chars.get(i + 1).is_some_and(char::is_ascii_digit) => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                found.push((number(&chars[i + 1..j]), out.len(), out.len()));
                i = j;
            }
            '$' if chars.get(i + 1) == Some(&'{') => {
                let end = closing_brace(chars, i + 2);
                let body = &chars[i + 2..end];
                let digits = body.iter().take_while(|c| c.is_ascii_digit()).count();
                let rest = &body[digits..];
                if digits > 0 {
                    let start = out.len();
                    match rest.first() {
                        Some(':') => expand_into(&rest[1..], line_indent, indent_unit, out, found),
                        // `${1|a,b|}`: the first choice.
                        Some('|') => {
                            out.extend(rest[1..].iter().take_while(|c| **c != ',' && **c != '|'))
                        }
                        _ => {}
                    }
                    found.push((number(&body[..digits]), start, out.len()));
                } else if let Some(colon) = body.iter().position(|c| *c == ':') {
                    // A variable with a default.
                    expand_into(&body[colon + 1..], line_indent, indent_unit, out, found);
                }
                i = end + 1;
            }
            '$' if chars
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_') =>
            {
                // `$TM_SELECTED_TEXT` and other variables: nothing.
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                i = j;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
}

/// Where the argument list at the end of an expanded suggestion starts (char index of its `(`):
/// Tab over a call that already has parentheses keeps those instead (`SPEC-0072` v7 behaviour 22).
pub fn own_arguments_start(text: &str) -> Option<usize> {
    if !text.ends_with(')') {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0usize;
    for (k, c) in chars.iter().enumerate().rev() {
        match c {
            ')' => depth += 1,
            '(' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// The order of overloads the server ranks alike: the one taking a variable number of arguments
/// first, then fewer parameters first (`SPEC-0072` v7 behaviour 17).
fn overload_key(item: &CompletionItem) -> (bool, usize) {
    let params = item
        .label_detail
        .as_deref()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let count = if params.is_empty() {
        0
    } else {
        params.matches(',').count() + 1
    };
    (!params.contains("..."), count)
}

/// A call inserted as `name($0)` whose resolved signature takes no arguments (`()`, `(self)`)
/// puts the caret after the parentheses instead (`SPEC-0072` v9 behaviour 37: `os.getcwd()|`,
/// `item.total_cents()|`).
pub fn caret_after_empty_call(item: &mut CompletionItem) {
    let Some(signature) = item.label_detail.as_deref() else {
        return;
    };
    if item.snippet
        && signature.trim_start().starts_with('(')
        && idecraft_languages::python_completion::passed_parameters(signature) == 0
        && let Some(name) = item.insert.strip_suffix("($0)")
    {
        item.insert = format!("{name}()$0");
    }
}

/// The server's ranking of `item`: preselected first, then items with sort text by it, ignoring
/// case first (pyright's `09.9999.abort` before `09.9999.CLD_CONTINUED`, `SPEC-0072` v9
/// behaviour 35), then the rest (`SPEC-0072` v5 behaviour 12).
fn server_rank(item: &CompletionItem) -> (bool, bool, String, &str) {
    let sort = item.sort_text.as_deref().unwrap_or_default();
    (
        !item.preselect,
        item.sort_text.is_none(),
        sort.to_lowercase(),
        sort,
    )
}

/// Items matching the typed `prefix`: case-insensitive prefix matches first in the server's
/// ranking (shorter first where it does not decide), then fuzzy matches; all items in the server's
/// ranking when the prefix is empty (behaviours 3 and 12).
pub fn filter(items: &[CompletionItem], prefix: &str) -> Vec<CompletionItem> {
    if prefix.is_empty() {
        let mut all: Vec<&CompletionItem> = items.iter().collect();
        all.sort_by(|a, b| {
            server_rank(a).cmp(&server_rank(b)).then_with(|| {
                if a.label.trim() == b.label.trim() {
                    overload_key(a).cmp(&overload_key(b))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        });
        return all.into_iter().cloned().collect();
    }
    let lower = prefix.to_lowercase();
    let mut starts: Vec<&CompletionItem> = Vec::new();
    let mut fuzzy: Vec<(i64, &CompletionItem)> = Vec::new();
    for item in items {
        let label = item.label.trim();
        if label.to_lowercase().starts_with(&lower) {
            starts.push(item);
        } else if let Some(m) = fuzzy_match(prefix, label) {
            fuzzy.push((m.score, item));
        }
    }
    // A suggestion named exactly as typed comes first (`SPEC-0072` v7 behaviour 23: `.for` before
    // `forEach`).
    // The same case before other case (`Path` before `path` for `Path`, v9 behaviour 35).
    let exact = |i: &CompletionItem| {
        (
            i.label.trim().to_lowercase() != lower,
            i.label.trim() != prefix,
        )
    };
    starts.sort_by(|a, b| {
        exact(a)
            .cmp(&exact(b))
            .then_with(|| server_rank(a).cmp(&server_rank(b)))
            .then_with(|| a.label.trim().len().cmp(&b.label.trim().len()))
            .then_with(|| a.label.trim().cmp(b.label.trim()))
            .then_with(|| overload_key(a).cmp(&overload_key(b)))
    });
    fuzzy.sort_by(|a, b| rank((a.0, a.1.label.trim()), (b.0, b.1.label.trim())));
    starts
        .into_iter()
        .chain(fuzzy.into_iter().map(|(_, i)| i))
        .cloned()
        .collect()
}

/// Whether the text before the caret asks for the list to open by itself: an identifier that does
/// not start with a digit, or a member access `.`, `->` or `::` (SPEC-0072 v3 behaviour 8).
pub fn auto_trigger(before: &str) -> bool {
    if before.ends_with('.') || before.ends_with("->") || before.ends_with("::") {
        // Not a number like `1.` or a range `..`.
        let rest = &before[..before.len() - 1];
        return before.ends_with("->")
            || before.ends_with("::")
            || rest
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ')' || c == ']')
                && !rest
                    .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .is_some_and(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()));
    }
    let word: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    word.chars()
        .last()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
}

/// Whether the text before the caret in an SQL file asks for the table list by itself: one space
/// typed right after the word `FROM`, `JOIN`, `INTO` or `UPDATE` (any case; `SPEC-0014` v3
/// behaviour 17). Other words, a second space or a qualified name (`x.from `) do not.
pub fn sql_auto_trigger(before: &str) -> bool {
    let Some(head) = before.strip_suffix(' ') else {
        return false;
    };
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(head.len(), |(i, _)| i);
    let word = head[start..].to_ascii_lowercase();
    matches!(word.as_str(), "from" | "join" | "into" | "update") && !head[..start].ends_with('.')
}

/// `text` as one line for a row of the list: line breaks and runs of whitespace become single
/// spaces, ends trimmed (`SPEC-0072` v4 behaviour 9).
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The icon of a completion kind (`SPEC-0072` v5 behaviour 10): the Structure view's symbol icons,
/// the database icons for SQL items, the template icon for live and postfix templates (v8
/// behaviour 31); `None` for kinds without one (keywords, text, plugins), which leave the place
/// empty.
pub fn kind_icon(kind: Option<&str>) -> Option<&'static str> {
    match kind? {
        "column" => Some("nodes/dataColumn"),
        "table" | "view" | "join" => Some("nodes/dataTables"),
        "schema" => Some("nodes/dataSchema"),
        "snippet" => Some("nodes/template"),
        "keyword" | "text" | "plugin" | "color" | "file" | "folder" | "unit" => None,
        "value" => Some("nodes/constant"),
        "reference" => Some("nodes/variable"),
        "struct" => Some("nodes/class"),
        other => Some(idecraft_ui::icons::symbol_icon(other)),
    }
}

/// What one row of the list shows (`SPEC-0072` v5 behaviours 10–11), each piece on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Kind icon key, if the kind has one.
    pub icon: Option<&'static str>,
    /// The label.
    pub label: String,
    /// Char positions in `label` matching the typed prefix, drawn highlighted.
    pub matched: Vec<usize>,
    /// The signature part right after the label (grey).
    pub signature: String,
    /// The type at the right edge (grey), followed by the origin of a member when known
    /// (`int · Item`, v10 behaviour 45).
    pub type_text: String,
}

/// The row for `item` with `prefix` typed.
pub fn row(item: &CompletionItem, prefix: &str) -> Row {
    let label = one_line(&item.label);
    Row {
        icon: kind_icon(item.kind.as_deref()),
        matched: matched_positions(&label, prefix),
        signature: one_line(item.label_detail.as_deref().unwrap_or_default()),
        type_text: {
            let ty = one_line(
                item.description
                    .as_deref()
                    .or(item.detail.as_deref())
                    .unwrap_or_default(),
            );
            // The class a member comes from follows the type (SPEC-0072 v10 behaviour 45).
            match item.origin.as_deref().map(one_line) {
                Some(origin) if !origin.is_empty() && ty.is_empty() => origin,
                Some(origin) if !origin.is_empty() => format!("{ty} · {origin}"),
                _ => ty,
            }
        },
        label,
    }
}

/// Char positions of `label` that match `prefix`: its leading characters when the label starts
/// with it (ignoring case), the fuzzy match's characters otherwise (behaviour 11).
pub fn matched_positions(label: &str, prefix: &str) -> Vec<usize> {
    if prefix.is_empty() {
        return Vec::new();
    }
    if label.to_lowercase().starts_with(&prefix.to_lowercase()) {
        return (0..prefix.chars().count()).collect();
    }
    fuzzy_match(prefix, label).map_or_else(Vec::new, |m| m.positions)
}

/// How many chars of the identifier after the caret Tab replaces (`SPEC-0072` v5 behaviour 14):
/// the identifier characters at the start of `after`, the line from the caret on.
pub fn identifier_rest(after: &str) -> usize {
    after.chars().take_while(|c| is_ident(*c)).count()
}

/// The range a suggestion replaces at a caret other than the primary one (`SPEC-0025` v5
/// behaviour 10): its selection, or at an empty caret `typed` chars before it (not past
/// `line_start`) and `rest` after it.
pub fn caret_range(
    anchor: usize,
    head: usize,
    typed: usize,
    rest: usize,
    line_start: usize,
) -> std::ops::Range<usize> {
    if anchor != head {
        return anchor.min(head)..anchor.max(head);
    }
    head.saturating_sub(typed).max(line_start.min(head))..head + rest
}

/// Where each of `edits` (sorted, not overlapping) ends once all of them are applied.
pub fn ends_after(edits: &[(std::ops::Range<usize>, String)]) -> Vec<usize> {
    let mut shift = 0isize;
    edits
        .iter()
        .map(|(r, t)| {
            let n = t.chars().count();
            let end = (r.start as isize + shift) as usize + n;
            shift += n as isize - r.len() as isize;
            end
        })
        .collect()
}

/// `text` cut to `max` characters, the last one an ellipsis when it was longer.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Characters kept between the signature and the type.
const TYPE_GAP: usize = 2;

/// The signature and type of a row whose texts get `width` characters (`SPEC-0072` v6 behaviour
/// 15): the signature gives way first (cut with "…", left out below two characters), the type
/// stays whole unless label and type alone do not fit.
pub fn fit_row(label: &str, signature: &str, type_text: &str, width: usize) -> (String, String) {
    let (l, s, t) = (
        label.chars().count(),
        signature.chars().count(),
        type_text.chars().count(),
    );
    let gap = if t > 0 { TYPE_GAP } else { 0 };
    if l + s + gap + t <= width {
        return (signature.to_owned(), type_text.to_owned());
    }
    let room = width.saturating_sub(l + gap + t);
    if room >= 2 {
        return (cut(signature, room), type_text.to_owned());
    }
    if l + gap + t <= width {
        return (String::new(), type_text.to_owned());
    }
    (
        String::new(),
        cut(type_text, width.saturating_sub(l + gap).max(1)),
    )
}

/// Where the completion list and its documentation panel go (`SPEC-0072` v6 behaviour 16), in
/// window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Left edge of the list.
    pub list_x: f32,
    /// Left edge and width of the panel; `None` when no side has room.
    pub panel: Option<(f32, f32)>,
}

/// Distance kept from the window edges.
pub const WINDOW_MARGIN: f32 = 8.0;
/// Narrowest documentation panel shown.
pub const MIN_PANEL_WIDTH: f32 = 160.0;

/// The placement of a list of `list_w` wanted at `at_x` with a panel of `panel_w` beside it, `gap`
/// apart, in a window `window_w` wide whose editor area begins at `editor_left`.
///
/// The panel prefers the right side of the list (`SPEC-0072` v12 behaviour 16): there when its
/// whole width fits into the window; else on the left with its whole width, when that stays
/// inside the editor area — never over a tool window left of the editor; else narrowed on the
/// side with more room (the right one when both have the same), not below [`MIN_PANEL_WIDTH`];
/// else there is none. It never lies over the list.
pub fn doc_placement(
    window_w: f32,
    editor_left: f32,
    at_x: f32,
    list_w: f32,
    panel_w: f32,
    gap: f32,
) -> Placement {
    let list_x = at_x
        .min(window_w - WINDOW_MARGIN - list_w)
        .max(WINDOW_MARGIN);
    // The left side ends at the editor area, the right side at the window.
    let leftmost = editor_left.max(WINDOW_MARGIN);
    let right_room = window_w - WINDOW_MARGIN - (list_x + list_w + gap);
    let left_room = list_x - gap - leftmost;
    let panel = if right_room >= panel_w {
        Some((list_x + list_w + gap, panel_w))
    } else if left_room >= panel_w {
        Some((list_x - gap - panel_w, panel_w))
    } else if right_room.max(left_room) < MIN_PANEL_WIDTH {
        None
    } else if right_room >= left_room {
        Some((list_x + list_w + gap, right_room))
    } else {
        Some((leftmost, left_room))
    };
    Placement { list_x, panel }
}

/// The hint line at the bottom of the list (behaviour 14).
pub const HINT: &str = "⏎ insert  ·  ⇥ replace";

/// A key the open completion list takes (behaviour 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    /// Select the next item.
    Down,
    /// Select the previous item.
    Up,
    /// Insert the selected item in front of the rest of the identifier (Enter).
    Accept,
    /// Insert the selected item over the rest of the identifier too (Tab, v5 behaviour 14).
    Replace,
    /// Insert the selected item and type `.` behind it (⌃., v13 behaviour 49).
    AcceptDot,
    /// Close the list (Escape).
    Close,
    /// The focus left the editor: close the list and let the key go on to where the focus is.
    CloseAndPass,
}

/// The keymap action that inserts the selected suggestion (Enter in the bundled keymaps; v11
/// behaviour 47).
pub const CHOOSE: &str = "EditorChooseLookupItem";
/// The keymap action that inserts it over the rest of the identifier too (Tab in the bundled
/// keymaps; v11 behaviour 47).
pub const CHOOSE_REPLACE: &str = "EditorChooseLookupItemReplace";
/// The keymap action that inserts it and types `.` behind it (⌃. in the bundled keymaps; v13
/// behaviour 49).
pub const CHOOSE_DOT: &str = "EditorChooseLookupItemDot";

/// The keymap action of class name completion (⌃⌥Space; v13 behaviour 48).
pub const CLASS_NAME_COMPLETION: &str = "ClassNameCompletion";

/// The server's kinds that class name completion keeps (v13 behaviour 48).
pub const CLASS_KINDS: [&str; 4] = ["class", "interface", "enum", "struct"];

/// The suggestions of `items` that name a type, in their order (v13 behaviour 48).
pub fn class_names(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .filter(|i| i.kind.as_deref().is_some_and(|k| CLASS_KINDS.contains(&k)))
        .collect()
}

/// The keymap actions that move between the places of an inserted template (Tab and Enter, ⇧Tab
/// in the bundled keymaps; v14 behaviour 51).
pub const NEXT_TEMPLATE_VARIABLE: &str = "NextTemplateVariable";
/// See [`NEXT_TEMPLATE_VARIABLE`].
pub const PREVIOUS_TEMPLATE_VARIABLE: &str = "PreviousTemplateVariable";

/// The status line when no live template applies at the caret (v14 behaviour 50).
pub const NO_LIVE_TEMPLATES: &str = "No live templates here";

/// The suggestions of `items` that are templates (the server's kind `snippet`), in their order
/// (v14 behaviour 50).
pub fn live_templates(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .filter(|i| i.kind.as_deref() == Some("snippet"))
        .collect()
}

/// Which suggestions a completion request shows: all, the type names (v13 behaviour 48) or the
/// live templates (v14 behaviour 50).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// Every suggestion.
    All,
    /// The type names.
    Classes,
    /// The live templates.
    Templates,
}

/// What the open completion list does with `key` (toolkit name) pressed with or without
/// modifiers, `bound` being the keymap actions of the pressed keystroke. It takes keys only while
/// its editor has the focus; once a tool window has it (⌘6 opened Problems), Down and Enter belong
/// to the window, not to the list (hand test 7 of round 4). Inserting follows the keymap (v11
/// behaviour 47): a key bound to [`CHOOSE`] or [`CHOOSE_REPLACE`] does that, with or without
/// modifiers; Up, Down and Escape are the list's fixed keys.
pub fn list_key(
    editor_focused: bool,
    key: &str,
    modified: bool,
    bound: &[&str],
) -> Option<ListKey> {
    if !editor_focused {
        return Some(ListKey::CloseAndPass);
    }
    if bound.contains(&CHOOSE_DOT) {
        return Some(ListKey::AcceptDot);
    }
    if bound.contains(&CHOOSE_REPLACE) {
        return Some(ListKey::Replace);
    }
    if bound.contains(&CHOOSE) {
        return Some(ListKey::Accept);
    }
    if modified {
        return None;
    }
    match key {
        "down" => Some(ListKey::Down),
        "up" => Some(ListKey::Up),
        "escape" => Some(ListKey::Close),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0025 AK-14 (v5): the ranges a suggestion replaces at the other carets and where the
    /// carets stand after the texts went in.
    #[test]
    fn a_suggestion_goes_in_at_every_caret() {
        use super::{caret_range, ends_after};
        // `ar|` typed at an empty caret: the two chars before it.
        assert_eq!(caret_range(12, 12, 2, 0, 10), 10..12);
        // With Tab also the rest of the identifier after it.
        assert_eq!(caret_range(12, 12, 2, 3, 10), 10..15);
        // Never back into the line before.
        assert_eq!(caret_range(11, 11, 2, 0, 10), 10..11);
        // A selection is replaced as it is, in either direction.
        assert_eq!(caret_range(20, 16, 2, 3, 10), 16..20);
        // `ar` at 0..2, 5..7 and 10..12 become `area`.
        let edits = vec![
            (0..2, "area".to_owned()),
            (5..7, "area".to_owned()),
            (10..12, "area".to_owned()),
        ];
        assert_eq!(ends_after(&edits), vec![4, 11, 18]);
        // An import before them moves them all.
        let edits = vec![(0..0, "use a;\n".to_owned()), (9..10, "xy".to_owned())];
        assert_eq!(ends_after(&edits), vec![7, 18]);
    }

    use super::*;

    /// SPEC-0072 AK-33 (v14): only the server's templates stay, in their order.
    #[test]
    fn live_templates_keep_snippets_only() {
        let item = |label: &str, kind: &str| CompletionItem {
            label: label.into(),
            insert: label.into(),
            kind: Some(kind.into()),
            ..Default::default()
        };
        let kept = live_templates(vec![
            item("tfn", "snippet"),
            item("main", "function"),
            item("tmod", "snippet"),
            item("Shop", "struct"),
        ]);
        let labels: Vec<&str> = kept.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["tfn", "tmod"]);
    }

    /// SPEC-0072 v6 AK-12: the signature gives way before the type.
    #[test]
    fn long_signature_gives_way_to_the_type() {
        let fits = fit_row("of", "(E e1)", "List<E>", 40);
        assert_eq!(fits, ("(E e1)".to_owned(), "List<E>".to_owned()));
        let sig = "(E e1, E e2, E e3, E e4, E e5, E e6, E e7, E e8, E e9, E e10)";
        let (s, t) = fit_row("of", sig, "List<E>", 30);
        assert_eq!(t, "List<E>", "the type stays whole");
        assert!(s.ends_with('…'));
        assert_eq!(2 + s.chars().count() + 2 + 7, 30);
        // No room for two signature characters: left out.
        assert_eq!(
            fit_row("of", sig, "List<E>", 12),
            (String::new(), "List<E>".to_owned())
        );
        // Label and type do not fit: the type is cut too, the label never.
        let (s, t) = fit_row("averyveryverylongname", sig, "Map<String, Object>", 30);
        assert_eq!(s, "");
        assert!(t.ends_with('…'));
        assert_eq!(21 + 2 + t.chars().count(), 30);
        // Without a type the signature takes the rest.
        let (s, t) = fit_row("of", sig, "", 20);
        assert_eq!((s.chars().count(), t.as_str()), (18, ""));
    }

    /// SPEC-0072 v6 AK-12: the documentation panel stays in the window.
    #[test]
    fn documentation_panel_stays_in_the_window() {
        // Room on the right.
        let p = doc_placement(1600.0, 0.0, 200.0, 460.0, 380.0, 4.0);
        assert_eq!(
            p,
            Placement {
                list_x: 200.0,
                panel: Some((664.0, 380.0))
            }
        );
        // Near the right edge: the panel goes left.
        let p = doc_placement(1600.0, 0.0, 1000.0, 460.0, 380.0, 4.0);
        assert_eq!(p.list_x, 1000.0);
        assert_eq!(p.panel, Some((616.0, 380.0)));
        // The list itself would cross the edge: it moves in.
        let p = doc_placement(1600.0, 0.0, 1500.0, 460.0, 380.0, 4.0);
        assert_eq!(p.list_x, 1600.0 - 8.0 - 460.0);
        assert_eq!(p.panel, Some((p.list_x - 4.0 - 380.0, 380.0)));
        // Neither side has the full width: the wider one, narrowed.
        let p = doc_placement(1000.0, 0.0, 300.0, 460.0, 380.0, 4.0);
        let (x, w) = p.panel.unwrap();
        assert_eq!((x, w), (8.0, 300.0 - 4.0 - 8.0), "left has 288, right 228");
        assert!(x + w <= 1000.0 - 8.0);
        // No side with 160 px: no panel.
        assert_eq!(
            doc_placement(600.0, 0.0, 60.0, 460.0, 380.0, 4.0).panel,
            None
        );
        // Every placement stays inside.
        for at in (0..2000).step_by(50) {
            let p = doc_placement(1400.0, 0.0, at as f32, 460.0, 380.0, 4.0);
            assert!(p.list_x >= 8.0 && p.list_x + 460.0 <= 1392.0);
            if let Some((x, w)) = p.panel {
                assert!(x >= 8.0 && x + w <= 1392.0 && w >= MIN_PANEL_WIDTH, "{at}");
                assert!(
                    x + w <= p.list_x || x >= p.list_x + 460.0,
                    "beside, not over"
                );
            }
        }
    }

    /// SPEC-0072 v12 AK-35 (window check of 2026-11-14): with a tool window left of the editor
    /// the panel stays off it — right of the list, narrowed when the window ends there — and
    /// goes left only inside the editor area; it never lies over the list.
    #[test]
    fn documentation_panel_prefers_the_right_and_stays_off_the_tool_window_on_the_left() {
        // As in the check: a window 1300 wide, the Commit window to x = 450, the list at 520.
        // Right of the list are 308 px, left of it inside the editor only 66.
        let p = doc_placement(1300.0, 450.0, 520.0, 460.0, 380.0, 4.0);
        assert_eq!(p.list_x, 520.0);
        assert_eq!(
            p.panel,
            Some((984.0, 308.0)),
            "right, narrowed to the window"
        );
        // Before v12 the same list had its panel at x = 136, over the tool window.
        let (x, _) = p.panel.unwrap();
        assert!(x >= 450.0);
        // The whole width fits on the left inside the editor: there, when the right has less.
        let p = doc_placement(1600.0, 300.0, 1100.0, 460.0, 380.0, 4.0);
        assert_eq!(p.panel, Some((1100.0 - 4.0 - 380.0, 380.0)));
        // Both sides are short: the roomier one, the right one when they are equal.
        let p = doc_placement(1300.0, 300.0, 520.0, 460.0, 380.0, 4.0);
        assert_eq!(p.panel, Some((984.0, 308.0)), "right 308, left 216");
        let p = doc_placement(1200.0, 100.0, 420.0, 460.0, 380.0, 4.0);
        assert_eq!(p.panel, Some((100.0, 316.0)), "right 308, left 316");
        let p = doc_placement(1200.0, 108.0, 420.0, 460.0, 380.0, 4.0);
        assert_eq!(p.panel, Some((884.0, 308.0)), "308 on both sides: right");
        // No side with 160 px inside its bounds: no panel rather than one over the tool window.
        assert_eq!(
            doc_placement(1000.0, 400.0, 420.0, 460.0, 380.0, 4.0).panel,
            None,
            "right 108, left 16"
        );
        // Wherever the list is and wherever the editor begins: inside the window, off the list,
        // and on the left never before the editor area.
        for editor_left in [0.0_f32, 48.0, 300.0, 450.0, 700.0] {
            for at in (0..2000).step_by(25) {
                let p = doc_placement(1372.0, editor_left, at as f32, 460.0, 380.0, 4.0);
                assert!(p.list_x >= 8.0 && p.list_x + 460.0 <= 1364.0);
                let Some((x, w)) = p.panel else { continue };
                assert!(w >= MIN_PANEL_WIDTH && x >= 8.0 && x + w <= 1364.0, "{at}");
                let right = x >= p.list_x + 460.0;
                assert!(right || x + w <= p.list_x, "beside, not over ({at})");
                assert!(
                    right || x >= editor_left,
                    "off the tool window ({editor_left}, {at})"
                );
            }
        }
    }

    #[test]
    fn the_list_takes_keys_only_while_its_editor_has_the_focus() {
        let enter = ["EditorEnter", CHOOSE];
        assert_eq!(list_key(true, "down", false, &[]), Some(ListKey::Down));
        assert_eq!(
            list_key(true, "enter", false, &enter),
            Some(ListKey::Accept)
        );
        assert_eq!(list_key(true, "escape", false, &[]), Some(ListKey::Close));
        assert_eq!(
            list_key(true, "down", true, &[]),
            None,
            "⌥↓ and the like go on"
        );
        assert_eq!(list_key(true, "a", false, &[]), None);
        // Focus in the Problems window: the keys are the window's.
        for key in ["down", "enter", "escape", "a"] {
            assert_eq!(
                list_key(false, key, false, &enter),
                Some(ListKey::CloseAndPass)
            );
        }
    }

    /// SPEC-0072 v11 AK-30: inserting follows the keymap; Up, Down and Escape are fixed keys.
    #[test]
    fn list_keys_follow_the_keymap() {
        use idecraft_ui::keymap::{Keymap, Keystroke};
        let stroke = |text: &str| Keystroke::parse(text).expect("a keystroke");
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let enter = keymap.actions_for(&stroke("ENTER"), None);
            let tab = keymap.actions_for(&stroke("TAB"), None);
            assert_eq!(
                list_key(true, "enter", false, &enter),
                Some(ListKey::Accept)
            );
            assert_eq!(list_key(true, "tab", false, &tab), Some(ListKey::Replace));
            // ⌃. inserts with a dot (v13 behaviour 49).
            let dot = keymap.actions_for(&stroke("control PERIOD"), None);
            assert_eq!(
                list_key(true, "period", true, &dot),
                Some(ListKey::AcceptDot)
            );
            // ⇧Enter and ⇧Tab are not the list's.
            for (key, text) in [("enter", "shift ENTER"), ("tab", "shift TAB")] {
                let bound = keymap.actions_for(&stroke(text), None);
                assert_eq!(list_key(true, key, true, &bound), None, "{text}");
            }
            for (key, text, action) in [
                ("down", "DOWN", ListKey::Down),
                ("up", "UP", ListKey::Up),
                ("escape", "ESCAPE", ListKey::Close),
            ] {
                let bound = keymap.actions_for(&stroke(text), None);
                assert_eq!(list_key(true, key, false, &bound), Some(action), "{text}");
            }
        }
        // A rebound key acts with its modifier; a key that lost the action is the editor's.
        assert_eq!(
            list_key(true, "y", true, &[CHOOSE]),
            Some(ListKey::Accept),
            "⌃Y bound to the action"
        );
        assert_eq!(
            list_key(true, "space", true, &[CHOOSE_REPLACE, CHOOSE]),
            Some(ListKey::Replace),
            "a key bound to both replaces"
        );
        assert_eq!(list_key(true, "enter", false, &["EditorEnter"]), None);
        assert_eq!(list_key(true, "tab", false, &["EditorTab"]), None);
        assert_eq!(
            list_key(false, "y", true, &[CHOOSE]),
            Some(ListKey::CloseAndPass)
        );
    }

    /// SPEC-0072 AK-31 (v13): class name completion keeps the types, in their order.
    #[test]
    fn class_names_keep_types_only() {
        let of = |label: &str, kind: Option<&str>| CompletionItem {
            label: label.into(),
            kind: kind.map(str::to_owned),
            insert: label.into(),
            ..Default::default()
        };
        let kept = class_names(vec![
            of("ArrayList", Some("class")),
            of("add", Some("method")),
            of("List", Some("interface")),
            of("size", Some("field")),
            of("Color", Some("enum")),
            of("Point", Some("struct")),
            of("java", Some("module")),
            of("plain", None),
        ]);
        let labels: Vec<&str> = kept.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["ArrayList", "List", "Color", "Point"]);
        assert!(class_names(vec![of("x", Some("variable"))]).is_empty());
    }

    fn item(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            kind: Some("function".into()),
            insert: label.into(),
            ..Default::default()
        }
    }

    /// SPEC-0072/AK-36 (v15, behaviour 52): in an HTML class value the typed name takes `-`
    /// along; white space and quotes end it; elsewhere `-` ends it as before.
    #[test]
    fn css_names_keep_their_hyphen() {
        assert!(is_typed_name("card-w", true));
        assert!(is_typed_name("--x_1", true));
        assert!(is_typed_name("", true));
        assert!(!is_typed_name("card w", true));
        assert!(!is_typed_name("card\"", true));
        assert!(!is_typed_name("card-w", false));
        assert!(is_typed_name("card_w2", false));
    }

    #[test]
    fn kind_icons() {
        assert_eq!(kind_icon(Some("method")), Some("nodes/method"));
        assert_eq!(kind_icon(Some("class")), Some("nodes/class"));
        assert_eq!(kind_icon(Some("column")), Some("nodes/dataColumn"));
        assert_eq!(kind_icon(Some("keyword")), None);
        // SPEC-0072 v8 AK-20: templates have the template icon.
        assert_eq!(kind_icon(Some("snippet")), Some("nodes/template"));
        assert_eq!(kind_icon(None), None);
    }

    /// SPEC-0072 v8 AK-20: placeholders that name a new variable, with expression and type.
    #[test]
    fn name_placeholders() {
        use idecraft_languages::names::NameKind;
        let var = expand_snippet("int ${1:length} = s.length();${0}", "", "    ");
        let stop = name_stop(&var.text, &var.stops[0][0]).unwrap();
        assert_eq!(stop.kind, NameKind::Value);
        assert_eq!(stop.expression.as_deref(), Some("s.length()"));
        assert_eq!(stop.type_name.as_deref(), Some("int"));
        assert_eq!(stop_names(&stop, "length"), ["length", "i", "j", "k"]);
        let each = expand_snippet(
            "for (${1:String} ${2:names2} : ${3:names}) {\n\t$0\n}",
            "",
            "    ",
        );
        let stop = name_stop(&each.text, &each.stops[1][0]).unwrap();
        assert_eq!(stop.kind, NameKind::Element);
        assert_eq!(stop.expression.as_deref(), Some("names"));
        assert!(server_placeholder(&stop, "names2"));
        assert_eq!(stop_names(&stop, "names2")[0], "name");
        // The type placeholder and a plain argument are no names.
        assert_eq!(name_stop(&each.text, &each.stops[0][0]), None);
        let call = expand_snippet("System.out.println(${1:x});", "", "    ");
        assert_eq!(name_stop(&call.text, &call.stops[0][0]), None);
        let ts = expand_snippet("const ${1:name} = item.name;$0", "", "  ");
        let stop = name_stop(&ts.text, &ts.stops[0][0]).unwrap();
        assert_eq!((stop.kind, stop.type_name), (NameKind::Value, None));
        let of = expand_snippet("for (const ${1:item} of items) {\n\t$0\n}", "", "  ");
        assert_eq!(
            name_stop(&of.text, &of.stops[0][0]).unwrap().kind,
            NameKind::Element
        );
        // Replacing the server's name moves the later stops.
        let mut each = each;
        replace_stop(&mut each, 1, "name");
        assert_eq!(each.text, "for (String name : names) {\n    \n}");
        assert_eq!(each.stops[1], vec![12..16]);
        assert_eq!(each.stops[2], vec![19..24]);
        assert_eq!(each.stops[0], vec![5..11]);
    }

    /// SPEC-0072 v8 AK-20: jdtls's `fori` starts at the name, not the type.
    #[test]
    fn type_stop_after_name_stop() {
        let mut fori = expand_snippet(
            "for (${1:int} ${2:i} = ${3:0}; ${2:i} < ${4:array.length}; ${2:i}++) {\n\t$0\n}",
            "",
            "    ",
        );
        name_first(&mut fori);
        assert_eq!(fori.stops[0].len(), 3, "the name in all three places first");
        assert_eq!(fori.stops[1], vec![5..8], "then the type");
        // A snippet that starts with its name stays.
        let mut var = expand_snippet("int ${1:length} = s.length();${0}", "", "    ");
        let before = var.clone();
        name_first(&mut var);
        assert_eq!(var, before);
    }

    /// SPEC-0072 v10 AK-29: the origin of a member follows the type in the right column.
    #[test]
    fn row_shows_the_origin_after_the_type() {
        let item = |description: Option<&str>, origin: Option<&str>| CompletionItem {
            label: "total".into(),
            label_detail: Some("()".into()),
            description: description.map(str::to_owned),
            origin: origin.map(str::to_owned),
            ..Default::default()
        };
        let both = row(&item(Some("float"), Some("Item")), "");
        assert_eq!(
            (both.signature.as_str(), both.type_text.as_str()),
            ("()", "float · Item")
        );
        assert_eq!(row(&item(None, Some("object")), "").type_text, "object");
        assert_eq!(row(&item(Some("float"), None), "").type_text, "float");
        assert_eq!(row(&item(Some("float"), Some(" ")), "").type_text, "float");
        // Too long for the row: the signature gives way, then the right column is cut.
        let (signature, right) = fit_row("total", "()", "float · Item", 14);
        assert_eq!((signature.as_str(), right.as_str()), ("", "float …"));
    }

    #[test]
    fn row_pieces_from_label_details() {
        let of = CompletionItem {
            label: "of".into(),
            kind: Some("method".into()),
            detail: Some("java.util.List.of(E... elements) : List<E>".into()),
            label_detail: Some("(E... elements)".into()),
            description: Some("List<E>".into()),
            ..Default::default()
        };
        let r = row(&of, "o");
        assert_eq!(r.icon, Some("nodes/method"));
        assert_eq!(r.label, "of");
        assert_eq!(r.matched, [0]);
        assert_eq!(r.signature, "(E... elements)");
        assert_eq!(
            r.type_text, "List<E>",
            "the description, not the long detail"
        );
        // Without label details the detail stands at the right.
        let plain = CompletionItem {
            label: "helper".into(),
            detail: Some("int".into()),
            ..Default::default()
        };
        let r = row(&plain, "");
        assert_eq!((r.signature.as_str(), r.type_text.as_str()), ("", "int"));
        assert!(r.matched.is_empty());
    }

    #[test]
    fn prefix_positions_for_highlight() {
        assert_eq!(matched_positions("helper", "HEL"), [0, 1, 2]);
        assert_eq!(matched_positions("toString", "tS"), [0, 2]);
        assert!(matched_positions("main", "zz").is_empty());
    }

    /// SPEC-0072 v9 AK-25 (behaviour 35): sort text ignoring case, then the typed case first.
    #[test]
    fn sort_text_ignores_case_and_typed_case_first() {
        let ranked = |label: &str, sort: &str| CompletionItem {
            sort_text: Some(sort.into()),
            ..item(label)
        };
        let items = [
            ranked("CLD_CONTINUED", "09.9999.CLD_CONTINUED"),
            ranked("abort", "09.9999.abort"),
            ranked("altsep", "09.9999.altsep"),
            ranked("AT_NO_AUTOMOUNT", "09.9999.AT_NO_AUTOMOUNT"),
        ];
        let labels: Vec<String> = filter(&items, "").into_iter().map(|i| i.label).collect();
        assert_eq!(
            labels,
            ["abort", "altsep", "AT_NO_AUTOMOUNT", "CLD_CONTINUED"]
        );
        let paths = [
            ranked("path", "09.9999.path"),
            ranked("PathLike", "15.9999.PathLike.05.os"),
            ranked("Path", "15.9999.Path.99.pathlib"),
        ];
        let l =
            |p: &str| -> Vec<String> { filter(&paths, p).into_iter().map(|i| i.label).collect() };
        assert_eq!(l("Path"), ["Path", "path", "PathLike"]);
        assert_eq!(l("path"), ["path", "Path", "PathLike"]);
    }

    /// SPEC-0072 v9 AK-25 (behaviour 37): no arguments, caret after the parentheses.
    #[test]
    fn caret_after_a_call_without_arguments() {
        let call = |sig: Option<&str>| {
            let mut i = CompletionItem {
                insert: "getcwd($0)".into(),
                snippet: true,
                label_detail: sig.map(str::to_owned),
                ..item("getcwd")
            };
            caret_after_empty_call(&mut i);
            i.insert
        };
        assert_eq!(call(Some("()")), "getcwd()$0");
        assert_eq!(call(Some("(self)")), "getcwd()$0");
        assert_eq!(call(Some("(path: str)")), "getcwd($0)");
        assert_eq!(call(None), "getcwd($0)");
    }

    #[test]
    fn server_ranking_before_length() {
        let ranked = |label: &str, sort: &str| CompletionItem {
            sort_text: Some(sort.into()),
            ..item(label)
        };
        let items = [
            ranked("of", "0002"),
            ranked("ofLongerName", "0001"),
            CompletionItem {
                preselect: true,
                ..ranked("offer", "0009")
            },
            item("o"),
        ];
        let labels =
            |p: &str| -> Vec<String> { filter(&items, p).into_iter().map(|i| i.label).collect() };
        assert_eq!(labels(""), ["offer", "ofLongerName", "of", "o"]);
        assert_eq!(labels("off"), ["offer"]);
        assert_eq!(labels("ofl"), ["ofLongerName"]);
        // v7 behaviour 23: the exact name first, then the server's ranking.
        assert_eq!(labels("of"), ["of", "offer", "ofLongerName"]);
        // Without sort text: shorter first, as before.
        let plain = [item("helper_value"), item("help")];
        let l: Vec<String> = filter(&plain, "he").into_iter().map(|i| i.label).collect();
        assert_eq!(l, ["help", "helper_value"]);
    }

    #[test]
    fn tab_replaces_the_identifier_rest() {
        assert_eq!(
            list_key(true, "tab", false, &[CHOOSE_REPLACE]),
            Some(ListKey::Replace)
        );
        assert_eq!(
            list_key(true, "enter", false, &[CHOOSE]),
            Some(ListKey::Accept)
        );
        assert_eq!(identifier_rest("lper();"), 4);
        assert_eq!(identifier_rest("();"), 0);
        assert_eq!(identifier_rest(""), 0);
    }

    #[test]
    fn auto_trigger_after_identifiers_and_member_access() {
        for yes in [
            "he",
            "int a = he",
            "_x",
            "point.",
            "p->",
            "std::",
            "f().",
            "a[0].",
        ] {
            assert!(auto_trigger(yes), "{yes}");
        }
        for no in ["", "x = 1", "1.", "a = 12", "a ..", "x + ", "x;"] {
            assert!(!auto_trigger(no), "{no}");
        }
    }

    #[test]
    fn sql_list_opens_after_table_keywords() {
        for yes in [
            "select * from ",
            "select * from movement m join ",
            "select * from a LEFT JOIN ",
            "insert into ",
            "UPDATE ",
            "delete from ",
        ] {
            assert!(sql_auto_trigger(yes), "{yes}");
        }
        for no in [
            "select ",
            "select * from  ",
            "select * from",
            "select * from stock ",
            "select * from stock where ",
            "x.from ",
            "fromage ",
            "",
        ] {
            assert!(!sql_auto_trigger(no), "{no}");
        }
        // The plain rule still does not open on a space.
        assert!(!auto_trigger("select * from "));
        assert_eq!(kind_icon(Some("join")), Some("nodes/dataTables"));
    }

    #[test]
    fn word_start_and_snippets() {
        assert_eq!(word_start("    return helper_v", 19), 11);
        assert_eq!(word_start("a.b", 2), 2);
        assert_eq!(word_start("x", 0), 0);
        assert_eq!(
            snippet_text("helper(${1:int a}, ${2:b})$0"),
            "helper(int a, b)"
        );
        assert_eq!(snippet_text("fmt.Println($1)"), "fmt.Println()");
        assert_eq!(snippet_text("cost \\$5"), "cost $5");
        assert_eq!(snippet_text("${1:outer ${2:inner}}"), "outer inner");
    }

    #[test]
    fn row_texts_are_single_line() {
        assert_eq!(
            one_line("java.util.List.of(E e1,\n    E e2)  : List<E>\r\n"),
            "java.util.List.of(E e1, E e2) : List<E>"
        );
        assert_eq!(one_line("  of()  "), "of()");
        assert_eq!(one_line("\n\t"), "");
    }

    #[test]
    fn prefix_then_fuzzy() {
        let items = [
            item("helper_value"),
            item("help"),
            item("other_help"),
            item("main"),
        ];
        let labels: Vec<String> = filter(&items, "hel").into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["help", "helper_value", "other_help"]);
        assert_eq!(filter(&items, "").len(), 4);
        assert!(filter(&items, "zz").is_empty());
    }

    /// SPEC-0072 v7 AK-14: overloads the server ranks alike, varargs first (OBS-0017).
    #[test]
    fn overloads_varargs_first_then_fewer_parameters() {
        let of = |params: &str| CompletionItem {
            label_detail: Some(params.into()),
            sort_text: Some("999999179".into()),
            ..item("of")
        };
        let items = [
            of("()"),
            of("(E e1, E e2)"),
            of("(E... elements)"),
            of("(E e1)"),
            CompletionItem {
                sort_text: Some("999999179".into()),
                ..item("ofNullable")
            },
        ];
        for prefix in ["", "o"] {
            let order: Vec<String> = filter(&items, prefix)
                .into_iter()
                .map(|i| format!("{}{}", i.label, i.label_detail.unwrap_or_default()))
                .collect();
            assert_eq!(
                order,
                [
                    "of(E... elements)",
                    "of()",
                    "of(E e1)",
                    "of(E e1, E e2)",
                    "ofNullable"
                ],
                "prefix {prefix:?}"
            );
        }
    }

    /// SPEC-0072 v7 AK-15: snippets keep their tab stops, indentation and defaults.
    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn snippets_expand_with_stops_and_indentation() {
        let e = expand_snippet("of(${0})", "    ", "    ");
        assert_eq!(e.text, "of()");
        assert_eq!(e.stops, [vec![3..3]]);
        let fori = expand_snippet(
            "for (${1:int} ${2:i} = ${3:0}; ${2:i} < ${4:array.length}; ${2:i}++) {\n\t$TM_SELECTED_TEXT${0}\n}",
            "        ",
            "    ",
        );
        assert_eq!(
            fori.text,
            "for (int i = 0; i < array.length; i++) {\n            \n        }"
        );
        assert_eq!(fori.stops.len(), 5, "$1–$4 and $0");
        assert_eq!(fori.stops[0], [5..8], "$1 first");
        assert_eq!(fori.stops[1], [9..10, 16..17, 34..35], "$2 in three places");
        assert_eq!(fori.stops[4], [53..53], "$0 last, inside the body");
        let var = expand_snippet("int ${1:length} = s.length();${0}", "", "\t");
        assert_eq!(var.text, "int length = s.length();");
        assert_eq!(var.stops, [vec![4..10], vec![24..24]]);
        // Choices, variables with defaults, escapes, nested placeholders.
        let misc = expand_snippet("${1|a,b|} ${NAME:x} \\$ ${2:f(${3:y})}", "", "");
        assert_eq!(misc.text, "a x $ f(y)");
        assert_eq!(misc.stops, [vec![0..1], vec![6..10], vec![8..9]]);
    }

    /// SPEC-0072 v7 behaviour 22: the argument list at the end of a suggestion.
    #[test]
    fn own_argument_list_at_the_end() {
        assert_eq!(own_arguments_start("isBlank()"), Some(7));
        assert_eq!(own_arguments_start("of(f(x))"), Some(2));
        assert_eq!(own_arguments_start("ArrayList<>"), None);
        assert_eq!(own_arguments_start("System.out.println(s);"), None);
    }
}

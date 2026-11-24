//! The typed prefix and the narrowing of a completion list by it (`SPEC-0072` behaviours 2, 3 and
//! 12): one matcher for the editor's list and for the MCP tool `complete_code` (v18 behaviour 55),
//! so agents get the list people see.

use ferris_idea_core::{fuzzy_match, rank};

use crate::insight::CompletionItem;

/// Whether `c` belongs to an identifier.
pub fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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

/// Kinds that can be called: in a call position they come first, before the server's rank class
/// (`SPEC-0072` v21 behaviour 57). A class is called to construct it in several languages.
const CALLABLE_KINDS: [&str; 5] = ["function", "method", "constructor", "class", "struct"];

/// Kinds whose name is no declaration of the project even when a declaration has the same name.
const UNDECLARED_KINDS: [&str; 3] = ["keyword", "snippet", "text"];

/// Whether `(` follows the identifier at `column` (chars) of `line`, the rest of the identifier
/// and blanks skipped: a call position (`SPEC-0072` v21 behaviour 57).
pub fn call_position(line: &str, column: usize) -> bool {
    line.chars()
        .skip(column)
        .skip_while(|c| is_ident(*c))
        .find(|c| !c.is_whitespace())
        == Some('(')
}

/// Marks each of `items` with where the project declares its name — in `file` (content `text`)
/// or in another project file of the same extension — and, at a call position, whether it is no
/// callable (`SPEC-0072` v21 behaviours 56 and 57). [`filter`] ranks by both after the server's
/// own ranking, so the server's order stays wherever it ranks the items apart.
pub fn mark_declared(
    items: &mut [CompletionItem],
    root: &std::path::Path,
    file: &std::path::Path,
    text: &str,
    line: usize,
    column: usize,
) {
    if items.is_empty() {
        return;
    }
    let here = crate::declared_names::names_in(file, text);
    let name = |i: &CompletionItem| i.label.trim().trim_start_matches('$').to_owned();
    let open = |i: &CompletionItem| {
        !i.kind
            .as_deref()
            .is_some_and(|k| UNDECLARED_KINDS.contains(&k))
    };
    let elsewhere = if items.iter().any(|i| open(i) && !here.contains(&name(i))) {
        crate::declared_names::project_names(root, file)
    } else {
        Default::default()
    };
    let call = text
        .lines()
        .nth(line)
        .is_some_and(|l| call_position(l, column));
    for item in items.iter_mut() {
        let n = name(item);
        item.declared_in = if !open(item) {
            None
        } else if here.contains(&n) {
            Some(crate::insight::DeclaredIn::File)
        } else if elsewhere.contains(&n) {
            Some(crate::insight::DeclaredIn::Project)
        } else {
            None
        };
        item.not_callable_here = call
            && !item
                .kind
                .as_deref()
                .is_some_and(|k| CALLABLE_KINDS.contains(&k));
    }
}

/// The server's rank class of `item`: preselected first, then its sort text without the item's
/// label at its end, ignoring case — intelephense sends `0002` + the label for every name, which
/// orders only by the alphabet — then items without sort text (`SPEC-0072` v21 behaviour 56).
fn server_class(item: &CompletionItem) -> (bool, bool, String) {
    let sort = item.sort_text.as_deref().unwrap_or_default().to_lowercase();
    let label = item.label.trim().to_lowercase();
    let class = match sort.strip_suffix(&label) {
        Some(rest) if !label.is_empty() => rest.to_owned(),
        _ => sort,
    };
    (!item.preselect, item.sort_text.is_none(), class)
}

/// The order [`mark_declared`] adds among items the server ranks alike: callables first in a call
/// position, then names declared in the file, then in the project, then the rest.
fn declared_rank(item: &CompletionItem) -> (bool, u8) {
    let tier = match item.declared_in {
        Some(crate::insight::DeclaredIn::File) => 0,
        Some(crate::insight::DeclaredIn::Project) => 1,
        None => 2,
    };
    (item.not_callable_here, tier)
}

/// Items matching the typed `prefix`: case-insensitive prefix matches first in the server's
/// ranking (shorter first where it does not decide), then fuzzy matches; all items in the server's
/// ranking when the prefix is empty (behaviours 3 and 12).
pub fn filter(items: &[CompletionItem], prefix: &str) -> Vec<CompletionItem> {
    if prefix.is_empty() {
        let mut all: Vec<&CompletionItem> = items.iter().collect();
        all.sort_by(|a, b| {
            a.not_callable_here
                .cmp(&b.not_callable_here)
                .then_with(|| server_class(a).cmp(&server_class(b)))
                .then_with(|| declared_rank(a).cmp(&declared_rank(b)))
                .then_with(|| server_rank(a).cmp(&server_rank(b)))
                .then_with(|| {
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
            .then_with(|| a.not_callable_here.cmp(&b.not_callable_here))
            .then_with(|| server_class(a).cmp(&server_class(b)))
            .then_with(|| declared_rank(a).cmp(&declared_rank(b)))
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

/// The typed prefix at `column` (chars) of `line`: the identifier part left of the caret
/// (behaviour 2), empty after a `.`, `<` or a blank.
pub fn typed_prefix(line: &str, column: usize) -> String {
    let start = word_start(line, column);
    line.chars()
        .skip(start)
        .take(column.saturating_sub(start))
        .collect()
}

/// The typed prefix at 0-based `line` and `column` (chars) of the file `file` with content
/// `text`: in a `class` or `id` value of an HTML file the typed name of `SPEC-0072` v15 behaviour
/// 52, which includes `-`; everywhere else [`typed_prefix`] of the line (v18 behaviour 55).
pub fn typed_prefix_in(file: &std::path::Path, text: &str, line: usize, column: usize) -> String {
    let Some(row) = text.lines().nth(line) else {
        return String::new();
    };
    let value_start = crate::html_css::is_html(file)
        .then(|| crate::html_css::value_word(text, line as u64, column as u64))
        .flatten();
    match value_start {
        Some(start) => {
            let start = start as usize;
            row.chars()
                .skip(start)
                .take(column.saturating_sub(start))
                .collect()
        }
        None => typed_prefix(row, column),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, sort: Option<&str>) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            insert: label.into(),
            sort_text: sort.map(str::to_owned),
            ..Default::default()
        }
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    /// SPEC-0072 v18 AK-39 (behaviour 55): the prefix left of the caret, as the editor reads it.
    #[test]
    fn the_typed_prefix() {
        assert_eq!(typed_prefix("<sec", 4), "sec");
        assert_eq!(typed_prefix("  display: fl", 13), "fl");
        assert_eq!(typed_prefix("    r.", 6), "");
        assert_eq!(typed_prefix("    r.ar", 8), "ar");
        assert_eq!(typed_prefix("grüße", 3), "grü");
        // A column past the end of the line counts to its end.
        assert_eq!(typed_prefix("ab", 9), "ab");
        assert_eq!(typed_prefix("", 0), "");
    }

    /// SPEC-0072 v18 AK-39 with v15 behaviour 52: in an HTML `class` value the prefix keeps its
    /// `-`; elsewhere in HTML and in other files `-` ends it.
    #[test]
    fn the_typed_prefix_in_an_html_class_value() {
        let html = std::path::Path::new("index.html");
        let text = "<div class=\"box card-w\"></div>\n<p>card-w</p>\n";
        assert_eq!(typed_prefix_in(html, text, 0, 22), "card-w");
        assert_eq!(typed_prefix_in(html, text, 0, 15), "box");
        assert_eq!(typed_prefix_in(html, text, 1, 9), "w");
        let css = std::path::Path::new("a.css");
        assert_eq!(typed_prefix_in(css, "  card-w", 0, 8), "w");
        assert_eq!(typed_prefix_in(css, "", 3, 0), "");
    }

    /// SPEC-0072 v18 AK-39 (behaviour 55, OBS-0081 L7): an HTML server's whole tag list narrowed
    /// to `<sec`: prefix matches first, shorter first, then fuzzy matches; the rest dropped.
    #[test]
    fn a_server_list_is_narrowed_by_the_prefix() {
        let items: Vec<CompletionItem> = [
            "html",
            "head",
            "title",
            "section",
            "select",
            "sectionHeader",
            "aside",
            "s",
        ]
        .into_iter()
        .map(|l| item(l, None))
        .collect();
        let shown = filter(&items, "sec");
        assert_eq!(labels(&shown), ["section", "sectionHeader", "select"]);
        // Case is ignored; the same case comes first.
        let items = [item("Path", Some("1")), item("path", Some("1"))];
        assert_eq!(labels(&filter(&items, "Pa")), ["Path", "path"]);
        // No prefix: everything in the server's ranking.
        let items = [
            item("into()", Some("7ffffffb")),
            item("area()", Some("7ffffff6")),
        ];
        assert_eq!(labels(&filter(&items, "")), ["area()", "into()"]);
    }

    /// SPEC-0072 v21 AK-42 (behaviours 56 and 57, F-038 PH1): intelephense ranks every name
    /// alike (`0002…`); the function declared in the file comes first, then the one declared in
    /// the project, and in a call position the functions before the constants. Where the server
    /// ranks apart, its order stays.
    #[test]
    fn project_names_and_callables_first_among_ties() {
        let root = std::env::temp_dir().join(format!("ferris-idea-rank-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("index.php");
        let text = "<?php\n\nfunction greet(string $name): string\n{\n    return $name;\n}\n\necho greet(\"you\");\n";
        std::fs::write(&file, text).unwrap();
        std::fs::write(
            root.join("lib/util.php"),
            "<?php\nfunction greeting_card() {}\n",
        )
        .unwrap();
        let php = |label: &str, kind: &str| CompletionItem {
            label: label.into(),
            insert: label.into(),
            kind: Some(kind.into()),
            sort_text: Some(format!("0002{label}")),
            ..Default::default()
        };
        let server = vec![
            php("GRAPHEME_EXTR_COUNT", "constant"),
            php("GRAPHEME_EXTR_MAXBYTES", "constant"),
            php("grapheme_extract", "function"),
            php("grapheme_strlen", "function"),
            php("greeting_card", "function"),
            php("greet", "function"),
            php("GROUPING", "constant"),
            php("gr", "keyword"),
        ];
        // `echo gr|eet("you");`: a call position.
        assert!(call_position("echo greet(\"you\");", 7));
        assert!(call_position("x = f  (1)", 4));
        assert!(!call_position("echo gr", 7));
        assert!(!call_position("a.b", 1));
        let mut items = server.clone();
        mark_declared(&mut items, &root, &file, text, 7, 7);
        assert_eq!(items[5].declared_in, Some(crate::insight::DeclaredIn::File));
        assert_eq!(
            items[4].declared_in,
            Some(crate::insight::DeclaredIn::Project)
        );
        assert_eq!(items[7].declared_in, None, "a keyword is no declaration");
        assert!(items[0].not_callable_here && !items[2].not_callable_here);
        assert_eq!(
            labels(&filter(&items, "gr")),
            [
                "gr",
                "greet",
                "greeting_card",
                "grapheme_extract",
                "grapheme_strlen",
                "GRAPHEME_EXTR_COUNT",
                "GRAPHEME_EXTR_MAXBYTES",
                "GROUPING",
            ]
        );
        // Not in a call position: declared names first, the rest in the server's order.
        let mut items = server.clone();
        let plain =
            "<?php\n\nfunction greet(string $name): string\n{\n    return $name;\n}\n\necho gr;\n";
        mark_declared(&mut items, &root, &file, plain, 7, 7);
        assert_eq!(
            labels(&filter(&items, "gr"))[..5],
            [
                "gr",
                "greet",
                "greeting_card",
                "GRAPHEME_EXTR_COUNT",
                "GRAPHEME_EXTR_MAXBYTES"
            ]
        );
        assert_eq!(labels(&filter(&items, ""))[..2], ["greet", "greeting_card"]);
        // A server that ranks a built-in first keeps it first.
        let mut items = server.clone();
        items[3].sort_text = Some("0001grapheme_strlen".into());
        mark_declared(&mut items, &root, &file, text, 7, 7);
        assert_eq!(
            labels(&filter(&items, "gr"))[..3],
            ["gr", "grapheme_strlen", "greet"]
        );
        // A server that ranks a constant first does not put it before the functions in a call
        // position (intelephense on Linux ranked `GROUPING` ahead, v21 behaviour 57).
        let mut items = server.clone();
        items[6].sort_text = Some("0001GROUPING".into());
        mark_declared(&mut items, &root, &file, text, 7, 7);
        let filtered = filter(&items, "gr");
        let ranked = labels(&filtered);
        assert_eq!(ranked[1], "greet");
        assert_eq!(ranked.iter().position(|l| *l == "GROUPING"), Some(5));
        let _ = std::fs::remove_dir_all(&root);
    }
}

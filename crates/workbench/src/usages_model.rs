//! Rows of the usages popup (`SPEC-0073` behaviour 2), without toolkit.

use std::path::{Path, PathBuf};

use crate::nav_model::Place;

/// One usage: where it is and the text of its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    /// Where the usage starts.
    pub place: Place,
    /// Path relative to the workspace, or absolute outside it.
    pub shown_path: String,
    /// Text of the line, trimmed.
    pub text: String,
    /// The type the place is in, for a chooser of targets (`SPEC-0081` v2 behaviour 4).
    pub owner: Option<Owner>,
}

/// The type a target is in, as a chooser row shows it (`SPEC-0081` v2 behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// The type with its enclosing types, e.g. `Discounts.Half`.
    pub name: String,
    /// Its kind, for the icon.
    pub kind: String,
    /// The package, grey after the name.
    pub package: Option<String>,
    /// The module, at the right edge.
    pub module: String,
}

/// Rows of a chooser of targets: each with its owner when `contexts` names one, sorted by owner,
/// package, module, path and line (`SPEC-0081` v2 behaviour 4); rows without an owner keep the
/// line text and `path:line`.
pub fn chooser(
    root: &Path,
    places: Vec<Place>,
    contexts: &[Option<idecraft_languages::context::PlaceContext>],
    line_text: impl Fn(&Path, usize) -> Option<String>,
) -> Vec<Usage> {
    let mut out: Vec<Usage> = places
        .into_iter()
        .enumerate()
        .map(|(i, place)| {
            let owner = contexts.get(i).and_then(Option::as_ref).and_then(|c| {
                Some(Owner {
                    name: c.container.clone()?,
                    kind: c.container_kind.clone().unwrap_or_else(|| "class".into()),
                    package: c.package.clone(),
                    module: c.module.clone(),
                })
            });
            let mut u = usages(root, vec![place], &line_text).remove(0);
            u.owner = owner;
            u
        })
        .collect();
    out.sort_by_cached_key(|u| {
        let o = u.owner.as_ref();
        (
            o.map(|o| o.name.to_lowercase()),
            o.and_then(|o| o.package.clone()),
            o.map(|o| o.module.clone()),
            u.shown_path.clone(),
            u.place.line,
            u.place.column,
        )
    });
    out.dedup_by(|a, b| a.place == b.place);
    out
}

/// The title of a chooser: `Choose implementation of apply(int) (2)`, or without a name
/// `Choose implementation (2)` (`SPEC-0081` v2 behaviour 4).
pub fn chooser_title(what: &str, symbol: Option<&str>, n: usize) -> String {
    match symbol {
        Some(s) if !s.is_empty() => format!("Choose {what} of {s} ({n})"),
        _ => format!("Choose {what} ({n})"),
    }
}

/// What ⌥⌘F7 does with a single usage (`SPEC-0073` v5 behaviour 7): whether the caret jumps to
/// it, and the hint shown.
pub fn one_usage(symbol: &str, usage: &Place, length: usize, caret: &Place) -> (bool, String) {
    let at_caret = usage.path == caret.path
        && usage.line == caret.line
        && (usage.column..=usage.column + length.max(symbol.chars().count()))
            .contains(&caret.column);
    if at_caret {
        (
            false,
            format!(
                "{symbol} has no other usage in project files; Find Usages (⌥F7) shows it in the Find tool window"
            ),
        )
    } else {
        (true, format!("Only usage of {symbol} in project files"))
    }
}

/// Usages sorted by path and position, with each line's text from `line_text` (the open editor's
/// text when the file is open, else the file on disk).
pub fn usages(
    root: &Path,
    places: Vec<Place>,
    line_text: impl Fn(&Path, usize) -> Option<String>,
) -> Vec<Usage> {
    let mut out: Vec<Usage> = places
        .into_iter()
        .map(|place| {
            let shown_path = place
                .path
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| place.path.display().to_string());
            // One line for a row of the popup (SPEC-0073 v4).
            let text = line_text(&place.path, place.line)
                .map(|t| crate::completion_model::one_line(&t))
                .unwrap_or_default();
            Usage {
                place,
                shown_path,
                text,
                owner: None,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        (&a.shown_path, a.place.line, a.place.column).cmp(&(
            &b.shown_path,
            b.place.line,
            b.place.column,
        ))
    });
    out.dedup_by(|a, b| a.place == b.place);
    out
}

/// Text of a 0-based line of a file on disk.
pub fn disk_line(path: &Path, line: usize) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .nth(line)
        .map(str::to_owned)
}

/// The identifier around a char column of a line, for the popup's title.
pub fn word_at(line: &str, column: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let is = |c: char| c.is_alphanumeric() || c == '_';
    let mut a = column.min(chars.len());
    let mut b = a;
    while a > 0 && is(chars[a - 1]) {
        a -= 1;
    }
    while b < chars.len() && is(chars[b]) {
        b += 1;
    }
    chars[a..b].iter().collect()
}

/// What Find Usages, Find Usages in File, Show Usages and Highlight Usages in File say when the
/// caret is on no name (`SPEC-0293` v8 behaviour 20, `SPEC-0295` behaviour 8).
pub const NO_NAME_AT_CARET: &str = "No usages found: the caret is not on a name";

/// The name a usage search at a char column of a line is about, by the rule of [`word_at`]: the
/// letters, digits and underscores around the column, also when the caret stands right after
/// them. Without one there is nothing to search for and nobody is asked: the text for the status
/// bar (`SPEC-0293` v8 behaviour 20).
pub fn search_subject(line: &str, column: usize) -> Result<String, &'static str> {
    let name = word_at(line, column);
    if name.is_empty() {
        Err(NO_NAME_AT_CARET)
    } else {
        Ok(name)
    }
}

/// A path as given; helper for tests and callers that need owned paths.
pub fn place(path: impl Into<PathBuf>, line: usize, column: usize) -> Place {
    Place {
        path: path.into(),
        line,
        column,
    }
}

/// The status line after Change Signature (SPEC-0113 behaviour 5): what changed, and how many calls
/// were left alone with the first reason.
pub fn signature_report(function: &str, files: usize, skipped: &[String]) -> String {
    let mut s = format!("Changed the signature of {function} in {files} file(s).");
    if let Some(first) = skipped.first() {
        s.push_str(&format!(
            " {} call(s) left unchanged, e.g. {first}",
            skipped.len()
        ));
    }
    s
}

/// The status line after Move (SPEC-0114 behaviour 5); `outside` files that refer to the moved
/// file are not the project's own and stay unchanged (v3 behaviour 8).
pub fn move_report(to: &str, changed: usize, server: bool, outside: usize) -> String {
    let by = if server {
        "the language server and IDECraft"
    } else {
        "IDECraft"
    };
    let mut report = match changed {
        0 => format!("Moved to {to}; no references to update."),
        n => format!("Moved to {to}; {n} file(s) updated by {by}."),
    };
    if outside > 0 {
        report.push_str(&format!(
            " {outside} file(s) outside the project's own sources left unchanged."
        ));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0293 AK-13 (v8): a place without a name gives no search but the status text; a
    /// name, also in a comment or right before the caret, gives the search.
    #[test]
    fn a_place_without_a_name_is_not_searched() {
        let annotated = "@Override public int total() { // sum of all";
        // Column 1 of a line that starts with an annotation's `@`.
        assert_eq!(search_subject(annotated, 0), Err(NO_NAME_AT_CARET));
        assert_eq!(search_subject(annotated, 1).as_deref(), Ok("Override"));
        // Right after a name counts as on it; between two spaces or signs does not.
        assert_eq!(search_subject(annotated, 9).as_deref(), Ok("Override"));
        assert_eq!(search_subject("a  b", 2), Err(NO_NAME_AT_CARET));
        assert_eq!(search_subject("x = (  )", 6), Err(NO_NAME_AT_CARET));
        // White space only, an empty line, a column beyond the end.
        assert_eq!(search_subject("        ", 4), Err(NO_NAME_AT_CARET));
        assert_eq!(search_subject("", 0), Err(NO_NAME_AT_CARET));
        assert_eq!(search_subject("  ;", 40), Err(NO_NAME_AT_CARET));
        // A comment: its signs and gaps are no name, a word in it is one (the server decides).
        let comment = annotated.find("//").unwrap();
        assert_eq!(
            search_subject(annotated, comment + 1),
            Err(NO_NAME_AT_CARET)
        );
        assert_eq!(search_subject(annotated, comment + 4).as_deref(), Ok("sum"));
        assert_eq!(search_subject("größe_2 = 1", 3).as_deref(), Ok("größe_2"));
    }

    #[test]
    fn move_report_names_the_target() {
        assert_eq!(
            move_report("include/a.h", 0, false, 0),
            "Moved to include/a.h; no references to update."
        );
        assert_eq!(
            move_report("include/a.h", 2, true, 0),
            "Moved to include/a.h; 2 file(s) updated by the language server and IDECraft."
        );
        assert_eq!(
            move_report("include/a.h", 1, false, 2),
            "Moved to include/a.h; 1 file(s) updated by IDECraft. 2 file(s) outside the project's own sources left unchanged."
        );
    }

    #[test]
    fn signature_report_names_left_calls() {
        assert_eq!(
            signature_report("add", 2, &[]),
            "Changed the signature of add in 2 file(s)."
        );
        assert_eq!(
            signature_report("add", 1, &["a.c:3: passes no argument for `b`".into()]),
            "Changed the signature of add in 1 file(s). 1 call(s) left unchanged, e.g. a.c:3: passes no argument for `b`"
        );
    }

    /// SPEC-0081 v2 AK-4: rows name the class, sorted by it.
    #[test]
    fn chooser_rows_name_the_class() {
        use idecraft_languages::context::PlaceContext;
        let ctx = |c: &str| {
            Some(PlaceContext {
                module: "shop".into(),
                package: Some("lab.inventory".into()),
                container: Some(c.into()),
                container_kind: Some("class".into()),
                ..PlaceContext::default()
            })
        };
        let rows = chooser(
            Path::new("/w"),
            vec![
                place("/w/D.java", 4, 19),
                place("/w/D.java", 10, 19),
                place("/w/x.c", 1, 0),
            ],
            &[ctx("Discounts.None"), ctx("Discounts.Half"), None],
            |_, _| Some("public int apply(int cents) {".into()),
        );
        let names: Vec<Option<&str>> = rows
            .iter()
            .map(|r| r.owner.as_ref().map(|o| o.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [None, Some("Discounts.Half"), Some("Discounts.None")]
        );
        assert_eq!(
            rows[1].owner.as_ref().unwrap().package.as_deref(),
            Some("lab.inventory")
        );
        assert_eq!(rows[1].owner.as_ref().unwrap().module, "shop");
        assert_eq!(rows[0].text, "public int apply(int cents) {");
        assert_eq!(
            chooser_title("implementation", Some("apply(int)"), 2),
            "Choose implementation of apply(int) (2)"
        );
        assert_eq!(
            chooser_title("declaration", None, 3),
            "Choose declaration (3)"
        );
    }

    /// SPEC-0073 v5 AK-6.
    #[test]
    fn one_usage_is_a_hint() {
        let usage = place("/w/bad.c", 1, 25);
        let (jump, hint) = one_usage("helper", &usage, 6, &place("/w/bad.c", 1, 27));
        assert!(!jump);
        assert!(hint.starts_with("helper has no other usage"), "{hint}");
        let (jump, hint) = one_usage("helper", &usage, 6, &place("/w/bad.c", 0, 5));
        assert!(jump);
        assert_eq!(hint, "Only usage of helper in project files");
    }

    /// SPEC-0073 v4 AK-5: a usage's text is one line, whatever its line holds.
    #[test]
    fn usage_text_is_one_line() {
        let u = usages(Path::new("/w"), vec![place("/w/A.java", 3, 4)], |_, _| {
            Some("\tpublic record Item(String sku,\t\tString name)  {\r\n".to_owned())
        });
        assert_eq!(u[0].text, "public record Item(String sku, String name) {");
        assert!(!u[0].text.contains(['\n', '\r', '\t']));
    }

    #[test]
    fn sorted_deduplicated_with_text() {
        let root = Path::new("/w");
        let lines = |p: &Path, l: usize| -> Option<String> {
            let file = p.file_name()?.to_str()?;
            Some(format!("  line {l} of {file}  "))
        };
        let u = usages(
            root,
            vec![
                place("/w/src/b.c", 3, 1),
                place("/w/a.c", 9, 0),
                place("/w/a.c", 2, 4),
                place("/w/a.c", 2, 4),
                place("/elsewhere/x.h", 0, 0),
            ],
            lines,
        );
        let shown: Vec<(&str, usize)> = u
            .iter()
            .map(|x| (x.shown_path.as_str(), x.place.line))
            .collect();
        assert_eq!(
            shown,
            [
                ("/elsewhere/x.h", 0),
                ("a.c", 2),
                ("a.c", 9),
                ("src/b.c", 3)
            ]
        );
        assert_eq!(u[1].text, "line 2 of a.c");
        assert_eq!(word_at("int a = helper();", 10), "helper");
        assert_eq!(word_at("x", 5), "x");
    }
}

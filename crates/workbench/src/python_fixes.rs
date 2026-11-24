//! Install entries of the ⌥↩ list on a Python import the language server cannot resolve
//! (`SPEC-0117` v4 behaviour 8): the modules marked by pyright's rules on the caret's line, mapped
//! to the distributions to install by `ferris_idea_core::install_fixes`.

use std::ops::Range;

use ferris_idea_core::{InstallFix, MISSING_IMPORT_RULES, Unsatisfied};

use crate::diagnostics_model::Problem;

/// Char range of the line holding char offset `caret` in `text`, without its line break.
fn line_around(text: &str, caret: usize) -> Range<usize> {
    let mut start = 0;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            if i >= caret {
                return start..i;
            }
            start = i + 1;
        }
    }
    start..text.chars().count()
}

/// The text of char range `range` in `text`.
fn chars_of(text: &str, range: &Range<usize>) -> String {
    text.chars()
        .skip(range.start)
        .take(range.end.saturating_sub(range.start))
        .collect()
}

/// The first `"…"` of a message, e.g. `requests` of `Import "requests" could not be resolved`.
fn quoted(message: &str) -> Option<&str> {
    let start = message.find('"')? + 1;
    let len = message[start..].find('"')?;
    Some(&message[start..start + len])
}

/// The modules marked as unresolved imports (pyright's `reportMissingImports` and
/// `reportMissingModuleSource`) on the line of char offset `caret`: the marked text, or the quoted
/// name of the message when the mark is no module name.
pub fn missing_imports_on_line(problems: &[Problem], text: &str, caret: usize) -> Vec<String> {
    let line = line_around(text, caret);
    let is_module = |s: &str| ferris_idea_core::import_package(s, &[]).is_some();
    problems
        .iter()
        .filter(|p| {
            p.code
                .as_deref()
                .is_some_and(|c| MISSING_IMPORT_RULES.contains(&c))
                && p.range.start <= line.end
                && line.start < p.range.end.max(p.range.start + 1)
        })
        .filter_map(|p| {
            let marked = chars_of(text, &p.range);
            let marked = marked.trim();
            if is_module(marked) {
                return Some(marked.to_owned());
            }
            quoted(&p.message)
                .filter(|q| is_module(q))
                .map(str::to_owned)
        })
        .collect()
}

/// The install entries at `caret` (`SPEC-0117` v4 behaviour 8); `unsatisfied` are the declared
/// dependencies the selected environment does not satisfy (empty when it is not known).
pub fn install_entries(
    problems: &[Problem],
    text: &str,
    caret: usize,
    unsatisfied: &[Unsatisfied],
) -> Vec<InstallFix> {
    ferris_idea_core::install_fixes(&missing_imports_on_line(problems, text, caret), unsatisfied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics_model::Severity;

    fn mark(text: &str, what: &str, code: &str) -> Problem {
        let byte = text.find(what).unwrap();
        let start = text[..byte].chars().count();
        Problem {
            range: start..start + what.chars().count(),
            severity: Severity::Warning,
            message: format!("Pyright: Import \"{what}\" could not be resolved"),
            inspection: None,
            unused: false,
            code: Some(code.into()),
        }
    }

    fn missing(name: &str, requirement: &str) -> Unsatisfied {
        Unsatisfied {
            name: name.into(),
            requirement: requirement.into(),
            installed: None,
        }
    }

    const TEXT: &str = "import os\nimport requests, yaml\nfrom cv2 import imread\nx = foo\n";

    /// SPEC-0117 v4 AK-9: only the two rules, only on the caret's line, the marked module.
    #[test]
    fn entries_come_from_missing_imports_on_the_caret_line() {
        let problems = vec![
            mark(TEXT, "requests", "reportMissingImports"),
            mark(TEXT, "yaml", "reportMissingModuleSource"),
            mark(TEXT, "cv2", "reportMissingImports"),
            mark(TEXT, "foo", "reportUndefinedVariable"),
        ];
        let second = TEXT.find("requests").unwrap();
        assert_eq!(
            missing_imports_on_line(&problems, TEXT, second),
            ["requests", "yaml"]
        );
        // The caret at the start of the line still finds them.
        assert_eq!(
            missing_imports_on_line(&problems, TEXT, 10),
            ["requests", "yaml"]
        );
        let third = TEXT.find("from").unwrap();
        let declared = [missing("requests", ">=2.31"), missing("PyYAML", ">=6")];
        let titles: Vec<String> = install_entries(&problems, TEXT, third, &declared)
            .into_iter()
            .map(|f| f.title)
            .collect();
        assert_eq!(
            titles,
            [
                "Install package opencv-python",
                "Install all missing requirements (2)"
            ]
        );
        let entries = install_entries(&problems, TEXT, second, &declared);
        assert_eq!(entries[0].specs, ["requests>=2.31"]);
        assert_eq!(entries[1].specs, ["PyYAML>=6"]);
        assert_eq!(entries[2].title, "Install all missing requirements (2)");
        // Other rules and lines without a mark give nothing.
        assert!(install_entries(&problems, TEXT, 0, &declared).is_empty());
        let last = TEXT.find("foo").unwrap();
        assert!(install_entries(&problems, TEXT, last, &declared).is_empty());
    }

    /// SPEC-0117 v4 AK-9: a mark that is no module name falls back to the message's quoted name.
    #[test]
    fn a_wide_mark_falls_back_to_the_message() {
        let text = "import requests as r\n";
        let mut wide = mark(text, "requests", "reportMissingImports");
        wide.range = 0..text.chars().count() - 1;
        assert_eq!(missing_imports_on_line(&[wide], text, 3), ["requests"]);
        let mut neither = mark(text, "requests", "reportMissingImports");
        neither.range = 0..10;
        neither.message = "something else".into();
        assert!(missing_imports_on_line(&[neither], text, 3).is_empty());
    }
}

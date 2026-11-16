//! Diagnostics of the language server mapped onto the buffer (`SPEC-0070` behaviours 2–4), without
//! toolkit.

use std::ops::Range;

use idecraft_languages::lsp::Diagnostic;

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Errors (`ERRORS_ATTRIBUTES`).
    Error,
    /// Warnings (`WARNING_ATTRIBUTES`).
    Warning,
    /// Information and hints (`WEAK_WARNING_ATTRIBUTES`).
    Weak,
}

/// A diagnostic as a char range of the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Char range in the buffer; never empty.
    pub range: Range<usize>,
    /// Severity.
    pub severity: Severity,
    /// Message with the source in front, e.g. `clang: use of undeclared identifier 'x'`.
    pub message: String,
    /// Id of the IDECraft inspection that found it; `None` for language servers and plugins.
    pub inspection: Option<&'static str>,
    /// Marks unused code: drawn grey instead of wavy (`SPEC-0070` v5 behaviour 6).
    pub unused: bool,
    /// The language server's code of the diagnostic, e.g. pyright's `reportMissingImports`
    /// (`SPEC-0117` v4 behaviour 8).
    pub code: Option<String>,
}

/// Char offset of a UTF-16 position within `line`.
fn char_of_utf16(line: &str, units: u64) -> usize {
    let mut seen = 0u64;
    for (i, c) in line.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16() as u64;
    }
    line.chars().count()
}

/// Maps diagnostics onto the buffer: `line` returns a line's text without its line break and
/// `line_start` its first char offset. Empty ranges cover the next char (or the previous one at the
/// end of a line), so every problem is visible. Sorted by position.
pub fn to_problems(
    diagnostics: &[Diagnostic],
    line: impl Fn(usize) -> Option<String>,
    line_start: impl Fn(usize) -> usize,
) -> Vec<Problem> {
    let mut out: Vec<Problem> = diagnostics
        .iter()
        .filter_map(|d| {
            let (sl, el) = (d.start_line as usize, d.end_line as usize);
            let first = line(sl)?;
            let last = if el == sl {
                first.clone()
            } else {
                line(el).unwrap_or_default()
            };
            let start = line_start(sl) + char_of_utf16(&first, d.start_character);
            let mut end = line_start(el.max(sl)) + char_of_utf16(&last, d.end_character);
            let mut start = start.min(end.max(start));
            if end <= start {
                let line_end = line_start(sl) + first.chars().count();
                if start < line_end {
                    end = start + 1;
                } else if start > line_start(sl) {
                    start -= 1;
                    end = start + 1;
                } else {
                    end = start + 1; // an empty line: mark its line break
                }
            }
            let severity = match d.severity.as_str() {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                _ => Severity::Weak,
            };
            let message = match &d.source {
                Some(s) => format!("{s}: {}", d.message),
                None => d.message.clone(),
            };
            Some(Problem {
                range: start..end,
                severity,
                message,
                inspection: None,
                unused: d.unnecessary,
                code: d.code.clone(),
            })
        })
        .collect();
    out.sort_by_key(|p| (p.range.start, p.severity));
    out
}

/// The problems shown for a file: the server's, and the own ones that do not cover exactly the
/// same text as a server diagnostic (`SPEC-0070` v5 behaviour 7).
pub fn without_duplicates(server: &[Problem], own: Vec<Problem>) -> Vec<Problem> {
    own.into_iter()
        .filter(|o| !server.iter().any(|s| s.range == o.range))
        .collect()
}

pub use idecraft_languages::highlighting::HighlightLevel;

/// Whether the highlighting level `level` shows problem `p` (`SPEC-0070` v6 behaviour 11):
/// unused code counts as weak.
pub fn level_shows(level: HighlightLevel, p: &Problem) -> bool {
    use idecraft_languages::highlighting::Seriousness;
    let s = match p.severity {
        _ if p.unused => Seriousness::Weak,
        Severity::Error => Seriousness::Error,
        Severity::Warning => Seriousness::Warning,
        Severity::Weak => Seriousness::Weak,
    };
    level.shows_problem(s)
}

/// The problems `level` shows and how many it hides (v6 behaviour 11).
pub fn by_level(problems: Vec<Problem>, level: HighlightLevel) -> (Vec<Problem>, usize) {
    let total = problems.len();
    let shown: Vec<Problem> = problems
        .into_iter()
        .filter(|p| level_shows(level, p))
        .collect();
    let hidden = total - shown.len();
    (shown, hidden)
}

/// What the indicator shows in front of the counts for `level` (v6 behaviour 11): nothing at All,
/// the level's name at Syntax and Essential, a crossed-out eye at None.
pub fn level_mark(level: HighlightLevel) -> Option<&'static str> {
    match level {
        HighlightLevel::All => None,
        HighlightLevel::None => Some("⊘"),
        l => Some(l.label()),
    }
}

/// Editor views narrower than this show the compact indicator (v7 behaviour 12).
pub const COMPACT_BELOW: f32 = 520.0;

/// Widest the compact indicator gets (v7 behaviour 12).
pub const COMPACT_MAX_WIDTH: f32 = 64.0;

/// What the compact indicator shows (v7 behaviour 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compact {
    /// The level None's mark.
    Off,
    /// No problem shown: the check mark.
    Clean,
    /// The sign of the most severe kind present and the sum of all shown problems.
    Sum {
        /// Errors are present (●), else warnings (▲), else only weak warnings (▲ weak).
        severity: u8,
        /// All shown problems.
        total: usize,
    },
}

/// The compact indicator of `counts` (errors, warnings, weak) at `level`: severity 0 for errors, 1
/// for warnings, 2 for weak warnings (v7 behaviour 12).
pub fn compact((errors, warnings, weak): (usize, usize, usize), level: HighlightLevel) -> Compact {
    let total = errors + warnings + weak;
    match () {
        _ if level == HighlightLevel::None => Compact::Off,
        _ if total == 0 => Compact::Clean,
        _ => Compact::Sum {
            severity: if errors > 0 {
                0
            } else if warnings > 0 {
                1
            } else {
                2
            },
            total,
        },
    }
}

/// "4 problems hidden by the level", or `None` when nothing is hidden (v6 behaviour 11, IMP-0310).
pub fn hidden_words(hidden: usize) -> Option<String> {
    match hidden {
        0 => None,
        1 => Some("1 problem hidden by the level".into()),
        n => Some(format!("{n} problems hidden by the level")),
    }
}

/// Number of errors and warnings (behaviour 3).
pub fn counts(problems: &[Problem]) -> (usize, usize) {
    let (errors, warnings, _) = counts3(problems);
    (errors, warnings)
}

/// Number of errors, warnings and weak warnings (v5 behaviour 9).
pub fn counts3(problems: &[Problem]) -> (usize, usize, usize) {
    let n = |s: Severity| problems.iter().filter(|p| p.severity == s).count();
    (n(Severity::Error), n(Severity::Warning), n(Severity::Weak))
}

/// The counts in words, e.g. "1 error, 4 warnings, 1 weak warning" or "No problems" (v5
/// behaviour 9).
pub fn summary_words((errors, warnings, weak): (usize, usize, usize)) -> String {
    let parts: Vec<String> = [
        (errors, "error", "errors"),
        (warnings, "warning", "warnings"),
        (weak, "weak warning", "weak warnings"),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
    .collect();
    if parts.is_empty() {
        "No problems".into()
    } else {
        parts.join(", ")
    }
}

/// What the problem indicator at the editor's top right shows (v4 behaviour 3, v5 behaviour 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    /// No problem at all: a check mark.
    Clean,
    /// The counts; a zero count is not shown.
    Counts {
        /// Errors (●).
        errors: usize,
        /// Warnings (▲).
        warnings: usize,
        /// Weak warnings and information (▲ in the weak-warning colour).
        weak: usize,
    },
}

/// The indicator for `(errors, warnings, weak)` as [`counts3`] gives them.
pub fn indicator((errors, warnings, weak): (usize, usize, usize)) -> Indicator {
    if errors + warnings + weak == 0 {
        Indicator::Clean
    } else {
        Indicator::Counts {
            errors,
            warnings,
            weak,
        }
    }
}

/// One mark of the error stripe (v5 behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeMark {
    /// 0-based line.
    pub line: usize,
    /// The most serious severity on the line.
    pub severity: Severity,
    /// Whether every problem of that severity on the line marks unused code.
    pub unused: bool,
    /// Char offset of the most serious problem's start, where a click puts the caret.
    pub offset: usize,
    /// The messages of the line, most serious first.
    pub messages: Vec<String>,
}

/// The stripe marks of `problems`, one per line, by line; `line_of` maps a char offset to its line.
pub fn stripe_marks(problems: &[Problem], line_of: impl Fn(usize) -> usize) -> Vec<StripeMark> {
    let mut by_line: std::collections::BTreeMap<usize, Vec<&Problem>> = Default::default();
    for p in problems {
        by_line.entry(line_of(p.range.start)).or_default().push(p);
    }
    by_line
        .into_iter()
        .map(|(line, mut list)| {
            list.sort_by_key(|p| (p.severity, p.range.start));
            let top = list[0];
            StripeMark {
                line,
                severity: top.severity,
                unused: list
                    .iter()
                    .filter(|p| p.severity == top.severity)
                    .all(|p| p.unused),
                offset: top.range.start,
                messages: list.iter().map(|p| p.message.clone()).collect(),
            }
        })
        .collect()
}

/// Where a mark of `line` sits in a stripe for a document of `lines` lines, as a fraction of the
/// stripe's height (v5 behaviour 8).
pub fn stripe_fraction(line: usize, lines: usize) -> f32 {
    if lines <= 1 {
        0.0
    } else {
        line as f32 / lines as f32
    }
}

/// The most serious problem inside the char range `hidden` (a folded region), for its placeholder
/// (v5 behaviour 10).
pub fn hidden_in(problems: &[Problem], hidden: Range<usize>) -> Option<&Problem> {
    problems
        .iter()
        .filter(|p| p.range.start < hidden.end && p.range.end > hidden.start)
        .min_by_key(|p| (p.severity, p.unused))
}

/// The problems covering char offset `offset`, most serious first (`SPEC-0078` v2 behaviour 5).
pub fn under(problems: &[Problem], offset: usize) -> Vec<&Problem> {
    let mut found: Vec<&Problem> = problems
        .iter()
        .filter(|p| p.range.start <= offset && offset < p.range.end.max(p.range.start + 1))
        .collect();
    found.sort_by_key(|p| p.severity);
    found
}

/// The most serious problem covering `offset` (the caret), if any (behaviour 4).
pub fn at(problems: &[Problem], offset: usize) -> Option<&Problem> {
    problems
        .iter()
        .filter(|p| p.range.start <= offset && offset <= p.range.end)
        .min_by_key(|p| p.severity)
}

/// The problem F2 (`forward`) or ⇧F2 jumps to from `offset`: errors when there are any, otherwise all
/// problems; the next start after `offset` (the previous start before it), wrapping around at the end
/// of the file (`SPEC-0078` behaviour 1).
pub fn next(problems: &[Problem], offset: usize, forward: bool) -> Option<&Problem> {
    let errors = problems.iter().any(|p| p.severity == Severity::Error);
    let mut candidates: Vec<&Problem> = problems
        .iter()
        .filter(|p| !errors || p.severity == Severity::Error)
        .collect();
    candidates.sort_by_key(|p| (p.range.start, p.severity));
    candidates.dedup_by_key(|p| p.range.start);
    if forward {
        candidates
            .iter()
            .find(|p| p.range.start > offset)
            .or(candidates.first())
            .copied()
    } else {
        candidates
            .iter()
            .rev()
            .find(|p| p.range.start < offset)
            .or(candidates.last())
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0070 v7 AK-7.
    #[test]
    fn compact_indicator_in_narrow_editors() {
        let all = HighlightLevel::All;
        assert_eq!(compact((0, 0, 0), all), Compact::Clean);
        assert_eq!(
            compact((1, 3, 2), all),
            Compact::Sum {
                severity: 0,
                total: 6
            }
        );
        assert_eq!(
            compact((0, 3, 2), all),
            Compact::Sum {
                severity: 1,
                total: 5
            }
        );
        assert_eq!(
            compact((0, 0, 2), all),
            Compact::Sum {
                severity: 2,
                total: 2
            }
        );
        assert_eq!(compact((4, 0, 0), HighlightLevel::None), Compact::Off);
        const { assert!(COMPACT_BELOW > COMPACT_MAX_WIDTH * 4.0) };
    }

    // SPEC-0070 v4 AK-4.
    #[test]
    fn indicator_is_a_check_mark_or_counts() {
        assert_eq!(indicator((0, 0, 0)), Indicator::Clean);
        assert_eq!(
            indicator((2, 0, 0)),
            Indicator::Counts {
                errors: 2,
                warnings: 0,
                weak: 0
            }
        );
        assert_eq!(
            indicator((0, 3, 1)),
            Indicator::Counts {
                errors: 0,
                warnings: 3,
                weak: 1
            }
        );
        // v5 behaviour 9: a weak warning alone is no clean file.
        assert_ne!(indicator((0, 0, 1)), Indicator::Clean);
    }

    // SPEC-0070 v5 AK-5.
    #[test]
    fn unused_duplicates_stripe_summary_and_folds() {
        let mut own = problem(4, Severity::Weak);
        own.inspection = Some("java.unused-local");
        own.unused = true;
        let server = problem(4, Severity::Warning);
        let other = problem(9, Severity::Warning);
        assert!(without_duplicates(std::slice::from_ref(&server), vec![own.clone()]).is_empty());
        assert_eq!(
            without_duplicates(std::slice::from_ref(&other), vec![own.clone()]),
            [own.clone()]
        );
        let error = problem(12, Severity::Error);
        let list = vec![own.clone(), other.clone(), error.clone()];
        // Lines of 10 chars.
        let marks = stripe_marks(&list, |o| o / 10);
        assert_eq!(marks.len(), 2);
        assert_eq!((marks[0].line, marks[0].severity), (0, Severity::Warning));
        assert!(!marks[0].unused);
        assert_eq!(marks[0].offset, 9);
        assert_eq!(marks[0].messages, ["at 9", "at 4"]);
        assert_eq!(
            (marks[1].line, marks[1].severity, marks[1].offset),
            (1, Severity::Error, 12)
        );
        assert!(stripe_marks(std::slice::from_ref(&own), |o| o / 10)[0].unused);
        assert_eq!(stripe_fraction(0, 40), 0.0);
        assert_eq!(stripe_fraction(20, 40), 0.5);
        assert_eq!(counts3(&list), (1, 1, 1));
        assert_eq!(
            summary_words(counts3(&list)),
            "1 error, 1 warning, 1 weak warning"
        );
        assert_eq!(summary_words((0, 4, 2)), "4 warnings, 2 weak warnings");
        assert_eq!(summary_words((0, 0, 0)), "No problems");
        assert_eq!(
            hidden_in(&list, 8..20).map(|p| p.severity),
            Some(Severity::Error)
        );
        assert!(hidden_in(&list, 20..30).is_none());
        assert_eq!(under(&list, 4).len(), 1);
        assert!(under(&list, 5).is_empty());
    }

    // SPEC-0070 v6 AK-6.
    #[test]
    fn highlighting_levels_filter_problems() {
        let mut unused = problem(3, Severity::Warning);
        unused.unused = true;
        let list = vec![
            problem(0, Severity::Error),
            problem(1, Severity::Warning),
            problem(2, Severity::Weak),
            unused,
        ];
        let shown = |l| by_level(list.clone(), l);
        assert_eq!(shown(HighlightLevel::All).1, 0);
        assert_eq!(shown(HighlightLevel::All).0.len(), 4);
        let (essential, hidden) = shown(HighlightLevel::Essential);
        assert_eq!((essential.len(), hidden), (2, 2));
        assert!(essential.iter().all(|p| !p.unused));
        let (syntax, hidden) = shown(HighlightLevel::Syntax);
        assert_eq!((syntax.len(), hidden), (1, 3));
        assert_eq!(syntax[0].severity, Severity::Error);
        assert_eq!(shown(HighlightLevel::None), (Vec::new(), 4));
        assert_eq!(level_mark(HighlightLevel::All), None);
        assert_eq!(level_mark(HighlightLevel::None), Some("⊘"));
        assert_eq!(level_mark(HighlightLevel::Syntax), Some("Syntax"));
        assert_eq!(hidden_words(0), None);
        assert_eq!(
            hidden_words(1).as_deref(),
            Some("1 problem hidden by the level")
        );
        assert_eq!(
            hidden_words(4).as_deref(),
            Some("4 problems hidden by the level")
        );
    }

    fn problem(start: usize, severity: Severity) -> Problem {
        Problem {
            range: start..start + 1,
            severity,
            message: format!("at {start}"),
            inspection: None,
            unused: false,
            code: None,
        }
    }

    #[test]
    fn next_prefers_errors_and_wraps_around() {
        let list = vec![
            problem(30, Severity::Error),
            problem(5, Severity::Warning),
            problem(10, Severity::Error),
        ];
        let start = |p: Option<&Problem>| p.map(|p| p.range.start);
        assert_eq!(start(next(&list, 0, true)), Some(10));
        assert_eq!(start(next(&list, 10, true)), Some(30));
        assert_eq!(start(next(&list, 30, true)), Some(10));
        assert_eq!(start(next(&list, 30, false)), Some(10));
        assert_eq!(start(next(&list, 10, false)), Some(30));
        assert_eq!(start(next(&list, 20, false)), Some(10));
    }

    #[test]
    fn next_uses_warnings_without_errors_and_none_when_clean() {
        let list = vec![problem(8, Severity::Weak), problem(3, Severity::Warning)];
        assert_eq!(next(&list, 0, true).map(|p| p.range.start), Some(3));
        assert_eq!(next(&list, 3, true).map(|p| p.range.start), Some(8));
        assert!(next(&[], 0, true).is_none());
    }

    fn d(severity: &str, sl: u64, sc: u64, el: u64, ec: u64, message: &str) -> Diagnostic {
        Diagnostic {
            severity: severity.into(),
            start_line: sl,
            start_character: sc,
            end_line: el,
            end_character: ec,
            message: message.into(),
            source: Some("clang".into()),
            code: None,
            unnecessary: false,
        }
    }

    #[test]
    fn ranges_utf16_empty_and_counts() {
        let lines = ["int main() {", "  return 😀x;", "}"];
        let line = |i: usize| lines.get(i).map(|s| s.to_string());
        let starts = [0usize, 13, 26];
        let start = |i: usize| starts.get(i).copied().unwrap_or(28);
        let problems = to_problems(
            &[
                // `x` after an emoji: UTF-16 column 11 is char 10.
                d("error", 1, 11, 1, 12, "use of undeclared identifier 'x'"),
                d("warning", 0, 4, 0, 8, "unused"),
                d("information", 2, 1, 2, 1, "at end"),
            ],
            line,
            start,
        );
        assert_eq!(problems[0].range, 4..8);
        assert_eq!(problems[1].range, 23..24);
        assert_eq!(
            problems[1].message,
            "clang: use of undeclared identifier 'x'"
        );
        assert_eq!(
            problems[2].range,
            26..27,
            "an empty range at a line end marks the char before"
        );
        assert_eq!(counts(&problems), (1, 1));
        assert_eq!(at(&problems, 23).map(|p| p.severity), Some(Severity::Error));
        assert!(at(&problems, 12).is_none());
    }
}

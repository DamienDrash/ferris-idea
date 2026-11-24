//! The SQL console (`SPEC-0093`): which statement runs, and the texts and column widths of the result
//! grid and the output log.

use std::ops::Range;
use std::time::Duration;

use ferris_idea_database::{Dialect, Outcome, Statement, split_statements};

/// Most rows shown in the grid (behaviour 5).
pub const MAX_ROWS: usize = 500;
/// Widest column in pixels (behaviour 5).
pub const MAX_COLUMN: f32 = 320.;
/// How SQL NULL is shown (behaviour 5).
pub const NULL_TEXT: &str = "<null>";

/// The statements to run (behaviour 1): those overlapping a non-empty `selection`, else the statement
/// at `caret`. Offsets are bytes of `script`.
pub fn statements_to_run(
    script: &str,
    selection: Range<usize>,
    caret: usize,
    dialect: Dialect,
) -> Vec<Statement> {
    if !selection.is_empty() {
        let text = &script[selection.clone()];
        return split_statements(text, dialect)
            .into_iter()
            .map(|s| Statement {
                range: s.range.start + selection.start..s.range.end + selection.start,
                text: s.text,
            })
            .collect();
    }
    statement_at(script, caret, dialect).into_iter().collect()
}

/// The statements Explain takes (`SPEC-0110` v6 behaviour 15): a non-empty `selection` only when it
/// is made of whole statements of `script` (each starts where one of the script's statements starts
/// and has its text), else the statement at `caret`. The second value says why the selection was
/// set aside; with no statement at the caret the list is empty and it says that too.
pub fn statements_to_explain(
    script: &str,
    selection: Range<usize>,
    caret: usize,
    dialect: Dialect,
) -> (Vec<Statement>, Option<String>) {
    if selection.is_empty() {
        return (
            statement_at(script, caret, dialect).into_iter().collect(),
            None,
        );
    }
    let selected = statements_to_run(script, selection, caret, dialect);
    let all = split_statements(script, dialect);
    let bare = |t: &str| t.trim().trim_end_matches(';').trim_end().to_owned();
    let whole = !selected.is_empty()
        && selected.iter().all(|s| {
            all.iter()
                .any(|t| t.range.start == s.range.start && bare(&t.text) == bare(&s.text))
        });
    if whole {
        return (selected, None);
    }
    match statement_at(script, caret, dialect) {
        Some(s) => (
            vec![s],
            Some(
                "The selection is not a whole statement: explained the statement at the caret"
                    .into(),
            ),
        ),
        None => (
            Vec::new(),
            Some("The selection is not a whole statement and the caret is on no statement".into()),
        ),
    }
}

/// The statement at `caret`: the one containing it; else the one ending earlier on the caret's line
/// (the caret is after its `;`); else the one starting later on that line; else, on a comment line,
/// the statement those comment lines stand right before (`SPEC-0093` v6 behaviour 1). `None` on a
/// line without a statement.
pub fn statement_at(script: &str, caret: usize, dialect: Dialect) -> Option<Statement> {
    let statements = split_statements(script, dialect);
    let same_line = |a: usize, b: usize| !script[a.min(b)..a.max(b)].contains('\n');
    if let Some(s) = statements
        .iter()
        .find(|s| s.range.start <= caret && caret <= s.range.end)
    {
        return Some(s.clone());
    }
    if let Some(s) = statements
        .iter()
        .rev()
        .find(|s| s.range.end <= caret && same_line(s.range.end, caret))
    {
        return Some(s.clone());
    }
    if let Some(s) = statements
        .iter()
        .find(|s| s.range.start >= caret && same_line(caret, s.range.start))
    {
        return Some(s.clone());
    }
    // A comment line right before a statement: every line from the caret's to the statement's
    // first is a comment, no blank line between.
    let line_start = script[..caret].rfind('\n').map_or(0, |i| i + 1);
    let next = statements.iter().find(|s| s.range.start >= caret)?;
    let gap = &script[line_start..next.range.start];
    let comments_only = gap.lines().all(|l| comment_line(l.trim(), dialect));
    (comments_only && !gap.trim().is_empty()).then(|| next.clone())
}

/// Whether `line` (trimmed) is a comment line: `--`, MySQL's `#`, or part of a `/* … */` block.
fn comment_line(line: &str, dialect: Dialect) -> bool {
    line.starts_with("--")
        || (line.starts_with('#') && matches!(dialect, Dialect::MySql | Dialect::MariaDb))
        || line.starts_with("/*")
        || line.starts_with('*')
        || line.ends_with("*/")
}

/// The text of a cell.
pub fn cell_text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or(NULL_TEXT)
}

/// Column widths in pixels for a monospace font of `char_width`: the longest shown value or the
/// header, plus padding, at most [`MAX_COLUMN`].
pub fn column_widths(
    columns: &[String],
    rows: &[Vec<Option<String>>],
    char_width: f32,
) -> Vec<f32> {
    columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let longest = rows
                .iter()
                .take(MAX_ROWS)
                .filter_map(|r| r.get(i))
                .map(|v| cell_text(v).chars().count())
                .chain([name.chars().count()])
                .max()
                .unwrap_or(0);
            (longest as f32 * char_width + 16.).min(MAX_COLUMN)
        })
        .collect()
}

fn rows(n: usize) -> String {
    format!("{n} {}", if n == 1 { "row" } else { "rows" })
}

fn millis(time: Duration) -> String {
    format!("{} ms", time.as_millis())
}

/// The line under the grid: "2 rows · 5 ms" or "first 500 of 1200 rows · 5 ms".
pub fn summary(total: usize, time: Duration) -> String {
    if total > MAX_ROWS {
        format!("first {MAX_ROWS} of {} · {}", rows(total), millis(time))
    } else {
        format!("{} · {}", rows(total), millis(time))
    }
}

/// One line of the output log for a statement (behaviour 6), and whether it is an error.
pub fn output_line(statement: &str, outcome: &Outcome, time: Duration) -> (String, bool) {
    let first = statement.lines().next().unwrap_or_default().trim();
    let head: String = if first.chars().count() > 60 {
        first.chars().take(59).chain(['…']).collect()
    } else if statement.trim().contains('\n') {
        format!("{first} …")
    } else {
        first.to_owned()
    };
    let (result, failed) = match outcome {
        Outcome::ResultSet { rows: r, .. } => (rows(r.len()), false),
        Outcome::UpdateCount(n) => (format!("{} affected", rows(*n as usize)), false),
        Outcome::Error(e) => match e.position.and_then(|p| error_place(statement, p)) {
            // After the message's first line, before a help line such as Oracle's.
            Some(place) => {
                let text = e.to_string();
                let text = match text.split_once('\n') {
                    Some((first, rest)) => format!("{first} ({place})\n{rest}"),
                    None => format!("{text} ({place})"),
                };
                (text, true)
            }
            None => (e.to_string(), true),
        },
    };
    (format!("{head} — {result} · {}", millis(time)), failed)
}

/// Where a reported error position (1-based character offset into `statement`, `SPEC-0007`
/// behaviour 5) is, for the Output line: `at column 8`, or `at line 2, column 3 of the statement`
/// for a statement of several lines; `None` outside the statement (`SPEC-0093` v7 behaviour 6).
pub fn error_place(statement: &str, position: usize) -> Option<String> {
    let index = position.checked_sub(1)?;
    if index >= statement.chars().count() {
        return None;
    }
    let before: String = statement.chars().take(index).collect();
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    Some(if statement.contains('\n') {
        format!("at line {line}, column {column} of the statement")
    } else {
        format!("at column {column}")
    })
}

/// One result set: column names and rows (`None` is SQL NULL).
pub type Rows = (Vec<String>, Vec<Vec<Option<String>>>);

/// What one statement's outcomes add to a console run (`SPEC-0005` v4 behaviour 18): an Output line
/// per outcome, a result tab per result set (columns and rows, in order), and whether it failed.
#[derive(Debug, Default, PartialEq)]
pub struct StatementResults {
    /// Output lines and whether each is a failure.
    pub lines: Vec<(String, bool)>,
    /// One entry per result set: its columns and rows.
    pub sets: Vec<Rows>,
    /// Whether an outcome failed.
    pub failed: bool,
}

/// [`StatementResults`] of `statement` from its `outcomes`, which took `time`.
pub fn statement_results(
    statement: &str,
    outcomes: Vec<Outcome>,
    time: Duration,
) -> StatementResults {
    let mut out = StatementResults::default();
    for outcome in outcomes {
        let (line, failed) = output_line(statement, &outcome, time);
        out.lines.push((line, failed));
        out.failed |= failed;
        if let Outcome::ResultSet { columns, rows } = outcome {
            out.sets.push((columns, rows));
        }
    }
    out
}

/// The Output line of a server notice (`NOTICE: inside the block`), indented under its statement's
/// line (`SPEC-0093` v8 behaviour 6a).
pub fn notice_line(notice: &str) -> String {
    format!("  {}", notice.trim_end())
}

/// The output line for statements skipped after a failure.
pub fn not_run_line(count: usize) -> String {
    format!(
        "{count} {} not run",
        if count == 1 {
            "statement"
        } else {
            "statements"
        }
    )
}

/// One statement of a file as the result tabs know it (`SPEC-0190` v7 behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementKey {
    /// Its position among the file's statements, when it is one of them.
    pub index: Option<usize>,
    /// How many statements the file had.
    pub count: usize,
    /// Its text with white space folded.
    pub text: String,
}

impl StatementKey {
    /// Whether `other` is the same statement: the same text, or the same position in a file with
    /// the same number of statements (the statement was edited).
    fn same(&self, other: &StatementKey) -> bool {
        self.text == other.text
            || (self.index.is_some() && self.index == other.index && self.count == other.count)
    }
}

/// Folds the white space of a statement's text.
fn folded(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where a result tab came from: the file and its statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabOrigin {
    /// The file (or console) that ran.
    pub source: std::path::PathBuf,
    /// The statement that gave the result.
    pub statement: StatementKey,
}

/// What a console run ran: its file, whether that was the file's whole script, and the statements
/// in run order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOrigin {
    /// The file (or console).
    pub source: std::path::PathBuf,
    /// Every statement of the file ran (or was meant to).
    pub whole: bool,
    /// The statements, in run order.
    pub statements: Vec<StatementKey>,
}

impl RunOrigin {
    /// The origin of running `run` out of `script` (the file's text) of `source` in `dialect`.
    pub fn of(
        source: std::path::PathBuf,
        script: &str,
        run: &[Statement],
        dialect: Dialect,
    ) -> Self {
        let all = split_statements(script, dialect);
        let whole = !all.is_empty() && all.iter().all(|a| run.iter().any(|s| s.range == a.range));
        let statements = run
            .iter()
            .map(|s| StatementKey {
                index: all.iter().position(|a| a.range == s.range),
                count: all.len(),
                text: folded(&s.text),
            })
            .collect();
        Self {
            source,
            whole,
            statements,
        }
    }

    /// The origin of each result set, given the texts of the statements that produced them in
    /// result order (a statement may give several).
    pub fn result_origins<'a>(
        &self,
        results: impl IntoIterator<Item = &'a str>,
    ) -> Vec<Option<TabOrigin>> {
        let mut at = 0;
        results
            .into_iter()
            .map(|sql| {
                let text = folded(sql);
                let k = (at..self.statements.len()).find(|&k| self.statements[k].text == text)?;
                at = k;
                Some(TabOrigin {
                    source: self.source.clone(),
                    statement: self.statements[k].clone(),
                })
            })
            .collect()
    }
}

/// A result tab before a run, as [`arrange_tabs`] sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldTab {
    /// Pinned, or a table's data editor: it always stays.
    pub kept: bool,
    /// Where its result came from.
    pub origin: Option<TabOrigin>,
}

/// A tab after a run: an earlier one, or the run's result of that index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// The earlier tab with this index.
    Old(usize),
    /// The new result with this index.
    New(usize),
}

/// The result tabs after a run (`SPEC-0190` v7 behaviour 1): `old` are the tabs before it, `new`
/// the origins of the run's results, `run` what ran (`None` for a plan, a comparison or an HTTP
/// request) and `ran` how many of its statements ran before a failure stopped it. A whole script,
/// or a run without an origin, replaces every tab that is not kept; a run of some statements of a
/// file replaces only the tabs of the statements that ran, each new result taking the place of its
/// statement's tab, and the tabs from other files. Results without a place come last.
pub fn arrange_tabs(
    old: &[OldTab],
    new: &[Option<TabOrigin>],
    run: Option<&RunOrigin>,
    ran: usize,
) -> Vec<Slot> {
    let partial = run.filter(|r| !r.whole);
    let ran_keys = partial.map_or(&[][..], |r| &r.statements[..ran.min(r.statements.len())]);
    // The earlier tab's statement when it is one of the file's statements that ran again.
    let rerun = |o: &OldTab| -> Option<StatementKey> {
        let (r, origin) = (partial?, o.origin.as_ref()?);
        (origin.source == r.source && ran_keys.iter().any(|k| k.same(&origin.statement)))
            .then(|| origin.statement.clone())
    };
    let stays = |o: &OldTab| {
        o.kept
            || partial.is_some_and(|r| {
                o.origin
                    .as_ref()
                    .is_some_and(|origin| origin.source == r.source)
                    && rerun(o).is_none()
            })
    };
    let mut placed = vec![false; new.len()];
    let mut out = Vec::new();
    for (i, o) in old.iter().enumerate() {
        if stays(o) {
            out.push(Slot::Old(i));
            continue;
        }
        let Some(statement) = rerun(o) else { continue };
        for (j, n) in new.iter().enumerate() {
            if !placed[j] && n.as_ref().is_some_and(|n| n.statement.same(&statement)) {
                placed[j] = true;
                out.push(Slot::New(j));
            }
        }
    }
    out.extend((0..new.len()).filter(|&j| !placed[j]).map(Slot::New));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0093 v8 AK-15: a notice stands indented under its statement's line.
    #[test]
    fn a_notice_is_indented_under_its_statement() {
        assert_eq!(
            notice_line("NOTICE: inside; the block\n"),
            "  NOTICE: inside; the block"
        );
    }

    /// SPEC-0093 v7 behaviour 6 (hand test 68 retest): the error's place in the Output line.
    #[test]
    fn errors_name_their_place() {
        assert_eq!(
            error_place("select nosuch from dual", 8).as_deref(),
            Some("at column 8")
        );
        assert_eq!(
            error_place("select a,\n  nosuch\nfrom t", 13).as_deref(),
            Some("at line 2, column 3 of the statement")
        );
        assert_eq!(error_place("select 1", 0), None);
        assert_eq!(error_place("select 1", 9), None, "outside the statement");
        let e = Outcome::Error(ferris_idea_database::DbError {
            message: "ORA-00904: \"NOSUCH\": invalid identifier".into(),
            sqlstate: None,
            position: Some(8),
        });
        let (line, failed) = output_line("select nosuch from dual", &e, Duration::from_millis(2));
        assert!(failed);
        assert!(
            line.contains("ORA-00904") && line.contains("(at column 8)"),
            "{line}"
        );
    }

    /// SPEC-0005 v4 AK-19 (OBS-0051 L2): two result sets of one `CALL` are two result tabs, in
    /// order, with an Output line each; a failure after them is kept last.
    #[test]
    fn every_result_set_of_a_statement_is_a_tab() {
        let set = |v: &str| Outcome::ResultSet {
            columns: vec!["v".into()],
            rows: vec![vec![Some(v.into())]],
        };
        let r = statement_results("call two_sets()", vec![set("a"), set("b")], Duration::ZERO);
        assert_eq!(r.sets.len(), 2);
        assert_eq!(r.sets[0].1[0][0].as_deref(), Some("a"));
        assert_eq!(r.sets[1].1[0][0].as_deref(), Some("b"));
        assert_eq!(r.lines.len(), 2);
        assert!(!r.failed);
        let failure = Outcome::Error(ferris_idea_database::DbError {
            message: "boom".into(),
            ..Default::default()
        });
        let r = statement_results("call two_sets()", vec![set("a"), failure], Duration::ZERO);
        assert_eq!((r.sets.len(), r.lines.len(), r.failed), (1, 2, true));
        assert!(r.lines[1].1);
    }

    /// SPEC-0110 v6 AK-15 (OBS-0051 L3).
    #[test]
    fn explain_takes_the_statement_at_the_caret_for_a_partial_selection() {
        let d = Dialect::PostgreSql;
        let script = "select a from t;\nselect b\n  from u;\n\n-- note\n";
        let at = |needle: &str| script.find(needle).unwrap();
        let texts = |v: &[Statement]| v.iter().map(|s| s.text.clone()).collect::<Vec<_>>();
        // A whole statement, with or without its `;` and blanks around it.
        let (one, note) = statements_to_explain(script, 0..16, 3, d);
        assert_eq!(
            (texts(&one), note),
            (vec!["select a from t".to_owned()], None)
        );
        let (one, note) = statements_to_explain(script, 0..15, 3, d);
        assert_eq!(
            (texts(&one), note),
            (vec!["select a from t".to_owned()], None)
        );
        // Two whole statements.
        let (two, note) = statements_to_explain(script, 0..at("-- note"), 3, d);
        assert_eq!((two.len(), note), (2, None));
        // A selected word: the statement at the caret, and the status line says so.
        let from = at("from u");
        let (taken, note) = statements_to_explain(script, from..from + 4, from + 4, d);
        assert_eq!(texts(&taken), ["select b\n  from u"]);
        assert_eq!(
            note.as_deref(),
            Some("The selection is not a whole statement: explained the statement at the caret")
        );
        // Half a statement through the end of the file, the caret on the comment's line: none.
        let note_at = at("note");
        let (none, note) = statements_to_explain(script, note_at..note_at + 4, script.len(), d);
        assert!(none.is_empty());
        assert_eq!(
            note.as_deref(),
            Some("The selection is not a whole statement and the caret is on no statement")
        );
    }
    use ferris_idea_database::DbError;

    const SQL: &str = "select 1;\nselect *\n  from t;   \n\nselect 3";

    fn text_at(caret: usize) -> Option<String> {
        statement_at(SQL, caret, Dialect::Sqlite).map(|s| s.text)
    }

    #[test]
    fn statement_at_the_caret() {
        assert_eq!(text_at(3).as_deref(), Some("select 1"), "inside");
        assert_eq!(
            text_at(9).as_deref(),
            Some("select 1"),
            "after the semicolon"
        );
        assert_eq!(
            text_at(12).as_deref(),
            Some("select *\n  from t"),
            "over lines"
        );
        let after = SQL.find("t;").unwrap() + 4;
        assert_eq!(
            text_at(after).as_deref(),
            Some("select *\n  from t"),
            "spaces after ;"
        );
        let blank = SQL.find("\n\n").unwrap() + 1;
        assert_eq!(text_at(blank), None, "a line without a statement");
        // SPEC-0093 v6 AK-13: on a comment line right before a statement that statement runs;
        // after a blank line, or on a comment after the last statement, nothing does.
        let commented = "select 1;\n-- totals per day\n/* all shops */\nselect 2;\n\n-- end\n";
        let at = |needle: &str| {
            statement_at(
                commented,
                commented.find(needle).unwrap() + 2,
                Dialect::Sqlite,
            )
            .map(|s| s.text)
        };
        assert_eq!(at("totals").as_deref(), Some("select 2"));
        assert_eq!(at("all shops").as_deref(), Some("select 2"));
        assert_eq!(at("end"), None);
        let apart = "-- note\n\nselect 5;";
        assert_eq!(statement_at(apart, 3, Dialect::Sqlite), None);
        assert_eq!(
            text_at(SQL.len()).as_deref(),
            Some("select 3"),
            "last without ;"
        );
        let two = statements_to_run(SQL, 0..SQL.find("from").unwrap() + 9, 0, Dialect::Sqlite);
        assert_eq!(two.len(), 2, "a selection runs all its statements");
        assert_eq!(&SQL[two[1].range.clone()], "select *\n  from t");
    }

    /// SPEC-0005 v3 AK-16: with `DELIMITER` the caret in a procedure body runs the whole
    /// procedure, and the `DELIMITER` line itself is no statement.
    #[test]
    fn statement_at_the_caret_with_delimiter() {
        let sql = "DELIMITER //\ncreate procedure p()\nbegin\n  select 1;\nend //\nDELIMITER ;\nselect 2;\n";
        let at = |needle: &str| {
            statement_at(sql, sql.find(needle).unwrap() + 1, Dialect::MySql).map(|s| s.text)
        };
        let procedure = "create procedure p()\nbegin\n  select 1;\nend";
        assert_eq!(at("select 1").as_deref(), Some(procedure));
        assert_eq!(at("begin").as_deref(), Some(procedure));
        assert_eq!(at("select 2").as_deref(), Some("select 2"));
        assert_eq!(at("DELIMITER ;"), None);
        // The caret on the `DELIMITER //` line: the procedure starts on the next line, but the
        // command line is no comment, so nothing runs there.
        assert_eq!(statement_at(sql, 3, Dialect::MySql), None);
    }

    #[test]
    fn grid_and_log_texts() {
        let rows = vec![
            vec![Some("1".into()), None],
            vec![Some("12345".into()), Some("x".repeat(100))],
        ];
        let w = column_widths(&["id".into(), "name".into()], &rows, 8.);
        assert_eq!(w[0], 5. * 8. + 16.);
        assert_eq!(w[1], MAX_COLUMN);
        assert_eq!(cell_text(&None), "<null>");
        let ms = Duration::from_millis(5);
        assert_eq!(summary(1, ms), "1 row · 5 ms");
        assert_eq!(summary(1200, ms), "first 500 of 1200 rows · 5 ms");
        let set = Outcome::ResultSet {
            columns: vec!["a".into()],
            rows: vec![vec![None], vec![None]],
        };
        assert_eq!(
            output_line("select a\nfrom t", &set, ms),
            ("select a … — 2 rows · 5 ms".into(), false)
        );
        assert_eq!(
            output_line("delete from t", &Outcome::UpdateCount(3), ms).0,
            "delete from t — 3 rows affected · 5 ms"
        );
        let err = Outcome::Error(DbError {
            message: "no such table: x".into(),
            sqlstate: None,
            position: None,
        });
        assert_eq!(
            output_line("select * from x", &err, ms),
            ("select * from x — no such table: x · 5 ms".into(), true)
        );
        assert_eq!(not_run_line(2), "2 statements not run");
    }

    /// SPEC-0190 v7 AK-11 (OBS-0070 V2): which tabs a run replaces and where its results go.
    #[test]
    fn a_run_replaces_the_tabs_of_its_statements() {
        use std::path::PathBuf;
        let script = "select 1;\nselect 2;\nselect 3;";
        let (a, b) = (PathBuf::from("/w/a.sql"), PathBuf::from("/w/b.sql"));
        let all = split_statements(script, Dialect::Sqlite);
        let run = |source: &PathBuf, statements: &[Statement]| {
            RunOrigin::of(source.clone(), script, statements, Dialect::Sqlite)
        };
        let whole = run(&a, &all);
        let second = run(&a, &all[1..2]);
        assert!(whole.whole && !second.whole);
        assert_eq!(
            second.statements,
            [StatementKey {
                index: Some(1),
                count: 3,
                text: "select 2".into()
            }]
        );
        // A statement with two result sets gives two tabs of the same statement.
        let origins = second.result_origins(["select 2", "select 2"]);
        assert!(
            origins
                .iter()
                .all(|o| o.as_ref().is_some_and(|o| o.statement.index == Some(1)))
        );
        // The tabs of a whole run of a.sql, and a pinned tab of b.sql before them.
        let tab = |source: &PathBuf, i: usize, text: &str, kept: bool| OldTab {
            kept,
            origin: Some(TabOrigin {
                source: source.clone(),
                statement: StatementKey {
                    index: Some(i),
                    count: 3,
                    text: text.into(),
                },
            }),
        };
        let old = [
            tab(&b, 0, "select 9", true),
            tab(&a, 0, "select 1", false),
            tab(&a, 1, "select 2", false),
            tab(&a, 2, "select 3", false),
        ];
        let new_of = |r: &RunOrigin, texts: &[&str]| r.result_origins(texts.iter().copied());
        // One statement: only its tab is replaced, in its place.
        assert_eq!(
            arrange_tabs(&old, &new_of(&second, &["select 2"]), Some(&second), 1),
            [Slot::Old(0), Slot::Old(1), Slot::New(0), Slot::Old(3)]
        );
        // The whole script again: every tab that is not pinned.
        assert_eq!(
            arrange_tabs(
                &old,
                &new_of(&whole, &["select 1", "select 2", "select 3"]),
                Some(&whole),
                3
            ),
            [Slot::Old(0), Slot::New(0), Slot::New(1), Slot::New(2)]
        );
        // An edited statement keeps its place: same position, same number of statements.
        let edited = RunOrigin {
            source: a.clone(),
            whole: false,
            statements: vec![StatementKey {
                index: Some(1),
                count: 3,
                text: "select 22".into(),
            }],
        };
        assert_eq!(
            arrange_tabs(&old, &new_of(&edited, &["select 22"]), Some(&edited), 1),
            [Slot::Old(0), Slot::Old(1), Slot::New(0), Slot::Old(3)]
        );
        // With a statement more in the file the edited one is new: the old tab stays.
        let moved = RunOrigin {
            statements: vec![StatementKey {
                index: Some(1),
                count: 4,
                text: "select 22".into(),
            }],
            ..edited.clone()
        };
        assert_eq!(
            arrange_tabs(&old, &new_of(&moved, &["select 22"]), Some(&moved), 1),
            [
                Slot::Old(0),
                Slot::Old(1),
                Slot::Old(2),
                Slot::Old(3),
                Slot::New(0)
            ]
        );
        // The second statement fails: its tab goes, the third did not run and keeps its tab.
        let two = run(&a, &all[1..3]);
        assert_eq!(
            arrange_tabs(&old, &[], Some(&two), 1),
            [Slot::Old(0), Slot::Old(1), Slot::Old(3)]
        );
        // Another file, or a run without a file: the tabs that are not pinned are replaced.
        let other = RunOrigin {
            source: b.clone(),
            ..second.clone()
        };
        assert_eq!(
            arrange_tabs(&old, &new_of(&other, &["select 2"]), Some(&other), 1),
            [Slot::Old(0), Slot::New(0)]
        );
        assert_eq!(
            arrange_tabs(&old, &[None], None, 0),
            [Slot::Old(0), Slot::New(0)]
        );
    }
}

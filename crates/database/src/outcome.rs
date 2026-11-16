//! Outcomes of running an SQL script, one per statement (`SPEC-0007`, behaviour 1–6).
//!
//! The executor (drivers, M5) reports what each statement did; this module keeps
//! every outcome in script order, stops or continues after errors, and maps the
//! database's error position onto the script.

use std::fmt;
use std::ops::Range;

use crate::Statement;

/// What one statement produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Rows returned by a query.
    ResultSet {
        /// Column names in result order.
        columns: Vec<String>,
        /// Rows; `None` is SQL NULL.
        rows: Vec<Vec<Option<String>>>,
    },
    /// Number of rows changed by a data-modifying statement.
    UpdateCount(u64),
    /// The statement failed.
    Error(DbError),
}

/// An error as reported by the database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DbError {
    /// Message text from the database; may be empty.
    pub message: String,
    /// Five-character SQLSTATE, if the driver reports one.
    pub sqlstate: Option<String>,
    /// 1-based character offset into the statement text, if reported.
    pub position: Option<usize>,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.trim().is_empty() {
            f.write_str("the database returned no error message")?;
        } else {
            f.write_str(self.message.trim())?;
        }
        if let Some(state) = &self.sqlstate {
            write!(f, " (SQLSTATE {state})")?;
        }
        Ok(())
    }
}

/// What to do after a statement fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnError {
    /// Stop the run; later statements are not executed.
    Stop,
    /// Run the remaining statements.
    Continue,
}

/// The outcome of one statement together with where it is in the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementOutcome {
    /// Byte range of the statement in the script.
    pub range: Range<usize>,
    /// What the statement produced.
    pub outcome: Outcome,
    /// Byte offset in the script that the error points at, if any.
    pub error_offset: Option<usize>,
    /// What the server said while the statement ran (PostgreSQL's `RAISE NOTICE`, warnings), as
    /// `SEVERITY: message`, in order (`SPEC-0093` v8 behaviour 6a).
    pub notices: Vec<String>,
}

/// The outcomes of a script run, in script order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptRun {
    /// One entry per executed statement.
    pub outcomes: Vec<StatementOutcome>,
    /// Statements skipped because an earlier one failed with [`OnError::Stop`].
    pub not_run: usize,
}

/// Runs `statements` through `execute` and records every outcome.
///
/// `script` is the text the statements were split from; it is needed to map
/// error positions (character offsets) to byte offsets.
pub fn run_script<F>(
    script: &str,
    statements: &[Statement],
    on_error: OnError,
    mut execute: F,
) -> ScriptRun
where
    F: FnMut(&Statement) -> Outcome,
{
    let mut run = ScriptRun::default();
    for (index, statement) in statements.iter().enumerate() {
        let failed = run.push(script, statement, execute(statement));
        if failed && on_error == OnError::Stop {
            run.not_run = statements.len() - index - 1;
            break;
        }
    }
    run
}

impl ScriptRun {
    /// Records the outcome of `statement`, mapping an error position into
    /// `script`. Returns `true` when the outcome is an error, so the caller can
    /// stop (asynchronous drivers use this instead of [`run_script`]).
    pub fn push(&mut self, script: &str, statement: &Statement, outcome: Outcome) -> bool {
        let error_offset = match &outcome {
            Outcome::Error(err) => err
                .position
                .and_then(|pos| error_offset(script, &statement.range, pos)),
            _ => None,
        };
        let failed = matches!(outcome, Outcome::Error(_));
        self.outcomes.push(StatementOutcome {
            range: statement.range.clone(),
            outcome,
            error_offset,
            notices: Vec::new(),
        });
        failed
    }
}

/// Maps a 1-based character `position` inside the statement at `range` to a
/// byte offset in `script`. Positions outside the statement give `None`.
pub fn error_offset(script: &str, range: &Range<usize>, position: usize) -> Option<usize> {
    let text = script.get(range.clone())?;
    let index = position.checked_sub(1)?;
    text.char_indices()
        .nth(index)
        .map(|(byte, _)| range.start + byte)
}

/// 1-based character position in `statement` where `near` starts on its 1-based `line`, or of the
/// statement's last character when `near` is empty (the server stopped at its end). For servers that
/// name the text they stopped at instead of a position (SPEC-0010 v2 behaviour 4).
pub(crate) fn position_near(statement: &str, line: usize, near: &str) -> Option<usize> {
    let start = line_start(statement, line)?;
    let byte = if near.is_empty() {
        statement.char_indices().last()?.0
    } else {
        start + statement[start..].find(near)?
    };
    Some(statement[..byte].chars().count() + 1)
}

/// 1-based character position of the first non-blank character of the 1-based `line` of
/// `statement`; `None` for a statement of one line, where it would say nothing (SPEC-0116 v2).
pub(crate) fn position_of_line(statement: &str, line: usize) -> Option<usize> {
    if !statement.trim_end().contains('\n') {
        return None;
    }
    let start = line_start(statement, line)?;
    let blank = statement[start..].len() - statement[start..].trim_start().len();
    let byte = start + blank;
    (byte < statement.len()).then(|| statement[..byte].chars().count() + 1)
}

fn line_start(text: &str, line: usize) -> Option<usize> {
    if line <= 1 {
        return Some(0);
    }
    text.match_indices('\n').nth(line - 2).map(|(i, _)| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dialect, split_statements};

    fn rows(value: &str) -> Outcome {
        Outcome::ResultSet {
            columns: vec!["v".into()],
            rows: vec![vec![Some(value.into())]],
        }
    }

    fn error(position: Option<usize>) -> Outcome {
        Outcome::Error(DbError {
            message: "column \"x\" does not exist".into(),
            sqlstate: Some("42703".into()),
            position,
        })
    }

    #[test]
    fn every_statement_keeps_its_outcome_in_order() {
        let script = "select 1; select 2";
        let statements = split_statements(script, Dialect::PostgreSql);
        let run = run_script(script, &statements, OnError::Stop, |s| rows(&s.text));
        assert_eq!(run.outcomes.len(), 2);
        assert_eq!(run.outcomes[0].outcome, rows("select 1"));
        assert_eq!(run.outcomes[1].outcome, rows("select 2"));
        assert_eq!(&script[run.outcomes[1].range.clone()], "select 2");
        assert_eq!(run.not_run, 0);
    }

    #[test]
    fn stop_and_continue_after_an_error() {
        let script = "select 1; select x; select 3";
        let statements = split_statements(script, Dialect::PostgreSql);
        let exec = |s: &Statement| {
            if s.text.contains('x') {
                error(None)
            } else {
                rows(&s.text)
            }
        };
        let stopped = run_script(script, &statements, OnError::Stop, exec);
        assert_eq!(stopped.outcomes.len(), 2);
        assert!(matches!(stopped.outcomes[1].outcome, Outcome::Error(_)));
        assert_eq!(stopped.not_run, 1);

        let all = run_script(script, &statements, OnError::Continue, exec);
        assert_eq!(all.outcomes.len(), 3);
        assert_eq!(all.not_run, 0);
    }

    #[test]
    fn error_position_maps_to_script_bytes() {
        // Multi-byte characters before and inside the failing statement.
        let script = "select 'äö'; select ü, x from t";
        let statements = split_statements(script, Dialect::PostgreSql);
        let run = run_script(script, &statements, OnError::Stop, |s| {
            if s.text.contains('x') {
                error(Some(11))
            } else {
                rows("ok")
            }
        });
        let offset = run.outcomes[1].error_offset.expect("position mapped");
        assert_eq!(&script[offset..], "x from t");
    }

    #[test]
    fn position_outside_the_statement_is_dropped() {
        let script = "select x";
        assert_eq!(error_offset(script, &(0..8), 8), Some(7));
        assert_eq!(error_offset(script, &(0..8), 9), None);
        assert_eq!(error_offset(script, &(0..8), 0), None);
    }

    #[test]
    fn missing_message_is_said_plainly() {
        let empty = DbError::default();
        assert_eq!(empty.to_string(), "the database returned no error message");
        let full = DbError {
            message: "column \"x\" does not exist".into(),
            sqlstate: Some("42703".into()),
            position: Some(8),
        };
        assert_eq!(
            full.to_string(),
            "column \"x\" does not exist (SQLSTATE 42703)"
        );
        assert!(!empty.to_string().contains("unknown"));
    }

    #[test]
    fn positions_from_a_line_and_the_text_the_server_stopped_at() {
        let sql = "select 1,\n  2 frm movement";
        let at = position_near(sql, 2, "frm movement").unwrap();
        assert_eq!(sql.chars().nth(at - 1), Some('f'));
        assert_eq!(position_near("select ä frm t", 1, "frm t"), Some(10));
        // Empty text: the end of the statement.
        assert_eq!(position_near("select", 1, ""), Some(6));
        assert_eq!(position_near(sql, 3, "x"), None);
        assert_eq!(position_near(sql, 2, "nowhere"), None);

        let at = position_of_line(sql, 2).unwrap();
        assert_eq!(sql.chars().nth(at - 1), Some('2'));
        assert_eq!(position_of_line("select 1 frm t", 1), None);
        assert_eq!(position_of_line(sql, 4), None);
    }
}

//! Oracle sessions (`SPEC-0142`, ADR-0011).
//!
//! Through the `oracle` crate (ODPI-C), which loads Oracle Instant Client at run time: from
//! `FERRIS_IDEA_ORACLE_CLIENT`, else the newest `~/.local/opt/instantclient_*`, else the system's
//! library search path. The driver is blocking; every call runs on Tokio's blocking pool. Each
//! statement is committed after it ran, except inside a transaction opened with `SET
//! TRANSACTION` (editing, SPEC-0017), which ends with `COMMIT` or `ROLLBACK`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::session::{SessionError, TlsPlan, mode_name, prefer_needs_tls, prepare, tls_plan};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, SslMode,
    StatementClass, classify, split_statements,
};

/// Rows of a query: column names and values as text.
pub type ResultSet = (Vec<String>, Vec<Vec<Option<String>>>);

struct Inner {
    conn: oracle::Connection,
    /// A transaction opened with `SET TRANSACTION` is running.
    in_transaction: bool,
}

/// An open connection to an Oracle database.
pub struct OracleSession {
    inner: Arc<Mutex<Inner>>,
    read_only: bool,
}

/// The Instant Client folder to load: `FERRIS_IDEA_ORACLE_CLIENT`, else the newest
/// `~/.local/opt/instantclient_*` with a client library, else none (system search path).
pub fn client_dir() -> Option<PathBuf> {
    if let Some(dir) = ferris_idea_core::product_var_os("FERRIS_IDEA_ORACLE_CLIENT") {
        return Some(PathBuf::from(dir));
    }
    let opt = PathBuf::from(std::env::var_os("HOME")?).join(".local/opt");
    let mut found: Vec<PathBuf> = std::fs::read_dir(opt)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("instantclient_"))
                && (p.join("libclntsh.dylib").exists() || p.join("libclntsh.so").exists())
        })
        .collect();
    found.sort();
    found.pop()
}

/// Loads Instant Client once; later calls return the first result.
fn init_client() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        let mut params = oracle::InitParams::new();
        if let Some(dir) = client_dir() {
            params
                .oracle_client_lib_dir(&dir)
                .map_err(|e| e.to_string())?;
        }
        params.init().map(|_| ()).map_err(|e| {
            format!(
                "Oracle Instant Client could not be loaded ({e}); install it under \
                 ~/.local/opt/instantclient_* or set FERRIS_IDEA_ORACLE_CLIENT"
            )
        })
    })
    .clone()
}

/// The EZConnect string: `//host:port/service`, or `tcps://…` when the profile needs TLS —
/// `require`, `verify-ca`, `verify-full`, and `prefer` off the loopback (`SPEC-0142` v3 behaviour 2:
/// TCPS has a listener port of its own, so there is nothing to try first).
pub fn connect_string(profile: &ConnectionProfile) -> String {
    let host = profile.host.clone().unwrap_or_else(|| "localhost".into());
    let port = profile.effective_port().unwrap_or(1521);
    let service = profile.database.clone().unwrap_or_default();
    match tls_plan(profile) {
        TlsPlan::Required => format!("tcps://{host}:{port}/{service}"),
        TlsPlan::Off | TlsPlan::Optional => format!("//{host}:{port}/{service}"),
    }
}

fn to_db_error(e: &oracle::Error) -> DbError {
    let position = e
        .db_error()
        .map(|d| d.offset())
        .filter(|o| *o > 0)
        .map(|o| o as usize + 1);
    DbError {
        message: e.to_string(),
        sqlstate: None,
        position,
    }
}

impl OracleSession {
    /// Opens a session for `profile` with already resolved `credentials`.
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        let password = prepare(profile, credentials, Dialect::Oracle)?.unwrap_or_default();
        let user = profile.user.clone().unwrap_or_default();
        let target = connect_string(profile);
        let name = profile.name.clone();
        let address = target.clone();
        let tls = tls_plan(profile) == TlsPlan::Required;
        let mode = profile.ssl;
        let opened = tokio::task::spawn_blocking(move || {
            init_client()?;
            oracle::Connection::connect(user, password, &target).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        let mut conn = opened.map_err(|message| {
            if tls && mode == SslMode::Prefer {
                // Off the loopback `prefer` needs TCPS (`SPEC-0016` v3 behaviour 13).
                prefer_needs_tls(profile, &address, &message)
            } else if tls {
                SessionError::Tls {
                    profile: name.clone(),
                    mode: mode_name(mode),
                    message: format!("{address}: {message}"),
                }
            } else {
                SessionError::Connect {
                    profile: name.clone(),
                    address: address.clone(),
                    message,
                }
            }
        })?;
        conn.set_autocommit(false);
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                conn,
                in_transaction: false,
            })),
            read_only: profile.read_only,
        })
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let statements = split_statements(script, Dialect::Oracle);
        let inner = Arc::clone(&self.inner);
        let read_only = self.read_only;
        let texts: Vec<String> = statements.iter().map(|s| s.text.clone()).collect();
        let outcomes = tokio::task::spawn_blocking(move || {
            let mut inner = match inner.lock() {
                Ok(i) => i,
                Err(p) => p.into_inner(),
            };
            let mut out = Vec::new();
            for sql in &texts {
                let note = (!inner.in_transaction && bare_rollback(sql))
                    .then(|| NOTHING_TO_ROLL_BACK.to_owned());
                let outcome = match refusal(sql, read_only) {
                    Some(error) => Outcome::Error(error),
                    None => execute(&mut inner, sql),
                };
                let failed = matches!(outcome, Outcome::Error(_));
                out.push((outcome, note.filter(|_| !failed)));
                if failed && on_error == OnError::Stop {
                    break;
                }
            }
            out
        })
        .await
        .unwrap_or_default();
        let mut run = ScriptRun::default();
        let count = outcomes.len();
        for (statement, (outcome, note)) in statements.iter().zip(outcomes) {
            run.push(script, statement, outcome);
            if let (Some(note), Some(last)) = (note, run.outcomes.last_mut()) {
                last.notices.push(note);
            }
        }
        if count < statements.len() {
            run.not_run = statements.len() - count;
        }
        run
    }

    /// The `PLAN_TABLE` rows of `statement` under `id` (`SPEC-0142` v4 behaviour 6): its old rows
    /// are deleted, `EXPLAIN PLAN` writes the new ones, `select` reads them and they are deleted
    /// again. The statement itself never runs. This also works on a read-only profile, whose
    /// refusal would stop the plan's own `DELETE` and `EXPLAIN PLAN`: Oracle's `PLAN_TABLE` is a
    /// temporary table private to the session. A schema with a `PLAN_TABLE` of its own would be
    /// written to, so a read-only profile refuses the plan then.
    pub async fn plan_rows(
        &self,
        statement: &str,
        id: &str,
        select: &str,
    ) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let inner = Arc::clone(&self.inner);
        let read_only = self.read_only;
        let delete = format!("DELETE FROM plan_table WHERE statement_id = '{id}'");
        let explain = format!("EXPLAIN PLAN SET STATEMENT_ID = '{id}' FOR {statement}");
        let select = select.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut inner = match inner.lock() {
                Ok(i) => i,
                Err(p) => p.into_inner(),
            };
            if read_only {
                let (_, own) = rows(
                    &inner.conn,
                    "select count(*) from user_tables where table_name = 'PLAN_TABLE'",
                )?;
                if own
                    .first()
                    .and_then(|r| r.first())
                    .cloned()
                    .flatten()
                    .as_deref()
                    != Some("0")
                {
                    return Err(DbError {
                        message: "the connection is read-only and the schema has a PLAN_TABLE of \
                                  its own, which Explain Plan would write to"
                            .into(),
                        ..DbError::default()
                    });
                }
            }
            let step = |inner: &mut Inner, sql: &str| match execute(inner, sql) {
                Outcome::Error(e) => Err(e),
                _ => Ok(()),
            };
            step(&mut inner, &delete)?;
            step(&mut inner, &explain)?;
            let read = rows(&inner.conn, &select).map(|(_, rows)| rows);
            let cleared = step(&mut inner, &delete);
            let read = read?;
            cleared?;
            Ok(read)
        })
        .await
        .map_err(|e| DbError {
            message: e.to_string(),
            ..DbError::default()
        })?
    }

    /// The rows of one query.
    pub async fn query(&self, sql: &str) -> Result<ResultSet, DbError> {
        let inner = Arc::clone(&self.inner);
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || {
            let inner = match inner.lock() {
                Ok(i) => i,
                Err(p) => p.into_inner(),
            };
            rows(&inner.conn, &sql)
        })
        .await
        .map_err(|e| DbError {
            message: e.to_string(),
            ..DbError::default()
        })?
    }
}

/// Oracle has no read-only session that also refuses DDL: a read-only profile refuses everything
/// that is not known to only read, except `SET` statements.
fn refusal(sql: &str, read_only: bool) -> Option<DbError> {
    let first = crate::script::code_words(sql, Dialect::Oracle)
        .into_iter()
        .next();
    (read_only
        && classify(sql, Dialect::Oracle) != StatementClass::ReadData
        && first.as_deref() != Some("set"))
    .then(|| DbError {
        message: "the connection is read-only; only queries and SET statements run".into(),
        sqlstate: None,
        position: None,
    })
}

fn rows(conn: &oracle::Connection, sql: &str) -> Result<ResultSet, DbError> {
    let result = conn.query(sql, &[]).map_err(|e| to_db_error(&e))?;
    let columns: Vec<String> = result
        .column_info()
        .iter()
        .map(|c| c.name().to_owned())
        .collect();
    // The scale of each NUMBER(p, s) column with s > 0: its values keep that many digits.
    let scales: Vec<Option<usize>> = result
        .column_info()
        .iter()
        .map(|c| match c.oracle_type() {
            oracle::sql_type::OracleType::Number(_, s) if *s > 0 => Some(*s as usize),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for row in result {
        let row = row.map_err(|e| to_db_error(&e))?;
        let values = (0..columns.len())
            .map(|i| {
                let value = row.get::<usize, Option<String>>(i).unwrap_or(None)?;
                Some(match scales[i] {
                    Some(scale) => number_text(&value, scale),
                    None => value,
                })
            })
            .collect();
        out.push(values);
    }
    Ok((columns, out))
}

/// A value of a `NUMBER(p, s)` column with exactly `scale` digits after the point and a zero
/// before it (`SPEC-0142` v5 behaviour 3b): Oracle keeps no trailing zeros, so 4.50 comes as
/// `4.5`. Text that is no plain decimal number (an exponent) is left as it is.
fn number_text(value: &str, scale: usize) -> String {
    let (sign, digits) = match value.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", value),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return value.to_owned();
    }
    let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if frac.contains('.') || frac.len() > scale {
        return value.to_owned();
    }
    let int = if int.is_empty() { "0" } else { int };
    format!("{sign}{int}.{frac:0<scale$}")
}

/// The note for a `ROLLBACK` outside a transaction opened with `SET TRANSACTION`: every statement
/// was committed when it ran, so there is nothing to undo (`SPEC-0142` v5 behaviour 3c).
pub(crate) const NOTHING_TO_ROLL_BACK: &str = "NOTE: auto-commit is on: every statement was committed when it ran, so ROLLBACK had nothing to undo";

/// Whether `sql` is a `ROLLBACK [WORK]` of the whole transaction, not to a savepoint.
fn bare_rollback(sql: &str) -> bool {
    let words = crate::script::code_words(sql, Dialect::Oracle);
    words.first().map(String::as_str) == Some("rollback") && !words.iter().any(|w| w == "to")
}

fn execute(inner: &mut Inner, sql: &str) -> Outcome {
    let words = crate::script::code_words(sql, Dialect::Oracle);
    let first = words.first().map(String::as_str);
    let second = words.get(1).map(String::as_str);
    // Transaction control goes to the driver, so its state stays consistent.
    match (first, second) {
        (Some("commit"), _) => {
            inner.in_transaction = false;
            return match inner.conn.commit() {
                Ok(()) => Outcome::UpdateCount(0),
                Err(e) => Outcome::Error(to_db_error(&e)),
            };
        }
        (Some("rollback"), None) => {
            inner.in_transaction = false;
            return match inner.conn.rollback() {
                Ok(()) => Outcome::UpdateCount(0),
                Err(e) => Outcome::Error(to_db_error(&e)),
            };
        }
        (Some("set"), Some("transaction")) => inner.in_transaction = true,
        _ => {}
    }
    let statement = inner.conn.statement(sql).build();
    let mut statement = match statement {
        Ok(s) => s,
        Err(e) => return Outcome::Error(to_db_error(&e)),
    };
    if statement.is_query() {
        drop(statement);
        return match rows(&inner.conn, sql) {
            Ok((columns, rows)) => Outcome::ResultSet { columns, rows },
            Err(e) => Outcome::Error(e),
        };
    }
    let outcome = match statement.execute(&[]) {
        Ok(()) => Outcome::UpdateCount(statement.row_count().unwrap_or(0)),
        Err(e) => return Outcome::Error(to_db_error(&e)),
    };
    drop(statement);
    // A unit Oracle stored but could not compile comes back as a warning (`SPEC-0142` v2
    // behaviour 3a).
    if let Some(warning) = inner.conn.last_warning()
        && warning
            .db_error()
            .is_some_and(|d| d.code() == COMPILED_WITH_ERRORS)
        && let Some(unit) = created_unit(sql)
    {
        let listed = format!(
            "select line, position, text from user_errors where type = '{}' and name = '{}' \
             order by sequence",
            unit.kind,
            unit.name.replace('\'', "''")
        );
        let errors = rows(&inner.conn, &listed)
            .map(|(_, rows)| {
                rows.into_iter()
                    .map(|r| {
                        let number = |i: usize| {
                            r.get(i)
                                .cloned()
                                .flatten()
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(0)
                        };
                        let text = r.get(2).cloned().flatten().unwrap_or_default();
                        (number(0), number(1), text.trim().to_owned())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let message = warning
            .db_error()
            .map(|d| d.message().to_owned())
            .unwrap_or_default();
        return Outcome::Error(compile_error(sql, &message, &unit, &errors));
    }
    if !inner.in_transaction
        && let Err(e) = inner.conn.commit()
    {
        return Outcome::Error(to_db_error(&e));
    }
    outcome
}

/// ORA-24344, success with compilation error: the unit is stored but invalid.
const COMPILED_WITH_ERRORS: i32 = 24344;

/// At most this many compile errors are named in the message.
const NAMED_ERRORS: usize = 10;

/// A PL/SQL unit a `CREATE` statement stores, as `user_errors` names it.
#[derive(Debug, PartialEq, Eq)]
struct Unit {
    /// `PROCEDURE`, `FUNCTION`, `PACKAGE`, `PACKAGE BODY`, `TRIGGER`, `TYPE` or `TYPE BODY`.
    kind: String,
    /// The unit's name without its schema; upper case unless it was quoted.
    name: String,
    /// Byte offset of the kind's first word in the statement: Oracle counts the unit's lines
    /// from there.
    kind_at: usize,
}

/// The next word of `sql` from byte `at` on (letters, digits, `_`, `$`, `#`, or a quoted name),
/// skipping blanks: its start, its end and whether it was quoted.
fn next_word(sql: &str, at: usize) -> Option<(usize, usize, bool)> {
    let rest = &sql[at..];
    let start = at + (rest.len() - rest.trim_start().len());
    let bytes = sql.as_bytes();
    if bytes.get(start) == Some(&b'"') {
        let end = sql[start + 1..].find('"')? + start + 2;
        return Some((start, end, true));
    }
    let len = sql[start..]
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '#')))
        .unwrap_or(sql.len() - start);
    (len > 0).then_some((start, start + len, false))
}

/// The unit that `sql` (`CREATE [OR REPLACE] [EDITIONABLE|NONEDITIONABLE] PROCEDURE …`) stores,
/// or `None` for any other statement.
fn created_unit(sql: &str) -> Option<Unit> {
    let word = |at: usize| next_word(sql, at).map(|(s, e, q)| (s, e, q, sql[s..e].to_owned()));
    let (_, mut at, _, first) = word(0)?;
    if !first.eq_ignore_ascii_case("create") {
        return None;
    }
    let (mut start, mut end, _, mut w) = word(at)?;
    while ["or", "replace", "editionable", "noneditionable"]
        .iter()
        .any(|m| w.eq_ignore_ascii_case(m))
    {
        at = end;
        (start, end, _, w) = word(at)?;
    }
    let kind_at = start;
    let mut kind = w.to_ascii_uppercase();
    if !["PROCEDURE", "FUNCTION", "PACKAGE", "TRIGGER", "TYPE"].contains(&kind.as_str()) {
        return None;
    }
    let (_, mut e, mut quoted, mut name) = word(end)?;
    if matches!(kind.as_str(), "PACKAGE" | "TYPE") && !quoted && name.eq_ignore_ascii_case("body") {
        kind.push_str(" BODY");
        (_, e, quoted, name) = word(e)?;
    }
    // `schema.name`: the unit's own name follows the dot.
    if sql[e..].starts_with('.') {
        (_, _, quoted, name) = word(e + 1)?;
    }
    let name = if quoted {
        name.trim_matches('"').to_owned()
    } else {
        name.to_ascii_uppercase()
    };
    Some(Unit {
        kind,
        name,
        kind_at,
    })
}

/// The error of a unit that did not compile: Oracle's warning `message`, then each of `errors`
/// (line and column in the unit, text), at most [`NAMED_ERRORS`]; the position is that of the first
/// one in the statement `sql`, except for a trigger, whose lines Oracle counts from its body.
fn compile_error(
    sql: &str,
    message: &str,
    unit: &Unit,
    errors: &[(usize, usize, String)],
) -> DbError {
    // The compile errors follow Oracle's first line; a help line of the warning comes last.
    let (head, help) = message.split_once('\n').unwrap_or((message, ""));
    let mut text = if head.starts_with("ORA-") {
        head.to_owned()
    } else {
        format!("ORA-{COMPILED_WITH_ERRORS}: {head}")
    };
    for (line, column, error) in errors.iter().take(NAMED_ERRORS) {
        text.push_str(&format!("\nline {line}, column {column}: {error}"));
    }
    if errors.len() > NAMED_ERRORS {
        text.push_str(&format!("\n… {} more", errors.len() - NAMED_ERRORS));
    }
    if !help.trim().is_empty() {
        text.push('\n');
        text.push_str(help.trim_end());
    }
    let position =
        errors
            .first()
            .filter(|_| unit.kind != "TRIGGER")
            .and_then(|&(line, column, _)| {
                let before = &sql[..unit.kind_at];
                let base_line = before.matches('\n').count();
                let base_column = before
                    .rsplit('\n')
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .count();
                let target = base_line + line.checked_sub(1)?;
                let line_start = match target {
                    0 => 0,
                    n => sql.match_indices('\n').nth(n - 1)?.0 + 1,
                };
                let column = if line == 1 {
                    base_column + column
                } else {
                    column
                };
                let line_text = sql[line_start..].split('\n').next().unwrap_or_default();
                (column >= 1 && column <= line_text.chars().count())
                    .then(|| sql[..line_start].chars().count() + column)
            });
    DbError {
        message: text,
        sqlstate: None,
        position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0142 v5 AK-9: NUMBER(p, s) values keep their scale and a zero before the point;
    /// a ROLLBACK of the whole transaction is told apart from one to a savepoint.
    #[test]
    fn numbers_keep_their_scale_and_rollbacks_are_recognised() {
        assert_eq!(number_text("4.5", 2), "4.50");
        assert_eq!(number_text(".05", 2), "0.05");
        assert_eq!(number_text("-.5", 3), "-0.500");
        assert_eq!(number_text("12", 2), "12.00");
        assert_eq!(number_text("1.2345", 2), "1.2345");
        assert_eq!(number_text("1E+40", 2), "1E+40");
        assert!(bare_rollback("ROLLBACK"));
        assert!(bare_rollback("rollback work"));
        assert!(!bare_rollback("rollback to savepoint a"));
        assert!(!bare_rollback("select 1 from dual"));
    }

    /// SPEC-0142 v2 AK-5: the unit a `CREATE` stores and where its compile errors are.
    #[test]
    fn compile_errors_name_their_lines() {
        let unit = |sql: &str| created_unit(sql).map(|u| (u.kind, u.name));
        assert_eq!(
            unit("create or replace procedure pd_broken as begin null; end;"),
            Some(("PROCEDURE".into(), "PD_BROKEN".into()))
        );
        assert_eq!(
            unit("CREATE OR REPLACE EDITIONABLE PACKAGE BODY app.\"Pkg\" AS END;"),
            Some(("PACKAGE BODY".into(), "Pkg".into()))
        );
        assert_eq!(
            unit("create function f return number is begin return 1; end;"),
            Some(("FUNCTION".into(), "F".into()))
        );
        assert_eq!(
            unit("create or replace type body t as end;"),
            Some(("TYPE BODY".into(), "T".into()))
        );
        assert_eq!(
            unit("create or replace trigger trg before insert on t begin null; end;"),
            Some(("TRIGGER".into(), "TRG".into()))
        );
        assert_eq!(unit("create table t (a number)"), None);
        assert_eq!(unit("begin null; end;"), None);

        // Line 3 of the unit is line 3 of the statement; on line 1 the unit starts at `procedure`.
        let sql = "create or replace procedure pd_broken as\nbegin\n  no_such_call;\nend;";
        let broken = created_unit(sql).unwrap();
        let errors = [
            (
                3,
                3,
                "PLS-00201: identifier 'NO_SUCH_CALL' must be declared".to_owned(),
            ),
            (3, 3, "PL/SQL: Statement ignored".to_owned()),
        ];
        let e = compile_error(
            sql,
            "ORA-24344: success with compilation error",
            &broken,
            &errors,
        );
        assert_eq!(
            e.message,
            "ORA-24344: success with compilation error\n\
             line 3, column 3: PLS-00201: identifier 'NO_SUCH_CALL' must be declared\n\
             line 3, column 3: PL/SQL: Statement ignored"
        );
        let at = e.position.unwrap() - 1;
        assert!(
            sql.chars()
                .skip(at)
                .collect::<String>()
                .starts_with("no_such_call")
        );
        let first_line = compile_error(sql, "x\nHelp: h", &broken, &[(1, 11, "PLS-1".into())]);
        assert_eq!(
            first_line.message,
            "ORA-24344: x\nline 1, column 11: PLS-1\nHelp: h"
        );
        let at = first_line.position.unwrap() - 1;
        assert!(
            sql.chars()
                .skip(at)
                .collect::<String>()
                .starts_with("pd_broken")
        );
        // The unit on the line after `create or replace`: its line 1 is the statement's line 2.
        let split = "create or replace\nprocedure p as\nbegin\n  x;\nend;";
        let p = created_unit(split).unwrap();
        let at = compile_error(split, "w", &p, &[(3, 3, "e".into())])
            .position
            .unwrap()
            - 1;
        assert!(split.chars().skip(at).collect::<String>().starts_with("x;"));
        // A trigger's lines count from its body: no position. More than ten errors are cut.
        let trigger = created_unit("create trigger t before insert on x begin y; end;").unwrap();
        let many: Vec<_> = (1..=12).map(|i| (i, 1, format!("E{i}"))).collect();
        let e = compile_error("create trigger t …", "w", &trigger, &many);
        assert_eq!(e.position, None);
        assert!(
            e.message.ends_with("line 10, column 1: E10\n… 2 more"),
            "{}",
            e.message
        );
    }

    #[test]
    fn connect_strings() {
        let mut p = ConnectionProfile::from_url("o", "oracle://scott@db:1522/FREEPDB1").unwrap();
        p.ssl = SslMode::Disable;
        assert_eq!(connect_string(&p), "//db:1522/FREEPDB1");
        p.ssl = SslMode::Require;
        assert_eq!(connect_string(&p), "tcps://db:1522/FREEPDB1");
        let d = ConnectionProfile::from_url("d", "oracle://scott@localhost/XE").unwrap();
        assert_eq!(connect_string(&d), "//localhost:1521/XE");
    }

    /// SPEC-0142 v3 AK-6: `prefer` is TCPS off the loopback and plain on it; `disable` is plain
    /// everywhere.
    #[test]
    fn prefer_off_the_loopback_is_tcps() {
        let at = |host: &str, mode: SslMode| {
            let mut p = ConnectionProfile::from_url("o", "oracle://scott@h:1521/FREEPDB1").unwrap();
            p.host = Some(host.into());
            p.ssl = mode;
            connect_string(&p)
        };
        assert_eq!(
            at("db.example", SslMode::Prefer),
            "tcps://db.example:1521/FREEPDB1"
        );
        assert_eq!(
            at("localhost", SslMode::Prefer),
            "//localhost:1521/FREEPDB1"
        );
        assert_eq!(
            at("127.0.0.1", SslMode::Prefer),
            "//127.0.0.1:1521/FREEPDB1"
        );
        for host in ["db.example", "localhost", "127.0.0.1"] {
            assert_eq!(
                at(host, SslMode::Disable),
                format!("//{host}:1521/FREEPDB1")
            );
        }
    }

    #[test]
    fn read_only_refuses_changes_but_not_queries() {
        assert!(refusal("select 1 from dual", true).is_none());
        assert!(refusal("set transaction read only", true).is_none());
        assert!(refusal("delete from t", true).is_some());
        assert!(refusal("begin null; end;", true).is_some());
        assert!(refusal("delete from t", false).is_none());
    }
}

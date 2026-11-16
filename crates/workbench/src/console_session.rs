//! The SQL console's toolbar state (`SPEC-0093` v3 behaviours 9–13): a console in Manual mode
//! keeps one session with an open transaction until Commit or Rollback; the running statement can
//! be cancelled; the query history of the connection is offered as a list.

use std::sync::{Arc, Mutex};

use idecraft_database::{CancelHandle, ConnectionProfile};

use crate::table_session::TableSession;

/// Where a running console statement puts its cancel handle once it is connected.
pub type CancelSlot = Arc<Mutex<Option<CancelHandle>>>;

/// A console in Manual mode (behaviour 11): its own session, kept while the console is open.
pub struct ConsoleSession {
    /// The worker holding the connection.
    pub worker: TableSession,
    /// Whether a transaction was started on it.
    pub begun: bool,
    /// Statements run inside the open transaction.
    pub applied: usize,
    /// How to cancel what runs on it.
    pub cancel: CancelSlot,
    /// The connection it was opened on; a run on another one leaves it (`SPEC-0093` v7
    /// behaviour 12).
    pub profile_id: String,
}

impl ConsoleSession {
    /// Opens the worker for `profile`; it connects in the background.
    pub fn open(profile: ConnectionProfile) -> Self {
        let profile_id = profile.id.clone();
        let cancel: CancelSlot = Arc::default();
        let worker = TableSession::open(profile);
        let slot = cancel.clone();
        // The cancel handle is taken once the worker is connected.
        drop(worker.run(move |_, session| {
            if let Ok(mut s) = slot.lock() {
                *s = session.cancel_handle();
            }
            Ok(())
        }));
        Self {
            worker,
            begun: false,
            applied: 0,
            cancel,
            profile_id,
        }
    }
}

/// The open history popup (behaviour 13).
#[derive(Debug, Clone, Default)]
pub struct HistoryPopup {
    /// Connection id whose history is shown.
    pub connection: String,
    /// Text the entries must contain.
    pub query: String,
}

/// The label of the transaction button: "Tx: Auto", "Tx: Manual", or with an open transaction
/// "Tx: Manual (3)" (behaviour 9).
pub fn tx_label(manual: bool, applied: usize) -> String {
    match (manual, applied) {
        (false, _) => "Tx: Auto".into(),
        (true, 0) => "Tx: Manual".into(),
        (true, n) => format!("Tx: Manual ({n})"),
    }
}

/// One line of the history list: the statement on one line, cut after 80 characters (behaviour
/// 13).
pub fn history_line(sql: &str) -> String {
    let one: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 80 {
        format!("{}…", one.chars().take(79).collect::<String>())
    } else {
        one
    }
}

/// Seconds since 1970, for history entries.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Sends `handle`'s cancel request on a thread of its own (behaviour 12); the running statement
/// then ends with the database's error.
pub fn cancel_in_background(handle: CancelHandle) -> std::sync::mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
            .and_then(|rt| rt.block_on(handle.cancel()));
        let _ = tx.send(result);
    });
    rx
}

/// Whether statements run from the editor of `path` go into the connection's query history: a
/// console's and a bound project `.sql` file's alike (`SPEC-0093` v4 behaviour 14).
pub fn keeps_history(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
}

/// Whether `sql` ends the transaction it runs in: `COMMIT` (also `COMMIT WORK`, `END` is left
/// alone) or `ROLLBACK`, but not `ROLLBACK TO` a savepoint. Comment lines before it are skipped.
/// A Manual console starts its transaction again after one (`SPEC-0093` v7 behaviour 11).
pub fn ends_transaction(sql: &str) -> bool {
    let code: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ");
    let words: Vec<String> = code
        .split(|c: char| c.is_whitespace() || c == ';')
        .filter(|w| !w.is_empty())
        .take(3)
        .map(str::to_ascii_lowercase)
        .collect();
    match words.first().map(String::as_str) {
        Some("commit") => true,
        // `ROLLBACK TO x` and `ROLLBACK WORK TO x` go back to a savepoint and keep it open.
        Some("rollback") => !words[1..].iter().any(|w| w == "to"),
        _ => false,
    }
}

/// Whether `sql` can change the schema the SQL editors know: DDL (`CREATE`, `ALTER`, `DROP`,
/// `RENAME`, `COMMENT ON`, `TRUNCATE` is left out) and blocks or calls that may run DDL inside
/// (`DO`, `CALL`, `EXEC`, `EXECUTE`, `BEGIN … END` of PL/SQL). Comment lines before it are skipped.
/// After a run with one, the connection's editors read the schema again (`SPEC-0192` v5 behaviour
/// 11a).
pub fn changes_schema(sql: &str) -> bool {
    let code: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ");
    let first = code
        .trim_start()
        .trim_start_matches('(')
        .split(|c: char| c.is_whitespace() || c == ';' || c == '(')
        .find(|w| !w.is_empty())
        .map(str::to_ascii_lowercase);
    matches!(
        first.as_deref(),
        Some(
            "create"
                | "alter"
                | "drop"
                | "rename"
                | "comment"
                | "do"
                | "call"
                | "exec"
                | "execute"
                | "declare"
        )
    ) || (first.as_deref() == Some("begin")
        && code
            .to_ascii_lowercase()
            .split_whitespace()
            .nth(1)
            .is_some_and(|w| w != "transaction" && w != "work" && w != "tran"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0192 v5 AK-9 (OBS-0068 L2): what makes the editors read the schema again.
    #[test]
    fn ddl_and_blocks_change_the_schema() {
        for sql in [
            "create table pd_note (id int)",
            "-- note\nCREATE TABLE t (a int);",
            "alter table t add column b int",
            "drop view v",
            "comment on table t is 'x'",
            "do $$ begin execute 'create table x (a int)'; end $$",
            "call make_tables()",
            "EXEC dbo.make",
            "begin\n  execute immediate 'create table x (a number)';\nend;",
        ] {
            assert!(changes_schema(sql), "{sql}");
        }
        for sql in [
            "select * from t",
            "insert into t values (1)",
            "update t set a = 1",
            "begin",
            "begin transaction",
            "truncate t",
            "-- create table x\nselect 1",
        ] {
            assert!(!changes_schema(sql), "{sql}");
        }
    }

    /// SPEC-0093 v7 AK: what ends a Manual console's transaction.
    #[test]
    fn transaction_ends() {
        for sql in [
            "commit",
            "COMMIT;",
            "commit work",
            "-- done\nrollback",
            "Rollback Work",
        ] {
            assert!(ends_transaction(sql), "{sql}");
        }
        for sql in [
            "rollback to savepoint a",
            "ROLLBACK TO a",
            "rollback work to a",
            "select 1",
            "begin",
            "end",
        ] {
            assert!(!ends_transaction(sql), "{sql}");
        }
    }

    /// SPEC-0093 v3 AK-6: the toolbar's labels.
    #[test]
    fn labels() {
        assert_eq!(tx_label(false, 3), "Tx: Auto");
        assert_eq!(tx_label(true, 0), "Tx: Manual");
        assert_eq!(tx_label(true, 2), "Tx: Manual (2)");
        assert_eq!(history_line("select *\n  from  t"), "select * from t");
        let long = format!("select {}", "x".repeat(100));
        assert_eq!(history_line(&long).chars().count(), 80);
        assert!(history_line(&long).ends_with('…'));
    }

    /// SPEC-0093 v4 AK-11: a bound project file keeps history like a console.
    #[test]
    fn project_sql_files_keep_history() {
        use std::path::Path;
        assert!(keeps_history(Path::new(
            "/w/.idecraft/consoles/inv/console.sql"
        )));
        assert!(keeps_history(Path::new("/w/orders.sql")));
        assert!(keeps_history(Path::new("/w/Report.SQL")));
        assert!(!keeps_history(Path::new("/w/notes.txt")));
    }
}

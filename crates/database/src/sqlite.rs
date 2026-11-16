//! SQLite sessions (`SPEC-0011`, ADR-0011).
//!
//! SQLite is in-process and blocking, so statements run on the blocking pool.
//! A missing database file is refused instead of silently created.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use crate::session::{SessionError, prepare};
use crate::{
    ConnectionProfile, Credentials, DbError, Dialect, OnError, Outcome, ScriptRun, split_statements,
};

/// An open SQLite database file.
pub struct SqliteSession {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSession {
    /// Opens the file of `profile`; the file must exist. A read-only profile opens it read-only
    /// at the SQLite level, and the first page is read at once, so a file that is no database,
    /// is locked or cannot be read fails here with words the user can act on (`SPEC-0011` v2
    /// behaviour 7, `SPEC-0108` v8 behaviour 16).
    pub async fn connect(
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Self, SessionError> {
        prepare(profile, credentials, Dialect::Sqlite)?;
        let path = profile.file_path();
        let open_error = |message: String| SessionError::Connect {
            profile: profile.name.clone(),
            address: path.display().to_string(),
            message,
        };
        if path.is_dir() {
            return Err(open_error("this is a folder, not a database file".into()));
        }
        if !path.is_file() {
            return Err(open_error(
                "file does not exist (SQLite files are not created implicitly)".into(),
            ));
        }
        let read_only = profile.read_only;
        let opened: PathBuf = path.clone();
        let conn = tokio::task::spawn_blocking(move || open_checked(&opened, read_only))
            .await
            .map_err(|e| open_error(e.to_string()))?
            .map_err(open_error)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Interrupts the running statement from another thread (`SPEC-0093` v3 behaviour 12).
    pub fn interrupt_handle(&self) -> Arc<rusqlite::InterruptHandle> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::new(conn.get_interrupt_handle())
    }

    /// Runs `script` statement by statement (SPEC-0007 outcomes).
    pub async fn run_script(&self, script: &str, on_error: OnError) -> ScriptRun {
        let conn = Arc::clone(&self.conn);
        let script = script.to_owned();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let statements = split_statements(&script, Dialect::Sqlite);
            let mut run = ScriptRun::default();
            for (index, statement) in statements.iter().enumerate() {
                let failed = run.push(&script, statement, execute(&conn, &statement.text));
                if failed && on_error == OnError::Stop {
                    run.not_run = statements.len() - index - 1;
                    break;
                }
            }
            run
        })
        .await
        .unwrap_or_else(|e| {
            ScriptRun {
                outcomes: Vec::new(),
                not_run: 0,
            }
            .with_failure(e.to_string())
        })
    }
}

impl ScriptRun {
    fn with_failure(mut self, message: String) -> Self {
        self.outcomes.push(crate::StatementOutcome {
            range: 0..0,
            outcome: Outcome::Error(DbError {
                message,
                sqlstate: None,
                position: None,
            }),
            error_offset: None,
            notices: Vec::new(),
        });
        self
    }
}

/// Opens `path` and reads the schema once. Nothing is ever created: not the file, and with
/// `read_only` no journal either. A database in WAL mode that lies where nothing can be written
/// (a read-only file in a read-only folder, a read-only volume) cannot be opened the usual way,
/// because SQLite wants its `-shm` file; it is then opened as an unchanging file, which is safe
/// only while no `-wal` file holds changes that are not in the database file yet.
fn open_checked(path: &std::path::Path, read_only: bool) -> Result<Connection, String> {
    let mode = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let probe =
        |conn: &Connection| conn.query_row("select count(*) from sqlite_master", [], |_| Ok(()));
    let conn = Connection::open_with_flags(path, mode).map_err(|e| open_message(&e, path))?;
    let first = match probe(&conn) {
        Ok(()) => return Ok(conn),
        Err(e) => e,
    };
    let code = first.sqlite_error_code();
    let unwritable_place = matches!(
        code,
        Some(rusqlite::ErrorCode::ReadOnly | rusqlite::ErrorCode::CannotOpen)
    );
    if unwritable_place && !pending_wal(path) {
        drop(conn);
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI;
        if let Ok(conn) = Connection::open_with_flags(immutable_uri(path), flags)
            && probe(&conn).is_ok()
        {
            return Ok(conn);
        }
    }
    Err(open_message(&first, path))
}

/// Whether a `-wal` file next to `path` holds content.
fn pending_wal(path: &std::path::Path) -> bool {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    std::fs::metadata(PathBuf::from(wal)).is_ok_and(|m| m.len() > 0)
}

/// `path` as a `file:` URI that SQLite opens without locks and without side files.
fn immutable_uri(path: &std::path::Path) -> String {
    let mut uri = String::from("file:");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'%' | b'?' | b'#' => uri.push_str(&format!("%{b:02X}")),
            _ if b.is_ascii() => uri.push(b as char),
            _ => uri.push_str(&format!("%{b:02X}")),
        }
    }
    uri.push_str("?immutable=1");
    uri
}

/// Why a database file could not be opened, in words the user can act on; the result code of
/// SQLite follows in brackets for searching.
fn open_message(error: &rusqlite::Error, path: &std::path::Path) -> String {
    use rusqlite::ErrorCode as C;
    let raw = db_error(error, "", false).message;
    let advice = match error.sqlite_error_code() {
        Some(C::NotADatabase) => {
            "the file is not a SQLite database (or it is encrypted); choose the database file itself"
                .to_owned()
        }
        Some(C::DatabaseBusy | C::DatabaseLocked) => {
            "the database is locked: another program is writing to it; try again when that \
             program has finished or closed the file"
                .to_owned()
        }
        Some(C::DatabaseCorrupt) => {
            "the database file is damaged; restore a copy or let `sqlite3` recover what it can"
                .to_owned()
        }
        Some(C::PermissionDenied | C::CannotOpen) => {
            "the file cannot be opened; check that you may read it and its folder".to_owned()
        }
        Some(C::ReadOnly) if pending_wal(path) => {
            "the database has changes waiting in its `-wal` file, and its folder cannot be \
             written; open it from a folder you may write to"
                .to_owned()
        }
        Some(C::ReadOnly) => {
            "the database or its folder cannot be written; mark the connection read-only or \
             open a copy in a folder you may write to"
                .to_owned()
        }
        _ => return raw,
    };
    format!("{advice} [{raw}]")
}

fn execute(conn: &Connection, sql: &str) -> Outcome {
    let mut statement = match conn.prepare(sql) {
        Ok(statement) => statement,
        Err(e) => return Outcome::Error(to_db_error(&e, sql)),
    };
    let width = statement.column_count();
    if width == 0 {
        return match statement.execute([]) {
            Ok(_) => Outcome::UpdateCount(conn.changes()),
            Err(e) => Outcome::Error(to_db_error(&e, sql)),
        };
    }
    let columns: Vec<String> = statement.column_names().map(str::to_owned).collect();
    let mut rows = Vec::new();
    let mut cursor = match statement.query([]) {
        Ok(cursor) => cursor,
        Err(e) => return Outcome::Error(to_db_error(&e, sql)),
    };
    loop {
        match cursor.next() {
            Ok(Some(row)) => rows.push(
                (0..width)
                    .map(|i| row.get_ref(i).map(text_of).unwrap_or(None))
                    .collect(),
            ),
            Ok(None) => break,
            Err(e) => return Outcome::Error(to_db_error(&e, sql)),
        }
    }
    Outcome::ResultSet { columns, rows }
}

/// Renders a value by storage class (SPEC-0011 behaviour 4).
fn text_of(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        // Debug gives the shortest text that reads back as the same number.
        ValueRef::Real(r) => Some(format!("{r:?}")),
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Some(format!(
            "0x{}",
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        )),
    }
}

fn to_db_error(error: &rusqlite::Error, sql: &str) -> DbError {
    db_error(error, sql, true)
}

/// The error of a statement; with `advice` a lock says what the user can do about it.
fn db_error(error: &rusqlite::Error, sql: &str, advice: bool) -> DbError {
    let code_name = |code: rusqlite::ErrorCode, extended: i32| {
        format!("{} ({code:?}, {extended})", code_label(code))
    };
    match error {
        rusqlite::Error::SqlInputError {
            error, msg, offset, ..
        } => DbError {
            message: format!("{}: {msg}", code_name(error.code, error.extended_code)),
            sqlstate: None,
            position: usize::try_from(*offset)
                .ok()
                .and_then(|byte| sql.get(..byte))
                .map(|before| before.chars().count() + 1),
        },
        rusqlite::Error::SqliteFailure(failure, msg) => DbError {
            message: format!(
                "{}: {}{}",
                code_name(failure.code, failure.extended_code),
                msg.clone().unwrap_or_else(|| failure.to_string()),
                // What the user can do about a lock (SPEC-0011 v2 behaviour 7).
                if advice && failure.code == rusqlite::ErrorCode::DatabaseBusy {
                    " (another program is writing to the database; try again when it has finished)"
                } else {
                    ""
                }
            ),
            sqlstate: None,
            position: None,
        },
        other => DbError {
            message: other.to_string(),
            sqlstate: None,
            position: None,
        },
    }
}

/// The primary result code name as SQLite documents it.
fn code_label(code: rusqlite::ErrorCode) -> &'static str {
    use rusqlite::ErrorCode as C;
    match code {
        C::ReadOnly => "SQLITE_READONLY",
        C::ConstraintViolation => "SQLITE_CONSTRAINT",
        C::DatabaseBusy => "SQLITE_BUSY",
        C::DatabaseLocked => "SQLITE_LOCKED",
        C::CannotOpen => "SQLITE_CANTOPEN",
        C::NotADatabase => "SQLITE_NOTADB",
        C::DatabaseCorrupt => "SQLITE_CORRUPT",
        C::TypeMismatch => "SQLITE_MISMATCH",
        C::PermissionDenied => "SQLITE_PERM",
        C::OperationAborted => "SQLITE_ABORT",
        C::OperationInterrupted => "SQLITE_INTERRUPT",
        C::DiskFull => "SQLITE_FULL",
        C::TooBig => "SQLITE_TOOBIG",
        _ => "SQLITE_ERROR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_render_by_storage_class() {
        assert_eq!(text_of(ValueRef::Null), None);
        assert_eq!(text_of(ValueRef::Integer(-30)).as_deref(), Some("-30"));
        assert_eq!(text_of(ValueRef::Real(0.1)).as_deref(), Some("0.1"));
        assert_eq!(text_of(ValueRef::Real(1e300)).as_deref(), Some("1e300"));
        assert_eq!(
            text_of(ValueRef::Blob(&[0xff, 0x00])).as_deref(),
            Some("0xff00")
        );
    }
}

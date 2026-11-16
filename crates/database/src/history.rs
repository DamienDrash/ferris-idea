//! The query history of a connection (`SPEC-0093` v3 behaviour 13): every statement a console
//! ran, newest first, kept per connection and workspace in the user's settings folder (never in
//! the workspace, which may be shared). Statements that carry a password are never written down
//! (IMP-0372).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Entries kept per connection.
pub const HISTORY_LIMIT: usize = 500;

/// One statement that was run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// The statement's text.
    pub sql: String,
    /// When it ran, seconds since 1970 (UTC).
    pub at: u64,
    /// Whether it ran without an error.
    pub ok: bool,
}

/// The statements of one connection, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryHistory {
    entries: Vec<HistoryEntry>,
}

/// Whether `sql` carries a secret and must not be kept (IMP-0372): `CREATE`/`ALTER USER` or
/// `ROLE` with `PASSWORD` or `IDENTIFIED`, `SET PASSWORD`, `GRANT … IDENTIFIED BY`, PostgreSQL's
/// `\password`, and anything mentioning `password =`/`pwd=` in a connection string.
pub fn keeps_secret(sql: &str) -> bool {
    let words: Vec<String> = sql
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let has = |w: &str| words.iter().any(|x| x == w);
    let lower = sql.to_lowercase();
    (has("password") || has("identified") || has("pwd") || has("passwd"))
        && (has("user")
            || has("role")
            || has("login")
            || has("grant")
            || words.first().is_some_and(|w| w == "set")
            || lower.contains("password=")
            || lower.contains("pwd="))
        || lower.trim_start().starts_with("\\password")
}

impl QueryHistory {
    /// The entries, newest first.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Notes that `sql` ran at `at`; returns whether it was kept. Empty text and statements with
    /// a secret are left out; running the newest statement again only moves its time; the
    /// oldest entries go beyond [`HISTORY_LIMIT`].
    pub fn record(&mut self, sql: &str, ok: bool, at: u64) -> bool {
        let sql = sql.trim();
        if sql.is_empty() || keeps_secret(sql) {
            return false;
        }
        if let Some(first) = self.entries.first_mut()
            && first.sql == sql
        {
            first.at = at;
            first.ok = ok;
            return true;
        }
        self.entries.insert(
            0,
            HistoryEntry {
                sql: sql.to_owned(),
                at,
                ok,
            },
        );
        self.entries.truncate(HISTORY_LIMIT);
        true
    }

    /// The entries whose text contains `query`, ignoring case.
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let q = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| q.is_empty() || e.sql.to_lowercase().contains(&q))
            .collect()
    }

    /// Reads the history in `file`; a missing or damaged file gives what could be read.
    pub fn load(file: &Path) -> Self {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let mut entries: Vec<HistoryEntry> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|e: &HistoryEntry| !keeps_secret(&e.sql))
            .collect();
        entries.truncate(HISTORY_LIMIT);
        Self { entries }
    }

    /// Writes the history to `file`, one JSON object per line, readable by the user only.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        let tmp = file.with_extension(format!("tmp-{}", std::process::id()));
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut out = options.open(&tmp)?;
            for e in &self.entries {
                let line = serde_json::to_string(e).map_err(std::io::Error::other)?;
                writeln!(out, "{line}")?;
            }
        }
        std::fs::rename(&tmp, file)
    }
}

/// The history file of connection `id` of the workspace at `root`: in the user's settings folder
/// under `sql-history/`, named after the connection and a hash of the workspace path.
pub fn history_file(root: &Path, id: &str) -> Option<PathBuf> {
    use sha2::Digest;
    let dir = idecraft_core::user_config_dir()?;
    let digest = sha2::Sha256::digest(root.to_string_lossy().as_bytes());
    let hash: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(dir.join("sql-history").join(format!("{safe}-{hash}.jsonl")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0093 v3 AK-8: newest first, repeats folded, limit, search.
    #[test]
    fn records_newest_first() {
        let mut h = QueryHistory::default();
        assert!(h.record("select 1", true, 1));
        assert!(h.record("select 2", false, 2));
        assert!(h.record(" select 2 ", true, 3));
        assert!(!h.record("  ", true, 4));
        let sql: Vec<_> = h.entries().iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(sql, ["select 2", "select 1"]);
        assert_eq!((h.entries()[0].at, h.entries()[0].ok), (3, true));
        assert_eq!(h.search("1").len(), 1);
        for i in 0..HISTORY_LIMIT + 5 {
            h.record(&format!("select {i} + 0"), true, 10);
        }
        assert_eq!(h.entries().len(), HISTORY_LIMIT);
    }

    /// IMP-0372: statements with a password are never kept, in memory or on disk.
    #[test]
    fn secrets_are_never_kept() {
        for sql in [
            "CREATE USER app IDENTIFIED BY 'x'",
            "alter user 'idecraft'@'localhost' identified by 'x'",
            "ALTER ROLE app PASSWORD 'x'",
            "create role r login password 'x'",
            "SET PASSWORD FOR app = 'x'",
            "grant all on *.* to u identified by 'x'",
            "select dblink_connect('host=h user=u password=x')",
            "\\password app",
        ] {
            assert!(keeps_secret(sql), "{sql}");
            assert!(!QueryHistory::default().record(sql, true, 1), "{sql}");
        }
        for sql in [
            "select * from users",
            "select password_changed_at from accounts",
            "update account set note = 'role'",
        ] {
            assert!(!keeps_secret(sql), "{sql}");
        }
    }

    #[test]
    fn saves_privately_and_loads() {
        let dir = std::env::temp_dir().join(format!("idecraft-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("sql-history").join("c.jsonl");
        let mut h = QueryHistory::default();
        h.record("select 1", true, 1);
        h.record("select 'two'\nfrom t", false, 2);
        h.save(&file).unwrap();
        assert_eq!(QueryHistory::load(&file), h);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // A secret written by hand into the file is dropped on reading.
        std::fs::write(
            &file,
            "{\"sql\":\"alter user u identified by 'x'\",\"at\":1,\"ok\":true}\nnot json\n",
        )
        .unwrap();
        assert!(QueryHistory::load(&file).entries().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let a = history_file(Path::new("/w/a"), "pg/1").unwrap();
        let b = history_file(Path::new("/w/b"), "pg/1").unwrap();
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("sql-history/pg_1-"));
    }
}

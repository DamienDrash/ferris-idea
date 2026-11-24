//! The audit log of the vault (`SPEC-0353` behaviour 20): one JSON line per use, refusal,
//! denial, store, generation and removal, never a value. Each line carries the previous line's
//! MAC and its own, an HMAC-SHA-256 under a key derived from the vault key, so a changed,
//! removed or reordered line breaks the chain.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Keys, VaultError, crypto};

/// One line of the audit log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// 1 for the first line, then one more per line (set when appended).
    pub seq: u64,
    /// Seconds since the Unix epoch.
    pub time: u64,
    /// The resolved workspace root the record belongs to.
    pub project: String,
    /// The entry's handle.
    pub handle: String,
    /// The entry's name.
    pub entry: String,
    /// The agent identity (session and reported client name).
    pub agent: String,
    /// The MCP tool.
    pub tool: String,
    /// The injection form, or empty for a store, generation or removal.
    pub form: String,
    /// `host:port`, `process`, or empty.
    pub destination: String,
    /// `used`, `denied`, `refused`, `expired`, `unanswered`, `stored`, `generated`, `removed`.
    pub result: String,
    /// What the use rested on: `window: once`, `session grant until <t>`, `own entry`, or empty.
    pub approval: String,
    /// The previous line's MAC (empty for the first line; set when appended).
    pub prev: String,
    /// This line's MAC (set when appended).
    pub mac: String,
}

/// The audit log as read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AuditLog {
    /// Every readable line, oldest first.
    pub records: Vec<AuditRecord>,
    /// Whether the chain is intact.
    pub intact: bool,
    /// Where the chain breaks, if it does.
    pub problem: Option<String>,
}

fn unsigned_bytes(r: &AuditRecord) -> Vec<u8> {
    let mut u = r.clone();
    u.mac = String::new();
    serde_json::to_vec(&u).unwrap_or_default()
}

/// Reads and verifies the log at `path` (no file: empty and intact).
pub(crate) fn read(path: &Path, keys: &Keys) -> Result<AuditLog, VaultError> {
    let key = crypto::audit_key(keys);
    let text = crate::read_private(path)?.unwrap_or_default();
    let mut log = AuditLog {
        intact: true,
        ..AuditLog::default()
    };
    let mut prev = String::new();
    for (i, line) in text.lines().enumerate() {
        let n = i as u64 + 1;
        let record: AuditRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => {
                if log.intact {
                    log.intact = false;
                    log.problem = Some(format!("line {n} cannot be read"));
                }
                continue;
            }
        };
        if log.intact {
            let chained = record.seq == n && record.prev == prev;
            let authentic = crypto::mac_ok(
                key.as_ref(),
                &[record.prev.as_bytes(), &unsigned_bytes(&record)],
                &record.mac,
            );
            if !(chained && authentic) {
                log.intact = false;
                log.problem = Some(format!(
                    "line {n} was changed, removed or moved (the chain breaks there)"
                ));
            }
        }
        prev = record.mac.clone();
        log.records.push(record);
    }
    Ok(log)
}

/// Appends `record` with its sequence number and MACs. The caller holds the vault's lock.
pub(crate) fn append(path: &Path, keys: &Keys, mut record: AuditRecord) -> Result<(), VaultError> {
    let text = crate::read_private(path)?.unwrap_or_default();
    let last = text
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<AuditRecord>(l).ok());
    record.seq = text.lines().count() as u64 + 1;
    record.prev = last.map(|r| r.mac).unwrap_or_default();
    record.mac = String::new();
    let key = crypto::audit_key(keys);
    record.mac = crypto::mac_hex(
        key.as_ref(),
        &[record.prev.as_bytes(), &unsigned_bytes(&record)],
    );
    let mut line = serde_json::to_string(&record).map_err(|e| VaultError::Io(e.to_string()))?;
    line.push('\n');
    let mut file = crate::append_private(path)?;
    file.write_all(line.as_bytes()).map_err(crate::io)?;
    file.sync_all().map_err(crate::io)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{MemoryKeyStore, Vault};

    fn record(result: &str) -> AuditRecord {
        AuditRecord {
            time: 7,
            handle: "vlt_aaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            entry: "shop-db".into(),
            agent: "client (session s1)".into(),
            tool: "run_command".into(),
            form: "process-env".into(),
            destination: "process".into(),
            result: result.into(),
            approval: "window: once".into(),
            ..AuditRecord::default()
        }
    }

    /// SPEC-0353 AK-3: the chain verifies, and a changed or removed line breaks it; no value is
    /// in the log.
    #[test]
    fn the_chain_finds_changed_and_removed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::new(
            dir.path().join("vault"),
            Arc::new(MemoryKeyStore::default()),
        );
        let scope = crate::Scope {
            projects: vec!["/w".into()],
            uses: vec![crate::Use::ProcessEnv],
            hosts: vec![],
        };
        let new = crate::NewEntry {
            name: "e".into(),
            kind: crate::Kind::Token,
            scope,
            created_by: crate::Agent {
                session: "s1".into(),
                client: "c".into(),
            },
            expires_in_days: None,
        };
        let value = "dummy-audit-0123456789";
        v.put(new, value, 1).unwrap();
        for r in ["used", "denied", "refused"] {
            v.record(record(r)).unwrap();
        }
        let log = v.audit().unwrap();
        assert!(log.intact, "{log:?}");
        assert_eq!(log.records.len(), 3);
        assert_eq!(log.records[2].seq, 3);
        assert_eq!(log.records[1].prev, log.records[0].mac);

        let path = dir.path().join("vault").join(crate::AUDIT_NAME);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(value));
        let changed = text.replacen("\"denied\"", "\"used\"", 1);
        std::fs::write(&path, &changed).unwrap();
        let log = v.audit().unwrap();
        assert!(!log.intact);
        assert!(log.problem.unwrap().contains("line 2"));

        let removed: Vec<&str> = text
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| l)
            .collect();
        std::fs::write(&path, removed.join("\n") + "\n").unwrap();
        assert!(!v.audit().unwrap().intact);
    }
}

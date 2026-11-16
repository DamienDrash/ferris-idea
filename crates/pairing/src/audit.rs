//! Reading the audit log (`SPEC-0324` behaviour 10): the newest lines of `remote/audit.jsonl`,
//! for all devices or one. The log holds no content, so a line can be shown as it is.

use std::path::Path;

use serde_json::Value;

/// How many lines a reader gets at most.
pub const MOST: usize = 2000;

/// The newest `limit` lines of the log in `state`, oldest first, for the device with this
/// fingerprint when one is named. A missing log is empty.
pub fn read_log(state: &Path, fingerprint: Option<&str>, limit: usize) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(state.join("remote/audit.jsonl")) else {
        return Vec::new();
    };
    let wanted = fingerprint.map(|fp| format!("device:{fp}"));
    let mut lines: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| match &wanted {
            Some(prefix) => v["peer"].as_str().is_some_and(|p| p.starts_with(prefix)),
            None => true,
        })
        .collect();
    let keep = limit.min(MOST);
    if lines.len() > keep {
        lines.drain(..lines.len() - keep);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_lines_for_one_device() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("remote")).unwrap();
        let mut body = String::new();
        for i in 0..5 {
            body.push_str(&format!(
                "{{\"peer\":\"device:aa:phone\",\"method\":\"m{i}\"}}\n"
            ));
        }
        body.push_str("{\"peer\":\"local:me\",\"method\":\"x\"}\nnot json\n");
        std::fs::write(dir.path().join("remote/audit.jsonl"), body).unwrap();
        let all = read_log(dir.path(), None, 100);
        assert_eq!(all.len(), 6);
        let one = read_log(dir.path(), Some("aa"), 2);
        assert_eq!(one.len(), 2);
        assert_eq!(one[1]["method"], "m4");
        assert!(read_log(dir.path(), Some("bb"), 10).is_empty());
    }
}

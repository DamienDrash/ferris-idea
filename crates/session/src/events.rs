//! The numbered event stream of a session (`ADR-0026` §8): every notification of the backend
//! carries `seq`, the last ones stay in a ring buffer until the window acknowledges them, and a
//! resumed window gets what it missed — or `session.resync` when that is gone.

use std::collections::VecDeque;

/// At least this many notifications are kept …
pub const RING_ENTRIES: usize = 10_000;
/// … or this many bytes, whichever is reached first.
pub const RING_BYTES: usize = 8 << 20;

/// The notifications of one session.
#[derive(Debug)]
pub struct EventLog {
    next: u64,
    entries: VecDeque<(u64, String)>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::with_limits(RING_ENTRIES, RING_BYTES)
    }
}

/// The window missed notifications that are no longer kept; it must read the state again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resync;

impl EventLog {
    /// A log keeping at most `entries` notifications or `bytes` bytes (tests use small values).
    pub fn with_limits(entries: usize, bytes: usize) -> Self {
        Self {
            next: 1,
            entries: VecDeque::new(),
            bytes: 0,
            max_entries: entries,
            max_bytes: bytes,
        }
    }

    /// Numbers the notification `method` with `params` (an object), keeps it and returns its
    /// `seq` and JSON text.
    pub fn push(&mut self, method: &str, mut params: serde_json::Value) -> (u64, String) {
        let seq = self.next;
        self.next += 1;
        if let Some(o) = params.as_object_mut() {
            o.insert("seq".into(), seq.into());
        }
        let text = crate::rpc::Message::notification(method, params).to_text();
        self.bytes += text.len();
        self.entries.push_back((seq, text.clone()));
        while self.entries.len() > self.max_entries || self.bytes > self.max_bytes {
            match self.entries.pop_front() {
                Some((_, t)) => self.bytes -= t.len(),
                None => break,
            }
        }
        (seq, text)
    }

    /// The window handled everything up to `seq`; older entries can go.
    pub fn ack(&mut self, seq: u64) {
        while self.entries.front().is_some_and(|(s, _)| *s <= seq) {
            if let Some((_, t)) = self.entries.pop_front() {
                self.bytes -= t.len();
            }
        }
    }

    /// The `seq` of the last notification (0 before the first).
    pub fn last(&self) -> u64 {
        self.next - 1
    }

    /// The notifications after `last_seq`, or [`Resync`] when some of them were dropped.
    pub fn since(&self, last_seq: u64) -> Result<Vec<String>, Resync> {
        if last_seq > self.last() {
            return Err(Resync);
        }
        let first_kept = self.entries.front().map_or(self.next, |(s, _)| *s);
        if last_seq + 1 < first_kept {
            return Err(Resync);
        }
        Ok(self
            .entries
            .iter()
            .filter(|(s, _)| *s > last_seq)
            .map(|(_, t)| t.clone())
            .collect())
    }

    /// Kept notifications (tests).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbered_replayed_acknowledged_and_resynced() {
        let mut log = EventLog::with_limits(3, 1 << 20);
        let (s1, t1) = log.push("fs.changed", json!({"events": []}));
        assert_eq!(s1, 1);
        assert!(t1.contains(r#""seq":1"#), "{t1}");
        log.push("doc.changed", json!({}));
        assert_eq!(log.since(0).unwrap().len(), 2);
        assert_eq!(log.since(1).unwrap().len(), 1);
        assert!(log.since(2).unwrap().is_empty());
        // A seq the backend never sent is a resync (another backend).
        assert_eq!(log.since(9), Err(Resync));
        // Acknowledged entries go; resuming after them still works.
        log.ack(1);
        assert_eq!(log.len(), 1);
        assert_eq!(log.since(1).unwrap().len(), 1);
        // Overflow drops unacknowledged entries: a window behind them must resync.
        for _ in 0..4 {
            log.push("fs.changed", json!({}));
        }
        assert_eq!(log.len(), 3);
        assert_eq!(log.since(1), Err(Resync));
        assert_eq!(log.since(3).unwrap().len(), 3);
        // The byte limit counts too.
        let mut small = EventLog::with_limits(100, 120);
        for _ in 0..5 {
            small.push("fs.changed", json!({"x": "0123456789"}));
        }
        assert!(small.len() < 5);
        assert!(!small.is_empty());
    }
}

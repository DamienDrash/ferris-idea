//! The engine's event stream (`SPEC-0119` v5 behaviour 9, gap D1 of OBS-0116): containers and
//! images created, started, stopped or removed outside Ferris IDEA reach the window without F5. A
//! watcher follows `/events` in its own thread and connects again with a growing pause after the
//! engine went away.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::DockerHost;
use crate::stream::{StopFlag, read_lines};

/// One event of the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DockerEvent {
    /// `container`, `image`, `network`, `volume`.
    pub kind: String,
    /// `create`, `start`, `die`, `destroy`, `pull`, `delete`, …
    pub action: String,
    /// Id of the object (an image's reference for some image events).
    pub id: String,
    /// The `name` attribute (a container's name, an image's reference), if any.
    pub name: Option<String>,
    /// When it happened, nanoseconds since 1970.
    pub time: i128,
}

impl DockerEvent {
    /// Reads one line of the stream.
    pub fn parse(line: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(line).ok()?;
        let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_owned);
        let time = v
            .get("timeNano")
            .and_then(Value::as_i64)
            .map(i128::from)
            .or_else(|| {
                v.get("time")
                    .and_then(Value::as_i64)
                    .map(|t| i128::from(t) * 1_000_000_000)
            })
            .unwrap_or(0);
        Some(Self {
            kind: s("/Type")?,
            action: s("/Action")?,
            id: s("/Actor/ID").unwrap_or_default(),
            name: s("/Actor/Attributes/name"),
            time,
        })
    }

    /// Whether the event changes what the lists show (an exec or a health probe does not).
    pub fn changes_lists(&self) -> bool {
        let action = self.action.split(':').next().unwrap_or("");
        !(action.starts_with("exec_")
            || matches!(
                action,
                "health_status"
                    | "top"
                    | "attach"
                    | "detach"
                    | "resize"
                    | "commit"
                    | "copy"
                    | "export"
                    | "archive-path"
                    | "extract-to-dir"
            ))
    }
}

/// What a watcher reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// The event stream is (again) connected; whatever happened while it was not is unknown.
    Connected,
    /// An event.
    Event(DockerEvent),
    /// The stream is gone; the watcher connects again after `retry_in`.
    Lost {
        /// Why.
        reason: String,
        /// Pause before the next try.
        retry_in: Duration,
    },
}

/// Pause before reconnect try `attempt` (from 0): 0.5 s, doubled each time, at most 10 s.
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1u64 << attempt.min(5)))
        .min(Duration::from_secs(10))
}

/// Path of the event stream: containers, images and networks, from now on.
const EVENTS: &str =
    "/events?filters=%7B%22type%22%3A%5B%22container%22%2C%22image%22%2C%22network%22%5D%7D";

impl DockerHost {
    /// Follows the event stream until it ends, fails or `stop` is set; `connected` is called once
    /// the engine answered, `on` for every event.
    pub fn follow_events(
        &self,
        stop: &StopFlag,
        connected: impl FnOnce(),
        mut on: impl FnMut(DockerEvent),
    ) -> Result<(), crate::DockerError> {
        let stream = self.open_stream("GET", EVENTS)?;
        connected();
        read_lines(stream, stop, |line| {
            if let Some(e) = DockerEvent::parse(line) {
                on(e);
            }
        })
        .map_err(|e| crate::DockerError::Connect(self.to_string(), e.to_string()))
    }
}

/// Follows an engine's events in a background thread until dropped.
pub struct EventWatch {
    stop: StopFlag,
}

impl EventWatch {
    /// Starts watching `host`; `on` runs in the watcher's thread.
    pub fn start(host: DockerHost, mut on: impl FnMut(WatchEvent) + Send + 'static) -> Self {
        let stop = StopFlag::new();
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("docker-events".into())
            .spawn(move || {
                let mut attempt = 0u32;
                let on = std::cell::RefCell::new(&mut on);
                while !flag.is_stopped() {
                    let mut was_connected = false;
                    let result = host.follow_events(
                        &flag,
                        || {
                            was_connected = true;
                            (on.borrow_mut())(WatchEvent::Connected);
                        },
                        |e| (on.borrow_mut())(WatchEvent::Event(e)),
                    );
                    if flag.is_stopped() {
                        break;
                    }
                    if was_connected {
                        attempt = 0;
                    }
                    let reason = match result {
                        Ok(()) => "the engine closed the event stream".to_owned(),
                        Err(e) => e.to_string(),
                    };
                    let retry_in = backoff(attempt);
                    attempt = attempt.saturating_add(1);
                    (on.borrow_mut())(WatchEvent::Lost { reason, retry_in });
                    if flag.wait(retry_in) {
                        break;
                    }
                }
            })
            .ok();
        Self { stop }
    }

    /// Stops watching (also when dropped).
    pub fn stop(&self) {
        self.stop.stop();
    }
}

impl Drop for EventWatch {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_parse_and_matter() {
        let e = DockerEvent::parse(
            r#"{"status":"start","id":"abc","Type":"container","Action":"start","Actor":{"ID":"abc","Attributes":{"image":"nginx","name":"web"}},"scope":"local","time":1700000000,"timeNano":1700000000123456789}"#,
        )
        .unwrap();
        assert_eq!(e.kind, "container");
        assert_eq!(e.action, "start");
        assert_eq!(e.name.as_deref(), Some("web"));
        assert_eq!(e.time, 1_700_000_000_123_456_789);
        assert!(e.changes_lists());
        let exec = DockerEvent::parse(
            r#"{"Type":"container","Action":"exec_start: sh -c ls","Actor":{"ID":"abc"},"time":1}"#,
        )
        .unwrap();
        assert!(!exec.changes_lists());
        assert_eq!(exec.time, 1_000_000_000);
        let health = DockerEvent::parse(
            r#"{"Type":"container","Action":"health_status: healthy","Actor":{"ID":"a"}}"#,
        )
        .unwrap();
        assert!(!health.changes_lists());
        assert!(DockerEvent::parse("not json").is_none());
    }

    #[test]
    fn reconnect_pauses_grow_and_stop_at_ten_seconds() {
        let ms: Vec<u128> = (0..8).map(|a| backoff(a).as_millis()).collect();
        assert_eq!(ms, [500, 1000, 2000, 4000, 8000, 10000, 10000, 10000]);
        assert_eq!(backoff(u32::MAX), Duration::from_secs(10));
    }
}

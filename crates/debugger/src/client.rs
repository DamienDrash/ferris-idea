//! A debug adapter process and the request/response/event bookkeeping (`SPEC-0039` behaviour 1).

use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::protocol::{read_message, write_message};
use crate::{DapError, PathMap};

/// Limits the adapter's exception breakpoints to types outside the protocol (java-debug reads them
/// from its debug settings, set through jdtls; `SPEC-0146` v2, D30).
pub type ExceptionTypes = Box<dyn FnMut(&[String]) -> Result<(), String> + Send>;

/// What the program hosting an adapter does for it outside the protocol (jdtls for java-debug):
/// debug settings and the build of changed sources for hot swap (`SPEC-0091` v12 behaviours 34,
/// 37).
pub trait AdapterHost: Send {
    /// Applies debug settings (java-debug: `vscode.java.updateDebugSettings`).
    fn update_settings(&mut self, settings: &Value) -> Result<(), String>;
    /// Builds `files` again after they changed on disk.
    fn sources_changed(&mut self, files: &[std::path::PathBuf]) -> Result<(), String>;
}

/// Settings of an adapter host as a function (see [`FnHost`]).
pub type HostSettings = Box<dyn FnMut(&Value) -> Result<(), String> + Send>;
/// Changed sources of an adapter host as a function (see [`FnHost`]).
pub type HostSources = Box<dyn FnMut(&[std::path::PathBuf]) -> Result<(), String> + Send>;

/// Opens one more connection to an adapter serving several (js-debug's child sessions).
pub type DapConnect = std::sync::Arc<dyn Fn() -> Result<DapClient, DapError> + Send + Sync>;

/// An [`AdapterHost`] made of two functions.
pub struct FnHost {
    /// Applies settings.
    pub settings: HostSettings,
    /// Builds changed sources.
    pub sources: HostSources,
}

impl AdapterHost for FnHost {
    fn update_settings(&mut self, settings: &Value) -> Result<(), String> {
        (self.settings)(settings)
    }

    fn sources_changed(&mut self, files: &[std::path::PathBuf]) -> Result<(), String> {
        (self.sources)(files)
    }
}

/// A running debug adapter.
pub struct DapClient {
    /// The adapter process, when it was started here.
    child: Option<Child>,
    stdin: Box<dyn Write + Send>,
    /// Something to keep alive with the connection (the language server hosting the adapter).
    _keep: Option<Box<dyn Send>>,
    /// Source paths on the wire of an adapter on another machine (`SPEC-0322` behaviour 14).
    paths: Option<PathMap>,
    /// How exception types reach the adapter when the protocol cannot carry them.
    exception_types: Option<ExceptionTypes>,
    /// Settings and builds outside the protocol.
    host: Option<Box<dyn AdapterHost>>,
    incoming: Receiver<Value>,
    seq: u64,
    events: VecDeque<Value>,
    responses: Vec<Value>,
    /// Arguments of `startDebugging` reverse requests, answered with success (js-debug's child
    /// sessions, `SPEC-0039` v5 behaviour 10).
    starts: VecDeque<Value>,
}

impl DapClient {
    /// Starts the adapter `command` (program and arguments).
    pub fn start(command: &[String]) -> Result<Self, DapError> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| DapError::Protocol("empty adapter command".into()))?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Its own process group, so ending it also ends what a start script launched (the JVM of
        // kotlin-debug-adapter).
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|e| DapError::Start(format!("{program}: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| DapError::io("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DapError::io("no stdout"))?;
        Ok(Self::over(Some(child), Box::new(stdin), stdout, None, None))
    }

    /// Connects to an adapter listening on `address` (e.g. `127.0.0.1:5005`, java-debug in jdtls);
    /// The same client with a way to limit exception breakpoints to types (java-debug).
    pub fn with_exception_types(mut self, set: ExceptionTypes) -> Self {
        self.exception_types = Some(set);
        self
    }

    /// The same client with the program hosting the adapter (`SPEC-0091` v12).
    pub fn with_host(mut self, host: Box<dyn AdapterHost>) -> Self {
        self.host = Some(host);
        self
    }

    /// Hands `settings` to the host; `None` without one.
    pub(crate) fn update_settings(&mut self, settings: &Value) -> Option<Result<(), String>> {
        self.host.as_mut().map(|h| h.update_settings(settings))
    }

    /// Tells the host that `files` changed; `None` without one.
    pub(crate) fn sources_changed(
        &mut self,
        files: &[std::path::PathBuf],
    ) -> Option<Result<(), String>> {
        self.host.as_mut().map(|h| h.sources_changed(files))
    }

    /// Hands `types` to the hook of [`DapClient::with_exception_types`]; `None` without one.
    pub(crate) fn set_exception_types(&mut self, types: &[String]) -> Option<Result<(), String>> {
        self.exception_types.as_mut().map(|set| set(types))
    }

    /// `keep` lives as long as the connection.
    pub fn connect(address: &str, keep: Option<Box<dyn Send>>) -> Result<Self, DapError> {
        #[cfg(unix)]
        if let Some(path) = address.strip_prefix("unix:") {
            let stream = std::os::unix::net::UnixStream::connect(path)
                .map_err(|e| DapError::Start(format!("{address}: {e}")))?;
            let reader = stream
                .try_clone()
                .map_err(|e| DapError::io(e.to_string()))?;
            return Ok(Self::over(None, Box::new(stream), reader, keep, None));
        }
        let stream = std::net::TcpStream::connect(address)
            .map_err(|e| DapError::Start(format!("{address}: {e}")))?;
        let reader = stream
            .try_clone()
            .map_err(|e| DapError::io(e.to_string()))?;
        Ok(Self::over(None, Box::new(stream), reader, keep, None))
    }

    /// A client on any byte link to an adapter (`SPEC-0322`: a session stream to a host); `paths`
    /// rewrites source paths on the way.
    pub fn over_link(
        writer: Box<dyn Write + Send>,
        reader: impl Read + Send + 'static,
        keep: Option<Box<dyn Send>>,
        paths: Option<PathMap>,
    ) -> Self {
        Self::over(None, writer, reader, keep, paths)
    }

    /// `keep` lives as long as the connection, in addition to what it keeps already.
    pub fn keep_also(&mut self, keep: Box<dyn Send>) {
        let before = self._keep.take();
        self._keep = Some(Box::new((before, keep)));
    }

    fn over(
        child: Option<Child>,
        writer: Box<dyn Write + Send>,
        reader: impl Read + Send + 'static,
        keep: Option<Box<dyn Send>>,
        paths: Option<PathMap>,
    ) -> Self {
        let (tx, incoming) = channel();
        let map = paths.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(mut message)) = read_message(&mut reader) {
                if let Some(map) = &map {
                    map.to_local(&mut message);
                }
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin: writer,
            _keep: keep,
            paths,
            exception_types: None,
            host: None,
            incoming,
            seq: 0,
            events: VecDeque::new(),
            responses: Vec::new(),
            starts: VecDeque::new(),
        }
    }

    /// Sends a request and returns its sequence number.
    pub fn send(&mut self, command: &str, arguments: Value) -> Result<u64, DapError> {
        self.seq += 1;
        let mut arguments = arguments;
        if let Some(map) = &self.paths {
            map.to_host(&mut arguments);
        }
        let message = json!({
            "seq": self.seq, "type": "request", "command": command, "arguments": arguments,
        });
        write_message(&mut self.stdin, &message)?;
        Ok(self.seq)
    }

    /// Takes the next message, sorting events and responses; answers reverse requests.
    /// Drops the queued events called `name` (stale ones from before a resume).
    pub fn discard_events(&mut self, name: &str) {
        self.events.retain(|e| e["event"].as_str() != Some(name));
    }

    fn pump(&mut self, deadline: Instant) -> Result<(), DapError> {
        let left = deadline.saturating_duration_since(Instant::now());
        let message = match self.incoming.recv_timeout(left) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => return Err(DapError::Timeout),
            Err(RecvTimeoutError::Disconnected) => return Err(DapError::Closed),
        };
        match message["type"].as_str() {
            Some("event") => self.events.push_back(message),
            Some("response") => self.responses.push(message),
            Some("request") => {
                self.seq += 1;
                let start = message["command"] == "startDebugging";
                let answer = if start {
                    json!({
                        "seq": self.seq, "type": "response",
                        "request_seq": message["seq"], "command": message["command"],
                        "success": true,
                    })
                } else {
                    json!({
                        "seq": self.seq, "type": "response",
                        "request_seq": message["seq"], "command": message["command"],
                        "success": false, "message": "not supported by Ferris IDEA",
                    })
                };
                write_message(&mut self.stdin, &answer)?;
                if start {
                    self.starts.push_back(message["arguments"].clone());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Waits for the response to request `seq`; returns its body or the adapter's error.
    pub fn wait_response(&mut self, seq: u64, timeout: Duration) -> Result<Value, DapError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(i) = self
                .responses
                .iter()
                .position(|r| r["request_seq"].as_u64() == Some(seq))
            {
                let response = self.responses.remove(i);
                if response["success"].as_bool() == Some(true) {
                    return Ok(response.get("body").cloned().unwrap_or(Value::Null));
                }
                let message = response["body"]["error"]["format"]
                    .as_str()
                    .or(response["message"].as_str())
                    .unwrap_or("request failed");
                return Err(DapError::Failed {
                    command: response["command"].as_str().unwrap_or_default().to_owned(),
                    message: message.to_owned(),
                });
            }
            self.pump(deadline)?;
        }
    }

    /// Sends a request and waits for its response body.
    pub fn request(
        &mut self,
        command: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<Value, DapError> {
        let seq = self.send(command, arguments)?;
        self.wait_response(seq, timeout)
    }

    /// Waits for the first queued or arriving event named in `names` and removes it.
    pub fn wait_event(&mut self, names: &[&str], timeout: Duration) -> Result<Value, DapError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(i) = self
                .events
                .iter()
                .position(|e| names.contains(&e["event"].as_str().unwrap_or_default()))
            {
                return Ok(self.events.remove(i).unwrap_or_default());
            }
            self.pump(deadline)?;
        }
    }

    /// Like [`DapClient::wait_event`], but ends with the adapter's error as soon as request `seq`
    /// is answered with a failure: a `launch` that cannot start the program (Delve: the build
    /// failed) never sends `initialized`. What the adapter wrote on the error output until then
    /// is part of the message.
    pub fn wait_event_unless_failed(
        &mut self,
        names: &[&str],
        seq: u64,
        timeout: Duration,
    ) -> Result<Value, DapError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(i) = self
                .events
                .iter()
                .position(|e| names.contains(&e["event"].as_str().unwrap_or_default()))
            {
                return Ok(self.events.remove(i).unwrap_or_default());
            }
            let failed = self.responses.iter().any(|r| {
                r["request_seq"].as_u64() == Some(seq) && r["success"].as_bool() != Some(true)
            });
            if failed {
                let said: String = self
                    .events
                    .iter()
                    .filter(|e| e["event"] == "output" && e["body"]["category"] == "stderr")
                    .filter_map(|e| e["body"]["output"].as_str())
                    .collect();
                return match self.wait_response(seq, Duration::ZERO) {
                    Err(DapError::Failed { command, message }) if !said.trim().is_empty() => {
                        Err(DapError::Failed {
                            command,
                            message: format!("{message}\n{}", said.trim_end()),
                        })
                    }
                    Err(e) => Err(e),
                    Ok(_) => Err(DapError::Closed),
                };
            }
            self.pump(deadline)?;
        }
    }

    /// Waits for the adapter to ask for a child session (`startDebugging`) and returns its
    /// arguments: `request` (`launch` or `attach`) and `configuration`.
    pub fn wait_start_debugging(&mut self, timeout: Duration) -> Result<Value, DapError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(start) = self.starts.pop_front() {
                return Ok(start);
            }
            self.pump(deadline)?;
        }
    }

    /// The arguments of a `startDebugging` request that arrived, if any.
    pub fn take_start(&mut self) -> Option<Value> {
        self.starts.pop_front()
    }

    /// Whether the adapter sees other paths than this machine (`SPEC-0322` behaviour 14).
    pub(crate) fn remote(&self) -> bool {
        self.paths.is_some()
    }

    /// Whether an event named `name` is queued.
    pub(crate) fn has_event(&self, name: &str) -> bool {
        self.events.iter().any(|e| e["event"] == name)
    }

    /// Removes and returns all queued events named `name`.
    pub fn take_events(&mut self, name: &str) -> Vec<Value> {
        let (taken, kept): (VecDeque<Value>, VecDeque<Value>) = std::mem::take(&mut self.events)
            .into_iter()
            .partition(|e| e["event"] == name);
        self.events = kept;
        taken.into()
    }

    /// Reads whatever has arrived within `wait` without blocking longer.
    pub fn drain(&mut self, wait: Duration) {
        let deadline = Instant::now() + wait;
        while self.pump(deadline).is_ok() {}
    }
}

impl Drop for DapClient {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            // The whole group: an adapter started by a script leaves its real process otherwise.
            #[cfg(unix)]
            terminate_group(child.id());
            if let Ok(None) = child.try_wait() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

/// Sends SIGTERM to the process group `leader` leads (the adapter was spawned with
/// `process_group(0)`, so the group's id is its pid).
///
/// Directly with `killpg`, not through the `kill` program: procps-ng's `kill` (Ubuntu 24.04)
/// reads `kill -TERM -<pgid>` wrongly and signals other processes; with a pid of six digits it sent
/// SIGTERM to -1, every process of the user, which ended the IDE, its terminal and the login
/// session when a debug session closed (found by the Linux window self-test). A group id of 0 or
/// 1 would mean the caller's own group or every process, so those are never signalled.
#[cfg(unix)]
fn terminate_group(leader: u32) {
    let Ok(group) = i32::try_from(leader) else {
        return;
    };
    if group > 1 {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(group),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn alive(pid: u32) -> bool {
        let pid = nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap());
        nix::sys::signal::kill(pid, None).is_ok()
    }

    /// The adapter's group ends — its leader and what the leader started — and nothing else: this
    /// process and a process in another group keep running.
    #[test]
    fn ending_an_adapter_signals_its_group_only() {
        let mut bystander = Command::new("sleep").arg("30").spawn().unwrap();
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & echo $!; wait"])
            .stdout(Stdio::piped());
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut adapter = command.spawn().unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut BufReader::new(adapter.stdout.take().unwrap()),
            &mut line,
        )
        .unwrap();
        let grandchild: u32 = line.trim().parse().unwrap();
        assert!(alive(grandchild));

        terminate_group(adapter.id());
        let _ = adapter.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(grandchild) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !alive(grandchild),
            "the process the adapter started ends with it"
        );
        assert!(
            bystander.try_wait().unwrap().is_none(),
            "a process outside the group keeps running"
        );
        let _ = bystander.kill();
        let _ = bystander.wait();
    }

    #[test]
    fn the_own_group_and_every_process_are_never_signalled() {
        // Would end this test process (0) or every process of the user (1).
        terminate_group(0);
        terminate_group(1);
        terminate_group(u32::MAX);
    }
}

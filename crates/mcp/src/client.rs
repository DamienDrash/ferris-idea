//! A small MCP client for the running window's HTTP server (`SPEC-0221`), used by the command line
//! (`ferris-idea quit`, `SPEC-0261` v2 behaviour 11). It finds running windows through their discovery
//! files and speaks JSON-RPC over plain HTTP/1.1 to `127.0.0.1`.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

/// How long one request may take: a call can wait for the user's answer in the window (2 minutes)
/// and for saving files.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// A running window found through its discovery file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningWindow {
    /// Its process id.
    pub pid: u32,
    /// The port it listens on (127.0.0.1).
    pub port: u16,
    /// The bearer token of this start.
    pub token: String,
    /// Project roots it serves.
    pub projects: Vec<String>,
}

/// The windows that serve MCP over HTTP for the user whose settings folder is `config_dir`, by
/// process id; files of processes that no longer run are skipped.
pub fn running_windows(config_dir: &Path) -> Vec<RunningWindow> {
    let Ok(entries) = std::fs::read_dir(config_dir.join("mcp-http")) else {
        return Vec::new();
    };
    let mut found: Vec<RunningWindow> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
            Some(RunningWindow {
                pid,
                port: u16::try_from(v["port"].as_u64()?).ok()?,
                token: v["token"].as_str()?.to_owned(),
                projects: v["projects"]
                    .as_array()
                    .map(|ps| {
                        ps.iter()
                            .filter_map(|p| p["root"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .filter(|w| alive(w.pid))
        .collect();
    found.sort_by_key(|w| w.pid);
    found
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    pid == std::process::id()
        || std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

/// Without a check of other processes a discovery file of a window that ended could name a port
/// someone else holds now, so off Unix only this process's own file counts (`SPEC-0321` v6
/// behaviour 8a).
#[cfg(not(unix))]
fn alive(pid: u32) -> bool {
    pid == std::process::id()
}

/// A connected MCP session with a window.
pub struct Client {
    port: u16,
    token: String,
    /// The route: `/mcp` (the first project) or `/mcp/<id>` of one project.
    path: String,
    session: Option<String>,
    next_id: u64,
    /// How long one request may take ([`REQUEST_TIMEOUT`] unless set otherwise).
    timeout: Duration,
}

/// Why a call within a limit failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// No answer within the limit; the call was withdrawn (`SPEC-0261` v10 behaviour 11).
    TimedOut,
    /// Refused, failed, or the window could not be reached: the error text.
    Failed(String),
}

impl Client {
    /// Connects to the first project of `window` as client `name` (the name the window shows when it
    /// asks for a right).
    pub fn connect(window: &RunningWindow, name: &str) -> Result<Self, String> {
        Self::connect_path(window, "/mcp".into(), name)
    }

    /// Connects to the route of the project at `root` (as the discovery file names it), so the
    /// client reaches only that project's windows (`SPEC-0319` behaviour 34).
    pub fn connect_project(window: &RunningWindow, root: &str, name: &str) -> Result<Self, String> {
        let id = crate::http::workspace_id(Path::new(root));
        Self::connect_path(window, format!("/mcp/{id}"), name)
    }

    /// [`Client::connect`] that gives up after `limit`, also for every later request until
    /// [`Client::set_timeout`] changes it.
    pub fn connect_within(
        window: &RunningWindow,
        name: &str,
        limit: Duration,
    ) -> Result<Self, String> {
        Self::connect_path_within(window, "/mcp".into(), name, limit)
    }

    /// The limit of each later request.
    pub fn set_timeout(&mut self, limit: Duration) {
        self.timeout = limit;
    }

    fn connect_path(window: &RunningWindow, path: String, name: &str) -> Result<Self, String> {
        Self::connect_path_within(window, path, name, REQUEST_TIMEOUT)
    }

    fn connect_path_within(
        window: &RunningWindow,
        path: String,
        name: &str,
        limit: Duration,
    ) -> Result<Self, String> {
        let mut client = Self {
            port: window.port,
            token: window.token.clone(),
            path,
            session: None,
            next_id: 0,
            timeout: limit,
        };
        let reply = client.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": name, "version": env!("CARGO_PKG_VERSION")}}),
        )?;
        if let Some(e) = reply.get("error") {
            return Err(format!("initialize: {}", e["message"]));
        }
        client.post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(client)
    }

    /// Calls `tool`: `Ok` with the tool's JSON result (text that is not JSON as a string), `Err` with
    /// the error text of a refused or failed call.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        self.call_with_images(tool, arguments).map(|(v, _)| v)
    }

    /// Like [`Client::call`], with the image content of the result as `(mime type, base64
    /// data)` (`ui_screenshot`, `SPEC-0319` v7 behaviour 53).
    pub fn call_with_images(
        &mut self,
        tool: &str,
        arguments: Value,
    ) -> Result<(Value, Vec<(String, String)>), String> {
        self.call_within_images(tool, arguments, self.timeout)
            .map_err(|e| match e {
                CallError::TimedOut => format!(
                    "no answer within {} s; the call was withdrawn",
                    self.timeout.as_secs()
                ),
                CallError::Failed(m) => m,
            })
    }

    /// [`Client::call`] with its own limit: without an answer by then the connection closes and
    /// the call is cancelled (`notifications/cancelled`), so a question it waits for is withdrawn
    /// and it never takes effect later (`SPEC-0261` v10 behaviour 11, `SPEC-0053` v12 behaviour 8).
    pub fn call_within(
        &mut self,
        tool: &str,
        arguments: Value,
        limit: Duration,
    ) -> Result<Value, CallError> {
        self.call_within_images(tool, arguments, limit)
            .map(|(v, _)| v)
    }

    fn call_within_images(
        &mut self,
        tool: &str,
        arguments: Value,
        limit: Duration,
    ) -> Result<(Value, Vec<(String, String)>), CallError> {
        self.next_id += 1;
        let id = self.next_id;
        let message = json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        });
        let body = match self.post_within(&message, limit) {
            Ok((_, body)) => body,
            Err(PostError::TimedOut) => {
                let note = json!({
                    "jsonrpc": "2.0", "method": "notifications/cancelled",
                    "params": {"requestId": id, "reason": "no answer in time"},
                });
                let _ = self.post_within(&note, Duration::from_secs(1));
                return Err(CallError::TimedOut);
            }
            Err(PostError::Failed(m)) => return Err(CallError::Failed(m)),
        };
        let reply: Value = serde_json::from_str(&body)
            .map_err(|e| CallError::Failed(format!("tools/call: {e}: {body:?}")))?;
        Self::reply_of(reply).map_err(CallError::Failed)
    }

    /// The tool's result and its images in a `tools/call` reply.
    fn reply_of(reply: Value) -> Result<(Value, Vec<(String, String)>), String> {
        if let Some(error) = reply.get("error") {
            return Err(error["message"].as_str().unwrap_or_default().to_owned());
        }
        let result = &reply["result"];
        let text = result["content"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if result["isError"] == json!(true) {
            return Err(text);
        }
        let images = result["content"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter(|p| p["type"] == json!("image"))
                    .filter_map(|p| {
                        Some((
                            p["mimeType"].as_str()?.to_owned(),
                            p["data"].as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok((
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
            images,
        ))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let (session, body) = self.post(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }))?;
        if method == "initialize" {
            self.session = session;
        }
        serde_json::from_str(&body).map_err(|e| format!("{method}: {e}: {body:?}"))
    }

    /// One POST; returns the session header and the JSON-RPC message of the body.
    fn post(&self, message: &Value) -> Result<(Option<String>, String), String> {
        self.post_within(message, self.timeout)
            .map_err(|e| match e {
                PostError::TimedOut => format!(
                    "the window at 127.0.0.1:{} did not answer within {} s",
                    self.port,
                    self.timeout.as_secs()
                ),
                PostError::Failed(m) => m,
            })
    }

    /// [`Client::post`] that gives up after `limit`; closing the connection then ends the waiting
    /// call on the server.
    fn post_within(
        &self,
        message: &Value,
        limit: Duration,
    ) -> Result<(Option<String>, String), PostError> {
        let body = message.to_string();
        let mut head = format!(
            "POST {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {}\r\nMCP-Protocol-Version: 2025-06-18\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.path,
            self.port,
            self.token,
            body.len()
        );
        if let Some(s) = &self.session {
            head.push_str(&format!("Mcp-Session-Id: {s}\r\n"));
        }
        head.push_str("\r\n");
        let port = self.port;
        let failed = move |e: std::io::Error| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) {
                PostError::TimedOut
            } else {
                PostError::Failed(format!("the window at 127.0.0.1:{port}: {e}"))
            }
        };
        let start = std::time::Instant::now();
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        let mut stream = std::net::TcpStream::connect_timeout(&address, limit).map_err(failed)?;
        stream.set_write_timeout(Some(limit)).map_err(failed)?;
        stream.write_all(head.as_bytes()).map_err(failed)?;
        stream.write_all(body.as_bytes()).map_err(failed)?;
        // One limit for the whole answer, also when it arrives in pieces.
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let left = limit.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Err(PostError::TimedOut);
            }
            stream.set_read_timeout(Some(left)).map_err(failed)?;
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(failed(e)),
            }
        }
        parse_response(&String::from_utf8_lossy(&raw)).map_err(PostError::Failed)
    }
}

/// Why one POST failed.
enum PostError {
    TimedOut,
    Failed(String),
}

/// Status, session header and JSON-RPC message of a raw HTTP response (JSON body or event stream,
/// plain or chunked).
fn parse_response(text: &str) -> Result<(Option<String>, String), String> {
    let (head, rest) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "no HTTP answer".to_owned())?;
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "no HTTP status".to_owned())?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    let body = if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        dechunk(rest)
    } else {
        rest.to_owned()
    };
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    let body = if header("content-type").is_some_and(|v| v.starts_with("text/event-stream")) {
        body.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim)
            .find(|d| d.starts_with('{'))
            .unwrap_or_default()
            .to_owned()
    } else {
        body
    };
    Ok((header("mcp-session-id").map(str::to_owned), body))
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_json_chunked_and_streamed() {
        let plain = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nmcp-session-id: s1\r\n\r\n{\"id\":1}";
        assert_eq!(
            parse_response(plain).unwrap(),
            (Some("s1".into()), "{\"id\":1}".into())
        );
        let chunked = "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\ncontent-type: application/json\r\n\r\n4\r\n{\"a\"\r\n4\r\n:1}\n\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap().1, "{\"a\":1}\n");
        let stream =
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\nid: 0\ndata: {\"id\":2}\n\n";
        assert_eq!(parse_response(stream).unwrap().1, "{\"id\":2}");
        let refused = "HTTP/1.1 401 Unauthorized\r\n\r\nmissing or wrong bearer token";
        assert_eq!(
            parse_response(refused).unwrap_err(),
            "HTTP 401: missing or wrong bearer token"
        );
    }

    #[test]
    fn running_windows_skip_dead_processes() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-client-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mcp-http")).unwrap();
        let me = std::process::id();
        std::fs::write(
            dir.join(format!("mcp-http/{me}.json")),
            format!("{{\"pid\": {me}, \"port\": 4000, \"token\": \"t\", \"projects\": [{{\"root\": \"/p\"}}]}}"),
        )
        .unwrap();
        // A process id that does not run.
        std::fs::write(
            dir.join("mcp-http/999999.json"),
            "{\"pid\": 999999, \"port\": 4001, \"token\": \"u\"}",
        )
        .unwrap();
        let found = running_windows(&dir);
        assert_eq!(
            found,
            vec![RunningWindow {
                pid: me,
                port: 4000,
                token: "t".into(),
                projects: vec!["/p".into()]
            }]
        );
        assert!(running_windows(&dir.join("none")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

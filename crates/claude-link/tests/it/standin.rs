//! A stand-in window and a stand-in Claude Code client.

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferris_idea_claude_link::server::AUTH_HEADER;
use ferris_idea_claude_link::{
    Diagnostic, DiffDecision, DiffProposal, Editor, LinkConfig, LinkServer, OpenEditor,
    OpenRequest, Selection,
};
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

/// The window as the tests set it up.
#[derive(Default)]
pub struct StandIn {
    pub selection: Mutex<Option<Selection>>,
    pub editors: Mutex<Vec<OpenEditor>>,
    pub diagnostics: Mutex<Vec<Diagnostic>>,
    pub opened: Mutex<Vec<OpenRequest>>,
    pub diffs: Mutex<Vec<(DiffProposal, Sender<DiffDecision>)>>,
    pub closed: Mutex<Vec<String>>,
    pub changes: AtomicUsize,
}

impl Editor for StandIn {
    fn selection(&self, _: PathBuf, reply: Sender<Option<Selection>>) {
        let _ = reply.send(self.selection.lock().unwrap().clone());
    }
    fn open_editors(&self, _: PathBuf, reply: Sender<Vec<OpenEditor>>) {
        let _ = reply.send(self.editors.lock().unwrap().clone());
    }
    fn diagnostics(&self, _: PathBuf, file: Option<PathBuf>, reply: Sender<Vec<Diagnostic>>) {
        let all = self.diagnostics.lock().unwrap().clone();
        let _ = reply.send(
            all.into_iter()
                .filter(|d| file.as_ref().is_none_or(|f| *f == d.path))
                .collect(),
        );
    }
    fn open_file(&self, _: PathBuf, request: OpenRequest, reply: Sender<Result<(), String>>) {
        self.opened.lock().unwrap().push(request);
        let _ = reply.send(Ok(()));
    }
    fn show_diff(&self, _: PathBuf, proposal: DiffProposal, reply: Sender<DiffDecision>) {
        self.diffs.lock().unwrap().push((proposal, reply));
    }
    fn close_diff(&self, _: PathBuf, tab_name: String) {
        self.closed.lock().unwrap().push(tab_name);
    }
    fn link_changed(&self, _: PathBuf) {
        self.changes.fetch_add(1, Ordering::SeqCst);
    }
}

impl StandIn {
    /// Waits for the next diff the link shows.
    pub fn next_diff(&self) -> (DiffProposal, Sender<DiffDecision>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(d) = self.diffs.lock().unwrap().pop() {
                return d;
            }
            assert!(Instant::now() < deadline, "no diff was shown");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A project folder, a lock folder and a running link, all in a temporary folder.
pub struct Setup {
    pub dir: tempfile::TempDir,
    pub workspace: PathBuf,
    pub lock_dir: PathBuf,
    pub window: Arc<StandIn>,
    pub link: Option<LinkServer>,
}

impl Setup {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("project");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let lock_dir = dir.path().join("claude/ide");
        let window = Arc::new(StandIn::default());
        let editor: Arc<dyn Editor> = window.clone();
        let link = LinkServer::start(
            LinkConfig {
                workspace: workspace.clone(),
                lock_dir: lock_dir.clone(),
                ide_name: "Ferris IDEA".into(),
                ide_version: "0.0.0-test".into(),
            },
            editor,
        )
        .unwrap();
        Self {
            dir,
            workspace,
            lock_dir,
            window,
            link: Some(link),
        }
    }

    pub fn link(&self) -> &LinkServer {
        self.link.as_ref().unwrap()
    }

    /// The lock file's content.
    pub fn lock(&self) -> Value {
        let path = self.link().lock_file().unwrap();
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    pub fn token(&self) -> String {
        self.lock()["authToken"].as_str().unwrap().to_owned()
    }

    /// A client with the right token, initialized.
    pub fn client(&self) -> Client {
        let mut c = Client::connect(self.link().port(), Some(&self.token()), &[]).unwrap();
        let init = c.call(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "claude-code", "version": "9.9.9"}}),
        );
        assert!(init.get("result").is_some(), "{init}");
        c.notify("notifications/initialized", json!({}));
        c.notify("ide_connected", json!({"pid": 4242}));
        c
    }
}

/// A stand-in Claude Code client.
pub struct Client {
    pub ws: WebSocket<TcpStream>,
    next: u64,
    /// Notifications received while waiting for answers.
    pub notifications: Vec<Value>,
    /// The subprotocol the link answered.
    pub protocol: Option<String>,
}

impl Client {
    /// Opens the WebSocket with `token` in the header (or none) and `extra` headers.
    pub fn connect(
        port: u16,
        token: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<Self, tungstenite::Error> {
        let mut request = format!("ws://127.0.0.1:{port}/")
            .into_client_request()
            .unwrap();
        if let Some(t) = token {
            request
                .headers_mut()
                .insert(AUTH_HEADER, t.parse().unwrap());
        }
        for (k, v) in extra {
            let name: tungstenite::http::HeaderName = k.parse().unwrap();
            request.headers_mut().insert(name, v.parse().unwrap());
        }
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let (ws, response) = tungstenite::client::client(request, stream).map_err(|e| match e {
            tungstenite::HandshakeError::Failure(f) => f,
            tungstenite::HandshakeError::Interrupted(_) => {
                tungstenite::Error::Io(std::io::ErrorKind::WouldBlock.into())
            }
        })?;
        let protocol = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok(Self {
            ws,
            next: 1,
            notifications: Vec::new(),
            protocol,
        })
    }

    pub fn send(&mut self, v: Value) {
        self.ws.send(Message::text(v.to_string())).unwrap();
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Sends a request and returns its id without waiting.
    pub fn start(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    /// Reads one message.
    pub fn read(&mut self) -> Value {
        loop {
            match self.ws.read().unwrap() {
                Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
                Message::Close(_) => panic!("the link closed the connection"),
                _ => {}
            }
        }
    }

    /// Waits for the answer to `id`, keeping notifications.
    pub fn answer(&mut self, id: u64) -> Value {
        loop {
            let m = self.read();
            if m["id"] == json!(id) {
                return m;
            }
            if m.get("method").is_some() {
                self.notifications.push(m);
            }
        }
    }

    /// A request and its answer.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.start(method, params);
        self.answer(id)
    }

    /// A tool call; returns the answer.
    pub fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.call("tools/call", json!({"name": name, "arguments": arguments}))
    }

    /// The first text of a tool answer, parsed as JSON when it is.
    pub fn tool_text(&mut self, name: &str, arguments: Value) -> String {
        let a = self.tool(name, arguments);
        a["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{a}"))
            .to_owned()
    }

    pub fn tool_json(&mut self, name: &str, arguments: Value) -> Value {
        serde_json::from_str(&self.tool_text(name, arguments)).unwrap()
    }
}

//! SPEC-0221 end to end: MCP over Streamable HTTP with raw HTTP/1.1 requests, as a client sends them.

use std::path::{Path, PathBuf};

use idecraft_core::Workspace;
use idecraft_editor::TextSnapshot;
use idecraft_editor::documents::DocumentRegistry;
use idecraft_mcp::IdeServer;
use idecraft_mcp::http::HttpServer;
use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-http-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn server_for(root: &Path, documents: &DocumentRegistry) -> IdeServer {
    let permissions = idecraft_core::AgentPermissions {
        allow: Default::default(),
        file: Some(root.join("agent-permissions.toml")),
        problem: None,
    };
    IdeServer::with_permissions(Workspace::open(root).unwrap(), permissions)
        .with_plugins_dir(root.join(".test-plugins"))
        .with_documents(documents.clone())
}

/// Status, headers (lower-case names) and body of one HTTP/1.1 exchange.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The JSON-RPC message of the reply: the body, or the event stream's `data:` line.
    fn message(&self) -> Value {
        if self
            .header("content-type")
            .is_some_and(|c| c.starts_with("text/event-stream"))
        {
            let data = self
                .body
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim)
                .find(|d| d.starts_with('{'))
                .expect("an event with data");
            serde_json::from_str(data).unwrap()
        } else {
            serde_json::from_str(&self.body).unwrap()
        }
    }
}

/// Whether an MCP server with this token answers `initialize` at `port` with success.
async fn serves(port: u16, auth: &str, init: &Value) -> bool {
    let Ok(mut stream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
        return false;
    };
    let body = serde_json::to_string(init).unwrap();
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    // A listener that is not an HTTP server, or the connection itself, never answers.
    let mut raw = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut raw),
    )
    .await;
    read.is_ok() && String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 200")
}

async fn post(port: u16, path: &str, headers: &[(&str, &str)], body: &Value) -> Reply {
    let body = serde_json::to_string(body).unwrap();
    let mut req = format!(
        "POST {path} HTTP/1.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (n, v) in headers {
        req.push_str(&format!("{n}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&body);
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        stream.read_to_end(&mut raw),
    );
    read.await.expect("reply in time").unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").expect("a header block");
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v.contains("chunked"));
    let body = if chunked {
        dechunk(rest)
    } else {
        rest.to_owned()
    };
    Reply {
        status,
        headers,
        body,
    }
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

/// An MCP client over HTTP with a session as revision 2025-06-18 requires.
struct Client {
    port: u16,
    path: String,
    auth: String,
    session: Option<String>,
    next_id: u64,
}

impl Client {
    async fn connect(port: u16, path: &str, token: &str) -> Self {
        let mut c = Self {
            port,
            path: path.to_owned(),
            auth: format!("Bearer {token}"),
            session: None,
            next_id: 0,
        };
        let init = c
            .exchange(
                json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}}}),
            )
            .await;
        assert_eq!(init.status, 200, "{}", init.body);
        c.session = init.header("mcp-session-id").map(str::to_owned);
        let info = init.message();
        assert_eq!(info["result"]["serverInfo"]["name"], "idecraft");
        let note = c
            .exchange(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        assert!(note.status == 202 || note.status == 200, "{}", note.status);
        c
    }

    async fn exchange(&self, body: Value) -> Reply {
        let mut headers = vec![
            ("Authorization", self.auth.as_str()),
            ("MCP-Protocol-Version", "2025-06-18"),
        ];
        if let Some(s) = &self.session {
            headers.push(("Mcp-Session-Id", s.as_str()));
        }
        post(self.port, &self.path, &headers, &body).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let reply = self
            .exchange(
                json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}),
            )
            .await;
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.message()
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await
    }
}

fn text(response: &Value) -> String {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn tool_names(list: &Value) -> Vec<String> {
    let mut names: Vec<String> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

/// The tool names a stdio client sees, for comparison.
async fn stdio_tool_names(server: IdeServer) -> Vec<String> {
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    tokio::spawn(async move {
        let running = server.serve(tokio::io::split(server_io)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    for m in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    ] {
        write.write_all(format!("{m}\n").as_bytes()).await.unwrap();
    }
    loop {
        let line = lines.next_line().await.unwrap().unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == json!(2) {
            return tool_names(&v);
        }
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test(flavor = "multi_thread")]
async fn token_origin_host_discovery_and_stdio_parity() {
    let root = temp_dir("ws");
    std::fs::write(root.join("notes.txt"), "one\ntwo\nthree\n").unwrap();
    let config = temp_dir("config");
    let documents = DocumentRegistry::default();
    let server = HttpServer::start(Some(&config)).unwrap();
    let port = server.port();
    let token = server.token().to_owned();
    let url = server.add_workspace(server_for(&root, &documents)).unwrap();
    assert!(url.starts_with(&format!("http://127.0.0.1:{port}/mcp/")));
    let path = url.trim_start_matches(&format!("http://127.0.0.1:{port}"));
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "x", "version": "0"}}});

    // Missing and wrong tokens never reach MCP (behaviour 2).
    let r = post(port, "/mcp", &[], &init).await;
    assert_eq!(r.status, 401);
    assert!(r.header("www-authenticate").unwrap().starts_with("Bearer"));
    let wrong = format!("Bearer {}", "0".repeat(64));
    let r = post(port, "/mcp", &[("Authorization", &wrong)], &init).await;
    assert_eq!(r.status, 401);
    assert!(!r.body.contains("jsonrpc"));

    // DNS rebinding: a foreign Host or Origin is refused even with the token (behaviour 3).
    let auth = format!("Bearer {token}");
    let r = post(
        port,
        "/mcp",
        &[("Authorization", &auth), ("Host", "evil.example")],
        &init,
    )
    .await;
    assert_eq!(r.status, 403);
    let r = post(
        port,
        "/mcp",
        &[("Authorization", &auth), ("Origin", "http://evil.example")],
        &init,
    )
    .await;
    assert_eq!(r.status, 403);
    let other_port = format!("http://127.0.0.1:{}", port.wrapping_add(1));
    let r = post(
        port,
        "/mcp",
        &[("Authorization", &auth), ("Origin", &other_port)],
        &init,
    )
    .await;
    assert_eq!(r.status, 403);
    let r = post(port, "/elsewhere", &[("Authorization", &auth)], &init).await;
    assert_eq!(r.status, 404);

    // With the token and a loopback origin the tools behave as over stdio (behaviour 5).
    let own_origin = format!("http://localhost:{port}");
    let r = post(
        port,
        "/mcp",
        &[("Authorization", &auth), ("Origin", &own_origin)],
        &init,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let mut client = Client::connect(port, path, &token).await;
    let list = client.request("tools/list", json!({})).await;
    let names = tool_names(&list);
    assert!(names.contains(&"open_documents".to_owned()));
    assert_eq!(names, stdio_tool_names(server_for(&root, &documents)).await);
    let read = client
        .call(
            "read_file",
            json!({"path": "notes.txt", "start_line": 2, "end_line": 3}),
        )
        .await;
    assert_eq!(text(&read), "two\nthree");
    let escape = client.call("read_file", json!({"path": "../x"})).await;
    assert!(escape.get("error").is_some() || escape["result"]["isError"] == json!(true));
    // The first project is also served at `/mcp`.
    let mut first = Client::connect(port, "/mcp", &token).await;
    let info = first.call("workspace_info", json!({})).await;
    assert!(text(&info).contains(&*root.to_string_lossy()), "{info}");

    // Discovery file: 0600 in a 0700 folder outside the project, removed on stop (behaviour 4).
    let file = server.discovery_file().unwrap().to_path_buf();
    assert!(file.starts_with(&config) && !file.starts_with(&root));
    let found: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(found["token"], json!(token));
    assert_eq!(found["port"], json!(port));
    assert_eq!(found["projects"][0]["url"], json!(url));
    #[cfg(unix)]
    {
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
    }
    // Loopback only (behaviour 1).
    assert_eq!(server.local_addr().ip(), std::net::Ipv4Addr::LOCALHOST);
    server.stop();
    assert!(!file.exists());
    // The server no longer answers: a request with its token is refused or answered by no MCP
    // server of this token. The port number itself may meanwhile be given to another listener
    // (a server of a test running beside this one got it on a busy machine) or to this very
    // connection (a loopback connection to itself), so a connection alone proves nothing.
    assert!(!serves(port, &auth, &init).await, "no answer after stop");

    // A stale file of a process that no longer runs is removed at the next start.
    let stale = config.join("mcp-http/999999.json");
    std::fs::write(&stale, "{}").unwrap();
    let again = HttpServer::start(Some(&config)).unwrap();
    assert!(!stale.exists());
    assert_ne!(again.token(), token);
    drop(again);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(config);
}

#[tokio::test(flavor = "multi_thread")]
async fn unsaved_buffer_is_visible_over_http() {
    let root = temp_dir("buffer");
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    let documents = DocumentRegistry::default();
    let server = HttpServer::start(None).unwrap();
    server.add_workspace(server_for(&root, &documents)).unwrap();
    let mut client = Client::connect(server.port(), "/mcp", server.token()).await;
    let empty = client.call("open_documents", json!({})).await;
    assert_eq!(text(&empty), "[]");

    // The window opens the file and types without saving (SPEC-0220 behaviour 2).
    let key = DocumentRegistry::key(&root.join("main.rs"));
    documents.open(&key, TextSnapshot::from_text("fn main() {}\n"), false);
    documents.publish(
        &key,
        TextSnapshot::from_text("fn main() {\n    println!(\"unsaved\");\n}\n"),
        true,
    );
    let read = client.call("read_file", json!({"path": "main.rs"})).await;
    assert!(text(&read).contains("unsaved"), "{read}");
    let open = client.call("open_documents", json!({})).await;
    let list: Value = serde_json::from_str(&text(&open)).unwrap();
    assert_eq!(
        list,
        json!([{"path": "main.rs", "revision": 2, "modified": true}])
    );
    // Closed again, the disk text is read.
    documents.close(&key);
    let read = client.call("read_file", json!({"path": "main.rs"})).await;
    assert_eq!(text(&read), "fn main() {}");
    let _ = std::fs::remove_dir_all(root);
}

/// A stand-in window that allows once and records questions and quits.
#[derive(Default)]
struct AllowingWindow {
    clients: std::sync::Mutex<Vec<String>>,
    quits: std::sync::Mutex<Vec<idecraft_mcp::window::QuitMode>>,
}

impl idecraft_mcp::window::Window for AllowingWindow {
    fn ask(
        &self,
        request: idecraft_mcp::window::PermissionRequest,
        reply: tokio::sync::oneshot::Sender<idecraft_mcp::window::PermissionAnswer>,
    ) {
        self.clients.lock().unwrap().push(request.client);
        let _ = reply.send(idecraft_mcp::window::PermissionAnswer::AllowOnce);
    }
    fn pending_work(&self, reply: tokio::sync::oneshot::Sender<idecraft_mcp::window::PendingWork>) {
        let _ = reply.send(Default::default());
    }
    fn quit(
        &self,
        mode: idecraft_mcp::window::QuitMode,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    ) {
        self.quits.lock().unwrap().push(mode);
        let _ = reply.send(Ok(()));
    }
}

/// SPEC-0221 v2 AK-7, SPEC-0261 v2 AK-4: the command-line client finds the window through the
/// discovery file; the client name it sends reaches the question; `quit_ide` quits.
#[tokio::test(flavor = "multi_thread")]
async fn the_cli_client_is_named_in_the_question_and_quits() {
    let root = temp_dir("cli");
    let config = temp_dir("cli-config");
    let window = std::sync::Arc::new(AllowingWindow::default());
    let server = HttpServer::start(Some(&config)).unwrap();
    let w: std::sync::Arc<dyn idecraft_mcp::window::Window> = window.clone();
    server
        .add_workspace(server_for(&root, &DocumentRegistry::default()).with_window(w))
        .unwrap();
    let cfg = config.clone();
    let (created, quit) = tokio::task::spawn_blocking(move || {
        let found = idecraft_mcp::client::running_windows(&cfg);
        assert_eq!(found.len(), 1, "{found:?}");
        let mut client = idecraft_mcp::client::Client::connect(&found[0], "idecraft-cli").unwrap();
        let created = client.call("create_file", json!({"dir": "", "name": "a.txt"}));
        let quit = client.call("quit_ide", json!({}));
        (created, quit)
    })
    .await
    .unwrap();
    assert!(created.is_ok(), "{created:?}");
    assert!(root.join("a.txt").exists());
    // Creating the file and quitting are each asked (SPEC-0261 v8: quitting needs `quit`).
    let clients = window.clients.lock().unwrap().clone();
    assert_eq!(clients.len(), 2);
    assert!(
        clients.iter().all(|c| c.starts_with("idecraft-cli ")),
        "{clients:?}"
    );
    assert_eq!(quit.unwrap()["quit"], true);
    assert_eq!(
        *window.quits.lock().unwrap(),
        [idecraft_mcp::window::QuitMode::IfNothingPending]
    );
    server.stop();
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(config);
}

/// SPEC-0094 v3 behaviour 8: a project whose window closed is no longer served nor listed in the
/// discovery file; the other project stays.
#[tokio::test]
async fn a_closed_project_is_no_longer_served() {
    let one = temp_dir("closed-one");
    let two = temp_dir("closed-two");
    let config = temp_dir("closed-config");
    let documents = DocumentRegistry::default();
    let server = HttpServer::start(Some(&config)).unwrap();
    let port = server.port();
    let token = format!("Bearer {}", server.token());
    let url_one = server.add_workspace(server_for(&one, &documents)).unwrap();
    let url_two = server.add_workspace(server_for(&two, &documents)).unwrap();
    let path_one = url_one.trim_start_matches(&format!("http://127.0.0.1:{port}"));
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "x", "version": "0"}}});
    let ask = |path: String| {
        let (token, init) = (token.clone(), init.clone());
        async move {
            post(port, &path, &[("Authorization", &token)], &init)
                .await
                .status
        }
    };
    assert_eq!(ask(path_one.to_owned()).await, 200);

    assert!(server.remove_workspace(&one).unwrap());
    assert!(!server.remove_workspace(&one).unwrap(), "only once");
    assert_eq!(ask(path_one.to_owned()).await, 404);
    let file = server.discovery_file().unwrap().to_path_buf();
    let found: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(found["projects"].as_array().unwrap().len(), 1);
    assert_eq!(found["projects"][0]["url"], json!(url_two));
}

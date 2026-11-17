//! Black-box access to IDECraft as a user or agent has it: the `idecraft` binary (`open`, `mcp` on
//! stdin/stdout) and the MCP Streamable HTTP transport the window serves (SPEC-0221). Tests see
//! only tool names, arguments and the JSON a tool returns; no crate internals.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

/// How long one tool call may take before the test fails (language servers start cold).
const CALL_TIMEOUT: Duration = Duration::from_secs(180);

/// The `idecraft` binary under test.
pub fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_idecraft")
}

/// The reference projects folder (`tests/reference-projects`).
pub fn reference_projects() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects")
}

/// Records that a test could not exercise its row on this machine and why. With
/// `IDECRAFT_REQUIRE_TOOLS` set the test fails instead. The coverage table
/// (`scripts/functional.sh`) lists the reason.
pub fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    skip_optional(reason);
}

/// Records a skip for something CI does not provide either (a running reference database, a debug
/// adapter or Docker it does not install): never fails.
pub fn skip_optional(reason: &str) {
    eprintln!("skipped: {reason}");
    if let Some(dir) = std::env::var_os("IDECRAFT_FUNCTIONAL_REPORT") {
        let test = std::thread::current().name().unwrap_or("?").to_owned();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(Path::new(&dir).join("skipped.tsv"))
            .expect("report folder");
        // One write per line: tests run in parallel and append to the same file.
        let line = format!("{test}\t{}\n", reason.replace(['\t', '\n'], " "));
        f.write_all(line.as_bytes()).unwrap();
    }
}

/// A database URL from the environment (set by `scripts/ci-local.sh` when the reference databases
/// run); `None` records a skip.
pub fn live_db(var: &str) -> Option<String> {
    match std::env::var(var) {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            skip_optional(&format!("{var} not set (reference database not running)"));
            None
        }
    }
}

/// Whether a program is on `PATH`.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// A throw-away copy of a project with its own IDECraft settings folder outside it.
pub struct Project {
    base: PathBuf,
    /// Workspace root (resolved, so it matches what the IDE reports).
    pub root: PathBuf,
    /// The user's IDECraft settings folder for this test (`IDECRAFT_CONFIG_DIR`).
    pub config: PathBuf,
    /// The test password folder (`IDECRAFT_TEST_SECRETS_DIR`, SPEC-0008 behaviour 14) the
    /// processes started for this project get, so they never touch the system keychain.
    pub secrets: PathBuf,
}

impl Project {
    /// An empty workspace.
    pub fn empty() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir().join(format!("idecraft-func-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("ws")).unwrap();
        std::fs::create_dir_all(base.join("config")).unwrap();
        let base = base.canonicalize().unwrap();
        let p = Self {
            root: base.join("ws"),
            config: base.join("config"),
            secrets: base.join("secrets"),
            base,
        };
        // Agents may write files and change the repository unless a test says otherwise
        // (SPEC-0053 v4); [`Project::grant_exactly`] grants less.
        p.grant(&[]);
        p
    }

    /// A copy of `tests/reference-projects/<name>` (without `node_modules`).
    pub fn reference(name: &str) -> Self {
        let p = Self::empty();
        copy_dir(&reference_projects().join(name), &p.root);
        p
    }

    /// Writes a workspace file (folders are created).
    pub fn write(&self, rel: &str, text: &str) -> &Self {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        self
    }

    /// Reads a workspace file.
    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root.join(rel)).unwrap()
    }

    /// Absolute path of a workspace file.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Grants agents rights in this workspace, as the user does in their settings folder, together
    /// with `write` and `vcs-write`.
    pub fn grant(&self, rights: &[&str]) -> &Self {
        let mut all = vec!["write", "vcs-write"];
        all.extend(
            rights
                .iter()
                .copied()
                .filter(|r| !matches!(*r, "write" | "vcs-write")),
        );
        self.grant_exactly(&all)
    }

    /// Grants agents exactly these rights in this workspace. The database permissions granted
    /// with [`Project::connection`] stay.
    pub fn grant_exactly(&self, rights: &[&str]) -> &Self {
        let allow: Vec<String> = rights.iter().map(|r| format!("\"{r}\"")).collect();
        let file = self.config.join("agent-permissions.toml");
        let before = std::fs::read_to_string(&file).unwrap_or_default();
        let connections = before.find("[[connection]]").map_or("", |at| &before[at..]);
        std::fs::write(
            &file,
            format!(
                "[[workspace]]\npath = \"{}\"\nallow = [{}]\n\n{connections}",
                self.root.display(),
                allow.join(", ")
            ),
        )
        .unwrap();
        self
    }

    /// Grants agents `allow` on connection `id` as it is now, as the user does in their settings
    /// folder (SPEC-0012 v3): an entry for the connection's present target, which
    /// `list_connections` names. What the project file says grants nothing.
    pub fn grant_connection(&self, id: &str, allow: &[&str]) -> &Self {
        let out = self.cli(&["call", &self.root.to_string_lossy(), "list_connections"]);
        let listed: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "list_connections: {e}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
        let target = listed
            .as_array()
            .and_then(|all| all.iter().find(|c| c["id"] == id))
            .and_then(|c| c["target"].as_str())
            .unwrap_or_else(|| panic!("no connection `{id}` in {listed}"))
            .to_owned();
        idecraft_core::add_connection_grant(
            &self.config.join("agent-permissions.toml"),
            &self.root,
            id,
            &target,
            allow,
        )
        .unwrap();
        self
    }

    /// Runs `git` in the workspace with a fixed identity; panics when it fails.
    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=Func Test",
                "-c",
                "user.email=func@example.invalid",
            ])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// `git init` and one commit of everything. The repository gets its own identity, so commits
    /// the IDE makes in it (a merge through `git_merge`, a commit tool) work on a machine without a
    /// global Git identity, such as a CI runner.
    pub fn git_repo(&self) -> &Self {
        self.git(&["init", "-q"]);
        self.git(&["config", "user.name", "Func Test"]);
        self.git(&["config", "user.email", "func@example.invalid"]);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "initial"]);
        self
    }

    /// A connection profile `name` as the connection dialog writes it, with the given agent
    /// permissions (`read-schema`, `read-data`, `modify-data`, `modify-schema`) as the project
    /// file's upper limit and granted by the user for the connection as written
    /// ([`Project::grant_connection`]).
    pub fn connection(&self, name: &str, fields: &str, ai: &[&str]) -> &Self {
        let granted: Vec<&str> = ai.to_vec();
        let path = self.root.join(".idecraft/connections.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        let ai: Vec<String> = ai.iter().map(|r| format!("\"{r}\"")).collect();
        text.push_str(&format!(
            "[[connection]]\nid = \"{name}\"\nname = \"{name}\"\n{fields}\nai = [{}]\n\n",
            ai.join(", ")
        ));
        std::fs::write(path, text).unwrap();
        self.grant_connection(name, &granted)
    }

    /// A SQLite connection `name` on a new empty database file in the workspace, every right given.
    pub fn sqlite(&self, name: &str) -> &Self {
        let file = self.root.join(format!("{name}.sqlite"));
        std::fs::write(&file, b"").unwrap();
        self.connection(
            name,
            &format!("dialect = \"sqlite\"\npath = \"{}\"", file.display()),
            &["read-schema", "read-data", "modify-data", "modify-schema"],
        )
    }

    /// Runs `idecraft <args>` against this project's settings folder.
    pub fn cli(&self, args: &[&str]) -> std::process::Output {
        Command::new(binary())
            .args(args)
            .env("IDECRAFT_CONFIG_DIR", &self.config)
            .env(TEST_SECRETS_ENV, &self.secrets)
            .env_remove("IDECRAFT_PLUGINS")
            .output()
            .expect("idecraft runs")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "node_modules" || name == "target" || name == ".venv" {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

enum Transport {
    Stdio {
        child: Child,
        stdin: Option<ChildStdin>,
        lines: Receiver<String>,
    },
    Http {
        server: Option<idecraft_mcp::http::HttpServer>,
        port: u16,
        path: String,
        token: String,
        session: Option<String>,
    },
}

/// An MCP client connected to IDECraft for one project.
pub struct Ide {
    transport: Transport,
    next_id: u64,
}

impl Ide {
    /// `idecraft mcp <root>` on stdin/stdout, as an agent configured with the command starts it,
    /// with the project trusted through `IDECRAFT_TRUST_PROJECTS` (SPEC-0278 behaviour 10).
    pub fn stdio(project: &Project) -> Self {
        Self::stdio_trusted(project, true)
    }

    /// [`Ide::stdio`]; `trusted: false` serves the project in safe mode (SPEC-0278).
    pub fn stdio_trusted(project: &Project, trusted: bool) -> Self {
        let mut command = Command::new(binary());
        if trusted {
            command.env("IDECRAFT_TRUST_PROJECTS", "all");
        } else {
            command.env_remove("IDECRAFT_TRUST_PROJECTS");
        }
        let mut child = command
            .arg("mcp")
            .arg(&project.root)
            .env("IDECRAFT_CONFIG_DIR", &project.config)
            .env(TEST_SECRETS_ENV, &project.secrets)
            .env_remove("IDECRAFT_PLUGINS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("idecraft mcp starts");
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        let mut ide = Self {
            transport: Transport::Stdio {
                child,
                stdin,
                lines,
            },
            next_id: 0,
        };
        ide.initialize("2025-03-26");
        ide
    }

    /// The Streamable HTTP transport the running window serves (SPEC-0221), started for this
    /// project; the client reads URL and token from the discovery file as an agent does. Agent
    /// rights come from the process environment there, so HTTP tests use reading tools only.
    pub fn http(project: &Project) -> Self {
        let server = idecraft_mcp::http::HttpServer::start(Some(&project.config)).unwrap();
        let workspace = idecraft_core::Workspace::open(&project.root).unwrap();
        let permissions = idecraft_core::AgentPermissions {
            allow: Default::default(),
            file: Some(project.config.join("agent-permissions.toml")),
            problem: None,
        };
        let ide_server = idecraft_mcp::IdeServer::with_permissions(workspace, permissions)
            .with_plugins_dir(project.config.join("plugins"));
        server.add_workspace(ide_server).unwrap();
        let discovery: Value = serde_json::from_str(
            &std::fs::read_to_string(server.discovery_file().expect("a discovery file")).unwrap(),
        )
        .unwrap();
        let url = discovery["projects"][0]["url"].as_str().unwrap().to_owned();
        let token = discovery["token"].as_str().unwrap().to_owned();
        let port = discovery["port"].as_u64().unwrap() as u16;
        let path = url
            .strip_prefix(&format!("http://127.0.0.1:{port}"))
            .unwrap()
            .to_owned();
        let mut ide = Self {
            transport: Transport::Http {
                server: Some(server),
                port,
                path,
                token,
                session: None,
            },
            next_id: 0,
        };
        ide.initialize("2025-06-18");
        ide
    }

    fn initialize(&mut self, version: &str) {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": version, "capabilities": {},
                   "clientInfo": {"name": "functional-tests", "version": "0"}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "idecraft", "{init}");
        self.notify("notifications/initialized");
    }

    fn notify(&mut self, method: &str) {
        let message = json!({"jsonrpc": "2.0", "method": method});
        match &mut self.transport {
            Transport::Stdio { stdin, .. } => {
                let stdin = stdin.as_mut().unwrap();
                writeln!(stdin, "{message}").unwrap();
                stdin.flush().unwrap();
            }
            Transport::Http { .. } => {
                let (status, _, _) = self.post(&message);
                assert!(status == 200 || status == 202, "notification: {status}");
            }
        }
    }

    /// One JSON-RPC request; returns the whole response message.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        match &mut self.transport {
            Transport::Stdio { stdin, lines, .. } => {
                let stdin = stdin.as_mut().unwrap();
                writeln!(stdin, "{message}").unwrap();
                stdin.flush().unwrap();
                loop {
                    let line = lines
                        .recv_timeout(CALL_TIMEOUT)
                        .unwrap_or_else(|_| panic!("no answer to {method} {params}"));
                    let reply: Value = serde_json::from_str(&line).unwrap();
                    if reply["id"] == json!(id) {
                        return reply;
                    }
                }
            }
            Transport::Http { .. } => {
                let (status, headers, body) = self.post(&message);
                assert_eq!(status, 200, "{body}");
                if method == "initialize"
                    && let Transport::Http { session, .. } = &mut self.transport
                {
                    *session = headers
                        .iter()
                        .find(|(n, _)| n == "mcp-session-id")
                        .map(|(_, v)| v.clone());
                }
                // A JSON body, or an event stream whose `data:` line carries the message.
                let data = if headers
                    .iter()
                    .any(|(n, v)| n == "content-type" && v.starts_with("text/event-stream"))
                {
                    body.lines()
                        .filter_map(|l| l.strip_prefix("data:"))
                        .map(str::trim)
                        .find(|d| d.starts_with('{'))
                        .unwrap_or_default()
                        .to_owned()
                } else {
                    body
                };
                serde_json::from_str(&data).unwrap_or_else(|e| panic!("{e}: {data:?}"))
            }
        }
    }

    fn post(&self, message: &Value) -> (u16, Vec<(String, String)>, String) {
        let Transport::Http {
            port,
            path,
            token,
            session,
            ..
        } = &self.transport
        else {
            unreachable!()
        };
        let body = message.to_string();
        let mut req = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {token}\r\nMCP-Protocol-Version: 2025-06-18\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if let Some(s) = session {
            req.push_str(&format!("Mcp-Session-Id: {s}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&body);
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", *port)).unwrap();
        stream.set_read_timeout(Some(CALL_TIMEOUT)).unwrap();
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
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
        (status, headers, body)
    }

    /// Names of the tools the server offers.
    pub fn tools(&mut self) -> Vec<String> {
        let reply = self.request("tools/list", json!({}));
        reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Calls a tool. `Ok` holds the tool's text parsed as JSON (or as a JSON string when it is
    /// not JSON); `Err` the error text of a refused or failed call.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        let reply = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        if let Some(error) = reply.get("error") {
            return Err(error["message"].as_str().unwrap_or_default().to_owned());
        }
        let result = &reply["result"];
        let text: String = result["content"]
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
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// Calls a tool that must succeed.
    pub fn ok(&mut self, tool: &str, arguments: Value) -> Value {
        match self.call(tool, arguments.clone()) {
            Ok(v) => v,
            Err(e) => panic!("{tool} {arguments} failed: {e}"),
        }
    }

    /// Calls a tool that must be refused; returns the reason.
    pub fn refused(&mut self, tool: &str, arguments: Value) -> String {
        match self.call(tool, arguments.clone()) {
            Ok(v) => panic!("{tool} {arguments} should be refused, returned {v}"),
            Err(e) => e,
        }
    }
}

impl Drop for Ide {
    fn drop(&mut self) {
        match &mut self.transport {
            Transport::Stdio { child, stdin, .. } => {
                // Closing stdin ends the server; kill it if it lingers.
                drop(stdin.take());
                for _ in 0..50 {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            Transport::Http { server, .. } => {
                if let Some(s) = server.take() {
                    s.stop();
                }
            }
        }
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

/// Text of a JSON value for `contains` checks.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether a language-server tool failed only because no server is installed or ready here;
/// records the skip.
pub fn lsp_unavailable(result: &Result<Value, String>) -> bool {
    let Err(e) = result else { return false };
    let lower = e.to_lowercase();
    let missing = [
        "not installed",
        "no language server",
        "not found",
        "missing",
        "could not start",
        "unusable",
        "timed out",
    ]
    .iter()
    .any(|m| lower.contains(m));
    if missing {
        skip(&format!("language server unavailable: {e}"));
    }
    missing
}

/// Creates the `sql-basic` reference schema and seed data in connection `conn` (an empty SQLite
/// database from [`Project::sqlite`]) through `run_sql`, as a user runs the scripts.
pub fn load_reference_schema(ide: &mut Ide, conn: &str) {
    let dir = reference_projects().join("sql-basic");
    let script = std::fs::read_to_string(dir.join("schema.sql")).unwrap()
        + &std::fs::read_to_string(dir.join("seed.sql")).unwrap();
    let r = ide.ok(
        "run_sql",
        json!({"apply": true, "connection": conn, "sql": script}),
    );
    assert_eq!(r["not_run"], 0, "{r}");
}

/// Environment variable of the test password folder (SPEC-0008 behaviour 14; debug builds only).
pub const TEST_SECRETS_ENV: &str = "IDECRAFT_TEST_SECRETS_DIR";

/// Service of the reference databases' passwords in the test password folder.
pub const TEST_SECRET_SERVICE: &str = "idecraft";

/// Adds connection `name` for the reference database in environment variable `var`
/// (`IDECRAFT_TEST_PG`, `IDECRAFT_TEST_MYSQL`, `IDECRAFT_TEST_MARIADB`: URLs without password) with
/// reading rights only; `false` (and a recorded skip) when the variable is not set.
///
/// When the server needs a password (`<var>_PASSWORD` or the file named by `<var>_PASSWORD_FILE`,
/// set by `scripts/functional.sh` and `scripts/ci-local.sh`), it is copied into the project's test
/// password folder ([`Project::secrets`], 0700) as `idecraft/connection:<name>` (0600) — the
/// entry IDECraft keeps for the connection, the only one an agent's call uses (SPEC-0012 v3
/// behaviour 20) — and the profile refers to it, as a password typed into the connection dialog
/// would be. The processes
/// started for the project read it from there (`IDECRAFT_TEST_SECRETS_DIR`); nothing touches the
/// system keychain.
pub fn live_connection(p: &Project, name: &str, var: &str) -> bool {
    let Some(url) = live_db(var) else {
        return false;
    };
    let (scheme, rest) = url.split_once("://").expect("a URL");
    let (authority, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let (user, hostport) = authority.split_once('@').unwrap_or(("", authority));
    let (host, port) = hostport.split_once(':').unwrap_or((hostport, ""));
    let database = rest.split('?').next().unwrap_or("");
    let mut fields = format!(
        "dialect = \"{scheme}\"\nhost = \"{host}\"\ndatabase = \"{database}\"\nuser = \"{user}\"\nssl = \"disable\""
    );
    if !port.is_empty() {
        fields.push_str(&format!("\nport = {port}"));
    }
    if let Some(password) = test_password(var) {
        let account = format!("connection:{name}");
        p.store_secret(&account, &password);
        fields.push_str(&format!(
            "\nsecret = {{ service = \"{TEST_SECRET_SERVICE}\", account = \"{account}\" }}"
        ));
    }
    p.connection(name, &fields, &["read-schema", "read-data"]);
    true
}

impl Project {
    /// Writes `password` to `<secrets>/idecraft/<account>` with the modes the test store
    /// demands (folders 0700, file 0600).
    pub fn store_secret(&self, account: &str, password: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            let dir = self.secrets.join(TEST_SECRET_SERVICE);
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)
                .unwrap();
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(dir.join(account))
                .unwrap();
            f.write_all(password.as_bytes()).unwrap();
        }
        #[cfg(not(unix))]
        panic!(
            "the test password folder needs Unix file modes ({account}, {})",
            password.len()
        );
    }
}

/// `<var>_PASSWORD`, else the content of the file named by `<var>_PASSWORD_FILE`; `None` when the
/// server needs no password (GitHub's service containers). A named file that cannot be read fails.
fn test_password(var: &str) -> Option<String> {
    if let Ok(password) = std::env::var(format!("{var}_PASSWORD"))
        && !password.is_empty()
    {
        return Some(password);
    }
    let file = std::env::var(format!("{var}_PASSWORD_FILE")).ok()?;
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{var}_PASSWORD_FILE {file}: {e}"));
    Some(text.trim().to_owned())
}

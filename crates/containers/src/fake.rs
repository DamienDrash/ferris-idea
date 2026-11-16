//! Stand-ins for a Docker daemon and `kubectl`, for tests and the window self-test on machines
//! without either (`SPEC-0119` AK-2, AK-4). They answer like the real ones for what IDECraft
//! asks and keep the requests they saw.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A fake Docker Engine API on a Unix socket with two containers (`web` running, `db` exited)
/// and two images. Execs run in `web`: without TTY a command prints `ran: <command>` on stdout and
/// `note` on stderr and ends with the number after `exit` in it (else 0); with TTY the session greets
/// with `fake shell`, echoes what it reads and ends with `exit <n>` (`SPEC-0240` AK-2).
pub struct FakeEngine {
    /// Socket path.
    pub socket: PathBuf,
    /// Request lines seen, e.g. `POST /containers/web/stop?t=10`.
    pub requests: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone, Default)]
struct State {
    web: Option<&'static str>,
    db: &'static str,
    /// Exec id → (command, tty, exit code once ended).
    execs: Vec<(String, Vec<String>, bool, Option<i64>)>,
}

fn containers_json(s: &State) -> String {
    let mut items = Vec::new();
    if let Some(web) = s.web {
        items.push(format!(
            r#"{{"Id":"aaaaaaaaaaaa1111","Names":["/web"],"Image":"nginx:1.27","State":"{web}","Status":"{}","Ports":[{{"IP":"0.0.0.0","PrivatePort":80,"PublicPort":8080,"Type":"tcp"}}],"Labels":{{"com.docker.compose.project":"shop","com.docker.compose.service":"web"}}}}"#,
            if web == "running" { "Up 3 minutes" } else { "Exited (0) 1 second ago" }
        ));
    }
    items.push(format!(
        r#"{{"Id":"bbbbbbbbbbbb2222","Names":["/db"],"Image":"postgres:18","State":"{}","Status":"{}","Ports":[],"Labels":{{}}}}"#,
        s.db,
        if s.db == "running" { "Up 1 second" } else { "Exited (0) 2 hours ago" }
    ));
    format!("[{}]", items.join(","))
}

fn respond(stream: &mut impl Write, status: &str, body: &[u8], chunked: bool) {
    let _ = if chunked {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n"
        )
        .into_bytes();
        for part in body.chunks(7) {
            out.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
            out.extend_from_slice(part);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"0\r\n\r\n");
        stream.write_all(&out)
    } else {
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(body))
    };
}

fn frame(stream: u8, text: &[u8]) -> Vec<u8> {
    let mut out = vec![stream, 0, 0, 0];
    out.extend_from_slice(&u32::try_from(text.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(text);
    out
}

/// The number after `exit ` in a command, else 0.
fn exit_code(command: &str) -> i64 {
    command
        .split("exit ")
        .nth(1)
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// An upgraded TTY session: greet, echo, end at `exit <n>`.
fn tty_session(mut stream: std::os::unix::net::UnixStream, state: Arc<Mutex<State>>, id: String) {
    let _ = stream.write_all(
        b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.raw-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\nfake shell\r\n$ ",
    );
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    let code = loop {
        let n = match std::io::Read::read(&mut stream, &mut buf) {
            Ok(0) | Err(_) => break 0,
            Ok(n) => n,
        };
        let _ = stream.write_all(&buf[..n]);
        line.extend_from_slice(&buf[..n]);
        if let Some(end) = line.iter().position(|b| *b == b'\r' || *b == b'\n') {
            let text = String::from_utf8_lossy(&line[..end]).trim().to_owned();
            line.drain(..=end);
            if let Some(rest) = text.strip_prefix("exit") {
                break rest.trim().parse().unwrap_or(0);
            }
            let _ = stream.write_all(format!("\r\nyou said {text}\r\n$ ").as_bytes());
        }
    };
    if let Ok(mut s) = state.lock()
        && let Some(e) = s.execs.iter_mut().find(|e| e.0 == id)
    {
        e.3 = Some(code);
    }
}

impl FakeEngine {
    /// Starts the engine on `socket` (removed first) in a background thread.
    #[cfg(unix)]
    pub fn start(socket: &Path) -> std::io::Result<Self> {
        let _ = std::fs::remove_file(socket);
        let listener = std::os::unix::net::UnixListener::bind(socket)?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let state = Arc::new(Mutex::new(State {
            web: Some("running"),
            db: "exited",
            execs: Vec::new(),
        }));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                });
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut length = 0usize;
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    header.clear();
                }
                let mut body = vec![0u8; length];
                if length > 0 && std::io::Read::read_exact(&mut reader, &mut body).is_err() {
                    continue;
                }
                let body: serde_json::Value =
                    serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                let request: Vec<&str> = line.split_whitespace().collect();
                let (method, path) = (
                    request.first().copied().unwrap_or(""),
                    request.get(1).copied().unwrap_or(""),
                );
                if let Ok(mut r) = seen.lock() {
                    r.push(format!("{method} {path}"));
                }
                let Ok(mut st) = state.lock() else { break };
                if let Some(rest) = path.strip_prefix("/exec/") {
                    let id = rest.split(['/', '?']).next().unwrap_or("").to_owned();
                    let Some(exec) = st.execs.iter().find(|e| e.0 == id).cloned() else {
                        respond(
                            &mut stream,
                            "404 Not Found",
                            br#"{"message":"No such exec instance"}"#,
                            false,
                        );
                        continue;
                    };
                    if rest.contains("/start") {
                        if exec.2 {
                            drop(st);
                            let (state, id) = (Arc::clone(&state), id.clone());
                            std::thread::spawn(move || tty_session(stream, state, id));
                            continue;
                        }
                        let command = exec.1.join(" ");
                        let mut out = b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\n\r\n".to_vec();
                        out.extend(frame(1, format!("ran: {command}\n").as_bytes()));
                        out.extend(frame(2, b"note\n"));
                        let _ = stream.write_all(&out);
                        if let Some(e) = st.execs.iter_mut().find(|e| e.0 == id) {
                            e.3 = Some(exit_code(&command));
                        }
                    } else if rest.contains("/resize") {
                        respond(&mut stream, "201 Created", b"", false);
                    } else {
                        respond(
                            &mut stream,
                            "200 OK",
                            serde_json::json!({"Running": exec.3.is_none(), "ExitCode": exec.3.unwrap_or(0)})
                                .to_string()
                                .as_bytes(),
                            false,
                        );
                    }
                    continue;
                }
                let exists =
                    |name: &str, s: &State| name == "db" || (name == "web" && s.web.is_some());
                let target = path
                    .strip_prefix("/containers/")
                    .map(|p| p.split(['/', '?']).next().unwrap_or(""))
                    .unwrap_or("");
                match (method, path) {
                    ("GET", "/version") => respond(
                        &mut stream,
                        "200 OK",
                        br#"{"Version":"27.3.1","ApiVersion":"1.47","Os":"linux","Arch":"arm64"}"#,
                        false,
                    ),
                    ("GET", "/containers/json?all=1") => {
                        respond(&mut stream, "200 OK", containers_json(&st).as_bytes(), true)
                    }
                    ("GET", "/images/json") => respond(
                        &mut stream,
                        "200 OK",
                        br#"[{"Id":"sha256:cccccccccccc3333","RepoTags":["nginx:1.27"],"Size":192000000},{"Id":"sha256:dddddddddddd4444","RepoTags":["<none>:<none>"],"Size":5000}]"#,
                        false,
                    ),
                    _ if !exists(target, &st) => respond(
                        &mut stream,
                        "404 Not Found",
                        format!(r#"{{"message":"No such container: {target}"}}"#).as_bytes(),
                        false,
                    ),
                    ("POST", p) if p.ends_with("/exec") => {
                        let running = if target == "web" { st.web == Some("running") } else { st.db == "running" };
                        if !running {
                            respond(
                                &mut stream,
                                "409 Conflict",
                                format!(r#"{{"message":"container {target} is not running"}}"#).as_bytes(),
                                true,
                            );
                            continue;
                        }
                        let id = format!("e{}", st.execs.len() + 1);
                        let cmd: Vec<String> = body
                            .get("Cmd")
                            .and_then(serde_json::Value::as_array)
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                            .unwrap_or_default();
                        let tty = body.get("Tty").and_then(serde_json::Value::as_bool) == Some(true);
                        st.execs.push((id.clone(), cmd, tty, None));
                        respond(&mut stream, "201 Created", format!(r#"{{"Id":"{id}"}}"#).as_bytes(), false);
                    }
                    ("GET", p) if p.contains("/logs?") => {
                        let mut body = frame(1, b"GET / 200\n");
                        body.extend(frame(2, b"warn: slow\n"));
                        respond(&mut stream, "200 OK", &body, true)
                    }
                    ("POST", p) => {
                        let running = if target == "web" { st.web == Some("running") } else { st.db == "running" };
                        let start = p.contains("/start");
                        let stop = p.contains("/stop");
                        if (start && running) || (stop && !running) {
                            respond(&mut stream, "304 Not Modified", b"", false);
                            continue;
                        }
                        let new = if stop { "exited" } else { "running" };
                        if target == "web" {
                            st.web = Some(new);
                        } else {
                            st.db = new;
                        }
                        respond(&mut stream, "204 No Content", b"", false);
                    }
                    ("DELETE", _) => {
                        if target == "web" {
                            st.web = None;
                        }
                        respond(&mut stream, "204 No Content", b"", false);
                    }
                    _ => respond(&mut stream, "404 Not Found", br#"{"message":"page not found"}"#, false),
                }
            }
        });
        Ok(Self {
            socket: socket.to_path_buf(),
            requests,
        })
    }

    /// Request lines seen so far.
    pub fn seen(&self) -> Vec<String> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

/// Writes a stand-in for `ssh` into `dir` that answers `-Q kex` with a post-quantum method and
/// `-G` with a configuration without a jump host, ignores its other arguments and connects its
/// input and output to the Unix socket `socket` (macOS and BSD `nc -U`), like `docker system
/// dial-stdio`.
#[cfg(unix)]
pub fn ssh_script(dir: &Path, socket: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("ssh");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = -Q ]; then echo mlkem768x25519-sha256; exit 0; fi\n\
             for a in \"$@\"; do [ \"$a\" = -G ] && {{ echo 'hostname box'; exit 0; }}; done\n\
             echo \"$*\" >> '{}'\nexec nc -U '{}'\n",
            dir.join("ssh.log").display(),
            socket.display()
        ),
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

/// Writes a stand-in for the standalone `docker-compose` into `dir` that logs its arguments and
/// `DOCKER_HOST` to `dir/compose.log` and prints a line (`SPEC-0242` AK-2).
#[cfg(unix)]
pub fn compose_script(dir: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("docker-compose");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\necho \"DOCKER_HOST=$DOCKER_HOST $*\" >> '{}'\necho \"Container shop-web-1  Started\"\n",
            dir.join("compose.log").display()
        ),
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

/// Writes a fake `kubectl` shell script into `dir` that answers `config view`, `get namespaces`,
/// `get …` (resources), `logs`, `rollout restart`/`delete pod`, `exec` (exit code 2) and
/// `port-forward` (prints its forwarding line, then waits), logging each call to
/// `dir/kubectl.log`. Returns its path.
#[cfg(unix)]
pub fn kubectl_script(dir: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    let log = dir.join("kubectl.log");
    let script = format!(
        r#"#!/bin/sh
echo "$*" >> '{log}'
case "$*" in
  "config view -o json")
    echo '{{"current-context":"dev","contexts":[{{"name":"dev","context":{{"cluster":"kind-dev","namespace":"shop"}}}},{{"name":"prod","context":{{"cluster":"eks-prod"}}}}]}}' ;;
  "get namespaces -o json --context dev")
    echo '{{"items":[{{"metadata":{{"name":"shop"}}}},{{"metadata":{{"name":"default"}}}}]}}' ;;
  "get deployments,statefulsets,daemonsets,jobs,pods,services -o json --context dev --namespace shop")
    echo '{{"items":[
      {{"kind":"Service","metadata":{{"name":"web","namespace":"shop","creationTimestamp":"2026-11-01T10:00:00Z"}},"spec":{{"type":"ClusterIP","clusterIP":"10.96.0.7","ports":[{{"port":80,"protocol":"TCP"}}]}}}},
      {{"kind":"Pod","metadata":{{"name":"web-7d9","namespace":"shop","creationTimestamp":"2026-11-01T10:00:00Z"}},"spec":{{"containers":[{{"image":"nginx:1.27","ports":[{{"containerPort":80}}]}}]}},"status":{{"phase":"Running","containerStatuses":[{{"ready":true,"restartCount":2,"state":{{"running":{{}}}}}}]}}}},
      {{"kind":"Deployment","metadata":{{"name":"web","namespace":"shop","creationTimestamp":"2026-11-01T10:00:00Z"}},"spec":{{"replicas":2}},"status":{{"readyReplicas":1}}}}
    ]}}' ;;
  "get deployments,statefulsets,daemonsets,jobs,pods,services -o json --context prod --namespace default")
    echo 'error: You must be logged in to the server (Unauthorized)' >&2; exit 1 ;;
  "logs web-7d9 --all-containers=true --prefix=false --tail="*" --context dev --namespace shop")
    printf 'started on :80\nGET /healthz 200\n' ;;
  "rollout restart deployment/web --context dev --namespace shop")
    echo 'deployment.apps/web restarted' ;;
  "delete pod web-7d9 --wait=false --context dev --namespace shop")
    echo 'pod "web-7d9" deleted' ;;
  "exec web-7d9 --context dev --namespace shop -- "*)
    echo "in pod: $*"; echo 'pod note' >&2; exit 2 ;;
  "port-forward pod/web-7d9 "*" --address 127.0.0.1 --context dev --namespace shop")
    echo "Forwarding from 127.0.0.1:${{3%%:*}} -> 80"; exec sleep 600 ;;
  "port-forward pod/gone "*)
    echo 'error: pods "gone" not found' >&2; exit 1 ;;
  *) echo "error: unknown call: $*" >&2; exit 1 ;;
esac
"#,
        log = log.display()
    );
    let path = dir.join("kubectl");
    std::fs::write(&path, script)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

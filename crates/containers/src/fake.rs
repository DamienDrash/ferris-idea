//! Stand-ins for a Docker daemon and `kubectl`, for tests and the window self-test on machines
//! without either (`SPEC-0119` AK-2, AK-4). They answer like the real ones for what Ferris IDEA
//! asks and keep the requests they saw.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::logs::rfc3339;

/// A fake Docker Engine API on a Unix socket with two containers (`web` running, `db` exited)
/// and two images. Execs run in `web`: without TTY a command prints `ran: <command>` on stdout and
/// `note` on stderr and ends with the number after `exit` in it (else 0); with TTY the session greets
/// with `fake shell`, echoes what it reads and ends with `exit <n>` (`SPEC-0240` AK-2).
///
/// `SPEC-0119` v5: it streams `/events` and followed logs, answers inspect, keeps the start times
/// of each container (a restart adds one and writes `restarted`), and can change containers "from
/// outside" ([`FakeEngine::outside_create`], …) and go away and come back with its socket
/// ([`FakeEngine::go_away`], [`FakeEngine::come_back`]).
pub struct FakeEngine {
    /// Socket path.
    pub socket: PathBuf,
    /// Request lines seen, e.g. `POST /containers/web/stop?t=10`.
    pub requests: Arc<Mutex<Vec<String>>>,
    state: Arc<Mutex<State>>,
    /// The listener whose number this is serves; going away or coming back counts it up.
    generation: Arc<AtomicU64>,
}

/// A container of the fake.
#[derive(Clone)]
struct Fc {
    id: String,
    name: String,
    image: String,
    running: bool,
    ports: &'static str,
    labels: String,
    env: Vec<String>,
    /// Start times in nanoseconds.
    starts: Vec<i128>,
    /// Log lines: time, stream (1 or 2), text with its line end.
    logs: Vec<(i128, u8, String)>,
    /// Network it is attached to.
    network: String,
}

/// An image of the fake.
#[derive(Clone)]
struct Fi {
    id: String,
    tags: Vec<String>,
    digests: Vec<String>,
    size: u64,
    created: i64,
}

#[derive(Default)]
struct State {
    containers: Vec<Fc>,
    images: Vec<Fi>,
    /// Exec id → (command, tty, exit code once ended).
    execs: Vec<(String, Vec<String>, bool, Option<i64>)>,
    /// Connections following the event stream.
    event_subs: Vec<UnixStream>,
    /// Connections following a container's logs: name, timestamps wanted, connection.
    log_subs: Vec<(String, bool, UnixStream)>,
    /// Every event so far as its JSON line (for history queries with `until`).
    history: Vec<String>,
    /// Last time handed out, so times always grow.
    clock: i128,
}

fn now_nanos() -> i128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}

impl State {
    fn tick(&mut self) -> i128 {
        self.clock = now_nanos().max(self.clock + 1);
        self.clock
    }

    fn find(&mut self, name: &str) -> Option<&mut Fc> {
        self.containers
            .iter_mut()
            .find(|c| c.name == name || (name.len() >= 4 && c.id.starts_with(name)))
    }

    /// Sends an event to every follower and keeps it for history queries.
    fn emit(&mut self, kind: &str, action: &str, id: &str, name: &str) {
        let t = self.tick();
        let line = format!(
            "{{\"Type\":\"{kind}\",\"Action\":\"{action}\",\"Actor\":{{\"ID\":\"{id}\",\"Attributes\":{{\"name\":\"{name}\"}}}},\"scope\":\"local\",\"time\":{},\"timeNano\":{t}}}\n",
            t / 1_000_000_000
        );
        self.history.push(line.clone());
        let chunk = chunk(line.as_bytes());
        self.event_subs.retain_mut(|s| s.write_all(&chunk).is_ok());
    }

    /// Adds a log line to `name` and sends it to its followers.
    fn log(&mut self, name: &str, stream: u8, text: &str) {
        let t = self.tick();
        let text = format!("{text}\n");
        if let Some(c) = self.find(name) {
            c.logs.push((t, stream, text.clone()));
        }
        self.log_subs.retain_mut(|(n, ts, s)| {
            n != name
                || s.write_all(&chunk(&log_frame(stream, t, &text, *ts)))
                    .is_ok()
        });
    }

    /// Ends the followed logs of `name` (the engine ends them when the container stops).
    fn end_logs(&mut self, name: &str) {
        self.log_subs.retain_mut(|(n, _, s)| {
            if n == name {
                let _ = s.write_all(b"0\r\n\r\n");
                false
            } else {
                true
            }
        });
    }

    fn set_running(&mut self, name: &str, running: bool, restart: bool) {
        let t = self.tick();
        let Some(c) = self.find(name) else { return };
        let id = c.id.clone();
        let was = c.running;
        c.running = running;
        if running {
            c.starts.push(t);
        }
        if was && (!running || restart) {
            self.end_logs(name);
            self.emit("container", "die", &id, name);
        }
        if running {
            self.emit("container", "start", &id, name);
            if restart {
                self.emit("container", "restart", &id, name);
                self.log(name, 1, "restarted");
            }
        }
    }
}

fn chunk(body: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\r\n");
    out
}

fn log_frame(stream: u8, t: i128, text: &str, timestamps: bool) -> Vec<u8> {
    if timestamps {
        frame(stream, format!("{} {text}", rfc3339(t)).as_bytes())
    } else {
        frame(stream, text.as_bytes())
    }
}

fn container_json(c: &Fc, images: &[Fi]) -> String {
    let network = &c.network;
    let image_id = images
        .iter()
        .find(|i| i.tags.contains(&c.image))
        .map_or(String::new(), |i| i.id.clone());
    let status = match (c.running, c.name.as_str()) {
        (true, "web") => "Up 3 minutes".to_owned(),
        (true, _) => "Up 1 second".to_owned(),
        (false, "db") => "Exited (0) 2 hours ago".to_owned(),
        (false, _) => "Exited (0) 1 second ago".to_owned(),
    };
    format!(
        r#"{{"Id":"{}","Names":["/{}"],"Image":"{}","ImageID":"{image_id}","State":"{}","Status":"{status}","Ports":[{}],"Labels":{{{}}},"NetworkSettings":{{"Networks":{{"{network}":{{}}}}}}}}"#,
        c.id,
        c.name,
        c.image,
        if c.running { "running" } else { "exited" },
        c.ports,
        c.labels
    )
}

fn containers_json(s: &State) -> String {
    let items: Vec<String> = s
        .containers
        .iter()
        .map(|c| container_json(c, &s.images))
        .collect();
    format!("[{}]", items.join(","))
}

fn inspect_json(c: &Fc) -> String {
    let started = c
        .starts
        .last()
        .map_or("0001-01-01T00:00:00Z".to_owned(), |t| rfc3339(*t));
    let created = c
        .starts
        .first()
        .map_or(started.clone(), |t| rfc3339(*t - 1_000_000_000));
    let (ports, exposed) = if c.ports.is_empty() {
        ("{}".to_owned(), "{}".to_owned())
    } else {
        (
            r#"{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"}]}"#.to_owned(),
            r#"{"80/tcp":{}}"#.to_owned(),
        )
    };
    serde_json::json!({
        "Id": c.id,
        "Name": format!("/{}", c.name),
        "Created": created,
        "Args": [],
        "State": {
            "Status": if c.running { "running" } else { "exited" },
            "Running": c.running,
            "StartedAt": started,
            "ExitCode": 0,
        },
        "Config": {
            "Image": c.image,
            "Env": c.env,
            "Cmd": ["serve", "--port", "80"],
            "Labels": serde_json::from_str::<serde_json::Value>(&format!("{{{}}}", c.labels)).unwrap_or_default(),
            "ExposedPorts": serde_json::from_str::<serde_json::Value>(&exposed).unwrap_or_default(),
            "Tty": false,
        },
        "NetworkSettings": {
            "Ports": serde_json::from_str::<serde_json::Value>(&ports).unwrap_or_default(),
            "Networks": {"bridge": {"IPAddress": "172.17.0.2"}},
        },
        "Mounts": [{"Type": "volume", "Name": format!("{}-data", c.name), "Source": "/var/lib/docker/volumes/x/_data",
                    "Destination": "/data", "Mode": "z", "RW": true}],
    })
    .to_string()
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

/// The head of a chunked answer that stays open.
const STREAM_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";

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

/// The value of query parameter `key` in `path`.
fn query<'a>(path: &'a str, key: &str) -> Option<&'a str> {
    path.split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

/// An upgraded TTY session: greet, echo, end at `exit <n>`.
fn tty_session(mut stream: UnixStream, state: Arc<Mutex<State>>, id: String) {
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

/// The two containers every fake starts with.
fn first_containers(st: &mut State) {
    let t0 = st.tick() - 180_000_000_000;
    st.containers = vec![
        Fc {
            id: "aaaaaaaaaaaa1111".into(),
            name: "web".into(),
            image: "nginx:1.27".into(),
            running: true,
            ports: r#"{"IP":"0.0.0.0","PrivatePort":80,"PublicPort":8080,"Type":"tcp"}"#,
            labels: r#""com.docker.compose.project":"shop","com.docker.compose.service":"web""#
                .into(),
            env: vec![
                "GREETING=hello".into(),
                "DB_PASSWORD=fake-placeholder-pw".into(),
                "PATH=/usr/local/bin:/usr/bin".into(),
            ],
            starts: vec![t0],
            logs: vec![
                (t0 + 1_000_000, 1, "GET / 200\n".into()),
                (t0 + 2_000_000, 2, "warn: slow\n".into()),
            ],
            network: "shop_default".into(),
        },
        Fc {
            id: "bbbbbbbbbbbb2222".into(),
            name: "db".into(),
            image: "postgres:18".into(),
            running: false,
            ports: "",
            labels: String::new(),
            env: vec!["POSTGRES_USER=app".into()],
            starts: vec![t0 - 7_200_000_000_000],
            logs: vec![
                (t0 - 7_199_000_000_000, 1, "GET / 200\n".into()),
                (t0 - 7_198_000_000_000, 2, "warn: slow\n".into()),
            ],
            network: "bridge".into(),
        },
    ];
    st.images = vec![
        Fi {
            id: "sha256:cccccccccccc3333".into(),
            tags: vec!["nginx:1.27".into()],
            digests: vec![format!("nginx@sha256:{}", "c".repeat(64))],
            size: 192_000_000,
            created: 1_700_000_000,
        },
        Fi {
            id: "sha256:dddddddddddd4444".into(),
            tags: vec![],
            digests: vec![],
            size: 5000,
            created: 1_600_000_000,
        },
    ];
    // The engine's event history knows the first starts too.
    for c in st.containers.clone() {
        for t in c.starts {
            st.history.push(format!(
                "{{\"Type\":\"container\",\"Action\":\"start\",\"Actor\":{{\"ID\":\"{}\",\"Attributes\":{{\"name\":\"{}\"}}}},\"time\":{},\"timeNano\":{t}}}\n",
                c.id,
                c.name,
                t / 1_000_000_000
            ));
        }
    }
}

fn serve(
    listener: std::os::unix::net::UnixListener,
    state: Arc<Mutex<State>>,
    seen: Arc<Mutex<Vec<String>>>,
    generation: Arc<AtomicU64>,
    mine: u64,
) {
    for stream in listener.incoming() {
        if generation.load(Ordering::SeqCst) != mine {
            break;
        }
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
        if method == "GET" && path.starts_with("/events") {
            if query(path, "until").is_some() {
                // History: the start events of the container named in the filter.
                let text: String = st
                    .history
                    .iter()
                    .filter(|l| l.contains("\"Action\":\"start\""))
                    .filter(|l| {
                        st.containers
                            .iter()
                            .any(|c| path.contains(&c.id) && l.contains(&c.id))
                    })
                    .cloned()
                    .collect();
                respond(&mut stream, "200 OK", text.as_bytes(), true);
            } else if stream.write_all(STREAM_HEAD).is_ok() {
                st.event_subs.push(stream);
            }
            continue;
        }
        let target = path
            .strip_prefix("/containers/")
            .map(|p| p.split(['/', '?']).next().unwrap_or(""))
            .unwrap_or("")
            .to_owned();
        match (method, path) {
            ("GET", "/version") => respond(
                &mut stream,
                "200 OK",
                br#"{"Version":"27.3.1","ApiVersion":"1.47","Os":"linux","Arch":"arm64"}"#,
                false,
            ),
            ("GET", "/containers/json?all=1") => {
                let mut list: Vec<Fc> = st.containers.clone();
                list.sort_by(|a, b| (!a.running, &a.name).cmp(&(!b.running, &b.name)));
                let json = containers_json(&State {
                    containers: list,
                    images: st.images.clone(),
                    ..State::default()
                });
                respond(&mut stream, "200 OK", json.as_bytes(), true)
            }
            ("GET", "/images/json") => {
                let list: Vec<serde_json::Value> = st
                    .images
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "Id": i.id,
                            "RepoTags": if i.tags.is_empty() { vec!["<none>:<none>".to_owned()] } else { i.tags.clone() },
                            "Size": i.size,
                            "Created": i.created,
                        })
                    })
                    .collect();
                respond(
                    &mut stream,
                    "200 OK",
                    serde_json::json!(list).to_string().as_bytes(),
                    false,
                )
            }
            ("POST", p) if p.starts_with("/images/create?") => {
                let repo = query(p, "fromImage").unwrap_or("").replace("%2F", "/");
                let tag = query(p, "tag").unwrap_or("latest").replace("%3A", ":");
                let mut out = STREAM_HEAD.to_vec();
                if repo == "nope" {
                    out.extend(chunk(b"{\"errorDetail\":{\"message\":\"pull access denied for nope, repository does not exist\"},\"error\":\"pull access denied for nope, repository does not exist\"}\n"));
                } else {
                    let reference = format!("{repo}:{tag}");
                    for line in [
                        format!("{{\"status\":\"Pulling from library/{repo}\",\"id\":\"{tag}\"}}"),
                        "{\"status\":\"Pulling fs layer\",\"id\":\"a1b2c3d4e5f6\"}".to_owned(),
                        "{\"status\":\"Downloading\",\"progressDetail\":{\"current\":1,\"total\":2},\"progress\":\"[=====>     ] 1MB/2MB\",\"id\":\"a1b2c3d4e5f6\"}".to_owned(),
                        "{\"status\":\"Pull complete\",\"id\":\"a1b2c3d4e5f6\"}".to_owned(),
                        "{\"status\":\"Digest: sha256:1111111111111111111111111111111111111111111111111111111111111111\"}".to_owned(),
                        format!("{{\"status\":\"Status: Downloaded newer image for {reference}\"}}"),
                    ] {
                        out.extend(chunk(format!("{line}\n").as_bytes()));
                    }
                    if !st.images.iter().any(|i| i.tags.contains(&reference)) {
                        let n = st.images.len();
                        st.images.push(Fi {
                            id: format!("sha256:eeee{n:08x}5555"),
                            tags: vec![reference.clone()],
                            digests: vec![format!("{repo}@sha256:{}", "1".repeat(64))],
                            size: 4_400_000,
                            created: 1_700_000_000,
                        });
                    }
                    st.emit("image", "pull", &reference, &reference);
                }
                out.extend_from_slice(b"0\r\n\r\n");
                let _ = stream.write_all(&out);
            }
            (m, p) if p.starts_with("/images/") && (m == "GET" || m == "DELETE") => {
                let name = p
                    .trim_start_matches("/images/")
                    .trim_end_matches("/json")
                    .replace("%3A", ":")
                    .replace("%40", "@");
                let found = st.images.iter().position(|i| {
                    i.tags.contains(&name)
                        || (name.len() >= 4
                            && i.id
                                .trim_start_matches("sha256:")
                                .starts_with(name.trim_start_matches("sha256:")))
                });
                let Some(at) = found else {
                    respond(
                        &mut stream,
                        "404 Not Found",
                        format!(r#"{{"message":"No such image: {name}"}}"#).as_bytes(),
                        false,
                    );
                    continue;
                };
                let img = st.images[at].clone();
                if m == "GET" {
                    let json = serde_json::json!({
                        "Id": img.id, "RepoTags": img.tags, "RepoDigests": img.digests, "Size": img.size,
                        "Os": "linux", "Architecture": "arm64", "Variant": "v8",
                        "Created": crate::logs::rfc3339(i128::from(img.created) * 1_000_000_000),
                    });
                    respond(&mut stream, "200 OK", json.to_string().as_bytes(), false);
                    continue;
                }
                let user = st
                    .containers
                    .iter()
                    .find(|c| img.tags.contains(&c.image))
                    .map(|c| c.id.clone());
                if let Some(user) = user {
                    respond(&mut stream, "409 Conflict", format!(r#"{{"message":"conflict: unable to delete {name} (cannot be forced) - image is being used by running container {user}"}}"#).as_bytes(), false);
                    continue;
                }
                st.images.remove(at);
                let mut lines: Vec<serde_json::Value> = img
                    .tags
                    .iter()
                    .map(|t| serde_json::json!({"Untagged": t}))
                    .collect();
                lines.push(serde_json::json!({"Deleted": img.id}));
                st.emit("image", "delete", &img.id, &name);
                respond(
                    &mut stream,
                    "200 OK",
                    serde_json::json!(lines).to_string().as_bytes(),
                    false,
                );
            }
            _ if st.find(&target).is_none() => respond(
                &mut stream,
                "404 Not Found",
                format!(r#"{{"message":"No such container: {target}"}}"#).as_bytes(),
                false,
            ),
            ("GET", p) if p.ends_with("/json") => {
                let c = st.find(&target).cloned();
                if let Some(c) = c {
                    respond(&mut stream, "200 OK", inspect_json(&c).as_bytes(), false);
                }
            }
            ("POST", p) if p.ends_with("/exec") => {
                let running = st.find(&target).is_some_and(|c| c.running);
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
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let tty = body.get("Tty").and_then(serde_json::Value::as_bool) == Some(true);
                st.execs.push((id.clone(), cmd, tty, None));
                respond(
                    &mut stream,
                    "201 Created",
                    format!(r#"{{"Id":"{id}"}}"#).as_bytes(),
                    false,
                );
            }
            ("GET", p) if p.contains("/logs?") => {
                let timestamps = query(p, "timestamps") == Some("1");
                let follow = query(p, "follow") == Some("1");
                let since = query(p, "since")
                    .and_then(|s| s.parse::<i128>().ok())
                    .map(|s| s * 1_000_000_000);
                let tail = query(p, "tail").and_then(|t| t.parse::<usize>().ok());
                let Some(c) = st.find(&target).cloned() else {
                    continue;
                };
                let mut lines: Vec<&(i128, u8, String)> = c
                    .logs
                    .iter()
                    .filter(|(t, _, _)| since.is_none_or(|s| *t >= s))
                    .collect();
                if let Some(n) = tail {
                    lines.drain(..lines.len().saturating_sub(n));
                }
                let body: Vec<u8> = lines
                    .iter()
                    .flat_map(|(t, s, text)| log_frame(*s, *t, text, timestamps))
                    .collect();
                if follow && c.running {
                    let mut out = STREAM_HEAD.to_vec();
                    if !body.is_empty() {
                        out.extend(chunk(&body));
                    }
                    if stream.write_all(&out).is_ok() {
                        st.log_subs.push((c.name.clone(), timestamps, stream));
                    }
                } else {
                    respond(&mut stream, "200 OK", &body, true)
                }
            }
            ("POST", p) => {
                let running = st.find(&target).is_some_and(|c| c.running);
                let start = p.contains("/start");
                let stop = p.contains("/stop");
                if (start && running) || (stop && !running) {
                    respond(&mut stream, "304 Not Modified", b"", false);
                    continue;
                }
                let restart = p.contains("/restart");
                st.set_running(&target, !stop, restart && running);
                respond(&mut stream, "204 No Content", b"", false);
            }
            ("DELETE", _) => {
                st.outside_remove(&target);
                respond(&mut stream, "204 No Content", b"", false);
            }
            _ => respond(
                &mut stream,
                "404 Not Found",
                br#"{"message":"page not found"}"#,
                false,
            ),
        }
    }
}

impl State {
    fn outside_remove(&mut self, name: &str) {
        let Some(c) = self.find(name).cloned() else {
            return;
        };
        if c.running {
            self.end_logs(&c.name);
            self.emit("container", "die", &c.id, &c.name);
        }
        self.containers.retain(|x| x.name != c.name);
        self.emit("container", "destroy", &c.id, &c.name);
    }
}

impl FakeEngine {
    /// Starts the engine on `socket` (removed first) in a background thread.
    #[cfg(unix)]
    pub fn start(socket: &Path) -> std::io::Result<Self> {
        let _ = std::fs::remove_file(socket);
        let listener = std::os::unix::net::UnixListener::bind(socket)?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut st = State::default();
        first_containers(&mut st);
        let state = Arc::new(Mutex::new(st));
        let generation = Arc::new(AtomicU64::new(0));
        let (s, r, g) = (
            Arc::clone(&state),
            Arc::clone(&requests),
            Arc::clone(&generation),
        );
        std::thread::spawn(move || serve(listener, s, r, g, 0));
        Ok(Self {
            socket: socket.to_path_buf(),
            requests,
            state,
            generation,
        })
    }

    /// Request lines seen so far.
    pub fn seen(&self) -> Vec<String> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> Option<T> {
        self.state.lock().ok().map(|mut s| f(&mut s))
    }

    /// Someone else creates and starts container `name` from `image` (`docker run` on the
    /// command line).
    pub fn outside_create(&self, name: &str, image: &str) {
        self.with(|st| {
            let n = st.containers.len() + st.history.len();
            let id = format!("ffff{n:08x}{:04x}", name.len());
            st.containers.push(Fc {
                id: id.clone(),
                name: name.into(),
                image: image.into(),
                running: false,
                ports: "",
                labels: String::new(),
                env: Vec::new(),
                starts: Vec::new(),
                logs: Vec::new(),
                network: "bridge".into(),
            });
            st.emit("container", "create", &id, name);
            st.set_running(name, true, false);
        });
    }

    /// Someone else runs container `name` of service `service` of Compose project `project`,
    /// started from folder `dir` (its labels and its project network as Compose sets them).
    pub fn outside_compose(
        &self,
        project: &str,
        service: &str,
        name: &str,
        image: &str,
        dir: &str,
    ) {
        self.with(|st| {
            let id = format!("cccc{:08x}{:04x}", st.containers.len() + st.history.len(), name.len());
            st.containers.push(Fc {
                id: id.clone(),
                name: name.into(),
                image: image.into(),
                running: false,
                ports: "",
                labels: format!(
                    r#""com.docker.compose.project":"{project}","com.docker.compose.service":"{service}","com.docker.compose.project.working_dir":"{dir}""#
                ),
                env: Vec::new(),
                starts: Vec::new(),
                logs: Vec::new(),
                network: format!("{project}_default"),
            });
            st.emit("container", "create", &id, name);
            st.set_running(name, true, false);
        });
    }

    /// Someone else starts or stops container `name`.
    pub fn outside_set(&self, name: &str, running: bool) {
        self.with(|st| st.set_running(name, running, false));
    }

    /// Someone else restarts container `name` (it writes `restarted` in its new run).
    pub fn outside_restart(&self, name: &str) {
        self.with(|st| st.set_running(name, true, true));
    }

    /// Someone else removes container `name`.
    pub fn outside_remove(&self, name: &str) {
        self.with(|st| st.outside_remove(name));
    }

    /// Container `name` writes a line (stream 1 stdout, 2 stderr).
    pub fn log_line(&self, name: &str, stream: u8, text: &str) {
        self.with(|st| st.log(name, stream, text));
    }

    /// Number of connections following the event stream.
    pub fn event_followers(&self) -> usize {
        self.with(|st| st.event_subs.len()).unwrap_or(0)
    }

    /// The engine goes away: its socket disappears and every open stream ends.
    pub fn go_away(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        // Wake the accepting thread so it sees the new number and drops the listener.
        let _ = UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
        self.with(|st| {
            st.event_subs.clear();
            st.log_subs.clear();
        });
    }

    /// The engine is back on the same socket with the same containers.
    #[cfg(unix)]
    pub fn come_back(&self) -> std::io::Result<()> {
        let _ = std::fs::remove_file(&self.socket);
        let listener = std::os::unix::net::UnixListener::bind(&self.socket)?;
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (s, r, g) = (
            Arc::clone(&self.state),
            Arc::clone(&self.requests),
            Arc::clone(&self.generation),
        );
        std::thread::spawn(move || serve(listener, s, r, g, mine));
        Ok(())
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
/// `DOCKER_HOST` to `dir/compose.log`, prints its progress on stderr as Compose does, one line twice as
/// Compose repeats it without a terminal (`SPEC-0242` AK-2).
#[cfg(unix)]
pub fn compose_script(dir: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("docker-compose");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\necho \"DOCKER_HOST=$DOCKER_HOST $*\" >> '{}'\necho ' Network shop_default Creating' >&2\necho ' Network shop_default Creating' >&2\necho ' Network shop_default Created' >&2\necho \"Container shop-web-1  Started\" >&2\n",
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

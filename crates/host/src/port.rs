//! `port.*` (`SPEC-0318` behaviours 25–27, `ADR-0025` §6): ports that programs started by the
//! backend listen on are offered; a forward the user asked for relays each local connection over
//! the session to the target on the host. The backend never opens a listener for this, on either
//! machine: the window's end listens on loopback, and the backend only connects on the host.
//! Debug ports are refused (the debugger uses the session), database ports need the user's
//! confirmation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use ferris_idea_session::{Backend, ErrorCode, Right, RpcError, StreamIn};
use serde_json::{Value, json};

use crate::{HostOptions, add, invalid, number};

/// Ports of programs that wait for a debugger (`ADR-0025` §6).
pub const DEBUG_PORTS: [u16; 3] = [5005, 5678, 9229];

/// Usual database ports (`ADR-0025` §6: forwarded only after a confirmed warning).
pub const DATABASE_PORTS: [u16; 6] = [5432, 3306, 1433, 1521, 6379, 27017];

/// Words in a command line that mark a debug adapter or a debuggee waiting for a debugger.
const DEBUG_WORDS: [&str; 7] = [
    "jdwp",
    "debugpy",
    "lldb-server",
    "gdbserver",
    "--inspect",
    "dlv",
    "ptvsd",
];

/// One listening socket on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listening {
    /// Port.
    pub port: u16,
    /// Address listened on (`127.0.0.1`, `0.0.0.0`, `::` …).
    pub address: String,
    /// Process.
    pub pid: u32,
    /// Its command line.
    pub command: String,
}

/// TCP ports of debug adapters this backend started (`SPEC-0322` AK-4). Their command line need
/// not name a debugger (java-debug runs inside jdtls), and the port list may not have seen them.
static ADAPTER_PORTS: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());

/// Marks `port` as a debug adapter's while it is open.
pub(crate) fn hold_adapter_port(port: u16) {
    ADAPTER_PORTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(port);
}

/// The adapter on `port` has ended.
pub(crate) fn release_adapter_port(port: u16) {
    ADAPTER_PORTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&port);
}

fn is_adapter_port(port: u16) -> bool {
    ADAPTER_PORTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&port)
}

/// `debug`, `database` or `service` (behaviour 25).
pub fn kind(port: u16, command: &str) -> &'static str {
    let lower = command.to_lowercase();
    if DEBUG_WORDS.iter().any(|w| lower.contains(w))
        || DEBUG_PORTS.contains(&port)
        || is_adapter_port(port)
    {
        "debug"
    } else if DATABASE_PORTS.contains(&port) {
        "database"
    } else {
        "service"
    }
}

/// Process ids and parents with command lines, from `ps`.
fn processes() -> HashMap<u32, (u32, String)> {
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "command="])
        .output();
    let mut map = HashMap::new();
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let mut it = line.split_whitespace();
            let (Some(pid), Some(ppid)) = (it.next(), it.next()) else {
                continue;
            };
            if let (Ok(pid), Ok(ppid)) = (pid.parse(), ppid.parse()) {
                map.insert(pid, (ppid, it.collect::<Vec<_>>().join(" ")));
            }
        }
    }
    map
}

/// The processes below `root` (not `root` itself).
fn descendants(tree: &HashMap<u32, (u32, String)>, root: u32) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (pid, (ppid, _)) in tree {
            if !out.contains(pid) && (*ppid == root || out.contains(ppid)) && *pid != root {
                out.insert(*pid);
                changed = true;
            }
        }
    }
    out
}

/// Listening TCP sockets of `pids` (`lsof`; on Linux without it, `/proc/net/tcp*`).
fn listening(pids: &HashSet<u32>) -> Vec<(u32, String, u16)> {
    if pids.is_empty() {
        return Vec::new();
    }
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    let out = std::process::Command::new("lsof")
        .args([
            "-nP",
            "-a",
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fpn",
            "-p",
            &list.join(","),
        ])
        .output();
    if let Ok(o) = out {
        let mut found = Vec::new();
        let mut pid = 0;
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            if let Some(p) = line.strip_prefix('p') {
                pid = p.parse().unwrap_or(0);
            } else if let Some(n) = line.strip_prefix('n')
                && let Some((addr, port)) = n.rsplit_once(':')
                && let Ok(port) = port.parse()
            {
                found.push((pid, addr.trim_matches(['[', ']']).to_owned(), port));
            }
        }
        return found;
    }
    proc_listening(pids)
}

#[cfg(target_os = "linux")]
fn proc_listening(pids: &HashSet<u32>) -> Vec<(u32, String, u16)> {
    let mut inodes: HashMap<String, u32> = HashMap::new();
    for pid in pids {
        if let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
            for e in dir.flatten() {
                if let Ok(target) = std::fs::read_link(e.path()) {
                    let t = target.to_string_lossy();
                    if let Some(inode) =
                        t.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']'))
                    {
                        inodes.insert(inode.to_owned(), *pid);
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 || f[3] != "0A" {
                continue;
            }
            let Some(pid) = inodes.get(f[9]) else {
                continue;
            };
            let Some((addr, port)) = f[1].split_once(':') else {
                continue;
            };
            let Ok(port) = u16::from_str_radix(port, 16) else {
                continue;
            };
            let address = if v6 {
                if addr.chars().all(|c| c == '0') {
                    "::".to_owned()
                } else {
                    "::1".to_owned()
                }
            } else {
                let n = u32::from_str_radix(addr, 16).unwrap_or(0).swap_bytes();
                std::net::Ipv4Addr::from(n).to_string()
            };
            found.push((*pid, address, port));
        }
    }
    found
}

#[cfg(not(target_os = "linux"))]
fn proc_listening(_pids: &HashSet<u32>) -> Vec<(u32, String, u16)> {
    Vec::new()
}

/// The command lines of `pids` now, from `ps`.
fn commands_of(pids: &HashSet<u32>) -> HashMap<u32, String> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    let out = std::process::Command::new("ps")
        .args(["-o", "pid=", "-o", "command=", "-p", &list.join(",")])
        .output();
    let mut map = HashMap::new();
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let line = line.trim_start();
            let (pid, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            if let Ok(pid) = pid.parse() {
                map.insert(pid, rest.trim().to_owned());
            }
        }
    }
    map
}

/// The command a listening process is named by: the one read after its socket was found, else the
/// one of the earlier process list. A process that runs `exec` between the two reads (a shell
/// starting `python3 -m http.server`) would otherwise keep its old name, and the port's kind, which
/// follows from the command, could be wrong (a debug port offered as a service).
fn listener_command(
    pid: u32,
    fresh: &HashMap<u32, String>,
    tree: &HashMap<u32, (u32, String)>,
) -> String {
    fresh
        .get(&pid)
        .filter(|c| !c.is_empty())
        .cloned()
        .or_else(|| tree.get(&pid).map(|(_, c)| c.clone()))
        .unwrap_or_default()
}

/// Ports that programs started by this backend (terminals, runs and their children) listen on.
pub fn detect() -> Vec<Listening> {
    let tree = processes();
    let mine = descendants(&tree, std::process::id());
    let found = listening(&mine);
    let fresh = commands_of(&found.iter().map(|(pid, ..)| *pid).collect());
    let mut by_port: BTreeMap<u16, Listening> = BTreeMap::new();
    for (pid, address, port) in found {
        let command = listener_command(pid, &fresh, &tree);
        by_port.entry(port).or_insert(Listening {
            port,
            address,
            pid,
            command,
        });
    }
    by_port.into_values().collect()
}

fn listening_json(l: &Listening) -> Value {
    // The command line is shortened to its program; arguments may hold secrets (SEC-10).
    let program = l
        .command
        .split_whitespace()
        .next()
        .and_then(|p| p.rsplit('/').next())
        .unwrap_or_default();
    json!({
        "port": l.port, "address": l.address, "pid": l.pid, "command": program,
        "kind": kind(l.port, &l.command),
    })
}

struct Forward {
    owner: String,
    port: u16,
    target: String,
    kind: &'static str,
    connections: Mutex<Vec<TcpStream>>,
}

#[derive(Default)]
struct Forwards {
    map: Mutex<HashMap<String, Arc<Forward>>>,
    next: AtomicU64,
    detected: Mutex<Vec<Listening>>,
}

impl Forwards {
    fn close(&self, id: &str) -> bool {
        let f = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(f) = &f {
            for c in f
                .connections
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .drain(..)
            {
                let _ = c.shutdown(Shutdown::Both);
            }
        }
        f.is_some()
    }
}

/// Binds `port.*` and starts watching for listening ports.
pub fn bind(backend: &Backend, options: &HostOptions) {
    let forwards = Arc::new(Forwards::default());
    let f = Arc::clone(&forwards);
    add(
        backend,
        "port",
        "port.list",
        Right::View,
        false,
        move |call| {
            let detected: Vec<Value> = f
                .detected
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .map(listening_json)
                .collect();
            let map = f.map.lock().unwrap_or_else(|e| e.into_inner());
            let mut fw: Vec<Value> = map
            .iter()
            .filter(|(_, x)| x.owner == call.session.id())
            .map(|(id, x)| json!({ "forward": id, "port": x.port, "target": x.target, "kind": x.kind }))
            .collect();
            fw.sort_by_key(|v| v["forward"].as_str().unwrap_or_default().to_owned());
            Ok(json!({ "detected": detected, "forwards": fw }))
        },
    );
    let f = Arc::clone(&forwards);
    add(
        backend,
        "port",
        "port.forward",
        Right::Ports,
        true,
        move |call| {
            let port = number(&call.params, "port")?;
            let port = u16::try_from(port)
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| invalid("`port` is not a TCP port"))?;
            let host = call.params["target_host"]
                .as_str()
                .unwrap_or("127.0.0.1")
                .trim();
            if host.is_empty() || host.contains(['/', ' ', '@']) {
                return Err(invalid("`target_host` is not a host name or address"));
            }
            let command = f
                .detected
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|l| l.port == port)
                .map(|l| l.command.clone())
                .unwrap_or_default();
            let k = kind(port, &command);
            if k == "debug" {
                return Err(RpcError::new(
                    ErrorCode::Denied,
                    format!("port {port} belongs to a debugger or debuggee; it is not forwarded"),
                )
                .hint(
                    "debug ports are reached by the debugger through the session (ADR-0025 §6)",
                ));
            }
            if k == "database" && call.params["confirm"].as_bool() != Some(true) {
                return Err(RpcError::new(
                ErrorCode::Denied,
                format!("port {port} looks like a database: a forwarded port can be reached by every user of this computer"),
            )
            .hint("confirm the forward after this warning, or use the database tools, which go through the session"));
            }
            let id = format!("f{}", f.next.fetch_add(1, Ordering::SeqCst) + 1);
            let target = format!("{host}:{port}");
            f.map.lock().unwrap_or_else(|e| e.into_inner()).insert(
                id.clone(),
                Arc::new(Forward {
                    owner: call.session.id().to_owned(),
                    port,
                    target: target.clone(),
                    kind: k,
                    connections: Mutex::new(Vec::new()),
                }),
            );
            let (f2, id2) = (Arc::clone(&f), id.clone());
            call.session.on_end(Box::new(move || {
                f2.close(&id2);
            }));
            Ok(json!({ "forward": id, "port": port, "target": target, "kind": k }))
        },
    );
    let f = Arc::clone(&forwards);
    add(
        backend,
        "port",
        "port.connect",
        Right::Ports,
        false,
        move |call| {
            let id = crate::text(&call.params, "forward")?;
            let fw = f
                .map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned()
                .filter(|x| x.owner == call.session.id())
                .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no forward {id}")))?;
            let addr = fw
                .target
                .to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .ok_or_else(|| {
                    RpcError::new(
                        ErrorCode::NotFound,
                        format!("{} cannot be resolved", fw.target),
                    )
                })?;
            let socket =
                TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| {
                    RpcError::new(
                        ErrorCode::Unavailable,
                        format!("{} on the host: {e}", fw.target),
                    )
                })?;
            let out = call.session.open_stream().ok_or_else(|| {
                RpcError::new(ErrorCode::Unavailable, "the session has no connection now")
            })?;
            let stream = out.id();
            if let Ok(c) = socket.try_clone() {
                fw.connections
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(c);
            }
            // Window → target: through a queue, so a slow target never blocks the session's reader.
            let (tx, rx) = mpsc::sync_channel::<Option<Vec<u8>>>(256);
            let mut writer = socket.try_clone().map_err(crate::internal)?;
            std::thread::spawn(move || {
                while let Ok(Some(bytes)) = rx.recv() {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                }
                let _ = writer.shutdown(Shutdown::Write);
            });
            call.session.on_input(
                stream,
                Arc::new(move |i| match i {
                    StreamIn::Data(b) => {
                        let _ = tx.send(Some(b));
                    }
                    StreamIn::Closed(_) => {
                        let _ = tx.send(None);
                    }
                }),
            );
            // Target → window, with credit.
            let mut reader = socket;
            let session = call.session.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 32 << 10];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if !out.write(&buf[..n]) {
                                break;
                            }
                        }
                    }
                }
                out.close("done");
                session.drop_input(stream);
                let _ = reader.shutdown(Shutdown::Both);
            });
            Ok(json!({ "forward": id, "stream": stream }))
        },
    );
    let f = Arc::clone(&forwards);
    add(
        backend,
        "port",
        "port.close",
        Right::Ports,
        true,
        move |call| {
            let id = crate::text(&call.params, "forward")?;
            let mine = f
                .map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .is_some_and(|x| x.owner == call.session.id());
            if !mine || !f.close(id) {
                return Err(RpcError::new(
                    ErrorCode::NotFound,
                    format!("no forward {id}"),
                ));
            }
            Ok(json!({ "forward": id, "closed": true }))
        },
    );
    watch(backend, options, &forwards);
}

/// Announces new and closed listening ports to every session (behaviour 25).
fn watch(backend: &Backend, options: &HostOptions, forwards: &Arc<Forwards>) {
    let weak = backend.downgrade();
    let every = options.poll;
    let forwards = Arc::clone(forwards);
    std::thread::Builder::new()
        .name("host-port-watch".into())
        .spawn(move || {
            loop {
                std::thread::sleep(every);
                let Some(b) = weak.upgrade() else { break };
                if b.sessions() == 0 {
                    continue;
                }
                let now = detect();
                let before = std::mem::replace(
                    &mut *forwards.detected.lock().unwrap_or_else(|e| e.into_inner()),
                    now.clone(),
                );
                let old: HashSet<u16> = before.iter().map(|l| l.port).collect();
                let new: HashSet<u16> = now.iter().map(|l| l.port).collect();
                for l in now.iter().filter(|l| !old.contains(&l.port)) {
                    b.broadcast("port.detected", listening_json(l));
                }
                for p in old.difference(&new) {
                    b.broadcast("port.gone", json!({ "port": p }));
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flake of 2026-11-13 (`ports_are_offered_forwarded_on_loopback_only_and_debug_ports_refused`):
    /// the command read after the socket was found names the listener; the earlier list only
    /// stands in when the process is gone.
    #[test]
    fn the_listener_is_named_by_its_command_after_exec() {
        let tree: HashMap<u32, (u32, String)> = [(7, (1, "it-d3c80a870b103590".to_owned()))].into();
        let fresh: HashMap<u32, String> = [(7, "python3 -m http.server 40961".to_owned())].into();
        assert_eq!(
            listener_command(7, &fresh, &tree),
            "python3 -m http.server 40961"
        );
        assert_eq!(
            listener_command(7, &HashMap::new(), &tree),
            "it-d3c80a870b103590"
        );
        assert_eq!(listener_command(8, &fresh, &tree), "");
        // The kind follows the fresh command: a debugger is not offered as a service.
        let node: HashMap<u32, String> = [(7, "node --inspect=9230 app.js".to_owned())].into();
        assert_eq!(kind(9230, &listener_command(7, &node, &tree)), "debug");
    }

    /// The fresh read covers the process asked for (this test process itself).
    #[test]
    fn commands_are_read_for_the_given_processes() {
        let me = std::process::id();
        let got = commands_of(&[me].into());
        assert!(got.get(&me).is_some_and(|c| !c.is_empty()), "{got:?}");
        assert!(commands_of(&HashSet::new()).is_empty());
    }

    #[test]
    fn debug_and_database_ports_are_told_apart() {
        assert_eq!(kind(8080, "python3 -m http.server 8080"), "service");
        assert_eq!(
            kind(8000, "java -agentlib:jdwp=transport=dt_socket,server=y"),
            "debug"
        );
        assert_eq!(kind(5678, "python3 app.py"), "debug");
        assert_eq!(kind(4000, "node --inspect=4000 app.js"), "debug");
        assert_eq!(kind(5432, "postgres -D data"), "database");
    }

    #[test]
    fn descendants_follow_the_tree() {
        let tree: HashMap<u32, (u32, String)> = [
            (10, (1, "serve".into())),
            (11, (10, "sh".into())),
            (12, (11, "python3".into())),
            (13, (1, "other".into())),
        ]
        .into_iter()
        .collect();
        let d = descendants(&tree, 10);
        assert_eq!(d, [11, 12].into_iter().collect());
    }
}

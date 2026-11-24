//! `dap.*` (`SPEC-0322`, `SPEC-0318` behaviour 28): debug adapters on the host. Whatever the
//! adapter's own transport (standard input and output, a Unix socket, a loopback port, jdtls), the
//! window gets one session stream with unchanged DAP messages; no port of an adapter or debuggee
//! is opened on the window's machine (`ADR-0025` §6). Output is kept so that a window can reattach
//! after a break without losing or repeating a byte (behaviour 7).

mod target;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use ferris_idea_languages::java_debug::{DebugEndpoint, JavaDebugServer};
use ferris_idea_session::{Backend, Call, ErrorCode, Right, RpcError, SessionHandle, StreamIn};
use serde_json::{Value, json};

use crate::ring::Output;
use crate::{HostOptions, add, internal, invalid, text};

pub use target::{Resolved, docker_target, forwarding_line, kubectl_args};

/// How long an adapter or `kubectl` may take to say that it listens.
const LISTEN: Duration = Duration::from_secs(30);

/// How an adapter is reached on the host (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Stdio,
    Socket,
    Tcp,
    Jdtls,
}

impl Transport {
    fn name(self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Socket => "socket",
            Transport::Tcp => "tcp",
            Transport::Jdtls => "jdtls",
        }
    }

    fn of(adapter: &str) -> Self {
        match adapter {
            "js-debug" => Transport::Socket,
            // Delve listens on a Unix socket given as `--listen=unix:…`; Windows has none.
            "dlv" if cfg!(unix) => Transport::Socket,
            "dlv" => Transport::Tcp,
            "java-debug" => Transport::Jdtls,
            _ => Transport::Stdio,
        }
    }
}

struct Known {
    name: String,
    transport: Transport,
    command: Option<Vec<String>>,
}

/// The adapters of this host.
fn adapters(options: &HostOptions) -> Vec<Known> {
    let mut out: Vec<Known> = ferris_idea_debugger::known_adapters()
        .into_iter()
        .map(|a| Known {
            name: a.name.to_owned(),
            transport: Transport::of(a.name),
            command: a.command,
        })
        .collect();
    out.push(Known {
        name: "java-debug".into(),
        transport: Transport::Jdtls,
        command: ferris_idea_languages::java_debug::java_debug_plugin()
            .map(|p| vec![p.display().to_string()]),
    });
    for (id, cmd) in &options.extra_adapters {
        out.push(Known {
            name: id.clone(),
            transport: Transport::Stdio,
            command: Some(cmd.clone()),
        });
    }
    for (id, cmd) in &options.extra_tcp_adapters {
        out.push(Known {
            name: id.clone(),
            transport: Transport::Tcp,
            command: Some(cmd.clone()),
        });
    }
    out
}

/// What belongs to one started adapter: its process, its socket folder, the tunnel to its target,
/// its jdtls. Ends once, with the session or `dap.stop`.
#[derive(Default)]
struct Group {
    child: Mutex<Option<Child>>,
    /// Ends the adapter and what it started should this process end without ending them
    /// (`SPEC-0263` v2 behaviour 3).
    guard: Mutex<Option<ferris_idea_debugger::lifetime::ExitGuard>>,
    tunnel: Mutex<Option<Child>>,
    folder: Mutex<Option<PathBuf>>,
    /// The adapter's TCP port on the host, which `port.forward` refuses while it is open.
    port: Mutex<Option<u16>>,
    java: Mutex<Option<JavaDebugServer>>,
    connect: Mutex<Option<Connect>>,
}

impl Group {
    /// Records the adapter's port on the host: it is never forwarded (`ADR-0025` §6, AK-4).
    fn hold_port(&self, port: u16) {
        crate::port::hold_adapter_port(port);
        *self.port.lock().unwrap_or_else(|e| e.into_inner()) = Some(port);
    }

    /// Keeps the adapter's process, guarded so it never outlives this process.
    fn keep_child(&self, child: Child) {
        *self.guard.lock().unwrap_or_else(|e| e.into_inner()) =
            ferris_idea_debugger::lifetime::ExitGuard::watch(child.id());
        *self.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    }

    fn end(&self) {
        drop(self.guard.lock().unwrap_or_else(|e| e.into_inner()).take());
        // The adapter with what it started (js-debug's debugged program).
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            ferris_idea_debugger::lifetime::end_tree(&mut c, Duration::from_secs(2));
        }
        if let Some(mut c) = self.tunnel.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.connect
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.java.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(port) = self.port.lock().unwrap_or_else(|e| e.into_inner()).take() {
            crate::port::release_adapter_port(port);
        }
        if let Some(dir) = self.folder.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

type Link = (Box<dyn Read + Send>, Box<dyn Write + Send>);
type Connect = Box<dyn Fn() -> Result<Link, RpcError> + Send>;
/// What happens when an adapter connection ends: whether to wait for the process (a stdio
/// adapter ends with it; a server does not) and the report.
type Exit = (bool, Box<dyn FnOnce(&str, Option<i32>) + Send>);

/// One connection to an adapter.
struct Channel {
    owner: String,
    group: Arc<Group>,
    output: Output,
    input: mpsc::SyncSender<Vec<u8>>,
    /// Bytes from the window handed to the adapter, for an exact resend (behaviour 7).
    received: Arc<AtomicU64>,
}

#[derive(Default)]
struct Channels {
    map: Mutex<HashMap<String, Arc<Channel>>>,
    next: AtomicU64,
}

impl Channels {
    fn get(&self, call: &Call) -> Result<(String, Arc<Channel>), RpcError> {
        let id = text(&call.params, "session")?.to_owned();
        let c = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .filter(|x| x.owner == call.session.id())
            .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no debug session {id}")))?;
        Ok((id, c))
    }
}

/// Starts the threads that relay `link` to a new stream of the session. `exit` runs when the
/// adapter side ends (it reports `dap.exited` for an adapter process).
fn relay(
    call: &Call,
    channels: &Arc<Channels>,
    group: &Arc<Group>,
    link: Link,
    exit: Option<Exit>,
) -> Result<(String, u32), RpcError> {
    let (mut reader, mut writer) = link;
    let out = call.session.open_stream().ok_or_else(|| {
        RpcError::new(ErrorCode::Unavailable, "the session has no connection now")
    })?;
    let stream = out.id();
    let id = format!("d{}", channels.next.fetch_add(1, Ordering::SeqCst) + 1);
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(256);
    let output = Output::default();
    let channel = Arc::new(Channel {
        owner: call.session.id().to_owned(),
        group: Arc::clone(group),
        output: output.clone(),
        input: tx,
        received: Arc::default(),
    });
    channels
        .map
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.clone(), Arc::clone(&channel));
    std::thread::spawn(move || {
        while let Ok(b) = rx.recv() {
            if writer.write_all(&b).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });
    listen_input(&call.session, &channel, stream);
    let start_output = output.clone();
    std::thread::spawn(move || start_output.attach(out, 0));
    {
        let (session, map, id) = (call.session.clone(), Arc::clone(channels), id.clone());
        let group = Arc::clone(group);
        std::thread::spawn(move || {
            let mut buf = [0u8; 32 << 10];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => output.push(&buf[..n]),
                }
            }
            output.end("done");
            session.drop_input(stream);
            map.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            if let Some((wait, report)) = exit {
                let code = group
                    .child
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_mut()
                    .and_then(|c| {
                        if wait {
                            c.wait().ok()
                        } else {
                            c.try_wait().ok().flatten()
                        }
                    })
                    .and_then(|s| s.code());
                report(&id, code);
                group.end();
            }
        });
    }
    Ok((id, stream))
}

fn listen_input(session: &SessionHandle, channel: &Arc<Channel>, stream: u32) {
    let (tx, received) = (channel.input.clone(), Arc::clone(&channel.received));
    session.on_input(
        stream,
        Arc::new(move |i| {
            if let StreamIn::Data(b) = i {
                received.fetch_add(b.len() as u64, Ordering::SeqCst);
                let _ = tx.send(b);
            }
        }),
    );
}

fn run_error(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Unavailable, message)
}

/// The process of a stdio adapter joined to a link (behaviour 1).
fn start_stdio(command: &[String], root: &Path) -> Result<(Child, Link), RpcError> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| internal("the adapter has no command"))?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| run_error(format!("{program}: {e}")))?;
    let stdin = child.stdin.take().ok_or_else(|| internal("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| internal("no stdout"))?;
    Ok((child, (Box::new(stdout), Box::new(stdin))))
}

/// The output of `child` line by line in a thread; waits until `listening` accepts a line.
fn wait_line<T: Send + 'static>(
    child: &mut Child,
    what: &str,
    listening: impl Fn(&str) -> Option<T> + Send + 'static,
) -> Result<T, RpcError> {
    let stdout = child.stdout.take().ok_or_else(|| internal("no stdout"))?;
    let (tx, rx) = mpsc::channel::<Result<T, String>>();
    std::thread::spawn(move || {
        let mut sent = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if !sent && let Some(t) = listening(&line) {
                sent = tx.send(Ok(t)).is_ok();
            }
        }
        if !sent {
            let _ = tx.send(Err("it ended without listening".into()));
        }
    });
    match rx.recv_timeout(LISTEN) {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(why)) => Err(run_error(format!("{what}: {why}"))),
        Err(_) => Err(run_error(format!(
            "{what}: no answer within {} s",
            LISTEN.as_secs()
        ))),
    }
}

fn spawn_listening(command: &[String], extra: &[String], root: &Path) -> Result<Child, RpcError> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| internal("the adapter has no command"))?;
    Command::new(program)
        .args(args)
        .args(extra)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| run_error(format!("{program}: {e}")))
}

/// A connector to a loopback port on the host.
fn tcp_connect(address: SocketAddr) -> Connect {
    Box::new(move || {
        let socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))
            .map_err(|e| run_error(format!("{address} on the host: {e}")))?;
        let reader = socket.try_clone().map_err(internal)?;
        Ok((Box::new(reader) as _, Box::new(socket) as _))
    })
}

/// Where the socket folder goes: the temporary folder, or `/tmp` when a socket path below it
/// would not fit `sun_path` (104 bytes on macOS, 108 on Linux; behaviour 2).
#[cfg(unix)]
fn socket_parent(temporary: PathBuf) -> PathBuf {
    /// The longest `/ferris-idea-dap-<pid>-<nanos>/dap.sock`, and the smaller limit.
    const NAME: usize = 58;
    const LIMIT: usize = 104;
    if temporary.as_os_str().len() + NAME < LIMIT {
        temporary
    } else {
        PathBuf::from("/tmp")
    }
}

/// Unique folder of mode 0700 for an adapter's socket (behaviour 2). `mkdir` fails on anything
/// already there, so a planted folder or link is never used.
#[cfg(unix)]
fn private_folder() -> Result<PathBuf, RpcError> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = socket_parent(std::env::temp_dir())
        .join(format!("ferris-idea-dap-{}-{nanos}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(internal)?;
    Ok(dir)
}

/// How `adapter` is told its socket: js-debug takes the path, Delve `--listen=unix:<path>`.
#[cfg(unix)]
fn socket_argument(adapter: &str, path: &Path) -> String {
    match adapter {
        "dlv" => format!("--listen=unix:{}", path.display()),
        _ => path.display().to_string(),
    }
}

/// Starts a socket adapter (js-debug, Delve) on its private Unix socket, or js-debug on a Windows
/// host on a free loopback port (behaviour 2).
#[cfg(unix)]
fn start_socket(
    adapter: &str,
    command: &[String],
    root: &Path,
    group: &Group,
) -> Result<Connect, RpcError> {
    let dir = private_folder()?;
    *group.folder.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.clone());
    let path = dir.join("dap.sock");
    let mut child = spawn_listening(command, &[socket_argument(adapter, &path)], root)?;
    let waited = wait_line(&mut child, adapter, |l: &str| {
        l.contains("listening").then_some(())
    });
    group.keep_child(child);
    waited?;
    Ok(unix_connect(adapter.to_owned(), path))
}

/// Connects to an adapter's Unix socket on the host.
#[cfg(unix)]
fn unix_connect(adapter: String, path: PathBuf) -> Connect {
    Box::new(move || {
        let socket = std::os::unix::net::UnixStream::connect(&path)
            .map_err(|e| run_error(format!("{adapter} on the host: {e}")))?;
        let reader = socket.try_clone().map_err(internal)?;
        Ok((Box::new(reader) as _, Box::new(socket) as _))
    })
}

/// Without Unix sockets the adapter could only listen on a loopback port every local account can
/// reach, so it is refused (`SPEC-0322` v6 behaviour 2).
#[cfg(not(unix))]
fn start_socket(
    adapter: &str,
    command: &[String],
    root: &Path,
    group: &Group,
) -> Result<Connect, RpcError> {
    let _ = (command, root, group);
    Err(run_error(format!(
        "{adapter} is not supported on this platform yet: it cannot listen privately"
    )))
}

/// Starts a TCP adapter on a free loopback port of the host (behaviour 3): the `127.0.0.1:…` it
/// prints is what the backend connects to.
fn start_tcp(command: &[String], root: &Path, group: &Group) -> Result<Connect, RpcError> {
    let name = command.first().cloned().unwrap_or_default();
    // Off Unix a loopback port has no per-user check (dlv's `--only-same-user` needs Unix), so the
    // adapter is refused (`SPEC-0322` v6 behaviour 3).
    if cfg!(not(unix)) {
        return Err(run_error(format!(
            "{name} is not supported on this platform yet: it cannot listen privately"
        )));
    }
    let mut child = spawn_listening(command, &["--listen=127.0.0.1:0".to_owned()], root)?;
    let waited = wait_line(&mut child, &name, |line: &str| {
        line.split_whitespace()
            .find_map(|w| w.parse::<SocketAddr>().ok())
            .filter(|a| a.ip().is_loopback())
    });
    group.keep_child(child);
    let address = waited?;
    group.hold_port(address.port());
    Ok(tcp_connect(address))
}

/// Starts jdtls with java-debug for the project and resolves `main_class` (behaviour 4).
fn start_java(
    call: &Call,
    main_class: Option<&str>,
    group: &Group,
) -> Result<(Connect, Option<Value>), RpcError> {
    let root = &call.root;
    let server = JavaDebugServer::start(root).map_err(run_error)?;
    let mut classpath = None;
    if let Some(class) = main_class {
        let simple = class.rsplit('.').next().unwrap_or(class);
        let relative = format!("{}.java", class.replace('.', "/"));
        let candidates: Vec<PathBuf> = [root.join(&relative), root.join(format!("{simple}.java"))]
            .into_iter()
            .chain(
                ferris_idea_languages::java_roots::source_roots(root)
                    .iter()
                    .map(|r| root.join(r).join(&relative)),
            )
            .collect();
        if let Some(file) = candidates.iter().find(|f| f.is_file()) {
            server.open(file).map_err(run_error)?;
        }
        let (modules, classes) = server
            .classpath(class, Duration::from_secs(120))
            .map_err(run_error)?;
        let _ = server.build();
        classpath = Some(json!({
            "modules": modules, "classes": classes, "project": server.project_name(class),
        }));
    }
    let connect = match server.endpoint.clone() {
        #[cfg(unix)]
        DebugEndpoint::Unix(path) => unix_connect("java-debug".to_owned(), path),
        #[cfg(not(unix))]
        DebugEndpoint::Unix(_) => return Err(run_error("no Unix sockets here")),
        DebugEndpoint::Loopback(port) => {
            group.hold_port(port);
            tcp_connect(SocketAddr::from(([127, 0, 0, 1], port)))
        }
    };
    *group.java.lock().unwrap_or_else(|e| e.into_inner()) = Some(server);
    Ok((connect, classpath))
}

/// What `dap.start` does for `known`; returns the link and what it adds to the answer.
fn open_adapter(
    call: &Call,
    known: &Known,
    group: &Arc<Group>,
) -> Result<(Link, bool, Option<Value>), RpcError> {
    let command = known.command.as_deref().unwrap_or_default();
    match known.transport {
        Transport::Stdio => {
            let (child, link) = start_stdio(command, &call.root)?;
            group.keep_child(child);
            Ok((link, true, None))
        }
        Transport::Socket | Transport::Tcp => {
            let connect = if known.transport == Transport::Socket {
                start_socket(&known.name, command, &call.root, group)?
            } else {
                start_tcp(command, &call.root, group)?
            };
            let link = connect()?;
            *group.connect.lock().unwrap_or_else(|e| e.into_inner()) = Some(connect);
            Ok((link, false, None))
        }
        Transport::Jdtls => {
            let (connect, classpath) = start_java(call, call.params["main_class"].as_str(), group)?;
            let link = connect()?;
            *group.connect.lock().unwrap_or_else(|e| e.into_inner()) = Some(connect);
            Ok((link, false, classpath))
        }
    }
}

/// Binds `dap.*`.
pub fn bind(backend: &Backend, options: &HostOptions) {
    let opts = options.clone();
    add(
        backend,
        "dap",
        "dap.adapters",
        Right::Debug,
        false,
        move |call| {
            let list: Vec<Value> = adapters(&opts)
                .iter()
                .map(|k| {
                    json!({
                        "adapter": k.name, "installed": k.command.is_some(),
                        "transport": k.transport.name(),
                    })
                })
                .collect();
            Ok(json!({ "root": call.root, "adapters": list }))
        },
    );
    add(backend, "dap", "dap.plan", Right::Debug, false, |call| {
        plan(call)
    });
    add(backend, "dap", "dap.processes", Right::Debug, false, |_| {
        let list: Vec<Value> = ferris_idea_debugger::processes()
            .into_iter()
            .map(|p| {
                // Only the program: arguments may hold secrets (SEC-10).
                let program = p
                    .command
                    .split_whitespace()
                    .next()
                    .and_then(|c| c.rsplit('/').next())
                    .unwrap_or_default()
                    .to_owned();
                json!({
                    "pid": p.pid, "name": p.name, "command": program,
                    "runtime": p.runtime, "debug_port": p.debug_port,
                })
            })
            .collect();
        Ok(json!({ "processes": list }))
    });
    let channels: Arc<Channels> = Arc::default();
    let (opts, c) = (options.clone(), Arc::clone(&channels));
    add(
        backend,
        "dap",
        "dap.start",
        Right::Debug,
        true,
        move |call| start(call, &opts, &c),
    );
    let c = Arc::clone(&channels);
    add(
        backend,
        "dap",
        "dap.connect",
        Right::Debug,
        false,
        move |call| {
            let (_, parent) = c.get(call)?;
            let group = Arc::clone(&parent.group);
            let link = {
                let guard = group.connect.lock().unwrap_or_else(|e| e.into_inner());
                let connect = guard.as_ref().ok_or_else(|| {
                    RpcError::new(ErrorCode::Unavailable, "this adapter has one connection")
                })?;
                connect()?
            };
            let (session, stream) = relay(call, &c, &group, link, None)?;
            Ok(json!({ "session": session, "stream": stream }))
        },
    );
    let c = Arc::clone(&channels);
    add(
        backend,
        "dap",
        "dap.attach",
        Right::Debug,
        false,
        move |call| {
            let (_, channel) = c.get(call)?;
            let from = call.params["from"].as_u64();
            let out = call.session.open_stream().ok_or_else(|| {
                RpcError::new(ErrorCode::Unavailable, "the session has no connection now")
            })?;
            let stream = out.id();
            listen_input(&call.session, &channel, stream);
            let output = channel.output.clone();
            let start = output.start_for(from);
            std::thread::spawn(move || output.attach(out, start));
            Ok(json!({
                "stream": stream, "start": start,
                "received": channel.received.load(Ordering::SeqCst),
            }))
        },
    );
    let c = Arc::clone(&channels);
    add(
        backend,
        "dap",
        "dap.host",
        Right::Debug,
        true,
        move |call| host_action(call, &c),
    );
    let c = Arc::clone(&channels);
    add(
        backend,
        "dap",
        "dap.stop",
        Right::Debug,
        true,
        move |call| {
            let (id, channel) = c.get(call)?;
            channel.group.end();
            Ok(json!({ "session": id, "stopped": true }))
        },
    );
}

/// `dap.plan`: what a run configuration or a run marker means here (behaviour 6).
fn plan(call: &Call) -> Result<Value, RpcError> {
    let root = call.root.clone();
    let (configs, name) = crate::run::requested_config(call)?;
    let config = configs
        .configs
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| {
            RpcError::new(ErrorCode::NotFound, format!("no run configuration {name}"))
        })?;
    if config.url.is_some() {
        return Err(RpcError::new(
            ErrorCode::Unavailable,
            "a page cannot be debugged on the host: it has no browser for this window",
        ));
    }
    let cwd = root.join(config.cwd.as_deref().unwrap_or(""));
    // The command as it starts, with the host workspace's Python interpreter (SPEC-0050 v2).
    let (kind, interpreter, program, args) =
        ferris_idea_languages::debug_plan::plan_run_config(&root, config, &cwd)
            .map_err(|why| RpcError::new(ErrorCode::Unavailable, why))?
            .ok_or_else(|| {
                RpcError::new(ErrorCode::Unavailable, format!("{name} cannot be debugged"))
            })?;
    // For Kotlin the plan's arguments are the build command (SPEC-0132 behaviour 2).
    let (args, prepare) = if kind == ferris_idea_debugger::Kind::Kotlin {
        (Vec::new(), args.join(" "))
    } else {
        (args, String::new())
    };
    Ok(json!({
        "root": root, "name": name, "kind": kind.wire_name(), "adapter": kind.adapter_name(),
        "interpreter": interpreter, "program": program, "args": args, "cwd": cwd,
        "env": config.env, "prepare": prepare,
    }))
}

/// `dap.start` (behaviours 5, 10).
fn start(call: &Call, options: &HostOptions, channels: &Arc<Channels>) -> Result<Value, RpcError> {
    let name = text(&call.params, "adapter")?.to_owned();
    let attach = call.params["mode"].as_str() == Some("attach");
    let prepare = call.params["prepare"]
        .as_str()
        .filter(|p| !p.trim().is_empty());
    let root = call.root.clone();
    if (!attach || prepare.is_some() || name == "java-debug")
        && !ferris_idea_core::trust::allows_project_code(&root)
    {
        return Err(crate::run::trust_refusal(&root));
    }
    let known = adapters(options)
        .into_iter()
        .find(|a| a.name == name)
        .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no adapter {name:?}")))?;
    // `debugpy --listen` and a headless Delve server are the adapter themselves: nothing has to
    // run besides them, the backend talks to them directly (behaviour 10).
    let direct = attach
        && matches!(name.as_str(), "debugpy" | "dlv")
        && call.params["attach"]["kind"] != "pid";
    if known.command.is_none() && !direct {
        return Err(RpcError::new(
            ErrorCode::Unavailable,
            format!("{name} is not installed on the host"),
        ));
    }
    let mut prepared = None;
    if let Some(cmd) = prepare {
        let o = Command::new("sh")
            .args(["-c", cmd])
            .current_dir(&root)
            .output()
            .map_err(internal)?;
        let text =
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
        if !o.status.success() {
            return Err(RpcError::new(
                ErrorCode::Unavailable,
                format!("the build failed; nothing was debugged\n{text}"),
            ));
        }
        prepared = Some(text);
    }
    let group = Arc::new(Group::default());
    let result = start_inner(
        call, options, channels, &name, &known, attach, direct, &group,
    );
    if result.is_err() {
        group.end();
    }
    let mut answer = result?;
    if let Some(p) = prepared {
        answer["prepared"] = json!(p);
    }
    Ok(answer)
}

#[allow(clippy::too_many_arguments)]
fn start_inner(
    call: &Call,
    options: &HostOptions,
    channels: &Arc<Channels>,
    name: &str,
    known: &Known,
    attach: bool,
    direct: bool,
    group: &Arc<Group>,
) -> Result<Value, RpcError> {
    let resolved = if attach {
        Some(target::resolve(
            &call.params["attach"],
            options,
            &call.root,
            group,
        )?)
    } else {
        None
    };
    let (link, stdio, classpath) = if direct {
        let (host, port) = resolved
            .as_ref()
            .and_then(|r| r.address.clone())
            .ok_or_else(|| invalid(format!("a {name} attach needs an address")))?;
        // A Delve server's port is neither encrypted nor authenticated: only one on the host's
        // own loopback is talked to (behaviour 10, SPEC-0147 v3 behaviour 7).
        if name == "dlv" && !ferris_idea_debugger::is_loopback(&host) {
            return Err(RpcError::new(
                ErrorCode::Denied,
                format!("a Delve server is attached to on the host's loopback only, not on {host}"),
            )
            .hint("its port is not protected: start it with --listen=127.0.0.1:<port> on the host where the program runs"));
        }
        let address = (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .ok_or_else(|| run_error(format!("{host} cannot be resolved on the host")))?;
        (tcp_connect(address)()?, false, None)
    } else {
        open_adapter(call, known, group)?
    };
    let handle = call.session.clone();
    let exit: Exit = (
        stdio,
        Box::new(move |id, code| {
            handle.notify("dap.exited", json!({ "session": id, "code": code }));
        }),
    );
    let (session, stream) = relay(call, channels, group, link, Some(exit))?;
    {
        let group = Arc::clone(group);
        call.session.on_end(Box::new(move || group.end()));
    }
    let mut answer = json!({ "session": session, "stream": stream, "adapter": name });
    if let Some(port) = *group.port.lock().unwrap_or_else(|e| e.into_inner()) {
        answer["adapter_port"] = json!(port);
    }
    if let Some(r) = resolved {
        if let Some((host, port)) = r.address {
            answer["address"] = json!({ "host": host, "port": port });
        }
        if let Some(pid) = r.pid {
            answer["pid"] = json!(pid);
        }
        answer["mappings"] = json!(r.mappings);
    }
    if let Some(c) = classpath {
        answer["classpath"] = c;
    }
    Ok(answer)
}

/// `dap.host`: what the program hosting an adapter does for it outside the protocol (behaviour 4).
fn host_action(call: &Call, channels: &Channels) -> Result<Value, RpcError> {
    let (_, channel) = channels.get(call)?;
    let action = text(&call.params, "action")?;
    let value = &call.params["value"];
    let java = channel.group.java.lock().unwrap_or_else(|e| e.into_inner());
    let server = java
        .as_ref()
        .ok_or_else(|| invalid("this adapter has no host program"))?;
    let result = match action {
        "settings" => server.update_settings(value),
        "exception_types" => {
            let types: Vec<String> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            server.set_exception_types(&types)
        }
        "sources" => {
            let files: Vec<PathBuf> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter_map(|p| crate::resolve(&call.root, p).ok())
                        .collect()
                })
                .unwrap_or_default();
            server.sources_changed(&files)
        }
        other => return Err(invalid(format!("unknown action {other}"))),
    };
    result.map_err(run_error)?;
    Ok(json!({ "ok": true }))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// SPEC-0322 behaviour 2: a socket path has to fit `sun_path`.
    #[test]
    fn a_long_temporary_folder_moves_the_socket_to_tmp() {
        let short = PathBuf::from("/var/folders/ab/cdefghij/T");
        assert_eq!(socket_parent(short.clone()), short);
        let long = PathBuf::from(format!("/private/tmp/{}", "x".repeat(60)));
        assert_eq!(socket_parent(long), PathBuf::from("/tmp"));
    }

    #[test]
    fn each_adapter_is_told_its_socket_in_its_own_way() {
        let path = Path::new("/tmp/f/dap.sock");
        assert_eq!(
            socket_argument("dlv", path),
            "--listen=unix:/tmp/f/dap.sock"
        );
        assert_eq!(socket_argument("js-debug", path), "/tmp/f/dap.sock");
    }
}

//! A debug adapter on the host for the window and for agents (`SPEC-0322`): a [`DapClient`] whose
//! bytes travel over a session stream instead of a local process or port. The link rewrites
//! source paths between the stand-in folder and the host folder (behaviour 14) and survives a
//! break of the session: output comes back from the offset the window has and input is resent
//! from the offset the adapter has (behaviour 7).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use idecraft_debugger::{DapClient, DapConnect, DapError, FnHost, PathMap};
use idecraft_session::{Connection, ErrorCode, RpcError};
use serde_json::{Value, json};

use crate::live::LiveProject;

const START: Duration = Duration::from_secs(180);
const CALL: Duration = Duration::from_secs(30);
/// What the window keeps of its own messages for a resend.
const KEPT_INPUT: usize = 1 << 20;
/// How long a stopped link waits for the session to come back.
const REATTACH: Duration = Duration::from_secs(300);

/// How the host is reached: a window's live project or a connection of a headless one.
#[derive(Clone)]
pub enum HostLink {
    /// A project window, which reconnects by itself.
    Live(LiveProject),
    /// A connection that ends the session when it breaks.
    Connection(Connection),
}

impl HostLink {
    fn connection(&self) -> Option<Connection> {
        match self {
            HostLink::Live(l) => l.host_connection(),
            HostLink::Connection(c) => Some(c.clone()),
        }
    }

    /// A request through the link, with the connection of the moment.
    pub fn call(&self, method: &str, params: Value, limit: Duration) -> Result<Value, RpcError> {
        match self {
            HostLink::Live(l) => l.host_call(method, params, limit),
            HostLink::Connection(c) => c.call(method, params, limit),
        }
    }
}

/// What `dap.start` answered, for the launch or attach request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAdapter {
    /// The adapter connection's id on the host.
    pub session: String,
    /// The address of an attach target, as the adapter on the host reaches it.
    pub address: Option<(String, u16)>,
    /// The process of an attach target.
    pub pid: Option<u32>,
    /// Host path and path in the target, for the adapter's source mapping.
    pub mappings: Vec<(String, String)>,
    /// `modules`, `classes`, `project` of a Java launch.
    pub classpath: Option<Value>,
    /// Output of the build the launch asked for.
    pub prepared: Option<String>,
    /// The adapter's TCP port on the host, when it has one.
    pub port: Option<u16>,
}

/// A started adapter: its first connection, more connections for js-debug, and what it answered.
pub struct HostDebug {
    /// The first connection.
    pub client: DapClient,
    /// Makes one more connection to the same adapter.
    pub connect: DapConnect,
    /// The answer of `dap.start`.
    pub adapter: HostAdapter,
}

/// The host's root and the folders for the source mapping (behaviour 14).
pub fn path_map(stand_in: &Path, root: &str) -> PathMap {
    PathMap::new(stand_in, root)
}

/// Starts `adapter` on the host with `params` (`mode`, `attach`, `main_class`, `prepare`).
pub fn start_host_debug(
    link: &HostLink,
    adapter: &str,
    params: Value,
    paths: Option<PathMap>,
) -> Result<HostDebug, RpcError> {
    let mut params = params;
    params["adapter"] = json!(adapter);
    let started = link.call("dap.start", params, START)?;
    let session = text_of(&started, "session").to_owned();
    let stream = started["stream"].as_u64().unwrap_or_default() as u32;
    let mappings: Vec<(String, String)> = started["mappings"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| Some((m[0].as_str()?.to_owned(), m[1].as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let info = HostAdapter {
        session: session.clone(),
        address: started["address"]["host"]
            .as_str()
            .map(str::to_owned)
            .zip(started["address"]["port"].as_u64().map(|p| p as u16)),
        pid: started["pid"].as_u64().map(|p| p as u32),
        mappings,
        classpath: Some(started["classpath"].clone()).filter(|c| !c.is_null()),
        prepared: started["prepared"].as_str().map(str::to_owned),
        port: started["adapter_port"].as_u64().map(|p| p as u16),
    };
    let client = client_on(link, &session, stream, paths.clone(), true);
    let connect: DapConnect = {
        let (link, session, paths) = (link.clone(), session.clone(), paths);
        Arc::new(move || {
            let more = link
                .call("dap.connect", json!({ "session": session }), CALL)
                .map_err(rpc_to_dap)?;
            let stream = more["stream"].as_u64().unwrap_or_default() as u32;
            let id = text_of(&more, "session").to_owned();
            Ok(client_on(&link, &id, stream, paths.clone(), false))
        })
    };
    Ok(HostDebug {
        client,
        connect,
        adapter: info,
    })
}

fn text_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

fn rpc_to_dap(e: RpcError) -> DapError {
    DapError::Start(format!("{}: {}", e.code.as_str(), e.message))
}

/// The java-debug helpers outside DAP, as a [`DapClient`] host (`SPEC-0322` behaviour 4).
pub fn java_host(link: &HostLink, session: &str, stand_in: &Path) -> FnHost {
    let call = {
        let (link, session) = (link.clone(), session.to_owned());
        move |action: &str, value: Value| {
            link.call(
                "dap.host",
                json!({ "session": session, "action": action, "value": value }),
                CALL,
            )
            .map(drop)
            .map_err(|e| e.message)
        }
    };
    let (call2, stand_in) = (call.clone(), stand_in.to_path_buf());
    FnHost {
        settings: Box::new(move |s| call("settings", s.clone())),
        sources: Box::new(move |files: &[PathBuf]| {
            let list: Vec<String> = files
                .iter()
                .filter_map(|f| f.strip_prefix(&stand_in).ok())
                .map(|f| f.to_string_lossy().into_owned())
                .collect();
            call2("sources", json!(list))
        }),
    }
}

/// `dap.host` for exception types.
pub fn java_exception_types(link: &HostLink, session: &str) -> idecraft_debugger::ExceptionTypes {
    let (link, session) = (link.clone(), session.to_owned());
    Box::new(move |types| {
        link.call(
            "dap.host",
            json!({ "session": session, "action": "exception_types", "value": types }),
            CALL,
        )
        .map(drop)
        .map_err(|e| e.message)
    })
}

/// A [`DapClient`] on stream `stream` of adapter connection `session`.
fn client_on(
    link: &HostLink,
    session: &str,
    stream: u32,
    paths: Option<PathMap>,
    owner: bool,
) -> DapClient {
    let (out_tx, out_rx) = channel::<Vec<u8>>();
    let (in_tx, in_rx) = channel::<Vec<u8>>();
    let worker = Worker {
        link: link.clone(),
        session: session.to_owned(),
        stream,
        conn: link.connection(),
        out: out_rx,
        to_reader: in_tx,
        seen: 0,
        sent: 0,
        kept: VecDeque::new(),
        owner,
    };
    std::thread::Builder::new()
        .name("host-debug".into())
        .spawn(move || worker.run())
        .ok();
    DapClient::over_link(
        Box::new(ChanWriter(out_tx)),
        ChanReader {
            rx: in_rx,
            pending: VecDeque::new(),
        },
        None,
        paths,
    )
}

struct ChanWriter(Sender<Vec<u8>>);

impl Write for ChanWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .send(buf.to_vec())
            .map(|()| buf.len())
            .map_err(|_| std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct ChanReader {
    rx: Receiver<Vec<u8>>,
    pending: VecDeque<u8>,
}

impl Read for ChanReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.pending.is_empty() {
            match self.rx.recv() {
                Ok(data) => self.pending.extend(data),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        for b in buf.iter_mut().take(n) {
            *b = self.pending.pop_front().unwrap_or_default();
        }
        Ok(n)
    }
}

/// An `output` event the debugger shows in the console (behaviours 7 and 18).
fn console_frame(text: &str) -> Vec<u8> {
    let body = json!({
        "seq": 0, "type": "event", "event": "output",
        "body": { "category": "console", "output": format!("[IDECraft] {text}\n") },
    })
    .to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

struct Worker {
    link: HostLink,
    session: String,
    stream: u32,
    conn: Option<Connection>,
    out: Receiver<Vec<u8>>,
    to_reader: Sender<Vec<u8>>,
    seen: u64,
    sent: u64,
    kept: VecDeque<u8>,
    owner: bool,
}

/// Why the relay loop stopped.
enum Stop {
    /// The adapter's connection ended.
    Done,
    /// The client is gone; the adapter is to be stopped.
    ClientGone,
    /// The connection broke.
    Broken,
}

impl Worker {
    fn remember(&mut self, data: &[u8]) {
        self.sent += data.len() as u64;
        self.kept.extend(data);
        let over = self.kept.len().saturating_sub(KEPT_INPUT);
        self.kept.drain(..over);
    }

    fn pump(&mut self) -> Stop {
        let Some(conn) = self.conn.clone() else {
            return Stop::Broken;
        };
        conn.open_sending(self.stream);
        loop {
            match self.out.try_recv() {
                Ok(data) => {
                    self.remember(&data);
                    if conn.send_data(self.stream, &data, CALL).is_err() {
                        return Stop::Broken;
                    }
                    continue;
                }
                Err(TryRecvError::Disconnected) => return Stop::ClientGone,
                Err(TryRecvError::Empty) => {}
            }
            let (data, done) = conn.read_stream(self.stream, Duration::from_millis(15));
            if !data.is_empty() {
                self.seen += data.len() as u64;
                if self.to_reader.send(data).is_err() {
                    return Stop::ClientGone;
                }
            }
            match done.as_deref() {
                None => {}
                Some("done") => return Stop::Done,
                Some(_) => {
                    conn.drop_stream(self.stream);
                    return Stop::Broken;
                }
            }
        }
    }

    /// `dap.attach` once the session is back; resends what the adapter has not got.
    fn reattach(&mut self) -> Result<(), String> {
        let since = Instant::now();
        let _ = self.to_reader.send(console_frame(
            "Reconnecting… the debug session waits on the host.",
        ));
        loop {
            if since.elapsed() > REATTACH {
                return Err("the connection did not come back".into());
            }
            // A window reconnects by itself; a headless link is over with its connection.
            if let HostLink::Connection(c) = &self.link
                && !c.alive()
            {
                return Err("the session ended".into());
            }
            let Some(conn) = self.link.connection() else {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            };
            match conn.call(
                "dap.attach",
                json!({ "session": self.session, "from": self.seen }),
                CALL,
            ) {
                Ok(a) => {
                    let start = a["start"].as_u64().unwrap_or_default();
                    if start > self.seen {
                        return Err(
                            "more debugger output arrived during the break than the host keeps"
                                .into(),
                        );
                    }
                    self.stream = a["stream"].as_u64().unwrap_or_default() as u32;
                    self.conn = Some(conn.clone());
                    conn.open_sending(self.stream);
                    let received = a["received"].as_u64().unwrap_or_default();
                    let missing = self.sent.saturating_sub(received) as usize;
                    if missing > self.kept.len() {
                        return Err("the debugger's input could not be resent".into());
                    }
                    let tail: Vec<u8> = self
                        .kept
                        .iter()
                        .skip(self.kept.len() - missing)
                        .copied()
                        .collect();
                    if !tail.is_empty() {
                        conn.send_data(self.stream, &tail, CALL)
                            .map_err(|e| e.message)?;
                    }
                    let _ = self.to_reader.send(console_frame("Reconnected."));
                    return Ok(());
                }
                Err(e) if e.code == ErrorCode::NotFound => {
                    return Err("the debug adapter ended on the host".into());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }

    fn run(mut self) {
        loop {
            match self.pump() {
                Stop::Done => break,
                Stop::ClientGone => {
                    if self.owner {
                        let _ =
                            self.link
                                .call("dap.stop", json!({ "session": self.session }), CALL);
                    }
                    return;
                }
                Stop::Broken => {
                    if let Err(why) = self.reattach() {
                        let _ = self
                            .to_reader
                            .send(console_frame(&format!("The debug session ended: {why}")));
                        break;
                    }
                }
            }
        }
        if let Some(c) = &self.conn {
            c.drop_stream(self.stream);
        }
    }
}

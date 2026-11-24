//! The port forwards of the remote projects in this process (`SPEC-0318` behaviours 25–27): the
//! window's Ports list and the MCP tools `remote_ports`, `remote_port_forward` and
//! `remote_port_close` share them. A forward starts only from a user action or an agent with the
//! `ports` right, never because the backend asked; its local end listens on loopback only.

use std::sync::Mutex;
use std::time::Duration;

use ferris_idea_session::forward::LocalForward;
use ferris_idea_session::{Connection, ErrorCode, RpcError};
use serde_json::{Value, json};

struct Entry {
    host: String,
    folder: String,
    remote_port: u16,
    target: String,
    kind: String,
    local: LocalForward,
}

static FORWARDS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    FORWARDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Who asks for a forward: the user (may confirm a database port) or an agent (may not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    /// A user action in the window, with the database warning confirmed or not.
    User {
        /// The user confirmed the warning for a database port.
        confirmed: bool,
    },
    /// An agent (its `ports` grant was checked by the caller).
    Agent,
}

/// Forwards `port` of the project's host to this computer's loopback through `conn`; `target_host`
/// (a user's explicit entry only) replaces `127.0.0.1` on the host. Returns the forward as listed.
pub fn forward(
    conn: &Connection,
    host: &str,
    folder: &str,
    port: u16,
    target_host: Option<&str>,
    asker: Asker,
) -> Result<Value, RpcError> {
    let mut params = json!({ "port": port });
    if let (Some(t), Asker::User { .. }) = (target_host, asker) {
        params["target_host"] = json!(t);
    }
    if let Asker::User { confirmed: true } = asker {
        params["confirm"] = json!(true);
    }
    let f = conn.call("port.forward", params, Duration::from_secs(30))?;
    let id = f["forward"].as_str().unwrap_or_default().to_owned();
    let local = LocalForward::start(conn.clone(), &id, port).map_err(|e| {
        let _ = conn.call(
            "port.close",
            json!({ "forward": id }),
            Duration::from_secs(10),
        );
        RpcError::new(ErrorCode::Unavailable, format!("no local port: {e}"))
    })?;
    let entry = Entry {
        host: host.to_owned(),
        folder: folder.to_owned(),
        remote_port: port,
        target: f["target"].as_str().unwrap_or_default().to_owned(),
        kind: f["kind"].as_str().unwrap_or("service").to_owned(),
        local,
    };
    let v = describe(&entry);
    lock().push(entry);
    Ok(v)
}

fn describe(e: &Entry) -> Value {
    json!({
        "forward": e.local.forward, "host": e.host, "folder": e.folder,
        "remote_port": e.remote_port, "target": e.target, "kind": e.kind,
        "local": e.local.addresses.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "local_port": e.local.port(),
    })
}

/// The open forwards of a project.
pub fn list(host: &str, folder: &str) -> Vec<Value> {
    lock()
        .iter()
        .filter(|e| e.host == host && e.folder == folder)
        .map(describe)
        .collect()
}

/// Closes forward `id` of a project: the local listener stops, the backend closes its
/// connections. Returns whether there was one.
pub fn close(conn: Option<&Connection>, host: &str, folder: &str, id: &str) -> bool {
    let mut list = lock();
    let Some(i) = list
        .iter()
        .position(|e| e.host == host && e.folder == folder && e.local.forward == id)
    else {
        return false;
    };
    let e = list.remove(i);
    drop(list);
    e.local.close();
    if let Some(c) = conn {
        let _ = c.call(
            "port.close",
            json!({ "forward": id }),
            Duration::from_secs(10),
        );
    }
    true
}

/// Closes every forward of a project (the window or the session closes).
pub fn close_all(host: &str, folder: &str) {
    let mut list = lock();
    list.retain(|e| {
        let mine = e.host == host && e.folder == folder;
        if mine {
            e.local.close();
        }
        !mine
    });
}

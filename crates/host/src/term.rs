//! `term.*` (`SPEC-0318` behaviours 17–20): pseudo terminals on the host. The shell's output goes
//! to the window on a stream with credit, keystrokes come back on the same stream; a terminal
//! outlives a broken connection and keeps its last 1 MiB of output for `term.attach`. No `ssh`
//! process is started for it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use idecraft_session::{Backend, Call, ErrorCode, Right, RpcError, SessionHandle, StreamIn};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};

use crate::ring::Output;
use crate::{HostOptions, add, internal, invalid, text};

/// Input chunks queued per terminal before the window's stream waits (`ADR-0026` §9).
const INPUT_QUEUE: usize = 256;

struct Term {
    owner: String,
    cwd: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    input: mpsc::SyncSender<Vec<u8>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    output: Output,
    size: Mutex<(u16, u16)>,
    exit: Mutex<Option<i32>>,
    pid: Option<u32>,
}

impl Term {
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid.and_then(|p| i32::try_from(p).ok()) {
            // The shell leads its own session and process group (the PTY's).
            let group = nix::unistd::Pid::from_raw(pid);
            let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGHUP);
            // A second for the shell's own hang-up handling, as a closed terminal gives it.
            let t0 = std::time::Instant::now();
            while t0.elapsed() < std::time::Duration::from_secs(1)
                && self.exit.lock().is_ok_and(|e| e.is_none())
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        }
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }

    fn describe(&self, id: &str) -> Value {
        let (cols, rows) = *self.size.lock().unwrap_or_else(|e| e.into_inner());
        let exit = *self.exit.lock().unwrap_or_else(|e| e.into_inner());
        json!({
            "term": id, "cwd": self.cwd, "cols": cols, "rows": rows,
            "running": exit.is_none(), "exit_code": exit,
        })
    }
}

#[derive(Default)]
struct Terms {
    map: Mutex<HashMap<String, Arc<Term>>>,
    next: AtomicU64,
}

impl Terms {
    fn get(&self, call: &Call) -> Result<(String, Arc<Term>), RpcError> {
        let id = text(&call.params, "term")?.to_owned();
        let t = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .filter(|t| t.owner == call.session.id())
            .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no terminal {id}")))?;
        Ok((id, t))
    }
}

fn size(params: &Value) -> Result<(u16, u16), RpcError> {
    let get = |k: &str, default: u64| {
        let n = params[k].as_u64().unwrap_or(default);
        if (1..=1000).contains(&n) {
            Ok(n as u16)
        } else {
            Err(invalid(format!("`{k}` must be between 1 and 1000")))
        }
    };
    Ok((get("cols", 80)?, get("rows", 24)?))
}

/// `[A-Z_][A-Z0-9_]*`, the variable names a window may set (behaviour 17).
pub fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Joins a new stream to `term`: the output from `from` (or everything kept) first, then
/// everything new; keystrokes from the window. Returns the stream and the offset it starts at.
fn attach(
    session: &SessionHandle,
    term: &Arc<Term>,
    from: Option<u64>,
) -> Result<(u32, u64), RpcError> {
    let out = session.open_stream().ok_or_else(|| {
        RpcError::new(ErrorCode::Unavailable, "the session has no connection now")
    })?;
    let id = out.id();
    let input = term.input.clone();
    session.on_input(
        id,
        Arc::new(move |i| {
            if let StreamIn::Data(bytes) = i {
                let _ = input.send(bytes);
            }
        }),
    );
    let output = term.output.clone();
    let start = output.start_for(from);
    // Sending the kept output waits for credit; not on the request's thread.
    std::thread::spawn(move || output.attach(out, start));
    Ok((id, start))
}

/// Binds `term.*`.
pub fn bind(backend: &Backend, options: &HostOptions) {
    let terms = Arc::new(Terms::default());
    let shell = options.shell.clone();
    let t = Arc::clone(&terms);
    add(
        backend,
        "term",
        "term.open",
        Right::Run,
        true,
        move |call| {
            let root = call.root.clone();
            let cwd = idecraft_session::paths::check_relative(
                call.params["cwd"].as_str().unwrap_or("."),
            )?;
            let dir = crate::resolve(&root, &cwd)?;
            if !dir.is_dir() {
                return Err(invalid(format!("{cwd} is not a folder")));
            }
            let (cols, rows) = size(&call.params)?;
            let mut env = Vec::new();
            if let Some(o) = call.params["env"].as_object() {
                for (k, v) in o {
                    if !valid_env_name(k) {
                        return Err(invalid(format!(
                            "{k:?} is not a variable name of the form [A-Z_][A-Z0-9_]*"
                        )));
                    }
                    env.push((k.clone(), v.as_str().unwrap_or_default().to_owned()));
                }
            }
            let program = shell
                .clone()
                .or_else(|| std::env::var("SHELL").ok())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/bin/sh".to_owned());
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(internal)?;
            let mut cmd = CommandBuilder::new(&program);
            // A login shell, as an SSH login would start it.
            cmd.arg("-l");
            cmd.cwd(&dir);
            cmd.env("TERM", "xterm-256color");
            for (k, v) in env {
                cmd.env(k, v);
            }
            let child = pair.slave.spawn_command(cmd).map_err(internal)?;
            drop(pair.slave);
            let pid = child.process_id();
            let mut reader = pair.master.try_clone_reader().map_err(internal)?;
            let mut writer = pair.master.take_writer().map_err(internal)?;
            let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);
            std::thread::spawn(move || {
                while let Ok(bytes) = rx.recv() {
                    if writer
                        .write_all(&bytes)
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            });
            let id = format!("t{}", t.next.fetch_add(1, Ordering::SeqCst) + 1);
            let term = Arc::new(Term {
                owner: call.session.id().to_owned(),
                cwd: cwd.clone(),
                master: Mutex::new(pair.master),
                input: tx,
                child: Mutex::new(child),
                output: Output::default(),
                size: Mutex::new((cols, rows)),
                exit: Mutex::new(None),
                pid,
            });
            t.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone(), Arc::clone(&term));
            {
                let (term, session, id) = (Arc::clone(&term), call.session.clone(), id.clone());
                std::thread::spawn(move || {
                    let mut buf = [0u8; 16 << 10];
                    while let Ok(n) = reader.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        term.output.push(&buf[..n]);
                    }
                    let code = term
                        .child
                        .lock()
                        .ok()
                        .and_then(|mut c| c.wait().ok())
                        .map(|s| s.exit_code() as i32);
                    *term.exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.unwrap_or(-1));
                    term.output.end("exited");
                    session.notify("term.exited", json!({ "term": id, "code": code }));
                });
            }
            {
                let (t2, id2, term) = (Arc::clone(&t), id.clone(), Arc::clone(&term));
                call.session.on_end(Box::new(move || {
                    term.kill();
                    t2.map
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id2);
                }));
            }
            let (stream, _) = attach(&call.session, &term, Some(0))?;
            Ok(json!({ "term": id, "stream": stream, "shell": program, "cwd": cwd }))
        },
    );
    let t = Arc::clone(&terms);
    add(
        backend,
        "term",
        "term.attach",
        Right::Run,
        false,
        move |call| {
            let (id, term) = t.get(call)?;
            let from = call.params["from"].as_u64();
            let (stream, start) = attach(&call.session, &term, from)?;
            Ok(json!({ "term": id, "stream": stream, "from": start }))
        },
    );
    let t = Arc::clone(&terms);
    add(
        backend,
        "term",
        "term.resize",
        Right::Run,
        false,
        move |call| {
            let (id, term) = t.get(call)?;
            let (cols, rows) = size(&call.params)?;
            term.master
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(internal)?;
            *term.size.lock().unwrap_or_else(|e| e.into_inner()) = (cols, rows);
            Ok(json!({ "term": id, "cols": cols, "rows": rows }))
        },
    );
    let t = Arc::clone(&terms);
    add(
        backend,
        "term",
        "term.close",
        Right::Run,
        true,
        move |call| {
            let (id, term) = t.get(call)?;
            term.kill();
            t.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            Ok(json!({ "term": id, "closed": true }))
        },
    );
    let t = Arc::clone(&terms);
    add(
        backend,
        "term",
        "term.list",
        Right::View,
        false,
        move |call| {
            let map = t.map.lock().unwrap_or_else(|e| e.into_inner());
            let mut list: Vec<Value> = map
                .iter()
                .filter(|(_, x)| x.owner == call.session.id())
                .map(|(id, x)| x.describe(id))
                .collect();
            list.sort_by_key(|v| v["term"].as_str().unwrap_or_default().to_owned());
            Ok(json!({ "terminals": list }))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_variable_names_are_taken() {
        for ok in ["LANG", "_X", "A1_B"] {
            assert!(valid_env_name(ok), "{ok}");
        }
        for bad in ["", "1A", "lang", "A-B", "A B", "A=B", "LD_PRELOAD\0"] {
            assert!(!valid_env_name(bad), "{bad:?}");
        }
    }
}

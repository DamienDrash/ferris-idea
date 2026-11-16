//! `run.*` (`SPEC-0318` behaviours 5 and 21–24): the host project's run configurations, run on the
//! host with their before-launch chains; output on two streams (standard output and error) with
//! the last 1 MiB of each kept for `run.attach`, events in the session's event stream. Untrusted
//! host projects run nothing until `run.trust`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use idecraft_core::{RunConfig, RunConfigs, RunEvent, add_detected, load_run_configs};
use idecraft_session::{Backend, Call, ErrorCode, Right, RpcError, SessionHandle};
use serde_json::{Value, json};

use crate::ring::Output;
use crate::{HostOptions, add, internal, invalid};

struct Run {
    owner: String,
    name: String,
    stdout: Output,
    stderr: Output,
    stop: Mutex<Option<Box<dyn Fn() + Send>>>,
    state: Mutex<Value>,
}

#[derive(Default)]
struct Runs {
    map: Mutex<HashMap<String, Arc<Run>>>,
    next: AtomicU64,
}

impl Runs {
    fn get(&self, call: &Call) -> Result<(String, Arc<Run>), RpcError> {
        let id = crate::text(&call.params, "run")?.to_owned();
        let r = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .filter(|r| r.owner == call.session.id())
            .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no run {id}")))?;
        Ok((id, r))
    }
}

/// The configurations of `root`: `.idecraft/run.toml` and the ones detected from build files.
pub fn configurations(root: &Path) -> RunConfigs {
    let mut configs = load_run_configs(root);
    if let Ok(ws) = idecraft_core::Workspace::open(root)
        && let Ok(files) = ws.files()
    {
        let model = idecraft_core::read_project(root, &files);
        // The reference IDE's configurations before the detected ones (SPEC-0049 v3 behaviour 13).
        add_detected(&mut configs, idecraft_core::idea_run_configs(root));
        add_detected(&mut configs, idecraft_core::suggest_run_configs(&model));
    }
    configs
}

/// What a run marker or ⌃⇧R stands for at `line` of `doc` with `text` (`SPEC-0097`).
fn context_config(root: &Path, doc: &str, text: &str, line: usize) -> Option<RunConfig> {
    let files = idecraft_core::Workspace::open(root).ok()?.files().ok()?;
    let model = idecraft_core::read_project(root, &files);
    if let Some(lang) = idecraft_languages::jvm::Jvm::of(doc) {
        let markers = idecraft_languages::jvm::run_markers(lang, text);
        if let Some(m) = markers.iter().find(|m| m.line == line)
            && let (idecraft_languages::jvm::MarkerKind::TestMethod, Some(method)) =
                (&m.kind, &m.method)
        {
            return idecraft_core::test_method_config(&model, doc, text, method);
        }
    }
    // The test or suite around the line of a Vitest or Jest file (SPEC-0045 v4 behaviour 12).
    if let Some(m) = idecraft_languages::js_tests::marker_at(doc, text, line)
        && let Some(c) = idecraft_core::js_test_config(
            &model,
            doc,
            text,
            &m.class,
            m.kind == idecraft_languages::jvm::MarkerKind::TestClass,
        )
    {
        return Some(c);
    }
    idecraft_core::context_config(&model, doc, text)
}

pub(crate) fn trust_refusal(root: &Path) -> RpcError {
    RpcError::new(ErrorCode::Denied, idecraft_core::trust::refusal(root))
        .hint("trust the host project with run.trust (the window's trust banner)")
}

/// Joins two new streams to a run's outputs.
/// The streams of an attach (standard output, error) and the offsets they start at.
type Attached = ((u32, u32), (u64, u64));

/// New streams for the run's standard output and error, from the given offsets (or everything
/// kept); returns the streams and the offsets they start at.
fn attach(
    session: &SessionHandle,
    run: &Arc<Run>,
    from: (Option<u64>, Option<u64>),
) -> Result<Attached, RpcError> {
    let none = || RpcError::new(ErrorCode::Unavailable, "the session has no connection now");
    let out = session.open_stream().ok_or_else(none)?;
    let err = session.open_stream().ok_or_else(none)?;
    let ids = (out.id(), err.id());
    let (o, e) = (run.stdout.clone(), run.stderr.clone());
    let starts = (o.start_for(from.0), e.start_for(from.1));
    std::thread::spawn(move || o.attach(out, starts.0));
    std::thread::spawn(move || e.attach(err, starts.1));
    Ok((ids, starts))
}

/// The configurations and the name a `run.start` or `dap.plan` asks for (`name` or `context`).
pub(crate) fn requested_config(call: &Call) -> Result<(RunConfigs, String), RpcError> {
    let root = call.root.clone();
    match (&call.params["name"], &call.params["context"]) {
        (Value::String(n), _) => Ok((configurations(&root), n.clone())),
        (_, ctx @ Value::Object(_)) => {
            let b = crate::backend_of(call)?;
            let doc = crate::paths::doc_param(&root, ctx)?;
            let line = ctx["line"].as_u64().unwrap_or(0) as usize;
            let text = match b.document(&doc) {
                Some(d) => d.text,
                None => std::fs::read_to_string(crate::resolve(&root, &doc)?).map_err(internal)?,
            };
            let c = context_config(&root, &doc, &text, line).ok_or_else(|| {
                RpcError::new(
                    ErrorCode::NotFound,
                    format!("nothing to run at {doc}:{}", line + 1),
                )
            })?;
            let name = c.name.clone();
            Ok((
                RunConfigs {
                    configs: vec![c],
                    problems: Vec::new(),
                },
                name,
            ))
        }
        _ => Err(invalid("`name` or `context` is missing")),
    }
}

/// Binds `run.*`.
pub fn bind(backend: &Backend, options: &HostOptions) {
    let runs = Arc::new(Runs::default());
    let trust_file = options.trust_file.clone();
    add(backend, "run", "run.configs", Right::View, false, |call| {
        let c = configurations(&call.root);
        Ok(json!({
            "configs": c.configs, "problems": c.problems,
            "trusted": idecraft_core::trust::allows_project_code(&call.root),
        }))
    });
    add(backend, "run", "run.trust", Right::Run, true, move |call| {
        let file = trust_file
            .clone()
            .or_else(idecraft_core::trust::trust_file)
            .ok_or_else(|| {
                RpcError::new(ErrorCode::Unavailable, "no settings folder on the host")
            })?;
        idecraft_core::trust::add_location(
            &file,
            &call.root,
            idecraft_core::trust::TrustKind::Project,
        )
        .map_err(internal)?;
        // Effective now, without reading the file again.
        idecraft_core::trust::trust_for_process(&call.root);
        Ok(json!({ "trusted": true }))
    });
    let r = Arc::clone(&runs);
    add(backend, "run", "run.start", Right::Run, true, move |call| {
        let root = call.root.clone();
        if !idecraft_core::trust::allows_project_code(&root) {
            return Err(trust_refusal(&root));
        }
        let (configs, name) = requested_config(call)?;
        let handle = idecraft_core::start_host_run(&root, &configs, &name).map_err(|e| {
            if e.contains("Safe mode") {
                trust_refusal(&root)
            } else {
                RpcError::new(ErrorCode::NotFound, e)
            }
        })?;
        let id = format!("r{}", r.next.fetch_add(1, Ordering::SeqCst) + 1);
        let flag = handle.stopper();
        let run = Arc::new(Run {
            owner: call.session.id().to_owned(),
            name: name.to_owned(),
            stdout: Output::default(),
            stderr: Output::default(),
            stop: Mutex::new(Some(Box::new(move || {
                flag.store(true, Ordering::SeqCst);
            }))),
            state: Mutex::new(json!({ "state": "running" })),
        });
        r.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), Arc::clone(&run));
        {
            let (run, session, id) = (Arc::clone(&run), call.session.clone(), id.clone());
            std::thread::spawn(move || follow(handle, &run, &session, &id));
        }
        {
            let run = Arc::clone(&run);
            call.session.on_end(Box::new(move || {
                if let Some(stop) = run.stop.lock().ok().and_then(|mut s| s.take()) {
                    stop();
                }
            }));
        }
        let ((stdout, stderr), _) = attach(&call.session, &run, (Some(0), Some(0)))?;
        Ok(json!({ "run": id, "name": name, "stdout": stdout, "stderr": stderr }))
    });
    let r = Arc::clone(&runs);
    add(backend, "run", "run.stop", Right::Run, true, move |call| {
        let (id, run) = r.get(call)?;
        if let Some(stop) = run.stop.lock().ok().and_then(|mut s| s.take()) {
            stop();
        }
        Ok(json!({ "run": id, "stopping": true }))
    });
    let r = Arc::clone(&runs);
    add(
        backend,
        "run",
        "run.attach",
        Right::Run,
        false,
        move |call| {
            let (id, run) = r.get(call)?;
            let from = (
                call.params["stdout_from"].as_u64(),
                call.params["stderr_from"].as_u64(),
            );
            let ((stdout, stderr), (so, se)) = attach(&call.session, &run, from)?;
            Ok(json!({
                "run": id, "stdout": stdout, "stderr": stderr,
                "stdout_from": so, "stderr_from": se,
            }))
        },
    );
    let r = Arc::clone(&runs);
    add(
        backend,
        "run",
        "run.list",
        Right::View,
        false,
        move |call| {
            let map = r.map.lock().unwrap_or_else(|e| e.into_inner());
            let mut list: Vec<Value> = map
                .iter()
                .filter(|(_, x)| x.owner == call.session.id())
                .map(|(id, x)| {
                    let mut v = x.state.lock().map(|s| s.clone()).unwrap_or_default();
                    v["run"] = json!(id);
                    v["name"] = json!(x.name);
                    v
                })
                .collect();
            list.sort_by_key(|v| v["run"].as_str().unwrap_or_default().to_owned());
            Ok(json!({ "runs": list }))
        },
    );
}

/// Turns a run's events into stream data and `run.event` notifications until it ends.
fn follow(handle: idecraft_core::RunHandle, run: &Run, session: &SessionHandle, id: &str) {
    while let Ok(event) = handle.events.recv() {
        match event {
            RunEvent::Started { name, command } => {
                session.notify(
                    "run.event",
                    json!({ "run": id, "kind": "started", "name": name, "command": command }),
                );
            }
            RunEvent::Output { stderr, text } => {
                let mut line = text.into_bytes();
                line.push(b'\n');
                if stderr {
                    run.stderr.push(&line);
                } else {
                    run.stdout.push(&line);
                }
            }
            RunEvent::Finished(outcome) => {
                if let Some(report) = &outcome.report {
                    session.notify(
                        "run.event",
                        json!({ "run": id, "kind": "tests", "name": outcome.name, "report": report }),
                    );
                }
                let v = json!({
                    "run": id, "kind": "finished", "name": outcome.name,
                    "exit_code": outcome.exit_code, "timed_out": outcome.timed_out,
                    "stopped": outcome.stopped, "start_error": outcome.start_error,
                    "millis": outcome.millis, "success": outcome.success(),
                });
                if let Ok(mut s) = run.state.lock() {
                    *s = json!({ "state": "finished", "exit_code": outcome.exit_code,
                        "success": outcome.success() });
                }
                // The notice first: a window that hears `finished` finds it in `notice.list`
                // (`SPEC-0324` v3 behaviour 13).
                let title = if outcome.success() {
                    format!("{} finished", outcome.name)
                } else {
                    format!("{} failed", outcome.name)
                };
                session.post_notice("run", &title, None);
                session.notify("run.event", v);
            }
            RunEvent::ChainStopped { failed } => {
                session.notify(
                    "run.event",
                    json!({ "run": id, "kind": "chain_stopped", "failed": failed }),
                );
            }
        }
    }
    run.stdout.end("done");
    run.stderr.end("done");
    if let Ok(mut s) = run.stop.lock() {
        s.take();
    }
}

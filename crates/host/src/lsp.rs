//! `lsp.*` (`SPEC-0318` behaviours 7–12): the host's language servers (the pool of `SPEC-0090`)
//! on the backend's documents. The backend keeps every server's copy of a document in step with
//! the registry, so the window never sends text to a server; answers come as IDECraft's parsed
//! results with relative paths.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use idecraft_languages::insight::{self, LocationKind, Range};
use idecraft_languages::{LanguageId, actions, pool, rename, semantic};
use idecraft_session::{Backend, Call, ErrorCode, Op, Right, RpcError};
use serde_json::{Value, json};

use crate::paths::{DocCtx, char_offset, doc_ctx, relative};
use crate::{HostOptions, add, backend_of, block_on, internal, invalid, number, text};

/// How long a question waits for its server (the first start included).
const WAIT: Duration = Duration::from_secs(30);

/// How soon a document change reaches its server.
const FOLLOW: Duration = Duration::from_millis(150);

fn lsp_error(e: idecraft_languages::lsp::LspError) -> RpcError {
    let text = e.to_string();
    if matches!(e, idecraft_languages::lsp::LspError::NoServer(_)) {
        RpcError::new(ErrorCode::Unavailable, text)
            .hint("no language server for this file is installed on the host")
    } else {
        RpcError::new(ErrorCode::Internal, text)
    }
}

fn range_of(v: &Value) -> Result<Range, RpcError> {
    let n = |k: &str| {
        v[k].as_u64()
            .ok_or_else(|| invalid(format!("`range.{k}` is missing")))
    };
    Ok(Range {
        start_line: n("start_line")?,
        start_character: n("start_character")?,
        end_line: n("end_line")?,
        end_character: n("end_character")?,
    })
}

fn position(call: &Call) -> Result<(u64, u64), RpcError> {
    Ok((
        number(&call.params, "line")?,
        number(&call.params, "character")?,
    ))
}

fn ctx(call: &Call) -> Result<(Backend, DocCtx), RpcError> {
    let b = backend_of(call)?;
    let c = doc_ctx(&b, &call.params, true)?;
    Ok((b, c))
}

/// Binds `lsp.*` and, with language servers on, starts following the documents.
pub fn bind(backend: &Backend, options: &HostOptions) {
    if options.language_servers {
        pool::enable();
        follow(backend);
        // Files changed on the host's disk reach its language servers (SPEC-0042 v11
        // behaviour 20), from the backend's own watcher.
        backend.on_disk_changes(Box::new(pool::disk_changes));
    }
    add(backend, "lsp", "lsp.status", Right::View, false, |call| {
        let root = backend_of(call)?.root().unwrap_or_default();
        // Languages and the program each would use; no host paths (behaviour 3).
        let servers: Vec<Value> = idecraft_languages::servers::overview_for(&root)
            .into_iter()
            .map(|l| json!({ "language": l.language, "program": l.selected }))
            .collect();
        Ok(json!({
            "servers": servers,
            "running": pool::running(),
            "safe_mode": !idecraft_core::trust::allows_project_code(&root),
        }))
    });
    add(
        backend,
        "lsp",
        "lsp.diagnostics",
        Right::View,
        false,
        |call| {
            let b = backend_of(call)?;
            let c = doc_ctx(&b, &call.params, false)?;
            // The server gets the backend's text first; the newest publication is the answer, later
            // ones arrive as notifications.
            let text = match &c.text {
                Some(t) => t.clone(),
                None => std::fs::read_to_string(&c.file).map_err(internal)?,
            };
            block_on(pool::sync_text(&c.root, &c.file, &text))?.map_err(lsp_error)?;
            let items = pool::diagnostics_since(0)
                .1
                .into_iter()
                .find(|(p, _)| p == &c.file)
                .map(|(_, d)| d)
                .unwrap_or_default();
            Ok(json!({ "doc": c.doc, "revision": c.revision, "items": items }))
        },
    );
    add(backend, "lsp", "lsp.inspect", Right::View, false, |call| {
        let b = backend_of(call)?;
        let c = doc_ctx(&b, &call.params, true)?;
        let text = c.text.clone().unwrap_or_default();
        let findings = idecraft_languages::inspections::inspect_text(&c.file, &text);
        let items: Vec<Value> = findings
            .iter()
            .map(|f| {
                let (sl, sc) = line_col(&text, f.start);
                let (el, ec) = line_col(&text, f.end);
                json!({
                    "id": f.id, "level": format!("{:?}", f.level).to_lowercase(),
                    "message": f.message, "start_line": sl, "start_character": sc,
                    "end_line": el, "end_character": ec,
                })
            })
            .collect();
        Ok(json!({ "doc": c.doc, "revision": c.revision, "items": items }))
    });
    add(backend, "lsp", "lsp.complete", Right::View, false, |call| {
        let (_, c) = ctx(call)?;
        let (line, ch) = position(call)?;
        let limit = call.params["limit"].as_u64().unwrap_or(200) as usize;
        let items = block_on(insight::complete_in(
            &c.root,
            &c.file,
            c.text.as_deref(),
            line,
            ch,
            limit,
            WAIT,
        ))?
        .map_err(lsp_error)?;
        Ok(json!({ "revision": c.revision, "items": items }))
    });
    add(backend, "lsp", "lsp.hover", Right::View, false, |call| {
        let (_, c) = ctx(call)?;
        let (line, ch) = position(call)?;
        let h = block_on(insight::hover_in(
            &c.root,
            &c.file,
            c.text.as_deref(),
            line,
            ch,
            WAIT,
        ))?
        .map_err(lsp_error)?;
        Ok(json!({ "revision": c.revision, "hover": h }))
    });
    add(
        backend,
        "lsp",
        "lsp.signature",
        Right::View,
        false,
        |call| {
            let (_, c) = ctx(call)?;
            let (line, ch) = position(call)?;
            let s = block_on(insight::signature_help_in(
                &c.root,
                &c.file,
                c.text.as_deref(),
                line,
                ch,
                WAIT,
            ))?
            .map_err(lsp_error)?;
            Ok(json!({ "revision": c.revision, "signature": s }))
        },
    );
    add(
        backend,
        "lsp",
        "lsp.locations",
        Right::View,
        false,
        |call| {
            let (_, c) = ctx(call)?;
            let (line, ch) = position(call)?;
            let kind = match call.params["kind"].as_str().unwrap_or("definition") {
                "definition" => LocationKind::Definition,
                "references" => LocationKind::References,
                "type_definition" => LocationKind::TypeDefinition,
                "implementation" => LocationKind::Implementation,
                other => return Err(invalid(format!("unknown kind {other:?}"))),
            };
            let found = block_on(insight::found_in(
                &c.root,
                &c.file,
                c.text.as_deref(),
                line,
                ch,
                kind,
                WAIT,
            ))?
            .map_err(lsp_error)?;
            Ok(locations_json(&c.root, c.revision, &found))
        },
    );
    add(backend, "lsp", "lsp.symbols", Right::View, false, |call| {
        let b = backend_of(call)?;
        let c = doc_ctx(&b, &call.params, false)?;
        let symbols = block_on(insight::symbols_in(
            &c.root,
            &c.file,
            c.text.as_deref(),
            WAIT,
        ))?
        .map_err(lsp_error)?;
        // An inherited member's file is a host path; it goes out as a relative path or not at all.
        let list: Vec<Value> = symbols
            .iter()
            .map(|s| {
                let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
                if let Some(p) = &s.path {
                    v["path"] = relative(&c.root, p).map_or(Value::Null, Value::String);
                }
                v
            })
            .collect();
        Ok(json!({ "revision": c.revision, "symbols": list }))
    });
    add(backend, "lsp", "lsp.semantic", Right::View, false, |call| {
        let (_, c) = ctx(call)?;
        let tokens = block_on(semantic::semantic_tokens_in(
            &c.root,
            &c.file,
            c.text.as_deref(),
            WAIT,
        ))?
        .map_err(lsp_error)?;
        Ok(json!({ "revision": c.revision, "tokens": tokens }))
    });
    add(
        backend,
        "lsp",
        "lsp.code_actions",
        Right::View,
        false,
        |call| {
            let (_, c) = ctx(call)?;
            let range = range_of(&call.params["range"])?;
            let list = block_on(actions::code_actions_in(
                &c.root,
                &c.file,
                c.text.as_deref(),
                &range,
                WAIT,
            ))?
            .map_err(lsp_error)?;
            Ok(json!({ "revision": c.revision, "actions": list }))
        },
    );
    add(
        backend,
        "lsp",
        "lsp.apply_action",
        Right::View,
        true,
        |call| {
            let (b, c) = ctx(call)?;
            let range = range_of(&call.params["range"])?;
            let title = text(&call.params, "title")?;
            let files = block_on(actions::plan_code_action_in(
                &c.root,
                &c.file,
                c.text.as_deref(),
                &range,
                title,
                WAIT,
            ))?
            .map_err(lsp_error)?;
            let apply = call.params["apply"].as_bool() == Some(true);
            if apply && !session_may_edit(call) {
                return Err(not_granted_edit());
            }
            let planned = plan_json(&c.root, &files)?;
            if !apply {
                return Ok(json!({ "files": planned, "applied": false }));
            }
            let changed = apply_files(&b, &c.root, &files, "lsp")?;
            Ok(json!({ "files": planned, "applied": true, "changed": changed }))
        },
    );
    add(backend, "lsp", "lsp.rename", Right::View, true, |call| {
        let (b, c) = ctx(call)?;
        let (line, ch) = position(call)?;
        let new_name = text(&call.params, "new_name")?;
        let plan = block_on(rename::plan_rename_in(
            &c.root,
            &c.file,
            c.text.as_deref(),
            line,
            ch,
            new_name,
            WAIT,
        ))?
        .map_err(lsp_error)?;
        let apply = call.params["apply"].as_bool() == Some(true);
        if apply && !session_may_edit(call) {
            return Err(not_granted_edit());
        }
        let planned = plan_json(&c.root, &plan.files)?;
        let mut out = json!({
            "old_name": plan.old_name, "new_name": plan.new_name, "files": planned,
            "applied": false,
        });
        if apply {
            out["changed"] = json!(apply_files(&b, &c.root, &plan.files, "lsp")?);
            out["applied"] = json!(true);
        }
        Ok(out)
    });
}

/// Whether the calling session may change documents (`apply: true` needs `edit`).
fn session_may_edit(call: &Call) -> bool {
    call.rights.contains(&Right::Edit)
}

fn not_granted_edit() -> RpcError {
    RpcError::new(
        ErrorCode::NotGranted,
        "not granted: applying edits needs the right `edit`",
    )
    .hint("this session was opened without that right")
}

/// 0-based line and character (Unicode scalars) of byte offset `at`.
pub(crate) fn line_col(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let before = &text[..at];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count())
}

fn locations_json(root: &Path, revision: Option<u64>, found: &insight::Found) -> Value {
    let mut locations = Vec::new();
    let mut outside: Vec<String> = found.outside.clone();
    for l in &found.locations {
        match relative(root, &l.path) {
            Some(doc) => locations.push(json!({ "doc": doc, "range": l.range })),
            None => outside.push(crate::paths::outside_name(&l.path)),
        }
    }
    json!({ "revision": revision, "locations": locations, "outside": outside })
}

/// Files of a plan as relative paths with their edit counts; a file outside the project refuses
/// the whole plan (behaviour 12, SEC-14).
fn plan_json(root: &Path, files: &[rename::FileEdits]) -> Result<Vec<Value>, RpcError> {
    files
        .iter()
        .map(|f| {
            let doc = relative(root, &f.path).ok_or_else(|| {
                invalid(format!(
                    "the change touches {}; nothing was changed",
                    crate::paths::outside_name(&f.path)
                ))
            })?;
            Ok(json!({ "doc": doc, "edits": f.edits.len() }))
        })
        .collect()
}

/// The edits of one file as document operations on `text` (sorted, checked against the old
/// text).
fn ops_of(text: &str, edits: &[rename::Edit]) -> Result<Vec<Op>, RpcError> {
    let mut ops = Vec::new();
    for e in edits {
        let a = char_offset(text, e.range.start_line, e.range.start_character);
        let z = char_offset(text, e.range.end_line, e.range.end_character);
        let (Some(a), Some(z)) = (a, z) else {
            return Err(RpcError::new(
                ErrorCode::Conflict,
                "an edit lies outside the document",
            ));
        };
        let old: String = text.chars().skip(a).take(z.saturating_sub(a)).collect();
        if old != e.old_text {
            return Err(RpcError::new(
                ErrorCode::Conflict,
                "the document changed since the plan; nothing was changed",
            )
            .hint("ask again"));
        }
        ops.push(Op {
            at: a,
            delete: z - a,
            insert: e.new_text.clone(),
        });
    }
    ops.sort_by_key(|o| o.at);
    if ops.windows(2).any(|w| w[0].at + w[0].delete > w[1].at) {
        return Err(RpcError::new(ErrorCode::Conflict, "overlapping edits"));
    }
    Ok(ops)
}

/// Carries out `files`: open documents through an edit of `author`, other files written on the
/// host through a temporary file (mode kept, local history). Everything is checked first.
pub(crate) fn apply_files(
    backend: &Backend,
    root: &Path,
    files: &[rename::FileEdits],
    author: &str,
) -> Result<Vec<String>, RpcError> {
    plan_json(root, files)?;
    enum Step {
        Doc(String, Vec<Op>),
        File(PathBuf, String, String),
    }
    let mut steps = Vec::new();
    for f in files {
        let doc = relative(root, &f.path).unwrap_or_default();
        match backend.document(&doc) {
            Some(d) => steps.push(Step::Doc(doc, ops_of(&d.text, &f.edits)?)),
            None => {
                let path = crate::resolve(root, &doc)?;
                let old = std::fs::read_to_string(&path).map_err(internal)?;
                let new = rename::apply_edits(&old, &f.edits)
                    .map_err(|e| RpcError::new(ErrorCode::Conflict, e))?;
                steps.push(Step::File(path, doc, new));
            }
        }
    }
    let mut changed = Vec::new();
    for s in steps {
        match s {
            Step::Doc(doc, ops) => {
                backend.host_edit(&doc, author, &ops)?;
                changed.push(doc);
            }
            Step::File(path, doc, text) => {
                write_keeping_mode(&path, &text).map_err(internal)?;
                if let Ok(mut h) = idecraft_core::LocalHistory::open(root) {
                    let _ = h.record(&doc);
                }
                changed.push(doc);
            }
        }
    }
    Ok(changed)
}

fn write_keeping_mode(path: &Path, text: &str) -> std::io::Result<()> {
    let mode = std::fs::metadata(path).map(|m| m.permissions()).ok();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".idecraft-{}", std::process::id()));
    let tmp = path.with_file_name(name);
    let r = std::fs::write(&tmp, text)
        .and_then(|()| match &mode {
            Some(m) => std::fs::set_permissions(&tmp, m.clone()),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// Keeps the servers' copies of the open documents in step with the registry and sends every
/// publication of diagnostics to the clients that have the document open (behaviours 7, 8).
fn follow(backend: &Backend) {
    let weak = backend.downgrade();
    std::thread::Builder::new()
        .name("host-lsp-follow".into())
        .spawn(move || {
            let mut synced: HashMap<String, u64> = HashMap::new();
            let mut since = 0;
            let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, PathBuf, Option<String>)>();
            // Server starts can take seconds; they happen on a thread of their own.
            std::thread::spawn(move || {
                while let Ok((root, file, text)) = rx.recv() {
                    let _ = block_on(async {
                        match text {
                            Some(t) => {
                                let _ = pool::sync_text(&root, &file, &t).await;
                            }
                            None => pool::close_text(&file).await,
                        }
                    });
                }
            });
            loop {
                std::thread::sleep(FOLLOW);
                let Some(b) = weak.upgrade() else { break };
                let Some(root) = b.root() else { continue };
                let open: HashSet<String> = b.documents().into_iter().map(|(d, _)| d).collect();
                for (doc, rev) in b.documents() {
                    if synced.get(&doc) == Some(&rev) {
                        continue;
                    }
                    let file = root.join(&doc);
                    if LanguageId::detect(&file).is_none() {
                        synced.insert(doc, rev);
                        continue;
                    }
                    if let Some(d) = b.document(&doc) {
                        let _ = tx.send((root.clone(), file, Some(d.text)));
                    }
                    synced.insert(doc, rev);
                }
                let gone: Vec<String> = synced
                    .keys()
                    .filter(|d| !open.contains(*d))
                    .cloned()
                    .collect();
                for doc in gone {
                    synced.remove(&doc);
                    let _ = tx.send((root.clone(), root.join(&doc), None));
                }
                let (now, changed) = pool::diagnostics_since(since);
                since = now;
                for (path, items) in changed {
                    let Some(doc) = relative(&root, &path) else {
                        continue;
                    };
                    if !open.contains(&doc) {
                        continue;
                    }
                    let revision = b.document(&doc).map(|d| d.revision);
                    b.notify_doc(
                        &doc,
                        "lsp.diagnostics",
                        json!({ "doc": doc, "revision": revision, "items": items }),
                    );
                }
            }
        })
        .ok();
}

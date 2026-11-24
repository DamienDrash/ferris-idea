//! `search.*` (`SPEC-0318` behaviours 13–14): Find in Files, Go to File and Search Everywhere on
//! the host project, with the backend's text for documents that have unsaved changes. Find in
//! Files searches the project's own sources as the local search does (`SPEC-0029` v5 behaviour
//! 13).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use ferris_idea_core::{Finder, Query, searchable_text};
use ferris_idea_languages::everywhere::{Categories, Sources, TextOptions, search_everywhere};
use ferris_idea_languages::syntax::SearchContext;
use ferris_idea_session::{Backend, ErrorCode, Right, RpcError};
use serde_json::{Value, json};

use crate::{add, backend_of, internal, invalid, text};

/// Matches returned at most unless asked for fewer.
const MAX_MATCHES: usize = 1000;

fn workspace(backend: &Backend) -> Result<ferris_idea_core::Workspace, RpcError> {
    let root = backend
        .root()
        .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
    ferris_idea_core::Workspace::open(&root).map_err(internal)
}

/// Binds `search.*`.
pub fn bind(backend: &Backend) {
    add(
        backend,
        "search",
        "search.text",
        Right::View,
        false,
        |call| {
            let b = backend_of(call)?;
            let ws = workspace(&b)?;
            let p = &call.params;
            let query = Query {
                pattern: text(p, "query")?.to_owned(),
                regex: p["regex"].as_bool() == Some(true),
                match_case: p["case_sensitive"].as_bool() == Some(true),
                whole_words: p["whole_words"].as_bool() == Some(true),
            };
            if query.pattern.is_empty() {
                return Err(invalid("`query` is empty"));
            }
            let finder = Finder::new(&query).map_err(|e| invalid(e.to_string()))?;
            let max = p["max"].as_u64().map_or(MAX_MATCHES, |m| m as usize).max(1);
            let mask = p["mask"].as_str().filter(|m| !m.is_empty());
            let scope = ferris_idea_core::SearchScope {
                mask: mask.map(str::to_owned),
                place: place(&ws, p)?,
                include_non_project: p["include_non_project"].as_bool() == Some(true),
            };
            // Unsaved text of open documents wins over the disk.
            let open: HashMap<String, String> = b
                .documents()
                .into_iter()
                .filter_map(|(d, _)| b.document(&d).map(|i| (d, i.text)))
                .collect();
            let context = match p["context"].as_str() {
                None => SearchContext::Anywhere,
                Some(id) => SearchContext::from_id(id)
                    .ok_or_else(|| invalid(format!("`{id}` is no search context")))?,
            };
            // The same listing as the local search (SPEC-0029 v5 behaviour 13): the project's own
            // sources unless a place was named or everything was asked for.
            let (candidates, left_out) = ferris_idea_core::search_candidates(&ws, &scope);
            let mut files = Vec::new();
            let (mut total, mut skipped, mut truncated) = (0usize, 0usize, false);
            for rel in candidates {
                if call.cancel.stopped().is_some() {
                    break;
                }
                let rel_text = rel.to_string_lossy().replace('\\', "/");
                let content = match open.get(&rel_text) {
                    Some(t) => Some(t.clone()),
                    None => searchable_text(&ws, &rel),
                };
                let Some(content) = content else {
                    skipped += 1;
                    continue;
                };
                let mut found = finder.find(&content);
                // Before `max` counts them (SPEC-0318 v5 behaviour 40).
                let ranges = found.iter().map(|m| m.start..m.end).collect();
                let kept = context.filter(&rel, &content, ranges);
                found.retain(|m| kept.contains(&(m.start..m.end)));
                if found.is_empty() {
                    continue;
                }
                if total + found.len() > max {
                    found.truncate(max - total);
                    truncated = true;
                }
                total += found.len();
                let matches: Vec<Value> = found
                    .iter()
                    .map(|m| {
                        json!({
                            "line": m.line, "column": m.column, "length": m.end - m.start,
                            "start": m.start, "text": m.line_text,
                        })
                    })
                    .collect();
                files.push(json!({
                    "doc": rel_text, "matches": matches, "unsaved": open.contains_key(&rel_text),
                }));
                if truncated {
                    break;
                }
            }
            let mut v = json!({
                "files": files, "matches": total, "skipped": skipped, "truncated": truncated,
            });
            let folders = left_out.iter().filter(|o| o.folder).count();
            if let Some(line) =
                ferris_idea_core::not_searched_line(folders, left_out.len() - folders)
            {
                v["not_searched"] = json!(line);
            }
            Ok(v)
        },
    );
    add(
        backend,
        "search",
        "search.excerpt",
        Right::View,
        false,
        |call| {
            let b = backend_of(call)?;
            let ws = workspace(&b)?;
            let doc = ferris_idea_session::paths::check_relative(text(&call.params, "path")?)?;
            crate::resolve(&call.root, &doc)?;
            let line = call.params["line"].as_u64().unwrap_or(0) as usize;
            let around = call.params["context"].as_u64().unwrap_or(5).min(20) as usize;
            let content = b
                .document(&doc)
                .map(|d| d.text)
                .or_else(|| searchable_text(&ws, std::path::Path::new(&doc)))
                .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("{doc} has no text")))?;
            let first = line.saturating_sub(around);
            let lines: Vec<&str> = content
                .split('\n')
                .skip(first)
                .take(line - first + around + 1)
                .collect();
            Ok(json!({ "first": first, "lines": lines }))
        },
    );
    add(
        backend,
        "search",
        "search.files",
        Right::View,
        false,
        |call| {
            let b = backend_of(call)?;
            let ws = workspace(&b)?;
            let query = text(&call.params, "query")?;
            let limit = call.params["limit"].as_u64().unwrap_or(50) as usize;
            let files: Vec<PathBuf> = ws.files().map_err(internal)?;
            let names = FileNames::of(&ws, &call.params, true)?;
            let found = everywhere(
                &ws,
                &files,
                &names,
                query,
                Categories {
                    files: true,
                    ..Categories::NONE
                },
                limit,
                None,
            );
            let mut v = json!({ "files": found.files.clone().unwrap_or_default() });
            if let Some(line) = &found.not_listed {
                v["not_listed"] = json!(line);
            }
            with_positions(&mut v, &found);
            Ok(v)
        },
    );
    add(
        backend,
        "search",
        "search.everywhere",
        Right::View,
        false,
        |call| {
            let b = backend_of(call)?;
            let ws = workspace(&b)?;
            let query = text(&call.params, "query")?;
            let limit = call.params["limit"].as_u64().unwrap_or(30) as usize;
            let wanted: Vec<&str> = call.params["categories"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_else(|| vec!["files", "symbols", "text"]);
            let categories = Categories {
                files: wanted.contains(&"files"),
                classes: wanted.contains(&"classes"),
                symbols: wanted.contains(&"symbols"),
                text: wanted.contains(&"text"),
                // Actions are the window's own.
                actions: false,
            };
            let files: Vec<PathBuf> = ws.files().map_err(internal)?;
            let names = FileNames::of(&ws, &call.params, categories.files)?;
            let found = everywhere(&ws, &files, &names, query, categories, limit, None);
            // Every hit carries a workspace-relative path already (SPEC-0031).
            let mut v = serde_json::to_value(&found).map_err(internal)?;
            with_positions(&mut v, &found);
            Ok(v)
        },
    );
}

/// Where `search.text` looks (`SPEC-0318` v8 behaviour 13): the files named in `files` whatever
/// they are (the window's Open Files scope), else below `folder`, else the whole project. Every
/// path from the window is checked as a relative path inside the project; a named file outside
/// the project folder or behind a symbolic link is not searched.
fn place(
    ws: &ferris_idea_core::Workspace,
    p: &Value,
) -> Result<ferris_idea_core::SearchPlace, RpcError> {
    if let Some(list) = p["files"].as_array() {
        let own = ferris_idea_core::OwnSources::new(ws.root());
        let mut files = Vec::new();
        for f in list {
            let f = f
                .as_str()
                .ok_or_else(|| invalid("`files` holds relative paths"))?;
            let f = ferris_idea_session::paths::check_relative(f)?;
            // No symbolic link anywhere on the way, also not inside `.venv` or `node_modules`.
            if let Ok(f) = own.readable(std::path::Path::new(&f)) {
                files.push(f);
            }
        }
        return Ok(ferris_idea_core::SearchPlace::Files(files));
    }
    Ok(
        match p["folder"].as_str().filter(|f| !f.is_empty() && *f != ".") {
            Some(f) => ferris_idea_core::SearchPlace::Directory(PathBuf::from(
                ferris_idea_session::paths::check_relative(f)?,
            )),
            None => ferris_idea_core::SearchPlace::Project,
        },
    )
}

/// Adds the highlight positions of the hits (v2 protocol detail 8; they are not part of the
/// result's own serialisation, which agents read).
fn with_positions(v: &mut Value, found: &ferris_idea_languages::everywhere::Everywhere) {
    let put = |list: &mut Value, positions: Vec<&Vec<usize>>| {
        if let Some(a) = list.as_array_mut() {
            for (hit, p) in a.iter_mut().zip(positions) {
                if !p.is_empty() {
                    hit["positions"] = json!(p);
                }
            }
        }
    };
    if let Some(f) = &found.files {
        put(&mut v["files"], f.iter().map(|h| &h.positions).collect());
    }
    if let Some(c) = &found.classes {
        put(&mut v["classes"], c.iter().map(|h| &h.positions).collect());
    }
    if let Some(s) = &found.symbols {
        put(&mut v["symbols"], s.iter().map(|h| &h.positions).collect());
    }
}

/// The files of the files category (`SPEC-0318` v8 behaviour 14, `SPEC-0031` v5 behaviour 12):
/// the project's own sources unless `include_non_project` is true, the window's recently opened
/// files (`recent`, relative paths) in any case, and the line on what was left out.
struct FileNames {
    list: Option<Vec<PathBuf>>,
    recent: Vec<PathBuf>,
    not_listed: Option<String>,
}

impl FileNames {
    fn of(
        ws: &ferris_idea_core::Workspace,
        params: &Value,
        wanted: bool,
    ) -> Result<Self, RpcError> {
        let mut recent = Vec::new();
        for p in params["recent"].as_array().into_iter().flatten() {
            let Some(p) = p.as_str() else {
                continue;
            };
            recent.push(PathBuf::from(ferris_idea_session::paths::check_relative(
                p,
            )?));
        }
        if !wanted {
            return Ok(Self {
                list: None,
                recent,
                not_listed: None,
            });
        }
        let own = ferris_idea_core::OwnSources::new(ws.root());
        Ok(if params["include_non_project"].as_bool() == Some(true) {
            Self {
                list: Some(own.everything(None)),
                recent,
                not_listed: None,
            }
        } else {
            let listing = own.listing(None);
            Self {
                not_listed: listing.not_listed_line(),
                list: Some(listing.files),
                recent,
            }
        })
    }
}

fn everywhere(
    ws: &ferris_idea_core::Workspace,
    files: &[PathBuf],
    names: &FileNames,
    query: &str,
    categories: Categories,
    limit: usize,
    cancel: Option<&AtomicBool>,
) -> ferris_idea_languages::everywhere::Everywhere {
    let keymap = ferris_idea_ui::keymap::Keymap::bundled_default();
    let mut found = search_everywhere(
        Sources {
            root: ws.root(),
            files,
            keymap: &keymap,
            extra_actions: &[],
            recent: &names.recent,
            text_options: TextOptions::default(),
            file_names: names.list.as_deref(),
        },
        query,
        categories,
        limit,
        cancel,
    );
    found.not_listed = names.not_listed.clone();
    found
}

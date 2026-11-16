//! `git.*` (`SPEC-0318` behaviours 15–16): the host repository through IDECraft's own Git library
//! (no `git` process for these). Paths are relative to the project folder on the wire; a
//! repository path outside the project (a project in a sub-folder of its repository) goes out as a
//! file name with `outside: true`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use idecraft_session::{Backend, ErrorCode, Right, RpcError};
use idecraft_vcs::{CommitOptions, Repo};
use serde_json::{Value, json};

use crate::{HostOptions, add, backend_of, internal, invalid, text};

/// The most commits one `git.commits_touching` looks at.
const MAX_TOUCHING: usize = 5000;

pub(crate) fn git_error(e: idecraft_vcs::GitError) -> RpcError {
    let s = e.to_string();
    if s.contains("not a Git repository") || s.contains("NotARepository") {
        RpcError::new(ErrorCode::NotFound, s).hint("the host project is not in a Git repository")
    } else {
        RpcError::new(ErrorCode::Conflict, s)
    }
}

/// The repository of the project and the project's path inside its working tree (`""` when the
/// project is the working tree).
pub(crate) fn open(backend: &Backend) -> Result<(Repo, PathBuf, String), RpcError> {
    let root = backend
        .root()
        .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
    let repo = Repo::discover(&root).map_err(git_error)?;
    let work = repo
        .workdir()
        .and_then(|w| std::fs::canonicalize(w).ok())
        .ok_or_else(|| RpcError::new(ErrorCode::NotFound, "the repository has no working tree"))?;
    let prefix = idecraft_session::paths::relative(&work, &root)
        .filter(|p| p != ".")
        .unwrap_or_default();
    Ok((repo, root, prefix))
}

/// A project-relative path as the repository names it.
pub(crate) fn to_repo(prefix: &str, doc: &str) -> String {
    if prefix.is_empty() {
        doc.to_owned()
    } else {
        format!("{prefix}/{doc}")
    }
}

/// Rewrites every `path` string of `v` from repository-relative to project-relative.
fn map_paths(v: &mut Value, prefix: &str) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if (k == "path" || k == "old_path") && x.is_string() {
                    let p = x.as_str().unwrap_or_default().to_owned();
                    *x = if prefix.is_empty() {
                        Value::String(p)
                    } else {
                        match p.strip_prefix(&format!("{prefix}/")) {
                            Some(rest) => Value::String(rest.to_owned()),
                            None => json!({
                                "outside": Path::new(&p).file_name()
                                    .map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            }),
                        }
                    };
                } else {
                    map_paths(x, prefix);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| map_paths(x, prefix)),
        _ => {}
    }
}

pub(crate) fn out<T: serde::Serialize>(value: T, prefix: &str) -> Result<Value, RpcError> {
    let mut v = serde_json::to_value(value).map_err(internal)?;
    map_paths(&mut v, prefix);
    Ok(v)
}

/// The repository and the checked project path a request names; the path is checked first, so a
/// path leaving the project is refused before Git is asked (SEC-14).
fn open_doc(call: &idecraft_session::Call) -> Result<(Repo, String, String), RpcError> {
    let doc = idecraft_session::paths::check_relative(text(&call.params, "path")?)?;
    crate::resolve(&call.root, &doc)?;
    let (repo, _, prefix) = open(&backend_of(call)?)?;
    Ok((repo, prefix, doc))
}

/// Binds `git.*` and the watcher of `git.changed`.
pub fn bind(backend: &Backend, options: &HostOptions) {
    add(backend, "git", "git.status", Right::View, false, |call| {
        let (repo, _, prefix) = open(&backend_of(call)?)?;
        let status = repo.status().map_err(git_error)?;
        let branch = repo.current_branch().map_err(git_error)?;
        Ok(json!({ "branch": branch, "files": out(status, &prefix)? }))
    });
    add(backend, "git", "git.base", Right::View, false, |call| {
        let (repo, prefix, doc) = open_doc(call)?;
        let text = repo.head_text(&to_repo(&prefix, &doc)).map_err(git_error)?;
        Ok(json!({ "path": doc, "text": text }))
    });
    add(backend, "git", "git.diff", Right::View, false, |call| {
        let (repo, prefix, doc) = open_doc(call)?;
        let rows = repo
            .side_by_side_file(&to_repo(&prefix, &doc))
            .map_err(git_error)?;
        Ok(json!({ "path": doc, "rows": out(rows, &prefix)? }))
    });
    add(backend, "git", "git.log", Right::View, false, |call| {
        let (repo, _, prefix) = open(&backend_of(call)?)?;
        let limit = call.params["limit"].as_u64().unwrap_or(50).min(1000) as usize;
        let log = match call.params["branch"].as_str() {
            Some(b) => repo.log_from(b, limit),
            None => repo.log(limit),
        }
        .map_err(git_error)?;
        Ok(json!({ "commits": out(log, &prefix)? }))
    });
    // The Git library only, like `git.log` (`SPEC-0318` v6 behaviour 47).
    add(
        backend,
        "git",
        "git.commits_touching",
        Right::View,
        false,
        |call| {
            let (repo, _, prefix) = open(&backend_of(call)?)?;
            let list = |key: &str| {
                call.params[key]
                    .as_array()
                    .ok_or_else(|| invalid(format!("`{key}` is missing")))
                    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            };
            let ids: Vec<String> = list("ids")?
                .into_iter()
                .take(MAX_TOUCHING)
                .map(str::to_owned)
                .collect();
            // Checked as paths only: a deleted file's history is the point of the filter.
            let paths: Vec<String> = list("paths")?
                .into_iter()
                .map(|p| idecraft_session::paths::check_relative(p).map(|p| to_repo(&prefix, &p)))
                .collect::<Result<_, _>>()?;
            let found = repo.commits_touching(&ids, &paths).map_err(git_error)?;
            let touching: Vec<&String> = ids.iter().filter(|i| found.contains(*i)).collect();
            Ok(json!({ "ids": touching }))
        },
    );
    add(backend, "git", "git.show", Right::View, false, |call| {
        let (repo, _, prefix) = open(&backend_of(call)?)?;
        let id = text(&call.params, "id")?;
        if id.starts_with('-') || id.contains(['\n', ' ']) {
            return Err(invalid("`id` is not a revision"));
        }
        let mut details = out(repo.commit_details(id).map_err(git_error)?, &prefix)?;
        // The window reads this as a typed result: a file outside the project stays a name.
        crate::git_ops::tidy(&mut details, &prefix, &call.root);
        Ok(details)
    });
    add(backend, "git", "git.branches", Right::View, false, |call| {
        let (repo, _, prefix) = open(&backend_of(call)?)?;
        out(repo.branches().map_err(git_error)?, &prefix)
    });
    add(
        backend,
        "git",
        "git.commit",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, root, prefix) = open(&backend_of(call)?)?;
            let message = text(&call.params, "message")?;
            if message.trim().is_empty() {
                return Err(invalid("the commit message is empty"));
            }
            let paths: Vec<String> = call.params["paths"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default()
                .into_iter()
                .map(|p| idecraft_session::paths::check_relative(p).map(|p| to_repo(&prefix, &p)))
                .collect::<Result<_, _>>()?;
            // Hooks are project code: they run only for a trusted host project (behaviour 5).
            let trusted = idecraft_core::trust::allows_project_code(&root);
            let options = CommitOptions {
                verify: trusted,
                amend: call.params["amend"].as_bool() == Some(true),
                author: None,
            };
            let dry_run = call.params["dry_run"].as_bool() == Some(true);
            let outcome = repo
                .commit_opts(message, &paths, dry_run, &options)
                .map_err(git_error)?;
            let mut v = out(outcome, &prefix)?;
            if !trusted {
                v["note"] = json!("hooks were skipped: the host project is not trusted");
            }
            Ok(v)
        },
    );
    add(
        backend,
        "git",
        "git.create_branch",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, _, prefix) = open(&backend_of(call)?)?;
            let name = text(&call.params, "name")?;
            let start = call.params["start"].as_str();
            out(repo.create_branch(name, start).map_err(git_error)?, &prefix)
        },
    );
    add(
        backend,
        "git",
        "git.checkout",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, _, prefix) = open(&backend_of(call)?)?;
            let name = text(&call.params, "name")?;
            let create = call.params["create"].as_bool() == Some(true);
            out(repo.checkout(name, create).map_err(git_error)?, &prefix)
        },
    );
    watch(backend, options);
}

/// The modification times that change with HEAD, the index and the refs.
fn stamp(git_dir: &Path) -> Vec<Option<SystemTime>> {
    ["HEAD", "index", "packed-refs", "refs/heads", "ORIG_HEAD"]
        .iter()
        .map(|f| {
            std::fs::metadata(git_dir.join(f))
                .and_then(|m| m.modified())
                .ok()
        })
        .collect()
}

/// Sends `git.changed {branch, head}` when the host repository moves (behaviour 15).
fn watch(backend: &Backend, options: &HostOptions) {
    let weak = backend.downgrade();
    let every = options.poll;
    std::thread::Builder::new()
        .name("host-git-watch".into())
        .spawn(move || {
            let mut last: Option<Vec<Option<SystemTime>>> = None;
            loop {
                std::thread::sleep(every);
                let Some(b) = weak.upgrade() else { break };
                if b.sessions() == 0 {
                    continue;
                }
                let Ok((repo, root, _)) = open(&b) else {
                    continue;
                };
                let now = stamp(&root.join(repo.git_dir()));
                if last.as_ref().is_some_and(|l| *l != now) {
                    let branch = repo.current_branch().ok().flatten();
                    let head = repo.short_commit_id("HEAD").ok();
                    b.broadcast("git.changed", json!({ "branch": branch, "head": head }));
                }
                last = Some(now);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_paths_become_project_paths() {
        let mut v = json!({"files": [{"path": "app/src/a.rs"}, {"path": "lib/b.rs"}], "x": {"path": "app/c"}});
        map_paths(&mut v, "app");
        assert_eq!(v["files"][0]["path"], "src/a.rs");
        assert_eq!(v["files"][1]["path"], json!({"outside": "b.rs"}));
        assert_eq!(v["x"]["path"], "c");
        let mut same = json!([{"path": "a"}]);
        map_paths(&mut same, "");
        assert_eq!(same[0]["path"], "a");
        assert_eq!(to_repo("app", "x.rs"), "app/x.rs");
        assert_eq!(to_repo("", "x.rs"), "x.rs");
    }
}

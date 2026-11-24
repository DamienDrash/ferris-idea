//! The remaining `git.*` methods (`SPEC-0318` v5 behaviours 31–39): fetch, pull and push with the
//! host's own credentials, rollback, stash and the actions of the log's commit menu.
//!
//! Everything but `git.rollback` starts a `git` process, which reads the repository's configuration
//! and may run programs named there, so an untrusted host project refuses it (behaviour 39). No
//! request or answer carries a credential; `git` is told not to ask for one, and a missing one comes
//! back as `auth_required` (behaviour 32). Options and URLs cannot be passed in: remotes must be
//! configured ones, revisions and names may not look like options.

use std::path::Path;

use ferris_idea_session::{Backend, Call, ErrorCode, Right, RpcError};
use ferris_idea_vcs::{
    CommitAction, OpOutcome, PushOptions, REMOTE_TIMEOUT, Repo, ResetMode, StashOptions, TodoEntry,
    valid_branch_name,
};
use serde_json::{Value, json};

use crate::git::{git_error, open, out, to_repo};
use crate::{add, backend_of, invalid, text};

/// `Denied` with the trust message unless the host project's code may run (behaviour 39).
fn need_trust(call: &Call) -> Result<(), RpcError> {
    if ferris_idea_core::trust::allows_project_code(&call.root) {
        Ok(())
    } else {
        Err(RpcError::new(
            ErrorCode::Denied,
            ferris_idea_core::trust::refusal(&call.root),
        )
        .hint("trust the host project with run.trust (the window's trust banner)"))
    }
}

/// `text` without `user:password@` of URLs and with the project's path replaced by `.`
/// (behaviour 34).
pub(crate) fn scrub(text: &str, project: &Path) -> String {
    let text = text.replace(&project.display().to_string(), ".");
    let mut clean = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        clean.push_str(head);
        let end = tail
            .find(|c: char| c == '/' || c == '\'' || c == '"' || c.is_whitespace())
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(at) => {
                clean.push_str("***@");
                rest = &tail[at + 1..];
            }
            None => rest = tail,
        }
    }
    clean.push_str(rest);
    clean
}

/// A repository path as the project names it, for lists the window only shows.
fn shown_path(path: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return path.to_owned();
    }
    match path.strip_prefix(&format!("{prefix}/")) {
        Some(rest) => rest.to_owned(),
        None => format!(
            "…/{}",
            Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
    }
}

/// The messages cleaned, path lists made project-relative, and a file outside the project given
/// by a `…/name` the typed results of the window can read.
pub(crate) fn tidy(v: &mut Value, prefix: &str, project: &Path) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if let Value::Object(inner) = x
                    && inner.len() == 1
                    && let Some(name) = inner.get("outside").and_then(Value::as_str)
                {
                    *x = Value::String(format!("…/{name}"));
                    continue;
                }
                match (k.as_str(), &mut *x) {
                    ("output" | "messages" | "ssh_refusal", Value::String(s)) => {
                        *s = scrub(s, project);
                    }
                    (
                        "conflicts" | "restored" | "unstaged" | "deleted" | "local_conflicts"
                        | "merge_conflicts",
                        Value::Array(a),
                    ) => {
                        for p in a.iter_mut() {
                            if let Value::String(s) = p {
                                *s = shown_path(s, prefix);
                            }
                        }
                    }
                    _ => tidy(x, prefix, project),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| tidy(x, prefix, project)),
        _ => {}
    }
}

fn finish<T: serde::Serialize>(value: T, prefix: &str, project: &Path) -> Result<Value, RpcError> {
    let mut v = out(value, prefix)?;
    tidy(&mut v, prefix, project);
    Ok(v)
}

/// A revision from the window: never something Git could read as an option.
fn revision<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    let id = text(params, key)?;
    if id.is_empty() || id.len() > 256 || id.starts_with('-') || id.contains(char::is_whitespace) {
        return Err(invalid(format!("`{key}` is not a revision")));
    }
    Ok(id)
}

fn opt_revision<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, RpcError> {
    match params[key] {
        Value::Null => Ok(None),
        _ => revision(params, key).map(Some),
    }
}

/// A branch or tag name from the window.
fn ref_name<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, RpcError> {
    match params[key].as_str() {
        None => Ok(None),
        Some(n) if valid_branch_name(n) => Ok(Some(n)),
        Some(_) => Err(invalid(format!("`{key}` is not a valid name"))),
    }
}

/// A project-relative path from the window as the repository names it; refused when it leaves the
/// project (SEC-14).
fn repo_path(call: &Call, key: &str, prefix: &str) -> Result<String, RpcError> {
    let doc = ferris_idea_session::paths::check_relative(text(&call.params, key)?)?;
    crate::resolve(&call.root, &doc)?;
    Ok(to_repo(prefix, &doc))
}

/// The configured remote named by `remote`, if one is named.
fn configured_remote(repo: &Repo, params: &Value) -> Result<Option<String>, RpcError> {
    let Some(name) = params["remote"].as_str() else {
        return Ok(None);
    };
    if repo
        .remotes()
        .map_err(git_error)?
        .iter()
        .any(|r| r.name == name)
    {
        Ok(Some(name.to_owned()))
    } else {
        Err(invalid(format!("`{name}` is no configured remote")))
    }
}

/// The repository and the prefix, after the trust check.
fn opened(call: &Call) -> Result<(Repo, String, Backend), RpcError> {
    need_trust(call)?;
    let backend = backend_of(call)?;
    let (repo, _, prefix) = open(&backend)?;
    Ok((repo, prefix, backend))
}

fn op(
    call: &Call,
    done: Result<OpOutcome, ferris_idea_vcs::GitError>,
    prefix: &str,
) -> Result<Value, RpcError> {
    finish(done.map_err(git_error)?, prefix, &call.root)
}

fn index(params: &Value) -> Result<usize, RpcError> {
    params["index"]
        .as_u64()
        .map(|i| i as usize)
        .ok_or_else(|| invalid("`index` is missing"))
}

/// Binds the methods of this module.
pub fn bind(backend: &Backend) {
    network(backend);
    rollback_and_stash(backend);
    log_actions(backend);
    branch_actions(backend);
}

/// A branch name from the window; a missing or odd one is `invalid`.
fn branch<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    ref_name(params, key)?.ok_or_else(|| invalid(format!("`{key}` is missing")))
}

/// The Branches popup's actions on the host (`SPEC-0318` v6 behaviours 44–46): each starts a `git`
/// process, so all need a trusted host project (behaviour 39).
fn branch_actions(backend: &Backend) {
    add(backend, "git", "git.merge", Right::VcsWrite, true, |call| {
        let (repo, prefix, _) = opened(call)?;
        let source = revision(&call.params, "source")?;
        op(call, repo.merge_stopping(source), &prefix)
    });
    add(
        backend,
        "git",
        "git.rebase_onto",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let onto = revision(&call.params, "onto")?;
            let done = match ref_name(&call.params, "branch")? {
                Some(branch) => repo.checkout_and_rebase(branch, onto),
                None => repo.rebase(onto),
            };
            op(call, done, &prefix)
        },
    );
    add(
        backend,
        "git",
        "git.rename_branch",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let (old, new) = (branch(&call.params, "old")?, branch(&call.params, "new")?);
            op(call, repo.rename_branch(old, new), &prefix)
        },
    );
    add(
        backend,
        "git",
        "git.delete_branch",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let name = branch(&call.params, "name")?;
            op(call, repo.delete_branch(name), &prefix)
        },
    );
}

fn network(backend: &Backend) {
    add(backend, "git", "git.fetch", Right::VcsWrite, true, |call| {
        let (repo, prefix, _) = opened(call)?;
        let remote = configured_remote(&repo, &call.params)?;
        let done = repo
            .fetch(remote.as_deref(), REMOTE_TIMEOUT)
            .map_err(git_error)?;
        finish(done, &prefix, &call.root)
    });
    add(backend, "git", "git.pull", Right::VcsWrite, true, |call| {
        let (repo, prefix, _) = opened(call)?;
        let dry_run = call.params["dry_run"].as_bool() == Some(true);
        let done = if call.params["rebase"].as_bool() == Some(true) {
            repo.pull_rebase(dry_run, REMOTE_TIMEOUT)
        } else {
            repo.pull(dry_run, REMOTE_TIMEOUT)
        }
        .map_err(git_error)?;
        finish(done, &prefix, &call.root)
    });
    add(
        backend,
        "git",
        "git.reset_upstream",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let done = repo
                .reset_to_upstream(false, REMOTE_TIMEOUT)
                .map_err(git_error)?;
            finish(done, &prefix, &call.root)
        },
    );
    add(backend, "git", "git.outgoing", Right::View, false, |call| {
        let (repo, prefix, _) = opened(call)?;
        let branch = ref_name(&call.params, "branch")?;
        let up_to = opt_revision(&call.params, "up_to")?;
        let list = repo.outgoing_of(branch, up_to).map_err(git_error)?;
        finish(list, &prefix, &call.root)
    });
    add(backend, "git", "git.push", Right::Push, true, |call| {
        let (repo, prefix, _) = opened(call)?;
        let p = &call.params;
        let options = PushOptions {
            target: ref_name(p, "target")?.map(str::to_owned),
            set_upstream: p["set_upstream"].as_bool() == Some(true),
            tags: p["tags"].as_bool() == Some(true),
            force_with_lease: p["force_with_lease"].as_bool() == Some(true),
            branch: ref_name(p, "branch")?.map(str::to_owned),
            up_to: opt_revision(p, "up_to")?.map(str::to_owned),
        };
        let done = repo.push_with(&options, REMOTE_TIMEOUT).map_err(|e| {
            call.session.post_notice("git", "Push failed", None);
            git_error(e)
        })?;
        call.session.post_notice("git", "Push finished", None);
        finish(done, &prefix, &call.root)
    });
}

fn rollback_and_stash(backend: &Backend) {
    // The Git library only: no process starts, so it needs no trust (behaviour 39).
    add(
        backend,
        "git",
        "git.rollback",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, _, prefix) = open(&backend_of(call)?)?;
            let paths: Vec<String> = call.params["paths"]
                .as_array()
                .ok_or_else(|| invalid("`paths` is missing"))?
                .iter()
                .map(|p| {
                    let p = p.as_str().ok_or_else(|| invalid("`paths` holds no text"))?;
                    let doc = ferris_idea_session::paths::check_relative(p)?;
                    crate::resolve(&call.root, &doc)?;
                    Ok(to_repo(&prefix, &doc))
                })
                .collect::<Result<_, RpcError>>()?;
            let done = repo
                .rollback(
                    &paths,
                    call.params["delete_added"].as_bool() == Some(true),
                    call.params["dry_run"].as_bool() == Some(true),
                )
                .map_err(git_error)?;
            finish(done, &prefix, &call.root)
        },
    );
    add(backend, "git", "git.stashes", Right::View, false, |call| {
        let (repo, prefix, _) = opened(call)?;
        let list = repo.stashes().map_err(git_error)?;
        Ok(json!({ "stashes": finish(list, &prefix, &call.root)? }))
    });
    add(
        backend,
        "git",
        "git.stash_files",
        Right::View,
        false,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let list = repo.stash_files(index(&call.params)?).map_err(git_error)?;
            Ok(json!({ "files": finish(list, &prefix, &call.root)? }))
        },
    );
    add(
        backend,
        "git",
        "git.stash_push",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let p = &call.params;
            let options = StashOptions {
                message: p["message"].as_str().map(str::to_owned),
                include_untracked: p["include_untracked"].as_bool() == Some(true),
                keep_index: p["keep_index"].as_bool() == Some(true),
            };
            let done = repo.stash_push_with(&options);
            op(call, done, &prefix)
        },
    );
    add(
        backend,
        "git",
        "git.stash_apply",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let pop = call.params["pop"].as_bool() == Some(true);
            let done = repo.stash_apply(index(&call.params)?, pop);
            op(call, done, &prefix)
        },
    );
    add(
        backend,
        "git",
        "git.stash_drop",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let done = repo.stash_drop(index(&call.params)?);
            op(call, done, &prefix)
        },
    );
}

/// The outcome of a step that has none of its own.
fn done_output(output: String) -> OpOutcome {
    OpOutcome {
        ok: true,
        conflicts: Vec::new(),
        in_progress: None,
        output,
    }
}

fn log_actions(backend: &Backend) {
    add(
        backend,
        "git",
        "git.commit_action",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let p = &call.params;
            let id = revision(p, "id")?;
            let name = || ref_name(p, "name")?.ok_or_else(|| invalid("`name` is missing"));
            let action = text(p, "action")?;
            let action = CommitAction::from_id(action)
                .ok_or_else(|| invalid(format!("`{action}` is no commit action")))?;
            let done = match action {
                CommitAction::CherryPick => repo.cherry_pick(id),
                CommitAction::Checkout => repo.checkout_revision(id),
                CommitAction::Reset => {
                    let mode = ResetMode::from_id(p["mode"].as_str().unwrap_or("mixed"))
                        .ok_or_else(|| invalid("`mode` is no reset mode"))?;
                    repo.reset_to(id, mode)
                }
                CommitAction::Revert => repo.revert_commit(id),
                CommitAction::Undo => repo.undo_commit(id),
                CommitAction::Drop => repo.drop_commit(id),
                CommitAction::EditMessage => {
                    let message = text(p, "message")?;
                    if message.trim().is_empty() {
                        return Err(invalid("the commit message is empty"));
                    }
                    repo.reword_commit(id, message)
                }
                CommitAction::NewBranch => repo
                    .create_branch(name()?, Some(id))
                    .map(|_| done_output(format!("created branch at {id}"))),
                CommitAction::NewTag => repo
                    .create_tag(name()?, id)
                    .map(|()| done_output(format!("created tag at {id}"))),
                other => {
                    return Err(invalid(format!(
                        "`{}` does not run on the host",
                        other.id()
                    )));
                }
            };
            op(call, done, &prefix)
        },
    );
    add(backend, "git", "git.patch", Right::View, false, |call| {
        let (repo, _, _) = opened(call)?;
        let patch = repo
            .commit_patch(revision(&call.params, "id")?)
            .map_err(git_error)?;
        Ok(json!({ "patch": patch }))
    });
    add(
        backend,
        "git",
        "git.commit_diff",
        Right::View,
        false,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let id = revision(&call.params, "id")?;
            let path = repo_path(call, "path", &prefix)?;
            let rows = repo.side_by_side_in_commit(id, &path).map_err(git_error)?;
            Ok(json!({ "rows": finish(rows, &prefix, &call.root)? }))
        },
    );
    add(
        backend,
        "git",
        "git.fixup_message",
        Right::View,
        false,
        |call| {
            let (repo, _, _) = opened(call)?;
            let squash = call.params["squash"].as_bool() == Some(true);
            let message = repo
                .fixup_message(revision(&call.params, "id")?, squash)
                .map_err(git_error)?;
            Ok(json!({ "message": message }))
        },
    );
    add(backend, "git", "git.files_at", Right::View, false, |call| {
        let (repo, prefix, _) = opened(call)?;
        let files: Vec<String> = repo
            .files_at(revision(&call.params, "id")?)
            .map_err(git_error)?
            .iter()
            .map(|p| shown_path(p, &prefix))
            .collect();
        Ok(json!({ "files": files }))
    });
    add(backend, "git", "git.text_at", Right::View, false, |call| {
        let (repo, prefix, _) = opened(call)?;
        let id = revision(&call.params, "id")?;
        let path = repo_path(call, "path", &prefix)?;
        let text = repo.text_at(id, &path).map_err(git_error)?;
        Ok(json!({ "text": text }))
    });
    add(
        backend,
        "git",
        "git.worktree_changes",
        Right::View,
        false,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let list = repo
                .changes_against_worktree(revision(&call.params, "id")?)
                .map_err(git_error)?;
            Ok(json!({ "changes": finish(list, &prefix, &call.root)? }))
        },
    );
    add(
        backend,
        "git",
        "git.worktree_text",
        Right::View,
        false,
        |call| {
            let b = backend_of(call)?;
            let doc = ferris_idea_session::paths::check_relative(text(&call.params, "path")?)?;
            let file = crate::resolve(&call.root, &doc)?;
            // The document's own text first; a saved file only up to the search limit.
            if let Some(d) = b.document(&doc) {
                return Ok(json!({ "text": d.text }));
            }
            let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            if size > 10 * 1024 * 1024 {
                return Err(RpcError::new(
                    ErrorCode::TooLarge,
                    format!("{doc} is too large"),
                ));
            }
            Ok(json!({ "text": std::fs::read_to_string(&file).unwrap_or_default() }))
        },
    );
    add(
        backend,
        "git",
        "git.rebase_todo",
        Right::View,
        false,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let onto = revision(&call.params, "onto")?;
            let entries = repo.rebase_todo(onto).map_err(git_error)?;
            let label = repo.short_commit_id(onto).ok();
            Ok(json!({ "entries": finish(entries, &prefix, &call.root)?, "short": label }))
        },
    );
    add(
        backend,
        "git",
        "git.rebase",
        Right::VcsWrite,
        true,
        |call| {
            let (repo, prefix, _) = opened(call)?;
            let onto = revision(&call.params, "onto")?;
            let entries: Vec<TodoEntry> = serde_json::from_value(call.params["entries"].clone())
                .map_err(|e| invalid(format!("`entries`: {e}")))?;
            let done = repo.rebase_interactive(onto, &entries);
            op(call, done, &prefix)
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_lose_passwords_of_urls_and_the_project_path() {
        let p = Path::new("/home/u/proj");
        assert_eq!(
            scrub(
                "fatal: 'https://me:tok3n@x.example/r.git' at /home/u/proj/a",
                p
            ),
            "fatal: 'https://***@x.example/r.git' at ./a"
        );
        assert_eq!(scrub("ssh://git@h/x", p), "ssh://***@h/x");
        assert_eq!(
            scrub("see https://x.example/a@b", p),
            "see https://x.example/a@b"
        );
    }

    #[test]
    fn lists_and_outside_paths_are_tidied() {
        let mut v = json!({
            "conflicts": ["app/a.rs", "lib/b.rs"],
            "file": { "outside": "c.rs" },
            "output": "https://a:b@h/x",
        });
        tidy(&mut v, "app", Path::new("/p"));
        assert_eq!(v["conflicts"], json!(["a.rs", "…/b.rs"]));
        assert_eq!(v["file"], "…/c.rs");
        assert_eq!(v["output"], "https://***@h/x");
    }
}

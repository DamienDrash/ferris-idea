//! The remaining host actions of a remote project in the window (`SPEC-0318` v5 behaviours 31–42):
//! fetch, pull, push, stash and the log's commit actions on the host, and the Ports tab of the
//! Services window. The dialogs are the local ones; only the work runs on the host, and the
//! editors follow through the session. Nothing here reads this machine's stand-in folder.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, Window, prelude::*};
use idecraft_vcs::{
    CommitAction, GitError, OpOutcome, PushOptions, ResetMode, StashOptions, TodoEntry,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{HostCall, WorkbenchView};

/// Time limit of a host Git method: above the host's own limit for a network operation, so the
/// host's answer (with what happened) arrives rather than a timeout here.
const GIT_LIMIT: Duration = Duration::from_secs(150);

/// An answer of the host as a typed result; a refusal stays a Git error for the local reports.
fn typed<T: DeserializeOwned>(r: Result<Value, String>) -> Result<T, GitError> {
    r.map_err(GitError::Git)
        .and_then(|v| serde_json::from_value(v).map_err(|e| GitError::Git(e.to_string())))
}

impl WorkbenchView {
    fn remote_git_call(&self) -> Option<HostCall> {
        let live = self.remote_project.as_ref()?.live.clone();
        Some(Arc::new(move |method: &str, params: Value| {
            live.host_call(method, params, GIT_LIMIT)
                .map_err(|e| e.to_string())
        }))
    }

    /// Runs `method` on the host; the status bar shows `label…`, then `report`'s text. The window
    /// follows through `git.changed` and the documents' own events.
    fn remote_git(
        &mut self,
        label: String,
        method: &'static str,
        params: Value,
        report: impl FnOnce(&mut Self, Result<Value, String>) -> String + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.remote_git_call() else {
            return;
        };
        self.status = Some(format!("{label}…"));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { host(method, params) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                let text = report(this, done);
                // A credential the host lacks is the host's to set up.
                let host = this.remote_host_label();
                this.status = Some(text.replace(
                    "or a credential helper",
                    &format!("or a credential helper on {host}"),
                ));
                if let Some(r) = this.remote_project.as_mut() {
                    r.host.branch_asked = false;
                }
                this.remote_git_moved(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// A stash, cherry-pick or rebase on the host with the local report; conflicts are named and
    /// continued in a host terminal (behaviour 38).
    pub(super) fn remote_git_operation(
        &mut self,
        label: String,
        method: &'static str,
        params: Value,
        cx: &mut Context<Self>,
    ) {
        let name = label.clone();
        self.remote_git(
            label,
            method,
            params,
            move |_, done| {
                let done: Result<OpOutcome, GitError> = typed(done);
                let mut text = crate::git_ops_model::report(&name, &done);
                if matches!(&done, Ok(o) if o.in_progress.is_some()) {
                    text.push_str("; continue in a terminal on the host");
                }
                text
            },
            cx,
        );
    }

    /// Fetches every remote of the host repository (`git.fetch`).
    pub(super) fn remote_fetch(&mut self, cx: &mut Context<Self>) {
        self.remote_git(
            "Fetch".into(),
            "git.fetch",
            json!({}),
            |_, done| crate::vcs_ops_model::fetch_report(&typed(done)),
            cx,
        );
    }

    /// Update Project on the host (`git.pull`); what came in opens the Update Info popup.
    pub(super) fn remote_pull(&mut self, rebase: bool, cx: &mut Context<Self>) {
        self.remote_git(
            "Update".into(),
            "git.pull",
            json!({ "rebase": rebase }),
            |this, done| {
                let done: Result<idecraft_vcs::PullOutcome, GitError> = typed(done);
                if let Some(info) = done
                    .as_ref()
                    .ok()
                    .and_then(|p| p.info.clone())
                    .filter(|i| !i.commits.is_empty() || !i.files.is_empty())
                {
                    this.git_ui.last_update_info = Some(info.clone());
                    this.git_ui.update_info_next = Some(info);
                }
                crate::vcs_ops_model::pull_report(&done)
            },
            cx,
        );
    }

    /// Update Project's reset to the upstream on the host (`git.reset_upstream`).
    pub(super) fn remote_reset_upstream(&mut self, cx: &mut Context<Self>) {
        self.remote_git(
            "Reset".into(),
            "git.reset_upstream",
            json!({}),
            |_, done| crate::vcs_ops_model::reset_report(&typed(done)),
            cx,
        );
    }

    /// The Push popup for the host's outgoing commits (`git.outgoing`).
    pub(super) fn remote_open_push(
        &mut self,
        branch: Option<String>,
        up_to: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.remote_host_call() else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            host("git.outgoing", json!({ "branch": branch, "up_to": up_to }))
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let listed = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let outgoing = typed(listed).map_err(|e: GitError| e.to_string());
                    this.show_push(outgoing, window, cx);
                });
            });
        })
        .detach();
    }

    /// Pushes on the host with the Push popup's options (`git.push`).
    pub(super) fn remote_push(&mut self, o: PushOptions, count: usize, cx: &mut Context<Self>) {
        self.remote_git(
            "Push".into(),
            "git.push",
            json!({
                "branch": o.branch, "target": o.target, "set_upstream": o.set_upstream,
                "tags": o.tags, "force_with_lease": o.force_with_lease, "up_to": o.up_to,
            }),
            move |_, done| crate::vcs_ops_model::push_report(&typed(done), count),
            cx,
        );
    }

    /// The stash popup on the host's stashes (`git.stashes`).
    pub(super) fn remote_open_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.remote_host_call() else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { host("git.stashes", json!({})) });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let listed = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let stashes = listed
                        .map(|mut v| v["stashes"].take())
                        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
                    match stashes {
                        Ok(s) => this.show_stashes(s, window, cx),
                        Err(e) => {
                            this.status = Some(e);
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    /// Stashes on the host with the stash dialog's options.
    pub(super) fn remote_stash_push(&mut self, o: StashOptions, cx: &mut Context<Self>) {
        self.remote_git_operation(
            "Stash".into(),
            "git.stash_push",
            json!({
                "message": o.message, "include_untracked": o.include_untracked,
                "keep_index": o.keep_index,
            }),
            cx,
        );
    }

    /// A stash popup choice on the host; `true` keeps the popup's focus as local Show does.
    pub(super) fn remote_stash_event(
        &mut self,
        event: crate::StashEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match event {
            crate::StashEvent::Push(untracked) => self.remote_git_operation(
                "Stash".into(),
                "git.stash_push",
                json!({ "include_untracked": untracked }),
                cx,
            ),
            crate::StashEvent::Apply(i, pop) => self.remote_git_operation(
                if pop { "Pop stash" } else { "Apply stash" }.into(),
                "git.stash_apply",
                json!({ "index": i, "pop": pop }),
                cx,
            ),
            crate::StashEvent::Drop(i) => self.remote_git_operation(
                "Drop stash".into(),
                "git.stash_drop",
                json!({ "index": i }),
                cx,
            ),
            crate::StashEvent::Show(i) => {
                self.remote_show_stash_diff(i, window, cx);
                return true;
            }
            crate::StashEvent::Close => {}
        }
        false
    }

    fn remote_show_stash_diff(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.remote_host_call() else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            let files = host("git.stash_files", json!({ "index": index }))?;
            let listed = host("git.stashes", json!({}))?;
            Ok::<_, String>((files, listed))
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let decoded = done.and_then(|(mut files, listed)| {
                        let message = listed["stashes"][index]["message"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        serde_json::from_value::<Vec<idecraft_vcs::StashFile>>(
                            files["files"].take(),
                        )
                        .map(|f| (message, f))
                        .map_err(|e| e.to_string())
                    });
                    match decoded {
                        Ok((message, files)) => {
                            this.present_stash_diff(index, &message, &files, window, cx)
                        }
                        Err(e) => {
                            this.status = Some(e);
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    /// An entry of the Git log's commit menu for the host repository (`SPEC-0318` v5 behaviour
    /// 37).
    pub(super) fn remote_commit_action(
        &mut self,
        commit: String,
        action: CommitAction,
        mode: Option<ResetMode>,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let short = commit[..commit.len().min(7)].to_owned();
        match action {
            CommitAction::CopyRevision | CommitAction::GoToChild | CommitAction::GoToParent => {}
            CommitAction::CherryPick
            | CommitAction::Checkout
            | CommitAction::Reset
            | CommitAction::Revert
            | CommitAction::Undo
            | CommitAction::Drop
            | CommitAction::NewBranch
            | CommitAction::NewTag => {
                let mode = mode.unwrap_or(ResetMode::Mixed);
                let label = match action {
                    CommitAction::CherryPick => format!("Cherry-pick {short}"),
                    CommitAction::Checkout => format!("Checkout {short}"),
                    CommitAction::Reset => format!("Reset ({}) to {short}", mode.id()),
                    CommitAction::Revert => format!("Revert {short}"),
                    CommitAction::Undo => format!("Undo commit {short}"),
                    CommitAction::Drop => format!("Drop {short}"),
                    CommitAction::NewTag => format!("New tag at {short}"),
                    _ => format!("New branch at {short}"),
                };
                self.remote_git_operation(
                    label,
                    "git.commit_action",
                    json!({
                        "action": action.id(), "id": commit, "mode": mode.id(), "name": name,
                    }),
                    cx,
                );
            }
            CommitAction::CopyPatch => self.remote_read(
                "git.patch",
                json!({ "id": commit }),
                move |this, v, _, cx| {
                    let patch = v["patch"].as_str().unwrap_or_default().to_owned();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(patch));
                    this.status = Some(format!("Copied {short} as a patch"));
                },
                window,
                cx,
            ),
            CommitAction::ShowDiff => {
                let id = commit.clone();
                self.remote_read(
                    "git.show",
                    json!({ "id": commit }),
                    move |this, v, window, cx| {
                        let Some(path) = v["changes"][0]["path"].as_str().map(str::to_owned) else {
                            this.status = Some(format!("{short} changes no file"));
                            return;
                        };
                        let shown = path.clone();
                        let commit = id.clone();
                        this.remote_read(
                            "git.commit_diff",
                            json!({ "id": id, "path": path }),
                            move |this, mut v, window, cx| match serde_json::from_value(
                                v["rows"].take(),
                            ) {
                                Ok(rows) => {
                                    this.present_commit_diff(&commit, &shown, rows, window, cx)
                                }
                                Err(e) => this.status = Some(e.to_string()),
                            },
                            window,
                            cx,
                        );
                    },
                    window,
                    cx,
                );
            }
            CommitAction::Fixup | CommitAction::SquashInto => {
                let squash = action == CommitAction::SquashInto;
                self.remote_read(
                    "git.fixup_message",
                    json!({ "id": commit, "squash": squash }),
                    |this, v, window, cx| {
                        let message = v["message"].as_str().unwrap_or_default().to_owned();
                        this.run_action("CheckinProject", window, cx);
                        if let Some(view) = this.commit.clone() {
                            view.update(cx, |c, cx| c.set_message(&message, cx));
                        }
                        this.status =
                            Some(format!("Choose the files for \"{message}\" and commit"));
                    },
                    window,
                    cx,
                );
            }
            CommitAction::EditMessage => {
                self.remote_open_rebase(format!("{commit}^"), Some(commit), window, cx)
            }
            CommitAction::RebaseFromHere => {
                self.remote_open_rebase(format!("{commit}^"), None, window, cx)
            }
            CommitAction::PushUpTo => self.open_push_of(None, Some(commit), window, cx),
            CommitAction::ShowRepository => self.remote_show_repository(commit, window, cx),
            CommitAction::CompareLocal => self.remote_revision_changes(commit, window, cx),
        }
        cx.notify();
    }

    /// Reads `method` from the host, then `then` on the window's thread; a refusal goes to the
    /// status bar.
    fn remote_read(
        &mut self,
        method: &'static str,
        params: Value,
        then: impl FnOnce(&mut Self, Value, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.remote_host_call() else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { host(method, params) });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    match done {
                        Ok(v) => then(this, v, window, cx),
                        Err(e) => this.status = Some(e),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// The interactive rebase dialog on the host's commits since `onto`; with `reword` the
    /// commit's message field open (Edit Commit Message).
    pub(super) fn remote_open_rebase(
        &mut self,
        onto: String,
        reword: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let base = onto.clone();
        self.remote_read(
            "git.rebase_todo",
            json!({ "onto": onto }),
            move |this, mut v, window, cx| {
                let entries: Vec<TodoEntry> = match serde_json::from_value(v["entries"].take()) {
                    Ok(e) => e,
                    Err(e) => {
                        this.status = Some(e.to_string());
                        return;
                    }
                };
                if entries.is_empty() {
                    this.status = Some("Interactive rebase: no commits since the base".into());
                    return;
                }
                let label = crate::git_ops_model::rebase_base_label(&base, v["short"].as_str());
                this.show_rebase(base, label, entries, None, window, cx);
                if let (Some(id), Some(dialog)) = (reword, this.rebase.clone()) {
                    dialog.update(cx, |d, cx| d.preset_reword(&id, window, cx));
                }
            },
            window,
            cx,
        );
    }

    /// Starts the interactive rebase of the dialog on the host.
    pub(super) fn remote_rebase(
        &mut self,
        onto: String,
        entries: Vec<TodoEntry>,
        cx: &mut Context<Self>,
    ) {
        self.remote_git_operation(
            "Interactive rebase".into(),
            "git.rebase",
            json!({ "onto": onto, "entries": entries }),
            cx,
        );
    }

    /// Show Repository at Revision on the host's files.
    fn remote_show_repository(&mut self, rev: String, window: &mut Window, cx: &mut Context<Self>) {
        self.remote_read(
            "git.files_at",
            json!({ "id": rev }),
            move |this, v, window, cx| {
                let files: Vec<String> = v["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect();
                let short = rev[..rev.len().min(7)].to_owned();
                let shown = files.clone();
                let on_choose: super::revisions::OnChoose = Rc::new(move |this, i, window, cx| {
                    let Some(path) = files.get(i).cloned() else {
                        return;
                    };
                    let (rev, shown) = (rev.clone(), path.clone());
                    this.remote_read(
                        "git.text_at",
                        json!({ "id": rev, "path": path }),
                        move |this, v, window, cx| {
                            let text = v["text"].as_str().unwrap_or_default();
                            this.present_file_at(&rev, &shown, text, window, cx);
                        },
                        window,
                        cx,
                    );
                });
                this.revision_popup(format!("Files at {short}"), shown, on_choose, window, cx);
            },
            window,
            cx,
        );
    }

    /// Compare with Local: the host's saved files against `rev`.
    fn remote_revision_changes(
        &mut self,
        rev: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remote_read(
            "git.worktree_changes",
            json!({ "id": rev }),
            move |this, mut v, window, cx| {
                let changes: Vec<idecraft_vcs::ChangedPath> =
                    match serde_json::from_value(v["changes"].take()) {
                        Ok(c) => c,
                        Err(e) => {
                            this.status = Some(e.to_string());
                            return;
                        }
                    };
                let short = rev[..rev.len().min(7)].to_owned();
                let items = changes
                    .iter()
                    .map(|c| format!("{}  {}", c.status, c.path))
                    .collect();
                let paths: Vec<String> = changes.into_iter().map(|c| c.path).collect();
                let title = format!("Differences between {short} and the working tree");
                let on_choose: super::revisions::OnChoose = Rc::new(move |this, i, window, cx| {
                    let Some(path) = paths.get(i).cloned() else {
                        return;
                    };
                    let (rev, shown, label) = (rev.clone(), path.clone(), short.clone());
                    this.remote_read(
                        "git.text_at",
                        json!({ "id": rev, "path": path }),
                        move |this, v, window, cx| {
                            let old = v["text"].as_str().unwrap_or_default().to_owned();
                            let again = shown.clone();
                            this.remote_read(
                                "git.worktree_text",
                                json!({ "path": shown }),
                                move |this, v, window, cx| {
                                    let new = v["text"].as_str().unwrap_or_default();
                                    this.present_worktree_diff(
                                        &label, &again, &old, new, window, cx,
                                    );
                                },
                                window,
                                cx,
                            );
                        },
                        window,
                        cx,
                    );
                });
                this.revision_popup(title, items, on_choose, window, cx);
            },
            window,
            cx,
        );
    }

    // ---------------------------------------------------------------------------------------
    // The Ports tab of the Services window (behaviour 42)

    /// Opens the Services window on its Ports tab.
    pub fn show_remote_ports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.ensure_services(window, cx);
        self.show_services = true;
        view.update(cx, |v, cx| v.show_ports(cx));
        self.remote_refresh_services_ports(cx);
        cx.notify();
    }

    /// Reads the host's ports and this window's forwards into the Services window.
    pub(super) fn remote_refresh_services_ports(&mut self, cx: &mut Context<Self>) {
        let (Some(view), Some(host)) = (self.services.clone(), self.remote_host_call()) else {
            return;
        };
        let (label, folder) = self.remote_project().unwrap_or_default();
        let task = cx
            .background_executor()
            .spawn(async move { host("port.list", json!({})) });
        cx.spawn(async move |_, cx| {
            let Ok(listed) = task.await else {
                return;
            };
            let forwards = idecraft_remote::ports::list(&label, &folder);
            let rows = port_rows(&listed, &forwards);
            let _ = view.update(cx, |v, cx| v.set_ports(label, rows, cx));
        })
        .detach();
    }

    /// A row's button in the Ports tab.
    pub(super) fn services_port_event(
        &mut self,
        event: &crate::services_view::ServicesEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::services_view::ServicesEvent as E;
        match event {
            E::ForwardPort(port) => self.remote_forward_port(*port, false, window, cx),
            E::StopForward(id) => {
                let id = id.clone();
                self.remote_close_forward(&id, cx);
            }
            E::OpenForward(local) => cx.open_url(&format!("http://127.0.0.1:{local}")),
            _ => {}
        }
    }
}

/// The rows of the Ports tab: every open forward, then the detected ports without one.
pub(super) fn port_rows(listed: &Value, forwards: &[Value]) -> Vec<crate::services_view::PortRow> {
    let mut rows: Vec<crate::services_view::PortRow> = forwards
        .iter()
        .map(|f| crate::services_view::PortRow {
            port: f["remote_port"].as_u64().unwrap_or_default() as u16,
            kind: "forwarded".into(),
            command: String::new(),
            forward: Some((
                f["forward"].as_str().unwrap_or_default().to_owned(),
                f["local_port"].as_u64().unwrap_or_default() as u16,
            )),
        })
        .collect();
    for d in listed["detected"].as_array().into_iter().flatten() {
        let Some(port) = d["port"].as_u64().and_then(|p| u16::try_from(p).ok()) else {
            continue;
        };
        if rows.iter().any(|r| r.port == port) {
            continue;
        }
        rows.push(crate::services_view::PortRow {
            port,
            kind: d["kind"].as_str().unwrap_or("service").to_owned(),
            command: d["command"].as_str().unwrap_or_default().to_owned(),
            forward: None,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0318 v5 behaviour 42: a forward once, the detected ports without one, debug kept.
    #[test]
    fn port_rows_list_forwards_first_and_each_port_once() {
        let listed = json!({ "detected": [
            { "port": 8080, "kind": "service", "command": "python -m http.server" },
            { "port": 5005, "kind": "debug", "command": "java -agentlib:jdwp" },
        ]});
        let forwards = [json!({ "remote_port": 8080, "local_port": 18080, "forward": "f1" })];
        let rows = port_rows(&listed, &forwards);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].port, 8080);
        assert_eq!(rows[0].forward, Some(("f1".to_owned(), 18080)));
        assert_eq!(rows[1].port, 5005);
        assert_eq!(rows[1].kind, "debug");
        assert_eq!(rows[1].forward, None);
    }

    #[test]
    fn host_answers_become_the_local_results() {
        let done: Result<OpOutcome, GitError> = typed(Ok(json!({
            "ok": false, "conflicts": ["a.rs"], "in_progress": "cherry-pick", "output": "x",
        })));
        let o = done.unwrap();
        assert_eq!(o.conflicts, ["a.rs"]);
        assert!(o.in_progress.is_some());
        let refused: Result<OpOutcome, GitError> = typed(Err("Safe mode: …".into()));
        assert!(refused.unwrap_err().to_string().contains("Safe mode"));
        let fetched: Result<idecraft_vcs::RemoteOutcome, GitError> = typed(Ok(json!({
            "ok": false, "command": ["git", "fetch"], "updates": [], "auth_required": true,
            "timed_out": false, "messages": "fatal",
        })));
        let report = crate::vcs_ops_model::fetch_report(&fetched);
        assert!(report.contains("credentials"), "{report}");
    }
}

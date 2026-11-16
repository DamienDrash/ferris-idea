//! The seventh package of workbench actions that exist under their keymap id
//! (`docs/research/KEYMAP_COVERAGE.md`): the chooser of the file's custom regions
//! (`GotoCustomRegion`, `SPEC-0083` v9 behaviour 24), the end of input for the program of the Run
//! tool window (`SendEOF`, `SPEC-0087` v10 behaviour 18), the list of the background tasks that
//! can be stopped (`StopBackgroundProcesses`, `SPEC-0061` v16 behaviour 62) and the type of the
//! expression at the caret (`ExpressionTypeInfo`, `SPEC-0077` v7 behaviour 6). The first and
//! last row of a list are in [`crate::list_nav`]; the decisions without a window are in
//! [`crate::keymap_actions`] and [`crate::doc_model`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{AppContext, Context, Window};

use super::WorkbenchView;
use crate::keymap_actions::{self, BackgroundTask};

impl WorkbenchView {
    /// `GotoCustomRegion`: the custom regions of the active file in a list, the one around the
    /// caret selected; the chosen one gets the caret on its first line and the place left is
    /// remembered for Back. False without an editor; a file without regions says so.
    pub(super) fn goto_custom_region(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let (regions, line) = {
            let e = editor.read(cx);
            (e.custom_regions(), e.status().line.saturating_sub(1))
        };
        let Some(selected) = idecraft_languages::folding::region_at(&regions, line) else {
            self.status = Some(keymap_actions::NO_CUSTOM_REGIONS.into());
            cx.notify();
            return true;
        };
        let items = regions.iter().map(|r| r.label()).collect();
        self.choose_from(
            keymap_actions::CUSTOM_REGION_TITLE.into(),
            items,
            selected,
            window,
            cx,
            move |this, i, window, cx| {
                let Some(region) = regions.get(i) else {
                    return;
                };
                // The file may have been closed or another one made active meanwhile.
                let Some(editor) = this.editors.get(&path).cloned() else {
                    return;
                };
                this.remember_place(cx);
                this.open(&path, window, cx);
                let line = region.start_line;
                editor.update(cx, |e, cx| {
                    // The caret in front of the marker's first sign, not in the indentation.
                    let column = e
                        .state
                        .buffer
                        .line(line)
                        .map(|t| t.chars().take_while(|c| c.is_whitespace()).count())
                        .unwrap_or(0);
                    e.go_to(line, column, cx)
                });
                this.focus_active(window, cx);
                cx.notify();
            },
        );
        true
    }

    /// `SendEOF`: the end of input for the program of the Run tool window's selected tab; the
    /// status bar says what happened. False without a Run tool window.
    pub(super) fn send_eof(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.run_view.clone() else {
            return false;
        };
        let sent = run.update(cx, |r, _| r.send_eof());
        self.status = Some(keymap_actions::eof_message(sent).into());
        cx.notify();
        true
    }

    /// The background tasks of this window that run now and can be stopped, each with the row
    /// the list shows for it (`SPEC-0061` v16 behaviour 62).
    pub fn background_tasks(&self, cx: &gpui::App) -> Vec<(BackgroundTask, String)> {
        BackgroundTask::ALL
            .into_iter()
            .filter_map(|task| {
                let detail = match task {
                    BackgroundTask::CloneRepository => {
                        Some(self.clone_job.as_ref()?.1.as_str().to_owned())
                    }
                    BackgroundTask::RemoteOpen => self.remote_opening.as_ref().map(|_| None)?,
                    BackgroundTask::FindUsages => self.find_usages_stop.as_ref().map(|_| None)?,
                    BackgroundTask::TodoScan => self.todo.scanning().then_some(None)?,
                    BackgroundTask::ConsoleStatement => {
                        self.console_cancellable().then_some(None)?
                    }
                    BackgroundTask::AssistantAnswer => self
                        .ai_chat
                        .as_ref()
                        .is_some_and(|v| v.read(cx).busy())
                        .then_some(None)?,
                };
                Some((task, task.label(detail.as_deref())))
            })
            .collect()
    }

    /// Tells `task` to stop, the way its own Cancel or Stop does; false when it does not run.
    pub fn stop_background_task(&mut self, task: BackgroundTask, cx: &mut Context<Self>) -> bool {
        if !self.background_tasks(cx).iter().any(|(t, _)| *t == task) {
            return false;
        }
        match task {
            // The popup's Cancel: the clone ends and removes what it wrote (`SPEC-0094` v5).
            BackgroundTask::CloneRepository => {
                if let Some((cancel, _)) = &self.clone_job {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            BackgroundTask::RemoteOpen => self.remote_cancel_open(cx),
            BackgroundTask::FindUsages => self.stop_find_usages(),
            BackgroundTask::TodoScan => self.todo.stop(),
            // The console says itself how the request went.
            BackgroundTask::ConsoleStatement => {
                self.cancel_console(cx);
                cx.notify();
                return true;
            }
            BackgroundTask::AssistantAnswer => {
                if let Some(view) = self.ai_chat.clone() {
                    view.update(cx, |v, cx| v.stop(cx));
                }
            }
        }
        self.status = Some(task.stopped());
        cx.notify();
        true
    }

    /// `StopBackgroundProcesses`: the running background tasks that can be stopped in a list;
    /// Enter on a row stops that task. Without one the status bar says so.
    pub(super) fn stop_background_processes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tasks = self.background_tasks(cx);
        if tasks.is_empty() {
            self.status = Some(keymap_actions::NO_BACKGROUND_TASKS.into());
            cx.notify();
            return;
        }
        let items = tasks.iter().map(|(_, label)| label.clone()).collect();
        self.choose_from(
            keymap_actions::STOP_BACKGROUND_TITLE.into(),
            items,
            0,
            window,
            cx,
            move |this, i, _, cx| {
                if let Some((task, _)) = tasks.get(i) {
                    // A task that ended while the list was open is gone: nothing to stop.
                    if !this.stop_background_task(*task, cx) {
                        this.status = Some(keymap_actions::NO_BACKGROUND_TASKS.into());
                        cx.notify();
                    }
                }
            },
        );
    }

    /// Makes `task` count as running with a stop flag of its own, as its start does, and gives
    /// the flag: for checks of the list without a network, a host or a language server. `None`
    /// for the two tasks whose state is not a flag (the console statement, the assistant).
    #[doc(hidden)]
    pub fn hold_background_task(
        &mut self,
        task: BackgroundTask,
        cx: &mut Context<Self>,
    ) -> Option<Arc<AtomicBool>> {
        let flag = Arc::new(AtomicBool::new(false));
        match task {
            BackgroundTask::CloneRepository => {
                self.clone_job = Some((flag.clone(), "Receiving objects: 12 %".into()))
            }
            BackgroundTask::RemoteOpen => self.remote_opening = Some(flag.clone()),
            BackgroundTask::FindUsages => self.find_usages_stop = Some(flag.clone()),
            BackgroundTask::TodoScan => self.todo.hold(flag.clone()),
            BackgroundTask::ConsoleStatement | BackgroundTask::AssistantAnswer => return None,
        }
        cx.notify();
        Some(flag)
    }

    /// `ExpressionTypeInfo`: asks the language server for the hover at the caret (at the start
    /// of a selection) and shows the type it names below the caret. False without an editor
    /// of a language IDECraft has a server for.
    pub(super) fn show_type_info(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        if idecraft_languages::LanguageId::detect(&path).is_none() {
            return false;
        }
        let (text, at, revision) = {
            let e = editor.read(cx);
            let at = e.selection_range().map_or(e.caret(), |r| r.start);
            (e.state.buffer.text(), at, e.revision())
        };
        let Ok(position) = editor.read(cx).state.buffer.offset_to_position(at) else {
            return false;
        };
        let (line, column) = (position.line, position.column);
        let root = self.root.clone();
        let remote = self.remote_ask(&path);
        let asked = path.clone();
        cx.spawn_in(window, async move |this, cx| {
            let from = asked.clone();
            let found = cx
                .background_spawn(async move {
                    // A remote document's hover comes from the host (SPEC-0318).
                    if let Some(remote) = remote {
                        return remote.hover(line, column);
                    }
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    rt.block_on(idecraft_languages::insight::hover_in(
                        &root,
                        &from,
                        Some(&text),
                        line as u64,
                        column as u64,
                        std::time::Duration::from_secs(30),
                    ))
                    .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update_in(cx, |this, _window, cx| {
                // Another file is active or the text changed: the answer is stale.
                let same = this.active_path().as_ref() == Some(&asked)
                    && this
                        .active_editor()
                        .is_some_and(|e| e.read(cx).revision() == revision);
                if !same {
                    return;
                }
                match found {
                    Ok(hover) => {
                        this.show_type_info_of(hover.as_ref().map(|h| h.text.as_str()), cx);
                    }
                    Err(e) => {
                        this.status = Some(format!("Type info failed: {e}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        true
    }

    /// Shows the type the hover text `hover` names for the expression at the caret of the
    /// active editor, as the answer of the language server does; without a type the status bar
    /// says so. Whether a type was shown. Also for checks without a language server.
    pub fn show_type_info_of(&mut self, hover: Option<&str>, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.active_editor() else {
            return false;
        };
        let name = {
            let e = editor.read(cx);
            let chars = |range: std::ops::Range<usize>| -> String {
                range.filter_map(|i| e.state.buffer.char_at(i)).collect()
            };
            match e.selection_range() {
                // A selection is named by its text when it is one word.
                Some(range) => Some(chars(range)).filter(|t| {
                    !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || c == '_')
                }),
                // The word the caret stands in, or directly behind.
                None => {
                    let at = e.caret();
                    let word = |offset: usize| {
                        crate::doc_model::word_at(|i| e.state.buffer.char_at(i), offset)
                    };
                    word(at)
                        .or_else(|| at.checked_sub(1).and_then(word))
                        .map(chars)
                }
            }
        };
        let (Some(path), offset) = (self.active_path(), editor.read(cx).caret()) else {
            return false;
        };
        match hover.and_then(|h| crate::doc_model::type_info(h, name.as_deref())) {
            // Below the caret, in the popup of the documentation and closed like it.
            Some(shown) => {
                self.doc = Some(super::DocPopup {
                    path,
                    offset,
                    blocks: vec![crate::doc_model::DocBlock::Code(shown)],
                    problem: None,
                    pointer: None,
                });
                cx.notify();
                true
            }
            None => {
                self.status = Some(crate::doc_model::NO_TYPE_INFO.into());
                cx.notify();
                false
            }
        }
    }
}

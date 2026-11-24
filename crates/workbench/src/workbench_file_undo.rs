//! Undo and redo of the project tree's file operations in the window (`SPEC-0275` v16 behaviours
//! 38–43, v22 behaviours 38 and 49, `SPEC-0057` v8 behaviour 29): the question before a step is taken, what the window
//! follows afterwards (tabs, tree, status), the Edit menu's titles, and the MCP tool
//! `file_operation_undo`. The history itself and its checks are `ferris_idea_core::file_undo`.

use std::path::Path;

use ferris_idea_core::file_undo::{StepInfo, UndoError};
use gpui::{Context, Window};

use super::{DrawnFor, WorkbenchView};
use crate::menu_bar::UndoTitles;

impl WorkbenchView {
    /// `$Undo` (or `$Redo`) while the Project tree has the focus: asks about the next step of the
    /// file operations; without one the status bar says so. The editor's text is never touched
    /// (`SPEC-0057` v8 behaviour 29).
    pub(super) fn tree_undo_action(&mut self, redo: bool, cx: &mut Context<Self>) {
        // A refactoring that moved files (Move, F6) after the newest file operation is taken back
        // as a whole, as ⌘Z in one of its editors does (`SPEC-0193` v7 behaviour 12).
        let tree_steps = self.file_undo.steps().0.len();
        let tree_redo = self.file_undo.next_redo().is_some();
        if let Some(i) = self.undo_groups.for_tree(redo, tree_steps, tree_redo) {
            // A step that would be refused asks nothing (`SPEC-0275` v22 behaviour 38).
            let refused = self
                .undo_groups
                .get(i)
                .and_then(|g| self.group_occupied(g, redo));
            if let Some(reason) = refused {
                self.status = Some(reason);
                cx.notify();
                return;
            }
            // Asked first, as the tree's own file operations are (`SPEC-0193` v8 behaviour 12,
            // `SPEC-0275` v22 behaviour 49; the reference asks, OBS-0088 U1).
            let action = if redo { "Redo" } else { "Undo" };
            let name = self
                .undo_groups
                .get(i)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            self.drawn_question = Some((
                crate::dialogs::DrawnQuestion {
                    message: format!("{action} {name}?"),
                    detail: "The files of this refactoring change back and moved files move back."
                        .into(),
                    buttons: vec![action.into(), "Cancel".into()],
                    default: 0,
                    cancel: Some(1),
                },
                DrawnFor::GroupUndo(i, redo),
            ));
            cx.notify();
            return;
        }
        let step = if redo {
            self.file_undo.next_redo()
        } else {
            self.file_undo.next_undo()
        };
        let Some(step) = step else {
            self.status = Some(nothing(redo).to_owned());
            cx.notify();
            return;
        };
        // A step that would be refused is not offered: the reason at once, no question
        // (`SPEC-0275` v22 behaviour 38).
        if let Err(reason) = self.file_undo_refusal(redo, cx) {
            self.status = Some(reason);
            cx.notify();
            return;
        }
        let (message, detail) = step.question(redo);
        let action = if redo { "Redo" } else { "Undo" };
        self.drawn_question = Some((
            crate::dialogs::DrawnQuestion {
                message,
                detail,
                buttons: vec![action.into(), "Cancel".into()],
                default: 0,
                cancel: Some(1),
            },
            DrawnFor::FileUndo(redo),
        ));
        cx.notify();
    }

    /// The answer to the question of [`Self::tree_undo_action`]: the first button carries the
    /// step out; either way the tree has the focus again.
    /// Why a group's move cannot go back (or on): its place is taken (`SPEC-0193` v8).
    pub(super) fn group_occupied(
        &self,
        group: &ferris_idea_languages::undo_group::Group,
        redo: bool,
    ) -> Option<String> {
        let at = group.occupied(redo)?;
        let shown = at.strip_prefix(&self.root).unwrap_or(at);
        Some(format!(
            "{} refused: {} already exists; nothing was touched",
            if redo { "Redo" } else { "Undo" },
            shown.display()
        ))
    }

    pub(super) fn answer_group_undo(
        &mut self,
        i: usize,
        redo: bool,
        yes: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if yes {
            self.undo_group_at(i, redo, cx);
        }
        window.focus(&self.tree_focus);
        cx.notify();
    }

    /// The answer to the question of [`Self::tree_undo_action`]: the first button carries the
    /// step out; either way the tree has the focus again.
    pub(super) fn answer_file_undo(
        &mut self,
        redo: bool,
        yes: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if yes {
            self.status = Some(match self.apply_file_undo(redo, window, cx) {
                Ok(done) | Err(done) => done,
            });
        }
        window.focus(&self.tree_focus);
        cx.notify();
    }

    /// Takes the next step of the file operations back (or makes it again) and lets the window
    /// follow: tabs of items that went close, tabs of moved items follow, the tree shows what is
    /// back. `Ok` is the status line, `Err` the reason nothing was done (behaviours 39 and 40).
    /// Why the next undo (or redo) step would be refused, checked without touching anything:
    /// an editor of one of its items with unsaved changes, then the checks of behaviour 40.
    fn file_undo_refusal(&self, redo: bool, cx: &Context<Self>) -> Result<(), String> {
        let verb = if redo { "Redo" } else { "Undo" };
        // An editor with unsaved changes of an item of the step refuses it, as it refuses Rename.
        if let Some(unsaved) = self
            .file_undo
            .touched(redo)
            .iter()
            .find(|p| self.open_and_modified(p, cx))
        {
            return Err(format!(
                "{verb} refused: save {} first; it has unsaved changes",
                name_of(unsaved)
            ));
        }
        self.file_undo.check_next(redo).map_err(|e| match e {
            UndoError::Nothing => nothing(redo).to_owned(),
            e => format!("{verb} refused: {e}"),
        })
    }

    pub(super) fn apply_file_undo(
        &mut self,
        redo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let verb = if redo { "Redo" } else { "Undo" };
        // Checked again: the disk may have changed while the question was open.
        self.file_undo_refusal(redo, cx)?;
        let turned = if redo {
            self.file_undo.redo()
        } else {
            self.file_undo.undo()
        };
        let outcome = turned.map_err(|e| match e {
            UndoError::Nothing => nothing(redo).to_owned(),
            e => format!("{verb} refused: {e}"),
        })?;
        for gone in &outcome.gone {
            self.close_tabs_under(gone, window, cx);
        }
        for (from, to) in &outcome.moved {
            if to.is_file() {
                self.tabs_follow(from, to, window, cx);
            }
            self.close_tabs_under(from, window, cx);
        }
        self.show_pasted(&outcome.back, window, cx);
        Ok(format!(
            "{}: {}",
            if redo { "Redone" } else { "Undone" },
            outcome.info.label
        ))
    }

    /// What Edit › Undo and Edit › Redo are called now: the next step of the file operations
    /// while the Project tree has the focus, else plain (behaviour 42).
    pub fn file_undo_titles(&self, window: &Window) -> UndoTitles {
        if !self.tree_focus.is_focused(window) {
            return UndoTitles::default();
        }
        UndoTitles {
            undo: self.file_undo.next_undo().map(|s| s.label),
            redo: self.file_undo.next_redo().map(|s| s.label),
        }
    }

    /// Shows [`Self::file_undo_titles`] in the menu bar when they changed; called while the
    /// window is drawn, the menu bar is set afterwards.
    pub(super) fn sync_undo_titles(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            return;
        }
        let titles = self.file_undo_titles(window);
        if self.undo_titles_shown.as_ref() == Some(&titles) {
            return;
        }
        self.undo_titles_shown = Some(titles.clone());
        cx.defer(move |cx| crate::menu_bar::set_undo_titles(titles, cx));
    }

    /// The history of file operations as the MCP tool `file_operation_undo` reports it
    /// (behaviour 43).
    pub fn file_undo_state(&self) -> serde_json::Value {
        let list = |steps: Vec<StepInfo>| -> Vec<serde_json::Value> {
            steps
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "label": s.label, "kind": s.kind.id(), "paths": s.paths,
                    })
                })
                .collect()
        };
        let (undo, redo) = self.file_undo.steps();
        serde_json::json!({ "undo": list(undo), "redo": list(redo) })
    }

    /// One call of the MCP tool `file_operation_undo`: `request` holds `action` (`list`, `undo`,
    /// `redo`). `undo` and `redo` take the next step without the question, under every check;
    /// the reply is the history afterwards with `done`, the status line.
    pub fn file_undo_call(
        &mut self,
        request: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        let action = request["action"].as_str().unwrap_or("list");
        let done = match action {
            "list" => None,
            "undo" | "redo" => {
                if self.remote_project.is_some() {
                    return Err(
                        "a remote project keeps no history of file operations (SPEC-0275 v16)"
                            .into(),
                    );
                }
                let done = self.apply_file_undo(action == "redo", window, cx)?;
                self.status = Some(done.clone());
                cx.notify();
                Some(done)
            }
            other => return Err(format!("unknown action `{other}`; one of list, undo, redo")),
        };
        let mut state = self.file_undo_state();
        if let Some(done) = done {
            state["done"] = done.into();
        }
        Ok(state)
    }

    /// `file_operation_undo` for the project window of `workspace` (MCP over HTTP).
    pub fn file_undo_in(
        workspace: &Path,
        request: &serde_json::Value,
        cx: &mut gpui::App,
    ) -> Result<serde_json::Value, String> {
        let wanted = workspace.canonicalize().unwrap_or(workspace.to_path_buf());
        let bench = crate::project_windows::workbenches(cx)
            .into_iter()
            .find(|b| {
                let root = b.read(cx).root.clone();
                root.canonicalize().unwrap_or(root) == wanted
            })
            .ok_or("no project window is open for this workspace")?;
        let window = crate::project_windows::project_window(&bench.read(cx).root, cx)
            .ok_or("no project window is open for this workspace")?;
        window
            .update(cx, |_, window, cx| {
                bench.update(cx, |b, cx| b.file_undo_call(request, window, cx))
            })
            .map_err(|e| e.to_string())?
    }
}

fn nothing(redo: bool) -> &'static str {
    if redo {
        "Nothing to redo in the project tree"
    } else {
        "Nothing to undo in the project tree"
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

//! The usages of the symbol at the caret in the workbench (`SPEC-0295`): the check of the caret's
//! rest, the question to the file's language server and its answer, and `HighlightUsagesInFile`
//! with its status line, and the step to the next and previous usage (v2 behaviour 15).
//!
//! When a question is due, which answer is still wanted and what is marked is decided in
//! [`crate::occurrence_model`]; the request itself is
//! [`ferris_idea_languages::occurrences::document_highlights_in`].

use std::path::PathBuf;
use std::time::Instant;

use ferris_idea_languages::lsp::LspError;
use ferris_idea_languages::occurrences::{Highlights, MAX_TEXT_BYTES, document_highlights_in};
use gpui::{AppContext, Context, Entity};

use super::WorkbenchView;
use crate::EditorView;
use crate::occurrence_model::{End, Request, StepNow, StepTo, Taken, WAIT, no_other, status};

impl WorkbenchView {
    /// Every editor view: the main group's and the other groups'.
    fn every_editor(&self) -> Vec<Entity<EditorView>> {
        self.editors
            .values()
            .chain(self.groups.iter().flat_map(|g| g.editors.values()))
            .cloned()
            .collect()
    }

    /// The check every [`crate::occurrence_model::TICK`]: the automatic highlight follows its
    /// setting in every editor, the status line of `HighlightUsagesInFile` goes with its marks,
    /// and the active editor's question is sent when it is due (behaviours 1, 9 and 12).
    pub(super) fn occurrence_tick(&mut self, cx: &mut Context<Self>) {
        let automatic = crate::appearance::ide_settings(cx)
            .editor
            .highlight_usages_at_caret;
        if automatic != self.occurrences_on {
            self.occurrences_on = automatic;
            if !automatic {
                for editor in self.every_editor() {
                    editor.update(cx, |e, cx| e.occurrences_off(cx));
                }
            }
        }
        let Some(editor) = self.active_editor() else {
            return;
        };
        // "N usages highlighted" is true only while the marks are there.
        if let Some(said) = &self.occurrence_status
            && !editor.read(cx).occurrences_kept()
        {
            if self.status.as_ref() == Some(said) {
                self.status = None;
                cx.notify();
            }
            self.occurrence_status = None;
        }
        self.drive_occurrences(&editor, cx);
    }

    /// Sends the question of `editor` that is due, if any.
    fn drive_occurrences(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        // A remote document's language services are the host's (SPEC-0318); not covered yet.
        if self.remote_project.is_some() {
            return;
        }
        let Some(path) = editor.read(cx).file_path().map(PathBuf::from) else {
            return;
        };
        if ferris_idea_languages::LanguageId::detect(&path).is_none() {
            return;
        }
        let automatic = self.occurrences_on;
        let request = {
            let root = &self.root;
            // A server that is starting or loading is not asked (behaviour 3).
            let ready = || ferris_idea_languages::pool::loading(root, &path).is_none();
            editor.update(cx, |e, cx| {
                e.due_occurrences(Instant::now(), automatic, ready, cx)
            })
        };
        if let Some(request) = request {
            self.ask_occurrences(editor, path, request, cx);
        }
    }

    /// Asks the server in the background and hands the answer to the editor; a question that
    /// waited for this one to end follows at once (behaviour 10).
    fn ask_occurrences(
        &mut self,
        editor: &Entity<EditorView>,
        path: PathBuf,
        request: Request,
        cx: &mut Context<Self>,
    ) {
        let (text, at) = {
            let e = editor.read(cx);
            let buffer = &e.state.buffer;
            (buffer.text(), buffer.offset_to_position(request.caret).ok())
        };
        let Some(at) = at else {
            editor.update(cx, |e, cx| e.take_occurrences(&request, None, cx));
            return;
        };
        // The name `HighlightUsagesInFile` was used on, for its status line.
        let symbol = request
            .kept
            .then(|| self.occurrence_symbol.take())
            .flatten()
            .unwrap_or_default();
        // The name a step was asked for, for the status line when it leads nowhere.
        let step_symbol = request
            .step
            .and_then(|_| self.step_symbol.take())
            .unwrap_or_default();
        let root = self.root.clone();
        let weak = editor.downgrade();
        cx.spawn(async move |this, cx| {
            let stop = std::sync::Arc::clone(&request.stop);
            let result = cx
                .background_spawn(async move {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| LspError::Io(e.to_string()))?;
                    rt.block_on(document_highlights_in(
                        &root,
                        &path,
                        Some(&text),
                        at.line as u64,
                        at.column as u64,
                        WAIT,
                        Some(stop),
                        true,
                    ))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(editor) = weak.upgrade() else {
                    return;
                };
                let found = match &result {
                    Ok(Highlights::Found(found)) => Some(found.as_slice()),
                    _ => None,
                };
                let (taken, stepped) =
                    editor.update(cx, |e, cx| e.take_occurrences(&request, found, cx));
                if request.step.is_some() {
                    let said = match (stepped, taken) {
                        (Some(StepTo::Usage(_)), _) => None,
                        (Some(StepTo::Alone), _) => Some(no_other(&step_symbol)),
                        // The caret moved on or the text changed: the step is given up silently.
                        (None, Taken::Stale) if result.is_ok() => None,
                        (None, _) => Some(status(&step_symbol, &End::of(&result, taken))),
                    };
                    if let Some(said) = said {
                        this.status = Some(said);
                        cx.notify();
                    }
                }
                if request.kept {
                    let said = status(&symbol, &End::of(&result, taken));
                    this.occurrence_status =
                        editor.read(cx).occurrences_kept().then(|| said.clone());
                    this.status = Some(said);
                    cx.notify();
                }
                this.drive_occurrences(&editor, cx);
            });
        })
        .detach();
    }

    /// `HighlightUsagesInFile` (⇧⌘F7): marks the usages of the symbol at the caret in the file
    /// and keeps them until Escape; the status bar says how it ended (behaviour 8).
    pub(super) fn highlight_usages_in_file(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        if self.remote_project.is_some() {
            self.status =
                Some("Highlight Usages in File is not available in a remote project yet".into());
            cx.notify();
            return;
        }
        let (symbol, path, large) = {
            let e = editor.read(cx);
            (
                e.name_at_caret(),
                e.file_path().map(PathBuf::from),
                e.state.buffer.len_chars() > MAX_TEXT_BYTES,
            )
        };
        let end = match (&symbol, &path) {
            (None, _) => Some(End::NoName),
            (_, None) => Some(End::Failed("the editor has no file".into())),
            (Some(_), Some(_)) if large => Some(End::TooLarge),
            (Some(_), Some(path)) => match ferris_idea_languages::LanguageId::detect(path) {
                None => Some(End::Failed(
                    LspError::NoServer(path.display().to_string()).to_string(),
                )),
                // Not asked and not queued while the server is loading (behaviour 3).
                Some(_) => {
                    ferris_idea_languages::pool::loading(&self.root, path).map(End::NotReady)
                }
            },
        };
        let symbol = symbol.unwrap_or_default();
        if let Some(end) = end {
            self.status = Some(status(&symbol, &end));
            cx.notify();
            return;
        }
        editor.update(cx, |e, _| e.keep_occurrences(Instant::now()));
        self.occurrence_symbol = Some(symbol);
        // Asked at once, or when the question in flight has ended.
        self.drive_occurrences(&editor, cx);
    }

    /// `GotoNextElementUnderCaretUsage` (⌃⌥↓) and `GotoPrevElementUnderCaretUsage` (⌃⌥↑): the
    /// caret goes to the next or previous usage of the symbol it is on, around the ends; among
    /// the marks at once, else after asking the server (`SPEC-0295` v2 behaviour 15).
    pub(super) fn step_usage(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        match editor.update(cx, |e, cx| e.step_occurrence(forward, cx)) {
            StepNow::Nothing | StepNow::Done(StepTo::Usage(_)) => return,
            StepNow::Done(StepTo::Alone) => {
                let symbol = editor.read(cx).name_at_caret().unwrap_or_default();
                self.status = Some(no_other(&symbol));
                cx.notify();
                return;
            }
            StepNow::Ask => {}
        }
        if self.remote_project.is_some() {
            self.status =
                Some("The usages in the file are not available in a remote project yet".into());
            cx.notify();
            return;
        }
        let (symbol, path, large) = {
            let e = editor.read(cx);
            (
                e.name_at_caret(),
                e.file_path().map(PathBuf::from),
                e.state.buffer.len_chars() > MAX_TEXT_BYTES,
            )
        };
        let end = match (&symbol, &path) {
            (None, _) => Some(End::NoName),
            (_, None) => Some(End::Failed("the editor has no file".into())),
            (Some(_), Some(_)) if large => Some(End::TooLarge),
            (Some(_), Some(path)) => match ferris_idea_languages::LanguageId::detect(path) {
                None => Some(End::Failed(
                    LspError::NoServer(path.display().to_string()).to_string(),
                )),
                // Not asked and not queued while the server is loading (behaviour 3).
                Some(_) => {
                    ferris_idea_languages::pool::loading(&self.root, path).map(End::NotReady)
                }
            },
        };
        let symbol = symbol.unwrap_or_default();
        if let Some(end) = end {
            self.status = Some(status(&symbol, &end));
            cx.notify();
            return;
        }
        let mark = self.occurrences_on;
        editor.update(cx, |e, _| {
            e.want_occurrence_step(Instant::now(), forward, mark)
        });
        self.step_symbol = Some(symbol);
        // Asked at once, or when the question in flight has ended.
        self.drive_occurrences(&editor, cx);
    }
}

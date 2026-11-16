//! Reformat Code and Optimize Imports of a Python file in the window (`SPEC-0329` behaviour 7):
//! the project's `ruff` or `black` on the editor's text, in the background, as one undo step that
//! keeps carets and selections on their text.

use gpui::Context;
use idecraft_languages::insight::Range;
use idecraft_languages::python_tools::{
    self, ImportCleanup, ToolKind, ToolSearch, find_formatter, find_ruff,
};

use super::WorkbenchView;

impl WorkbenchView {
    /// Formats the active Python file, or its selection, with the project's formatter
    /// (behaviour 4).
    pub(super) fn python_reformat(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return;
        };
        let (text, range) = {
            let e = editor.read(cx);
            let buffer = &e.state.buffer;
            let selection = e.state.carets.primary().range();
            let range = (!selection.is_empty()).then(|| {
                let a = buffer
                    .offset_to_position(selection.start)
                    .unwrap_or_default();
                let b = buffer.offset_to_position(selection.end).unwrap_or_default();
                Range {
                    start_line: a.line as u64,
                    start_character: a.column as u64,
                    end_line: b.line as u64,
                    end_character: b.column as u64,
                }
            });
            (buffer.text(), range)
        };
        let root = self.root.clone();
        let (file, before) = (path.clone(), text.clone());
        let task = cx.background_executor().spawn(async move {
            let search = ToolSearch::for_workspace(&root);
            let Some(tool) = find_formatter(&search) else {
                return Err(python_tools::missing(
                    &search,
                    &[ToolKind::Ruff, ToolKind::Black],
                ));
            };
            python_tools::format(&tool, &file, &before, range.as_ref()).map(|new| (tool.kind, new))
        });
        self.status = Some("Reformatting…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let now = editor.read(cx).state.buffer.text();
                this.status = Some(match result {
                    Err(e) => format!("Reformat failed: {e}"),
                    Ok(_) if now != text => {
                        "The file changed meanwhile; nothing was applied".into()
                    }
                    Ok((kind, new)) => {
                        // One undo step; carets stay on their text (behaviour 7, v3).
                        if editor.update(cx, |e, cx| e.apply_reformat(&new, cx)) {
                            format!("Reformatted with {}.", kind.program())
                        } else {
                            format!("Already formatted ({}).", kind.program())
                        }
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Removes the unused imports of the active Python file and, with `ruff`, sorts its imports
    /// (behaviours 5 and 6).
    pub(super) fn python_optimize_imports(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let root = self.root.clone();
        let (file, before) = (path.clone(), text.clone());
        let task = cx.background_executor().spawn(async move {
            let search = ToolSearch::for_workspace(&root);
            // Behaviour 6 (v2): a ruff that only waits for trust is named as such.
            let ruff_waits = python_tools::ruff_waits_for_trust(&search);
            let done = match find_ruff(&search) {
                Some(ruff) => python_tools::ruff_imports(&ruff, &file, &before),
                None => super::java_actions::blocking(async move {
                    python_tools::language_server_imports_in(
                        &root,
                        &file,
                        &before,
                        super::java_actions::WAIT,
                    )
                    .await
                })(),
            };
            done.map(|done| (done, ruff_waits))
        });
        self.status = Some("Optimizing imports…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result: Result<(ImportCleanup, bool), String> = task.await;
            let _ = this.update(cx, |this, cx| {
                let now = editor.read(cx).state.buffer.text();
                this.status = Some(match result {
                    Err(e) => e,
                    Ok(_) if now != text => {
                        "The file changed meanwhile; nothing was applied".into()
                    }
                    Ok((done, ruff_waits)) => {
                        let changed = editor.update(cx, |e, cx| e.apply_reformat(&done.text, cx));
                        python_tools::summary(&done, changed, ruff_waits)
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }
}

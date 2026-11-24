//! The costs of the profile shown next to the lines of the editors (`SPEC-0120` v5 behaviour 10):
//! computed by file in the background whenever the Profiler window shows another profile, a
//! failed run or none; every editor of such a file gets its costs, and a click on one shows its
//! function in the Profiler window.

use std::path::Path;

use gpui::{Context, Entity, Window};

use super::WorkbenchView;
use crate::EditorView;

impl WorkbenchView {
    /// The profile shown changed: its costs by file replace the old ones in every editor.
    pub(super) fn refresh_profile_lines(&mut self, cx: &mut Context<Self>) {
        self.profile_lines_refresh += 1;
        let refresh = self.profile_lines_refresh;
        let run = self
            .profiler
            .as_ref()
            .and_then(|v| v.read(cx).shown_run().cloned());
        let Some(run) = run else {
            self.profile_lines.clear();
            self.push_profile_lines(cx);
            return;
        };
        let root = self.root.clone();
        let task = cx
            .background_executor()
            .spawn(async move { crate::profile_lines_model::file_costs(&run, &root) });
        cx.spawn(async move |this, cx| {
            let costs = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.profile_lines_refresh == refresh {
                    this.profile_lines = costs;
                    this.push_profile_lines(cx);
                }
            });
        })
        .detach();
    }

    /// Gives every editor its file's costs, or none.
    fn push_profile_lines(&self, cx: &mut Context<Self>) {
        let editors: Vec<Entity<EditorView>> = self
            .editors
            .values()
            .chain(self.groups.iter().flat_map(|g| g.editors.values()))
            .cloned()
            .collect();
        for editor in editors {
            self.show_profile_lines(&editor, cx);
        }
    }

    /// The costs of `path`, found by its path as opened or as it really is.
    fn profile_lines_of(
        &self,
        path: &Path,
    ) -> Option<std::sync::Arc<crate::profile_lines_model::FileCosts>> {
        if self.profile_lines.is_empty() {
            return None;
        }
        self.profile_lines.get(path).cloned().or_else(|| {
            let real = path.canonicalize().ok()?;
            self.profile_lines
                .iter()
                .find(|(p, _)| p.canonicalize().ok().as_ref() == Some(&real))
                .map(|(_, c)| c.clone())
        })
    }

    /// Shows the costs of `editor`'s file in its gutter, or none.
    pub(super) fn show_profile_lines(&self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let costs = editor
            .read(cx)
            .file_path()
            .and_then(|p| self.profile_lines_of(p));
        editor.update(cx, |e, cx| e.set_profile_costs(costs, cx));
    }

    /// A click on the cost of 0-based `line` of `path`: the Profiler window shows the costliest
    /// call of the line's function in its Call Tree and takes the focus.
    pub(super) fn reveal_profile_line(
        &mut self,
        path: &Path,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mark) = self
            .profile_lines_of(path)
            .and_then(|c| c.lines.iter().find(|l| l.line == line).cloned())
        else {
            return;
        };
        let view = self.ensure_profiler(window, cx);
        self.show_profiler = true;
        let found = view.update(cx, |v, cx| v.reveal(&mark.reported, &mark.function, cx));
        if !found {
            self.status = Some(format!("{} is not in the profile shown", mark.function));
        }
        window.focus(&view.read(cx).focus());
        cx.notify();
    }

    /// The costs per line of the profile shown, by file (checks).
    pub fn profile_lines(
        &self,
    ) -> &std::collections::HashMap<
        std::path::PathBuf,
        std::sync::Arc<crate::profile_lines_model::FileCosts>,
    > {
        &self.profile_lines
    }
}

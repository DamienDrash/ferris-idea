//! Run, debug and window actions that exist under their keymap id because the bundled keymaps
//! bind them (`docs/research/KEYMAP_COVERAGE.md`): Toggle Temporary Line Breakpoint (`SPEC-0127`
//! v8), Toggle Breakpoint Enabled and Remove All Breakpoints (`SPEC-0127` v15), the chooser opened for debugging (`SPEC-0087` v8), Rerun Tests (`SPEC-0088` v4) and the
//! next and previous project window (`SPEC-0094` v2). The decisions without a window are in
//! [`crate::run_debug_actions`], `ferris_idea_debugger::saved` and [`crate::run_list_model`].

use gpui::{Context, Window};

use super::{RunStart, WorkbenchView};
use crate::run_debug_actions::{step_window, test_reruns};

impl WorkbenchView {
    /// `ToggleTemporaryLineBreakpoint`: on the caret line of the active editor a breakpoint that
    /// is removed once it is hit, or no breakpoint where the line had one (`SPEC-0127` v8
    /// behaviour 18). False without an editor.
    pub(super) fn toggle_temporary_breakpoint(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return false;
        };
        let line = editor.read(cx).status().line - 1;
        self.toggle_temporary_at(&path, &editor, line, cx);
        true
    }

    /// Toggle Temporary Line Breakpoint on `line` (0-based) of the editor of `path`.
    fn toggle_temporary_at(
        &mut self,
        path: &std::path::Path,
        editor: &gpui::Entity<crate::EditorView>,
        line: usize,
        cx: &mut Context<Self>,
    ) {
        let path = path.to_path_buf();
        let props = crate::debug_model::toggle_temporary(editor.read(cx).breakpoints(), line);
        editor.update(cx, |e, cx| e.toggle_breakpoint(line, cx));
        // The editor's own notice arrives after this action. The lines are taken over now, so
        // that the property finds its breakpoint; the notice then changes nothing.
        self.breakpoints_changed(&path, cx);
        if let Some(props) = props {
            self.set_breakpoint_props(&path, line, props, cx);
        }
    }

    /// `ToggleBreakpointEnabled`: switches the breakpoint on the caret line of the active editor
    /// off or on, keeping its other properties (`SPEC-0127` v15 behaviour 31). False without an
    /// editor or without a breakpoint on that line.
    pub(super) fn toggle_breakpoint_enabled(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return false;
        };
        let line = editor.read(cx).status().line - 1;
        self.toggle_enabled_at(&path, line, cx)
    }

    /// Switches the breakpoint on `line` (0-based) of `path` off or on; false without one.
    fn toggle_enabled_at(
        &mut self,
        path: &std::path::Path,
        line: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let lines = self.breakpoints.get(path).cloned().unwrap_or_default();
        let Some(props) =
            crate::debug_model::toggle_enabled(&lines, self.breakpoint_props.get(path), line)
        else {
            return false;
        };
        self.set_breakpoint_props(path, line, props, cx);
        true
    }

    /// ⌥-click (Alt-click) on a line number (`SPEC-0127` v15 behaviour 31): a breakpoint there is
    /// switched off or on, a line without one gets a temporary breakpoint.
    pub(super) fn alt_click_breakpoint(
        &mut self,
        path: &std::path::Path,
        line: usize,
        cx: &mut Context<Self>,
    ) {
        if self.toggle_enabled_at(path, line, cx) {
            return;
        }
        if let Some(editor) = self.editors.get(path).cloned() {
            self.toggle_temporary_at(path, &editor, line, cx);
        }
    }

    /// `Debugger.RemoveAllBreakpoints` (every file of the workspace) and
    /// `Debugger.RemoveAllBreakpointsInFile` (the active editor's file): removes the line
    /// breakpoints with their properties, saves, hands the change to a running session and says
    /// in the status bar how many were removed (`SPEC-0127` v15 behaviour 32). False when the
    /// file action has no editor.
    pub(super) fn remove_all_breakpoints(&mut self, in_file: bool, cx: &mut Context<Self>) -> bool {
        let file = if in_file {
            match self.active_path() {
                Some(p) => Some(p),
                None => return false,
            }
        } else {
            None
        };
        let touched: Vec<std::path::PathBuf> = match &file {
            Some(f) => vec![f.clone()],
            None => self.breakpoints.keys().cloned().collect(),
        };
        let removed = crate::debug_model::remove_all(
            &mut self.breakpoints,
            &mut self.breakpoint_props,
            file.as_deref(),
        );
        for path in &touched {
            if let Some(editor) = self.editors.get(path).cloned() {
                editor.update(cx, |e, cx| e.set_breakpoints(Vec::new(), cx));
            }
        }
        crate::debug_model::save_all(&self.root, &self.breakpoints, &self.breakpoint_props);
        for path in &touched {
            self.breakpoint_settings_changed(path, cx);
        }
        self.status = Some(crate::debug_model::removed_breakpoints_note(
            removed, in_file,
        ));
        cx.notify();
        true
    }

    /// `ChooseDebugConfiguration`: the configuration chooser with Enter debugging and ⇧ running
    /// (`SPEC-0087` v8 behaviour 16).
    pub fn open_debug_chooser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_run_chooser_with(false, true, window, cx);
    }

    /// `RerunTests`: every tab of a test configuration runs again in its tab, whichever tab is
    /// selected and wherever the focus is (`SPEC-0088` v4 behaviour 11). False without such a
    /// tab.
    pub(super) fn rerun_tests(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.run_view.clone() else {
            return false;
        };
        self.reload_run_configs();
        let (tabs, selected) = {
            let v = view.read(cx);
            (v.tabs(), v.selected_title())
        };
        let runs = test_reruns(&tabs, &self.run_configs.configs);
        if runs.is_empty() {
            return false;
        }
        for run in &runs {
            // The tab's own run is the one to stop: no question, no further instance, as its
            // Rerun (SPEC-0087 v6 behaviour 12).
            self.start_run_in(&run.config, RunStart::InTab(run.tab.clone()), window, cx);
        }
        if let Some(title) = selected {
            view.update(cx, |v, cx| v.select_title(&title, cx));
        }
        self.show_run = true;
        cx.notify();
        true
    }

    /// `NextWindow` and `PreviousWindow`: the window after or before this one among all windows
    /// of the application comes to the front (`SPEC-0094` v6 behaviour 17). False with a
    /// single window.
    pub(super) fn step_any_window(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let opened = crate::project_windows::opened_windows(cx);
        let all = crate::keymap_actions::window_cycle(&opened, &cx.windows());
        let Some(next) = step_window(&all, window.window_handle(), forward) else {
            return false;
        };
        cx.defer(move |cx| {
            let _ = next.update(cx, |_, window, _| window.activate_window());
        });
        true
    }

    /// `NextProjectWindow` and `PreviousProjectWindow`: the project window opened after or before
    /// this one comes to the front (`SPEC-0094` v2 behaviour 5). False with a single one.
    pub(super) fn step_project_window(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let opened = crate::project_windows::opened_windows(cx);
        let Some(next) = step_window(&opened, window.window_handle(), forward) else {
            return false;
        };
        // After this action: the other window is not reached from inside this one's update.
        cx.defer(move |cx| {
            let _ = next.update(cx, |_, window, _| window.activate_window());
        });
        true
    }
}

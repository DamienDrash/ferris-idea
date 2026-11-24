//! The context actions' own rows and options (`SPEC-0076` v11): Run, Debug and Profile at a run
//! marker (behaviour 14), the submenu of an intention with Disable and the intention settings
//! (behaviour 15). The list itself is `IntentionsView`; the language server's part is in
//! `WorkbenchView::show_intentions`.

use std::path::Path;

use ferris_idea_languages::actions::CodeAction;
use ferris_idea_languages::context_actions::{
    IntentionOption, RunEntry, RunMode, intention_options, is_intention, run_entries,
};
use gpui::{Context, Window};

use super::WorkbenchView;

impl WorkbenchView {
    /// The run rows of the ▶ marker on 0-based `line` of `path` (behaviour 14), kept for Enter
    /// and the preview. None in a remote project (its host decides what runs) and none where the
    /// gutter has no marker on the line.
    pub(super) fn context_run_rows(
        &mut self,
        path: &Path,
        text: &str,
        line: usize,
    ) -> Vec<CodeAction> {
        self.context_run.clear();
        let marked = self
            .run_marker_info
            .get(path)
            .is_some_and(|ms| ms.iter().any(|m| m.line == line));
        if self.remote_project.is_some() || !marked {
            return Vec::new();
        }
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return Vec::new();
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let model = self.project_model();
        self.context_run = run_entries(&model, &self.root, &rel, text, line, |c| {
            ferris_idea_profiler::can_profile(&c.command)
        });
        self.context_run.iter().map(RunEntry::as_action).collect()
    }

    /// The titles of the run rows offered last (tests).
    pub fn context_run_titles(&self) -> Vec<String> {
        self.context_run.iter().map(|e| e.title.clone()).collect()
    }

    /// The preview of the run row titled `title`: its command and folder.
    pub(super) fn context_run_preview(&self, title: &str) -> Option<Vec<String>> {
        self.context_run
            .iter()
            .find(|e| e.title == title)
            .map(RunEntry::preview)
    }

    /// Enter on the run row titled `title` does what the ▶ menu's entry does; whether it was one.
    pub(super) fn apply_context_run(
        &mut self,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(entry) = self.context_run.iter().find(|e| e.title == title).cloned() else {
            return false;
        };
        self.context_run.clear();
        self.intentions = None;
        self.focus_active(window, cx);
        let name = entry.config.name.clone();
        match entry.mode {
            RunMode::Run => self.run_temporary_config(entry.config, window, cx),
            RunMode::Debug | RunMode::Profile => {
                self.push_temporary(entry.config);
                self.run_selected = Some(name);
                self.run_current_file = false;
                if entry.mode == RunMode::Debug {
                    self.debug_selected(window, cx);
                } else {
                    self.profile_selected(window, cx);
                }
            }
        }
        cx.notify();
        true
    }

    /// The user's disabled intentions: the settings file's, so that a change through MCP counts
    /// at once, else the settings in effect.
    fn intention_settings(&self, cx: &gpui::App) -> ferris_idea_core::IntentionSettings {
        match crate::appearance::ide_settings_file(cx) {
            Some(file) if file.exists() => ferris_idea_core::load_ide_settings(&file).intentions,
            _ => crate::appearance::ide_settings(cx).intentions,
        }
    }

    /// The language server's `actions` without the intentions the user turned off, each other
    /// intention with its submenu (behaviour 15).
    pub(super) fn with_intention_options(
        &mut self,
        actions: Vec<CodeAction>,
        cx: &mut Context<Self>,
    ) -> Vec<CodeAction> {
        let settings = self.intention_settings(cx);
        let actions: Vec<CodeAction> = actions
            .into_iter()
            .filter(|a| !(is_intention(a) && settings.is_disabled(&a.title)))
            .collect();
        for a in &actions {
            let labels = intention_options(a);
            if !labels.is_empty() && !self.intention_options.iter().any(|o| o.title == a.title) {
                self.intention_options
                    .push(crate::intentions_view::RowOptions {
                        title: a.title.clone(),
                        labels,
                        only_options: false,
                    });
            }
        }
        actions
    }

    /// Opens the context actions at the caret of the active editor with `actions` as the language
    /// server's answer (tests and checks without a language server): disabled intentions are left
    /// out and the others get their submenu, as after a real answer.
    pub fn open_context_actions_with(
        &mut self,
        actions: Vec<CodeAction>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let Some(path) = self.active_path() else {
            return;
        };
        let (range, offset) = {
            let e = editor.read(cx);
            let head = e.state.carets.primary().head;
            let p = e.state.buffer.offset_to_position(head).unwrap_or_default();
            let at = p.line as u64;
            let ch = p.column as u64;
            (
                ferris_idea_languages::insight::Range {
                    start_line: at,
                    start_character: ch,
                    end_line: at,
                    end_character: ch,
                },
                head,
            )
        };
        self.intention_options.clear();
        let actions = self.with_intention_options(actions, cx);
        self.open_intentions(
            actions,
            super::IntentionsAt {
                path,
                range,
                offset,
            },
            window,
            cx,
        );
    }

    /// Runs an option of an intention's submenu; whether `label` was one (own findings have
    /// their own options, `SPEC-0125` v2 behaviour 8).
    pub(super) fn run_intention_option(
        &mut self,
        title: &str,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match IntentionOption::of_label(title, label) {
            Some(IntentionOption::Disable) => {
                self.focus_active(window, cx);
                self.set_intention_enabled(title, false, cx);
            }
            Some(IntentionOption::Enable) => {
                self.focus_active(window, cx);
                self.set_intention_enabled(title, true, cx);
            }
            Some(IntentionOption::Settings) => {
                self.open_settings(crate::settings_model::Page::Editor, window, cx);
            }
            None => return false,
        }
        true
    }

    /// Turns the intention titled `title` off or on again in the user's settings and says so in
    /// the status bar.
    pub fn set_intention_enabled(&mut self, title: &str, enabled: bool, cx: &mut Context<Self>) {
        let mut settings = crate::appearance::ide_settings(cx);
        settings.intentions = self.intention_settings(cx);
        settings.intentions.set_enabled(title, enabled);
        self.status = Some(match crate::appearance::set_ide_settings(settings, cx) {
            Err(e) => format!("Settings not saved: {e}"),
            Ok(()) if enabled => format!("Intention '{title}' turned on"),
            Ok(()) => format!(
                "Intention '{title}' turned off; turn it on again in Settings › Editor › General"
            ),
        });
        cx.notify();
    }
}

//! The tenth package of workbench actions that exist under their keymap id
//! (`docs/research/KEYMAP_COVERAGE.md`): Insert Live Template, the live templates at the caret
//! (`SPEC-0072` v14 behaviour 50). Next and Previous Template Variable are keys of an inserted
//! template (behaviour 51, in the editor's key handling), Iterate Recent Files a key of the
//! Switcher (`SPEC-0059` v9.10 behaviour 27h, `tab_list_view.rs`).

use std::path::PathBuf;

use ferris_idea_languages::insight::CompletionItem;
use gpui::{Context, Window};

use super::WorkbenchView;
use crate::completion_model;

impl WorkbenchView {
    /// The actions of the tenth package by id; false for another id or where the action is not
    /// available (then the next action of the chord is tried).
    pub(super) fn run_tenth_package_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            "InsertLiveTemplate" => {
                if self.active_editor().is_none() {
                    return false;
                }
                self.insert_live_template(window, cx);
                true
            }
            _ => false,
        }
    }

    /// `InsertLiveTemplate` (⌘J, Ctrl+J): asks for the suggestions at the caret as Code Completion
    /// does and shows only the templates among them (behaviour 50).
    fn insert_live_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_path() else {
            return;
        };
        let sql = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("sql"));
        if sql || ferris_idea_languages::LanguageId::detect(&path).is_none() {
            self.status = Some(completion_model::NO_LIVE_TEMPLATES.into());
            cx.notify();
            return;
        }
        self.start_completion_of(false, completion_model::Shown::Templates, window, cx);
    }

    /// Shows the templates among `found` in the completion list, or says there are none.
    pub(super) fn show_live_templates(
        &mut self,
        path: PathBuf,
        start: usize,
        found: Option<Vec<CompletionItem>>,
        cx: &mut Context<Self>,
    ) {
        let templates = completion_model::live_templates(found.unwrap_or_default());
        if templates.is_empty() {
            self.status = Some(completion_model::NO_LIVE_TEMPLATES.into());
            cx.notify();
            return;
        }
        self.show_completion(false, path, start, Some(templates), cx);
    }
}

//! The eighth package of workbench actions that exist under their keymap id
//! (`docs/research/KEYMAP_COVERAGE.md`): the overloads of parameter info (`SPEC-0080` v4
//! behaviour 8), class name completion and inserting with a dot (`SPEC-0072` v13 behaviours 48
//! and 49), Inspect Current File and Run Inspection by Name (`SPEC-0098` v10 behaviours 20 and
//! 21), the files of a commit (`SPEC-0086` v12 behaviour 21), the gutter icon tooltip from the
//! keyboard (`SPEC-0294` v5 behaviour 12), Recompile (`SPEC-0100` v5 behaviour 22), Unwrap/Remove
//! (`SPEC-0056` v5 behaviour 21) and Quick Evaluate Expression (`SPEC-0091` v19 behaviour 44).
//! Next and previous editor tab are the tab steps of `run_action`; Introduce Parameter is a
//! refactoring shortcut (`SPEC-0095` v11 behaviour 14); Complete Current Statement is an editor
//! command (`ferris_idea_editor::statement`). The decisions without a window are in
//! [`crate::keymap_actions`], [`crate::completion_model`], [`crate::debug_model`] and
//! [`crate::tips`].

use std::path::PathBuf;

use ferris_idea_languages::insight::{CompletionItem, SignatureHelp};
use gpui::{Context, Window};

use super::{DocPopup, ParamInfo, WorkbenchView};
use crate::completion_model;
use crate::keymap_actions;

impl ParamInfo {
    /// Takes a new answer of the server for the open popup: a chosen overload stays while the
    /// number of signatures does (`SPEC-0080` v4 behaviour 8).
    pub(super) fn renew(&mut self, help: SignatureHelp) {
        let count = self.help.signatures.len();
        self.help = help;
        match self.overload {
            Some(chosen) if self.help.signatures.len() == count && chosen < count => {
                self.help.active_signature = chosen;
            }
            _ => self.overload = None,
        }
    }
}

impl WorkbenchView {
    /// The actions of the eighth package by id; false for another id or where the action is not
    /// available (then the next action of the chord is tried).
    pub(super) fn run_eighth_package_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            keymap_actions::OVERLOAD_UP => self.switch_overload(false, cx),
            keymap_actions::OVERLOAD_DOWN => self.switch_overload(true, cx),
            "ClassNameCompletion" => {
                self.class_name_completion(window, cx);
                true
            }
            completion_model::CHOOSE_DOT => {
                if self.completion.is_none() {
                    return false;
                }
                self.accept_completion_with_dot(cx);
                true
            }
            "CodeInspection.OnEditor" => {
                self.inspect_current_file(window, cx);
                true
            }
            "RunInspection" => {
                self.run_inspection_by_name(window, cx);
                true
            }
            keymap_actions::SHOW_ALL_AFFECTED => self.show_all_affected(window, cx),
            "EditorShowGutterIconTooltip" => self.show_gutter_tooltip(cx),
            "Compile" => {
                self.recompile(window, cx);
                true
            }
            "Unwrap" => self.unwrap_block(window, cx),
            "QuickEvaluateExpression" => self.quick_evaluate_expression(window, cx),
            "IntroduceParameter" => {
                self.refactor_shortcut(
                    ferris_idea_languages::actions::Refactoring::Parameter,
                    window,
                    cx,
                );
                true
            }
            _ => false,
        }
    }

    /// `MethodOverloadSwitchUp` and `MethodOverloadSwitchDown`: the signature above or below
    /// the highlighted one is highlighted in parameter info, wrapping. False without parameter
    /// info, with one signature and while the completion list is open.
    pub(super) fn switch_overload(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        if self.completion.is_some() {
            return false;
        }
        let Some(p) = self.params.as_mut() else {
            return false;
        };
        let Some(next) =
            keymap_actions::overload_step(p.help.signatures.len(), p.help.active_signature, down)
        else {
            return false;
        };
        p.help.active_signature = next;
        p.overload = Some(next);
        cx.notify();
        true
    }

    /// Takes `help` as a new answer of the server for the open parameter info, as following the
    /// caret does; false without parameter info. For checks without a language server.
    pub fn refresh_param_info(&mut self, help: SignatureHelp, cx: &mut Context<Self>) -> bool {
        let Some(p) = self.params.as_mut() else {
            return false;
        };
        p.renew(help);
        cx.notify();
        true
    }

    /// The index of the highlighted signature of parameter info, if it is open.
    pub fn param_info_active(&self) -> Option<usize> {
        self.params.as_ref().map(|p| p.help.active_signature)
    }

    /// `ClassNameCompletion`: the suggestions at the caret that name a type (`SPEC-0072` v13
    /// behaviour 48).
    fn class_name_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_path() else {
            return;
        };
        let sql = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("sql"));
        if sql || ferris_idea_languages::LanguageId::detect(&path).is_none() {
            self.status = Some(keymap_actions::NO_CLASS_NAMES.into());
            cx.notify();
            return;
        }
        self.start_completion_of(false, completion_model::Shown::Classes, window, cx);
    }

    /// Shows the type names among `found` in the completion list, or says there are none.
    pub(super) fn show_class_names(
        &mut self,
        path: PathBuf,
        start: usize,
        found: Option<Vec<CompletionItem>>,
        cx: &mut Context<Self>,
    ) {
        let types = completion_model::class_names(found.unwrap_or_default());
        if types.is_empty() {
            self.status = Some(keymap_actions::NO_CLASS_NAMES.into());
            cx.notify();
            return;
        }
        self.show_completion(false, path, start, Some(types), cx);
    }

    /// Opens the completion list with `items` for the identifier at the caret of the active
    /// editor, as an answer of the language server does; with `classes` as class name
    /// completion. False without an editor. For checks without a language server.
    pub fn show_completion_items(
        &mut self,
        items: Vec<CompletionItem>,
        classes: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let start = {
            let e = editor.read(cx);
            let s = e.status();
            let line = e
                .state
                .buffer
                .line(s.line - 1)
                .unwrap_or_default()
                .trim_end_matches(['\n', '\r'])
                .to_owned();
            let column = s.column - 1;
            e.caret() - (column - completion_model::word_start(&line, column))
        };
        if classes {
            self.show_class_names(path, start, Some(items), cx);
        } else {
            self.show_completion(false, path, start, Some(items), cx);
        }
        true
    }

    /// `EditorChooseLookupItemDot`: the selected suggestion is inserted and `.` typed behind it,
    /// after the parentheses of an empty call; not while the suggestion's places are filled in
    /// (`SPEC-0072` v13 behaviour 49).
    pub(super) fn accept_completion_with_dot(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.completion.as_ref().map(|c| c.path.clone()) else {
            return;
        };
        self.accept_completion(false, cx);
        if self.snippet.is_some() {
            return;
        }
        let Some(editor) = self.editors.get(&path).cloned() else {
            return;
        };
        editor.update(cx, |e, cx| {
            let caret = e.caret();
            let buffer = &e.state.buffer;
            let around = (
                caret
                    .checked_sub(1)
                    .and_then(|b| buffer.slice(b..caret).ok()),
                buffer.slice(caret..caret + 1).ok(),
            );
            let at = match around {
                (Some(open), Some(close)) if open == "(" && close == ")" => caret + 1,
                _ => caret,
            };
            e.replace_ranges(&[(at..at, ".".to_owned())], cx);
        });
        cx.notify();
    }

    /// `CodeInspection.OnEditor`: Inspect Code over the active file, without the scope dialog
    /// (`SPEC-0098` v10 behaviour 20).
    fn inspect_current_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_path() else {
            self.status = Some(keymap_actions::INSPECT_FILE_NEEDS_FILE.into());
            cx.notify();
            return;
        };
        self.inspect_only = None;
        self.inspect_dialog = None;
        self.run_inspection(crate::problems_model::InspectScope::File(path), window, cx);
    }

    /// `RunInspection`: the catalogue's inspections under their language, filtered by typing;
    /// the chosen one opens the scope dialog for it alone (`SPEC-0098` v10 behaviour 21).
    fn run_inspection_by_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut items = Vec::new();
        let mut headings = Vec::new();
        let mut ids = Vec::new();
        let mut language = "";
        for info in ferris_idea_languages::inspections::catalogue() {
            if info.language != language {
                language = info.language;
                items.push(language.to_owned());
                headings.push(true);
                ids.push(None);
            }
            items.push(info.name.to_owned());
            headings.push(false);
            ids.push(Some(info.id));
        }
        if ids.iter().all(Option::is_none) {
            return;
        }
        self.choose_from_with(
            keymap_actions::RUN_INSPECTION_TITLE.into(),
            items,
            1,
            move |v| v.with_checks(Vec::new(), headings).with_filter(),
            window,
            cx,
            move |this, i, window, cx| {
                let Some(Some(id)) = ids.get(i).copied() else {
                    return;
                };
                // The scope dialog for this inspection only (SPEC-0098 v4 behaviour 10).
                let choices =
                    crate::problems_model::InspectScope::choices(this.active_path().as_deref());
                this.inspect_only = Some(id);
                this.copyright_dialog = false;
                this.inspect_dialog = Some((choices, 0));
                window.focus(&this.inspect_dialog_focus);
                cx.notify();
            },
        );
    }

    /// The one inspection the next run is limited to, if any (`SPEC-0098` v4 behaviour 10).
    pub fn inspect_only(&self) -> Option<&'static str> {
        self.inspect_only
    }

    /// The scope of the last Inspect Code.
    pub fn inspect_scope(&self) -> crate::problems_model::InspectScope {
        self.inspect_scope.clone()
    }

    /// `VcsHistory.ShowAllAffected`: every file of the commit selected in the Git log, Enter
    /// opens its diff (`SPEC-0086` v12 behaviour 21). False without a selected commit.
    fn show_all_affected(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(log) = self.git_log.clone() else {
            return false;
        };
        let Some(id) = log.read(cx).selected_id().map(str::to_owned) else {
            return false;
        };
        let details = ferris_idea_vcs::Repo::discover(&self.root)
            .and_then(|r| r.commit_details(&id))
            .ok()
            .or_else(|| log.read(cx).details().filter(|d| d.info.id == id).cloned());
        let Some(details) = details else {
            return false;
        };
        if details.changes.is_empty() {
            self.status = Some(keymap_actions::NO_AFFECTED_FILES.into());
            cx.notify();
            return true;
        }
        let rows = keymap_actions::affected_rows(&details.changes);
        let paths: Vec<String> = details.changes.iter().map(|c| c.path.clone()).collect();
        let commit = details.info.id.clone();
        self.choose_from(
            keymap_actions::affected_title(&details.info.short_id, &details.info.summary),
            rows,
            0,
            window,
            cx,
            move |this, i, window, cx| {
                if let Some(path) = paths.get(i) {
                    this.show_commit_diff(&commit, path, window, cx);
                }
            },
        );
        true
    }

    /// `EditorShowGutterIconTooltip`: the tooltips of the gutter icons on the caret's line
    /// below the caret (`SPEC-0294` v5 behaviour 12). False without an editor.
    fn show_gutter_tooltip(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let (tips, offset) = {
            let e = editor.read(cx);
            (
                e.gutter_tips_on_line(e.status().line.saturating_sub(1)),
                e.state.carets.primary().head,
            )
        };
        let text = crate::tips::gutter_tips_text(&tips);
        if text.is_empty() {
            self.status = Some(keymap_actions::NO_GUTTER_ICONS.into());
            cx.notify();
            return true;
        }
        self.doc = Some(DocPopup {
            path,
            offset,
            blocks: text
                .split('\n')
                .filter(|l| !l.is_empty())
                .map(|l| crate::doc_model::DocBlock::Text(l.to_owned(), Vec::new()))
                .collect(),
            problem: None,
            pointer: None,
        });
        cx.notify();
        true
    }

    /// `Compile`, Recompile: the build of the module of the active file, or of the file
    /// selected in the Project tree while it has the focus (`SPEC-0100` v5 behaviour 22).
    fn recompile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let file = if self.tree_focus.is_focused(window) {
            Some(self.tree.selected_path().to_path_buf())
        } else {
            self.active_path()
        };
        let files = ferris_idea_core::Workspace::open(&self.root)
            .and_then(|w| w.files())
            .unwrap_or_default();
        let model = ferris_idea_core::read_project(&self.root, &files);
        let options = ferris_idea_core::buildtools::BuildOptions::load(&self.root);
        let Some(step) = file.and_then(|f| {
            ferris_idea_core::buildtasks::recompile_step(&self.root, &model, &options, &f)
        }) else {
            self.status = Some(keymap_actions::NOTHING_TO_COMPILE.into());
            cx.notify();
            return;
        };
        let name = step.name.clone();
        self.run_temporary.retain(|t| t.name != name);
        self.run_temporary.push(step);
        self.reload_run_configs();
        if let Some(problem) = self.run_problem() {
            self.status = Some(problem);
            cx.notify();
            return;
        }
        self.start_run_named(&name, window, cx);
        self.status = Some(format!("Recompile: {name}"));
        cx.notify();
    }

    /// `Unwrap`: the blocks around the caret that can be unwrapped, innermost first; the chosen
    /// one loses its opening and closing lines in one undo step (`SPEC-0056` v5 behaviour 21).
    /// False without an editor.
    fn unwrap_block(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let (lines, choices) = {
            let e = editor.read(cx);
            let s = e.status();
            let lines: Vec<String> = e
                .state
                .buffer
                .text()
                .split('\n')
                .map(|l| l.trim_end_matches('\r').to_owned())
                .collect();
            let choices = ferris_idea_editor::statement::unwrap_choices(
                &lines,
                s.line - 1,
                s.column - 1,
                e.state.language.as_deref(),
            );
            (lines, choices)
        };
        if choices.is_empty() {
            self.status = Some(keymap_actions::NOTHING_TO_UNWRAP.into());
            cx.notify();
            return true;
        }
        let labels = choices.iter().map(|u| u.label.clone()).collect();
        self.choose_from(
            keymap_actions::UNWRAP_TITLE.into(),
            labels,
            0,
            window,
            cx,
            move |this, i, window, cx| {
                let Some(u) = choices.get(i) else {
                    return;
                };
                // The file may have been closed meanwhile.
                let Some(editor) = this.editors.get(&path).cloned() else {
                    return;
                };
                let (range, body) = ferris_idea_editor::statement::unwrapped(&lines, u);
                editor.update(cx, |e, cx| {
                    let buffer = &e.state.buffer;
                    let nl = buffer.line_ending().as_str();
                    let start = buffer
                        .position_to_offset(ferris_idea_editor::Position::new(range.start, 0))
                        .unwrap_or(0);
                    let last = range.end >= buffer.len_lines();
                    let end = if last {
                        buffer.len_chars()
                    } else {
                        buffer
                            .position_to_offset(ferris_idea_editor::Position::new(range.end, 0))
                            .unwrap_or(buffer.len_chars())
                    };
                    let mut text = body.join(nl);
                    if !last && !body.is_empty() {
                        text.push_str(nl);
                    }
                    e.replace_ranges(&[(start..end, text)], cx);
                    e.select_range(start..start, cx);
                });
                this.focus_active(window, cx);
            },
        );
        true
    }

    /// `QuickEvaluateExpression`: the selection or the expression at the caret, evaluated in
    /// the paused session's frame, shown below the caret (`SPEC-0091` v19 behaviour 44). False
    /// without a session.
    fn quick_evaluate_expression(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.debug_view.clone().filter(|v| v.read(cx).active()) else {
            return false;
        };
        let _ = window;
        let Some(text) = self
            .active_editor()
            .and_then(|e| e.read(cx).expression_at_caret())
        else {
            self.status = Some(crate::debug_model::NOTHING_TO_EVALUATE.into());
            cx.notify();
            return true;
        };
        if let Err(why) = view.update(cx, |v, _| v.quick_evaluate(&text)) {
            self.status = Some(why.into());
        }
        cx.notify();
        true
    }
}

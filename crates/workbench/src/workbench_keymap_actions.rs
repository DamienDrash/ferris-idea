//! Workbench actions that exist under their keymap id because the bundled keymaps bind them
//! (`docs/research/KEYMAP_COVERAGE.md`): the diff viewer's stepping and Compare Two Files
//! (`SPEC-0084` v4), the Commit tool window's operations (`SPEC-0063` v9), New Branch and Rename
//! (`SPEC-0068` v9), the Git log's actions of the selected commit (`SPEC-0086` v11), Jump to Last
//! Tool Window and Close Active Tab (`SPEC-0167` v5; Restore Current Layout is in
//! `workbench_layouts.rs`, `SPEC-0333`), Select In, Reload
//! All from Disk and Save As (`SPEC-0059` v9.3), the Find tool window's grouping toggles (`SPEC-0293`
//! v6), Quick Definition (`SPEC-0081` v4), Go to Test and Go to Related (`SPEC-0071` v10),
//! Recently Changed Files (`SPEC-0069` v3), and of the fourth package Expand All and Collapse All
//! of the focused tree, File Path, New in the same place and Open in Right Split (`SPEC-0275`
//! v13), the query history (`SPEC-0093` v5) and the recent searches (`SPEC-0065` v5), and of the
//! fifth package the editor font size (`SPEC-0129` v4). The decisions without a window are in
//! [`crate::keymap_actions`].

use gpui::{AppContext, Context, Window};
use idecraft_ui::tool_windows::ToolWindow;
use idecraft_vcs::Repo;

use super::{DocPopup, WorkbenchView, file_status_keys};
use crate::DiffView;
use crate::diff_actions::{self, Pane, Side};
use crate::keymap_actions::{self, CommitOp, QuickTarget, Scope};
use crate::{UsagesEvent, UsagesView};

impl WorkbenchView {
    /// `EditorIncreaseFontSizeGlobal` and `EditorDecreaseFontSizeGlobal` (`SPEC-0129` v4
    /// behaviour 8): the editor font size of the user's settings one step of `delta` larger or
    /// smaller, for every window, saved, and named in the status bar; at a limit nothing changes
    /// and the status bar says so. Not carried out (`false`) while the window has no editor.
    pub(super) fn change_editor_font_size(&mut self, delta: f32, cx: &mut Context<Self>) -> bool {
        if self.active_editor().is_none() {
            return false;
        }
        let mut settings = crate::appearance::ide_settings(cx);
        let size = settings.editor.font_size;
        let Some(next) = idecraft_core::font_size_step(size, delta) else {
            self.status = Some(format!(
                "Editor font size: {size} is the {}",
                if delta > 0.0 { "largest" } else { "smallest" }
            ));
            cx.notify();
            return true;
        };
        settings.editor.font_size = next;
        self.status = Some(match crate::appearance::set_ide_settings(settings, cx) {
            Ok(()) => format!("Editor font size: {next}"),
            Err(e) => format!("Settings not saved: {e}"),
        });
        // An open Settings dialog shows the new size.
        if let Some(view) = self.settings.clone() {
            view.update(cx, |v, cx| v.reload_ide(cx));
        }
        cx.notify();
        true
    }

    /// Whether the part `scope` has the keyboard focus: the keys of its actions run only there.
    pub(super) fn scope_focused(&self, scope: Scope, window: &Window, cx: &gpui::App) -> bool {
        match scope {
            Scope::Diff => self
                .diff
                .as_ref()
                .is_some_and(|d| d.read(cx).focus().contains_focused(window, cx)),
            // The whole window, its file list as well as its fields: a click on a row puts the
            // focus on the window, not into the message field (`SPEC-0028` v6 behaviour 11).
            Scope::Commit => {
                self.tool_window_shown(ToolWindow::Commit)
                    && self.commit.as_ref().is_some_and(|c| {
                        gpui::Focusable::focus_handle(c.read(cx), cx).contains_focused(window, cx)
                    })
            }
            Scope::CommitMessage => {
                self.tool_window_shown(ToolWindow::Commit)
                    && self
                        .commit
                        .as_ref()
                        .is_some_and(|c| c.read(cx).focus(cx).contains_focused(window, cx))
            }
            Scope::Branches => self
                .branches
                .as_ref()
                .is_some_and(|b| b.read(cx).focus(cx).contains_focused(window, cx)),
            Scope::Log => self.git_log.as_ref().is_some_and(|l| {
                let log = l.read(cx);
                log.in_commit_list() && log.focus().contains_focused(window, cx)
            }),
            Scope::Tree => self.tree_focus.is_focused(window),
            Scope::Find => self
                .find_tool_window()
                .is_some_and(|v| v.read(cx).focus().contains_focused(window, cx)),
            Scope::Todo => self.todo_focused(window, cx),
            Scope::Trees => self.focused_tree(window, cx).is_some(),
            Scope::Run => {
                self.tool_window_shown(ToolWindow::Run)
                    && self
                        .run_view
                        .as_ref()
                        .is_some_and(|r| r.read(cx).focus().contains_focused(window, cx))
            }
        }
    }

    /// Runs the action the keymap binds to this key event for the focused text input itself, if
    /// there is one, before the input sees the key (`SPEC-0028` v6 behaviour 11b): the input's
    /// editing action on the same chord would take it otherwise. A key that arrives while the
    /// input method composes is left alone (behaviour 11a), and so is a key without ⌃, ⌥ or ⌘
    /// that types text.
    pub(super) fn input_chord(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &event.keystroke;
        let m = &k.modifiers;
        if !(m.control || m.alt || m.platform) && k.key_char.is_some() {
            return false;
        }
        let keymap = crate::keymaps::current();
        let Some(stroke) = idecraft_ui::keymap::EventStrokes::of(
            &k.key,
            m.control,
            m.alt,
            m.shift,
            m.platform,
            cx.keyboard_mapper().get_key_equivalents(),
        )
        .resolve(&keymap) else {
            return false;
        };
        let bound: Vec<String> = keymap
            .actions_for(&stroke, None)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let own = keymap_actions::input_actions(bound, |scope| {
            self.scope_focused(scope, window, cx) && !self.input_composing(scope, cx)
        });
        for id in own {
            if self.run_action(&id, window, cx) {
                crate::key_diagnostics::note_ran(&id, "text input");
                return true;
            }
        }
        false
    }

    /// Whether the input method composes in a text input of the part `scope` names: a key it
    /// left over is then no chord of that part (`SPEC-0028` v6 behaviour 11a).
    pub(super) fn input_composing(&self, scope: Scope, cx: &gpui::App) -> bool {
        match scope {
            Scope::Commit | Scope::CommitMessage => self
                .commit
                .as_ref()
                .is_some_and(|c| c.read(cx).composing(cx)),
            _ => false,
        }
    }

    /// The tool window whose tree has the keyboard focus, of those with Expand All or Collapse
    /// All (`SPEC-0275` v13 behaviour 33).
    fn focused_tree(&self, window: &Window, cx: &gpui::App) -> Option<ToolWindow> {
        if self.tree_focus.is_focused(window) {
            return Some(ToolWindow::Project);
        }
        [
            ToolWindow::Commit,
            ToolWindow::Find,
            ToolWindow::Run,
            ToolWindow::Build,
            ToolWindow::Database,
        ]
        .into_iter()
        .find(|w| self.tool_window_shown(*w) && self.tool_window_focused(*w, window, cx))
    }

    /// `ExpandAll` and `CollapseAll`: the Expand All or Collapse All button of the tree that has
    /// the focus, else of the Project tree (`SPEC-0275` v13 behaviour 33). False where the tree
    /// has no such button.
    pub(super) fn tree_all_action(
        &mut self,
        expand: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.focused_tree(window, cx).unwrap_or(ToolWindow::Project) {
            ToolWindow::Commit => {
                let Some(view) = self.commit.clone() else {
                    return false;
                };
                view.update(cx, |v, cx| {
                    if expand {
                        v.expand_all(cx);
                    } else {
                        v.collapse_all(cx);
                    }
                });
            }
            ToolWindow::Find => {
                let Some(view) = self.find_tool_window() else {
                    return false;
                };
                view.update(cx, |v, cx| {
                    v.press(if expand { "expand" } else { "collapse" }, cx)
                });
            }
            ToolWindow::Run => {
                let Some(view) = self.run_view.clone() else {
                    return false;
                };
                view.update(cx, |v, cx| v.expand_all(expand, cx));
            }
            ToolWindow::Build => {
                let Some(view) = self.build_tools.clone() else {
                    return false;
                };
                view.update(cx, |v, cx| v.expand_all(expand, cx));
            }
            ToolWindow::Database => {
                // The Database tree has Collapse All only.
                let Some(view) = self.database.clone().filter(|_| !expand) else {
                    return false;
                };
                view.update(cx, |v, cx| v.collapse_all(cx));
            }
            _ => {
                if expand {
                    self.tree.expand_all();
                } else {
                    self.tree.collapse_all();
                }
                self.scroll_tree_to_selection();
            }
        }
        cx.notify();
        true
    }

    /// `ExpandTreeNode`, `CollapseTreeNode` and `FullyExpandTreeNode` chosen in Find Action: the
    /// key's command on the selected row of the Project tree (`SPEC-0275` v13 behaviour 32).
    pub(super) fn tree_node_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(key) = crate::tree_nav::action_key(id) else {
            return false;
        };
        let command = crate::tree_nav::on_key(&self.tree.nav_rows(), self.tree.selected_row(), key);
        self.tree.apply(command);
        cx.notify();
        true
    }

    /// `ShowFilePath`: Reveal in Finder for the active editor's file, or for the Project tree's
    /// selection while the tree has the focus (`SPEC-0275` v13 behaviour 34).
    pub(super) fn show_file_path(&mut self, window: &Window, cx: &mut Context<Self>) {
        let selected = (!self.tree.is_virtual(self.tree.selected))
            .then(|| self.tree.selected_path().to_path_buf());
        let target = keymap_actions::file_path_target(
            self.tree_focus.is_focused(window),
            self.active_path().as_deref(),
            selected.as_deref(),
        );
        match target {
            Some(path) => self.reveal_in_file_manager(&path),
            None => self.status = Some(keymap_actions::NO_FILE_TO_SHOW.into()),
        }
        cx.notify();
    }

    /// What Copy Path/Reference… acts on (`SPEC-0332` v2 behaviours 13 and 13d): the project
    /// tree's selection while the tree has the keyboard focus, else the file of the active
    /// editor with its caret's line.
    fn copy_subject(
        &self,
        window: &Window,
        cx: &gpui::App,
    ) -> Option<crate::copy_path_model::Subject> {
        use crate::copy_path_model::Subject;
        if self.tree_focus.is_focused(window) {
            let selected = self.tree.selected_paths();
            if !selected.is_empty() {
                return Some(Subject::tree(selected));
            }
        }
        self.here(cx)
            .map(|here| Subject::editor(here.path, here.line))
    }

    /// The project's source folders for the copy rows; a remote project has none.
    fn copy_source_roots(&self) -> Vec<std::path::PathBuf> {
        if self.remote_project.is_some() {
            Vec::new()
        } else {
            self.tree.source_roots()
        }
    }

    /// The rows of Copy Path/Reference… for `subject` (`SPEC-0332` v2 behaviour 13). The Copy
    /// Reference row of an editor shows file and line where no symbol will be asked for.
    fn copy_path_rows(
        &self,
        subject: &crate::copy_path_model::Subject,
    ) -> Vec<crate::copy_path_model::Row> {
        let folders = self.remote_folders();
        let roots = self.copy_source_roots();
        let scope = crate::copy_path_model::Scope {
            root: &self.root,
            source_roots: &roots,
            remote: folders.as_ref(),
        };
        let reference = subject.line.zip(subject.paths.first()).map(|(line, path)| {
            let language = idecraft_languages::LanguageId::detect(path);
            if folders.is_none() && idecraft_languages::reference_name::has_rules(language) {
                String::new()
            } else {
                scope.line_reference(path, line)
            }
        });
        crate::copy_path_model::rows(subject, &scope, reference)
    }

    /// Puts a row of Copy Path/Reference… on the clipboard, as plain text, and says so in the
    /// status bar (`SPEC-0332` v2 behaviour 13f).
    fn copy_path_row(&mut self, row: &crate::copy_path_model::Row, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(row.text.clone()));
        self.status = Some(row.copied());
        cx.notify();
    }

    /// `CopyReferencePopupGroup`: the list of what can be copied for the active editor's file or
    /// the project tree's selection (`SPEC-0332` v2 behaviour 13).
    pub(super) fn copy_path_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self
            .copy_subject(window, cx)
            .map(|subject| self.copy_path_rows(&subject))
            .unwrap_or_default();
        if rows.is_empty() {
            self.status = Some(crate::copy_path_model::NO_FILE.into());
            cx.notify();
            return;
        }
        let items = rows.iter().map(|r| r.label()).collect();
        self.show_jvm_popup(
            crate::copy_path_model::TITLE.into(),
            items,
            window,
            cx,
            move |this, i, window, cx| match rows.get(i) {
                // The name of the symbol is asked for now.
                Some(row) if row.action == crate::copy_path_model::REFERENCE => {
                    this.copy_reference(window, cx);
                }
                Some(row) => this.copy_path_row(row, cx),
                None => {}
            },
        );
    }

    /// `CopyAbsolutePath`, `CopyFileName`, `CopyPathWithLineNumber`, `CopyContentRootPath`,
    /// `CopySourceRootPath`: one row of the popup, copied at once (`SPEC-0332` v2 behaviour 13).
    pub(super) fn copy_path_action(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self
            .copy_subject(window, cx)
            .map(|subject| self.copy_path_rows(&subject))
            .unwrap_or_default();
        match crate::copy_path_model::row_of(&rows, action) {
            Some(row) => self.copy_path_row(row, cx),
            None if rows.is_empty() => {
                self.status = Some(crate::copy_path_model::NO_FILE.into());
            }
            None => self.status = Some(crate::copy_path_model::unavailable(action).into()),
        }
        cx.notify();
    }

    /// One kind of path of the items `targets` of the project tree or of a tab's file, copied
    /// from their menu (`SPEC-0332` v2 behaviours 13d and 13e).
    pub(super) fn copy_path_of(
        &mut self,
        action: &str,
        targets: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let rows = self.copy_path_rows(&crate::copy_path_model::Subject::tree(targets));
        match crate::copy_path_model::row_of(&rows, action) {
            Some(row) => self.copy_path_row(row, cx),
            None => self.status = Some(crate::copy_path_model::unavailable(action).into()),
        }
        cx.notify();
    }

    /// Path With Line Number of a tab's menu: the active editor's file, whatever has the focus
    /// (`SPEC-0332` v2 behaviour 13e).
    pub(super) fn copy_path_with_line(&mut self, cx: &mut Context<Self>) {
        use crate::copy_path_model::{PATH_WITH_LINE, Subject, row_of, unavailable};
        let rows = self
            .here(cx)
            .map(|here| self.copy_path_rows(&Subject::editor(here.path, here.line)))
            .unwrap_or_default();
        match row_of(&rows, PATH_WITH_LINE) {
            Some(row) => self.copy_path_row(row, cx),
            None => self.status = Some(unavailable(PATH_WITH_LINE).into()),
        }
        cx.notify();
    }

    /// `CopyReference`: the qualified name of the symbol around the active editor's caret, from
    /// the language server's symbols of the file; file and line where there is none (`SPEC-0332`
    /// v2 behaviours 13b and 13c).
    pub(super) fn copy_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use idecraft_languages::reference_name;
        let (Some(here), Some(editor)) = (self.here(cx), self.active_editor()) else {
            self.status = Some(crate::copy_path_model::NO_EDITOR.into());
            cx.notify();
            return;
        };
        let folders = self.remote_folders();
        let roots = self.copy_source_roots();
        let fallback = crate::copy_path_model::Scope {
            root: &self.root,
            source_roots: &roots,
            remote: folders.as_ref(),
        }
        .line_reference(&here.path, here.line);
        let language = idecraft_languages::LanguageId::detect(&here.path);
        // A remote document's symbols are not asked of the host; other languages have no names.
        if folders.is_some() || !reference_name::has_rules(language) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(fallback.clone()));
            self.status = Some(crate::copy_path_model::reference_copied(&fallback, false));
            cx.notify();
            return;
        }
        let text = editor.read(cx).state.buffer.text();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let (file, asked, folder) = (here.path.clone(), text.clone(), root.clone());
            let found = cx
                .background_spawn(async move {
                    // Where the file's names start: reads the folders around it.
                    let module = reference_name::module_of(language, &folder, &file, &roots);
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    let symbols = rt
                        .block_on(idecraft_languages::insight::symbols_in(
                            &folder,
                            &file,
                            Some(&asked),
                            std::time::Duration::from_secs(5),
                        ))
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>((module, symbols))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let (copied, no_symbols) = match found {
                    Ok((module, symbols)) => (
                        reference_name::qualified_name(
                            language,
                            module.as_deref(),
                            &text,
                            &symbols,
                            here.line as u64,
                            here.column as u64,
                        )
                        .unwrap_or(fallback),
                        false,
                    ),
                    Err(_) => (fallback, true),
                };
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                this.status = Some(crate::copy_path_model::reference_copied(
                    &copied, no_symbols,
                ));
                cx.notify();
            });
        })
        .detach();
    }

    /// `NewElementSamePlace`: the New menu for the folder of the active editor's file; without
    /// one, New of the tree's selection (`SPEC-0275` v13 behaviour 35).
    pub(super) fn new_in_the_same_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match keymap_actions::same_place_folder(self.active_path().as_deref()) {
            Some(folder) => self.open_new_popup_for(folder, window, cx),
            None => self.open_new_popup(window, cx),
        }
    }

    /// `OpenInRightSplit` from the Project tree: the selected file opens and goes into the right
    /// split, as ⇧↩ does in Search Everywhere (`SPEC-0275` v13 behaviour 36).
    pub(super) fn open_tree_file_in_right_split(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.tree.selected_path().to_path_buf();
        if self.tree.is_virtual(self.tree.selected) || !path.is_file() {
            self.status = Some(keymap_actions::SELECT_A_FILE_TO_SPLIT.into());
            cx.notify();
            return;
        }
        self.open(&path, window, cx);
        if self.tabs.paths().len() < 2 {
            self.split_editor(false, window, cx);
        } else {
            self.split_and_move(false, window, cx);
        }
    }

    /// `Console.History.Browse`: opens or closes the query history, as the History button of the
    /// console bar does; false when the active editor is no `.sql` file (`SPEC-0093` v5 behaviour
    /// 16).
    pub(super) fn browse_history(&mut self, cx: &mut Context<Self>) -> bool {
        let sql = self
            .active_path()
            .is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")));
        if !sql {
            return false;
        }
        self.toggle_history(cx);
        true
    }

    /// `ShowSearchHistory`: the recent searches of the open find bar; false without a bar
    /// (`SPEC-0065` v5 behaviour 19).
    pub(super) fn show_search_history(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(bar) = self.find.as_mut() else {
            return false;
        };
        bar.menu = Some(crate::find_bar::FindMenu::History);
        cx.notify();
        true
    }

    /// Whether the find bar shows its recent searches.
    pub fn search_history_shown(&self) -> bool {
        self.find
            .as_ref()
            .is_some_and(|b| b.menu == Some(crate::find_bar::FindMenu::History))
    }

    /// `UsageGrouping.UsageType`, `.Module`, `.Directory` and `.FileStructure`: flips the grouping
    /// toggle of the Find tool window's result (`SPEC-0293` v6 behaviour 18); without a result the
    /// status line says so.
    pub(super) fn usage_grouping_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(toggle) = keymap_actions::usage_grouping(id) else {
            return false;
        };
        match self
            .find_tool_window()
            .filter(|v| v.read(cx).tree().is_some())
        {
            Some(view) => view.update(cx, |v, cx| v.press(toggle, cx)),
            None => self.status = Some(keymap_actions::NO_USAGES_TO_GROUP.into()),
        }
        cx.notify();
        true
    }

    /// The filters (`UsageFiltering.ReadAccess`, `.WriteAccess`, `.Imports`), the nested folders
    /// (`UsageGrouping.DirectoryStructure`), the module groups (`UsageGrouping.FlattenModules`)
    /// and `UsageView.Exclude` / `UsageView.Include` of the Find tool window's result
    /// (`SPEC-0293` v9 behaviours 22–25). Without a result, and when Exclude or Include have
    /// nothing to do, the status bar says so. False only for Flatten Modules without Group by
    /// Module: its key then goes on to the next action bound to it.
    pub(super) fn usage_view_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(change) = keymap_actions::usage_view_change(id) else {
            return false;
        };
        let done = match self
            .find_tool_window()
            .filter(|v| v.read(cx).tree().is_some())
        {
            Some(view) => {
                let changed = view.update(cx, |v, cx| v.change(change, cx));
                if !changed && keymap_actions::usage_view_passes_on(id) {
                    false
                } else {
                    if !changed {
                        self.status = Some(keymap_actions::usage_view_unchanged(id).into());
                    }
                    true
                }
            }
            None => {
                self.status = Some(keymap_actions::NO_USAGES_TO_GROUP.into());
                true
            }
        };
        cx.notify();
        done
    }

    /// `ExportToTextFile`: the usages of the Find tool window written to a text file chosen in
    /// the system's save dialog (`SPEC-0293` v9 behaviour 26). Without a result the status bar
    /// says so.
    pub(super) fn export_usages(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(symbol) = self
            .find_tool_window()
            .filter(|v| v.read(cx).tree().is_some())
            .map(|v| v.read(cx).symbol().to_owned())
        else {
            self.status = Some(keymap_actions::NO_USAGES_TO_EXPORT.into());
            cx.notify();
            return true;
        };
        let name = keymap_actions::export_file_name(&symbol);
        let chosen = cx.prompt_for_new_path(&self.root, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(target))) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.export_usages_to(&target, cx));
        })
        .detach();
        true
    }

    /// Writes the usages of the Find tool window to `target` and says so in the status bar;
    /// the file is not opened and the project tree is read again when it lies in the project.
    pub fn export_usages_to(&mut self, target: &std::path::Path, cx: &mut Context<Self>) -> bool {
        let Some((text, count)) = self.find_tool_window().and_then(|v| v.read(cx).export()) else {
            self.status = Some(keymap_actions::NO_USAGES_TO_EXPORT.into());
            cx.notify();
            return false;
        };
        let written = std::fs::write(target, text.as_bytes());
        self.status = Some(match &written {
            Ok(()) => keymap_actions::exported(count, target),
            Err(e) => format!("Could not write {}: {e}", target.display()),
        });
        if written.is_ok() && target.starts_with(&self.root) {
            self.tree = self.tree.reloaded();
        }
        cx.notify();
        written.is_ok()
    }

    /// A list to choose one of `items` from, `selected` first; `chosen` gets the index. The
    /// focus goes back to where it was when the list closes.
    pub(super) fn choose_from(
        &mut self,
        title: String,
        items: Vec<String>,
        selected: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl Fn(&mut Self, usize, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.choose_from_with(title, items, selected, |v| v, window, cx, chosen);
    }

    /// [`Self::choose_from`] with the list shaped by `shape` first (headings, a filter).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn choose_from_with(
        &mut self,
        title: String,
        items: Vec<String>,
        selected: usize,
        shape: impl FnOnce(crate::ChoiceView) -> crate::ChoiceView,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl Fn(&mut Self, usize, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let before = window.focused(cx);
        let view =
            cx.new(|cx| shape(crate::ChoiceView::new(title, items, cx)).with_selected(selected));
        self._compare_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.compare_targets = None;
                match &before {
                    Some(focus) => window.focus(focus),
                    None => this.focus_active(window, cx),
                }
                if let crate::ChoiceEvent::Chosen(i) = *event {
                    chosen(this, i, window, cx);
                }
                cx.notify();
            },
        ));
        let focus = view.read(cx).focus();
        window.focus(&focus);
        self.close_on_focus_out(super::Popup::CompareTargets, &view, &focus, window, cx);
        self.compare_targets = Some(view);
        cx.notify();
    }

    /// `NextParameter` and `PrevParameter`: while parameter info is shown, the caret goes to
    /// the start of the next or previous argument of the call (`SPEC-0080` v3 behaviour 7).
    /// False without parameter info or without a further argument.
    pub(super) fn step_parameter(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self
            .params
            .as_ref()
            .and_then(|p| self.editors.get(&p.path).cloned())
        else {
            return false;
        };
        let (text, caret) = {
            let e = editor.read(cx);
            (e.state.buffer.text(), e.caret())
        };
        let Some(to) = keymap_actions::parameter_step(&text, caret, forward) else {
            return false;
        };
        // The shown signatures follow the caret by themselves (behaviour 3).
        editor.update(cx, |e, cx| {
            e.select_ranges(std::slice::from_ref(&(to..to)), cx)
        });
        cx.notify();
        true
    }

    /// Shows parameter info with `help` at the caret of the active editor, as an answer of the
    /// language server does; false without an editor. For checks without a language server.
    pub fn show_param_info(
        &mut self,
        help: idecraft_languages::insight::SignatureHelp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let (caret, revision) = {
            let e = editor.read(cx);
            (e.caret(), e.revision())
        };
        let watch = cx.observe_in(&editor, window, |this, editor, window, cx| {
            this.follow_params(editor, window, cx)
        });
        self.params = Some(super::ParamInfo {
            path,
            offset: caret,
            help,
            asked: (caret, revision),
            pending: false,
            overload: None,
            _watch: watch,
        });
        cx.notify();
        true
    }

    /// `ViewSource`, Show Source: the file selected in the Project tree, or the usage selected
    /// in the Find tool window, opens in the editor and the focus stays where it is
    /// (`SPEC-0275` v18 behaviour 45). False where neither has the focus.
    pub(super) fn view_source(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.tree_focus.is_focused(window) {
            let path = self.tree.selected_path().to_path_buf();
            if path.is_file() {
                self.open(&path, window, cx);
                self.focus_tree(window, cx);
            } else {
                self.status = Some(keymap_actions::NOTHING_TO_SHOW_SOURCE_OF.into());
            }
            cx.notify();
            return true;
        }
        let Some(view) = self
            .find_tool_window()
            .filter(|v| v.read(cx).focus().contains_focused(window, cx))
        else {
            return false;
        };
        match view.read(cx).selected_place() {
            Some(place) => {
                self.remember_place(cx);
                self.show_place(&place, window, cx);
                window.focus(&view.read(cx).focus());
            }
            None => self.status = Some(keymap_actions::NOTHING_TO_SHOW_SOURCE_OF.into()),
        }
        cx.notify();
        true
    }

    /// `ShowContent`, Show List of Tabs: the tabs of the focused tool window in a list, the
    /// chosen one is selected (`SPEC-0167` v7 behaviour 19). False unless the focused tool
    /// window has more than one tab.
    pub(super) fn show_tool_window_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(which) = self.focused_tool_window(window, cx) else {
            return false;
        };
        let (titles, selected) = match which {
            ToolWindow::Terminal => (self.terminal_tabs(), self.terminal_tab),
            ToolWindow::Run => match &self.run_view {
                Some(run) => {
                    let run = run.read(cx);
                    let titles: Vec<String> = run.tabs().into_iter().map(|t| t.0).collect();
                    let selected = run
                        .selected_title()
                        .and_then(|t| titles.iter().position(|x| *x == t))
                        .unwrap_or(0);
                    (titles, selected)
                }
                None => return false,
            },
            _ => return false,
        };
        if titles.len() < 2 {
            return false;
        }
        let names = titles.clone();
        self.choose_from(
            keymap_actions::tab_list_title(which.title()),
            titles,
            selected,
            window,
            cx,
            move |this, i, window, cx| match which {
                ToolWindow::Terminal => this.select_terminal_tab(i, window, cx),
                _ => {
                    if let (Some(run), Some(name)) = (this.run_view.clone(), names.get(i)) {
                        run.update(cx, |r, cx| r.select_title(name, cx));
                    }
                }
            },
        );
        true
    }

    /// `ShowFilterPopup`: the filter menu of the open find bar; false without a bar
    /// (`SPEC-0065` v6 behaviour 20).
    pub(super) fn show_find_filter(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(bar) = self.find.as_mut() else {
            return false;
        };
        bar.menu = Some(crate::find_bar::FindMenu::Filter);
        cx.notify();
        true
    }

    /// Whether the find bar shows its filter menu.
    pub fn find_filter_shown(&self) -> bool {
        self.find
            .as_ref()
            .is_some_and(|b| b.menu == Some(crate::find_bar::FindMenu::Filter))
    }

    /// `NewScratchFile`: a list of file kinds; the chosen one gets an empty file in the user's
    /// scratches folder, opened in the editor (`SPEC-0059` v9.6 behaviour 28).
    pub(super) fn new_scratch_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_project.is_some() {
            self.status = Some("Scratch files are not available in a remote project yet".into());
            cx.notify();
            return;
        }
        let Some(dir) = idecraft_core::user_config_dir().map(|d| d.join("scratches")) else {
            self.status = Some("No settings folder to keep scratch files in".into());
            cx.notify();
            return;
        };
        let kinds = keymap_actions::SCRATCH_KINDS;
        let selected = keymap_actions::scratch_kind_for(self.active_path().as_deref());
        self.choose_from(
            "New Scratch File".into(),
            kinds.iter().map(|(label, _)| (*label).to_owned()).collect(),
            selected,
            window,
            cx,
            move |this, i, window, cx| {
                if let Some((_, extension)) = kinds.get(i) {
                    this.create_scratch_in(&dir, extension, window, cx);
                }
            },
        );
    }

    /// Creates the next scratch file with `extension` in `dir` and opens it; the status bar
    /// names it, or says why it could not be created.
    pub fn create_scratch_in(
        &mut self,
        dir: &std::path::Path,
        extension: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<std::path::PathBuf> {
        let made = keymap_actions::create_scratch(dir, extension);
        let path = match made {
            Ok(path) => path,
            Err(e) => {
                self.status = Some(format!("Could not create a scratch file: {e}"));
                cx.notify();
                return None;
            }
        };
        self.tree = self.tree.reloaded();
        self.open(&path, window, cx);
        self.focus_active(window, cx);
        self.status = Some(format!("Created scratch file {}", path.display()));
        cx.notify();
        Some(path)
    }

    /// `NextDiff` and `PreviousDiff`: the next or previous difference of the open viewer; false
    /// without one (`SPEC-0084` v4 behaviour 14). At the edge of a file it may go on to another
    /// file (v5 behaviour 21).
    pub(super) fn step_diff(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(diff) = self.diff.clone() else {
            return false;
        };
        let moved = diff.update(cx, |d, cx| {
            cx.notify();
            d.step(forward)
        });
        self.diff_stepped(forward, moved, window, cx);
        true
    }

    /// After a step in the viewer: a step that moved forgets an earlier press at the edge; one
    /// that could not move asks for a second press or shows the next (previous) file of the
    /// list (`SPEC-0084` v5 behaviour 21).
    pub(super) fn diff_stepped(
        &mut self,
        forward: bool,
        moved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if moved {
            self.diff_asked = None;
            return;
        }
        let neighbour = self.diff_neighbour(forward, cx);
        match diff_actions::edge(self.diff_asked, forward, neighbour.is_some()) {
            diff_actions::Edge::Stop => self.diff_asked = None,
            diff_actions::Edge::Ask => {
                self.diff_asked = Some(forward);
                self.status = neighbour.map(|file| diff_actions::press_again(forward, &file));
            }
            diff_actions::Edge::Go => {
                if let Some(file) = neighbour {
                    self.show_diff(&file, window, cx);
                    // Backwards the file is shown at its last difference.
                    if let Some(diff) = self.diff.clone().filter(|_| !forward) {
                        diff.update(cx, |d, _| d.go_to_last());
                    }
                }
            }
        }
        cx.notify();
    }

    /// The files the viewer of Show Diff goes through: those of the Commit tool window in the
    /// order of its list, or the changed files as Git reports them while that window was never
    /// opened (`SPEC-0084` v5 behaviour 20).
    fn diff_files(&self, cx: &gpui::App) -> Vec<String> {
        match &self.commit {
            Some(commit) => commit.read(cx).files(),
            None => Repo::discover(&self.root)
                .and_then(|r| r.status())
                .map(|s| {
                    s.into_iter()
                        .map(|f| f.path)
                        .filter(|p| !p.ends_with('/'))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The file after (before) the one the viewer shows, in its list; `None` at the end of the
    /// list and in a viewer without a list.
    fn diff_neighbour(&self, forward: bool, cx: &gpui::App) -> Option<String> {
        let (current, _) = self.diff_file.as_ref()?;
        diff_actions::neighbour(&self.diff_files(cx), current, forward).map(str::to_owned)
    }

    /// `Diff.NextChange` and `Diff.PrevChange`: the diff of the next (previous) file of the
    /// viewer's list, without wrapping (`SPEC-0084` v5 behaviour 20). False without a viewer.
    pub(super) fn compare_neighbour_file(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.diff.is_none() {
            return false;
        }
        self.diff_asked = None;
        if self.diff_file.is_none() {
            self.status = Some(diff_actions::NO_FILE_LIST.into());
        } else {
            match self.diff_neighbour(forward, cx) {
                Some(file) => self.show_diff(&file, window, cx),
                None => self.status = Some(diff_actions::no_further_file(forward).into()),
            }
        }
        cx.notify();
        true
    }

    /// `Diff.FocusOppositePane`: the other pane of the side-by-side view becomes the current
    /// one (`SPEC-0084` v5 behaviour 19). False without a viewer.
    pub(super) fn focus_opposite_diff_pane(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(diff) = self.diff.clone() else {
            return false;
        };
        let switched = diff.update(cx, |d, cx| {
            cx.notify();
            d.focus_opposite_pane()
        });
        self.status = Some(match switched {
            Some((pane, line)) => {
                let view = diff.read(cx);
                let (left, right) = view.side_titles();
                let title = match pane {
                    Pane::Left => left,
                    Pane::Right => right,
                };
                diff_actions::pane_status(pane, title, line)
            }
            None => diff_actions::ONE_PANE.into(),
        });
        cx.notify();
        true
    }

    /// `Diff.ShowSettingsPopup`: the settings of the viewer as a popup; a chosen view is shown
    /// (`SPEC-0084` v5 behaviour 22). False without a viewer.
    pub(super) fn show_diff_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(diff) = self.diff.clone() else {
            return false;
        };
        let rows = diff_actions::settings_rows(diff.read(cx).unified());
        let on_choose: super::revisions::OnChoose = std::rc::Rc::new(|this, i, _, cx| {
            let (Some(unified), Some(diff)) = (diff_actions::setting_of_row(i), this.diff.clone())
            else {
                return;
            };
            this.diff_unified = unified;
            this.diff_asked = None;
            diff.update(cx, |d, cx| {
                if d.unified() != unified {
                    d.set_unified(unified);
                }
                cx.notify();
            });
        });
        self.revision_popup(
            diff_actions::SETTINGS_TITLE.into(),
            rows,
            on_choose,
            window,
            cx,
        );
        true
    }

    /// The text a side of the viewer has now: the open editor's, else the file's on disk; fixed
    /// text as it is.
    fn diff_side_text(&self, side: &Side, cx: &gpui::App) -> Result<String, String> {
        match side {
            Side::Fixed(text) => Ok(text.clone()),
            Side::File(file) => match self.diff_side_editor(file) {
                Some(editor) => Ok(editor.read(cx).state.buffer.text()),
                None => {
                    std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))
                }
            },
        }
    }

    /// The open editor of a side's file: the main group's, else one of another group, so a file
    /// that is open anywhere is changed in its editor and not behind it on disk.
    fn diff_side_editor(&self, file: &std::path::Path) -> Option<gpui::Entity<crate::EditorView>> {
        let canonical = file.canonicalize().ok();
        let open_in = |editors: &std::collections::HashMap<
            std::path::PathBuf,
            gpui::Entity<crate::EditorView>,
        >| {
            canonical
                .as_ref()
                .and_then(|f| editors.get(f))
                .or_else(|| editors.get(file))
                .cloned()
        };
        open_in(&self.editors).or_else(|| self.groups.iter().find_map(|g| open_in(&g.editors)))
    }

    /// `Diff.ApplyLeftSide` (`from_left`) and `Diff.ApplyRightSide`: takes the lines of `change`
    /// — of the current difference without one — from one side into the other, and says in the
    /// status line what happened or why not (`SPEC-0084` v5 behaviour 18). False without a
    /// viewer; true otherwise, also when nothing was written.
    pub(super) fn take_diff_side(
        &mut self,
        from_left: bool,
        change: Option<idecraft_vcs::LineChange>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.diff.is_none() {
            return false;
        }
        self.diff_asked = None;
        self.status = Some(match self.take_in_diff(from_left, change, cx) {
            Ok(done) | Err(done) => done,
        });
        cx.notify();
        true
    }

    /// Takes a change over; `Ok` with what was done, `Err` with why nothing was written.
    fn take_in_diff(
        &mut self,
        from_left: bool,
        change: Option<idecraft_vcs::LineChange>,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let diff = self.diff.clone().ok_or_else(String::new)?;
        // The side that is written is the other one.
        let target_pane = if from_left { Pane::Right } else { Pane::Left };
        let target_title = {
            let view = diff.read(cx);
            let (left, right) = view.side_titles();
            match target_pane {
                Pane::Left => left.to_owned(),
                Pane::Right => right.to_owned(),
            }
        };
        let Some(sides) = self.diff_sides.clone() else {
            return Err(diff_actions::Refusal::Fixed.message(&target_title));
        };
        let target = sides.of(target_pane);
        let editor = match target {
            Side::File(file) => self.diff_side_editor(file),
            Side::Fixed(_) => None,
        };
        let facts = match target {
            Side::File(file) => diff_actions::TargetFacts::of(
                file,
                self.remote_project.is_some(),
                editor.as_ref().is_some_and(|e| e.read(cx).is_read_only()),
            ),
            Side::Fixed(_) => diff_actions::TargetFacts::default(),
        };
        if let Some(refusal) = diff_actions::refusal(target, &facts) {
            return Err(refusal.message(&target_title));
        }
        let Side::File(file) = target else {
            return Err(diff_actions::Refusal::Fixed.message(&target_title));
        };
        let change = change
            .or_else(|| diff.read(cx).current_change())
            .ok_or_else(|| diff_actions::NO_DIFFERENCE.to_owned())?;
        // Both sides are read again: what is written must be what the viewer shows.
        let left = self.diff_side_text(&sides.left, cx)?;
        let right = self.diff_side_text(&sides.right, cx)?;
        if idecraft_vcs::side_by_side(&left, &right) != diff.read(cx).rows() {
            diff.update(cx, |d, cx| {
                d.refresh(&left, &right);
                cx.notify();
            });
            return Err(diff_actions::SIDES_CHANGED.into());
        }
        let taken = idecraft_vcs::take_change(&left, &right, &change, from_left);
        let before = if from_left { &right } else { &left };
        if &taken == before {
            return Err(diff_actions::ALREADY_EQUAL.into());
        }
        match &editor {
            // One undo step in the open editor, nothing saved.
            Some(editor) => {
                editor.update(cx, |e, cx| e.replace_text(&taken, cx));
            }
            None => std::fs::write(file, &taken).map_err(|e| format!("{target_title}: {e}"))?,
        }
        let (left, right) = if from_left {
            (left, taken)
        } else {
            (taken, right)
        };
        diff.update(cx, |d, cx| {
            d.refresh(&left, &right);
            cx.notify();
        });
        Ok(format!(
            "Accepted the {} side: {target_title} changed{}",
            target_pane.other().name(),
            if editor.is_some() { ", not saved" } else { "" },
        ))
    }

    /// `CompareTwoFiles`: the two files selected in the Project tree side by side, read-only
    /// (`SPEC-0084` v4 behaviour 15).
    pub fn compare_two_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (left, right) = match keymap_actions::compare_pair(&self.tree.selected_paths()) {
            Ok(pair) => pair,
            Err(reason) => {
                self.status = Some(reason.into());
                cx.notify();
                return;
            }
        };
        self.compare_files(left, right, window, cx);
    }

    /// Shows the files `left` and `right` in the diff viewer as Compare Two Files does
    /// (`SPEC-0084` v4 behaviour 15); both sides are files and take changes of each other (v5
    /// behaviour 17).
    pub fn compare_files(
        &mut self,
        left: std::path::PathBuf,
        right: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let read = |path: &std::path::Path| {
            std::fs::read_to_string(path)
                .map_err(|_| format!("{} cannot be read as text", path.display()))
        };
        let (old, new) = match (read(&left), read(&right)) {
            (Ok(old), Ok(new)) => (old, new),
            (Err(e), _) | (_, Err(e)) => {
                self.status = Some(e);
                cx.notify();
                return;
            }
        };
        let (left_title, right_title) = keymap_actions::compare_titles(&self.root, &left, &right);
        let rows = idecraft_vcs::side_by_side(&old, &new);
        let view = cx.new(|cx| {
            DiffView::new(format!("{left_title} vs {right_title}"), rows, cx)
                .with_sides(&left_title, &right_title)
                .with_texts(&right, &old, &new)
        });
        self.present_diff(view, window, cx);
        // Both sides are files: each takes changes of the other (`SPEC-0084` v5 behaviour 17).
        self.diff_sides = Some(diff_actions::Sides {
            left: Side::File(left),
            right: Side::File(right),
        });
    }

    /// The Commit tool window's operation of `id` while the window is shown (`SPEC-0063` v9
    /// behaviour 21); false when the window is hidden or the operation has nothing to act on.
    pub(super) fn commit_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(op) = keymap_actions::commit_op(id) else {
            return false;
        };
        let Some(view) = self
            .commit
            .clone()
            .filter(|_| self.tool_window_shown(ToolWindow::Commit))
        else {
            return false;
        };
        view.update(cx, |v, cx| match op {
            CommitOp::ShowDiff => {
                let row = v.current().is_some();
                v.press("diff", cx);
                row
            }
            CommitOp::ToggleAmend => {
                if v.amend_target().is_some() {
                    let on = !v.amend();
                    v.set_amend(on, cx);
                }
                true
            }
            CommitOp::GroupByDirectory => {
                v.toggle_group_by(true, cx);
                true
            }
            CommitOp::GroupByModule => {
                v.toggle_group_by(false, cx);
                true
            }
            CommitOp::Options => {
                v.press("options", cx);
                true
            }
            CommitOp::MessageHistory => {
                v.show_message_history(window, cx);
                true
            }
            // Without something to act on the window says so (`SPEC-0335` behaviours 8–12).
            CommitOp::Changelist(op) => {
                v.changelist_action(op, window, cx);
                true
            }
        })
    }

    /// `Git.CreateNewBranch`: the branch popup with its field taking the new branch's name
    /// (`SPEC-0068` v9 behaviour 32). A remote project's popup opens with its list, where
    /// "New Branch…" is the first row.
    pub(super) fn new_branch_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branches.is_none() {
            self.open_branches(window, cx);
        }
        if let Some(popup) = self.branches.clone() {
            popup.update(cx, |b, cx| b.name_new_branch(cx));
        }
    }

    /// `Git.Rename.Local.Branch`: in the open popup the highlighted local branch, else the
    /// current branch in a popup opened for it (`SPEC-0068` v9 behaviour 33).
    pub(super) fn rename_branch_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popup) = self.branches.clone() {
            match popup.read(cx).highlighted_local() {
                Some(name) => popup.update(cx, |b, cx| b.rename_branch(name, cx)),
                None => self.status = Some("Select a local branch to rename it".into()),
            }
            cx.notify();
            return;
        }
        let current = Repo::discover(&self.root)
            .and_then(|r| r.branches())
            .ok()
            .and_then(|b| b.branches.into_iter().find(|b| b.current && !b.remote));
        let Some(current) = current else {
            self.status = Some("HEAD is not on a branch".into());
            cx.notify();
            return;
        };
        self.open_branches(window, cx);
        if let Some(popup) = self.branches.clone() {
            popup.update(cx, |b, cx| b.rename_branch(current.name, cx));
        }
    }

    /// `Git.Reword.Commit` and `Git.New.Branch.In.Log`: the commit menu's entry of the selected
    /// commit (`SPEC-0086` v11 behaviour 20); false without a shown log or a selected commit.
    pub(super) fn log_commit_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let (Some(entry), Some(log)) = (keymap_actions::log_entry(id), self.git_log.clone()) else {
            return false;
        };
        log.update(cx, |l, cx| l.run_commit_entry(entry, cx))
    }

    /// Notes the focused tool window for `JumpToLastWindow`; called with every frame.
    pub(super) fn observe_tool_window_focus(&mut self, window: &Window, cx: &gpui::App) {
        let focused = self.focused_tool_window(window, cx);
        self.last_tool_window.observe(focused);
    }

    /// `JumpToLastWindow`: the focus goes to the tool window that had it last, shown again when
    /// it was hidden (`SPEC-0167` v5 behaviour 16).
    pub fn jump_to_last_tool_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.last_tool_window.get() {
            None => self.status = Some(keymap_actions::NO_LAST_TOOL_WINDOW.into()),
            Some(w) => {
                if !(self.tool_window_shown(w) && self.tool_window_focused(w, window, cx)) {
                    self.toggle_tool_window(w, window, cx);
                }
            }
        }
        cx.notify();
    }

    /// `CloseActiveTab`: the selected tab of the focused Terminal or Run tool window; false
    /// elsewhere (`SPEC-0167` v5 behaviour 18).
    pub(super) fn close_active_tool_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.focused_tool_window(window, cx) {
            Some(ToolWindow::Terminal) => {
                let i = self.terminal_tab;
                self.close_terminal_tab(i, window, cx);
                true
            }
            Some(ToolWindow::Run) => match self.run_view.clone() {
                Some(run) => {
                    run.update(cx, |r, cx| r.close_selected_tab(cx));
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    /// `SelectIn`: the active tab's file selected in the Project tree, which is shown and takes
    /// the focus (`SPEC-0059` v9.3 behaviour 24).
    pub fn select_in_project_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_path() else {
            self.status = Some("No file is open to select".into());
            cx.notify();
            return;
        };
        if !self.reveal_in_tree(&path, cx) {
            self.status = Some(format!("{} is not in the Project tree", path.display()));
            cx.notify();
            return;
        }
        if !self.tool_window_shown(ToolWindow::Project) {
            self.toggle_tool_window(ToolWindow::Project, window, cx);
        }
        self.focus_tree(window, cx);
    }

    /// `Synchronize`: tree, open editors, file status and branch are read from disk at once;
    /// nothing is saved or discarded (`SPEC-0059` v9.3 behaviour 25).
    pub fn synchronize(&mut self, cx: &mut Context<Self>) {
        self.tree = self.tree.reloaded();
        for editor in self.editors.values() {
            editor.update(cx, |e, cx| e.check_disk(cx));
        }
        self.refresh_branch();
        if self.remote_project.is_none() {
            let keys = Repo::discover(&self.root).ok().and_then(|r| {
                let workdir = r.workdir()?.canonicalize().ok()?;
                Some(file_status_keys(&workdir, &r.status().ok()?))
            });
            if let Some(keys) = keys {
                self.file_status = keys;
            }
        }
        if let Some(commit) = &self.commit {
            commit.update(cx, |v, cx| {
                v.refresh();
                cx.notify();
            });
        }
        self.reload_git_log(cx);
        self.status = Some("Reloaded from disk".into());
        cx.notify();
    }

    /// `SaveAs`: the active editor's text written to a file chosen in the system's save dialog
    /// and that file opened in a tab; the original stays as it is (`SPEC-0059` v9.3 behaviour
    /// 26). False without a local file editor.
    pub(super) fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.remote_project.is_some() {
            return false;
        }
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let text = {
            let e = editor.read(cx);
            if e.is_remote_document() {
                return false;
            }
            e.state.buffer.text()
        };
        let (dir, name) = keymap_actions::save_as_start(&path, &self.root);
        let chosen = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(target))) = chosen.await else {
                return;
            };
            let written = std::fs::write(&target, text.as_bytes());
            let _ = this.update_in(cx, |this, window, cx| {
                match written {
                    Ok(()) => {
                        this.tree = this.tree.reloaded();
                        this.open(&target, window, cx);
                        this.status = Some(format!("Saved as {}", target.display()));
                    }
                    Err(e) => {
                        this.status = Some(format!("Could not save {}: {e}", target.display()))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        true
    }

    /// `QuickImplementations` (⌥Space, ⌘Y): the source of the implementations of the symbol at
    /// the caret, else of its definition, read-only in the documentation popup below the caret
    /// (`SPEC-0081` v4 behaviour 6).
    pub(super) fn quick_definition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(here) = self.here(cx) else {
            return;
        };
        let Some(language) = idecraft_languages::LanguageId::detect(&here.path) else {
            return;
        };
        if self.remote_ask(&here.path).is_some() {
            self.status = Some("Quick Definition is not available for a remote document".into());
            cx.notify();
            return;
        }
        let Some(editor) = self.active_editor() else {
            return;
        };
        let (text, offset, column) = {
            let e = editor.read(cx);
            // A caret right after a name asks about that name (SPEC-0071 v5 behaviour 7).
            let column = e.state.buffer.line(here.line).map_or(here.column, |line| {
                crate::nav_model::symbol_column(&line, here.column)
            });
            (e.state.buffer.text(), e.state.carets.primary().head, column)
        };
        // Open editors show their own text, unsaved changes included.
        let open: std::collections::HashMap<std::path::PathBuf, String> = self
            .editors
            .iter()
            .map(|(p, e)| (p.clone(), e.read(cx).state.buffer.text()))
            .collect();
        self.status = Some("Looking for the definition…".into());
        cx.notify();
        let root = self.root.clone();
        let from = here.clone();
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    let targets = rt
                        .block_on(idecraft_languages::quick_definition::targets(
                            &root,
                            &from.path,
                            Some(&text),
                            from.line as u64,
                            column as u64,
                            std::time::Duration::from_secs(30),
                        ))
                        .map_err(|e| match e {
                            idecraft_languages::lsp::LspError::NoServer(why) => {
                                idecraft_languages::servers::no_server_message(language, &why)
                            }
                            e => format!("The language server gave no answer: {e}"),
                        })?;
                    let shown: Vec<QuickTarget> = targets
                        .locations
                        .iter()
                        .filter_map(|l| {
                            let path = l.path.canonicalize().unwrap_or_else(|_| l.path.clone());
                            let text = match open.get(&path) {
                                Some(text) => text.clone(),
                                None => std::fs::read_to_string(&path).ok()?,
                            };
                            let source = idecraft_languages::quick_definition::source_at(
                                &path,
                                &text,
                                l.range.start_line as usize,
                            )?;
                            Some(QuickTarget {
                                shown_path: path
                                    .strip_prefix(&root)
                                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                                    .unwrap_or_else(|_| path.display().to_string()),
                                source,
                            })
                        })
                        .collect();
                    Ok::<_, String>((shown, targets.outside))
                })
                .await;
            let _ = this.update_in(cx, |this, _window, cx| {
                // The caret moved or another file is active: the answer is stale.
                if this.here(cx).as_ref() != Some(&here) {
                    return;
                }
                match found {
                    Err(e) => this.status = Some(e),
                    Ok((shown, outside)) if shown.is_empty() => {
                        this.status = Some(keymap_actions::quick_definition_nothing(&outside));
                    }
                    Ok((shown, _)) => {
                        this.status = None;
                        this.doc = Some(DocPopup {
                            path: here.path.clone(),
                            offset,
                            blocks: keymap_actions::quick_definition_blocks(&shown),
                            problem: None,
                            pointer: None,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the one file of `files`, offers several in a chooser titled `title`, or says
    /// `nothing` in the status line. The place left is recorded for Back; a chosen file opens
    /// where its caret was.
    pub(super) fn open_or_choose(
        &mut self,
        files: Vec<std::path::PathBuf>,
        title: String,
        nothing: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match files.as_slice() {
            [] => self.status = Some(nothing),
            [only] => {
                self.status = None;
                self.remember_place(cx);
                let only = only.clone();
                self.open(&only, window, cx);
            }
            _ => {
                self.status = None;
                let rows = keymap_actions::file_rows(&self.root, &files);
                let view = cx.new(|cx| UsagesView::titled(title, rows, cx));
                self.usages_at = self.caret_anchor(cx);
                self._usages_events = Some(cx.subscribe_in(
                    &view,
                    window,
                    |this, _, event: &UsagesEvent, window, cx| {
                        this.usages = None;
                        match event {
                            UsagesEvent::Open(place) => {
                                this.remember_place(cx);
                                let path = place.path.clone();
                                this.open(&path, window, cx);
                            }
                            UsagesEvent::Close => this.focus_active(window, cx),
                        }
                        cx.notify();
                    },
                ));
                window.focus(&view.read(cx).focus());
                self.usages = Some(view);
            }
        }
        cx.notify();
    }

    /// The active file when Go to Test and Go to Related can ask about it: a file of this
    /// project on this machine. Says why not in the status line.
    pub(super) fn local_project_file(
        &mut self,
        what: &str,
        cx: &mut Context<Self>,
    ) -> Option<(std::path::PathBuf, std::path::PathBuf, String)> {
        let path = self.active_path()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.remote_ask(&path).is_some() {
            self.status = Some(format!("{what} is not available for a remote document"));
            cx.notify();
            return None;
        }
        let Ok(relative) = path
            .strip_prefix(&self.root)
            .map(std::path::Path::to_path_buf)
        else {
            self.status = Some(format!("{name} is not a file of this project"));
            cx.notify();
            return None;
        };
        Some((path, relative, name))
    }

    /// `GotoTest`: from a source file to its test, from a test to the file it tests, by the file
    /// names among the project's own files (`SPEC-0071` v10 behaviour 15).
    pub(super) fn goto_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, relative, name)) = self.local_project_file("Go to Test", cx) else {
            return;
        };
        let Some(me) = idecraft_languages::related::test_name(&relative) else {
            self.status = Some(keymap_actions::goto_test_nothing(&name, None));
            cx.notify();
            return;
        };
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let walk_root = root.clone();
            let found = cx
                .background_spawn(async move {
                    let files = idecraft_core::OwnSources::new(&walk_root).files();
                    idecraft_languages::related::counterparts(&relative, &files).unwrap_or_default()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let files: Vec<std::path::PathBuf> = found.iter().map(|f| root.join(f)).collect();
                let title = keymap_actions::goto_test_title(&name, me.is_test, files.len());
                let nothing = keymap_actions::goto_test_nothing(&name, Some(me.is_test));
                this.open_or_choose(files, title, nothing, window, cx);
            });
        })
        .detach();
    }

    /// `GotoRelated` (⌃⌘↑): the stylesheets an HTML file links, the HTML files that link a
    /// stylesheet (`SPEC-0071` v10 behaviour 17).
    pub(super) fn goto_related(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, _, name)) = self.local_project_file("Go to Related", cx) else {
            return;
        };
        let text = self.active_editor().map(|e| e.read(cx).state.buffer.text());
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let ask_root = root.clone();
            let found = cx
                .background_spawn(async move {
                    idecraft_languages::related::related_files(&ask_root, &path, text.as_deref())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let known = found.is_some();
                let files: Vec<std::path::PathBuf> = found
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(f, _)| root.join(f))
                    .collect();
                let title = keymap_actions::related_title(&name, files.len());
                let nothing = keymap_actions::related_nothing(&name, known);
                this.open_or_choose(files, title, nothing, window, cx);
            });
        })
        .detach();
    }

    /// `RecentChangedFiles`: the Recent Files popup listing only the files changed in this
    /// session (`SPEC-0069` v3 behaviour 9).
    pub(super) fn open_recent_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_goto(true, window, cx);
        if let Some(popup) = self.goto.clone() {
            popup.update(cx, |v, cx| v.set_edited_only(true, cx));
        }
    }
}

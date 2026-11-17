//! The ninth package of workbench actions that exist under their keymap id
//! (`docs/research/KEYMAP_COVERAGE.md`): Safe Delete of a declaration and of files in the
//! Project tree (`SPEC-0095` v12 behaviours 15 and 16), Reformat File… with its options
//! (`SPEC-0075` v6 behaviour 8), Force Refresh (`SPEC-0059` v9.8 behaviour 30) and Header/Source
//! (`SPEC-0071` v12 behaviour 19). The decisions without a window are in
//! [`crate::keymap_actions`], [`idecraft_languages::safe_delete`] and
//! [`idecraft_languages::related`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Window};
use idecraft_languages::find_usages::{DEADLINE, UsageSource};
use idecraft_languages::insight::Range;
use idecraft_languages::safe_delete::SafeDeletePlan;

use super::WorkbenchView;
use crate::keymap_actions::{self, ReformatOption};

/// Where a usage is: the file, the 0-based line and character.
type UsagePlace = (PathBuf, usize, usize);

/// What Reformat File… computes in the background: the new text and the status line, or why
/// nothing was changed.
type ReformatOutcome = Result<(String, String), String>;

impl WorkbenchView {
    /// The actions of the ninth package by id; false for another id or where the action is not
    /// available (then the next action of the chord is tried).
    pub(super) fn run_ninth_package_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            "SafeDelete" => self.safe_delete(window, cx),
            "ShowReformatFileDialog" => self.show_reformat_file_dialog(window, cx),
            "ForceRefresh" => self.force_refresh(cx),
            "SwitchHeaderSource" => self.header_source(window, cx),
            _ => return false,
        }
        true
    }

    /// The text of `path` as the user sees it: the open editor's, else the disk's.
    fn shown_text(&self, path: &Path, cx: &gpui::App) -> Option<String> {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        match self.editors.get(&key).or_else(|| self.editors.get(path)) {
            Some(e) => Some(e.read(cx).state.buffer.text()),
            None => std::fs::read_to_string(path).ok(),
        }
    }

    /// `SafeDelete` (⌘⌦): in the Project tree the selected files (behaviour 16), else the
    /// declaration of the symbol at the caret after a search for its usages (behaviour 15).
    pub fn safe_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree_focus.is_focused(window) {
            self.safe_delete_files(window, cx);
            return;
        }
        let Some(path) = self.active_path() else {
            self.status = Some(
                "Safe Delete needs a declaration in the editor or a file in the Project tree"
                    .into(),
            );
            cx.notify();
            return;
        };
        if self.remote_project.is_some() || self.remote_ask(&path).is_some() {
            self.status = Some(keymap_actions::SAFE_DELETE_REMOTE.into());
            cx.notify();
            return;
        }
        let (Some(here), Some(editor)) = (self.here(cx), self.active_editor()) else {
            return;
        };
        if idecraft_languages::LanguageId::detect(&path).is_none() {
            self.status = Some(format!(
                "Safe Delete needs a language server: IDECraft knows no language for {}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            cx.notify();
            return;
        }
        let (subject, text) = {
            let e = editor.read(cx);
            let line = e.state.buffer.line(here.line).unwrap_or_default();
            (
                crate::usages_model::search_subject(
                    line.trim_end_matches(['\n', '\r']),
                    here.column,
                ),
                e.state.buffer.text(),
            )
        };
        let symbol = match subject {
            Ok(symbol) => symbol,
            Err(nothing) => {
                self.status = Some(nothing.to_owned());
                cx.notify();
                return;
            }
        };
        let open: HashMap<PathBuf, String> = self
            .editors
            .iter()
            .map(|(p, e)| (p.clone(), e.read(cx).state.buffer.text()))
            .collect();
        self.status = Some(keymap_actions::safe_delete_looking(&symbol));
        cx.notify();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let planned = cx
                .background_spawn(async move {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    rt.block_on(idecraft_languages::safe_delete::plan_in(
                        &root,
                        &here.path,
                        Some(&text),
                        here.line as u64,
                        here.column as u64,
                        &symbol,
                        &open,
                        DEADLINE,
                        None,
                    ))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match planned {
                    Ok(plan) => this.safe_delete_found(plan, window, cx),
                    Err(why) => this.status = Some(why),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Takes the answer of Safe Delete's search (behaviours 15c and 15d): without usages the
    /// declaration is deleted, with usages the list opens with Cancel selected. Public for
    /// checks without a language server.
    pub fn safe_delete_found(
        &mut self,
        plan: SafeDeletePlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if plan.usages.is_empty() {
            self.apply_safe_delete(&plan, cx);
            return;
        }
        let noun = match plan.source {
            UsageSource::Text => "text occurrence",
            UsageSource::LanguageServer => "usage",
        };
        let places: Vec<UsagePlace> = plan
            .usages
            .iter()
            .map(|u| {
                (
                    u.path.clone(),
                    u.range.start_line as usize,
                    u.range.start_character as usize,
                )
            })
            .collect();
        let title = keymap_actions::safe_delete_title(&plan.name, places.len(), noun);
        self.safe_delete_list(title, places, window, cx, move |this, _, cx| {
            this.apply_safe_delete(&plan, cx);
        });
    }

    /// The list of behaviour 15d: Cancel (selected), Delete anyway (`delete`), the usages, each
    /// opened when chosen.
    fn safe_delete_list(
        &mut self,
        title: String,
        places: Vec<UsagePlace>,
        window: &mut Window,
        cx: &mut Context<Self>,
        delete: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let rows: Vec<(PathBuf, usize, String)> = places
            .iter()
            .map(|(path, line, _)| {
                let text = self
                    .shown_text(path, cx)
                    .and_then(|t| t.lines().nth(*line).map(str::to_owned))
                    .unwrap_or_default();
                (path.clone(), *line, text)
            })
            .collect();
        let items = keymap_actions::safe_delete_rows(&self.root, &rows);
        self.choose_from(
            title,
            items,
            0,
            window,
            cx,
            move |this, i, window, cx| match i {
                0 => {}
                1 => delete(this, window, cx),
                n => {
                    if let Some((path, line, column)) = places.get(n - 2).cloned() {
                        this.open_at_line(&path, line, window, cx);
                        if let Some(e) = this.active_editor() {
                            e.update(cx, |e, cx| e.go_to(line, column, cx));
                        }
                    }
                }
            },
        );
    }

    /// Deletes the declaration of `plan` if its file still holds the text the search saw
    /// (behaviours 15c and 15e): through the editor as one undo step, else on disk with a Local
    /// History revision before and after. Whether it was deleted.
    pub fn apply_safe_delete(&mut self, plan: &SafeDeletePlan, cx: &mut Context<Self>) -> bool {
        let left = plan.usages.len();
        let key = plan
            .path
            .canonicalize()
            .unwrap_or_else(|_| plan.path.clone());
        let open = self
            .editors
            .get(&key)
            .or_else(|| self.editors.get(&plan.path))
            .cloned();
        let done = match open {
            Some(editor) => {
                let now = editor.read(cx).state.buffer.text();
                if now != plan.text {
                    None
                } else {
                    let edit = idecraft_languages::jvm::TextEdit {
                        range: plan.range.clone(),
                        text: String::new(),
                    };
                    super::apply_byte_edits(&editor, &now, &[edit], true, cx);
                    Some(keymap_actions::safe_deleted(
                        &plan.kind, &plan.name, None, left,
                    ))
                }
            }
            None => self.safe_delete_on_disk(plan, &key).map(|relative| {
                keymap_actions::safe_deleted(&plan.kind, &plan.name, Some(&relative), left)
            }),
        };
        let deleted = done.is_some();
        self.status = Some(done.unwrap_or_else(|| keymap_actions::SAFE_DELETE_CHANGED.into()));
        cx.notify();
        deleted
    }

    /// Writes `plan` into the closed file `path` if it still holds the planned text, with Local
    /// History before and after (`SPEC-0164` v3 behaviour 13); the file's name below the
    /// project folder, or `None` when it changed or could not be written.
    fn safe_delete_on_disk(&self, plan: &SafeDeletePlan, path: &Path) -> Option<String> {
        if std::fs::read_to_string(path).ok().as_deref() != Some(plan.text.as_str()) {
            return None;
        }
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let relative = path
            .strip_prefix(&root)
            .or_else(|_| path.strip_prefix(&self.root))
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let mut history = idecraft_core::LocalHistory::open(&self.root).ok();
        if let Some(h) = history.as_mut() {
            let _ = h.record(&relative);
        }
        std::fs::write(path, plan.new_text()).ok()?;
        if let Some(h) = history.as_mut() {
            let _ = h.record(&relative);
        }
        Some(relative)
    }

    /// Safe Delete in the Project tree (behaviour 16): the mentions of the selected files'
    /// names first; without any the tree's Delete question, with some the list of behaviour
    /// 15d.
    fn safe_delete_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree.selection_fixed() {
            return;
        }
        let paths = self.tree.selected_paths();
        let files: Vec<PathBuf> = paths.iter().filter(|p| p.is_file()).cloned().collect();
        if self.remote_project.is_some() || files.is_empty() {
            self.ask_delete(&paths, window, cx);
            return;
        }
        let name = match paths.as_slice() {
            [one] => one
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            many => format!("{} items", many.len()),
        };
        self.status = Some(format!("Safe Delete: looking for mentions of {name}…"));
        cx.notify();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let mut all = Vec::new();
                    for f in &files {
                        all.extend(idecraft_languages::safe_delete::file_mentions(&root, f));
                    }
                    all.truncate(idecraft_languages::safe_delete::MAX_MENTIONS);
                    all
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.status = None;
                if found.is_empty() {
                    this.ask_delete(&paths, window, cx);
                    return;
                }
                let places: Vec<UsagePlace> = found
                    .iter()
                    .map(|m| (m.path.clone(), m.line, m.character))
                    .collect();
                let title = keymap_actions::safe_delete_title(&name, places.len(), "mention");
                this.safe_delete_list(title, places, window, cx, move |this, window, cx| {
                    match idecraft_core::file_ops::delete_plans(&this.root, &paths) {
                        Ok(plans) => {
                            this.delete_confirm = Some(plans);
                            this.finish_delete(true, window, cx);
                        }
                        Err(e) => this.status = Some(e.to_string()),
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// `ShowReformatFileDialog` (⌥⇧⌘L): the options of Reformat File… for the active file
    /// (`SPEC-0075` v6 behaviour 8); Enter runs [`Self::reformat_file_with`] with the checked
    /// ones.
    pub fn show_reformat_file_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            self.status = Some("Reformat File needs a file open in the editor".into());
            cx.notify();
            return;
        };
        let (selection, versioned) = {
            let e = editor.read(cx);
            (!e.state.carets.primary().range().is_empty(), e.has_base())
        };
        let options = keymap_actions::reformat_file_rows(selection, versioned);
        let (changed, imports) = self.reformat_choice;
        let checked: Vec<bool> = options
            .iter()
            .map(|o| match o {
                ReformatOption::Selection => true,
                ReformatOption::ChangedLines => changed,
                ReformatOption::OptimizeImports => imports,
            })
            .collect();
        let items: Vec<String> = options.iter().map(|o| o.label().to_owned()).collect();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let before = window.focused(cx);
        let view = cx.new(|cx| {
            crate::ChoiceView::new(keymap_actions::reformat_file_title(&name), items, cx)
                .with_checks(checked, Vec::new())
                .with_none_allowed()
                .with_selected(0)
        });
        self._compare_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.compare_targets = None;
                match &before {
                    Some(focus) => window.focus(focus),
                    None => this.focus_active(window, cx),
                }
                if let crate::ChoiceEvent::ChosenMany(rows) = event {
                    let chosen: Vec<ReformatOption> = rows
                        .iter()
                        .filter_map(|r| options.get(*r).copied())
                        .collect();
                    this.reformat_file_with(&chosen, window, cx);
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

    /// Reformat File… with the `chosen` options (behaviour 8): formatting and optimizing are
    /// computed on the editor's text in the background and applied as one undo step.
    pub fn reformat_file_with(
        &mut self,
        chosen: &[ReformatOption],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use idecraft_languages::format::{FormatOptions, OwnFormatter, no_formatter};
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return;
        };
        let selection = chosen.contains(&ReformatOption::Selection);
        let changed = chosen.contains(&ReformatOption::ChangedLines);
        let imports = chosen.contains(&ReformatOption::OptimizeImports);
        self.reformat_choice = (changed, imports);
        let (text, ranges, options) = {
            let e = editor.read(cx);
            let buffer = &e.state.buffer;
            let picked = e.state.carets.primary().range();
            let at = |offset: usize| buffer.offset_to_position(offset).unwrap_or_default();
            let selected = (selection && !picked.is_empty()).then(|| {
                let (a, b) = (at(picked.start), at(picked.end));
                (a, b)
            });
            let line_end = |line: usize| {
                buffer
                    .line(line)
                    .map(|l| l.trim_end_matches(['\n', '\r']).chars().count())
                    .unwrap_or(0) as u64
            };
            let ranges: Option<Vec<Range>> = if changed {
                let within = selected.map(|(a, b)| (a.line, b.line));
                let runs =
                    keymap_actions::changed_ranges(&e.changes_now().unwrap_or_default(), within);
                Some(
                    runs.into_iter()
                        .map(|(first, last)| Range {
                            start_line: first as u64,
                            start_character: 0,
                            end_line: last as u64,
                            end_character: line_end(last),
                        })
                        .collect(),
                )
            } else {
                selected.map(|(a, b)| {
                    vec![Range {
                        start_line: a.line as u64,
                        start_character: a.column as u64,
                        end_line: b.line as u64,
                        end_character: b.column as u64,
                    }]
                })
            };
            let indent = &e.state.indent;
            let options = FormatOptions {
                tab_size: if indent == "\t" {
                    e.tab_width() as u32
                } else {
                    indent.chars().count().max(1) as u32
                },
                insert_spaces: indent != "\t",
                trim_trailing_whitespace: true,
                insert_final_newline: true,
            }
            .with_editorconfig(&idecraft_core::editorconfig::editorconfig_for(&path));
            (buffer.text(), ranges, options)
        };
        let prettier = (ranges.is_none()
            && idecraft_core::trust::require_trusted(&self.root).is_ok())
        .then(|| idecraft_languages::js_tools::find_prettier(&self.root, &path))
        .flatten();
        let python = idecraft_languages::LanguageId::detect(&path)
            == Some(idecraft_languages::LanguageId::Python);
        // Refusals before anything runs (behaviour 8, behaviours 6 and 7); Prettier and Python
        // go their own way first, as with `ReformatCode`.
        if ranges.as_ref().is_none_or(|r| !r.is_empty()) && prettier.is_none() && !python {
            let refused = match (&ranges, OwnFormatter::of(&path)) {
                (Some(_), Some(own)) => Some(own.whole_files_only()),
                (_, Some(_)) => None,
                (_, None) => no_formatter(&path),
            };
            if let Some(why) = refused {
                self.status = Some(why);
                cx.notify();
                return;
            }
        }
        self.status = Some("Reformatting…".into());
        cx.notify();
        let root = self.root.clone();
        let (file, before) = (path.clone(), text.clone());
        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    reformat_job(&root, &file, before, ranges, imports, options, prettier)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let now = editor.read(cx).state.buffer.text();
                this.status = Some(match outcome {
                    Err(why) => why,
                    Ok(_) if now != text => {
                        "The file changed meanwhile; nothing was applied".into()
                    }
                    Ok((new, line)) => {
                        // One undo step; carets stay on their text.
                        editor.update(cx, |e, cx| e.apply_reformat(&new, cx));
                        line
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// `ForceRefresh` (⌥⇧⌘R, `SPEC-0059` v9.8 behaviour 30): Reload All from Disk, then every
    /// open file compared with the disk whatever its time stamp, the HEAD texts of the change
    /// markers and the run configurations read again.
    pub fn force_refresh(&mut self, cx: &mut Context<Self>) {
        self.synchronize(cx);
        if self.remote_project.is_some() {
            self.status = Some(
                "Force Refresh: the host's files were read again; their Git state comes through the session"
                    .into(),
            );
            cx.notify();
            return;
        }
        let editors: Vec<(PathBuf, gpui::Entity<crate::EditorView>)> = self
            .editors
            .iter()
            .map(|(p, e)| (p.clone(), e.clone()))
            .collect();
        for (_, editor) in &editors {
            editor.update(cx, |e, cx| e.check_disk_now(cx));
        }
        let paths: Vec<PathBuf> = editors.into_iter().map(|(p, _)| p).collect();
        let n = paths.len();
        self.load_bases(paths, cx);
        self.reload_run_configs();
        self.status = Some(format!(
            "Force Refresh: {n} open file{} compared with the disk, Git state and run configurations read again",
            if n == 1 { "" } else { "s" }
        ));
        cx.notify();
    }

    /// `SwitchHeaderSource` (F10): from a C or C++ header to its source and back, by file
    /// names among the project's own files (`SPEC-0071` v12 behaviour 19).
    pub fn header_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, relative, name)) = self.local_project_file("Header/Source", cx) else {
            return;
        };
        let Some(is_header) = idecraft_languages::related::is_header(&relative) else {
            self.status = Some(keymap_actions::header_source_nothing(&name, None));
            cx.notify();
            return;
        };
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let walk_root = root.clone();
            let found = cx
                .background_spawn(async move {
                    let files = idecraft_core::OwnSources::new(&walk_root).files();
                    idecraft_languages::related::header_source(&relative, &files)
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let files: Vec<PathBuf> = found.iter().map(|f| root.join(f)).collect();
                let title = keymap_actions::header_source_title(&name, is_header, files.len());
                let nothing = keymap_actions::header_source_nothing(&name, Some(is_header));
                this.open_or_choose(files, title, nothing, window, cx);
            });
        })
        .detach();
    }
}

/// The work of Reformat File… on `text` (behaviour 8): the formatting of the whole file, the
/// selection or the runs of changed lines (the last first), then Optimize Imports.
fn reformat_job(
    root: &Path,
    file: &Path,
    text: String,
    ranges: Option<Vec<Range>>,
    imports: bool,
    options: idecraft_languages::format::FormatOptions,
    prettier: Option<idecraft_languages::js_tools::JsTool>,
) -> ReformatOutcome {
    use idecraft_languages::format::OwnFormatter;
    use idecraft_languages::python_tools::{self, ToolKind, ToolSearch};
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let python = idecraft_languages::LanguageId::detect(file)
        == Some(idecraft_languages::LanguageId::Python);
    let python_format = |now: &str, range: Option<&Range>| -> Result<String, String> {
        let search = ToolSearch::for_workspace(root);
        let tool = python_tools::find_formatter(&search)
            .ok_or_else(|| python_tools::missing(&search, &[ToolKind::Ruff, ToolKind::Black]))?;
        python_tools::format(&tool, file, now, range).map_err(|e| format!("Reformat failed: {e}"))
    };
    let server_format = |now: &str, range: Option<&Range>| -> Result<String, String> {
        let plan = rt
            .block_on(idecraft_languages::format::plan_format_in(
                root,
                file,
                Some(now),
                range,
                options,
                std::time::Duration::from_secs(30),
            ))
            .map_err(|e| format!("Reformat failed: {e}"))?;
        idecraft_languages::rename::apply_edits(now, &plan.edits.edits)
            .map_err(|e| format!("Reformat failed: {e}"))
    };
    let mut now = text.clone();
    match &ranges {
        Some(runs) => {
            for range in runs.iter().rev() {
                now = if python {
                    python_format(&now, Some(range))?
                } else {
                    server_format(&now, Some(range))?
                };
            }
        }
        None => {
            now = if let Some(tool) = &prettier {
                idecraft_languages::js_tools::prettier_format(tool, file, &now)?
            } else if python {
                python_format(&now, None)?
            } else if let Some(own) = OwnFormatter::of(file) {
                let formatted = match own {
                    OwnFormatter::Sql => {
                        idecraft_database::sql_format::format_sql(&now, options.tab_size as usize)
                    }
                    OwnFormatter::Markdown => {
                        idecraft_languages::markdown_format::format_markdown(&now)
                    }
                };
                formatted.map_err(|why| {
                    format!(
                        "Reformat failed: IDECraft's {} formatter changed nothing: {why}",
                        own.language()
                    )
                })?
            } else {
                server_format(&now, None)?
            };
        }
    }
    let format_line = keymap_actions::reformat_file_line(ranges.map(|r| r.len()), now != text);
    if !imports {
        return Ok((now, format_line));
    }
    let imports_line = if python {
        let search = ToolSearch::for_workspace(root);
        let ruff_waits = python_tools::ruff_waits_for_trust(&search);
        let done = match python_tools::find_ruff(&search) {
            Some(ruff) => python_tools::ruff_imports(&ruff, file, &now)?,
            None => rt.block_on(python_tools::language_server_imports_in(
                root,
                file,
                &now,
                super::java_actions::WAIT,
            ))?,
        };
        let changed = done.text != now;
        now = done.text.clone();
        python_tools::summary(&done, changed, ruff_waits)
    } else if idecraft_languages::js_imports::applies(file) {
        let done = rt.block_on(idecraft_languages::js_imports::organize_imports_in(
            root,
            file,
            &now,
            super::java_actions::WAIT,
        ))?;
        let changed = done.text != now;
        now = done.text.clone();
        idecraft_languages::js_imports::summary(&done, changed)
    } else if let Some(lang) = idecraft_languages::jvm::Jvm::of(&file.to_string_lossy()) {
        match idecraft_languages::jvm::optimize_imports(lang, &now)? {
            None => "Imports are already optimized".to_owned(),
            Some(plan) => {
                now.replace_range(plan.range.clone(), &plan.replacement);
                match plan.removed.len() {
                    0 => "Imports sorted".to_owned(),
                    1 => "Removed 1 import".to_owned(),
                    n => format!("Removed {n} imports"),
                }
            }
        }
    } else {
        "imports not optimized: Optimize Imports works in Java, Kotlin, Python, JavaScript, TypeScript, Vue and Svelte files".to_owned()
    };
    Ok((
        now,
        format!("{}; {imports_line}", format_line.trim_end_matches('.')),
    ))
}

//! The file type actions in the window (`SPEC-0004` v5 behaviours 12, 13): Override File Type,
//! Revert File Type Override and Associate with File Type on the Project tree's selection or the
//! active file. What a file's language is and where the associations are kept is in
//! [`idecraft_languages::file_types`].

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use idecraft_languages::LanguageId;
use idecraft_languages::file_types::{self as ft, Choice};

use super::WorkbenchView;
use crate::context_menu::{self as model, FileTypeTarget};

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// What the status bar says after a choice for `files` (behaviour 13).
fn treated_as(files: &[PathBuf], choice: Choice) -> String {
    match files {
        [one] => format!("{} is treated as {}", name_of(one), choice.title()),
        _ => format!("{} files are treated as {}", files.len(), choice.title()),
    }
}

/// Says that a remote project keeps no file types (behaviour 13).
const REMOTE: &str = "File types are kept on the local machine only; nothing was changed";
/// Says that no file is chosen.
const NO_FILE: &str = "No file is selected";

impl WorkbenchView {
    /// Carries out a file type action on the tree's selection or the active file; false when
    /// `id` is none of them.
    pub(super) fn file_type_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(
            id,
            model::OVERRIDE_FILE_TYPE | model::REVERT_FILE_TYPE | model::ASSOCIATE_FILE_TYPE
        ) {
            return false;
        }
        let files = self.file_type_targets(window);
        self.file_type_action_on(id, files, window, cx);
        true
    }

    /// The file type actions that do not apply to the files they would act on now (behaviour
    /// 16): Override while every target has an override, Revert while none has one; both
    /// without a target file.
    pub(super) fn inapplicable_file_type_actions(&self, window: &Window) -> Vec<String> {
        let files = self.file_type_targets(window);
        let overridden: Vec<bool> = files
            .iter()
            .map(|p| self.file_type_target_of(p).overridden)
            .collect();
        let mut hidden = Vec::new();
        if overridden.iter().all(|o| *o) {
            hidden.push(model::OVERRIDE_FILE_TYPE.to_owned());
        }
        if !overridden.iter().any(|o| *o) {
            hidden.push(model::REVERT_FILE_TYPE.to_owned());
        }
        hidden
    }

    /// The files an action acts on: the Project tree's selected files while the tree has the
    /// focus, else the active editor's file; folders are left out.
    fn file_type_targets(&self, window: &Window) -> Vec<PathBuf> {
        let files = if self.tree_focus.is_focused(window) {
            self.tree.selected_paths()
        } else {
            self.active_path().into_iter().collect()
        };
        files.into_iter().filter(|p| !p.is_dir()).collect()
    }

    /// Carries out the file type action `id` on `files` (from the tree's menu, its selection).
    pub(super) fn file_type_action_on(
        &mut self,
        id: &str,
        files: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files: Vec<PathBuf> = files.into_iter().filter(|p| !p.is_dir()).collect();
        if self.remote_project.is_some() {
            return self.tell(REMOTE, cx);
        }
        if files.is_empty() {
            return self.tell(NO_FILE, cx);
        }
        match id {
            model::OVERRIDE_FILE_TYPE => {
                let choices = Choice::all();
                self.file_type_list(
                    "Override File Type",
                    &choices.clone(),
                    window,
                    cx,
                    move |this, i, cx| {
                        if let Some(c) = choices.get(i) {
                            this.override_file_type(&files, *c, cx);
                        }
                    },
                );
            }
            model::REVERT_FILE_TYPE => self.revert_file_type(&files, cx),
            model::ASSOCIATE_FILE_TYPE => {
                let file = files[0].clone();
                if let Some(l) = LanguageId::detect(&file) {
                    return self.tell(format!("{} already is {}", name_of(&file), l.title()), cx);
                }
                let choices: Vec<Choice> = Choice::all()
                    .into_iter()
                    .filter(|c| *c != Choice::PlainText)
                    .collect();
                let pattern = ft::pattern_of_name(&name_of(&file));
                let title = format!("Associate {pattern} with File Type");
                self.file_type_list(&title, &choices.clone(), window, cx, move |this, i, cx| {
                    if let Some(c) = choices.get(i) {
                        this.associate_file_type(&pattern, *c, cx);
                    }
                });
            }
            _ => {}
        }
    }

    /// The list of file types, each with its icon: typed text narrows it (the exact match
    /// selected, behaviour 17), Enter chooses, Escape closes.
    fn file_type_list(
        &mut self,
        title: &str,
        choices: &[Choice],
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl Fn(&mut Self, usize, &mut Context<Self>) + 'static,
    ) {
        let items = choices.iter().map(|c| c.title().to_owned()).collect();
        let icons = choices
            .iter()
            .map(|c| crate::file_type_icons::choice_icon(*c))
            .collect();
        self.show_jvm_popup(
            title.to_owned(),
            items,
            window,
            cx,
            move |this, i, _, cx| chosen(this, i, cx),
        );
        if let Some(popup) = &self.jvm_popup {
            popup.update(cx, |p, _| {
                p.enable_filter();
                p.set_icons(icons);
            });
        }
    }

    /// The project folder as the associations name it.
    fn file_types_root(&self) -> PathBuf {
        self.root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone())
    }

    /// `path` as the overrides name it: relative to the project folder.
    fn file_type_key(&self, path: &Path) -> Option<String> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        ft::relative_key(&self.file_types_root(), &canonical)
            .or_else(|| ft::relative_key(&self.root, path))
    }

    /// What the file type items of `path` show (behaviour 13).
    pub(super) fn file_type_target_of(&self, path: &Path) -> FileTypeTarget {
        let overridden = self.file_type_key(path).is_some_and(|k| {
            ft::associations(&self.file_types_root())
                .overrides
                .contains_key(&k)
        });
        FileTypeTarget {
            known: LanguageId::detect(path).is_some(),
            overridden,
        }
    }

    /// Saves `choice` as the override of every file of `files` and shows it in their editors.
    pub fn override_file_type(
        &mut self,
        files: &[PathBuf],
        choice: Choice,
        cx: &mut Context<Self>,
    ) {
        let keys: Vec<String> = files.iter().filter_map(|p| self.file_type_key(p)).collect();
        if keys.is_empty() {
            return self.tell(
                "The file is not inside the project; nothing was changed",
                cx,
            );
        }
        let result = ft::update(&self.file_types_root(), |a| {
            for k in &keys {
                a.overrides.insert(k.clone(), choice);
            }
        });
        match result {
            Ok(_) => {
                self.redetect_open_editors(cx);
                self.tell(treated_as(files, choice), cx);
            }
            Err(e) => self.tell(format!("{} is not written: {e}", ft::FILE), cx),
        }
    }

    /// Takes the overrides of `files` away.
    pub fn revert_file_type(&mut self, files: &[PathBuf], cx: &mut Context<Self>) {
        let keys: Vec<String> = files.iter().filter_map(|p| self.file_type_key(p)).collect();
        let result = ft::update(&self.file_types_root(), |a| {
            for k in &keys {
                a.overrides.remove(k);
            }
        });
        match result {
            Ok((before, after)) => {
                let removed = before.overrides.len() - after.overrides.len();
                self.redetect_open_editors(cx);
                let text = match (files, removed) {
                    ([one], 0) => format!("{} has no file type override", name_of(one)),
                    ([one], _) => format!("{} is detected from its name again", name_of(one)),
                    (_, 0) => "The files have no file type override".to_owned(),
                    (_, n) => format!("{n} files are detected from their names again"),
                };
                self.tell(text, cx);
            }
            Err(e) => self.tell(format!("{} is not written: {e}", ft::FILE), cx),
        }
    }

    /// Saves `pattern` with `choice` as a file name pattern of the project.
    pub fn associate_file_type(&mut self, pattern: &str, choice: Choice, cx: &mut Context<Self>) {
        let result = ft::update(&self.file_types_root(), |a| {
            a.patterns.insert(pattern.to_owned(), choice);
        });
        match result {
            Ok(_) => {
                self.redetect_open_editors(cx);
                let what = if pattern.contains('*') {
                    format!("{pattern} files are")
                } else {
                    format!("{pattern} is")
                };
                self.tell(format!("{what} treated as {}", choice.title()), cx);
            }
            Err(e) => self.tell(format!("{} is not written: {e}", ft::FILE), cx),
        }
    }

    /// Every open editor follows its file's language now (behaviour 12).
    fn redetect_open_editors(&mut self, cx: &mut Context<Self>) {
        // The tree's and the tabs' icons follow at once (behaviour 15).
        self.file_type_icons.borrow_mut().forget();
        let paths: Vec<PathBuf> = self.editors.keys().cloned().collect();
        for path in paths {
            for view in self.views_of(&path) {
                view.update(cx, |e, cx| e.redetect_language(cx));
            }
        }
        cx.notify();
    }

    /// Says `text` in the status bar.
    fn tell(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.status = Some(text.into());
        cx.notify();
    }
}

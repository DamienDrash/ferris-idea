//! The file property actions in the window (`SPEC-0336`): line separators, encoding, byte order
//! mark and the read-only state of the active file or the Project tree's selection. What they
//! offer, ask and say is in [`crate::file_properties_model`]; the properties and every write to
//! a file that is not open are in [`idecraft_core::file_properties`].

use std::path::{Path, PathBuf};

use gpui::{Context, Entity, Window};
use idecraft_core::file_properties as fp;

use super::{DrawnFor, WorkbenchView};
use crate::editor_view::EditorView;
use crate::file_properties_model::{self as model, EncodingAnswer, EncodingRow, EncodingStep};

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl WorkbenchView {
    /// Carries out a file property action; false when `id` is none (behaviours 7, 12, 15, 16).
    pub(super) fn file_property_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(target) = fp::LineSeparator::of_action(id) {
            self.convert_line_separators(target, window, cx);
            return true;
        }
        match id {
            "ChangeLineSeparators" => self.line_separator_popup(window, cx),
            "ChangeFileEncodingAction" => self.file_encoding_popup(window, cx),
            "AddBom" => self.change_bom(true, cx),
            "RemoveBom" => self.change_bom(false, cx),
            "ToggleReadOnlyAttribute" => self.toggle_read_only(window, cx),
            _ => return false,
        }
        true
    }

    /// The files an action acts on and the selected folders it leaves out (behaviour 1): the
    /// Project tree's selection while the tree has the focus, else the active editor's file.
    fn property_targets(&self, window: &Window) -> (Vec<PathBuf>, Vec<String>) {
        if self.tree_focus.is_focused(window) {
            let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = self
                .tree
                .selected_paths()
                .into_iter()
                .partition(|p| p.is_dir());
            (files, folders.iter().map(|p| name_of(p)).collect())
        } else {
            (self.active_path().into_iter().collect(), Vec::new())
        }
    }

    /// Says `text` in the status bar.
    fn say(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.status = Some(text.into());
        cx.notify();
    }

    /// Whether the action stops here because this is a remote project (behaviour 4).
    fn no_properties_for_remote(&mut self, cx: &mut Context<Self>) -> bool {
        let remote = self.remote_project.is_some();
        if remote {
            self.say(model::REMOTE, cx);
        }
        remote
    }

    /// `ConvertTo…LineSeparators` (behaviours 7 to 9).
    pub(super) fn convert_line_separators(
        &mut self,
        target: fp::LineSeparator,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.no_properties_for_remote(cx) {
            return;
        }
        let (files, folders) = self.property_targets(window);
        if files.is_empty() && folders.is_empty() {
            return self.say(model::NO_FILE, cx);
        }
        let results: Vec<model::SeparatorResult> = files
            .iter()
            .map(|path| {
                let result = self.convert_separators_of(path, target, cx);
                (name_of(path), result.map_err(|why| why.to_string()))
            })
            .collect();
        self.say(model::separator_summary(target, &results, &folders), cx);
    }

    /// One file: an open editor's text changes as one undo step, a file that is not open is
    /// rewritten on disk (behaviour 8). The line breaks changed.
    fn convert_separators_of(
        &mut self,
        path: &Path,
        target: fp::LineSeparator,
        cx: &mut Context<Self>,
    ) -> Result<usize, fp::Refusal> {
        // Inside the project, also for an open editor (behaviour 2).
        fp::resolve(&self.root, path)?;
        match self.views_of(path).first() {
            Some(editor) => editor
                .update(cx, |e, cx| e.convert_line_separators(target, cx))
                .ok_or(fp::Refusal::ReadOnly),
            None => fp::set_line_separator(&self.root, path, target, false).map(|c| c.changed),
        }
    }

    /// The three conversions as a list, from the status bar (behaviour 21).
    pub(super) fn line_separator_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.no_properties_for_remote(cx) {
            return;
        }
        if self.active_path().is_none() {
            return self.say(model::NO_FILE, cx);
        }
        let items = fp::LineSeparator::ALL
            .iter()
            .map(|s| s.title().to_owned())
            .collect();
        self.show_jvm_popup(
            model::SEPARATORS_TITLE.into(),
            items,
            window,
            cx,
            |this, i, window, cx| {
                if let Some(target) = fp::LineSeparator::ALL.get(i) {
                    this.convert_line_separators(*target, window, cx);
                }
            },
        );
    }

    /// The active editor's file and editor for the encoding actions, when the file is one of
    /// this project (behaviours 1, 2, 4); else the status bar says why not.
    fn encoding_target(&mut self, cx: &mut Context<Self>) -> Option<(PathBuf, Entity<EditorView>)> {
        if self.no_properties_for_remote(cx) {
            return None;
        }
        let Some(path) = self.active_path() else {
            self.say(model::NO_FILE, cx);
            return None;
        };
        if let Err(why) = fp::resolve(&self.root, &path) {
            self.say(
                format!("The encoding of {} is not changed: {why}", name_of(&path)),
                cx,
            );
            return None;
        }
        let editor = self.views_of(&path).into_iter().next()?;
        Some((path, editor))
    }

    /// `ChangeFileEncodingAction`: the list of encodings for the active file (behaviour 12).
    pub(super) fn file_encoding_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, editor)) = self.encoding_target(cx) else {
            return;
        };
        let (rows, selected) = model::encoding_rows(editor.read(cx).encoding());
        let items = rows.iter().map(|r| r.label()).collect();
        self.show_jvm_popup(
            model::ENCODING_TITLE.into(),
            items,
            window,
            cx,
            move |this, i, _window, cx| match rows.get(i) {
                Some(EncodingRow::AddBom) => this.change_bom(true, cx),
                Some(EncodingRow::RemoveBom) => this.change_bom(false, cx),
                Some(EncodingRow::Charset(to)) => this.change_encoding(&path, *to, cx),
                None => {}
            },
        );
        if let Some(popup) = &self.jvm_popup {
            popup.update(cx, |p, cx| {
                p.select(selected);
                cx.notify();
            });
        }
    }

    /// Another encoding was chosen for the open file `path` (behaviour 12): set at once when
    /// nothing changes, else the question.
    pub fn change_encoding(&mut self, path: &Path, to: fp::Charset, cx: &mut Context<Self>) {
        let Some(editor) = self.views_of(path).into_iter().next() else {
            return;
        };
        let name = name_of(path);
        let (current, text) = {
            let e = editor.read(cx);
            (e.encoding(), e.state.buffer.text())
        };
        if current.charset == to {
            return self.say(format!("{name} already is {to}"), cx);
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return self.say(format!("{name} cannot be read: {e}"), cx),
        };
        let reload = fp::reload_safety(&bytes, &text, to);
        let convert = fp::convert_safety(&text, &bytes, fp::FileEncoding::of(to));
        match model::encoding_step(&name, to, &reload, convert) {
            EncodingStep::Set(encoding) => {
                fp::choose(path, to);
                for view in self.views_of(path) {
                    view.update(cx, |e, cx| {
                        e.set_encoding(encoding);
                        cx.notify();
                    });
                }
                self.say(format!("{name} is read and saved as {to} from now on"), cx);
            }
            EncodingStep::Ask(question, answers) => {
                self.drawn_question = Some((
                    question,
                    DrawnFor::Encoding(path.to_path_buf(), to, answers),
                ));
                cx.notify();
            }
        }
    }

    /// An answer to the question of [`Self::change_encoding`] (behaviours 13, 14): a reload
    /// that discards changes and a conversion that loses characters ask once more.
    pub(super) fn answer_encoding(
        &mut self,
        path: PathBuf,
        to: fp::Charset,
        answer: EncodingAnswer,
        cx: &mut Context<Self>,
    ) {
        let name = name_of(&path);
        match answer {
            EncodingAnswer::Cancel => {}
            EncodingAnswer::Reload => {
                if self.doc_modified(&path, cx) {
                    self.drawn_question = Some((
                        model::reload_unsaved_question(&name, to),
                        DrawnFor::EncodingReload(path, to),
                    ));
                    cx.notify();
                } else {
                    self.reload_in_encoding(&path, to, cx);
                }
            }
            EncodingAnswer::Convert => {
                let lost = self
                    .views_of(&path)
                    .first()
                    .and_then(|e| fp::unrepresentable(&e.read(cx).state.buffer.text(), to));
                match lost {
                    Some(lost) => {
                        self.drawn_question = Some((
                            model::lossy_question(&name, to, lost),
                            DrawnFor::EncodingLossy(path, to),
                        ));
                        cx.notify();
                    }
                    None => self.convert_to_encoding(&path, fp::FileEncoding::of(to), false, cx),
                }
            }
        }
    }

    /// Reads the open file `path` again in `to`, discarding unsaved changes (behaviour 13).
    /// Nothing is written.
    pub(super) fn reload_in_encoding(
        &mut self,
        path: &Path,
        to: fp::Charset,
        cx: &mut Context<Self>,
    ) {
        let name = name_of(path);
        let views = self.views_of(path);
        let Some(editor) = views.first() else {
            return;
        };
        // Judged again on the bytes as they are now: never read into replacement characters.
        let possible = std::fs::read(path)
            .map_err(|e| fp::Refusal::Missing(e.to_string()))
            .and_then(|bytes| fp::reload_safety(&bytes, "", to));
        if let Err(why) = possible {
            return self.say(format!("{name} was not reloaded in {to}: {why}"), cx);
        }
        fp::choose(path, to);
        editor.update(cx, |e, cx| e.reload_from_disk(cx));
        let encoding = editor.read(cx).encoding();
        for view in views.iter().skip(1) {
            view.update(cx, |e, _| e.set_encoding(encoding));
        }
        self.mirror(path, Some(editor), &[], cx);
        self.say(format!("{name} was reloaded in {}", encoding.label()), cx);
    }

    /// Writes the open file `path` in `encoding` (behaviours 14, 15); `lossy` is the answer
    /// Convert Anyway.
    pub(super) fn convert_to_encoding(
        &mut self,
        path: &Path,
        encoding: fp::FileEncoding,
        lossy: bool,
        cx: &mut Context<Self>,
    ) {
        let name = name_of(path);
        let views = self.views_of(path);
        let Some(editor) = views.first() else {
            return;
        };
        match editor.update(cx, |e, cx| e.save_in_encoding(encoding, lossy, cx)) {
            Ok(replaced) => {
                for view in views.iter().skip(1) {
                    view.update(cx, |e, _| e.set_encoding(encoding));
                }
                self.mirror(path, Some(editor), &[], cx);
                let mut said = format!("{name} was converted to {}", encoding.label());
                if replaced > 0 {
                    said.push_str(&format!(
                        ": {replaced} {} written as ?",
                        if replaced == 1 {
                            "character was"
                        } else {
                            "characters were"
                        }
                    ));
                }
                self.say(said, cx);
            }
            Err(why) => self.say(
                format!("{name} was not converted to {}: {why}", encoding.label()),
                cx,
            ),
        }
    }

    /// `AddBom`, `RemoveBom` for the active file (behaviour 15).
    pub(super) fn change_bom(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some((path, editor)) = self.encoding_target(cx) else {
            return;
        };
        let current = editor.read(cx).encoding();
        if let Err(why) = fp::bom_change(current, on) {
            let verb = if on { "added to" } else { "removed from" };
            return self.say(
                format!("No byte order mark was {verb} {}: {why}", name_of(&path)),
                cx,
            );
        }
        let encoding = fp::FileEncoding {
            charset: current.charset,
            bom: on,
        };
        self.convert_to_encoding(&path, encoding, false, cx);
    }

    /// `ToggleReadOnlyAttribute` (behaviours 16 to 18): each target gets the opposite state; a
    /// modified editor is saved before its file becomes read-only; the editors follow.
    pub(super) fn toggle_read_only(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.no_properties_for_remote(cx) {
            return;
        }
        let (files, folders) = self.property_targets(window);
        if files.is_empty() && folders.is_empty() {
            return self.say(model::NO_FILE, cx);
        }
        let results: Vec<model::ReadOnlyResult> = files
            .iter()
            .map(|path| {
                let result = self.toggle_read_only_of(path, cx);
                (name_of(path), result)
            })
            .collect();
        self.say(model::read_only_summary(&results, &folders), cx);
    }

    /// One file; the new state.
    fn toggle_read_only_of(&mut self, path: &Path, cx: &mut Context<Self>) -> Result<bool, String> {
        let resolved = fp::resolve(&self.root, path).map_err(|why| why.to_string())?;
        let make_read_only = !fp::is_read_only(&resolved).map_err(|e| e.to_string())?;
        let views = self.views_of(path);
        if make_read_only
            && let Some(editor) = views.first()
            && editor.read(cx).is_modified()
        {
            editor.update(cx, |e, cx| e.run_action("SaveDocument", cx));
            if editor.read(cx).is_modified() {
                return Err("its unsaved changes could not be saved, so it stays writable".into());
            }
            self.mirror(path, Some(editor), &[], cx);
        }
        fp::set_read_only(&self.root, path, make_read_only, false)
            .map_err(|why| why.to_string())?;
        for view in views {
            view.update(cx, |e, cx| {
                e.follow_file_attribute();
                cx.notify();
            });
        }
        Ok(make_read_only)
    }
}

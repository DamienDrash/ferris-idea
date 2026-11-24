//! JavaScript and TypeScript class structure in the window (`SPEC-0320`): gutter marks and the
//! class refactorings. Optimize Imports through the language server (`SPEC-0123` v4 behaviour 6).

use std::path::Path;

use ferris_idea_languages::java_marks::is_java;
use ferris_idea_languages::kotlin_structure::is_kotlin;
use ferris_idea_languages::rename::FileEdits;
use ferris_idea_languages::ts_refactor::{
    ClassMembers, class_members, extract_interface_in, extract_superclass, pull_members_up_in,
    pull_targets_in,
};
use ferris_idea_languages::ts_structure::{declaration_marks_in, is_script};
use gpui::{Context, Window, prelude::*};

use super::WorkbenchView;
use super::java_actions::{WAIT, blocking};
use crate::nav_model::Place;

/// The three refactorings of a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassRefactoring {
    /// `ExtractInterface`.
    Interface,
    /// `ExtractSuperclass`.
    Superclass,
    /// `MembersPullUp`.
    PullUp,
}

impl WorkbenchView {
    /// Optimize Imports of the active JavaScript, TypeScript, Vue or Svelte file (`SPEC-0123` v4
    /// behaviour 6): the language server's organize-imports actions on the editor's text, in the
    /// background, as one undo step that keeps carets and selections on their text.
    pub(super) fn js_optimize_imports(&mut self, cx: &mut Context<Self>) {
        use ferris_idea_languages::js_imports;
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let root = self.root.clone();
        let (file, before) = (path.clone(), text.clone());
        let job =
            blocking(
                async move { js_imports::organize_imports_in(&root, &file, &before, WAIT).await },
            );
        let task = cx.background_executor().spawn(async move { job() });
        self.status = Some("Optimizing imports…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let now = editor.read(cx).state.buffer.text();
                this.status = Some(match result {
                    Err(e) => e,
                    Ok(_) if now != text => {
                        "The file changed meanwhile; nothing was applied".into()
                    }
                    Ok(done) => {
                        let changed = editor.update(cx, |e, cx| e.apply_reformat(&done.text, cx));
                        js_imports::summary(&done, changed)
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }
}

impl ClassRefactoring {
    fn name(self) -> &'static str {
        match self {
            Self::Interface => "Extract Interface",
            Self::Superclass => "Extract Superclass",
            Self::PullUp => "Pull Members Up",
        }
    }
}

impl WorkbenchView {
    /// Computes the gutter marks of a script file in the background (`SPEC-0320` behaviour 9).
    /// Kotlin marks come from the syntax trees and need no server (v3 behaviour 19).
    pub(super) fn request_declaration_marks(&mut self, path: &Path, cx: &mut Context<Self>) {
        let served = ferris_idea_languages::pool::enabled() && (is_script(path) || is_java(path));
        if self.remote_project.is_some() || !(served || is_kotlin(path)) {
            return;
        }
        let Some(editor) = self.editors.get(path) else {
            return;
        };
        if !self.marks_running.insert(path.to_path_buf()) {
            self.marks_again.insert(path.to_path_buf());
            return;
        }
        let (text, revision) = {
            let e = editor.read(cx);
            (e.state.buffer.text(), e.revision())
        };
        let (root, file) = (self.root.clone(), path.to_path_buf());
        let from = file.clone();
        let job = blocking(async move {
            let wait = std::time::Duration::from_secs(60);
            if is_java(&from) || is_kotlin(&from) {
                ferris_idea_languages::java_marks::declaration_marks_in(
                    &root,
                    &from,
                    Some(&text),
                    wait,
                )
                .await
            } else {
                declaration_marks_in(&root, &from, Some(&text), wait).await
            }
            .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                this.marks_running.remove(&file);
                if let (Ok(marks), Some(editor)) = (found, this.editors.get(&file).cloned()) {
                    editor.update(cx, |e, cx| e.set_declaration_marks(revision, &marks, cx));
                }
                if this.marks_again.remove(&file) {
                    this.request_declaration_marks(&file, cx);
                }
            });
        })
        .detach();
    }

    /// A click on the gutter mark of `line`: its one target, or a choice (`SPEC-0320` behaviour 9).
    pub(super) fn declaration_mark(
        &mut self,
        path: &Path,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mark) = self
            .editors
            .get(path)
            .and_then(|e| e.read(cx).declaration_mark(line).cloned())
        else {
            return;
        };
        let places: Vec<Place> = mark
            .targets
            .iter()
            .map(|t| Place {
                path: t.location.path.clone(),
                line: t.location.range.start_line as usize,
                column: t.location.range.start_character as usize,
            })
            .collect();
        if let [only] = places.as_slice() {
            self.show_place(only, window, cx);
            return;
        }
        let items = mark.targets.iter().map(|t| t.name.clone()).collect();
        self.show_jvm_popup(
            mark.tip.clone(),
            items,
            window,
            cx,
            move |this, i, window, cx| {
                if let Some(p) = places.get(i) {
                    this.show_place(p, window, cx);
                }
            },
        );
    }

    /// The text of the active script file at the caret, or a message why not.
    fn script_here(&mut self, what: &str, cx: &mut Context<Self>) -> Option<(Place, String)> {
        let place = self.here(cx)?;
        if !is_script(&place.path) {
            self.status = Some(format!("{what} works in JavaScript and TypeScript files"));
            cx.notify();
            return None;
        }
        let text = self
            .editors
            .get(&place.path)
            .map(|e| e.read(cx).state.buffer.text())?;
        Some((place, text))
    }

    /// The members of the type at `place`, or a message in the status bar.
    fn members_at(
        &mut self,
        place: &Place,
        text: &str,
        what: &str,
        cx: &mut Context<Self>,
    ) -> Option<ClassMembers> {
        match class_members(&place.path, text, place.line as u64, place.column as u64) {
            Ok(m) => Some(m),
            Err(e) => {
                self.status = Some(format!("{what}: {e}"));
                cx.notify();
                None
            }
        }
    }

    /// `ExtractInterface`, `ExtractSuperclass` and `MembersPullUp` at the caret.
    pub(super) fn class_refactoring(
        &mut self,
        kind: ClassRefactoring,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((place, text)) = self.script_here(kind.name(), cx) else {
            return;
        };
        if self.members_at(&place, &text, kind.name(), cx).is_none() {
            return;
        }
        match kind {
            ClassRefactoring::PullUp => self.pull_up_targets(place, text, window, cx),
            ClassRefactoring::Interface => self.start_class_name(true, place, window, cx),
            ClassRefactoring::Superclass => self.start_class_name(false, place, window, cx),
        }
    }

    /// The chooser after the name of an extracted type was given.
    pub(super) fn choose_members_to_extract(
        &mut self,
        interface: bool,
        place: Place,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = if interface {
            ClassRefactoring::Interface
        } else {
            ClassRefactoring::Superclass
        };
        let Some(text) = self.text_at(&place, cx) else {
            return;
        };
        let Some(members) = self.members_at(&place, &text, kind.name(), cx) else {
            return;
        };
        let rows: Vec<_> = members
            .members
            .into_iter()
            .filter(|m| {
                if interface {
                    m.for_interface
                } else {
                    m.movable
                }
            })
            .collect();
        self.member_chooser(
            kind.name().to_owned(),
            rows,
            interface,
            window,
            cx,
            move |this, names, cx| {
                let (root, from, name, place) = (
                    this.root.clone(),
                    place.path.clone(),
                    name.clone(),
                    place.clone(),
                );
                let Some(text) = this.text_at(&place, cx) else {
                    return;
                };
                if interface {
                    this.run_class_refactoring(
                        kind,
                        cx,
                        blocking(async move {
                            extract_interface_in(
                                &root,
                                &from,
                                Some(&text),
                                place.line as u64,
                                place.column as u64,
                                &name,
                                &names,
                                WAIT,
                            )
                            .await
                        }),
                    );
                } else {
                    let planned = extract_superclass(
                        &from,
                        &text,
                        place.line as u64,
                        place.column as u64,
                        &name,
                        &names,
                    );
                    this.finish_class_refactoring(kind, planned, cx);
                }
            },
        );
    }

    fn text_at(&self, place: &Place, cx: &gpui::App) -> Option<String> {
        self.editors
            .get(&place.path)
            .map(|e| e.read(cx).state.buffer.text())
    }

    /// Finds the targets of a pull, then asks for the target and the members.
    fn pull_up_targets(
        &mut self,
        place: Place,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let what = ClassRefactoring::PullUp.name();
        self.status = Some(format!("{what}…"));
        cx.notify();
        let root = self.root.clone();
        let from = place.clone();
        let job = blocking(async move {
            pull_targets_in(
                &root,
                &from.path,
                Some(&text),
                from.line as u64,
                from.column as u64,
                WAIT,
            )
            .await
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let targets = match found {
                    Ok(t) => t,
                    Err(e) => {
                        this.status = Some(format!("{what}: {e}"));
                        cx.notify();
                        return;
                    }
                };
                this.status = None;
                match targets.as_slice() {
                    [] => {
                        this.status = Some(format!("{what}: no supertype in the project"));
                        cx.notify();
                    }
                    [only] => {
                        let only = only.clone();
                        this.choose_members_to_pull(place, only, window, cx);
                    }
                    _ => {
                        let names = targets.clone();
                        this.show_jvm_popup(
                            format!("{what} to"),
                            targets,
                            window,
                            cx,
                            move |this, i, window, cx| {
                                if let Some(t) = names.get(i).cloned() {
                                    this.choose_members_to_pull(place.clone(), t, window, cx);
                                }
                            },
                        );
                    }
                }
            });
        })
        .detach();
    }

    fn choose_members_to_pull(
        &mut self,
        place: Place,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = ClassRefactoring::PullUp;
        let Some(text) = self.text_at(&place, cx) else {
            return;
        };
        let Some(members) = self.members_at(&place, &text, kind.name(), cx) else {
            return;
        };
        let rows: Vec<_> = members.members.into_iter().filter(|m| m.movable).collect();
        let title = format!("{} to {target}", kind.name());
        self.member_chooser(title, rows, false, window, cx, move |this, names, cx| {
            let (root, place, target) = (this.root.clone(), place.clone(), target.clone());
            let Some(text) = this.text_at(&place, cx) else {
                return;
            };
            this.run_class_refactoring(
                kind,
                cx,
                blocking(async move {
                    pull_members_up_in(
                        &root,
                        &place.path,
                        Some(&text),
                        place.line as u64,
                        place.column as u64,
                        Some(&target),
                        &names,
                        false,
                        WAIT,
                    )
                    .await
                }),
            );
        });
    }

    /// A chooser with check boxes over `rows`.
    fn member_chooser(
        &mut self,
        title: String,
        rows: Vec<ferris_idea_languages::ts_refactor::ClassMember>,
        check_all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl Fn(&mut Self, Vec<String>, &mut Context<Self>) + 'static,
    ) {
        if rows.is_empty() {
            self.status = Some(format!("{title}: no member to choose"));
            cx.notify();
            return;
        }
        let items: Vec<String> = rows.iter().map(|m| m.label.clone()).collect();
        let checked = vec![check_all; rows.len()];
        let headings = vec![false; rows.len()];
        let view =
            cx.new(|cx| crate::ChoiceView::new(title, items, cx).with_checks(checked, headings));
        self.jvm_popup_place = super::JvmPopupPlace::Top;
        self._jvm_popup_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.jvm_popup = None;
                this.focus_active(window, cx);
                if let crate::ChoiceEvent::ChosenMany(picked) = event {
                    let names = picked
                        .iter()
                        .filter_map(|i| rows.get(*i).map(|m| m.name.clone()))
                        .collect();
                    chosen(this, names, cx);
                }
                cx.notify();
            },
        ));
        let focus = view.read(cx).focus();
        window.focus(&focus);
        self.close_on_focus_out(super::popups::Popup::RunMarker, &view, &focus, window, cx);
        self.jvm_popup = Some(view);
        cx.notify();
    }

    fn run_class_refactoring(
        &mut self,
        kind: ClassRefactoring,
        cx: &mut Context<Self>,
        job: impl FnOnce() -> Result<Vec<FileEdits>, String> + Send + 'static,
    ) {
        self.status = Some(format!("{}…", kind.name()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let planned = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                this.finish_class_refactoring(kind, planned, cx);
            });
        })
        .detach();
    }

    fn finish_class_refactoring(
        &mut self,
        kind: ClassRefactoring,
        planned: Result<Vec<FileEdits>, String>,
        cx: &mut Context<Self>,
    ) {
        let done = planned.and_then(|files| self.apply_refactoring(kind.name(), &files, &[], cx));
        self.status = Some(match done {
            Ok(()) => format!("{}: done", kind.name()),
            Err(e) => format!("{}: {e}", kind.name()),
        });
        cx.notify();
    }
}

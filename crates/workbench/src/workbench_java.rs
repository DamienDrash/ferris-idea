//! Java class structure in the window (`SPEC-0082` v5, `SPEC-0122` v3, OBS-0027): Type Hierarchy
//! (⌃H), Method Hierarchy (⇧⌘H), Go to Super Method (⌘U), Override Methods (⌃O), Implement
//! Methods (⌃I) and Surround With (⌥⌘T). The lists of Generate, Override/Implement and Delegate
//! Methods open at the caret and filter while typing, and a shown import hint imports at once
//! (`SPEC-0122` v5 behaviours 11a, 13 and 14). The hierarchies, Go to Super Method and the two
//! choosers work in Kotlin files as well (`SPEC-0082` v6, `SPEC-0122` v6).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_languages::java_generate::{
    DelegateField, DelegateMethod, add_delegate_methods_in, delegate_fields_in,
    import_action_title, import_candidates_at, import_hint_text, unresolved_name,
};
use ferris_idea_languages::java_structure::{
    OverridableMethod, SurroundKind, TypeDirection, add_overridable_methods_in,
    method_hierarchy_in, overridable_methods_in, super_target_in, type_hierarchy_in,
};
use gpui::{Context, Entity, Window, prelude::*};

use super::{WorkbenchView, apply_byte_edits, byte_of};
use crate::hierarchy_view::{HierarchyEvent, HierarchyMode, HierarchyView};
use crate::nav_model::Place;

/// How long a question to the language server may take.
pub(super) const WAIT: Duration = Duration::from_secs(30);

/// Runs `job` on a runtime of its own off the UI thread.
pub(super) fn blocking<T: Send + 'static>(
    job: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
) -> impl FnOnce() -> Result<T, String> + Send + 'static {
    move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(job)
    }
}

/// The rows of the Override/Implement chooser: a heading per declaring type, then its methods;
/// `implement` lists only the methods still to implement and checks them all (`SPEC-0122` v3
/// behaviour 6). Returns the items, which are headings, which are checked, and the method per row.
pub fn override_rows(
    methods: &[OverridableMethod],
    implement: bool,
) -> (Vec<String>, Vec<bool>, Vec<bool>, Vec<Option<usize>>) {
    let mut items = Vec::new();
    let mut headings = Vec::new();
    let mut checked = Vec::new();
    let mut index = Vec::new();
    let mut groups: Vec<&str> = Vec::new();
    for m in methods {
        if !groups.contains(&m.declaring_class.as_str()) {
            groups.push(&m.declaring_class);
        }
    }
    let mut first = true;
    for g in groups {
        let rows: Vec<usize> = methods
            .iter()
            .enumerate()
            // Implement lists what is still to implement and a Java interface's default
            // methods (`SPEC-0122` v7 behaviour 6a).
            .filter(|(_, m)| {
                m.declaring_class == g && (!implement || m.unimplemented || m.is_default_method())
            })
            .map(|(i, _)| i)
            .collect();
        if rows.is_empty() {
            continue;
        }
        items.push(g.to_owned());
        headings.push(true);
        checked.push(false);
        index.push(None);
        for i in rows {
            items.push(methods[i].label());
            headings.push(false);
            // Implement checks every method still to implement, not the default methods; Override
            // only the first one (the reference's).
            checked.push(if implement {
                methods[i].unimplemented
            } else {
                first
            });
            first = false;
            index.push(Some(i));
        }
    }
    (items, headings, checked, index)
}

/// What a row of Kotlin's Generate list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KotlinGenerate {
    /// Generates code from the syntax tree.
    Kind(ferris_idea_languages::jvm::GenerateKind),
    /// Opens the Override Members chooser.
    Override,
    /// Opens the Implement Members chooser.
    Implement,
}

/// The rows of Generate in a Kotlin class (`SPEC-0132` v3 behaviour 1a): what is generated from
/// the syntax tree, then the two choosers of `SPEC-0122` v6 behaviour 15.
pub fn kotlin_generate_rows() -> Vec<(String, KotlinGenerate)> {
    use ferris_idea_languages::jvm::GenerateKind;
    let mut rows: Vec<(String, KotlinGenerate)> = ferris_idea_languages::jvm::KOTLIN_KINDS
        .iter()
        .map(|k| {
            let label = match k {
                GenerateKind::Constructor => "Secondary Constructor".to_owned(),
                k => k.label().to_owned(),
            };
            (label, KotlinGenerate::Kind(*k))
        })
        .collect();
    rows.push(("Override Methods…".into(), KotlinGenerate::Override));
    rows.push(("Implement Methods…".into(), KotlinGenerate::Implement));
    rows
}

/// What a row of Java's Generate list does (`SPEC-0122` v10 behaviour 3d).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaGenerate {
    /// Generates code from the syntax tree (behaviours 3–3b).
    Kind(ferris_idea_languages::jvm::GenerateKind),
    /// Opens the Override Methods chooser (behaviour 6).
    Override,
    /// Opens the Implement Methods chooser (behaviour 6).
    Implement,
    /// Delegate Methods (behaviour 10).
    Delegate,
    /// Opens the class's test, or creates it in the test root (behaviour 3e).
    Test,
    /// Writes or updates the copyright notice of the file (`SPEC-0155` v5 behaviour 11).
    Copyright,
}

/// What the class at the caret offers beyond the generated kinds (`SPEC-0122` v10 behaviour 3d).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GenerateFacts {
    /// The class extends or implements a type, or is abstract: it may have methods to implement.
    pub supertypes: bool,
    /// The class has instance fields to delegate to.
    pub fields: bool,
    /// The file is named like a test: it gets no Test… row.
    pub is_test: bool,
    /// The project has a copyright profile.
    pub copyright: bool,
}

/// The rows of Generate in a Java class (`SPEC-0122` v10 behaviour 3d): the kinds that apply, then
/// Override Methods…, Implement Methods… (with supertypes), Delegate Methods… (with fields),
/// Test… (not in a test) and Copyright (with a profile), in the reference's order.
pub fn java_generate_rows(
    kinds: &[ferris_idea_languages::jvm::GenerateKind],
    facts: GenerateFacts,
) -> Vec<(String, JavaGenerate)> {
    let mut rows: Vec<(String, JavaGenerate)> = kinds
        .iter()
        .map(|k| (k.label().to_owned(), JavaGenerate::Kind(*k)))
        .collect();
    rows.push(("Override Methods…".into(), JavaGenerate::Override));
    if facts.supertypes {
        rows.push(("Implement Methods…".into(), JavaGenerate::Implement));
    }
    if facts.fields {
        rows.push(("Delegate Methods…".into(), JavaGenerate::Delegate));
    }
    if !facts.is_test {
        rows.push(("Test…".into(), JavaGenerate::Test));
    }
    if facts.copyright {
        rows.push(("Copyright".into(), JavaGenerate::Copyright));
    }
    rows
}

/// Whether the declaration of class `class` in `text` extends or implements a type or is
/// abstract (its header up to the opening brace says so).
pub fn class_has_supertypes(text: &str, class: &str) -> bool {
    let mut from = 0;
    while let Some(i) = text[from..].find(class) {
        let start = from + i;
        let end = start + class.len();
        from = end;
        let before = &text[..start];
        let word_end = text[end..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !word_end || before.split_whitespace().last() != Some("class") {
            continue;
        }
        let line_start = text[..start].rfind('\n').map_or(0, |n| n + 1);
        let header_end = text[end..].find('{').map_or(text.len(), |b| end + b);
        let header = &text[line_start..header_end];
        let has = |w: &str| header.split_whitespace().any(|t| t == w);
        return has("extends") || has("implements") || has("abstract");
    }
    false
}

/// The hint at the caret for a class that lacks its import (`SPEC-0122` v4 behaviour 11).
pub struct ImportHint {
    /// The file.
    pub path: PathBuf,
    /// Its 0-based line; the hint shows only while the caret is on it.
    pub line: usize,
    /// Char offset of the unresolved name.
    pub offset: usize,
    /// Its column in the line.
    pub column: usize,
    /// The unresolved simple name.
    pub name: String,
    /// The classes that would import it, fully qualified (`SPEC-0122` v5 behaviour 11a).
    pub candidates: Vec<String>,
    /// The text.
    pub text: String,
}

/// The rows of Generate's field chooser (`SPEC-0122` v4 behaviour 3b): items, which are checked and
/// the field per row; the constructor has a first row for no field at all.
pub fn field_rows(
    constructor: bool,
    fields: &[(ferris_idea_languages::jvm::Field, bool)],
) -> (Vec<String>, Vec<bool>, Vec<Option<String>>) {
    let mut items = Vec::new();
    let mut checked = Vec::new();
    let mut names = Vec::new();
    if constructor {
        items.push("(no fields)".to_owned());
        checked.push(false);
        names.push(None);
    }
    for (f, c) in fields {
        items.push(format!("{}: {}", f.name, f.ty));
        checked.push(*c);
        names.push(Some(f.name.clone()));
    }
    (items, checked, names)
}

/// The rows of Generate's template list: label, template and whether `toString` has `@Override`.
pub fn template_rows(
    kind: ferris_idea_languages::jvm::GenerateKind,
) -> Vec<(String, ferris_idea_languages::jvm::GenerateTemplate, bool)> {
    use ferris_idea_languages::jvm::{GenerateKind, GenerateTemplate};
    let mut rows: Vec<(String, GenerateTemplate, bool)> = GenerateTemplate::of(kind)
        .iter()
        .map(|t| (t.label(kind).to_owned(), *t, true))
        .collect();
    if kind == GenerateKind::ToString {
        let plain: Vec<_> = rows
            .iter()
            .map(|(l, t, _)| (format!("{l}, without @Override"), *t, false))
            .collect();
        rows.extend(plain);
    }
    rows
}

impl WorkbenchView {
    /// Asks which fields Generate uses, then which template (`SPEC-0122` v4 behaviour 3b).
    pub fn generate_choose(
        &mut self,
        kind: ferris_idea_languages::jvm::GenerateKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use ferris_idea_languages::jvm::{GenerateKind, chooser_fields};
        let Some(editor) = self.active_editor() else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let at = byte_of(&text, editor.read(cx).state.carets.primary().head);
        let fields = chooser_fields(&text, at, kind);
        if fields.is_empty() {
            self.generate_template(kind, None, window, cx);
            return;
        }
        let constructor = kind == GenerateKind::Constructor;
        let (items, checked, names) = field_rows(constructor, &fields);
        let view = cx.new(|cx| {
            crate::ChoiceView::new("Choose Fields".into(), items, cx)
                .with_checks(checked, vec![])
                .with_filter()
        });
        self.jvm_popup_place = self.caret_list_place(cx);
        self._jvm_popup_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.jvm_popup = None;
                this.focus_active(window, cx);
                if let crate::ChoiceEvent::ChosenMany(rows) = event {
                    let chosen: Vec<String> = rows
                        .iter()
                        .filter_map(|r| names.get(*r).cloned().flatten())
                        .collect();
                    this.generate_template(kind, Some(chosen), window, cx);
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

    /// Asks for the template when there are several, then generates.
    fn generate_template(
        &mut self,
        kind: ferris_idea_languages::jvm::GenerateKind,
        fields: Option<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use ferris_idea_languages::jvm::GenerateOptions;
        let rows = template_rows(kind);
        if rows.len() == 1 {
            self.generate_with(
                kind,
                GenerateOptions {
                    fields,
                    ..Default::default()
                },
                cx,
            );
            return;
        }
        let items = rows.iter().map(|(l, ..)| l.clone()).collect();
        self.show_caret_list(
            "Template".into(),
            items,
            window,
            cx,
            move |this, i, _, cx| {
                let (_, template, override_annotation) = rows[i].clone();
                this.generate_with(
                    kind,
                    GenerateOptions {
                        fields: fields.clone(),
                        template,
                        override_annotation,
                    },
                    cx,
                );
            },
        );
    }

    /// Generates in the Java class at the caret as one undo step.
    fn generate_with(
        &mut self,
        kind: ferris_idea_languages::jvm::GenerateKind,
        options: ferris_idea_languages::jvm::GenerateOptions,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let caret = editor.read(cx).state.carets.primary().head;
        match ferris_idea_languages::jvm::generate_with(
            &text,
            byte_of(&text, caret),
            kind,
            &options,
        ) {
            Ok(edits) => {
                apply_byte_edits(&editor, &text, &edits, false, cx);
                self.status = Some(format!("Generated {}", kind.label()));
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }

    /// Test… of Generate (`SPEC-0122` v10 behaviour 3e): opens the class's test when the project
    /// has one, else creates it in the test root, opens it and puts the caret into its body.
    pub fn generate_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, relative, _)) = self.local_project_file("Test…", cx) else {
            return;
        };
        let text = self
            .editors
            .get(&path)
            .map(|e| e.read(cx).state.buffer.text())
            .unwrap_or_default();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let planned = cx
                .background_spawn(async move {
                    let files = ferris_idea_core::OwnSources::new(&root).files();
                    ferris_idea_languages::test_class::plan(&root, &relative, &text, &files)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let said = match planned {
                    Ok(plan) => this.open_test_class(&plan, window, cx),
                    Err(why) => why,
                };
                this.status = Some(said);
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the test class of `plan`, writing it first when it is new (never over a file that
    /// exists meanwhile); returns what the status bar says.
    pub fn open_test_class(
        &mut self,
        plan: &ferris_idea_languages::test_class::TestClassPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        use std::io::Write as _;
        let file = self.root.join(&plan.path);
        if !plan.exists {
            // The folders of the new file may run through a link out of the project.
            if let Err(e) = ferris_idea_core::file_ops::stays_inside(&self.root, &file) {
                return format!("Test… could not create {}: {e}", plan.path);
            }
            let written = file
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&file)
                })
                .and_then(|mut f| f.write_all(plan.text.as_bytes()));
            if let Err(e) = written {
                return format!("Test… could not create {}: {e}", plan.path);
            }
            ferris_idea_languages::undo_group::record_history(
                &self.root,
                &[file.as_path()],
                Some("Create Test"),
            );
        }
        self.open(&file, window, cx);
        if !plan.exists {
            let at = ferris_idea_languages::test_class::caret_in(&plan.text);
            let line = plan.text[..at].matches('\n').count();
            if let Some(editor) = self.active_editor() {
                editor.update(cx, |e, cx| e.go_to(line, 0, cx));
            }
        }
        self.focus_active(window, cx);
        if plan.exists {
            format!("Opened the test class {}", plan.path)
        } else {
            format!("Created the test class {}", plan.path)
        }
    }

    /// Delegate Methods at the caret (`SPEC-0122` v4 behaviour 10): which field, then which methods.
    pub fn delegate_methods(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((place, text)) = self.java_here("Delegate Methods", cx) else {
            return;
        };
        self.status = Some("Delegate Methods…".into());
        cx.notify();
        let root = self.root.clone();
        let from = place.clone();
        let job = blocking(async move {
            delegate_fields_in(
                &root,
                &from.path,
                Some(&text),
                from.line as u64,
                from.column as u64,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let fields: Vec<DelegateField> = match found {
                    Ok(f) => f.into_iter().filter(|f| !f.methods.is_empty()).collect(),
                    Err(e) => {
                        this.status = Some(format!("Delegate Methods failed: {e}"));
                        cx.notify();
                        return;
                    }
                };
                this.status = None;
                match fields.len() {
                    0 => {
                        this.status = Some("No field to delegate to".into());
                        cx.notify();
                    }
                    1 => this.delegate_chooser(place.clone(), fields[0].clone(), window, cx),
                    _ => {
                        let items = fields
                            .iter()
                            .map(|f| format!("{}: {}", f.name, f.ty))
                            .collect();
                        this.show_caret_list(
                            "Delegate to Field".into(),
                            items,
                            window,
                            cx,
                            move |this, i, window, cx| {
                                this.delegate_chooser(place.clone(), fields[i].clone(), window, cx);
                            },
                        );
                    }
                }
            });
        })
        .detach();
    }

    /// The methods of `field` to forward to, with check boxes.
    pub fn delegate_chooser(
        &mut self,
        place: Place,
        field: DelegateField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items: Vec<String> = field.methods.iter().map(DelegateMethod::label).collect();
        let checked = vec![false; items.len()];
        let title = format!("Delegate to {}", field.name);
        let view = cx.new(|cx| {
            crate::ChoiceView::new(title, items, cx)
                .with_checks(checked, vec![])
                .with_filter()
        });
        self.jvm_popup_place = self.caret_list_place(cx);
        self._jvm_popup_events = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &crate::ChoiceEvent, window, cx| {
                this.jvm_popup = None;
                this.focus_active(window, cx);
                if let crate::ChoiceEvent::ChosenMany(rows) = event {
                    let chosen: Vec<(DelegateField, DelegateMethod)> = rows
                        .iter()
                        .filter_map(|r| field.methods.get(*r).cloned())
                        .map(|m| (field.clone(), m))
                        .collect();
                    this.add_delegates(place.clone(), chosen, window, cx);
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

    /// Adds the forwarding methods through jdtls as one undo step.
    fn add_delegates(
        &mut self,
        place: Place,
        chosen: Vec<(DelegateField, DelegateMethod)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if chosen.is_empty() {
            return;
        }
        let Some(text) = self
            .editors
            .get(&place.path)
            .map(|e| e.read(cx).state.buffer.text())
        else {
            return;
        };
        let names: Vec<String> = chosen.iter().map(|(_, m)| m.name.clone()).collect();
        let root = self.root.clone();
        let from = place.clone();
        let job = blocking(async move {
            add_delegate_methods_in(
                &root,
                &from.path,
                Some(&text),
                from.line as u64,
                from.column as u64,
                &chosen,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let planned = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(
                    match planned.and_then(|files| this.apply_file_edits(&files, cx)) {
                        Ok(()) => format!("Added {}", names.join(", ")),
                        Err(e) => format!("Adding the methods failed: {e}"),
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Looks whether the caret's line holds a class without import and asks once for the classes
    /// that would import it (`SPEC-0122` v4 behaviour 11).
    pub(super) fn refresh_import_hint(&mut self, cx: &mut Context<Self>) {
        let found = (|| {
            if self.remote_project.is_some() {
                return None;
            }
            let path = self.active_path()?;
            if ferris_idea_languages::jvm::Jvm::of(&path.to_string_lossy())
                != Some(ferris_idea_languages::jvm::Jvm::Java)
            {
                return None;
            }
            let editor = self.active_editor()?;
            let e = editor.read(cx);
            let buffer = &e.state.buffer;
            let line = buffer.offset_to_position(e.caret()).ok()?.line;
            let start = buffer
                .position_to_offset(ferris_idea_editor::Position::new(line, 0))
                .ok()?;
            let end = buffer
                .position_to_offset(ferris_idea_editor::Position::new(line + 1, 0))
                .unwrap_or_else(|_| buffer.len_chars());
            e.server_problems_in(start..end)
                .into_iter()
                .find_map(|(offset, message)| {
                    let name = unresolved_name(&message)?.to_owned();
                    let at = buffer.offset_to_position(offset).ok()?;
                    Some((path.clone(), line, offset, at.column, name))
                })
                .map(|(path, line, offset, column, name)| {
                    (path, line, offset, column, name, buffer.text())
                })
        })();
        let Some((path, line, offset, column, name, text)) = found else {
            self.import_asked = None;
            if self.import_hint.take().is_some() {
                cx.notify();
            }
            return;
        };
        let key = (path.clone(), line, name.clone());
        if self.import_asked.as_ref() == Some(&key) {
            return;
        }
        self.import_asked = Some(key.clone());
        self.import_hint = None;
        let root = self.root.clone();
        let file = path.clone();
        let asked = name.clone();
        let job = blocking(async move {
            import_candidates_at(
                &root,
                &file,
                Some(&text),
                line as u64,
                column as u64,
                &asked,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                if this.import_asked.as_ref() != Some(&key) {
                    return;
                }
                let shortcut = crate::keymaps::current()
                    .shortcut_label("ShowIntentionActions", cfg!(target_os = "macos"));
                let candidates = found.unwrap_or_default();
                this.import_hint =
                    import_hint_text(&name, &candidates, &shortcut).map(|text| ImportHint {
                        path,
                        line,
                        offset,
                        column,
                        name,
                        candidates,
                        text,
                    });
                cx.notify();
            });
        })
        .detach();
    }

    /// The import hint's text while it may show (tests).
    pub fn import_hint_text(&self) -> Option<String> {
        self.import_hint.as_ref().map(|h| h.text.clone())
    }

    /// The import hint while it shows: the caret is on its line and no other popup is open.
    fn import_hint_shown(&self, cx: &gpui::App) -> Option<&ImportHint> {
        let h = self.import_hint.as_ref()?;
        if self.params.is_some()
            || self.completion.is_some()
            || self.intentions_popup().is_some()
            || self.jvm_popup.is_some()
            || self.active_path().as_deref() != Some(h.path.as_path())
        {
            return None;
        }
        let editor = self.editors.get(&h.path)?.read(cx);
        let caret = editor
            .state
            .buffer
            .offset_to_position(editor.caret())
            .ok()?;
        (caret.line == h.line).then_some(h)
    }

    /// Whether the import hint shows now (tests).
    pub fn import_hint_is_shown(&self, cx: &gpui::App) -> bool {
        self.import_hint_shown(cx).is_some()
    }

    /// Show Context Actions on a shown import hint (`SPEC-0122` v5 behaviour 11a): one class is
    /// imported at once, several are listed at the caret. `false` without a shown hint.
    pub(super) fn import_from_hint(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(h) = self.import_hint_shown(cx) else {
            return false;
        };
        let at = super::IntentionsAt {
            path: h.path.clone(),
            range: ferris_idea_languages::insight::Range {
                start_line: h.line as u64,
                start_character: h.column as u64,
                end_line: h.line as u64,
                end_character: h.column as u64,
            },
            offset: h.offset,
        };
        let name = h.name.clone();
        let candidates = h.candidates.clone();
        let titles: Vec<String> = candidates
            .iter()
            .filter_map(|class| import_action_title(&name, class))
            .collect();
        if titles.is_empty() || titles.len() != candidates.len() {
            return false;
        }
        // The hint has done its work; it comes back only for another line or name.
        self.import_hint = None;
        if let [title] = titles.as_slice() {
            self.apply_intention(at, title.clone(), window, cx);
        } else {
            self.show_caret_list(
                format!("Import {name}"),
                candidates,
                window,
                cx,
                move |this, i, window, cx| {
                    this.apply_intention(at.clone(), titles[i].clone(), window, cx);
                },
            );
        }
        cx.notify();
        true
    }

    /// The hint above the unresolved name, while the caret is on its line and no popup is open.
    pub(super) fn import_hint_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl gpui::IntoElement + use<>> {
        use gpui::{Styled, div, px};
        let h = self.import_hint_shown(cx)?;
        let editor = self.editors.get(&h.path)?.read(cx);
        let at = editor.point_above(h.offset)?;
        let pal = crate::palette::Palette::current();
        Some(
            gpui::anchored()
                .anchor(gpui::Corner::BottomLeft)
                .position(at)
                .child(
                    div()
                        .occlude()
                        .flex()
                        .px_2()
                        .h(px(24.))
                        .items_center()
                        .mb_1()
                        .bg(super::c(pal.island))
                        .border_1()
                        .border_color(super::c(pal.separator))
                        .rounded(px(6.))
                        .shadow_lg()
                        .text_color(super::c(pal.ui_text))
                        .child(h.text.clone()),
                ),
        )
    }

    /// Whether the active file is Java; otherwise says what the action needs.
    fn java_here(&mut self, what: &str, cx: &mut Context<Self>) -> Option<(Place, String)> {
        let place = self.here(cx)?;
        let java = ferris_idea_languages::jvm::Jvm::of(&place.path.to_string_lossy())
            == Some(ferris_idea_languages::jvm::Jvm::Java);
        if !java {
            self.status = Some(format!("{what} works in Java files"));
            cx.notify();
            return None;
        }
        let text = self
            .editors
            .get(&place.path)
            .map(|e| e.read(cx).state.buffer.text())?;
        Some((place, text))
    }

    /// Whether the active file is Java or Kotlin (`SPEC-0082` v6, `SPEC-0122` v6); otherwise says
    /// what the action needs.
    fn jvm_here(&mut self, what: &str, cx: &mut Context<Self>) -> Option<(Place, String)> {
        let place = self.here(cx)?;
        if ferris_idea_languages::jvm::Jvm::of(&place.path.to_string_lossy()).is_none() {
            self.status = Some(format!("{what} works in Java and Kotlin files"));
            cx.notify();
            return None;
        }
        let text = self
            .editors
            .get(&place.path)
            .map(|e| e.read(cx).state.buffer.text())?;
        Some((place, text))
    }

    /// The unsaved texts of open editors, for reading other project files.
    fn open_texts(&self, cx: &gpui::App) -> Vec<(PathBuf, String)> {
        self.editors
            .iter()
            .filter(|(_, e)| e.read(cx).is_modified())
            .map(|(p, e)| (p.clone(), e.read(cx).state.buffer.text()))
            .collect()
    }

    /// Shows `view` in the Hierarchy tool window and follows its events (`SPEC-0082`).
    pub(super) fn install_hierarchy(
        &mut self,
        view: Entity<HierarchyView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._hierarchy_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, view, event: &HierarchyEvent, window, cx| match event {
                HierarchyEvent::Open(place) => {
                    // The tool window stays open, as the reference's does.
                    this.remember_place(cx);
                    let place = place.clone();
                    this.show_place(&place, window, cx);
                }
                HierarchyEvent::Direction(direction) => {
                    let origin = view.read(cx).origin.clone();
                    this.request_hierarchy(origin, *direction, window, cx);
                }
                HierarchyEvent::Types(direction) => {
                    let origin = view.read(cx).origin.clone();
                    this.request_type_hierarchy(origin, *direction, window, cx);
                }
                HierarchyEvent::Method => {
                    let origin = view.read(cx).origin.clone();
                    this.request_method_hierarchy(origin, window, cx);
                }
                HierarchyEvent::Close => {
                    this.hierarchy = None;
                    this.focus_active(window, cx);
                    cx.notify();
                }
            },
        ));
        window.focus(&view.read(cx).focus());
        self.hierarchy = Some(view);
        cx.notify();
    }

    /// Type Hierarchy (⌃H) at the caret (`SPEC-0082` v5 behaviour 13).
    pub fn type_hierarchy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let script = self
            .here(cx)
            .is_some_and(|p| ferris_idea_languages::ts_structure::is_script(&p.path));
        let place = if script {
            self.here(cx)
        } else {
            self.jvm_here("Type Hierarchy", cx).map(|(p, _)| p)
        };
        if let Some(place) = place {
            self.request_type_hierarchy(place, TypeDirection::Class, window, cx);
        }
    }

    /// Builds a part of the type hierarchy of the type at `origin` and shows it.
    pub fn request_type_hierarchy(
        &mut self,
        origin: Place,
        direction: TypeDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self
            .editors
            .get(&origin.path)
            .map(|e| e.read(cx).state.buffer.text());
        self.status = Some("Building the type hierarchy…".into());
        cx.notify();
        let root = self.root.clone();
        let from = origin.clone();
        let job = blocking(async move {
            // JavaScript and TypeScript have no server hierarchy (SPEC-0320 behaviour 1).
            if ferris_idea_languages::ts_structure::is_script(&from.path) {
                ferris_idea_languages::ts_structure::type_hierarchy_in(
                    &root,
                    &from.path,
                    text.as_deref(),
                    from.line as u64,
                    from.column as u64,
                    direction,
                    WAIT,
                )
                .await
            } else {
                type_hierarchy_in(
                    &root,
                    &from.path,
                    text.as_deref(),
                    from.line as u64,
                    from.column as u64,
                    direction,
                    WAIT,
                )
                .await
            }
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| match found {
                Ok(Some(tree)) => {
                    this.status = None;
                    let view = cx.new(|cx| {
                        HierarchyView::new(
                            tree,
                            ferris_idea_languages::insight::CallDirection::Callers,
                            origin,
                            cx,
                        )
                        .with_mode(HierarchyMode::Types(direction), None)
                    });
                    this.install_hierarchy(view, window, cx);
                }
                Ok(None) => {
                    this.status = Some("No type at the caret.".into());
                    cx.notify();
                }
                Err(e) => {
                    this.status = Some(format!("Type hierarchy failed: {e}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Method Hierarchy (⇧⌘H) of the method at the caret (`SPEC-0082` v5 behaviour 14).
    pub fn method_hierarchy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((place, _)) = self.jvm_here("Method Hierarchy", cx) {
            self.request_method_hierarchy(place, window, cx);
        }
    }

    fn request_method_hierarchy(
        &mut self,
        origin: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self
            .editors
            .get(&origin.path)
            .map(|e| e.read(cx).state.buffer.text());
        let texts = self.open_texts(cx);
        self.status = Some("Building the method hierarchy…".into());
        cx.notify();
        let root = self.root.clone();
        let from = origin.clone();
        let job = blocking(async move {
            let lookup = move |p: &Path| {
                let p = p.canonicalize().unwrap_or(p.to_path_buf());
                texts.iter().find(|(q, _)| *q == p).map(|(_, t)| t.clone())
            };
            method_hierarchy_in(
                &root,
                &from.path,
                text.as_deref(),
                from.line as u64,
                from.column as u64,
                &lookup,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| match found {
                Ok(Some((method, tree))) => {
                    this.status = None;
                    let view = cx.new(|cx| {
                        HierarchyView::new(
                            tree,
                            ferris_idea_languages::insight::CallDirection::Callers,
                            origin,
                            cx,
                        )
                        .with_mode(HierarchyMode::Method, Some(method))
                    });
                    this.install_hierarchy(view, window, cx);
                }
                Ok(None) => {
                    this.status = Some("No method at the caret.".into());
                    cx.notify();
                }
                Err(e) => {
                    this.status = Some(format!("Method hierarchy failed: {e}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Go to Super Method (⌘U): the method the one at the caret overrides or implements, or the
    /// superclass of the type at the caret (`SPEC-0082` v5 behaviour 15).
    pub fn goto_super(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((place, text)) = self.jvm_here("Go to Super Method", cx) else {
            return;
        };
        let texts = self.open_texts(cx);
        self.status = Some("Looking for the super method…".into());
        cx.notify();
        let root = self.root.clone();
        let from = place.clone();
        let job = blocking(async move {
            let lookup = move |p: &Path| {
                let p = p.canonicalize().unwrap_or(p.to_path_buf());
                texts.iter().find(|(q, _)| *q == p).map(|(_, t)| t.clone())
            };
            super_target_in(
                &root,
                &from.path,
                Some(&text),
                from.line as u64,
                from.column as u64,
                &lookup,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match found {
                    Ok(Some(target)) => match target.location {
                        Some(at) => {
                            this.status = None;
                            this.remember_place(cx);
                            let place = Place {
                                path: at.path.canonicalize().unwrap_or(at.path.clone()),
                                line: at.range.start_line as usize,
                                column: at.range.start_character as usize,
                            };
                            this.show_place(&place, window, cx);
                        }
                        None => {
                            this.status = Some(format!(
                                "{} is a library type; Ferris IDEA cannot open library sources yet.",
                                target.name
                            ))
                        }
                    },
                    Ok(None) => {
                        this.status = Some("No super method or supertype found.".into());
                    }
                    Err(e) => this.status = Some(format!("Go to Super Method failed: {e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Override Methods (⌃O) or, with `implement`, Implement Methods (⌃I): a chooser with check
    /// boxes grouped by declaring type, then the methods from jdtls (`SPEC-0122` v3 behaviour 6);
    /// in a Kotlin file the members from the project's syntax trees (v6 behaviour 15).
    pub fn override_methods(
        &mut self,
        implement: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let what = if implement {
            "Implement Methods"
        } else {
            "Override Methods"
        };
        let Some((place, text)) = self.jvm_here(what, cx) else {
            return;
        };
        // Kotlin overrides properties too, so its choosers speak of members (behaviour 15).
        let kotlin = ferris_idea_languages::kotlin_structure::is_kotlin(&place.path);
        self.status = Some(format!("{what}…"));
        cx.notify();
        let root = self.root.clone();
        let from = place.clone();
        let job = blocking(async move {
            overridable_methods_in(
                &root,
                &from.path,
                Some(&text),
                from.line as u64,
                from.column as u64,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = cx.background_spawn(async move { job() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let methods = match found {
                    Ok((_, methods)) => methods,
                    Err(e) => {
                        this.status = Some(format!("{what} failed: {e}"));
                        cx.notify();
                        return;
                    }
                };
                let (items, headings, checked, index) = override_rows(&methods, implement);
                if items.is_empty() {
                    this.status = Some(
                        match (implement, kotlin) {
                            (true, false) => "No methods to implement",
                            (false, false) => "No methods to override",
                            (true, true) => "No members to implement",
                            (false, true) => "No members to override",
                        }
                        .into(),
                    );
                    cx.notify();
                    return;
                }
                this.status = None;
                let title = match (implement, kotlin) {
                    (true, false) => "Select Methods to Implement",
                    (false, false) => "Select Methods to Override/Implement",
                    (true, true) => "Implement Members",
                    (false, true) => "Override Members",
                };
                let view = cx.new(|cx| {
                    crate::ChoiceView::new(title.into(), items, cx)
                        .with_checks(checked, headings)
                        .with_filter()
                });
                this.jvm_popup_place = this.caret_list_place(cx);
                this._jvm_popup_events = Some(cx.subscribe_in(
                    &view,
                    window,
                    move |this, _, event: &crate::ChoiceEvent, window, cx| {
                        this.jvm_popup = None;
                        this.focus_active(window, cx);
                        if let crate::ChoiceEvent::ChosenMany(rows) = event {
                            let chosen: Vec<OverridableMethod> = rows
                                .iter()
                                .filter_map(|r| index.get(*r).copied().flatten())
                                .filter_map(|i| methods.get(i).cloned())
                                .collect();
                            this.add_methods(place.clone(), chosen, window, cx);
                        }
                        cx.notify();
                    },
                ));
                let focus = view.read(cx).focus();
                window.focus(&focus);
                this.close_on_focus_out(super::popups::Popup::RunMarker, &view, &focus, window, cx);
                this.jvm_popup = Some(view);
                cx.notify();
            });
        })
        .detach();
    }

    /// Adds the chosen methods (through jdtls in Java) as one undo step.
    fn add_methods(
        &mut self,
        place: Place,
        methods: Vec<OverridableMethod>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if methods.is_empty() {
            return;
        }
        let Some(text) = self
            .editors
            .get(&place.path)
            .map(|e| e.read(cx).state.buffer.text())
        else {
            return;
        };
        let names: Vec<String> = methods.iter().map(|m| m.name.clone()).collect();
        let root = self.root.clone();
        let from = place.clone();
        let job = blocking(async move {
            add_overridable_methods_in(
                &root,
                &from.path,
                Some(&text),
                from.line as u64,
                from.column as u64,
                &methods,
                WAIT,
            )
            .await
            .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let planned = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(
                    match planned.and_then(|files| this.apply_file_edits(&files, cx)) {
                        Ok(()) => format!("Added {}", names.join(", ")),
                        Err(e) => format!("Adding the methods failed: {e}"),
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Surround With (⌥⌘T): the wrappers for the selected statements, then one undo step
    /// (`SPEC-0122` v3 behaviour 7).
    pub fn surround_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.java_here("Surround With", cx).is_none() {
            return;
        }
        let items = SurroundKind::ALL
            .iter()
            .map(|k| k.label().to_owned())
            .collect();
        self.show_jvm_popup(
            "Surround With".into(),
            items,
            window,
            cx,
            |this, i, _, cx| {
                this.surround(SurroundKind::ALL[i], cx);
            },
        );
    }

    /// Wraps the selected lines (or the caret's line) in `kind`.
    pub fn surround(&mut self, kind: SurroundKind, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let caret = editor.read(cx).state.carets.primary();
        let (a, b) = (caret.anchor.min(caret.head), caret.anchor.max(caret.head));
        let selection = byte_of(&text, a)..byte_of(&text, b);
        match ferris_idea_languages::java_structure::surround(&text, selection, kind) {
            Ok(s) => {
                let edit = ferris_idea_languages::jvm::TextEdit {
                    range: s.range.clone(),
                    text: s.text.clone(),
                };
                apply_byte_edits(&editor, &text, &[edit], false, cx);
                let mut after = text.clone();
                after.replace_range(s.range, &s.text);
                let at = after[..s.caret.min(after.len())].chars().count();
                editor.update(cx, |e, cx| e.select_range(at..at, cx));
                self.status = Some(format!("Surrounded with {}", kind.label()));
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_and_template_rows() {
        use ferris_idea_languages::jvm::{Field, GenerateKind, GenerateTemplate};
        let f = |n: &str| Field {
            name: n.into(),
            ty: "int".into(),
            is_static: false,
            is_final: false,
            initialized: false,
        };
        let (items, checked, names) = field_rows(true, &[(f("a"), false), (f("b"), true)]);
        assert_eq!(items, ["(no fields)", "a: int", "b: int"]);
        assert_eq!(checked, [false, false, true]);
        assert_eq!(names[0], None);
        assert_eq!(names[2].as_deref(), Some("b"));
        let (items, ..) = field_rows(false, &[(f("a"), true)]);
        assert_eq!(items, ["a: int"]);
        let rows = template_rows(GenerateKind::ToString);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].1, GenerateTemplate::Default);
        assert!(rows[0].2);
        assert!(!rows[3].2);
        assert_eq!(template_rows(GenerateKind::Constructor).len(), 1);
    }

    fn m(name: &str, class: &str, unimplemented: bool) -> OverridableMethod {
        OverridableMethod {
            name: name.into(),
            parameters: Vec::new(),
            unimplemented,
            declaring_class: class.into(),
            declaring_class_type: "class".into(),
            return_type: None,
            raw: serde_json::Value::Null,
        }
    }

    /// SPEC-0132 AK-6 (v3): Kotlin's Generate list ends with the two choosers.
    #[test]
    fn kotlin_generate_list() {
        let rows = kotlin_generate_rows();
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Secondary Constructor",
                "equals() and hashCode()",
                "toString()",
                "Override Methods…",
                "Implement Methods…"
            ]
        );
        assert_eq!(rows[3].1, KotlinGenerate::Override);
        assert_eq!(rows[4].1, KotlinGenerate::Implement);
    }

    /// SPEC-0122 v7 (OBS-0027 L30): Implement lists a Java interface's default methods too,
    /// unchecked, and the labels carry the return type when the source gives one.
    #[test]
    fn implement_lists_default_methods_unchecked() {
        let mut owner = m("owner", "shop.Pet", true);
        owner.declaring_class_type = "interface".into();
        owner.return_type = Some("String".into());
        let mut friendly = m("friendly", "shop.Pet", false);
        friendly.declaring_class_type = "interface".into();
        let describe = m("describe", "shop.Animal", false);
        let (items, headings, checked, _) = override_rows(&[describe, owner, friendly], true);
        assert_eq!(items, ["shop.Pet", "owner(): String", "friendly()"]);
        assert_eq!(headings, [true, false, false]);
        assert_eq!(checked, [false, true, false]);
    }

    /// SPEC-0122 AK-19 (v6): a Kotlin property keeps its own row label.
    #[test]
    fn override_rows_show_kotlin_labels() {
        let mut owner = m("owner", "zoo.Pet", true);
        owner.raw = serde_json::json!({"label": "owner: String"});
        let (items, _, checked, _) = override_rows(&[owner], true);
        assert_eq!(items, ["zoo.Pet", "owner: String"]);
        assert_eq!(checked, [false, true]);
    }

    #[test]
    fn override_rows_group_by_type_and_preselect() {
        let ms = [
            m("describe", "shop.Animal", false),
            m("owner", "shop.Pet", true),
            m("friendly", "shop.Pet", false),
            m("toString", "java.lang.Object", false),
        ];
        let (items, headings, checked, index) = override_rows(&ms, false);
        assert_eq!(
            items,
            [
                "shop.Animal",
                "describe()",
                "shop.Pet",
                "owner()",
                "friendly()",
                "java.lang.Object",
                "toString()"
            ]
        );
        assert_eq!(headings, [true, false, true, false, false, true, false]);
        assert_eq!(checked, [false, true, false, false, false, false, false]);
        assert_eq!(index[1], Some(0));
        let (items, _, checked, _) = override_rows(&ms, true);
        assert_eq!(items, ["shop.Pet", "owner()"]);
        assert_eq!(checked, [false, true]);
    }
}

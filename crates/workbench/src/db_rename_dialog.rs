//! Renaming a table or column from the Database tool window (`SPEC-0128`): the new name, a preview
//! of the `ALTER TABLE` statement and of the places in the workspace's SQL files, then one action
//! that runs the statement and changes the files. v2: each file in its own dialect (other dialects
//! only when included) and constraint or index names that embed the old name.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferris_idea_database::{
    ConstraintName, DerivedRename, Dialect, Refactoring, RenameError, TextEdit, derived_name_edits,
    derived_renames, file_dialect, refactor, rename_sequence_in,
};
use gpui::{Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::database_view::RenameTarget;
use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::workbench_view::c;

/// A SQL file of the workspace as the rename sees it: the editor's text when it is open.
#[derive(Debug, Clone)]
pub struct SqlFile {
    /// Absolute path.
    pub path: PathBuf,
    /// Path shown, relative to the workspace.
    pub label: String,
    /// Its text when the dialog opened.
    pub text: String,
    /// Why the file is not one of the project's own sources (`SPEC-0128` v3 behaviour 6); `None`
    /// when it is.
    pub outside: Option<ferris_idea_core::NotOwn>,
    /// The connection the file is bound to (`SPEC-0093`): id, name and dialect; `None` when
    /// unbound.
    pub bound: Option<(String, String, Dialect)>,
}

/// The changes of one file.
#[derive(Debug, Clone)]
pub struct FileChange {
    /// The file.
    pub file: SqlFile,
    /// Edits on its text (byte ranges).
    pub edits: Vec<TextEdit>,
    /// Places left alone because they could refer to another table, with the reason.
    pub ambiguous: Vec<(usize, String)>,
    /// The dialect the file was analysed in (v2).
    pub dialect: Dialect,
    /// Of another dialect than the connection.
    pub other: bool,
    /// The name of the other connection the file is bound to: listed, never changed (`SPEC-0164` v4
    /// behaviour 15).
    pub other_connection: Option<String>,
    /// Whether Refactor changes it (files of another dialect and files that are not the project's
    /// own only when included).
    pub included: bool,
}

/// What the rename would do (behaviour 2).
#[derive(Debug, Clone)]
pub struct Preview {
    /// The `ALTER TABLE` statement.
    pub ddl: String,
    /// Files with edits or ambiguous places.
    pub files: Vec<FileChange>,
    /// Constraint and index names that embed the old name, and whether each is renamed too.
    pub derived: Vec<(DerivedRename, bool)>,
    /// What it risks and how it runs, said before it runs (`SPEC-0164` v3 behaviour 12).
    pub notes: Vec<ferris_idea_database::RefactorNote>,
    /// Whether the statements run in one transaction (`SPEC-0164` v3 behaviour 11).
    pub transaction: bool,
}

impl Preview {
    /// Number of places changed in the included files.
    pub fn edit_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.included)
            .map(|f| f.edits.len())
            .sum()
    }

    /// Statements of the derived names that are renamed too.
    pub fn chosen_derived(&self) -> impl Iterator<Item = &String> {
        self.derived
            .iter()
            .filter(|(_, on)| *on)
            .filter_map(|(d, _)| d.ddl.as_ref())
    }

    /// Number of places left alone.
    pub fn ambiguous_count(&self) -> usize {
        self.files.iter().map(|f| f.ambiguous.len()).sum()
    }
}

/// The preview of renaming `target` to `new_name` over `files`: each file in its own dialect
/// (`SPEC-0019` v2 behaviour 8); files of another dialect and files that are not the project's
/// own (`SPEC-0128` v3 behaviour 6) are included when their label is in `include`, and the latter
/// come after the others; derived names are renamed unless their old name is in `leave`
/// (behaviour 9).
pub fn preview(
    target: &RenameTarget,
    new_name: &str,
    files: &[SqlFile],
    include: &BTreeSet<String>,
    leave: &BTreeSet<String>,
) -> Result<Preview, RenameError> {
    let run = |sql: &str, dialect: Dialect| match &target.what {
        // SPEC-0164 v5 behaviour 8: a sequence literal changes when it names the sequence of the
        // connection's schema without doubt.
        Refactoring::RenameSequence(sequence) => rename_sequence_in(
            sql,
            dialect,
            sequence,
            target.home_schema.as_deref(),
            new_name,
        ),
        _ => refactor(
            sql,
            dialect,
            &target.tables,
            &target.table,
            target.column.as_deref(),
            &target.what,
            new_name,
        ),
    };
    let own = target.profile.dialect;
    let ddl = run("", own)?.ddl;
    let derived: Vec<(DerivedRename, bool)> = if target.what == Refactoring::Rename {
        derived_renames(
            own,
            &target.tables,
            &target.constraints,
            &target.table,
            target.column.as_deref(),
            new_name,
        )
        .into_iter()
        .map(|d| {
            let on = d.ddl.is_some() && !leave.contains(&d.old);
            (d, on)
        })
        .collect()
    } else {
        Vec::new()
    };
    let mut changes = Vec::new();
    for f in files {
        // A file bound to another connection is read in that connection's dialect and only
        // listed (`SPEC-0164` v4 behaviour 15).
        let other_connection = f
            .bound
            .as_ref()
            .filter(|(id, ..)| *id != target.profile.id)
            .map(|(_, name, _)| name.clone());
        let dialect = match &f.bound {
            Some((.., d)) => *d,
            None => file_dialect(Path::new(&f.label)).unwrap_or(own),
        };
        let other = dialect != own;
        let r = match run(&f.text, dialect) {
            Ok(r) => r,
            Err(_) if other || other_connection.is_some() => continue,
            Err(e) => return Err(e),
        };
        let mut edits = r.edits;
        // The file's DDL follows the derived names renamed too (v3).
        for (d, _) in derived.iter().filter(|(_, on)| *on) {
            edits.extend(derived_name_edits(&f.text, dialect, &d.old, &d.new));
        }
        edits.sort_by_key(|e| e.start);
        edits.dedup_by_key(|e| e.start);
        if edits.is_empty() && r.ambiguous.is_empty() {
            continue;
        }
        changes.push(FileChange {
            file: f.clone(),
            edits,
            ambiguous: r
                .ambiguous
                .into_iter()
                .map(|a| (a.start, a.reason))
                .collect(),
            dialect,
            other,
            included: other_connection.is_none()
                && ((!other && f.outside.is_none()) || include.contains(&f.label)),
            other_connection,
        });
    }
    // Files that are not the project's own after the others (stable: each part keeps its order).
    changes.sort_by_key(|c| c.file.outside.is_some());
    Ok(Preview {
        ddl,
        files: changes,
        derived,
        notes: ferris_idea_database::refactoring_notes(&target.what, own),
        transaction: ferris_idea_database::ddl_in_transaction(own),
    })
}

/// The first answer when undo took back a database refactoring's file change (`SPEC-0164` v5
/// behaviour 16): the database stays as it is.
pub const KEEP_DATABASE: &str = "Keep the database as it is";

/// What a database refactoring leaves with the undo step of an open file it changed (`SPEC-0164` v5
/// behaviour 16, IMP-0419): undoing that step takes back the file only.
#[derive(Debug, Clone)]
pub struct DbUndo {
    /// The connection the refactoring ran on.
    pub profile: ferris_idea_database::ConnectionProfile,
    /// The refactoring that takes the database change back.
    pub back: ferris_idea_database::WayBack,
    /// What the database keeps, e.g. `the column is still pd_note.qty`.
    pub keeps: String,
    /// The second answer, which opens the dialog for the way back.
    pub offer: String,
}

impl DbUndo {
    /// The status note when the step of the file `file` (its name) is undone.
    pub fn note(&self, file: &str) -> String {
        format!("Undo took back {file} only: in the database {}", self.keeps)
    }

    /// The answers of the popup, the database kept first.
    pub fn answers(&self) -> Vec<String> {
        vec![KEEP_DATABASE.to_owned(), self.offer.clone()]
    }
}

/// The undo record of `target` run with `input` (the new name or type); `None` when there is no way
/// back (a type change of a column not in the schema read).
pub fn db_undo(target: &RenameTarget, input: &str) -> Option<DbUndo> {
    use ferris_idea_database::Refactoring as R;
    let back = ferris_idea_database::way_back(
        &target.tables,
        &target.table,
        target.column.as_deref(),
        &target.what,
        input,
    )?;
    let kind = target
        .tables
        .iter()
        .find(|t| t.name == target.table)
        .map_or("table", |t| if t.kind == "view" { "view" } else { "table" });
    let keeps = match (&target.what, &target.column) {
        (R::Rename, Some(_)) => format!("the column is still {}.{input}", target.table),
        (R::Rename, None) => format!("the {kind} is still called {input}"),
        (R::RenameIndex(_), _) => format!("the index is still called {input}"),
        (R::RenameRoutine(_), _) => format!("the routine is still called {input}"),
        (R::RenameSequence(_), _) => format!("the sequence is still called {input}"),
        (R::RenameConstraint { kind, .. }, _) => format!("the {kind} is still called {input}"),
        (R::ChangeType, column) => format!(
            "{}.{} is still {input}",
            target.table,
            column.clone().unwrap_or_default()
        ),
    };
    let offer = if target.what == R::ChangeType {
        "Change the type back in the database…"
    } else {
        "Rename back in the database…"
    };
    Some(DbUndo {
        profile: target.profile.clone(),
        back,
        keeps,
        offer: offer.into(),
    })
}

/// What the dialog asks the workbench to do.
pub enum DbRenameEvent {
    /// Run the statement on the connection, then change the files.
    Apply(Box<(RenameTarget, Preview)>),
    /// Close without renaming.
    Close,
}

/// The rename dialog (behaviour 1).
pub struct DbRenameDialog {
    target: RenameTarget,
    files: Vec<SqlFile>,
    field: Entity<EditorView>,
    preview: Result<Preview, String>,
    /// Text the preview was made for.
    previewed: String,
    /// Set while the workbench runs the statement, or after it failed.
    note: Option<(String, bool)>,
    /// Files of another dialect included by a click (labels).
    include: BTreeSet<String>,
    /// Derived names left out by a click (old names).
    leave: BTreeSet<String>,
    focus: FocusHandle,
}

impl EventEmitter<DbRenameEvent> for DbRenameDialog {}

impl DbRenameDialog {
    /// A dialog renaming `target`, with the workspace's SQL `files`.
    pub fn new(target: RenameTarget, files: Vec<SqlFile>, cx: &mut Context<Self>) -> Self {
        let old = target.current();
        let field = cx.new(|cx| {
            let mut f = EditorView::field(cx);
            f.set_text_selected(&old, cx);
            f
        });
        cx.observe(&field, |this: &mut Self, _, cx| this.update_preview(cx))
            .detach();
        let mut dialog = Self {
            target,
            files,
            field,
            preview: Err(String::new()),
            previewed: String::new(),
            note: None,
            include: BTreeSet::new(),
            leave: BTreeSet::new(),
            focus: cx.focus_handle(),
        };
        dialog.update_preview(cx);
        dialog
    }

    /// Focus handle of the name field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus()
    }

    /// The new name typed.
    pub fn new_name(&self, cx: &gpui::App) -> String {
        self.field.read(cx).state.buffer.text().trim().to_owned()
    }

    /// Sets the new name (tests).
    pub fn set_new_name(&mut self, name: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set_text(name, cx));
        self.update_preview(cx);
    }

    /// The dialog's title, e.g. `Rename column stock.qty` (tests).
    pub fn title(&self) -> String {
        self.target.title()
    }

    /// The preview for the name typed, or why there is none.
    pub fn preview(&self) -> Result<&Preview, &str> {
        self.preview.as_ref().map_err(String::as_str)
    }

    /// The text under the preview and whether it is an error.
    pub fn note(&self) -> Option<(&str, bool)> {
        self.note.as_ref().map(|(t, e)| (t.as_str(), *e))
    }

    /// Shows `text` under the preview, as an error or not.
    pub fn set_note(&mut self, text: String, error: bool, cx: &mut Context<Self>) {
        self.note = Some((text, error));
        cx.notify();
    }

    /// The constraint names of the connection, read after the dialog opened (v2).
    pub fn set_constraints(&mut self, constraints: Vec<ConstraintName>, cx: &mut Context<Self>) {
        self.target.constraints = constraints;
        self.refresh(cx);
    }

    /// The schema the connection works in, read after the dialog opened (`SPEC-0164` v5
    /// behaviour 8).
    pub fn set_home_schema(&mut self, schema: String, cx: &mut Context<Self>) {
        self.target.home_schema = Some(schema);
        self.refresh(cx);
    }

    /// Includes or leaves out file `i` of the preview (only files of another dialect and files
    /// that are not the project's own).
    pub fn toggle_file(&mut self, i: usize, cx: &mut Context<Self>) {
        let Ok(p) = &self.preview else { return };
        let Some(f) = p
            .files
            .get(i)
            .filter(|f| f.other_connection.is_none() && (f.other || f.file.outside.is_some()))
        else {
            return;
        };
        let label = f.file.label.clone();
        if !self.include.remove(&label) {
            self.include.insert(label);
        }
        self.refresh(cx);
    }

    /// Renames derived name `i` too, or leaves it.
    pub fn toggle_derived(&mut self, i: usize, cx: &mut Context<Self>) {
        let Ok(p) = &self.preview else { return };
        let Some((d, _)) = p.derived.get(i).filter(|(d, _)| d.ddl.is_some()) else {
            return;
        };
        let old = d.old.clone();
        if !self.leave.remove(&old) {
            self.leave.insert(old);
        }
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.previewed.clear();
        self.preview = Err(String::new());
        self.update_preview(cx);
    }

    fn update_preview(&mut self, cx: &mut Context<Self>) {
        let name = self.new_name(cx);
        if name == self.previewed && self.preview.is_ok() {
            return;
        }
        let old = self.target.current();
        self.preview = if name == old {
            Err(self.prompt().into())
        } else {
            preview(&self.target, &name, &self.files, &self.include, &self.leave)
                .map_err(|e| e.to_string())
        };
        self.previewed = name;
        self.note = None;
        cx.notify();
    }

    /// What the field asks for.
    fn prompt(&self) -> &'static str {
        if self.target.what == Refactoring::ChangeType {
            "Type the new type"
        } else {
            "Type the new name"
        }
    }

    /// Enter: renames when the preview is valid (behaviour 3).
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        self.update_preview(cx);
        if self.target.profile.read_only {
            self.note = Some((
                format!(
                    "Connection `{}` is read-only; nothing was renamed",
                    self.target.profile.name
                ),
                true,
            ));
            cx.notify();
            return;
        }
        if let Ok(p) = &self.preview {
            self.note = Some(("Renaming…".into(), false));
            cx.emit(DbRenameEvent::Apply(Box::new((
                self.target.clone(),
                p.clone(),
            ))));
            cx.notify();
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" => self.apply(cx),
            "escape" => cx.emit(DbRenameEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for DbRenameDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let what = self.target.title();
        let prompt = self.prompt();
        let field_label = if self.target.what == Refactoring::ChangeType {
            "New type"
        } else {
            "New name"
        };
        let error_colour = c(p.status_text(crate::palette::Status::Error));
        let button =
            |id: &'static str, label: &'static str, on: bool| crate::metrics::button(id, label, on);
        let body = match &self.preview {
            Err(e) => div()
                .text_color(if e == prompt {
                    c(p.ui_dim)
                } else {
                    error_colour
                })
                .child(e.clone())
                .into_any_element(),
            Ok(pr) => {
                let summary = format!(
                    "{} place(s) in {} file(s) change{}",
                    pr.edit_count(),
                    pr.files
                        .iter()
                        .filter(|f| f.included && !f.edits.is_empty())
                        .count(),
                    match pr.ambiguous_count() {
                        0 => String::new(),
                        n => format!("; {n} ambiguous place(s) stay as they are"),
                    }
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(c(p.ui_muted)).child("Statement"))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(px(4.))
                            .bg(gpui::rgb(p.background))
                            .font_family(crate::fonts::editor_family())
                            .child(pr.ddl.clone()),
                    )
                    // SPEC-0164 v3 behaviour 12: what it risks, before anything runs.
                    .children(pr.notes.iter().enumerate().map(|(i, n)| {
                        div()
                            .id(("db-rename-note", i))
                            .text_xs()
                            .text_color(if n.data_loss {
                                c(p.status_text(crate::palette::Status::Warning))
                            } else {
                                c(p.ui_dim)
                            })
                            .child(if n.data_loss {
                                format!("⚠ {}", n.text)
                            } else {
                                n.text.clone()
                            })
                    }))
                    .child(div().text_color(c(p.ui_muted)).child(summary))
                    .children(pr.files.iter().enumerate().map(|(i, f)| {
                        let line_of = |b: usize| f.file.text[..b].matches('\n').count() + 1;
                        let mut detail = format!("{} change(s)", f.edits.len());
                        if f.other {
                            detail.push_str(&format!(" · {}", f.dialect.label()));
                        }
                        if let Some(why) = f.file.outside {
                            detail.push_str(&format!(" · {}", why.reason()));
                        }
                        if let Some(name) = &f.other_connection {
                            detail.push_str(&format!(
                                " · bound to {name}, not changed: its database keeps the old name"
                            ));
                        }
                        let optional =
                            f.other_connection.is_none() && (f.other || f.file.outside.is_some());
                        if optional && !f.included {
                            detail.push_str(", click to include");
                        }
                        for (at, reason) in &f.ambiguous {
                            detail.push_str(&format!(" · line {}: {reason}", line_of(*at)));
                        }
                        div()
                            .id(("db-rename-file", i))
                            .flex()
                            .gap_2()
                            .when(optional, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(if f.included { "[x]" } else { "[ ]" })
                            .child(f.file.label.clone())
                            .child(div().text_color(c(p.ui_dim)).child(detail))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_file(i, cx)
                            }))
                    }))
                    .when(!pr.derived.is_empty(), |d| {
                        d.child(
                            div()
                                .text_color(c(p.ui_muted))
                                .child("Names that contain the old name"),
                        )
                    })
                    .children(pr.derived.iter().enumerate().map(|(i, (d, on))| {
                        let detail = match (&d.ddl, &d.reason) {
                            (Some(ddl), _) => ddl.clone(),
                            (None, Some(reason)) => reason.clone(),
                            _ => String::new(),
                        };
                        div()
                            .id(("db-rename-derived", i))
                            .flex()
                            .gap_2()
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(if *on { "[x]" } else { "[ ]" })
                            .child(format!("{} {} → {}", d.kind, d.old, d.new))
                            .child(
                                div()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(c(p.ui_dim))
                                    .child(detail),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_derived(i, cx)
                            }))
                    }))
                    .into_any_element()
            }
        };
        div()
            .id("db-rename-dialog")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(620.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(what))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(90.))
                            .text_color(c(p.ui_muted))
                            .child(field_label),
                    )
                    .child(
                        crate::metrics::field_frame()
                            .flex_1()
                            .child(self.field.clone()),
                    ),
            )
            .child(body)
            .when_some(self.note.clone(), |d, (text, error)| {
                d.child(
                    div()
                        .when(error, |d| d.text_color(error_colour))
                        .when(!error, |d| d.text_color(c(p.ui_muted)))
                        .child(text),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child("Runs the statement on the connection, then changes the files"),
                    )
                    .child(div().flex_1())
                    .child(button("db-rename-cancel", "Cancel", false).on_click(
                        cx.listener(|_, _: &gpui::ClickEvent, _, cx| cx.emit(DbRenameEvent::Close)),
                    ))
                    .child(
                        button("db-rename-do", "Refactor", true).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.apply(cx)),
                        ),
                    ),
            )
    }
}

/// Why a run of the dialog's statement did not change the database, or `None` when it did
/// (`SPEC-0164`, hand test 90 / D32): the database's refusal of a statement, or that no statement
/// ran at all — never a success message for a change that did not happen.
pub fn run_failure(run: &ferris_idea_database::ScriptRun) -> Option<String> {
    ferris_idea_database::run_failure(run)
}

/// Writes `edits` into `path`, a file no editor has open, if it still holds `text`, with a local
/// history revision before and after so Show History has the old text (`SPEC-0164` v3 behaviour
/// 13). `Ok(false)` when the file changed since the preview.
pub fn write_closed_file(
    root: &Path,
    path: &Path,
    text: &str,
    edits: &[TextEdit],
    history: Option<&mut ferris_idea_core::LocalHistory>,
) -> std::io::Result<bool> {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        return Ok(false);
    }
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut history = history;
    if let Some(h) = history.as_deref_mut() {
        let _ = h.record(&relative);
    }
    std::fs::write(path, ferris_idea_database::apply_edits(text, edits))?;
    if let Some(h) = history {
        let _ = h.record(&relative);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_database::{Column, Index, Table};

    /// Hand test 90 (D32): a refusal of the database is reported, and a run in which no statement
    /// ran is not taken for a change; a statement that ran is.
    #[test]
    fn a_refused_or_empty_run_is_no_change() {
        use ferris_idea_database::{DbError, Outcome, ScriptRun, StatementOutcome};
        let one = |outcome| ScriptRun {
            outcomes: vec![StatementOutcome {
                range: 0..10,
                outcome,
                error_offset: None,
                notices: Vec::new(),
            }],
            not_run: 0,
        };
        let refused = one(Outcome::Error(DbError {
            message: "cannot alter type of a column used by a view or rule".into(),
            sqlstate: Some("0A000".into()),
            position: None,
        }));
        assert_eq!(
            run_failure(&refused).as_deref(),
            Some("cannot alter type of a column used by a view or rule")
        );
        assert_eq!(
            run_failure(&ScriptRun::default()).as_deref(),
            Some("no statement was run")
        );
        assert_eq!(run_failure(&one(Outcome::UpdateCount(0))), None);
    }

    fn target() -> RenameTarget {
        let profile = serde_json::from_value(serde_json::json!({
            "id": "pg", "name": "pg", "dialect": "postgresql"
        }))
        .unwrap();
        let col = |n: &str| Column {
            name: n.into(),
            data_type: "integer".into(),
            nullable: true,
            default: None,
            generated: None,
            expression: None,
            identity: None,
            comment: None,
        };
        RenameTarget {
            profile,
            tables: vec![Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name: "ht_b".into(),
                kind: "table".into(),
                columns: vec![col("id"), col("menge")],
                primary_key: vec![],
                foreign_keys: vec![],
                indexes: vec![Index {
                    name: "ht_b_menge_idx".into(),
                    unique: false,
                    columns: vec!["menge".into()],
                    condition: None,
                    collations: Vec::new(),
                    method: None,
                    orders: Vec::new(),
                    prefixes: Vec::new(),
                }],
                unreadable: None,
                constraints: Vec::new(),
                comment: None,
            }],
            table: "ht_b".into(),
            column: Some("menge".into()),
            what: Refactoring::Rename,
            constraints: vec![ConstraintName {
                table: "ht_b".into(),
                name: "ht_b_menge_check".into(),
                kind: "check".into(),
            }],
            home_schema: None,
        }
    }

    /// Handtest 78: the index name in the file's CREATE INDEX stayed old after the rename.
    #[test]
    fn files_follow_derived_index_and_constraint_names() {
        let text = "create table ht_b (id integer, menge integer constraint ht_b_menge_check check (menge > 0));\ncreate index ht_b_menge_idx on ht_b (menge);\nselect 'ht_b_menge_idx' from ht_b;";
        let files = [SqlFile {
            path: PathBuf::from("/w/joins.sql"),
            label: "joins.sql".into(),
            text: text.into(),
            outside: None,
            bound: None,
        }];
        let none = BTreeSet::new();
        let p = preview(&target(), "anzahl", &files, &none, &none).unwrap();
        assert_eq!(p.derived.len(), 2);
        let f = &p.files[0];
        let out = ferris_idea_database::apply_edits(text, &f.edits);
        assert!(
            out.contains("anzahl integer constraint ht_b_anzahl_check check (anzahl > 0)"),
            "{out}"
        );
        assert!(
            out.contains("create index ht_b_anzahl_idx on ht_b (anzahl);"),
            "{out}"
        );
        assert!(out.contains("select 'ht_b_menge_idx' from ht_b"), "{out}");
        // A derived name left out stays in the file too.
        let leave: BTreeSet<String> = ["ht_b_menge_idx".to_owned()].into();
        let p = preview(&target(), "anzahl", &files, &none, &leave).unwrap();
        let out = ferris_idea_database::apply_edits(text, &p.files[0].edits);
        assert!(out.contains("create index ht_b_menge_idx on"), "{out}");
    }

    /// SPEC-0128/AK-8 (v3): files that are not the project's own come after the others, are not
    /// included and do not count until they are included on purpose.
    #[test]
    fn files_outside_the_own_sources_are_listed_last_and_left_out() {
        let file = |label: &str, outside| SqlFile {
            path: PathBuf::from("/w").join(label),
            label: label.into(),
            text: "select menge from ht_b;".into(),
            outside,
            bound: None,
        };
        let files = [
            file(
                ".venv/lib/python3.12/site-packages/x.sql",
                Some(ferris_idea_core::NotOwn::VirtualEnvironment),
            ),
            file("a.sql", None),
            file("local.sql", Some(ferris_idea_core::NotOwn::Ignored)),
            file("z.sql", None),
        ];
        let none = BTreeSet::new();
        let p = preview(&target(), "anzahl", &files, &none, &none).unwrap();
        let rows: Vec<(&str, bool)> = p
            .files
            .iter()
            .map(|f| (f.file.label.as_str(), f.included))
            .collect();
        assert_eq!(
            rows,
            [
                ("a.sql", true),
                ("z.sql", true),
                (".venv/lib/python3.12/site-packages/x.sql", false),
                ("local.sql", false),
            ]
        );
        assert_eq!(p.edit_count(), 2);
        let include: BTreeSet<String> = ["local.sql".to_owned()].into();
        let p = preview(&target(), "anzahl", &files, &include, &none).unwrap();
        assert!(p.files[3].included && !p.files[2].included);
        assert_eq!(p.edit_count(), 3);
    }

    /// SPEC-0164 v4 AK-12 (OBS-0051 L1): renaming on connection `pg` changes the file bound to it
    /// and an unbound one; a file bound to another connection is listed in its dialect, never
    /// included, also when asked to, and counts no change.
    #[test]
    fn files_of_another_connection_are_listed_and_never_changed() {
        let file = |label: &str, bound: Option<(&str, Dialect)>| SqlFile {
            path: PathBuf::from("/w").join(label),
            label: label.into(),
            text: "select menge from ht_b;".into(),
            outside: None,
            bound: bound.map(|(id, d)| (id.to_owned(), format!("{id} db"), d)),
        };
        let files = [
            file("pg.sql", Some(("pg", Dialect::PostgreSql))),
            file("mysql.sql", Some(("my", Dialect::MySql))),
            file("plain.sql", None),
        ];
        let none = BTreeSet::new();
        let p = preview(&target(), "anzahl", &files, &none, &none).unwrap();
        let rows: Vec<(&str, bool, Option<&str>, Dialect)> = p
            .files
            .iter()
            .map(|f| {
                let other = f.other_connection.as_deref();
                (f.file.label.as_str(), f.included, other, f.dialect)
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("pg.sql", true, None, Dialect::PostgreSql),
                ("mysql.sql", false, Some("my db"), Dialect::MySql),
                ("plain.sql", true, None, Dialect::PostgreSql),
            ]
        );
        assert_eq!(p.files[1].edits.len(), 1, "its place is listed");
        assert_eq!(p.edit_count(), 2);
        // Asking for it (as a click would) does not include it.
        let include: BTreeSet<String> = ["mysql.sql".to_owned()].into();
        let p = preview(&target(), "anzahl", &files, &include, &none).unwrap();
        assert!(!p.files[1].included);
        assert_eq!(p.edit_count(), 2);
    }

    /// SPEC-0164 v3 AK-9: the preview says what a statement risks and whether it runs in one
    /// transaction.
    #[test]
    fn the_preview_carries_notes_and_the_transaction() {
        let none = BTreeSet::new();
        let p = preview(&target(), "anzahl", &[], &none, &none).unwrap();
        assert!(p.transaction);
        assert!(p.notes.iter().all(|n| !n.data_loss));
        let mut t = target();
        t.what = Refactoring::ChangeType;
        let p = preview(&t, "bigint", &[], &none, &none).unwrap();
        assert!(p.notes[0].data_loss);
        assert_eq!(p.ddl, "ALTER TABLE ht_b ALTER COLUMN menge TYPE bigint");
        t.profile.dialect = Dialect::MySql;
        let p = preview(&t, "bigint", &[], &none, &none).unwrap();
        assert!(!p.transaction);
        assert!(p.notes[1].text.starts_with("MySQL commits"));
    }

    /// SPEC-0164 v3 AK-10: a closed file gets a history revision before and after the change, and
    /// one that changed since the preview is left alone.
    #[test]
    fn closed_files_get_history_revisions() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-dbr-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("q.sql");
        let text = "select menge from ht_b;";
        std::fs::write(&path, text).unwrap();
        let edits = [TextEdit {
            start: 7,
            end: 12,
            replacement: "anzahl".into(),
        }];
        let mut history = ferris_idea_core::LocalHistory::open(&root).unwrap();
        assert!(write_closed_file(&root, &path, text, &edits, Some(&mut history)).unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "select anzahl from ht_b;"
        );
        let revisions = history.revisions("q.sql");
        assert_eq!(revisions.len(), 2, "{revisions:?}");
        // The old text is still in the history.
        let old = revisions
            .iter()
            .filter_map(|r| history.content("q.sql", r.id).ok().flatten())
            .any(|c| c == text.as_bytes());
        assert!(old);
        // Changed meanwhile: not written.
        assert!(!write_closed_file(&root, &path, text, &edits, Some(&mut history)).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0164 v5 AK-14 (OBS-0070 V3): a sequence literal is one of the preview's places once the
    /// sequence's schema is known; with another schema written it is left alone.
    #[test]
    fn a_sequence_literal_is_a_place_of_the_preview() {
        let text = "insert into pd_note values (nextval('pd_seq'));\n\
                    select currval('public.pd_seq'), nextval('pd_b.pd_seq');";
        let files = [SqlFile {
            path: PathBuf::from("/w/pd.sql"),
            label: "pd.sql".into(),
            text: text.into(),
            outside: None,
            bound: None,
        }];
        let mut t = target();
        t.what = Refactoring::RenameSequence("pd_seq".into());
        let none = BTreeSet::new();
        // The schema not read yet: both qualified literals are listed.
        let p = preview(&t, "pd_seq2", &files, &none, &none).unwrap();
        assert_eq!(p.ddl, "ALTER SEQUENCE pd_seq RENAME TO pd_seq2");
        assert_eq!((p.edit_count(), p.ambiguous_count()), (1, 2));
        t.home_schema = Some("public".into());
        let p = preview(&t, "pd_seq2", &files, &none, &none).unwrap();
        assert_eq!((p.edit_count(), p.ambiguous_count()), (2, 0));
        assert_eq!(
            ferris_idea_database::apply_edits(text, &p.files[0].edits),
            "insert into pd_note values (nextval('pd_seq2'));\n\
             select currval('public.pd_seq2'), nextval('pd_b.pd_seq');"
        );
    }

    /// SPEC-0164 v5 AK-15 (OBS-0070 V4): each refactoring has its way back and says what the
    /// database keeps.
    #[test]
    fn each_refactoring_has_its_way_back() {
        let t = target();
        let undo = db_undo(&t, "anzahl").unwrap();
        assert_eq!(
            undo.note("q.sql"),
            "Undo took back q.sql only: in the database the column is still ht_b.anzahl"
        );
        assert_eq!(
            undo.answers(),
            [KEEP_DATABASE, "Rename back in the database…"]
        );
        assert_eq!(
            (
                undo.back.table.as_str(),
                undo.back.column.as_deref(),
                &undo.back.what,
                undo.back.input.as_str()
            ),
            ("ht_b", Some("anzahl"), &Refactoring::Rename, "menge")
        );
        // A table: renamed back by its new name.
        let mut table = t.clone();
        table.column = None;
        let undo = db_undo(&table, "ht_c").unwrap();
        assert_eq!(undo.keeps, "the table is still called ht_c");
        assert_eq!(
            (undo.back.table.as_str(), undo.back.input.as_str()),
            ("ht_c", "ht_b")
        );
        // An index.
        let mut index = table.clone();
        index.what = Refactoring::RenameIndex("ht_b_menge_idx".into());
        let undo = db_undo(&index, "ht_b_i").unwrap();
        assert_eq!(undo.keeps, "the index is still called ht_b_i");
        assert_eq!(undo.back.what, Refactoring::RenameIndex("ht_b_i".into()));
        // A type change goes back to the old type.
        let mut ty = t.clone();
        ty.what = Refactoring::ChangeType;
        let undo = db_undo(&ty, "bigint").unwrap();
        assert_eq!(undo.keeps, "ht_b.menge is still bigint");
        assert_eq!(undo.offer, "Change the type back in the database…");
        assert_eq!(
            (undo.back.what.clone(), undo.back.input.as_str()),
            (Refactoring::ChangeType, "integer")
        );
        ty.column = Some("gone".into());
        assert!(db_undo(&ty, "bigint").is_none());
    }
}

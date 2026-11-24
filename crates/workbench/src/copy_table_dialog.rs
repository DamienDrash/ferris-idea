//! Copying a table to another schema or name from the Database tool window (`SPEC-0191`): target
//! schema and table, what is kept, a live preview of the statements, and the result in the dialog.

use ferris_idea_database::{
    ConnectionProfile, CopyOptions, CopyReport, CopySource, CopyTarget, copy_statements,
    sequence_note,
};
use gpui::{Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::database_view::RenameTarget;
use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, Role, Semantics};
use crate::table_session::TableSession;
use crate::workbench_view::c;

/// What the dialog tells the workbench.
pub enum CopyTableEvent {
    /// The copy ran; the report says how it went.
    Done(Box<(ConnectionProfile, CopyReport)>),
    /// Closed without copying.
    Close,
}

/// The switches of behaviour 3, with their labels.
const SWITCHES: [(&str, &str); 7] = [
    ("copy-primary-key", "Primary key"),
    ("copy-unique", "Unique constraints"),
    ("copy-not-null", "NOT NULL"),
    ("copy-defaults", "Defaults"),
    ("copy-checks", "CHECK constraints"),
    ("copy-foreign-keys", "Foreign keys"),
    ("copy-rows", "Rows"),
];

fn switch(options: &mut CopyOptions, n: usize) -> &mut bool {
    match n {
        0 => &mut options.primary_key,
        1 => &mut options.unique,
        2 => &mut options.not_null,
        3 => &mut options.defaults,
        4 => &mut options.checks,
        5 => &mut options.foreign_keys,
        _ => &mut options.rows,
    }
}

/// The dialog (behaviour 1).
pub struct CopyTableDialog {
    profile: ConnectionProfile,
    table: String,
    schema: Entity<EditorView>,
    name: Entity<EditorView>,
    options: CopyOptions,
    /// The source as read when the dialog opened (behaviour 4).
    source: Option<Result<CopySource, String>>,
    session: TableSession,
    note: Option<(String, bool)>,
    running: bool,
    focus: FocusHandle,
}

impl EventEmitter<CopyTableEvent> for CopyTableDialog {}

impl CopyTableDialog {
    /// A dialog copying the table of `target`; reads its structure in the background.
    pub fn new(target: RenameTarget, cx: &mut Context<Self>) -> Self {
        let default = CopyTarget::default_for(&target.table, None);
        // The proposed table name is selected: typing replaces it (`SPEC-0335` v2).
        let field = |text: &str, cx: &mut Context<Self>| {
            let text = text.to_owned();
            cx.new(move |cx| {
                let mut f = EditorView::field(cx);
                f.set_text_selected(&text, cx);
                f
            })
        };
        let schema = field("", cx);
        let name = field(&default.table, cx);
        cx.observe(&schema, |this: &mut Self, _, cx| this.schema_changed(cx))
            .detach();
        cx.observe(&name, |_: &mut Self, _, cx| cx.notify())
            .detach();
        let session = TableSession::open(target.profile.clone());
        let table = target.table.clone();
        let rx = session.run(move |rt, s| {
            rt.block_on(ferris_idea_database::read_source(s, &table))
                .map_err(|e| e.to_string())
        });
        let task = cx.background_executor().spawn(async move {
            rx.recv()
                .unwrap_or_else(|_| Err("the connection closed".into()))
        });
        cx.spawn(async move |this, cx| {
            let source = task.await;
            let _ = this.update(cx, |this, cx| {
                this.source = Some(source);
                cx.notify();
            });
        })
        .detach();
        Self {
            profile: target.profile,
            table: target.table,
            schema,
            name,
            options: CopyOptions::default(),
            source: None,
            session,
            note: None,
            running: false,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the target table field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.name.read(cx).focus()
    }

    /// The title, e.g. `Copy Table customers To…`.
    pub fn title(&self) -> String {
        format!("Copy Table {} To…", self.table)
    }

    /// The target as typed.
    pub fn target(&self, cx: &gpui::App) -> CopyTarget {
        let schema = self.schema.read(cx).state.buffer.text().trim().to_owned();
        CopyTarget {
            schema: (!schema.is_empty()).then_some(schema),
            table: self.name.read(cx).state.buffer.text().trim().to_owned(),
        }
    }

    /// The switches.
    pub fn options(&self) -> CopyOptions {
        self.options
    }

    /// Sets the switches (tests).
    pub fn set_options(&mut self, options: CopyOptions, cx: &mut Context<Self>) {
        self.options = options;
        cx.notify();
    }

    /// Sets the target schema and table (tests).
    pub fn set_target(&mut self, schema: &str, table: &str, cx: &mut Context<Self>) {
        self.schema.update(cx, |f, cx| f.set_text(schema, cx));
        self.name.update(cx, |f, cx| f.set_text(table, cx));
    }

    /// A new schema suggests the same table name; clearing it suggests `<table>_copy`.
    fn schema_changed(&mut self, cx: &mut Context<Self>) {
        let schema = self.schema.read(cx).state.buffer.text();
        let name = self.name.read(cx).state.buffer.text();
        let same = format!("{}_copy", self.table);
        let suggested = CopyTarget::default_for(&self.table, Some(&schema)).table;
        if (name == same || name == self.table) && name != suggested {
            self.name.update(cx, |f, cx| f.set_text(&suggested, cx));
        }
        cx.notify();
    }

    /// The statements as they would run, or why there are none yet (behaviour 5).
    pub fn preview(&self, cx: &gpui::App) -> Result<(Vec<String>, Option<String>), String> {
        match &self.source {
            None => Err("Reading the table…".into()),
            Some(Err(e)) => Err(e.clone()),
            Some(Ok(source)) => {
                let target = self.target(cx);
                if target.table.is_empty() {
                    return Err("Type the name of the new table".into());
                }
                Ok((
                    copy_statements(source, &target, self.options, self.profile.dialect),
                    sequence_note(source, self.options),
                ))
            }
        }
    }

    /// The note under the preview (tests).
    pub fn note(&self) -> Option<&(String, bool)> {
        self.note.as_ref()
    }

    /// Shows how the copy went (the workbench closes the dialog after a success).
    pub fn set_note(&mut self, text: String, error: bool, cx: &mut Context<Self>) {
        self.note = Some((text, error));
        self.running = false;
        cx.notify();
    }

    /// Runs the copy (behaviour 6).
    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if self.running || self.preview(cx).is_err() {
            return;
        }
        if self.profile.read_only {
            self.note = Some((
                format!(
                    "Connection `{}` is read-only; nothing was copied",
                    self.profile.name
                ),
                true,
            ));
            cx.notify();
            return;
        }
        let (table, target, options) = (self.table.clone(), self.target(cx), self.options);
        let rx = self.session.run(move |rt, s| {
            Ok(rt.block_on(ferris_idea_database::copy_table(
                s, &table, &target, options,
            )))
        });
        self.running = true;
        self.note = Some(("Copying…".into(), false));
        let profile = self.profile.clone();
        let task = cx.background_executor().spawn(async move {
            rx.recv()
                .unwrap_or_else(|_| Err("the connection closed".into()))
        });
        cx.spawn(async move |this, cx| {
            let report = task.await;
            let _ = this.update(cx, |this, cx| match report {
                Ok(report) => cx.emit(CopyTableEvent::Done(Box::new((profile, report)))),
                Err(e) => this.set_note(e, true, cx),
            });
        })
        .detach();
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" => self.copy(cx),
            "escape" => cx.emit(CopyTableEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for CopyTableDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let error = c(p.status_text(crate::palette::Status::Error));
        let labelled = |label: &'static str, field: Entity<EditorView>| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(110.)).text_color(c(p.ui_muted)).child(label))
                .child(
                    crate::metrics::field_frame()
                        .relative()
                        .semantics(
                            Semantics::new(Role::TextInput)
                                .name(label)
                                .test_id(format!("copy-table-{label}")),
                        )
                        .flex_1()
                        .child(field),
                )
        };
        let switches = SWITCHES.iter().enumerate().map(|(n, (id, label))| {
            let on = *switch(&mut self.options.clone(), n);
            div()
                .id(*id)
                .relative()
                .semantics(
                    Semantics::new(Role::CheckBox)
                        .name(*label)
                        .checked(on)
                        .test_id(*id),
                )
                .px_2()
                .rounded(px(4.))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(format!("{} {label}", if on { "☑" } else { "☐" }))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    let s = switch(&mut this.options, n);
                    *s = !*s;
                    cx.notify();
                }))
        });
        let body = match self.preview(cx) {
            Err(e) => div().text_color(c(p.ui_dim)).child(e).into_any_element(),
            Ok((statements, note)) => div()
                .id("copy-table-preview")
                .max_h(px(260.))
                .overflow_y_scroll()
                .px_2()
                .py_1()
                .rounded(px(4.))
                .bg(gpui::rgb(p.background))
                .font_family(crate::fonts::editor_family())
                .children(
                    statements
                        .join(";\n\n")
                        .lines()
                        .map(|l| div().whitespace_nowrap().child(l.to_owned()))
                        .collect::<Vec<_>>(),
                )
                .when_some(note, |d, n| {
                    d.child(div().mt_1().text_color(c(p.ui_muted)).child(n))
                })
                .into_any_element(),
        };
        div()
            .id("copy-table-dialog")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(680.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.title()),
            )
            .child(labelled("Target schema", self.schema.clone()))
            .child(labelled("Target table", self.name.clone()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(div().w(px(110.)).text_color(c(p.ui_muted)).child("Keep"))
                    .children(switches),
            )
            .child(body)
            .when_some(self.note.clone(), |d, (text, failed)| {
                d.child(
                    div()
                        .when(failed, |d| d.text_color(error))
                        .when(!failed, |d| d.text_color(c(p.ui_muted)))
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
                            .child("Empty schema: the table's own schema"),
                    )
                    .child(div().flex_1())
                    .child(
                        crate::metrics::button("copy-table-cancel", "Cancel", false)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Cancel")
                                    .test_id("copy-table-cancel"),
                            )
                            .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(CopyTableEvent::Close)
                            })),
                    )
                    .child(
                        crate::metrics::button("copy-table-do", "Copy", !self.running)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Copy")
                                    .enabled(!self.running)
                                    .test_id("copy-table-do"),
                            )
                            .on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.copy(cx)),
                            ),
                    ),
            )
    }
}

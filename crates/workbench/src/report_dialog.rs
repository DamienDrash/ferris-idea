//! "Report a problem with this database" (`SPEC-0301`): the report is built locally, shown as text
//! the user edits, then copied or opened as a prefilled issue in the browser. Ferris IDEA sends nothing
//! itself (ADR-0007).

use ferris_idea_database::report::{
    Report, ReportInput, ReportStep, build_report, destination, issue_url,
};
use ferris_idea_database::{ConnectionProfile, Connector};
use gpui::{
    ClipboardItem, Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div,
    prelude::*, px,
};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the dialog asks the workbench to do.
pub enum ReportDialogEvent {
    /// Close the dialog.
    Close,
}

/// The report dialog (behaviour 4).
pub struct ReportDialog {
    connector: Connector,
    profile: Option<ConnectionProfile>,
    server_version: Option<String>,
    step: ReportStep,
    error: String,
    statement: Option<String>,
    include_hosts: bool,
    include_statement: bool,
    /// The setting `database.report_url` (empty: Ferris IDEA's issue template, `off`: copy only).
    setting: String,
    text: Entity<EditorView>,
    note: Option<String>,
    focus: FocusHandle,
}

impl EventEmitter<ReportDialogEvent> for ReportDialog {}

impl ReportDialog {
    /// A report about `connector`, on `profile` if the problem happened on a connection.
    pub fn new(
        connector: Connector,
        profile: Option<ConnectionProfile>,
        step: ReportStep,
        error: String,
        statement: Option<String>,
        setting: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(EditorView::field);
        let mut dialog = Self {
            connector,
            profile,
            server_version: None,
            step,
            error,
            statement,
            include_hosts: false,
            include_statement: false,
            setting,
            text,
            note: None,
            focus: cx.focus_handle(),
        };
        dialog.rebuild(cx);
        dialog
    }

    /// Adds the server version once a session told it, and builds the report again.
    pub fn set_server_version(&mut self, version: Option<String>, cx: &mut Context<Self>) {
        self.server_version = version;
        self.rebuild(cx);
    }

    /// Builds the report from the inputs and the two choices; edits in the text are replaced.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let report = build_report(&ReportInput {
            connector: &self.connector,
            profile: self.profile.as_ref(),
            server_version: self.server_version.clone(),
            step: self.step,
            error: self.error.clone(),
            statement: self.statement.clone(),
            include_hosts: self.include_hosts,
            include_statement: self.include_statement,
        });
        self.text.update(cx, |t, cx| t.set_text(&report.text(), cx));
        cx.notify();
    }

    /// Focus handle of the text.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.text.read(cx).focus()
    }

    /// The report as it stands in the text field: first line title, the rest body.
    pub fn report(&self, cx: &gpui::App) -> Report {
        let text = self.text.read(cx).state.buffer.text();
        let (title, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        Report {
            title: title.trim().to_owned(),
            body: body.trim_start_matches('\n').to_owned(),
        }
    }

    /// The template the browser button opens, if any (behaviour 5).
    pub fn destination(&self) -> Option<String> {
        destination(&self.setting, &self.connector)
    }

    /// Toggles "Include host and database names".
    pub fn toggle_hosts(&mut self, cx: &mut Context<Self>) {
        self.include_hosts = !self.include_hosts;
        self.rebuild(cx);
    }

    /// Toggles "Include the statement".
    pub fn toggle_statement(&mut self, cx: &mut Context<Self>) {
        self.include_statement = !self.include_statement;
        self.rebuild(cx);
    }

    /// The note under the text (what Copy or Open did).
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Copies title and body to the clipboard.
    pub fn copy(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.report(cx).text()));
        self.note = Some("Copied the report to the clipboard.".into());
        cx.notify();
    }

    /// Copies the full report, then opens the destination with title and body in the browser.
    pub fn open_issue(&mut self, cx: &mut Context<Self>) {
        let Some(template) = self.destination() else {
            return;
        };
        let report = self.report(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(report.text()));
        match issue_url(&template, &report) {
            Ok((url, cut)) => {
                cx.open_url(&url);
                self.note = Some(if cut {
                    "Opened the issue form; the body was shortened, the full report is on the clipboard."
                        .into()
                } else {
                    "Opened the issue form in the browser; the report is also on the clipboard."
                        .into()
                });
            }
            Err(e) => self.note = Some(e),
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(ReportDialogEvent::Close);
            cx.stop_propagation();
        }
    }
}

impl Render for ReportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let button =
            |id: &'static str, label: String, on: bool| crate::metrics::button(id, label, on);
        let check = |on: bool, label: &str| format!("{} {label}", if on { "☑" } else { "☐" });
        let destination = self.destination();
        div()
            .id("report-dialog")
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
                    .child(format!("Report a problem with {}", self.connector.name)),
            )
            .child(div().text_xs().text_color(c(p.ui_muted)).child(
                "Nothing is sent by Ferris IDEA. Check and edit the text; passwords, data and your user name are never in it.",
            ))
            .child(
                div()
                    .id("report-text")
                    .h(px(320.))
                    .p_1()
                    .border_1()
                    .border_color(c(p.separator))
                    .rounded(px(4.))
                    .overflow_y_scroll()
                    .child(self.text.clone()),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button(
                            "report-hosts",
                            check(self.include_hosts, "Include host and database names"),
                            false,
                        )
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            this.toggle_hosts(cx)
                        })),
                    )
                    .when(self.statement.is_some(), |d| {
                        d.child(
                            button(
                                "report-statement",
                                check(self.include_statement, "Include the statement"),
                                false,
                            )
                            .on_click(cx.listener(
                                |this, _: &gpui::ClickEvent, _, cx| this.toggle_statement(cx),
                            )),
                        )
                    }),
            )
            .when_some(self.note.clone(), |d, note| {
                d.child(div().text_color(c(p.ui_muted)).child(note))
            })
            .when(destination.is_none(), |d| {
                d.child(div().text_xs().text_color(c(p.ui_dim)).child(
                    "Opening an issue is switched off (setting database.report_url = \"off\"); copy the report instead.",
                ))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        button("report-cancel", "Cancel".into(), false).on_click(cx.listener(
                            |_, _: &gpui::ClickEvent, _, cx| cx.emit(ReportDialogEvent::Close),
                        )),
                    )
                    .child(
                        button("report-copy", "Copy".into(), destination.is_none()).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.copy(cx)),
                        ),
                    )
                    .when(destination.is_some(), |d| {
                        d.child(
                            button("report-open", "Open issue in browser".into(), true).on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.open_issue(cx)
                                }),
                            ),
                        )
                    }),
            )
    }
}

//! "Import database system…" (`SPEC-0302` behaviours 1–3): a file path or an `https://` URL is
//! read, validated and shown with every query it would run; nothing is stored before Save.

use ferris_idea_database::connector_share::{Review, review};
use gpui::{Context, Entity, EventEmitter, FocusHandle, Window, div, prelude::*, px};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the panel tells the connection dialog.
pub enum ConnectorImportEvent {
    /// A system was stored; its id.
    Imported(String),
    /// Closed without storing.
    Cancel,
}

/// The import panel inside the connection dialog.
pub struct ConnectorImport {
    source: Entity<EditorView>,
    review: Option<Review>,
    /// Where the text came from (path or URL), recorded with the import.
    origin: String,
    /// Loading, or why loading or saving failed.
    note: Option<(String, bool)>,
    focus: FocusHandle,
}

impl EventEmitter<ConnectorImportEvent> for ConnectorImport {}

/// Reads `source`: an `https://` URL through the small fetcher (10 s, 64 KiB, 3 redirects), else a
/// file path (a leading `~/` is the home folder).
pub fn read_source(source: &str) -> Result<(String, String), String> {
    let source = source.trim();
    if source.starts_with("http://") {
        return Err("only https:// addresses are fetched".into());
    }
    if source.starts_with("https://") {
        let (url, text) = ferris_idea_http::fetch_small_text(
            source,
            ferris_idea_database::connector::MAX_DEFINITION_BYTES,
            3,
            std::time::Duration::from_secs(10),
        )?;
        return Ok((url, text));
    }
    let path = match source.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(rest))
            .ok_or("no home folder")?,
        None => std::path::PathBuf::from(source),
    };
    let size = std::fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size > ferris_idea_database::connector::MAX_DEFINITION_BYTES as u64 {
        return Err("the file is larger than 64 KiB".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path.display().to_string(), text))
}

impl ConnectorImport {
    /// An empty panel.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            source: cx.new(EditorView::field),
            review: None,
            origin: String::new(),
            note: None,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the source field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.source.read(cx).focus()
    }

    /// Types `text` into the source field (tests, pasting).
    pub fn set_source(&mut self, text: &str, cx: &mut Context<Self>) {
        self.source.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// The review of the loaded definition.
    pub fn review(&self) -> Option<&Review> {
        self.review.as_ref()
    }

    /// The note under the panel and whether it is an error.
    pub fn note(&self) -> Option<&(String, bool)> {
        self.note.as_ref()
    }

    /// Reads the source in the background and reviews it (behaviours 1, 2).
    pub fn load(&mut self, cx: &mut Context<Self>) {
        let source = self.source.read(cx).state.buffer.text();
        if source.trim().is_empty() {
            self.note = Some(("Type a file path or an https:// address".into(), true));
            cx.notify();
            return;
        }
        self.note = Some(("Reading…".into(), false));
        self.review = None;
        let task = cx
            .background_executor()
            .spawn(async move { read_source(&source) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((origin, text)) => this.show(origin, &text),
                    Err(e) => this.note = Some((e, true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Shows the review of `text` read from `origin`.
    pub fn show(&mut self, origin: String, text: &str) {
        let r = review(text, &ferris_idea_database::reload_catalogue());
        self.note = if r.ok() {
            Some((
                match &r.replaces {
                    Some(name) => format!("Saving replaces {name}."),
                    None => "Check the queries below, then save.".into(),
                },
                false,
            ))
        } else {
            Some((
                format!(
                    "{} problem(s); fix the file and load it again.",
                    r.problems.len()
                ),
                true,
            ))
        };
        self.origin = origin;
        self.review = Some(r);
    }

    /// Stores the reviewed definition (behaviour 3).
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.review.as_ref().filter(|r| r.ok()) else {
            return;
        };
        let Some(dir) = ferris_idea_database::connector::connectors_dir() else {
            self.note = Some(("No settings folder".into(), true));
            cx.notify();
            return;
        };
        let catalogue = ferris_idea_database::reload_catalogue();
        match ferris_idea_database::connector_share::import(
            &dir,
            &r.text,
            &self.origin,
            r.replaces.is_some(),
            &catalogue,
        ) {
            Ok(_) => {
                ferris_idea_database::reload_catalogue();
                let id = r
                    .connector
                    .as_ref()
                    .map(|c| c.id.clone())
                    .unwrap_or_default();
                cx.emit(ConnectorImportEvent::Imported(id));
            }
            Err(e) => {
                self.note = Some((e.to_string(), true));
                cx.notify();
            }
        }
    }
}

impl Render for ConnectorImport {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let button =
            |id: &'static str, label: String, on: bool| crate::metrics::button(id, label, on);
        let ok = self.review.as_ref().is_some_and(Review::ok);
        let save_label = match self.review.as_ref().and_then(|r| r.replaces.as_ref()) {
            Some(name) => format!("Replace {name}"),
            None => "Save".into(),
        };
        let details = self.review.as_ref().map(|r| {
            let summary = r.connector.as_ref().map(|k| {
                format!(
                    "{} (`{}`) · family {} · {} · schemes {} · port {}",
                    k.name,
                    k.id,
                    k.family.name(),
                    k.category.label(),
                    k.connection.schemes.join(", "),
                    k.connection
                        .default_port
                        .map_or_else(|| "—".to_owned(), |p| p.to_string())
                )
            });
            div()
                .id("connector-import-review")
                .max_h(px(260.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .when_some(summary, |d, s| d.child(div().child(s)))
                .children(r.problems.iter().map(|problem| {
                    div()
                        .text_color(c(p.status_text(crate::palette::Status::Error)))
                        .child(problem.to_string())
                }))
                .when(!r.queries.is_empty(), |d| {
                    d.child(
                        div()
                            .pt_1()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Queries this system will run on your connections"),
                    )
                })
                .children(r.queries.iter().map(|(key, sql)| {
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_xs().text_color(c(p.ui_dim)).child(key.clone()))
                        .child(div().font_family("monospace").text_xs().child(sql.clone()))
                }))
        });
        div()
            .id("connector-import")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(6.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Import database system"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        crate::metrics::field_frame()
                            .flex_1()
                            .child(self.source.clone()),
                    )
                    .child(
                        button("connector-import-load", "Load".into(), false).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.load(cx)),
                        ),
                    ),
            )
            .child(div().text_xs().text_color(c(p.ui_dim)).child(
                "A .idcconnector or .toml file, or an https:// address; nothing is stored before Save.",
            ))
            .when_some(self.note.clone(), |d, (text, error)| {
                d.child(
                    div()
                        .when(error, |d| {
                            d.text_color(c(p.status_text(crate::palette::Status::Error)))
                        })
                        .when(!error, |d| d.text_color(c(p.ui_muted)))
                        .child(text),
                )
            })
            .when_some(details, |d, details| d.child(details))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        button("connector-import-cancel", "Cancel".into(), false).on_click(
                            cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(ConnectorImportEvent::Cancel)
                            }),
                        ),
                    )
                    .when(ok, |d| {
                        d.child(
                            button("connector-import-save", save_label, true).on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.save(cx)),
                            ),
                        )
                    }),
            )
    }
}

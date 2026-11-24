//! The Endpoints tool window (`SPEC-0103`): the HTTP endpoints declared in the project's code; Enter
//! opens the declaration, "Request" writes a request for it into `.ferris-idea/generated-requests.http`.

use std::path::{Path, PathBuf};

use ferris_idea_languages::endpoints::{Endpoint, EndpointKind, project_endpoints};
use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::palette::Palette;
use crate::tips::Tipped;
use crate::workbench_view::c;

/// The file generated requests are added to, workspace-relative.
pub const GENERATED_REQUESTS: &str = ".ferris-idea/generated-requests.http";

/// What the window asks the workbench to do.
pub enum EndpointsEvent {
    /// Hide the window.
    Hide,
    /// Open this file at this 0-based line.
    Open(PathBuf, usize),
}

/// The window.
pub struct EndpointsView {
    root: PathBuf,
    list: Vec<Endpoint>,
    selected: usize,
    loading: bool,
    focus: FocusHandle,
}

impl EventEmitter<EndpointsEvent> for EndpointsView {}

pub use ferris_idea_languages::endpoints::request_text;

impl EndpointsView {
    /// A window for the workspace at `root`; scans it in the background.
    pub fn new(root: &Path, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            root: root.to_path_buf(),
            list: Vec::new(),
            selected: 0,
            loading: false,
            focus: cx.focus_handle(),
        };
        view.reload(cx);
        view
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Scans the project again (behaviour 1).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        cx.notify();
        let root = self.root.clone();
        // Only the project's own sources (SPEC-0103 v2 behaviour 5).
        let task = cx
            .background_executor()
            .spawn(async move { project_endpoints(&root) });
        cx.spawn(async move |this, cx| {
            let list = task.await;
            let _ = this.update(cx, |this, cx| {
                this.list = list;
                this.loading = false;
                this.selected = this.selected.min(this.list.len().saturating_sub(1));
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether a scan is running.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// The rows as `METHODS path handler` (for tests): several methods joined with `|`, none for
    /// a mapping without a method, a client row marked ` (client)` (`SPEC-0103` v4 behaviour 7).
    pub fn texts(&self) -> Vec<String> {
        self.list.iter().map(Endpoint::row_text).collect()
    }

    /// Selects the endpoint whose text starts with `prefix` (for tests).
    pub fn select_text(&mut self, prefix: &str) -> bool {
        match self.texts().iter().position(|t| t.starts_with(prefix)) {
            Some(i) => {
                self.selected = i;
                true
            }
            None => false,
        }
    }

    /// Opens the selected endpoint's declaration (Enter, double click; behaviour 2).
    pub fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = self.list.get(self.selected) {
            cx.emit(EndpointsEvent::Open(self.root.join(&e.file), e.line));
        }
    }

    /// Adds a request for the selected endpoint to the generated requests file and opens it there
    /// (behaviour 3). A client row has none: another service serves its path (v4 behaviour 7).
    pub fn generate_request(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(e) = self.list.get(self.selected) else {
            return Err("No endpoint selected".into());
        };
        let Some(request) = request_text(&self.root, e) else {
            return Err(
                "A client interface calls another service; there is no request for it".into(),
            );
        };
        let file = self.root.join(GENERATED_REQUESTS);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        if !text.is_empty() && !text.ends_with("\n\n") {
            text.push('\n');
        }
        let line = text.lines().count();
        text.push_str(&request);
        std::fs::write(&file, &text).map_err(|e| e.to_string())?;
        cx.emit(EndpointsEvent::Open(file, line + 1));
        Ok(())
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let n = self.list.len();
        // The first and the last endpoint (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(&event.keystroke, n) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match event.keystroke.key.as_str() {
            "down" if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "enter" => self.open_selected(cx),
            "escape" => cx.emit(EndpointsEvent::Hide),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for EndpointsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let rows = self.list.clone();
        div()
            .id("endpoints-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title("Endpoints"))
                    .child(div().text_color(c(p.ui_muted)).child(if self.loading {
                        "Scanning…".to_owned()
                    } else {
                        format!("{} endpoints", rows.len())
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("endpoints-request")
                            .px_2()
                            .py(px(2.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child("Generate Request")
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                let _ = this.generate_request(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("endpoints-reload")
                            .tip(crate::tips::declared!(OTHER_TOOL_WINDOWS, "endpoints-reload"))
                            .p(px(2.))
                            .rounded(px(4.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(crate::icon_images::icon("general/refresh"))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.reload(cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("endpoints-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .px_1()
                    .when(rows.is_empty() && !self.loading, |d| {
                        d.child(div().px_3().py_2().text_color(c(p.ui_muted)).child(
                            "No endpoints found (Spring, JAX-RS, Micronaut, NestJS, FastAPI, Flask, Express)",
                        ))
                    })
                    .children(rows.into_iter().enumerate().map(|(i, e)| {
                        let on = i == selected;
                        let file = format!(
                            "{}:{}",
                            e.file
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            e.line + 1
                        );
                        div()
                            .id(("endpoint", i))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            // Every method of the mapping in one row, nothing for a mapping
                            // without a method (SPEC-0103 v4 behaviour 7).
                            .child(
                                div()
                                    .min_w(px(64.))
                                    .flex_none()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(e.method_label()),
                            )
                            .child(div().flex_none().child(e.path.clone()))
                            .child(div().flex_none().text_color(c(p.ui_muted)).child(e.handler.clone()))
                            .when(e.kind == EndpointKind::Client, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .px_1()
                                        .rounded(px(3.))
                                        .border_1()
                                        .border_color(c(p.separator))
                                        .text_color(c(p.ui_muted))
                                        .child("client"),
                                )
                            })
                            .child(div().flex_1())
                            .child(div().flex_none().text_color(c(p.ui_dim)).child(format!("{} · {file}", e.framework)))
                            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                if ev.click_count() > 1 {
                                    this.open_selected(cx);
                                }
                                cx.notify();
                            }))
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_for_an_endpoint() {
        let mut e = Endpoint {
            framework: "FastAPI",
            kind: EndpointKind::Server,
            methods: Vec::new(),
            path: "/users/{uid}".into(),
            handler: "read_user".into(),
            file: PathBuf::from("users.py"),
            line: 3,
        };
        assert_eq!(
            request_text(Path::new("/nonexistent"), &e).as_deref(),
            Some("### read_user\nGET http://localhost:8000/users/{uid}\n")
        );
        // Several methods: the first; a client row: none (SPEC-0103 v4 behaviour 7).
        e.methods = vec!["POST".into(), "PUT".into()];
        assert_eq!(
            request_text(Path::new("/nonexistent"), &e).as_deref(),
            Some("### read_user\nPOST http://localhost:8000/users/{uid}\n")
        );
        e.kind = EndpointKind::Client;
        assert_eq!(request_text(Path::new("/nonexistent"), &e), None);
    }
}

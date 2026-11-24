//! The Attach to Process dialog (`XDebugger.AttachToProcess`, ⌥⇧F5; `SPEC-0147`): the local
//! processes with their runtime, filtered as you type; a typed `host:port` attaches to a debug port.

use ferris_idea_debugger::{Process, Runtime, Target};
use gpui::{Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the dialog asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachEvent {
    /// Attach this runtime's debugger to the target; the string names it for the Debug window.
    Attach(Runtime, Target, String),
    /// Attach to a container or pod on the host of a remote project (`SPEC-0322` behaviour 17):
    /// runtime, `attach` parameter, folder in the target and the label.
    AttachHost(Runtime, serde_json::Value, Option<String>, String),
    /// Close without attaching.
    Close,
}

/// A row of the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachRow {
    /// The debugger it uses.
    pub runtime: Runtime,
    /// What it attaches to; `None` when it cannot be attached (a JVM without JDWP agent).
    pub target: Option<Target>,
    /// Main text: the process name or the address.
    pub title: String,
    /// Dim text after it: process id, port, command line.
    pub detail: String,
    /// A container or pod target of a remote project, with the folder in it.
    pub host_target: Option<(serde_json::Value, Option<String>)>,
}

/// The rows for `filter` (behaviours 1 and 2): a `host:port` (or `:port`) first offers each
/// runtime for that address, then the processes whose name, command line or id contain the
/// filter (case-insensitive), as [`ferris_idea_debugger::processes`] ordered them.
pub fn attach_rows(processes: &[Process], filter: &str) -> Vec<AttachRow> {
    let filter = filter.trim();
    let mut rows = Vec::new();
    if let Some(target) = Target::parse_address(filter) {
        // A Delve server is talked to on this machine only: its port is not protected
        // (SPEC-0147 v3 behaviour 7).
        let local = matches!(&target, Target::Address { host, .. } if ferris_idea_debugger::is_loopback(host));
        let offered = Runtime::ALL
            .iter()
            .filter(|&&runtime| runtime != Runtime::Go || local);
        rows.extend(offered.map(|&runtime| AttachRow {
            runtime,
            title: target.to_string(),
            detail: match runtime {
                Runtime::Java => "JDWP".into(),
                Runtime::Python => "debugpy --listen".into(),
                Runtime::Node => "node --inspect".into(),
                Runtime::Native => "gdb-remote server".into(),
                Runtime::Go => "dlv --headless".into(),
            },
            target: Some(target.clone()),
            host_target: None,
        }));
    }
    if let Some((attach, folder)) = ferris_idea_debugger::parse_host_target(filter) {
        // Not Go: its programs in a container or pod are not attached to (SPEC-0147 v3).
        let offered = Runtime::ALL.iter().filter(|&&r| r != Runtime::Go);
        rows.extend(offered.map(|&runtime| AttachRow {
            runtime,
            title: filter.to_owned(),
            detail: format!("container or pod ({})", runtime.label()),
            target: None,
            host_target: Some((attach.clone(), folder.clone())),
        }));
    }
    let needle = filter.to_lowercase();
    rows.extend(
        processes
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || p.name.to_lowercase().contains(&needle)
                    || p.command.to_lowercase().contains(&needle)
                    || p.pid.to_string() == needle
            })
            .map(|p| {
                let port = match (p.debug_port, p.runtime) {
                    (Some(port), _) => format!("  port {port}"),
                    (None, Runtime::Java) => "  no JDWP agent".into(),
                    (None, Runtime::Go) => "  no listen address".into(),
                    _ => String::new(),
                };
                AttachRow {
                    runtime: p.runtime,
                    target: p.target(),
                    title: p.name.clone(),
                    detail: format!("{}{port}  {}", p.pid, p.command),
                    host_target: None,
                }
            }),
    );
    rows
}

/// Rows shown at most; the filter narrows a longer list.
const SHOWN: usize = 200;

/// The dialog.
pub struct AttachView {
    processes: Vec<Process>,
    field: Entity<EditorView>,
    rows: Vec<AttachRow>,
    selected: usize,
    note: Option<String>,
    /// The project is on a host: container and pod targets are possible (`SPEC-0322`).
    remote: bool,
    _watch: gpui::Subscription,
}

impl EventEmitter<AttachEvent> for AttachView {}

impl AttachView {
    /// A dialog over `processes`.
    pub fn new(processes: Vec<Process>, cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        let watch = cx.observe(&field, |this, _, cx| this.refilter(cx));
        let rows = attach_rows(&processes, "");
        Self {
            processes,
            field,
            rows,
            selected: 0,
            note: None,
            remote: false,
            _watch: watch,
        }
    }

    /// A dialog over the processes of a remote project's host (`SPEC-0322` behaviour 17).
    pub fn new_remote(processes: Vec<Process>, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(processes, cx);
        view.remote = true;
        view
    }

    /// Focus handle of the filter field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus()
    }

    /// The rows as filtered now.
    pub fn rows(&self) -> &[AttachRow] {
        &self.rows
    }

    /// Index of the selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Why Enter did nothing, if it did nothing.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Sets the filter (tests).
    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set_text(text, cx));
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).state.buffer.text();
        self.rows = attach_rows(&self.processes, &text);
        self.selected = 0;
        self.note = None;
        cx.notify();
    }

    /// Selects row `i`.
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = i.min(self.rows.len().saturating_sub(1));
        cx.notify();
    }

    /// Attaches to the selected row, or says why it cannot (behaviour 3).
    pub fn choose(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(self.selected) else {
            self.note = Some("No process matches".into());
            cx.notify();
            return;
        };
        if let Some((attach, folder)) = &row.host_target {
            if !self.remote {
                self.note = Some("Container and pod targets need a remote project".into());
                cx.notify();
                return;
            }
            let label = format!("{} ({})", row.title, row.runtime.label());
            cx.emit(AttachEvent::AttachHost(
                row.runtime,
                attach.clone(),
                folder.clone(),
                label,
            ));
            return;
        }
        match &row.target {
            Some(target) => {
                let label = match target {
                    Target::Pid(pid) => format!("{} ({pid})", row.title),
                    Target::Address { .. } => format!("{} ({})", row.title, row.runtime.label()),
                };
                cx.emit(AttachEvent::Attach(row.runtime, target.clone(), label));
            }
            None => {
                self.note = Some(
                    "This JVM has no JDWP agent: start it with \
                     -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005 \
                     and type :5005"
                        .into(),
                );
                cx.notify();
            }
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, self.rows.len().min(SHOWN)) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        let n = self.rows.len().min(SHOWN);
        match k.key.as_str() {
            "down" if n > 0 => self.selected = (self.selected + 1) % n,
            "up" if n > 0 => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose(cx),
            "escape" => cx.emit(AttachEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for AttachView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let list = div()
            .id("attach-rows")
            .max_h(px(380.))
            .overflow_y_scroll()
            .children(self.rows.iter().take(SHOWN).enumerate().map(|(i, row)| {
                div()
                    .id(("attach-row", i))
                    .h(px(24.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                    .when(row.target.is_none(), |d| d.text_color(c(p.ui_dim)))
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .text_color(c(p.ui_muted))
                            .child(row.runtime.label()),
                    )
                    .child(div().flex_none().child(row.title.clone()))
                    .child(div().text_color(c(p.ui_dim)).child(row.detail.clone()))
                    .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                        this.select(i, cx);
                        if e.click_count() > 1 {
                            this.choose(cx);
                        }
                    }))
            }));
        div()
            .id("attach-dialog")
            .occlude()
            .w(px(720.))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .capture_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .text_color(c(p.ui_muted))
                    .child("Attach to Process (filter, or host:port of a debug port)"),
            )
            .child(
                crate::metrics::field_frame()
                    .m_2()
                    // The field needs a height of its own, or typed text is not shown (hand test 73).
                    .child(
                        div()
                            .flex_1()
                            .h(px(18.))
                            .overflow_hidden()
                            .child(self.field.clone()),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .py_1()
                    .child(list),
            )
            .when_some(self.note.clone(), |d, n| {
                d.child(div().px_3().pb_1().text_color(c(p.ui_muted)).child(n))
            })
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Enter attaches · Stop detaches, the program goes on · Escape closes"),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(
        pid: u32,
        name: &str,
        command: &str,
        runtime: Runtime,
        port: Option<u16>,
    ) -> Process {
        Process {
            pid,
            name: name.into(),
            command: command.into(),
            runtime,
            debug_port: port,
        }
    }

    #[test]
    fn rows_filter_by_name_command_and_id() {
        let list = [
            process(
                10,
                "java",
                "java -agentlib:jdwp=…:5005 Main",
                Runtime::Java,
                Some(5005),
            ),
            process(11, "java", "java Other", Runtime::Java, None),
            process(12, "server", "./server --port 80", Runtime::Native, None),
        ];
        assert_eq!(attach_rows(&list, "").len(), 3);
        let rows = attach_rows(&list, "SERVER");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, Some(Target::Pid(12)));
        let rows = attach_rows(&list, "11");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, None);
        assert!(rows[0].detail.contains("no JDWP agent"));
        assert!(attach_rows(&list, "main")[0].detail.contains("port 5005"));
    }

    #[test]
    fn an_address_offers_each_runtime() {
        let rows = attach_rows(&[], ":5005");
        assert_eq!(
            rows.iter().map(|r| r.runtime).collect::<Vec<_>>(),
            Runtime::ALL
        );
        assert!(rows.iter().all(|r| r.title == "127.0.0.1:5005"));
        assert_eq!(rows[4].detail, "dlv --headless");
        // A Delve server on another machine is not offered: its port is not protected.
        let far = attach_rows(&[], "build.example:2345");
        assert_eq!(far.len(), 4);
        assert!(far.iter().all(|r| r.runtime != Runtime::Go));
        // Nor a Go program in a container.
        let container = attach_rows(&[], "docker:web:2345");
        assert!(!container.is_empty());
        assert!(container.iter().all(|r| r.runtime != Runtime::Go));
    }
}

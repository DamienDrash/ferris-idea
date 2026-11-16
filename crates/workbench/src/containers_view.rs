//! The Containers tool window (`SPEC-0119`): Docker containers and images, Kubernetes workloads of
//! one context and namespace, and the logs of what is selected; shells (`SPEC-0240`), ports and
//! port forwards (`SPEC-0241`), Compose services (`SPEC-0242`) and engines (`SPEC-0243`).

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, rgb};
use idecraft_containers::compose::{ComposeAction, ComposeProject, ServiceState};
use idecraft_containers::ports::ForwardInfo;
use idecraft_containers::{Container, ContainerAction, DockerInfo, Image, KubeResource};

use crate::palette::Palette;
use crate::workbench_view::c;

/// Something logs can be shown for or an action applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerTarget {
    /// A Docker container by name.
    Docker(String),
    /// A Kubernetes object by kind and name.
    Kube(String, String),
}

/// What the window asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainersEvent {
    /// Hide the window.
    Hide,
    /// Read everything again.
    Refresh,
    /// Show the logs of the target.
    Logs(ContainerTarget),
    /// Start, stop, restart or remove a container; restart a Kubernetes workload.
    Act(ContainerTarget, ContainerAction),
    /// Use the next Kubernetes context.
    NextContext,
    /// Use the next namespace of the context.
    NextNamespace,
    /// Open a shell in the target; for Docker, whether the container runs.
    Exec(ContainerTarget, bool),
    /// Open an address in the browser, or say why there is none.
    Open(Result<String, String>),
    /// Forward the first port of a Kubernetes object: kind, name, port.
    Forward(String, String, u16),
    /// Stop port forward `id`.
    StopForward(u32),
    /// Compose up or down for the project (`None`) or one service.
    Compose(ComposeAction, Option<String>),
    /// Show the logs of a Compose service.
    ServiceLogs(String),
    /// Use the next Docker engine.
    NextEngine,
}

/// An engine as the window lists it: name, address, note, whether it is used.
pub type EngineRow = (String, String, Option<String>, bool);

/// The Compose project of the folder with its services' states.
pub type ComposeView = (ComposeProject, Vec<ServiceState>);

/// What Docker answered: host label, version, containers, images; or why it failed.
pub type DockerView = Result<(String, DockerInfo, Vec<Container>, Vec<Image>), String>;

/// What Kubernetes answered: context, namespace, resources; or why it failed.
pub type KubeView = Result<(String, String, Vec<KubeResource>), String>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Heading(String),
    Note(String, bool),
    Container(Container),
    Image(Image),
    Kube(KubeResource),
    Engine(EngineRow),
    Compose(ComposeProject),
    Service(ServiceState),
    Forward(ForwardInfo),
}

/// The window.
pub struct ContainersView {
    docker: Option<DockerView>,
    kube: Option<KubeView>,
    rows: Vec<Row>,
    selected: usize,
    removing: Option<String>,
    logs: Option<(String, String)>,
    engines: Vec<EngineRow>,
    compose: Option<ComposeView>,
    forwards: Vec<ForwardInfo>,
    focus: FocusHandle,
}

impl EventEmitter<ContainersEvent> for ContainersView {}

fn size_label(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / f64::from(1u32 << 30)),
        b if b >= 1 << 20 => format!("{:.0} MB", b as f64 / f64::from(1u32 << 20)),
        b => format!("{:.0} kB", b as f64 / 1024.0),
    }
}

impl ContainersView {
    /// An empty window, reading.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            docker: None,
            kube: None,
            rows: Vec::new(),
            selected: 0,
            removing: None,
            logs: None,
            engines: Vec::new(),
            compose: None,
            forwards: Vec::new(),
            focus: cx.focus_handle(),
        };
        v.rebuild();
        v
    }

    fn rebuild(&mut self) {
        let before = self.rows.get(self.selected).and_then(row_key);
        let mut rows = vec![];
        let others: Vec<Row> = self
            .engines
            .iter()
            .filter(|e| !e.3)
            .cloned()
            .map(Row::Engine)
            .collect();
        let warning = self.engines.iter().find(|e| e.3).and_then(|e| e.2.clone());
        match &self.docker {
            None => rows.push(Row::Heading("Docker — reading…".into())),
            Some(Err(e)) => {
                rows.push(Row::Heading("Docker".into()));
                rows.push(Row::Note(e.clone(), true));
                rows.extend(others);
            }
            Some(Ok((host, info, containers, images))) => {
                rows.push(Row::Heading(format!(
                    "Docker {} — {host} ({})",
                    info.version, info.platform
                )));
                if let Some(w) = warning {
                    rows.push(Row::Note(format!("Warning: {w}"), true));
                }
                rows.extend(others);
                if let Some((project, states)) = &self.compose {
                    rows.push(Row::Compose(project.clone()));
                    if let Some(e) = &project.error {
                        rows.push(Row::Note(e.clone(), true));
                    }
                    rows.extend(states.iter().cloned().map(Row::Service));
                }
                rows.push(Row::Heading(format!("Containers ({})", containers.len())));
                rows.extend(containers.iter().cloned().map(Row::Container));
                rows.push(Row::Heading(format!("Images ({})", images.len())));
                rows.extend(images.iter().cloned().map(Row::Image));
            }
        }
        match &self.kube {
            None => rows.push(Row::Heading("Kubernetes — reading…".into())),
            Some(Err(e)) => {
                rows.push(Row::Heading("Kubernetes".into()));
                rows.push(Row::Note(e.clone(), true));
            }
            Some(Ok((context, namespace, resources))) => {
                rows.push(Row::Heading(format!(
                    "Kubernetes — {context} / {namespace} ({})",
                    resources.len()
                )));
                if !self.forwards.is_empty() {
                    rows.push(Row::Heading(format!(
                        "Port forwards ({})",
                        self.forwards.len()
                    )));
                    rows.extend(self.forwards.iter().cloned().map(Row::Forward));
                }
                if resources.is_empty() {
                    rows.push(Row::Note(
                        "No workloads or services in this namespace".into(),
                        false,
                    ));
                }
                rows.extend(resources.iter().cloned().map(Row::Kube));
            }
        }
        self.rows = rows;
        self.selected = reselect(before.as_deref(), &self.rows, self.selected);
    }

    /// Shows the engines (behaviour 1 of SPEC-0243): name, address, note, whether used.
    pub fn show_engines(&mut self, engines: Vec<EngineRow>, cx: &mut Context<Self>) {
        self.engines = engines;
        self.rebuild();
        cx.notify();
    }

    /// Shows the folder's Compose project and its services, if it has one.
    pub fn show_compose(&mut self, compose: Option<ComposeView>, cx: &mut Context<Self>) {
        self.compose = compose;
        self.rebuild();
        cx.notify();
    }

    /// Shows the running port forwards.
    pub fn show_forwards(&mut self, forwards: Vec<ForwardInfo>, cx: &mut Context<Self>) {
        self.forwards = forwards;
        self.rebuild();
        cx.notify();
    }

    /// Shows what Docker answered: the host label with version, containers and images, or why
    /// it could not be reached.
    pub fn show_docker(&mut self, docker: DockerView, cx: &mut Context<Self>) {
        self.docker = Some(docker);
        self.rebuild();
        cx.notify();
    }

    /// Shows the Kubernetes context, namespace and resources, or why they could not be read.
    pub fn show_kube(&mut self, kube: KubeView, cx: &mut Context<Self>) {
        self.kube = Some(kube);
        self.rebuild();
        cx.notify();
    }

    /// Shows `text` as the logs of `title`.
    pub fn show_logs(&mut self, title: String, text: String, cx: &mut Context<Self>) {
        self.logs = Some((title, text));
        cx.notify();
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The rows as text (for tests).
    pub fn texts(&self) -> Vec<String> {
        self.rows.iter().map(row_text).collect()
    }

    /// The logs shown: title and text.
    pub fn logs(&self) -> Option<&(String, String)> {
        self.logs.as_ref()
    }

    /// The container waiting for a second Delete.
    pub fn removing(&self) -> Option<&str> {
        self.removing.as_deref()
    }

    /// Text of the selected row.
    pub fn selected_text(&self) -> Option<String> {
        self.rows.get(self.selected).map(row_text)
    }

    /// Selects the first row whose text contains `part`; false when there is none.
    pub fn select_text(&mut self, part: &str) -> bool {
        match self.texts().iter().position(|t| t.contains(part)) {
            Some(i) => {
                self.selected = i;
                self.removing = None;
                true
            }
            None => false,
        }
    }

    fn target(&self) -> Option<ContainerTarget> {
        match self.rows.get(self.selected)? {
            Row::Container(c) => Some(ContainerTarget::Docker(c.name.clone())),
            Row::Kube(r) => Some(ContainerTarget::Kube(r.kind.clone(), r.name.clone())),
            _ => None,
        }
    }

    /// Handles one key as the window does (for tests too): Enter logs, `s` start, `x` stop (a
    /// forward: stop it), `r` restart, Delete twice removes a container (once stops a forward),
    /// `e` shell (on a heading: next engine), `o` open in the browser, `f` forward a port, `u`/`d`
    /// Compose up/down, `c`/`n` next context/namespace, F5 refresh.
    pub fn press(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let n = self.rows.len();
        let row = self.rows.get(self.selected).cloned();
        let target = self.target();
        if key != "delete" && key != "backspace" {
            self.removing = None;
        }
        match (key, row) {
            ("down", _) if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            ("up", _) => self.selected = self.selected.saturating_sub(1),
            ("escape", _) => cx.emit(ContainersEvent::Hide),
            ("f5", _) => cx.emit(ContainersEvent::Refresh),
            ("c", _) => cx.emit(ContainersEvent::NextContext),
            ("n", _) => cx.emit(ContainersEvent::NextNamespace),
            ("x" | "delete" | "backspace", Some(Row::Forward(f))) => {
                cx.emit(ContainersEvent::StopForward(f.id))
            }
            ("o", Some(Row::Forward(f))) => cx.emit(ContainersEvent::Open(Ok(f.url()))),
            ("o", Some(Row::Container(c))) => {
                let url = c
                    .ports
                    .iter()
                    .find_map(|p| idecraft_containers::ports::browser_url(p))
                    .ok_or_else(|| format!("{} publishes no TCP port", c.name));
                cx.emit(ContainersEvent::Open(url));
            }
            ("f", Some(Row::Kube(k))) => match k.ports.first() {
                Some(p) if k.kind != "Job" => {
                    cx.emit(ContainersEvent::Forward(k.kind.clone(), k.name.clone(), *p))
                }
                _ => cx.emit(ContainersEvent::Open(Err(format!(
                    "{} {} has no port to forward",
                    k.kind, k.name
                )))),
            },
            ("e", Some(Row::Container(c))) => cx.emit(ContainersEvent::Exec(
                ContainerTarget::Docker(c.name.clone()),
                c.state == "running",
            )),
            ("e", Some(Row::Kube(k))) if k.kind != "Service" => cx.emit(ContainersEvent::Exec(
                ContainerTarget::Kube(k.kind.clone(), k.name.clone()),
                true,
            )),
            ("e", Some(Row::Heading(_) | Row::Engine(_) | Row::Note(..))) => {
                cx.emit(ContainersEvent::NextEngine)
            }
            ("u" | "d", Some(row @ (Row::Compose(_) | Row::Service(_)))) => {
                let action = if key == "u" {
                    ComposeAction::Up
                } else {
                    ComposeAction::Down
                };
                let service = match row {
                    Row::Service(s) => Some(s.service),
                    _ => None,
                };
                cx.emit(ContainersEvent::Compose(action, service));
            }
            ("enter" | "l", Some(Row::Service(s))) => {
                cx.emit(ContainersEvent::ServiceLogs(s.service))
            }
            ("enter" | "l", _) => {
                if let Some(t) = target {
                    cx.emit(ContainersEvent::Logs(t));
                }
            }
            ("s" | "x", _) => {
                if let Some(t @ ContainerTarget::Docker(_)) = target {
                    let action = if key == "s" {
                        ContainerAction::Start
                    } else {
                        ContainerAction::Stop
                    };
                    cx.emit(ContainersEvent::Act(t, action));
                }
            }
            ("r", _) => {
                if let Some(t) = target {
                    cx.emit(ContainersEvent::Act(t, ContainerAction::Restart));
                }
            }
            ("delete" | "backspace", _) => {
                if let Some(ContainerTarget::Docker(name)) = target {
                    if self.removing.as_deref() == Some(name.as_str()) {
                        self.removing = None;
                        cx.emit(ContainersEvent::Act(
                            ContainerTarget::Docker(name),
                            ContainerAction::Remove,
                        ));
                    } else {
                        self.removing = Some(name);
                    }
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(&event.keystroke, self.rows.len()) {
            self.selected = to;
            self.removing = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let m = &event.keystroke.modifiers;
        if m.platform || m.control || m.alt {
            return;
        }
        if self.press(event.keystroke.key.as_str(), cx) {
            cx.stop_propagation();
        }
    }
}

fn row_text(r: &Row) -> String {
    match r {
        Row::Heading(h) => h.clone(),
        Row::Note(n, _) => format!("  {n}"),
        Row::Container(c) => format!(
            "  {} {} {} {}{}",
            if c.state == "running" { "●" } else { "○" },
            c.name,
            c.image,
            c.status,
            if c.ports.is_empty() {
                String::new()
            } else {
                format!(" {}", c.ports.join(" "))
            }
        ),
        Row::Image(i) => format!(
            "  {} {} {}",
            if i.tags.is_empty() {
                "<none>".to_owned()
            } else {
                i.tags.join(", ")
            },
            i.id,
            size_label(i.size)
        ),
        Row::Engine((name, address, note, _)) => format!(
            "  Engine {name} {address}{}",
            note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default()
        ),
        Row::Compose(p) => format!(
            "Compose: {} ({})",
            p.name,
            p.files
                .iter()
                .filter_map(|f| f.file_name())
                .map(|f| f.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" + ")
        ),
        Row::Service(s) => format!("  ◆ {} {}", s.service, s.state),
        Row::Forward(f) => format!(
            "  ⇄ {} 127.0.0.1:{} → {} ({} / {}){}",
            f.target,
            f.local_port,
            f.remote_port,
            f.context,
            f.namespace,
            if f.running {
                String::new()
            } else {
                format!(" ended: {}", f.message)
            }
        ),
        Row::Kube(k) => format!(
            "  {} {} {}{}{} {}",
            k.kind,
            k.name,
            k.status,
            k.ready
                .as_ref()
                .map(|r| format!(" {r}"))
                .unwrap_or_default(),
            k.restarts
                .filter(|r| *r > 0)
                .map(|r| format!(" ↻{r}"))
                .unwrap_or_default(),
            k.age
        ),
    }
}

impl Render for ContainersView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let rows = self.rows.clone();
        let removing = self.removing.clone();
        div()
            .id("containers-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title("Containers"))
                    // Cut at the header's buttons instead of running under them.
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_xs().text_color(c(p.ui_dim)).child(
                        "Enter logs · e shell · o open · f forward · s start · x stop · r restart · Delete twice removes · u/d compose up/down · c context · n namespace · F5 refresh",
                    ))
                    .when_some(removing, |d, name| {
                        d.child(
                            div()
                                .text_color(rgb(p.console_ansi_text[3]))
                                .child(format!("Press Delete again to remove {name}")),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .id("containers-list")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .px_1()
                            .children(rows.into_iter().enumerate().map(|(i, row)| {
                                let on = i == selected;
                                let text = row_text(&row);
                                let mark = row_key(&row)
                                    .and_then(|k| crate::hit_targets::mark(format!("containers-row-{k}")));
                                let base = div()
                                    .id(("containers-row", i))
                                    .relative()
                                    .children(mark)
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .px_2()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .when(on, |d| d.bg(c(p.tree_selection)))
                                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                    .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                        this.selected = i;
                                        this.removing = None;
                                        if ev.click_count() > 1 {
                                            this.press("enter", cx);
                                        }
                                        cx.notify();
                                    }));
                                match row {
                                    Row::Heading(_) | Row::Compose(_) => {
                                        base.font_weight(gpui::FontWeight::SEMIBOLD).child(text)
                                    }
                                    Row::Engine(_) => base.text_color(c(p.ui_muted)).child(text),
                                    Row::Note(_, true) => {
                                        base.text_color(rgb(p.console_ansi_text[1])).child(text)
                                    }
                                    Row::Note(_, false) => base.text_color(c(p.ui_muted)).child(text),
                                    Row::Container(ref ct) if ct.state != "running" => {
                                        base.text_color(c(p.ui_muted)).child(text)
                                    }
                                    _ => base.child(text),
                                }
                            })),
                    )
                    .when_some(self.logs.clone(), |d, (title, text)| {
                        d.child(
                            div()
                                .id("containers-logs")
                                .w(px(460.))
                                .flex_none()
                                .border_l_1()
                                .border_color(c(p.separator))
                                .overflow_y_scroll()
                                .px_2()
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .py_1()
                                        .child(title),
                                )
                                .children(text.lines().map(|l| {
                                    div()
                                        .font_family(crate::fonts::editor_family())
                                        .text_xs()
                                        .whitespace_nowrap()
                                        .child(l.to_owned())
                                })),
                        )
                    }),
            )
    }
}

/// What a row stands for, kept across refreshes: names and ids, not the changing status.
fn row_key(row: &Row) -> Option<String> {
    Some(match row {
        Row::Heading(_) | Row::Note(..) => return None,
        Row::Container(c) => format!("container:{}", c.name),
        Row::Image(i) => format!("image:{}", i.id),
        Row::Kube(k) => format!("kube:{}/{}/{}", k.kind, k.namespace, k.name),
        Row::Engine(e) => format!("engine:{}", e.0),
        Row::Compose(p) => format!("compose:{}", p.name),
        Row::Service(s) => format!("service:{}", s.service),
        Row::Forward(f) => format!("forward:{}", f.id),
    })
}

/// The row to select after the rows changed: the one standing for `key` (D49: F5 or a new section
/// above moved the selection to another container, risky with x and Delete), else the old index
/// within the new rows.
fn reselect(key: Option<&str>, rows: &[Row], old: usize) -> usize {
    key.and_then(|k| rows.iter().position(|r| row_key(r).as_deref() == Some(k)))
        .unwrap_or_else(|| old.min(rows.len().saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_follows_its_container_across_a_refresh() {
        let c = |name: &str, status: &str| {
            Row::Container(Container {
                id: format!("id-{name}"),
                name: name.into(),
                image: "nginx".into(),
                state: "running".into(),
                status: status.into(),
                ports: Vec::new(),
                compose_project: None,
                compose_service: None,
            })
        };
        let before = [
            Row::Heading("Containers (2)".into()),
            c("web", "Up 1 s"),
            c("db", "Up 1 s"),
        ];
        let key = row_key(&before[2]);
        assert_eq!(key.as_deref(), Some("container:db"));
        // A new section above and a new status: still db.
        let after = [
            Row::Heading("Port forwards (1)".into()),
            Row::Note("x".into(), false),
            Row::Heading("Containers (2)".into()),
            c("web", "Up 9 s"),
            c("db", "Up 9 s"),
        ];
        assert_eq!(reselect(key.as_deref(), &after, 2), 4);
        // Gone: the old place, within the rows.
        assert_eq!(reselect(Some("container:gone"), &after, 9), 4);
        assert_eq!(reselect(None, &after, 1), 1);
    }

    #[test]
    fn sizes_read_like_docker() {
        assert_eq!(size_label(192_000_000), "183 MB");
        assert_eq!(size_label(5000), "5 kB");
        assert_eq!(size_label(3 << 30), "3.0 GB");
    }

    #[test]
    fn rows_name_state_ports_and_readiness() {
        let c = Container {
            id: "a".into(),
            name: "web".into(),
            image: "nginx".into(),
            state: "running".into(),
            status: "Up".into(),
            ports: vec!["0.0.0.0:8080->80/tcp".into()],
            compose_project: None,
            compose_service: None,
        };
        assert_eq!(
            row_text(&Row::Container(c)),
            "  ● web nginx Up 0.0.0.0:8080->80/tcp"
        );
        let k = KubeResource {
            kind: "Pod".into(),
            name: "web-1".into(),
            namespace: "shop".into(),
            status: "Running".into(),
            ready: Some("1/1".into()),
            restarts: Some(3),
            age: "5m".into(),
            detail: String::new(),
            ports: vec![80],
        };
        assert_eq!(row_text(&Row::Kube(k)), "  Pod web-1 Running 1/1 ↻3 5m");
        let f = ForwardInfo {
            id: 1,
            context: "dev".into(),
            namespace: "shop".into(),
            target: "pod/web-1".into(),
            local_port: 8080,
            remote_port: 80,
            running: false,
            message: "pod gone".into(),
        };
        assert_eq!(
            row_text(&Row::Forward(f)),
            "  ⇄ pod/web-1 127.0.0.1:8080 → 80 (dev / shop) ended: pod gone"
        );
        assert_eq!(
            row_text(&Row::Engine((
                "build".into(),
                "tcp://10.0.0.5:2375".into(),
                Some("unencrypted".into()),
                false
            ))),
            "  Engine build tcp://10.0.0.5:2375 (unencrypted)"
        );
        let s = ServiceState {
            service: "db".into(),
            state: "not created".into(),
            containers: vec![],
        };
        assert_eq!(row_text(&Row::Service(s)), "  ◆ db not created");
    }
}

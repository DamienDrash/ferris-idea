//! The Containers tool window (`SPEC-0119`): Docker containers and images, Kubernetes workloads of
//! one context and namespace, and the logs of what is selected; shells (`SPEC-0240`), ports and
//! port forwards (`SPEC-0241`), Compose services (`SPEC-0242`) and engines (`SPEC-0243`).
//! `SPEC-0119` v5: live updates from the engine's events with a note while they are paused,
//! followed and searched logs and container details in the side pane ([`crate::containers_pane`]).

use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use ferris_idea_containers::compose::{ComposeAction, ComposeProject, ServiceState};
use ferris_idea_containers::details::ContainerDetails;
use ferris_idea_containers::logs::LogBook;
use ferris_idea_containers::ports::ForwardInfo;
use ferris_idea_containers::{Container, ContainerAction, DockerInfo, Image, KubeResource};
use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, rgb};

use crate::containers_pane::{Pane, PaneKey, PaneRowKind, ROW_H};

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
    /// Show the details of a Docker container (`SPEC-0119` v5 behaviour 11).
    Details(String),
    /// Pull an image by its full reference (`SPEC-0119` v6 behaviour 13).
    Pull(String),
    /// Remove an image (tag or id).
    RemoveImage(String),
    /// Show the details of an image.
    ImageDetails(String),
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
    /// A container and how deep it is nested (0 flat, 1 under a Compose service).
    Container(Container, usize),
    Image(Image),
    /// A Compose project of another folder, found by its labels (read-only): name, folder.
    Group(String, Option<String>),
    /// A service of such a project: project, service, state.
    GroupService(String, String, String),
    /// A Compose project's network.
    Network(String),
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
    /// The image waiting for a second Delete.
    removing_image: Option<String>,
    /// The reference typed for a pull, while the field is open.
    pull_input: Option<String>,
    pane: Option<Pane>,
    /// Whether the keys go to the side pane instead of the list.
    pane_focus: bool,
    /// Why live updates are paused, while they are.
    events_note: Option<String>,
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
            removing_image: None,
            pull_input: None,
            pane: None,
            pane_focus: false,
            events_note: None,
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
                if let Some(n) = &self.events_note {
                    rows.push(Row::Note(n.clone(), true));
                }
                rows.extend(others);
            }
            Some(Ok((host, info, containers, images))) => {
                rows.push(Row::Heading(format!(
                    "Docker {} — {host} ({})",
                    info.version, info.platform
                )));
                if let Some(n) = &self.events_note {
                    rows.push(Row::Note(n.clone(), true));
                }
                if let Some(w) = warning {
                    rows.push(Row::Note(format!("Warning: {w}"), true));
                }
                rows.extend(others);
                let folder = self.compose.as_ref().map(|(p, _)| p.name.clone());
                if let Some((project, states)) = &self.compose {
                    rows.push(Row::Compose(project.clone()));
                    if let Some(e) = &project.error {
                        rows.push(Row::Note(e.clone(), true));
                    }
                    rows.extend(compose_rows(&project.name, states, containers));
                }
                // Compose projects of other folders, by their labels (SPEC-0242 v4).
                let mut projects: Vec<&str> = containers
                    .iter()
                    .filter_map(|c| c.compose_project.as_deref())
                    .filter(|p| Some(*p) != folder.as_deref())
                    .collect();
                projects.sort_unstable();
                projects.dedup();
                for p in projects {
                    rows.extend(group_rows(p, containers));
                }
                let flat: Vec<&Container> = containers
                    .iter()
                    .filter(|c| c.compose_project.is_none())
                    .collect();
                rows.push(Row::Heading(format!("Containers ({})", flat.len())));
                rows.extend(flat.into_iter().cloned().map(|c| Row::Container(c, 0)));
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

    /// Shows `text` as the logs of `title` (output that is not followed: Kubernetes, services).
    pub fn show_logs(&mut self, title: String, text: String, cx: &mut Context<Self>) {
        let mut book = LogBook::new(Vec::new());
        book.push(&text);
        if !text.is_empty() && !text.ends_with('\n') {
            book.push("\n");
        }
        let mut pane = Pane::logs(title, None, book, false);
        pane.after_change();
        self.pane = Some(pane);
        cx.notify();
    }

    /// Starts showing the followed output of container `name`, whose runs started at `starts`
    /// (behaviour 10); [`Self::append_logs`] adds what arrives.
    pub fn start_log_follow(
        &mut self,
        title: String,
        name: String,
        starts: Vec<i128>,
        cx: &mut Context<Self>,
    ) {
        self.pane = Some(Pane::logs(title, Some(name), LogBook::new(starts), true));
        cx.notify();
    }

    /// Output of followed container `name` arrived.
    pub fn append_logs(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.pane.as_mut().filter(|p| p.log_target() == Some(name)) {
            p.append(text);
            cx.notify();
        }
    }

    /// Followed container `name` was started again at `time`: a new run.
    pub fn log_new_run(&mut self, name: &str, time: i128, cx: &mut Context<Self>) {
        if let Some(p) = self.pane.as_mut().filter(|p| p.log_target() == Some(name)) {
            p.new_run(time);
            cx.notify();
        }
    }

    /// The followed output of `name` ended (the container stopped) or follows again.
    pub fn log_live(&mut self, name: &str, live: bool, cx: &mut Context<Self>) {
        if let Some(p) = self.pane.as_mut().filter(|p| p.log_target() == Some(name)) {
            p.set_live(live);
            cx.notify();
        }
    }

    /// The followed container, its newest line's time, while its logs are shown.
    pub fn followed(&self) -> Option<(String, Option<i128>)> {
        let p = self.pane.as_ref()?;
        Some((p.log_target()?.to_owned(), p.last_time()))
    }

    /// Shows the details of container `name` (behaviour 11).
    pub fn show_details(
        &mut self,
        name: &str,
        details: Result<ContainerDetails, String>,
        cx: &mut Context<Self>,
    ) {
        let keep = self
            .pane
            .as_ref()
            .filter(|p| p.details_of().is_some())
            .map(|p| (p.search.clone(), p.only_matching, p.searching));
        let mut pane = Pane::details(format!("Details: {name}"), details);
        if let Some((search, only, searching)) = keep {
            (pane.search, pane.only_matching, pane.searching) = (search, only, searching);
        }
        self.pane = Some(pane);
        cx.notify();
    }

    /// Why live updates are paused, or `None` once they run again (behaviour 9).
    pub fn set_events_note(&mut self, note: Option<String>, cx: &mut Context<Self>) {
        if self.events_note != note {
            self.events_note = note;
            self.rebuild();
            cx.notify();
        }
    }

    /// Whether live updates are paused.
    pub fn has_events_note(&self) -> bool {
        self.events_note.is_some()
    }

    /// The pane's rows as text (for tests).
    pub fn pane_texts(&self) -> Vec<String> {
        self.pane.as_ref().map(Pane::texts).unwrap_or_default()
    }

    /// The pane's status line: following, the search and its count (for tests).
    pub fn pane_status(&self) -> Option<String> {
        self.pane.as_ref().map(Pane::status)
    }

    /// Whether the pane follows new output.
    pub fn pane_follows(&self) -> bool {
        self.pane.as_ref().is_some_and(|p| p.follow)
    }

    /// Whether the keys go to the pane.
    pub fn pane_has_keys(&self) -> bool {
        self.pane_focus && self.pane.is_some()
    }

    /// Hands the keys to the side pane or back to the list.
    pub fn focus_pane(&mut self, on: bool, cx: &mut Context<Self>) {
        self.pane_focus = on && self.pane.is_some();
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
    pub fn logs(&self) -> Option<(String, String)> {
        let p = self.pane.as_ref()?;
        Some((p.title.clone(), p.log_text()?))
    }

    /// The container waiting for a second Delete.
    pub fn removing(&self) -> Option<&str> {
        self.removing.as_deref()
    }

    /// The image waiting for a second Delete.
    pub fn removing_image(&self) -> Option<&str> {
        self.removing_image.as_deref()
    }

    /// The reference typed for a pull, while the field is open.
    pub fn pull_input(&self) -> Option<&str> {
        self.pull_input.as_deref()
    }

    /// Shows the details of image `name` (behaviour 13 of SPEC-0119 v6).
    pub fn show_image(
        &mut self,
        name: &str,
        details: Result<ferris_idea_containers::images::ImageDetails, String>,
        cx: &mut Context<Self>,
    ) {
        self.pane = Some(Pane::image(format!("Image: {name}"), details));
        cx.notify();
    }

    /// Adds lines to the pane when it has this title (a pull's progress).
    pub fn append_titled(&mut self, title: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.pane.as_mut().filter(|p| p.title == title) {
            p.append(text);
            cx.notify();
        }
    }

    /// A key while the pull field is open: typing, Backspace, Enter pulls, Escape cancels.
    fn pull_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        let Some(text) = self.pull_input.as_mut() else {
            return;
        };
        match ks.key.as_str() {
            "escape" => self.pull_input = None,
            "backspace" => {
                text.pop();
            }
            "enter" => {
                let r = text.trim().to_owned();
                match ferris_idea_containers::images::ImageRef::parse(&r) {
                    Some(_) => {
                        self.pull_input = None;
                        cx.emit(ContainersEvent::Pull(r));
                    }
                    None => cx.emit(ContainersEvent::Open(Err(format!(
                        "{r:?} is not an image reference (repository[:tag] or repository@digest)"
                    )))),
                }
            }
            // Typed text comes from the text input ([`Self::take_committed`]).
            _ => {}
        }
        cx.notify();
    }

    /// Whether typing goes into a field: the pull field or the pane's search.
    pub fn typing(&self) -> bool {
        self.pull_input.is_some()
            || (self.pane_focus && self.pane.as_ref().is_some_and(|p| p.searching))
    }

    /// Text the platform's text input committed (keys, input methods, dictation, synthetic
    /// input), for the field that takes typing (OBS-0116 D6); otherwise nothing.
    fn take_committed(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.pull_input.as_mut() {
            t.push_str(text);
        } else if self.pane_focus
            && let Some(p) = self.pane.as_mut().filter(|p| p.searching)
        {
            p.type_text(text);
        } else {
            return;
        }
        cx.notify();
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
            Row::Container(c, _) => Some(ContainerTarget::Docker(c.name.clone())),
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
            self.removing_image = None;
        }
        match (key, row) {
            ("down", _) if n > 0 => {
                self.selected = (self.selected + 1).min(n - 1);
                self.details_follow_selection(cx);
            }
            ("up", _) => {
                self.selected = self.selected.saturating_sub(1);
                self.details_follow_selection(cx);
            }
            ("tab", _) if self.pane.is_some() => self.pane_focus = true,
            ("p", Some(Row::Image(_))) => self.pull_input = Some(String::new()),
            ("p", Some(Row::Heading(h))) if h.starts_with("Images") => {
                self.pull_input = Some(String::new())
            }
            ("enter" | "l" | "i", Some(Row::Image(i))) => {
                cx.emit(ContainersEvent::ImageDetails(image_name(&i)))
            }
            ("delete" | "backspace", Some(Row::Image(i))) => {
                let name = image_name(&i);
                if self.removing_image.as_deref() == Some(name.as_str()) {
                    self.removing_image = None;
                    cx.emit(ContainersEvent::RemoveImage(name));
                } else {
                    self.removing_image = Some(name);
                }
            }
            ("u" | "d", Some(Row::Group(..) | Row::GroupService(..))) => {
                cx.emit(ContainersEvent::Open(Err(
                    "Compose up and down only for the project folder's Compose project; s and x start and stop these containers".into(),
                )))
            }
            ("i", Some(Row::Container(c, _))) => cx.emit(ContainersEvent::Details(c.name.clone())),
            ("escape", _) => cx.emit(ContainersEvent::Hide),
            ("f5", _) => cx.emit(ContainersEvent::Refresh),
            ("c", _) => cx.emit(ContainersEvent::NextContext),
            ("n", _) => cx.emit(ContainersEvent::NextNamespace),
            ("x" | "delete" | "backspace", Some(Row::Forward(f))) => {
                cx.emit(ContainersEvent::StopForward(f.id))
            }
            ("o", Some(Row::Forward(f))) => cx.emit(ContainersEvent::Open(Ok(f.url()))),
            ("o", Some(Row::Container(c, _))) => {
                let url = c
                    .ports
                    .iter()
                    .find_map(|p| ferris_idea_containers::ports::browser_url(p))
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
            ("e", Some(Row::Container(c, _))) => cx.emit(ContainersEvent::Exec(
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

    /// With details shown, moving to another container shows its details.
    fn details_follow_selection(&mut self, cx: &mut Context<Self>) {
        let image = self
            .pane
            .as_ref()
            .and_then(Pane::image_of)
            .map(str::to_owned);
        if let (Some(shown), Some(Row::Image(i))) = (image, self.rows.get(self.selected)) {
            let name = image_name(i);
            if shown != name {
                cx.emit(ContainersEvent::ImageDetails(name));
            }
            return;
        }
        let shown = self
            .pane
            .as_ref()
            .and_then(Pane::details_of)
            .map(str::to_owned);
        if let (Some(shown), Some(Row::Container(c, _))) = (shown, self.rows.get(self.selected))
            && shown != c.name
        {
            cx.emit(ContainersEvent::Details(c.name.clone()));
        }
    }

    /// A key while the pane has the keyboard.
    fn pane_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(pane) = self.pane.as_mut() else {
            self.pane_focus = false;
            return false;
        };
        let m = &ks.modifiers;
        let done = pane.key(ks.key.as_str(), m.shift, m.alt, m.platform || m.control);
        match done {
            PaneKey::Handled => {}
            PaneKey::Unhandled => return false,
            PaneKey::Leave => self.pane_focus = false,
            PaneKey::Open(url) => cx.emit(ContainersEvent::Open(Ok(url))),
        }
        cx.notify();
        true
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // In a field a printable key is left to the text input, which commits the text the
        // system means; the key code alone is `a` for synthetic input (OBS-0116 D6).
        let k = &event.keystroke;
        if self.typing()
            && !k.modifiers.alt
            && crate::committed_text::leave_to_text_input(
                k.key_char.as_deref(),
                k.modifiers.platform || k.modifiers.control,
            )
        {
            return;
        }
        if self.pull_input.is_some() {
            self.pull_key(&event.keystroke, cx);
            cx.stop_propagation();
            return;
        }
        if self.pane_focus {
            if self.pane_key(&event.keystroke, cx) {
                cx.stop_propagation();
            }
            return;
        }
        // ⌘F with a pane open searches it.
        let m = &event.keystroke.modifiers;
        if (m.platform || m.control) && event.keystroke.key == "f" && self.pane.is_some() {
            self.pane_focus = true;
            self.pane_key(&event.keystroke, cx);
            cx.stop_propagation();
            return;
        }
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

impl ContainersView {
    /// The side pane: title, status line with following and the search, and the rows.
    fn render_pane(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let pane = self.pane.as_ref()?;
        let p = Palette::current();
        let (rows, _) = pane.rows();
        let status = pane.status();
        let current = pane.current_row();
        let focused = self.pane_focus;
        let is_logs = pane.log_target().is_some();
        let hint = match pane.log_target() {
            Some(_) => {
                "Tab keys here · ↑ pauses · End follows · / search · ⌥C ⌥W ⌥X ⌥L · Enter next match · p earlier runs"
            }
            None if pane.details_of().is_some() => {
                "Tab keys here · / search · ⌥C ⌥W ⌥X ⌥L · j inspect JSON · o open port"
            }
            None => "Tab keys here · / search · ⌥C ⌥W ⌥X ⌥L · Enter next match",
        };
        let mono = crate::fonts::editor_family();
        let rows = std::rc::Rc::new(rows);
        let list_rows = rows.clone();
        let hit_bg = rgb(p.selection);
        let current_bg = c(p.tree_selection);
        let link = c(p.link);
        let list = gpui::uniform_list(
            "containers-pane-rows",
            rows.len(),
            cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let r = &list_rows[i];
                        let base = div()
                            .id(("containers-pane-row", i))
                            .h(px(ROW_H))
                            .px_2()
                            .flex()
                            .items_center()
                            .whitespace_nowrap()
                            .when(current == Some(i), |d| d.bg(current_bg));
                        let base = match &r.kind {
                            PaneRowKind::Heading => base.font_weight(gpui::FontWeight::SEMIBOLD),
                            PaneRowKind::Run => base
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    if let Some(p) = this.pane.as_mut() {
                                        p.toggle_earlier();
                                    }
                                    cx.notify();
                                })),
                            PaneRowKind::Link(url) => {
                                let url = url.clone();
                                base.text_color(link).cursor_pointer().on_click(cx.listener(
                                    move |_this, _: &gpui::ClickEvent, _, cx| {
                                        cx.emit(ContainersEvent::Open(Ok(url.clone())));
                                    },
                                ))
                            }
                            PaneRowKind::Line => base,
                        };
                        // The line in pieces, the matches with a background.
                        let mut at = 0;
                        let mut parts: Vec<gpui::AnyElement> = Vec::new();
                        for h in &r.hits {
                            if h.start > at {
                                parts.push(
                                    div()
                                        .child(r.text[at..h.start].to_owned())
                                        .into_any_element(),
                                );
                            }
                            parts.push(
                                div()
                                    .bg(hit_bg)
                                    .child(r.text[h.start..h.end].to_owned())
                                    .into_any_element(),
                            );
                            at = h.end;
                        }
                        if at < r.text.len() || parts.is_empty() {
                            parts.push(div().child(r.text[at..].to_owned()).into_any_element());
                        }
                        base.children(parts)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(pane.scroll.clone())
        .font_family(mono)
        .text_xs()
        .flex_1();
        Some(
            div()
                .id("containers-pane")
                // The UI driver reads the pane as one document (SPEC-0319 v7 behaviour 54).
                .semantics(
                    Semantics::new(Role::Document)
                        .name(if is_logs { "Logs" } else { "Details" })
                        .test_id("containers-logs")
                        .read_only(true),
                )
                .w(px(520.))
                .flex_none()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(if focused { c(p.focus) } else { c(p.separator) })
                // Scrolling up with the wheel or the trackpad stops following.
                .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _, cx| {
                    if f32::from(ev.delta.pixel_delta(px(ROW_H)).y) > 0.
                        && let Some(p) = this.pane.as_mut()
                        && p.follow
                    {
                        p.scrolled_up();
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .flex()
                        .gap_2()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(pane.title.clone()),
                        )
                        .child(div().text_xs().text_color(c(p.ui_dim)).child(hint)),
                )
                .when(!status.is_empty(), |d| {
                    d.child(
                        div()
                            .px_2()
                            .text_xs()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(c(p.ui_muted))
                            .child(status),
                    )
                })
                .child(list)
                .into_any_element(),
        )
    }
}

/// The name an image is acted on by: its first tag, else its id.
fn image_name(i: &Image) -> String {
    i.tags.first().cloned().unwrap_or_else(|| i.id.clone())
}

/// ` · 3 days` for an image built at Unix time `created`, empty when unknown.
fn age_label(created: i64) -> String {
    if created <= 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let s = (now - created).max(0);
    let (n, unit) = match s {
        s if s < 3600 => (s / 60, "min"),
        s if s < 86_400 => (s / 3600, "h"),
        s if s < 365 * 86_400 => (s / 86_400, "days"),
        s => (s / (365 * 86_400), "years"),
    };
    format!(" · {n} {unit}")
}

/// State of a service from its containers: `running`, `n/m running`, `exited`.
fn state_of(members: &[&Container]) -> String {
    let running = members.iter().filter(|c| c.state == "running").count();
    match (running, members.len()) {
        (_, 0) => "not created".into(),
        (r, n) if r == n => "running".into(),
        (0, _) => "exited".into(),
        (r, n) => format!("{r}/{n} running"),
    }
}

/// The folder's Compose project: each service with its containers under it, containers of
/// services the file no longer names, and the project's networks (SPEC-0242 v4).
fn compose_rows(project: &str, states: &[ServiceState], containers: &[Container]) -> Vec<Row> {
    let members: Vec<&Container> = containers
        .iter()
        .filter(|c| c.compose_project.as_deref() == Some(project))
        .collect();
    let mut rows = Vec::new();
    for st in states {
        rows.push(Row::Service(st.clone()));
        rows.extend(
            members
                .iter()
                .filter(|c| c.compose_service.as_deref() == Some(st.service.as_str()))
                .map(|c| Row::Container((*c).clone(), 1)),
        );
    }
    rows.extend(
        members
            .iter()
            .filter(|c| {
                !states
                    .iter()
                    .any(|s| c.compose_service.as_deref() == Some(s.service.as_str()))
            })
            .map(|c| Row::Container((*c).clone(), 1)),
    );
    rows.extend(networks_of(&members).into_iter().map(Row::Network));
    rows
}

/// A Compose project of another folder found by its labels: heading, services with their
/// containers, networks.
fn group_rows(project: &str, containers: &[Container]) -> Vec<Row> {
    let members: Vec<&Container> = containers
        .iter()
        .filter(|c| c.compose_project.as_deref() == Some(project))
        .collect();
    let dir = members.iter().find_map(|c| c.compose_dir.clone());
    let mut rows = vec![Row::Group(project.to_owned(), dir)];
    let mut services: Vec<&str> = members
        .iter()
        .map(|c| c.compose_service.as_deref().unwrap_or(""))
        .collect();
    services.sort_unstable();
    services.dedup();
    for s in services {
        let of: Vec<&Container> = members
            .iter()
            .copied()
            .filter(|c| c.compose_service.as_deref().unwrap_or("") == s)
            .collect();
        rows.push(Row::GroupService(
            project.to_owned(),
            s.to_owned(),
            state_of(&of),
        ));
        rows.extend(of.into_iter().map(|c| Row::Container(c.clone(), 1)));
    }
    rows.extend(networks_of(&members).into_iter().map(Row::Network));
    rows
}

/// The networks of a project's containers, without the default bridge.
fn networks_of(members: &[&Container]) -> Vec<String> {
    let mut n: Vec<String> = members
        .iter()
        .flat_map(|c| c.networks.iter().cloned())
        .filter(|n| n != "bridge" && n != "host" && n != "none")
        .collect();
    n.sort();
    n.dedup();
    n
}

crate::committed_text::committed_text_input!(ContainersView, take_committed);

fn row_text(r: &Row) -> String {
    match r {
        Row::Heading(h) => h.clone(),
        Row::Note(n, _) => format!("  {n}"),
        Row::Container(c, depth) => format!(
            "{}{} {} {} {}{}",
            "  ".repeat(depth + 1),
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
        Row::Group(name, dir) => format!(
            "Compose: {name} ({}, read-only)",
            dir.as_deref().unwrap_or("another folder")
        ),
        Row::GroupService(_, service, state) => format!("  ◆ {service} {state}"),
        Row::Network(n) => format!("  network {n}"),
        Row::Image(i) => format!(
            "  {} {} {}{}",
            if i.tags.is_empty() {
                "<none>".to_owned()
            } else {
                i.tags.join(", ")
            },
            i.id,
            size_label(i.size),
            age_label(i.created)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let focused = self.focus.is_focused(window);
        let selected = self.selected;
        let rows = self.rows.clone();
        let removing = self.removing.clone();
        let pulling = self.pull_input.is_some();
        div()
            .id("containers-panel")
            .track_focus(&self.focus)
            // Typed text of the fields from the text input (OBS-0116 D6).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
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
                        "Enter logs · i details · Tab pane · p pull image · e shell · o open · f forward · s start · x stop · r restart · Delete twice removes · u/d compose up/down · c context · n namespace · F5 refresh",
                    ))
                    .when_some(removing, |d, name| {
                        d.child(
                            div()
                                .text_color(rgb(p.console_ansi_text[3]))
                                .child(format!("Press Delete again to remove {name}")),
                        )
                    })
                    .when_some(self.removing_image.clone(), |d, name| {
                        d.child(
                            div()
                                .text_color(rgb(p.console_ansi_text[3]))
                                .child(format!("Press Delete again to remove image {name}")),
                        )
                    }),
            )
            .when_some(self.pull_input.clone(), |d, text| {
                let warning = ferris_idea_containers::images::ImageRef::parse(&text)
                    .and_then(|r| r.warning());
                d.child(
                    div()
                        .id("containers-pull")
                        // The field of the reference: typing goes into it, Enter pulls the image
                        // through the engine (`run`, as `docker_image` needs; SPEC-0319 v7
                        // behaviour 56).
                        .semantics(
                            Semantics::new(Role::TextInput)
                                .name("Pull image")
                                .value(text.clone())
                                .focused(focused)
                                .test_id("containers-pull")
                                .effects(&[Effect::Run])
                                .handles_keys(&["enter", "escape", "backspace"]),
                        )
                        .px_2()
                        .py_1()
                        .flex()
                        .gap_2()
                        .border_t_1()
                        .border_color(c(p.separator))
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Pull image:"))
                        .child(
                            div()
                                .font_family(crate::fonts::editor_family())
                                .child(format!("{text}▏")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(c(p.ui_dim))
                                .child("Enter pulls · Esc cancels"),
                        )
                        .when_some(warning, |d, w| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(p.console_ansi_text[3]))
                                    .child(w),
                            )
                        }),
                )
            })
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
                            .relative()
                            // Keys on the list start, stop, restart and remove containers, open
                            // shells, ports and URLs: the rights of `docker_container` and port
                            // forwarding (SPEC-0319 v7 behaviour 56).
                            .semantics(
                                Semantics::new(Role::List)
                                    .name("Containers")
                                    .test_id("containers-list")
                                    // The pull field has the keys while it is open.
                                    .focused(focused && !pulling)
                                    .effects(&[Effect::Run, Effect::Ports])
                                    // Before the keymap (`press`): Delete twice removes,
                                    // Enter shows the logs, Escape hides the window.
                                    .handles_keys(&[
                                        "enter", "escape", "tab", "delete", "backspace", "f5",
                                        "c", "d", "e", "f", "i", "l", "n", "o", "p", "r", "s",
                                        "u", "x",
                                    ]),
                            )
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .px_1()
                            .children(rows.into_iter().enumerate().map(|(i, row)| {
                                let on = i == selected;
                                let text = row_text(&row);
                                let mark = row_key(&row)
                                    .and_then(|k| crate::hit_targets::mark(format!("containers-row-{k}")));
                                // After the mark: the annotation replaces its node (same test id).
                                let node = Semantics::new(Role::ListItem)
                                    .name(text.trim().to_owned())
                                    .selected(on)
                                    .test_id(
                                        row_key(&row)
                                            .map(|k| format!("containers-row-{k}"))
                                            .unwrap_or_else(|| format!("containers-row-{i}")),
                                    )
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]);
                                let base = div()
                                    .id(("containers-row", i))
                                    .relative()
                                    .children(mark)
                                    .semantics(node)
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
                                        this.pane_focus = false;
                                        this.details_follow_selection(cx);
                                        if ev.click_count() > 1 {
                                            this.press("enter", cx);
                                        }
                                        cx.notify();
                                    }));
                                match row {
                                    Row::Heading(_) | Row::Compose(_) | Row::Group(..) => {
                                        base.font_weight(gpui::FontWeight::SEMIBOLD).child(text)
                                    }
                                    Row::Engine(_) => base.text_color(c(p.ui_muted)).child(text),
                                    Row::Note(_, true) => {
                                        base.text_color(rgb(p.console_ansi_text[1])).child(text)
                                    }
                                    Row::Note(_, false) => base.text_color(c(p.ui_muted)).child(text),
                                    Row::Container(ref ct, _) if ct.state != "running" => {
                                        base.text_color(c(p.ui_muted)).child(text)
                                    }
                                    _ => base.child(text),
                                }
                            })),
                    )
                    .when_some(self.render_pane(cx), |d, pane| d.child(pane)),
            )
    }
}

/// What a row stands for, kept across refreshes: names and ids, not the changing status.
fn row_key(row: &Row) -> Option<String> {
    Some(match row {
        Row::Heading(_) | Row::Note(..) => return None,
        Row::Container(c, _) => format!("container:{}", c.name),
        Row::Image(i) => format!("image:{}", i.id),
        Row::Group(p, _) => format!("group:{p}"),
        Row::GroupService(p, s, _) => format!("group-service:{p}/{s}"),
        Row::Network(n) => format!("network:{n}"),
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
            Row::Container(
                Container {
                    id: format!("id-{name}"),
                    name: name.into(),
                    image: "nginx".into(),
                    image_id: String::new(),
                    state: "running".into(),
                    status: status.into(),
                    ports: Vec::new(),
                    compose_project: None,
                    compose_service: None,
                    compose_dir: None,
                    networks: Vec::new(),
                },
                0,
            )
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
            image_id: String::new(),
            state: "running".into(),
            status: "Up".into(),
            ports: vec!["0.0.0.0:8080->80/tcp".into()],
            compose_project: None,
            compose_service: None,
            compose_dir: None,
            networks: Vec::new(),
        };
        assert_eq!(
            row_text(&Row::Container(c, 0)),
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

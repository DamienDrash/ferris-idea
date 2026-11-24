//! The Welcome window's section Remote Development › SSH (`SPEC-0355` v3 behaviour 12): the
//! recent SSH projects and the hosts of the SSH configuration, New Project with Host, User and
//! Port, the folder chooser of Remote Development (`SPEC-0317` behaviour 2) and opening a folder
//! as a remote project. Everything goes through Ferris IDEA's one SSH stack: the host list and the
//! connection of `ferris_idea_remote` (`SPEC-0121`, ADR-0025: the system `ssh` in batch mode, strict
//! host keys, keys from the agent or the configuration only) and the opening path of
//! [`crate::workbench_view::start_remote_open`]. The rows and checks are in
//! [`crate::welcome_ssh_model`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ferris_idea_remote::{HostInfo, HostSpec, RecentRemote, RemoteEntry};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, Global, KeyDownEvent, Subscription, Window,
    div, prelude::*, px,
};
use serde_json::Value;

use crate::editor_view::EditorView;
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::welcome_model::step;
use crate::welcome_ssh_model::{
    AUTH_NOTE, EMPTY, HEADING, OpenSsh, SshRow, connect_spec, filter, open_row, ssh_rows,
};
use crate::workbench_view::c;
use crate::workbench_view::{STOPPED, open_already, recent_file, start_remote_open, state_dir};
use crate::{RemoteEvent, RemoteView};

/// Extra `ssh` arguments of the Welcome window's SSH section instead of none (tests: `-F` with a
/// configuration of their own, so the user's `~/.ssh/config`, keys and known hosts are never read).
pub struct WelcomeSshArgs(pub Vec<String>);

impl Global for WelcomeSshArgs {}

/// The extra `ssh` arguments: none in the product (the user's own configuration).
pub fn ssh_args(cx: &App) -> Vec<String> {
    cx.try_global::<WelcomeSshArgs>()
        .map(|a| a.0.clone())
        .unwrap_or_default()
}

/// What the section asks the Welcome window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeSshEvent {
    /// A remote project window opened: the Welcome window closes (behaviour 4).
    Opened,
    /// The list has the keys again: the Welcome window takes the focus back.
    FocusList,
}

/// What the section shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The recent SSH projects and the configured hosts (behaviour 12a).
    List,
    /// New Project: Host, User, Port (behaviour 12d).
    Form,
    /// The folder chooser of the connected host (behaviour 12e).
    Chooser,
}

/// Opens `folder` of the SSH host `host` as a remote project as a click in the section does
/// (behaviour 12c) — also for the MCP tool `welcome` `open_ssh` and while no Welcome window is
/// open. On success the Welcome window closes. `done` gets the project's state or the reason.
pub fn open_ssh_project(
    host: &str,
    folder: &str,
    stop: Arc<AtomicBool>,
    done: impl FnOnce(Result<Value, String>, &mut App) + 'static,
    cx: &mut App,
) {
    let spec = match HostSpec::parse(host) {
        Ok(spec @ HostSpec::Ssh(_)) => spec,
        Ok(_) => {
            return done(
                Err(format!(
                    "{host} is not an SSH host; open it from Remote Development… in a project window"
                )),
                cx,
            );
        }
        Err(e) => return done(Err(e.to_string()), cx),
    };
    if !ferris_idea_remote::bootstrap::absolute_folder(folder) {
        return done(
            Err("the folder must be an absolute path on the host".into()),
            cx,
        );
    }
    let extra = ssh_args(cx);
    let finish = move |result: Result<Value, String>, cx: &mut App| {
        if result.is_ok() {
            cx.defer(crate::welcome_view::close);
        }
        done(result, cx);
    };
    if let Some(shown) = open_already(&spec, folder, &extra, cx) {
        return finish(shown, cx);
    }
    let Some(base) = state_dir(cx) else {
        return finish(Err("no settings folder for remote projects".into()), cx);
    };
    start_remote_open(
        spec,
        folder.to_owned(),
        base,
        extra,
        ferris_idea_remote::BACKEND_DIR.to_owned(),
        stop,
        finish,
        cx,
    );
}

/// The section's view.
pub struct WelcomeSshView {
    recent: Vec<RecentRemote>,
    hosts: Vec<HostInfo>,
    rows: Vec<SshRow>,
    query: String,
    selected: usize,
    /// The line under the list: why something did not happen, or the opening's steps.
    note: Option<String>,
    stage: Stage,
    host: Entity<EditorView>,
    user: Entity<EditorView>,
    port: Entity<EditorView>,
    form_error: Option<String>,
    /// The fields' texts when the error was found; editing any of them drops it.
    error_texts: [String; 3],
    chooser: Option<Entity<RemoteView>>,
    remote: Option<ferris_idea_remote::Remote>,
    remote_home: Option<String>,
    /// Set to stop the opening that runs (behaviour 12c).
    opening: Option<Arc<AtomicBool>>,
    /// Whether the configured hosts were read since the section was shown.
    loaded: bool,
    _chooser_events: Option<Subscription>,
}

impl EventEmitter<WelcomeSshEvent> for WelcomeSshView {}

impl WelcomeSshView {
    /// The section, empty until [`Self::reload`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let host = cx.new(EditorView::field);
        let user = cx.new(EditorView::field);
        let port = cx.new(EditorView::field);
        for field in [&host, &user, &port] {
            cx.observe(field, |this: &mut Self, _, cx| {
                if this.form_error.is_some() && this.texts(cx) != this.error_texts {
                    this.form_error = None;
                    cx.notify();
                }
            })
            .detach();
        }
        Self {
            recent: Vec::new(),
            hosts: Vec::new(),
            rows: Vec::new(),
            query: String::new(),
            selected: 0,
            note: None,
            stage: Stage::List,
            host,
            user,
            port,
            form_error: None,
            error_texts: Default::default(),
            chooser: None,
            remote: None,
            remote_home: None,
            opening: None,
            loaded: false,
            _chooser_events: None,
        }
    }

    fn texts(&self, cx: &App) -> [String; 3] {
        [&self.host, &self.user, &self.port].map(|f| f.read(cx).state.buffer.text())
    }

    /// Reads the recent remote projects now and the configured hosts in the background (`ssh -G`
    /// per host; nothing is connected).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.recent = state_dir(cx)
            .map(|b| ferris_idea_remote::load_recent_remote(&recent_file(&b)))
            .unwrap_or_default();
        self.rebuild();
        self.loaded = true;
        let extra = ssh_args(cx);
        let task = cx
            .background_executor()
            .spawn(async move { ferris_idea_remote::host_list(&extra) });
        cx.spawn(async move |this, cx| {
            let hosts = task.await;
            let _ = this.update(cx, |this, cx| {
                this.hosts = hosts;
                this.rebuild();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Reads the lists the first time the section is shown.
    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            self.reload(cx);
        }
    }

    fn rebuild(&mut self) {
        self.rows = ssh_rows(&self.recent, &self.hosts);
        self.selected = self.selected.min(self.shown().len().saturating_sub(1));
    }

    /// What the section shows.
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Every row.
    pub fn rows(&self) -> &[SshRow] {
        &self.rows
    }

    /// The indexes of the rows the search leaves (behaviour 12b).
    pub fn shown(&self) -> Vec<usize> {
        filter(&self.rows, &self.query)
    }

    /// The rows shown, as text (tests, agents).
    pub fn shown_texts(&self) -> Vec<String> {
        self.shown()
            .into_iter()
            .map(|i| self.rows[i].text())
            .collect()
    }

    /// The selected row among the shown ones.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The search text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The line under the list.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The form's error, if any.
    pub fn form_error(&self) -> Option<&str> {
        self.form_error.as_deref()
    }

    /// The form's Host, User and Port.
    pub fn form_texts(&self, cx: &App) -> [String; 3] {
        self.texts(cx)
    }

    /// The folder chooser, while shown.
    pub fn chooser(&self) -> Option<Entity<RemoteView>> {
        self.chooser.clone()
    }

    /// Whether a remote project is being opened.
    pub fn opening(&self) -> bool {
        self.opening.is_some()
    }

    /// Sets the search text (behaviour 12b).
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_owned();
        self.selected = 0;
        cx.notify();
    }

    /// Fills the form's fields (tests and agents).
    pub fn fill(&mut self, host: &str, user: &str, port: &str, cx: &mut Context<Self>) {
        for (field, text) in [(&self.host, host), (&self.user, user), (&self.port, port)] {
            field.update(cx, |f, cx| f.set_text(text, cx));
        }
        cx.notify();
    }

    /// New Project: the form, with `host` filled in (behaviour 12d).
    pub fn show_form(&mut self, host: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.fill(host, "", "", cx);
        self.form_error = None;
        self.stage = Stage::Form;
        let target = if host.is_empty() {
            &self.host
        } else {
            &self.user
        };
        window.focus(&target.read(cx).focus());
        cx.notify();
    }

    /// Back to the list; the Welcome window takes the keys.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.stage = Stage::List;
        self.form_error = None;
        self.chooser = None;
        self._chooser_events = None;
        self.remote = None;
        self.remote_home = None;
        cx.emit(WelcomeSshEvent::FocusList);
        cx.notify();
    }

    /// Opens the shown row `shown`: a recent project opens, a host fills the form (behaviours 12c
    /// and 12d).
    pub fn open_shown(&mut self, shown: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&i) = self.shown().get(shown) else {
            return;
        };
        self.selected = shown;
        match open_row(&self.rows[i]) {
            OpenSsh::Project { host, folder } => self.open_project(host, folder, cx),
            OpenSsh::Form(host) => self.show_form(&host, window, cx),
        }
        cx.notify();
    }

    /// Takes the shown row `shown` out of the recent remote projects; a configured host stays
    /// (behaviour 12f).
    pub fn remove_shown(&mut self, shown: usize, cx: &mut Context<Self>) {
        let Some(&i) = self.shown().get(shown) else {
            return;
        };
        let SshRow::Recent(r) = self.rows[i].clone() else {
            self.note = Some(format!(
                "{} is a host of your SSH configuration; Ferris IDEA never changes that file.",
                self.rows[i].name()
            ));
            cx.notify();
            return;
        };
        let removed = state_dir(cx)
            .map(|b| ferris_idea_remote::forget_remote(&recent_file(&b), &r.host, &r.folder))
            .unwrap_or(Ok(false));
        self.note = Some(match removed {
            Ok(_) => format!(
                "Removed {} on {} from the list; nothing on the host changed.",
                r.folder, r.host
            ),
            Err(e) => format!("The list could not be written: {e}"),
        });
        self.recent = state_dir(cx)
            .map(|b| ferris_idea_remote::load_recent_remote(&recent_file(&b)))
            .unwrap_or_default();
        self.rebuild();
        cx.notify();
    }

    /// Opens `folder` of `host` as a remote project (behaviour 12c): the steps in the line under
    /// the list or in the chooser; Escape stops.
    pub fn open_project(&mut self, host: String, folder: String, cx: &mut Context<Self>) {
        if self.opening.is_some() {
            self.note = Some("A remote project is opening; Escape stops it.".into());
            cx.notify();
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.opening = Some(stop.clone());
        let text = format!(
            "Opening {folder} on {host}: connecting, checking the backend, starting it, opening the session… Escape stops"
        );
        self.note = Some(text.clone());
        if let Some(v) = &self.chooser {
            v.update(cx, |v, cx| v.show_busy(text, cx));
        }
        let weak = cx.entity().downgrade();
        open_ssh_project(
            &host,
            &folder,
            stop,
            move |result, cx| {
                let _ = weak.update(cx, |this, cx| this.opened(result, cx));
            },
            cx,
        );
        cx.notify();
    }

    fn opened(&mut self, result: Result<Value, String>, cx: &mut Context<Self>) {
        self.opening = None;
        let why = match result {
            Ok(_) => {
                self.note = None;
                self.stage = Stage::List;
                self.chooser = None;
                cx.emit(WelcomeSshEvent::Opened);
                cx.notify();
                return;
            }
            Err(e) if e == STOPPED => {
                "Opening stopped; whatever was started on the host is ended.".to_owned()
            }
            Err(e) => e,
        };
        self.note = Some(why.clone());
        if let Some(v) = &self.chooser {
            v.update(cx, |v, cx| v.show_error(why, cx));
        }
        cx.notify();
    }

    /// Stops the opening that runs (behaviour 12c).
    pub fn cancel_open(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(stop) = &self.opening else {
            return false;
        };
        stop.store(true, Ordering::SeqCst);
        self.note = Some("Stopping; whatever was started on the host is ended…".into());
        cx.notify();
        true
    }

    /// Connect in the form: checks the fields, then connects and shows the host's home folder
    /// (behaviours 12d and 12e).
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let [host, user, port] = self.texts(cx);
        let spec = match connect_spec(&host, &user, &port) {
            Ok(spec) => spec,
            Err(why) => {
                self.form_error = Some(why);
                self.error_texts = self.texts(cx);
                cx.notify();
                return;
            }
        };
        self.form_error = None;
        let chooser = cx.new(RemoteView::new);
        chooser.update(cx, |v, cx| {
            v.set_chooser_only(cx);
            v.show_busy(format!("Connecting to {spec}…"), cx);
        });
        self._chooser_events = Some(cx.subscribe_in(&chooser, window, Self::on_chooser));
        window.focus(&chooser.read(cx).field_focus(cx));
        self.chooser = Some(chooser);
        self.stage = Stage::Chooser;
        self.connect(spec, cx);
        cx.notify();
    }

    /// Connects to `spec` in the background and shows its home folder.
    fn connect(&mut self, spec: String, cx: &mut Context<Self>) {
        let target = ferris_idea_remote::SshTarget::parse(&spec, &ssh_args(cx));
        let remote = ferris_idea_remote::Remote::new(target);
        let r = remote.clone();
        let task = cx.background_executor().spawn(async move {
            let home = r.home()?;
            let entries = r.list(&home)?;
            Ok::<_, ferris_idea_remote::RemoteError>((home, entries))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(v) = this.chooser.clone() else {
                    return;
                };
                match result {
                    Ok((home, entries)) => {
                        this.remote = Some(remote);
                        this.remote_home = Some(home.clone());
                        // The host as typed, so the list remembers it as the user wrote it.
                        v.update(cx, |v, cx| v.show_dir(spec, home, entries, cx));
                    }
                    Err(e) => v.update(cx, |v, cx| v.show_error(e.to_string(), cx)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows folder `path` of the connected host (`~` is the home folder).
    fn browse(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(remote) = self.remote.clone() else {
            return;
        };
        let path = match (path.strip_prefix('~'), &self.remote_home) {
            (Some(rest), Some(home)) => format!("{home}{rest}"),
            _ => path,
        };
        let task = cx.background_executor().spawn(async move {
            let dir = remote.resolve_dir(&path)?;
            let entries: Vec<RemoteEntry> = remote.list(&dir)?;
            Ok::<_, ferris_idea_remote::RemoteError>((dir, entries))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(v) = this.chooser.clone() {
                    let host = v
                        .read(cx)
                        .place()
                        .map(|(h, _)| h.clone())
                        .unwrap_or_default();
                    match result {
                        Ok((dir, entries)) => {
                            v.update(cx, |v, cx| v.show_dir(host, dir, entries, cx))
                        }
                        Err(e) => v.update(cx, |v, cx| v.show_error(e.to_string(), cx)),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn on_chooser(
        &mut self,
        view: &Entity<RemoteView>,
        event: &RemoteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RemoteEvent::Hide | RemoteEvent::Hosts => self.back(cx),
            RemoteEvent::Connect(spec) => self.connect(spec.clone(), cx),
            RemoteEvent::Browse(path) => self.browse(path.clone(), cx),
            RemoteEvent::Expand(path) => {
                if let Some(remote) = self.remote.clone() {
                    crate::remote_view::expand(remote, view.clone(), path.clone(), cx);
                }
            }
            RemoteEvent::Refresh => {
                if let Some((_, dir)) = view.read(cx).place().cloned() {
                    self.browse(dir, cx);
                }
            }
            RemoteEvent::OpenProject(host, folder) => {
                self.open_project(host.clone(), folder.clone(), cx)
            }
            // Escape while connecting (nothing opens yet) goes back to the list.
            RemoteEvent::CancelOpen => {
                if !self.cancel_open(cx) {
                    self.back(cx);
                }
            }
            RemoteEvent::FocusField => window.focus(&view.read(cx).field_focus(cx)),
            // Files, the terminal and the backend belong to the project window.
            _ => view.update(cx, |v, cx| {
                v.show_error(
                    "Open the folder as a remote project first; its files open in the project window"
                        .into(),
                    cx,
                )
            }),
        }
    }

    /// A key of the list while the Welcome window has the keys (behaviour 12b); whether it was
    /// taken.
    pub fn list_key(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let n = self.shown().len();
        let m = &k.modifiers;
        if let Some(to) = crate::list_nav::edge_with(
            k,
            n,
            crate::list_nav::Keys::beside_field(self.query.is_empty()),
        ) {
            self.selected = to;
            cx.notify();
            return true;
        }
        match k.key.as_str() {
            "down" => self.selected = step(self.selected, n, true),
            "up" => self.selected = step(self.selected, n, false),
            "enter" => self.open_shown(self.selected, window, cx),
            "escape" if self.cancel_open(cx) => {}
            "escape" => {
                self.query.clear();
                self.selected = 0;
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
            }
            _ => match k.key_char.as_deref() {
                Some(ch) if !m.control && !m.platform && !ch.chars().any(char::is_control) => {
                    self.query.push_str(ch);
                    self.selected = 0;
                }
                _ => return false,
            },
        }
        cx.notify();
        true
    }

    /// Enter, Escape and Tab of the form; the other keys edit its fields.
    fn on_form_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        if m.platform || m.control || m.alt {
            return;
        }
        match k.key.as_str() {
            "enter" => self.submit(window, cx),
            "escape" => self.back(cx),
            "tab" => {
                let fields = [&self.host, &self.user, &self.port];
                let at = fields
                    .iter()
                    .position(|f| f.read(cx).focus().is_focused(window))
                    .unwrap_or(0);
                let next = (at + if m.shift { 2 } else { 1 }) % 3;
                window.focus(&fields[next].read(cx).focus());
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// A plain button of the section.
fn button(
    id: impl Into<gpui::ElementId>,
    text: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    let p = Palette::current();
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(4.))
        .flex_none()
        .when(primary, |d| d.bg(c(p.accent)).text_color(c(p.accent_text)))
        .when(!primary, |d| {
            d.border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tree_hover)))
        })
        .child(text)
}

impl WelcomeSshView {
    fn render_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let search = div()
            .id("welcome-ssh-search")
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(4.))
            .border_1()
            .border_color(c(p.focus))
            .child(icon("general/search"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(self.query.is_empty(), |d| {
                        d.text_color(c(p.ui_dim))
                            .child("Search SSH projects and hosts")
                    })
                    .when(!self.query.is_empty(), |d| d.child(self.query.clone())),
            )
            .on_click(
                cx.listener(|_, _: &gpui::ClickEvent, _, cx| cx.emit(WelcomeSshEvent::FocusList)),
            );
        let actions =
            div().flex().items_center().gap_2().child(search).child(
                button("welcome-ssh-new", "New Project", true).on_click(cx.listener(
                    |this, _: &gpui::ClickEvent, window, cx| this.show_form("", window, cx),
                )),
            );
        let shown = self.shown();
        let mut list = div()
            .id("welcome-ssh-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if self.rows.is_empty() {
            list = list.child(div().pt_4().text_color(c(p.ui_dim)).child(EMPTY));
        } else if shown.is_empty() {
            list = list.child(
                div()
                    .pt_4()
                    .text_color(c(p.ui_dim))
                    .child("No matching SSH projects or hosts."),
            );
        }
        for (n, &i) in shown.iter().enumerate() {
            let r = self.rows[i].clone();
            let selected = n == self.selected;
            let recent = matches!(r, SshRow::Recent(_));
            list = list.child(
                div()
                    .id(("welcome-ssh-row", n))
                    .group("welcome-ssh-row")
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .when(selected, |d| d.bg(c(p.tree_selection)))
                    .when(!selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(c(if recent { p.accent } else { p.separator }))
                            .text_color(c(p.accent_text))
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(r.initials()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div().flex().items_center().gap_2().child(r.name()).when(
                                    !recent,
                                    |d| {
                                        d.child(
                                            div().text_xs().text_color(c(p.ui_dim)).child("host"),
                                        )
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(c(p.ui_dim))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(r.detail()),
                            ),
                    )
                    .when(recent, |d| {
                        d.child(
                            div()
                                .id(("welcome-ssh-remove", n))
                                .flex_none()
                                .px_2()
                                .rounded(px(4.))
                                .text_xs()
                                .border_1()
                                .border_color(c(p.separator))
                                .hover(|s| s.bg(c(p.tree_hover)))
                                .when(!selected, |d| {
                                    d.invisible()
                                        .group_hover("welcome-ssh-row", |s| s.visible())
                                })
                                .child("Remove from list")
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_shown(n, cx)
                                })),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        cx.emit(WelcomeSshEvent::FocusList);
                        this.open_shown(n, window, cx)
                    })),
            );
        }
        let mut content = div()
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_2()
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(HEADING))
            .child(actions)
            .child(list);
        if let Some(note) = self.note.clone() {
            content = content.child(div().text_xs().text_color(c(p.ui_muted)).child(note));
        }
        content.into_any_element()
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let field = |label: &'static str, hint: &'static str, editor: Entity<EditorView>| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .w(px(60.))
                        .flex_none()
                        .text_color(c(p.ui_muted))
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(24.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.separator))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(18.))
                                .overflow_hidden()
                                .child(editor),
                        ),
                )
                .child(
                    div()
                        .w(px(190.))
                        .flex_none()
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .child(hint),
                )
        };
        let [host, user, port] = self.texts(cx);
        let target = connect_spec(&host, &user, &port).ok();
        div()
            .id("welcome-ssh-form")
            .capture_key_down(cx.listener(Self::on_form_key))
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .pb_2()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("New Project: Connect to an SSH Host"),
            )
            .child(field(
                "Host",
                "alias, host name or address",
                self.host.clone(),
            ))
            .child(field(
                "User",
                "empty: from the SSH configuration",
                self.user.clone(),
            ))
            .child(field(
                "Port",
                "empty: from the configuration, else 22",
                self.port.clone(),
            ))
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(c(p.ui_muted))
                    .child(AUTH_NOTE),
            )
            .child(div().text_xs().text_color(c(p.ui_dim)).child(match target {
                Some(t) => format!("Connects as ssh {t}, then you choose the project folder"),
                None => "Tab moves between the fields · Enter connects · Escape goes back".into(),
            }))
            .when_some(self.form_error.clone(), |d, e| {
                d.child(div().text_xs().text_color(c(p.error_text)).child(e))
            })
            .child(
                div()
                    .pt_2()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("welcome-ssh-cancel", "Cancel", false).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.back(cx)),
                        ),
                    )
                    .child(
                        button("welcome-ssh-connect", "Connect", true).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, window, cx| this.submit(window, cx),
                        )),
                    ),
            )
            .into_any_element()
    }
}

impl Render for WelcomeSshView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match (self.stage, self.chooser.clone()) {
            (Stage::Form, _) => self.render_form(cx),
            (Stage::Chooser, Some(chooser)) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_1().min_h_0().child(chooser))
                .when_some(
                    self.note.clone().filter(|_| self.opening.is_some()),
                    |d, note| {
                        let p = Palette::current();
                        d.child(div().p_2().text_xs().text_color(c(p.ui_muted)).child(note))
                    },
                )
                .into_any_element(),
            _ => self.render_list(cx),
        }
    }
}

/// The recent remote projects file in the settings folder of `cx` (tests: their own).
pub fn recent_remote_file(cx: &App) -> Option<PathBuf> {
    state_dir(cx).map(|b| recent_file(&b))
}

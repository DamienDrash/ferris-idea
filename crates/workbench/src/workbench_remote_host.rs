//! The tool windows of a remote project on the host's functions (`SPEC-0318` v2 window details
//! 2–13): host terminals in the Terminal tool window, host runs in the Run tool window with their
//! markers, the trust banner, offered ports and the Ports list, Find in Files, Search
//! Everywhere, the Commit and Git windows, Rename, context actions, parameter info, semantic
//! colours and inspections from the host, and what agents see of it all. A remote project is used
//! like a local one: the same windows, keys and look; only the data comes from the session.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Context, Entity, WeakEntity, Window, div, prelude::*};
use idecraft_remote::{HostTerminal, RunTarget, TermOutput, TermPhase};
use idecraft_session::ErrorCode;
use idecraft_terminal::{LinkRead, PtyLink};
use serde_json::{Value, json};

use super::{WorkbenchView, c};
use crate::TerminalView;
use crate::palette::Palette;

/// A host function the views ask through the session (`SPEC-0318`): method and parameters in, the
/// answer or the reason out. Never blocks typing: views call it on background threads.
pub(crate) type HostCall = Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;

/// How long one host question waits (the first start of a server included).
const HOST_LIMIT: Duration = Duration::from_secs(60);

/// What agents see of the window is published at most this often.
const PUBLISH_EVERY: Duration = Duration::from_secs(1);

/// The slot a host terminal fills once its view knows its size.
type TermSlot = Arc<Mutex<Option<Arc<HostTerminal>>>>;

/// A port the host offered, for the banner (v2 detail 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PortOffer {
    pub port: u16,
    pub kind: String,
    pub command: String,
}

/// Where a remote document opened from a search result goes once its editor is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingPlace {
    /// Select a char range.
    Select(std::ops::Range<usize>),
    /// Go to a 0-based line and column (or the line's start).
    GoTo(usize, Option<usize>),
    /// The debugged program stopped at this 0-based line (`SPEC-0322` v5 behaviour 14).
    Execution(usize),
}

impl PendingPlace {
    pub(super) fn apply(&self, e: &mut crate::EditorView, cx: &mut Context<crate::EditorView>) {
        match self {
            Self::Select(r) => e.select_range(r.clone(), cx),
            Self::GoTo(line, Some(col)) => e.go_to(*line, *col, cx),
            Self::GoTo(line, None) => e.go_to_line(*line, cx),
            Self::Execution(line) => {
                e.go_to_line(*line, cx);
                e.set_execution_line(Some(*line), cx);
            }
        }
    }
}

/// The window's state of a remote project's host functions.
#[derive(Default)]
pub(super) struct HostState {
    /// Places to go to in remote documents that are still opening.
    pub pending_place: HashMap<PathBuf, PendingPlace>,
    /// The host's run configurations and whether its project is trusted (`run.configs`).
    pub run_configs: Option<idecraft_core::RunConfigs>,
    pub trusted: Option<bool>,
    configs_loading: bool,
    /// Configurations are to be read again at the next look.
    pub configs_due: bool,
    /// The host branch (`git.status`, `git.changed`).
    pub branch: Option<String>,
    pub(super) branch_asked: bool,
    /// Ports offered this session, and the offers still shown (first: the banner).
    offered: HashSet<u16>,
    pub offers: Vec<PortOffer>,
    /// Host terminals of the window, for agents.
    terms: Vec<(WeakEntity<TerminalView>, TermSlot)>,
    /// Documents whose semantic tokens or inspections are being asked, and those asked again.
    semantic_running: HashSet<PathBuf>,
    semantic_again: HashSet<PathBuf>,
    inspect_running: HashSet<PathBuf>,
    inspect_again: HashSet<PathBuf>,
    published: Option<Instant>,
    started: bool,
}

/// A host terminal behind the terminal view's screen.
struct HostPty(Arc<HostTerminal>);

impl PtyLink for HostPty {
    fn read(&self, wait: Duration) -> LinkRead {
        match self.0.read(wait) {
            None => LinkRead::Idle,
            Some(TermOutput::Data(b)) => LinkRead::Data(b),
            Some(TermOutput::Redraw) => LinkRead::Redraw,
            Some(TermOutput::Ended(code, note)) => {
                LinkRead::Ended(code.map(|c| c.max(0) as u32), note)
            }
        }
    }
    fn write(&self, bytes: &[u8]) {
        self.0.write(bytes);
    }
    fn resize(&self, rows: u16, cols: u16) {
        self.0.resize(cols, rows);
    }
    fn close(&self) {
        self.0.close();
    }
}

/// The next tab name of a host terminal: the host, then `host (2)` … (v2 detail 2).
pub(super) fn host_tab_name(host: &str, existing: &[String]) -> String {
    if !existing.iter().any(|n| n == host) {
        return host.to_owned();
    }
    (2..)
        .map(|i| format!("{host} ({i})"))
        .find(|n| !existing.contains(n))
        .unwrap_or_else(|| host.to_owned())
}

/// The message of a refused host request, with its hint.
fn reason(e: &idecraft_session::RpcError) -> String {
    match &e.hint {
        Some(h) if !h.is_empty() => format!("{} ({h})", e.message),
        _ => e.message.clone(),
    }
}

impl WorkbenchView {
    /// A host function caller for the views of this window's remote project.
    pub(crate) fn remote_host_call(&self) -> Option<HostCall> {
        let live = self.remote_project.as_ref()?.live.clone();
        Some(Arc::new(move |method: &str, params: Value| {
            live.host_call(method, params, HOST_LIMIT)
                .map_err(|e| reason(&e))
        }))
    }

    /// Remembers where `path` is to show once its remote document has opened (a search result
    /// opened before the document is there).
    pub(super) fn remote_pend(&mut self, path: &Path, place: PendingPlace) {
        if self.editors.contains_key(path) {
            return;
        }
        if let Some(r) = self.remote_project.as_mut() {
            r.host.pending_place.insert(path.to_path_buf(), place);
        }
    }

    pub(super) fn remote_host_label(&self) -> String {
        self.remote_project
            .as_ref()
            .map(|r| r.live.host())
            .unwrap_or_default()
    }

    /// One look at the host functions, with the window's look at its project: configurations
    /// and branch when due, what agents see.
    pub(super) fn remote_host_tick(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if !r.host.started {
            r.host.started = true;
            r.host.configs_due = true;
        }
        let due = std::mem::take(&mut r.host.configs_due);
        let branch_due = !r.host.branch_asked;
        r.host.branch_asked = true;
        if due {
            self.remote_load_run_configs(cx);
        }
        if branch_due {
            self.remote_load_branch(cx);
        }
        self.remote_publish(cx);
    }

    // ---------------------------------------------------------------------------------------
    // Terminals (v2 detail 2)

    /// A host terminal view in `cwd` (relative to the project folder) for this window.
    pub(super) fn remote_terminal_view(
        &mut self,
        cwd: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let r = self.remote_project.as_mut()?;
        let live = r.live.clone();
        let local = if cwd == "." {
            r.stand_in.clone()
        } else {
            r.stand_in.join(cwd)
        };
        let slot: TermSlot = Arc::default();
        let (fill, look) = (Arc::clone(&slot), Arc::clone(&slot));
        let cwd = cwd.to_owned();
        let opener: crate::terminal_view::LinkOpener = Box::new(move |rows, cols| {
            let term = Arc::new(HostTerminal::open(live, &cwd, cols, rows));
            if let Ok(mut s) = fill.lock() {
                *s = Some(Arc::clone(&term));
            }
            Arc::new(HostPty(term))
        });
        let note: Arc<dyn Fn() -> Option<String> + Send + Sync> = Arc::new(move || {
            let term = look.lock().ok()?.clone()?;
            (term.phase() == TermPhase::Reconnecting).then(|| "Reconnecting".to_owned())
        });
        let view = cx.new(|cx| TerminalView::linked(local, opener, note, cx));
        r.host.terms.push((view.downgrade(), slot));
        Some(view)
    }

    /// The terminal tabs' names as the header shows them: a host terminal's tab says
    /// "(Reconnecting)" while its connection is broken (v2 detail 2).
    pub fn terminal_tab_labels(&self, cx: &gpui::App) -> Vec<String> {
        self.terminals
            .iter()
            .map(|t| {
                let note = t.panes.iter().find_map(|p| p.view.read(cx).link_note());
                match note {
                    Some(n) => format!("{} ({n})", t.name),
                    None => t.name.clone(),
                }
            })
            .collect()
    }

    // ---------------------------------------------------------------------------------------
    // Run configurations, runs and markers (v2 details 3, 4, 12)

    /// Reads the host's run configurations and its trust in the background (`run.configs`).
    pub(super) fn remote_load_run_configs(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if r.host.configs_loading {
            return;
        }
        r.host.configs_loading = true;
        let live = r.live.clone();
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call("run.configs", json!({}), HOST_LIMIT) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(r) = this.remote_project.as_mut() else {
                    return;
                };
                r.host.configs_loading = false;
                match read {
                    Ok(v) => {
                        let configs = v["configs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|c| serde_json::from_value(c.clone()).ok())
                            .collect();
                        let problems = v["problems"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str().map(str::to_owned))
                            .collect();
                        r.host.run_configs = Some(idecraft_core::RunConfigs { configs, problems });
                        r.host.trusted = v["trusted"].as_bool();
                        this.remote_take_run_configs(false);
                    }
                    // Disconnected or an older backend: asked again later.
                    Err(e) if e.code == ErrorCode::Unavailable => {}
                    Err(e) if e.code == ErrorCode::NotFound => {
                        this.status = Some(format!(
                            "Running is not available on this backend: {}",
                            e.message
                        ));
                    }
                    Err(e) => this.status = Some(format!("Run configurations: {}", e.message)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The window's configurations are the host's (instead of reading this machine's files);
    /// with `again` they are read from the host once more (Run or the chooser opens, v2 detail 3).
    pub(super) fn remote_take_run_configs(&mut self, again: bool) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        r.host.configs_due |= again;
        self.run_configs =
            r.host
                .run_configs
                .clone()
                .unwrap_or_else(|| idecraft_core::RunConfigs {
                    configs: Vec::new(),
                    problems: Vec::new(),
                });
        self.run_detected = Vec::new();
        let keep = self
            .run_selected
            .as_ref()
            .is_some_and(|n| self.run_configs.configs.iter().any(|c| &c.name == n));
        if !keep {
            self.run_selected = self.run_configs.configs.first().map(|c| c.name.clone());
        }
    }

    /// The names of the host's run configurations, once read (self-test, agents).
    pub fn remote_run_config_names(&self) -> Option<Vec<String>> {
        let r = self.remote_project.as_ref()?;
        let c = r.host.run_configs.as_ref()?;
        Some(c.configs.iter().map(|c| c.name.clone()).collect())
    }

    /// Whether the host said its project is not trusted (v2 detail 12).
    pub fn remote_untrusted(&self) -> bool {
        self.remote_project
            .as_ref()
            .is_some_and(|r| r.host.trusted == Some(false))
    }

    /// Starts `target` on the host and shows it in the Run tool window (v2 details 3–4).
    pub(super) fn remote_start_run(
        &mut self,
        target: RunTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            return;
        };
        let host = live.host();
        if self.remote_untrusted() {
            self.status = Some(self.remote_trust_text());
            cx.notify();
            return;
        }
        // The host runs what is on its disk (SPEC-0087 v5 behaviour 3a).
        if !self.save_before_launch(cx) {
            return;
        }
        let view = self.ensure_run_view(window, cx);
        let what = match &target {
            RunTarget::Name(n) => n.clone(),
            RunTarget::Context { doc, line } => format!("{doc}:{}", line + 1),
        };
        self.status = Some(format!("Starting {what} on {host}…"));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { idecraft_remote::start_host_run(&live, &target) });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            let _ = this.update(cx, |this, cx| {
                match started {
                    Ok(run) => {
                        view.update(cx, |v, cx| v.run_handle(&run.name, run.handle, cx));
                        this.show_run = true;
                        // The Run window shows it running and ending, as for a local run; a
                        // status line here would stay after the end.
                        this.status = None;
                    }
                    Err(e) => {
                        if e.code == ErrorCode::Denied
                            && let Some(r) = this.remote_project.as_mut()
                        {
                            // The host refused for its trust (behaviour 5): the banner says why.
                            r.host.configs_due = true;
                        }
                        this.status = Some(format!("Not started on {host}: {}", reason(&e)));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Debugs what `target` stands for on the host (`SPEC-0322` behaviour 16).
    pub(super) fn remote_debug(
        &mut self,
        target: RunTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            return;
        };
        let host = live.host();
        if self.remote_untrusted() {
            self.status = Some(self.remote_trust_text());
            cx.notify();
            return;
        }
        // The host debugs what is on its disk.
        if !self.save_before_launch(cx) {
            return;
        }
        let params = match &target {
            RunTarget::Name(n) => json!({ "name": n }),
            RunTarget::Context { doc, line } => json!({ "context": { "doc": doc, "line": line } }),
        };
        self.status = Some(format!("Debugging on {host}…"));
        cx.notify();
        let plan_live = live.clone();
        let task = cx
            .background_executor()
            .spawn(async move { plan_live.host_call("dap.plan", params, HOST_LIMIT) });
        let handle = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let planned = task.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let Some(this) = handle.upgrade() else {
                    return;
                };
                this.update(cx, |this, cx| match planned {
                    Ok(plan) => this.remote_debug_planned(&live, &plan, window, cx),
                    Err(e) => {
                        this.status = Some(format!("Cannot debug on {host}: {}", reason(&e)));
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn remote_debug_planned(
        &mut self,
        live: &idecraft_remote::LiveProject,
        plan: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(stand_in) = self.remote_project.as_ref().map(|r| r.stand_in.clone()) else {
            return;
        };
        let text = |k: &str| plan[k].as_str().unwrap_or_default().to_owned();
        let Some(kind) = crate::debug_model::Kind::from_wire(&text("kind")) else {
            self.status = Some("Cannot debug on the host: unknown program kind".into());
            cx.notify();
            return;
        };
        let name = text("name");
        let launch = crate::debug_model::Launch {
            kind,
            adapter: Vec::new(),
            interpreter: plan["interpreter"].as_str().map(PathBuf::from),
            program: PathBuf::from(text("program")),
            args: plan["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            cwd: PathBuf::from(text("cwd")),
            env: serde_json::from_value(plan["env"].clone()).unwrap_or_default(),
            breakpoints: self.all_source_breakpoints(),
            prepare: text("prepare"),
            special: self.special_breakpoints(),
            attach: None,
            browser: None,
            remote: Some(crate::debug_remote::RemoteLaunch {
                live: live.clone(),
                stand_in,
                root: text("root"),
                attach: None,
                target_root: None,
            }),
        };
        self.status = None;
        let view = self.ensure_debug_view(window, cx);
        view.update(cx, |v, cx| v.start(&name, launch, cx));
        self.show_debug = true;
        cx.notify();
    }

    /// Attach to Process in a remote project (`SPEC-0322` behaviour 17): the host's processes.
    pub(super) fn remote_open_attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            return;
        };
        let host = live.host();
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call("dap.processes", json!({}), HOST_LIMIT) });
        let handle = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let listed = task.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let Some(this) = handle.upgrade() else {
                    return;
                };
                this.update(cx, |this, cx| match listed {
                    Ok(v) => {
                        let processes = host_processes(&v);
                        let view = cx.new(|cx| crate::AttachView::new_remote(processes, cx));
                        this.show_attach_dialog(view, window, cx);
                    }
                    Err(e) => {
                        this.status = Some(format!("No processes of {host}: {}", reason(&e)));
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    /// Attaches the `runtime`'s debugger to `attach` on the host (`SPEC-0322` behaviours 10–15).
    pub(super) fn remote_attach(
        &mut self,
        runtime: idecraft_debugger::Runtime,
        attach: serde_json::Value,
        target_root: Option<String>,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(r) = self.remote_project.as_ref() else {
            return;
        };
        // Go programs on a host are started under Delve, not attached to (SPEC-0147 v3).
        if runtime == idecraft_debugger::Runtime::Go {
            self.status = Some(
                "Cannot attach: Go programs on a host are debugged by starting them, not by attaching"
                    .into(),
            );
            cx.notify();
            return;
        }
        let (live, stand_in) = (r.live.clone(), r.stand_in.clone());
        let launch = crate::debug_model::Launch {
            kind: crate::debug_model::Kind::of_runtime(runtime),
            adapter: Vec::new(),
            interpreter: None,
            program: PathBuf::from(label),
            args: Vec::new(),
            cwd: PathBuf::new(),
            env: Default::default(),
            breakpoints: self.all_source_breakpoints(),
            prepare: String::new(),
            special: self.special_breakpoints(),
            attach: None,
            browser: None,
            remote: Some(crate::debug_remote::RemoteLaunch {
                root: String::new(),
                live,
                stand_in,
                attach: Some(attach),
                target_root,
            }),
        };
        let view = self.ensure_debug_view(window, cx);
        view.update(cx, |v, cx| v.start(label, launch, cx));
        self.show_debug = true;
        cx.notify();
    }

    /// ⌃⇧R and the Debug variants in a remote document (v2 details 3–4, `SPEC-0322` behaviour
    /// 16): runs or debugs what the line stands for on the host.
    pub(super) fn remote_run_context(
        &mut self,
        debug: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(path) = self.active_path() else {
            return false;
        };
        let Some(doc) = self.remote_rel(&path) else {
            return false;
        };
        let line = self
            .caret_position(cx)
            .map(|(l, _)| l.saturating_sub(1))
            .unwrap_or(0);
        let target = RunTarget::Context { doc, line };
        if debug {
            self.remote_debug(target, window, cx);
        } else {
            self.remote_start_run(target, window, cx);
        }
        true
    }

    /// The menu of a ▶ marker in a remote document (v2 detail 4): Run on the host.
    pub(super) fn remote_marker_menu(
        &mut self,
        path: &Path,
        marker: idecraft_languages::jvm::RunMarker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.remote_rel(path) else {
            return;
        };
        let name = match &marker.method {
            Some(m) => format!("{}.{m}", marker.class),
            None => marker.class.clone(),
        };
        let host = self.remote_host_label();
        let line = marker.line;
        self.show_jvm_popup(
            name.clone(),
            vec![
                format!("Run '{name}' on {host}"),
                format!("Debug '{name}' on {host}"),
            ],
            window,
            cx,
            move |this, i, window, cx| {
                let target = RunTarget::Context {
                    doc: doc.clone(),
                    line,
                };
                match i {
                    0 => this.remote_start_run(target, window, cx),
                    1 => this.remote_debug(target, window, cx),
                    _ => {}
                }
            },
        );
    }

    // ---------------------------------------------------------------------------------------
    // Trust (v2 detail 12)

    fn remote_trust_text(&self) -> String {
        let (host, folder) = self.remote_project().unwrap_or_default();
        let name = folder.rsplit('/').find(|s| !s.is_empty()).unwrap_or("/");
        format!(
            "{name} on {host} is not trusted: nothing of it runs on the host — run configurations and Git hooks are skipped, language servers run in their safe configuration"
        )
    }

    /// The safe-mode banner of an untrusted host project.
    pub(super) fn remote_trust_banner(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        if !self.remote_untrusted() {
            return None;
        }
        let p = Palette::current();
        Some(
            div()
                .id("safe-mode-banner")
                .px_3()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .bg(c(p.popup))
                .border_b_1()
                .border_color(c(p.separator))
                .children(crate::hit_targets::mark("safe-mode-banner"))
                .child(div().text_color(c(p.accent)).child("⚠"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(c(p.ui_text))
                        .child(self.remote_trust_text()),
                )
                .child(
                    crate::metrics::button("trust-project", "Trust Project…", false).on_click(
                        cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.remote_ask_trust(window, cx)
                        }),
                    ),
                ),
        )
    }

    /// The banner's Trust Project…: asks once more, then `run.trust` on the host.
    pub fn remote_ask_trust(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (host, folder) = self.remote_project().unwrap_or_default();
        self.show_jvm_popup(
            format!("Trust {folder} on {host}? Its run configurations, Git hooks and build scripts may run code on the host"),
            vec!["Trust Project".into(), "Stay in Safe Mode".into()],
            window,
            cx,
            |this, i, _, cx| {
                if i == 0 {
                    this.remote_trust(cx);
                }
            },
        );
    }

    /// Trusts the host project (`run.trust`, audited on the host).
    pub fn remote_trust(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call("run.trust", json!({}), HOST_LIMIT) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                let host = this.remote_host_label();
                this.status = Some(match done {
                    Ok(_) => {
                        if let Some(r) = this.remote_project.as_mut() {
                            r.host.trusted = Some(true);
                            r.host.configs_due = true;
                        }
                        format!("The project is trusted on {host}")
                    }
                    Err(e) => format!("Could not trust the project on {host}: {}", reason(&e)),
                });
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------
    // Ports (v2 details 10–11)

    /// A `port.detected` or `port.gone` of the host.
    pub(super) fn remote_port_event(&mut self, method: &str, p: &Value) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let Some(port) = p["port"].as_u64().and_then(|n| u16::try_from(n).ok()) else {
            return;
        };
        if method == "port.gone" {
            r.host.offers.retain(|o| o.port != port);
            return;
        }
        if !r.host.offered.insert(port) {
            return;
        }
        r.host.offers.push(PortOffer {
            port,
            kind: p["kind"].as_str().unwrap_or("service").to_owned(),
            command: p["command"].as_str().unwrap_or_default().to_owned(),
        });
    }

    /// The offer shown in the banner, if any (self-test, agents).
    pub fn remote_port_offer(&self) -> Option<(u16, String, String)> {
        let o = self.remote_project.as_ref()?.host.offers.first()?;
        Some((o.port, o.kind.clone(), self.remote_offer_text(o)))
    }

    fn remote_offer_text(&self, o: &PortOffer) -> String {
        let host = self.remote_host_label();
        if o.kind == "debug" {
            return format!(
                "Port {} on {host} is a debug port: the debugger reaches it through the session",
                o.port
            );
        }
        let program = o
            .command
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        if program.is_empty() {
            format!("Port {} on {host} is listening", o.port)
        } else {
            format!("Port {} on {host} is listening ({program})", o.port)
        }
    }

    /// The banner of the first offered port (v2 detail 10): Forward (not for debug ports) and
    /// Ignore.
    pub(super) fn remote_port_banner(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let o = self.remote_project.as_ref()?.host.offers.first()?.clone();
        let p = Palette::current();
        let text = self.remote_offer_text(&o);
        let port = o.port;
        let mut bar = div()
            .id("remote-port-banner")
            .px_3()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .bg(c(p.popup))
            .border_b_1()
            .border_color(c(p.separator))
            .children(crate::hit_targets::mark("remote-port-banner"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(c(p.ui_text))
                    .child(text),
            );
        if o.kind != "debug" {
            bar = bar.child(
                crate::metrics::button("remote-port-forward", "Forward", false).on_click(
                    cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.remote_dismiss_offer(port);
                        this.remote_forward_port(port, false, window, cx);
                    }),
                ),
            );
        }
        Some(bar.child(
            crate::metrics::button("remote-port-ignore", "Ignore", false).on_click(cx.listener(
                move |this, _: &gpui::ClickEvent, _, cx| {
                    this.remote_dismiss_offer(port);
                    cx.notify();
                },
            )),
        ))
    }

    /// Hides the offer of `port` (Ignore, or Forward chosen).
    pub fn remote_dismiss_offer(&mut self, port: u16) {
        if let Some(r) = self.remote_project.as_mut() {
            r.host.offers.retain(|o| o.port != port);
        }
    }

    /// Forwards `port` of the host to this computer's loopback (behaviours 26–27): debug ports
    /// are refused, a database port asks first unless `confirmed`.
    pub fn remote_forward_port(
        &mut self,
        port: u16,
        confirmed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            return;
        };
        let kind = idecraft_host_kind(port, &self.remote_known_command(port));
        if kind == "debug" {
            self.status = Some(format!(
                "Port {port} is a debug port: the debugger reaches it through the session, it is not forwarded"
            ));
            cx.notify();
            return;
        }
        if kind == "database" && !confirmed {
            self.show_jvm_popup(
                format!("Port {port} looks like a database: a forwarded port can be reached by every user of this computer"),
                vec!["Forward Anyway".into(), "Cancel".into()],
                window,
                cx,
                move |this, i, window, cx| {
                    if i == 0 {
                        this.remote_forward_port(port, true, window, cx);
                    }
                },
            );
            // ⏎ alone never exposes a database port: Cancel is preselected (security first).
            if let Some(p) = self.jvm_popup.clone() {
                p.update(cx, |p, _| p.select(1));
            }
            return;
        }
        let host = live.host();
        self.status = Some(format!("Forwarding port {port} of {host}…"));
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            let conn = live.host_connection().ok_or_else(|| {
                idecraft_session::RpcError::new(
                    ErrorCode::Unavailable,
                    format!("reconnecting to {}", live.host()),
                )
            })?;
            idecraft_remote::ports::forward(
                &conn,
                &live.host(),
                &live.folder(),
                port,
                None,
                idecraft_remote::ports::Asker::User { confirmed },
            )
        });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match done {
                    Ok(f) => format!(
                        "Port {port} of {host} is forwarded to 127.0.0.1:{}",
                        f["local_port"]
                    ),
                    Err(e) => format!("Port {port} not forwarded: {}", reason(&e)),
                });
                this.remote_refresh_services_ports(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The command the host gave for `port`, if it offered it.
    fn remote_known_command(&self, port: u16) -> String {
        self.remote_project
            .as_ref()
            .and_then(|r| r.host.offers.iter().find(|o| o.port == port))
            .map(|o| o.command.clone())
            .unwrap_or_default()
    }

    /// The Ports list (v2 detail 11): detected ports and open forwards; choosing one forwards it
    /// or offers Open in Browser and Stop Forwarding.
    pub fn remote_show_ports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            self.status = Some("Ports are listed for a remote project".into());
            cx.notify();
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { live.host_call("port.list", json!({}), HOST_LIMIT) });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let listed = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| match listed {
                    Ok(v) => this.remote_ports_popup(&v, window, cx),
                    Err(e) => {
                        this.status = Some(format!("Cannot list the ports: {}", reason(&e)));
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn remote_ports_popup(&mut self, listed: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let (host, folder) = self.remote_project().unwrap_or_default();
        let forwards = idecraft_remote::ports::list(&host, &folder);
        let mut items = Vec::new();
        let mut actions: Vec<PortRow> = Vec::new();
        for f in &forwards {
            // The host is in the popup's title; the row stays short enough to show the
            // loopback address in full.
            items.push(format!(
                "Port {} → 127.0.0.1:{} (forwarded)",
                f["remote_port"], f["local_port"]
            ));
            actions.push(PortRow::Forward(
                f["forward"].as_str().unwrap_or_default().to_owned(),
                f["local_port"].as_u64().unwrap_or_default() as u16,
            ));
        }
        for d in listed["detected"].as_array().into_iter().flatten() {
            let Some(port) = d["port"].as_u64().and_then(|n| u16::try_from(n).ok()) else {
                continue;
            };
            if forwards.iter().any(|f| f["remote_port"] == port) {
                continue;
            }
            let kind = d["kind"].as_str().unwrap_or("service");
            let command = d["command"].as_str().unwrap_or_default();
            items.push(match kind {
                "debug" => {
                    format!("Port {port} — debug port, reached by the debugger through the session")
                }
                k => format!("Port {port} — {k} — {command}"),
            });
            actions.push(PortRow::Detected(port, kind.to_owned(), command.to_owned()));
        }
        if items.is_empty() {
            items.push(format!("No program started on {host} listens on a port"));
        }
        self.show_jvm_popup(
            format!("Ports — {host}"),
            items,
            window,
            cx,
            move |this, i, window, cx| match actions.get(i).cloned() {
                Some(PortRow::Detected(port, kind, command)) => {
                    if let Some(r) = this.remote_project.as_mut()
                        && !r.host.offers.iter().any(|o| o.port == port)
                    {
                        r.host.offered.insert(port);
                        r.host.offers.push(PortOffer {
                            port,
                            kind,
                            command,
                        });
                    }
                    this.remote_dismiss_offer(port);
                    this.remote_forward_port(port, false, window, cx);
                }
                Some(PortRow::Forward(id, local)) => {
                    cx.defer_in(window, move |this, window, cx| {
                        this.remote_forward_menu(id.clone(), local, window, cx)
                    });
                }
                None => {}
            },
        );
    }

    fn remote_forward_menu(
        &mut self,
        id: String,
        local: u16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_jvm_popup(
            format!("127.0.0.1:{local}"),
            vec!["Open in Browser".into(), "Stop Forwarding".into()],
            window,
            cx,
            move |this, i, _, cx| match i {
                0 => cx.open_url(&format!("http://127.0.0.1:{local}")),
                1 => this.remote_close_forward(&id, cx),
                _ => {}
            },
        );
    }

    /// Stops forward `id`: its local listener and its connections close.
    pub fn remote_close_forward(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(live) = self.remote_project.as_ref().map(|r| r.live.clone()) else {
            return;
        };
        let id = id.to_owned();
        let task = cx.background_executor().spawn(async move {
            let conn = live.host_connection();
            idecraft_remote::ports::close(conn.as_ref(), &live.host(), &live.folder(), &id)
        });
        cx.spawn(async move |this, cx| {
            let closed = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(if closed {
                    "The forward is stopped".into()
                } else {
                    "No such forward".into()
                });
                this.remote_refresh_services_ports(cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------
    // Git (v2 detail 7)

    /// Reads the host branch for the status bar.
    fn remote_load_branch(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.remote_host_call() else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { host("git.status", json!({})) });
        cx.spawn(async move |this, cx| {
            let status = task.await;
            let _ = this.update(cx, |this, cx| {
                if let (Ok(v), Some(r)) = (status, this.remote_project.as_mut()) {
                    r.host.branch = v["branch"].as_str().map(str::to_owned);
                    this.branch = r.host.branch.clone();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The host repository moved (`git.changed`): the Commit and Git windows read it again.
    pub(super) fn remote_git_moved(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = &self.remote_project {
            self.branch = r.host.branch.clone();
        }
        if let Some(c) = self.commit.clone() {
            c.update(cx, |c, cx| {
                c.refresh();
                cx.notify();
            });
        }
        self.reload_git_log(cx);
    }

    /// The host's name for a new Services window; `None` outside a remote project.
    pub(super) fn remote_host_label_if_remote(&self) -> Option<String> {
        self.remote_project
            .is_some()
            .then(|| self.remote_host_label())
    }

    /// The Branches popup on the host's branches (`git.branches`).
    pub(super) fn remote_open_branches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.remote_host_call() else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { host("git.branches", json!({})) });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let listed = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let branches = listed.and_then(|v| {
                        serde_json::from_value::<idecraft_vcs::Branches>(v)
                            .map_err(|e| e.to_string())
                    });
                    match branches {
                        Ok(b) => this.show_branches_data(
                            crate::branches_model::PopupData::of(&b.branches, None),
                            window,
                            cx,
                        ),
                        Err(e) => {
                            this.status = Some(e);
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    /// A choice of the Branches popup for the host repository: Checkout, New Branch, Merge, Rebase,
    /// Rename and Delete run there (`SPEC-0318` v6), the others are not on the host yet. Returns
    /// whether it was handled.
    pub(super) fn remote_branches_event(
        &mut self,
        event: &crate::BranchesEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::BranchesEvent as E;
        if self.remote_project.is_none() {
            return false;
        }
        let call = match event {
            E::Checkout(name, create) => Some((
                "git.checkout",
                json!({ "name": name, "create": create }),
                format!("Checked out {name}"),
            )),
            E::CreateFrom(name, start) => Some((
                "git.checkout",
                json!({ "name": name, "create": true, "start": start }),
                format!("Created and checked out {name}"),
            )),
            E::Merge(source) => {
                let label = format!("Merge {source}");
                self.remote_git_operation(label, "git.merge", json!({ "source": source }), cx);
                return true;
            }
            E::Rebase(onto) => {
                let label = format!("Rebase onto {onto}");
                self.remote_git_operation(label, "git.rebase_onto", json!({ "onto": onto }), cx);
                return true;
            }
            E::CheckoutAndRebase(branch, onto) => {
                let label = format!("Rebase {branch} onto {onto}");
                let params = json!({ "onto": onto, "branch": branch });
                self.remote_git_operation(label, "git.rebase_onto", params, cx);
                return true;
            }
            E::Rename(old, new) => {
                let label = format!("Rename {old} to {new}");
                let params = json!({ "old": old, "new": new });
                self.remote_git_operation(label, "git.rename_branch", params, cx);
                return true;
            }
            E::Delete(name) => {
                let label = format!("Delete branch {name}");
                self.remote_git_operation(label, "git.delete_branch", json!({ "name": name }), cx);
                return true;
            }
            E::Favorite(..) => return false,
            // Push and the actions that run on the host take the local way in.
            E::PushBranch(..) => return false,
            E::Run(
                "Git.Fetch" | "Git.Pull" | "Vcs.Push" | "Vcs.UpdateProject" | "Git.Stash"
                | "Git.Unstash",
            ) => {
                return false;
            }
            _ => None,
        };
        let Some((method, params, done)) = call else {
            self.status = Some("Not available for a remote project yet".into());
            cx.notify();
            return true;
        };
        let Some(host) = self.remote_host_call() else {
            return true;
        };
        let method = method.to_owned();
        let task = cx
            .background_executor()
            .spawn(async move { host(&method, params) });
        cx.spawn(async move |this, cx| {
            let r = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match r {
                    Ok(_) => done,
                    Err(e) => e,
                });
                if let Some(r) = this.remote_project.as_mut() {
                    r.host.branch_asked = false;
                }
                this.remote_git_moved(cx);
                cx.notify();
            });
        })
        .detach();
        true
    }

    // ---------------------------------------------------------------------------------------
    // Language functions (v2 details 8–9)

    /// Rename on the host (`lsp.rename` with `apply: true`): the editors take the edits through
    /// the session.
    pub(super) fn remote_lsp_rename(
        &mut self,
        ask: super::remote_project::RemoteAsk,
        line: usize,
        column: usize,
        new_name: String,
        cx: &mut Context<Self>,
    ) {
        let host = self.remote_host_label();
        self.status = Some(format!("Renaming to {new_name} on {host}…"));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { ask.rename(line, column, &new_name) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match done {
                    Ok(v) if v["files"].as_array().is_some_and(|f| !f.is_empty()) => {
                        let n = v["files"].as_array().map_or(0, Vec::len);
                        format!(
                            "Renamed {} to {} in {n} file{} on {host}",
                            v["old_name"].as_str().unwrap_or_default(),
                            v["new_name"].as_str().unwrap_or_default(),
                            if n == 1 { "" } else { "s" }
                        )
                    }
                    Ok(_) => "Nothing to rename here.".into(),
                    Err(e) => format!("Rename failed: {e}"),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Renames the symbol at the caret to `new_name`, as the rename popup's Enter does (the
    /// window self-test).
    pub fn rename_symbol_at_caret(
        &mut self,
        new_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(here) = self.here(cx) {
            self.rename_at = Some(here);
            self.rename_to(new_name.to_owned(), window, cx);
        }
    }

    /// Applies the host server's action `title` (`lsp.apply_action` with `apply: true`).
    pub(super) fn remote_apply_action(
        &mut self,
        ask: super::remote_project::RemoteAsk,
        range: idecraft_languages::insight::Range,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.status = Some(format!("{title}…"));
        cx.notify();
        let task = cx.background_executor().spawn({
            let title = title.clone();
            async move { ask.apply_action(&range, &title, true) }
        });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match done {
                    Ok(_) => format!("{title}: done."),
                    Err(e) => format!("{title} failed: {e}"),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Semantic tokens of a remote document from the host (`lsp.semantic`), one question per
    /// document at a time.
    pub(super) fn remote_semantic(&mut self, path: &Path, cx: &mut Context<Self>) {
        let (Some(ask), Some(editor)) = (self.remote_ask(path), self.editors.get(path).cloned())
        else {
            return;
        };
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if !r.host.semantic_running.insert(path.to_path_buf()) {
            r.host.semantic_again.insert(path.to_path_buf());
            return;
        }
        let revision = editor.read(cx).revision();
        let file = path.to_path_buf();
        let task = cx
            .background_executor()
            .spawn(async move { ask.semantic() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                let again = this.remote_project.as_mut().is_some_and(|r| {
                    r.host.semantic_running.remove(&file);
                    r.host.semantic_again.remove(&file)
                });
                let on = crate::appearance::ide_settings(cx)
                    .editor
                    .semantic_highlighting;
                if let (Ok(tokens), Some(editor), true) =
                    (found, this.editors.get(&file).cloned(), on)
                    && editor.update(cx, |e, cx| e.set_semantic_tokens(revision, &tokens, cx))
                {
                    this.sync_twin(&file, cx);
                }
                if again && on {
                    this.remote_semantic(&file, cx);
                }
            });
        })
        .detach();
    }

    /// IDECraft's inspections of a remote document on the host (`lsp.inspect`).
    pub(super) fn remote_inspect(&mut self, path: &Path, cx: &mut Context<Self>) {
        let (Some(ask), Some(editor)) = (self.remote_ask(path), self.editors.get(path).cloned())
        else {
            return;
        };
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if !r.host.inspect_running.insert(path.to_path_buf()) {
            r.host.inspect_again.insert(path.to_path_buf());
            return;
        }
        let revision = editor.read(cx).revision();
        let file = path.to_path_buf();
        let task = cx.background_executor().spawn(async move { ask.inspect() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                let again = this.remote_project.as_mut().is_some_and(|r| {
                    r.host.inspect_running.remove(&file);
                    r.host.inspect_again.remove(&file)
                });
                if let Ok(items) = found {
                    for e in this.views_of(&file) {
                        e.update(cx, |e, cx| e.set_host_findings(revision, &items, cx));
                    }
                }
                if again {
                    this.remote_inspect(&file, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------
    // Agents (v2 detail 13)

    /// Publishes the window's host terminals and runs for agents (`remote_projects`).
    fn remote_publish(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if r.host
            .published
            .is_some_and(|t| t.elapsed() < PUBLISH_EVERY)
        {
            return;
        }
        r.host.published = Some(Instant::now());
        r.host.terms.retain(|(v, _)| v.upgrade().is_some());
        let names: HashMap<gpui::EntityId, String> = self
            .terminals
            .iter()
            .flat_map(|t| {
                t.panes
                    .iter()
                    .map(move |p| (p.view.entity_id(), t.name.clone()))
            })
            .collect();
        let terminals: Vec<Value> = r
            .host
            .terms
            .iter()
            .filter_map(|(view, slot)| {
                let view = view.upgrade()?;
                let term = slot.lock().ok()?.clone()?;
                let state = match term.phase() {
                    TermPhase::Opening => "opening",
                    TermPhase::Running => "running",
                    TermPhase::Reconnecting => "reconnecting",
                    TermPhase::Ended => "ended",
                };
                Some(json!({
                    "tab": names.get(&view.entity_id()), "term": term.id(), "state": state,
                }))
            })
            .collect();
        let runs: Vec<Value> = self
            .run_view
            .as_ref()
            .map(|v| v.read(cx).tab_states())
            .unwrap_or_default()
            .into_iter()
            .map(|(name, running, exit_code, failed)| {
                json!({ "name": name, "running": running, "exit_code": exit_code, "failed_tests": failed })
            })
            .collect();
        r.live.set_window_part("terminals", json!(terminals));
        r.live.set_window_part("runs", json!(runs));
        r.live
            .set_window_part("trusted", r.host.trusted.map_or(Value::Null, Value::Bool));
    }
}

/// The kind the host gives a port (`debug`, `database`, `service`; behaviour 25).
fn idecraft_host_kind(port: u16, command: &str) -> &'static str {
    let debug_programs = ["jdwp", "debugpy", "lldb-server", "gdbserver", "--inspect"];
    if debug_programs.iter().any(|p| command.contains(p))
        || (matches!(port, 5005 | 5678 | 9229) && command.is_empty())
    {
        "debug"
    } else if matches!(port, 5432 | 3306 | 1433 | 1521 | 6379 | 27017) {
        "database"
    } else {
        "service"
    }
}

#[derive(Debug, Clone)]
enum PortRow {
    Detected(u16, String, String),
    Forward(String, u16),
}

/// The processes `dap.processes` listed, for the Attach to Process dialog.
fn host_processes(list: &serde_json::Value) -> Vec<idecraft_debugger::Process> {
    list["processes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(idecraft_debugger::Process {
                pid: p["pid"].as_u64()? as u32,
                name: p["name"].as_str()?.to_owned(),
                command: p["command"].as_str().unwrap_or_default().to_owned(),
                runtime: idecraft_debugger::Runtime::parse(p["runtime"].as_str()?)?,
                debug_port: p["debug_port"].as_u64().map(|n| n as u16),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_terminal_tabs_are_named_after_the_host() {
        assert_eq!(host_tab_name("build", &[]), "build");
        assert_eq!(
            host_tab_name("build", &["build".into(), "build (2)".into()]),
            "build (3)"
        );
    }

    #[test]
    fn the_window_knows_debug_and_database_ports_before_it_asks() {
        assert_eq!(idecraft_host_kind(5005, ""), "debug");
        assert_eq!(
            idecraft_host_kind(8000, "python3 -m debugpy --listen"),
            "debug"
        );
        assert_eq!(idecraft_host_kind(5432, "postgres"), "database");
        assert_eq!(
            idecraft_host_kind(8080, "python3 -m http.server"),
            "service"
        );
    }
}

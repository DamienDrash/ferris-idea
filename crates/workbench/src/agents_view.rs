//! The Agents tab of the AI Chat tool window (`SPEC-0357`, OBS-0121 gaps A7–A11): the agents of
//! `agent-host.toml` with Start, ACP programs found on this computer with Add, the hosted agents
//! with their state and Stop, the selected agent's output, a card for each waiting permission
//! request with the command or the diff of the edit, the files the newest turn changed with Undo,
//! and the prompt field.
//!
//! Approving is the person's decision (`SPEC-0349` behaviour 7): the Allow buttons carry the
//! effect reserved for the user, so neither the UI driver nor an agent operating the window can
//! press them; there is never an "always allow". Adding a program to the configuration is
//! reserved for the user too.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ferris_idea_core::agent_detect::{self, Found};
use ferris_idea_core::agent_host::{
    self as host, AgentHost, AgentSnapshot, AgentState, Choice, LoadedHostConfig, OutputEntry,
    PendingPermission,
};
use ferris_idea_core::agent_turns::{ChangeKind, TurnChanges};
use gpui::{
    Context, Div, Entity, EventEmitter, FocusHandle, FontWeight, KeyDownEvent, Stateful, Window,
    div, prelude::*, px,
};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// Most diff lines a card shows (behaviour 8).
pub const CARD_LINES: usize = 400;
/// Output entries shown of the selected agent.
const SHOWN_ENTRIES: usize = 200;

/// What the Agents tab asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsEvent {
    /// Open this workspace file in the editor (behaviour 11).
    OpenFile(PathBuf),
}

/// The Agents tab (`SPEC-0357`).
pub struct AgentsView {
    root: PathBuf,
    host: Arc<AgentHost>,
    /// The configuration as last read.
    config: LoadedHostConfig,
    /// The configuration file, in the user's settings folder.
    file: Option<PathBuf>,
    /// ACP programs found on this computer.
    found: Vec<Found>,
    /// Where programs are looked for (tests set their own).
    folders: Option<Vec<PathBuf>>,
    /// The hosted agent shown.
    selected: Option<String>,
    /// A note for the person: why something was refused, or what was done.
    note: Option<String>,
    input: Entity<EditorView>,
    scroll: gpui::ScrollHandle,
    focus: FocusHandle,
    /// What was drawn last, to redraw only when the agents changed.
    seen: Vec<(String, u64, u64, AgentState)>,
    /// What the end of the output held when last drawn; a change scrolls to it.
    shown_end: (usize, usize),
    _poll: gpui::Task<()>,
}

impl EventEmitter<AgentsEvent> for AgentsView {}

fn state_words(s: AgentState) -> &'static str {
    match s {
        AgentState::Working => "Working",
        AgentState::WaitingForApproval => "Needs approval",
        AgentState::WaitingForInput => "Needs input",
        AgentState::Idle => "Idle",
        AgentState::Done => "Done",
        AgentState::Failed => "Failed",
        AgentState::Unknown => "Unknown",
    }
}

fn masked(text: &str) -> String {
    ferris_idea_ai::context::mask_secrets(text).0
}

fn button(id: impl Into<gpui::ElementId>, label: impl Into<gpui::SharedString>) -> Stateful<Div> {
    let p = Palette::current();
    div()
        .id(id)
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(c(p.separator))
        .bg(c(p.island))
        .text_xs()
        .text_color(c(p.ui_text))
        .cursor_pointer()
        .hover(|s| s.bg(c(p.tree_hover)))
        .child(label.into())
}

fn button_node(name: &str, test_id: String, effect: Effect) -> Semantics {
    Semantics::new(Role::Button)
        .name(name)
        .test_id(test_id)
        .action(AxAction::Click)
        .effects(&[effect])
}

/// The lines of a proposed edit as a diff: `-` removed, `+` added, ` ` unchanged.
pub fn diff_lines(old: Option<&str>, new: &str) -> Vec<String> {
    let before = old.unwrap_or("");
    let unified = ferris_idea_core::agent_turns::unified_diff("", before, new);
    unified
        .lines()
        .skip(2)
        .filter(|l| !l.starts_with("@@"))
        .map(str::to_owned)
        .collect()
}

impl AgentsView {
    /// The tab of the workspace at `root`, on the host shared with the window's MCP server.
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let host = host::shared(&root);
        let file = crate::appearance::ide_settings_file(cx)
            .and_then(|f| f.parent().map(|d| d.join(host::HOST_FILE)));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |v, cx| v.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let mut v = Self {
            root,
            host,
            config: LoadedHostConfig::default(),
            file,
            found: Vec::new(),
            folders: None,
            selected: None,
            note: None,
            input: cx.new(EditorView::field),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
            seen: Vec::new(),
            shown_end: (0, 0),
            _poll: poll,
        };
        v.reload();
        v
    }

    /// Looks for programs in `folders` instead of `PATH` and the usual folders (tests).
    pub fn with_folders(&mut self, folders: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.folders = Some(folders);
        self.reload();
        cx.notify();
    }

    /// Reads the configuration and looks for programs again.
    pub fn reload(&mut self) {
        self.config = match &self.file {
            Some(f) => host::load_host_config(f, &self.root),
            None => LoadedHostConfig {
                problem: Some("no settings folder is known, so no agent is configured".into()),
                ..LoadedHostConfig::default()
            },
        };
        self.found = match &self.folders {
            Some(f) => agent_detect::detect(f, &self.config.config),
            None => agent_detect::detect_here(&self.config.config),
        };
    }

    /// Focus handle of the prompt field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.input.read(cx).focus()
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let now: Vec<(String, u64, u64, AgentState)> = self
            .host
            .list()
            .into_iter()
            .map(|s| (s.id, s.output_seq, s.progress, s.state))
            .collect();
        if now != self.seen {
            self.seen = now;
            if self.selected.is_none() {
                self.selected = self.seen.last().map(|s| s.0.clone());
            }
            self.scroll.scroll_to_bottom();
            cx.notify();
        }
    }

    /// The number of agents that need the person: an approval or input (behaviour 10).
    pub fn needing(&self) -> usize {
        self.host
            .list()
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    AgentState::WaitingForApproval | AgentState::WaitingForInput
                ) && !s.exited
            })
            .count()
    }

    fn selected_snapshot(&self) -> Option<AgentSnapshot> {
        self.selected
            .as_ref()
            .and_then(|id| self.host.snapshot(id).ok())
    }

    /// Starts configured agent `agent` (behaviour 2).
    pub fn start(&mut self, agent: &str, cx: &mut Context<Self>) {
        self.reload();
        if let Some(p) = &self.config.problem {
            self.note = Some(format!("Nothing can be started: {p}"));
            cx.notify();
            return;
        }
        let Some(def) = self.config.config.get(agent).cloned() else {
            self.note = Some(format!("`{agent}` is not in agent-host.toml"));
            cx.notify();
            return;
        };
        let started = host::plan(&def, &self.root, |n| std::env::var_os(n))
            .and_then(|plan| self.host.start(&def, plan, self.config.config.max_agents));
        match started {
            Ok(id) => {
                self.note = None;
                self.selected = Some(id);
            }
            Err(e) => self.note = Some(format!("Starting {} failed: {e}", def.id)),
        }
        cx.notify();
    }

    /// Adds a found program to `agent-host.toml` (behaviour 3).
    pub fn add(&mut self, index: usize, cx: &mut Context<Self>) {
        let (Some(file), Some(found)) = (self.file.clone(), self.found.get(index).cloned()) else {
            return;
        };
        self.note = Some(match agent_detect::add(&file, &found) {
            Ok(id) => format!("Added {} as `{id}` to {}", found.name, file.display()),
            Err(e) => format!("Adding {} failed: {e}", found.name),
        });
        self.reload();
        cx.notify();
    }

    /// Sends the prompt field's text to the selected agent (behaviour 7).
    pub fn send(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            self.note = Some("Start an agent first".into());
            cx.notify();
            return;
        };
        let text = self.input.read(cx).state.buffer.text();
        if text.trim().is_empty() {
            return;
        }
        match self.host.prompt(&id, text.trim_end()) {
            Ok(()) => {
                self.note = None;
                self.input.update(cx, |e, cx| e.set_text("", cx));
            }
            Err(e) => self.note = Some(e),
        }
        cx.notify();
    }

    /// Answers request `request` of the selected agent with `option` (behaviours 8, 9).
    pub fn answer(&mut self, request: u64, option: &str, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if let Err(e) = self
            .host
            .respond(&id, request, Choice::Option(option.to_owned()))
        {
            self.note = Some(e);
        }
        cx.notify();
    }

    /// Stops hosted agent `id` (behaviour 5); the host waits for it in the background.
    pub fn stop(&mut self, id: &str, cx: &mut Context<Self>) {
        let host = self.host.clone();
        let id = id.to_owned();
        cx.background_executor()
            .spawn(async move {
                let _ = host.stop(&id);
            })
            .detach();
        cx.notify();
    }

    /// Undoes turn `turn` of the selected agent (behaviour 11).
    pub fn undo(&mut self, turn: u64, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.note = Some(match self.host.undo_turn(&id, turn) {
            Ok(back) => format!("Turn {turn} undone: {} file(s) put back", back.len()),
            Err(e) => format!("Undo refused: {e}"),
        });
        cx.notify();
    }

    /// Selects hosted agent `id`.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_owned());
        cx.notify();
    }

    /// The selected agent's output as drawn, `kind: text` (tests, MCP state).
    pub fn texts(&self) -> Vec<String> {
        self.entries()
            .iter()
            .map(|e| format!("{}: {}", e.kind, masked(&e.text)))
            .collect()
    }

    /// The hosted agents as rows: id and state in words.
    pub fn rows(&self) -> Vec<(String, String)> {
        self.host
            .list()
            .into_iter()
            .map(|s| (s.id, state_words(s.state).to_owned()))
            .collect()
    }

    /// The names of the programs offered under Found on this computer.
    pub fn found(&self) -> Vec<(String, bool)> {
        self.found
            .iter()
            .map(|f| (f.name.clone(), f.configured))
            .collect()
    }

    /// The note shown to the person.
    pub fn note(&self) -> Option<String> {
        self.note.clone()
    }

    fn entries(&self) -> Vec<OutputEntry> {
        let Some(id) = &self.selected else {
            return Vec::new();
        };
        let Ok((s, _, _)) = self.host.read(id, 0, 0) else {
            return Vec::new();
        };
        let since = s.output_seq.saturating_sub(SHOWN_ENTRIES as u64);
        self.host
            .read(id, since, SHOWN_ENTRIES)
            .map(|(_, e, _)| e)
            .unwrap_or_default()
    }

    fn newest_changes(&self) -> Option<TurnChanges> {
        let id = self.selected.as_ref()?;
        self.host
            .changes(id)
            .ok()?
            .into_iter()
            .rev()
            .find(|t| !t.files.is_empty())
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.key == "enter" && !k.modifiers.shift && !k.modifiers.alt {
            self.send(cx);
            cx.stop_propagation();
        }
    }

    fn card(&self, p: &PendingPermission, cx: &mut Context<Self>) -> Stateful<Div> {
        let pal = Palette::current();
        let request = p.request;
        let mut lines = Vec::new();
        for d in &p.diffs {
            lines.push((
                format!(
                    "{}{}",
                    masked(&d.path),
                    if d.old_text.is_none() {
                        " (new file)"
                    } else {
                        ""
                    }
                ),
                pal.ui_text,
            ));
            for l in diff_lines(d.old_text.as_deref(), &d.new_text) {
                let colour = if l.starts_with('+') {
                    pal.status_text(crate::palette::Status::Success)
                } else if l.starts_with('-') {
                    pal.status_text(crate::palette::Status::Error)
                } else {
                    pal.ui_dim
                };
                lines.push((masked(&l), colour));
            }
            if d.cut {
                lines.push((
                    "… the agent's text was cut at its size limit".into(),
                    pal.ui_dim,
                ));
            }
        }
        let more = lines.len().saturating_sub(CARD_LINES);
        lines.truncate(CARD_LINES);
        let buttons = p
            .options
            .iter()
            .filter(|o| o.kind == "allow_once" || o.kind.starts_with("reject"))
            .map(|o| {
                let allow = o.kind == "allow_once";
                let test_id = format!(
                    "agents-{}-{request}-{}",
                    if allow { "allow" } else { "reject" },
                    o.option_id
                );
                let option = o.option_id.clone();
                let label = if o.name.trim().is_empty() {
                    if allow {
                        "Allow once".to_owned()
                    } else {
                        "Reject".to_owned()
                    }
                } else {
                    masked(&o.name)
                };
                button(gpui::SharedString::from(test_id.clone()), label.clone())
                    .relative()
                    .semantics(button_node(
                        &label,
                        test_id,
                        // Approving is reserved for the person (behaviour 9).
                        if allow { Effect::Trust } else { Effect::None },
                    ))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.answer(request, &option, cx)
                    }))
            })
            .collect::<Vec<_>>();
        div()
            .id(("agents-card", request as usize))
            .relative()
            .semantics(
                Semantics::new(Role::Group)
                    .name(format!(
                        "Permission request {request}: {}",
                        masked(&p.title)
                    ))
                    .test_id(format!("agents-card-{request}")),
            )
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded(px(6.))
            .border_1()
            .border_color(c(pal.focus))
            .bg(c(pal.island))
            .child(div().text_xs().text_color(c(pal.ui_dim)).child(format!(
                "Agent asks ({})",
                if p.kind.is_empty() {
                    "other"
                } else {
                    p.kind.as_str()
                }
            )))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(masked(&p.title)),
            )
            .when_some(p.similar_refused, |d, r| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(c(pal.status_text(crate::palette::Status::Warning)))
                        .child(format!(
                            "You refused a similar request in this turn (request {r})"
                        )),
                )
            })
            .when(!lines.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .font_family(crate::fonts::editor_family())
                        .text_xs()
                        .children(lines.into_iter().map(|(l, colour)| {
                            div().whitespace_nowrap().text_color(c(colour)).child(l)
                        }))
                        .when(more > 0, |d| {
                            d.child(
                                div()
                                    .text_color(c(pal.ui_dim))
                                    .child(format!("… {more} more line(s)")),
                            )
                        }),
                )
            })
            .child(div().flex().gap_2().children(buttons))
    }

    fn changes_block(&self, t: &TurnChanges, cx: &mut Context<Self>) -> Div {
        let pal = Palette::current();
        let turn = t.turn;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded(px(6.))
            .border_1()
            .border_color(c(pal.separator))
            .relative()
            .semantics(
                Semantics::new(Role::Group)
                    .name(format!("Turn {turn} changed {} file(s)", t.files.len()))
                    .test_id("agents-changes"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_xs().child(format!(
                        "Turn {turn} changed {} file(s){}",
                        t.files.len(),
                        if t.undone { " — undone" } else { "" }
                    )))
                    .when(!t.undone, |d| {
                        d.child(
                            button("agents-undo", format!("Undo turn {turn}"))
                                .relative()
                                .semantics(button_node(
                                    &format!("Undo turn {turn}"),
                                    "agents-undo".into(),
                                    Effect::Write,
                                ))
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.undo(turn, cx)
                                })),
                        )
                    }),
            )
            .children(t.files.iter().enumerate().map(|(i, f)| {
                let kind = match f.kind {
                    ChangeKind::Created => "created",
                    ChangeKind::Modified => "modified",
                    ChangeKind::Deleted => "deleted",
                };
                let counts = if f.binary {
                    "binary".to_owned()
                } else {
                    format!("+{} −{}", f.added, f.removed)
                };
                let path = self.root.join(&f.path);
                let open = f.kind != ChangeKind::Deleted;
                div()
                    .id(("agents-change-file", i))
                    .relative()
                    .semantics(
                        Semantics::new(Role::ListItem)
                            .name(format!("{} {kind} {counts}", f.path))
                            .test_id(format!("agents-change-file-{i}"))
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    )
                    .flex()
                    .gap_2()
                    .text_xs()
                    .cursor_pointer()
                    .hover(|s| s.bg(c(pal.tree_hover)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(f.path.clone()),
                    )
                    .child(
                        div()
                            .text_color(c(pal.ui_dim))
                            .child(format!("{kind} {counts}")),
                    )
                    .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                        if open {
                            cx.emit(AgentsEvent::OpenFile(path.clone()));
                        }
                    }))
            }))
    }
}

impl Render for AgentsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = Palette::current();
        let hosted = self.host.list();
        let snapshot = self.selected_snapshot();
        let entries = self.entries();
        let changes = self.newest_changes();
        let configured: Vec<(String, String)> = self
            .config
            .config
            .agent
            .iter()
            .map(|a| (a.id.clone(), a.name.clone().unwrap_or_else(|| a.id.clone())))
            .collect();
        let can_prompt = snapshot.as_ref().is_some_and(|s| {
            matches!(s.state, AgentState::WaitingForInput | AgentState::Idle) && !s.exited
        });
        let why_not = match &snapshot {
            None => "Start an agent, then describe the task here".to_owned(),
            Some(s) if can_prompt => {
                let _ = s;
                "Describe the task for the agent (Enter sends, Shift+Enter: new line)".to_owned()
            }
            Some(s) => format!(
                "The agent is {}: {}",
                state_words(s.state).to_lowercase(),
                masked(&s.detail)
            ),
        };
        let pending = snapshot
            .as_ref()
            .map(|s| s.pending_approvals.clone())
            .unwrap_or_default();
        // A new card or change list comes into view (behaviours 8, 11).
        let end = (
            pending.len(),
            changes
                .as_ref()
                .map_or(0, |t| t.files.len() + usize::from(t.undone)),
        );
        if end != self.shown_end {
            self.shown_end = end;
            self.scroll.scroll_to_bottom();
        }
        div()
            .id("agents")
            .key_context("Agents")
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("agents-top")
                    .flex_none()
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when_some(self.config.problem.clone(), |d, prob| {
                        d.child(
                            div()
                                .relative()
                                .semantics(Semantics::new(Role::Status).name(prob.clone()).test_id("agents-config-problem"))
                                .text_xs()
                                .text_color(c(pal.status_text(crate::palette::Status::Error)))
                                .child(prob),
                        )
                    })
                    .when(configured.is_empty() && self.config.problem.is_none(), |d| {
                        d.child(div().text_xs().text_color(c(pal.ui_muted)).child(
                            "No agent is configured. Add one found on this computer, or write it into agent-host.toml in your settings folder.",
                        ))
                    })
                    .children(configured.into_iter().map(|(id, name)| {
                        let test_id = format!("agents-start-{id}");
                        let agent = id.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(name.clone()))
                            .child(
                                button(gpui::SharedString::from(test_id.clone()), "Start")
                                    .relative()
                                    .semantics(button_node(&format!("Start {name}"), test_id, Effect::Run))
                                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        this.start(&agent, cx)
                                    })),
                            )
                    }))
                    .when(self.found.iter().any(|f| !f.configured), |d| {
                        d.child(div().pt_1().text_xs().text_color(c(pal.ui_dim)).child("Found on this computer"))
                    })
                    .children(self.found.iter().enumerate().filter(|(_, f)| !f.configured).map(|(i, f)| {
                        let test_id = format!("agents-add-{}", f.id);
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(format!("{} — {} {}", f.name, f.command.display(), f.args.join(" "))),
                            )
                            .child(
                                button(gpui::SharedString::from(test_id.clone()), "Add")
                                    .relative()
                                    // Configuring a program is the person's decision (behaviour 3).
                                    .semantics(button_node(&format!("Add {}", f.name), test_id, Effect::Trust))
                                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        this.add(i, cx)
                                    })),
                            )
                    }))
                    .when(!hosted.is_empty(), |d| {
                        d.child(div().pt_1().text_xs().text_color(c(pal.ui_dim)).child("Running and finished agents"))
                    })
                    .children(hosted.iter().map(|s| {
                        let on = self.selected.as_deref() == Some(s.id.as_str());
                        let id = s.id.clone();
                        let stop_id = s.id.clone();
                        let words = state_words(s.state);
                        let attention = matches!(s.state, AgentState::WaitingForApproval | AgentState::WaitingForInput | AgentState::Failed | AgentState::Unknown);
                        div()
                            .id(gpui::SharedString::from(format!("agents-row-{}", s.id)))
                            .relative()
                            .semantics(
                                Semantics::new(Role::ListItem)
                                    .name(format!("{} {words}", s.id))
                                    .selected(on)
                                    .test_id(format!("agents-row-{}", s.id))
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_1()
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(pal.tree_selection)))
                            .when(!on, |d| d.hover(|h| h.bg(c(pal.tree_hover))))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.select(&id, cx)))
                            .child(div().flex_1().min_w_0().child(s.id.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .when(attention, |d| d.text_color(c(pal.status_text(crate::palette::Status::Warning))))
                                    .when(!attention, |d| d.text_color(c(pal.ui_dim)))
                                    .child(words),
                            )
                            .when(!s.exited, |d| {
                                let test_id = format!("agents-stop-{}", s.id);
                                d.child(
                                    button(gpui::SharedString::from(test_id.clone()), "Stop")
                                        .relative()
                                        .semantics(button_node("Stop", test_id, Effect::None))
                                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.stop(&stop_id, cx)
                                        })),
                                )
                            })
                    }))
                    .when_some(self.note.clone(), |d, n| {
                        d.child(
                            div()
                                .relative()
                                .semantics(Semantics::new(Role::Status).name(n.clone()).test_id("agents-note"))
                                .text_xs()
                                .text_color(c(pal.ui_muted))
                                .child(n),
                        )
                    }),
            )
            .child(div().h(px(1.)).flex_none().bg(c(pal.separator)))
            .child(
                div()
                    .id("agents-output")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(entries.iter().filter(|e| e.kind != "state").map(|e| {
                        let dim = matches!(e.kind.as_str(), "thought" | "stderr" | "host");
                        let strong = e.kind == "prompt";
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_xs().text_color(c(pal.ui_dim)).child(e.kind.clone()))
                            .child(
                                div()
                                    .text_sm()
                                    .when(dim, |d| d.text_color(c(pal.ui_muted)))
                                    .when(strong, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .child(masked(&e.text)),
                            )
                    }))
                    .children(pending.iter().map(|p| self.card(p, cx)))
                    .when_some(changes, |d, t| d.child(self.changes_block(&t, cx))),
            )
            .child(
                div()
                    .flex_none()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(c(pal.ui_dim)).child(why_not))
                    .child(
                        div()
                            .id("agents-prompt")
                            .capture_key_down(cx.listener(Self::on_key))
                            .relative()
                            .semantics(
                                Semantics::new(Role::MultilineTextInput)
                                    .name("Task for the agent")
                                    .test_id("agents-prompt"),
                            )
                            .min_h(px(44.))
                            .p_1()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(c(pal.separator))
                            .child(self.input.clone()),
                    )
                    .child(
                        div().flex().justify_end().child(
                            button("agents-send", "Send")
                                .relative()
                                .semantics(button_node("Send", "agents-send".into(), Effect::Run))
                                .when(!can_prompt, |d| d.opacity(0.5))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.send(cx))),
                        ),
                    ),
            )
    }
}

/// Whether `path` lies in `root` (for opening a changed file).
pub fn inside(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

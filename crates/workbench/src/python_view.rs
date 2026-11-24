//! The Python Packages tool window (`SPEC-0117`): the selected interpreter, declared dependencies
//! the environment does not satisfy, and the installed packages; a field installs what is typed.
//! A single name typed into the field searches the package index; Enter on a match shows its
//! versions and the install command (`SPEC-0171` behaviour 6).

use std::path::PathBuf;
use std::time::Duration;

use ferris_idea_core::{InstallPlan, Interpreter, PackageInfo, PythonEnvironments, pip_spec};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription, Window, div,
    prelude::*, px, rgb,
};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the window asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonEvent {
    /// Hide the window.
    Hide,
    /// Install these requirements into the selected environment.
    Install(Vec<String>),
    /// Open the interpreter chooser.
    ChooseInterpreter,
    /// Create a virtual environment in the workspace.
    CreateEnvironment,
    /// Search the package index for names like this (SPEC-0171).
    Search(String),
    /// Load the details of this package from the index.
    Details(String),
    /// Run this install command; the text names what is installed.
    InstallCommand(Vec<String>, String),
}

/// An install that failed or could not start, shown above the list (SPEC-0117 v5 behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallFailure {
    /// What happened, e.g. `Install failed (exit code 1)`.
    pub title: String,
    /// The command that ran, if one ran.
    pub command: Option<String>,
    /// The last lines of its output, or the reason nothing ran.
    pub output: Vec<String>,
}

/// Most output lines of a failed command shown in the window.
pub const FAILURE_LINES: usize = 12;

impl InstallFailure {
    /// The failure of a command that ran: its command line and the last [`FAILURE_LINES`] lines
    /// of its output.
    pub fn of_run(run: &ferris_idea_core::ToolRun) -> Self {
        let lines: Vec<&str> = run.output.lines().collect();
        Self {
            title: match run.exit_code {
                Some(code) => format!("Command failed (exit code {code})"),
                None => "Command could not be started".into(),
            },
            command: Some(run.command.join(" ")),
            output: lines[lines.len().saturating_sub(FAILURE_LINES)..]
                .iter()
                .map(|l| (*l).to_owned())
                .collect(),
        }
    }

    /// The lines as the window shows them: the title, `$ command`, then the output.
    pub fn texts(&self) -> Vec<String> {
        let mut lines = vec![self.title.clone()];
        lines.extend(self.command.iter().map(|c| format!("$ {c}")));
        lines.extend(self.output.iter().cloned());
        lines
    }
}

/// Pause after typing before the index is searched.
const SEARCH_DELAY: Duration = Duration::from_millis(250);

/// What the list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pane {
    /// Unmet declarations and installed packages.
    Installed,
    /// Index names matching the query.
    Matches(String, Vec<String>),
    /// One package from the index with the chosen release (index into its releases).
    Details(Box<PackageInfo>, usize),
}

/// Whether `text` is a name to search for rather than requirements to install: one word of
/// name characters, at least 2 long.
pub fn is_search_query(text: &str) -> bool {
    let t = text.trim();
    t.chars().count() >= 2
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// One row: a declared dependency that is not satisfied, or an installed package.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    /// Name, requirement, installed version, pip requirement that fixes it.
    Missing(String, String, Option<String>, String),
    /// Name and version.
    Package(String, String),
}

/// The window.
pub struct PythonView {
    label: String,
    detail: Vec<String>,
    rows: Vec<Row>,
    selected: usize,
    busy: Option<String>,
    /// The last install that failed, until the next command starts.
    failure: Option<InstallFailure>,
    /// Folders searched for installers instead of `PATH` (tests).
    tool_path: Option<std::ffi::OsString>,
    loading: bool,
    field: Entity<EditorView>,
    focus: FocusHandle,
    pane: Pane,
    /// Search or loading state, or why nothing was found.
    note: Option<String>,
    /// Last query sent to the index.
    searched: String,
    /// Workspace and selected interpreter, for the install command.
    root: PathBuf,
    interpreter: Option<Interpreter>,
    installed: Vec<(String, String)>,
    /// The matches a shown package was chosen from (Escape returns to them).
    matches_back: Option<(String, Vec<String>)>,
    _watch: Subscription,
}

impl EventEmitter<PythonEvent> for PythonView {}

fn rows_of(env: &PythonEnvironments) -> Vec<Row> {
    let mut rows: Vec<Row> = env
        .unsatisfied
        .iter()
        .map(|u| {
            Row::Missing(
                u.name.clone(),
                u.requirement.clone(),
                u.installed.clone(),
                pip_spec(u),
            )
        })
        .collect();
    let mut packages = env.packages.clone();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    rows.extend(
        packages
            .into_iter()
            .map(|p| Row::Package(p.name, p.version)),
    );
    rows
}

impl PythonView {
    /// An empty window for the workspace `root`, loading.
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let field = cx.new(EditorView::field);
        let watch = cx.observe(&field, |this, _, cx| this.field_changed(cx));
        Self {
            label: String::new(),
            detail: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            busy: None,
            failure: None,
            tool_path: None,
            loading: true,
            field,
            focus: cx.focus_handle(),
            pane: Pane::Installed,
            note: None,
            searched: String::new(),
            root,
            interpreter: None,
            installed: Vec::new(),
            matches_back: None,
            _watch: watch,
        }
    }

    fn query(&self, cx: &gpui::App) -> String {
        self.field.read(cx).state.buffer.text().trim().to_owned()
    }

    /// The field changed: a name searches the index after a short pause, anything else shows
    /// the installed list (SPEC-0171 behaviour 6).
    fn field_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.query(cx);
        if !is_search_query(&text) {
            self.searched.clear();
            if !matches!(self.pane, Pane::Installed) {
                self.pane = Pane::Installed;
                self.note = None;
                self.selected = 0;
                cx.notify();
            }
            return;
        }
        if text == self.searched {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.query(cx) == text && this.searched != text {
                    this.searched = text.clone();
                    this.note = Some(format!("Searching for {text}…"));
                    cx.emit(PythonEvent::Search(text));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Shows the names found for `query`, unless the field asks for something else by now.
    pub fn show_matches(&mut self, query: &str, names: Vec<String>, cx: &mut Context<Self>) {
        if self.query(cx) != query {
            return;
        }
        self.note = names
            .is_empty()
            .then(|| format!("No package on the index is named like {query}"));
        self.pane = Pane::Matches(query.to_owned(), names);
        self.selected = 0;
        cx.notify();
    }

    /// Shows one package from the index with its latest release chosen.
    pub fn show_details(&mut self, info: PackageInfo, cx: &mut Context<Self>) {
        let chosen = info
            .releases
            .iter()
            .position(|r| r.version == info.latest)
            .unwrap_or(0);
        self.note = None;
        self.pane = Pane::Details(Box::new(info), chosen);
        cx.notify();
    }

    /// Shows a line about the search (loading, failure).
    pub fn show_note(&mut self, note: Option<String>, cx: &mut Context<Self>) {
        self.note = note;
        cx.notify();
    }

    /// The version installed in the selected environment for `name`.
    fn installed_version(&self, name: &str) -> Option<&str> {
        let norm = ferris_idea_core::normalize_name(name);
        self.installed
            .iter()
            .find(|(n, _)| *n == norm)
            .map(|(_, v)| v.as_str())
    }

    /// The install plan for the shown package and chosen release.
    pub fn install_plan(&self) -> Option<InstallPlan> {
        let Pane::Details(info, chosen) = &self.pane else {
            return None;
        };
        let version = info.releases.get(*chosen).map(|r| r.version.as_str());
        Some(ferris_idea_core::install_plan(
            &self.root,
            self.interpreter.as_ref(),
            &ferris_idea_core::requirement(&info.name, version),
            &|program| ferris_idea_core::find_tool(self.tool_path.as_deref(), program),
        ))
    }

    /// The command line shown before installing (for tests and the window).
    pub fn command_text(&self) -> Option<String> {
        let plan = self.install_plan()?;
        Some(match &plan.refused {
            Some(why) => format!("cannot install: {why}"),
            None => plan.command.join(" "),
        })
    }

    fn install_chosen(&mut self, cx: &mut Context<Self>) {
        let (Some(plan), Pane::Details(info, chosen)) = (self.install_plan(), &self.pane) else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        if let Some(why) = plan.refused {
            self.note = Some(why);
            cx.notify();
            return;
        }
        let version = info
            .releases
            .get(*chosen)
            .map(|r| r.version.clone())
            .unwrap_or_default();
        cx.emit(PythonEvent::InstallCommand(
            plan.command,
            format!("{} {version}", info.name),
        ));
    }

    /// Handles a key as the window does (for tests); returns whether it was used.
    pub fn press(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        self.pane_key(key, cx)
    }

    /// Keys of the search panes (SPEC-0171 behaviour 6); false when not used here.
    fn pane_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match (&mut self.pane, key) {
            (Pane::Installed, _) => return false,
            (Pane::Matches(_, names), "down") => {
                self.selected = (self.selected + 1).min(names.len().saturating_sub(1))
            }
            (Pane::Matches(..), "up") => self.selected = self.selected.saturating_sub(1),
            (Pane::Matches(query, names), "enter") => {
                let Some(name) = names.get(self.selected).cloned() else {
                    return true;
                };
                self.matches_back = Some((query.clone(), names.clone()));
                self.note = Some(format!("Loading {name}…"));
                cx.emit(PythonEvent::Details(name));
            }
            (Pane::Matches(..), "escape") => {
                self.pane = Pane::Installed;
                self.note = None;
                self.selected = 0;
                self.searched.clear();
                self.field.update(cx, |f, cx| f.set_text("", cx));
            }
            (Pane::Details(info, chosen), "right" | "down") => {
                *chosen = (*chosen + 1).min(info.releases.len().saturating_sub(1))
            }
            (Pane::Details(_, chosen), "left" | "up") => *chosen = chosen.saturating_sub(1),
            (Pane::Details(..), "enter") => self.install_chosen(cx),
            (Pane::Details(..), "escape") => {
                // Back to the matches the package was chosen from.
                self.note = None;
                self.pane = match self.matches_back.take() {
                    Some((query, names)) => Pane::Matches(query, names),
                    None => Pane::Installed,
                };
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Shows `env` with `label` for its interpreter (behaviour 4).
    pub fn show(&mut self, env: &PythonEnvironments, label: String, cx: &mut Context<Self>) {
        self.label = label;
        self.interpreter = env.selected.and_then(|i| env.interpreters.get(i)).cloned();
        self.installed = env
            .packages
            .iter()
            .map(|p| (p.name.clone(), p.version.clone()))
            .collect();
        self.detail = env
            .reason
            .iter()
            .cloned()
            .chain(env.notes.clone())
            .collect();
        self.rows = rows_of(env);
        self.loading = false;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        cx.notify();
    }

    /// Marks the window as reading the environment again.
    pub fn loading(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        cx.notify();
    }

    /// Shows that a command runs (`Some`) or has ended (`None`). A new command takes the last
    /// failure away.
    pub fn set_busy(&mut self, busy: Option<String>, cx: &mut Context<Self>) {
        if busy.is_some() {
            self.failure = None;
        }
        self.busy = busy;
        cx.notify();
    }

    /// Shows an install that failed or could not start above the list (SPEC-0117 v5 behaviour
    /// 9), or takes it away.
    pub fn show_failure(&mut self, failure: Option<InstallFailure>, cx: &mut Context<Self>) {
        self.failure = failure;
        cx.notify();
    }

    /// The failure shown, if any.
    pub fn failure(&self) -> Option<&InstallFailure> {
        self.failure.as_ref()
    }

    /// Looks for installers in the folders of `paths` instead of `PATH` (tests).
    pub fn set_tool_path(&mut self, paths: Option<std::ffi::OsString>, cx: &mut Context<Self>) {
        self.tool_path = paths;
        cx.notify();
    }

    /// Focus handle of the list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Focus handle of the install field.
    pub fn field_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus()
    }

    /// Types `text` into the install field (for tests). The change is handled here as well: a
    /// notification sent while the window draws (as the self-test's steps do) reaches no observer.
    pub fn set_field(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set_text(text, cx));
        self.field_changed(cx);
    }

    /// The interpreter label shown in the header.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The rows as text (for tests): missing dependencies with `!`, packages as `name version`;
    /// matches as `name` or `name (installed v)`; details as summary lines, then releases with
    /// the chosen one marked `●`.
    pub fn texts(&self) -> Vec<String> {
        match &self.pane {
            Pane::Installed => {}
            Pane::Matches(_, names) => {
                return names
                    .iter()
                    .map(|n| match self.installed_version(n) {
                        Some(v) => format!("{n} (installed {v})"),
                        None => n.clone(),
                    })
                    .collect();
            }
            Pane::Details(info, chosen) => {
                let mut lines = vec![format!("{} {} — {}", info.name, info.latest, info.summary)];
                if let Some(rp) = &info.requires_python {
                    lines.push(format!("requires-python {rp}"));
                }
                lines.extend(info.releases.iter().enumerate().map(|(i, r)| {
                    let mark = if i == *chosen { "●" } else { " " };
                    let pre = if r.prerelease { " (pre-release)" } else { "" };
                    let here = if self.installed_version(&info.name) == Some(r.version.as_str()) {
                        " (installed)"
                    } else {
                        ""
                    };
                    format!("{mark} {}{pre}{here}", r.version)
                }));
                return lines;
            }
        }
        self.rows
            .iter()
            .map(|r| match r {
                Row::Missing(name, req, installed, _) => match installed {
                    Some(v) => format!("! {name} {req} (installed {v})"),
                    None => format!("! {name} {req} (missing)"),
                },
                Row::Package(name, version) => format!("{name} {version}"),
            })
            .collect()
    }

    /// Whether a command is running.
    pub fn busy(&self) -> Option<&str> {
        self.busy.as_deref()
    }

    /// Requirements that fix every unsatisfied dependency.
    pub fn missing_specs(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Missing(.., spec) => Some(spec.clone()),
                Row::Package(..) => None,
            })
            .collect()
    }

    /// Installs what the field holds (behaviour 5); an empty field does nothing.
    pub fn install_typed(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).state.buffer.text();
        let specs = ferris_idea_core::parse_specs(&text);
        if specs.is_empty() || self.busy.is_some() {
            return;
        }
        self.field.update(cx, |f, cx| f.set_text("", cx));
        cx.emit(PythonEvent::Install(specs));
    }

    fn install_missing(&mut self, cx: &mut Context<Self>) {
        let specs = self.missing_specs();
        if !specs.is_empty() && self.busy.is_none() {
            cx.emit(PythonEvent::Install(specs));
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let in_field = self.field.read(cx).focus().is_focused(window);
        let n = self.rows.len();
        // The first and the last row of the installed packages (`SPEC-0275` v19 behaviour 47);
        // the search panes and the name field keep their keys.
        if !in_field
            && matches!(self.pane, Pane::Installed)
            && let Some(to) = crate::list_nav::edge(&event.keystroke, n)
        {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.pane_key(event.keystroke.key.as_str(), cx) {
            cx.stop_propagation();
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" if in_field => self.install_typed(cx),
            "escape" => cx.emit(PythonEvent::Hide),
            "down" if !in_field && n > 0 => self.selected = (self.selected + 1).min(n - 1),
            "up" if !in_field => self.selected = self.selected.saturating_sub(1),
            // Enter on a missing dependency installs just that one.
            "enter" => match self.rows.get(self.selected) {
                Some(Row::Missing(.., spec)) if self.busy.is_none() => {
                    cx.emit(PythonEvent::Install(vec![spec.clone()]))
                }
                _ => return,
            },
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for PythonView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let missing = self.missing_specs().len();
        let packages = self.rows.len() - missing;
        let rows = if matches!(self.pane, Pane::Installed) {
            self.rows.clone()
        } else {
            Vec::new()
        };
        let pane = self.pane.clone();
        let command = self.command_text();
        let search_lines = if matches!(pane, Pane::Installed) {
            Vec::new()
        } else {
            self.texts()
        };
        let button = |id: &'static str, label: String| {
            div()
                .id(id)
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(label)
        };
        div()
            .id("python-panel")
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title("Python Packages"))
                    .child(button("python-interpreter", self.label.clone()).on_click(
                        cx.listener(|_, _, _, cx| cx.emit(PythonEvent::ChooseInterpreter)),
                    ))
                    .child(div().text_color(c(p.ui_muted)).child(if self.loading {
                        "Reading the environment…".to_owned()
                    } else {
                        format!("{packages} installed")
                    }))
                    .child(div().flex_1())
                    .when_some(self.busy.clone(), |d, b| {
                        d.child(div().text_color(c(p.ui_muted)).child(b))
                    })
                    .when(missing > 0, |d| {
                        d.child(
                            button(
                                "python-install-missing",
                                format!("Install missing ({missing})"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.install_missing(cx))),
                        )
                    })
                    .child(button("python-new-env", "New Environment".into()).on_click(
                        cx.listener(|_, _, _, cx| cx.emit(PythonEvent::CreateEnvironment)),
                    )),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .child(div().text_color(c(p.ui_muted)).child("Install"))
                    .child(
                        div()
                            .flex_1()
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .child(self.field.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child("a name searches the index; requirements (rich>=13) install"),
                    ),
            )
            .children(self.detail.iter().map(|d| {
                div()
                    .px_3()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(d.clone())
            }))
            .when_some(self.note.clone(), |d, note| {
                d.child(div().px_3().text_xs().text_color(c(p.ui_muted)).child(note))
            })
            .when_some(self.failure.clone(), |d, failure| {
                let lines = failure.texts();
                d.child(
                    div()
                        .id("python-install-failure")
                        .mx_3()
                        .my_1()
                        .px_2()
                        .py_1()
                        .flex_none()
                        .max_h(px(220.))
                        .overflow_y_scroll()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.error_text))
                        .children(lines.into_iter().enumerate().map(|(i, line)| {
                            div()
                                .text_xs()
                                .when(i == 0, |d| d.text_color(c(p.error_text)))
                                .when(i > 0, |d| d.font_family("monospace"))
                                .child(line)
                        })),
                )
            })
            .when_some(command, |d, command| {
                d.child(
                    div()
                        .id("python-install-command")
                        .px_3()
                        .py_1()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(div().text_color(c(p.ui_muted)).child("Enter runs"))
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .font_family("monospace")
                                .text_xs()
                                .child(command),
                        ),
                )
            })
            .child(
                div()
                    .id("python-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .px_1()
                    .children(search_lines.into_iter().enumerate().map(|(i, line)| {
                        // Matches: the selected row; details: the chosen release (marked ●).
                        let on = match &pane {
                            Pane::Matches(..) => i == selected,
                            _ => line.starts_with('●'),
                        };
                        let release = match &pane {
                            Pane::Details(info, _) => {
                                let head = 1 + usize::from(info.requires_python.is_some());
                                i.checked_sub(head)
                            }
                            _ => None,
                        };
                        let header = matches!(pane, Pane::Details(..)) && release.is_none();
                        div()
                            .id(("python-search-row", i))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .px_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(header, |d| d.text_color(c(p.ui_muted)))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on && !header, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match (&mut this.pane, release) {
                                    (Pane::Matches(..), _) => this.selected = i,
                                    (Pane::Details(_, chosen), Some(r)) => *chosen = r,
                                    _ => {}
                                }
                                cx.notify();
                            }))
                            .child(line)
                    }))
                    .when(
                        rows.is_empty() && !self.loading && matches!(pane, Pane::Installed),
                        |d| {
                            d.child(div().px_3().py_2().text_color(c(p.ui_muted)).child(
                                "No packages: choose an interpreter or create an environment",
                            ))
                        },
                    )
                    .children(rows.into_iter().enumerate().map(|(i, row)| {
                        let on = i == selected;
                        let base = div()
                            .id(("python-row", i))
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = i;
                                cx.notify();
                            }));
                        match row {
                            Row::Missing(name, req, installed, _) => base
                                .text_color(rgb(p.console_ansi_text[3]))
                                .child(format!("⚠ {name}"))
                                .child(div().text_color(c(p.ui_muted)).child(req))
                                .child(div().flex_1())
                                .child(div().text_color(c(p.ui_dim)).child(match installed {
                                    Some(v) => format!("installed {v}; Enter installs"),
                                    None => "missing; Enter installs".into(),
                                })),
                            Row::Package(name, version) => base
                                .child(name)
                                .child(div().flex_1())
                                .child(div().text_color(c(p.ui_muted)).child(version)),
                        }
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_core::{Package, Unsatisfied};

    #[test]
    fn a_single_name_searches_and_requirements_install() {
        assert!(is_search_query("rich"));
        assert!(is_search_query(" requests_toolbelt "));
        assert!(is_search_query("zope.interface"));
        assert!(!is_search_query("r"));
        assert!(!is_search_query("rich>=13"));
        assert!(!is_search_query("rich flask"));
        assert!(!is_search_query("rich[jupyter]"));
        assert!(!is_search_query(""));
    }

    #[test]
    fn a_failed_command_shows_its_command_and_output_tail() {
        let output: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        let run = ferris_idea_core::ToolRun {
            command: vec!["/v/bin/python".into(), "-m".into(), "pip".into()],
            ok: false,
            exit_code: Some(1),
            output: output.join("\n"),
        };
        let texts = InstallFailure::of_run(&run).texts();
        assert_eq!(texts[0], "Command failed (exit code 1)");
        assert_eq!(texts[1], "$ /v/bin/python -m pip");
        assert_eq!(texts.len(), 2 + FAILURE_LINES);
        assert_eq!(texts[2], "line 19");
        assert_eq!(texts.last().unwrap(), "line 30");
        let unstarted = ferris_idea_core::ToolRun {
            exit_code: None,
            output: "uv: No such file".into(),
            ..run
        };
        assert_eq!(
            InstallFailure::of_run(&unstarted).texts()[0],
            "Command could not be started"
        );
    }

    #[test]
    fn missing_dependencies_come_first_then_sorted_packages() {
        let env = PythonEnvironments {
            interpreters: Vec::new(),
            selected: None,
            reason: None,
            requires_python: None,
            compatible: None,
            packages: vec![
                Package {
                    name: "rich".into(),
                    version: "13.7".into(),
                },
                Package {
                    name: "pip".into(),
                    version: "24.0".into(),
                },
            ],
            unsatisfied: vec![
                Unsatisfied {
                    name: "requests".into(),
                    requirement: ">=2.31".into(),
                    installed: Some("2.28.0".into()),
                },
                Unsatisfied {
                    name: "flask".into(),
                    requirement: "^3.0".into(),
                    installed: None,
                },
            ],
            conditional: Vec::new(),
            test_framework: "unittest".into(),
            test_config: None,
            notes: Vec::new(),
        };
        let rows = rows_of(&env);
        assert_eq!(
            rows[0],
            Row::Missing(
                "requests".into(),
                ">=2.31".into(),
                Some("2.28.0".into()),
                "requests>=2.31".into()
            )
        );
        assert_eq!(
            rows[1],
            Row::Missing("flask".into(), "^3.0".into(), None, "flask".into())
        );
        assert_eq!(rows[2], Row::Package("pip".into(), "24.0".into()));
        assert_eq!(rows[3], Row::Package("rich".into(), "13.7".into()));
    }
}

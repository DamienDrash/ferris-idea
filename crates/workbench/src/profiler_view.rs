//! The Profiler tool window (`SPEC-0120`): the last profile as call tree, hot spots and flame
//! graph; Enter opens the source of the selected function. Memory profiles (`SPEC-0250`) read in
//! bytes; the process list attaches to a running program (`SPEC-0251`). Frames outside the
//! project are dimmed and "Only project code" filters them (`SPEC-0120` v5 behaviours 11, 12); a
//! click on a line's cost in the editor selects its function here (v5 behaviour 10).

use std::collections::HashSet;
use std::path::PathBuf;

use ferris_idea_profiler::{
    HotSpot, Kind, ProcessInfo, Profile, ProfileNode, ProfileRun, Unit, ViewSettings, hot_spots,
};
use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, relative, rgb,
    rgba,
};

use crate::palette::Palette;
use crate::semantics::{Annotated, Role, Semantics};
use crate::workbench_view::c;

/// What the window asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfilerEvent {
    /// Hide the window.
    Hide,
    /// Profile the selected run configuration again, for CPU or memory.
    Again(Kind),
    /// Attach to this process for CPU or a memory snapshot (`SPEC-0251`).
    Attach(u32, Kind),
    /// Close the process list.
    ClosePicker,
    /// Open this source file (as reported) at this 1-based line.
    Open(String, u32),
    /// The profile shown changed: another run, a failed one, or none after Close; the editors'
    /// costs follow (`SPEC-0120` v5 behaviour 10).
    Changed,
}

/// The three views of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilerTab {
    /// Callers above callees, expandable.
    CallTree,
    /// Functions by self cost.
    HotSpots,
    /// Width is cost, depth is the call stack.
    FlameGraph,
}

impl ProfilerTab {
    const ALL: [ProfilerTab; 3] = [Self::CallTree, Self::HotSpots, Self::FlameGraph];

    fn label(self) -> &'static str {
        match self {
            Self::CallTree => "Call Tree",
            Self::HotSpots => "Hot Spots",
            Self::FlameGraph => "Flame Graph",
        }
    }
}

/// One visible call tree row: path of child indices from a root.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeRow {
    path: Vec<usize>,
    depth: usize,
    has_children: bool,
}

/// Deepest flame graph level drawn; deeper frames are left out.
const FLAME_DEPTH: usize = 32;

/// The window.
pub struct ProfilerView {
    title: String,
    state: Result<Option<ProfileRun>, String>,
    busy: bool,
    tab: ProfilerTab,
    expanded: HashSet<Vec<usize>>,
    rows: Vec<TreeRow>,
    spots: Vec<HotSpot>,
    selected: usize,
    /// Path of the flame graph frame clicked last.
    flame_selected: Option<Vec<usize>>,
    /// The process list while picking a process to attach to (`SPEC-0251`).
    picker: Option<Picker>,
    /// The workspace, where "Only project code" is kept (`SPEC-0120` v5 behaviour 12).
    root: Option<PathBuf>,
    /// "Only project code" is on.
    only_project: bool,
    /// The profile folded to the project's frames while "Only project code" is on.
    folded: Option<Profile>,
    focus: FocusHandle,
}

/// Processes to attach to, a typed filter and the selected row.
#[derive(Debug, Clone, Default)]
struct Picker {
    processes: Vec<ProcessInfo>,
    filter: String,
    selected: usize,
    note: Option<String>,
}

impl Picker {
    fn shown(&self) -> Vec<&ProcessInfo> {
        let f = self.filter.to_lowercase();
        self.processes
            .iter()
            .filter(|p| {
                f.is_empty()
                    || p.command.to_lowercase().contains(&f)
                    || p.pid.to_string().starts_with(&f)
            })
            .collect()
    }
}

/// One process row: `pid  command`, marked when it cannot be attached to.
fn process_text(p: &ProcessInfo) -> String {
    let mut text = format!("{:>6}  {}", p.pid, p.command);
    if p.refused.is_some() {
        text.push_str("  — refused");
    }
    text
}

impl EventEmitter<ProfilerEvent> for ProfilerView {}

fn node_at<'a>(roots: &'a [ProfileNode], path: &[usize]) -> Option<&'a ProfileNode> {
    let (first, rest) = path.split_first()?;
    let mut node = roots.get(*first)?;
    for i in rest {
        node = node.children.get(*i)?;
    }
    Some(node)
}

/// Cost as time (a sample is one millisecond) or as a size.
fn time_label(unit: Unit, cost: f64) -> String {
    let ms = match unit {
        Unit::Samples => cost,
        Unit::Seconds => cost * 1000.0,
        Unit::Bytes => return ferris_idea_profiler::size_label(cost),
    };
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else {
        format!("{ms:.0} ms")
    }
}

/// A hot spot's own time: for samples its share of the run's wall time, since engines sample at
/// different rates (D37: JFR's 134 samples over 7.7 s read as "95 ms" for 71 %).
pub(crate) fn spot_time(unit: Unit, cost: f64, total: f64, millis: u64) -> String {
    match unit {
        Unit::Samples if total > 0.0 && millis > 0 => {
            time_label(Unit::Samples, cost / total * millis as f64)
        }
        _ => time_label(unit, cost),
    }
}

pub(crate) fn percent(cost: f64, total: f64) -> String {
    if total <= 0.0 {
        return "0.0 %".into();
    }
    format!("{:.1} %", cost / total * 100.0)
}

fn location_label(location: &Option<(String, u32)>) -> String {
    location
        .as_ref()
        .map(|(f, l)| {
            let base = f.rsplit(['/', '\\']).next().unwrap_or(f);
            format!("  {base}:{l}")
        })
        .unwrap_or_default()
}

/// Whether `n` is a frame outside the project, set apart in every view (`SPEC-0120` v5
/// behaviour 11); threads are no frames.
fn outside(n: &ProfileNode) -> bool {
    !n.in_project && !n.is_thread()
}

impl ProfilerView {
    /// An empty window.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            title: String::new(),
            state: Ok(None),
            busy: false,
            tab: ProfilerTab::CallTree,
            expanded: HashSet::new(),
            rows: Vec::new(),
            spots: Vec::new(),
            selected: 0,
            flame_selected: None,
            picker: None,
            root: None,
            only_project: false,
            folded: None,
            focus: cx.focus_handle(),
        }
    }

    /// The workspace: "Only project code" is read from its `.ferris-idea/profiles/view.json`
    /// (`SPEC-0120` v5 behaviour 12).
    pub fn set_project(&mut self, root: PathBuf) {
        self.only_project =
            ViewSettings::load(&root.join(ferris_idea_profiler::PROFILES_DIR)).only_project_code;
        self.root = Some(root);
    }

    /// Whether "Only project code" is on.
    pub fn only_project(&self) -> bool {
        self.only_project
    }

    /// Switches "Only project code" (`P`, the header's check box) and keeps the choice for the
    /// project.
    pub fn toggle_only_project(&mut self, cx: &mut Context<Self>) {
        self.only_project = !self.only_project;
        if let Some(root) = &self.root {
            let dir = root.join(ferris_idea_profiler::PROFILES_DIR);
            let _ = ViewSettings {
                only_project_code: self.only_project,
            }
            .save(&dir);
        }
        self.reshape();
        cx.notify();
    }

    /// The tree the views show: the profile, folded while "Only project code" is on.
    fn tree(&self) -> Option<&Profile> {
        let run = self.run()?;
        Some(match &self.folded {
            Some(f) if self.only_project => f,
            _ => &run.profile,
        })
    }

    /// Hot spots, the folded tree and the opened path after the profile or the filter changed.
    fn reshape(&mut self) {
        self.expanded.clear();
        self.selected = 0;
        self.flame_selected = None;
        let (folded, spots) = match self.run() {
            Some(run) => {
                let spots: Vec<HotSpot> = hot_spots(&run.profile)
                    .into_iter()
                    .filter(|h| !self.only_project || h.in_project)
                    .collect();
                (self.only_project.then(|| run.profile.project_only()), spots)
            }
            None => (None, Vec::new()),
        };
        self.folded = folded;
        self.spots = spots;
        let mut expanded = HashSet::new();
        if let Some(p) = self.tree() {
            for (i, root) in p.roots.iter().enumerate() {
                let mut path = vec![i];
                let mut node = root;
                while let Some(first) = node.children.first() {
                    if first.total < node.total * 0.5 {
                        break;
                    }
                    expanded.insert(path.clone());
                    path.push(0);
                    node = first;
                }
                // Other roots (threads) stay closed when the first one carries the run.
                if root.total >= p.total * 0.5 {
                    break;
                }
            }
        }
        self.expanded = expanded;
        self.rebuild();
    }

    /// Closes the profile shown: the window is empty and the editors lose its costs
    /// (`SPEC-0120` v5 behaviour 10).
    pub fn close_profile(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.run().is_none() && self.state.is_ok() {
            return;
        }
        self.state = Ok(None);
        self.reshape();
        cx.emit(ProfilerEvent::Changed);
        cx.notify();
    }

    /// Shows the Call Tree with the costliest call of `function` in the `file` (as reported)
    /// selected and its callers opened: what a click on a line's cost in the editor shows
    /// (`SPEC-0120` v5 behaviour 10). Returns whether it was found.
    pub fn reveal(&mut self, file: &str, function: &str, cx: &mut Context<Self>) -> bool {
        self.picker = None;
        let Some(p) = self.tree() else {
            return false;
        };
        fn find(
            n: &ProfileNode,
            path: &mut Vec<usize>,
            file: &str,
            function: &str,
            best: &mut Option<(f64, Vec<usize>)>,
        ) {
            let here = n.name == function && n.location.as_ref().is_some_and(|(f, _)| f == file);
            if here && best.as_ref().is_none_or(|(t, _)| n.total > *t) {
                *best = Some((n.total, path.clone()));
            }
            for (i, c) in n.children.iter().enumerate() {
                path.push(i);
                find(c, path, file, function, best);
                path.pop();
            }
        }
        let mut best = None;
        for (i, r) in p.roots.iter().enumerate() {
            find(r, &mut vec![i], file, function, &mut best);
        }
        let Some((_, path)) = best else {
            return false;
        };
        self.tab = ProfilerTab::CallTree;
        for end in 1..path.len() {
            self.expanded.insert(path[..end].to_vec());
        }
        self.rebuild();
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.selected = i;
        }
        cx.notify();
        true
    }

    /// For each row of the tab shown (the flame graph's frames as [`Self::texts`] lists them),
    /// whether it is a frame outside the project, drawn dimmed (`SPEC-0120` v5 behaviour 11).
    pub fn outside_rows(&self) -> Vec<bool> {
        let Some(p) = self.tree() else {
            return Vec::new();
        };
        match self.tab {
            ProfilerTab::CallTree => self
                .rows
                .iter()
                .filter_map(|r| node_at(&p.roots, &r.path).map(outside))
                .collect(),
            ProfilerTab::HotSpots => self.spots.iter().map(|h| !h.in_project).collect(),
            ProfilerTab::FlameGraph => {
                let mut out = Vec::new();
                fn walk(n: &ProfileNode, depth: usize, total: f64, out: &mut Vec<bool>) {
                    if depth >= FLAME_DEPTH || n.total < total * 0.005 {
                        return;
                    }
                    out.push(outside(n));
                    for c in &n.children {
                        walk(c, depth + 1, total, out);
                    }
                }
                for r in &p.roots {
                    walk(r, 0, p.total, &mut out);
                }
                out
            }
        }
    }

    /// Shows the user's processes to pick one to attach to (`SPEC-0251` behaviour 6).
    pub fn show_processes(&mut self, processes: Vec<ProcessInfo>, cx: &mut Context<Self>) {
        self.picker = Some(Picker {
            processes,
            ..Picker::default()
        });
        cx.notify();
    }

    /// Whether the process list is shown.
    pub fn picking(&self) -> bool {
        self.picker.is_some()
    }

    /// The process rows as text after the filter (for tests).
    pub fn process_texts(&self) -> Vec<String> {
        self.picker
            .as_ref()
            .map(|p| p.shown().into_iter().map(process_text).collect())
            .unwrap_or_default()
    }

    /// Why the selected process cannot be attached to, after Enter on it.
    pub fn picker_note(&self) -> Option<&str> {
        self.picker.as_ref().and_then(|p| p.note.as_deref())
    }

    /// Types `text` into the process filter (for tests too).
    pub fn type_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.picker {
            p.filter.push_str(text);
            p.selected = 0;
            p.note = None;
            cx.notify();
        }
    }

    /// The kind of the profile shown (CPU when none).
    pub fn shown_kind(&self) -> Kind {
        self.run().map(|r| r.kind).unwrap_or_default()
    }

    fn press_picker(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(picker) = &mut self.picker else {
            return false;
        };
        let n = picker.shown().len();
        match key {
            "down" if n > 0 => picker.selected = (picker.selected + 1).min(n - 1),
            "up" => picker.selected = picker.selected.saturating_sub(1),
            "backspace" => {
                picker.filter.pop();
                picker.selected = 0;
            }
            "enter" | "alt-enter" => {
                let Some(p) = picker.shown().get(picker.selected).map(|p| (*p).clone()) else {
                    return true;
                };
                if let Some(why) = p.refused {
                    picker.note = Some(why);
                } else {
                    let kind = if key == "alt-enter" {
                        Kind::Memory
                    } else {
                        Kind::Cpu
                    };
                    self.picker = None;
                    cx.emit(ProfilerEvent::Attach(p.pid, kind));
                }
            }
            "escape" => {
                self.picker = None;
                cx.emit(ProfilerEvent::ClosePicker);
            }
            k if k.chars().count() == 1 => {
                picker.filter.push_str(k);
                picker.selected = 0;
                picker.note = None;
            }
            "space" => picker.filter.push(' '),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Shows that `title` is being profiled.
    pub fn show_busy(&mut self, title: String, cx: &mut Context<Self>) {
        self.title = title;
        self.busy = true;
        cx.notify();
    }

    /// Shows a finished run (or why it failed). The most expensive path is opened down to the
    /// first function that spends most of its time itself.
    pub fn show_run(
        &mut self,
        title: String,
        run: Result<ProfileRun, String>,
        cx: &mut Context<Self>,
    ) {
        self.title = title;
        self.busy = false;
        self.state = run.map(Some);
        self.reshape();
        cx.emit(ProfilerEvent::Changed);
        cx.notify();
    }

    fn run(&self) -> Option<&ProfileRun> {
        self.state.as_ref().ok().and_then(Option::as_ref)
    }

    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        fn walk(
            node: &ProfileNode,
            path: &mut Vec<usize>,
            expanded: &HashSet<Vec<usize>>,
            rows: &mut Vec<TreeRow>,
        ) {
            rows.push(TreeRow {
                path: path.clone(),
                depth: path.len() - 1,
                has_children: !node.children.is_empty(),
            });
            if expanded.contains(path) {
                for (i, c) in node.children.iter().enumerate() {
                    path.push(i);
                    walk(c, path, expanded, rows);
                    path.pop();
                }
            }
        }
        if let Some(p) = self.tree() {
            for (i, r) in p.roots.iter().enumerate() {
                walk(r, &mut vec![i], &self.expanded, &mut rows);
            }
        }
        self.rows = rows;
        let n = match self.tab {
            ProfilerTab::CallTree => self.rows.len(),
            ProfilerTab::HotSpots => self.spots.len(),
            ProfilerTab::FlameGraph => 0,
        };
        self.selected = self.selected.min(n.saturating_sub(1));
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The tab shown.
    pub fn tab(&self) -> ProfilerTab {
        self.tab
    }

    /// Whether a run is going on.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// The run shown, if any.
    pub fn shown_run(&self) -> Option<&ProfileRun> {
        self.run()
    }

    /// The headline: what was profiled, how long, how much was measured, or why it failed.
    pub fn summary(&self) -> String {
        if self.busy {
            return format!("Profiling {}…", self.title);
        }
        match &self.state {
            Err(e) => format!("{}: {e}", self.title),
            Ok(None) => "No profile yet: select a run configuration and choose Profile".into(),
            Ok(Some(run)) => {
                let p = &run.profile;
                let measured = match p.unit {
                    Unit::Samples => format!("{:.0} samples", p.total),
                    Unit::Seconds => format!("{} measured", time_label(p.unit, p.total)),
                    Unit::Bytes => format!("{} held", time_label(p.unit, p.total)),
                };
                let end = end_label(run.timed_out, run.exit_code, run.exit_unknown);
                format!(
                    "{} · {} · {} ms · {measured}{end}",
                    self.title, p.engine, run.millis
                )
            }
        }
    }

    /// The profile's remarks in one line (e.g. that `leaks` does not report the exit code, the
    /// peak footprint), shown under the headline (SPEC-0250 behaviour 3; missing in hand test 180).
    pub fn notes_line(&self) -> Option<String> {
        if self.busy {
            return None;
        }
        let run = self.state.as_ref().ok()?.as_ref()?;
        notes_label(&run.profile.notes)
    }

    /// The rows of the tab shown as text (for tests); the flame graph lists its frames by level.
    pub fn texts(&self) -> Vec<String> {
        let (Some(run), Some(p)) = (self.run(), self.tree()) else {
            return Vec::new();
        };
        match self.tab {
            ProfilerTab::CallTree => self
                .rows
                .iter()
                .filter_map(|r| {
                    let n = node_at(&p.roots, &r.path)?;
                    let mark = match (r.has_children, self.expanded.contains(&r.path)) {
                        (false, _) => " ",
                        (true, true) => "▾",
                        (true, false) => "▸",
                    };
                    Some(format!(
                        "{}{mark} {}  {}  self {}{}",
                        "  ".repeat(r.depth),
                        n.name,
                        percent(n.total, p.total),
                        percent(n.self_cost, p.total),
                        location_label(&n.location)
                    ))
                })
                .collect(),
            ProfilerTab::HotSpots => self
                .spots
                .iter()
                .map(|h| {
                    format!(
                        "{}  self {} ({})  total {}{}",
                        h.name,
                        percent(h.self_cost, p.total),
                        spot_time(p.unit, h.self_cost, p.total, run.millis),
                        percent(h.total, p.total),
                        location_label(&h.location)
                    )
                })
                .collect(),
            ProfilerTab::FlameGraph => {
                let mut out = Vec::new();
                fn walk(n: &ProfileNode, depth: usize, total: f64, out: &mut Vec<String>) {
                    if depth >= FLAME_DEPTH || n.total < total * 0.005 {
                        return;
                    }
                    out.push(format!("{depth} {} {}", n.name, percent(n.total, total)));
                    for c in &n.children {
                        walk(c, depth + 1, total, out);
                    }
                }
                for r in &p.roots {
                    walk(r, 0, p.total, &mut out);
                }
                out
            }
        }
    }

    /// Index of the selected row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    fn selected_location(&self) -> Option<(String, u32)> {
        let p = self.tree()?;
        match self.tab {
            ProfilerTab::CallTree => node_at(&p.roots, &self.rows.get(self.selected)?.path)?
                .location
                .clone(),
            ProfilerTab::HotSpots => self.spots.get(self.selected)?.location.clone(),
            ProfilerTab::FlameGraph => node_at(&p.roots, self.flame_selected.as_ref()?)?
                .location
                .clone(),
        }
    }

    /// Handles one key as the window does (for tests too): Tab next view, ↑↓ select, → open a
    /// node, ← close it or go to its caller, Enter source, F5 profile again, Escape hide.
    pub fn press(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.picker.is_some() {
            return self.press_picker(key, cx);
        }
        let n = match self.tab {
            ProfilerTab::CallTree => self.rows.len(),
            ProfilerTab::HotSpots => self.spots.len(),
            ProfilerTab::FlameGraph => 0,
        };
        match key {
            "tab" => {
                let at = ProfilerTab::ALL
                    .iter()
                    .position(|t| *t == self.tab)
                    .unwrap_or(0);
                self.tab = ProfilerTab::ALL[(at + 1) % ProfilerTab::ALL.len()];
                self.selected = 0;
                self.rebuild();
            }
            "1" => self.set_tab(ProfilerTab::CallTree),
            "2" => self.set_tab(ProfilerTab::HotSpots),
            "3" => self.set_tab(ProfilerTab::FlameGraph),
            "down" if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "right" if self.tab == ProfilerTab::CallTree => {
                if let Some(r) = self.rows.get(self.selected).filter(|r| r.has_children) {
                    if !self.expanded.insert(r.path.clone()) && self.selected + 1 < n {
                        self.selected += 1;
                    }
                    self.rebuild();
                }
            }
            "left" if self.tab == ProfilerTab::CallTree => {
                if let Some(r) = self.rows.get(self.selected).cloned() {
                    if !self.expanded.remove(&r.path) && r.path.len() > 1 {
                        let parent = &r.path[..r.path.len() - 1];
                        if let Some(i) = self.rows.iter().position(|x| x.path == parent) {
                            self.selected = i;
                        }
                    }
                    self.rebuild();
                }
            }
            "enter" => match self.selected_location() {
                Some((file, line)) => cx.emit(ProfilerEvent::Open(file, line)),
                None => return true,
            },
            "f5" => cx.emit(ProfilerEvent::Again(self.shown_kind())),
            "escape" => cx.emit(ProfilerEvent::Hide),
            // "Only project code" and Close (SPEC-0120 v5 behaviours 10 and 12).
            "p" => self.toggle_only_project(cx),
            "backspace" | "delete" => self.close_profile(cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Shows `tab`.
    pub fn set_tab(&mut self, tab: ProfilerTab) {
        self.tab = tab;
        self.selected = 0;
        self.rebuild();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        if self.picker.is_some() && m.alt && event.keystroke.key == "enter" {
            self.press("alt-enter", cx);
            cx.stop_propagation();
            return;
        }
        // The first and the last row of the process list or of the open tab (`SPEC-0275` v19
        // behaviour 47).
        let rows = match (&self.picker, self.tab) {
            (Some(p), _) => p.shown().len(),
            (None, ProfilerTab::CallTree) => self.rows.len(),
            (None, ProfilerTab::HotSpots) => self.spots.len(),
            (None, ProfilerTab::FlameGraph) => 0,
        };
        if let Some(to) = crate::list_nav::edge(&event.keystroke, rows) {
            match self.picker.as_mut() {
                Some(p) => p.selected = to,
                None => self.selected = to,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if m.platform || m.control || m.alt {
            return;
        }
        // In the process list, typed characters (shifted ones too) filter it; the text arrives
        // through the platform's text input, not as the key behind it (`SPEC-0004` v8
        // behaviour 17b).
        if self.picker.is_some()
            && crate::committed_text::leave_to_text_input(
                event.keystroke.key_char.as_deref(),
                false,
            )
        {
            return;
        }
        if m.shift {
            return;
        }
        let key = event.keystroke.key.clone();
        if self.press(&key, cx) {
            cx.stop_propagation();
        }
    }

    /// Takes text the platform's text input committed: in the process list it extends the
    /// filter, character by character (`SPEC-0004` v8 behaviour 17b).
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        if self.picker.is_none() {
            return;
        }
        for ch in committed.chars() {
            self.press_picker(&ch.to_string(), cx);
        }
    }

    /// The typed filter of the process list, if it is open.
    pub fn picker_filter(&self) -> Option<&str> {
        self.picker.as_ref().map(|p| p.filter.as_str())
    }

    fn flame(
        &self,
        node: &ProfileNode,
        path: Vec<usize>,
        share: f32,
        total: f64,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current();
        let depth = path.len() - 1;
        let on = self.flame_selected.as_ref() == Some(&path);
        // Warm colours from the console palette, by name, so a function keeps its colour.
        let hash = node
            .name
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b.into()));
        let tone = [3usize, 1, 11, 9][(hash % 4) as usize];
        // Frames outside the project paler, their names in the secondary text colour
        // (SPEC-0120 v5 behaviour 11).
        let away = outside(node);
        let alpha = match (on, away) {
            (true, _) => 0xee,
            (false, false) => 0x66,
            (false, true) => 0x2a,
        };
        let fill = rgba((p.console_ansi[tone] << 8) | alpha);
        let label = format!("{}  {}", node.name, percent(node.total, total));
        let id = path.iter().fold(depth as u64, |h, i| {
            h.wrapping_mul(131).wrapping_add(*i as u64 + 1)
        });
        let click_path = path.clone();
        let mut children = div().flex().flex_row().w_full();
        if depth + 1 < FLAME_DEPTH {
            for (i, ch) in node.children.iter().enumerate() {
                if ch.total < total * 0.005 || node.total <= 0.0 {
                    continue;
                }
                let mut child_path = path.clone();
                child_path.push(i);
                let s = (ch.total / node.total) as f32;
                children = children.child(self.flame(ch, child_path, s, total, cx));
            }
        }
        div()
            .id(("flame", id))
            .w(relative(share))
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("flame-frame", id))
                    .h(px(18.))
                    .mr(px(1.))
                    .mb(px(1.))
                    .px_1()
                    .rounded(px(2.))
                    .bg(fill)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(c(if away { p.ui_muted } else { p.ui_text }))
                    .child(label)
                    .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                        this.flame_selected = Some(click_path.clone());
                        if ev.click_count() > 1 {
                            this.press("enter", cx);
                        }
                        cx.notify();
                    })),
            )
            .child(children)
    }
}

impl ProfilerView {
    fn picker_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let Some(picker) = &self.picker else {
            return div().into_any_element();
        };
        let selected = picker.selected;
        let head = match (&picker.note, picker.filter.is_empty()) {
            (Some(note), _) => note.clone(),
            (None, true) => "Attach to a running process: type part of its command or PID".into(),
            (None, false) => format!("Filter: {}", picker.filter),
        };
        let bad = picker.note.is_some();
        div()
            .id("profiler-picker")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_1()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .when(bad, |d| d.text_color(rgb(p.console_ansi_text[1])))
                    .when(!bad, |d| d.text_color(c(p.ui_muted)))
                    .child(head),
            )
            .children(picker.shown().into_iter().enumerate().map(|(i, proc_)| {
                let on = i == selected;
                let refused = proc_.refused.is_some();
                div()
                    .id(("profiler-process", i))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .px_2()
                    .rounded(px(4.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(crate::fonts::editor_family())
                    .text_sm()
                    .when(refused, |d| d.text_color(c(p.ui_dim)))
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(process_text(proc_))
                    .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                        if let Some(picker) = &mut this.picker {
                            picker.selected = i;
                        }
                        if ev.click_count() > 1 {
                            this.press("enter", cx);
                        }
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }
}

crate::committed_text::committed_text_input!(ProfilerView, take_committed);

impl Render for ProfilerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let texts = self.texts();
        let outside_rows = self.outside_rows();
        let only_project = self.only_project;
        let has_profile = self.run().is_some();
        let tab = self.tab;
        let failed = self.state.is_err();
        let memory = self.shown_kind() == Kind::Memory;
        let help = match tab {
            _ if self.picker.is_some() => {
                "Type to filter · Enter attaches (CPU) · ⌥Enter heap snapshot · Escape closes"
            }
            ProfilerTab::HotSpots if memory => {
                "Functions by bytes they allocated themselves · Enter source · Tab next view · F5 profile again"
            }
            ProfilerTab::FlameGraph if memory => {
                "Width = share of the bytes held, top calls below · double-click opens the source"
            }
            ProfilerTab::CallTree => {
                "↑↓ select · → open · ← close · Enter source · P only project code · Tab next view · F5 profile again"
            }
            ProfilerTab::HotSpots => {
                "Functions by time spent in themselves · Enter source · P only project code · Tab next view · F5 profile again"
            }
            ProfilerTab::FlameGraph => {
                "Width = share of the run, top calls below · click selects, double-click opens the source"
            }
        };
        let body = match (tab, self.run()) {
            _ if self.picker.is_some() => self.picker_body(cx),
            (ProfilerTab::FlameGraph, Some(run)) => {
                let tree = self.tree().unwrap_or(&run.profile);
                let total = tree.total;
                let roots: Vec<_> = tree
                    .roots
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| total > 0.0 && r.total >= total * 0.005)
                    .map(|(i, r)| self.flame(r, vec![i], (r.total / total) as f32, total, cx))
                    .collect();
                div()
                    .id("profiler-flame")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(div().flex().flex_row().w_full().children(roots))
                    .into_any_element()
            }
            _ => div()
                .id("profiler-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_1()
                .when(texts.is_empty(), |d| {
                    d.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_color(if failed {
                                rgb(p.console_ansi_text[1]).into()
                            } else {
                                c(p.ui_muted)
                            })
                            .child(self.summary()),
                    )
                })
                .children(texts.into_iter().enumerate().map(|(i, text)| {
                    let on = i == selected;
                    let away = outside_rows.get(i).copied().unwrap_or(false);
                    div()
                        .id(("profiler-row", i))
                        .when(away, |d| d.text_color(c(p.ui_muted)))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .px_2()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(crate::fonts::editor_family())
                        .text_sm()
                        .when(on, |d| d.bg(c(p.tree_selection)))
                        .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(text)
                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                            this.selected = i;
                            if ev.click_count() > 1 {
                                this.press("enter", cx);
                            }
                            cx.notify();
                        }))
                }))
                .into_any_element(),
        };
        div()
            .id("profiler-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title("Profiler"))
                    .children(ProfilerTab::ALL.into_iter().map(|t| {
                        let on = t == tab;
                        div()
                            .id(t.label())
                            .px_2()
                            .py(px(2.))
                            .rounded(px(4.))
                            .when(on, |d| {
                                d.bg(c(p.tab_selected))
                                    .border_1()
                                    .border_color(c(p.tab_selected_border))
                            })
                            .when(!on, |d| {
                                d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tab_hover)))
                            })
                            .child(t.label())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_tab(t);
                                cx.notify();
                            }))
                    }))
                    .child(
                        div()
                            .id("profiler-only-project")
                            .relative()
                            .semantics(
                                Semantics::new(Role::CheckBox)
                                    .name("Only project code")
                                    .checked(only_project)
                                    .test_id("profiler-only-project"),
                            )
                            .px_2()
                            .py(px(2.))
                            .rounded(px(4.))
                            .text_color(c(p.ui_muted))
                            .hover(|s| s.bg(c(p.tab_hover)))
                            .child(if only_project {
                                "☑ Only project code"
                            } else {
                                "☐ Only project code"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_only_project(cx))),
                    )
                    .when(has_profile, |d| {
                        d.child(
                            div()
                                .id("profiler-close")
                                .relative()
                                .semantics(
                                    Semantics::new(Role::Button)
                                        .name("Close profile")
                                        .test_id("profiler-close"),
                                )
                                .px_2()
                                .py(px(2.))
                                .rounded(px(4.))
                                .text_color(c(p.ui_muted))
                                .hover(|s| s.bg(c(p.tab_hover)))
                                .child("Close")
                                .on_click(cx.listener(|this, _, _, cx| this.close_profile(cx))),
                        )
                    })
                    .child(div().text_xs().text_color(c(p.ui_dim)).child(keys(help))),
            )
            .child(
                div()
                    .h(px(22.))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(c(p.ui_muted))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(self.summary()),
            )
            .when_some(self.notes_line(), |d, notes| {
                d.child(
                    div()
                        .id("profiler-notes")
                        .h(px(20.))
                        .flex_none()
                        .px_3()
                        .flex()
                        .items_center()
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(notes),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(body),
            )
    }
}

/// How the program ended, after the headline: nothing for exit code 0, the code otherwise, or
/// "exit code unknown" when the engine cannot know it (`leaks`, an attached process).
fn end_label(timed_out: bool, exit_code: Option<i32>, exit_unknown: bool) -> String {
    match (timed_out, exit_code) {
        (true, _) => " · ended at the time limit".to_owned(),
        (false, _) if exit_unknown => " · exit code unknown".to_owned(),
        (false, Some(0)) => String::new(),
        (false, Some(code)) => format!(" · exit code {code}"),
        (false, None) => " · ended by a signal".to_owned(),
    }
}

/// The remarks of a profile joined for the line under the headline; `None` without any.
fn notes_label(notes: &[String]) -> Option<String> {
    let notes: Vec<&str> = notes
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .collect();
    (!notes.is_empty()).then(|| notes.join(" · "))
}

/// A shortcut of this dialog written for the platform: ⌘ is Ctrl and ⌥ is Alt outside macOS
/// (`SPEC-0028` v8 behaviour 13).
fn keys(text: &str) -> String {
    ferris_idea_ui::keymap::local_keys(text, cfg!(target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand test 180: the leaks remark and the unknown exit code are shown.
    #[test]
    fn the_end_and_the_remarks_are_shown() {
        assert_eq!(end_label(false, Some(0), false), "");
        assert_eq!(end_label(false, Some(3), false), " · exit code 3");
        assert_eq!(end_label(false, None, true), " · exit code unknown");
        assert_eq!(end_label(true, None, true), " · ended at the time limit");
        assert_eq!(notes_label(&[]), None);
        assert_eq!(
            notes_label(&[
                "leaks does not report the program's exit code".into(),
                " ".into(),
                "peak physical footprint 16.1M".into(),
            ])
            .as_deref(),
            Some("leaks does not report the program's exit code · peak physical footprint 16.1M")
        );
    }

    #[test]
    fn labels_read_as_time_and_share() {
        assert_eq!(time_label(Unit::Samples, 640.0), "640 ms");
        // D37: 95 of 134 samples over 7701 ms are 70.9 % of the run, about 5.46 s.
        assert_eq!(spot_time(Unit::Samples, 95.0, 134.0, 7701), "5.46 s");
        assert_eq!(spot_time(Unit::Samples, 5.0, 0.0, 7701), "5 ms");
        assert_eq!(spot_time(Unit::Seconds, 1.5, 3.0, 9000), "1.50 s");
        assert_eq!(time_label(Unit::Seconds, 1.234), "1.23 s");
        assert_eq!(percent(25.0, 200.0), "12.5 %");
        assert_eq!(percent(1.0, 0.0), "0.0 %");
        assert_eq!(
            location_label(&Some(("/a/b/app.py".into(), 4))),
            "  app.py:4"
        );
        assert_eq!(location_label(&None), "");
        assert_eq!(time_label(Unit::Bytes, 3.0 * 1024.0 * 1024.0), "3.0 MB");
    }

    #[test]
    fn processes_filter_and_mark_refusals() {
        let picker = Picker {
            processes: vec![
                ProcessInfo {
                    pid: 42,
                    command: "./busy 7".into(),
                    refused: None,
                },
                ProcessInfo {
                    pid: 7,
                    command: "/Applications/IntelliJ IDEA.app/Contents/MacOS/idea".into(),
                    refused: Some("reference".into()),
                },
            ],
            filter: "BUSY".into(),
            ..Picker::default()
        };
        let shown = picker.shown();
        assert_eq!(shown.len(), 1);
        assert_eq!(process_text(shown[0]), "    42  ./busy 7");
        assert!(process_text(&picker.processes[1]).ends_with("— refused"));
    }

    /// A JFR profile of a project method called through a JDK frame, stored in a workspace with
    /// that method's file (`SPEC-0120` v5 behaviours 10–12).
    #[cfg(target_os = "macos")]
    fn project_run(root: &std::path::Path) -> ProfileRun {
        std::fs::create_dir_all(root.join("src/demo")).unwrap();
        std::fs::write(root.join("src/demo/Busy.java"), "class Busy {}\n").unwrap();
        let sample = |frames: &str| {
            format!(
                "jdk.ExecutionSample {{\n  sampledThread = \"main\" (javaThreadId = 1)\n  stackTrace = [\n{frames}  ]\n}}\n\n"
            )
        };
        let through = "    demo.Busy.inner(long) line: 6\n    java.util.ArrayList.forEach(Consumer) line: 1596\n    demo.Busy.main(String[]) line: 11\n";
        let jdk = "    java.util.Objects.equals(Object, Object) line: 64\n    demo.Busy.main(String[]) line: 12\n";
        let text = format!("{}{}{}", sample(through), sample(through), sample(jdk));
        let mut run = ProfileRun {
            plan: ferris_idea_profiler::ProfilePlan {
                engine: ferris_idea_profiler::Engine::Jfr,
                command: vec![],
                env: Default::default(),
            },
            exit_code: Some(0),
            timed_out: false,
            millis: 300,
            output: String::new(),
            profile: ferris_idea_profiler::parse_jfr(&text),
            kind: Kind::Cpu,
            exit_unknown: false,
            attached: None,
            sources: Default::default(),
            marked: false,
        };
        ferris_idea_profiler::prepare(&mut run, root);
        run
    }

    // gpui's test support is a dev-dependency on macOS only (Cargo.toml), like the other
    // window tests of this crate.
    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn project_frames_dimmed_filtered_revealed_and_closed(cx: &mut gpui::TestAppContext) {
        let root = std::env::temp_dir().join(format!("ferris-idea-pv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let run = project_run(&root);
        let view = cx.new(ProfilerView::new);
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = events.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, e: &ProfilerEvent, _| {
                seen.borrow_mut().push(e.clone())
            })
            .detach()
        });
        view.update(cx, |v, cx| {
            v.set_project(root.clone());
            v.show_run("busy".into(), Ok(run), cx);
        });
        cx.run_until_parked();
        assert_eq!(events.borrow().as_slice(), [ProfilerEvent::Changed]);
        view.update(cx, |v, cx| {
            // Hot Spots: the JDK's method dimmed.
            v.press("2", cx);
            let rows: Vec<(String, bool)> = v
                .texts()
                .into_iter()
                .zip(v.outside_rows())
                .map(|(t, o)| (t.split("  ").next().unwrap().to_owned(), o))
                .collect();
            assert_eq!(
                rows,
                vec![
                    ("Busy.inner".into(), false),
                    ("Objects.equals".into(), true)
                ]
            );
            // The call tree: the thread is no frame; forEach is dimmed.
            v.press("1", cx);
            let texts = v.texts();
            let outside = v.outside_rows();
            assert!(texts[0].contains("Thread main") && !outside[0], "{texts:?}");
            let each = texts
                .iter()
                .position(|t| t.contains("ArrayList.forEach"))
                .unwrap();
            assert!(outside[each], "{texts:?}");
            // P: only project functions, kept for the project; the call tree folded.
            v.press("p", cx);
            assert!(v.only_project());
            v.press("2", cx);
            assert_eq!(v.texts().len(), 1);
            assert!(v.texts()[0].starts_with("Busy.inner"));
            v.press("1", cx);
            let folded = v.texts();
            assert!(
                !folded
                    .iter()
                    .any(|t| t.contains("forEach") || t.contains("Objects")),
                "{folded:?}"
            );
            assert!(
                folded.iter().any(|t| t.contains("Busy.inner")),
                "{folded:?}"
            );
            // The flame graph draws the folded tree too.
            v.press("3", cx);
            assert!(!v.texts().iter().any(|t| t.contains("forEach")));
            assert!(v.outside_rows().iter().all(|o| !o));
        });
        let kept = ferris_idea_profiler::ViewSettings::load(
            &root.join(ferris_idea_profiler::PROFILES_DIR),
        );
        assert!(kept.only_project_code);
        let again = cx.new(ProfilerView::new);
        again.update(cx, |v, _| v.set_project(root.clone()));
        assert!(again.read_with(cx, |v, _| v.only_project()));
        view.update(cx, |v, cx| {
            v.press("p", cx);
            // A click on a line's cost: the costliest call of the line's function, selected.
            v.press("2", cx);
            assert!(v.reveal("demo/Busy.java", "Busy.inner", cx));
            assert_eq!(v.tab(), ProfilerTab::CallTree);
            let row = v.texts()[v.selected()].clone();
            assert!(
                row.contains("Busy.inner") && row.contains("Busy.java:6"),
                "{row}"
            );
            assert!(!v.reveal("demo/Busy.java", "Busy.nothing", cx));
            // Close: nothing shown, the editors are told.
            v.press("backspace", cx);
            assert!(v.shown_run().is_none());
            assert!(v.texts().is_empty());
        });
        cx.run_until_parked();
        assert_eq!(events.borrow().len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }
}

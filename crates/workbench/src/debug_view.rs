//! The Debug tool window (`SPEC-0091`): toolbar, call stack, variables and console of one debug
//! session.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, font, prelude::*, px,
    rgb,
};
use idecraft_debugger::extras::{self, Renderer, VarFilters};
use idecraft_debugger::{Frame, Lang, ThreadInfo, Variable, ViewAs};

use crate::debug_model::{Command, Controller, FrameRow, Launch, Update, VarTree};
use crate::editor_view::EditorView;
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// Results of evaluated expressions kept in the window, newest first.
const RESULTS: usize = 20;

/// How often the session's updates are taken.
const POLL: Duration = Duration::from_millis(50);

/// What the tool window asks the workbench to do.
pub enum DebugViewEvent {
    /// Hide the tool window.
    Hide,
    /// Show this line (0-based) of this file as the execution point.
    ShowLine(PathBuf, usize),
    /// The session ended; clear the execution point.
    Ended,
    /// Names and values of the shown frame's variables, for the editor (`SPEC-0091` v5 behaviour 13).
    Values(Vec<(String, String)>),
    /// A toolbar button for a workbench action: `Rerun`, `RunToCursor`, `ViewBreakpoints`,
    /// `XDebugger.MuteBreakpoints` (`SPEC-0194` v3 behaviour 6).
    Action(&'static str),
    /// Open the menu `DebugMenu` at this window position (`SPEC-0091` v11 behaviours 21, 23).
    Menu(DebugMenu, gpui::Point<gpui::Pixels>),
    /// The calls Smart Step Into offers, to choose from (v11 behaviour 18).
    StepTargets(Vec<(i64, String)>),
    /// The session removed the breakpoint on this 0-based line after its hit (`SPEC-0127` v3
    /// behaviour 9).
    RemoveBreakpoint(PathBuf, usize),
    /// The breakpoints of this file as the session placed them, bound or not (`SPEC-0127` v5
    /// behaviour 14).
    Placed(PathBuf, Vec<idecraft_debugger::Breakpoint>),
    /// Watch the field with this data breakpoint id (`SPEC-0146` v3).
    AddFieldWatch(String),
    /// Open the file a console line names (as written there) at this 1-based line (`SPEC-0091`
    /// v12 behaviour 38).
    OpenLocation(String, u64),
    /// The answer to Quick Evaluate Expression, as the hint below the caret shows it
    /// (`SPEC-0091` v19 behaviour 44).
    Quick(String),
}

/// The menus of the Debug window (`SPEC-0091` v11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugMenu {
    /// The toolbar's ⋮ with the further stepping actions (behaviour 21).
    More,
    /// A frame of the call stack (Reset Frame, Copy Stack).
    Frame,
    /// A row of the Variables list (behaviour 23).
    Variable,
}

/// Where the session is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugState {
    /// Adapter and program are starting.
    Starting,
    /// The program runs.
    Running,
    /// The program is paused; the reason as the adapter gave it.
    Paused(String),
    /// Over, with the closing line for the console.
    Ended(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Variables,
    Console,
    /// An Inspect or referring-objects tab (`SPEC-0091` v12 behaviours 30, 31).
    Inspect(usize),
}

/// An Inspect tab: a value followed through the session's stops (`SPEC-0091` v12 behaviour 30).
struct Inspect {
    /// The tab's title.
    title: String,
    /// The expression evaluated at each stop.
    expression: String,
    /// The objects referring to the value instead of the value (behaviour 31).
    referring: bool,
    tree: VarTree,
    error: Option<String>,
}

/// Inspect tabs open at most.
const INSPECTS: usize = 5;

/// What the expression field asks for when Enter does not evaluate.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FieldMode {
    /// A renderer's expression for this type (`SPEC-0091` v12 behaviour 33).
    Renderer(String),
}

/// The Debug tool window (behaviour 3).
pub struct DebugView {
    name: String,
    controller: Option<Controller>,
    state: DebugState,
    frames: Vec<Frame>,
    frame: usize,
    vars: VarTree,
    console: String,
    /// The command line written at the start, until the adapter echoes it (hand test 95).
    echo: Option<String>,
    tab: Tab,
    polling: bool,
    focus: FocusHandle,
    /// The expression field above the variables (behaviour 8).
    expression: Entity<EditorView>,
    /// Evaluated expressions with their value or error, newest first.
    results: Vec<(String, Result<String, String>)>,
    /// The expression Quick Evaluate Expression waits for (`SPEC-0091` v19 behaviour 44).
    quick: Option<String>,
    /// Watch expressions, kept across sessions, and their last values (v5 behaviour 11).
    watches: Vec<(String, Option<Result<String, String>>)>,
    /// Threads of the paused program and the one shown (v5 behaviour 12).
    threads: Vec<(i64, String)>,
    thread: i64,
    /// The row of the Variables list selected by a click (`SPEC-0194` behaviour 3).
    var_selected: Option<usize>,
    /// While Set Value is on: the variable's container (0 for the frame's locals) and name.
    setting: Option<(i64, String)>,
    /// The fields of the shown frame's object, for the editor's values (v8 behaviour 13).
    fields: Vec<idecraft_debugger::Variable>,
    /// The workspace folder: frames with a source outside it are library frames (v10 behaviour 15).
    workspace: PathBuf,
    /// The project's files are on a host (`SPEC-0322` v5 behaviour 14).
    files_on_host: bool,
    /// Which frames are library frames, and the folded runs opened by a click.
    library: Vec<bool>,
    opened: HashSet<usize>,
    /// Whether the thread selector's list is open (v10 behaviour 12).
    threads_open: bool,
    /// The editors of the saved sources, whose errors hold back hot swap (v13).
    saved_editors: std::collections::HashMap<PathBuf, gpui::WeakEntity<EditorView>>,
    /// The last configuration started, for Rerun (`SPEC-0194` v3 behaviour 7).
    last: Option<(String, Launch)>,
    /// Whether breakpoints are muted, for the pressed button (v3 behaviour 8).
    muted: bool,
    /// Labels of marked objects by the object's id in its value (`Debugger.MarkObject`, v11
    /// behaviour 24) and their colour (v12 behaviour 35); kept for the window's sessions.
    marks: Vec<(String, String, usize)>,
    /// While Mark Object is on: the id of the object being labelled.
    marking: Option<String>,
    /// Completions for the expression field, and the one highlighted (v11 behaviour 22).
    completions: Vec<String>,
    completion: usize,
    /// The field's text right after a completion was taken; not asked about again.
    accepted: Option<String>,
    /// The calls Smart Step Into offered last (v11 behaviour 18).
    step_targets: Vec<(i64, String)>,
    /// The colour of the mark being given (an index into the console's colours; v12 behaviour
    /// 35).
    mark_color: usize,
    /// Threads with state and group, when read (v12 behaviour 28).
    thread_infos: Vec<ThreadInfo>,
    /// Inspect tabs (v12 behaviours 30, 31).
    inspects: Vec<Inspect>,
    /// The view of a row's value, by the row's expression (v12 behaviour 32).
    views: HashMap<String, ViewAs>,
    /// Renderers by type, saved with the workspace, and whether they are muted (behaviour 33).
    renderers: Vec<Renderer>,
    renderers_muted: bool,
    /// Rows whose view or renderer was asked for in this stop: whether the value's children
    /// replace the row's.
    render_asked: HashMap<(i64, String), bool>,
    /// What the Variables list hides, and whether the user chose (behaviour 34).
    filters: VarFilters,
    filters_chosen: bool,
    /// What the expression field asks for besides evaluating.
    field_mode: Option<FieldMode>,
    /// Several lines in the expression field: a code fragment (behaviour 36).
    fragment: bool,
    /// The console's side bar: soft wrap, following the end, the stack line shown (behaviour 38).
    soft_wrap: bool,
    follow: bool,
    stack_line: Option<usize>,
    console_scroll: gpui::ScrollHandle,
    /// The console's input line (behaviour 38).
    console_input: Entity<EditorView>,
    /// Sources built for hot swap whose classes can be reloaded (behaviour 37).
    code_changed: Vec<PathBuf>,
}

impl EventEmitter<DebugViewEvent> for DebugView {}

impl DebugView {
    /// An idle tool window.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let expression = cx.new(EditorView::field);
        // The placeholder follows the field's text (behaviour 8); completions are asked for after
        // each key (v11 behaviour 22, `on_expression_key`).
        cx.observe(&expression, |_, _, cx| cx.notify()).detach();
        let console_input = cx.new(EditorView::field);
        Self {
            name: String::new(),
            controller: None,
            state: DebugState::Ended(String::new()),
            frames: Vec::new(),
            frame: 0,
            vars: VarTree::default(),
            console: String::new(),
            echo: None,
            tab: Tab::Variables,
            polling: false,
            focus: cx.focus_handle(),
            expression,
            results: Vec::new(),
            quick: None,
            watches: Vec::new(),
            threads: Vec::new(),
            thread: 0,
            var_selected: None,
            setting: None,
            fields: Vec::new(),
            workspace: PathBuf::new(),
            files_on_host: false,
            library: Vec::new(),
            opened: HashSet::new(),
            threads_open: false,
            saved_editors: Default::default(),
            last: None,
            muted: false,
            marks: Vec::new(),
            marking: None,
            completions: Vec::new(),
            completion: 0,
            accepted: None,
            step_targets: Vec::new(),
            mark_color: 0,
            thread_infos: Vec::new(),
            inspects: Vec::new(),
            views: HashMap::new(),
            renderers: Vec::new(),
            renderers_muted: false,
            render_asked: HashMap::new(),
            filters: VarFilters::default(),
            filters_chosen: false,
            field_mode: None,
            fragment: false,
            soft_wrap: true,
            follow: true,
            stack_line: None,
            console_scroll: gpui::ScrollHandle::new(),
            console_input,
            code_changed: Vec::new(),
        }
    }

    /// The configuration started last and its name, for Rerun (`SPEC-0194` v3 behaviour 7).
    pub fn last_launch(&self) -> Option<(String, Launch)> {
        self.last.clone()
    }

    /// Presses a toolbar button for a workbench action (what its click does; tests).
    pub fn press(&mut self, action: &'static str, cx: &mut Context<Self>) {
        cx.emit(DebugViewEvent::Action(action));
    }

    /// Shows the Mute Breakpoints button pressed or not (v3 behaviour 8).
    pub fn set_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        self.muted = muted;
        cx.notify();
    }

    /// The toolbar's enabled buttons (v3 behaviour 6).
    pub fn toolbar(&self) -> crate::debug_model::Toolbar {
        crate::debug_model::toolbar(
            self.last.is_some(),
            self.active(),
            matches!(self.state, DebugState::Running),
            matches!(self.state, DebugState::Paused(_)),
        )
    }

    /// Sets the workspace folder, for telling project frames from library frames.
    pub fn set_workspace(&mut self, root: PathBuf) {
        self.renderers = idecraft_debugger::saved::load_renderers(&root);
        self.workspace = root;
    }

    /// Says that the project's files are on a host (`SPEC-0322` v5 behaviour 14): a source inside
    /// the stand-in folder is then a project file although no such file is on this computer.
    pub fn set_files_on_host(&mut self, on_host: bool) {
        self.files_on_host = on_host;
    }

    /// Takes a new call stack: which frames are library frames, all runs folded again.
    fn set_frames(&mut self, frames: Vec<Frame>) {
        self.library = frames
            .iter()
            .map(|f| crate::debug_model::library_frame(f, &self.workspace, self.files_on_host))
            .collect();
        self.opened.clear();
        self.frames = frames;
        self.frame = 0;
    }

    /// The rows of the call stack (v10 behaviour 15).
    pub fn frame_rows(&self) -> Vec<FrameRow> {
        crate::debug_model::frame_rows(self.frames.len(), &self.library, self.frame, &self.opened)
    }

    /// The call stack as shown: function names and the folded rows' texts.
    pub fn frame_row_texts(&self) -> Vec<String> {
        self.frame_rows()
            .into_iter()
            .map(|r| match r {
                FrameRow::Frame(i) => self.frames[i].name.clone(),
                FrameRow::Hidden { count, .. } => crate::debug_model::hidden_label(count),
            })
            .collect()
    }

    /// Shows the frames of the folded run starting at `first` (a click on its row).
    pub fn open_hidden(&mut self, first: usize, cx: &mut Context<Self>) {
        self.opened.insert(first);
        cx.notify();
    }

    /// Name of the thread shown, as the selector shows it (v10 behaviour 12).
    pub fn shown_thread_name(&self) -> Option<&str> {
        self.threads
            .iter()
            .find(|(id, _)| *id == self.thread)
            .map(|(_, n)| n.as_str())
    }

    /// Focus handle of the window.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Starts debugging `launch` as configuration `name`, ending a session that still runs.
    pub fn start(&mut self, name: &str, launch: Launch, cx: &mut Context<Self>) {
        if let Some(old) = self.controller.take() {
            old.send(Command::Stop);
        }
        self.name = name.to_owned();
        self.console = match &launch.attach {
            Some(target) => format!("Attaching to {target}…\n"),
            None => format!("{} {}\n", launch.program.display(), launch.args.join(" ")),
        };
        self.echo = launch
            .attach
            .is_none()
            .then(|| self.console.trim_end().to_owned());
        self.frames.clear();
        self.vars = VarTree::default();
        self.results.clear();
        self.state = DebugState::Starting;
        self.tab = Tab::Variables;
        self.code_changed.clear();
        self.thread_infos.clear();
        self.stack_line = None;
        self.last = Some((name.to_owned(), launch.clone()));
        // java-debug hides static fields unless asked; Python shows its class variables (v12
        // behaviour 34).
        if !self.filters_chosen {
            self.filters.statics = self.lang() != Lang::Java;
        }
        let statics = self.filters.statics;
        let java = self.lang() == Lang::Java;
        let controller = Controller::start(launch);
        if java && statics {
            controller.send(Command::Settings(
                serde_json::json!({"showStaticVariables": true}),
            ));
        }
        self.controller = Some(controller);
        self.poll(cx);
        cx.notify();
    }

    /// Sends `command` to the session; stepping and resuming only while paused, Stop always.
    pub fn command(&mut self, command: Command, cx: &mut Context<Self>) -> bool {
        let Some(controller) = &self.controller else {
            return false;
        };
        let paused = matches!(self.state, DebugState::Paused(_));
        let ok = match command {
            Command::Continue
            | Command::StepOver
            | Command::StepInto
            | Command::StepOut
            | Command::RunToLine(..)
            | Command::SetValue(..)
            | Command::StepTargets(_)
            | Command::StepIntoTarget(_)
            | Command::ResetFrame(_)
            | Command::ForceStepOver
            | Command::ForceRunToLine(..)
            | Command::Completions(..)
            | Command::StepOutOfBlock
            | Command::ResumeAll
            | Command::ForceReturn(_)
            | Command::ThrowException(_)
            | Command::Threads(..)
            | Command::Inspect(..)
            | Command::Render(..)
            | Command::Fragment(..)
            | Command::Repl(..)
            | Command::FieldWatch(..) => paused,
            Command::Pause => matches!(self.state, DebugState::Running),
            Command::PauseAll => matches!(self.state, DebugState::Running) || paused,
            _ => true,
        };
        if ok {
            controller.send(command);
            cx.notify();
        }
        ok
    }

    /// Smart Step Into (`SmartStepInto`, ⇧F7; `SPEC-0091` v11 behaviour 18): asks for the calls on
    /// the paused line; with two or more the workbench offers them, else it is a Step Into.
    pub fn smart_step_into(&mut self, cx: &mut Context<Self>) -> bool {
        match self.frames.first().map(|f| f.id) {
            Some(top) => self.command(Command::StepTargets(top), cx),
            None => false,
        }
    }

    /// Reset Frame (`Debugger.PopFrame`, v11 behaviour 19) on the selected frame.
    pub fn reset_frame(&mut self, cx: &mut Context<Self>) -> bool {
        match self.frames.get(self.frame).map(|f| f.id) {
            Some(id) => self.command(Command::ResetFrame(id), cx),
            None => false,
        }
    }

    /// Shows the top frame's line again and selects that frame (`ShowExecutionPoint`, ⌥F10; v11
    /// behaviour 21).
    pub fn show_execution_point(&mut self, cx: &mut Context<Self>) -> bool {
        if self.frames.is_empty() {
            return false;
        }
        if self.frame != 0 {
            self.select_frame(0, cx);
        } else {
            self.show_frame(cx);
        }
        true
    }

    /// The call stack as text, one `name:line (file)` per frame (Copy Stack).
    pub fn stack_text(&self) -> String {
        self.frames
            .iter()
            .map(|f| {
                let file = f
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("{}:{} ({file})", f.name, f.line)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Name, value and expression of the selected row of the Variables list.
    pub fn selected_variable(&self) -> Option<(String, String, String)> {
        let rows = self.shown_rows();
        let i = self.var_selected?;
        let row = rows.get(i)?;
        Some((
            row.var.name.clone(),
            row.var.value.clone(),
            crate::debug_model::row_expression(&rows, i),
        ))
    }

    /// Whether the selected row is an object that can be marked, and whether it is marked.
    pub fn selected_mark(&self) -> (bool, bool) {
        match self
            .selected_variable()
            .and_then(|(_, v, _)| crate::debug_model::object_id(&v))
        {
            Some(id) => (true, self.marks.iter().any(|(m, _, _)| *m == id)),
            None => (false, false),
        }
    }

    /// Starts Mark Object on the selected row (`Debugger.MarkObject`, v11 behaviour 24): the
    /// expression field gets the row's name as the label, Enter applies it. False when the row
    /// is no object with an id.
    pub fn start_mark(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((name, value, _)) = self.selected_variable() else {
            return false;
        };
        let Some(id) = crate::debug_model::object_id(&value) else {
            return false;
        };
        self.setting = None;
        self.field_mode = None;
        self.marking = Some(id);
        self.mark_color = 0;
        self.set_expression(&name, cx);
        true
    }

    /// Labels object `id` (or the one Mark Object is on) with `label` in the chosen colour.
    pub fn mark(&mut self, id: &str, label: &str, cx: &mut Context<Self>) {
        self.marks.retain(|(m, _, _)| m != id);
        if !label.trim().is_empty() {
            self.marks
                .push((id.to_owned(), label.trim().to_owned(), self.mark_color));
        }
        cx.notify();
    }

    /// Chooses the colour of the mark being given (v12 behaviour 35): an index into
    /// [`MARK_COLORS`].
    pub fn set_mark_color(&mut self, color: usize, cx: &mut Context<Self>) {
        self.mark_color = color % MARK_COLORS.len();
        cx.notify();
    }

    /// The colour index of the mark of the object a value shows, if marked.
    pub fn mark_color_of(&self, value: &str) -> Option<usize> {
        let id = crate::debug_model::object_id(value)?;
        self.marks
            .iter()
            .find(|(m, _, _)| *m == id)
            .map(|(_, _, c)| *c)
    }

    /// Takes the label off the selected row's object.
    pub fn unmark_selected(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self
            .selected_variable()
            .and_then(|(_, v, _)| crate::debug_model::object_id(&v))
        else {
            return false;
        };
        self.mark(&id, "", cx);
        true
    }

    /// The label of the object a value shows, if marked.
    pub fn mark_of(&self, value: &str) -> Option<&str> {
        let id = crate::debug_model::object_id(value)?;
        self.marks
            .iter()
            .find(|(m, _, _)| *m == id)
            .map(|(_, l, _)| l.as_str())
    }

    /// Whether Mark Object waits for its label.
    pub fn marking(&self) -> bool {
        self.marking.is_some()
    }

    /// Selects row `i` of the Variables list (a click or a right click).
    pub fn select_variable_row(&mut self, i: usize, cx: &mut Context<Self>) {
        self.var_selected = Some(i);
        cx.notify();
    }

    /// Asks the session for completions of the expression field's text, while paused and not
    /// setting a value or a label (v11 behaviour 22).
    fn ask_completions(&mut self, cx: &mut Context<Self>) {
        let text = self.expression.read(cx).state.buffer.text();
        let last = text.chars().last();
        let wanted = self.setting.is_none()
            && self.marking.is_none()
            && last.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            && self.accepted.as_deref() != Some(text.as_str());
        if !wanted {
            self.completions.clear();
            return;
        }
        let column = text.chars().count() + 1;
        if let Some(frame) = self.frames.get(self.frame).map(|f| f.id)
            && matches!(self.state, DebugState::Paused(_))
            && let Some(c) = &self.controller
        {
            c.send(Command::Completions(text, column, frame));
        }
    }

    /// The value of the first visible variable named `name`, as its row shows it (with a numeric
    /// view applied).
    pub fn variable_value(&self, name: &str) -> Option<String> {
        self.shown_rows()
            .into_iter()
            .find(|r| r.var.name == name)
            .map(|r| r.var.value)
    }

    /// Opens the first visible variable named `name` (a click on its row's chevron; tests).
    pub fn expand_variable(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let Some(reference) = self
            .vars
            .rows()
            .iter()
            .find(|r| r.var.name == name && r.var.reference != 0 && !r.expanded)
            .map(|r| r.var.reference)
        else {
            return false;
        };
        self.toggle_var(reference, cx);
        true
    }

    /// The calls Smart Step Into offered last.
    pub fn step_targets(&self) -> &[(i64, String)] {
        &self.step_targets
    }

    /// Steps into the offered call `id` (a choice of Smart Step Into).
    pub fn step_into_target(&mut self, id: i64, cx: &mut Context<Self>) -> bool {
        self.command(Command::StepIntoTarget(id), cx)
    }

    /// The completions offered for the expression field.
    pub fn completion_items(&self) -> &[String] {
        &self.completions
    }

    /// Takes completion `i`: the word before the caret becomes it.
    pub fn accept_completion(&mut self, i: usize, cx: &mut Context<Self>) -> bool {
        let Some(item) = self.completions.get(i).cloned() else {
            return false;
        };
        let text = self.expression.read(cx).state.buffer.text();
        let completed = crate::debug_model::complete(&text, &item);
        self.completions.clear();
        self.accepted = Some(completed.clone());
        self.expression.update(cx, |f, cx| {
            f.set_text(&completed, cx);
            let len = f.state.buffer.len_chars();
            f.select_range(len..len, cx);
        });
        // The accepted text is not asked about again until it changes.
        self.completions.clear();
        cx.notify();
        true
    }

    /// The name of the session's configuration.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether a session is going (starting, running or paused).
    pub fn active(&self) -> bool {
        self.controller.is_some()
    }

    /// The session's state.
    pub fn state(&self) -> &DebugState {
        &self.state
    }

    /// Function names of the call stack, top first.
    pub fn frame_names(&self) -> Vec<String> {
        self.frames.iter().map(|f| f.name.clone()).collect()
    }

    /// Visible variables as `name = value`.
    pub fn variable_texts(&self) -> Vec<String> {
        self.shown_rows()
            .iter()
            .map(|r| format!("{} = {}", r.var.name, r.var.value))
            .collect()
    }

    /// Puts `text` into the expression field with all of it selected, on the variables tab.
    pub fn set_expression(&mut self, text: &str, cx: &mut Context<Self>) {
        self.tab = Tab::Variables;
        self.expression.update(cx, |f, cx| {
            f.set_text_selected(text, cx);
        });
        cx.notify();
    }

    /// Text of the expression field.
    pub fn expression_text(&self, cx: &gpui::App) -> String {
        self.expression.read(cx).state.buffer.text()
    }

    /// The hint drawn in the expression field: only while the field is empty (v6 behaviour 8).
    pub fn expression_placeholder(&self, cx: &gpui::App) -> Option<&'static str> {
        self.expression_text(cx)
            .is_empty()
            .then_some(if cfg!(target_os = "macos") {
                "Evaluate (↩) · Watch (⇧⌘↩)"
            } else {
                "Evaluate (Enter) · Watch (Ctrl+Shift+Enter)"
            })
    }

    /// Painted size of the expression field.
    pub fn expression_size(&self, cx: &gpui::App) -> (f32, f32) {
        self.expression.read(cx).painted_size()
    }

    /// Focus handle of the expression field.
    pub fn expression_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.expression.read(cx).focus()
    }

    /// Selects the first visible variable named `name` (a click on its row; tests).
    pub fn select_variable(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        self.var_selected = self.vars.rows().iter().position(|r| r.var.name == name);
        cx.notify();
        self.var_selected.is_some()
    }

    /// Starts Set Value on the selected variable (`XDebugger.SetValue`, `SPEC-0194` behaviour 3):
    /// its value goes into the expression field, selected, and Enter then sets it; false when no
    /// variable is selected or the program is not paused.
    pub fn start_set_value(&mut self, cx: &mut Context<Self>) -> bool {
        let rows = self.vars.rows();
        let Some(row) = self.var_selected.and_then(|i| rows.get(i)) else {
            return false;
        };
        if !matches!(self.state, DebugState::Paused(_)) {
            return false;
        }
        self.setting = Some((row.parent, row.var.name.clone()));
        let value = row.var.value.clone();
        self.set_expression(&value, cx);
        true
    }

    /// The variable Set Value is on for, if any.
    pub fn setting(&self) -> Option<&str> {
        self.setting.as_ref().map(|(_, n)| n.as_str())
    }

    /// Evaluates the expression in the field in the selected frame (behaviour 8); false when the
    /// field is empty or the program is not paused.
    pub fn evaluate(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self.expression.read(cx).state.buffer.text();
        let expression = text.trim();
        if expression.is_empty() {
            return false;
        }
        if let Some(id) = self.marking.take() {
            let label = expression.to_owned();
            self.mark(&id, &label, cx);
            self.expression.update(cx, |f, cx| f.set_text("", cx));
            return true;
        }
        // A renderer's expression for a type (v12 behaviour 33).
        if let Some(FieldMode::Renderer(kind)) = self.field_mode.take() {
            let expression = expression.to_owned();
            self.set_renderer(&kind, &expression, cx);
            self.expression.update(cx, |f, cx| f.set_text("", cx));
            return true;
        }
        let frame = self.frames.get(self.frame).map(|f| f.id);
        // A code fragment of several statements (v12 behaviour 36).
        if self.fragment || expression.contains('\n') {
            return match (&self.controller, frame, &self.state) {
                (Some(c), Some(frame), DebugState::Paused(_)) => {
                    c.send(Command::Fragment(expression.to_owned(), frame));
                    true
                }
                _ => {
                    self.add_result(
                        expression.to_owned(),
                        Err("Cannot evaluate: the program is not paused".into()),
                    );
                    cx.notify();
                    false
                }
            };
        }
        if let Some((parent, name)) = self.setting.take() {
            let sent = frame.is_some_and(|frame| {
                self.command(
                    Command::SetValue(frame, parent, name.clone(), expression.to_owned()),
                    cx,
                )
            });
            if !sent {
                self.add_result(name, Err("Cannot set: the program is not paused".into()));
                cx.notify();
            }
            return sent;
        }
        match (&self.controller, frame, &self.state) {
            (Some(c), Some(frame), DebugState::Paused(_)) => {
                c.send(Command::Evaluate(expression.to_owned(), frame));
                true
            }
            _ => {
                self.add_result(
                    expression.to_owned(),
                    Err("Cannot evaluate: the program is not paused".into()),
                );
                cx.notify();
                false
            }
        }
    }

    /// Quick Evaluate Expression (`SPEC-0091` v19 behaviour 44): evaluates `expression` in the
    /// selected frame without touching the field, the results or the tab; the answer comes as
    /// [`DebugViewEvent::Quick`]. `Err` with what the status bar says when the program is not
    /// paused.
    pub fn quick_evaluate(&mut self, expression: &str) -> Result<(), &'static str> {
        let expression = expression.trim();
        let frame = self.frames.get(self.frame).map(|f| f.id);
        match (&self.controller, frame, &self.state) {
            (Some(c), Some(frame), DebugState::Paused(_)) if !expression.is_empty() => {
                self.quick = Some(expression.to_owned());
                c.send(Command::Evaluate(expression.to_owned(), frame));
                Ok(())
            }
            _ => Err(crate::debug_model::NOT_PAUSED),
        }
    }

    /// Adds `expression` to the watches (once) and evaluates it when paused (v5 behaviour 11).
    pub fn add_watch(&mut self, expression: &str, cx: &mut Context<Self>) -> bool {
        let expression = expression.trim();
        if expression.is_empty() {
            return false;
        }
        if !self.watches.iter().any(|(e, _)| e == expression) {
            self.watches.push((expression.to_owned(), None));
        }
        self.tab = Tab::Variables;
        self.refresh_watches();
        cx.notify();
        true
    }

    /// Removes watch `i`.
    pub fn remove_watch(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.watches.len() {
            self.watches.remove(i);
            cx.notify();
        }
    }

    /// Watches as `expression = value`, `expression: error` or the bare expression before a value.
    pub fn watch_texts(&self) -> Vec<String> {
        self.watches
            .iter()
            .map(|(e, v)| match v {
                Some(Ok(v)) => format!("{e} = {v}"),
                Some(Err(why)) => format!("{e}: {why}"),
                None => e.clone(),
            })
            .collect()
    }

    /// Asks the session for the watches' values in the selected frame, when paused.
    fn refresh_watches(&mut self) {
        if self.watches.is_empty() || !matches!(self.state, DebugState::Paused(_)) {
            return;
        }
        if let (Some(c), Some(f)) = (&self.controller, self.frames.get(self.frame)) {
            c.send(Command::Watches(
                self.watches.iter().map(|(e, _)| e.clone()).collect(),
                f.id,
            ));
        }
    }

    /// Names of the paused program's threads.
    pub fn thread_names(&self) -> Vec<String> {
        self.threads.iter().map(|(_, n)| n.clone()).collect()
    }

    /// Shows the call stack of thread `id` (v5 behaviour 12).
    pub fn select_thread(&mut self, id: i64, cx: &mut Context<Self>) {
        if id == self.thread || !matches!(self.state, DebugState::Paused(_)) {
            return;
        }
        if let Some(c) = &self.controller {
            self.thread = id;
            c.send(Command::Thread(id));
            cx.notify();
        }
    }

    /// Tells the workbench the values of the shown variables and of the frame object's fields.
    /// The values in the editor follow the views of the variables (`0xFF` for a value shown in
    /// Hex, v13), as the reference shows them.
    fn emit_values(&self, cx: &mut Context<Self>) {
        let top: Vec<(String, String)> = self
            .shown_rows()
            .iter()
            .filter(|r| r.depth == 0)
            .map(|r| (r.var.name.clone(), r.var.value.clone()))
            .collect();
        cx.emit(DebugViewEvent::Values(crate::debug_model::editor_values(
            &top,
            &self.fields,
        )));
    }

    fn add_result(&mut self, expression: String, result: Result<String, String>) {
        self.results.insert(0, (expression, result));
        self.results.truncate(RESULTS);
    }

    /// Evaluated expressions, newest first, as `expression = value` or `expression: error`.
    pub fn result_texts(&self) -> Vec<String> {
        self.results
            .iter()
            .map(|(e, r)| match r {
                Ok(v) => format!("{e} = {v}"),
                Err(why) => format!("{e}: {why}"),
            })
            .collect()
    }

    fn on_expression_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &event.keystroke;
        // The completion list takes ↑ ↓ Tab Enter Escape while it shows (v11 behaviour 22).
        if !self.completions.is_empty() {
            let n = self.completions.len();
            match k.key.as_str() {
                "down" => self.completion = (self.completion + 1) % n,
                "up" => self.completion = (self.completion + n - 1) % n,
                "tab" | "enter" if !k.modifiers.shift && !k.modifiers.platform => {
                    let i = self.completion.min(n - 1);
                    self.accept_completion(i, cx);
                }
                "escape" => self.completions.clear(),
                _ => return self.ask_later(cx),
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape leaves the renderer's field (v12 behaviour 33).
        if k.key == "escape" && self.field_mode.take().is_some() {
            self.expression.update(cx, |f, cx| f.set_text("", cx));
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // In a code fragment Enter starts a new line and ⌘↩ / Ctrl+Enter evaluates; ⇧↩ turns
        // the field into a code fragment (v12 behaviour 36).
        if k.key == "enter" && self.fragment {
            if k.modifiers.platform || k.modifiers.control {
                self.evaluate(cx);
                cx.stop_propagation();
            }
            return;
        }
        if k.key == "enter"
            && k.modifiers.shift
            && !k.modifiers.platform
            && !k.modifiers.control
            && self.marking.is_none()
            && self.setting.is_none()
        {
            self.fragment = true;
            cx.notify();
            return;
        }
        // Escape leaves Mark Object (v11 behaviour 24).
        if k.key == "escape" && self.marking.take().is_some() {
            self.expression.update(cx, |f, cx| f.set_text("", cx));
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape leaves Set Value (SPEC-0194 behaviour 3).
        if k.key == "escape" && self.setting.take().is_some() {
            self.expression.update(cx, |f, cx| f.set_text("", cx));
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // ⇧⌘↩ / Ctrl+Shift+Enter adds the expression to the watches (v5 behaviour 11).
        if k.key == "enter" && k.modifiers.shift && (k.modifiers.platform || k.modifiers.control) {
            let text = self.expression.read(cx).state.buffer.text();
            self.add_watch(&text, cx);
            cx.stop_propagation();
            return;
        }
        if k.key == "enter"
            && !k.modifiers.control
            && !k.modifiers.alt
            && !k.modifiers.platform
            && !k.modifiers.shift
        {
            self.evaluate(cx);
            cx.stop_propagation();
            return;
        }
        // Any other key may change the text: completions are asked for once the field has it.
        self.ask_later(cx);
    }

    /// Asks for completions after the expression field took the key being handled.
    fn ask_later(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                this.ask_completions(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for completions of the field's text now (as typing does; tests).
    pub fn request_completions(&mut self, cx: &mut Context<Self>) {
        self.ask_completions(cx);
    }

    /// The program output and session notes so far.
    pub fn console(&self) -> &str {
        &self.console
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let go_on = this
                    .update(cx, |this, cx| {
                        let updates = this
                            .controller
                            .as_ref()
                            .map(Controller::poll)
                            .unwrap_or_default();
                        for u in updates {
                            this.apply(u, cx);
                        }
                        if this.controller.is_none() {
                            this.polling = false;
                        }
                        this.polling
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Running => {
                self.state = DebugState::Running;
            }
            Update::Stopped {
                reason,
                frames,
                locals,
            } => {
                self.state = DebugState::Paused(reason);
                self.set_frames(frames);
                self.vars = self.new_tree(locals);
                self.fields.clear();
                self.var_selected = None;
                self.setting = None;
                self.marking = None;
                self.field_mode = None;
                self.completions.clear();
                self.thread_infos.clear();
                self.show_frame(cx);
                self.emit_values(cx);
                self.refresh_watches();
                self.refresh_inspects();
                self.request_renders();
            }
            Update::Frames(frames, locals) => {
                self.set_frames(frames);
                self.vars = self.new_tree(locals);
                self.fields.clear();
                self.show_frame(cx);
                self.emit_values(cx);
                self.refresh_watches();
                self.refresh_inspects();
                self.request_renders();
            }
            Update::Threads(threads, shown) => {
                self.threads = threads;
                self.thread = shown;
            }
            Update::Watched(values) => {
                for (e, v) in values {
                    if let Some(w) = self.watches.iter_mut().find(|(w, _)| *w == e) {
                        w.1 = Some(v);
                    }
                }
            }
            Update::Exited(code) => {
                let line = match code {
                    Some(code) => format!("Process finished with exit code {code}"),
                    None => "Debugging stopped".to_owned(),
                };
                self.end(line, cx);
            }
            Update::Detached => self.end("Detached; the program goes on running".to_owned(), cx),
            Update::Output(text) => {
                let text = crate::debug_model::drop_echo(&mut self.echo, &text);
                self.console.push_str(&text);
                // Scroll to the End (v12 behaviour 38).
                if self.follow {
                    self.console_scroll.scroll_to_bottom();
                }
            }
            Update::Children(reference, vars) => {
                for i in &mut self.inspects {
                    i.tree.set_children(reference, vars.clone());
                }
                self.vars.set_children(reference, vars);
                self.request_renders();
            }
            Update::Values(_, vars) => self.vars.refresh_values(&vars),
            Update::Locals(_, vars) => {
                self.vars = self.new_tree(vars);
                self.fields.clear();
                self.emit_values(cx);
                self.refresh_watches();
                self.request_renders();
            }
            Update::ThreadInfos(infos) => self.thread_infos = infos,
            Update::Dump(text) => {
                self.console.push_str(&text);
                self.tab = Tab::Console;
                if self.follow {
                    self.console_scroll.scroll_to_bottom();
                }
            }
            Update::Inspected(key, value) => {
                if let Some(i) = self.inspects.iter_mut().find(|i| i.title == key) {
                    match value {
                        Ok(v) => {
                            let reference = v.reference;
                            i.tree = VarTree::new(vec![v]);
                            i.tree.set_filters(self.filters);
                            i.error = None;
                            if let Some(fetch) = i.tree.expand(reference)
                                && let Some(c) = &self.controller
                            {
                                c.send(Command::Children(fetch));
                            }
                        }
                        Err(e) => i.error = Some(e),
                    }
                }
            }
            Update::Rendered(values) => {
                for (key, value) in values {
                    let children = self.render_asked.get(&key).copied().unwrap_or(false);
                    let shown = match value {
                        Ok(v) => (
                            v.value,
                            (children && v.reference != 0).then_some(v.reference),
                        ),
                        Err(e) => (format!("(cannot show: {e})"), None),
                    };
                    self.vars.set_override(key, Some(shown));
                }
                self.emit_values(cx);
            }
            Update::Removed(path, line) => {
                cx.emit(DebugViewEvent::RemoveBreakpoint(
                    path,
                    usize::try_from(line.saturating_sub(1)).unwrap_or(0),
                ));
            }
            Update::CodeChanged(files) => {
                for f in files {
                    if !self.code_changed.contains(&f) {
                        self.code_changed.push(f);
                    }
                }
                self.console.push_str(
                    "[IDECraft] Code changed: Reload Changed Classes loads it into the program\n",
                );
            }
            Update::Reloaded(result) => {
                self.code_changed.clear();
                let line = match result {
                    Ok(classes) if classes.is_empty() => {
                        "[IDECraft] Reload Changed Classes: no class changed".to_owned()
                    }
                    Ok(classes) => format!(
                        "[IDECraft] Reloaded {} {}: {}",
                        classes.len(),
                        if classes.len() == 1 {
                            "class"
                        } else {
                            "classes"
                        },
                        classes.join(", ")
                    ),
                    Err(e) => format!("[IDECraft] Reload Changed Classes failed: {e}"),
                };
                self.console.push_str(&line);
                self.console.push('\n');
            }
            Update::FieldWatch(result) => match result {
                Ok(id) => cx.emit(DebugViewEvent::AddFieldWatch(id)),
                Err(e) => self
                    .console
                    .push_str(&format!("[IDECraft] Field Watchpoint: {e}\n")),
            },
            // Only for the frame still shown (v8 behaviour 13).
            Update::Fields(object) => {
                if self
                    .frames
                    .get(self.frame)
                    .is_some_and(|f| f.id == object.frame)
                {
                    self.vars.set_object(&object);
                    self.fields = object.fields;
                    self.emit_values(cx);
                    self.request_renders();
                }
            }
            Update::Placed(path, list) => cx.emit(DebugViewEvent::Placed(path, list)),
            Update::StepTargets(targets) => {
                self.step_targets = targets.clone();
                cx.emit(DebugViewEvent::StepTargets(targets));
            }
            Update::Completions(text, items) => {
                // Only for the text still in the field.
                if self.expression_text(cx) == text {
                    let word = crate::debug_model::completion_word(&text).to_owned();
                    self.completions = items
                        .into_iter()
                        .filter(|i| *i != word)
                        .take(crate::debug_model::COMPLETIONS)
                        .collect();
                    self.completion = 0;
                }
            }
            // The answer to Quick Evaluate Expression goes to the hint below the caret, not to
            // the results (`SPEC-0091` v19 behaviour 44).
            Update::Evaluated(expression, result)
                if self.quick.as_deref() == Some(expression.as_str()) =>
            {
                self.quick = None;
                cx.emit(DebugViewEvent::Quick(crate::debug_model::quick_text(
                    &expression,
                    &result,
                )));
            }
            Update::Evaluated(expression, result) => {
                self.tab = Tab::Variables;
                self.add_result(expression, result);
            }
            Update::ValueSet(name, result) => {
                self.tab = Tab::Variables;
                if result.is_ok() {
                    self.expression.update(cx, |f, cx| f.set_text("", cx));
                } else {
                    self.add_result(name, result);
                }
            }
            Update::Failed(e) => {
                if matches!(self.state, DebugState::Starting) {
                    self.end(format!("Cannot debug: {e}"), cx);
                } else {
                    self.console.push_str(&format!("{e}\n"));
                }
            }
        }
        cx.notify();
    }

    fn end(&mut self, line: String, cx: &mut Context<Self>) {
        self.console.push_str(&format!("\n{line}\n"));
        self.code_changed.clear();
        self.thread_infos.clear();
        self.field_mode = None;
        if self.follow {
            self.console_scroll.scroll_to_bottom();
        }
        self.state = DebugState::Ended(line);
        self.controller = None;
        self.set_frames(Vec::new());
        self.threads.clear();
        self.threads_open = false;
        self.vars = VarTree::default();
        self.fields.clear();
        for w in &mut self.watches {
            w.1 = None;
        }
        cx.emit(DebugViewEvent::Ended);
    }

    fn show_frame(&mut self, cx: &mut Context<Self>) {
        if let Some(f) = self.frames.get(self.frame)
            && let Some(path) = &f.path
        {
            cx.emit(DebugViewEvent::ShowLine(
                path.clone(),
                usize::try_from(f.line - 1).unwrap_or(0),
            ));
        }
    }

    fn select_frame(&mut self, i: usize, cx: &mut Context<Self>) {
        if i == self.frame || i >= self.frames.len() {
            return;
        }
        self.frame = i;
        if let Some(c) = &self.controller {
            c.send(Command::Frame(self.frames[i].id));
        }
        self.show_frame(cx);
        cx.notify();
    }

    fn toggle_var(&mut self, reference: i64, cx: &mut Context<Self>) {
        if let Some(fetch) = self.vars.toggle(reference)
            && let Some(c) = &self.controller
        {
            c.send(Command::Children(fetch));
        }
        cx.notify();
    }

    /// A toolbar button with its declared tooltip (`SPEC-0194` v4 behaviour 11, `SPEC-0294`).
    fn tool(
        &self,
        control: &'static idecraft_ui::tooltip::Control,
        key: &'static str,
        enabled: bool,
        command: Command,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        div()
            .id(control.name)
            .tip(control)
            .p(px(2.))
            .rounded(px(4.))
            .when(!enabled, |d| d.opacity(0.4))
            .when(enabled, |d| d.hover(|s| s.bg(c(p.tree_hover))))
            .child(icon(key))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.command(command.clone(), cx);
            }))
    }

    /// A toolbar button that asks the workbench to run `action` (`SPEC-0194` v3 behaviour 6).
    fn action_tool(
        &self,
        control: &'static idecraft_ui::tooltip::Control,
        key: &'static str,
        enabled: bool,
        pressed: bool,
        action: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        div()
            .id(control.name)
            .tip(control)
            .p(px(2.))
            .rounded(px(4.))
            .when(!enabled, |d| d.opacity(0.4))
            .when(pressed, |d| d.bg(c(p.tree_selection)))
            .when(enabled, |d| d.hover(|s| s.bg(c(p.tree_hover))))
            .child(icon(key))
            .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                if enabled {
                    cx.emit(DebugViewEvent::Action(action));
                }
            }))
    }
}

/// The colours a mark can have (`SPEC-0091` v12 behaviour 35): indexes into the console's ANSI
/// colours of the theme (red, yellow, green, blue, magenta, cyan).
pub const MARK_COLORS: [usize; 6] = [1, 3, 2, 4, 5, 6];

/// The further actions of the Debug window (`SPEC-0091` v12 behaviours 25–38).
impl DebugView {
    /// The thread that stopped last (the one shown after a stop).
    fn stopped_thread(&self) -> i64 {
        self.thread
    }

    /// The language of the session started last.
    pub fn lang(&self) -> Lang {
        use crate::debug_model::Kind;
        match self.last.as_ref().map(|(_, l)| l.kind) {
            Some(Kind::Java | Kind::Kotlin) => Lang::Java,
            Some(Kind::Python) => Lang::Python,
            _ => Lang::Other,
        }
    }

    /// A variables tree on `locals` with the window's filters.
    fn new_tree(&mut self, locals: Vec<Variable>) -> VarTree {
        self.render_asked.clear();
        let mut tree = VarTree::new(locals);
        tree.set_filters(self.filters);
        tree
    }

    /// The rows of the Variables list as shown: numbers in their chosen view (behaviour 32).
    pub fn shown_rows(&self) -> Vec<crate::debug_model::VarRow> {
        let mut rows = self.vars.rows();
        for i in 0..rows.len() {
            let expression = crate::debug_model::row_expression(&rows, i);
            if let Some(view) = self.views.get(&expression).filter(|v| v.numeric())
                && let Some(shown) = extras::view_number(&rows[i].var.value, *view)
            {
                rows[i].var.value = shown;
            }
        }
        rows
    }

    /// Asks the session for the rows shown in an evaluated view or by a renderer, once per stop
    /// (behaviours 32, 33).
    fn request_renders(&mut self) {
        let (Some(c), Some(frame), DebugState::Paused(_)) = (
            &self.controller,
            self.frames.get(self.frame).map(|f| f.id),
            &self.state,
        ) else {
            return;
        };
        let lang = match self.last.as_ref().map(|(_, l)| l.kind) {
            Some(crate::debug_model::Kind::Java | crate::debug_model::Kind::Kotlin) => Lang::Java,
            Some(crate::debug_model::Kind::Python) => Lang::Python,
            _ => Lang::Other,
        };
        let rows = self.vars.rows();
        let mut ask = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let key = (r.parent, r.var.name.clone());
            if self.render_asked.contains_key(&key) {
                continue;
            }
            let expression = crate::debug_model::row_expression(&rows, i);
            let view = self.views.get(&expression).copied().unwrap_or_default();
            let wanted = if view != ViewAs::Auto && !view.numeric() {
                extras::view_expression(lang, &expression, view)
                    .map(|e| (e, matches!(view, ViewAs::Elements | ViewAs::Entries)))
            } else if view == ViewAs::Auto && !self.renderers_muted && lang != Lang::Other {
                r.var
                    .kind
                    .as_deref()
                    .and_then(|k| extras::renderer_for(&self.renderers, k))
                    .map(|rd| {
                        (
                            extras::renderer_expression(lang, &rd.expression, &expression),
                            false,
                        )
                    })
            } else {
                None
            };
            if let Some((e, children)) = wanted {
                self.render_asked.insert(key.clone(), children);
                ask.push((key, e));
            }
        }
        if !ask.is_empty() {
            c.send(Command::Render(ask, frame));
        }
    }

    /// Evaluates the Inspect tabs again in the shown frame (behaviour 30).
    fn refresh_inspects(&mut self) {
        let (Some(c), Some(frame)) = (&self.controller, self.frames.get(self.frame).map(|f| f.id))
        else {
            return;
        };
        for i in &self.inspects {
            c.send(Command::Inspect(
                i.title.clone(),
                i.expression.clone(),
                i.referring,
                frame,
            ));
        }
    }

    /// Step Out of Code Block (`StepOutOfBlock`, behaviour 25).
    pub fn step_out_of_block(&mut self, cx: &mut Context<Self>) -> bool {
        self.command(Command::StepOutOfBlock, cx)
    }

    /// Resume All Threads (behaviour 27).
    pub fn resume_all(&mut self, cx: &mut Context<Self>) -> bool {
        self.command(Command::ResumeAll, cx)
    }

    /// Pause All Threads (behaviour 27).
    pub fn pause_all(&mut self, cx: &mut Context<Self>) -> bool {
        self.command(Command::PauseAll, cx)
    }

    /// Force Return in the selected frame (behaviour 29, refused by the session with a reason).
    pub fn force_return(&mut self, cx: &mut Context<Self>) -> bool {
        match self.frames.get(self.frame).map(|f| f.id) {
            Some(id) => self.command(Command::ForceReturn(id), cx),
            None => false,
        }
    }

    /// Throw Exception in the selected frame (behaviour 29, refused by the session with a reason).
    pub fn throw_exception(&mut self, cx: &mut Context<Self>) -> bool {
        match self.frames.get(self.frame).map(|f| f.id) {
            Some(id) => self.command(Command::ThrowException(id), cx),
            None => false,
        }
    }

    /// Get Thread Dump (`DumpThreads`, behaviour 26): into the console.
    pub fn thread_dump(&mut self, cx: &mut Context<Self>) -> bool {
        match self.frames.first().map(|f| f.id) {
            Some(id) => self.command(Command::Threads(true, id), cx),
            None => false,
        }
    }

    /// Reads the threads' states and groups for the thread list (behaviour 28).
    fn request_thread_states(&mut self, cx: &mut Context<Self>) {
        if self.thread_infos.is_empty()
            && let Some(id) = self.frames.first().map(|f| f.id)
        {
            self.command(Command::Threads(false, id), cx);
        }
    }

    /// Opens the thread list (the selector's click; tests).
    pub fn open_thread_list(&mut self, cx: &mut Context<Self>) {
        self.threads_open = true;
        self.request_thread_states(cx);
        cx.notify();
    }

    /// Closes the thread list if it is open (Escape, v13); whether it was.
    pub fn close_thread_list(&mut self, cx: &mut Context<Self>) -> bool {
        let open = std::mem::take(&mut self.threads_open);
        if open {
            cx.notify();
        }
        open
    }

    /// The threads as the list shows them: name, state and group, when read (behaviour 28).
    pub fn thread_infos(&self) -> &[ThreadInfo] {
        &self.thread_infos
    }

    /// The editor of a saved source, whose errors hold back hot swap (v13).
    pub fn watch_errors_of(&mut self, path: PathBuf, editor: gpui::WeakEntity<EditorView>) {
        self.saved_editors.insert(path, editor);
    }

    /// The changed sources the editor shows errors in, as `File.java (n errors)`.
    fn changed_with_errors(&self, cx: &gpui::App) -> Vec<String> {
        self.code_changed
            .iter()
            .filter_map(|p| {
                let n = self
                    .saved_editors
                    .get(p)
                    .and_then(gpui::WeakEntity::upgrade)
                    .map_or(0, |e| e.read(cx).status().problems.0);
                let name = p.file_name()?.to_string_lossy().into_owned();
                (n > 0).then(|| match n {
                    1 => format!("{name} (1 error)"),
                    n => format!("{name} ({n} errors)"),
                })
            })
            .collect()
    }

    /// Reload Changed Classes (`Hotswap`, behaviour 37). Refused while the editor shows errors in
    /// a changed source (v13): jdtls builds such a class with methods that throw, and reloading it
    /// would break the program; the reference does not reload either.
    pub fn reload_classes(&mut self, cx: &mut Context<Self>) -> bool {
        let ok = self.active() && !matches!(self.state, DebugState::Starting);
        let errors = self.changed_with_errors(cx);
        if ok && !errors.is_empty() {
            self.console.push_str(&format!(
                "[IDECraft] Reload Changed Classes: fix the errors first: {}\n",
                errors.join(", ")
            ));
            cx.notify();
            return false;
        }
        if ok && let Some(c) = &self.controller {
            c.send(Command::ReloadClasses);
            cx.notify();
        }
        ok
    }

    /// Saved Java sources while a Java session runs: built for hot swap (behaviour 37); false for
    /// other sessions and files.
    pub fn sources_saved(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) -> bool {
        let files: Vec<PathBuf> = files
            .into_iter()
            .filter(|f| f.extension().is_some_and(|e| e == "java"))
            .collect();
        if files.is_empty()
            || self.lang() != Lang::Java
            || !matches!(self.state, DebugState::Running | DebugState::Paused(_))
        {
            return false;
        }
        if let Some(c) = &self.controller {
            c.send(Command::SourcesChanged(files));
            cx.notify();
        }
        true
    }

    /// The sources built and waiting to be reloaded (behaviour 37).
    pub fn code_changed(&self) -> &[PathBuf] {
        &self.code_changed
    }

    /// Opens an Inspect tab (`referring`: the objects referring to it) for the selected row
    /// (behaviours 30, 31).
    pub fn inspect_selected(&mut self, referring: bool, cx: &mut Context<Self>) -> bool {
        let Some((name, _, expression)) = self.selected_variable() else {
            return false;
        };
        let title = if referring {
            format!("Referring to {name}")
        } else {
            format!("Inspect {name}")
        };
        let at = match self.inspects.iter().position(|i| i.title == title) {
            Some(at) => at,
            None => {
                if self.inspects.len() >= INSPECTS {
                    self.inspects.remove(0);
                }
                self.inspects.push(Inspect {
                    title: title.clone(),
                    expression,
                    referring,
                    tree: VarTree::default(),
                    error: None,
                });
                self.inspects.len() - 1
            }
        };
        self.tab = Tab::Inspect(at);
        self.refresh_inspects();
        cx.notify();
        true
    }

    /// Closes Inspect tab `i`.
    pub fn close_inspect(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.inspects.len() {
            self.inspects.remove(i);
            self.tab = Tab::Variables;
            cx.notify();
        }
    }

    /// Titles of the Inspect tabs.
    pub fn inspect_titles(&self) -> Vec<String> {
        self.inspects.iter().map(|i| i.title.clone()).collect()
    }

    /// The rows of Inspect tab `i` as `name = value`, or its error.
    pub fn inspect_texts(&self, i: usize) -> Vec<String> {
        let Some(inspect) = self.inspects.get(i) else {
            return Vec::new();
        };
        if let Some(e) = &inspect.error {
            return vec![e.clone()];
        }
        inspect
            .tree
            .rows()
            .iter()
            .map(|r| format!("{} = {}", r.var.name, r.var.value))
            .collect()
    }

    /// The view of the selected row's value (behaviour 32).
    pub fn view_of_selected(&self) -> ViewAs {
        self.selected_variable()
            .and_then(|(_, _, e)| self.views.get(&e).copied())
            .unwrap_or_default()
    }

    /// Whether the selected row shows an integer (the number views apply).
    pub fn selected_is_number(&self) -> bool {
        let rows = self.vars.rows();
        self.var_selected
            .and_then(|i| rows.get(i))
            .is_some_and(|r| extras::view_number(&r.var.value, ViewAs::Decimal).is_some())
    }

    /// Whether the selected row is a field of an object (a field watchpoint can be set).
    pub fn selected_is_field(&self) -> bool {
        let rows = self.vars.rows();
        self.var_selected
            .and_then(|i| rows.get(i))
            .is_some_and(|r| r.parent != 0)
    }

    /// Shows the selected row's value in `view` (behaviour 32).
    pub fn set_view_selected(&mut self, view: ViewAs, cx: &mut Context<Self>) -> bool {
        let rows = self.vars.rows();
        let Some(i) = self.var_selected.filter(|i| *i < rows.len()) else {
            return false;
        };
        let expression = crate::debug_model::row_expression(&rows, i);
        let key = (rows[i].parent, rows[i].var.name.clone());
        if view == ViewAs::Auto {
            self.views.remove(&expression);
        } else {
            self.views.insert(expression, view);
        }
        self.vars.set_override(key.clone(), None);
        self.render_asked.remove(&key);
        self.request_renders();
        self.emit_values(cx);
        cx.notify();
        true
    }

    /// Starts giving the selected row's type a renderer (behaviour 33): the expression field
    /// asks for the expression shown in place of the value, `this`/`self` being the value.
    pub fn start_renderer(&mut self, cx: &mut Context<Self>) -> bool {
        let rows = self.vars.rows();
        let Some(kind) = self
            .var_selected
            .and_then(|i| rows.get(i))
            .and_then(|r| r.var.kind.clone())
            .filter(|k| !k.is_empty())
        else {
            return false;
        };
        let current = self
            .renderers
            .iter()
            .find(|r| extras::type_matches(&r.type_name, &kind))
            .map(|r| r.expression.clone())
            .unwrap_or_else(|| {
                if self.lang() == Lang::Python {
                    "str(self)".into()
                } else {
                    "this.toString()".into()
                }
            });
        self.setting = None;
        self.marking = None;
        self.field_mode = Some(FieldMode::Renderer(kind));
        self.set_expression(&current, cx);
        true
    }

    /// Gives type `kind` the renderer `expression` (empty: removes it) and saves the renderers
    /// with the workspace.
    pub fn set_renderer(&mut self, kind: &str, expression: &str, cx: &mut Context<Self>) {
        self.renderers
            .retain(|r| !extras::type_matches(&r.type_name, kind));
        if !expression.trim().is_empty() {
            self.renderers.push(Renderer {
                type_name: kind.to_owned(),
                expression: expression.trim().to_owned(),
                enabled: true,
            });
        }
        if !self.workspace.as_os_str().is_empty() {
            idecraft_debugger::saved::save_renderers(&self.workspace, &self.renderers);
        }
        self.rerender(cx);
    }

    /// The renderers by type.
    pub fn renderers(&self) -> &[Renderer] {
        &self.renderers
    }

    /// Whether the renderers are muted.
    pub fn renderers_muted(&self) -> bool {
        self.renderers_muted
    }

    /// Mutes or unmutes the renderers (behaviour 33).
    pub fn toggle_mute_renderers(&mut self, cx: &mut Context<Self>) {
        self.renderers_muted = !self.renderers_muted;
        self.rerender(cx);
    }

    /// Drops the shown views and renderers and asks for them again.
    fn rerender(&mut self, cx: &mut Context<Self>) {
        for (key, _) in std::mem::take(&mut self.render_asked) {
            self.vars.set_override(key, None);
        }
        self.request_renders();
        self.emit_values(cx);
        cx.notify();
    }

    /// The filters of the Variables list (behaviour 34).
    pub fn filters(&self) -> VarFilters {
        self.filters
    }

    /// Switches a filter: `statics`, `synthetic` or `nulls` (behaviour 34). Java's static fields
    /// come from java-debug, so the frame's variables are fetched again.
    pub fn toggle_filter(&mut self, which: &str, cx: &mut Context<Self>) {
        match which {
            "statics" => self.filters.statics = !self.filters.statics,
            "synthetic" => self.filters.synthetic = !self.filters.synthetic,
            "nulls" => self.filters.nulls = !self.filters.nulls,
            _ => return,
        }
        self.filters_chosen = true;
        self.vars.set_filters(self.filters);
        for i in &mut self.inspects {
            i.tree.set_filters(self.filters);
        }
        if which == "statics"
            && self.lang() == Lang::Java
            && let Some(c) = &self.controller
        {
            c.send(Command::Settings(
                serde_json::json!({"showStaticVariables": self.filters.statics}),
            ));
            if let (DebugState::Paused(_), Some(f)) = (&self.state, self.frames.get(self.frame)) {
                c.send(Command::Frame(f.id));
            }
        }
        self.var_selected = None;
        cx.notify();
    }

    /// Asks for a field watchpoint on the selected field (`SPEC-0146` v3); the workbench adds it
    /// when the adapter gives the field's id.
    pub fn add_field_watch_selected(&mut self, cx: &mut Context<Self>) -> bool {
        let rows = self.vars.rows();
        let Some(row) = self.var_selected.and_then(|i| rows.get(i)) else {
            return false;
        };
        if row.parent == 0 {
            return false;
        }
        self.command(Command::FieldWatch(row.parent, row.var.name.clone()), cx)
    }

    /// Whether the expression field takes a code fragment of several lines (behaviour 36).
    pub fn fragment(&self) -> bool {
        self.fragment
    }

    /// Switches between one expression and a code fragment (behaviour 36).
    pub fn toggle_fragment(&mut self, cx: &mut Context<Self>) {
        self.fragment = !self.fragment;
        cx.notify();
    }

    /// Empties the console (Clear All, behaviour 38).
    pub fn clear_console(&mut self, cx: &mut Context<Self>) {
        self.console.clear();
        self.echo = None;
        self.stack_line = None;
        cx.notify();
    }

    /// Whether long console lines wrap (Soft-Wrap, behaviour 38).
    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    /// Switches Soft-Wrap.
    pub fn toggle_soft_wrap(&mut self, cx: &mut Context<Self>) {
        self.soft_wrap = !self.soft_wrap;
        cx.notify();
    }

    /// Whether the console follows its end (Scroll to the End, behaviour 38).
    pub fn follow(&self) -> bool {
        self.follow
    }

    /// Switches Scroll to the End; switching it on scrolls there.
    pub fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        self.follow = !self.follow;
        if self.follow {
            self.console_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// Up or Down the Stack Trace (behaviour 38): the previous or next console line that names a
    /// file and line as a stack frame is marked and opened; false when there is none.
    pub fn stack_step(&mut self, up: bool, cx: &mut Context<Self>) -> bool {
        let lines: Vec<&str> = self.console.lines().collect();
        let found = if up {
            let before = self.stack_line.unwrap_or(lines.len());
            (0..before)
                .rev()
                .find(|&i| extras::stack_location(lines[i]).is_some())
        } else {
            let after = self.stack_line.map_or(0, |l| l + 1);
            (after..lines.len()).find(|&i| extras::stack_location(lines[i]).is_some())
        };
        let Some(i) = found else {
            return false;
        };
        self.stack_line = Some(i);
        self.tab = Tab::Console;
        if let Some((file, line)) = extras::stack_location(lines[i]) {
            cx.emit(DebugViewEvent::OpenLocation(file, line));
        }
        cx.notify();
        true
    }

    /// The console line Up/Down the Stack Trace marked last.
    pub fn stack_line(&self) -> Option<usize> {
        self.stack_line
    }

    /// Puts `text` into the console's input line (tests).
    pub fn set_console_input(&mut self, text: &str, cx: &mut Context<Self>) {
        self.console_input.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Runs the console's input in the selected frame (behaviour 38): the `repl` context, so
    /// Python statements run too; false when empty or not paused.
    pub fn submit_console_input(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self.console_input.read(cx).state.buffer.text();
        let text = text.trim().to_owned();
        if text.is_empty() {
            return false;
        }
        let frame = self.frames.get(self.frame).map(|f| f.id);
        let sent = match frame {
            Some(frame) => self.command(Command::Repl(text.clone(), frame), cx),
            None => false,
        };
        if !sent {
            self.console.push_str(&format!(
                ">>> {text}\nCannot run: the program is not paused\n"
            ));
        }
        self.console_input.update(cx, |f, cx| f.set_text("", cx));
        if self.follow {
            self.console_scroll.scroll_to_bottom();
        }
        cx.notify();
        sent
    }

    fn on_console_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &event.keystroke;
        if k.key == "enter" && !k.modifiers.shift {
            self.submit_console_input(cx);
            cx.stop_propagation();
        }
    }
}

impl DebugView {
    /// The tab of the debug session for the shared header, when there is one (SPEC-0061 v5
    /// behaviour 18).
    pub fn header_tabs(&mut self, focused: bool, _cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        if self.name.is_empty() {
            return Vec::new();
        }
        vec![
            crate::metrics::header_tab("debug-session-tab", true, focused)
                .child(icon("run/debug"))
                .child(self.name.clone())
                .into_any_element(),
        ]
    }
}

impl Render for DebugView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let paused = matches!(self.state, DebugState::Paused(_));
        let tools = self.toolbar();
        let status = match &self.state {
            DebugState::Starting => "Starting…".to_owned(),
            DebugState::Running => "Running".to_owned(),
            DebugState::Paused(r) => format!("Paused ({r})"),
            DebugState::Ended(line) => line.clone(),
        };
        let mut mono = font(crate::fonts::editor_family());
        mono.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
            if cfg!(target_os = "macos") {
                "Menlo".into()
            } else {
                "DejaVu Sans Mono".into()
            },
        ]));
        let tab = |id: &'static str, label: &'static str, which: Tab, cx: &mut Context<Self>| {
            let on = self.tab == which;
            div()
                .id(id)
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .when(!on, |d| {
                    d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
                })
                .child(label)
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.tab = which;
                    cx.notify();
                }))
        };
        let body = match self.tab {
            // The console with its side bar and input line (v12 behaviour 38).
            Tab::Console => {
                let side = |control: &'static idecraft_ui::tooltip::Control,
                            key: &'static str,
                            on: bool,
                            cx: &mut Context<Self>,
                            act: fn(&mut Self, &mut Context<Self>)| {
                    div()
                        .id(control.name)
                        .tip(control)
                        .p(px(2.))
                        .rounded(px(4.))
                        .when(on, |d| d.bg(c(p.tree_selection)))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(icon(key))
                        .on_click(
                            cx.listener(move |this, _: &gpui::ClickEvent, _, cx| act(this, cx)),
                        )
                };
                let stack_line = self.stack_line;
                let wrap = self.soft_wrap;
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .id("debug-console-side")
                            .w(px(28.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .py_1()
                            .border_r_1()
                            .border_color(c(p.separator))
                            .child(side(
                                declared!(DEBUG_TOOLBAR, "debug-console-up"),
                                "general/up",
                                false,
                                cx,
                                |v, cx| {
                                    v.stack_step(true, cx);
                                },
                            ))
                            .child(side(
                                declared!(DEBUG_TOOLBAR, "debug-console-down"),
                                "general/down",
                                false,
                                cx,
                                |v, cx| {
                                    v.stack_step(false, cx);
                                },
                            ))
                            .child(side(
                                declared!(DEBUG_TOOLBAR, "debug-console-wrap"),
                                "general/softWrap",
                                self.soft_wrap,
                                cx,
                                Self::toggle_soft_wrap,
                            ))
                            .child(side(
                                declared!(DEBUG_TOOLBAR, "debug-console-end"),
                                "general/scrollDown",
                                self.follow,
                                cx,
                                Self::toggle_follow,
                            ))
                            .child(side(
                                declared!(DEBUG_TOOLBAR, "debug-console-clear"),
                                "general/delete",
                                false,
                                cx,
                                Self::clear_console,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id("debug-console")
                                    .track_scroll(&self.console_scroll)
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .when(!wrap, |d| d.overflow_x_scroll())
                                    .px_3()
                                    .py_1()
                                    .bg(rgb(p.console_background))
                                    .font(mono.clone())
                                    .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                                    .when(wrap, |d| d.whitespace_normal())
                                    .when(!wrap, |d| d.whitespace_nowrap())
                                    .children(self.console.lines().enumerate().map(|(i, l)| {
                                        div()
                                            .when(stack_line == Some(i), |d| {
                                                d.bg(c(p.tree_selection))
                                            })
                                            .child(l.to_owned())
                                    })),
                            )
                            // Commands for the paused program (Python: statements too).
                            .child(
                                div()
                                    .id("debug-console-input")
                                    .h(px(26.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .border_t_1()
                                    .border_color(c(p.separator))
                                    .font(mono.clone())
                                    .capture_key_down(cx.listener(Self::on_console_key))
                                    .child(div().flex_none().text_color(c(p.ui_muted)).child(">>>"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(20.))
                                            .overflow_hidden()
                                            .child(self.console_input.clone()),
                                    ),
                            ),
                    )
                    .into_any_element()
            }
            // An Inspect tab: the value's tree (v12 behaviours 30, 31).
            Tab::Inspect(at) => {
                let (rows, error) = match self.inspects.get(at) {
                    Some(i) => (i.tree.rows(), i.error.clone()),
                    None => (Vec::new(), None),
                };
                div()
                    .id("debug-inspect")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_1()
                    .children(error.map(|e| {
                        div()
                            .px_2()
                            .py_1()
                            .text_color(rgb(p.console_ansi_text[1]))
                            .child(e)
                    }))
                    .children(rows.into_iter().enumerate().map(|(k, r)| {
                        let reference = r.var.reference;
                        let chevron = if reference == 0 {
                            None
                        } else if r.expanded {
                            Some("general/chevronDown")
                        } else {
                            Some("general/chevronRight")
                        };
                        div()
                            .id(("debug-inspect-row", k))
                            .h(px(Palette::current().tree_row_height))
                            .flex()
                            .items_center()
                            .gap_1()
                            .pl(px(crate::metrics::tree_indent(r.depth)))
                            .pr_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(match chevron {
                                Some(key) => icon(key),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .child(icon(if reference == 0 {
                                "debugger/dbPrimitive"
                            } else {
                                "debugger/dbObject"
                            }))
                            .child(div().flex_none().child(r.var.name.clone()))
                            .child(div().flex_none().text_color(c(p.ui_dim)).child(" = "))
                            .child(
                                div()
                                    .flex_shrink()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .font(mono.clone())
                                    .child(value_text(&r.var.value, c(p.ui_dim))),
                            )
                            .children(
                                r.var.kind.clone().map(|t| {
                                    div().flex_none().pl_2().text_color(c(p.ui_dim)).child(t)
                                }),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                if let Some(i) = this.inspects.get_mut(at)
                                    && let Some(fetch) = i.tree.toggle(reference)
                                    && let Some(c) = &this.controller
                                {
                                    c.send(Command::Children(fetch));
                                }
                                cx.notify();
                            }))
                    }))
                    .into_any_element()
            }
            Tab::Variables => {
                let frames = self.frames.clone();
                let selected = self.frame;
                let rows = self.shown_rows();
                let selected_var = self.var_selected;
                let threads = self.threads.clone();
                let thread_rows = thread_rows(&threads, &self.thread_infos);
                let stopped_thread = self.stopped_thread();
                let shown_thread = self.thread;
                let threads_open = self.threads_open;
                let thread_name = self.shown_thread_name().unwrap_or_default().to_owned();
                let frame_rows = self.frame_rows();
                let watches = self.watch_texts();
                let completions = self.completions.clone();
                let completion = self.completion;
                let mode = if self.marking.is_some() {
                    Some("Mark as".to_owned())
                } else if self.setting.is_some() {
                    Some("Set to".to_owned())
                } else if let Some(FieldMode::Renderer(kind)) = &self.field_mode {
                    Some(format!("Show {kind} as"))
                } else {
                    None
                };
                let marking = self.marking.is_some();
                let mark_color = self.mark_color;
                let fragment = self.fragment;
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .id("debug-frames")
                            .w(px(300.))
                            .flex_none()
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(c(p.separator))
                            .px_1()
                            // The shown thread in a selector; a click lists all threads, a click
                            // on one shows its stack (v10 behaviour 12).
                            .when(!threads.is_empty(), |d| {
                                d.child(
                                    div()
                                        .id("debug-thread-selector")
                                        .h(px(24.))
                                        .my_1()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(c(p.separator))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .hover(|s| s.bg(c(p.tree_hover)))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_ellipsis()
                                                .child(thread_name),
                                        )
                                        .child(icon("general/chevronDown"))
                                        .on_click(cx.listener(
                                            |this, _: &gpui::ClickEvent, _, cx| {
                                                this.threads_open = !this.threads_open;
                                                // States and groups (v12 behaviour 28).
                                                if this.threads_open {
                                                    this.request_thread_states(cx);
                                                }
                                                cx.notify();
                                            },
                                        )),
                                )
                            })
                            .when(threads_open, |d| {
                                d.children(thread_rows.into_iter().map(|row| {
                                    let (id, name, info) = match row {
                                        ThreadRow::Group(group) => {
                                            return div()
                                                .h(px(20.))
                                                .flex()
                                                .items_center()
                                                .px_2()
                                                .text_xs()
                                                .text_color(c(p.ui_dim))
                                                .child(group)
                                                .into_any_element();
                                        }
                                        ThreadRow::Thread(id, name, info) => (id, name, info),
                                    };
                                    let on = id == shown_thread;
                                    let state_icon = info
                                        .as_ref()
                                        .map(|t| icon(thread_icon(t, id == stopped_thread)));
                                    let state_text = info
                                        .as_ref()
                                        .map(|t| t.state.clone())
                                        .filter(|s| !s.is_empty());
                                    div()
                                        .id(("debug-thread", id as u64))
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .pl(px(20.))
                                        .pr_2()
                                        .rounded(px(4.))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .when(on, |d| d.bg(c(p.tree_selection)))
                                        .when(!on, |d| {
                                            d.text_color(c(p.ui_muted))
                                                .hover(|s| s.bg(c(p.tree_hover)))
                                        })
                                        .children(state_icon)
                                        .child(div().flex_1().min_w_0().text_ellipsis().child(name))
                                        .children(state_text.map(|s| {
                                            div()
                                                .flex_none()
                                                .text_xs()
                                                .text_color(c(p.ui_dim))
                                                .child(s)
                                        }))
                                        .on_click(cx.listener(
                                            move |this, _: &gpui::ClickEvent, _, cx| {
                                                this.threads_open = false;
                                                this.select_thread(id, cx);
                                                cx.notify();
                                            },
                                        ))
                                        .into_any_element()
                                }))
                            })
                            .when(frames.is_empty(), |d| {
                                d.child(div().px_2().py_1().text_color(c(p.ui_muted)).child(
                                    if paused {
                                        "No frames"
                                    } else {
                                        "Frames appear when the program is paused"
                                    },
                                ))
                            })
                            .children(frame_rows.into_iter().map(|row| {
                                let i = match row {
                                    FrameRow::Frame(i) => i,
                                    // Folded library frames; a click shows them (v10 behaviour 15).
                                    FrameRow::Hidden { first, count } => {
                                        return div()
                                            .id(("debug-hidden", first))
                                            .h(px(22.))
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .px_2()
                                            .rounded(px(4.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_color(c(p.ui_dim))
                                            .hover(|s| s.bg(c(p.tree_hover)))
                                            .child(div().w(px(16.)).flex_none())
                                            .child(crate::debug_model::hidden_label(count))
                                            .on_click(cx.listener(
                                                move |this, _: &gpui::ClickEvent, _, cx| {
                                                    this.open_hidden(first, cx)
                                                },
                                            ))
                                            .into_any_element();
                                    }
                                };
                                let f = &frames[i];
                                let file = f
                                    .path
                                    .as_ref()
                                    .and_then(|p| p.file_name())
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                div()
                                    .id(("debug-frame", i))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                                    .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                    .child(icon("debugger/frame"))
                                    .child(
                                        div().flex_none().child(format!("{}:{}", f.name, f.line)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .text_color(c(if i == selected {
                                                p.ui_muted
                                            } else {
                                                p.ui_dim
                                            }))
                                            .child(file),
                                    )
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.select_frame(i, cx)
                                        },
                                    ))
                                    // Reset Frame and Copy Stack (v11 behaviour 19).
                                    .map(|d| {
                                        crate::mouse::on_secondary_click(
                                            d,
                                            cx.listener(
                                                move |this, e: &gpui::MouseDownEvent, _, cx| {
                                                    this.select_frame(i, cx);
                                                    cx.emit(DebugViewEvent::Menu(
                                                        DebugMenu::Frame,
                                                        e.position,
                                                    ));
                                                },
                                            ),
                                        )
                                    })
                                    .into_any_element()
                            })),
                    )
                    .child(
                        div()
                            .id("debug-variables")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .px_1()
                            .child(
                                div()
                                    .id("debug-expression")
                                    // What Enter does when it is not evaluating (v11 behaviour 24).
                                    .children(mode.map(|m| {
                                        div().flex_none().text_color(c(p.ui_muted)).child(m)
                                    }))
                                    .h(px(if fragment { 86. } else { 26. }))
                                    .my_1()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(c(p.separator))
                                    .capture_key_down(cx.listener(Self::on_expression_key))
                                    // The field fills its line: without a height it is drawn 0 px high
                                    // and cannot be clicked into. The hint is a placeholder inside the
                                    // field, shown only while it is empty (v6 behaviour 8).
                                    .child(
                                        div()
                                            .relative()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(if fragment { 80. } else { 20. }))
                                            .overflow_hidden()
                                            .child(self.expression.clone())
                                            .when(self.expression_placeholder(cx).is_some(), |d| {
                                                d.child(
                                                    div()
                                                        .absolute()
                                                        .top_0()
                                                        .left(px(4.))
                                                        .h(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .whitespace_nowrap()
                                                        .text_color(c(p.ui_dim))
                                                        .children(self.expression_placeholder(cx)),
                                                )
                                            }),
                                    )
                                    // The colours of the mark being given (v12 behaviour 35).
                                    .when(marking, |d| {
                                        d.children(MARK_COLORS.iter().enumerate().map(|(k, &a)| {
                                            div()
                                                .id(("debug-mark-color", k))
                                                .size(px(14.))
                                                .flex_none()
                                                .rounded(px(7.))
                                                .bg(rgb(p.console_ansi[a]))
                                                .when(k == mark_color, |d| {
                                                    d.border_2().border_color(c(p.ui_text))
                                                })
                                                // The field keeps the keyboard, so Enter still
                                                // gives the mark after the colour is chosen.
                                                .on_click(cx.listener(
                                                    move |this, _: &gpui::ClickEvent, window, cx| {
                                                        this.set_mark_color(k, cx);
                                                        window.focus(&this.expression_focus(cx));
                                                    },
                                                ))
                                        }))
                                    })
                                    // One expression or a code fragment (v12 behaviour 36).
                                    .child(
                                        div()
                                            .id("debug-fragment")
                                            .tip(declared!(DEBUG_TOOLBAR, "debug-fragment"))
                                            .flex_none()
                                            .p(px(2.))
                                            .rounded(px(4.))
                                            .when(fragment, |d| d.bg(c(p.tree_selection)))
                                            .hover(|s| s.bg(c(p.tree_hover)))
                                            .child(icon("idecraft/structure"))
                                            .on_click(cx.listener(
                                                |this, _: &gpui::ClickEvent, _, cx| {
                                                    this.toggle_fragment(cx)
                                                },
                                            )),
                                    ),
                            )
                            // Completions of the expression field (v11 behaviour 22).
                            .when(!completions.is_empty(), |d| {
                                d.child(
                                    div()
                                        .id("debug-completions")
                                        .mx_2()
                                        .mb_1()
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(c(p.separator))
                                        .bg(c(p.popup))
                                        .font(mono.clone())
                                        .children(completions.into_iter().enumerate().map(
                                            |(k, text)| {
                                                div()
                                                    .id(("debug-completion", k))
                                                    .h(px(20.))
                                                    .px_2()
                                                    .flex()
                                                    .items_center()
                                                    .when(k == completion, |d| {
                                                        d.bg(c(p.tree_selection))
                                                    })
                                                    .hover(|s| s.bg(c(p.tree_hover)))
                                                    .child(text)
                                                    .on_click(cx.listener(
                                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                                            this.accept_completion(k, cx);
                                                        },
                                                    ))
                                            },
                                        )),
                                )
                            })
                            .children(self.results.iter().enumerate().map(|(i, (e, r))| {
                                let (value, failed) = match r {
                                    Ok(v) => (v.clone(), false),
                                    Err(why) => (why.clone(), true),
                                };
                                div()
                                    .id(("debug-result", i))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .pl(px(20.))
                                    .pr_2()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(icon("debugger/dbPrimitive"))
                                    .child(div().flex_none().child(e.clone()))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(c(p.ui_dim))
                                            .child(if failed { ": " } else { " = " }),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .font(mono.clone())
                                            .when(failed, |d| {
                                                d.text_color(rgb(p.console_ansi_text[1]))
                                            })
                                            .child(value),
                                    )
                            }))
                            .children(watches.into_iter().enumerate().map(|(i, text)| {
                                div()
                                    .id(("debug-watch", i))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .pl(px(20.))
                                    .pr_2()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .hover(|s| s.bg(c(p.tree_hover)))
                                    .child(icon("debugger/dbPrimitive"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .font(mono.clone())
                                            .child(text),
                                    )
                                    .child(
                                        div()
                                            .id(("debug-watch-remove", i))
                                            .tip(crate::tips::declared!(
                                                OTHER_TOOL_WINDOWS,
                                                "debug-watch-remove"
                                            ))
                                            .flex_none()
                                            .rounded(px(4.))
                                            .hover(|s| s.bg(c(p.tree_selection)))
                                            .child(icon("general/closeSmall"))
                                            .on_click(cx.listener(
                                                move |this, _: &gpui::ClickEvent, _, cx| {
                                                    this.remove_watch(i, cx)
                                                },
                                            )),
                                    )
                            }))
                            .children(rows.into_iter().enumerate().map(|(i, r)| {
                                let reference = r.var.reference;
                                let chevron = if reference == 0 {
                                    None
                                } else if r.expanded {
                                    Some("general/chevronDown")
                                } else {
                                    Some("general/chevronRight")
                                };
                                div()
                                    .id(("debug-var", i))
                                    .h(px(crate::palette::Palette::current().tree_row_height))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .pl(px(crate::metrics::tree_indent(r.depth)))
                                    .pr_2()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .hover(|s| s.bg(c(p.tree_hover)))
                                    .child(match chevron {
                                        Some(key) => icon(key),
                                        None => div().w(px(16.)).flex_none().into_any_element(),
                                    })
                                    // A field of the frame's object at the top (v9 behaviour 14).
                                    .child(icon(if r.depth == 0 && r.parent != 0 {
                                        "nodes/field"
                                    } else if reference == 0 {
                                        "debugger/dbPrimitive"
                                    } else {
                                        "debugger/dbObject"
                                    }))
                                    // A marked object's label before its name (v11 behaviour 24).
                                    .children(self.mark_of(&r.var.value).map(|label| {
                                        let color = self
                                            .mark_color_of(&r.var.value)
                                            .and_then(|k| MARK_COLORS.get(k))
                                            .copied()
                                            .unwrap_or(1);
                                        div()
                                            .flex_none()
                                            .text_color(rgb(p.console_ansi_text[color]))
                                            .child(format!("[{label}]"))
                                    }))
                                    .child(div().flex_none().child(r.var.name.clone()))
                                    .child(div().flex_none().text_color(c(p.ui_dim)).child(" = "))
                                    .child(
                                        div()
                                            .flex_shrink()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .font(mono.clone())
                                            .child(value_text(&r.var.value, c(p.ui_dim))),
                                    )
                                    .children(r.var.kind.clone().map(|k| {
                                        div().flex_none().pl_2().text_color(c(p.ui_dim)).child(k)
                                    }))
                                    .when(selected_var == Some(i), |d| d.bg(c(p.tree_selection)))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.var_selected = Some(i);
                                            this.toggle_var(reference, cx)
                                        },
                                    ))
                                    // Set Value, Copy, Watch, Mark Object (v11 behaviour 23).
                                    .map(|d| {
                                        crate::mouse::on_secondary_click(
                                            d,
                                            cx.listener(
                                                move |this, e: &gpui::MouseDownEvent, _, cx| {
                                                    this.select_variable_row(i, cx);
                                                    cx.emit(DebugViewEvent::Menu(
                                                        DebugMenu::Variable,
                                                        e.position,
                                                    ));
                                                },
                                            ),
                                        )
                                    })
                            })),
                    )
                    .into_any_element()
            }
        };
        div()
            .id("debug-panel")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    // Title and session tab are in the shared header (SPEC-0061 v5 behaviour
                    // 18); SPEC-0194 v3 behaviour 6.
                    .child(self.action_tool(
                        declared!(DEBUG_TOOLBAR, "debug-rerun"),
                        "idecraft/debugRerun",
                        tools.rerun,
                        false,
                        "Rerun",
                        cx,
                    ))
                    .child(self.tool(
                        declared!(DEBUG_TOOLBAR, "debug-resume"),
                        "run/resume",
                        tools.paused,
                        Command::Continue,
                        cx,
                    ))
                    .child(self.tool(
                        declared!(DEBUG_TOOLBAR, "debug-pause"),
                        "idecraft/debugPause",
                        tools.pause,
                        Command::Pause,
                        cx,
                    ))
                    .child(self.tool(
                        declared!(DEBUG_TOOLBAR, "debug-stop"),
                        "run/stop",
                        tools.stop,
                        Command::Stop,
                        cx,
                    ))
                    .child(div().w(px(8.)))
                    .child(self.tool(
                        declared!(DEBUG_TOOLBAR, "debug-step-over"),
                        "run/stepOver",
                        tools.paused,
                        Command::StepOver,
                        cx,
                    ))
                    .child(self.tool(
                        declared!(DEBUG_TOOLBAR, "debug-step-into"),
                        "run/stepInto",
                        tools.paused,
                        Command::StepInto,
                        cx,
                    ))
                    .child(self.tool(
                        declared!(DEBUG_TOOLBAR, "debug-step-out"),
                        "run/stepOut",
                        tools.paused,
                        Command::StepOut,
                        cx,
                    ))
                    .child(self.action_tool(
                        declared!(DEBUG_TOOLBAR, "debug-run-to-cursor"),
                        "idecraft/debugRunToCursor",
                        tools.paused,
                        false,
                        "RunToCursor",
                        cx,
                    ))
                    .child(div().w(px(8.)))
                    .child(self.action_tool(
                        declared!(DEBUG_TOOLBAR, "debug-view-breakpoints"),
                        "idecraft/debugViewBreakpoints",
                        true,
                        false,
                        "ViewBreakpoints",
                        cx,
                    ))
                    .child(self.action_tool(
                        declared!(DEBUG_TOOLBAR, "debug-mute-breakpoints"),
                        "idecraft/debugMuteBreakpoints",
                        true,
                        self.muted,
                        "XDebugger.MuteBreakpoints",
                        cx,
                    ))
                    // The further stepping actions (SPEC-0091 v11 behaviour 21).
                    .child(
                        div()
                            .id("debug-more")
                            .tip(declared!(DEBUG_TOOLBAR, "debug-more"))
                            .p(px(2.))
                            .rounded(px(4.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(icon("general/moreVertical"))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|_, e: &gpui::MouseDownEvent, _, cx| {
                                    cx.emit(DebugViewEvent::Menu(DebugMenu::More, e.position));
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .child(div().flex_1()),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .child(tab(
                        "debug-tab-vars",
                        "Threads & Variables",
                        Tab::Variables,
                        cx,
                    ))
                    .child(tab("debug-tab-console", "Console", Tab::Console, cx))
                    // Inspect tabs with their close button (v12 behaviours 30, 31).
                    .children(self.inspects.iter().enumerate().map(|(k, i)| {
                        let on = self.tab == Tab::Inspect(k);
                        div()
                            .id(("debug-tab-inspect", k))
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py(px(2.))
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| {
                                d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
                            })
                            .child(i.title.clone())
                            .child(
                                div()
                                    .id(("debug-inspect-close", k))
                                    .rounded(px(4.))
                                    .hover(|s| s.bg(c(p.tree_hover)))
                                    .child(icon("general/closeSmall"))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.close_inspect(k, cx);
                                            cx.stop_propagation();
                                        },
                                    )),
                            )
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.tab = Tab::Inspect(k);
                                cx.notify();
                            }))
                    }))
                    .child(div().flex_1())
                    .child(div().text_color(c(p.ui_muted)).child(status)),
            )
            // Hot swap: the changed classes can be reloaded (v12 behaviour 37).
            .when(!self.code_changed.is_empty(), |d| {
                let names: Vec<String> = self
                    .code_changed
                    .iter()
                    .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .collect();
                d.child(
                    div()
                        .id("debug-code-changed")
                        .h(px(30.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .bg(c(p.tree_selection))
                        .child(format!("Code changed: {}", names.join(", ")))
                        .child(
                            crate::metrics::button(
                                "debug-reload-classes",
                                "Reload Changed Classes",
                                true,
                            )
                            .on_click(cx.listener(
                                |this, _: &gpui::ClickEvent, _, cx| {
                                    this.reload_classes(cx);
                                },
                            )),
                        ),
                )
            })
            .child(body)
    }
}

/// A row of the thread list: a group's heading or a thread with what is known of it.
enum ThreadRow {
    Group(String),
    Thread(i64, String, Option<ThreadInfo>),
}

/// A row's value on one line (`SPEC-0091` v14 behaviour 40) with its object id in the dimmed
/// colour `dim` (v16 behaviour 42).
fn value_text(value: &str, dim: gpui::Hsla) -> gpui::StyledText {
    let (text, id) = crate::debug_model::row_value(value);
    gpui::StyledText::new(text).with_highlights(id.map(|range| {
        (
            range,
            gpui::HighlightStyle {
                color: Some(dim),
                ..Default::default()
            },
        )
    }))
}

/// The thread list's rows (`SPEC-0091` v12 behaviour 28): without states the threads as the
/// adapter lists them; with them grouped by thread group, the stopped thread's group first.
fn thread_rows(threads: &[(i64, String)], infos: &[ThreadInfo]) -> Vec<ThreadRow> {
    if infos
        .iter()
        .all(|t| t.group.is_empty() && t.state.is_empty())
    {
        return threads
            .iter()
            .map(|(id, name)| ThreadRow::Thread(*id, name.clone(), None))
            .collect();
    }
    let mut groups: Vec<String> = Vec::new();
    for t in infos.iter().filter(|t| t.suspended) {
        if !groups.contains(&t.group) {
            groups.push(t.group.clone());
        }
    }
    for t in infos {
        if !groups.contains(&t.group) {
            groups.push(t.group.clone());
        }
    }
    let mut out = Vec::new();
    for g in groups {
        out.push(ThreadRow::Group(if g.is_empty() {
            "(no group)".to_owned()
        } else {
            g.clone()
        }));
        for t in infos.iter().filter(|t| t.group == g) {
            out.push(ThreadRow::Thread(t.id, t.name.clone(), Some(t.clone())));
        }
    }
    out
}

/// The icon of a thread's state: the stopped one at its breakpoint, a suspended one paused,
/// waiting, blocked on a monitor, running (`SPEC-0091` v12 behaviour 28).
fn thread_icon(t: &ThreadInfo, stopped: bool) -> &'static str {
    if stopped {
        "breakpoints/breakpoint"
    } else if t.suspended {
        "idecraft/debugPause"
    } else {
        match t.state.as_str() {
            "WAIT" | "SLEEPING" => "idecraft/history",
            "MONITOR" => "status/warning",
            "RUNNING" => "run/run",
            _ => "status/info",
        }
    }
}

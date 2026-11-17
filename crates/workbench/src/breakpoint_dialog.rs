//! The Breakpoints dialog (`SPEC-0127`): every breakpoint of the workspace on the left, grouped by
//! kind; condition, pass count, log message, whether it is enabled and the further options of v3
//! (suspend policy, messages, removal, dependency, filters) for the selected one on the right.
//! Below the line breakpoints: exception, method and field breakpoints (`SPEC-0146`). A line
//! breakpoint's Enabled and Suspend rows are the popup's ([`shared_switches`], `SPEC-0127` v13
//! behaviour 29).

use std::path::PathBuf;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use crate::breakpoint_popup::{PopupState, RowKind};
use crate::debug_model::BreakpointProps;
use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;
use idecraft_debugger::{
    BreakpointRules, EXCEPTION_LANGUAGES, FieldWatchpoint, FunctionBreakpoint, LanguageExceptions,
    SpecialBreakpoints, Suspend,
};

/// One breakpoint as the dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its file.
    pub path: PathBuf,
    /// Line, 0-based.
    pub line: usize,
    /// "relative/path:line" as shown.
    pub label: String,
    /// What it does besides stopping.
    pub props: BreakpointProps,
}

/// What the dialog asks the workbench to do.
pub enum BreakpointDialogEvent {
    /// Give the breakpoint on this line (0-based) of the file these properties.
    Set(PathBuf, usize, BreakpointProps),
    /// Remove the breakpoint.
    Remove(PathBuf, usize),
    /// Close the dialog and show the breakpoint's line.
    Show(PathBuf, usize),
    /// The exception and method breakpoints changed (`SPEC-0146`).
    Special(SpecialBreakpoints),
    /// Close the dialog.
    Close,
}

/// Field labels in Tab order (`SPEC-0127` v3 adds the filters; v10 chooses the dependency from
/// the list instead of a field).
const FIELDS: [&str; 6] = [
    "Condition",
    "Pass count",
    "Log message",
    "Instance filter",
    "Class filter",
    "Caller filter",
];

/// The dependency's label as `set_field` takes it (`SPEC-0127` v10 behaviour 22).
const DEPENDS: &str = "Disable until hitting";

/// The number of exception rows, one per language (`SPEC-0146` v6 behaviour 15).
const LANGUAGES: usize = EXCEPTION_LANGUAGES.len();

/// Under the condition of a Kotlin breakpoint: kotlin-debug-adapter cannot evaluate conditions, pass
/// counts or log messages, IDECraft does it at each hit (`SPEC-0178` behaviour 6).
pub const KOTLIN_HINT: &str =
    "stop only when true, e.g. i == 3 · Kotlin: evaluated by IDECraft at each hit (slower)";

fn kotlin(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e == "kt")
}

/// Labels of the fields of the exception row, of a method row and of a field watchpoint row.
const SPECIAL_FIELDS: [&str; 6] = [
    "Exception types",
    "Method",
    "Method condition",
    "Exception classes",
    "Field",
    "Field condition",
];

/// What the selected row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Line breakpoint `i` of the list.
    Line(usize),
    /// The exception breakpoints of language `l` of [`EXCEPTION_LANGUAGES`] (`SPEC-0146` v6).
    Exceptions(usize),
    /// Method breakpoint `i`.
    Method(usize),
    /// Field watchpoint `i` (`SPEC-0146` v3).
    Field(usize),
}

/// One switch of the form (`SPEC-0127` v12 behaviour 28): a check box, or an option under Suspend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    /// Its test id.
    pub id: SharedString,
    /// Its label.
    pub label: &'static str,
    /// Whether it is on (chosen).
    pub on: bool,
    /// Its mnemonic with ⌥, if it has one.
    pub key: Option<&'static str>,
    /// An option (All, Thread) rather than a check box.
    pub option: bool,
}

impl Switch {
    fn check(id: &'static str, label: &'static str, on: bool) -> Self {
        Self {
            id: id.into(),
            label,
            on,
            key: None,
            option: false,
        }
    }
}

/// The switches the dialog shares with the popup — Enabled, Suspend and, while it is on, All and
/// Thread — made from the popup's rows for the same state, so both views show a breakpoint alike
/// (`SPEC-0127` v13 behaviour 29). Test ids `breakpoint-<row id>`.
pub fn shared_switches(enabled: bool, suspend: Option<Suspend>) -> Vec<Switch> {
    let state = PopupState {
        enabled,
        suspend,
        method: None,
    };
    state
        .rows()
        .into_iter()
        .map(|r| Switch {
            id: format!("breakpoint-{}", r.id).into(),
            label: r.label,
            on: r.on,
            key: r.key,
            option: r.kind == RowKind::Option,
        })
        .collect()
}

/// The Breakpoints dialog (behaviour 2).
pub struct BreakpointDialog {
    list: Vec<Entry>,
    /// Rows: the line breakpoints, then the exception row, then the method breakpoints.
    selected: usize,
    enabled: bool,
    /// The further options of the selected line breakpoint that are switches (`SPEC-0127` v3).
    rules: BreakpointRules,
    /// The breakpoint the selected one waits for, `path:line` with the absolute path
    /// (`SPEC-0127` v10 behaviour 22).
    depends: Option<String>,
    fields: Vec<Entity<EditorView>>,
    special: SpecialBreakpoints,
    special_fields: Vec<Entity<EditorView>>,
    focus: FocusHandle,
}

impl EventEmitter<BreakpointDialogEvent> for BreakpointDialog {}

impl BreakpointDialog {
    /// A dialog over `list`, with the breakpoint at `at` (file, 0-based line) selected if it has one.
    pub fn new(
        list: Vec<Entry>,
        special: SpecialBreakpoints,
        at: Option<(PathBuf, usize)>,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields = FIELDS.iter().map(|_| cx.new(EditorView::field)).collect();
        let special_fields = SPECIAL_FIELDS
            .iter()
            .map(|_| cx.new(EditorView::field))
            .collect();
        // A line breakpoint at `at`, else a method breakpoint on that header line.
        let selected = at
            .and_then(|(path, line)| {
                list.iter()
                    .position(|e| e.path == path && e.line == line)
                    .or_else(|| {
                        special
                            .method_at(&path, line)
                            .map(|i| list.len() + LANGUAGES + i)
                    })
            })
            .unwrap_or(0);
        let mut dialog = Self {
            list,
            selected,
            enabled: true,
            rules: BreakpointRules::default(),
            depends: None,
            fields,
            special,
            special_fields,
            focus: cx.focus_handle(),
        };
        dialog.load(cx);
        dialog
    }

    /// Focus handle of the first field of the selected row.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        match self.row() {
            Row::Line(_) => self.fields[0].read(cx).focus(),
            Row::Exceptions(_) => self.special_fields[0].read(cx).focus(),
            Row::Method(_) => self.special_fields[1].read(cx).focus(),
            Row::Field(_) => self.special_fields[4].read(cx).focus(),
        }
    }

    /// What the selected row is.
    pub fn row(&self) -> Row {
        let n = self.list.len();
        let m = self.special.functions.len();
        match self.selected {
            i if i < n => Row::Line(i),
            i if i < n + LANGUAGES => Row::Exceptions(i - n),
            i if i < n + LANGUAGES + m => Row::Method(i - n - LANGUAGES),
            i => Row::Field(i - n - LANGUAGES - m),
        }
    }

    fn rows(&self) -> usize {
        self.list.len() + LANGUAGES + self.special.functions.len() + self.special.fields.len()
    }

    /// The language key of exception row `l`.
    fn language(l: usize) -> &'static str {
        EXCEPTION_LANGUAGES[l.min(LANGUAGES - 1)].0
    }

    /// The exception settings exception row `l` shows: its language's, else the shared ones.
    pub fn exceptions(&self, l: usize) -> LanguageExceptions {
        self.special.exceptions_of(Self::language(l))
    }

    fn set_exceptions(&mut self, l: usize, e: LanguageExceptions) {
        self.special
            .languages
            .insert(Self::language(l).to_owned(), e);
    }

    /// Selects the exception row of `language` (`java`, `python`, …), for tests.
    pub fn select_language(&mut self, language: &str, cx: &mut Context<Self>) {
        if let Some(l) = EXCEPTION_LANGUAGES.iter().position(|(k, _)| *k == language) {
            self.select(self.list.len() + l, cx);
        }
    }

    /// Switches the selected row on or off at once (⌥E, `SPEC-0127` v10 behaviour 21): a line or
    /// method breakpoint on or off; an exception row off (caught and uncaught) or on (uncaught).
    pub fn toggle_row(&mut self, cx: &mut Context<Self>) {
        match self.row() {
            Row::Line(_) => {
                self.enabled = !self.enabled;
                self.apply(cx);
            }
            Row::Method(i) => {
                self.apply(cx);
                if let Some(f) = self.special.functions.get_mut(i) {
                    f.disabled = !f.disabled;
                    cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
                }
            }
            Row::Exceptions(l) => {
                self.apply(cx);
                let mut e = self.exceptions(l);
                if e.on() {
                    e.caught = false;
                    e.uncaught = false;
                } else {
                    e.uncaught = true;
                }
                self.set_exceptions(l, e);
                cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
            }
            Row::Field(_) => return,
        }
        cx.notify();
    }

    /// The other line breakpoints the selected one can wait for, `path:line` (absolute) and as
    /// the list names them.
    fn depends_choices(&self) -> Vec<(String, String)> {
        let me = self.selected().map(|e| (e.path.clone(), e.line));
        self.list
            .iter()
            .filter(|e| Some((e.path.clone(), e.line)) != me)
            .map(|e| {
                (
                    format!("{}:{}", e.path.display(), e.line + 1),
                    e.label.clone(),
                )
            })
            .collect()
    }

    /// Steps "Disable until hitting" to the next choice: none, then each other line breakpoint,
    /// then none again (⌥D, `SPEC-0127` v10 behaviour 22).
    pub fn next_dependency(&mut self, cx: &mut Context<Self>) {
        let choices = self.depends_choices();
        let at = self
            .depends
            .as_ref()
            .and_then(|d| choices.iter().position(|(v, _)| v == d));
        self.depends = match at {
            None => choices.first().map(|(v, _)| v.clone()),
            Some(i) => choices.get(i + 1).map(|(v, _)| v.clone()),
        };
        cx.notify();
    }

    /// Suspend from the keys (`SPEC-0127` v10 behaviour 20): `s` switches the check box (off: the
    /// breakpoint does not stop; on again: as the debugger does), `a` all threads, `t` this one.
    pub fn suspend_key(&mut self, key: &str, cx: &mut Context<Self>) {
        self.rules.suspend = crate::breakpoint_popup::suspend_after(self.rules.suspend, key);
        cx.notify();
    }

    /// Adds a field watchpoint and selects it, with its field focused (`SPEC-0146` v3).
    pub fn add_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(cx);
        self.special.fields.push(FieldWatchpoint::default());
        self.selected = self.rows() - 1;
        self.load(cx);
        let f = self.special_fields[4].read(cx).focus();
        window.focus(&f);
        cx.notify();
    }

    /// Switches the selected field watchpoint between stopping on writes, reads, or both.
    pub fn cycle_field_access(&mut self, cx: &mut Context<Self>) {
        if let Row::Field(i) = self.row()
            && let Some(f) = self.special.fields.get_mut(i)
        {
            f.access = match f.access_type() {
                "write" => "read".into(),
                "read" => "readWrite".into(),
                _ => String::new(),
            };
            cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
            cx.notify();
        }
    }

    /// The further options of the form that are switches (`SPEC-0127` v3).
    pub fn rules(&self) -> &BreakpointRules {
        &self.rules
    }

    /// The choice of threads as the form shows it: a breakpoint without a choice shows All chosen
    /// while Suspend is on, as the popup does (`SPEC-0127` v12 behaviour 27, OBS-0056 D1) — by
    /// the popup's own rule (v13 behaviour 29).
    pub fn shown_suspend(&self) -> Option<Suspend> {
        PopupState {
            enabled: self.enabled,
            suspend: self.rules.suspend,
            method: None,
        }
        .shown_suspend()
    }

    /// The form's switches as a vertical list in the popup's order (`SPEC-0127` v12 behaviour 28,
    /// OBS-0056 D2): Enabled, Suspend with All and Thread under it while it is on — the popup's
    /// rows (v13 behaviour 29) — then the two log switches and Remove once hit; each with its test
    /// id, whether it is on and its mnemonic.
    pub fn switches(&self) -> Vec<Switch> {
        let mut out = shared_switches(self.enabled, self.rules.suspend);
        out.push(Switch::check(
            "breakpoint-log-hit",
            "Log \"breakpoint hit\" message",
            self.rules.log_hit,
        ));
        out.push(Switch::check(
            "breakpoint-log-stack",
            "Log stack trace",
            self.rules.log_stack,
        ));
        out.push(Switch::check(
            "breakpoint-remove-once",
            "Remove once hit",
            self.rules.remove_once_hit,
        ));
        out
    }

    /// What a click on the switch `id` does.
    fn click_switch(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "breakpoint-enabled" => self.set_enabled(!self.enabled, cx),
            "breakpoint-suspend" => self.suspend_key("s", cx),
            "breakpoint-suspend-all" => self.suspend_key("a", cx),
            "breakpoint-suspend-thread" => self.suspend_key("t", cx),
            "breakpoint-log-hit" => self.toggle_rule("log_hit", cx),
            "breakpoint-log-stack" => self.toggle_rule("log_stack", cx),
            "breakpoint-remove-once" => self.toggle_rule("remove_once_hit", cx),
            _ => {}
        }
    }

    /// Switches one of the further options of the selected line breakpoint (applied with the
    /// rest): `suspend` cycles adapter default, all threads, this thread, none; `log_hit`,
    /// `log_stack`, `remove_once_hit` and `depends_keep` switch.
    pub fn toggle_rule(&mut self, which: &str, cx: &mut Context<Self>) {
        let r = &mut self.rules;
        match which {
            "suspend" => {
                r.suspend = match r.suspend {
                    None => Some(Suspend::All),
                    Some(Suspend::All) => Some(Suspend::Thread),
                    Some(Suspend::Thread) => Some(Suspend::None),
                    Some(Suspend::None) => None,
                }
            }
            "log_hit" => r.log_hit = !r.log_hit,
            "log_stack" => r.log_stack = !r.log_stack,
            "remove_once_hit" => r.remove_once_hit = !r.remove_once_hit,
            "depends_keep" => r.depends_keep = !r.depends_keep,
            _ => return,
        }
        cx.notify();
    }

    /// The exception and method breakpoints as the dialog has them.
    pub fn special(&self) -> &SpecialBreakpoints {
        &self.special
    }

    /// Switches stopping on caught (every thrown) or uncaught exceptions of the selected
    /// language's row.
    pub fn toggle_exceptions(&mut self, uncaught: bool, cx: &mut Context<Self>) {
        let Row::Exceptions(l) = self.row() else {
            return;
        };
        let mut e = self.exceptions(l);
        if uncaught {
            e.uncaught = !e.uncaught;
        } else {
            e.caught = !e.caught;
        }
        self.set_exceptions(l, e);
        cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
        cx.notify();
    }

    /// Adds a method breakpoint and selects it, with its name field focused (⌘N).
    pub fn add_method(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(cx);
        self.special.functions.push(FunctionBreakpoint::default());
        self.selected = self.rows() - 1;
        self.load(cx);
        let f = self.special_fields[1].read(cx).focus();
        window.focus(&f);
        cx.notify();
    }

    /// Sets the text of special field `name` (one of its labels), for tests.
    pub fn set_special_field(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(i) = SPECIAL_FIELDS.iter().position(|f| *f == name) {
            self.special_fields[i].update(cx, |f, cx| f.set_text(text, cx));
        }
    }

    /// The breakpoints listed.
    pub fn list(&self) -> &[Entry] {
        &self.list
    }

    /// The selected line breakpoint.
    pub fn selected(&self) -> Option<&Entry> {
        match self.row() {
            Row::Line(i) => self.list.get(i),
            _ => None,
        }
    }

    /// Replaces the list (the workbench's breakpoints changed); the selection stays on its
    /// breakpoint when it still exists. Fields being edited are kept.
    pub fn set_list(&mut self, list: Vec<Entry>, cx: &mut Context<Self>) {
        let keep = self.selected().map(|e| (e.path.clone(), e.line));
        let same = keep
            .as_ref()
            .and_then(|(p, l)| list.iter().position(|e| &e.path == p && e.line == *l));
        let row = self.row();
        let old_len = self.list.len();
        self.list = list;
        match (same, row) {
            (Some(i), _) => self.selected = i,
            // A special row keeps its place below the (now longer or shorter) line list.
            (None, Row::Exceptions(_) | Row::Method(_) | Row::Field(_)) => {
                self.selected = self.selected - old_len + self.list.len();
            }
            (None, Row::Line(_)) => {
                self.selected = self.selected.min(self.list.len().saturating_sub(1));
                self.load(cx);
            }
        }
        cx.notify();
    }

    /// Sets the text of field `name` (one of the labels), for tests.
    pub fn set_field(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        if name == DEPENDS {
            let t = text.trim();
            self.depends = (!t.is_empty()).then(|| self.depends_path(t));
            return;
        }
        if let Some(i) = FIELDS.iter().position(|f| *f == name) {
            self.fields[i].update(cx, |f, cx| f.set_text(text, cx));
        }
    }

    /// Switches the selected breakpoint on or off in the form (applied with the rest).
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.enabled = enabled;
        cx.notify();
    }

    /// The form's properties: empty fields are left out.
    pub fn props(&self, cx: &gpui::App) -> BreakpointProps {
        let text = |i: usize| {
            let t = self.fields[i].read(cx).state.buffer.text();
            let t = t.trim();
            (!t.is_empty()).then(|| t.to_owned())
        };
        BreakpointProps {
            condition: text(0),
            hit_condition: text(1),
            log_message: text(2),
            disabled: !self.enabled,
            rules: BreakpointRules {
                depends_on: self.depends.clone(),
                instance_filter: text(3),
                class_filter: text(4),
                caller_filter: text(5),
                ..self.rules.clone()
            },
        }
    }

    /// The breakpoint `path:line` a dependency names, written as listed (`src/A.java:12`, a path
    /// relative to the workspace) or with an absolute path, made absolute from the listed ones.
    fn depends_path(&self, text: &str) -> String {
        let Some((path, line)) = idecraft_debugger::breakpoint_place(text) else {
            return text.to_owned();
        };
        let listed = self.list.iter().find(|e| {
            e.line + 1 == usize::try_from(line).unwrap_or(0)
                && (e.path == path || e.label.starts_with(&format!("{}:", path.display())))
        });
        match listed {
            Some(e) => format!("{}:{line}", e.path.display()),
            None => text.to_owned(),
        }
    }

    /// The text of a dependency for its field: as the list shows the breakpoint.
    fn depends_label(&self, text: &str) -> String {
        let Some((path, line)) = idecraft_debugger::breakpoint_place(text) else {
            return text.to_owned();
        };
        self.list
            .iter()
            .find(|e| e.path == path && e.line + 1 == usize::try_from(line).unwrap_or(0))
            .map_or_else(|| text.to_owned(), |e| e.label.clone())
    }

    /// Fills the form from the selected breakpoint.
    fn load(&mut self, cx: &mut Context<Self>) {
        let (types, classes) = match self.row() {
            Row::Exceptions(l) => {
                let e = self.exceptions(l);
                (e.exception_condition, e.exception_classes)
            }
            _ => Default::default(),
        };
        let (name, condition) = match self.row() {
            Row::Method(i) => self
                .special
                .functions
                .get(i)
                .map(|f| (f.name.clone(), f.condition.clone().unwrap_or_default()))
                .unwrap_or_default(),
            _ => Default::default(),
        };
        let (field_name, field_condition) = match self.row() {
            Row::Field(i) => self
                .special
                .fields
                .get(i)
                .map(|f| (f.field.clone(), f.condition.clone().unwrap_or_default()))
                .unwrap_or_default(),
            _ => Default::default(),
        };
        for (field, text) in self.special_fields.iter().zip([
            types,
            name,
            condition,
            classes,
            field_name,
            field_condition,
        ]) {
            field.update(cx, |f, cx| f.set_text(&text, cx));
        }
        let props = self.selected().map(|e| e.props.clone()).unwrap_or_default();
        self.depends = props
            .rules
            .depends_on
            .as_deref()
            .map(|d| self.depends_path(&self.depends_label(d)));
        let texts = [
            props.condition.unwrap_or_default(),
            props.hit_condition.unwrap_or_default(),
            props.log_message.unwrap_or_default(),
            props.rules.instance_filter.clone().unwrap_or_default(),
            props.rules.class_filter.clone().unwrap_or_default(),
            props.rules.caller_filter.clone().unwrap_or_default(),
        ];
        for (field, text) in self.fields.iter().zip(texts) {
            field.update(cx, |f, cx| f.set_text(&text, cx));
        }
        self.enabled = !props.disabled;
        self.rules = props.rules;
    }

    /// Applies the form to the selected breakpoint if it changed (behaviour 3).
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        let text = |f: &Entity<EditorView>, cx: &gpui::App| {
            f.read(cx).state.buffer.text().trim().to_owned()
        };
        match self.row() {
            Row::Line(i) => {
                let form = self.props(cx);
                if let Some(e) = self.list.get_mut(i) {
                    // A breakpoint without a choice of threads gets All, which the form showed
                    // chosen, once anything else changes (v12 behaviour 27) — the popup's rule
                    // (v13 behaviour 29).
                    let props = crate::breakpoint_popup::settle_suspend(&e.props, form);
                    if e.props != props {
                        e.props = props.clone();
                        cx.emit(BreakpointDialogEvent::Set(e.path.clone(), e.line, props));
                    }
                }
            }
            Row::Exceptions(l) => {
                let types = text(&self.special_fields[0], cx);
                let classes = text(&self.special_fields[3], cx);
                let mut e = self.exceptions(l);
                if types != e.exception_condition || classes != e.exception_classes {
                    e.exception_condition = types;
                    e.exception_classes = classes;
                    self.set_exceptions(l, e);
                    cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
                }
            }
            Row::Field(i) => {
                let field = text(&self.special_fields[4], cx);
                let condition = text(&self.special_fields[5], cx);
                let access = self
                    .special
                    .fields
                    .get(i)
                    .map(|f| f.access.clone())
                    .unwrap_or_default();
                let new = FieldWatchpoint {
                    field,
                    access,
                    condition: (!condition.is_empty()).then_some(condition),
                };
                if self.special.fields.get(i) != Some(&new) {
                    if new.field.is_empty() {
                        self.special.fields.remove(i);
                        self.selected = self.selected.min(self.rows() - 1);
                    } else {
                        self.special.fields[i] = new;
                    }
                    cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
                }
            }
            Row::Method(i) => {
                let name = text(&self.special_fields[1], cx);
                let condition = text(&self.special_fields[2], cx);
                // Entry, exit, on or off and the header's place stay (SPEC-0146 v6).
                let new = FunctionBreakpoint {
                    name,
                    condition: (!condition.is_empty()).then_some(condition),
                    ..self.special.functions.get(i).cloned().unwrap_or_default()
                };
                if self.special.functions.get(i) != Some(&new) {
                    if new.name.is_empty() {
                        // A method breakpoint without a name is no breakpoint.
                        self.special.functions.remove(i);
                        self.selected = self.selected.min(self.rows() - 1);
                    } else {
                        self.special.functions[i] = new;
                    }
                    cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
                }
            }
        }
    }

    /// Applies the form and selects breakpoint `i`.
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.rows() || i == self.selected {
            return;
        }
        self.apply(cx);
        self.selected = i;
        self.load(cx);
        cx.notify();
    }

    /// Removes the selected breakpoint (behaviour 4).
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        match self.row() {
            Row::Line(i) => {
                let e = self.list.remove(i);
                cx.emit(BreakpointDialogEvent::Remove(e.path, e.line));
                self.selected = self.selected.min(self.list.len().saturating_sub(1));
            }
            Row::Exceptions(l) => {
                self.set_exceptions(l, LanguageExceptions::default());
                cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
            }
            Row::Field(i) => {
                self.special.fields.remove(i);
                self.selected = self.selected.min(self.rows() - 1);
                cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
            }
            Row::Method(i) => {
                self.special.functions.remove(i);
                self.selected = self.selected.min(self.rows() - 1);
                cx.emit(BreakpointDialogEvent::Special(self.special.clone()));
            }
        }
        self.load(cx);
        cx.notify();
    }

    /// Applies the form and closes (Done, Enter).
    pub fn done(&mut self, cx: &mut Context<Self>) {
        self.apply(cx);
        cx.emit(BreakpointDialogEvent::Close);
    }

    /// Up and Down choose the breakpoint, Tab moves between the fields, Enter applies and closes,
    /// F4 shows the line, Escape closes without applying; taken before the fields see them.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let n = self.rows();
        match k.key.as_str() {
            "down" => {
                self.select((self.selected + 1) % n, cx);
                let f = self.focus(cx);
                window.focus(&f);
            }
            "up" => {
                self.select((self.selected + n - 1) % n, cx);
                let f = self.focus(cx);
                window.focus(&f);
            }
            "n" if (k.modifiers.platform || k.modifiers.control) && k.modifiers.shift => {
                self.add_field(window, cx)
            }
            "n" if k.modifiers.platform || k.modifiers.control => self.add_method(window, cx),
            "c" if k.modifiers.alt && matches!(self.row(), Row::Exceptions(_)) => {
                self.toggle_exceptions(false, cx)
            }
            "u" if k.modifiers.alt && matches!(self.row(), Row::Exceptions(_)) => {
                self.toggle_exceptions(true, cx)
            }
            // SPEC-0127 v10 behaviours 20–22.
            "e" if k.modifiers.alt => self.toggle_row(cx),
            "s" | "a" | "t" if k.modifiers.alt && matches!(self.row(), Row::Line(_)) => {
                self.suspend_key(k.key.as_str(), cx)
            }
            "d" if k.modifiers.alt && matches!(self.row(), Row::Line(_)) => {
                self.next_dependency(cx)
            }
            "k" if k.modifiers.alt && matches!(self.row(), Row::Line(_)) => {
                self.toggle_rule("depends_keep", cx)
            }
            "tab" => {
                let fields: Vec<&Entity<EditorView>> = match self.row() {
                    Row::Line(_) => self.fields.iter().collect(),
                    Row::Exceptions(_) => vec![&self.special_fields[0], &self.special_fields[3]],
                    Row::Method(_) => self.special_fields[1..3].iter().collect(),
                    Row::Field(_) => self.special_fields[4..6].iter().collect(),
                };
                let at = fields
                    .iter()
                    .position(|f| f.read(cx).focus().is_focused(window))
                    .unwrap_or(0);
                let m = fields.len();
                let next = if k.modifiers.shift {
                    (at + m - 1) % m
                } else {
                    (at + 1) % m
                };
                window.focus(&fields[next].read(cx).focus());
            }
            "enter" => self.done(cx),
            "f4" => {
                self.apply(cx);
                if let Some(e) = self.selected() {
                    cx.emit(BreakpointDialogEvent::Show(e.path.clone(), e.line));
                }
            }
            "escape" => cx.emit(BreakpointDialogEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// The heading of a language's exception breakpoints (`SPEC-0146` v6 behaviour 15).
pub fn exception_heading(language: &str) -> String {
    format!("{language} Exception Breakpoints")
}

/// The headings of the exception groups, in the list's order.
pub fn exception_headings() -> Vec<String> {
    EXCEPTION_LANGUAGES
        .iter()
        .map(|(_, name)| exception_heading(name))
        .collect()
}

/// A list row's texts for the label `dir/File.java:12`: `File.java:12` and `dir` (SPEC-0127 v4).
pub fn row_label(label: &str) -> (String, String) {
    match label.rsplit_once('/') {
        Some((dir, name)) => (name.to_owned(), dir.to_owned()),
        None => (label.to_owned(), String::new()),
    }
}

impl Render for BreakpointDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let button =
            |id: &'static str, label: String, on: bool| crate::metrics::button(id, label, on);
        let rows = self.list.iter().enumerate().map(|(i, e)| {
            let look = e.props.look();
            let mark = match look {
                crate::debug_model::BreakpointLook::Plain => "●",
                crate::debug_model::BreakpointLook::Conditional => "◉",
                crate::debug_model::BreakpointLook::Log => "◆",
                crate::debug_model::BreakpointLook::Disabled
                | crate::debug_model::BreakpointLook::MethodDisabled => "○",
                crate::debug_model::BreakpointLook::Method => "■",
            };
            // Red like the gutter's mark while enabled (SPEC-0127 v4); the file and line first,
            // the folder after it, so a narrow list never cuts the line number off.
            let mark_color = if look == crate::debug_model::BreakpointLook::Disabled {
                p.ui_muted
            } else {
                p.error_text
            };
            let (name, folder) = row_label(&e.label);
            let on = look != crate::debug_model::BreakpointLook::Disabled;
            div()
                .id(("breakpoint-row", i))
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(4.))
                .when(i == selected, |d| d.bg(c(p.tree_selection)))
                .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                // Enabled, switched with ⌥E (SPEC-0127 v10 behaviour 21).
                .child(div().w(px(14.)).child(if on { "☑" } else { "☐" }))
                .child(div().w(px(14.)).text_color(c(mark_color)).child(mark))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .gap_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(div().flex_none().child(name))
                        .child(div().text_color(c(p.ui_muted)).child(folder)),
                )
                .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                    this.select(i, cx);
                    let f = this.focus(cx);
                    window.focus(&f);
                    if ev.click_count() == 2 {
                        this.apply(cx);
                        if let Some(e) = this.selected() {
                            cx.emit(BreakpointDialogEvent::Show(e.path.clone(), e.line));
                        }
                    }
                }))
        });
        let mut hints = [
            "stop only when true, e.g. i == 3",
            "stop on this hit only, e.g. 5",
            "log instead of stopping; {expr} is evaluated",
            "only for these objects: ids as values show them, e.g. 33",
            "only in these classes: dbg.*, -dbg.Main$Item excludes",
            "only when called from: Main.main, -Main.test excludes",
        ];
        if self.selected().is_some_and(|e| kotlin(&e.path)) {
            hints[0] = KOTLIN_HINT;
        }
        let fields = FIELDS.iter().enumerate().map(|(i, label)| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_color(c(p.ui_muted)).child(*label))
                .child(crate::metrics::field_frame().child(self.fields[i].clone()))
                .child(div().text_xs().text_color(c(p.ui_dim)).child(hints[i]))
        });
        let has = matches!(self.row(), Row::Line(_));
        let n = self.list.len();
        let special = &self.special;
        let exceptions_label = |e: &LanguageExceptions| match (e.caught, e.uncaught) {
            (false, false) => "Any exception: off".to_owned(),
            (true, false) => "Any exception: caught".to_owned(),
            (false, true) => "Any exception: uncaught".to_owned(),
            (true, true) => "Any exception: caught and uncaught".to_owned(),
        };
        let special_row = |i: usize, on: Option<bool>, mark: &'static str, label: String| {
            div()
                .id(("breakpoint-row", i))
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(4.))
                .when(i == selected, |d| d.bg(c(p.tree_selection)))
                .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(div().w(px(14.)).child(match on {
                    Some(true) => "☑",
                    Some(false) => "☐",
                    None => "",
                }))
                .child(div().w(px(14.)).text_color(c(p.ui_muted)).child(mark))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(label),
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    this.select(i, cx);
                    let f = this.focus(cx);
                    window.focus(&f);
                }))
        };
        // Groups by kind (SPEC-0127 v3 behaviour 12).
        let heading = |text: String| {
            div()
                .h(px(22.))
                .px_2()
                .flex()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(c(p.ui_dim))
                .child(text)
        };
        let m = special.functions.len();
        // One group per language (SPEC-0146 v6 behaviour 15).
        let mut special_rows = Vec::new();
        for (l, (key, name)) in EXCEPTION_LANGUAGES.iter().enumerate() {
            let e = special.exceptions_of(key);
            special_rows.push(heading(exception_heading(name)).into_any_element());
            special_rows.push(
                special_row(n + l, Some(e.on()), "⚡", exceptions_label(&e)).into_any_element(),
            );
        }
        if m > 0 {
            special_rows.push(heading("Method Breakpoints".into()).into_any_element());
        }
        special_rows.extend(special.functions.iter().enumerate().map(|(i, f)| {
            let name = if f.name.is_empty() {
                "(new method breakpoint)".to_owned()
            } else {
                f.name.clone()
            };
            special_row(n + LANGUAGES + i, Some(!f.disabled), "ƒ", name).into_any_element()
        }));
        if !special.fields.is_empty() {
            special_rows.push(heading("Field Watchpoints".into()).into_any_element());
        }
        special_rows.extend(special.fields.iter().enumerate().map(|(i, f)| {
            let name = if f.field.is_empty() {
                "(new field watchpoint)".to_owned()
            } else {
                format!("{} ({})", f.field, f.access_type())
            };
            special_row(n + LANGUAGES + m + i, None, "◇", name).into_any_element()
        }));
        // The switches as a vertical list of check boxes, All and Thread as options under Suspend
        // (SPEC-0127 v12 behaviour 28).
        let switch_rows: Vec<gpui::AnyElement> = self
            .switches()
            .into_iter()
            .map(|sw| {
                let mark = match (sw.option, sw.on) {
                    (true, true) => "◉",
                    (true, false) => "○",
                    (false, true) => "☑",
                    (false, false) => "☐",
                };
                let id = sw.id.clone();
                let role = if sw.option {
                    Role::RadioButton
                } else {
                    Role::CheckBox
                };
                div()
                    .id(sw.id.clone())
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(sw.option, |d| d.ml(px(22.)))
                    .cursor_pointer()
                    .child(mark)
                    .child(sw.label)
                    .children(sw.key.map(|k| {
                        div()
                            .text_color(c(p.ui_dim))
                            .child(keys(&format!("⌥{}", k.to_uppercase())))
                    }))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.click_switch(&id, cx)
                    }))
                    .semantics(
                        Semantics::new(role)
                            .name(sw.label)
                            .test_id(sw.id.to_string())
                            .checked(sw.on)
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    )
                    .into_any_element()
            })
            .collect();
        let depends_label = self
            .depends
            .as_deref()
            .map_or_else(|| "None".to_owned(), |d| self.depends_label(d));
        let rules = self.rules.clone();
        let special_field = |i: usize, hint: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_color(c(p.ui_muted)).child(SPECIAL_FIELDS[i]))
                .child(crate::metrics::field_frame().child(self.special_fields[i].clone()))
                .child(div().text_xs().text_color(c(p.ui_dim)).child(hint))
        };
        let check = |on: bool| if on { "☑" } else { "☐" };
        let row = self.row();
        let shown = match row {
            Row::Exceptions(l) => self.exceptions(l),
            _ => LanguageExceptions::default(),
        };
        div()
            .id("breakpoint-dialog")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(760.))
            .h(px(560.))
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .child(
                div()
                    .h(px(36.))
                    .px_3()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Breakpoints"),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("breakpoint-list")
                            .w(px(260.))
                            .py_2()
                            .px_1()
                            .border_r_1()
                            .border_color(c(p.separator))
                            .overflow_y_scroll()
                            .when(n == 0, |d| {
                                d.child(
                                    div()
                                        .px_2()
                                        .text_color(c(p.ui_dim))
                                        .child("No line breakpoints. Click a line number to add one."),
                                )
                            })
                            .child(heading("Line Breakpoints".into()))
                            .children(rows)
                            .child(div().h(px(8.)))
                            .children(special_rows),
                    )
                    .child(
                        div()
                            .id("breakpoint-form")
                            .flex_1()
                            .min_w_0()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .overflow_y_scroll()
                            .when(has, |d| {
                                // Which threads stop, the messages and removal (SPEC-0127 v3
                                // behaviours 6–10), one under the other.
                                d.child(div().flex().flex_col().gap_1().children(switch_rows))
                                .children(fields)
                                // Chosen from the other breakpoints (SPEC-0127 v10 behaviour 22).
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(div().text_color(c(p.ui_muted)).child(DEPENDS))
                                        .child(
                                            button(
                                                "breakpoint-depends",
                                                format!("{depends_label} ({})", keys("⌥D")),
                                                false,
                                            )
                                            .on_click(cx.listener(
                                                |this, _: &gpui::ClickEvent, _, cx| {
                                                    this.next_dependency(cx)
                                                },
                                            ))
                                            .semantics(
                                                Semantics::new(Role::ComboBox)
                                                    .name(DEPENDS)
                                                    .value(depends_label.clone())
                                                    .test_id("breakpoint-depends")
                                                    .action(AxAction::Click)
                                                    .effects(&[Effect::None]),
                                            ),
                                        )
                                        .child(
                                            button(
                                                "breakpoint-depends-keep",
                                                format!(
                                                    "After hit: {} ({})",
                                                    if rules.depends_keep {
                                                        "Leave enabled"
                                                    } else {
                                                        "Disable again"
                                                    },
                                                    keys("⌥K")
                                                ),
                                                false,
                                            )
                                            .on_click(cx.listener(
                                                |this, _: &gpui::ClickEvent, _, cx| {
                                                    this.toggle_rule("depends_keep", cx)
                                                },
                                            ))
                                            .semantics(
                                                Semantics::new(Role::Button)
                                                    .name("After hit")
                                                    .value(if rules.depends_keep {
                                                        "Leave enabled"
                                                    } else {
                                                        "Disable again"
                                                    })
                                                    .test_id("breakpoint-depends-keep")
                                                    .action(AxAction::Click)
                                                    .effects(&[Effect::None]),
                                            ),
                                        ),
                                )
                            })
                            .when(matches!(row, Row::Exceptions(_)), |d| {
                                d.child(
                                    button(
                                        "breakpoint-caught",
                                        format!("{} Caught exceptions ({})", check(shown.caught), keys("⌥C")),
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &gpui::ClickEvent, _, cx| {
                                            this.toggle_exceptions(false, cx)
                                        },
                                    )),
                                )
                                .child(
                                    button(
                                        "breakpoint-uncaught",
                                        format!(
                                            "{} Uncaught exceptions ({})",
                                            check(shown.uncaught),
                                            keys("⌥U")
                                        ),
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &gpui::ClickEvent, _, cx| {
                                            this.toggle_exceptions(true, cx)
                                        },
                                    )),
                                )
                                .child(special_field(
                                    0,
                                    "empty: any exception; else types, e.g. java.lang.NullPointerException",
                                ))
                                .child(special_field(
                                    3,
                                    "only where thrown in these classes (Java): dbg.*, -dbg.Test excludes",
                                ))
                            })
                            // Field watchpoints (SPEC-0146 v3).
                            .when(matches!(row, Row::Field(_)), |d| {
                                let access = match self.row() {
                                    Row::Field(i) => special
                                        .fields
                                        .get(i)
                                        .map(|f| f.access_type().to_owned())
                                        .unwrap_or_default(),
                                    _ => String::new(),
                                };
                                d.child(special_field(
                                    4,
                                    "the field, e.g. dbg.Main.Counter.n or dbg.Main$Counter#n",
                                ))
                                .child(
                                    button(
                                        "breakpoint-field-access",
                                        format!("Stop on: {access}"),
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &gpui::ClickEvent, _, cx| {
                                            this.cycle_field_access(cx)
                                        },
                                    )),
                                )
                                .child(special_field(5, "stop only when true"))
                            })
                            .when(matches!(row, Row::Method(_)), |d| {
                                d.child(special_field(
                                    1,
                                    "function or method, e.g. main, Shapes::area, com.acme.Shop.total",
                                ))
                                .child(special_field(2, "stop only when true"))
                            }),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .gap_2()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .when(has, |d| {
                        d.child(
                            button("breakpoint-remove", "Remove".into(), false).on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.remove_selected(cx)
                                }),
                            ),
                        )
                        .child(
                            button("breakpoint-show", "Go to Source (F4)".into(), false).on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.apply(cx);
                                    if let Some(e) = this.selected() {
                                        cx.emit(BreakpointDialogEvent::Show(
                                            e.path.clone(),
                                            e.line,
                                        ));
                                    }
                                }),
                            ),
                        )
                    })
                    .when(!has, |d| {
                        d.child(
                            button("breakpoint-remove", "Remove".into(), false).on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.remove_selected(cx)
                                }),
                            ),
                        )
                    })
                    .child(
                        button("breakpoint-add-method", keys("+ Method Breakpoint (⌘N)"), false)
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.add_method(window, cx)
                            })),
                    )
                    .child(
                        button("breakpoint-add-field", keys("+ Field Watchpoint (⇧⌘N)"), false)
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.add_field(window, cx)
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        button("breakpoint-done", "Done".into(), true).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.done(cx)),
                        ),
                    ),
            )
    }
}

/// The dialog's own keys in the notation of the system the window runs on (`SPEC-0127` v14
/// behaviour 30).
fn keys(text: &str) -> String {
    idecraft_ui::keymap::local_keys(text, cfg!(target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use crate::breakpoint_popup::{PopupState, RowKind};
    use idecraft_debugger::Suspend;

    /// SPEC-0127 v13 AK-18: the switches the dialog shares with the popup are the popup's rows for
    /// every state; without a choice All is shown chosen, with Suspend off neither All nor Thread.
    #[test]
    fn shared_switches_are_the_popups_rows() {
        let states = [
            None,
            Some(Suspend::All),
            Some(Suspend::Thread),
            Some(Suspend::None),
        ];
        for enabled in [true, false] {
            for suspend in states {
                let rows = PopupState {
                    enabled,
                    suspend,
                    method: None,
                }
                .rows();
                let switches = super::shared_switches(enabled, suspend);
                assert_eq!(rows.len(), switches.len(), "{suspend:?}");
                for (r, s) in rows.iter().zip(&switches) {
                    assert_eq!(
                        (r.label, r.on, r.key, r.kind == RowKind::Option),
                        (s.label, s.on, s.key, s.option)
                    );
                    assert_eq!(s.id.as_ref(), format!("breakpoint-{}", r.id));
                }
            }
        }
        let on = |s: Option<Suspend>| -> Vec<String> {
            super::shared_switches(true, s)
                .into_iter()
                .filter(|s| s.on)
                .map(|s| s.id.to_string())
                .collect()
        };
        assert_eq!(
            on(None),
            [
                "breakpoint-enabled",
                "breakpoint-suspend",
                "breakpoint-suspend-all"
            ]
        );
        assert_eq!(
            on(Some(Suspend::Thread)),
            [
                "breakpoint-enabled",
                "breakpoint-suspend",
                "breakpoint-suspend-thread"
            ]
        );
        assert_eq!(on(Some(Suspend::None)), ["breakpoint-enabled"]);
        assert_eq!(super::shared_switches(true, Some(Suspend::None)).len(), 2);
    }

    // SPEC-0127 v4: the file and line first, the folder after it.
    #[test]
    fn rows_name_the_file_first() {
        assert_eq!(
            super::row_label("java/src/dbg/Probe.java:22"),
            ("Probe.java:22".to_owned(), "java/src/dbg".to_owned())
        );
        assert_eq!(
            super::row_label("app.py:3"),
            ("app.py:3".to_owned(), String::new())
        );
    }
}

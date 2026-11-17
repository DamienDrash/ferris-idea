//! The small popup that edits one breakpoint beside its line (`SPEC-0127` v10 behaviour 19):
//! Enabled, Suspend with All or Thread, the condition, for a method breakpoint its entry and exit
//! (`SPEC-0146` v6 behaviour 13), "More" for the Breakpoints dialog and Done. Since v11
//! (behaviours 23–26) it is headed by the kind of breakpoint, shows check boxes and options, All
//! chosen for a breakpoint without a choice, the Watch section, and a condition field that
//! expands and remembers the conditions used last. Since v13 (behaviour 29) the Breakpoints
//! dialog shows a line breakpoint's Enabled and Suspend rows from [`PopupState::rows`] and saves
//! by [`settle_suspend`], so both views show one state; the rows are in the semantic tree.

use std::path::PathBuf;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use crate::debug_model::BreakpointProps;
use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;
use idecraft_debugger::{FunctionBreakpoint, Suspend};

/// Suspend after one of its keys (`SPEC-0127` v10 behaviour 20): `s` switches the check box (off:
/// the breakpoint does not stop; on again: as the debugger does), `a` all threads, `t` this
/// thread; other keys change nothing.
pub fn suspend_after(suspend: Option<Suspend>, key: &str) -> Option<Suspend> {
    match key {
        "s" if suspend == Some(Suspend::None) => None,
        "s" => Some(Suspend::None),
        "a" => Some(Suspend::All),
        "t" => Some(Suspend::Thread),
        _ => suspend,
    }
}

/// What a row of the popup is (`SPEC-0127` v11 behaviour 23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A check box.
    Check,
    /// An option button of a group.
    Option,
    /// A section's heading.
    Heading,
}

/// One row of the popup: its kind, label, whether it is on and its mnemonic (⌥ and the letter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// Check box, option or heading.
    pub kind: RowKind,
    /// Its name in test ids: `enabled`, `suspend`, `suspend-all`, `suspend-thread`, `watch`,
    /// `entry`, `exit`. The popup's test id is `breakpoint-popup-<id>`, the dialog's
    /// `breakpoint-<id>` (`SPEC-0127` v13 behaviour 29).
    pub id: &'static str,
    /// The label.
    pub label: &'static str,
    /// Checked or chosen.
    pub on: bool,
    /// The letter of its mnemonic.
    pub key: Option<&'static str>,
}

fn row(kind: RowKind, id: &'static str, label: &'static str, on: bool, key: &'static str) -> Row {
    Row {
        kind,
        id,
        label,
        on,
        key: Some(key),
    }
}

/// A line breakpoint's properties after an edit in the popup or the dialog, `old` before it
/// (`SPEC-0127` v11 behaviour 24, v12 behaviour 27, v13 behaviour 29): one without a choice of
/// threads stays so while nothing changed; with any change it gets All, which both views showed
/// chosen.
pub fn settle_suspend(old: &BreakpointProps, mut new: BreakpointProps) -> BreakpointProps {
    if &new != old && new.rules.suspend.is_none() {
        new.rules.suspend = Some(Suspend::All);
    }
    new
}

/// The heading and the dimmed place of the popup for `target` (behaviour 23).
pub fn title_of(target: &Target) -> (&'static str, String) {
    match target {
        Target::Line(path, line, _) => (
            "Line breakpoint",
            format!(
                "{}:{}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                line + 1
            ),
        ),
        Target::Method(_, f) => ("Method breakpoint", f.name.clone()),
    }
}

/// The most conditions the popup remembers (behaviour 26).
pub const RECENT_CONDITIONS: usize = 20;

/// The conditions applied last, the most recent first, each once (behaviour 26); kept while
/// IDECraft runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionHistory(pub Vec<String>);

impl gpui::Global for ConditionHistory {}

impl ConditionHistory {
    /// Notes `condition` as the most recent one.
    pub fn note(&mut self, condition: &str) {
        let condition = condition.trim();
        if condition.is_empty() {
            return;
        }
        self.0.retain(|c| c != condition);
        self.0.insert(0, condition.to_owned());
        self.0.truncate(RECENT_CONDITIONS);
    }
}

/// Where ↑/↓ are in the history (behaviour 26): `None` at the text being typed, `Some(i)` at the
/// `i`-th recent condition. `older` is ↑. Returns the new place.
pub fn step_history(at: Option<usize>, len: usize, older: bool) -> Option<usize> {
    match (at, older) {
        (_, true) if len == 0 => None,
        (None, true) => Some(0),
        (Some(i), true) => Some((i + 1).min(len - 1)),
        (None, false) | (Some(0), false) => None,
        (Some(i), false) => Some(i - 1),
    }
}

/// The switches of the popup, apart from the condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupState {
    /// The breakpoint is on.
    pub enabled: bool,
    /// Which threads stop; `None` as the debugger does (line breakpoints).
    pub suspend: Option<Suspend>,
    /// For a method breakpoint: watches the entry and the exit.
    pub method: Option<(bool, bool)>,
}

impl PopupState {
    /// The popup's state for a line breakpoint.
    pub fn of_line(props: &BreakpointProps) -> Self {
        Self {
            enabled: !props.disabled,
            suspend: props.rules.suspend,
            method: None,
        }
    }

    /// The popup's state for a method breakpoint.
    pub fn of_method(f: &FunctionBreakpoint) -> Self {
        Self {
            enabled: !f.disabled,
            suspend: None,
            method: Some((f.entry, f.exit)),
        }
    }

    /// The choice of threads as the popup shows it: a line breakpoint without a choice shows All
    /// chosen while Suspend is on (behaviour 24).
    pub fn shown_suspend(&self) -> Option<Suspend> {
        match self.suspend {
            None => Some(Suspend::All),
            other => other,
        }
    }

    /// The rows of the popup in their order, the condition field standing after Suspend
    /// (behaviour 23); a method breakpoint's entry and exit under the heading Watch (behaviour 25).
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = vec![row(RowKind::Check, "enabled", "Enabled", self.enabled, "e")];
        match self.method {
            None => {
                let shown = self.shown_suspend();
                let on = shown != Some(Suspend::None);
                rows.push(row(RowKind::Check, "suspend", "Suspend", on, "s"));
                if on {
                    rows.push(row(
                        RowKind::Option,
                        "suspend-all",
                        "All",
                        shown == Some(Suspend::All),
                        "a",
                    ));
                    rows.push(row(
                        RowKind::Option,
                        "suspend-thread",
                        "Thread",
                        shown == Some(Suspend::Thread),
                        "t",
                    ));
                }
            }
            Some((entry, exit)) => {
                rows.push(Row {
                    kind: RowKind::Heading,
                    id: "watch",
                    label: "Watch",
                    on: false,
                    key: None,
                });
                rows.push(row(RowKind::Check, "entry", "Method entry", entry, "n"));
                rows.push(row(RowKind::Check, "exit", "Method exit", exit, "x"));
            }
        }
        rows
    }

    /// One mnemonic (⌥ plus the letter): `e` Enabled, `s`/`a`/`t` Suspend of a line breakpoint,
    /// `n`/`x` entry and exit of a method breakpoint, never both off. Whether it was the popup's.
    pub fn key(&mut self, key: &str) -> bool {
        match (key, self.method) {
            ("e", _) => self.enabled = !self.enabled,
            ("s" | "a" | "t", None) => self.suspend = suspend_after(self.suspend, key),
            ("n", Some((entry, exit))) if !entry || exit => self.method = Some((!entry, exit)),
            ("x", Some((entry, exit))) if entry || !exit => self.method = Some((entry, !exit)),
            ("n" | "x", Some(_)) => {}
            _ => return false,
        }
        true
    }

    /// A line breakpoint's properties with these switches and `condition`. A breakpoint without
    /// a choice of threads stays so while nothing else changes; with another change it gets All,
    /// which the popup showed chosen (behaviour 24, [`settle_suspend`]).
    pub fn line_props(
        &self,
        props: &BreakpointProps,
        condition: Option<String>,
    ) -> BreakpointProps {
        let mut p = props.clone();
        p.condition = condition;
        p.disabled = !self.enabled;
        p.rules.suspend = self.suspend;
        settle_suspend(props, p)
    }

    /// A method breakpoint with these switches and `condition`.
    pub fn method(&self, f: &FunctionBreakpoint, condition: Option<String>) -> FunctionBreakpoint {
        let (entry, exit) = self.method.unwrap_or((f.entry, f.exit));
        FunctionBreakpoint {
            condition,
            disabled: !self.enabled,
            entry,
            exit,
            ..f.clone()
        }
    }
}

/// The breakpoint the popup edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The line breakpoint on this 0-based line of the file, with its properties.
    Line(PathBuf, usize, BreakpointProps),
    /// Method breakpoint `i` of the saved ones.
    Method(usize, FunctionBreakpoint),
}

/// What the popup asks the workbench to do.
pub enum BreakpointPopupEvent {
    /// Give the line breakpoint these properties.
    SetLine(PathBuf, usize, BreakpointProps),
    /// Replace method breakpoint `i`.
    SetMethod(usize, FunctionBreakpoint),
    /// Close and open the Breakpoints dialog with the breakpoint on this file and 0-based line.
    More(Option<(PathBuf, usize)>),
    /// Close.
    Close,
}

/// The popup (behaviour 19).
pub struct BreakpointPopup {
    target: Target,
    state: PopupState,
    condition: Entity<EditorView>,
    focus: FocusHandle,
    /// The condition field shows five lines (behaviour 26).
    expanded: bool,
    /// The recent conditions when the popup opened, and where ↑/↓ are in them.
    history: Vec<String>,
    history_at: Option<usize>,
    /// The text typed before ↑ went into the history.
    typed: String,
}

impl EventEmitter<BreakpointPopupEvent> for BreakpointPopup {}

impl BreakpointPopup {
    /// A popup for `target`, the condition field holding its condition.
    pub fn new(target: Target, cx: &mut Context<Self>) -> Self {
        let (state, condition) = match &target {
            Target::Line(_, _, props) => (PopupState::of_line(props), props.condition.clone()),
            Target::Method(_, f) => (PopupState::of_method(f), f.condition.clone()),
        };
        let field = cx.new(EditorView::field);
        let text = condition.unwrap_or_default();
        field.update(cx, |f, cx| f.set_text(&text, cx));
        let history = cx
            .try_global::<ConditionHistory>()
            .map(|h| h.0.clone())
            .unwrap_or_default();
        Self {
            target,
            state,
            condition: field,
            focus: cx.focus_handle(),
            expanded: false,
            history,
            history_at: None,
            typed: text,
        }
    }

    /// Whether the condition field is expanded.
    pub fn expanded(&self) -> bool {
        self.expanded
    }

    /// The condition field's text.
    pub fn condition(&self, cx: &gpui::App) -> String {
        self.condition.read(cx).state.buffer.text()
    }

    /// ↑ (`older`) or ↓ in the collapsed condition field: the next recent condition, or back to
    /// the text being typed (behaviour 26).
    fn step(&mut self, older: bool, cx: &mut Context<Self>) {
        if self.history_at.is_none() {
            self.typed = self.condition(cx);
        }
        let at = step_history(self.history_at, self.history.len(), older);
        if at == self.history_at {
            return;
        }
        self.history_at = at;
        let text = match at {
            Some(i) => self.history[i].clone(),
            None => self.typed.clone(),
        };
        self.condition.update(cx, |f, cx| f.set_text(&text, cx));
    }

    /// The condition field's focus, where the popup opens.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.condition.read(cx).focus()
    }

    /// The switches as they are now.
    pub fn state(&self) -> PopupState {
        self.state
    }

    /// The breakpoint edited.
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The file and 0-based line of the breakpoint, where the popup shows.
    pub fn place(&self) -> Option<(PathBuf, usize)> {
        match &self.target {
            Target::Line(path, line, _) => Some((path.clone(), *line)),
            Target::Method(_, f) => Some((f.file.clone()?, f.line?)),
        }
    }

    fn condition_text(&self, cx: &gpui::App) -> Option<String> {
        let t = self.condition.read(cx).state.buffer.text();
        let t = t.trim();
        (!t.is_empty()).then(|| t.to_owned())
    }

    /// Hands a changed breakpoint to the workbench; its condition becomes the most recent one.
    fn apply(&mut self, cx: &mut Context<Self>) {
        let condition = self.condition_text(cx);
        if let Some(c) = &condition {
            let mut history = cx
                .try_global::<ConditionHistory>()
                .cloned()
                .unwrap_or_default();
            history.note(c);
            cx.set_global(history);
        }
        match &self.target {
            Target::Line(path, line, props) => {
                let new = self.state.line_props(props, condition);
                if &new != props {
                    cx.emit(BreakpointPopupEvent::SetLine(path.clone(), *line, new));
                }
            }
            Target::Method(i, f) => {
                let new = self.state.method(f, condition);
                if &new != f {
                    cx.emit(BreakpointPopupEvent::SetMethod(*i, new));
                }
            }
        }
    }

    /// Applies and closes (Done, Enter).
    pub fn done(&mut self, cx: &mut Context<Self>) {
        self.apply(cx);
        cx.emit(BreakpointPopupEvent::Close);
    }

    /// Applies and opens the Breakpoints dialog (More, ⇧⌘F8).
    pub fn more(&mut self, cx: &mut Context<Self>) {
        self.apply(cx);
        cx.emit(BreakpointPopupEvent::More(self.place()));
    }

    /// A mnemonic from a click.
    fn press(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.state.key(key) {
            cx.notify();
        }
    }

    /// ⌥ and a letter switch, Enter is Done, ⇧⌘F8 is More, Escape closes without applying;
    /// taken before the condition field sees them.
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let command = m.platform || m.control;
        match k.key.as_str() {
            // ⇧↩ expands and collapses the condition field; expanded, ↩ starts a new line and
            // ⌘↩ is Done (behaviour 26).
            "enter" if m.shift && !m.alt && !command => self.expanded = !self.expanded,
            "enter" if self.expanded && !command => return,
            "enter" if !m.alt && !m.shift => self.done(cx),
            "up" | "down" if !self.expanded && !m.alt && !m.shift && !command => {
                self.step(k.key == "up", cx)
            }
            "escape" => cx.emit(BreakpointPopupEvent::Close),
            "f8" if m.shift && (m.platform || m.control) => self.more(cx),
            key if m.alt && !m.platform && !m.control && self.state.key(key) => {}
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for BreakpointPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let (kind, place) = title_of(&self.target);
        let rows = self.state.rows();
        // A check box or an option with its label and, dimmed, its mnemonic (behaviour 23).
        let line = |r: Row, cx: &mut Context<Self>| -> gpui::AnyElement {
            let Some(key) = r.key else {
                return div()
                    .pt_1()
                    .text_color(c(p.ui_muted))
                    .child(r.label)
                    .into_any_element();
            };
            let mark = match (r.kind, r.on) {
                (RowKind::Option, true) => "◉",
                (RowKind::Option, false) => "○",
                (_, true) => "☑",
                (_, false) => "☐",
            };
            let role = if r.kind == RowKind::Option {
                Role::RadioButton
            } else {
                Role::CheckBox
            };
            let id = format!("breakpoint-popup-{}", r.id);
            div()
                .id(SharedString::from(id.clone()))
                .flex()
                .items_center()
                .gap_2()
                .when(r.kind == RowKind::Option, |d| d.pl(px(22.)))
                .cursor_pointer()
                .child(mark)
                .child(r.label)
                .child(
                    div()
                        .text_color(c(p.ui_dim))
                        .child(keys(&format!("⌥{}", key.to_uppercase()))),
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.press(key, cx)))
                // Role, name and state in the semantic tree, as the dialog's switches have them
                // (`SPEC-0127` v13 behaviour 29).
                .semantics(
                    Semantics::new(role)
                        .name(r.label)
                        .test_id(id)
                        .checked(r.on)
                        .action(AxAction::Click)
                        .effects(&[Effect::None]),
                )
                .into_any_element()
        };
        let watch_at = rows.iter().position(|r| r.kind == RowKind::Heading);
        let (before, after) = rows.split_at(watch_at.unwrap_or(rows.len()));
        let lines = if self.expanded { 5. } else { 1. };
        let field_height = lines * crate::fonts::line_height() + 8.;
        div()
            .id("breakpoint-popup")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(340.))
            .p_3()
            .flex()
            .flex_col()
            .gap_1p5()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(kind))
                    .child(
                        div()
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(place),
                    ),
            )
            .children(before.iter().map(|r| line(*r, cx)).collect::<Vec<_>>())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .text_color(c(p.ui_muted))
                            .child("Condition")
                            .child(div().flex_1())
                            .child(div().text_xs().text_color(c(p.ui_dim)).child(keys(
                                if self.expanded {
                                    "⇧↩ collapse · ⌘↩ done"
                                } else {
                                    "⇧↩ expand · ↑↓ recent"
                                },
                            ))),
                    )
                    .child(
                        crate::metrics::field_frame()
                            .h(px(field_height))
                            .child(self.condition.clone()),
                    ),
            )
            .children(after.iter().map(|r| line(*r, cx)).collect::<Vec<_>>())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("breakpoint-popup-more")
                            .cursor_pointer()
                            .text_color(c(p.link))
                            .child(more_label(
                                &crate::keymaps::current(),
                                cfg!(target_os = "macos"),
                            ))
                            .on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.more(cx)),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        crate::metrics::button("breakpoint-popup-done", "Done", true).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.done(cx)),
                        ),
                    ),
            )
    }
}

/// The popup's own keys in the notation of the system the window runs on (`SPEC-0127` v14
/// behaviour 30).
fn keys(text: &str) -> String {
    idecraft_ui::keymap::local_keys(text, cfg!(target_os = "macos"))
}

/// The button that opens the dialog, with the first shortcut of View Breakpoints in `keymap`
/// written for macOS when `macos` (`SPEC-0127` v14 behaviour 30): "More (⇧⌘F8)",
/// "More (Ctrl+Shift+F8)", or "More" without a shortcut.
pub fn more_label(keymap: &idecraft_ui::keymap::Keymap, macos: bool) -> String {
    match keymap.shortcut_label("ViewBreakpoints", macos) {
        k if k.is_empty() => "More".to_owned(),
        k => format!("More ({k})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0127 v14 AK-19 (PR #866): the keys per platform.
    #[test]
    fn keys_follow_the_platform() {
        let mac = idecraft_ui::keymap::Keymap::bundled_macos();
        let default = idecraft_ui::keymap::Keymap::bundled_default();
        assert_eq!(more_label(&mac, true), "More (⇧⌘F8)");
        assert_eq!(more_label(&default, false), "More (Ctrl+Shift+F8)");
        let empty = idecraft_ui::keymap::KeymapFile::user("empty", "none");
        let none = idecraft_ui::keymap::Keymap::resolve(&empty, None, false);
        assert_eq!(more_label(&none, false), "More");
        let hint = "⇧↩ collapse · ⌘↩ done";
        assert_eq!(idecraft_ui::keymap::local_keys(hint, true), hint);
        assert_eq!(
            idecraft_ui::keymap::local_keys(hint, false),
            "Shift+Enter collapse · Ctrl+Enter done"
        );
        assert_eq!(idecraft_ui::keymap::local_keys("⌥S", false), "Alt+S");
    }

    /// SPEC-0127 v10 behaviours 19 and 20: Enabled, Suspend with All or Thread.
    #[test]
    fn line_switches() {
        let props = BreakpointProps {
            condition: Some("old".into()),
            ..BreakpointProps::default()
        };
        let mut s = PopupState::of_line(&props);
        assert!(s.enabled && s.suspend.is_none() && s.method.is_none());
        assert!(s.key("s"));
        assert_eq!(s.suspend, Some(Suspend::None), "off: does not stop");
        assert!(s.key("s"));
        assert_eq!(s.suspend, None, "on again: as the debugger does");
        assert!(s.key("t"));
        assert_eq!(s.suspend, Some(Suspend::Thread));
        assert!(s.key("a"));
        assert_eq!(s.suspend, Some(Suspend::All));
        assert!(s.key("e"));
        assert!(!s.key("n"), "no entry for a line breakpoint");
        let new = s.line_props(&props, Some("n > 2".into()));
        assert_eq!(new.condition.as_deref(), Some("n > 2"));
        assert!(new.disabled);
        assert_eq!(new.rules.suspend, Some(Suspend::All));
    }

    /// SPEC-0146 v6 behaviour 13: entry and exit, never both off; no Suspend for a method.
    #[test]
    fn method_switches() {
        let f = FunctionBreakpoint {
            name: "shop.Cart.total".into(),
            line: Some(4),
            ..FunctionBreakpoint::default()
        };
        let mut s = PopupState::of_method(&f);
        assert_eq!(s.method, Some((true, false)));
        assert!(s.key("n"), "the popup's key");
        assert_eq!(s.method, Some((true, false)), "entry stays: exit is off");
        assert!(s.key("x"));
        assert!(s.key("n"));
        assert_eq!(s.method, Some((false, true)));
        assert!(s.key("x"));
        assert_eq!(s.method, Some((false, true)), "exit stays: entry is off");
        assert!(!s.key("s"), "no Suspend for a method breakpoint");
        let new = s.method(&f, None);
        assert!(!new.entry && new.exit && new.line == Some(4) && new.name == f.name);
    }

    /// SPEC-0127 v11 AK-16: the heading is the kind, the place is dimmed under it.
    #[test]
    fn titles_and_places() {
        let line = Target::Line(
            PathBuf::from("/w/Inventory.java"),
            7,
            BreakpointProps::default(),
        );
        assert_eq!(
            title_of(&line),
            ("Line breakpoint", "Inventory.java:8".to_owned())
        );
        let f = FunctionBreakpoint {
            name: "lab.inventory.Inventory.main".into(),
            ..FunctionBreakpoint::default()
        };
        assert_eq!(
            title_of(&Target::Method(0, f)),
            (
                "Method breakpoint",
                "lab.inventory.Inventory.main".to_owned()
            )
        );
    }

    /// SPEC-0127 v11 AK-16: check boxes and options with their keys; Watch for a method.
    #[test]
    fn rows_are_checks_and_options() {
        let s = PopupState::of_line(&BreakpointProps::default());
        let rows: Vec<_> = s
            .rows()
            .into_iter()
            .map(|r| (r.kind, r.label, r.on, r.key))
            .collect();
        assert_eq!(
            rows,
            [
                (RowKind::Check, "Enabled", true, Some("e")),
                (RowKind::Check, "Suspend", true, Some("s")),
                (RowKind::Option, "All", true, Some("a")),
                (RowKind::Option, "Thread", false, Some("t")),
            ]
        );
        let mut off = s;
        off.key("s");
        assert_eq!(off.rows().len(), 2, "no All/Thread while Suspend is off");
        let m = PopupState::of_method(&FunctionBreakpoint::default());
        let labels: Vec<_> = m.rows().into_iter().map(|r| (r.kind, r.label)).collect();
        assert_eq!(
            labels,
            [
                (RowKind::Check, "Enabled"),
                (RowKind::Heading, "Watch"),
                (RowKind::Check, "Method entry"),
                (RowKind::Check, "Method exit"),
            ]
        );
    }

    /// SPEC-0127 v11 AK-16: All shown chosen; untouched, nothing changes; with a change All is
    /// saved; ⌥T still chooses Thread.
    #[test]
    fn all_is_shown_chosen() {
        let props = BreakpointProps::default();
        let s = PopupState::of_line(&props);
        assert_eq!(s.shown_suspend(), Some(Suspend::All));
        assert_eq!(s.line_props(&props, None), props, "nothing changed");
        let with = s.line_props(&props, Some("n > 2".into()));
        assert_eq!(with.rules.suspend, Some(Suspend::All));
        let mut t = s;
        t.key("t");
        assert_eq!(
            t.line_props(&props, None).rules.suspend,
            Some(Suspend::Thread)
        );
        let mut off = s;
        off.key("s");
        assert_eq!(
            off.line_props(&props, None).rules.suspend,
            Some(Suspend::None)
        );
    }

    /// SPEC-0127 v13 AK-18: the rule both views save by — nothing changed, no choice is added;
    /// any change adds All; a choice made stays; the rows' test ids.
    #[test]
    fn all_is_saved_only_with_a_change() {
        let old = BreakpointProps::default();
        assert_eq!(settle_suspend(&old, old.clone()), old);
        let mut changed = old.clone();
        changed.rules.log_hit = true;
        assert_eq!(
            settle_suspend(&old, changed).rules.suspend,
            Some(Suspend::All)
        );
        let mut thread = old.clone();
        thread.rules.suspend = Some(Suspend::Thread);
        assert_eq!(
            settle_suspend(&old, thread).rules.suspend,
            Some(Suspend::Thread)
        );
        let mut off = old.clone();
        off.rules.suspend = Some(Suspend::None);
        assert_eq!(settle_suspend(&old, off).rules.suspend, Some(Suspend::None));
        let ids: Vec<_> = PopupState::of_line(&old)
            .rows()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["enabled", "suspend", "suspend-all", "suspend-thread"]);
        let ids: Vec<_> = PopupState::of_method(&FunctionBreakpoint::default())
            .rows()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["enabled", "watch", "entry", "exit"]);
    }

    /// SPEC-0127 v11 AK-16: the most recent first, each once, at most 20; ↑/↓ step and come
    /// back to the typed text.
    #[test]
    fn recent_conditions() {
        let mut h = ConditionHistory::default();
        h.note("a > 1");
        h.note("b");
        h.note(" a > 1 ");
        h.note("  ");
        assert_eq!(h.0, ["a > 1", "b"]);
        for i in 0..30 {
            h.note(&format!("c{i}"));
        }
        assert_eq!(h.0.len(), RECENT_CONDITIONS);
        assert_eq!(h.0[0], "c29");
        assert_eq!(step_history(None, 2, true), Some(0));
        assert_eq!(step_history(Some(0), 2, true), Some(1));
        assert_eq!(
            step_history(Some(1), 2, true),
            Some(1),
            "stays at the oldest"
        );
        assert_eq!(step_history(Some(1), 2, false), Some(0));
        assert_eq!(
            step_history(Some(0), 2, false),
            None,
            "back to the typed text"
        );
        assert_eq!(step_history(None, 0, true), None);
    }

    #[test]
    fn suspend_keys() {
        assert_eq!(
            suspend_after(Some(Suspend::Thread), "s"),
            Some(Suspend::None)
        );
        assert_eq!(suspend_after(Some(Suspend::None), "a"), Some(Suspend::All));
        assert_eq!(suspend_after(None, "q"), None);
    }
}

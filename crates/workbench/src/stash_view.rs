//! The stash popup (`SPEC-0106`): stash the local changes, or apply, pop or drop a stash.

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use idecraft_vcs::Stash;

use crate::palette::Palette;
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashEvent {
    /// Stash the local changes; `true` includes untracked files.
    Push(bool),
    /// Apply stash `index`; `true` drops it afterwards (pop).
    Apply(usize, bool),
    /// Delete stash `index`.
    Drop(usize),
    /// Show the changes of stash `index` (⌘D, `SPEC-0145`).
    Show(usize),
    /// Close the popup.
    Close,
}

/// The two stash commands followed by the stashes (behaviour 7).
pub struct StashView {
    stashes: Vec<Stash>,
    /// 0 and 1 are the commands, `i + 2` is stash `i`.
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<StashEvent> for StashView {}

/// Rows before the stashes.
const COMMANDS: [&str; 2] = ["Stash Changes", "Stash Changes with Untracked Files"];

/// The row selected when the popup opens: the newest stash, or the first command without
/// stashes (`SPEC-0106` v4 behaviour 15).
fn first_selection(stashes: usize) -> usize {
    if stashes == 0 { 0 } else { COMMANDS.len() }
}

#[cfg(test)]
mod tests {
    use super::first_selection;

    // SPEC-0106 v4 AK-11: Unstash opens on the newest stash.
    #[test]
    fn unstash_opens_on_the_newest_stash() {
        assert_eq!(first_selection(0), 0);
        assert_eq!(first_selection(3), 2);
    }
}

impl StashView {
    /// A popup listing `stashes`, newest first.
    /// The newest stash is selected when there is one, so Enter applies it right away, as the
    /// reference selects it when Unstash opens (SPEC-0106 v4 behaviour 15; before, Enter made a new stash).
    pub fn new(stashes: Vec<Stash>, cx: &mut Context<Self>) -> Self {
        Self {
            selected: first_selection(stashes.len()),
            stashes,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the popup.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The rows as shown: the commands, then each stash's message.
    pub fn rows(&self) -> Vec<String> {
        COMMANDS
            .iter()
            .map(|c| (*c).to_owned())
            .chain(self.stashes.iter().map(|s| s.message.clone()))
            .collect()
    }

    /// Selects row `i`.
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = i.min(COMMANDS.len() + self.stashes.len() - 1);
        cx.notify();
    }

    /// Enter on a command stashes; on a stash it applies it, with `pop` it pops it.
    pub fn choose(&mut self, pop: bool, cx: &mut Context<Self>) {
        match self.selected.checked_sub(COMMANDS.len()) {
            None => cx.emit(StashEvent::Push(self.selected == 1)),
            Some(i) => cx.emit(StashEvent::Apply(i, pop)),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let n = COMMANDS.len() + self.stashes.len();
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, n) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match k.key.as_str() {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose(k.modifiers.shift, cx),
            "d" if k.modifiers.platform || k.modifiers.control => {
                if let Some(i) = self.selected.checked_sub(COMMANDS.len()) {
                    cx.emit(StashEvent::Show(i));
                }
            }
            "delete" | "backspace" => {
                if let Some(i) = self.selected.checked_sub(COMMANDS.len()) {
                    cx.emit(StashEvent::Drop(i));
                }
            }
            "escape" => cx.emit(StashEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for StashView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let row = |i: usize, text: String| {
            div()
                .id(("stash-row", i))
                .h(px(26.))
                .mx_1()
                .px_2()
                .flex()
                .items_center()
                .rounded(px(4.))
                .overflow_hidden()
                .whitespace_nowrap()
                .when(i == selected, |d| d.bg(c(p.tree_selection)))
                .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(text)
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.selected = i;
                    this.choose(false, cx);
                }))
        };
        div()
            .id("stash-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(460.))
            .py_1()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .children(
                COMMANDS
                    .iter()
                    .enumerate()
                    .map(|(i, t)| row(i, (*t).to_owned())),
            )
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(if self.stashes.is_empty() {
                        "No stashes"
                    } else {
                        "Stashes · Enter applies, Shift+Enter pops, ⌘D shows changes, Delete drops"
                    }),
            )
            .children(
                self.stashes
                    .iter()
                    .enumerate()
                    .map(|(k, s)| row(k + COMMANDS.len(), s.message.clone())),
            )
    }
}

/// What the stash dialog asks the workbench to do (`SPEC-0106` v3 behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashDialogEvent {
    /// Stash the local changes with these options.
    Stash(idecraft_vcs::StashOptions),
    /// Close without stashing.
    Close,
}

/// Stash Changes: a message with the earlier messages on ↑/↓, "Keep index" (⌥K) and "Include
/// untracked files" (⌥U) (`SPEC-0106` v3 behaviour 14, OBS-0022 L14).
pub struct StashDialog {
    message: gpui::Entity<crate::EditorView>,
    branch: String,
    history: Vec<String>,
    /// Position in `history` while browsing it with ↑/↓.
    at: Option<usize>,
    keep_index: bool,
    include_untracked: bool,
}

impl EventEmitter<StashDialogEvent> for StashDialog {}

impl StashDialog {
    /// The dialog for the current `branch`, with earlier messages, newest first.
    pub fn new(branch: String, history: Vec<String>, cx: &mut Context<Self>) -> Self {
        Self {
            message: cx.new(crate::EditorView::field),
            branch,
            history,
            at: None,
            keep_index: false,
            include_untracked: false,
        }
    }

    /// Focus handle of the message field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.message.read(cx).focus()
    }

    /// The message field (edit actions of the menu bar go here, OBS-0022 L23).
    pub fn field(&self) -> gpui::Entity<crate::EditorView> {
        self.message.clone()
    }

    /// The options as chosen.
    pub fn options(&self, cx: &gpui::App) -> idecraft_vcs::StashOptions {
        let text = self.message.read(cx).state.buffer.text().trim().to_owned();
        idecraft_vcs::StashOptions {
            message: (!text.is_empty()).then_some(text),
            include_untracked: self.include_untracked,
            keep_index: self.keep_index,
        }
    }

    /// Sets the message (self-test).
    pub fn set_message(&mut self, text: &str, cx: &mut Context<Self>) {
        self.message.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Switches "Keep index" (⌥K).
    pub fn toggle_keep_index(&mut self, cx: &mut Context<Self>) {
        self.keep_index = !self.keep_index;
        cx.notify();
    }

    /// Switches "Include untracked files" (⌥U).
    pub fn toggle_untracked(&mut self, cx: &mut Context<Self>) {
        self.include_untracked = !self.include_untracked;
        cx.notify();
    }

    /// ↑ goes to an older message, ↓ to a newer one and finally back to an empty field.
    pub fn browse_history(&mut self, older: bool, cx: &mut Context<Self>) {
        if self.history.is_empty() {
            return;
        }
        self.at = match (self.at, older) {
            (None, true) => Some(0),
            (Some(i), true) => Some((i + 1).min(self.history.len() - 1)),
            (Some(0), false) | (None, false) => None,
            (Some(i), false) => Some(i - 1),
        };
        let text = self.at.map(|i| self.history[i].clone()).unwrap_or_default();
        self.message.update(cx, |f, cx| f.set_text(&text, cx));
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let alt = k.modifiers.alt && !k.modifiers.platform && !k.modifiers.control;
        match k.key.as_str() {
            "enter" if !k.modifiers.shift => cx.emit(StashDialogEvent::Stash(self.options(cx))),
            "escape" => cx.emit(StashDialogEvent::Close),
            "up" => self.browse_history(true, cx),
            "down" => self.browse_history(false, cx),
            "k" if alt => self.toggle_keep_index(cx),
            "u" if alt => self.toggle_untracked(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for StashDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let check = |on: bool| if on { "☑" } else { "☐" };
        let empty = self.message.read(cx).state.buffer.is_empty();
        div()
            .id("stash-dialog")
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(480.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("Stash Changes on {}", self.branch)),
            )
            .child(
                crate::metrics::field_frame().child(
                    div()
                        .relative()
                        .flex_1()
                        .child(self.message.clone())
                        .when(empty, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(px(2.))
                                    .text_color(c(p.ui_dim))
                                    .child("Message (↑/↓ earlier messages)"),
                            )
                        }),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(
                        div()
                            .id("stash-keep-index")
                            .child(format!("{} Keep index (⌥K)", check(self.keep_index)))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_keep_index(cx)
                            })),
                    )
                    .child(
                        div()
                            .id("stash-untracked")
                            .child(format!(
                                "{} Include untracked files (⌥U)",
                                check(self.include_untracked)
                            ))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_untracked(cx)
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Enter stashes, Escape cancels. Unstash Changes lists the stashes."),
            )
    }
}

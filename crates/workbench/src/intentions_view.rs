//! The context-actions popup (`SPEC-0076` v3): the language server's fixes and refactorings at the
//! caret; typing filters, Enter applies, the `QuickJavaDoc` key previews the selected action.

use std::ops::Range;

use ferris_idea_languages::actions::CodeAction;
use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, uniform_list,
};

use crate::icon_images::icon;
use crate::intentions_model::{filter, first_enabled, is_fix, order};
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum IntentionsEvent {
    /// Apply the action with this title.
    Apply(String),
    /// Plan the action with this title for the preview (behaviour 7).
    Preview(String),
    /// Close the popup.
    Close,
    /// Run the option (its label) of the submenu of the row titled first (`SPEC-0125` v2
    /// behaviour 8).
    RunOption(String, String),
}

/// The submenu of one row: the options of an own finding (`SPEC-0125` v2 behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowOptions {
    /// Title of the row.
    pub title: String,
    /// Labels of the options, in order.
    pub labels: Vec<String>,
    /// The row has no action of its own: Enter opens the submenu.
    pub only_options: bool,
}

/// The preview's lines, or why there are none.
type Preview = Result<Vec<String>, String>;

/// Context actions at one place.
pub struct IntentionsView {
    actions: Vec<CodeAction>,
    query: String,
    shown: Vec<usize>,
    selected: usize,
    focus: FocusHandle,
    /// Whether the preview is shown, and the action it shows with its lines or why there are none
    /// (`None` while planning).
    preview: Option<(String, Option<Preview>)>,
    /// Submenus of rows (`SPEC-0125` v2 behaviour 8).
    options: Vec<RowOptions>,
    /// The open submenu: shown row and selected option.
    submenu: Option<(usize, usize)>,
    /// The popup's left edge in window coordinates, set by the workbench where it places the popup;
    /// decides the submenu's side (`SPEC-0076` v13 behaviour 19).
    left: f32,
}

/// Width of the list.
const LIST_WIDTH: f32 = 420.;
/// Space between the list and a row's submenu.
const SUBMENU_GAP: f32 = 4.;

/// Width of a row's submenu.
const SUBMENU_WIDTH: f32 = 380.;

impl EventEmitter<IntentionsEvent> for IntentionsView {}

impl IntentionsView {
    /// A popup listing `actions` in the popup's order (behaviour 2).
    pub fn new(actions: Vec<CodeAction>, cx: &mut Context<Self>) -> Self {
        let actions = order(actions);
        let shown: Vec<usize> = (0..actions.len()).collect();
        let selected = first_enabled(&actions, &shown).unwrap_or(0);
        Self {
            actions,
            query: String::new(),
            shown,
            selected,
            focus: cx.focus_handle(),
            preview: None,
            options: Vec::new(),
            submenu: None,
            left: 0.,
        }
    }

    /// Gives rows their submenus (`SPEC-0125` v2 behaviour 8).
    pub fn with_options(mut self, options: Vec<RowOptions>) -> Self {
        self.options = options;
        self
    }

    /// Where the workbench put the popup's left edge, in window coordinates.
    pub fn set_left(&mut self, x: f32) {
        self.left = x;
    }

    /// The submenu of `row`; none when it would hold no option, so no key or click ever opens an
    /// empty submenu (`SPEC-0163` v3 behaviour 3, OBS-0117 G7).
    fn options_of(&self, row: usize) -> Option<&RowOptions> {
        let title = &self.actions[*self.shown.get(row)?].title;
        self.options
            .iter()
            .find(|o| &o.title == title && !o.labels.is_empty())
    }

    /// The labels of the open submenu, if any (tests).
    pub fn submenu_labels(&self) -> Option<Vec<String>> {
        let (row, _) = self.submenu?;
        self.options_of(row).map(|o| o.labels.clone())
    }

    /// Opens the submenu of the selected row, as → would (tests).
    pub fn open_submenu(&mut self, cx: &mut Context<Self>) {
        if self.options_of(self.selected).is_some() {
            self.submenu = Some((self.selected, 0));
            cx.notify();
        }
    }

    /// Runs the option labelled `label` of the open submenu (tests).
    pub fn choose_option(&mut self, label: &str, cx: &mut Context<Self>) {
        let Some((row, _)) = self.submenu else {
            return;
        };
        if let Some(o) = self.options_of(row)
            && o.labels.iter().any(|l| l == label)
        {
            cx.emit(IntentionsEvent::RunOption(
                o.title.clone(),
                label.to_owned(),
            ));
        }
    }

    /// Focus handle of the list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Titles of the rows shown, in order.
    pub fn titles(&self) -> Vec<String> {
        self.shown
            .iter()
            .map(|&i| self.actions[i].title.clone())
            .collect()
    }

    /// Applies the row titled `title`, as Enter on it would (tests).
    pub fn choose_title(&mut self, title: &str, cx: &mut Context<Self>) {
        if let Some(at) = self
            .shown
            .iter()
            .position(|&i| self.actions[i].title == title)
        {
            self.selected = at;
            self.choose(cx);
        }
    }

    /// Selects the row titled `title` (tests); the preview follows.
    pub fn select_title(&mut self, title: &str, cx: &mut Context<Self>) {
        if let Some(at) = self
            .shown
            .iter()
            .position(|&i| self.actions[i].title == title)
        {
            self.selected = at;
            self.follow_preview(cx);
            cx.notify();
        }
    }

    /// Turns the preview of the selected action on or off (behaviour 7).
    pub fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview.take().is_none() {
            self.preview = Some((String::new(), None));
            self.follow_preview(cx);
        }
        cx.notify();
    }

    /// Asks for the preview of the selected action when the preview is on and shows another one.
    fn follow_preview(&mut self, cx: &mut Context<Self>) {
        let Some((shown, _)) = &self.preview else {
            return;
        };
        let title = self
            .shown
            .get(self.selected)
            .map(|&i| self.actions[i].title.clone())
            .unwrap_or_default();
        if *shown != title {
            self.preview = Some((title.clone(), None));
            if !title.is_empty() {
                cx.emit(IntentionsEvent::Preview(title));
            }
        }
    }

    /// The planned preview of `title`; ignored when another action is selected by now.
    pub fn set_preview(&mut self, title: &str, lines: Preview, cx: &mut Context<Self>) {
        if let Some((shown, result)) = &mut self.preview
            && shown == title
        {
            *result = Some(lines);
            cx.notify();
        }
    }

    /// The preview's lines, or why there are none; `None` when it is off or still planning.
    pub fn preview_lines(&self) -> Option<Preview> {
        self.preview.as_ref().and_then(|(_, r)| r.clone())
    }

    /// The list's footer (`SPEC-0076` v11 behaviour 16): the key that toggles the preview, and
    /// "→ options" when the selected row has a submenu.
    pub fn footer(&self) -> String {
        let key = crate::keymaps::label("QuickJavaDoc");
        let mut out = if key.is_empty() {
            "Preview: QuickJavaDoc".to_owned()
        } else {
            format!("{key} toggles preview")
        };
        if self.options_of(self.selected).is_some() {
            out.push_str(" · → options");
        }
        out
    }

    /// The speed-search text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Adds actions that came later (the language server's, `SPEC-0076` v12 behaviour 18): the
    /// list keeps its order rules, the typed filter and the selected row.
    pub fn add_actions(&mut self, more: Vec<CodeAction>, cx: &mut Context<Self>) {
        let selected = self
            .shown
            .get(self.selected)
            .map(|&i| self.actions[i].title.clone());
        let mut all = std::mem::take(&mut self.actions);
        all.extend(more);
        self.actions = order(all);
        self.refilter();
        if let Some(title) = selected
            && let Some(row) = self
                .shown
                .iter()
                .position(|&i| self.actions[i].title == title)
        {
            self.selected = row;
        }
        cx.notify();
    }

    fn refilter(&mut self) {
        self.shown = filter(&self.actions, &self.query);
        self.selected = first_enabled(&self.actions, &self.shown).unwrap_or(0);
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        if let Some((row, option)) = self.submenu {
            if let Some(o) = self.options_of(row)
                && let Some(label) = o.labels.get(option)
            {
                cx.emit(IntentionsEvent::RunOption(o.title.clone(), label.clone()));
            }
            return;
        }
        if self
            .options_of(self.selected)
            .is_some_and(|o| o.only_options)
        {
            self.submenu = Some((self.selected, 0));
            return;
        }
        let Some(a) = self.shown.get(self.selected).map(|&i| &self.actions[i]) else {
            return;
        };
        if a.disabled.is_none() {
            cx.emit(IntentionsEvent::Apply(a.title.clone()));
        }
    }

    /// Moves the selection by `step` rows, skipping disabled actions, with wrap-around.
    fn step(&mut self, step: isize) {
        let n = self.shown.len();
        if n == 0 {
            return;
        }
        let mut at = self.selected;
        for _ in 0..n {
            at = (at as isize + step).rem_euclid(n as isize) as usize;
            if self.actions[self.shown[at]].disabled.is_none() {
                self.selected = at;
                return;
            }
        }
    }

    /// Up/Down select, Enter applies, Escape closes, other keys type into the speed search
    /// (behaviours 3–4).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let stroke = ferris_idea_ui::keymap::Keystroke::from_platform(
            &k.key, m.control, m.alt, m.shift, m.platform,
        );
        if stroke.is_some_and(|s| {
            crate::keymaps::current()
                .actions_for(&s, None)
                .contains(&"QuickJavaDoc")
        }) {
            self.toggle_preview(cx);
            cx.stop_propagation();
            return;
        }
        // The first and the last action that can be applied (`SPEC-0275` v19 behaviour 47).
        if self.submenu.is_none()
            && let Some(to) =
                crate::list_nav::edge_among(k, self.shown.len(), crate::list_nav::Keys::ALL, |i| {
                    self.actions[self.shown[i]].disabled.is_none()
                })
        {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.control || k.modifiers.platform {
            return;
        }
        // In an open submenu: Up/Down choose an option, ← or Escape close it.
        if let Some((row, option)) = self.submenu {
            let n = self.options_of(row).map_or(0, |o| o.labels.len()).max(1);
            match k.key.as_str() {
                "down" => self.submenu = Some((row, (option + 1) % n)),
                "up" => self.submenu = Some((row, (option + n - 1) % n)),
                "left" | "escape" => self.submenu = None,
                "enter" => self.choose(cx),
                _ => {}
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match k.key.as_str() {
            "down" => self.step(1),
            "up" => self.step(-1),
            "right" if self.options_of(self.selected).is_some() => {
                self.submenu = Some((self.selected, 0));
            }
            "enter" => self.choose(cx),
            "escape" => cx.emit(IntentionsEvent::Close),
            "backspace" => {
                self.query.pop();
                self.refilter();
            }
            // Typed text arrives through the platform's text input, not as the key behind it
            // (`SPEC-0004` v8 behaviour 17b).
            _ => return,
        }
        self.follow_preview(cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// Takes text the platform's text input committed: it extends the speed search
    /// (`SPEC-0004` v8 behaviour 17b).
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        self.query.push_str(committed);
        self.refilter();
        self.follow_preview(cx);
        cx.notify();
    }
}

crate::committed_text::committed_text_input!(IntentionsView, take_committed);

impl Render for IntentionsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows: Vec<CodeAction> = self
            .shown
            .iter()
            .map(|&i| self.actions[i].clone())
            .collect();
        let selected = self.selected;
        let count = rows.len();
        let with_options: Vec<bool> = (0..count).map(|i| self.options_of(i).is_some()).collect();
        // A thin line between the quick fixes and the other actions (SPEC-0125 v2 behaviour 9).
        let first_other = rows
            .iter()
            .position(|a| !is_fix(a))
            .filter(|&i| i > 0 && rows[..i].iter().all(is_fix));
        let list = uniform_list(
            "intentions",
            count,
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let a = &rows[i];
                        let enabled = a.disabled.is_none();
                        // Menu item metrics of Jewel (SPEC-0154 behaviour 3); disabled
                        // actions get no selection.
                        crate::metrics::menu_item(("intention", i), i == selected && enabled)
                            .gap_2()
                            .h(px(crate::metrics::MENU_ITEM_HEIGHT))
                            .overflow_hidden()
                            .when(!enabled, |d| d.text_color(c(p.ui_dim)))
                            .child(icon(row_icon(a)))
                            .when(Some(i) == first_other, |d| {
                                d.border_t_1().border_color(c(p.separator))
                            })
                            .child(div().flex_1().overflow_hidden().child(a.title.clone()))
                            .when_some(a.disabled.clone(), |d, why| {
                                d.child(div().text_color(c(p.ui_dim)).child(why))
                            })
                            .when(with_options[i], |d| {
                                d.child(div().text_color(c(p.ui_dim)).child("›"))
                            })
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                this.follow_preview(cx);
                                this.choose(cx);
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .h(px(
            (count.clamp(1, 12) as f32) * crate::metrics::MENU_ITEM_HEIGHT
        ));
        // The submenu beside the list (SPEC-0125 v2 behaviour 8), left of it when the window's right
        // edge would cut it off (SPEC-0076 v13 behaviour 19).
        let on_left = crate::popup_place::submenu_left(
            self.left,
            LIST_WIDTH,
            SUBMENU_WIDTH,
            f32::from(window.viewport_size().width),
        );
        let open = self
            .submenu
            .and_then(|(row, option)| Some((row, option, self.options_of(row)?.clone())));
        let mut submenu = None;
        if let Some((row, option, o)) = open {
            let mut items = Vec::new();
            for (k, label) in o.labels.into_iter().enumerate() {
                let title = o.title.clone();
                let chosen = label.clone();
                items.push(
                    crate::metrics::menu_item(("intention-option", k), k == option)
                        .h(px(crate::metrics::MENU_ITEM_HEIGHT))
                        .child(label)
                        .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                            cx.emit(IntentionsEvent::RunOption(title.clone(), chosen.clone()));
                        })),
                );
            }
            let top = px((row.min(11) as f32) * crate::metrics::MENU_ITEM_HEIGHT);
            let frame = crate::metrics::menu_frame()
                .bg(c(p.island))
                .id("intentions-submenu")
                .w(px(SUBMENU_WIDTH))
                .flex()
                .flex_col()
                .children(items);
            // In the flow beside the list on either side, so it is laid out and drawn as on the
            // right; on the left the whole popup moves left by the submenu's width, the list stays
            // where it was (OBS-0117 G7: the absolutely placed submenu came out empty).
            submenu = Some(if on_left {
                frame.mr(px(SUBMENU_GAP)).mt(top)
            } else {
                frame.ml(px(SUBMENU_GAP)).mt(top)
            });
        }
        let left_submenu = on_left && submenu.is_some();
        let frame = crate::metrics::menu_frame()
            .bg(c(p.island))
            .id("intentions-popup")
            .track_focus(&self.focus)
            .occlude()
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .w(px(LIST_WIDTH))
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key))
            .when(!self.query.is_empty(), |d| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_color(c(p.ui_muted))
                        .child(format!("Search for: {}", self.query)),
                )
            })
            .child(list)
            .when(count == 0, |d| {
                d.child(
                    div()
                        .px_3()
                        .pb_2()
                        .text_color(c(p.ui_dim))
                        .child("Nothing matches"),
                )
            })
            .child(
                div()
                    .id("intentions-footer")
                    .border_t_1()
                    .border_color(c(p.separator))
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(self.footer()),
            );
        // The preview beside the list, before and after side by side (v11 behaviour 16).
        let preview = self
            .preview
            .as_ref()
            .map(|(_, result)| preview_pane(result.as_ref(), p));
        let (before, after) = if left_submenu {
            (submenu, None)
        } else {
            (None, submenu)
        };
        div()
            .relative()
            .flex()
            .items_start()
            .when(left_submenu, |d| d.ml(px(-(SUBMENU_WIDTH + SUBMENU_GAP))))
            .children(before)
            .child(frame)
            .children(after)
            .children(preview)
    }
}

/// The icon of a row: Run, Debug or Profiler for a run row (`SPEC-0076` v11 behaviour 14), the red
/// bulb for a quick fix, the yellow one for the rest.
fn row_icon(a: &CodeAction) -> &'static str {
    if ferris_idea_languages::context_actions::is_run(a) {
        if a.title.starts_with("Debug '") {
            "run/debug"
        } else if a.title.starts_with("Profile '") {
            "ferris-idea/profiler"
        } else {
            "run/run"
        }
    } else if is_fix(a) {
        "codeInsight/quickfixBulb"
    } else {
        "codeInsight/intentionBulb"
    }
}

/// The preview pane beside the list (`SPEC-0076` v11 behaviour 16): per file its path, then each
/// changed block with the lines before (left) and after (right).
fn preview_pane(result: Option<&Preview>, p: &Palette) -> gpui::AnyElement {
    use ferris_idea_languages::context_actions::{PreviewBlock, blocks_of_lines};
    let line = |text: &str, rgb: Option<u32>| {
        div()
            .whitespace_nowrap()
            .overflow_hidden()
            .when_some(rgb, |d, rgb| d.text_color(gpui::rgb(rgb)))
            .child(if text.is_empty() {
                " ".to_owned()
            } else {
                text.to_owned()
            })
    };
    let (removed, added) = (p.console_ansi_text[1], p.console_ansi_text[2]);
    let body: Vec<gpui::AnyElement> = match result {
        None => vec![div().child("Planning…").into_any_element()],
        Some(Err(why)) => vec![div().child(why.clone()).into_any_element()],
        Some(Ok(lines)) => blocks_of_lines(lines)
            .into_iter()
            .map(|block| match block {
                PreviewBlock::File(path) => div()
                    .pt_1()
                    .text_color(c(p.ui_muted))
                    .child(path)
                    .into_any_element(),
                PreviewBlock::Note(note) => {
                    div().text_color(c(p.ui_dim)).child(note).into_any_element()
                }
                PreviewBlock::Change { before, after } => div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .children(before.iter().map(|l| line(l, Some(removed)))),
                    )
                    .child(
                        div()
                            .border_l_1()
                            .border_color(c(p.separator))
                            .pl_2()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .children(after.iter().map(|l| line(l, Some(added)))),
                    )
                    .into_any_element(),
            })
            .collect(),
    };
    crate::metrics::menu_frame()
        .bg(c(p.island))
        .id("intention-preview")
        .ml_1()
        .w(px(560.))
        .max_h(px(360.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .gap_2()
                .px_3()
                .pt_1()
                .text_xs()
                .text_color(c(p.ui_dim))
                .child(div().flex_1().child("Before"))
                .child(div().flex_1().child("After")),
        )
        .child(
            div()
                .px_3()
                .py_1()
                .flex()
                .flex_col()
                .font_family(crate::fonts::EDITOR_FONT)
                .text_color(c(p.ui_dim))
                .children(body),
        )
        .into_any_element()
}

//! The context-actions popup (`SPEC-0076` v3): the language server's fixes and refactorings at the
//! caret; typing filters, Enter applies, the `QuickJavaDoc` key previews the selected action.

use std::ops::Range;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, uniform_list,
};
use idecraft_languages::actions::CodeAction;

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
}

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
        }
    }

    /// Gives rows their submenus (`SPEC-0125` v2 behaviour 8).
    pub fn with_options(mut self, options: Vec<RowOptions>) -> Self {
        self.options = options;
        self
    }

    fn options_of(&self, row: usize) -> Option<&RowOptions> {
        let title = &self.actions[*self.shown.get(row)?].title;
        self.options.iter().find(|o| &o.title == title)
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

    /// The speed-search text.
    pub fn query(&self) -> &str {
        &self.query
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
        let stroke = idecraft_ui::keymap::Keystroke::from_platform(
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child(icon(if is_fix(a) {
                                "codeInsight/quickfixBulb"
                            } else {
                                "codeInsight/intentionBulb"
                            }))
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
        // The submenu beside the list (SPEC-0125 v2 behaviour 8).
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
            submenu = Some(
                crate::metrics::menu_frame()
                    .bg(c(p.island))
                    .id("intentions-submenu")
                    .ml_1()
                    .mt(px((row.min(11) as f32) * crate::metrics::MENU_ITEM_HEIGHT))
                    .w(px(380.))
                    .flex()
                    .flex_col()
                    .children(items),
            );
        }
        let frame = crate::metrics::menu_frame()
            .bg(c(p.island))
            .id("intentions-popup")
            .track_focus(&self.focus)
            .occlude()
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .w(px(420.))
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
            .when_some(self.preview.as_ref(), |d, (_, result)| {
                let body: Vec<gpui::AnyElement> = match result {
                    None => vec![div().child("Planning…").into_any_element()],
                    Some(Err(why)) => vec![div().child(why.clone()).into_any_element()],
                    Some(Ok(lines)) => lines
                        .iter()
                        .map(|l| {
                            let color = if l.starts_with("- ") {
                                Some(p.console_ansi_text[1])
                            } else if l.starts_with("+ ") {
                                Some(p.console_ansi_text[2])
                            } else {
                                None
                            };
                            div()
                                .whitespace_nowrap()
                                .when_some(color, |d, rgb| d.text_color(gpui::rgb(rgb)))
                                .when(color.is_none(), |d| d.text_color(c(p.ui_muted)))
                                .child(l.clone())
                                .into_any_element()
                        })
                        .collect(),
                };
                d.child(
                    div()
                        .id("intention-preview")
                        .border_t_1()
                        .border_color(c(p.separator))
                        .px_3()
                        .py_1()
                        .max_h(px(260.))
                        .overflow_hidden()
                        .font_family(crate::fonts::EDITOR_FONT)
                        .text_color(c(p.ui_dim))
                        .children(body),
                )
            });
        div().flex().items_start().child(frame).children(submenu)
    }
}

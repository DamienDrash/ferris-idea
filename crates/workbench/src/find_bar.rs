//! The find bar above the editor (`SPEC-0065`): query field, options, counter and navigation.

use std::path::PathBuf;

use ferris_idea_core::Query;
use gpui::{Context, Entity, Subscription, div, prelude::*, px};

use crate::EditorView;
use crate::find_model::FindState;

/// State of an open find bar.
pub struct FindBar {
    /// One-line editor holding the pattern.
    pub query: Entity<EditorView>,
    /// Options as toggled in the bar (behaviour 1).
    pub match_case: bool,
    /// Whole words only.
    pub whole_words: bool,
    /// Pattern is a regular expression.
    pub regex: bool,
    /// Matches in the active editor.
    pub state: FindState,
    /// Pattern and options the state was computed for.
    pub searched: Option<Query>,
    /// Editor and revision the state was computed for.
    pub target: Option<(PathBuf, u64)>,
    /// Observation of the active editor (re-made when the tab changes).
    pub watch: Option<(PathBuf, Subscription)>,
    /// Observation of the query field.
    pub _query_watch: Subscription,
    /// Replacement field, while the replace row is shown (behaviour 8).
    pub replace: Option<Entity<EditorView>>,
    /// Search only in this char range of the editor (v4 behaviour 12).
    pub in_selection: Option<std::ops::Range<usize>>,
    /// Comments and string literals filter (v4 behaviour 13).
    pub context: ferris_idea_languages::syntax::SearchContext,
    /// Typing in the query moves the editor to the first match (v4 behaviour 14).
    pub scroll_while_typing: bool,
    /// Replacements keep the case of the matched text (v4 behaviour 16).
    pub preserve_case: bool,
    /// Start offsets of matches left out of Replace All (v4 behaviour 17).
    pub excluded: Vec<usize>,
    /// Which menu of the bar is open.
    pub menu: Option<FindMenu>,
}

/// The menus of the find bar (`SPEC-0065` v4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindMenu {
    /// Recent searches.
    History,
    /// In selection and the comments/strings filter.
    Filter,
    /// Open in Find Window, the occurrence actions, scrolling while typing.
    More,
}

impl FindBar {
    /// A closed-state bar around `query`.
    pub fn new(query: Entity<EditorView>, watch: Subscription) -> Self {
        Self {
            query,
            match_case: false,
            whole_words: false,
            regex: false,
            state: FindState::default(),
            searched: None,
            target: None,
            watch: None,
            _query_watch: watch,
            replace: None,
            in_selection: None,
            context: ferris_idea_languages::syntax::SearchContext::Anywhere,
            scroll_while_typing: true,
            preserve_case: false,
            excluded: Vec::new(),
            menu: None,
        }
    }
}

impl FindBar {
    /// The query as typed and toggled.
    pub fn query_of(&self, cx: &gpui::App) -> Query {
        Query {
            pattern: self.query.read(cx).state.buffer.text(),
            regex: self.regex,
            match_case: self.match_case,
            whole_words: self.whole_words,
        }
    }
}

/// A toggle button of the bar: label, tooltip-like id, state.
pub fn toggle_button<V: 'static>(
    control: &'static ferris_idea_ui::tooltip::Control,
    label: &'static str,
    on: bool,
    cx: &mut Context<V>,
    flip: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let p = crate::palette::Palette::current();
    use crate::tips::Tipped;
    div()
        .id(control.name)
        .tip(control)
        .px_1()
        .h(px(20.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .text_size(px(12.))
        .when(on, |d| {
            d.bg(crate::workbench_view::c(p.tab_selected))
                .text_color(crate::workbench_view::c(p.ui_text))
        })
        .when(!on, |d| d.text_color(crate::workbench_view::c(p.ui_muted)))
        .hover(|s| s.bg(crate::workbench_view::c(p.toolbar_hover)))
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| flip(this, cx)))
}

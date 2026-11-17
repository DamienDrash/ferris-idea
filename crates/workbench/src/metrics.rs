//! Control metrics of the New UI (`SPEC-0131` behaviour 5, `SPEC-0154`): sizes of buttons, text
//! fields, popups, trees, tabs and menus as the open Jewel library of the IntelliJ Platform defines
//! them for Int UI (`platform/jewel/int-ui/int-ui-standalone`, `IntUiLazyTreeStyling`,
//! `IntUiTabStyling`, `IntUiMenuStyling`, Apache-2.0; values only, see `PROVENANCE.md`). Where the
//! theme sets a value (`Tree.rowHeight`, `Tree.border`, `TabbedPane.tabHeight`,
//! `EditorTabs.tabInsets`, …) the theme wins, as in the reference.

use gpui::{Div, ElementId, Stateful, div, prelude::*, px};
pub use idecraft_ui::metrics::*;

use crate::palette::Palette;
use crate::workbench_view::c;

/// Height of buttons and text fields (`ButtonMetrics`/`TextFieldMetrics` `minSize`).
pub const CONTROL_HEIGHT: f32 = 28.0;
/// Smallest button width.
pub const BUTTON_MIN_WIDTH: f32 = 72.0;
/// Space left and right of a button's label.
pub const BUTTON_PADDING_X: f32 = 12.0;
/// Corner radius of buttons and text fields.
pub const CONTROL_CORNER: f32 = 4.0;
/// Space left and right of a text field's content (8 plus the 1 px border).
pub const FIELD_PADDING_X: f32 = 9.0;
/// Corner radius of popups and dialogs (`PopupContainerMetrics`).
pub const POPUP_CORNER: f32 = 8.0;

/// A tree row at `depth` with the theme's row height, the margins, indentation and selection
/// corner of the metrics; selection and hover colours are the caller's.
pub fn tree_row(id: impl Into<ElementId>, depth: usize) -> Stateful<Div> {
    let p = Palette::current();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(TREE_ICON_GAP))
        .ml(px(p.metrics.tree_margin[0]))
        .mr(px(p.metrics.tree_margin[1]))
        .h(px(p.metrics.tree_row_height))
        .pl(px(tree_indent(depth)))
        .pr(px(TREE_ROW_PADDING))
        .rounded(px(p.metrics.tree_selection_corner))
}

/// An editor tab bar with the theme's height (`TabbedPane.tabHeight`) (`SPEC-0154` behaviour 2).
pub fn tab_bar() -> Div {
    let p = Palette::current();
    div()
        .flex()
        .items_center()
        .gap_1()
        // The first tab 6 from the island's edge (OBS-0020).
        .px(px(6.))
        .h(px(p.metrics.tab_bar_height))
        .flex_none()
        .border_b_1()
        .border_color(c(p.separator))
}

/// An editor tab: themes with negative top and bottom `EditorTabs.tabInsets` (Islands) draw the
/// selected tab as a pill inside the bar with `EditorTabs.underlineArc` corners, the others draw an
/// underline of `EditorTabs.underlineHeight`; side padding from the insets, else Jewel's.
/// `active` (the keyboard focus is in the tab's editor) draws the selected pill in the theme's
/// active colours, else in its inactive ones, each with its border (`SPEC-0061` v19 behaviour 6a).
/// Holds the content gap before the close button (`SPEC-0154` behaviour 2).
pub fn tab(id: impl Into<ElementId>, selected: bool, active: bool) -> Stateful<Div> {
    let p = Palette::current();
    let m = &p.metrics;
    let pill = m.tab_style == TabStyle::Pill;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(m.tab_gaps[1]))
        // The border is part of the padding (OBS-0020: the icon 8 from the pill's edge).
        .pl(px(m.tab_padding[0] - if pill { 1.0 } else { 0.0 }))
        .pr(px(m.tab_padding[1]))
        .h(px(m.tab_height))
        .when(pill, |d| {
            d.rounded(px(m.tab_arc))
                .border_1()
                .border_color(gpui::transparent_black())
        })
        .when(selected && pill, |d| {
            let (fill, border) = pill_colours(p, active);
            d.bg(c(fill)).border_color(c(border))
        })
        .when(selected && !pill, |d| {
            d.border_b(px(m.tab_underline_height))
                .border_color(c(p.tab_underline))
        })
        .when(!selected, |d| {
            d.opacity(p.tab_unselected_alpha)
                .hover(|s| s.opacity(1.0).bg(c(p.tab_hover)))
        })
}

/// Fill and border of a selected pill tab: the active colours while the keyboard focus is in its
/// editor, the inactive ones otherwise (`SPEC-0061` v19 behaviour 6a).
pub fn pill_colours(
    p: &Palette,
    active: bool,
) -> (idecraft_ui::theme::Rgba, idecraft_ui::theme::Rgba) {
    if active {
        (p.tab_selected, p.tab_selected_border)
    } else {
        (p.tab_selected_inactive, p.tab_selected_border_inactive)
    }
}

/// A tab's icon and label with Jewel's content gap.
pub fn tab_content(icon: impl IntoElement, label: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(TAB_CONTENT_GAP))
        .child(icon)
        .child(label)
}

/// The frame of a popup menu or list: corner, padding, border and shadow of the metrics; the
/// background is the caller's.
pub fn menu_frame() -> Div {
    let p = Palette::current();
    div()
        .py(px(MENU_PADDING_Y))
        .px(px(MENU_PADDING_X))
        .border_1()
        .border_color(c(p.popup_border))
        .rounded(px(MENU_CORNER))
        .shadow_lg()
}

/// One menu item; `selected` draws the selection.
pub fn menu_item(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    let p = Palette::current();
    div()
        .id(id)
        .flex()
        .items_center()
        .mx(px(MENU_ITEM_MARGIN_X))
        .px(px(MENU_ITEM_PADDING_X))
        // One line is 24 px, as in the reference's popup menus (OBS-0020); longer content grows.
        .min_h(px(MENU_ITEM_HEIGHT))
        .rounded(px(MENU_SELECTION_CORNER))
        .when(selected, |d| d.bg(c(p.tree_selection)))
        .when(!selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
}

/// A dialog button; `primary` is the default one (Enter).
pub fn button(id: impl Into<ElementId>, label: impl IntoElement, primary: bool) -> Stateful<Div> {
    let p = Palette::current();
    div()
        .id(id)
        .h(px(CONTROL_HEIGHT))
        .min_w(px(BUTTON_MIN_WIDTH))
        .px(px(BUTTON_PADDING_X))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(CONTROL_CORNER))
        .border_1()
        .border_color(c(p.separator))
        .when(primary, |d| {
            d.bg(c(p.accent))
                .border_color(c(p.accent))
                .text_color(c(p.accent_text))
        })
        .when(!primary, |d| d.hover(|s| s.bg(c(p.tree_hover))))
        .child(label)
}

/// The frame around a one-line text field; its content is clipped to it.
pub fn field_frame() -> Div {
    let p = Palette::current();
    div()
        .h(px(CONTROL_HEIGHT))
        .px(px(FIELD_PADDING_X))
        .flex()
        .items_center()
        // A long text stays inside the field (`SPEC-0108` v6 behaviour 15).
        .min_w_0()
        .overflow_hidden()
        .rounded(px(CONTROL_CORNER))
        .border_1()
        .border_color(c(p.separator))
}

/// A content tab in a tool window header (`SPEC-0061` v4 behaviour 10): the tab padding left and
/// right of its text, as high as the header; the selected tab is underlined, in the focused colour
/// while its tool window has the focus. Shared by every tool window header (v5 behaviour 18).
pub fn header_tab(id: impl Into<ElementId>, selected: bool, focused: bool) -> Stateful<Div> {
    let p = Palette::current();
    let m = &p.metrics.header;
    // Islands: a pill like the editor tabs, the editor's selected colours while the window has
    // the focus and the window colour with a grey border otherwise (SPEC-0061 v6 behaviour 23,
    // OBS-0020).
    if m.pill {
        return div()
            .id(id)
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .h(px(m.pill_height))
            .pl(px(m.pill_padding[0]))
            .pr(px(m.pill_padding[1]))
            .rounded(px(m.pill_arc))
            .border_1()
            .border_color(gpui::transparent_black())
            .when(selected && focused, |d| {
                d.bg(c(p.tab_selected))
                    .border_color(c(p.tab_selected_border))
            })
            .when(selected && !focused, |d| {
                d.bg(c(p.window)).border_color(c(p.popup_border))
            })
            .when(!selected, |d| d.hover(|s| s.bg(c(p.header_tab_hover))));
    }
    let underline = if focused {
        p.header_tab_underline
    } else {
        p.header_tab_underline_inactive
    };
    div()
        .id(id)
        .relative()
        .h_full()
        .flex()
        .items_center()
        .gap_1()
        .px(px(m.tab_padding))
        .when(!selected, |d| {
            d.text_color(c(p.ui_muted))
                .hover(|s| s.bg(c(p.header_tab_hover)))
        })
        .when(selected, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(m.tab_underline))
                    .rounded(px(m.tab_arc))
                    .bg(c(underline)),
            )
        })
}

/// The header row of a panel outside the tool window layout (Find, Find in Files, Endpoints,
/// Spring, AI Chat, Python Packages, Containers, Profiler, Remote Host): the height, title inset
/// and right inset of the shared tool window header (`SPEC-0061` v6 behaviour 33); room is left
/// at the right end for Options and Hide, which the workbench draws there (v7 behaviour 43).
pub fn panel_header(d: Div) -> Div {
    let p = Palette::current();
    let m = &p.metrics.header;
    d.h(px(m.height))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .pl(px(m.label_insets[0]))
        .pr(px(m.toolbar_insets[1] + 2.0 * m.button + 4.0))
}

/// The title of a panel header: semibold, the header's space after it (`SPEC-0061` v6
/// behaviour 33).
pub fn panel_title(title: impl IntoElement) -> Div {
    let p = Palette::current();
    div()
        .flex_none()
        .mr(px(p.metrics.header.label_insets[1] - 8.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(title)
}

/// A toolbar button of a tool window header (`SPEC-0061` v4 behaviour 9, v5 behaviour 18): a
/// square of the header's button size with the icon centred and the toolbar hover background.
pub fn header_button(id: impl Into<ElementId>) -> Stateful<Div> {
    let p = Palette::current();
    div()
        .id(id)
        .flex_none()
        .size(px(p.metrics.header.button))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .hover(|s| s.bg(c(p.toolbar_hover)))
}

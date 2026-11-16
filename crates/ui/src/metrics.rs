//! Metrics of trees, tabs and menus (`SPEC-0154`): the values of the open Jewel library of the
//! IntelliJ Platform for Int UI (`platform/jewel/int-ui/int-ui-standalone`, `IntUiLazyTreeStyling`,
//! `IntUiTabStyling`, `IntUiMenuStyling`, Apache-2.0; values only, see `PROVENANCE.md`), and what a
//! theme makes of them: where it sets `Tree.rowHeight`, `Tree.border`, `TabbedPane.tabHeight`,
//! `EditorTabs.tabInsets`, `EditorTabs.underlineArc` or `EditorTabs.underlineHeight`, the theme
//! wins, as in the reference.
//!
//! The window structure (`SPEC-0061` v4): tool window header and tabs, stripes, main toolbar,
//! status bar and islands, with the defaults of the open platform UI code
//! (`JBUI.CurrentTheme`, `ActionToolbar`, `IdeStatusBarImpl`; values only, see `PROVENANCE.md`)
//! and the theme's keys winning.

use serde::Serialize;

use crate::theme::UiTheme;

/// Indentation of one tree level: 19, measured in the reference (`OBS-0020`; the New UI tree's left
/// and right child indents, 7 + 12), not Jewel's 7 + 16.
pub const TREE_INDENT: f32 = 19.0;
/// Space left and right of a tree row's selection (`elementPadding`; `Tree.border` in the themes).
pub const TREE_ROW_MARGIN_X: f32 = 12.0;
/// Space inside a tree row before its content (`elementContentPadding`).
pub const TREE_ROW_PADDING: f32 = 4.0;
/// Corner radius of a tree row's selection (`elementBackgroundCornerSize`).
pub const TREE_SELECTION_CORNER: f32 = 2.0;
/// Gap between a tree row's icon and its text (`elementIconTextGap`).
pub const TREE_ICON_GAP: f32 = 4.0;
/// Gap between the chevron and the content (`chevronContentGap`).
pub const TREE_CHEVRON_GAP: f32 = 2.0;

/// Height of a tab bar (`TabMetrics.tabHeight`; `TabbedPane.tabHeight` in the themes).
pub const TAB_BAR_HEIGHT: f32 = 40.0;
/// Space left and right of a tab's content (`tabPadding`).
pub const TAB_PADDING_X: f32 = 8.0;
/// Gap between a tab's icon and label (`tabContentSpacing`).
pub const TAB_CONTENT_GAP: f32 = 4.0;
/// Gap before a tab's close button: 4, measured in the reference (`OBS-0020`), with 4 after it.
pub const TAB_CLOSE_GAP: f32 = 4.0;
/// Space after a tab's close button (`OBS-0020`).
pub const TAB_CLOSE_PADDING: f32 = 4.0;
/// Thickness of the selected tab's underline (`underlineThickness`).
pub const TAB_UNDERLINE: f32 = 3.0;

/// Corner radius of menus (`MenuMetrics.cornerSize`).
pub const MENU_CORNER: f32 = 8.0;
/// Space above and below a menu's items (`contentPadding` vertical).
pub const MENU_PADDING_Y: f32 = 7.0;
/// Space left and right of a menu's items (`contentPadding` horizontal).
pub const MENU_PADDING_X: f32 = 2.0;
/// Space left and right of an item's selection (`MenuItemMetrics.outerPadding`).
pub const MENU_ITEM_MARGIN_X: f32 = 6.0;
/// Space inside an item left and right (`contentPadding` horizontal).
pub const MENU_ITEM_PADDING_X: f32 = 12.0;
/// Space inside an item above and below (`contentPadding` vertical).
pub const MENU_ITEM_PADDING_Y: f32 = 4.0;
/// Smallest item height (`minHeight`).
pub const MENU_ITEM_MIN_HEIGHT: f32 = 20.0;
/// Corner radius of an item's selection (`selectionCornerSize`).
pub const MENU_SELECTION_CORNER: f32 = 4.0;
/// Height of a separator line's row (`separatorHeight`).
pub const MENU_SEPARATOR_HEIGHT: f32 = 9.0;
/// Space before an item's shortcut (`keybindingsPadding`).
pub const MENU_KEYBINDING_GAP: f32 = 36.0;

/// Height of a menu item with one line of UI text (13 px font, 16 px line).
pub const MENU_ITEM_HEIGHT: f32 = 16.0 + 2.0 * MENU_ITEM_PADDING_Y;

/// Height of a tool window header when the theme does not set `ToolWindow.Header.height`
/// (intellij-community `JBUI.CurrentTheme.ToolWindow.defaultHeaderHeight`).
pub const HEADER_HEIGHT: f32 = 41.0;
/// Space before and after a tool window's title (`ToolWindow.Header.labelLeftRightInsets`).
pub const HEADER_LABEL_INSETS: [f32; 2] = [12.0, 16.0];
/// Space before and after the header's toolbar (`ToolWindow.Header.toolbarLeftRightInsets`).
pub const HEADER_TOOLBAR_INSETS: [f32; 2] = [12.0, 8.0];
/// Edge of a header toolbar button: 26, measured in the reference (`OBS-0020`: buttons 26 apart,
/// Hide centred 21 from the right edge).
pub const HEADER_BUTTON: f32 = 26.0;
/// Space left and right of a header tab's text (`ToolWindow.HeaderTab.leftRightInsets`, New UI).
pub const HEADER_TAB_PADDING: f32 = 12.0;
/// Islands draw a header content tab as a pill like the editor tabs (`OBS-0020`): its height in
/// the normal and the compact mode, the space left of its text and right of its close button,
/// and its corner radius.
pub const HEADER_PILL: [f32; 2] = [28.0, 24.0];
/// Space inside a header pill left of its text and right of its content.
pub const HEADER_PILL_PADDING: [f32; 2] = [8.0, 6.0];
/// Corner radius of a header pill.
pub const HEADER_PILL_ARC: f32 = 6.0;
/// Space between the title and the first pill, and between pills.
pub const HEADER_PILL_GAP: f32 = 4.0;
/// Header height of the compact mode (`OBS-0020`: 31 and the line below).
pub const HEADER_HEIGHT_COMPACT: f32 = 32.0;
/// Thickness of the selected header tab's underline (`DefaultTabs.underlineHeight`).
pub const HEADER_TAB_UNDERLINE: f32 = 3.0;
/// Corner radius of that underline (`ToolWindow.HeaderTab.underlineArc`).
pub const HEADER_TAB_ARC: f32 = 4.0;

/// Size of a stripe button when the theme does not set `StripeToolbar.Button.size`.
pub const STRIPE_BUTTON: [f32; 2] = [40.0, 40.0];
/// Edge of a stripe icon (`StripeToolbar.Button.iconSize`).
pub const STRIPE_ICON: f32 = 20.0;
/// Padding of a stripe button's highlight when the theme sets none (`insets(5)`).
pub const STRIPE_ICON_PADDING: f32 = 5.0;
/// `Button.ToolWindow.arc` in the normal and the compact mode.
pub const STRIPE_BUTTON_ARC: [f32; 2] = [12.0, 8.0];

/// Edge of a main toolbar button (`MainToolbar.Button.size`).
pub const TOOLBAR_BUTTON: f32 = 30.0;
/// Edge of a main toolbar icon (`MainToolbar.Button.iconSize`).
pub const TOOLBAR_ICON: f32 = 20.0;
/// `MainToolbar.Button.arc` and `MainToolbar.Dropdown.arc`.
pub const TOOLBAR_ARC: f32 = 12.0;
/// Padding of a toolbar widget (`MainToolbar.Dropdown.borderInsets`: top, left, bottom, right).
pub const TOOLBAR_DROPDOWN_INSETS: [f32; 4] = [5.0, 10.0, 5.0, 6.0];
/// Widest toolbar widget when the theme sets no `MainToolbar.Dropdown.maxWidth`.
pub const TOOLBAR_DROPDOWN_MAX_WIDTH: f32 = 350.0;

/// Height of the Islands status bar in the normal and the compact mode, from the islands' foot to
/// the window's (`OBS-0020`: 32 and 26).
pub const STATUS_HEIGHT_ISLANDS: [f32; 2] = [32.0, 26.0];
/// Space between a stripe and the islands, and at the window edge without stripes, in the normal
/// and the compact mode (`OBS-0020`).
pub const ISLAND_EDGE: [f32; 2] = [3.5, 2.5];
/// Space between the main toolbar and the islands (`OBS-0020`).
pub const ISLAND_TOP: [f32; 2] = [3.0, 2.0];
/// Size and icon of a compact stripe button (`OBS-0020`: a 24 px highlight, 16 px icons).
pub const STRIPE_BUTTON_COMPACT: [f32; 2] = [32.0, 32.0];
/// Space left and right of the status bar (`IdeStatusBarImpl`, New UI).
pub const STATUS_PADDING_X: f32 = 10.0;
/// Padding of a status bar widget (`StatusBar.Widget.widgetInsets`: vertical, horizontal).
pub const STATUS_WIDGET_INSETS: [f32; 2] = [6.0, 8.0];
/// What the hover background leaves free of a widget (`StatusBar.Widget.hoverInsets`).
pub const STATUS_HOVER_INSETS: [f32; 4] = [3.0, 4.0, 1.0, 4.0];
/// Corner radius of a status bar widget's hover background.
pub const STATUS_HOVER_CORNER: f32 = 4.0;
/// One line of UI text.
pub const UI_LINE: f32 = 16.0;

/// Where the macOS close button sits in a workbench window's title bar, from the window's top
/// left (`SPEC-0061` v7 behaviour 44).
pub const TRAFFIC_LIGHT_POSITION: [f32; 2] = [14.0, 13.0];
/// Distance from one macOS window button to the next (`OBS-0020` item 17: 20 pt in the
/// reference; AppKit's own pitch on macOS 27 is 23).
pub const TRAFFIC_LIGHT_PITCH: f32 = 20.0;

/// Left padding of a tree row at `depth` (0 for the top level).
pub fn tree_indent(depth: usize) -> f32 {
    TREE_ROW_PADDING + TREE_INDENT * depth as f32
}

/// How the selected editor tab is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TabStyle {
    /// A rounded pill inside the bar (Islands: negative top and bottom `EditorTabs.tabInsets`).
    Pill,
    /// An underline at the bottom of the full-height tab.
    Underline,
}

/// The metrics a theme gives trees, tabs and menus (`SPEC-0154` behaviours 1–3).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeMetrics {
    /// Height of a tree row (`Tree.rowHeight`).
    pub tree_row_height: f32,
    /// Indentation per tree level.
    pub tree_indent: f32,
    /// Space left and right of a tree row (`Tree.border`).
    pub tree_margin: [f32; 2],
    /// Corner radius of a tree row's selection.
    pub tree_selection_corner: f32,
    /// Height of the editor tab bar (`TabbedPane.tabHeight`).
    pub tab_bar_height: f32,
    /// How the selected tab is drawn.
    pub tab_style: TabStyle,
    /// Height of a tab.
    pub tab_height: f32,
    /// Space left and right of a tab's content.
    pub tab_padding: [f32; 2],
    /// Corner radius of the pill or the underline (`EditorTabs.underlineArc`).
    pub tab_arc: f32,
    /// Thickness of the underline (`EditorTabs.underlineHeight`).
    pub tab_underline_height: f32,
    /// Gap between a tab's icon and label, and before its close button.
    pub tab_gaps: [f32; 2],
    /// Corner radius of menus.
    pub menu_corner: f32,
    /// Height of a menu item.
    pub menu_item_height: f32,
    /// Space left and right of an item's selection and inside it.
    pub menu_item_padding: [f32; 2],
    /// Corner radius of an item's selection.
    pub menu_selection_corner: f32,
    /// Height of a separator's row.
    pub menu_separator_height: f32,
    /// Height of the main toolbar (window header): 40, 32 in compact mode (`SPEC-0165`;
    /// intellij-community `CustomWindowHeaderUtil`).
    pub toolbar_height: f32,
    /// Height of a list row (`List.rowHeight`).
    pub list_row_height: f32,
    /// Whether these are the compact metrics.
    pub compact: bool,
    /// Tool window header and its content tabs (`SPEC-0061` v4 behaviours 9 and 10).
    pub header: HeaderMetrics,
    /// Tool window stripes (`SPEC-0061` v4 behaviour 12).
    pub stripe: StripeMetrics,
    /// Main toolbar widgets and buttons (`SPEC-0061` v4 behaviour 14).
    pub toolbar: ToolbarMetrics,
    /// Status bar (`SPEC-0061` v4 behaviour 15).
    pub status: StatusMetrics,
    /// Islands (`SPEC-0061` behaviour 1, v4 behaviour 15a).
    pub island: IslandMetrics,
}

/// The header of a tool window (`SPEC-0061` v4 behaviours 9 and 10).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeaderMetrics {
    /// Height (`ToolWindow.Header.height`, else 41).
    pub height: f32,
    /// Space before and after the title.
    pub label_insets: [f32; 2],
    /// Space before and after the toolbar on the right.
    pub toolbar_insets: [f32; 2],
    /// Edge of a toolbar button.
    pub button: f32,
    /// Space left and right of a content tab's text.
    pub tab_padding: f32,
    /// Thickness of the selected content tab's underline.
    pub tab_underline: f32,
    /// Corner radius of that underline.
    pub tab_arc: f32,
    /// Whether the selected content tab is a pill (Islands, `OBS-0020`) instead of an underline.
    pub pill: bool,
    /// Height of a pill.
    pub pill_height: f32,
    /// Space in a pill left of its text and right of its content.
    pub pill_padding: [f32; 2],
    /// Corner radius of a pill.
    pub pill_arc: f32,
    /// Space between the title and the first pill and between pills.
    pub pill_gap: f32,
}

/// The tool window stripes (`SPEC-0061` v4 behaviour 12).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StripeMetrics {
    /// Width and height of a button; the stripe is as wide as a button.
    pub button: [f32; 2],
    /// Edge of a button's icon.
    pub icon: f32,
    /// What the highlight leaves free on the left stripe (top, left, bottom, right).
    pub left_padding: [f32; 4],
    /// The same on the right stripe.
    pub right_padding: [f32; 4],
    /// Corner radius of the highlight (`Button.ToolWindow.arc` / 2).
    pub radius: f32,
}

impl StripeMetrics {
    /// Width of a stripe.
    pub fn width(&self) -> f32 {
        self.button[0]
    }

    /// The highlight of a button on the left (`left`) or right stripe inside the button: x, y,
    /// width, height.
    pub fn highlight(&self, left: bool) -> [f32; 4] {
        let [top, l, bottom, r] = if left {
            self.left_padding
        } else {
            self.right_padding
        };
        [
            l,
            top,
            self.button[0] - l - r,
            self.button[1] - top - bottom,
        ]
    }
}

/// The main toolbar (`SPEC-0061` v4 behaviour 14).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolbarMetrics {
    /// Edge of an icon button.
    pub button: f32,
    /// Edge of its icon.
    pub icon: f32,
    /// Corner radius of buttons and widgets (`MainToolbar.Button.arc` / 2).
    pub radius: f32,
    /// Padding of a widget (top, left, bottom, right).
    pub dropdown_insets: [f32; 4],
    /// Widest widget.
    pub dropdown_max_width: f32,
}

/// The status bar (`SPEC-0061` v4 behaviour 15).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusMetrics {
    /// Height: one line of text and the widget padding.
    pub height: f32,
    /// Space left and right of the bar.
    pub padding_x: f32,
    /// Padding of a widget (vertical, horizontal).
    pub widget_insets: [f32; 2],
    /// What the hover background leaves free (top, left, bottom, right).
    pub hover_insets: [f32; 4],
    /// Corner radius of the hover background.
    pub hover_radius: f32,
    /// Width of the line above the bar (`StatusBar.topBorderWidth`, else 1).
    pub top_border: f32,
}

/// Islands (`SPEC-0061` behaviour 1, v4 behaviour 15a); 0 for themes without islands.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IslandMetrics {
    /// Corner radius (`Island.arc` / 2).
    pub radius: f32,
    /// Visible gap between islands: `Island.borderWidth` less the 1 px the reference's islands
    /// keep for their antialiased edges (`OBS-0020`: 5, compact 3).
    pub gap: f32,
    /// Space between a stripe (or the window edge) and the islands (`OBS-0020`: 3.5, compact 2.5).
    pub edge: f32,
    /// Space between the main toolbar and the islands (`OBS-0020`: 3, compact 2).
    pub top: f32,
}

/// Where the parts of a tool window header sit, from the header's top left corner (`SPEC-0061`
/// v5 behaviour 20); rectangles are x, y, width, height.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeaderLayout {
    /// The title.
    pub title: [f32; 4],
    /// The content tabs, left to right, as high as the header.
    pub tabs: Vec<[f32; 4]>,
    /// The window's own toolbar buttons, left to right.
    pub toolbar: Vec<[f32; 4]>,
    /// Options (⋮).
    pub options: [f32; 4],
    /// Hide.
    pub hide: [f32; 4],
}

impl HeaderMetrics {
    /// Lays out a header `width` wide whose title is `title` wide, with content tabs whose texts are
    /// `tabs` wide and `toolbar` own buttons: the title after the label inset, the first tab after the
    /// space behind the title, tabs padded on both sides; at the right Hide before the toolbar's right
    /// inset, Options before it and the own buttons before Options, all buttons centred vertically
    /// and never left of the header (`SPEC-0061` v4 behaviours 9 and 10, v5 behaviour 20).
    pub fn layout(&self, width: f32, title: f32, tabs: &[f32], toolbar: usize) -> HeaderLayout {
        let y = ((self.height - self.button) / 2.0).max(0.0);
        let button = |x: f32| [x.max(0.0), y, self.button, self.button];
        let mut x = self.label_insets[0];
        let title_rect = [x, 0.0, title, self.height];
        x += title + self.label_insets[1];
        let tabs = tabs
            .iter()
            .map(|text| {
                if self.pill {
                    // Pills centred in the header, `pill_gap` before each (`OBS-0020`).
                    x += self.pill_gap;
                    let w = text + 2.0 * self.pill_padding[0];
                    let top = ((self.height - self.pill_height) / 2.0).floor();
                    let r = [x, top, w, self.pill_height];
                    x += w;
                    return r;
                }
                let w = text + 2.0 * self.tab_padding;
                let r = [x, 0.0, w, self.height];
                x += w;
                r
            })
            .collect();
        let hide_x = width - self.toolbar_insets[1] - self.button;
        let options_x = hide_x - self.button;
        let toolbar = (0..toolbar)
            .map(|i| button(options_x - (toolbar - i) as f32 * self.button))
            .collect();
        HeaderLayout {
            title: title_rect,
            tabs,
            toolbar,
            options: button(options_x),
            hide: button(hide_x),
        }
    }

    fn of(theme: &UiTheme, compact: bool, pill: bool) -> Self {
        let tab_insets = theme.insets("ToolWindow.HeaderTab.leftRightInsets");
        Self {
            height: theme
                .own_number_in("ToolWindow.Header.height", compact)
                .unwrap_or(if compact && pill {
                    HEADER_HEIGHT_COMPACT
                } else {
                    HEADER_HEIGHT
                }),
            label_insets: theme
                .insets("ToolWindow.Header.labelLeftRightInsets")
                .map_or(HEADER_LABEL_INSETS, |i| [i[1], i[3]]),
            toolbar_insets: theme
                .insets("ToolWindow.Header.toolbarLeftRightInsets")
                .map_or(HEADER_TOOLBAR_INSETS, |i| [i[1], i[3]]),
            button: HEADER_BUTTON,
            tab_padding: tab_insets.map_or(HEADER_TAB_PADDING, |i| i[1]),
            tab_underline: theme
                .own_number_in("ToolWindow.HeaderTab.underlineHeight", false)
                .or_else(|| theme.own_number_in("DefaultTabs.underlineHeight", false))
                .unwrap_or(HEADER_TAB_UNDERLINE),
            tab_arc: theme
                .own_number_in("ToolWindow.HeaderTab.underlineArc", false)
                .unwrap_or(HEADER_TAB_ARC),
            pill,
            pill_height: HEADER_PILL[usize::from(compact)],
            pill_padding: HEADER_PILL_PADDING,
            pill_arc: HEADER_PILL_ARC,
            pill_gap: HEADER_PILL_GAP,
        }
    }
}

impl StripeMetrics {
    fn of(theme: &UiTheme, compact: bool, islands: bool) -> Self {
        let padding = |side: &str| {
            theme
                .insets_in(
                    &format!("StripeToolbar.Button.{side}StripeIcon.padding"),
                    compact,
                )
                .unwrap_or([STRIPE_ICON_PADDING; 4])
        };
        // Compact Islands stripes are 32 px buttons with 16 px icons (`OBS-0020`).
        let small = compact && islands;
        Self {
            button: if small {
                STRIPE_BUTTON_COMPACT
            } else {
                theme
                    .size_in("StripeToolbar.Button.size", compact)
                    .unwrap_or(STRIPE_BUTTON)
            },
            icon: if small {
                16.0
            } else {
                theme
                    .own_number_in("StripeToolbar.Button.iconSize", compact)
                    .unwrap_or(STRIPE_ICON)
            },
            left_padding: padding("left"),
            right_padding: padding("right"),
            radius: theme
                .own_number_in("Button.ToolWindow.arc", compact)
                .unwrap_or(STRIPE_BUTTON_ARC[usize::from(compact)])
                / 2.0,
        }
    }
}

impl ToolbarMetrics {
    fn of(theme: &UiTheme) -> Self {
        Self {
            button: theme
                .size_in("MainToolbar.Button.size", false)
                .map_or(TOOLBAR_BUTTON, |s| s[0]),
            icon: theme
                .own_number_in("MainToolbar.Button.iconSize", false)
                .unwrap_or(TOOLBAR_ICON),
            radius: theme
                .own_number_in("MainToolbar.Button.arc", false)
                .unwrap_or(TOOLBAR_ARC)
                / 2.0,
            dropdown_insets: theme
                .insets("MainToolbar.Dropdown.borderInsets")
                .unwrap_or(TOOLBAR_DROPDOWN_INSETS),
            dropdown_max_width: theme
                .own_number_in("MainToolbar.Dropdown.maxWidth", false)
                .unwrap_or(TOOLBAR_DROPDOWN_MAX_WIDTH),
        }
    }
}

impl StatusMetrics {
    fn of(theme: &UiTheme, compact: bool, islands: bool) -> Self {
        let widget = theme
            .insets("StatusBar.Widget.widgetInsets")
            .map_or(STATUS_WIDGET_INSETS, |i| [i[0], i[1]]);
        // Islands: the bar fills the room below the islands, 32 px (compact 26), and the hover
        // background is 22 px (compact 20) high, centred (`OBS-0020`).
        let (height, widget, hover) = if islands {
            let h = STATUS_HEIGHT_ISLANDS[usize::from(compact)];
            let v = if compact { 3.0 } else { 5.0 };
            (h, [(h - UI_LINE) / 2.0, widget[1]], Some([v, 4.0, v, 4.0]))
        } else {
            (UI_LINE + 2.0 * widget[0], widget, None)
        };
        Self {
            height,
            padding_x: STATUS_PADDING_X,
            widget_insets: widget,
            hover_insets: hover.unwrap_or_else(|| {
                theme
                    .insets("StatusBar.Widget.hoverInsets")
                    .unwrap_or(STATUS_HOVER_INSETS)
            }),
            hover_radius: STATUS_HOVER_CORNER,
            top_border: theme
                .own_number_in("StatusBar.topBorderWidth", false)
                .unwrap_or(1.0),
        }
    }
}

impl IslandMetrics {
    /// The island values of `theme`, compact or not.
    pub fn of(theme: &UiTheme, compact: bool) -> Self {
        let border = theme
            .own_number_in("Island.borderWidth", compact)
            .unwrap_or(0.0);
        let islands = border > 0.0;
        let mode = usize::from(compact);
        Self {
            radius: theme.own_number_in("Island.arc", compact).unwrap_or(0.0) / 2.0,
            gap: (border - 1.0).max(0.0),
            edge: if islands { ISLAND_EDGE[mode] } else { 0.0 },
            top: if islands { ISLAND_TOP[mode] } else { 0.0 },
        }
    }
}

/// Main toolbar height in the normal and the compact mode (intellij-community
/// `CustomWindowHeaderUtil`: `HEADER_HEIGHT_NORMAL`, `HEADER_HEIGHT_COMPACT`).
pub const TOOLBAR_HEIGHT: [f32; 2] = [40.0, 32.0];

impl ThemeMetrics {
    /// The metrics of `theme`.
    pub fn of(theme: &UiTheme) -> Self {
        Self::of_density(theme, false)
    }

    /// The metrics of `theme`, compact or not (`SPEC-0165`): in compact mode the theme's
    /// `….compact` values (tree and list rows, tree border, tab insets) and the lower toolbar.
    pub fn of_density(theme: &UiTheme, compact: bool) -> Self {
        let border = theme.insets_in("Tree.border", compact).unwrap_or([
            4.0,
            TREE_ROW_MARGIN_X,
            4.0,
            TREE_ROW_MARGIN_X,
        ]);
        let tab_bar_height = theme
            .number("TabbedPane.tabHeight")
            .unwrap_or(TAB_BAR_HEIGHT);
        let height_insets = theme.insets("EditorTabs.tabInsets");
        let insets = theme.insets_in("EditorTabs.tabInsets", compact);
        let pill = height_insets.filter(|i| i[0] < 0.0);
        // Compact Islands tabs are 24 px pills in a 32 px bar (`OBS-0020`).
        let small = compact && pill.is_some();
        let tab_bar_height = if small { 32.0 } else { tab_bar_height };
        let islands = IslandMetrics::of(theme, compact);
        let has_islands = islands.edge > 0.0;
        let tree_row_height = theme.number_in("Tree.rowHeight", compact).unwrap_or(24.0);
        Self {
            tree_row_height,
            tree_indent: TREE_INDENT,
            tree_margin: [border[1], border[3]],
            tree_selection_corner: TREE_SELECTION_CORNER,
            tab_bar_height,
            tab_style: if pill.is_some() {
                TabStyle::Pill
            } else {
                TabStyle::Underline
            },
            tab_height: if small {
                24.0
            } else {
                pill.map_or(tab_bar_height, |i| tab_bar_height + i[0] + i[2])
            },
            tab_padding: insets.map_or([TAB_PADDING_X, TAB_PADDING_X], |i| [i[1], i[3]]),
            tab_arc: theme.number("EditorTabs.underlineArc").unwrap_or(4.0),
            tab_underline_height: theme
                .number("EditorTabs.underlineHeight")
                .unwrap_or(TAB_UNDERLINE),
            tab_gaps: [TAB_CONTENT_GAP, TAB_CLOSE_GAP],
            menu_corner: MENU_CORNER,
            menu_item_height: MENU_ITEM_HEIGHT,
            menu_item_padding: [MENU_ITEM_MARGIN_X, MENU_ITEM_PADDING_X],
            menu_selection_corner: MENU_SELECTION_CORNER,
            menu_separator_height: MENU_SEPARATOR_HEIGHT,
            toolbar_height: TOOLBAR_HEIGHT[usize::from(compact)],
            list_row_height: theme
                .number_in("List.rowHeight", compact)
                .unwrap_or(tree_row_height),
            compact,
            header: HeaderMetrics::of(theme, compact, pill.is_some()),
            stripe: StripeMetrics::of(theme, compact, has_islands),
            toolbar: ToolbarMetrics::of(theme),
            status: StatusMetrics::of(theme, compact, has_islands),
            island: islands,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeId;

    /// SPEC-0061 v7 AK-27: the close button where it was, the buttons 20 pt apart.
    #[test]
    fn traffic_lights_follow_the_reference() {
        assert_eq!(TRAFFIC_LIGHT_POSITION, [14.0, 13.0]);
        assert_eq!(TRAFFIC_LIGHT_PITCH, 20.0);
        // The three buttons (14 pt each) fit before the project widget's 83 pt (v6 behaviour 28).
        const { assert!(TRAFFIC_LIGHT_POSITION[0] + 2.0 * TRAFFIC_LIGHT_PITCH + 14.0 < 83.0) };
    }

    #[test]
    fn metrics_follow_jewel_and_the_theme() {
        assert_eq!(tree_indent(0), 4.0);
        // OBS-0020: 19 per level in the reference.
        assert_eq!(tree_indent(2), 42.0);
        assert_eq!(MENU_ITEM_HEIGHT, 24.0);
        let islands = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::IslandsDark));
        assert_eq!(islands.tab_style, TabStyle::Pill);
        assert_eq!(islands.tab_bar_height, 40.0);
        assert_eq!(islands.tab_height, 28.0);
        assert_eq!(islands.tab_padding, [8.0, 8.0]);
        assert_eq!(islands.tree_row_height, 24.0);
        assert_eq!(islands.tree_margin, [12.0, 12.0]);
        // Compact mode (SPEC-0165): the theme's compact values, the lower toolbar.
        let compact = ThemeMetrics::of_density(&UiTheme::for_theme(ThemeId::IslandsDark), true);
        assert_eq!(compact.tree_row_height, 22.0);
        assert_eq!(compact.list_row_height, 22.0);
        assert_eq!(compact.tree_margin, [8.0, 8.0]);
        assert_eq!(compact.tab_padding, [6.0, 4.0]);
        // OBS-0020: compact Islands tabs are 24 px in a 32 px bar.
        assert_eq!((compact.tab_height, compact.tab_bar_height), (24.0, 32.0));
        assert_eq!(compact.toolbar_height, 32.0);
        assert_eq!(islands.toolbar_height, 40.0);
        let dark = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::Dark));
        assert_eq!(dark.tab_style, TabStyle::Underline);
        assert_eq!(dark.tab_height, 40.0);
        assert_eq!(dark.tab_underline_height, 4.0);
    }

    /// SPEC-0061 v5 AK-12, v6 AK-16: title, pills, own buttons, Options and Hide in the 41 px
    /// header; the classic theme keeps full-height underlined tabs.
    #[test]
    fn header_layout_places_tabs_and_actions() {
        let m = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::IslandsDark)).header;
        let l = m.layout(400.0, 30.0, &[40.0, 60.0], 1);
        assert_eq!(l.title, [12.0, 0.0, 30.0, 41.0]);
        // OBS-0020: the first pill 20 after the title, 28 high, 6 from the header's top.
        assert_eq!(
            l.tabs,
            vec![[62.0, 6.0, 56.0, 28.0], [122.0, 6.0, 76.0, 28.0]]
        );
        assert_eq!(
            l.hide,
            [366.0, 7.5, 26.0, 26.0],
            "8 px from the right edge, centred"
        );
        assert_eq!(l.options, [340.0, 7.5, 26.0, 26.0]);
        assert_eq!(l.toolbar, vec![[314.0, 7.5, 26.0, 26.0]]);
        let classic = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::Dark)).header;
        assert!(!classic.pill);
        assert_eq!(
            classic.layout(400.0, 30.0, &[40.0], 0).tabs,
            vec![[58.0, 0.0, 64.0, 41.0]]
        );
        let narrow = m.layout(60.0, 30.0, &[40.0], 0);
        for r in [narrow.options, narrow.hide] {
            assert!(r[0] >= 0.0 && r[0] + r[2] <= 60.0, "{r:?}");
        }
    }

    /// SPEC-0061 v4 AK-6: header, stripe, toolbar, status bar and island values.
    #[test]
    fn window_structure_metrics() {
        let islands = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::IslandsDark));
        let h = &islands.header;
        assert_eq!(h.height, 41.0);
        assert_eq!(h.label_insets, [12.0, 16.0]);
        assert_eq!(h.toolbar_insets, [12.0, 8.0]);
        assert_eq!(h.button, 26.0);
        assert!(h.pill);
        assert_eq!(h.pill_height, 28.0);
        assert_eq!(h.tab_padding, 12.0);
        assert_eq!(h.tab_underline, 3.0);
        assert_eq!(h.tab_arc, 4.0);
        assert_eq!(islands.stripe.button, [37.0, 40.0]);
        assert_eq!(islands.stripe.width(), 37.0);
        assert_eq!(islands.stripe.icon, 20.0);
        assert_eq!(islands.stripe.radius, 6.0);
        assert_eq!(islands.toolbar.button, 30.0);
        assert_eq!(islands.toolbar.icon, 20.0);
        assert_eq!(islands.toolbar.radius, 6.0);
        assert_eq!(islands.toolbar.dropdown_insets, [5.0, 10.0, 5.0, 6.0]);
        assert_eq!(islands.toolbar.dropdown_max_width, 350.0);
        // OBS-0020: 32 px below the islands, a 22 px hover background.
        assert_eq!(islands.status.height, 32.0);
        assert_eq!(islands.status.padding_x, 10.0);
        assert_eq!(islands.status.widget_insets, [8.0, 8.0]);
        assert_eq!(islands.status.hover_insets, [5.0, 4.0, 5.0, 4.0]);
        assert_eq!(islands.status.top_border, 0.0);
        assert_eq!(
            islands.island,
            IslandMetrics {
                radius: 10.0,
                gap: 5.0,
                edge: 3.5,
                top: 3.0,
            }
        );
        let light = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::IslandsLight));
        assert_eq!(light.stripe.button, [37.0, 40.0]);
        assert_eq!(light.island.radius, 10.0);
        // The classic New UI theme: the defaults of the open sources, no islands.
        let dark = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::Dark));
        assert_eq!(dark.stripe.button, [40.0, 40.0]);
        assert_eq!(dark.stripe.left_padding, [5.0; 4]);
        assert_eq!(dark.status.top_border, 1.0);
        assert_eq!(dark.status.height, 28.0);
        assert_eq!(
            dark.island,
            IslandMetrics {
                radius: 0.0,
                gap: 0.0,
                edge: 0.0,
                top: 0.0,
            }
        );
        // Compact Islands (SPEC-0165): smaller islands and stripe corners.
        let compact = ThemeMetrics::of_density(&UiTheme::for_theme(ThemeId::IslandsDark), true);
        assert_eq!(
            compact.island,
            IslandMetrics {
                radius: 8.0,
                gap: 3.0,
                edge: 2.5,
                top: 2.0,
            }
        );
        assert_eq!(compact.stripe.radius, 4.0);
        assert_eq!(compact.stripe.left_padding, [4.0, 5.0, 4.0, 3.0]);
        // OBS-0020: 32 px stripe buttons with a 24 px highlight and 16 px icons, a 32 px header,
        // 26 px status bar.
        assert_eq!(compact.stripe.button, [32.0, 32.0]);
        assert_eq!(compact.stripe.highlight(true), [5.0, 4.0, 24.0, 24.0]);
        assert_eq!(compact.stripe.icon, 16.0);
        assert_eq!(compact.header.height, 32.0);
        assert_eq!(compact.header.pill_height, 24.0);
        assert_eq!(compact.status.height, 26.0);
    }

    /// SPEC-0061 v4 AK-6: the highlight sits inside the button, towards the editor on each side.
    #[test]
    fn stripe_highlight_insets() {
        let s = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::IslandsDark)).stripe;
        assert_eq!(s.left_padding, [5.0, 5.0, 5.0, 2.0]);
        assert_eq!(s.right_padding, [5.0, 2.0, 5.0, 5.0]);
        assert_eq!(s.highlight(true), [5.0, 5.0, 30.0, 30.0]);
        assert_eq!(s.highlight(false), [2.0, 5.0, 30.0, 30.0]);
        let dark = ThemeMetrics::of(&UiTheme::for_theme(ThemeId::Dark)).stripe;
        assert_eq!(dark.highlight(true), [5.0, 5.0, 30.0, 30.0]);
    }
}

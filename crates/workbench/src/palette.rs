//! Colours and metrics of the views, read from the bundled theme and editor scheme (`SPEC-0060`).
//! The views use the chosen theme, `Ferris IDEA Dark` (the brand palette on the Islands layout,
//! ADR-0018) until another is chosen (`SPEC-0060` v5, v6).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

pub use ferris_idea_ui::theme::ThemeId;
use ferris_idea_ui::theme::{ColorScheme, Rgba, TextStyle, UiTheme};

/// Colours the editor and workbench draw with. Editor colours are opaque `0xRRGGBB`; UI colours carry
/// the theme's alpha.
#[derive(Debug)]
pub struct Palette {
    /// Editor background (`TEXT` background).
    pub background: u32,
    /// Whether the theme is dark (icons use their dark variant).
    pub dark: bool,
    /// Plain text (`TEXT` foreground).
    pub text: u32,
    /// `LINE_NUMBERS_COLOR`.
    pub line_number: u32,
    /// `LINE_NUMBER_ON_CARET_ROW_COLOR`.
    pub line_number_current: u32,
    /// `CARET_ROW_COLOR`.
    pub caret_row: u32,
    /// `SELECTION_BACKGROUND`.
    pub selection: u32,
    /// `CARET_COLOR`.
    pub caret: u32,
    /// Window background behind the islands (`MainWindow.background`).
    pub window: Rgba,
    /// Island background (`ToolWindow.background`).
    pub island: Rgba,
    /// Corner radius of islands (`Island.arc` / 2).
    pub island_radius: f32,
    /// Gap between islands (`Island.borderWidth`).
    pub island_gap: f32,
    /// UI text (`*.foreground`).
    pub ui_text: Rgba,
    /// Secondary UI text (`StatusBar.Widget.foreground`, muted).
    pub ui_muted: Rgba,
    /// Dimmed UI text (`*.inactiveForeground`).
    pub ui_dim: Rgba,
    /// Line between editor tabs and text, gutter separator (`EditorTabs.underTabsBorderColor`).
    pub separator: Rgba,
    /// Selected tab (`EditorTabs.underlinedTabBackground`).
    pub tab_selected: Rgba,
    /// Border of the selected tab (`EditorTabs.underlinedBorderColor`).
    pub tab_selected_border: Rgba,
    /// Selected tab while the keyboard focus is elsewhere (`EditorTabs.inactiveUnderlinedTabBackground`;
    /// `SPEC-0061` v19 behaviour 6a).
    pub tab_selected_inactive: Rgba,
    /// Its border (`EditorTabs.inactiveUnderlinedTabBorderColor`).
    pub tab_selected_border_inactive: Rgba,
    /// Tab under the pointer (`EditorTabs.hoverBackground`).
    pub tab_hover: Rgba,
    /// Opacity of tabs that are not selected (`EditorTabs.unselectedAlpha`).
    pub tab_unselected_alpha: f32,
    /// Tree selection while the tree has focus (`Tree.selectionBackground`).
    pub tree_selection: Rgba,
    /// Tree selection without focus (`Tree.selectionInactiveBackground`).
    pub tree_selection_inactive: Rgba,
    /// Row under the pointer (`Tree.hoverBackground`).
    pub tree_hover: Rgba,
    /// `Tree.rowHeight`.
    pub tree_row_height: f32,
    /// Metrics of trees, tabs and menus for the theme (`SPEC-0154`).
    pub metrics: ferris_idea_ui::metrics::ThemeMetrics,
    /// `EditorTabs.underlineColor`.
    pub tab_underline: Rgba,
    /// Toolbar buttons under the pointer (`MainToolbar.Dropdown.transparentHoverBackground`).
    pub toolbar_hover: Rgba,
    /// Selected tool window button (`ToolWindow.Button.selectedBackground`).
    pub stripe_selected: Rgba,
    /// Button of a tool window shown without the focus: the window colour 16 % towards white
    /// (dark) or 12.5 % towards black (light), as measured in the reference (`SPEC-0061` v6
    /// behaviour 27, OBS-0020).
    pub stripe_shown: Rgba,
    /// Underline of the selected header tab while its tool window has the focus
    /// (`ToolWindow.HeaderTab.underlineColor`, SPEC-0061 v4 behaviour 10).
    pub header_tab_underline: Rgba,
    /// The same without the focus (`ToolWindow.HeaderTab.inactiveUnderlineColor`).
    pub header_tab_underline_inactive: Rgba,
    /// Header tab under the pointer (`ToolWindow.HeaderTab.hoverInactiveBackground`).
    pub header_tab_hover: Rgba,
    /// Line above the status bar (`StatusBar.borderColor`, SPEC-0061 v4 behaviour 15).
    pub status_border: Rgba,
    /// Background of popups (`Popup.background`).
    pub popup: Rgba,
    /// Border of popups (`Popup.borderColor`).
    pub popup_border: Rgba,
    /// Tooltip box, border, text and shortcut text (`ToolTip.*`, `SPEC-0294` behaviour 8).
    pub tooltip: ferris_idea_ui::tooltip::TooltipColors,
    /// The theme's accent: primary buttons, checked boxes, the project avatar
    /// (`Button.default.startBackground`; `SPEC-0060` v6).
    pub accent: Rgba,
    /// Text and marks drawn on [`Palette::accent`] (`Button.default.foreground`).
    pub accent_text: Rgba,
    /// Links (`Link.activeForeground`).
    pub link: Rgba,
    /// Border of a focused or switched-on control (`*.focusColor`).
    pub focus: Rgba,
    /// Error text such as a failed search or commit (`Label.errorForeground`).
    pub error_text: Rgba,
    /// Success marks such as the run arrow of a SQL console (`Label.successForeground`).
    pub success_text: Rgba,
    /// The 16 ANSI colours of the console (`CONSOLE_*_OUTPUT`), black to bright white.
    pub console_ansi: [u32; 16],
    /// The same 16 colours as text on the terminal's background and on islands: each as the
    /// scheme has it when it reads there, else moved towards the text colour until it does
    /// (`SPEC-0060` v10 behaviour 16). Backgrounds, fills and lines use [`Palette::console_ansi`].
    pub console_ansi_text: [u32; 16],
    /// Console background (`CONSOLE_BACKGROUND_KEY`).
    pub console_background: u32,
    /// Console text (`CONSOLE_NORMAL_OUTPUT`).
    pub console_text: u32,
    /// Row backgrounds of added, deleted, changed and conflicting lines in every diff and merge
    /// view (`DIFF_INSERTED`, `DIFF_DELETED`, `DIFF_MODIFIED`, `DIFF_CONFLICT`).
    pub diff_inserted: u32,
    /// See [`Palette::diff_inserted`].
    pub diff_deleted: u32,
    /// See [`Palette::diff_inserted`].
    pub diff_modified: u32,
    /// See [`Palette::diff_inserted`].
    pub diff_conflict: u32,
    /// The project gradient of the main toolbar in the Islands themes (`SPEC-0061` v14 behaviour
    /// 28a); `None` in the other themes, which tint the toolbar with the accent.
    pub project_gradient: Option<ProjectGradient>,
    status_text: [Rgba; 4],
    problem_marks: [u32; 4],
    notes: Vec<String>,
    scheme: ColorScheme,
    captures: Mutex<HashMap<String, TextStyle>>,
}

/// Index of the chosen theme in [`ThemeId::ALL`], [`ThemeId::DEFAULT`] (index 0) until one is chosen.
/// The theme is a setting of the application, as in the reference: every window draws with it.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn index(id: ThemeId) -> usize {
    ThemeId::ALL.iter().position(|t| *t == id).unwrap_or(0)
}

/// The theme the views draw with.
pub fn current_theme() -> ThemeId {
    ThemeId::ALL[CURRENT.load(Ordering::Relaxed) % ThemeId::ALL.len()]
}

/// Makes `id` the theme the views draw with; windows show it once they are drawn again.
pub fn set_current_theme(id: ThemeId) {
    CURRENT.store(index(id), Ordering::Relaxed);
}

/// Compact mode (`SPEC-0165`), an application setting like the theme.
static COMPACT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the views draw with the compact metrics.
pub fn compact_mode() -> bool {
    COMPACT.load(Ordering::Relaxed)
}

/// Switches compact mode; windows show it once they are drawn again.
pub fn set_compact_mode(on: bool) {
    COMPACT.store(on, Ordering::Relaxed);
}

/// The bundled editor colour schemes the Appearance page offers apart from the theme
/// (`SPEC-0337` behaviour 4): each scheme's name and the theme it comes with. Islands Light's own
/// scheme (`Light` with plain method declarations, `SPEC-0060` v13 behaviour 19) is not offered
/// apart from its theme.
pub const EDITOR_SCHEMES: [(&str, ThemeId); 5] = [
    ("Ferris IDEA Dark", ThemeId::FerrisIdeaDark),
    ("Ferris IDEA Light", ThemeId::FerrisIdeaLight),
    ("Islands Dark", ThemeId::IslandsDark),
    ("Dark", ThemeId::Dark),
    ("Light", ThemeId::Light),
];

/// The editor scheme chosen apart from the theme: 0 for the theme's own, else the index in
/// [`EDITOR_SCHEMES`] plus one. An application setting like the theme.
static SCHEME: AtomicUsize = AtomicUsize::new(0);

/// The theme whose editor scheme `name` is; `None` for an empty or unknown name (the theme's own
/// scheme).
pub fn scheme_of(name: &str) -> Option<ThemeId> {
    EDITOR_SCHEMES
        .iter()
        .find(|(n, _)| *n == name.trim())
        .map(|(_, t)| *t)
}

/// The editor scheme chosen apart from the theme, as the theme it comes with.
pub fn editor_scheme() -> Option<ThemeId> {
    match SCHEME.load(Ordering::Relaxed) {
        0 => None,
        i => EDITOR_SCHEMES.get(i - 1).map(|(_, t)| *t),
    }
}

/// Makes the editors draw with the scheme named `name` (empty or unknown: the theme's own);
/// windows show it once they are drawn again.
pub fn set_editor_scheme(name: &str) {
    let i = EDITOR_SCHEMES
        .iter()
        .position(|(n, _)| *n == name.trim())
        .map_or(0, |i| i + 1);
    SCHEME.store(i, Ordering::Relaxed);
}

/// Attribute keys of the problem severities, in the order of [`Palette::problem_mark`].
const PROBLEM_KEYS: [&str; 4] = [
    "ERRORS_ATTRIBUTES",
    "WARNING_ATTRIBUTES",
    "WEAK_WARNING_ATTRIBUTES",
    "INFO_ATTRIBUTES",
];

const fn opaque(rgb: u32) -> Rgba {
    Rgba { rgb, alpha: 255 }
}

/// The project colour of the main toolbar as an Islands theme gives it (`SPEC-0061` v14 behaviour
/// 28a): each of the nine `RecentProject.ColorN.MainToolbarGradientStart` colours mixed 85 % into
/// the window colour; the colour rises from the window colour at the left edge to the mix at the
/// project widget, falls back to the window colour over `width`, and fades towards the window
/// colour downwards over `height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectGradient {
    /// The mixed colour of each of the nine project colours.
    pub colours: [u32; 9],
    /// `RecentProject.MainToolbarGradient.width`.
    pub width: f32,
    /// `RecentProject.MainToolbarGradient.height`.
    pub height: f32,
}

impl ProjectGradient {
    /// Share of the project colour that stays in the mix (`SPEC-0061` v14 behaviour 28a).
    pub const SHARE: f32 = 0.85;

    /// The gradient of `theme` drawn on `window`, for the Islands themes only.
    fn of(theme: &UiTheme, window: Rgba) -> Option<Self> {
        if !matches!(theme.name.as_str(), "Islands Dark" | "Islands Light") {
            return None;
        }
        let mut colours = [0u32; 9];
        for (i, slot) in colours.iter_mut().enumerate() {
            let c = theme.color(&format!(
                "RecentProject.Color{}.MainToolbarGradientStart",
                i + 1
            ))?;
            *slot = ferris_idea_ui::theme::mix(
                window.rgb,
                c.rgb,
                Self::SHARE * f32::from(c.alpha) / 255.0,
            );
        }
        Some(Self {
            colours,
            width: theme
                .number("RecentProject.MainToolbarGradient.width")
                .unwrap_or(700.0),
            height: theme
                .number("RecentProject.MainToolbarGradient.height")
                .unwrap_or(200.0),
        })
    }

    /// The colour of the project at `path`: one of the nine, the same for the same path.
    pub fn colour_for(&self, path: &str) -> u32 {
        // FNV-1a: stable across runs and builds, unlike the standard hasher.
        let hash = path.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        self.colours[(hash % 9) as usize]
    }
}

/// A status whose colour is drawn as text (`SPEC-0060` v10 behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// A failure.
    Error,
    /// Something to look at.
    Warning,
    /// A neutral remark.
    Info,
    /// Something that worked.
    Success,
}

/// Contrast every status text reaches on islands and popups (WCAG 2.2 AA).
pub const TEXT_CONTRAST: f32 = 4.5;
/// Contrast every problem mark (wave, glyph) reaches on the editor and the islands (WCAG 2.2
/// non-text contrast).
pub const MARK_CONTRAST: f32 = 3.0;

/// Reads the colours of one theme and writes down every colour it had to make up.
struct Reader<'a> {
    theme: &'a UiTheme,
    notes: Vec<String>,
}

impl Reader<'_> {
    /// The first of `keys` the theme defines, else `default` with a note (`SPEC-0060` v10
    /// behaviour 15).
    fn ui(&mut self, keys: &[&str], default: Rgba) -> Rgba {
        if let Some(c) = keys.iter().find_map(|k| self.theme.color(k)) {
            return c;
        }
        let name = &self.theme.name;
        let key = keys.first().copied().unwrap_or_default();
        let unknown = keys
            .iter()
            .find_map(|k| Some((k, self.theme.unresolved(k)?)));
        self.notes.push(match unknown {
            Some((k, value)) => format!(
                "theme `{name}`: `{k}` names the unknown colour `{value}`; a colour derived \
                 from the theme's background and text is used"
            ),
            None => format!(
                "theme `{name}`: `{key}` is not set; a colour derived from the theme's \
                 background and text is used"
            ),
        });
        default
    }
}

impl Palette {
    /// Palette of a bundled theme and its editor scheme.
    pub fn bundled(id: ThemeId) -> Self {
        Self::bundled_in(id, false)
    }

    /// The palette of a bundled theme with the normal or the compact metrics (`SPEC-0165`).
    pub fn bundled_in(id: ThemeId, compact: bool) -> Self {
        Self::from_parts(UiTheme::for_theme(id), ColorScheme::for_theme(id), compact)
    }

    /// The palette of `theme` with the editor scheme `scheme`. Every colour the views draw with
    /// has a value: a colour the files leave out or spell wrongly is derived from the theme's own
    /// background, text and accent, so a light theme never gets a dark default, and is written to
    /// [`Palette::notes`] (`SPEC-0060` v10 behaviour 15).
    pub fn from_parts(theme: UiTheme, scheme: ColorScheme, compact: bool) -> Self {
        use ferris_idea_ui::theme::{mix, readable};
        let text = scheme.attributes("TEXT");
        let dark = theme.dark;
        let mut notes: Vec<String> = theme.notes().to_vec();
        notes.extend(scheme.notes());
        let mut plain = |what: &str, value: Option<u32>, default: u32| {
            value.unwrap_or_else(|| {
                notes.push(format!(
                    "scheme `{}`: `TEXT` has no {what}; the default of a {} scheme is used",
                    scheme.name,
                    if dark { "dark" } else { "light" }
                ));
                default
            })
        };
        let fg = plain(
            "FOREGROUND",
            text.foreground,
            if dark { 0xbcbec4 } else { 0x080808 },
        );
        let bg = plain(
            "BACKGROUND",
            text.background,
            if dark { 0x1e1f22 } else { 0xffffff },
        );
        let c = |key: &str, default: u32| scheme.color(key).unwrap_or(default);
        let mut r = Reader {
            theme: &theme,
            notes,
        };
        // No note for these three: the reference's themes leave them to the editor scheme.
        let island = theme
            .color("ToolWindow.background")
            .or_else(|| theme.color("*.background"))
            .unwrap_or(opaque(bg));
        let window = theme.color("MainWindow.background").unwrap_or(island);
        let ui_text = theme.color("*.foreground").unwrap_or(opaque(fg));
        // Derived defaults: the island towards the text, or towards the accent.
        let tint = |share: f32| opaque(mix(island.rgb, ui_text.rgb, share));
        let blue = opaque(0x3574f0);
        let accent = r.ui(&["Button.default.startBackground", "*.focusColor"], blue);
        let wash = opaque(mix(island.rgb, accent.rgb, 0.3));
        let toward = |to: f32, share: f32| {
            let mix = |shift: u32| {
                let v = ((window.rgb >> shift) & 0xff) as f32;
                ((v + (to - v) * share).round() as u32) << shift
            };
            opaque(mix(16) | mix(8) | mix(0))
        };
        let stripe_shown = if dark {
            toward(255.0, 0.16)
        } else {
            toward(0.0, 0.125)
        };
        let popup = r.ui(&["Popup.background", "PopupMenu.background"], island);
        let popup_border = r.ui(&["Popup.borderColor", "Popup.Border.color"], tint(0.2));
        let link = r.ui(
            &["Link.activeForeground"],
            opaque(readable(accent.rgb, island.rgb, ui_text.rgb, TEXT_CONTRAST)),
        );
        let (red, amber, green) = if dark {
            (0xde6a66, 0xf2c55c, 0x5fb865)
        } else {
            (0xdb3b4b, 0xa46704, 0x338555)
        };
        // Labels in the error and success colours stand on islands and read there (behaviour 16).
        let label = |c: Rgba| opaque(readable(c.rgb, island.rgb, ui_text.rgb, TEXT_CONTRAST));
        let error_text = label(r.ui(&["Label.errorForeground"], opaque(red)));
        let success_text = label(r.ui(&["Label.successForeground"], opaque(green)));
        // The terminal's colours: as the scheme has them for backgrounds and fills, and each as
        // text on the terminal's background and on islands (behaviour 16).
        let console_background = c("CONSOLE_BACKGROUND_KEY", bg);
        let console_text = scheme
            .attributes("CONSOLE_NORMAL_OUTPUT")
            .foreground
            .unwrap_or(fg);
        let console_ansi = [
            "BLACK",
            "RED",
            "GREEN",
            "YELLOW",
            "BLUE",
            "MAGENTA",
            "CYAN",
            "GRAY",
            "DARKGRAY",
            "RED_BRIGHT",
            "GREEN_BRIGHT",
            "YELLOW_BRIGHT",
            "BLUE_BRIGHT",
            "MAGENTA_BRIGHT",
            "CYAN_BRIGHT",
            "WHITE",
        ]
        .map(|n| {
            scheme
                .attributes(&format!("CONSOLE_{n}_OUTPUT"))
                .foreground
                .unwrap_or(fg)
        });
        let console_ansi_text = console_ansi.map(|colour| {
            let in_terminal = readable(colour, console_background, console_text, TEXT_CONTRAST);
            readable(in_terminal, island.rgb, ui_text.rgb, TEXT_CONTRAST)
        });
        // Status colours as text: readable on islands and on popups (behaviour 16).
        let as_text = |colour: u32| {
            let on_island = readable(colour, island.rgb, ui_text.rgb, TEXT_CONTRAST);
            opaque(readable(on_island, popup.rgb, ui_text.rgb, TEXT_CONTRAST))
        };
        let effect = |key: &str| {
            let a = scheme.attributes(key);
            a.effect.or(a.stripe)
        };
        let status_text = [
            as_text(error_text.rgb),
            as_text(effect("WARNING_ATTRIBUTES").unwrap_or(amber)),
            as_text(effect("INFO_ATTRIBUTES").unwrap_or(link.rgb)),
            as_text(success_text.rgb),
        ];
        let mark_default = |key: &str| match key {
            "ERRORS_ATTRIBUTES" => red,
            "WARNING_ATTRIBUTES" => amber,
            _ => mix(bg, fg, 0.5),
        };
        let problem_marks = PROBLEM_KEYS.map(|key| {
            let colour = effect(key).unwrap_or_else(|| mark_default(key));
            let on_editor = readable(colour, bg, fg, MARK_CONTRAST);
            readable(on_editor, island.rgb, ui_text.rgb, MARK_CONTRAST)
        });
        let diff = |key: &str, towards: u32| {
            scheme
                .attributes(key)
                .background
                .unwrap_or_else(|| mix(bg, towards, if dark { 0.3 } else { 0.25 }))
        };
        let tab_selected = r.ui(&["EditorTabs.underlinedTabBackground"], island);
        let tab_selected_border = r.ui(
            &[
                "EditorTabs.underlinedBorderColor",
                "EditorTabs.underlineColor",
            ],
            accent,
        );
        Self {
            background: bg,
            dark,
            stripe_shown,
            text: fg,
            line_number: c("LINE_NUMBERS_COLOR", mix(bg, fg, 0.5)),
            line_number_current: c("LINE_NUMBER_ON_CARET_ROW_COLOR", fg),
            caret_row: c("CARET_ROW_COLOR", bg),
            selection: c("SELECTION_BACKGROUND", mix(bg, accent.rgb, 0.35)),
            caret: c("CARET_COLOR", fg),
            window,
            island,
            // Compact islands take the theme's compact values (SPEC-0061 v4 behaviour 15a).
            island_radius: ferris_idea_ui::metrics::IslandMetrics::of(&theme, compact).radius,
            island_gap: ferris_idea_ui::metrics::IslandMetrics::of(&theme, compact).gap,
            ui_text,
            ui_muted: r.ui(&["StatusBar.Widget.foreground"], ui_text),
            ui_dim: r.ui(&["*.inactiveForeground"], tint(0.6)),
            separator: r.ui(
                &["EditorTabs.underTabsBorderColor", "Borders.color"],
                tint(0.15),
            ),
            tab_selected,
            tab_selected_border,
            // Only the pill tabs of the Islands themes use these; themes without them keep the
            // focused colours, without a note (SPEC-0061 v19 behaviour 6a).
            tab_selected_inactive: theme
                .color("EditorTabs.inactiveUnderlinedTabBackground")
                .unwrap_or(tab_selected),
            tab_selected_border_inactive: theme
                .color("EditorTabs.inactiveUnderlinedTabBorderColor")
                .unwrap_or(tab_selected_border),
            tab_hover: r.ui(&["EditorTabs.hoverBackground"], tint(0.08)),
            tab_unselected_alpha: theme.number("EditorTabs.unselectedAlpha").unwrap_or(1.0),
            tree_selection: r.ui(&["Tree.selectionBackground"], wash),
            tree_selection_inactive: r.ui(&["Tree.selectionInactiveBackground"], tint(0.12)),
            tree_hover: r.ui(&["Tree.hoverBackground"], tint(0.08)),
            tree_row_height: theme.number_in("Tree.rowHeight", compact).unwrap_or(24.0),
            metrics: ferris_idea_ui::metrics::ThemeMetrics::of_density(&theme, compact),
            tab_underline: r.ui(&["EditorTabs.underlineColor"], accent),
            toolbar_hover: r.ui(
                &[
                    "MainToolbar.Dropdown.transparentHoverBackground",
                    "*.hoverBackground",
                ],
                tint(0.08),
            ),
            popup,
            popup_border,
            tooltip: {
                let background = r.ui(&["ToolTip.background", "Popup.background"], popup);
                // Text of a tooltip without colours of its own: the text that reads on its box.
                let readable_text =
                    ferris_idea_ui::theme::contrast(ui_text.rgb, background.rgb) >= TEXT_CONTRAST;
                let on_box = if readable_text { ui_text } else { island };
                ferris_idea_ui::tooltip::TooltipColors {
                    background,
                    border: r.ui(&["ToolTip.borderColor", "Popup.borderColor"], popup_border),
                    text: r.ui(&["ToolTip.foreground"], on_box),
                    shortcut: r.ui(
                        &["ToolTip.shortcutForeground", "ToolTip.infoForeground"],
                        on_box,
                    ),
                }
            },
            accent,
            accent_text: r.ui(&["Button.default.foreground"], opaque(0xffffff)),
            link,
            focus: r.ui(&["*.focusColor", "Component.focusColor"], accent),
            error_text,
            success_text,
            status_text,
            problem_marks,
            diff_inserted: diff("DIFF_INSERTED", 0x4caf50),
            diff_deleted: diff("DIFF_DELETED", 0x808080),
            diff_modified: diff("DIFF_MODIFIED", 0x3d7edb),
            diff_conflict: diff("DIFF_CONFLICT", 0xd9534f),
            project_gradient: ProjectGradient::of(&theme, window),
            stripe_selected: r.ui(&["ToolWindow.Button.selectedBackground"], accent),
            header_tab_underline: r.ui(
                &[
                    "ToolWindow.HeaderTab.underlineColor",
                    "DefaultTabs.underlineColor",
                ],
                accent,
            ),
            header_tab_underline_inactive: r.ui(
                &[
                    "ToolWindow.HeaderTab.inactiveUnderlineColor",
                    "DefaultTabs.inactiveUnderlineColor",
                ],
                tint(0.4),
            ),
            header_tab_hover: r.ui(
                &[
                    "ToolWindow.HeaderTab.hoverInactiveBackground",
                    "ToolWindow.HeaderTab.hoverBackground",
                    "*.hoverBackground",
                ],
                tint(0.08),
            ),
            status_border: r.ui(&["StatusBar.borderColor"], tint(0.15)),
            console_ansi,
            console_ansi_text,
            console_background,
            console_text,
            notes: r.notes,
            scheme,
            captures: Mutex::new(HashMap::new()),
        }
    }

    /// A status colour as text, readable on islands and on popups (behaviour 16).
    pub fn status_text(&self, status: Status) -> Rgba {
        self.status_text[status as usize]
    }

    /// The colour of a problem's wave, glyph or mark for the attribute key of its severity
    /// (`ERRORS_ATTRIBUTES`, `WARNING_ATTRIBUTES`, `WEAK_WARNING_ATTRIBUTES`, `INFO_ATTRIBUTES`):
    /// the scheme's effect colour, else its stripe colour, else a default of the theme's kind,
    /// visible on the editor and on islands (behaviour 16). Another key gives the error's.
    pub fn problem_mark(&self, key: &str) -> u32 {
        let at = PROBLEM_KEYS.iter().position(|k| *k == key).unwrap_or(0);
        self.problem_marks[at]
    }

    /// What the theme's and the scheme's files left out or spelled wrongly, and what was used
    /// instead (behaviour 15); empty for a complete theme.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// The palette of a bundled theme, read once.
    pub fn for_theme(id: ThemeId) -> &'static Palette {
        Self::for_theme_in(id, false)
    }

    /// The palette of a bundled theme with the normal or compact metrics, read once.
    pub fn for_theme_in(id: ThemeId, compact: bool) -> &'static Palette {
        static PALETTES: [[OnceLock<Palette>; 2]; ThemeId::ALL.len()] =
            [const { [const { OnceLock::new() }; 2] }; ThemeId::ALL.len()];
        PALETTES[index(id)][usize::from(compact)].get_or_init(|| Palette::bundled_in(id, compact))
    }

    /// The palette of theme `id` with the editor scheme of the theme `scheme` (`SPEC-0337`
    /// behaviour 4), read once; the theme's own palette when `scheme` is `None` or its own.
    pub fn for_parts(id: ThemeId, scheme: Option<ThemeId>, compact: bool) -> &'static Palette {
        const N: usize = ThemeId::ALL.len();
        static MIXED: [[[OnceLock<Palette>; 2]; N]; N] =
            [const { [const { [const { OnceLock::new() }; 2] }; N] }; N];
        match scheme {
            Some(s) if s != id => {
                MIXED[index(id)][index(s)][usize::from(compact)].get_or_init(|| {
                    Palette::from_parts(UiTheme::for_theme(id), ColorScheme::for_theme(s), compact)
                })
            }
            _ => Self::for_theme_in(id, compact),
        }
    }

    /// The palette the views draw with: the chosen theme's with the chosen editor scheme,
    /// compact or not.
    pub fn current() -> &'static Palette {
        Self::for_parts(current_theme(), editor_scheme(), compact_mode())
    }

    /// An editor colour of the scheme, e.g. `FILESTATUS_MODIFIED`.
    pub fn scheme_color(&self, key: &str) -> Option<u32> {
        self.scheme.color(key)
    }

    /// Foreground of an attribute key of the scheme, e.g. `CONSOLE_ERROR_OUTPUT`.
    pub fn scheme_foreground(&self, key: &str) -> Option<u32> {
        self.scheme.attributes(key).foreground
    }

    /// Effect colour (underline, wave) of an attribute key, e.g. `ERRORS_ATTRIBUTES`.
    pub fn scheme_effect(&self, key: &str) -> Option<u32> {
        self.scheme.attributes(key).effect
    }

    /// Error-stripe colour of an attribute key of the scheme (SPEC-0070 v5 behaviour 8).
    pub fn scheme_stripe(&self, key: &str) -> Option<u32> {
        self.scheme.attributes(key).stripe
    }

    /// Background of an attribute key of the scheme, e.g. `TEXT_SEARCH_RESULT_ATTRIBUTES`.
    pub fn scheme_background(&self, key: &str) -> Option<u32> {
        self.scheme.attributes(key).background
    }

    /// Style of a syntax capture, cached per capture name.
    pub fn capture(&self, kind: &str) -> TextStyle {
        let mut cache = self.captures.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .entry(kind.to_owned())
            .or_insert_with(|| self.scheme.capture_style(kind))
            .clone()
    }
}

impl Palette {
    /// Style of an attribute key over the plain text style, cached (`SPEC-0290` behaviour 4).
    pub fn key_style(&self, key: &str) -> TextStyle {
        let mut cache = self.captures.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .entry(format!("\u{0}{key}"))
            .or_insert_with(|| {
                let text = self.scheme.attributes("TEXT");
                let s = self.scheme.attributes(key);
                TextStyle {
                    foreground: s.foreground.or(text.foreground),
                    ..s
                }
            })
            .clone()
    }
}

/// Colour of a terminal palette index: 0–15 from the scheme, 16–231 the xterm 6×6×6 cube, 232–255
/// the grey ramp (`SPEC-0062` behaviour 2).
pub fn xterm_color(index: u8, ansi: &[u32; 16]) -> u32 {
    match index {
        0..=15 => ansi[usize::from(index)],
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0u32 } else { 55 + 40 * u32::from(v) };
            (level(i / 36) << 16) | (level((i / 6) % 6) << 8) | level(i % 6)
        }
        _ => {
            let g = 8 + 10 * u32::from(index - 232);
            (g << 16) | (g << 8) | g
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0337 AK-4: an editor scheme chosen apart from the theme gives the editors the
    /// scheme's colours and the rest of the window the theme's; "Same as theme" is the theme's own
    /// palette, and every bundled scheme is offered once under its own name.
    #[test]
    fn editor_scheme_over_the_theme() {
        let names: Vec<&str> = EDITOR_SCHEMES.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ferris_idea_core::EDITOR_SCHEMES,
            "the names MCP accepts"
        );
        for (name, theme) in EDITOR_SCHEMES {
            assert_eq!(ColorScheme::for_theme(theme).name, name);
            assert_eq!(scheme_of(name), Some(theme));
        }
        assert_eq!(scheme_of(""), None);
        assert_eq!(scheme_of("Solarized"), None);
        let own = Palette::for_theme(ThemeId::FerrisIdeaDark);
        assert!(std::ptr::eq(
            Palette::for_parts(ThemeId::FerrisIdeaDark, None, false),
            own
        ));
        assert!(std::ptr::eq(
            Palette::for_parts(
                ThemeId::FerrisIdeaDark,
                Some(ThemeId::FerrisIdeaDark),
                false
            ),
            own
        ));
        // Islands Light's own scheme differs from `Light` in method declarations only
        // (SPEC-0060 v13 behaviour 19), so `Light` over it is a scheme of its own.
        let islands = Palette::for_theme(ThemeId::IslandsLight);
        let over = Palette::for_parts(ThemeId::IslandsLight, Some(ThemeId::Light), false);
        assert!(!std::ptr::eq(over, islands));
        assert_eq!(over.text, islands.text);
        assert_ne!(
            over.key_style("METHOD_DECLARATION_ATTRIBUTES").foreground,
            islands
                .key_style("METHOD_DECLARATION_ATTRIBUTES")
                .foreground
        );
        let mixed = Palette::for_parts(ThemeId::FerrisIdeaDark, Some(ThemeId::Light), false);
        let light = Palette::for_theme(ThemeId::Light);
        assert_eq!(mixed.text, light.text, "the editors draw with the scheme");
        assert_eq!(mixed.caret_row, light.caret_row);
        assert_eq!(mixed.ui_text, own.ui_text, "the window keeps the theme");
        assert_eq!(mixed.tree_selection, own.tree_selection);
        assert!(std::ptr::eq(
            mixed,
            Palette::for_parts(ThemeId::FerrisIdeaDark, Some(ThemeId::Light), false)
        ));
    }

    /// Every text-and-background pair the views draw with a palette, with the contrast it must
    /// reach (`SPEC-0060` v10 behaviour 16): 4.5 for text, 3.0 for marks.
    fn pairs(p: &Palette) -> Vec<(String, u32, u32, f32)> {
        use ferris_idea_ui::theme::over;
        let island = p.island.rgb;
        let on = |c: Rgba, under: u32| over(c.rgb, c.alpha, under);
        let popup = on(p.popup, island);
        let window = on(p.window, island);
        let mut out: Vec<(String, u32, u32, f32)> = Vec::new();
        let mut text =
            |what: &str, fg: u32, bg: u32| out.push((what.into(), fg, bg, TEXT_CONTRAST));
        for (name, bg) in [
            ("islands", island),
            ("the window", window),
            ("popups", popup),
        ] {
            text(&format!("UI text on {name}"), p.ui_text.rgb, bg);
            text(&format!("muted text on {name}"), p.ui_muted.rgb, bg);
        }
        for (name, bg) in [("islands", island), ("popups", popup)] {
            text(&format!("links on {name}"), p.link.rgb, bg);
            for status in [
                Status::Error,
                Status::Warning,
                Status::Info,
                Status::Success,
            ] {
                text(
                    &format!("{status:?} status text on {name}"),
                    p.status_text(status).rgb,
                    bg,
                );
            }
        }
        text("error label on islands", p.error_text.rgb, island);
        text("success label on islands", p.success_text.rgb, island);
        text("text on the accent", p.accent_text.rgb, p.accent.rgb);
        for (name, bg) in [
            ("the tree selection", p.tree_selection),
            ("the inactive selection", p.tree_selection_inactive),
            ("a hovered row", p.tree_hover),
            ("the selected tab", p.tab_selected),
            ("a hovered tab", p.tab_hover),
            ("a hovered toolbar button", p.toolbar_hover),
        ] {
            text(&format!("UI text on {name}"), p.ui_text.rgb, on(bg, island));
        }
        let tip = on(p.tooltip.background, island);
        text("tooltip text", p.tooltip.text.rgb, tip);
        text("tooltip shortcut", p.tooltip.shortcut.rgb, tip);
        text("editor text", p.text, p.background);
        text("editor text on the caret row", p.text, p.caret_row);
        text("editor text in a selection", p.text, p.selection);
        for (name, bg) in [
            ("an added row", p.diff_inserted),
            ("a deleted row", p.diff_deleted),
            ("a changed row", p.diff_modified),
            ("a conflicting row", p.diff_conflict),
        ] {
            text(&format!("editor text on {name}"), p.text, bg);
        }
        for capture in [
            "keyword",
            "string",
            "number",
            "comment",
            "comment.documentation",
            "function",
            "type",
            "attribute",
            "string.escape",
            "property",
            "constant",
        ] {
            let fg = p.capture(capture).foreground.unwrap_or(p.text);
            text(&format!("syntax `{capture}`"), fg, p.background);
        }
        text("console text", p.console_text, p.console_background);
        const ANSI: [&str; 16] = [
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "white",
            "bright black",
            "bright red",
            "bright green",
            "bright yellow",
            "bright blue",
            "bright magenta",
            "bright cyan",
            "bright white",
        ];
        for (name, fg) in ANSI.iter().zip(p.console_ansi_text) {
            text(&format!("terminal {name}"), fg, p.console_background);
        }
        for key in PROBLEM_KEYS {
            let mark = p.problem_mark(key);
            out.push((
                format!("{key} mark on the editor"),
                mark,
                p.background,
                MARK_CONTRAST,
            ));
            out.push((
                format!("{key} mark on islands"),
                mark,
                island,
                MARK_CONTRAST,
            ));
        }
        out
    }

    fn violations(id: ThemeId) -> Vec<String> {
        let p = Palette::bundled(id);
        pairs(&p)
            .into_iter()
            .filter_map(|(what, fg, bg, min)| {
                let ratio = ferris_idea_ui::theme::contrast(fg, bg);
                (ratio < min)
                    .then(|| format!("{what}: {fg:06x} on {bg:06x} is {ratio:.2} (< {min})"))
            })
            .collect()
    }

    /// SPEC-0060 v10 AK-15: prints the contrast of every pair of every bundled theme
    /// (`cargo test -p ferris-idea-workbench contrast_table -- --nocapture`).
    /// The brand themes, Ferris IDEA's own, reach the limits in every pair. The reference's themes
    /// are bundled unchanged (behaviour 1), so the pairs their files leave below the limit are
    /// listed here as they are; a pair that gets worse or a new one fails the test.
    #[test]
    fn contrast_table() {
        const KNOWN: [(ThemeId, &[&str]); 6] = [
            (ThemeId::FerrisIdeaDark, &[]),
            (ThemeId::FerrisIdeaLight, &[]),
            (
                ThemeId::IslandsDark,
                &[
                    "editor text on a changed row: bcbec4 on 385570 is 4.18 (< 4.5)",
                    "syntax `comment`: 7a7e85 on 191a1c is 4.27 (< 4.5)",
                    "syntax `comment.documentation`: 5f826b on 191a1c is 4.06 (< 4.5)",
                ],
            ),
            (
                ThemeId::IslandsLight,
                &[
                    "syntax `comment`: 8c8c8c on ffffff is 3.36 (< 4.5)",
                    "syntax `comment.documentation`: 8c8c8c on ffffff is 3.36 (< 4.5)",
                    "syntax `attribute`: 9e880d on ffffff is 3.51 (< 4.5)",
                ],
            ),
            (
                ThemeId::Dark,
                &[
                    "text on the accent: ffffff on 3574f0 is 4.28 (< 4.5)",
                    "tooltip shortcut: 9da0a8 on 393b40 is 4.28 (< 4.5)",
                    "editor text on a changed row: bcbec4 on 385570 is 4.18 (< 4.5)",
                    "syntax `comment`: 7a7e85 on 1e1f22 is 4.04 (< 4.5)",
                    "syntax `comment.documentation`: 5f826b on 1e1f22 is 3.84 (< 4.5)",
                ],
            ),
            (
                ThemeId::Light,
                &[
                    "text on the accent: ffffff on 3574f0 is 4.28 (< 4.5)",
                    "syntax `comment`: 8c8c8c on ffffff is 3.36 (< 4.5)",
                    "syntax `comment.documentation`: 8c8c8c on ffffff is 3.36 (< 4.5)",
                    "syntax `attribute`: 9e880d on ffffff is 3.51 (< 4.5)",
                ],
            ),
        ];
        for (id, known) in KNOWN {
            let found = violations(id);
            println!(
                "== {} ({} pairs)",
                id.name(),
                pairs(&Palette::bundled(id)).len()
            );
            for v in &found {
                println!("  {v}");
            }
            assert_eq!(found, known, "{}", id.name());
        }
        assert_eq!(KNOWN.map(|(id, _)| id), ThemeId::ALL, "every bundled theme");
    }

    fn is_light(rgb: u32) -> bool {
        use ferris_idea_ui::theme::contrast;
        contrast(rgb, 0x000000) > contrast(rgb, 0xffffff)
    }

    /// SPEC-0060 v10 AK-16: every bundled theme colours every surface in its own kind — editor,
    /// tool windows, the window behind them, popups, dialogs (which draw on the popup colour), the
    /// terminal and the rows of the diff views — so a switch reaches all of them and no surface
    /// keeps a colour of the other kind.
    #[test]
    fn every_theme_colours_every_surface_in_its_kind() {
        use ferris_idea_ui::theme::over;
        for id in ThemeId::ALL {
            let p = Palette::bundled(id);
            assert_eq!(p.dark, id.is_dark(), "{id:?}");
            let island = p.island.rgb;
            let surfaces = [
                ("editor", p.background),
                ("caret row", p.caret_row),
                ("selection", p.selection),
                ("tool windows", island),
                ("window", over(p.window.rgb, p.window.alpha, island)),
                (
                    "popups and dialogs",
                    over(p.popup.rgb, p.popup.alpha, island),
                ),
                (
                    "tree selection",
                    over(p.tree_selection.rgb, p.tree_selection.alpha, island),
                ),
                (
                    "inactive selection",
                    over(
                        p.tree_selection_inactive.rgb,
                        p.tree_selection_inactive.alpha,
                        island,
                    ),
                ),
                (
                    "selected tab",
                    over(p.tab_selected.rgb, p.tab_selected.alpha, island),
                ),
                ("terminal", p.console_background),
                ("added rows", p.diff_inserted),
                ("deleted rows", p.diff_deleted),
                ("changed rows", p.diff_modified),
                ("conflicting rows", p.diff_conflict),
            ];
            for (what, colour) in surfaces {
                assert_eq!(is_light(colour), !p.dark, "{id:?}: {what} is {colour:06x}");
            }
            // Text is of the other kind than its surface.
            for (what, colour) in [
                ("editor text", p.text),
                ("UI text", p.ui_text.rgb),
                ("terminal text", p.console_text),
            ] {
                assert_eq!(is_light(colour), p.dark, "{id:?}: {what} is {colour:06x}");
            }
            // The diff rows and the terminal's colours come from the theme's own scheme.
            for key in [
                "DIFF_INSERTED",
                "DIFF_DELETED",
                "DIFF_MODIFIED",
                "DIFF_CONFLICT",
            ] {
                assert!(p.scheme_background(key).is_some(), "{id:?} lacks {key}");
            }
            let distinct: std::collections::BTreeSet<u32> = p.console_ansi.into_iter().collect();
            assert!(
                distinct.len() >= 14,
                "{id:?}: terminal colours {distinct:x?}"
            );
            // A terminal colour that reads is drawn as the scheme has it.
            for (raw, text) in p.console_ansi.into_iter().zip(p.console_ansi_text) {
                let reads = ferris_idea_ui::theme::contrast(raw, p.console_background)
                    >= TEXT_CONTRAST
                    && ferris_idea_ui::theme::contrast(raw, island) >= TEXT_CONTRAST;
                assert_eq!(raw == text, reads, "{id:?}: {raw:06x} drawn as {text:06x}");
            }
        }
        // The two kinds differ in every surface, so a switch between them is visible everywhere.
        let (dark, light) = (
            Palette::bundled(ThemeId::FerrisIdeaDark),
            Palette::bundled(ThemeId::FerrisIdeaLight),
        );
        assert_ne!(dark.background, light.background);
        assert_ne!(dark.island, light.island);
        assert_ne!(dark.popup, light.popup);
        assert_ne!(dark.console_background, light.console_background);
        assert_ne!(dark.console_ansi_text, light.console_ansi_text);
        assert_ne!(dark.diff_inserted, light.diff_inserted);
        // Each palette is read once and kept: switching back and forth costs nothing.
        for id in ThemeId::ALL {
            assert!(std::ptr::eq(Palette::for_theme(id), Palette::for_theme(id)));
        }
    }

    /// SPEC-0060 v10 AK-14: the bundled themes are complete but for two labels the reference's
    /// classic themes leave out; nothing else is made up.
    #[test]
    fn bundled_themes_note_only_what_their_files_leave_out() {
        for id in ThemeId::ALL {
            let notes = Palette::bundled(id).notes().to_vec();
            let expected: Vec<String> = match id {
                ThemeId::Dark | ThemeId::Light => vec![format!(
                    "theme `{}`: `Label.successForeground` is not set; a colour derived from \
                     the theme's background and text is used",
                    id.name()
                )],
                _ => Vec::new(),
            };
            assert_eq!(notes, expected, "{id:?}");
        }
    }

    /// SPEC-0060 v10 AK-14: a light theme whose files lack most colours and spell others wrongly
    /// still gives a whole light palette — no dark default anywhere — whose texts read, and says
    /// what it made up.
    #[test]
    fn a_broken_theme_still_gives_a_whole_palette_of_its_kind() {
        use ferris_idea_ui::theme::{contrast, over, parse_schemes};
        let theme = UiTheme::parse(
            r##"{"name": "Sparse", "dark": false,
                "colors": {"paper": "#f4f1ea"},
                "ui": {"*": {"background": "paper"},
                       "Popup": {"background": "#fffff"},
                       "Tree": {"selectionBackground": "sky"},
                       "Label": {"errorForeground": "#ff8080"}}}"##,
        )
        .unwrap();
        let scheme = parse_schemes(
            r##"<scheme name="Sparse"><colors>
                  <option name="CARET_ROW_COLOR" value="pale"/>
                </colors><attributes>
                  <option name="TEXT"><value>
                    <option name="FOREGROUND" value="2b2b2b"/>
                    <option name="BACKGROUND" value="fbf8f1"/>
                  </value></option>
                  <option name="WARNING_ATTRIBUTES"><value>
                    <option name="EFFECT_COLOR" value="ffe066"/>
                  </value></option>
                </attributes></scheme>"##,
        )
        .unwrap()
        .remove(0);
        let scheme = ColorScheme::resolve(scheme, &[]).unwrap();
        let p = Palette::from_parts(theme, scheme, false);
        assert!(!p.dark);
        assert_eq!(p.island.rgb, 0xf4f1ea);
        assert_eq!(p.background, 0xfbf8f1);
        let island = p.island.rgb;
        for (what, c) in [
            ("popup", p.popup),
            ("tooltip", p.tooltip.background),
            ("tree selection", p.tree_selection),
            ("inactive selection", p.tree_selection_inactive),
            ("hovered row", p.tree_hover),
            ("selected tab", p.tab_selected),
            ("hovered toolbar button", p.toolbar_hover),
            ("status bar line", p.status_border),
            ("separator", p.separator),
        ] {
            assert!(
                is_light(over(c.rgb, c.alpha, island)),
                "{what} is {:06x}",
                c.rgb
            );
        }
        for (what, colour) in [
            ("caret row", p.caret_row),
            ("selection", p.selection),
            ("terminal", p.console_background),
            ("added rows", p.diff_inserted),
            ("deleted rows", p.diff_deleted),
            ("changed rows", p.diff_modified),
            ("conflicting rows", p.diff_conflict),
        ] {
            assert!(is_light(colour), "{what} is {colour:06x}");
        }
        // Texts read, also where the file's own colour does not (pink on cream).
        assert!(contrast(0xff8080, island) < TEXT_CONTRAST);
        assert_ne!(p.error_text.rgb, 0xff8080);
        for (what, fg, bg, min) in pairs(&p) {
            // The accent is the one fixed colour of a theme without any; its text is white.
            if what == "text on the accent" {
                continue;
            }
            let ratio = contrast(fg, bg);
            assert!(ratio >= min, "{what}: {fg:06x} on {bg:06x} is {ratio:.2}");
        }
        let notes = p.notes().join("\n");
        for part in [
            "theme `Sparse`: `Popup.background` has the malformed colour `#fffff`",
            "scheme `Sparse`: `CARET_ROW_COLOR` has the malformed colour `pale`",
            "theme `Sparse`: `Tree.selectionBackground` names the unknown colour `sky`",
            "theme `Sparse`: `Link.activeForeground` is not set",
            "theme `Sparse`: `StatusBar.Widget.foreground` is not set",
        ] {
            assert!(notes.contains(part), "{part} not in:\n{notes}");
        }
    }

    #[test]
    fn islands_palette_comes_from_the_bundled_files() {
        let p = Palette::bundled(ThemeId::IslandsDark);
        assert_eq!(p.window.rgb, 0x26282c);
        // Effect colours of problems (SPEC-0070).
        assert_eq!(p.scheme_effect("ERRORS_ATTRIBUTES"), Some(0xfa6675));
        assert_eq!(p.scheme_effect("WARNING_ATTRIBUTES"), Some(0xf2c55c));
        assert_eq!(p.island.rgb, 0x191a1c);
        assert_eq!(
            p.background, 0x191a1c,
            "editor and islands share the background"
        );
        assert_eq!(p.island_radius, 10.0);
        assert_eq!(p.island_gap, 5.0, "OBS-0020: 5 px visible between islands");
        // SPEC-0061 v6 behaviour 27: a shown window's stripe button without the focus is grey.
        assert_eq!(p.stripe_shown.rgb, 0x494a4e);
        // Compact islands (SPEC-0061 v4 behaviour 15a).
        let compact = Palette::bundled_in(ThemeId::IslandsDark, true);
        assert_eq!((compact.island_radius, compact.island_gap), (8.0, 3.0));
        assert_ne!(p.header_tab_underline, p.header_tab_underline_inactive);
        assert_eq!(p.tab_selected.rgb, 0x233558);
        assert_eq!(p.tree_selection_inactive.rgb, 0x33353b);
        assert_eq!(p.tree_hover.alpha, 16);
        assert_eq!(p.tab_unselected_alpha, 0.75);
        // Metrics from the theme (SPEC-0154).
        assert_eq!(p.metrics.tab_bar_height, 40.0);
        assert_eq!(p.metrics.tab_height, 28.0);
        assert_eq!(p.capture("keyword").foreground, Some(0xcf8e6d));
        assert_ne!(p.console_ansi[1], p.console_ansi[2], "red and green differ");
        assert_ne!(p.console_ansi[0], p.console_ansi[15]);
        assert_eq!(xterm_color(1, &p.console_ansi), p.console_ansi[1]);
        assert_eq!(xterm_color(16, &p.console_ansi), 0x000000);
        assert_eq!(xterm_color(196, &p.console_ansi), 0xff0000);
        assert_eq!(xterm_color(231, &p.console_ansi), 0xffffff);
        assert_eq!(xterm_color(232, &p.console_ansi), 0x080808);
        assert_eq!(xterm_color(255, &p.console_ansi), 0xeeeeee);
        let classic = Palette::bundled(ThemeId::Dark);
        assert_eq!(classic.background, 0x1e1f22);
        assert_eq!(
            classic.island_radius, 0.0,
            "the classic theme has no islands"
        );
    }

    /// SPEC-0061 v19 AK-v19 (behaviour 6a, OBS-0079 L4): the selected pill tab takes the theme's
    /// active colours while the keyboard focus is in its editor and its inactive ones otherwise:
    /// in `Islands Light` `blue-150` with a `blue-120` border, then `gray-150` with a `gray-130`
    /// border; every pill theme reads both pairs from its file, the underline themes keep one.
    #[test]
    fn selected_tab_follows_the_focus_in_islands_light() {
        let light = Palette::bundled(ThemeId::IslandsLight);
        assert_eq!(light.tab_selected.rgb, 0xe3ebfe);
        assert_eq!(light.tab_selected_border.rgb, 0xa7c5ff);
        assert_eq!(light.tab_selected_inactive.rgb, 0xe9eaee);
        assert_eq!(light.tab_selected_border_inactive.rgb, 0xd1d3d9);
        assert_eq!(
            crate::metrics::pill_colours(&light, true),
            (light.tab_selected, light.tab_selected_border)
        );
        assert_eq!(
            crate::metrics::pill_colours(&light, false),
            (
                light.tab_selected_inactive,
                light.tab_selected_border_inactive
            )
        );
        for id in ThemeId::ALL {
            let p = Palette::bundled(id);
            let theme = UiTheme::for_theme(id);
            let inactive = theme.color("EditorTabs.inactiveUnderlinedTabBackground");
            let border = theme.color("EditorTabs.inactiveUnderlinedTabBorderColor");
            if p.metrics.tab_style == ferris_idea_ui::metrics::TabStyle::Pill {
                assert_eq!(Some(p.tab_selected_inactive), inactive, "{id:?}");
                assert_eq!(Some(p.tab_selected_border_inactive), border, "{id:?}");
            } else {
                assert_eq!(p.tab_selected_inactive, p.tab_selected, "{id:?}");
                assert_eq!(
                    p.tab_selected_border_inactive, p.tab_selected_border,
                    "{id:?}"
                );
            }
        }
    }

    /// SPEC-0060 AK-17 (v11), OBS-0047 T1 and T6: with `Islands Dark` the frame (window, main
    /// toolbar, stripes, status bar) has the open theme's `main-window-bg` (`gray-30`), the
    /// islands and the editor its `tool-window-bg` and `editor-bg` (`gray-10`); a selected row is
    /// `selection-bg-active` (`blue-50`) while its tree has the focus and
    /// `selection-bg-inactive` (`gray-40`) otherwise. The other themes keep their colours.
    #[test]
    fn frame_islands_and_selection_come_from_the_open_theme() {
        use ferris_idea_ui::theme::UiTheme;
        let theme = UiTheme::for_theme(ThemeId::IslandsDark);
        for key in [
            "MainWindow.background",
            "MainToolbar.background",
            "MainToolbar.inactiveBackground",
            "ToolWindow.Stripe.background",
            "StatusBar.background",
        ] {
            assert_eq!(theme.color(key).map(|c| c.rgb), Some(0x26282c), "{key}");
        }
        for key in ["Tree.selectionBackground", "List.selectionBackground"] {
            assert_eq!(theme.color(key).map(|c| c.rgb), Some(0x2a4371), "{key}");
        }
        for key in [
            "Tree.selectionInactiveBackground",
            "List.selectionInactiveBackground",
        ] {
            assert_eq!(theme.color(key).map(|c| c.rgb), Some(0x33353b), "{key}");
        }
        let p = Palette::bundled(ThemeId::IslandsDark);
        let colours = |p: &Palette| {
            [
                p.window.rgb,
                p.island.rgb,
                p.background,
                p.tree_selection.rgb,
                p.tree_selection_inactive.rgb,
            ]
        };
        assert_eq!(
            colours(&p),
            [0x26282c, 0x191a1c, 0x191a1c, 0x2a4371, 0x33353b]
        );
        // SPEC-0061 v14 behaviour 28a: the toolbar's project colour is the theme's own, muted.
        let g = p
            .project_gradient
            .expect("Islands Dark has a project gradient");
        assert_eq!((g.width, g.height), (600.0, 300.0));
        assert_eq!(g.colours[4], 0x364268, "Color5 #394773, 85 % into #26282C");
        for (i, centre) in [
            0x664832, 0x5e4724, 0x45522f, 0x335661, 0x394773, 0x613861, 0x503d70, 0x325959,
            0x365439,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                g.colours[i],
                ferris_idea_ui::theme::mix(0x26282c, centre, 0.85),
                "Color{}",
                i + 1
            );
            // Dark and muted: never a bright accent.
            assert!(
                ferris_idea_ui::theme::contrast(g.colours[i], 0x26282c) < 2.0,
                "Color{}",
                i + 1
            );
        }
        assert_eq!(g.colour_for("/a/b"), g.colour_for("/a/b"));
        assert!(
            Palette::bundled(ThemeId::IslandsLight)
                .project_gradient
                .is_some()
        );
        for id in [
            ThemeId::FerrisIdeaDark,
            ThemeId::FerrisIdeaLight,
            ThemeId::Dark,
            ThemeId::Light,
        ] {
            assert_eq!(Palette::bundled(id).project_gradient, None, "{id:?}");
        }
        // Window, island, editor, focused and unfocused selection of the other themes.
        for (id, expected) in [
            (
                ThemeId::FerrisIdeaDark,
                [0x3b4252, 0x2e3440, 0x2e3440, 0x3e5871, 0x434c5e],
            ),
            (
                ThemeId::FerrisIdeaLight,
                [0xd8dee9, 0xeceff4, 0xeceff4, 0xc9d6e6, 0xd8dee9],
            ),
            (
                ThemeId::IslandsLight,
                [0xe9eaee, 0xffffff, 0xffffff, 0xd0dffe, 0xe9eaee],
            ),
            (
                ThemeId::Dark,
                [0x2b2d30, 0x2b2d30, 0x1e1f22, 0x2e436e, 0x43454a],
            ),
            (
                ThemeId::Light,
                [0xf7f8fa, 0xf7f8fa, 0xffffff, 0xd4e2ff, 0xdfe1e5],
            ),
        ] {
            assert_eq!(colours(&Palette::bundled(id)), expected, "{id:?}");
        }
    }

    #[test]
    fn light_palette_is_light_and_differs() {
        let light = Palette::bundled(ThemeId::Light);
        assert!(!light.dark && Palette::bundled(ThemeId::Dark).dark);
        assert_eq!(light.background, 0xffffff);
        assert_ne!(light.text, Palette::bundled(ThemeId::IslandsDark).text);
        assert_eq!(index(ThemeId::DEFAULT), 0, "the default comes first");
        assert_eq!(index(ThemeId::IslandsDark), 2);
    }

    #[test]
    fn brand_palette_is_the_default_and_carries_the_accent() {
        // Nothing chosen yet: the views draw with Ferris IDEA Dark (SPEC-0060 v6).
        assert_eq!(current_theme(), ThemeId::DEFAULT);
        let p = Palette::bundled(ThemeId::FerrisIdeaDark);
        assert!(p.dark);
        assert_eq!(p.background, 0x2e3440);
        assert_eq!(p.island.rgb, 0x2e3440);
        assert_eq!(p.window.rgb, 0x3b4252);
        assert_eq!(p.text, 0xd8dee9);
        assert_eq!(p.accent.rgb, 0x88c0d0);
        assert_eq!(p.accent_text.rgb, 0x2e3440);
        assert_eq!(p.link.rgb, 0x88c0d0);
        assert_eq!(p.focus.rgb, 0x88c0d0);
        assert_eq!(p.tab_selected_border.rgb, 0x88c0d0);
        assert_eq!(p.error_text.rgb, 0xde8c93);
        assert_eq!(p.success_text.rgb, 0xa3be8c);
        // Islands layout and metrics stay.
        let islands = Palette::bundled(ThemeId::IslandsDark);
        assert_eq!(p.island_radius, islands.island_radius);
        assert_eq!(p.island_gap, islands.island_gap);
        assert_eq!(p.metrics.tab_height, islands.metrics.tab_height);
        assert_eq!(p.tree_row_height, islands.tree_row_height);
        // The reference's accent where the reference's theme is chosen.
        assert_eq!(islands.accent.rgb, 0x3871e1);
        assert_eq!(islands.accent_text.rgb, 0xffffff);
        let light = Palette::bundled(ThemeId::FerrisIdeaLight);
        assert!(!light.dark);
        assert_eq!(light.background, 0xeceff4);
        assert_eq!(light.accent.rgb, 0x4c6a91);
        for t in ThemeId::ALL {
            let p = Palette::bundled(t);
            assert!(
                ferris_idea_ui::theme::contrast(p.accent_text.rgb, p.accent.rgb) >= 3.0,
                "{t:?}: marks on the accent"
            );
        }
    }
}

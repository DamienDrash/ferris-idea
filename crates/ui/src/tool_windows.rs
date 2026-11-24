//! Tool window layout (`SPEC-0167`): the side each tool window docks to, its order on the stripes,
//! whether the stripes are shown, "hide all" with restore, and the per-project file it is kept in.
//! `SPEC-0061` v7: six placements (split halves), view modes, "Remove from Sidebar", the More tool
//! windows list and the frames of separate windows.
//! Toolkit-free so the workbench and the MCP server share it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A side of the editor a tool window docks to (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Between the left stripe and the editor.
    Left,
    /// Below the editor.
    Bottom,
    /// Between the editor and the right stripe.
    Right,
}

impl Side {
    /// All sides in menu order.
    pub const ALL: [Side; 3] = [Side::Left, Side::Bottom, Side::Right];

    /// Name as used in menus and over MCP.
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Bottom => "bottom",
            Side::Right => "right",
        }
    }

    /// Parses `left`, `bottom` or `right` (any case).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Label of the stripe menu entry that moves a window here (behaviour 5).
    pub fn move_label(self) -> &'static str {
        match self {
            Side::Left => "Move to Left",
            Side::Bottom => "Move to Bottom",
            Side::Right => "Move to Right",
        }
    }
}

/// One of the six places a tool window docks to: a side and whether it is the side's second
/// (split) half — Left Bottom, Bottom Right, Right Bottom (`SPEC-0061` v7 behaviour 37).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement {
    /// The side.
    pub side: Side,
    /// The second half of the side.
    pub split: bool,
}

impl Placement {
    /// The six placements in menu order.
    pub const ALL: [Placement; 6] = [
        Placement::new(Side::Left, false),
        Placement::new(Side::Left, true),
        Placement::new(Side::Bottom, false),
        Placement::new(Side::Bottom, true),
        Placement::new(Side::Right, false),
        Placement::new(Side::Right, true),
    ];

    /// The placement on `side`, its second half when `split`.
    pub const fn new(side: Side, split: bool) -> Self {
        Self { side, split }
    }

    /// Name over MCP: `left_top`, `left_bottom`, `bottom_left`, `bottom_right`, `right_top`,
    /// `right_bottom`.
    pub fn name(self) -> &'static str {
        match (self.side, self.split) {
            (Side::Left, false) => "left_top",
            (Side::Left, true) => "left_bottom",
            (Side::Bottom, false) => "bottom_left",
            (Side::Bottom, true) => "bottom_right",
            (Side::Right, false) => "right_top",
            (Side::Right, true) => "right_bottom",
        }
    }

    /// Label in the menus: "Left Top" …
    pub fn label(self) -> &'static str {
        match (self.side, self.split) {
            (Side::Left, false) => "Left Top",
            (Side::Left, true) => "Left Bottom",
            (Side::Bottom, false) => "Bottom Left",
            (Side::Bottom, true) => "Bottom Right",
            (Side::Right, false) => "Right Top",
            (Side::Right, true) => "Right Bottom",
        }
    }

    /// Parses a name (any case, `-` or a space instead of `_`).
    pub fn from_name(name: &str) -> Option<Self> {
        let wanted = name.trim().replace(['-', ' '], "_");
        Self::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(&wanted))
    }
}

/// How a tool window is shown (`SPEC-0061` v7 behaviour 39).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ViewMode {
    /// Docked and kept until hidden.
    #[default]
    DockPinned,
    /// Docked; hides when the focus moves elsewhere in the IDE window.
    DockUnpinned,
    /// Drawn over the editor at its placement's edge.
    Undock,
    /// A separate window above the IDE window.
    Float,
    /// A separate normal window.
    Window,
}

impl ViewMode {
    /// The modes in menu order.
    pub const ALL: [ViewMode; 5] = [
        ViewMode::DockPinned,
        ViewMode::DockUnpinned,
        ViewMode::Undock,
        ViewMode::Float,
        ViewMode::Window,
    ];

    /// Name in the file and over MCP.
    pub fn id(self) -> &'static str {
        match self {
            ViewMode::DockPinned => "docked",
            ViewMode::DockUnpinned => "unpinned",
            ViewMode::Undock => "undocked",
            ViewMode::Float => "float",
            ViewMode::Window => "window",
        }
    }

    /// Label in the View Mode menu.
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::DockPinned => "Dock Pinned",
            ViewMode::DockUnpinned => "Dock Unpinned",
            ViewMode::Undock => "Undock",
            ViewMode::Float => "Float",
            ViewMode::Window => "Window",
        }
    }

    /// Parses a name or a label (any case, spaces ignored).
    pub fn from_id(id: &str) -> Option<Self> {
        let wanted: String = id.chars().filter(|c| !c.is_whitespace()).collect();
        Self::ALL.into_iter().find(|m| {
            m.id().eq_ignore_ascii_case(&wanted)
                || m.label().replace(' ', "").eq_ignore_ascii_case(&wanted)
        })
    }

    /// Takes room in the IDE window's layout (Dock Pinned and Dock Unpinned).
    pub fn docked(self) -> bool {
        matches!(self, ViewMode::DockPinned | ViewMode::DockUnpinned)
    }

    /// Shown in a separate window of its own (Float and Window).
    pub fn separate(self) -> bool {
        matches!(self, ViewMode::Float | ViewMode::Window)
    }

    /// Hides when the focus moves to another part of the IDE window.
    pub fn hides_on_focus_loss(self) -> bool {
        self == ViewMode::DockUnpinned
    }
}

/// A tool window of the layout (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolWindow {
    /// The Project tool window.
    Project,
    /// The Commit tool window.
    Commit,
    /// The Git tool window.
    Git,
    /// The Problems tool window.
    Problems,
    /// The Run tool window.
    Run,
    /// The Debug tool window.
    Debug,
    /// The Services tool window.
    Services,
    /// The Terminal tool window.
    Terminal,
    /// The Database tool window.
    Database,
    /// The Build tool window.
    Build,
    /// The Plugins tool window: sections of plugins' `panel` actions (`SPEC-0210`).
    Plugins,
    /// The Structure tool window: the active file's symbols (`SPEC-0061` v7 behaviour 42).
    Structure,
    /// The Find tool window with usages (`SPEC-0293`; a tool window since `SPEC-0061` v7).
    Find,
    /// Find in Files (`SPEC-0066`).
    FindInFiles,
    /// Endpoints (`SPEC-0109`).
    Endpoints,
    /// Spring.
    Spring,
    /// AI Chat (`SPEC-0138`).
    AiChat,
    /// Python Packages (`SPEC-0117`).
    Python,
    /// Containers.
    Containers,
    /// Profiler (`SPEC-0120`).
    Profiler,
    /// Remote Host.
    Remote,
    /// Bookmarks (`SPEC-0331`).
    Bookmarks,
    /// TODO (`SPEC-0334`).
    Todo,
    /// Notifications: the status bar's notes of this window (`SPEC-0061` v17 behaviour 64).
    Notifications,
}

impl ToolWindow {
    /// All tool windows in their default order: the reference's New UI default layout (Version
    /// Control, Problems, Terminal, Services at the bottom), then Run and Debug, which register when
    /// they first exist (`SPEC-0061` v4 behaviour 13); then Structure and the former panels outside
    /// the layout (v7 behaviours 42 and 43).
    pub const ALL: [ToolWindow; 24] = [
        ToolWindow::Project,
        ToolWindow::Commit,
        ToolWindow::Git,
        ToolWindow::Problems,
        ToolWindow::Terminal,
        ToolWindow::Services,
        ToolWindow::Run,
        ToolWindow::Debug,
        ToolWindow::Database,
        ToolWindow::Build,
        ToolWindow::Plugins,
        ToolWindow::Structure,
        ToolWindow::Find,
        ToolWindow::FindInFiles,
        ToolWindow::Endpoints,
        ToolWindow::Spring,
        ToolWindow::AiChat,
        ToolWindow::Python,
        ToolWindow::Containers,
        ToolWindow::Profiler,
        ToolWindow::Remote,
        ToolWindow::Bookmarks,
        ToolWindow::Todo,
        ToolWindow::Notifications,
    ];

    /// The former panels outside the layout, with their own header row (`SPEC-0061` v7 behaviour
    /// 43).
    pub fn own_header(self) -> bool {
        matches!(
            self,
            ToolWindow::Find
                | ToolWindow::FindInFiles
                | ToolWindow::Endpoints
                | ToolWindow::Spring
                | ToolWindow::AiChat
                | ToolWindow::Python
                | ToolWindow::Containers
                | ToolWindow::Profiler
                | ToolWindow::Remote
        )
    }

    /// Stable id used in the file and over MCP.
    pub fn id(self) -> &'static str {
        match self {
            ToolWindow::Project => "project",
            ToolWindow::Commit => "commit",
            ToolWindow::Git => "git",
            ToolWindow::Problems => "problems",
            ToolWindow::Run => "run",
            ToolWindow::Debug => "debug",
            ToolWindow::Services => "services",
            ToolWindow::Terminal => "terminal",
            ToolWindow::Database => "database",
            ToolWindow::Build => "build",
            ToolWindow::Plugins => "plugins",
            ToolWindow::Structure => "structure",
            ToolWindow::Find => "find",
            ToolWindow::FindInFiles => "find_in_files",
            ToolWindow::Endpoints => "endpoints",
            ToolWindow::Spring => "spring",
            ToolWindow::AiChat => "ai_chat",
            ToolWindow::Python => "python",
            ToolWindow::Containers => "containers",
            ToolWindow::Profiler => "profiler",
            ToolWindow::Remote => "remote",
            ToolWindow::Bookmarks => "bookmarks",
            ToolWindow::Todo => "todo",
            ToolWindow::Notifications => "notifications",
        }
    }

    /// Parses an id (any case).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|w| w.id().eq_ignore_ascii_case(id.trim()))
    }

    /// Title on the stripe button's tooltip and in menus.
    pub fn title(self) -> &'static str {
        match self {
            ToolWindow::Project => "Project",
            ToolWindow::Commit => "Commit",
            ToolWindow::Git => "Git",
            ToolWindow::Problems => "Problems",
            ToolWindow::Run => "Run",
            ToolWindow::Debug => "Debug",
            ToolWindow::Services => "Services",
            ToolWindow::Terminal => "Terminal",
            ToolWindow::Database => "Database",
            ToolWindow::Build => "Build",
            ToolWindow::Plugins => "Plugins",
            ToolWindow::Structure => "Structure",
            ToolWindow::Find => "Find",
            ToolWindow::FindInFiles => "Find in Files",
            ToolWindow::Endpoints => "Endpoints",
            ToolWindow::Spring => "Spring",
            ToolWindow::AiChat => "AI Chat",
            ToolWindow::Python => "Python Packages",
            ToolWindow::Containers => "Containers",
            ToolWindow::Profiler => "Profiler",
            ToolWindow::Remote => "Remote Host",
            ToolWindow::Bookmarks => "Bookmarks",
            ToolWindow::Todo => "TODO",
            ToolWindow::Notifications => "Notifications",
        }
    }

    /// The action that shows or hides it.
    pub fn action(self) -> &'static str {
        match self {
            ToolWindow::Project => "ActivateProjectToolWindow",
            ToolWindow::Commit => "ActivateCommitToolWindow",
            ToolWindow::Git => "ActivateVersionControlToolWindow",
            ToolWindow::Problems => "ActivateProblemsViewToolWindow",
            ToolWindow::Run => "ActivateRunToolWindow",
            ToolWindow::Debug => "ActivateDebugToolWindow",
            ToolWindow::Services => "ActivateServicesToolWindow",
            ToolWindow::Terminal => "ActivateTerminalToolWindow",
            ToolWindow::Database => "ActivateDatabaseToolWindow",
            ToolWindow::Build => "ActivateMavenToolWindow",
            ToolWindow::Plugins => "ActivatePluginsToolWindow",
            ToolWindow::Structure => "ActivateStructureToolWindow",
            ToolWindow::Find => "ActivateFindToolWindow",
            ToolWindow::FindInFiles => "FindInPath",
            ToolWindow::Endpoints => "ActivateEndpointsToolWindow",
            ToolWindow::Spring => "Spring.ToolWindow",
            ToolWindow::AiChat => "FerrisIDEA.AiChat",
            ToolWindow::Python => "ActivatePythonPackagesToolWindow",
            ToolWindow::Containers => "Containers.ToolWindow",
            ToolWindow::Profiler => "Profiler.ToolWindow",
            ToolWindow::Remote => "Remote.ToolWindow",
            ToolWindow::Bookmarks => "ActivateBookmarksToolWindow",
            ToolWindow::Todo => "ActivateTODOToolWindow",
            ToolWindow::Notifications => "ActivateNotificationsToolWindow",
        }
    }

    /// Stripe icon (New UI icon path).
    pub fn icon(self) -> &'static str {
        match self {
            ToolWindow::Project => "toolwindows/project",
            ToolWindow::Commit => "toolwindows/commit",
            ToolWindow::Git => "general/vcs",
            ToolWindow::Problems => "toolwindows/problems",
            ToolWindow::Run => "toolwindows/run",
            ToolWindow::Debug => "toolwindows/debug",
            ToolWindow::Services => "toolwindows/services",
            ToolWindow::Terminal => "toolwindows/terminal",
            ToolWindow::Database => "nodes/dataSchema",
            ToolWindow::Build => "nodes/module",
            ToolWindow::Plugins => "nodes/package",
            // The open New UI icon (SPEC-0061 v17 behaviour 63).
            ToolWindow::Structure => "toolwindows/structure",
            ToolWindow::Find | ToolWindow::FindInFiles => "general/search",
            ToolWindow::Endpoints => "fileTypes/http",
            ToolWindow::Spring => "ferris-idea/spring",
            ToolWindow::AiChat => "ferris-idea/chat",
            ToolWindow::Python => "fileTypes/python",
            ToolWindow::Containers => "fileTypes/docker",
            ToolWindow::Profiler => "ferris-idea/profiler",
            ToolWindow::Remote => "ferris-idea/remote",
            // The bundled icons have none for bookmarks yet (SPEC-0331, out of scope).
            ToolWindow::Bookmarks => "ferris-idea/tag",
            // The open platform's TODO tool window icon (SPEC-0334 behaviour 13).
            ToolWindow::Todo => "toolwindows/todo",
            // SPEC-0061 v17 behaviour 64.
            ToolWindow::Notifications => "toolwindows/notifications",
        }
    }

    /// Stripe icon: the open 20 px tool window icon where there is one, else the 16 px icon, drawn
    /// at 20 px (`SPEC-0061` v4 behaviour 12).
    pub fn stripe_icon(self) -> &'static str {
        match self {
            ToolWindow::Project => "toolwindows/project@20x20",
            ToolWindow::Commit => "toolwindows/commit@20x20",
            ToolWindow::Git => "toolwindows/vcs@20x20",
            ToolWindow::Problems => "toolwindows/problems@20x20",
            ToolWindow::Run => "toolwindows/run@20x20",
            ToolWindow::Debug => "toolwindows/debug@20x20",
            ToolWindow::Services => "toolwindows/services@20x20",
            ToolWindow::Terminal => "toolwindows/terminal@20x20",
            ToolWindow::Build => "toolwindows/build@20x20",
            ToolWindow::Todo => "toolwindows/todo@20x20",
            ToolWindow::Structure => "toolwindows/structure@20x20",
            ToolWindow::Notifications => "toolwindows/notifications@20x20",
            _ => self.icon(),
        }
    }

    /// The side of the reference (behaviour 1; `SPEC-0061` v10 behaviour 51: AI Chat on the right
    /// where the reference has its AI window).
    pub fn default_side(self) -> Side {
        match self {
            ToolWindow::Project
            | ToolWindow::Commit
            | ToolWindow::Structure
            | ToolWindow::Bookmarks => Side::Left,
            ToolWindow::Notifications
            | ToolWindow::AiChat
            | ToolWindow::Database
            | ToolWindow::Build
            | ToolWindow::Plugins => Side::Right,
            _ => Side::Bottom,
        }
    }

    /// The placement of the reference: Structure in the left side's second half (`SPEC-0061` v7
    /// behaviour 38), the others in the first half of their side.
    pub fn default_placement(self) -> Placement {
        Placement::new(self.default_side(), self == ToolWindow::Structure)
    }

    /// Whether the window has a stripe button in the default layout; Ferris IDEA's own and rarely
    /// used windows are reached through "…" until the user adds them (`SPEC-0061` v10
    /// behaviour 51).
    pub fn on_sidebar_by_default(self) -> bool {
        !matches!(
            self,
            ToolWindow::Plugins
                | ToolWindow::Endpoints
                | ToolWindow::Spring
                | ToolWindow::Python
                | ToolWindow::Containers
                | ToolWindow::Profiler
                | ToolWindow::Remote
                | ToolWindow::Bookmarks
                | ToolWindow::Todo
        )
    }

    /// The placement of `SPEC-0061` v7 before v10 (AI Chat at the bottom), to recognise layouts
    /// the user never arranged (v10 behaviour 54).
    fn v7_default_placement(self) -> Placement {
        match self {
            ToolWindow::AiChat => Placement::new(Side::Bottom, false),
            w => w.default_placement(),
        }
    }
}

/// The default order of the tool windows within their placements (`SPEC-0061` v10 behaviour
/// 51): Project and Commit, Structure; Git, Problems, Terminal, Services, Run, Debug, Find and
/// Find in Files at the bottom; Notifications (v17 behaviour 64), AI Chat, Database and Build on
/// the right; then the windows off the sidebar.
pub const DEFAULT_ORDER: [ToolWindow; 24] = [
    ToolWindow::Project,
    ToolWindow::Commit,
    ToolWindow::Structure,
    ToolWindow::Git,
    ToolWindow::Problems,
    ToolWindow::Terminal,
    ToolWindow::Services,
    ToolWindow::Run,
    ToolWindow::Debug,
    ToolWindow::Find,
    ToolWindow::FindInFiles,
    ToolWindow::Notifications,
    ToolWindow::AiChat,
    ToolWindow::Database,
    ToolWindow::Build,
    ToolWindow::Plugins,
    ToolWindow::Endpoints,
    ToolWindow::Spring,
    ToolWindow::Python,
    ToolWindow::Containers,
    ToolWindow::Profiler,
    ToolWindow::Remote,
    ToolWindow::Bookmarks,
    ToolWindow::Todo,
];

/// Version written into the layout file since `SPEC-0061` v10; files without it may be
/// migrated (behaviour 54).
pub const LAYOUT_VERSION: u32 = 2;

impl ToolWindow {
    /// Width when docked left or right in a window `window_width` wide (behaviour 3; `SPEC-0061`
    /// v6 behaviour 22): a third of the window less the island gap, as in the reference
    /// (`OBS-0020`: 457 in a 1400 wide window), never below [`SIDE_MIN_WIDTH`].
    pub fn width_in(self, window_width: f32, gap: f32) -> f32 {
        (window_width * DEFAULT_WEIGHT - gap).max(SIDE_MIN_WIDTH)
    }

    /// Width when docked left or right in a window of the default size (1400 px).
    pub fn width(self) -> f32 {
        self.width_in(1400.0, 5.0)
    }
}

/// Share of the window a side or bottom tool window takes by default (`OBS-0020`).
pub const DEFAULT_WEIGHT: f32 = 0.33;
/// Narrowest default width of a side tool window.
pub const SIDE_MIN_WIDTH: f32 = 240.0;

/// What the header of a tool window holds (`SPEC-0061` v5 behaviour 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Header {
    /// Built from the shared header component (title, tabs, toolbar, Options, Hide).
    pub shared: bool,
    /// The title at its left.
    pub title: &'static str,
    /// What its content tabs are: `shells`, `sessions`, `results`, `log`; `None` without tabs.
    pub content_tabs: Option<&'static str>,
    /// The window's own toolbar buttons, in a row below the header (`SPEC-0061` v6 behaviour 25).
    pub toolbar: &'static [&'static str],
    /// Buttons of the window in the header itself, before Options (v6 behaviour 26).
    pub buttons: &'static [&'static str],
    /// The header's actions at the right edge.
    pub actions: &'static [&'static str],
    /// A line below the header (every window but Project, v6 behaviour 24).
    pub line: bool,
}

/// Options (⋮, the tool window's menu) and Hide (`SPEC-0061` v4 behaviour 9).
pub const HEADER_ACTIONS: &[&str] = &["options", "hide"];

impl ToolWindow {
    /// Its header (`SPEC-0061` v5 behaviour 18, v6 behaviours 23–26): every window of the layout
    /// uses the shared header, Problems too since v6.
    pub fn header(self) -> Header {
        let shared = |content_tabs, toolbar| Header {
            shared: true,
            title: self.title(),
            content_tabs,
            toolbar,
            buttons: &[],
            actions: HEADER_ACTIONS,
            line: self != ToolWindow::Project,
        };
        match self {
            ToolWindow::Project => Header {
                buttons: &["new", "locate", "expand_all", "collapse_all"],
                ..shared(None, &[])
            },
            ToolWindow::Commit => shared(None, &[]),
            ToolWindow::Terminal => shared(Some("shells"), &[]),
            ToolWindow::Git => shared(Some("log"), &[]),
            ToolWindow::Run => shared(Some("sessions"), &["stop"]),
            ToolWindow::Debug => shared(Some("sessions"), &[]),
            ToolWindow::Services => {
                shared(Some("results"), &["connection", "running", "environment"])
            }
            ToolWindow::Database => shared(None, &["new", "edit", "refresh"]),
            ToolWindow::Build => shared(None, &["status", "reload"]),
            ToolWindow::Plugins => shared(None, &["refresh"]),
            // The scopes Project and Current File as content tabs (SPEC-0334 behaviour 14).
            ToolWindow::Problems | ToolWindow::Todo => shared(Some("scopes"), &[]),
            ToolWindow::Structure | ToolWindow::Bookmarks => shared(None, &[]),
            // Clear All in the row below the header (SPEC-0061 v17 behaviour 64).
            ToolWindow::Notifications => shared(None, &["clear_all"]),
            // The former panels keep their own header row; Options and Hide sit at its right
            // end (`SPEC-0061` v7 behaviour 43).
            _ => Header {
                shared: false,
                line: false,
                ..shared(None, &[])
            },
        }
    }
}

/// Height of a bottom tool window (behaviour 3).
pub const BOTTOM_HEIGHT: f32 = 280.0;

/// File the layout is kept in, below the project root (behaviour 8).
pub const LAYOUT_FILE: &str = ".ferris-idea/tool-windows.json";

/// Sides, order and stripes of the tool windows, plus what "hide all" hid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Every tool window once, in stripe order; a window's placement is the second field.
    order: Vec<(ToolWindow, Placement)>,
    /// Both stripes shown (behaviour 7).
    pub stripes: bool,
    /// Windows hidden by the last "hide all", to bring back (behaviour 6).
    hidden_by_all: Vec<ToolWindow>,
    /// Sizes set by dragging the edge or stretching, in whole pixels: the width of a left or
    /// right window, the height of a bottom one (`SPEC-0061` v6 behaviour 34).
    sizes: Vec<(ToolWindow, u32)>,
    /// View modes other than Dock Pinned (`SPEC-0061` v7 behaviour 39).
    modes: Vec<(ToolWindow, ViewMode)>,
    /// Windows without a stripe button (v7 behaviour 40).
    removed: Vec<ToolWindow>,
    /// Last frames of Float and Window windows: x, y, width, height in whole points.
    bounds: Vec<(ToolWindow, [i32; 4])>,
    /// Windows shown in the other half of their side by ⌥-click, while they are shown; never
    /// saved (`SPEC-0061` v10 behaviour 50).
    shown_elsewhere: Vec<(ToolWindow, Placement)>,
    /// The file was read as a layout the user never arranged and got the v10 defaults
    /// (`SPEC-0061` v10 behaviour 54).
    pub migrated: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            order: DEFAULT_ORDER
                .iter()
                .map(|w| (*w, w.default_placement()))
                .collect(),
            stripes: true,
            hidden_by_all: Vec::new(),
            sizes: Vec::new(),
            modes: Vec::new(),
            removed: Self::default_removed(),
            bounds: Vec::new(),
            shown_elsewhere: Vec::new(),
            migrated: false,
        }
    }
}

/// The saved form (behaviour 8).
#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    /// [`LAYOUT_VERSION`] since `SPEC-0061` v10; missing in older files.
    #[serde(default)]
    version: u32,
    #[serde(default)]
    windows: Vec<SavedWindow>,
    #[serde(default = "yes")]
    stripes: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedWindow {
    id: String,
    side: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
    /// The second half of the side; missing means the window's default (`SPEC-0061` v7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    split: Option<bool>,
    /// The view mode; missing means Dock Pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    /// No stripe button.
    #[serde(default, skip_serializing_if = "is_false")]
    removed: bool,
    /// Last frame of a Float or Window window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bounds: Option<[i32; 4]>,
}

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The buttons of one stripe, top to bottom (`SPEC-0061` v7 behaviour 38).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StripeGroups {
    /// The windows of the side's first half (Left Top, Right Top), first at the top.
    pub top: Vec<ToolWindow>,
    /// The windows of the side's second half (Left Bottom, Right Bottom), after a line.
    pub split: Vec<ToolWindow>,
    /// The bottom windows of this stripe's part (Bottom Left on the left, Bottom Right on the
    /// right), stacked from the foot upward, so the first sits lowest.
    pub foot: Vec<ToolWindow>,
}

/// Narrowest width or height a dragged or stretched tool window keeps.
pub const MIN_TOOL_WINDOW_SIZE: f32 = 80.0;
/// Share of the window height one stretch (⌃⌥ and an arrow) moves the inner edge by (`OBS-0020`:
/// 85 px in a 954 px high window).
pub const STRETCH_SHARE: f32 = 0.089;

impl Layout {
    /// The size the user gave `window` (width left or right, height at the bottom), if any
    /// (`SPEC-0061` v6 behaviour 34).
    pub fn size(&self, window: ToolWindow) -> Option<f32> {
        self.sizes
            .iter()
            .find(|(w, _)| *w == window)
            .map(|(_, v)| *v as f32)
    }

    /// Sets `window`'s size, never below [`MIN_TOOL_WINDOW_SIZE`].
    pub fn set_size(&mut self, window: ToolWindow, size: f32) {
        let v = size.max(MIN_TOOL_WINDOW_SIZE).round() as u32;
        match self.sizes.iter_mut().find(|(w, _)| *w == window) {
            Some(entry) => entry.1 = v,
            None => {
                self.sizes.push((window, v));
                self.sizes.sort_by_key(|(w, _)| *w);
            }
        }
    }

    /// Forgets `window`'s size: it takes the default share of the window again.
    pub fn clear_size(&mut self, window: ToolWindow) {
        self.sizes.retain(|(w, _)| *w != window);
    }

    /// The placement of `window` (`SPEC-0061` v7 behaviour 37).
    pub fn placement(&self, window: ToolWindow) -> Placement {
        self.order
            .iter()
            .find(|(w, _)| *w == window)
            .map_or(window.default_placement(), |(_, p)| *p)
    }

    /// The side `window` docks to.
    pub fn side(&self, window: ToolWindow) -> Side {
        self.placement(window).side
    }

    /// The windows on `side`, both halves, in stripe order (behaviour 2).
    pub fn on(&self, side: Side) -> Vec<ToolWindow> {
        self.order
            .iter()
            .filter(|(_, p)| p.side == side)
            .map(|(w, _)| *w)
            .collect()
    }

    /// The windows of one placement, in stripe order.
    pub fn in_half(&self, placement: Placement) -> Vec<ToolWindow> {
        self.order
            .iter()
            .filter(|(_, p)| *p == placement)
            .map(|(w, _)| *w)
            .collect()
    }

    /// The buttons of the left (`Side::Left`) or right stripe, top to bottom: the first half of
    /// that side, its second half after a line, and at the foot the bottom windows of the
    /// stripe's part (Bottom Left on the left stripe, Bottom Right on the right one), stacked from
    /// the foot upward, so the first sits lowest. Removed windows have no button (`SPEC-0061` v4
    /// behaviour 13, v7 behaviours 38 and 40; SPEC-0167 behaviour 2).
    pub fn stripe(&self, stripe: Side) -> StripeGroups {
        self.stripe_with(stripe, &[])
    }

    /// [`Layout::stripe`] with the `shown` windows off the sidebar keeping a button while they
    /// are shown (`SPEC-0061` v10 behaviour 52).
    pub fn stripe_with(&self, stripe: Side, shown: &[ToolWindow]) -> StripeGroups {
        let side = if stripe == Side::Right {
            Side::Right
        } else {
            Side::Left
        };
        let keep = |ws: Vec<ToolWindow>| -> Vec<ToolWindow> {
            ws.into_iter()
                .filter(|w| !self.removed(*w) || shown.contains(w))
                .collect()
        };
        let mut foot = keep(self.in_half(Placement::new(Side::Bottom, side == Side::Right)));
        foot.reverse();
        StripeGroups {
            top: keep(self.in_half(Placement::new(side, false))),
            split: keep(self.in_half(Placement::new(side, true))),
            foot,
        }
    }

    /// Position of `window` on its side (0-based).
    pub fn position(&self, window: ToolWindow) -> usize {
        self.on(self.side(window))
            .iter()
            .position(|w| *w == window)
            .unwrap_or(0)
    }

    /// Moves `window` to the first half of `side`, last there (behaviour 5); returns whether
    /// anything changed.
    pub fn move_to(&mut self, window: ToolWindow, side: Side) -> bool {
        self.place(window, Placement::new(side, false))
    }

    /// Moves `window` to `placement`, last there (`SPEC-0061` v7 behaviour 37); returns whether
    /// anything changed.
    pub fn place(&mut self, window: ToolWindow, placement: Placement) -> bool {
        if self.placement(window) == placement {
            return false;
        }
        self.clear_shown_elsewhere(window);
        self.order.retain(|(w, _)| *w != window);
        self.order.push((window, placement));
        true
    }

    /// The stripe menu's "Move to …" sides for `window`: all but its own (behaviour 5).
    pub fn move_targets(&self, window: ToolWindow) -> Vec<Side> {
        let own = self.side(window);
        Side::ALL.into_iter().filter(|s| *s != own).collect()
    }

    /// The placements `window` can move to: all six but its own (`SPEC-0061` v7 behaviour 37).
    pub fn placement_targets(&self, window: ToolWindow) -> Vec<Placement> {
        let own = self.placement(window);
        Placement::ALL.into_iter().filter(|p| *p != own).collect()
    }

    /// The windows to hide when `window` is shown: the shown ones of the placement it is shown
    /// in, on every side (behaviour 4; `SPEC-0061` v10 behaviour 49, `OBS-0020` item 2). Float
    /// and Window windows take no placement.
    pub fn displaced_by(&self, window: ToolWindow, shown: &[ToolWindow]) -> Vec<ToolWindow> {
        if self.mode(window).separate() {
            return Vec::new();
        }
        let placement = self.shown_placement(window);
        shown
            .iter()
            .copied()
            .filter(|w| {
                *w != window && !self.mode(*w).separate() && self.shown_placement(*w) == placement
            })
            .collect()
    }

    /// The windows to hide after the shown set went from `before` to `shown`, whichever way the
    /// windows became shown (`SPEC-0061` v10 behaviour 49): a window that appeared hides the others
    /// of its placement; of several that appeared in one placement the `focused` one stays, else
    /// the first in stripe order. Windows of different placements are left alone.
    pub fn newcomers_displace(
        &self,
        before: &[ToolWindow],
        shown: &[ToolWindow],
        focused: Option<ToolWindow>,
    ) -> Vec<ToolWindow> {
        let mut hide: Vec<ToolWindow> = Vec::new();
        for placement in Placement::ALL {
            let here: Vec<ToolWindow> = self
                .shown_in_half(placement)
                .into_iter()
                .filter(|w| shown.contains(w) && !self.mode(*w).separate())
                .collect();
            if here.len() < 2 {
                continue;
            }
            let new: Vec<ToolWindow> = here
                .iter()
                .copied()
                .filter(|w| !before.contains(w))
                .collect();
            let pool = if new.is_empty() { &here } else { &new };
            let keep = focused.filter(|f| pool.contains(f)).unwrap_or(pool[0]);
            hide.extend(here.into_iter().filter(|w| *w != keep));
        }
        hide
    }

    /// The other half of `window`'s side, where ⌥-click shows it (`SPEC-0061` v10 behaviour 50).
    pub fn other_half(&self, window: ToolWindow) -> Placement {
        let p = self.placement(window);
        Placement::new(p.side, !p.split)
    }

    /// The placement `window` is shown in: the other half after ⌥-click, else its own.
    pub fn shown_placement(&self, window: ToolWindow) -> Placement {
        self.shown_elsewhere
            .iter()
            .find(|(w, _)| *w == window)
            .map_or_else(|| self.placement(window), |(_, p)| *p)
    }

    /// Whether `window` is shown in the other half by ⌥-click.
    pub fn is_shown_elsewhere(&self, window: ToolWindow) -> bool {
        self.shown_elsewhere.iter().any(|(w, _)| *w == window)
    }

    /// Lets `window` be shown in the other half of its side until
    /// [`Layout::clear_shown_elsewhere`]; the saved placement stays (`SPEC-0061` v10 behaviour 50).
    pub fn show_in_other_half(&mut self, window: ToolWindow) {
        let p = self.other_half(window);
        self.shown_elsewhere.retain(|(w, _)| *w != window);
        self.shown_elsewhere.push((window, p));
    }

    /// Ends the ⌥-click exception of `window`, after it was hidden or moved.
    pub fn clear_shown_elsewhere(&mut self, window: ToolWindow) {
        self.shown_elsewhere.retain(|(w, _)| *w != window);
    }

    /// The windows shown in `placement` when shown, in stripe order: its own windows less those
    /// ⌥-clicked into the other half, plus those ⌥-clicked into this one.
    pub fn shown_in_half(&self, placement: Placement) -> Vec<ToolWindow> {
        self.order
            .iter()
            .map(|(w, _)| *w)
            .filter(|w| self.shown_placement(*w) == placement)
            .collect()
    }

    /// The view mode of `window` (`SPEC-0061` v7 behaviour 39).
    pub fn mode(&self, window: ToolWindow) -> ViewMode {
        self.modes
            .iter()
            .find(|(w, _)| *w == window)
            .map_or(ViewMode::DockPinned, |(_, m)| *m)
    }

    /// Sets `window`'s view mode; returns whether it changed.
    pub fn set_mode(&mut self, window: ToolWindow, mode: ViewMode) -> bool {
        if self.mode(window) == mode {
            return false;
        }
        self.modes.retain(|(w, _)| *w != window);
        if mode != ViewMode::DockPinned {
            self.modes.push((window, mode));
            self.modes.sort_by_key(|(w, _)| *w);
        }
        true
    }

    /// Puts every window back to its default side, half and order and drops sizes, view modes,
    /// removed buttons and saved frames; the stripes show again (`SPEC-0167` v5 behaviour 17).
    /// Returns whether anything changed.
    pub fn restore_default(&mut self) -> bool {
        let default = Self::default();
        let changed = *self != default;
        *self = default;
        changed
    }

    /// The windows off the sidebar in the default layout (`SPEC-0061` v10 behaviour 51).
    fn default_removed() -> Vec<ToolWindow> {
        let mut v: Vec<ToolWindow> = ToolWindow::ALL
            .into_iter()
            .filter(|w| !w.on_sidebar_by_default())
            .collect();
        v.sort();
        v
    }

    /// Whether `window` has no stripe button while it is hidden: removed, or off the sidebar by
    /// default and not added (`SPEC-0061` v7 behaviour 40, v10 behaviour 52).
    pub fn removed(&self, window: ToolWindow) -> bool {
        self.removed.contains(&window)
    }

    /// Takes `window`'s stripe button away; returns whether it changed.
    pub fn remove(&mut self, window: ToolWindow) -> bool {
        if self.removed(window) {
            return false;
        }
        self.removed.push(window);
        self.removed.sort();
        true
    }

    /// Brings `window`'s stripe button back; returns whether it changed.
    pub fn restore(&mut self, window: ToolWindow) -> bool {
        let before = self.removed.len();
        self.removed.retain(|w| *w != window);
        self.removed.len() != before
    }

    /// The last frame of `window` as a Float or Window window (`SPEC-0061` v7 behaviour 39).
    pub fn bounds(&self, window: ToolWindow) -> Option<Rect> {
        self.bounds
            .iter()
            .find(|(w, _)| *w == window)
            .map(|(_, [x, y, w, h])| Rect {
                x: *x as f32,
                y: *y as f32,
                w: *w as f32,
                h: *h as f32,
            })
    }

    /// Keeps `window`'s frame as a Float or Window window, rounded to whole points.
    pub fn set_bounds(&mut self, window: ToolWindow, r: Rect) {
        let v = [
            r.x.round() as i32,
            r.y.round() as i32,
            r.w.max(MIN_TOOL_WINDOW_SIZE).round() as i32,
            r.h.max(MIN_TOOL_WINDOW_SIZE).round() as i32,
        ];
        match self.bounds.iter_mut().find(|(w, _)| *w == window) {
            Some(entry) => entry.1 = v,
            None => {
                self.bounds.push((window, v));
                self.bounds.sort_by_key(|(w, _)| *w);
            }
        }
    }

    /// "Hide all" (behaviour 6): with windows shown, remembers and returns them to hide; with none
    /// shown, returns the windows the last "hide all" hid, to show again.
    pub fn hide_all(&mut self, shown: &[ToolWindow]) -> HideAll {
        if shown.is_empty() {
            HideAll::Restore(std::mem::take(&mut self.hidden_by_all))
        } else {
            self.hidden_by_all = shown.to_vec();
            HideAll::Hide(shown.to_vec())
        }
    }

    /// Forgets what "hide all" hid, after another window was shown.
    pub fn forget_hidden(&mut self) {
        self.hidden_by_all.clear();
    }

    /// Reads the layout of the project at `root`; a missing or broken file gives the default.
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(LAYOUT_FILE))
            .ok()
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    /// Parses the saved form; unknown windows and sides are skipped, missing windows keep their
    /// default side after the saved ones of that side (behaviour 8).
    pub fn parse(text: &str) -> Self {
        let Ok(saved) = serde_json::from_str::<Saved>(text) else {
            return Self::default();
        };
        let mut layout = Self::parse_saved(&saved);
        // A file the user never arranged gets the v10 defaults and keeps sizes, modes, frames
        // and the stripes setting (`SPEC-0061` v10 behaviour 54).
        if saved.version < LAYOUT_VERSION && untouched_v7(&saved) {
            let fresh = Self::default();
            layout.order = fresh.order;
            layout.removed = fresh.removed;
            layout.migrated = true;
        }
        layout
    }

    fn parse_saved(saved: &Saved) -> Self {
        let mut order: Vec<(ToolWindow, Placement)> = Vec::new();
        let mut sizes = Vec::new();
        let mut modes = Vec::new();
        let mut removed = Vec::new();
        let mut bounds = Vec::new();
        for w in &saved.windows {
            if let (Some(window), Some(side)) =
                (ToolWindow::from_id(&w.id), Side::from_name(&w.side))
                && !order.iter().any(|(o, _)| *o == window)
            {
                // A missing `split` is the window's default half on that side.
                let split = w
                    .split
                    .unwrap_or_else(|| window.default_placement() == Placement::new(side, true));
                order.push((window, Placement::new(side, split)));
                if let Some(size) = w.size {
                    sizes.push((window, size.max(MIN_TOOL_WINDOW_SIZE as u32)));
                }
                // An unknown mode reads as Dock Pinned (`SPEC-0061` v7 behaviour 45).
                if let Some(mode) = w.mode.as_deref().and_then(ViewMode::from_id)
                    && mode != ViewMode::DockPinned
                {
                    modes.push((window, mode));
                }
                if w.removed {
                    removed.push(window);
                }
                if let Some(b) = w.bounds {
                    bounds.push((window, b));
                }
            }
        }
        // Windows the file does not list take their default placement and sidebar.
        for window in DEFAULT_ORDER {
            if !order.iter().any(|(o, _)| *o == window) {
                order.push((window, window.default_placement()));
                if !window.on_sidebar_by_default() {
                    removed.push(window);
                }
            }
        }
        sizes.sort_by_key(|(w, _)| *w);
        modes.sort_by_key(|(w, _)| *w);
        removed.sort();
        removed.dedup();
        bounds.sort_by_key(|(w, _)| *w);
        Self {
            order,
            stripes: saved.stripes,
            hidden_by_all: Vec::new(),
            sizes,
            modes,
            removed,
            bounds,
            shown_elsewhere: Vec::new(),
            migrated: false,
        }
    }

    /// The saved form as JSON.
    pub fn to_json(&self) -> String {
        let saved = Saved {
            version: LAYOUT_VERSION,
            windows: self
                .order
                .iter()
                .map(|(w, p)| SavedWindow {
                    id: w.id().into(),
                    side: p.side.name().into(),
                    size: self.size(*w).map(|v| v.round() as u32),
                    // Written when it matters: a second half, or a window whose default is one.
                    split: (p.split || w.default_placement().split).then_some(p.split),
                    mode: (self.mode(*w) != ViewMode::DockPinned)
                        .then(|| self.mode(*w).id().into()),
                    removed: self.removed(*w),
                    bounds: self.bounds.iter().find(|(b, _)| b == w).map(|(_, v)| *v),
                })
                .collect(),
            stripes: self.stripes,
        };
        serde_json::to_string_pretty(&saved).unwrap_or_default() + "\n"
    }

    /// Writes the layout of the project at `root`.
    pub fn save(&self, root: &Path) -> std::io::Result<PathBuf> {
        let path = root.join(LAYOUT_FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::keymap::write_atomic(&path, self.to_json())?;
        Ok(path)
    }
}

/// Whether a saved layout of before `SPEC-0061` v10 is the v7 default the user never arranged:
/// every listed window at its v7 default placement, in the v7 default order within each placement,
/// and none removed (v10 behaviour 54). Sizes, modes and frames do not count as arranging.
fn untouched_v7(saved: &Saved) -> bool {
    let mut last: Vec<(Placement, usize)> = Vec::new();
    for w in &saved.windows {
        let (Some(window), Some(side)) = (ToolWindow::from_id(&w.id), Side::from_name(&w.side))
        else {
            continue;
        };
        let default = window.v7_default_placement();
        let split = w.split.unwrap_or(default.split && side == default.side);
        if w.removed || Placement::new(side, split) != default {
            return false;
        }
        // The v7 default order is `ToolWindow::ALL`.
        let index = ToolWindow::ALL
            .iter()
            .position(|x| *x == window)
            .unwrap_or(0);
        match last.iter_mut().find(|(p, _)| *p == default) {
            Some((_, i)) if *i > index => return false,
            Some((_, i)) => *i = index,
            None => last.push((default, index)),
        }
    }
    true
}

/// What a tool window's activation shortcut (⌘1, ⌘4, ⌘5, ⌘6, ⌘8, ⌘9, ⌥F12, …) does, as in the
/// reference: a hidden window is shown and gets the keyboard focus, a shown window without focus
/// gets the focus, and only a window that already has the focus is hidden, with the focus back in
/// the editor (behaviour 2; hand test 7 of round 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// Show the window and focus it.
    ShowAndFocus,
    /// The window is shown: focus it.
    Focus,
    /// The window is shown and focused: hide it and focus the editor.
    HideAndFocusEditor,
}

impl Activation {
    /// The activation for a window that is `shown` and whose content `focused`.
    pub fn of(shown: bool, focused: bool) -> Self {
        match (shown, focused) {
            (false, _) => Activation::ShowAndFocus,
            (true, false) => Activation::Focus,
            (true, true) => Activation::HideAndFocusEditor,
        }
    }

    /// The window ends up focused.
    pub fn focuses_window(self) -> bool {
        self != Activation::HideAndFocusEditor
    }
}

/// What "hide all" does (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HideAll {
    /// Hide these shown windows.
    Hide(Vec<ToolWindow>),
    /// Show these windows again.
    Restore(Vec<ToolWindow>),
}

/// The More tool windows popup's entries: the existing windows by title in alphabetical order
/// (`SPEC-0061` v7 behaviour 41).
pub fn more_tool_windows(existing: &[ToolWindow]) -> Vec<ToolWindow> {
    let mut all = existing.to_vec();
    all.sort_by_key(|w| w.title().to_lowercase());
    all.dedup();
    all
}

/// How much a saved frame must overlap a display to be used again (IMP-0313).
pub const FLOAT_MIN_VISIBLE: f32 = 40.0;

/// The frame a Float or Window tool window opens at: its `saved` frame when that overlaps one of
/// the `displays` by at least [`FLOAT_MIN_VISIBLE`] in both directions, otherwise `size` centred on
/// the IDE window `main` (`SPEC-0061` v7 behaviour 39, IMP-0313).
pub fn float_bounds(saved: Option<Rect>, displays: &[Rect], main: Rect, size: (f32, f32)) -> Rect {
    let visible = |r: &Rect| {
        displays.iter().any(|d| {
            let w = (r.x + r.w).min(d.x + d.w) - r.x.max(d.x);
            let h = (r.y + r.h).min(d.y + d.h) - r.y.max(d.y);
            w >= FLOAT_MIN_VISIBLE && h >= FLOAT_MIN_VISIBLE
        })
    };
    if let Some(r) = saved.filter(visible) {
        return r;
    }
    let (w, h) = (
        size.0.min(main.w).max(MIN_TOOL_WINDOW_SIZE),
        size.1.min(main.h).max(MIN_TOOL_WINDOW_SIZE),
    );
    Rect {
        x: main.x + (main.w - w) / 2.0,
        y: main.y + (main.h - h) / 2.0,
        w,
        h,
    }
}

/// Which Dock Unpinned windows had the focus, so that they hide once it moves elsewhere in the
/// IDE window (`SPEC-0061` v7 behaviour 39, IMP-0314).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoHide {
    armed: Vec<ToolWindow>,
}

impl AutoHide {
    /// Looks at a shown auto-hiding `window`: whether it has the focus and whether the IDE window
    /// is the active one. Returns `true` when the window should hide now: it had the focus and
    /// the focus moved to another part of the same, still active, IDE window. Switching to
    /// another application keeps it shown.
    pub fn observe(&mut self, window: ToolWindow, focused: bool, ide_active: bool) -> bool {
        if focused {
            if !self.armed.contains(&window) {
                self.armed.push(window);
            }
            false
        } else if !ide_active {
            false
        } else {
            let before = self.armed.len();
            self.armed.retain(|w| *w != window);
            self.armed.len() != before
        }
    }

    /// Forgets `window`, after it was hidden or its mode changed.
    pub fn forget(&mut self, window: ToolWindow) {
        self.armed.retain(|w| *w != window);
    }
}

/// Height the editor keeps when bottom windows have to shrink (v2 behaviour 10).
pub const EDITOR_MIN_HEIGHT: f32 = 120.0;
/// Height a bottom window keeps when it has to shrink, as long as the column has it (v2 behaviour 10).
pub const BOTTOM_MIN_HEIGHT: f32 = 60.0;

/// Splits the height of the editor column among the editor and `count` stacked bottom windows, each
/// separated by `gap` (v2 behaviour 10): every bottom window gets [`BOTTOM_HEIGHT`] when it fits,
/// otherwise they share what is left after the editor's [`EDITOR_MIN_HEIGHT`] equally. The parts never
/// add up to more than `column`, so nothing reaches into the status bar below. Returns the editor's
/// height and the bottom windows' heights.
pub fn bottom_heights(column: f32, gap: f32, count: usize) -> (f32, Vec<f32>) {
    bottom_heights_in(column, gap, count, BOTTOM_HEIGHT)
}

/// [`bottom_heights`] with `preferred` as the height each bottom window gets when it fits; the
/// workbench passes a third of the window less the gap (`SPEC-0061` v6 behaviour 22).
pub fn bottom_heights_in(column: f32, gap: f32, count: usize, preferred: f32) -> (f32, Vec<f32>) {
    let column = column.max(0.0);
    if count == 0 {
        return (column, Vec::new());
    }
    let n = count as f32;
    let available = (column - gap * n).max(0.0);
    let room = (available - EDITOR_MIN_HEIGHT)
        .max(BOTTOM_MIN_HEIGHT * n)
        .min(available);
    let total = (preferred * n).min(room);
    (available - total, vec![total / n; count])
}

/// A rectangle in window coordinates (logical pixels, origin top left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Whether the two rectangles share any area; touching edges and rounding below a hundredth of
    /// a pixel do not count.
    pub fn intersects(&self, other: &Rect) -> bool {
        const EPS: f32 = 0.01;
        self.x + EPS < other.x + other.w
            && other.x + EPS < self.x + self.w
            && self.y + EPS < other.y + other.h
            && other.y + EPS < self.y + self.h
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// What the main window frame is laid out from (v2 behaviour 10).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameInput {
    /// Window content width.
    pub width: f32,
    /// Window content height.
    pub height: f32,
    /// Height of the main toolbar.
    pub toolbar: f32,
    /// Height of the status bar.
    pub status: f32,
    /// Island gap.
    pub gap: f32,
    /// Space between a stripe (or the window edge) and the islands.
    pub edge: f32,
    /// Space between the toolbar and the islands.
    pub top: f32,
    /// Stripe width, `None` while the stripes are hidden.
    pub stripe: Option<f32>,
    /// Widths of the shown left windows.
    pub left: Vec<f32>,
    /// Widths of the shown right windows.
    pub right: Vec<f32>,
    /// Number of stacked bottom windows, both parts.
    pub bottom: usize,
    /// The first left column holds a Left Top and a Left Bottom window (`SPEC-0061` v7
    /// behaviour 37).
    pub left_split: bool,
    /// The first right column holds a Right Top and a Right Bottom window.
    pub right_split: bool,
    /// How many of the `bottom` windows are Bottom Right windows, laid out in the right part.
    pub bottom_right: usize,
    /// Undock windows drawn over the editor area: their side and their width (left, right) or
    /// height (bottom) (`SPEC-0061` v7 behaviour 39).
    pub undocked: Vec<(Side, f32)>,
}

/// Where everything of the main window frame sits (v2 behaviour 10).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The status bar: the full width at the foot of the window.
    pub status: Rect,
    /// The left and right stripes.
    pub stripes: Vec<Rect>,
    /// Left and right tool windows.
    pub sides: Vec<Rect>,
    /// The editor island.
    pub editor: Rect,
    /// The stacked bottom windows, top to bottom: the Bottom Left part, then the Bottom Right part.
    pub bottom: Vec<Rect>,
    /// Undock windows over the editor area, in `FrameInput::undocked` order.
    pub overlays: Vec<Rect>,
}

impl Frame {
    /// Every island and stripe (all but the status bar), overlays included.
    pub fn islands(&self) -> Vec<Rect> {
        let mut all = self.stripes.clone();
        all.extend(self.sides.iter().copied());
        all.push(self.editor);
        all.extend(self.bottom.iter().copied());
        all.extend(self.overlays.iter().copied());
        all
    }
}

/// `count` rectangles stacked in `area` from the top with `gap` between them.
fn stack(area: Rect, count: usize, gap: f32) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let h = ((area.h - gap * (count as f32 - 1.0)) / count as f32).max(0.0);
    (0..count)
        .map(|i| Rect {
            x: area.x,
            y: area.y + i as f32 * (h + gap),
            w: area.w,
            h,
        })
        .collect()
}

/// Lays out the main window: toolbar on top, status bar across the full width at the foot, the
/// stripes at the left and right edge and between them the islands, `edge` away from the stripes
/// and `top` below the toolbar (`SPEC-0061` v6 behaviour 20a). The islands are, left to right, the
/// left windows, the editor and the right windows, and below all of them the stacked bottom
/// windows across the full width (v6 behaviour 21), separated by the island gap (v2 behaviour
/// 10). A bottom window is a third of the window high less the gap when it fits (v6 behaviour 22).
pub fn frame(input: &FrameInput) -> Frame {
    let status = Rect {
        x: 0.0,
        y: (input.height - input.status).max(0.0),
        w: input.width,
        h: input.status.min(input.height),
    };
    let toolbar = input.toolbar.min(status.y);
    let top = (toolbar + input.top).min(status.y);
    let row = status.y - top;
    let gap = input.gap;
    let stripe = input.stripe.unwrap_or(0.0);
    let mut stripes = Vec::new();
    if let Some(s) = input.stripe {
        for x in [0.0, input.width - s] {
            stripes.push(Rect {
                x,
                y: toolbar,
                w: s,
                h: status.y - toolbar,
            });
        }
    }
    let left_edge = stripe + input.edge;
    let right_edge = (input.width - stripe - input.edge).max(left_edge);
    let preferred = DEFAULT_WEIGHT * input.height - gap;
    // The two bottom parts are as high as the one with more windows needs (`SPEC-0061` v7
    // behaviour 37).
    let right_count = input.bottom_right.min(input.bottom);
    let left_count = input.bottom - right_count;
    let rows = left_count.max(right_count);
    let (upper, heights) = bottom_heights_in(row, gap, rows, preferred);
    // A column of a left or right window, split into a top and a bottom half when `split`.
    let column = |x: f32, w: f32, split: bool, sides: &mut Vec<Rect>| {
        let r = Rect {
            x,
            y: top,
            w,
            h: upper,
        };
        if split {
            sides.extend(stack(r, 2, gap));
        } else {
            sides.push(r);
        }
    };
    let mut sides = Vec::new();
    let mut x = left_edge;
    for (i, &w) in input.left.iter().enumerate() {
        column(x, w, i == 0 && input.left_split, &mut sides);
        x += w + gap;
    }
    let right_used: f32 = input.right.iter().map(|w| w + gap).sum();
    let column_w = (right_edge - x - right_used).max(0.0);
    let editor = Rect {
        x,
        y: top,
        w: column_w,
        h: upper,
    };
    x += column_w + gap;
    for (i, &w) in input.right.iter().enumerate() {
        column(x, w, i == 0 && input.right_split, &mut sides);
        x += w + gap;
    }
    let mut bottom = Vec::new();
    if rows > 0 {
        let area_h = heights.iter().sum::<f32>() + gap * (rows as f32 - 1.0);
        let full = right_edge - left_edge;
        let both = left_count > 0 && right_count > 0;
        let part_w = if both { (full - gap) / 2.0 } else { full };
        let y = top + upper + gap;
        let left_part = Rect {
            x: left_edge,
            y,
            w: part_w,
            h: area_h,
        };
        let right_part = Rect {
            x: if both {
                left_edge + part_w + gap
            } else {
                left_edge
            },
            ..left_part
        };
        bottom.extend(stack(left_part, left_count, gap));
        bottom.extend(stack(right_part, right_count, gap));
    }
    // Undock windows lie over the editor area at their side (`SPEC-0061` v7 behaviour 39).
    let overlays = input
        .undocked
        .iter()
        .map(|&(side, size)| match side {
            Side::Left => Rect {
                x: left_edge,
                y: top,
                w: size.min(right_edge - left_edge),
                h: upper,
            },
            Side::Right => {
                let w = size.min(right_edge - left_edge);
                Rect {
                    x: right_edge - w,
                    y: top,
                    w,
                    h: upper,
                }
            }
            Side::Bottom => {
                let h = size.min(row);
                Rect {
                    x: left_edge,
                    y: status.y - h,
                    w: right_edge - left_edge,
                    h,
                }
            }
        })
        .collect();
    Frame {
        status,
        stripes,
        sides,
        editor,
        bottom,
        overlays,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ToolWindow::*;

    /// Hand test 7 of round 4: the activation shortcut moves the keyboard focus into the window;
    /// only a second press while it has the focus hides it.
    #[test]
    fn activation_focuses_before_it_hides() {
        assert_eq!(Activation::of(false, false), Activation::ShowAndFocus);
        assert_eq!(Activation::of(false, true), Activation::ShowAndFocus);
        assert_eq!(Activation::of(true, false), Activation::Focus);
        assert_eq!(Activation::of(true, true), Activation::HideAndFocusEditor);
        assert!(Activation::Focus.focuses_window());
        assert!(!Activation::HideAndFocusEditor.focuses_window());
        // The macOS shortcuts of the hand test reach the activation actions.
        let mac = crate::keymap::Keymap::bundled_macos();
        for (keys, window) in [
            ("meta 1", Project),
            ("meta 4", Run),
            ("meta 5", Debug),
            ("meta 6", Problems),
            ("meta 8", Services),
            ("meta 9", Git),
            ("alt F12", Terminal),
            ("meta 0", Commit),
        ] {
            let stroke = crate::keymap::Keystroke::parse(keys).unwrap();
            assert!(
                mac.actions_for(&stroke, None).contains(&window.action()),
                "{keys} does not activate {}",
                window.title()
            );
        }
    }

    /// SPEC-0167 v3 behaviour 11 as a sequence of presses, as the window self-test drives it: a
    /// shown window without the focus stays shown after the first press and hides after the second;
    /// a focused one hides at once; a hidden one comes back focused.
    #[test]
    fn a_shown_window_hides_on_the_second_press_from_the_editor() {
        // (shown, focused) after one press of the activation shortcut.
        let press = |(shown, focused): (bool, bool)| match Activation::of(shown, focused) {
            Activation::ShowAndFocus | Activation::Focus => (true, true),
            Activation::HideAndFocusEditor => (false, false),
        };
        let from_editor = (true, false);
        assert_eq!(press(from_editor), (true, true));
        assert_eq!(press(press(from_editor)), (false, false));
        assert_eq!(press((true, true)), (false, false));
        assert_eq!(press((false, false)), (true, true));
        // Pressing until hidden never takes more than two presses.
        for start in [(true, false), (true, true)] {
            let mut state = start;
            let mut presses = 0;
            while state.0 {
                state = press(state);
                presses += 1;
            }
            assert!(presses <= 2, "{start:?} took {presses} presses");
        }
    }

    /// SPEC-0061 v5 AK-11: every window of the layout but Problems has the shared header.
    #[test]
    fn headers_of_every_tool_window() {
        for w in ToolWindow::ALL {
            let h = w.header();
            assert_eq!(h.title, w.title());
            // SPEC-0061 v6 behaviour 23: Problems too; v7 behaviour 43: the former panels keep
            // their own header row, with Options and Hide.
            assert_eq!(h.shared, !w.own_header(), "{w:?}");
            assert_eq!(h.actions, HEADER_ACTIONS);
            // v6 behaviour 24: a line below every shared header but Project's.
            assert_eq!(h.line, w != Project && !w.own_header(), "{w:?}");
            // SPEC-0061 v7 AK-26: ids, titles and actions are unique, ids parse back.
            assert_eq!(ToolWindow::from_id(w.id()), Some(w));
            assert!(!w.action().is_empty() && !w.icon().is_empty());
        }
        let ids: std::collections::HashSet<_> = ToolWindow::ALL.iter().map(|w| w.id()).collect();
        let actions: std::collections::HashSet<_> =
            ToolWindow::ALL.iter().map(|w| w.action()).collect();
        assert_eq!(ids.len(), ToolWindow::ALL.len());
        assert_eq!(actions.len(), ToolWindow::ALL.len());
        assert!(Structure.header().shared && !Structure.own_header());
        assert_eq!(Structure.action(), "ActivateStructureToolWindow");
        assert_eq!(Find.action(), "ActivateFindToolWindow");
        assert_eq!(ToolWindow::from_id("find_in_files"), Some(FindInFiles));
        assert_eq!(Problems.header().content_tabs, Some("scopes"));
        assert_eq!(
            Project.header().buttons,
            ["new", "locate", "expand_all", "collapse_all"]
        );
        assert_eq!(Terminal.header().content_tabs, Some("shells"));
        assert_eq!(Run.header().content_tabs, Some("sessions"));
        assert_eq!(Debug.header().content_tabs, Some("sessions"));
        assert_eq!(Services.header().content_tabs, Some("results"));
        assert_eq!(Git.header().content_tabs, Some("log"));
        assert_eq!(Commit.header().content_tabs, None);
        assert_eq!(Run.header().toolbar, ["stop"]);
        assert_eq!(Database.header().toolbar, ["new", "edit", "refresh"]);
        assert!(
            Debug.header().toolbar.is_empty(),
            "the Debug toolbar stays below"
        );
    }

    /// SPEC-0334 AK-10: the TODO window's id, title, action, place and icons.
    #[test]
    fn todo_window_is_registered() {
        assert_eq!(ToolWindow::from_id("todo"), Some(Todo));
        assert_eq!(Todo.title(), "TODO");
        assert_eq!(Todo.action(), "ActivateTODOToolWindow");
        assert_eq!(Todo.default_side(), Side::Bottom);
        assert!(!Todo.on_sidebar_by_default() && Layout::default().removed(Todo));
        assert_eq!(Todo.icon(), "toolwindows/todo");
        assert_eq!(Todo.stripe_icon(), "toolwindows/todo@20x20");
        for key in [Todo.icon(), Todo.stripe_icon()] {
            assert!(
                crate::icons::svg(key, true).is_some() && crate::icons::svg(key, false).is_some()
            );
        }
        // The shared header with the scopes as content tabs.
        let header = Todo.header();
        assert!(header.shared && !Todo.own_header());
        assert_eq!(header.content_tabs, Some("scopes"));
        // The bundled keymaps give the window no shortcut.
        assert!(
            crate::keymap::Keymap::bundled_macos()
                .shortcuts(Todo.action())
                .is_empty()
        );
        assert!(
            crate::keymap::Keymap::bundled_default()
                .shortcuts(Todo.action())
                .is_empty()
        );
    }

    /// SPEC-0061 v17 AK-v17a, AK-v17b: the open Structure icon; the Notifications window's id,
    /// title, action, place, icons and header.
    #[test]
    fn f086_icons_of_oss0046() {
        assert_eq!(Structure.icon(), "toolwindows/structure");
        assert_eq!(Structure.stripe_icon(), "toolwindows/structure@20x20");
        assert_eq!(ToolWindow::from_id("notifications"), Some(Notifications));
        assert_eq!(Notifications.title(), "Notifications");
        assert_eq!(Notifications.action(), "ActivateNotificationsToolWindow");
        assert_eq!(
            Notifications.default_placement(),
            Placement::new(Side::Right, false)
        );
        assert!(Notifications.on_sidebar_by_default());
        let l = Layout::default();
        assert!(!l.removed(Notifications));
        assert_eq!(l.stripe(Side::Right).top.first(), Some(&Notifications));
        assert_eq!(Notifications.icon(), "toolwindows/notifications");
        assert_eq!(
            Notifications.stripe_icon(),
            "toolwindows/notifications@20x20"
        );
        for w in [Structure, Notifications] {
            for key in [w.icon(), w.stripe_icon()] {
                assert!(
                    crate::icons::svg(key, true).is_some()
                        && crate::icons::svg(key, false).is_some(),
                    "{key}"
                );
            }
        }
        let header = Notifications.header();
        assert!(header.shared && !Notifications.own_header());
        assert_eq!(header.content_tabs, None);
        assert_eq!(header.toolbar, ["clear_all"]);
        for keymap in [
            crate::keymap::Keymap::bundled_macos(),
            crate::keymap::Keymap::bundled_default(),
        ] {
            assert!(keymap.shortcuts(Notifications.action()).is_empty());
        }
        // A layout saved before v14 gets the window at the end of its side.
        let saved = Layout::default()
            .to_json()
            .replace("\"notifications\"", "\"gone\"");
        let parsed = Layout::parse(&saved);
        assert_eq!(
            parsed.placement(Notifications),
            Placement::new(Side::Right, false)
        );
        assert!(!parsed.removed(Notifications));
    }

    #[test]
    fn default_sides_and_stripe_groups() {
        let l = Layout::default();
        // Bookmarks docks left and is off the sidebar (SPEC-0331 behaviour 22).
        assert_eq!(
            l.on(Side::Left),
            vec![Project, Commit, Structure, Bookmarks]
        );
        assert_eq!(
            l.on(Side::Bottom),
            vec![
                Git,
                Problems,
                Terminal,
                Services,
                Run,
                Debug,
                Find,
                FindInFiles,
                Endpoints,
                Spring,
                Python,
                Containers,
                Profiler,
                Remote,
                Todo
            ]
        );
        // SPEC-0061 v10 behaviour 51: AI Chat, Database and Build on the right.
        assert_eq!(
            l.on(Side::Right),
            vec![Notifications, AiChat, Database, Build, Plugins]
        );
        // Ferris IDEA's own and rare windows are off the sidebar.
        for w in [
            Plugins, Endpoints, Spring, Python, Containers, Profiler, Remote, Bookmarks, Todo,
        ] {
            assert!(l.removed(w) && !w.on_sidebar_by_default(), "{w:?}");
        }
        for w in [
            Project, Commit, Structure, Git, Problems, Terminal, Services, Run, Debug,
        ] {
            assert!(!l.removed(w), "{w:?}");
        }
        assert!(!l.removed(AiChat) && !l.removed(Database) && !l.removed(Build));
        assert_eq!(DEFAULT_ORDER.len(), ToolWindow::ALL.len());
        for w in ToolWindow::ALL {
            assert!(DEFAULT_ORDER.contains(&w));
        }
        assert!(l.stripes);
        // SPEC-0061 v6 behaviour 22: a third of the window less the gap.
        assert!((Terminal.width_in(1400.0, 5.0) - 457.0).abs() < 0.01);
        assert_eq!(Database.width(), Terminal.width());
        assert_eq!(Project.width_in(600.0, 5.0), SIDE_MIN_WIDTH);
    }

    /// SPEC-0061 v4 AK-7: the reference's default order; the foot group stacks from the foot up,
    /// so the version control window sits lowest; stripe icons are the 20 px ones where open.
    #[test]
    fn stripe_groups_follow_the_reference() {
        let mut l = Layout::default();
        let left = l.stripe(Side::Left);
        assert_eq!(left.top, vec![Project, Commit]);
        // SPEC-0061 v7 behaviour 38: Structure after the line.
        assert_eq!(left.split, vec![Structure]);
        assert_eq!(
            &left.foot[left.foot.len() - 6..],
            [Debug, Run, Services, Terminal, Problems, Git]
        );
        let right = l.stripe(Side::Right);
        // Plugins is off the sidebar (SPEC-0061 v10 behaviour 51).
        assert_eq!(right.top, vec![Notifications, AiChat, Database, Build]);
        assert!(right.split.is_empty() && right.foot.is_empty());
        // A window moved to the bottom is the newest there: highest in the foot group.
        assert!(l.move_to(Project, Side::Bottom));
        assert_eq!(l.stripe(Side::Left).foot[0], Project);
        // Bottom Right windows sit at the foot of the right stripe; removed ones have no button.
        assert!(l.place(Terminal, Placement::new(Side::Bottom, true)));
        assert_eq!(l.stripe(Side::Right).foot, vec![Terminal]);
        assert!(!l.stripe(Side::Left).foot.contains(&Terminal));
        assert!(l.remove(Terminal));
        assert!(l.stripe(Side::Right).foot.is_empty());
        assert!(l.restore(Terminal));
        assert_eq!(l.stripe(Side::Right).foot, vec![Terminal]);
        assert_eq!(Git.stripe_icon(), "toolwindows/vcs@20x20");
        assert_eq!(Database.stripe_icon(), Database.icon());
        assert_eq!(Structure.stripe_icon(), "toolwindows/structure@20x20");
    }

    /// SPEC-0061 v7 AK-23: the six placements, one window per half on the left and right.
    #[test]
    fn placements_and_split_halves() {
        let names: Vec<&str> = Placement::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(
            names,
            [
                "left_top",
                "left_bottom",
                "bottom_left",
                "bottom_right",
                "right_top",
                "right_bottom"
            ]
        );
        for p in Placement::ALL {
            assert_eq!(Placement::from_name(p.name()), Some(p));
            assert_eq!(Placement::from_name(p.label()), Some(p));
        }
        assert_eq!(Placement::from_name("top_left"), None);
        let mut l = Layout::default();
        assert_eq!(l.placement(Structure), Placement::new(Side::Left, true));
        assert_eq!(l.placement(Project), Placement::new(Side::Left, false));
        assert_eq!(l.placement_targets(Project).len(), 5);
        assert!(!l.placement_targets(Project).contains(&l.placement(Project)));
        // Structure (Left Bottom) does not displace Project (Left Top); Commit does.
        assert!(l.displaced_by(Structure, &[Project]).is_empty());
        assert_eq!(l.displaced_by(Commit, &[Project, Structure]), vec![Project]);
        // Two windows in the same second half displace each other.
        assert!(l.place(Database, Placement::new(Side::Left, true)));
        assert_eq!(
            l.displaced_by(Database, &[Structure, Project]),
            vec![Structure]
        );
        assert!(!l.place(Database, Placement::new(Side::Left, true)));
        assert_eq!(
            l.in_half(Placement::new(Side::Left, true)),
            vec![Structure, Database]
        );
        // Moving to a side by its name keeps to the first half.
        assert!(l.move_to(Structure, Side::Right));
        assert_eq!(l.placement(Structure), Placement::new(Side::Right, false));
        // At the bottom one window per part (SPEC-0061 v10 behaviour 49): Git at Bottom Right
        // stays beside Terminal, Problems in Terminal's part goes.
        assert!(l.place(Git, Placement::new(Side::Bottom, true)));
        assert_eq!(l.displaced_by(Terminal, &[Git, Problems]), vec![Problems]);
    }

    /// SPEC-0061 v7 AK-25: modes, removed buttons and frames survive the file; unknown modes read
    /// as Dock Pinned.
    #[test]
    fn modes_removed_and_bounds_are_saved() {
        let mut l = Layout::default();
        assert_eq!(l.mode(Terminal), ViewMode::DockPinned);
        assert!(l.set_mode(Terminal, ViewMode::Float));
        assert!(!l.set_mode(Terminal, ViewMode::Float));
        assert!(l.set_mode(Project, ViewMode::DockUnpinned));
        assert!(l.set_mode(Run, ViewMode::Undock));
        assert!(l.remove(Git));
        assert!(!l.remove(Git));
        l.set_bounds(
            Terminal,
            Rect {
                x: 100.4,
                y: 80.0,
                w: 640.0,
                h: 30.0,
            },
        );
        assert!(l.place(Structure, Placement::new(Side::Right, false)));
        assert!(l.place(Commit, Placement::new(Side::Left, true)));
        let text = l.to_json();
        let back = Layout::parse(&text);
        assert_eq!(back, l);
        assert_eq!(back.mode(Terminal), ViewMode::Float);
        assert!(back.removed(Git));
        assert_eq!(
            back.bounds(Terminal),
            Some(Rect {
                x: 100.0,
                y: 80.0,
                w: 640.0,
                h: MIN_TOOL_WINDOW_SIZE
            })
        );
        assert_eq!(
            back.placement(Structure),
            Placement::new(Side::Right, false)
        );
        assert_eq!(back.placement(Commit), Placement::new(Side::Left, true));
        // Dock Pinned again is not written.
        assert!(l.set_mode(Terminal, ViewMode::DockPinned));
        assert!(!l.to_json().contains("\"float\""));
        for m in ViewMode::ALL {
            assert_eq!(ViewMode::from_id(m.id()), Some(m));
            assert_eq!(ViewMode::from_id(m.label()), Some(m));
        }
        assert!(ViewMode::Float.separate() && ViewMode::Window.separate());
        assert!(ViewMode::DockPinned.docked() && ViewMode::DockUnpinned.docked());
        assert!(!ViewMode::Undock.docked() && !ViewMode::Undock.separate());
        // Unknown modes, a missing split of Structure (its default half) and old files.
        let text = r#"{"windows":[{"id":"terminal","side":"bottom","mode":"sideways"},
            {"id":"structure","side":"left"},{"id":"git","side":"bottom","removed":true}]}"#;
        let l = Layout::parse(text);
        assert_eq!(l.mode(Terminal), ViewMode::DockPinned);
        assert_eq!(l.placement(Structure), Placement::new(Side::Left, true));
        assert!(l.removed(Git));
        assert_eq!(l.bounds(Git), None);
    }

    /// SPEC-0061 v7 AK-25, IMP-0313: a saved frame is used while it is on a display, otherwise
    /// the window opens centred on the IDE window.
    #[test]
    fn float_bounds_stay_on_a_display() {
        let display = Rect {
            x: 0.0,
            y: 0.0,
            w: 1512.0,
            h: 982.0,
        };
        let main = Rect {
            x: 100.0,
            y: 50.0,
            w: 1300.0,
            h: 900.0,
        };
        let saved = Rect {
            x: 200.0,
            y: 100.0,
            w: 500.0,
            h: 400.0,
        };
        assert_eq!(
            float_bounds(Some(saved), &[display], main, (457.0, 600.0)),
            saved
        );
        // On an unplugged second display: centred on the IDE window.
        let gone = Rect { x: 1700.0, ..saved };
        let centred = float_bounds(Some(gone), &[display], main, (457.0, 600.0));
        assert_eq!(
            centred,
            Rect {
                x: 100.0 + (1300.0 - 457.0) / 2.0,
                y: 50.0 + (900.0 - 600.0) / 2.0,
                w: 457.0,
                h: 600.0
            }
        );
        // Only a sliver visible is not enough.
        let sliver = Rect {
            x: 1512.0 - 20.0,
            ..saved
        };
        assert_eq!(
            float_bounds(Some(sliver), &[display], main, (457.0, 600.0)),
            centred
        );
        assert_eq!(
            float_bounds(None, &[display], main, (457.0, 600.0)),
            centred
        );
        // Larger than the IDE window: it fits into it.
        let fitted = float_bounds(None, &[display], main, (4000.0, 4000.0));
        assert_eq!((fitted.w, fitted.h), (1300.0, 900.0));
    }

    /// SPEC-0061 v7 AK-25, IMP-0314: a Dock Unpinned window hides once the focus left it for
    /// another part of the IDE window, not when another application takes the focus.
    #[test]
    fn unpinned_windows_hide_when_the_focus_leaves() {
        let mut a = AutoHide::default();
        // Shown but never focused: stays.
        assert!(!a.observe(Terminal, false, true));
        assert!(!a.observe(Terminal, true, true));
        assert!(!a.observe(Terminal, true, true));
        // Another application: stays, still armed.
        assert!(!a.observe(Terminal, false, false));
        assert!(
            a.observe(Terminal, false, true),
            "focus moved to the editor"
        );
        assert!(!a.observe(Terminal, false, true), "only once");
        a.observe(Project, true, true);
        a.forget(Project);
        assert!(!a.observe(Project, false, true));
        assert!(ViewMode::DockUnpinned.hides_on_focus_loss());
        assert!(!ViewMode::DockPinned.hides_on_focus_loss());
    }

    /// SPEC-0061 v7 AK-26: the More tool windows popup is sorted by title.
    #[test]
    fn more_tool_windows_are_sorted() {
        let listed = more_tool_windows(&[Terminal, AiChat, Structure, Project, Find, AiChat]);
        assert_eq!(listed, vec![AiChat, Find, Project, Structure, Terminal]);
        let titles: Vec<&str> = more_tool_windows(&ToolWindow::ALL)
            .into_iter()
            .map(|w| w.title())
            .collect();
        let mut sorted = titles.clone();
        sorted.sort_by_key(|t| t.to_lowercase());
        assert_eq!(titles, sorted);
        assert_eq!(titles.len(), ToolWindow::ALL.len());
    }

    /// SPEC-0061 v7 AK-24: a split left column, two bottom parts side by side, an overlay.
    #[test]
    fn split_columns_and_bottom_parts() {
        let gap = 5.0;
        let f = frame(&FrameInput {
            width: 1400.0,
            height: 954.0,
            toolbar: 40.0,
            status: 32.0,
            gap,
            edge: 3.5,
            top: 3.0,
            stripe: Some(37.0),
            left: vec![457.0],
            right: vec![],
            bottom: 3,
            left_split: true,
            right_split: false,
            bottom_right: 1,
            undocked: vec![(Side::Right, 300.0)],
        });
        assert_eq!(f.sides.len(), 2, "top and bottom half");
        let (top, low) = (f.sides[0], f.sides[1]);
        assert_eq!((top.x, top.w), (low.x, low.w));
        assert!((low.y - (top.y + top.h + gap)).abs() < 0.01);
        assert!((top.h - low.h).abs() < 0.01);
        assert_eq!(f.bottom.len(), 3);
        // Two Bottom Left windows stack in the left half, the Bottom Right one fills the right.
        let (a, b, r) = (f.bottom[0], f.bottom[1], f.bottom[2]);
        assert_eq!(a.x, 40.5);
        assert_eq!(a.w, b.w);
        assert!((r.x - (a.x + a.w + gap)).abs() < 0.01);
        assert_eq!(r.x + r.w, 1359.5);
        assert_eq!(r.y, a.y);
        assert!(
            (r.y + r.h - (b.y + b.h)).abs() < 0.01,
            "both parts equally high"
        );
        assert_eq!(b.y + b.h, f.status.y);
        assert!(!a.intersects(&b) && !b.intersects(&r));
        assert!(!top.intersects(&a) && !low.intersects(&a));
        // The Undock window lies over the editor at the right edge.
        let o = f.overlays[0];
        assert_eq!((o.x + o.w, o.w), (1359.5, 300.0));
        assert!(o.intersects(&f.editor));
        assert_eq!(o.y, f.editor.y);
        // Only Bottom Right windows: they take the full width.
        let only_right = frame(&FrameInput {
            bottom: 1,
            bottom_right: 1,
            left_split: false,
            undocked: vec![(Side::Bottom, 200.0)],
            ..FrameInput {
                width: 1400.0,
                height: 954.0,
                toolbar: 40.0,
                status: 32.0,
                gap,
                edge: 3.5,
                top: 3.0,
                stripe: Some(37.0),
                left: vec![],
                right: vec![],
                bottom: 0,
                left_split: false,
                right_split: false,
                bottom_right: 0,
                undocked: vec![],
            }
        });
        assert_eq!(
            (only_right.bottom[0].x, only_right.bottom[0].w),
            (40.5, 1319.0)
        );
        let o = only_right.overlays[0];
        assert_eq!((o.y + o.h, o.h), (only_right.status.y, 200.0));
    }

    #[test]
    fn moving_puts_a_window_last_on_the_new_side() {
        let mut l = Layout::default();
        assert_eq!(l.move_targets(Terminal), vec![Side::Left, Side::Right]);
        assert!(l.move_to(Terminal, Side::Right));
        assert!(!l.move_to(Terminal, Side::Right));
        assert_eq!(
            l.on(Side::Right),
            vec![Notifications, AiChat, Database, Build, Plugins, Terminal]
        );
        assert_eq!(l.position(Terminal), 5);
        assert!(!l.on(Side::Bottom).contains(&Terminal));
        assert!(l.move_to(Terminal, Side::Bottom));
        assert_eq!(*l.on(Side::Bottom).last().unwrap(), Terminal);
    }

    /// SPEC-0061 v10 AK-32, SPEC-0167 v4 AK-7: one window per placement on every side, as in
    /// the reference (`OBS-0020` item 2); Float and Window windows take no placement.
    #[test]
    fn one_window_per_placement_replaces() {
        let mut l = Layout::default();
        assert_eq!(l.displaced_by(Commit, &[Project, Terminal]), vec![Project]);
        // The bottom replaces too (formerly stacked, IMP-0311 v6).
        assert_eq!(
            l.displaced_by(Problems, &[Terminal, Git, Project]),
            vec![Terminal, Git]
        );
        l.move_to(Terminal, Side::Right);
        assert_eq!(l.displaced_by(Database, &[Terminal, Git]), vec![Terminal]);
        assert_eq!(
            l.displaced_by(AiChat, &[Database, Build]),
            vec![Database, Build]
        );
        // A floating window neither displaces nor is displaced.
        assert!(l.set_mode(Git, ViewMode::Float));
        assert!(l.displaced_by(Problems, &[Git]).is_empty());
        assert!(l.displaced_by(Git, &[Problems]).is_empty());
        // Every placement holds one window.
        let mut l = Layout::default();
        for p in Placement::ALL {
            let a = l.in_half(p);
            let (x, y) = match a.as_slice() {
                [x, y, ..] => (*x, *y),
                [x] => {
                    // Put a second window there.
                    let y = if *x == Terminal { Git } else { Terminal };
                    l.place(y, p);
                    (*x, y)
                }
                [] => {
                    l.place(Terminal, p);
                    l.place(Git, p);
                    (Terminal, Git)
                }
            };
            assert_eq!(l.displaced_by(x, &[x, y]), vec![y], "{p:?}");
        }
    }

    /// SPEC-0061 v10 AK-32: windows that became shown by any path replace the shown window of
    /// their placement; of several new ones the focused, else the first in stripe order, stays.
    #[test]
    fn newcomers_replace_the_shown_window() {
        let l = Layout::default();
        // Run started while Terminal was shown: Terminal goes.
        assert_eq!(
            l.newcomers_displace(&[Terminal, Project], &[Terminal, Project, Run], None),
            vec![Terminal]
        );
        // Different placements stay together.
        assert!(
            l.newcomers_displace(&[Project], &[Project, Terminal, Database, Structure], None)
                .is_empty()
        );
        // Window start: Problems and Terminal both appear; the first in stripe order stays …
        assert_eq!(
            l.newcomers_displace(&[], &[Terminal, Problems], None),
            vec![Terminal]
        );
        // … unless the other has the focus.
        assert_eq!(
            l.newcomers_displace(&[], &[Terminal, Problems], Some(Terminal)),
            vec![Problems]
        );
        // Nothing new but two shown (an old state): the first stays.
        assert_eq!(
            l.newcomers_displace(&[Git, Terminal], &[Git, Terminal], None),
            vec![Terminal]
        );
    }

    /// SPEC-0061 v10 AK-33, IMP-0311: ⌥-click shows a window in the other half of its side
    /// without moving it; a window there is replaced; hiding ends the exception.
    #[test]
    fn alt_click_opens_in_the_other_half() {
        let mut l = Layout::default();
        assert_eq!(l.other_half(Terminal), Placement::new(Side::Bottom, true));
        assert_eq!(l.other_half(Project), Placement::new(Side::Left, true));
        assert_eq!(l.other_half(Structure), Placement::new(Side::Left, false));
        assert_eq!(l.other_half(Database), Placement::new(Side::Right, true));
        let saved = l.to_json();
        l.show_in_other_half(Terminal);
        assert!(l.is_shown_elsewhere(Terminal));
        assert_eq!(l.placement(Terminal), Placement::new(Side::Bottom, false));
        assert_eq!(
            l.shown_placement(Terminal),
            Placement::new(Side::Bottom, true)
        );
        assert_eq!(
            l.shown_in_half(Placement::new(Side::Bottom, true)),
            vec![Terminal]
        );
        assert!(
            !l.shown_in_half(Placement::new(Side::Bottom, false))
                .contains(&Terminal)
        );
        // Run in Bottom Left and Terminal ⌥-clicked into Bottom Right are shown side by side.
        assert!(l.displaced_by(Terminal, &[Run]).is_empty());
        assert!(l.displaced_by(Run, &[Terminal]).is_empty());
        // The stripe button and the file stay as they were.
        assert!(l.stripe(Side::Left).foot.contains(&Terminal));
        assert!(l.stripe(Side::Right).foot.is_empty());
        assert_eq!(l.to_json(), saved);
        // Another ⌥-click into the same half replaces it there.
        l.show_in_other_half(Problems);
        assert_eq!(l.displaced_by(Problems, &[Terminal, Run]), vec![Terminal]);
        // Hiding (or moving) ends the exception.
        l.clear_shown_elsewhere(Terminal);
        assert_eq!(l.shown_placement(Terminal), l.placement(Terminal));
        assert!(l.place(Problems, Placement::new(Side::Right, false)));
        assert!(!l.is_shown_elsewhere(Problems));
        assert_eq!(
            Layout::parse(&l.to_json()).shown_placement(Terminal),
            l.placement(Terminal)
        );
    }

    /// SPEC-0061 v10 AK-34: a window off the sidebar has a button only while shown; "Add to
    /// Sidebar" (restore) keeps it.
    #[test]
    fn off_sidebar_windows_have_a_button_while_shown() {
        let mut l = Layout::default();
        assert!(!l.stripe(Side::Left).foot.contains(&Endpoints));
        assert!(
            l.stripe_with(Side::Left, &[Endpoints])
                .foot
                .contains(&Endpoints)
        );
        assert!(
            l.stripe_with(Side::Right, &[Plugins])
                .top
                .contains(&Plugins)
        );
        assert!(l.removed(Endpoints));
        assert!(l.restore(Endpoints));
        assert!(l.stripe(Side::Left).foot.contains(&Endpoints));
        let back = Layout::parse(&l.to_json());
        assert!(!back.removed(Endpoints) && back.removed(Plugins));
        assert!(l.remove(Terminal));
        assert!(
            l.stripe_with(Side::Left, &[Terminal])
                .foot
                .contains(&Terminal)
        );
    }

    /// SPEC-0061 v10 AK-35: an untouched v7 layout gets the new defaults and keeps sizes and
    /// modes; a moved, reordered or removed one stays; a version 2 file is not migrated.
    #[test]
    fn untouched_layouts_are_migrated() {
        // The v7 default as v7 wrote it, with a size and a mode.
        let v7: Vec<String> = ToolWindow::ALL
            .iter()
            .map(|w| {
                let p = w.v7_default_placement();
                let mut s = format!(r#"{{"id":"{}","side":"{}""#, w.id(), p.side.name());
                if *w == Structure {
                    s.push_str(r#","split":true"#);
                }
                if *w == Terminal {
                    s.push_str(r#","size":250,"mode":"unpinned""#);
                }
                s + "}"
            })
            .collect();
        let text = format!(r#"{{"windows":[{}],"stripes":false}}"#, v7.join(","));
        let l = Layout::parse(&text);
        assert!(l.migrated);
        assert_eq!(l.placement(AiChat), Placement::new(Side::Right, false));
        assert_eq!(l.on(Side::Right), Layout::default().on(Side::Right));
        assert!(l.removed(Plugins) && l.removed(Endpoints));
        assert_eq!(l.size(Terminal), Some(250.0));
        assert_eq!(l.mode(Terminal), ViewMode::DockUnpinned);
        assert!(!l.stripes);
        // Written back with the version, it is read as it is.
        let again = Layout::parse(&l.to_json());
        assert!(!again.migrated);
        assert!(l.to_json().contains("\"version\": 2"));
        assert_eq!(again.on(Side::Right), l.on(Side::Right));
        // Older files that list fewer windows, an empty file.
        let short =
            r#"{"windows":[{"id":"project","side":"left"},{"id":"terminal","side":"bottom"}]}"#;
        assert!(Layout::parse(short).migrated);
        assert!(Layout::parse("{}").migrated);
        // Arranged by the user: a moved window, a removed one, a changed order.
        let moved = r#"{"windows":[{"id":"terminal","side":"right"}]}"#;
        let removed = r#"{"windows":[{"id":"git","side":"bottom","removed":true}]}"#;
        let reordered =
            r#"{"windows":[{"id":"terminal","side":"bottom"},{"id":"git","side":"bottom"}]}"#;
        for t in [moved, removed, reordered] {
            let l = Layout::parse(t);
            assert!(!l.migrated, "{t}");
        }
        let l = Layout::parse(moved);
        assert_eq!(l.side(Terminal), Side::Right);
        // A version 2 file at the v7 placements is the user's choice.
        let v2 = text.replacen('{', r#"{"version":2,"#, 1);
        let l = Layout::parse(&v2);
        assert!(!l.migrated);
        assert_eq!(l.side(AiChat), Side::Bottom);
        assert!(!l.removed(Plugins));
    }

    #[test]
    fn hide_all_then_restore() {
        let mut l = Layout::default();
        assert_eq!(
            l.hide_all(&[Project, Terminal]),
            HideAll::Hide(vec![Project, Terminal])
        );
        assert_eq!(l.hide_all(&[]), HideAll::Restore(vec![Project, Terminal]));
        assert_eq!(l.hide_all(&[]), HideAll::Restore(vec![]));
        l.hide_all(&[Git]);
        l.forget_hidden();
        assert_eq!(l.hide_all(&[]), HideAll::Restore(vec![]));
    }

    /// SPEC-0167 AK-8 (v5).
    #[test]
    fn restore_default_puts_everything_back() {
        let mut l = Layout::default();
        assert!(!l.restore_default());
        l.move_to(Terminal, Side::Left);
        l.stripes = false;
        l.set_size(Project, 412.0);
        l.set_mode(Git, ViewMode::Float);
        l.remove(Commit);
        l.hide_all(&[Project]);
        assert!(l.restore_default());
        assert_eq!(l, Layout::default());
        assert!(l.stripes && !l.removed(Commit));
        assert_eq!(l.side(Terminal), Terminal.default_side());
        assert_eq!(l.size(Project), None);
        assert_eq!(l.mode(Git), ViewMode::default());
        // What was read from a file comes back the same after saving.
        assert_eq!(Layout::parse(&l.to_json()), Layout::default());
    }

    #[test]
    fn saved_and_read_with_unknown_and_missing_entries() {
        let mut l = Layout::default();
        l.move_to(Terminal, Side::Left);
        l.stripes = false;
        let back = Layout::parse(&l.to_json());
        assert_eq!(back, l);
        // SPEC-0061 v6 AK-20: a dragged size is kept; sizes never go below the minimum.
        l.set_size(Terminal, 312.4);
        l.set_size(Project, 10.0);
        let back = Layout::parse(&l.to_json());
        assert_eq!(back.size(Terminal), Some(312.0));
        assert_eq!(back.size(Project), Some(MIN_TOOL_WINDOW_SIZE));
        let mut cleared = back.clone();
        cleared.clear_size(Terminal);
        assert_eq!(cleared.size(Terminal), None);
        l.clear_size(Terminal);
        l.clear_size(Project);

        let text = r#"{"windows":[{"id":"terminal","side":"right"},{"id":"nope","side":"left"},
            {"id":"project","side":"top"}],"stripes":true}"#;
        let l = Layout::parse(text);
        assert_eq!(l.side(Terminal), Side::Right);
        assert_eq!(l.side(Project), Side::Left);
        assert_eq!(
            l.on(Side::Right),
            vec![Terminal, Notifications, AiChat, Database, Build, Plugins]
        );
        assert_eq!(Layout::parse("not json"), Layout::default());

        let dir = std::env::temp_dir().join(format!("ferris-idea-tw-{}", std::process::id()));
        let mut m = Layout::default();
        m.move_to(Git, Side::Right);
        m.save(&dir).unwrap();
        assert_eq!(Layout::load(&dir).side(Git), Side::Right);
        assert_eq!(Layout::load(&dir.join("missing")), Layout::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bottom_windows_fit_above_the_status_bar() {
        // Window sizes from small to large, with the windows of the reported case (Commit left,
        // Build right, Problems, Run and Debug at the bottom) and more.
        let sizes = [
            (800.0, 500.0),
            (1000.0, 640.0),
            (1280.0, 800.0),
            (1500.0, 1040.0),
            (2560.0, 1440.0),
            (640.0, 200.0),
        ];
        for (width, height) in sizes {
            for bottom in 0..=6 {
                for stripe in [Some(40.0), None] {
                    let f = frame(&FrameInput {
                        width,
                        height,
                        toolbar: 40.0,
                        status: 28.0,
                        gap: 6.0,
                        edge: 3.5,
                        top: 3.0,
                        stripe,
                        left: vec![300.0],
                        right: vec![400.0],
                        bottom,
                        left_split: bottom % 2 == 0,
                        right_split: false,
                        bottom_right: bottom / 2,
                        undocked: vec![(Side::Bottom, 2000.0)],
                    });
                    assert_eq!(f.status.w, width);
                    assert_eq!(f.status.bottom(), height);
                    for r in f.islands() {
                        assert!(
                            !r.intersects(&f.status) && r.bottom() <= f.status.y + 0.01,
                            "{width}x{height}, {bottom} bottom: {r:?} reaches into {:?}",
                            f.status
                        );
                        assert!(r.h >= 0.0 && r.y >= 40.0);
                    }
                    assert_eq!(f.bottom.len(), bottom);
                    for pair in f.bottom.windows(2) {
                        assert!(!pair[0].intersects(&pair[1]));
                    }
                    if let Some(first) = f.bottom.first() {
                        assert!(!first.intersects(&f.editor));
                        for side in &f.sides {
                            assert!(!first.intersects(side), "{first:?} {side:?}");
                        }
                    }
                }
            }
        }
    }

    /// SPEC-0061 v6 AK-15: the frame of the reference's 1400 × 954 window (`OBS-0020`): islands
    /// 41 from the left edge and 40 from the right, 43 from the top, the status bar 32 high, 5
    /// between islands, Project 457 wide, the bottom window 310 high across the full width.
    #[test]
    fn frame_matches_the_reference_window() {
        let gap = 5.0;
        let f = frame(&FrameInput {
            width: 1400.0,
            height: 954.0,
            toolbar: 40.0,
            status: 32.0,
            gap,
            edge: 3.5,
            top: 3.0,
            stripe: Some(37.0),
            left: vec![ToolWindow::Project.width_in(1400.0, gap)],
            right: vec![],
            bottom: 1,
            left_split: false,
            right_split: false,
            bottom_right: 0,
            undocked: vec![],
        });
        assert_eq!(f.sides[0].x, 40.5);
        assert_eq!(f.sides[0].y, 43.0);
        assert!((f.sides[0].w - 457.0).abs() < 0.01);
        assert!((f.editor.x - (f.sides[0].x + f.sides[0].w + gap)).abs() < 0.01);
        assert_eq!(f.editor.x + f.editor.w, 1359.5);
        assert_eq!(f.status.y, 922.0);
        let b = f.bottom[0];
        assert_eq!((b.x, b.x + b.w), (40.5, 1359.5), "across the full width");
        assert!((b.h - 309.82).abs() < 0.01);
        assert_eq!(b.y + b.h, 922.0);
        assert!(
            (f.sides[0].h - (b.y - gap - 43.0)).abs() < 0.01,
            "Project ends above it"
        );
        assert_eq!(f.stripes[1].x, 1363.0);
    }

    #[test]
    fn bottom_heights_prefer_280_then_share() {
        assert_eq!(bottom_heights(900.0, 6.0, 0), (900.0, vec![]));
        let (editor, h) = bottom_heights(900.0, 6.0, 2);
        assert_eq!(h, vec![280.0, 280.0]);
        assert_eq!(editor, 900.0 - 12.0 - 560.0);
        // The reported case: three bottom windows in a 948 px column keep the editor visible.
        let (editor, h) = bottom_heights(948.0, 6.0, 3);
        assert_eq!(editor, EDITOR_MIN_HEIGHT);
        assert!(h.iter().all(|&x| x < 280.0 && x > BOTTOM_MIN_HEIGHT));
        assert!((editor + h.iter().sum::<f32>() + 18.0 - 948.0).abs() < 0.01);
        // Too small for the minimums: everything still fits.
        let (editor, h) = bottom_heights(150.0, 6.0, 3);
        assert_eq!(editor, 0.0);
        assert!((h.iter().sum::<f32>() + 18.0 - 150.0).abs() < 0.01);
        assert_eq!(bottom_heights(10.0, 6.0, 3).1, vec![0.0; 3]);
    }
}

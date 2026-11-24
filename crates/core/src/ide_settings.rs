//! IDE settings of the user (`SPEC-0129`): editor font, what saving does, the terminal's shell. Kept
//! as TOML in `settings.toml` of the user's settings folder; missing or broken values fall back to
//! the defaults, values out of range are clamped.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::user_config_dir;

/// Smallest and largest editor font size in pixels.
pub const FONT_SIZES: (f32, f32) = (8.0, 40.0);
/// Smallest and largest line spacing.
pub const LINE_SPACINGS: (f32, f32) = (1.0, 2.0);

/// The editor font size one step of `delta` (1 or −1) from `size`, as the font size actions set
/// it (`SPEC-0129` v4 behaviour 8): a whole number inside [`FONT_SIZES`]; `None` when `size`
/// already stands at that limit, so the action changes nothing.
pub fn font_size_step(size: f32, delta: f32) -> Option<f32> {
    let from = if size.is_finite() { size.round() } else { 13.0 };
    let from = from.clamp(FONT_SIZES.0, FONT_SIZES.1);
    let to = (from + delta).clamp(FONT_SIZES.0, FONT_SIZES.1);
    (to != from).then_some(to)
}
/// The bundled editor font, the reference's default.
pub const DEFAULT_FONT: &str = "JetBrains Mono";
/// Smallest and largest right margin (hard wrap guide) in columns.
pub const RIGHT_MARGINS: (u32, u32) = (40, 400);
/// Smallest and largest indent and tab size.
pub const INDENTS: (u32, u32) = (1, 16);
/// The languages Code Style lists, after "Default" (`SPEC-0150`).
pub const CODE_STYLE_LANGUAGES: [&str; 18] = [
    "Default",
    "C/C++",
    "CSS",
    "Go",
    "HTML",
    "Java",
    "JavaScript",
    "JSON",
    "Kotlin",
    "Less",
    "Markdown",
    "Python",
    "Ruby",
    "Rust",
    "SCSS",
    "SQL",
    "TypeScript",
    "YAML",
];

/// Editor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Font family (`SPEC-0150`); the bundled JetBrains Mono by default.
    pub font_family: String,
    /// Font size in pixels (the reference's default 13).
    pub font_size: f32,
    /// Line height as a multiple of the font size (default 1.2).
    pub line_spacing: f32,
    /// Font ligatures (e.g. `->` drawn as one arrow); off by default, as in the reference
    /// (`SPEC-0027` v6).
    pub ligatures: bool,
    /// Saving removes trailing spaces and tabs, except on the caret line.
    pub strip_trailing_spaces: bool,
    /// Saving adds a line break at the end of a file that does not end with one.
    pub ensure_line_break_at_end: bool,
    /// Downloaded dictionary of the typo check, e.g. `en_GB` (`SPEC-0159`); empty: the first
    /// downloaded one.
    #[serde(default)]
    pub spelling_dictionary: String,
    /// Colours from the language server's semantic tokens (`SPEC-0290` behaviour 6).
    pub semantic_highlighting: bool,
    /// Parameter name hints before call arguments (`SPEC-0291` behaviour 6).
    pub parameter_hints: bool,
    /// Fold one-line Java methods when a file opens (`SPEC-0083` v2 behaviour 8).
    pub fold_one_line_methods: bool,
    /// Fold the import block of a Java or Kotlin file when it opens (`SPEC-0083` v4
    /// behaviour 10).
    pub fold_imports: bool,
    /// Resolved configuration values in place of `${…}` placeholders (`SPEC-0115` v5
    /// behaviour 13).
    pub config_values: bool,
    /// When the folding arrows in the gutter are shown (`SPEC-0083` v8 behaviour 22).
    #[serde(default)]
    pub folding_arrows: FoldingArrows,
    /// Usage counts of declarations (`SPEC-0292` behaviour 5, v8).
    pub usage_counts: bool,
    /// The usage counts on rows above the declarations instead of at the end of their lines
    /// (`SPEC-0292` v2 behaviours 4a and 5).
    pub usage_counts_above: bool,
    /// The main author of each declaration at the end of its line (`SPEC-0292` v4 behaviour 7f).
    pub code_authors: bool,
    /// The usages of the symbol at the caret are highlighted by themselves (`SPEC-0295`
    /// behaviour 12).
    pub highlight_usages_at_caret: bool,
    /// Editor Tabs (`SPEC-0059` v9 behaviour 17): `[editor.tabs]`.
    pub tabs: EditorTabSettings,
    /// Where grammar and spelling are checked (`SPEC-0155` v8 behaviour 14): `[editor.grammar]`.
    pub grammar: GrammarSettings,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: DEFAULT_FONT.into(),
            font_size: 13.0,
            line_spacing: 1.2,
            ligatures: false,
            strip_trailing_spaces: false,
            ensure_line_break_at_end: false,
            spelling_dictionary: String::new(),
            semantic_highlighting: true,
            parameter_hints: true,
            fold_one_line_methods: true,
            fold_imports: true,
            config_values: true,
            folding_arrows: FoldingArrows::default(),
            usage_counts: true,
            usage_counts_above: false,
            code_authors: true,
            highlight_usages_at_caret: true,
            tabs: EditorTabSettings::default(),
            grammar: GrammarSettings::default(),
        }
    }
}

/// Where the grammar and spelling checks look in files of one type (`SPEC-0155` v8 behaviour 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrammarScope {
    /// Files of the type are checked at all.
    pub enabled: bool,
    /// Ordinary comments.
    pub comments: bool,
    /// Documentation comments (`/** … */`, `///`) and Python docstrings.
    pub documentation: bool,
    /// String literals; off by default, as they mostly hold keys, messages for machines and
    /// formats.
    pub string_literals: bool,
}

impl Default for GrammarScope {
    fn default() -> Self {
        Self {
            enabled: true,
            comments: true,
            documentation: true,
            string_literals: false,
        }
    }
}

/// The scope of the grammar and spelling checks (`SPEC-0155` v8 behaviour 14): commit messages,
/// and per file type (the name the status bar shows, `Java`, `Markdown`, `Plain text`) the
/// contexts checked. A file type not listed has the defaults of [`GrammarScope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrammarSettings {
    /// Commit messages (`check_grammar` with the context `commit_message`).
    pub commit_messages: bool,
    /// File types whose scope differs from the default.
    pub file_types: std::collections::BTreeMap<String, GrammarScope>,
}

impl Default for GrammarSettings {
    fn default() -> Self {
        Self {
            commit_messages: true,
            file_types: std::collections::BTreeMap::new(),
        }
    }
}

impl GrammarSettings {
    /// The scope for files of `file_type`.
    pub fn scope(&self, file_type: &str) -> GrammarScope {
        self.file_types.get(file_type).copied().unwrap_or_default()
    }

    /// Sets the scope of `file_type`; a scope equal to the default is not kept.
    pub fn set_scope(&mut self, file_type: &str, scope: GrammarScope) {
        if scope == GrammarScope::default() {
            self.file_types.remove(file_type);
        } else {
            self.file_types.insert(file_type.to_owned(), scope);
        }
    }
}

/// Smallest and largest tab limit (`SPEC-0059` v9 behaviour 16).
pub const TAB_LIMITS: (u32, u32) = (1, 100);

/// When the folding arrows in the editor's gutter are shown (`SPEC-0083` v8 behaviour 22).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoldingArrows {
    /// At every foldable region, all the time.
    Always,
    /// At folded regions all the time, at the others while the pointer is over the editor.
    #[default]
    Hover,
}

/// Where the tab bar is (`SPEC-0059` v9 behaviour 17); Left, Right and Bottom are out of scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabPlacement {
    /// Above the editor.
    #[default]
    Top,
    /// No tab bar.
    None,
}

/// How tabs that do not fit are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabRows {
    /// One row that scrolls (behaviour 15).
    #[default]
    Scroll,
    /// One row, the tabs squeezed.
    Squeeze,
    /// As many rows as needed.
    Wrap,
}

/// Where a tab's close button is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseButtonPlace {
    /// After the name.
    #[default]
    Right,
    /// Before the icon.
    Left,
    /// No close button.
    None,
}

/// Which tab a full group closes over the limit (behaviour 16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabLimitRule {
    /// The least recently activated tab without unsaved changes.
    #[default]
    CloseUnchanged,
    /// The least recently activated tab; a modified one is saved first.
    CloseUnused,
}

/// Which tab becomes active when the active one closes (behaviour 17).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivateOnClose {
    /// The tab on the left.
    Left,
    /// The tab on the right.
    Right,
    /// The most recently activated one (behaviour 5).
    #[default]
    Recent,
}

/// Editor Tabs settings (`SPEC-0059` v9 behaviour 17); every one applies at once to every group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorTabSettings {
    /// Tab placement.
    pub placement: TabPlacement,
    /// One row with scrolling, squeezed, or several rows.
    pub rows: TabRows,
    /// Pinned tabs in a row of their own above the others (behaviour 20).
    pub pinned_row: bool,
    /// The file type icon before the name.
    pub show_icon: bool,
    /// The file extension in the name.
    pub show_extension: bool,
    /// The parent folder after names that repeat (behaviour 3).
    pub show_folder_for_equal_names: bool,
    /// ` *` after the name of a tab with unsaved changes (on in Ferris IDEA, a deliberate deviation).
    pub mark_modified: bool,
    /// The full path as the tab's tooltip.
    pub path_tooltip: bool,
    /// Where the close button is.
    pub close_button: CloseButtonPlace,
    /// The close button on every tab; off: only on the active and the hovered tab.
    pub close_button_on_all: bool,
    /// Tabs sorted by name.
    pub sort_alphabetically: bool,
    /// New tabs at the end instead of right of the active one.
    pub open_at_end: bool,
    /// A single click in the project tree opens a provisional tab the next click replaces.
    pub preview_tab: bool,
    /// Most tabs per group (1–100).
    pub limit: u32,
    /// Which tab closes over the limit.
    pub limit_rule: TabLimitRule,
    /// Which tab becomes active when the active one closes.
    pub activate_on_close: ActivateOnClose,
}

impl Default for EditorTabSettings {
    fn default() -> Self {
        Self {
            placement: TabPlacement::Top,
            rows: TabRows::Scroll,
            pinned_row: false,
            show_icon: true,
            show_extension: true,
            show_folder_for_equal_names: true,
            mark_modified: true,
            path_tooltip: true,
            close_button: CloseButtonPlace::Right,
            close_button_on_all: false,
            sort_alphabetically: false,
            open_at_end: false,
            preview_tab: false,
            limit: 30,
            limit_rule: TabLimitRule::CloseUnchanged,
            activate_on_close: ActivateOnClose::Recent,
        }
    }
}

impl EditorTabSettings {
    /// The settings with the limit inside [`TAB_LIMITS`].
    pub fn clamped(mut self) -> Self {
        self.limit = self.limit.clamp(TAB_LIMITS.0, TAB_LIMITS.1);
        self
    }

    /// The keys under `editor.tabs.*`, sorted.
    pub fn keys() -> Vec<String> {
        match serde_json::to_value(Self::default()) {
            Ok(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Changes the keys of `patch` (`editor.tabs.<key>`: value); an unknown key or a value of the
    /// wrong kind is refused and nothing changes (`SPEC-0059` v9.1 behaviour 22a). The limit is
    /// clamped.
    pub fn patched(
        &self,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, String> {
        let mut now = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return Err("the tab settings cannot be read".into()),
        };
        for (key, value) in patch {
            let key = key.strip_prefix("editor.tabs.").unwrap_or(key);
            if !now.contains_key(key) {
                return Err(format!(
                    "no setting `editor.tabs.{key}`; one of {}",
                    Self::keys().join(", ")
                ));
            }
            now.insert(key.to_owned(), value.clone());
        }
        serde_json::from_value::<Self>(serde_json::Value::Object(now))
            .map(Self::clamped)
            .map_err(|e| format!("invalid value for editor.tabs: {e}"))
    }
}

/// Terminal settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    /// Shell program; `None` means `$SHELL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

/// The bundled editor colour schemes `appearance.editor_scheme` may name (`SPEC-0337`
/// behaviour 4), in the order the Appearance page offers them.
pub const EDITOR_SCHEMES: [&str; 5] = [
    "Ferris IDEA Dark",
    "Ferris IDEA Light",
    "Islands Dark",
    "Dark",
    "Light",
];

/// Appearance settings (`SPEC-0131`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    /// The chosen theme (`SPEC-0303` behaviour 14; older versions kept it in a `theme` file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Follow the system's light or dark appearance.
    pub sync_with_os: bool,
    /// Theme used while the system is dark.
    pub dark_theme: String,
    /// Theme used while the system is light.
    pub light_theme: String,
    /// Compact mode (`SPEC-0165`): the theme's compact metrics, a lower toolbar.
    #[serde(default)]
    pub compact_mode: bool,
    /// The editor colour scheme by name (`SPEC-0337` behaviour 4); empty: the theme's own.
    #[serde(default)]
    pub editor_scheme: String,
    /// The startup animation while start-up work runs (`SPEC-0355` v2 behaviour 23); on by
    /// default, off shows nothing.
    pub startup_animation: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: None,
            sync_with_os: false,
            // The brand themes for new installs (SPEC-0060 v6); saved names are kept.
            dark_theme: "Ferris IDEA Dark".into(),
            light_theme: "Ferris IDEA Light".into(),
            compact_mode: false,
            editor_scheme: String::new(),
            startup_animation: true,
        }
    }
}

/// Settings › Appearance & Behavior › System Settings (`SPEC-0337` behaviour 2): `[system]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemSettings {
    /// ⌘Q and the menu ask "Quit Ferris IDEA?" also with nothing pending; never `quit_ide`,
    /// `ferris-idea quit` or a signal. Off by default (IMP-0002).
    pub confirm_quit: bool,
    /// A start without a folder opens the most recent project; off: the folder chooser.
    pub reopen_last_project: bool,
    /// A project window that stops being the active window saves its modified editors. Off by
    /// default (a deliberate deviation, `SPEC-0337`).
    pub save_on_frame_deactivation: bool,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            confirm_quit: false,
            reopen_last_project: true,
            save_on_frame_deactivation: false,
        }
    }
}

/// Settings › Appearance & Behavior › Notifications (`SPEC-0337` behaviour 3): `[notifications]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    /// Informational notes are kept in the Notifications tool window; failures always are.
    pub log_info: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { log_info: true }
    }
}

/// AI Assistant (`SPEC-0138`): off and without a provider until the user names one. The API key
/// is kept in the system keychain, never here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// Whether the assistant may send anything.
    pub enabled: bool,
    /// Base URL of an OpenAI-compatible API, e.g. `http://localhost:11434/v1`.
    pub endpoint: String,
    /// Model name the endpoint knows.
    pub model: String,
    /// Suggest code at the caret while typing (`SPEC-0157`); off unless switched on, as every
    /// pause sends the code around the caret to the endpoint.
    #[serde(default)]
    pub inline_completion: bool,
    /// Chat subscription used instead of the endpoint (`SPEC-0160`): `claude`, `codex` or
    /// `gemini` runs that provider's command line program; empty uses the endpoint and API key.
    #[serde(default)]
    pub subscription: String,
    /// How Claude with the user's Claude plan is used (`SPEC-0160` v2 behaviour 6): behind the
    /// chat (the default) or only as Claude Code in a terminal tab.
    #[serde(default)]
    pub claude_plan_mode: ClaudePlanMode,
    /// Lines of a terminal tab to attach to the chat (`SPEC-0313` behaviour 2); 0 is the
    /// default of 200.
    #[serde(default)]
    pub terminal_lines: usize,
    /// Size budget of one chat attachment in characters (`SPEC-0313` behaviour 7); 0 is the
    /// default of 24 000.
    #[serde(default)]
    pub attachment_chars: usize,
}

/// How Claude with the user's Claude plan is used (`SPEC-0160` v2 behaviour 6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaudePlanMode {
    /// AI Chat, Explain Code and inline completion run Claude Code (`claude -p`) with the plan.
    #[default]
    Chat,
    /// Claude Code runs only in a terminal tab of the project; the chat uses Claude through the
    /// Anthropic API key, or sends nothing.
    Terminal,
}

impl ClaudePlanMode {
    /// The value kept in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            ClaudePlanMode::Chat => "chat",
            ClaudePlanMode::Terminal => "terminal",
        }
    }
}

/// How Update Project brings in the upstream's commits (`SPEC-0144`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMethod {
    /// Merge the upstream into the current branch.
    #[default]
    Merge,
    /// Replay the branch's own commits onto the upstream.
    Rebase,
}

/// Version control (`SPEC-0144`, `SPEC-0150`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VcsSettings {
    /// The method Update Project uses; the Update dialog remembers the last choice.
    pub update_method: UpdateMethod,
    /// Whether force pushing (with lease) is offered at all.
    pub allow_force_push: bool,
    /// Branches that are never force pushed, comma-separated; `*` matches any text.
    pub protected_branches: String,
}

impl Default for VcsSettings {
    fn default() -> Self {
        Self {
            update_method: UpdateMethod::Merge,
            allow_force_push: true,
            protected_branches: "master, main".into(),
        }
    }
}

impl VcsSettings {
    /// Whether `branch` may be force pushed (`SPEC-0150` behaviour 4): force push is allowed and
    /// no protected pattern matches it.
    pub fn may_force_push(&self, branch: &str) -> bool {
        self.allow_force_push
            && !self
                .protected_branches
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .any(|p| glob_match(p, branch))
    }
}

/// `pattern` with `*` for any text matches all of `text`.
fn glob_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(after) = text.strip_prefix(head) else {
                return false;
            };
            (0..=after.len())
                .filter(|i| after.is_char_boundary(*i))
                .any(|i| glob_match(rest, &after[i..]))
        }
    }
}

/// Indentation of one language (`SPEC-0150` behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndentStyle {
    /// Columns one indent level takes.
    pub indent: u32,
    /// Columns a tab advances to.
    pub tab: u32,
    /// Indent with tab characters instead of spaces.
    pub use_tabs: bool,
}

impl IndentStyle {
    const fn spaces(n: u32) -> Self {
        Self {
            indent: n,
            tab: n,
            use_tabs: false,
        }
    }

    /// The text one indent level inserts.
    pub fn unit(&self) -> String {
        if self.use_tabs {
            "\t".into()
        } else {
            " ".repeat(self.indent as usize)
        }
    }
}

/// Code Style (`SPEC-0150`): the right margin and indentation per language. An `.editorconfig`
/// that sets a property wins over these for its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeStyleSettings {
    /// Column of the right margin (the reference's "Hard wrap at", default 120).
    pub right_margin: u32,
    /// Whether the editor draws the right margin.
    pub show_right_margin: bool,
    /// Indentation the user changed, by language name of [`CODE_STYLE_LANGUAGES`].
    pub indents: std::collections::BTreeMap<String, IndentStyle>,
}

impl Default for CodeStyleSettings {
    fn default() -> Self {
        Self {
            right_margin: 120,
            show_right_margin: true,
            indents: Default::default(),
        }
    }
}

impl CodeStyleSettings {
    /// The built-in indentation of `language`, as the reference sets it up: tabs for Go, two
    /// spaces for JSON, YAML, Ruby, SCSS and Less (OBS-0036 FM1), four spaces otherwise — CSS
    /// among them. This is the one table every default indentation comes from: the editor's and
    /// what formatters are asked with (`SPEC-0043` v5 behaviour 1).
    pub fn default_indent(language: &str) -> IndentStyle {
        match language {
            "Go" => IndentStyle {
                indent: 4,
                tab: 4,
                use_tabs: true,
            },
            "JSON" | "YAML" | "Ruby" | "SCSS" | "Less" => IndentStyle::spaces(2),
            _ => IndentStyle::spaces(4),
        }
    }

    /// The indentation of `language`: what the user set, else what "Default" was set to for a
    /// language without its own built-in style, else the built-in one.
    pub fn indent_for(&self, language: &str) -> IndentStyle {
        if let Some(s) = self.indents.get(language) {
            return *s;
        }
        let own = Self::default_indent(language);
        match self.indents.get("Default") {
            Some(d) if own == Self::default_indent("Default") => *d,
            _ => own,
        }
    }
}

/// Git over SSH (`SPEC-0055` v3, ADR-0025 section 1a).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitSshSettings {
    /// `git.ssh.allowClassicKexForAzureDevOps`: Git over SSH to Azure DevOps
    /// (`ssh.dev.azure.com`, `vs-ssh.visualstudio.com`) may use the strongest classical key
    /// exchanges Azure offers, after the post-quantum ones, with a visible warning. On by the
    /// Project Owner's decision (PO-STATUS question 30); off restores the full refusal.
    #[serde(rename = "allowClassicKexForAzureDevOps")]
    pub allow_classic_kex_for_azure_devops: bool,
}

impl Default for GitSshSettings {
    fn default() -> Self {
        Self {
            allow_classic_kex_for_azure_devops: true,
        }
    }
}

/// SSH on every path (ADR-0025 v5 section 1a).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshSettings {
    /// `ssh.allowOwnProxyCommand`: a `ProxyCommand` of the user's own in `~/.ssh/config` may run
    /// for remote hosts, terminals, tunnels, Docker and Git. Ferris IDEA cannot hold such a program to
    /// the post-quantum key exchange, so it is off by default (security first) and the setting
    /// shows a warning; `ProxyJump` needs no exception, every hop gets the shared options.
    #[serde(rename = "allowOwnProxyCommand")]
    pub allow_own_proxy_command: bool,
}

/// Git (`SPEC-0055`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitSettings {
    /// Git over SSH.
    pub ssh: GitSshSettings,
}

/// MCP for agents (`SPEC-0221`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpSettings {
    /// Whether the window serves MCP over HTTP on loopback with a token (`SPEC-0221` v3 behaviour
    /// 9): off unless the user switches it on in Settings › AI Assistant, because a listening port
    /// is attack surface that only the user should open (security first).
    pub http: bool,
}

/// UI automation (`SPEC-0319` behaviour 32): `[ui.driver]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// The UI driver.
    pub driver: UiDriverSettings,
}

/// The UI driver (`SPEC-0319` behaviour 32).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiDriverSettings {
    /// `ui.driver.external`: agents and scripts may operate the window (with the right `ui`).
    /// Off by default; only the user switches it on in Settings, never an MCP tool or an
    /// imported file, and the emergency stop switches it off.
    pub external: bool,
}

/// The Project tool window's tree (`SPEC-0275` v10 behaviour 30): `[project_view]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectViewSettings {
    /// "Compact Middle Packages": a package in a source folder that holds nothing but one
    /// sub-package shares its row with it (`lab.shop.core`). On by default, as in the reference.
    pub compact_middle_packages: bool,
}

impl Default for ProjectViewSettings {
    fn default() -> Self {
        Self {
            compact_middle_packages: true,
        }
    }
}

/// Database tools (`SPEC-0301`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseSettings {
    /// Where "Report a problem with this database" opens a prefilled issue: an `https://` template
    /// with `{title}` and `{body}`; empty for Ferris IDEA's own issue template, `off` for copying only.
    pub report_url: String,
}

/// Intentions of the context actions (`SPEC-0076` v11 behaviour 15): `[intentions]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntentionSettings {
    /// Titles of the language server's intentions the user turned off; ⌥↩ leaves them out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
}

impl IntentionSettings {
    /// Whether the intention titled `title` is turned off.
    pub fn is_disabled(&self, title: &str) -> bool {
        self.disabled.iter().any(|t| t == title)
    }

    /// Turns the intention titled `title` off (`false`) or on again (`true`); whether that
    /// changed anything. The list stays sorted and holds each title once.
    pub fn set_enabled(&mut self, title: &str, enabled: bool) -> bool {
        let had = self.is_disabled(title);
        if enabled {
            self.disabled.retain(|t| t != title);
        } else if !had {
            self.disabled.push(title.to_owned());
            self.disabled.sort();
        }
        had == enabled
    }
}

/// All IDE settings of the user.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdeSettings {
    /// Editor.
    pub editor: EditorSettings,
    /// Terminal.
    pub terminal: TerminalSettings,
    /// Appearance.
    pub appearance: AppearanceSettings,
    /// AI Assistant.
    pub ai: AiSettings,
    /// Version control.
    pub vcs: VcsSettings,
    /// Git (`git.ssh.…`, SPEC-0055 v3).
    pub git: GitSettings,
    /// SSH (`ssh.…`, ADR-0025 v5).
    pub ssh: SshSettings,
    /// Code Style (`SPEC-0150`).
    pub code_style: CodeStyleSettings,
    /// MCP (`SPEC-0221`).
    pub mcp: McpSettings,
    /// Database tools (`SPEC-0301`).
    pub database: DatabaseSettings,
    /// UI automation (`SPEC-0319`).
    pub ui: UiSettings,
    /// The project tree (`SPEC-0275` v10).
    pub project_view: ProjectViewSettings,
    /// System Settings (`SPEC-0337`).
    pub system: SystemSettings,
    /// Notifications (`SPEC-0337`).
    pub notifications: NotificationSettings,
    /// Intentions of the context actions (`SPEC-0076` v11).
    pub intentions: IntentionSettings,
}

impl IdeSettings {
    /// Theme and editor scheme names saved under the product's former name, as today's names
    /// (`SPEC-0356` behaviour 7); saving writes today's names.
    pub fn with_current_theme_names(mut self) -> Self {
        let current = |s: &str| crate::takeover::current_theme_name(s).into_owned();
        let a = &mut self.appearance;
        a.theme = a.theme.as_deref().map(current);
        a.dark_theme = current(&a.dark_theme);
        a.light_theme = current(&a.light_theme);
        a.editor_scheme = current(&a.editor_scheme);
        self
    }

    /// The settings with every value inside its range.
    pub fn clamped(mut self) -> Self {
        let e = &mut self.editor;
        let fix = |v: f32, (lo, hi): (f32, f32), default: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                default
            }
        };
        e.font_size = fix(e.font_size, FONT_SIZES, 13.0).round();
        e.line_spacing = (fix(e.line_spacing, LINE_SPACINGS, 1.2) * 10.0).round() / 10.0;
        if e.font_family.trim().is_empty() {
            e.font_family = DEFAULT_FONT.into();
        }
        e.tabs = e.tabs.clone().clamped();
        let cs = &mut self.code_style;
        cs.right_margin = cs.right_margin.clamp(RIGHT_MARGINS.0, RIGHT_MARGINS.1);
        cs.indents
            .retain(|lang, _| CODE_STYLE_LANGUAGES.contains(&lang.as_str()));
        for style in cs.indents.values_mut() {
            style.indent = style.indent.clamp(INDENTS.0, INDENTS.1);
            style.tab = style.tab.clamp(INDENTS.0, INDENTS.1);
        }
        if self
            .terminal
            .shell
            .as_deref()
            .is_some_and(|s| s.trim().is_empty())
        {
            self.terminal.shell = None;
        }
        self
    }

    /// Line height in whole pixels.
    pub fn line_height(&self) -> f32 {
        (self.editor.font_size * self.editor.line_spacing).round()
    }
}

/// Where the settings are kept: `settings.toml` in [`user_config_dir`].
pub fn ide_settings_file() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join("settings.toml"))
}

/// The settings in `file`; the defaults when it is missing or cannot be read.
pub fn load_ide_settings(file: &Path) -> IdeSettings {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
        .and_then(|mut t| {
            // With or without the envelope (`SPEC-0303` behaviour 16); another kind or a newer
            // format gives the defaults, and saving refuses to replace it.
            crate::format::take_envelope(&mut t, "ide-settings").ok()?;
            Some(read_values(t))
        })
        .unwrap_or_default()
        .clamped()
        .with_current_theme_names()
}

/// The settings in `table`. A value of the wrong kind (a text where a number belongs, an unknown
/// choice) is left out alone and its setting keeps the default; every other value of the file
/// counts (`SPEC-0129` v3 behaviour 1).
fn read_values(table: toml::Table) -> IdeSettings {
    let read = |t: &toml::Table| toml::Value::Table(t.clone()).try_into::<IdeSettings>();
    if let Ok(settings) = read(&table) {
        return settings;
    }
    fn leaves(
        prefix: &mut Vec<String>,
        t: &toml::Table,
        out: &mut Vec<(Vec<String>, toml::Value)>,
    ) {
        for (key, value) in t {
            prefix.push(key.clone());
            match value {
                toml::Value::Table(inner) if !inner.is_empty() => leaves(prefix, inner, out),
                other => out.push((prefix.clone(), other.clone())),
            }
            prefix.pop();
        }
    }
    fn put(t: &mut toml::Table, path: &[String], value: toml::Value) {
        match path {
            [] => {}
            [key] => {
                t.insert(key.clone(), value);
            }
            [key, rest @ ..] => {
                let inner = t
                    .entry(key.clone())
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                if let toml::Value::Table(inner) = inner {
                    put(inner, rest, value);
                }
            }
        }
    }
    let mut all = Vec::new();
    leaves(&mut Vec::new(), &table, &mut all);
    let mut kept = toml::Table::new();
    let mut left: Vec<(Vec<String>, toml::Value)> = Vec::new();
    for (path, value) in all {
        let mut trial = kept.clone();
        put(&mut trial, &path, value.clone());
        if read(&trial).is_ok() {
            kept = trial;
        } else {
            left.push((path, value));
        }
    }
    // Values that only count together, such as the three of one language's indentation: the
    // values of one table that were left, at once.
    while let Some((first, _)) = left.first() {
        let parent = first[..first.len() - 1].to_vec();
        let (group, rest): (Vec<_>, Vec<_>) = left
            .into_iter()
            .partition(|(path, _)| path[..path.len() - 1] == parent[..]);
        left = rest;
        let mut trial = kept.clone();
        for (path, value) in group {
            put(&mut trial, &path, value);
        }
        if read(&trial).is_ok() {
            kept = trial;
        }
    }
    read(&kept).unwrap_or_default()
}

/// Saves `settings` to `file` with the envelope, creating its folder; a file without envelope is
/// kept once as `.bak`, one of a newer format is not replaced (`SPEC-0303` behaviours 3, 15).
pub fn save_ide_settings(file: &Path, settings: &IdeSettings) -> std::io::Result<()> {
    crate::format::prepare_write(file, "ide-settings")?;
    let text = crate::format::write(&crate::format::Envelope::current("ide-settings"), settings)
        .map_err(std::io::Error::other)?;
    crate::write_atomic(file, text)
}

/// The edits saving makes to `text` (behaviour 4), as byte ranges and replacements in order:
/// trailing spaces and tabs removed from every line but `keep_line` (0-based, the caret's), and a
/// final line break added, as `settings` ask.
pub fn save_edits(
    text: &str,
    settings: &EditorSettings,
    keep_line: Option<usize>,
) -> Vec<(std::ops::Range<usize>, String)> {
    let mut edits = Vec::new();
    if settings.strip_trailing_spaces {
        let mut at = 0;
        for (i, line) in text.split_inclusive('\n').enumerate() {
            let body = line.strip_suffix('\n').unwrap_or(line);
            let body = body.strip_suffix('\r').unwrap_or(body);
            let kept = body.trim_end_matches([' ', '\t']).len();
            if Some(i) != keep_line && kept < body.len() {
                edits.push((at + kept..at + body.len(), String::new()));
            }
            at += line.len();
        }
    }
    if settings.ensure_line_break_at_end && !text.is_empty() && !text.ends_with('\n') {
        let end = text.len();
        // A last line that is only spaces is removed above; the break goes after what stays.
        edits.push((end..end, "\n".into()));
    }
    edits
}

/// `text` as saving leaves it: [`save_edits`] applied.
pub fn on_save(text: &str, settings: &EditorSettings, keep_line: Option<usize>) -> String {
    let mut out = String::with_capacity(text.len() + 1);
    let mut at = 0;
    for (range, with) in save_edits(text, settings, keep_line) {
        out.push_str(&text[at..range.start]);
        out.push_str(&with);
        at = range.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0076 v11 AK-12: disabled intentions are kept sorted, once each, and round-trip
    /// through the settings file; an empty list is not written.
    #[test]
    fn disabled_intentions_round_trip() {
        let mut s = IdeSettings::default();
        assert!(s.intentions.set_enabled("Generate toString()", false));
        assert!(s.intentions.set_enabled("Add Javadoc", false));
        assert!(
            !s.intentions.set_enabled("Add Javadoc", false),
            "already off"
        );
        assert_eq!(
            s.intentions.disabled,
            ["Add Javadoc", "Generate toString()"]
        );
        assert!(s.intentions.is_disabled("Add Javadoc"));
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-intentions-{}", std::process::id()));
        let file = dir.join("settings.toml");
        save_ide_settings(&file, &s).unwrap();
        assert_eq!(load_ide_settings(&file).intentions, s.intentions);
        assert!(s.intentions.set_enabled("Add Javadoc", true));
        assert!(!s.intentions.set_enabled("Add Javadoc", true), "already on");
        s.intentions.set_enabled("Generate toString()", true);
        save_ide_settings(&file, &s).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("disabled ="), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0059 v9 behaviours 16–17, AK-11: the tab settings have the reference's defaults (the
    /// modified mark stays on), round-trip through `editor.tabs.*` and refuse unknown keys.
    #[test]
    fn editor_tab_settings_round_trip_through_editor_tabs_keys() {
        let d = EditorTabSettings::default();
        assert_eq!(d.limit, 30);
        assert!(d.mark_modified && !d.close_button_on_all && !d.preview_tab);
        assert_eq!(d.limit_rule, TabLimitRule::CloseUnchanged);
        assert_eq!(d.activate_on_close, ActivateOnClose::Recent);
        let patch: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{"editor.tabs.limit": 500, "limit_rule": "close_unused", "close_button": "left",
                "editor.tabs.mark_modified": false}"#,
        )
        .unwrap();
        let p = d.patched(&patch).unwrap();
        assert_eq!(p.limit, 100, "clamped");
        assert_eq!(p.limit_rule, TabLimitRule::CloseUnused);
        assert_eq!(p.close_button, CloseButtonPlace::Left);
        assert!(!p.mark_modified);
        let unknown: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(r#"{"editor.tabs.colour": "red"}"#).unwrap();
        assert!(
            d.patched(&unknown)
                .unwrap_err()
                .contains("editor.tabs.colour")
        );
        let wrong: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(r#"{"placement": "left"}"#).unwrap();
        assert!(d.patched(&wrong).is_err());
        // Saved and read back as `[editor.tabs]`.
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-tab-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("settings.toml");
        let mut all = IdeSettings::default();
        all.editor.tabs = p.clone();
        save_ide_settings(&file, &all).unwrap();
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("[editor.tabs]")
        );
        assert_eq!(load_ide_settings(&file).editor.tabs, p);
        assert!(EditorTabSettings::keys().contains(&"activate_on_close".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0337 AK-1: the system and notification settings keep Ferris IDEA's behaviour until the
    /// user changes them, also for a file written before they existed.
    #[test]
    fn system_and_notification_defaults() {
        let d = IdeSettings::default();
        assert!(
            !d.system.confirm_quit,
            "quitting asks only with pending work (IMP-0002)"
        );
        assert!(d.system.reopen_last_project);
        assert!(!d.system.save_on_frame_deactivation);
        assert!(d.notifications.log_info);
        assert!(
            d.appearance.editor_scheme.is_empty(),
            "the theme's own scheme"
        );
        let old: IdeSettings = toml::from_str("[editor]\nfont_size = 14.0\n").unwrap();
        assert_eq!(old.system, d.system);
        assert_eq!(old.notifications, d.notifications);
        let set: IdeSettings = toml::from_str(
            "[system]\nconfirm_quit = true\nreopen_last_project = false\n[notifications]\nlog_info = false\n",
        )
        .unwrap();
        assert!(set.system.confirm_quit && !set.system.reopen_last_project);
        assert!(!set.system.save_on_frame_deactivation);
        assert!(!set.notifications.log_info);
    }

    /// SPEC-0275 v10 AK-26: "Compact Middle Packages" is on unless the file says otherwise.
    #[test]
    fn project_view_defaults_and_loads() {
        assert!(IdeSettings::default().project_view.compact_middle_packages);
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-project-view-{}",
            std::process::id()
        ));
        let file = dir.join("settings.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "[editor]\nfont_size = 14\n").unwrap();
        assert!(
            load_ide_settings(&file)
                .project_view
                .compact_middle_packages
        );
        let mut s = load_ide_settings(&file);
        s.project_view.compact_middle_packages = false;
        save_ide_settings(&file, &s).unwrap();
        assert!(
            !load_ide_settings(&file)
                .project_view
                .compact_middle_packages
        );
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("[project_view]\ncompact_middle_packages = false")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0221 v3 AK-8: MCP over HTTP is off without a settings file and with a file that does
    /// not name it; `http = true` in `[mcp]` switches it on.
    #[test]
    fn mcp_over_http_is_off_by_default() {
        assert!(!IdeSettings::default().mcp.http);
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-mcp-http-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.toml");
        assert!(!load_ide_settings(&file).mcp.http, "no file");
        std::fs::write(&file, "[editor]\nfont_size = 14\n[mcp]\n").unwrap();
        assert!(!load_ide_settings(&file).mcp.http, "file without the key");
        std::fs::write(&file, "[mcp]\nhttp = true\n").unwrap();
        assert!(load_ide_settings(&file).mcp.http, "switched on");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_and_clamp() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-ide-settings-{}", std::process::id()));
        let file = dir.join("settings.toml");
        assert_eq!(load_ide_settings(&file), IdeSettings::default());
        let mut s = IdeSettings::default();
        s.editor.font_size = 16.0;
        s.editor.strip_trailing_spaces = true;
        s.terminal.shell = Some("/bin/zsh".into());
        save_ide_settings(&file, &s).unwrap();
        assert_eq!(load_ide_settings(&file), s);
        std::fs::write(&file, "[editor]\nfont_size = 200\nline_spacing = 0.2\n").unwrap();
        let clamped = load_ide_settings(&file);
        assert_eq!(clamped.editor.font_size, 40.0);
        assert_eq!(clamped.editor.line_spacing, 1.0);
        std::fs::write(&file, "not toml [").unwrap();
        assert_eq!(load_ide_settings(&file), IdeSettings::default());
        assert_eq!(IdeSettings::default().line_height(), 16.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0129 v3 AK-5: one value of the wrong kind costs that setting alone, wherever it is.
    #[test]
    fn a_wrong_value_keeps_the_other_settings() {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-lenient-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.toml");
        std::fs::write(
            &file,
            "[editor]\nfont_size = \"big\"\nline_spacing = 1.4\nstrip_trailing_spaces = true\n\
             [editor.tabs]\nplacement = \"diagonal\"\nlimit = 12\n\
             [terminal]\nshell = \"/bin/bash\"\n\
             [appearance]\ntheme = \"Light\"\nsync_with_os = \"yes\"\n\
             [vcs]\nupdate_method = 7\nprotected_branches = \"release/*\"\n\
             [code_style]\nright_margin = 100\n[code_style.indents.Python]\nindent = 2\ntab = 2\nuse_tabs = false\n\
             [mcp]\nhttp = true\n",
        )
        .unwrap();
        let s = load_ide_settings(&file);
        let d = IdeSettings::default();
        // The wrong values fall back to their defaults …
        assert_eq!(s.editor.font_size, d.editor.font_size);
        assert_eq!(s.editor.tabs.placement, d.editor.tabs.placement);
        assert_eq!(s.appearance.sync_with_os, d.appearance.sync_with_os);
        assert_eq!(s.vcs.update_method, d.vcs.update_method);
        // … and every other value of the file counts.
        assert_eq!(s.editor.line_spacing, 1.4);
        assert!(s.editor.strip_trailing_spaces);
        assert_eq!(s.editor.tabs.limit, 12);
        assert_eq!(s.terminal.shell.as_deref(), Some("/bin/bash"));
        assert_eq!(s.appearance.theme.as_deref(), Some("Light"));
        assert_eq!(s.vcs.protected_branches, "release/*");
        assert_eq!(s.code_style.right_margin, 100);
        assert_eq!(s.code_style.indent_for("Python"), IndentStyle::spaces(2));
        assert!(s.mcp.http);
        // A section that is no table costs that section alone.
        std::fs::write(&file, "editor = 5\n[terminal]\nshell = \"/bin/zsh\"\n").unwrap();
        let s = load_ide_settings(&file);
        assert_eq!(s.editor, d.editor);
        assert_eq!(s.terminal.shell.as_deref(), Some("/bin/zsh"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0129 v3 AK-5: every setting of the IDE layer, changed from its default, is written to
    /// the settings file and read back at the next start. A setting added later fails here until
    /// it is given a changed value.
    #[test]
    fn every_setting_survives_a_restart() {
        // Texts and choices get a named value; switches and numbers are changed by rule.
        let texts: &[(&str, &str)] = &[
            ("editor.font_family", "Menlo"),
            ("editor.spelling_dictionary", "en_GB"),
            ("editor.folding_arrows", "always"),
            ("editor.tabs.placement", "none"),
            ("editor.tabs.rows", "wrap"),
            ("editor.tabs.close_button", "left"),
            ("editor.tabs.limit_rule", "close_unused"),
            ("editor.tabs.activate_on_close", "left"),
            ("terminal.shell", "/bin/bash"),
            ("appearance.theme", "Islands Light"),
            ("appearance.dark_theme", "Dark"),
            ("appearance.light_theme", "Light"),
            ("ai.endpoint", "http://localhost:11434/v1"),
            ("ai.model", "local"),
            ("ai.subscription", "claude"),
            ("ai.claude_plan_mode", "terminal"),
            ("vcs.update_method", "rebase"),
            ("vcs.protected_branches", "release/*"),
            ("database.report_url", "off"),
            ("appearance.editor_scheme", "Islands Dark"),
        ];
        fn change(path: &str, v: &mut toml::Value, texts: &[(&str, &str)], seen: &mut Vec<String>) {
            match v {
                toml::Value::Table(t) => {
                    for (k, inner) in t.iter_mut() {
                        let path = if path.is_empty() {
                            k.clone()
                        } else {
                            format!("{path}.{k}")
                        };
                        change(&path, inner, texts, seen);
                    }
                    return;
                }
                toml::Value::Boolean(b) => *b = !*b,
                toml::Value::Integer(n) => *n += 1,
                toml::Value::Float(f) => *f += if f.fract() == 0.0 { 1.0 } else { 0.5 },
                toml::Value::String(s) => {
                    let (_, text) = texts
                        .iter()
                        .find(|(p, _)| *p == path)
                        .unwrap_or_else(|| panic!("no changed value for the setting `{path}`"));
                    *s = (*text).to_owned();
                }
                other => panic!("setting `{path}` of an unexpected kind: {other:?}"),
            }
            seen.push(path.to_owned());
        }
        // Optional texts and the map are not in the defaults: set first.
        let mut start = IdeSettings::default();
        start.terminal.shell = Some(String::new());
        start.appearance.theme = Some(String::new());
        let mut value = toml::Value::try_from(&start).unwrap();
        let mut seen = Vec::new();
        change("", &mut value, texts, &mut seen);
        for (path, _) in texts {
            assert!(
                seen.iter().any(|s| s == path),
                "`{path}` is no setting any more"
            );
        }
        let mut changed: IdeSettings = value.try_into().unwrap();
        changed.editor.grammar.file_types.insert(
            "Java".into(),
            GrammarScope {
                enabled: true,
                comments: false,
                documentation: true,
                string_literals: true,
            },
        );
        changed.code_style.indents.insert(
            "Python".into(),
            IndentStyle {
                indent: 2,
                tab: 3,
                use_tabs: true,
            },
        );
        assert_eq!(
            changed.clone().clamped(),
            changed,
            "the changed values are in range"
        );
        let d = IdeSettings::default();
        assert_ne!(changed.editor, d.editor);
        assert_ne!(changed.code_style, d.code_style);
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-all-{}",
            std::process::id()
        ));
        let file = dir.join("settings.toml");
        save_ide_settings(&file, &changed).unwrap();
        assert_eq!(
            load_ide_settings(&file),
            changed,
            "read back as at the next start"
        );
        // Saved a second time (a later change in the dialog) nothing is lost either.
        save_ide_settings(&file, &load_ide_settings(&file)).unwrap();
        assert_eq!(load_ide_settings(&file), changed);
        // Every changed value is in the file of the IDE layer under its section.
        let text = std::fs::read_to_string(&file).unwrap();
        for section in [
            "[editor]",
            "[editor.tabs]",
            "[terminal]",
            "[appearance]",
            "[ai]",
            "[vcs]",
            "[git.ssh]",
            "[ssh]",
            "[code_style]",
            "[mcp]",
            "[database]",
            "[ui.driver]",
            "[project_view]",
            "[system]",
            "[notifications]",
        ] {
            assert!(text.contains(section), "{section} missing in:\n{text}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0027 v6: ligatures are off by default (settings files without the key included); a
    /// saved choice stays.
    #[test]
    fn ligatures_are_off_by_default_and_a_saved_choice_stays() {
        assert!(!IdeSettings::default().editor.ligatures);
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-ligatures-{}",
            std::process::id()
        ));
        let file = dir.join("settings.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "[editor]\nfont_size = 14\n").unwrap();
        assert!(!load_ide_settings(&file).editor.ligatures);
        std::fs::write(&file, "[editor]\nligatures = true\n").unwrap();
        assert!(load_ide_settings(&file).editor.ligatures);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0295/AK-6: the usages at the caret are highlighted by default (settings files without
    /// the key included); switched off, the choice is saved and read back.
    #[test]
    fn usages_at_the_caret_are_highlighted_by_default() {
        assert!(IdeSettings::default().editor.highlight_usages_at_caret);
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-occurrences-{}",
            std::process::id()
        ));
        let file = dir.join("settings.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "[editor]\nfont_size = 14\n").unwrap();
        assert!(load_ide_settings(&file).editor.highlight_usages_at_caret);
        let mut s = IdeSettings::default();
        s.editor.highlight_usages_at_caret = false;
        save_ide_settings(&file, &s).unwrap();
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(
            saved.contains("highlight_usages_at_caret = false"),
            "{saved}"
        );
        assert!(!load_ide_settings(&file).editor.highlight_usages_at_caret);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// New installs get the brand themes; a saved choice stays (SPEC-0060 v6).
    #[test]
    fn appearance_defaults_to_the_brand_themes_and_keeps_saved_choices() {
        let a = AppearanceSettings::default();
        assert_eq!(a.dark_theme, "Ferris IDEA Dark");
        assert_eq!(a.light_theme, "Ferris IDEA Light");
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-ide-settings-theme-{}",
            std::process::id()
        ));
        let file = dir.join("settings.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &file,
            "[appearance]\nsync_with_os = true\ndark_theme = \"Islands Dark\"\nlight_theme = \"Light\"\n",
        )
        .unwrap();
        let s = load_ide_settings(&file);
        assert_eq!(s.appearance.dark_theme, "Islands Dark");
        assert_eq!(s.appearance.light_theme, "Light");
        assert!(
            s.appearance.startup_animation,
            "on for a file written before the setting (SPEC-0355 v2)"
        );
        std::fs::write(&file, "[appearance]\nstartup_animation = false\n").unwrap();
        assert!(!load_ide_settings(&file).appearance.startup_animation);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn code_style_and_vcs_defaults() {
        let mut cs = CodeStyleSettings::default();
        assert_eq!(cs.right_margin, 120);
        assert_eq!(cs.indent_for("Java").unit(), "    ");
        assert_eq!(cs.indent_for("Go").unit(), "\t");
        assert_eq!(cs.indent_for("YAML").unit(), "  ");
        // OBS-0036 FM1: SCSS and Less with two spaces, CSS with four.
        for (language, indent) in [("JSON", 2), ("SCSS", 2), ("Less", 2), ("CSS", 4)] {
            assert_eq!(cs.indent_for(language).indent, indent, "{language}");
            assert!(CODE_STYLE_LANGUAGES.contains(&language));
        }
        cs.indents.insert("Default".into(), IndentStyle::spaces(3));
        assert_eq!(
            cs.indent_for("Java").indent,
            3,
            "Default applies where nothing else is set"
        );
        assert_eq!(
            cs.indent_for("YAML").indent,
            2,
            "a built-in style of its own stays"
        );
        cs.indents.insert("Java".into(), IndentStyle::spaces(2));
        assert_eq!(cs.indent_for("Java").indent, 2);
        let mut s = IdeSettings::default();
        s.code_style.right_margin = 5;
        s.code_style
            .indents
            .insert("Nope".into(), IndentStyle::spaces(2));
        s.code_style
            .indents
            .insert("Rust".into(), IndentStyle::spaces(99));
        s.editor.font_family = " ".into();
        let s = s.clamped();
        assert_eq!(s.code_style.right_margin, 40);
        assert!(!s.code_style.indents.contains_key("Nope"));
        assert_eq!(s.code_style.indents["Rust"].indent, 16);
        assert_eq!(s.editor.font_family, DEFAULT_FONT);
        let v = VcsSettings::default();
        assert!(!v.may_force_push("main"));
        assert!(!v.may_force_push("master"));
        assert!(v.may_force_push("feature/x"));
        let release = VcsSettings {
            protected_branches: "release/*, main".into(),
            ..VcsSettings::default()
        };
        assert!(!release.may_force_push("release/1.0"));
        assert!(release.may_force_push("master"));
        let off = VcsSettings {
            allow_force_push: false,
            ..VcsSettings::default()
        };
        assert!(!off.may_force_push("feature/x"));
    }

    #[test]
    fn saving_strips_spaces_and_ends_with_a_line_break() {
        let mut e = EditorSettings::default();
        assert_eq!(on_save("a  \nb", &e, None), "a  \nb", "off by default");
        e.strip_trailing_spaces = true;
        assert_eq!(on_save("a  \nb\t\nc ", &e, Some(1)), "a\nb\t\nc");
        assert_eq!(on_save("a \r\nb", &e, None), "a\r\nb", "CRLF kept");
        e.ensure_line_break_at_end = true;
        assert_eq!(on_save("a \nb ", &e, None), "a\nb\n");
        assert_eq!(on_save("", &e, None), "", "an empty file stays empty");
    }

    /// SPEC-0129 v4 AK-6: one step up or down, whole sizes, nothing past the limits.
    #[test]
    fn font_size_steps_stop_at_the_limits() {
        assert_eq!(font_size_step(13.0, 1.0), Some(14.0));
        assert_eq!(font_size_step(14.0, -1.0), Some(13.0));
        assert_eq!(font_size_step(39.0, 1.0), Some(40.0));
        assert_eq!(font_size_step(40.0, 1.0), None);
        assert_eq!(font_size_step(40.0, -1.0), Some(39.0));
        assert_eq!(font_size_step(9.0, -1.0), Some(8.0));
        assert_eq!(font_size_step(8.0, -1.0), None);
        assert_eq!(font_size_step(8.0, 1.0), Some(9.0));
        // A size between whole numbers, or outside the limits, lands on a whole one inside.
        assert_eq!(font_size_step(13.4, 1.0), Some(14.0));
        assert_eq!(font_size_step(200.0, -1.0), Some(39.0));
        assert_eq!(font_size_step(200.0, 1.0), None);
        assert_eq!(font_size_step(f32::NAN, 1.0), Some(14.0));
        // What the step gives is what the settings keep.
        let mut s = IdeSettings::default();
        s.editor.font_size = font_size_step(s.editor.font_size, 1.0).unwrap();
        assert_eq!(s.clone().clamped().editor.font_size, 14.0);
    }
}

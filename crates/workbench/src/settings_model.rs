//! What the Settings dialog shows (`SPEC-0105`), without drawing: its pages in the reference's
//! outline (`SPEC-0337` behaviour 1) and the keymap and Advanced Settings rows that match the search
//! text.

use ferris_idea_ui::keymap::{Keymap, Shortcut};

/// A page of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Appearance & Behavior › Appearance: the theme and the editor scheme.
    Appearance,
    /// Appearance & Behavior › System Settings: quitting, start-up, saving (`SPEC-0337`).
    SystemSettings,
    /// Appearance & Behavior › Notifications (`SPEC-0337` behaviour 3).
    Notifications,
    /// Keymap: the actions and their shortcuts.
    Keymap,
    /// Editor › General: what saving does (`SPEC-0129`).
    Editor,
    /// Editor › Font: family, size and line spacing.
    Font,
    /// Editor › Editor Tabs: placement, rows, close button, limit … (`SPEC-0059` v9 behaviour 17).
    EditorTabs,
    /// Editor › Code Style: right margin and indentation per language (`SPEC-0150`).
    CodeStyle,
    /// Editor › Natural Languages: dictionaries of the typo check (`SPEC-0159`).
    NaturalLanguages,
    /// Editor › Natural Languages › Grammar and Style: where grammar and spelling are checked, per
    /// file type and context, and the writing rules (`SPEC-0155` v8 behaviour 14).
    Grammar,
    /// Editor › Inspections: Ferris IDEA's inspections in the project's profile (`SPEC-0315`).
    Inspections,
    /// Plugins: installed plugins, install from disk, enable, disable, remove (SPEC-0126).
    Plugins,
    /// Version Control: Update method, force push and protected branches (`SPEC-0150`).
    VersionControl,
    /// Build, Execution, Deployment › Build Tools: JDKs, Maven and Gradle.
    BuildTools,
    /// Appearance & Behavior › Trusted Locations: projects and folders whose code may run
    /// (`SPEC-0278` v3 behaviour 8).
    TrustedLocations,
    /// Languages & Frameworks: the language servers.
    Languages,
    /// Tools › Terminal: the shell.
    Terminal,
    /// Tools › AI Assistant: switch, endpoint, model, API key (`SPEC-0138`).
    Ai,
    /// Advanced Settings: the settings of `settings.toml` without a page (`SPEC-0337` behaviour 5).
    Advanced,
}

impl Page {
    /// Every page, in the order of the page list.
    pub const ALL: [Page; 19] = [
        Page::Appearance,
        Page::SystemSettings,
        Page::Notifications,
        Page::TrustedLocations,
        Page::Keymap,
        Page::Editor,
        Page::Font,
        Page::EditorTabs,
        Page::CodeStyle,
        Page::Inspections,
        Page::NaturalLanguages,
        Page::Grammar,
        Page::Plugins,
        Page::VersionControl,
        Page::BuildTools,
        Page::Languages,
        Page::Terminal,
        Page::Ai,
        Page::Advanced,
    ];

    /// The page's name in the page list.
    pub fn title(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::SystemSettings => "System Settings",
            Page::Notifications => "Notifications",
            Page::Advanced => "Advanced Settings",
            Page::Keymap => "Keymap",
            Page::Editor => "General",
            Page::Font => "Font",
            Page::EditorTabs => "Editor Tabs",
            Page::CodeStyle => "Code Style",
            Page::NaturalLanguages => "Natural Languages",
            Page::Grammar => "Grammar and Style",
            Page::Inspections => "Inspections",
            Page::Plugins => "Plugins",
            Page::VersionControl => "Version Control",
            Page::BuildTools => "Build Tools",
            Page::TrustedLocations => "Trusted Locations",
            Page::Languages => "Languages & Frameworks",
            Page::Terminal => "Terminal",
            Page::Ai => "AI Assistant",
        }
    }

    /// Words the search finds the page by besides its title.
    fn keywords(self) -> &'static str {
        match self {
            Page::Appearance => {
                "theme look and feel dark light islands ferris-idea brand colour scheme color scheme startup splash animation motion"
            }
            Page::SystemSettings => {
                "system startup start-up reopen quit exit confirm autosave save focus window project"
            }
            Page::Notifications => "notifications notes balloon messages status bar log",
            Page::Advanced => "advanced low-level options reset default",
            Page::Keymap => "shortcut keyboard key binding",
            Page::Editor => {
                "editor general save trailing spaces whitespace line break newline end of file code \
                 folding fold imports one-line methods highlight usages symbol caret \
                 configuration values placeholders properties"
            }
            Page::Font => {
                "font size family typeface line spacing height zoom editor text ligatures ligature"
            }
            Page::EditorTabs => {
                "editor tabs tab limit close button pinned pin preview sort alphabetically scroll rows squeeze placement modified asterisk"
            }
            Page::CodeStyle => {
                "code style indent indentation tab tabs spaces right margin hard wrap guide formatter"
            }
            Page::NaturalLanguages => {
                "natural languages spelling spell check typo dictionary hunspell grammar english grazie"
            }
            Page::Grammar => {
                "grammar style writing scope file type context comments documentation docstrings \
                 commit messages string literals rules spelling typo repeated word article"
            }
            Page::Inspections => {
                "inspections inspection profile severity suppress warning weak error code analysis lint"
            }
            Page::Plugins => "plugin extension install wasm webassembly",
            Page::VersionControl => {
                "version control vcs git push force protected branches update merge rebase ssh azure devops key exchange post-quantum proxycommand proxyjump jump host bastion"
            }
            Page::BuildTools => "build execution deployment jdk java maven gradle sdk",
            Page::TrustedLocations => {
                "trust trusted safe mode security project folder locations untrusted"
            }
            Page::Languages => "languages frameworks language server lsp install",
            Page::Terminal => "terminal shell tools zsh bash",
            Page::Ai => {
                "ai assistant chat llm model endpoint openai anthropic claude code plan terminal ollama api key inline completion"
            }
        }
    }

    /// The labels the page draws for its settings and that never change (`SPEC-0105` v5
    /// behaviour 3b); `settings_view` draws exactly these texts.
    pub fn fixed_labels(self) -> &'static [&'static str] {
        match self {
            Page::Appearance => &[
                "Theme",
                "Editor color scheme",
                "Sync with OS",
                "Compact mode",
                "Startup animation",
            ],
            Page::SystemSettings => &[
                "Confirm before quitting",
                "Reopen the last project at start-up",
                "Save files when the window loses the focus",
            ],
            Page::Notifications => &["Keep informational notes in the Notifications tool window"],
            Page::Editor => &[
                "On Save",
                "Remove trailing spaces on save (not on the caret line)",
                "Ensure every saved file ends with a line break",
                "Code Insight",
                "Semantic highlighting from the language server",
                "Show parameter name hints",
                "Show usages of declarations",
                "Show usages above the declaration (else at the line end)",
                "Show code author",
                "Code Folding",
                "Fold one-line methods when a file opens",
                "Fold the imports of Java and Kotlin files when a file opens",
                "Show configuration values in place of ${…} placeholders",
            ],
            Page::Font => &["Font", "Size", "Line spacing", "Enable ligatures"],
            Page::CodeStyle => &[
                "Hard wrap at",
                "Show the right margin in the editor",
                "Indentation",
            ],
            Page::Plugins => &["Installed", "Install Plugin from Disk…"],
            Page::VersionControl => &[
                "Update method",
                "Allow force push (with lease)",
                "Protected branches",
                "Allow a classical SSH key exchange for Azure DevOps",
                "Allow a ProxyCommand from the SSH configuration",
            ],
            Page::Terminal => &["Shell path"],
            Page::Ai => &[
                "Enable AI Assistant",
                "Provider",
                "Endpoint",
                "Model",
                "API key",
                "Inline completion",
                "Sign in",
                "Serve MCP to local agents over HTTP",
                "Terminal lines to attach",
                "Allow agents and scripts to operate the window",
                "Claude plan",
            ],
            Page::Grammar => &[
                "Commit messages",
                "Comments",
                "Documentation",
                "String literals",
            ],
            Page::Keymap
            | Page::EditorTabs
            | Page::NaturalLanguages
            | Page::Inspections
            | Page::BuildTools
            | Page::TrustedLocations
            | Page::Languages
            | Page::Advanced => &[],
        }
    }

    /// Every label of a setting on the page: the fixed ones and the rows that come from a list
    /// (theme names, the Editor Tabs rows, the Code Style languages). The search finds the page
    /// by each of them (behaviour 3b).
    pub fn labels(self) -> Vec<&'static str> {
        let mut labels = self.fixed_labels().to_vec();
        match self {
            Page::Appearance => {
                labels.extend(ferris_idea_ui::theme::ThemeId::ALL.iter().map(|t| t.name()))
            }
            Page::EditorTabs => labels.extend(
                tab_setting_rows(&ferris_idea_core::EditorTabSettings::default())
                    .into_iter()
                    .map(|(label, _)| label),
            ),
            Page::CodeStyle => labels.extend(ferris_idea_core::CODE_STYLE_LANGUAGES),
            Page::Advanced => labels.extend(
                ferris_idea_core::advanced_settings::ADVANCED
                    .iter()
                    .map(|a| a.label),
            ),
            _ => {}
        }
        labels
    }

    /// The group the reference shows the page under, for the page list.
    pub fn group(self) -> Option<&'static str> {
        match self {
            // SPEC-0337 behaviour 1 (OBS-0048 S1, S2).
            Page::Appearance
            | Page::SystemSettings
            | Page::Notifications
            | Page::TrustedLocations => Some("Appearance & Behavior"),
            Page::Editor
            | Page::Font
            | Page::EditorTabs
            | Page::CodeStyle
            | Page::Inspections
            | Page::NaturalLanguages
            | Page::Grammar => Some("Editor"),
            Page::BuildTools => Some("Build, Execution, Deployment"),
            Page::Terminal | Page::Ai => Some("Tools"),
            _ => None,
        }
    }
}

/// Where the settings of a page belong (`SPEC-0105` v9 behaviour 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The IDE's own settings, the same for every project.
    Ide,
    /// Settings kept with the project and shared with its files.
    Project,
    /// Nothing is stored: the page shows what Ferris IDEA finds.
    Found,
}

impl Page {
    /// Where the page's settings belong and the sentence the bar above the page shows about it
    /// (`SPEC-0105` v9 behaviour 13).
    pub fn scope(self) -> (Scope, &'static str) {
        match self {
            Page::Keymap => (Scope::Ide, "IDE · keymaps/FerrisIDEA.toml"),
            Page::Plugins => (Scope::Ide, "IDE · plugins folder"),
            Page::TrustedLocations => (Scope::Ide, "IDE · trusted-projects.toml"),
            Page::Grammar => (
                Scope::Ide,
                "IDE · settings.toml for the scope; the rules are kept in the project's inspection profile",
            ),
            Page::Inspections => (
                Scope::Project,
                "Project · .idea/inspectionProfiles/Project_Default.xml, shared with the project's files",
            ),
            Page::BuildTools | Page::Languages => (
                Scope::Found,
                "Read from this project and this machine; nothing is stored here",
            ),
            Page::CodeStyle => (
                Scope::Ide,
                "IDE · settings.toml, the same for every project; a project's .editorconfig comes first",
            ),
            Page::Appearance
            | Page::SystemSettings
            | Page::Notifications
            | Page::Editor
            | Page::Font
            | Page::EditorTabs
            | Page::NaturalLanguages
            | Page::VersionControl
            | Page::Terminal
            | Page::Ai
            | Page::Advanced => (
                Scope::Ide,
                "IDE · settings.toml, the same for every project",
            ),
        }
    }
}

/// Pages whose title or keywords contain `query` (all pages for an empty query).
pub fn pages(query: &str) -> Vec<Page> {
    let q = query.trim().to_lowercase();
    Page::ALL
        .into_iter()
        .filter(|p| {
            q.is_empty()
                || p.title().to_lowercase().contains(&q)
                || p.keywords().contains(&q)
                // A setting's label finds its page (SPEC-0105 v5 behaviour 3b).
                || p.labels().iter().any(|l| l.to_lowercase().contains(&q))
                // Every action is found on the keymap page.
                || *p == Page::Keymap
                // An inspection's name finds its page (SPEC-0315 behaviour 4).
                || (*p == Page::Inspections && !inspection_rows(&q).is_empty())
                // A key or sentence of an advanced setting finds its page (SPEC-0337 5a).
                || (*p == Page::Advanced && !advanced_rows(&q).is_empty())
        })
        .collect()
}

/// The rows Advanced Settings lists for the dialog's search text `query` (`SPEC-0337` behaviour
/// 5a).
pub fn advanced_rows(
    query: &str,
) -> Vec<&'static ferris_idea_core::advanced_settings::AdvancedSetting> {
    ferris_idea_core::advanced_settings::rows(query)
}

/// The inspections the Inspections page lists for the dialog's search text `query`: all when the
/// text is empty or names the page, else those whose name, id, short name or language contains
/// every word (`SPEC-0315` behaviour 4). Sorted by language, then name.
pub fn inspection_rows(query: &str) -> Vec<ferris_idea_languages::inspections::InspectionInfo> {
    let q = query.trim().to_lowercase();
    let all = ferris_idea_languages::inspections::catalogue();
    if q.is_empty() || "inspections".contains(&q) || Page::Inspections.keywords().contains(&q) {
        return all;
    }
    all.into_iter()
        .filter(|i| {
            let hay = format!(
                "{} {} {} {}",
                i.name,
                i.id,
                i.short_name.unwrap_or_default(),
                i.language
            )
            .to_lowercase();
            q.split_whitespace().all(|w| hay.contains(w))
        })
        .collect()
}

/// The columns of a file type's row on the Grammar and Style page (`SPEC-0155` v8 behaviour 14):
/// the file type itself, then its contexts.
pub const GRAMMAR_COLUMNS: [&str; 4] = ["Checked", "Comments", "Documentation", "String literals"];

/// One row of the Grammar and Style page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarRow {
    /// Commit messages.
    CommitMessages,
    /// A file type with its contexts.
    FileType(&'static str),
    /// A writing rule of the inspection profile (its id).
    Rule(&'static str),
}

/// The rows of the Grammar and Style page in order: commit messages, every file type, the rules.
pub fn grammar_rows() -> Vec<GrammarRow> {
    let mut rows = vec![GrammarRow::CommitMessages];
    rows.extend(
        ferris_idea_languages::grazie::grammar_file_types()
            .into_iter()
            .map(GrammarRow::FileType),
    );
    rows.extend(
        ferris_idea_languages::inspections::WRITING_RULES
            .iter()
            .map(|r| GrammarRow::Rule(r)),
    );
    rows
}

/// Turns the switch of `row` in `column` over (`SPEC-0155` v8 behaviour 14); `false` for a rule
/// row, whose switch lives in the inspection profile.
pub fn toggle_grammar(
    g: &mut ferris_idea_core::GrammarSettings,
    row: GrammarRow,
    column: usize,
) -> bool {
    match row {
        GrammarRow::CommitMessages => g.commit_messages = !g.commit_messages,
        GrammarRow::FileType(ty) => {
            let mut scope = g.scope(ty);
            match column {
                0 => scope.enabled = !scope.enabled,
                1 => scope.comments = !scope.comments,
                2 => scope.documentation = !scope.documentation,
                _ => scope.string_literals = !scope.string_literals,
            }
            g.set_scope(ty, scope);
        }
        GrammarRow::Rule(_) => return false,
    }
    true
}

/// One row of the keymap page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapRow {
    /// Action id.
    pub action: String,
    /// The action's title as Find Action shows it, e.g. `Save All` for `SaveAll` (hand test 30).
    pub title: String,
    /// Its shortcuts as shown, comma-separated; empty when it has none.
    pub shortcuts: String,
    /// In the macOS keymap, a shortcut of it that macOS takes itself and what for
    /// (`SPEC-0105` v12 behaviour 18, IMP-0544): the key never reaches Ferris IDEA.
    pub system: Option<String>,
}

/// The chords macOS takes for itself before an application sees them, and what for
/// (`SPEC-0105` v12 behaviour 18; the system's default shortcuts).
const MACOS_RESERVED: [(&str, &str); 12] = [
    ("control SPACE", "the previous input source"),
    ("control alt SPACE", "the next input source"),
    ("meta SPACE", "Spotlight"),
    ("alt meta SPACE", "a Finder search window"),
    ("meta TAB", "switching applications"),
    ("shift meta 3", "screenshots"),
    ("shift meta 4", "screenshots"),
    ("shift meta 5", "screenshots"),
    ("control UP", "Mission Control"),
    ("control DOWN", "the application's windows"),
    ("control LEFT", "moving between spaces"),
    ("control RIGHT", "moving between spaces"),
];

/// What macOS takes `stroke` for, when it is one of its own chords (behaviour 18).
pub fn macos_reserved(stroke: &ferris_idea_ui::keymap::Keystroke) -> Option<&'static str> {
    MACOS_RESERVED.iter().find_map(|(text, what)| {
        ferris_idea_ui::keymap::Keystroke::parse(text)
            .ok()
            .filter(|k| k == stroke)
            .map(|_| *what)
    })
}

/// The note of the keymap page for the first of `shortcuts` that macOS takes (behaviour 18):
/// `⌃Space is taken by macOS (the previous input source)`.
pub fn macos_reserved_note(shortcuts: &[Shortcut]) -> Option<String> {
    shortcuts.iter().find_map(|s| match s {
        Shortcut::Keyboard {
            first,
            second: None,
        } => macos_reserved(first)
            .map(|what| format!("{} is taken by macOS ({what})", s.display(true))),
        _ => None,
    })
}

/// Rows for the actions of `keymap` and `extra` (actions without shortcuts in the bundled keymap),
/// sorted by id, whose id or shortcut text contains `query` (case and spaces ignored).
pub fn keymap_rows(keymap: &Keymap, extra: &[&str], query: &str) -> Vec<KeymapRow> {
    let fold = |s: &str| s.to_lowercase().replace([' ', '.', '_'], "");
    let q = fold(query);
    let mut ids: Vec<String> = keymap
        .actions()
        .map(|(id, _)| id.to_owned())
        .chain(extra.iter().map(|s| (*s).to_owned()))
        .collect();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .map(|action| {
            let shortcuts = keymap
                .shortcuts(&action)
                .iter()
                .map(|s: &Shortcut| s.display(keymap.macos))
                .collect::<Vec<_>>()
                .join(", ");
            let title = ferris_idea_languages::everywhere::action_title(&action).0;
            let system = keymap
                .macos
                .then(|| macos_reserved_note(keymap.shortcuts(&action)))
                .flatten();
            KeymapRow {
                action,
                title,
                shortcuts,
                system,
            }
        })
        .filter(|r| {
            q.is_empty()
                || fold(&r.action).contains(&q)
                || fold(&r.title).contains(&q)
                || fold(&r.shortcuts).contains(&q)
        })
        .collect()
}

/// Copies the settings page `page` shows from `from` into `to` (`SPEC-0105` v7 behaviour 8):
/// what "Revert changes" and "Restore defaults" act on. False for a page without settings of the
/// IDE layer of its own (Keymap, Plugins, Inspections, Build Tools, Languages, Trusted Locations).
pub fn copy_page(
    page: Page,
    from: &ferris_idea_core::IdeSettings,
    to: &mut ferris_idea_core::IdeSettings,
) -> bool {
    match page {
        Page::Appearance => to.appearance = from.appearance.clone(),
        Page::SystemSettings => to.system = from.system.clone(),
        Page::Notifications => to.notifications = from.notifications.clone(),
        Page::Editor => {
            let (f, t) = (&from.editor, &mut to.editor);
            t.strip_trailing_spaces = f.strip_trailing_spaces;
            t.ensure_line_break_at_end = f.ensure_line_break_at_end;
            t.semantic_highlighting = f.semantic_highlighting;
            t.parameter_hints = f.parameter_hints;
            t.usage_counts = f.usage_counts;
            t.usage_counts_above = f.usage_counts_above;
            t.code_authors = f.code_authors;
            t.highlight_usages_at_caret = f.highlight_usages_at_caret;
            t.fold_one_line_methods = f.fold_one_line_methods;
            t.fold_imports = f.fold_imports;
            t.config_values = f.config_values;
            t.folding_arrows = f.folding_arrows;
        }
        Page::Font => {
            let (f, t) = (&from.editor, &mut to.editor);
            t.font_family = f.font_family.clone();
            t.font_size = f.font_size;
            t.line_spacing = f.line_spacing;
            t.ligatures = f.ligatures;
        }
        Page::EditorTabs => to.editor.tabs = from.editor.tabs.clone(),
        Page::NaturalLanguages => {
            to.editor.spelling_dictionary = from.editor.spelling_dictionary.clone()
        }
        Page::Grammar => to.editor.grammar = from.editor.grammar.clone(),
        Page::CodeStyle => to.code_style = from.code_style.clone(),
        Page::VersionControl => {
            to.vcs = from.vcs.clone();
            to.git.ssh.allow_classic_kex_for_azure_devops =
                from.git.ssh.allow_classic_kex_for_azure_devops;
            to.ssh.allow_own_proxy_command = from.ssh.allow_own_proxy_command;
        }
        Page::Terminal => to.terminal = from.terminal.clone(),
        Page::Ai => {
            to.ai = from.ai.clone();
            to.mcp.http = from.mcp.http;
            to.ui.driver.external = from.ui.driver.external;
        }
        Page::Advanced => {
            for a in ferris_idea_core::advanced_settings::ADVANCED {
                let _ = a.set(to, a.value(from));
            }
        }
        Page::Keymap
        | Page::Plugins
        | Page::Inspections
        | Page::BuildTools
        | Page::Languages
        | Page::TrustedLocations => return false,
    }
    true
}

/// Whether the settings of page `page` differ between `a` and `b` (behaviour 8).
pub fn page_differs(
    page: Page,
    a: &ferris_idea_core::IdeSettings,
    b: &ferris_idea_core::IdeSettings,
) -> bool {
    let mut x = a.clone();
    copy_page(page, b, &mut x) && x != *a
}

/// Whether page `page` holds a setting changed from its default: the page list marks it with a
/// dot (`SPEC-0105` v7 behaviour 9).
pub fn page_modified(page: Page, ide: &ferris_idea_core::IdeSettings) -> bool {
    page_differs(page, ide, &ferris_idea_core::IdeSettings::default())
}

/// Whether `label`, drawn on the page, matches the search text `query`: the page marks it
/// (`SPEC-0105` v7 behaviour 7).
pub fn label_matches(label: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    !q.is_empty() && label.to_lowercase().contains(&q)
}

/// The rows of Advanced Settings: those the dialog's search and the page's own search
/// `own` both leave, and with `modified_only` only those changed from their default
/// (`SPEC-0337` v2 behaviour 5b).
pub fn advanced_page_rows(
    dialog: &str,
    own: &str,
    modified_only: bool,
    ide: &ferris_idea_core::IdeSettings,
) -> Vec<&'static ferris_idea_core::advanced_settings::AdvancedSetting> {
    let own = own.trim().to_lowercase();
    advanced_rows(dialog)
        .into_iter()
        .filter(|a| own.is_empty() || a.matches(&own))
        .filter(|a| !modified_only || a.modified(ide))
        .collect()
}

/// Room the dialog leaves to the window's edges and its lower edge (`SPEC-0105` v8 behaviour 12).
pub const DIALOG_MARGIN: f32 = 40.0;
/// The smallest dialog, width and height.
pub const DIALOG_MIN: (f32, f32) = (560.0, 320.0);
/// The largest dialog, width and height.
pub const DIALOG_MAX: (f32, f32) = (1000.0, 900.0);

/// Width and height of the Settings dialog in a window `viewport` points large, the dialog's
/// top `top` points below the window's (`SPEC-0105` v8 behaviour 12): the window's room less the
/// margins, within the limits.
pub fn dialog_size(viewport: (f32, f32), top: f32) -> (f32, f32) {
    let (w, h) = viewport;
    (
        (w - 2.0 * DIALOG_MARGIN).clamp(DIALOG_MIN.0, DIALOG_MAX.0),
        (h - top - DIALOG_MARGIN).clamp(DIALOG_MIN.1, DIALOG_MAX.1),
    )
}

/// The page ↑ (`down` false) or ↓ goes to from `page` in the page list `list`; `None` at its
/// first and its last page (`SPEC-0105` v8 behaviour 11).
pub fn step_page(list: &[Page], page: Page, down: bool) -> Option<Page> {
    let Some(at) = list.iter().position(|p| *p == page) else {
        return list.first().copied();
    };
    if down {
        list.get(at + 1).copied()
    } else {
        at.checked_sub(1).and_then(|i| list.get(i).copied())
    }
}

/// Whether the page rolls its own list between fixed lines; every other page rolls as a whole
/// when it is taller than its area (`SPEC-0105` v8 behaviour 12).
pub fn page_rolls_itself(page: Page) -> bool {
    matches!(page, Page::Keymap | Page::Inspections)
}

#[cfg(test)]
mod page_settings_tests {
    use super::*;
    use ferris_idea_core::IdeSettings;

    /// SPEC-0105 v6 AK-11: Revert and Restore defaults act on the page's settings only; a page
    /// with a changed setting is marked; the search marks matching labels.
    #[test]
    fn revert_restore_and_marks_act_on_one_page() {
        let opened = IdeSettings::default();
        let mut now = opened.clone();
        now.editor.font_size = 16.0;
        now.editor.strip_trailing_spaces = !opened.editor.strip_trailing_spaces;
        now.terminal.shell = Some("/bin/bash".into());
        assert!(page_differs(Page::Font, &now, &opened));
        assert!(page_differs(Page::Editor, &now, &opened));
        assert!(!page_differs(Page::CodeStyle, &now, &opened));
        assert!(page_modified(Page::Terminal, &now));
        assert!(!page_modified(Page::Appearance, &now));
        // Revert the Font page: the size goes back, the Editor and Terminal pages keep theirs.
        let mut reverted = now.clone();
        assert!(copy_page(Page::Font, &opened, &mut reverted));
        assert_eq!(reverted.editor.font_size, opened.editor.font_size);
        assert_eq!(
            reverted.editor.strip_trailing_spaces,
            now.editor.strip_trailing_spaces
        );
        assert_eq!(reverted.terminal.shell.as_deref(), Some("/bin/bash"));
        assert!(
            !copy_page(Page::Keymap, &opened, &mut reverted),
            "no settings of its own"
        );
        // Every page with settings of its own is reverted by its copy.
        for page in Page::ALL {
            let mut x = now.clone();
            if copy_page(page, &opened, &mut x) {
                assert!(!page_differs(page, &x, &opened), "{page:?}");
            }
        }
        assert!(label_matches("Line spacing", "spac"));
        assert!(!label_matches("Line spacing", ""));
        assert!(!label_matches("Size", "font"));
    }

    /// SPEC-0337 v2 AK-6: the page's own search and "Show modified only".
    #[test]
    fn advanced_rows_search_and_modified_only() {
        let mut ide = IdeSettings::default();
        let all = advanced_page_rows("", "", false, &ide);
        assert_eq!(
            all.len(),
            ferris_idea_core::advanced_settings::ADVANCED.len()
        );
        assert!(
            advanced_page_rows("", "", true, &ide).is_empty(),
            "nothing modified"
        );
        let first = all[0];
        first.step(&mut ide, 1);
        assert!(first.modified(&ide));
        let modified = advanced_page_rows("", "", true, &ide);
        assert_eq!(
            modified.iter().map(|a| a.key).collect::<Vec<_>>(),
            [first.key]
        );
        let found = advanced_page_rows("", "report", false, &ide);
        assert!(found.iter().all(|a| a.matches("report")) && !found.is_empty());
        assert!(advanced_page_rows("", "no such setting", false, &ide).is_empty());
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0105 v12 AK-20 (behaviour 18, OBS-0119 K5, IMP-0544): in the macOS keymap a shortcut
    /// macOS takes is marked with what for; other chords and the default keymap are not.
    #[test]
    fn chords_macos_takes_are_marked() {
        use ferris_idea_ui::keymap::Keystroke;
        let k = |t: &str| Keystroke::parse(t).unwrap();
        assert_eq!(
            macos_reserved(&k("control SPACE")),
            Some("the previous input source")
        );
        assert_eq!(macos_reserved(&k("meta SPACE")), Some("Spotlight"));
        assert_eq!(macos_reserved(&k("meta shift SPACE")), None);
        assert_eq!(macos_reserved(&k("control shift SPACE")), None);
        let one = |t: &str| Shortcut::Keyboard {
            first: k(t),
            second: None,
        };
        assert_eq!(
            macos_reserved_note(&[one("meta J"), one("control SPACE")]).as_deref(),
            Some("⌃Space is taken by macOS (the previous input source)")
        );
        assert_eq!(macos_reserved_note(&[one("meta J")]), None);
        // The bundled macOS keymap: basic completion on ⌃Space is marked, ⌘J is not.
        let mac = Keymap::bundled_macos();
        let rows = keymap_rows(&mac, &[], "CodeCompletion");
        let basic = rows.iter().find(|r| r.action == "CodeCompletion").unwrap();
        assert!(
            basic
                .system
                .as_deref()
                .is_some_and(|n| n.contains("input source"))
        );
        let template = keymap_rows(&mac, &[], "InsertLiveTemplate");
        assert!(template.iter().all(|r| r.system.is_none()));
        let default = Keymap::bundled_default();
        assert!(
            keymap_rows(&default, &[], "CodeCompletion")
                .iter()
                .all(|r| r.system.is_none())
        );
    }

    /// SPEC-0105 AK-15 (v9): every page says where its settings are kept.
    #[test]
    fn every_page_says_where_its_settings_are_kept() {
        let mut probe = ferris_idea_core::IdeSettings::default();
        for page in Page::ALL {
            let (scope, text) = page.scope();
            assert!(!text.is_empty(), "{page:?}");
            assert_eq!(
                scope == Scope::Project,
                page == Page::Inspections,
                "{page:?}"
            );
            // The pages with settings of the IDE layer of their own name `settings.toml`.
            let own = copy_page(page, &ferris_idea_core::IdeSettings::default(), &mut probe);
            assert_eq!(own, text.contains("settings.toml"), "{page:?}: {text}");
            if own {
                assert_eq!(scope, Scope::Ide);
            }
        }
        assert!(Page::CodeStyle.scope().1.contains(".editorconfig"));
        assert!(Page::Inspections.scope().1.contains("Project_Default.xml"));
        assert_eq!(Page::BuildTools.scope().0, Scope::Found);
        assert_eq!(Page::Keymap.scope().0, Scope::Ide);
    }
    use super::*;

    /// SPEC-0155/AK-19 (v8, OBS-0117 G6): the Grammar and Style rows and their switches.
    #[test]
    fn grammar_rows_and_switches() {
        let rows = grammar_rows();
        assert_eq!(rows[0], GrammarRow::CommitMessages);
        assert!(rows.contains(&GrammarRow::FileType("Java")));
        assert!(rows.contains(&GrammarRow::FileType("Plain text")));
        assert_eq!(
            rows.last(),
            Some(&GrammarRow::Rule("grazie.mixed-spelling"))
        );
        let mut g = ferris_idea_core::GrammarSettings::default();
        assert!(toggle_grammar(&mut g, GrammarRow::FileType("Java"), 3));
        assert!(g.scope("Java").string_literals);
        assert!(toggle_grammar(&mut g, GrammarRow::FileType("Java"), 3));
        assert!(g.file_types.is_empty(), "back to the default, nothing kept");
        assert!(toggle_grammar(&mut g, GrammarRow::FileType("Markdown"), 0));
        assert!(!g.scope("Markdown").enabled);
        assert!(toggle_grammar(&mut g, GrammarRow::CommitMessages, 0));
        assert!(!g.commit_messages);
        assert!(!toggle_grammar(&mut g, GrammarRow::Rule("grazie.typo"), 0));
        let mut to = ferris_idea_core::IdeSettings::default();
        let mut from = to.clone();
        from.editor.grammar = g.clone();
        assert!(copy_page(Page::Grammar, &from, &mut to));
        assert_eq!(to.editor.grammar, g);
    }

    /// SPEC-0105 v8 AK-13: the dialog takes the window's room (OBS-0062 S7: in a window 901
    /// points high it used about two thirds), within its limits.
    #[test]
    fn the_dialog_takes_the_windows_room() {
        let top = 80.0;
        let (w, h) = dialog_size((1300.0, 901.0), top);
        assert_eq!(w, DIALOG_MAX.0);
        assert_eq!(h, 901.0 - top - DIALOG_MARGIN);
        assert!(h > 0.85 * (901.0 - top), "{h}");
        assert_eq!(dialog_size((400.0, 300.0), top), DIALOG_MIN);
        assert_eq!(dialog_size((4000.0, 3000.0), top), DIALOG_MAX);
        assert_eq!(dialog_size((800.0, 700.0), top), (720.0, 580.0));
    }

    /// SPEC-0105 v8 AK-13: ↑/↓ on the list go page by page and stop at its ends; with a search
    /// they go through the pages it leaves.
    #[test]
    fn arrows_on_the_list_go_through_the_pages() {
        let all = pages("");
        assert_eq!(
            step_page(&all, Page::Appearance, true),
            Some(Page::SystemSettings)
        );
        assert_eq!(step_page(&all, Page::Appearance, false), None);
        assert_eq!(step_page(&all, Page::Advanced, true), None);
        assert_eq!(step_page(&all, Page::Advanced, false), Some(Page::Ai));
        let mut at = Page::Appearance;
        let mut steps = 0;
        while let Some(next) = step_page(&all, at, true) {
            at = next;
            steps += 1;
        }
        assert_eq!((at, steps), (Page::Advanced, Page::ALL.len() - 1));
        let found = pages("theme");
        assert_eq!(
            step_page(&found, Page::Appearance, true),
            Some(Page::Keymap)
        );
        // A page the search took away: the list's first.
        assert_eq!(step_page(&found, Page::Font, true), Some(Page::Appearance));
        assert!(page_rolls_itself(Page::Keymap) && !page_rolls_itself(Page::Font));
    }

    #[test]
    fn search_filters_pages_and_rows() {
        assert_eq!(pages(""), Page::ALL);
        assert_eq!(
            pages("theme"),
            [Page::Appearance, Page::Keymap],
            "the keymap page always searches actions"
        );
        assert_eq!(pages("shortcut"), [Page::Keymap]);
        assert_eq!(pages("plugin"), [Page::Keymap, Page::Plugins]);
        assert_eq!(pages("font size"), [Page::Keymap, Page::Font]);
        assert!(pages("spelling").contains(&Page::NaturalLanguages));
        assert!(pages("string literals").contains(&Page::Grammar));
        assert_eq!(pages("jdk"), [Page::Keymap, Page::BuildTools]);
        assert_eq!(pages("shell"), [Page::Keymap, Page::Terminal]);
        assert_eq!(pages("safe mode"), [Page::TrustedLocations, Page::Keymap]);
        assert_eq!(
            Page::TrustedLocations.group(),
            Some("Appearance & Behavior")
        );
        let keymap = Keymap::bundled_default();
        let rows = keymap_rows(&keymap, &["ChangeLaf"], "save all");
        assert!(
            rows.iter()
                .any(|r| r.action == "SaveAll" && r.title == "Save All" && !r.shortcuts.is_empty())
        );
        let laf = keymap_rows(&keymap, &["ChangeLaf"], "changelaf");
        assert_eq!(laf.len(), 1);
        assert_eq!(laf[0].shortcuts, "");
        let all = keymap_rows(&keymap, &[], "");
        assert_eq!(all.len(), keymap.len());
        assert!(all.windows(2).all(|w| w[0].action < w[1].action));
        // By shortcut text.
        let by_key = keymap_rows(&keymap, &[], "ctrl+alt+s");
        assert!(
            by_key.iter().any(|r| r.action == "ShowSettings"),
            "{by_key:?}"
        );
    }

    /// SPEC-0105 v5 AK-8: every page of the specs exists under its group, and the search finds
    /// each page by every label of a setting on it, whole or by a word of it.
    #[test]
    fn every_setting_is_found_by_its_label() {
        // The pages SPEC-0105, SPEC-0129 and SPEC-0150 list, with their groups.
        let listed = [
            ("Appearance", Some("Appearance & Behavior")),
            ("System Settings", Some("Appearance & Behavior")),
            ("Notifications", Some("Appearance & Behavior")),
            ("Trusted Locations", Some("Appearance & Behavior")),
            ("Advanced Settings", None),
            ("Keymap", None),
            ("General", Some("Editor")),
            ("Font", Some("Editor")),
            ("Code Style", Some("Editor")),
            ("Plugins", None),
            ("Version Control", None),
            ("Build Tools", Some("Build, Execution, Deployment")),
            ("Languages & Frameworks", None),
            ("Terminal", Some("Tools")),
        ];
        for (title, group) in listed {
            let page = Page::ALL.into_iter().find(|p| p.title() == title);
            assert_eq!(page.map(Page::group), Some(group), "page {title}");
        }
        let mut missing = Vec::new();
        for page in Page::ALL {
            assert!(pages(page.title()).contains(&page), "{page:?} by its title");
            for label in page.labels() {
                if !pages(label).contains(&page) || !pages(&label.to_uppercase()).contains(&page) {
                    missing.push(format!("{page:?}: {label}"));
                }
                // A word of the label is enough.
                let word = label
                    .split_whitespace()
                    .max_by_key(|w| w.len())
                    .unwrap_or(label);
                if !pages(word).contains(&page) {
                    missing.push(format!("{page:?}: {label} by {word}"));
                }
            }
        }
        assert!(missing.is_empty(), "not found: {missing:#?}");
        // The settings of hand tests 56 and 76 by what their rows say.
        assert_eq!(pages("sync with os"), [Page::Appearance, Page::Keymap]);
        assert_eq!(pages("compact mode"), [Page::Appearance, Page::Keymap]);
        assert_eq!(pages("line spacing"), [Page::Keymap, Page::Font]);
        assert_eq!(pages("ligatures"), [Page::Keymap, Page::Font]);
        assert_eq!(pages("hard wrap at"), [Page::Keymap, Page::CodeStyle]);
        assert_eq!(
            pages("protected branches"),
            [Page::Keymap, Page::VersionControl]
        );
        assert_eq!(pages("update method"), [Page::Keymap, Page::VersionControl]);
        assert_eq!(pages("shell path"), [Page::Keymap, Page::Terminal]);
        assert_eq!(
            pages("ends with a line break"),
            [Page::Keymap, Page::Editor]
        );
        assert!(pages("islands light").contains(&Page::Appearance));
    }

    /// SPEC-0337 AK-2: the reference's outline — Appearance & Behavior first with its four pages,
    /// Advanced Settings last — the new pages found by their labels, Advanced Settings by a key
    /// with its rows filtered.
    #[test]
    fn the_outline_follows_the_reference() {
        let head: Vec<_> = Page::ALL[..5]
            .iter()
            .map(|p| (p.title(), p.group()))
            .collect();
        let ab = Some("Appearance & Behavior");
        assert_eq!(
            head,
            [
                ("Appearance", ab),
                ("System Settings", ab),
                ("Notifications", ab),
                ("Trusted Locations", ab),
                ("Keymap", None),
            ]
        );
        assert_eq!(Page::ALL.last(), Some(&Page::Advanced));
        assert_eq!(Page::Advanced.group(), None);
        // A group's pages stand together.
        let mut groups: Vec<Option<&str>> = Vec::new();
        for p in Page::ALL {
            if groups.last() != Some(&p.group()) {
                assert!(
                    p.group().is_none() || !groups.contains(&p.group()),
                    "{p:?} apart from its group"
                );
                groups.push(p.group());
            }
        }
        assert_eq!(
            pages("confirm before quitting"),
            [Page::SystemSettings, Page::Keymap]
        );
        assert!(pages("reopen").contains(&Page::SystemSettings));
        assert!(pages("informational").contains(&Page::Notifications));
        assert!(pages("color scheme").contains(&Page::Appearance));
        assert!(pages("colour scheme").contains(&Page::Appearance));
        assert_eq!(
            pages("project_view.compact"),
            [Page::Keymap, Page::Advanced]
        );
        assert_eq!(advanced_rows("project_view.compact").len(), 1);
        assert_eq!(
            advanced_rows("").len(),
            ferris_idea_core::advanced_settings::ADVANCED.len()
        );
        // The searches of the window self-test keep their pages (SPEC-0105 v5 AK-8).
        assert_eq!(pages("sync with os"), [Page::Appearance, Page::Keymap]);
        assert_eq!(pages("trust"), [Page::TrustedLocations, Page::Keymap]);
    }
}

/// The rows of Settings › Editor › Editor Tabs with their values as shown (`SPEC-0059` v9
/// behaviour 17).
pub fn tab_setting_rows(s: &ferris_idea_core::EditorTabSettings) -> Vec<(&'static str, String)> {
    use ferris_idea_core::{
        ActivateOnClose, CloseButtonPlace, TabLimitRule, TabPlacement, TabRows,
    };
    let on = |b: bool| if b { "☑" } else { "☐" }.to_owned();
    vec![
        (
            "Tab placement",
            match s.placement {
                TabPlacement::Top => "Top",
                TabPlacement::None => "None",
            }
            .into(),
        ),
        (
            "Tabs that do not fit",
            match s.rows {
                TabRows::Scroll => "One row, scrolling",
                TabRows::Squeeze => "One row, squeezed",
                TabRows::Wrap => "Multiple rows",
            }
            .into(),
        ),
        ("Pinned tabs in a separate row", on(s.pinned_row)),
        ("Show file icon", on(s.show_icon)),
        ("Show file extension", on(s.show_extension)),
        (
            "Show the folder for equal names",
            on(s.show_folder_for_equal_names),
        ),
        ("Mark modified tabs with *", on(s.mark_modified)),
        ("Show the full path on hover", on(s.path_tooltip)),
        (
            "Close button position",
            match s.close_button {
                CloseButtonPlace::Right => "Right",
                CloseButtonPlace::Left => "Left",
                CloseButtonPlace::None => "None",
            }
            .into(),
        ),
        (
            "Close button on",
            if s.close_button_on_all {
                "Every tab"
            } else {
                "Active and hovered tab"
            }
            .into(),
        ),
        ("Sort tabs alphabetically", on(s.sort_alphabetically)),
        ("Open new tabs at the end", on(s.open_at_end)),
        (
            "Preview tab (single click in the project tree)",
            on(s.preview_tab),
        ),
        ("Tab limit", s.limit.to_string()),
        (
            "When the limit is reached",
            match s.limit_rule {
                TabLimitRule::CloseUnchanged => "Close unchanged",
                TabLimitRule::CloseUnused => "Close unused (saved first)",
            }
            .into(),
        ),
        (
            "When the active tab closes, activate",
            match s.activate_on_close {
                ActivateOnClose::Recent => "Most recently opened",
                ActivateOnClose::Left => "The tab on the left",
                ActivateOnClose::Right => "The tab on the right",
            }
            .into(),
        ),
    ]
}

/// Changes row `row` of the Editor Tabs page: a switch toggles, a choice moves by `step` (0 is
/// forward), the limit changes by `step` within 1–100.
pub fn change_tab_setting(s: &mut ferris_idea_core::EditorTabSettings, row: usize, step: i32) {
    use ferris_idea_core::{
        ActivateOnClose, CloseButtonPlace, TabLimitRule, TabPlacement, TabRows,
    };
    fn cycle<T: Copy + PartialEq>(all: &[T], now: T, step: i32) -> T {
        let n = all.len() as i32;
        let at = all.iter().position(|v| *v == now).unwrap_or(0) as i32;
        let by = if step < 0 { -1 } else { 1 };
        all[(at + by).rem_euclid(n) as usize]
    }
    match row {
        0 => s.placement = cycle(&[TabPlacement::Top, TabPlacement::None], s.placement, step),
        1 => {
            s.rows = cycle(
                &[TabRows::Scroll, TabRows::Squeeze, TabRows::Wrap],
                s.rows,
                step,
            )
        }
        2 => s.pinned_row = !s.pinned_row,
        3 => s.show_icon = !s.show_icon,
        4 => s.show_extension = !s.show_extension,
        5 => s.show_folder_for_equal_names = !s.show_folder_for_equal_names,
        6 => s.mark_modified = !s.mark_modified,
        7 => s.path_tooltip = !s.path_tooltip,
        8 => {
            s.close_button = cycle(
                &[
                    CloseButtonPlace::Right,
                    CloseButtonPlace::Left,
                    CloseButtonPlace::None,
                ],
                s.close_button,
                step,
            )
        }
        9 => s.close_button_on_all = !s.close_button_on_all,
        10 => s.sort_alphabetically = !s.sort_alphabetically,
        11 => s.open_at_end = !s.open_at_end,
        12 => s.preview_tab = !s.preview_tab,
        13 => {
            let (lo, hi) = ferris_idea_core::TAB_LIMITS;
            s.limit = (s.limit as i64 + step as i64).clamp(lo as i64, hi as i64) as u32;
        }
        14 => {
            s.limit_rule = cycle(
                &[TabLimitRule::CloseUnchanged, TabLimitRule::CloseUnused],
                s.limit_rule,
                step,
            )
        }
        15 => {
            s.activate_on_close = cycle(
                &[
                    ActivateOnClose::Recent,
                    ActivateOnClose::Left,
                    ActivateOnClose::Right,
                ],
                s.activate_on_close,
                step,
            )
        }
        _ => {}
    }
}

#[cfg(test)]
mod tab_settings_tests {
    use super::*;

    /// SPEC-0059 v9 behaviour 17: the page lists every option and each row changes its setting.
    #[test]
    fn the_editor_tabs_page_changes_every_option() {
        let mut s = ferris_idea_core::EditorTabSettings::default();
        let rows = tab_setting_rows(&s);
        assert_eq!(rows.len(), 16);
        assert_eq!(rows[13], ("Tab limit", "30".to_owned()));
        assert_eq!(rows[9].1, "Active and hovered tab");
        assert_eq!(rows[6].1, "☑", "the modified mark is on by default");
        for row in 0..rows.len() {
            let before = s.clone();
            change_tab_setting(&mut s, row, 1);
            assert_ne!(before, s, "row {row} changes its setting");
        }
        change_tab_setting(&mut s, 13, -500);
        assert_eq!(s.limit, 1);
        change_tab_setting(&mut s, 13, 500);
        assert_eq!(s.limit, 100);
        assert!(pages("pinned").contains(&Page::EditorTabs));
        assert_eq!(Page::EditorTabs.group(), Some("Editor"));
    }
}

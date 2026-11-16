//! The rows of the run widget's list and the configuration chooser (`SPEC-0087` v6 behaviours
//! 9–11, v7 behaviour 15), free of the UI toolkit: headings, the order of entries, the digits that
//! pick a row, the filter typed into the list, the actions of a row, and the pins kept in the
//! project's workspace state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use idecraft_core::RunConfig;

/// The row that opens the configuration editor (behaviour 9, `SPEC-0327`).
pub const EDIT_CONFIGURATIONS: &str = "Edit Configurations…";
/// The row that selects running the file of the focused editor (behaviour 9).
pub const CURRENT_FILE: &str = "Current File";
/// The chooser's footer (behaviour 9).
pub const FOOTER: &str = "Hold ⇧ to debug the chosen configuration";
/// The footer of the chooser opened for debugging (`SPEC-0087` v8 behaviour 16).
pub const FOOTER_DEBUG: &str = "Hold ⇧ to run the chosen configuration";

/// The chooser's footer: it names the way of starting that ⇧ gives.
pub fn footer(debug_first: bool) -> &'static str {
    if debug_first { FOOTER_DEBUG } else { FOOTER }
}

/// Whether a choice debugs: with ⇧ in the chooser, without it in the chooser opened for debugging
/// (`ChooseDebugConfiguration`, `SPEC-0087` v8 behaviour 16). The result is the `shift` of
/// [`choice`].
pub fn debug_asked(debug_first: bool, shift: bool) -> bool {
    debug_first != shift
}
/// Heading of pinned configurations (behaviour 10).
pub const PINNED: &str = "Pinned";
/// Heading of the configurations of `run.toml` below pinned ones, so they do not look pinned
/// (window check of #485, finding a).
pub const PROJECT: &str = "Run Configurations";
/// Heading of the configurations made by Run Current File (behaviour 11).
pub const TEMPORARY: &str = "Temporary";
/// Heading of the configurations detected from build files (behaviour 8).
pub const DETECTED: &str = "Detected from build files";

/// Where a configuration comes from, which decides its heading and its actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Section {
    /// Pinned by the user (from any of the others).
    Pinned,
    /// Written in `run.toml` (or a Before Run task of the build tools window).
    Project,
    /// Made by Run Current File for this session.
    Temporary,
    /// Detected from the build files.
    Detected,
    /// Suggested by a plugin, under this heading.
    Plugin(String),
}

/// What a row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// A configuration and where it comes from (for a pinned one: where it came from).
    Config {
        /// Its name.
        name: String,
        /// Its section as listed.
        section: Section,
        /// The section it has without the pin.
        origin: Section,
    },
    /// "Current File".
    CurrentFile,
    /// "Edit Configurations…".
    EditConfigurations,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// What it stands for.
    pub kind: RowKind,
    /// The heading shown above it, for the first row of a section with one.
    pub heading: Option<String>,
    /// The digit that picks it (behaviour 9).
    pub digit: Option<u8>,
    /// The command line shown after the name, shell-quoted (behaviour 13).
    pub command: String,
    /// Why it cannot start, shown as a warning sign with this hint (behaviour 14).
    pub warning: Option<String>,
    /// The folder of a configuration of `run.toml` listed under its folder's heading
    /// (`SPEC-0327` v2 behaviour 11); a pinned one is listed under "Pinned" without it.
    pub folder: Option<String>,
}

impl ListRow {
    /// The label: the configuration's name or the entry's text.
    pub fn label(&self) -> &str {
        match &self.kind {
            RowKind::Config { name, .. } => name,
            RowKind::CurrentFile => CURRENT_FILE,
            RowKind::EditConfigurations => EDIT_CONFIGURATIONS,
        }
    }

    /// The configuration's name, for a configuration row.
    pub fn config_name(&self) -> Option<&str> {
        match &self.kind {
            RowKind::Config { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// What the list is made of.
#[derive(Debug, Default, Clone)]
pub struct ListInput<'a> {
    /// Every configuration in load order.
    pub configs: &'a [RunConfig],
    /// Names of the temporary configurations.
    pub temporary: &'a [String],
    /// Names of the detected configurations.
    pub detected: &'a [String],
    /// Plugin suggestions: configuration name and heading.
    pub plugin: &'a [(String, String)],
    /// Pinned names, in the order they were pinned.
    pub pins: &'a [String],
    /// Why a configuration cannot start, by name.
    pub blockers: Option<&'a HashMap<String, String>>,
}

impl ListInput<'_> {
    fn origin(&self, name: &str) -> Section {
        if self.temporary.iter().any(|n| n == name) {
            Section::Temporary
        } else if self.detected.iter().any(|n| n == name) {
            Section::Detected
        } else if let Some((_, h)) = self.plugin.iter().find(|(n, _)| n == name) {
            Section::Plugin(h.clone())
        } else {
            Section::Project
        }
    }
}

fn heading(section: &Section) -> Option<String> {
    match section {
        Section::Pinned => Some(PINNED.into()),
        Section::Project => None,
        Section::Temporary => Some(TEMPORARY.into()),
        Section::Detected => Some(DETECTED.into()),
        Section::Plugin(h) => Some(h.clone()),
    }
}

/// The rows in order (behaviours 9–11): pinned configurations under "Pinned", those of `run.toml`,
/// the temporary ones under "Temporary", the detected ones, the plugins' suggestions, then
/// "Current File" and "Edit Configurations…". The first nine rows get the digits 1–9 and "Edit
/// Configurations…" gets 0.
pub fn rows(input: &ListInput) -> Vec<ListRow> {
    let config_row = |c: &RunConfig, section: Section, origin: Section| ListRow {
        kind: RowKind::Config {
            name: c.name.clone(),
            section,
            origin,
        },
        heading: None,
        digit: None,
        command: if c.command.is_empty() {
            c.url.clone().unwrap_or_default()
        } else {
            idecraft_core::join_command_line(&c.command)
        },
        warning: input.blockers.and_then(|b| b.get(&c.name).cloned()),
        folder: None,
    };
    let mut out: Vec<ListRow> = Vec::new();
    for pin in input.pins {
        if let Some(c) = input.configs.iter().find(|c| &c.name == pin) {
            out.push(config_row(c, Section::Pinned, input.origin(&c.name)));
        }
    }
    let unpinned = |c: &&RunConfig| !input.pins.contains(&c.name);
    let mut plugin_headings: Vec<String> = Vec::new();
    for (_, h) in input.plugin {
        if !plugin_headings.contains(h) {
            plugin_headings.push(h.clone());
        }
    }
    let mut sections = vec![Section::Project, Section::Temporary, Section::Detected];
    sections.extend(plugin_headings.into_iter().map(Section::Plugin));
    for section in sections {
        let of_section: Vec<&RunConfig> = input
            .configs
            .iter()
            .filter(unpinned)
            .filter(|c| input.origin(&c.name) == section)
            .collect();
        if section != Section::Project {
            for c in of_section {
                out.push(config_row(c, section.clone(), section.clone()));
            }
            continue;
        }
        // Those of `run.toml`: first the ones without a folder, then folder by folder in the
        // order of each folder's first configuration (SPEC-0327 v2 behaviour 11).
        let mut folders: Vec<Option<&str>> = vec![None];
        for c in &of_section {
            if !folders.contains(&c.folder.as_deref()) {
                folders.push(c.folder.as_deref());
            }
        }
        for folder in folders {
            for c in of_section.iter().filter(|c| c.folder.as_deref() == folder) {
                let mut row = config_row(c, Section::Project, Section::Project);
                row.folder = folder.map(str::to_owned);
                out.push(row);
            }
        }
    }
    out.push(ListRow {
        kind: RowKind::CurrentFile,
        heading: None,
        digit: None,
        command: String::new(),
        warning: None,
        folder: None,
    });
    // Headings above the first row of each section; those of `run.toml` get one only below
    // pinned ones.
    let mut last: Option<(Section, Option<String>)> = None;
    for row in &mut out {
        if let RowKind::Config { section, .. } = &row.kind {
            let group = (section.clone(), row.folder.clone());
            if last.as_ref() != Some(&group) {
                row.heading = match (section, &row.folder, last.as_ref().map(|l| &l.0)) {
                    (Section::Project, Some(folder), _) => Some(folder.clone()),
                    (Section::Project, None, Some(Section::Pinned)) => Some(PROJECT.into()),
                    _ => heading(section),
                };
            }
            last = Some(group);
        }
    }
    for (i, row) in out.iter_mut().take(9).enumerate() {
        row.digit = Some(i as u8 + 1);
    }
    out.push(ListRow {
        kind: RowKind::EditConfigurations,
        heading: None,
        digit: Some(0),
        command: String::new(),
        warning: None,
        folder: None,
    });
    out
}

/// What choosing a row asks for (`SPEC-0087` behaviours 2 and 9; `SPEC-0327` behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Run this configuration.
    Run(String),
    /// Only select it (the run widget's list).
    Select(String),
    /// Debug it (⇧ held).
    Debug(String),
    /// Run or debug the current file, or in the run widget's list only select doing so.
    CurrentFile {
        /// Debug instead of run.
        debug: bool,
        /// Only select it.
        select_only: bool,
    },
    /// Open the configuration editor — the same in the run widget's list and in the chooser.
    EditConfigurations,
}

/// What choosing `row` asks for: with ⇧ debugging, in the run widget's list (`select_only`)
/// selecting, else running. "Edit Configurations…" always opens the editor.
pub fn choice(row: &ListRow, select_only: bool, shift: bool) -> Choice {
    match &row.kind {
        RowKind::Config { name, .. } if shift => Choice::Debug(name.clone()),
        RowKind::Config { name, .. } if select_only => Choice::Select(name.clone()),
        RowKind::Config { name, .. } => Choice::Run(name.clone()),
        RowKind::CurrentFile => Choice::CurrentFile {
            debug: shift,
            select_only: select_only && !shift,
        },
        RowKind::EditConfigurations => Choice::EditConfigurations,
    }
}

/// The row a digit picks.
pub fn row_for_digit(rows: &[ListRow], digit: u8) -> Option<usize> {
    rows.iter().position(|r| r.digit == Some(digit))
}

/// What the chooser shows when the typed text leaves no row (`SPEC-0087` v7 behaviour 15).
pub const NOTHING_MATCHES: &str = "No matching configuration";

/// The rows a typed `filter` leaves (`SPEC-0087` v7 behaviour 15): those whose label contains it,
/// ignoring case, in their order — "Current File" and "Edit Configurations…" by their text like
/// the configurations. A heading stays above the first row left of its group and goes with its
/// last row; no row has a digit, because digits are typed into the filter then. An empty filter
/// leaves the rows as they are.
pub fn filtered(rows: &[ListRow], filter: &str) -> Vec<ListRow> {
    if filter.is_empty() {
        return rows.to_vec();
    }
    let mut out = Vec::new();
    // The heading of the group the walk is in, until a row of the group took it.
    let mut heading: Option<String> = None;
    for row in rows {
        if row.heading.is_some() || row.config_name().is_none() {
            heading = row.heading.clone();
        }
        if crate::speed_search::match_range(row.label(), filter).is_none() {
            continue;
        }
        let mut row = row.clone();
        row.heading = heading.take();
        row.digit = None;
        out.push(row);
    }
    out
}

/// What a key does in the chooser while it filters by typing (`SPEC-0087` v7 behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterKey {
    /// The filter becomes this text.
    Filter(String),
    /// The digit picks its row (the filter is empty).
    Digit(u8),
    /// Not a key of the filter.
    Other,
}

/// What typing `ch` does with `filter` typed so far: a digit picks its row while the filter is
/// empty and is part of the name after that; other printable text extends the filter (a space
/// not at its start).
pub fn typed(filter: &str, ch: &str) -> FilterKey {
    let mut chars = ch.chars();
    if let (true, Some(d), None) = (filter.is_empty(), chars.next(), chars.next())
        && let Some(digit) = d.to_digit(10)
    {
        return FilterKey::Digit(digit as u8);
    }
    if crate::speed_search::types_into(filter, ch) {
        FilterKey::Filter(format!("{filter}{ch}"))
    } else {
        FilterKey::Other
    }
}

/// What Escape does: with typed text it clears the filter and the chooser stays open (`true`);
/// without, the chooser closes (`false`).
pub fn escape_clears(filter: &str) -> bool {
    !filter.is_empty()
}

/// An action offered on a row (behaviour 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    /// Edit… in the configuration editor.
    Edit,
    /// Delete, after a confirmation.
    Delete,
    /// Pin to the top.
    Pin,
    /// Unpin.
    Unpin,
    /// Save a temporary configuration to `run.toml`.
    SaveConfiguration,
    /// Copy a detected configuration to `run.toml`.
    CopyToRunToml,
}

impl RowAction {
    /// Its label in the ⋮ menu.
    pub fn label(self) -> &'static str {
        match self {
            RowAction::Edit => "Edit…",
            RowAction::Delete => "Delete",
            RowAction::Pin => "Pin",
            RowAction::Unpin => "Unpin",
            RowAction::SaveConfiguration => "Save Configuration",
            RowAction::CopyToRunToml => "Copy to run.toml",
        }
    }
}

/// The ⋮ menu of a row (behaviour 10); Run and Debug are on every configuration row.
pub fn menu(row: &ListRow) -> Vec<RowAction> {
    let RowKind::Config {
        section, origin, ..
    } = &row.kind
    else {
        return Vec::new();
    };
    let pin = if *section == Section::Pinned {
        RowAction::Unpin
    } else {
        RowAction::Pin
    };
    match origin {
        Section::Detected => vec![RowAction::CopyToRunToml],
        Section::Plugin(_) => Vec::new(),
        Section::Temporary => vec![
            RowAction::Edit,
            RowAction::Delete,
            pin,
            RowAction::SaveConfiguration,
        ],
        Section::Project | Section::Pinned => vec![RowAction::Edit, RowAction::Delete, pin],
    }
}

/// The confirmation before Delete (behaviour 10): headline and detail naming the configuration.
pub fn delete_question(name: &str, temporary: bool) -> (String, String) {
    (
        format!("Delete run configuration \"{name}\"?"),
        if temporary {
            "The temporary configuration is removed from this session.".into()
        } else {
            format!("It is removed from {}.", idecraft_core::RUN_FILE)
        },
    )
}

/// Where the pins of a project are kept: its workspace state, not `run.toml` (behaviour 10).
pub fn pins_file(root: &Path) -> PathBuf {
    root.join(".idecraft").join("run-pins.json")
}

/// The pinned configuration names of the project at `root`.
pub fn load_pins(root: &Path) -> Vec<String> {
    std::fs::read_to_string(pins_file(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Pins `name`, or unpins it when pinned; writes the pins and returns them.
pub fn toggle_pin(root: &Path, name: &str) -> std::io::Result<Vec<String>> {
    let mut pins = load_pins(root);
    if let Some(i) = pins.iter().position(|p| p == name) {
        pins.remove(i);
    } else {
        pins.push(name.to_owned());
    }
    let text = serde_json::to_string_pretty(&pins).map_err(std::io::Error::other)?;
    // The pins are state of one user (`SPEC-0069` behaviour 3a).
    idecraft_core::local_state::refresh_local_state_ignore(root);
    idecraft_core::write_atomic(&pins_file(root), text)?;
    Ok(pins)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str, command: &[&str]) -> RunConfig {
        RunConfig {
            name: name.into(),
            command: command.iter().map(|s| (*s).to_owned()).collect(),
            cwd: None,
            env: Default::default(),
            kind: idecraft_core::RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
        }
    }

    fn summary(rows: &[ListRow]) -> Vec<(Option<u8>, Option<&str>, &str)> {
        rows.iter()
            .map(|r| (r.digit, r.heading.as_deref(), r.label()))
            .collect()
    }

    /// SPEC-0087 v6 AK-7: rows, numbering, the Temporary and Pinned headings and the order.
    #[test]
    fn rows_numbering_headings_and_order() {
        let configs = vec![
            config("app", &["node", "main.js", "two words"]),
            config("tests", &["npm", "test"]),
            config("cargo build", &["cargo", "build"]),
            config("hello.py", &["python3", "hello.py"]),
            config("lint", &["eslint", "."]),
        ];
        let temporary = vec!["hello.py".to_owned()];
        let detected = vec!["cargo build".to_owned()];
        let plugin = vec![("lint".to_owned(), "Lint plugin · ESLint".to_owned())];
        let pins = vec!["tests".to_owned()];
        let mut blockers = HashMap::new();
        blockers.insert("app".to_owned(), "program not found: node".to_owned());
        let rows = rows(&ListInput {
            configs: &configs,
            temporary: &temporary,
            detected: &detected,
            plugin: &plugin,
            pins: &pins,
            blockers: Some(&blockers),
        });
        assert_eq!(
            summary(&rows),
            [
                (Some(1), Some(PINNED), "tests"),
                (Some(2), Some(PROJECT), "app"),
                (Some(3), Some(TEMPORARY), "hello.py"),
                (Some(4), Some(DETECTED), "cargo build"),
                (Some(5), Some("Lint plugin · ESLint"), "lint"),
                (Some(6), None, CURRENT_FILE),
                (Some(0), None, EDIT_CONFIGURATIONS),
            ]
        );
        // The command is shown shell-quoted (behaviour 13), the warning carries the reason.
        assert_eq!(rows[1].command, "node main.js 'two words'");
        assert_eq!(rows[1].warning.as_deref(), Some("program not found: node"));
        assert_eq!(row_for_digit(&rows, 0), Some(6));
        assert_eq!(row_for_digit(&rows, 3), Some(2));
        assert_eq!(row_for_digit(&rows, 9), None);
        // A temporary configuration is never under "Detected from build files" (behaviour 11).
        assert!(matches!(
            &rows[2].kind,
            RowKind::Config {
                section: Section::Temporary,
                ..
            }
        ));
        // Row menus (behaviour 10).
        assert_eq!(
            menu(&rows[0]),
            [RowAction::Edit, RowAction::Delete, RowAction::Unpin]
        );
        assert_eq!(
            menu(&rows[1]),
            [RowAction::Edit, RowAction::Delete, RowAction::Pin]
        );
        assert_eq!(
            menu(&rows[2]),
            [
                RowAction::Edit,
                RowAction::Delete,
                RowAction::Pin,
                RowAction::SaveConfiguration
            ]
        );
        assert_eq!(menu(&rows[3]), [RowAction::CopyToRunToml]);
        assert!(menu(&rows[4]).is_empty() && menu(&rows[5]).is_empty());
        let (q, detail) = delete_question("app", false);
        assert_eq!(q, "Delete run configuration \"app\"?");
        assert_eq!(detail, "It is removed from .idecraft/run.toml.");
    }

    /// Window check of PR #485 (D1): "Edit Configurations…" and digit 0 open the editor from the
    /// run widget's list as from the chooser; the other rows keep their meaning.
    /// SPEC-0327 v2 AK-7: the chooser and the widget list group by folder.
    #[test]
    fn configurations_are_listed_under_their_folder() {
        let in_folder = |name: &str, folder: &str| {
            let mut c = config(name, &["x"]);
            c.folder = Some(folder.into());
            c
        };
        let configs = vec![
            in_folder("api", "Backend"),
            config("lint", &["x"]),
            in_folder("web", "Frontend"),
            in_folder("worker", "Backend"),
            config("tmp.js", &["node", "tmp.js"]),
        ];
        let temporary = vec!["tmp.js".to_owned()];
        let pins = vec!["worker".to_owned()];
        let listed = rows(&ListInput {
            configs: &configs,
            temporary: &temporary,
            pins: &pins,
            ..ListInput::default()
        });
        assert_eq!(
            summary(&listed),
            [
                (Some(1), Some("Pinned"), "worker"),
                (Some(2), Some("Run Configurations"), "lint"),
                (Some(3), Some("Backend"), "api"),
                (Some(4), Some("Frontend"), "web"),
                (Some(5), Some("Temporary"), "tmp.js"),
                (Some(6), None, "Current File"),
                (Some(0), None, "Edit Configurations…"),
            ]
        );
        // Without pins the ones without a folder need no heading.
        let listed = rows(&ListInput {
            configs: &configs[..4],
            ..ListInput::default()
        });
        assert_eq!(
            summary(&listed)[..4],
            [
                (Some(1), None, "lint"),
                (Some(2), Some("Backend"), "api"),
                (Some(3), None, "worker"),
                (Some(4), Some("Frontend"), "web"),
            ]
        );
        // The actions of a row do not depend on its folder.
        assert_eq!(
            menu(&listed[1]),
            [RowAction::Edit, RowAction::Delete, RowAction::Pin]
        );
    }

    #[test]
    fn choosing_a_row_in_the_widget_list_and_the_chooser() {
        let configs = vec![config("app", &["node", "main.js"])];
        let rows = rows(&ListInput {
            configs: &configs,
            ..ListInput::default()
        });
        let edit = &rows[row_for_digit(&rows, 0).unwrap()];
        for select_only in [true, false] {
            for shift in [true, false] {
                assert_eq!(choice(edit, select_only, shift), Choice::EditConfigurations);
            }
        }
        assert_eq!(choice(&rows[0], true, false), Choice::Select("app".into()));
        assert_eq!(choice(&rows[0], false, false), Choice::Run("app".into()));
        assert_eq!(choice(&rows[0], true, true), Choice::Debug("app".into()));
        assert_eq!(
            choice(&rows[1], true, false),
            Choice::CurrentFile {
                debug: false,
                select_only: true
            }
        );
        assert_eq!(
            choice(&rows[1], true, true),
            Choice::CurrentFile {
                debug: true,
                select_only: false
            }
        );
    }

    /// SPEC-0087 AK-10 (v8): the chooser opened for debugging debugs without ⇧ and runs with
    /// it, the current file likewise; the editor entry and the footer follow.
    #[test]
    fn the_chooser_for_debugging_exchanges_run_and_debug() {
        let configs = vec![config("app", &["node", "main.js"])];
        let rows = rows(&ListInput {
            configs: &configs,
            ..ListInput::default()
        });
        let pick = |i: usize, debug_first: bool, shift: bool| {
            choice(&rows[i], false, debug_asked(debug_first, shift))
        };
        // Opened for debugging: Enter debugs, ⇧ runs.
        assert_eq!(pick(0, true, false), Choice::Debug("app".into()));
        assert_eq!(pick(0, true, true), Choice::Run("app".into()));
        // The chooser as it was: Enter runs, ⇧ debugs.
        assert_eq!(pick(0, false, false), Choice::Run("app".into()));
        assert_eq!(pick(0, false, true), Choice::Debug("app".into()));
        for (shift, debug) in [(false, true), (true, false)] {
            assert_eq!(
                pick(1, true, shift),
                Choice::CurrentFile {
                    debug,
                    select_only: false
                }
            );
        }
        let edit = row_for_digit(&rows, 0).unwrap();
        for shift in [true, false] {
            assert_eq!(pick(edit, true, shift), Choice::EditConfigurations);
        }
        assert_eq!(footer(false), FOOTER);
        assert_eq!(footer(true), "Hold ⇧ to run the chosen configuration");
    }

    /// SPEC-0087 v7 AK-9: typing narrows the rows by name, headings stay above what is left of
    /// their group and go when it is empty, the fixed rows are found by their text.
    #[test]
    fn typing_filters_rows_and_keeps_headings_above_them() {
        let in_folder = |name: &str, folder: &str| {
            let mut c = config(name, &["x"]);
            c.folder = Some(folder.into());
            c
        };
        let configs = vec![
            in_folder("api", "Backend"),
            config("lint", &["x"]),
            in_folder("web", "Frontend"),
            in_folder("worker", "Backend"),
            in_folder("Web tests", "Frontend"),
            config("tmp.js", &["node", "tmp.js"]),
        ];
        let temporary = vec!["tmp.js".to_owned()];
        let pins = vec!["lint".to_owned()];
        let all = rows(&ListInput {
            configs: &configs,
            temporary: &temporary,
            pins: &pins,
            ..ListInput::default()
        });
        let shown = |filter: &str| {
            filtered(&all, filter)
                .iter()
                .map(|r| (r.heading.clone(), r.label().to_owned()))
                .collect::<Vec<_>>()
        };
        let row =
            |heading: Option<&str>, label: &str| (heading.map(str::to_owned), label.to_owned());
        // An empty filter changes nothing, digits included.
        assert_eq!(filtered(&all, ""), all);
        // Case is ignored; the heading moves to the first row left of its folder.
        assert_eq!(
            shown("W"),
            [
                row(Some("Backend"), "worker"),
                row(Some("Frontend"), "web"),
                row(None, "Web tests"),
            ]
        );
        assert_eq!(shown("work"), [row(Some("Backend"), "worker")]);
        // A group without a row left loses its heading; "Pinned" and "Temporary" behave alike.
        assert_eq!(
            shown("t"),
            [
                row(Some("Pinned"), "lint"),
                row(Some("Frontend"), "Web tests"),
                row(Some("Temporary"), "tmp.js"),
                row(None, "Current File"),
                row(None, "Edit Configurations…"),
            ]
        );
        // The fixed rows are found by their text and never take a heading from above.
        assert_eq!(shown("current"), [row(None, "Current File")]);
        assert_eq!(shown("edit c"), [row(None, "Edit Configurations…")]);
        assert!(shown("zzz").is_empty());
        // No digits while a filter is typed.
        assert!(filtered(&all, "w").iter().all(|r| r.digit.is_none()));
        // Choosing a filtered row means what it meant before.
        assert_eq!(
            choice(&filtered(&all, "work")[0], false, false),
            Choice::Run("worker".into())
        );
    }

    /// SPEC-0087 v7 AK-9: digits pick rows only while nothing is typed; Backspace and Escape.
    #[test]
    fn keys_of_the_filter() {
        assert_eq!(typed("", "3"), FilterKey::Digit(3));
        assert_eq!(typed("", "0"), FilterKey::Digit(0));
        assert_eq!(typed("test", "2"), FilterKey::Filter("test2".into()));
        assert_eq!(typed("", "a"), FilterKey::Filter("a".into()));
        assert_eq!(typed("a", " "), FilterKey::Filter("a ".into()));
        assert_eq!(typed("", " "), FilterKey::Other);
        assert_eq!(typed("a", "\u{7f}"), FilterKey::Other);
        assert_eq!(typed("a", ""), FilterKey::Other);
        assert!(escape_clears("a"));
        assert!(!escape_clears(""));
    }

    #[test]
    fn only_nine_rows_get_digits() {
        let configs: Vec<RunConfig> = (0..12).map(|i| config(&format!("c{i}"), &["x"])).collect();
        let rows = rows(&ListInput {
            configs: &configs,
            ..ListInput::default()
        });
        assert_eq!(rows.len(), 14);
        assert_eq!(rows[8].digit, Some(9));
        // Without pins the configurations of run.toml need no heading.
        assert_eq!(rows[0].heading, None);
        assert_eq!(rows[9].digit, None);
        assert_eq!(rows[12].label(), CURRENT_FILE);
        assert_eq!(rows[12].digit, None);
        assert_eq!(rows[13].digit, Some(0));
    }

    #[test]
    fn pins_live_in_the_workspace_state() {
        let root = std::env::temp_dir().join(format!("idecraft-pins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(load_pins(&root).is_empty());
        assert_eq!(toggle_pin(&root, "a").unwrap(), ["a"]);
        assert_eq!(toggle_pin(&root, "b").unwrap(), ["a", "b"]);
        assert_eq!(toggle_pin(&root, "a").unwrap(), ["b"]);
        assert_eq!(load_pins(&root), ["b"]);
        assert!(!root.join(".idecraft/run.toml").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

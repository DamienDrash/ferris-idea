//! Exchange files of IDECraft (`SPEC-0303`): reading `.idcrun`, `.idckeymap`, `.idctheme`,
//! `.idcsources` and `.idcsettings` files into a preview of what they contain and change, applying
//! a preview after the user confirmed it, exporting the current state, and the Share link.
//!
//! The envelope, problems, shapes and migrations are [`idecraft_core::format`]; this crate knows
//! the targets of each kind (run configurations, the user keymap, the IDE settings, connection
//! profiles), which is why it sits above `core`, `ui` and `database`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use idecraft_core::format::{self, Envelope, Problem, Use};
use idecraft_core::{IdeSettings, RunConfig};
use idecraft_database::ConnectionProfile;
use idecraft_ui::keymap::{Keymap, KeymapFile, Shortcut};
use idecraft_ui::theme::ThemeId;
use serde::Serialize;

/// Where an import writes and an export reads.
#[derive(Debug, Clone, Default)]
pub struct Targets {
    /// The workspace (run configurations, connections).
    pub root: PathBuf,
    /// The user keymap (`keymaps/IDECraft.toml`).
    pub keymap_file: Option<PathBuf>,
    /// The IDE settings (`settings.toml`).
    pub settings_file: Option<PathBuf>,
    /// The theme file of older versions, read while the settings name no theme.
    pub legacy_theme_file: Option<PathBuf>,
}

impl Targets {
    /// The user's own targets for workspace `root`.
    pub fn user(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            keymap_file: idecraft_core::keymap_file(),
            settings_file: idecraft_core::ide_settings_file(),
            legacy_theme_file: idecraft_core::theme_file(),
        }
    }

    fn settings(&self) -> IdeSettings {
        self.settings_file
            .as_deref()
            .map(idecraft_core::load_ide_settings)
            .unwrap_or_default()
    }

    fn keymap(&self) -> KeymapFile {
        KeymapFile::read_user(self.keymap_file.as_deref())
    }

    fn theme(&self) -> String {
        let chosen = self
            .settings_file
            .as_deref()
            .and_then(|f| idecraft_core::chosen_theme(f, self.legacy_theme_file.as_deref()));
        ThemeId::from_saved(chosen.as_deref()).name().to_owned()
    }
}

/// What a change does to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Something new.
    Add,
    /// Something existing gets another value.
    Replace,
    /// Already as the file says.
    Unchanged,
}

/// One change an import makes (behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// The file changed, as shown (`.idecraft/run.toml`, `IDE settings`, `user keymap`).
    pub target: String,
    /// Add, replace or unchanged.
    pub action: Action,
    /// What is changed: a configuration, an action, a key path.
    pub item: String,
    /// The value before, for a replacement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    /// The value after.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

impl Change {
    /// One line for the preview.
    pub fn line(&self) -> String {
        let what = match self.action {
            Action::Add => "add",
            Action::Replace => "replace",
            Action::Unchanged => "unchanged",
        };
        let mut s = format!("{what} {} in {}", self.item, self.target);
        match (&self.old, &self.new) {
            (Some(o), Some(n)) => s.push_str(&format!(": {o} → {n}")),
            (None, Some(n)) => s.push_str(&format!(": {n}")),
            _ => {}
        }
        s
    }
}

#[derive(Debug, Clone)]
enum Plan {
    Run(RunConfig),
    Keymap(BTreeMap<String, Vec<Shortcut>>),
    Theme {
        name: String,
        compact: Option<bool>,
    },
    Sources(Vec<ConnectionProfile>),
    Settings {
        settings: IdeSettings,
        keymap: Option<BTreeMap<String, Vec<Shortcut>>>,
    },
}

/// What an exchange file contains and would change (behaviour 11). Nothing is written until
/// [`apply`].
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    /// The file's name.
    pub file: String,
    /// Its kind, when the envelope could be read.
    pub kind: Option<String>,
    /// The kind in words.
    pub title: Option<String>,
    /// Format after migration.
    pub format: Option<u32>,
    /// The format the file had when it was older.
    pub migrated_from: Option<u32>,
    /// The envelope (name, author, licence, homepage).
    pub envelope: Option<Envelope>,
    /// The contents as lines.
    pub contents: Vec<String>,
    /// The changes an import makes.
    pub changes: Vec<Change>,
    /// Every problem; with any, nothing can be imported.
    pub problems: Vec<Problem>,
    /// The file's text, for Share.
    #[serde(skip)]
    pub text: String,
    #[serde(skip)]
    plan: Option<Plan>,
}

impl Preview {
    fn failed(file: &str, text: String, problems: Vec<Problem>) -> Self {
        Self {
            file: file.into(),
            kind: None,
            title: None,
            format: None,
            migrated_from: None,
            envelope: None,
            contents: Vec::new(),
            changes: Vec::new(),
            problems,
            text,
            plan: None,
        }
    }

    /// Whether the import may be confirmed.
    pub fn can_import(&self) -> bool {
        self.problems.is_empty() && self.plan.is_some()
    }

    /// The heading: kind in words and the envelope's name or the file name.
    pub fn heading(&self) -> String {
        let name = self
            .envelope
            .as_ref()
            .and_then(|e| e.name.clone())
            .unwrap_or_else(|| self.file.clone());
        match &self.title {
            Some(t) => format!("Import {t}: {name}"),
            None => format!("Import {name}"),
        }
    }

    /// The preview as text lines (`idecraft open --print`, the window).
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![self.heading()];
        if let Some(k) = &self.kind {
            let mut l = format!("kind {k}, format {}", self.format.unwrap_or_default());
            if let Some(f) = self.migrated_from {
                l.push_str(&format!(" (migrated from format {f})"));
            }
            out.push(l);
        }
        if let Some(e) = &self.envelope {
            for (label, v) in [
                ("author", &e.author),
                ("licence", &e.licence),
                ("homepage", &e.homepage),
            ] {
                if let Some(v) = v {
                    out.push(format!("{label}: {v}"));
                }
            }
        }
        if !self.contents.is_empty() {
            out.push("Contents:".into());
            out.extend(self.contents.iter().map(|c| format!("  {c}")));
        }
        if !self.changes.is_empty() {
            out.push("Changes:".into());
            out.extend(self.changes.iter().map(|c| format!("  {}", c.line())));
        }
        if !self.problems.is_empty() {
            out.push("Problems:".into());
            out.extend(self.problems.iter().map(|p| format!("  {p}")));
        }
        out
    }
}

/// Reads the exchange file at `path` into a preview.
pub fn preview_file(path: &Path, targets: &Targets) -> Preview {
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match std::fs::metadata(path) {
        Ok(m) if m.len() > format::MAX_EXCHANGE_BYTES as u64 => {
            return Preview::failed(
                &file,
                String::new(),
                vec![Problem::file(format!(
                    "larger than 64 KiB ({} bytes); not read",
                    m.len()
                ))],
            );
        }
        Err(e) => return Preview::failed(&file, String::new(), vec![Problem::file(e.to_string())]),
        Ok(_) => {}
    }
    if format::kind_for_path(path).is_some_and(|k| k.zip) {
        return Preview::failed(
            &file,
            String::new(),
            vec![Problem::file(
                "importing `plugin` files is not available in this version",
            )],
        );
    }
    match std::fs::read_to_string(path) {
        Ok(text) => preview_text(&text, &file, targets),
        Err(e) => Preview::failed(&file, String::new(), vec![Problem::file(e.to_string())]),
    }
}

/// Reads `text` (the file `file`) into a preview.
pub fn preview_text(text: &str, file: &str, targets: &Targets) -> Preview {
    let expected = format::kind_for_path(Path::new(file)).map(|k| k.id);
    let doc = match format::read(text, expected, Use::Exchange) {
        Ok(d) => d,
        Err(problems) => return Preview::failed(file, text.into(), private_keys(problems)),
    };
    let kind = format::find_kind(&doc.envelope.kind).expect("read checks the kind");
    let mut preview = Preview {
        file: file.into(),
        kind: Some(kind.id.into()),
        title: Some(kind.title.into()),
        format: Some(doc.envelope.format),
        migrated_from: doc.migrated_from,
        envelope: Some(doc.envelope.clone()),
        contents: Vec::new(),
        changes: Vec::new(),
        problems: Vec::new(),
        text: text.into(),
        plan: None,
    };
    if kind.extension.is_none() {
        preview.problems.push(Problem::new(
            "idecraft.kind",
            format!("`{}` is a project file, not an exchange file", kind.id),
        ));
        return preview;
    }
    if !kind.importable {
        let message = if kind.id == "connector" {
            // Connector definitions have their own review and import (SPEC-0302).
            "connector definitions are reviewed and imported in the New Data Source dialog or with the MCP tool `import_connector`".to_owned()
        } else {
            format!(
                "importing `{}` files is not available in this version",
                kind.id
            )
        };
        preview
            .problems
            .push(Problem::new("idecraft.kind", message));
        return preview;
    }
    let result = match kind.id {
        "run" => plan_run(&doc.body, targets, &mut preview),
        "keymap" => plan_keymap(&doc.body, targets, &mut preview),
        "theme" => plan_theme(&doc.body, targets, &mut preview),
        "sources" => plan_sources(&doc.body, targets, &mut preview),
        "settings" => plan_settings(&doc.body, targets, &mut preview),
        _ => Err(vec![Problem::file("unexpected kind")]),
    };
    match result {
        Ok(plan) if preview.problems.is_empty() => preview.plan = Some(plan),
        Ok(_) => {}
        Err(p) => preview.problems.extend(p),
    }
    preview
}

/// Gives the private settings sections their reason instead of "unknown key" (behaviour 5).
fn private_keys(problems: Vec<Problem>) -> Vec<Problem> {
    problems
        .into_iter()
        .map(|mut p| {
            if p.message == "unknown key"
                && ["settings.ai", "settings.mcp", "settings.terminal", "settings.database", "settings.git"].contains(&p.path.as_str())
            {
                p.message = "not shared in settings files (an endpoint, a server switch, a program to run or a security exception); set it in Settings".into();
            }
            if p.message == "unknown key" && p.path.ends_with(".secret") {
                p.message = "a stored-password reference is never imported".into();
            }
            if p.message == "unknown key" && p.path.ends_with(".ai") {
                p.message = "agent rights are never imported; new connections get read-only rights".into();
            }
            p
        })
        .collect()
}

/// Whether `name` looks like a secret (behaviour 5).
pub fn secret_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    [
        "PASSWORD",
        "PASSWD",
        "SECRET",
        "TOKEN",
        "API_KEY",
        "APIKEY",
        "PRIVATE_KEY",
        "CREDENTIAL",
    ]
    .iter()
    .any(|s| n.contains(s))
}

fn relative_inside(p: &str) -> bool {
    let path = Path::new(p);
    !path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn body_part<T: serde::de::DeserializeOwned>(
    body: &toml::Table,
    key: &str,
) -> Result<T, Vec<Problem>> {
    body.get(key)
        .cloned()
        .ok_or_else(|| vec![Problem::new(key, "missing")])?
        .try_into()
        .map_err(|e: toml::de::Error| vec![Problem::new(key, e.message().to_owned())])
}

const RUN_TARGET: &str = idecraft_core::RUN_FILE;

/// Checks of a run configuration besides its shape (behaviour 10).
fn check_run(c: &RunConfig, problems: &mut Vec<Problem>) {
    if c.name.trim().is_empty() {
        problems.push(Problem::new("run.name", "empty"));
    }
    if c.command.is_empty() && c.url.is_none() {
        problems.push(Problem::new("run.command", "missing (or give `url`)"));
    }
    for (key, v) in [("cwd", &c.cwd), ("report_file", &c.report_file)] {
        if let Some(v) = v
            && !relative_inside(v)
        {
            problems.push(Problem::new(
                format!("run.{key}"),
                "must be relative to the workspace, without `..`",
            ));
        }
    }
    if let Some(u) = &c.url
        && !(u.starts_with("http://") || u.starts_with("https://"))
    {
        problems.push(Problem::new(
            "run.url",
            "must start with http:// or https://",
        ));
    }
    for k in c.env.keys().filter(|k| secret_name(k)) {
        problems.push(Problem::new(
            format!("run.env.{k}"),
            "looks like a secret; shared files never carry secrets",
        ));
    }
    // Before-launch tasks start programs and open pages (SPEC-0047 v3 behaviours 8 and 9); an
    // exchange file does not carry them, so nothing runs that the preview does not name.
    if !c.before_launch.is_empty() {
        problems.push(Problem::new(
            "run.before_launch",
            "not part of an exchange file; add the tasks in Edit Configurations after the import",
        ));
    }
}

fn plan_run(body: &toml::Table, targets: &Targets, p: &mut Preview) -> Result<Plan, Vec<Problem>> {
    let mut c: RunConfig = body_part(body, "run")?;
    check_run(&c, &mut p.problems);
    p.contents.push(format!("configuration `{}`", c.name));
    if !c.command.is_empty() {
        p.contents.push(format!("command: {}", c.command.join(" ")));
    }
    if let Some(u) = &c.url {
        p.contents.push(format!("opens {u}"));
    }
    if let Some(d) = &c.cwd {
        p.contents.push(format!("folder: {d}"));
    }
    for (k, v) in &c.env {
        p.contents.push(format!("env {k}={v}"));
    }
    if !c.before.is_empty() {
        p.contents
            .push(format!("runs first: {}", c.before.join(", ")));
    }
    let existing = idecraft_core::load_run_configs(&targets.root);
    let taken = |n: &str| existing.configs.iter().any(|e| e.name == n);
    let wanted = c.name.clone();
    if taken(&wanted) {
        let mut n = 2;
        while taken(&format!("{wanted} ({n})")) {
            n += 1;
        }
        c.name = format!("{wanted} ({n})");
    }
    let mut change = Change {
        target: RUN_TARGET.into(),
        action: Action::Add,
        item: format!("run configuration `{}`", c.name),
        old: None,
        new: None,
    };
    if c.name != wanted {
        change.new = Some(format!("`{wanted}` exists, imported as `{}`", c.name));
    }
    p.changes.push(change);
    Ok(Plan::Run(c))
}

/// The shortcuts of a keymap table, with problems at `prefix.actions.<id>[i]`.
fn keymap_actions(
    table: &toml::Value,
    prefix: &str,
    problems: &mut Vec<Problem>,
) -> BTreeMap<String, Vec<Shortcut>> {
    let mut out = BTreeMap::new();
    let Some(actions) = table.get("actions").and_then(toml::Value::as_table) else {
        return out;
    };
    for (id, list) in actions {
        let mut shortcuts = Vec::new();
        for (i, s) in list.as_array().into_iter().flatten().enumerate() {
            match s.as_str().map(Shortcut::from_entry) {
                Some(Ok(sc)) => shortcuts.push(sc),
                Some(Err(e)) => problems.push(Problem::new(
                    format!("{prefix}.actions.{id}[{i}]"),
                    e.to_string(),
                )),
                None => {}
            }
        }
        out.insert(id.clone(), shortcuts);
    }
    out
}

fn shortcut_text(list: &[Shortcut], macos: bool) -> String {
    if list.is_empty() {
        "none".into()
    } else {
        list.iter()
            .map(|s| s.display(macos))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn keymap_changes(actions: &BTreeMap<String, Vec<Shortcut>>, targets: &Targets, p: &mut Preview) {
    let current = Keymap::bundled_for_platform().with_overrides(&targets.keymap());
    for (id, list) in actions {
        let old = current.shortcuts(id);
        let new = shortcut_text(list, current.macos);
        p.contents.push(format!("{id}: {new}"));
        let unchanged = old == list.as_slice();
        p.changes.push(Change {
            target: "user keymap".into(),
            action: if unchanged {
                Action::Unchanged
            } else {
                Action::Replace
            },
            item: format!("shortcuts of {id}"),
            old: (!unchanged).then(|| shortcut_text(old, current.macos)),
            new: (!unchanged).then_some(new),
        });
    }
}

fn plan_keymap(
    body: &toml::Table,
    targets: &Targets,
    p: &mut Preview,
) -> Result<Plan, Vec<Problem>> {
    let table = body
        .get("keymap")
        .cloned()
        .unwrap_or(toml::Value::Table(toml::Table::new()));
    let actions = keymap_actions(&table, "keymap", &mut p.problems);
    if let Some(name) = table.get("name").and_then(toml::Value::as_str) {
        p.contents.push(format!("keymap `{name}`"));
    }
    keymap_changes(&actions, targets, p);
    Ok(Plan::Keymap(actions))
}

fn plan_theme(
    body: &toml::Table,
    targets: &Targets,
    p: &mut Preview,
) -> Result<Plan, Vec<Problem>> {
    let t = &body["theme"];
    let name = t["name"].as_str().unwrap_or_default();
    let compact = t.get("compact_mode").and_then(toml::Value::as_bool);
    let Some(theme) = ThemeId::from_name(name) else {
        let known: Vec<&str> = ThemeId::ALL.iter().map(|t| t.name()).collect();
        return Err(vec![Problem::new(
            "theme.name",
            format!("unknown theme `{name}` (one of {})", known.join(", ")),
        )]);
    };
    p.contents.push(format!("theme {}", theme.name()));
    let old = targets.theme();
    p.changes.push(Change {
        target: "IDE settings".into(),
        action: if old == theme.name() {
            Action::Unchanged
        } else {
            Action::Replace
        },
        item: "appearance.theme".into(),
        old: (old != theme.name()).then(|| old.clone()),
        new: (old != theme.name()).then(|| theme.name().into()),
    });
    if let Some(c) = compact {
        p.contents
            .push(format!("compact mode {}", if c { "on" } else { "off" }));
        let was = targets.settings().appearance.compact_mode;
        p.changes.push(Change {
            target: "IDE settings".into(),
            action: if was == c {
                Action::Unchanged
            } else {
                Action::Replace
            },
            item: "appearance.compact_mode".into(),
            old: (was != c).then(|| was.to_string()),
            new: (was != c).then(|| c.to_string()),
        });
    }
    Ok(Plan::Theme {
        name: theme.name().into(),
        compact,
    })
}

fn plan_sources(
    body: &toml::Table,
    targets: &Targets,
    p: &mut Preview,
) -> Result<Plan, Vec<Problem>> {
    let list: Vec<ConnectionProfile> = body_part(body, "connection")?;
    if let Err(e) = idecraft_database::validate_all(&list) {
        p.problems.push(Problem::new("connection", e.to_string()));
    }
    for (i, c) in list.iter().enumerate() {
        if let Some(path) = &c.path
            && !relative_inside(&path.to_string_lossy())
        {
            p.problems.push(Problem::new(
                format!("connection[{i}].path"),
                "must be relative to the workspace, without `..`",
            ));
        }
    }
    let existing = idecraft_database::load_profiles(&targets.root)
        .map_err(|e| vec![Problem::new(idecraft_database::PROFILE_FILE, e.to_string())])?;
    let mut out = Vec::new();
    for c in list {
        let url = c.to_url();
        p.contents.push(format!("{} ({}): {url}", c.name, c.id));
        let mut c = c;
        let action = match existing.iter().find(|e| e.id == c.id) {
            None => Action::Add,
            Some(e) => {
                c.secret = e.secret.clone();
                c.ai = e.ai.clone();
                if *e == c {
                    Action::Unchanged
                } else {
                    Action::Replace
                }
            }
        };
        let old = existing
            .iter()
            .find(|e| e.id == c.id && action == Action::Replace)
            .map(ConnectionProfile::to_url);
        p.changes.push(Change {
            target: idecraft_database::PROFILE_FILE.into(),
            action,
            item: format!("connection `{}`", c.id),
            old,
            new: (action == Action::Replace).then(|| url.clone()),
        });
        out.push(c);
    }
    Ok(Plan::Sources(out))
}

/// The settings tables shared in settings files (behaviour 9).
pub const SHARED_SETTINGS: [&str; 4] = ["editor", "appearance", "vcs", "code_style"];

fn flatten(prefix: &str, v: &toml::Value, out: &mut BTreeMap<String, String>) {
    match v {
        toml::Value::Table(t) => {
            for (k, v) in t {
                flatten(&format!("{prefix}.{k}"), v, out);
            }
        }
        other => {
            out.insert(prefix.to_owned(), other.to_string());
        }
    }
}

fn settings_value(s: &IdeSettings) -> toml::Table {
    match toml::Value::try_from(s) {
        Ok(toml::Value::Table(t)) => t,
        _ => toml::Table::new(),
    }
}

fn plan_settings(
    body: &toml::Table,
    targets: &Targets,
    p: &mut Preview,
) -> Result<Plan, Vec<Problem>> {
    let current = targets.settings();
    let mut merged = settings_value(&current);
    let given = body["settings"].as_table().cloned().unwrap_or_default();
    for (section, value) in &given {
        let Some(t) = value.as_table() else { continue };
        let entry = merged
            .entry(section.clone())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let Some(target) = entry.as_table_mut() {
            for (k, v) in t {
                target.insert(k.clone(), v.clone());
            }
        }
    }
    let settings: IdeSettings = toml::Value::Table(merged)
        .try_into()
        .map(IdeSettings::clamped)
        .map_err(|e: toml::de::Error| vec![Problem::new("settings", e.message().to_owned())])?;
    if let Some(theme) = &settings.appearance.theme
        && ThemeId::from_name(theme).is_none()
    {
        p.problems.push(Problem::new(
            "settings.appearance.theme",
            format!("unknown theme `{theme}`"),
        ));
    }
    let (mut before, mut after) = (BTreeMap::new(), BTreeMap::new());
    let (old_t, new_t) = (settings_value(&current), settings_value(&settings));
    for section in SHARED_SETTINGS {
        if let Some(v) = old_t.get(section) {
            flatten(section, v, &mut before);
        }
        if let Some(v) = new_t.get(section) {
            flatten(section, v, &mut after);
        }
    }
    let mut given_keys = BTreeMap::new();
    for (section, v) in &given {
        flatten(section, v, &mut given_keys);
    }
    for (key, value) in &given_keys {
        p.contents.push(format!("{key} = {value}"));
        let new = after.get(key).cloned().unwrap_or_else(|| value.clone());
        let old = before.get(key).cloned();
        let action = match &old {
            None => Action::Add,
            Some(o) if *o == new => Action::Unchanged,
            Some(_) => Action::Replace,
        };
        p.changes.push(Change {
            target: "IDE settings".into(),
            action,
            item: key.clone(),
            old: (action == Action::Replace).then(|| old.clone().unwrap_or_default()),
            new: (action != Action::Unchanged).then_some(new),
        });
    }
    let keymap = body.get("keymap").map(|t| {
        let actions = keymap_actions(t, "keymap", &mut p.problems);
        keymap_changes(&actions, targets, p);
        actions
    });
    Ok(Plan::Settings { settings, keymap })
}

/// Applies a preview without problems (behaviour 12); returns the files written.
pub fn apply(preview: &Preview, targets: &Targets) -> Result<Vec<String>, String> {
    if !preview.problems.is_empty() {
        return Err(format!(
            "{} has problems; nothing was imported: {}",
            preview.file,
            format::problems_text(&preview.problems)
        ));
    }
    let plan = preview
        .plan
        .as_ref()
        .ok_or_else(|| format!("{}: nothing to import", preview.file))?;
    let io = |e: std::io::Error| e.to_string();
    match plan {
        Plan::Run(c) => {
            idecraft_core::append_run_config(&targets.root, c).map_err(io)?;
            Ok(vec![RUN_TARGET.into()])
        }
        Plan::Keymap(actions) => {
            write_keymap(actions, targets)?;
            Ok(vec!["user keymap".into()])
        }
        Plan::Theme { name, compact } => {
            let file = targets.settings_file.as_ref().ok_or("no settings folder")?;
            let mut s = targets.settings();
            s.appearance.theme = Some(name.clone());
            if let Some(c) = compact {
                s.appearance.compact_mode = *c;
            }
            idecraft_core::save_ide_settings(file, &s).map_err(io)?;
            if let Some(old) = &targets.legacy_theme_file {
                format::retire(old).map_err(io)?;
            }
            Ok(vec!["IDE settings".into()])
        }
        Plan::Sources(list) => {
            let mut profiles =
                idecraft_database::load_profiles(&targets.root).map_err(|e| e.to_string())?;
            for c in list {
                match profiles.iter_mut().find(|p| p.id == c.id) {
                    Some(p) => *p = c.clone(),
                    None => profiles.push(c.clone()),
                }
            }
            idecraft_database::save_profiles(&targets.root, &profiles)
                .map_err(|e| e.to_string())?;
            Ok(vec![idecraft_database::PROFILE_FILE.into()])
        }
        Plan::Settings { settings, keymap } => {
            let file = targets.settings_file.as_ref().ok_or("no settings folder")?;
            // Whether agents may operate the window is never taken from a file: only the user
            // switches it on in Settings (SPEC-0319 behaviour 32).
            let mut settings = settings.clone();
            settings.ui = targets.settings().ui;
            idecraft_core::save_ide_settings(file, &settings).map_err(io)?;
            if let Some(old) = &targets.legacy_theme_file
                && settings.appearance.theme.is_some()
            {
                format::retire(old).map_err(io)?;
            }
            let mut written = vec!["IDE settings".to_owned()];
            if let Some(actions) = keymap {
                write_keymap(actions, targets)?;
                written.push("user keymap".into());
            }
            Ok(written)
        }
    }
}

fn write_keymap(
    actions: &BTreeMap<String, Vec<Shortcut>>,
    targets: &Targets,
) -> Result<(), String> {
    let file = targets.keymap_file.as_ref().ok_or("no settings folder")?;
    let mut user = targets.keymap();
    for (id, list) in actions {
        user.set_action(id, Some(list.clone()));
    }
    user.write(file).map_err(|e| e.to_string())
}

/// What to export.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// The run configuration's name.
    pub name: Option<String>,
    /// Connection ids; none: all.
    pub ids: Option<Vec<String>>,
}

/// An exported file (behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Export {
    /// The file's text.
    pub text: String,
    /// Suggested file name.
    pub file_name: String,
    /// What was left out and why (behaviour 5).
    pub left_out: Vec<String>,
}

/// Exports `kind` from the targets.
pub fn export(kind: &str, selection: &Selection, targets: &Targets) -> Result<Export, String> {
    let k = format::find_kind(kind)
        .filter(|k| k.extension.is_some())
        .ok_or_else(|| format!("unknown exchange kind `{kind}`"))?;
    if !k.importable {
        return Err(format!(
            "exporting `{kind}` files is not available in this version"
        ));
    }
    let mut left_out = Vec::new();
    let (name, body): (String, toml::Table) = match kind {
        "run" => {
            let wanted = selection
                .name
                .as_deref()
                .ok_or("give the run configuration's `name`")?;
            let configs = idecraft_core::load_run_configs(&targets.root);
            let mut c = configs
                .configs
                .into_iter()
                .find(|c| c.name == wanted)
                .ok_or_else(|| format!("no run configuration `{wanted}` in {RUN_TARGET}"))?;
            c.env.retain(|k, _| {
                let secret = secret_name(k);
                if secret {
                    left_out.push(format!("run.env.{k}: looks like a secret"));
                }
                !secret
            });
            for (key, v) in [("cwd", &mut c.cwd), ("report_file", &mut c.report_file)] {
                if v.as_deref().is_some_and(|p| !relative_inside(p)) {
                    left_out.push(format!("run.{key}: not a path inside the workspace"));
                    *v = None;
                }
            }
            // Where it is listed, how many instances it allows and its before-launch tasks are
            // the project's own arrangement and stay out of the exchange file (SPEC-0047 v3).
            if c.folder.take().is_some() {
                left_out.push("run.folder: the folder stays with the project".into());
            }
            if std::mem::take(&mut c.allow_multiple) {
                left_out.push("run.allow_multiple: stays with the project".into());
            }
            if !std::mem::take(&mut c.before_launch).is_empty() {
                left_out.push(
                    "run.before_launch: before-launch tasks are not part of an exchange file"
                        .into(),
                );
            }
            let mut t = toml::Table::new();
            t.insert("run".into(), to_value(&c)?);
            (c.name, t)
        }
        "keymap" => {
            let user = targets.keymap();
            let mut t = toml::Table::new();
            t.insert("keymap".into(), toml::Value::Table(user.to_toml()));
            (user.name.clone(), t)
        }
        "theme" => {
            let theme = targets.theme();
            let s = targets.settings();
            let mut inner = toml::Table::new();
            inner.insert("name".into(), theme.clone().into());
            inner.insert("compact_mode".into(), s.appearance.compact_mode.into());
            let mut t = toml::Table::new();
            t.insert("theme".into(), toml::Value::Table(inner));
            (theme, t)
        }
        "sources" => {
            let profiles =
                idecraft_database::load_profiles(&targets.root).map_err(|e| e.to_string())?;
            let chosen: Vec<ConnectionProfile> = match &selection.ids {
                Some(ids) => ids
                    .iter()
                    .map(|id| {
                        profiles
                            .iter()
                            .find(|p| &p.id == id)
                            .cloned()
                            .ok_or_else(|| format!("no connection `{id}`"))
                    })
                    .collect::<Result<_, _>>()?,
                None => profiles,
            };
            if chosen.is_empty() {
                return Err("no connections to export".into());
            }
            let mut list = Vec::new();
            for p in &chosen {
                let mut v = match to_value(p)? {
                    toml::Value::Table(t) => t,
                    _ => toml::Table::new(),
                };
                if v.remove("secret").is_some() {
                    left_out.push(format!("{}: stored-password reference", p.id));
                }
                if v.remove("ai").is_some() {
                    left_out.push(format!("{}: agent rights", p.id));
                }
                if let Some(path) = v.get("path").and_then(toml::Value::as_str)
                    && !relative_inside(path)
                {
                    v.remove("path");
                    left_out.push(format!("{}: database file outside the workspace", p.id));
                }
                list.push(toml::Value::Table(v));
            }
            let mut t = toml::Table::new();
            t.insert("connection".into(), toml::Value::Array(list));
            let name = match chosen.as_slice() {
                [one] => one.name.clone(),
                _ => "data-sources".into(),
            };
            (name, t)
        }
        "settings" => {
            let s = targets.settings();
            let mut all = settings_value(&s);
            let mut shared = toml::Table::new();
            for section in SHARED_SETTINGS {
                if let Some(v) = all.remove(section) {
                    shared.insert(section.into(), v);
                }
            }
            if let Some(toml::Value::Table(a)) = shared.get_mut("appearance") {
                a.insert("theme".into(), targets.theme().into());
            }
            for private in all.keys() {
                left_out.push(format!("settings.{private}: not shared in settings files"));
            }
            let mut t = toml::Table::new();
            t.insert("settings".into(), toml::Value::Table(shared));
            let user = targets.keymap();
            if !user.actions.is_empty() {
                t.insert("keymap".into(), toml::Value::Table(user.to_toml()));
            }
            ("settings".into(), t)
        }
        _ => unreachable!("importable kinds"),
    };
    let text = format::write(&Envelope::current(kind).named(name.clone()), &body)?;
    debug_assert!(
        format::read(&text, Some(kind), Use::Exchange).is_ok(),
        "{text}"
    );
    Ok(Export {
        file_name: k.file_name(&name),
        text,
        left_out,
    })
}

fn to_value(v: &impl Serialize) -> Result<toml::Value, String> {
    toml::Value::try_from(v).map_err(|e| e.to_string())
}

/// Issue page of the IDECraft repository (behaviour 18).
pub const ISSUES_URL: &str = "https://github.com/DamienDrash/idecraft/issues/new";
/// Issue form for shared files.
pub const SHARE_TEMPLATE: &str = "shared-file.yml";
/// Longest Share URL; longer contents are left for the user to attach.
pub const MAX_SHARE_URL: usize = 8000;

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The prefilled issue URL for sharing a file of `kind` named `name` with `text` (behaviour 18).
/// The second value says whether the text fit into the URL.
pub fn share_url(kind: &str, name: &str, text: &str) -> (String, bool) {
    let base = format!(
        "{ISSUES_URL}?template={SHARE_TEMPLATE}&title={}&kind={}",
        encode(&format!("{kind}: {name}")),
        encode(kind)
    );
    let with = format!("{base}&content={}", encode(text));
    if with.len() <= MAX_SHARE_URL {
        (with, true)
    } else {
        let note = encode("The file is too large for a link; please attach it to this issue.");
        (format!("{base}&notes={note}"), false)
    }
}

#[cfg(test)]
mod tests;

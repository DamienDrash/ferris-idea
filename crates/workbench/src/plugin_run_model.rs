//! Run configurations suggested by plugins in the Run dialog (`SPEC-0276` behaviours 4, 5 and 8):
//! the chooser's entries and the confirmation shown before a suggestion first starts.

use std::path::Path;

use ferris_idea_plugins::{RunAllowance, RunAllowances, RunType, SuggestedCommand};

/// One suggestion as the Run dialog knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRunEntry {
    /// The run configuration name, `<plugin name>: <name>`.
    pub config_name: String,
    /// The plugin's name.
    pub plugin_name: String,
    /// The type's title.
    pub type_title: String,
    /// The suggested command.
    pub command: SuggestedCommand,
    /// What "always allow" stores for it.
    pub allowance: RunAllowance,
    /// Whether the user always allows exactly this command in this workspace.
    pub always_allowed: bool,
}

impl PluginRunEntry {
    /// The chooser's heading above this entry's type (behaviour 4).
    pub fn heading(&self) -> String {
        format!("From plugin: {} › {}", self.plugin_name, self.type_title)
    }
}

/// Why the configuration `selected` cannot be debugged: a plugin's suggestion can only be run
/// (`SPEC-0276` behaviour 5, hand test 194 / D53). `None` for every other configuration.
pub fn debug_refusal(selected: Option<&str>, entries: &[PluginRunEntry]) -> Option<String> {
    let name = selected?;
    entries
        .iter()
        .any(|e| e.config_name == name)
        .then(|| format!("{name}: plugin configurations can only be run"))
}

/// The entries of `types` (only granted ones have commands), with what `allowances` allows.
pub fn entries(types: &[RunType], allowances: &RunAllowances) -> Vec<PluginRunEntry> {
    types
        .iter()
        .flat_map(|t| {
            t.commands.iter().map(move |c| {
                let allowance = t.allowance(c);
                PluginRunEntry {
                    config_name: t.config_name(c),
                    plugin_name: t.plugin_name.clone(),
                    type_title: t.title.clone(),
                    command: c.clone(),
                    always_allowed: allowances.allows(&allowance),
                    allowance,
                }
            })
        })
        .collect()
}

/// A choice of the confirmation, in the order shown; Cancel comes first and is selected first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmChoice {
    /// Start nothing.
    Cancel,
    /// Start it this time.
    RunOnce,
    /// Start it and keep the permission for this workspace (behaviour 7).
    AlwaysAllow,
}

impl ConfirmChoice {
    /// All choices in order.
    pub const ALL: [ConfirmChoice; 3] = [Self::Cancel, Self::RunOnce, Self::AlwaysAllow];

    /// The button's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cancel => "Cancel",
            Self::RunOnce => "Run Once",
            Self::AlwaysAllow => "Always Allow in This Workspace",
        }
    }
}

/// The confirmation's content: a title and labelled lines with the exact command (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    /// The heading.
    pub title: String,
    /// Label and value, one per line; arguments and variables take a line each.
    pub lines: Vec<(String, String)>,
    /// The selected choice (index into [`ConfirmChoice::ALL`]).
    pub selected: usize,
}

impl Confirmation {
    /// The confirmation for `entry` in the project at `root`.
    pub fn new(entry: &PluginRunEntry, root: &Path) -> Self {
        let c = &entry.command;
        let mut lines = vec![
            ("Plugin".to_owned(), entry.plugin_name.clone()),
            ("Type".to_owned(), entry.type_title.clone()),
            ("Program".to_owned(), c.program.clone()),
        ];
        if c.args.is_empty() {
            lines.push(("Arguments".into(), "(none)".into()));
        }
        for (i, a) in c.args.iter().enumerate() {
            // Each argument exactly, framed so spaces at its ends stay visible.
            lines.push((
                if i == 0 { "Arguments" } else { "" }.into(),
                format!("{}. ⟨{a}⟩", i + 1),
            ));
        }
        let folder = if c.cwd == "." {
            root.to_path_buf()
        } else {
            root.join(&c.cwd)
        };
        lines.push(("Folder".into(), folder.display().to_string()));
        for (i, (k, v)) in c.env.iter().enumerate() {
            lines.push((
                if i == 0 { "Environment" } else { "" }.into(),
                format!("{k}={v}"),
            ));
        }
        Self {
            title: format!("Run a command suggested by {}?", entry.plugin_name),
            lines,
            selected: 0,
        }
    }

    /// The selected choice.
    pub fn choice(&self) -> ConfirmChoice {
        ConfirmChoice::ALL[self.selected.min(ConfirmChoice::ALL.len() - 1)]
    }

    /// Handles a key (behaviour 5): ↑/↓, Tab and ⇧Tab move, Enter chooses, Escape cancels. The
    /// choice made, if any; `None` for a key that only moved or did nothing.
    pub fn key(&mut self, key: &str, shift: bool) -> Option<ConfirmChoice> {
        let n = ConfirmChoice::ALL.len();
        match key {
            "down" | "right" => self.selected = (self.selected + 1) % n,
            "tab" if !shift => self.selected = (self.selected + 1) % n,
            "up" | "left" | "tab" => self.selected = (self.selected + n - 1) % n,
            "enter" => return Some(self.choice()),
            "escape" => return Some(ConfirmChoice::Cancel),
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn run_type(commands: Vec<SuggestedCommand>) -> RunType {
        RunType {
            plugin: "runconfig".into(),
            plugin_name: "Sample Tasks".into(),
            kind: "tasks".into(),
            title: "Tasks".into(),
            granted: true,
            commands,
            problems: Vec::new(),
        }
    }

    fn command(name: &str, args: &[&str], cwd: &str) -> SuggestedCommand {
        SuggestedCommand {
            name: name.into(),
            program: "make".into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
        }
    }

    /// SPEC-0276 AK-4: chooser entries are named after the plugin, carry their heading and say
    /// whether exactly this command is always allowed.
    #[test]
    fn entries_name_the_plugin_and_the_permission() {
        let t = run_type(vec![
            command("build", &["build"], "."),
            command("test", &["test"], "."),
        ]);
        let allowances = RunAllowances {
            allow: vec![t.allowance(&t.commands[1])],
            file: None,
            problem: None,
        };
        let e = entries(std::slice::from_ref(&t), &allowances);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].config_name, "Sample Tasks: build");
        assert_eq!(e[0].heading(), "From plugin: Sample Tasks › Tasks");
        assert!(!e[0].always_allowed && e[1].always_allowed);
        // A changed command under the same name is not allowed any more.
        let changed = run_type(vec![command("test", &["test", "--all"], ".")]);
        assert!(!entries(&[changed], &allowances)[0].always_allowed);
        // A type without the grant has no commands and no entries.
        let mut ungranted = run_type(Vec::new());
        ungranted.granted = false;
        assert!(entries(&[ungranted], &allowances).is_empty());
    }

    /// Hand test 194 (D53): a plugin entry selected — by ⌃D on its highlighted row in the chooser,
    /// `take_chooser_highlight` — is refused for debugging; other configurations are not.
    #[test]
    fn a_plugin_entry_is_not_debugged() {
        let t = run_type(vec![command("hello", &["hello"], ".")]);
        let e = entries(std::slice::from_ref(&t), &RunAllowances::default());
        assert_eq!(
            debug_refusal(Some("Sample Tasks: hello"), &e).as_deref(),
            Some("Sample Tasks: hello: plugin configurations can only be run")
        );
        assert_eq!(debug_refusal(Some("leak-c"), &e), None);
        assert_eq!(debug_refusal(None, &e), None);
    }

    /// SPEC-0276 AK-4: the confirmation shows program, each argument, the absolute folder and the
    /// environment exactly.
    #[test]
    fn confirmation_shows_the_exact_command() {
        let mut c = command("deploy", &["a b", " x", "${workspace}"], "sub/dir");
        c.env.insert("MODE".into(), "fast".into());
        c.env.insert("A".into(), "1".into());
        let t = run_type(vec![c]);
        let e = &entries(&[t], &RunAllowances::default())[0];
        let conf = Confirmation::new(e, Path::new("/work/demo"));
        assert_eq!(conf.title, "Run a command suggested by Sample Tasks?");
        let lines: Vec<(&str, &str)> = conf
            .lines
            .iter()
            .map(|(l, v)| (l.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                ("Plugin", "Sample Tasks"),
                ("Type", "Tasks"),
                ("Program", "make"),
                ("Arguments", "1. ⟨a b⟩"),
                ("", "2. ⟨ x⟩"),
                ("", "3. ⟨${workspace}⟩"),
                ("Folder", "/work/demo/sub/dir"),
                ("Environment", "A=1"),
                ("", "MODE=fast"),
            ]
        );
        let none = run_type(vec![command("x", &[], ".")]);
        let conf = Confirmation::new(
            &entries(&[none], &RunAllowances::default())[0],
            Path::new("/w"),
        );
        assert!(conf.lines.contains(&("Arguments".into(), "(none)".into())));
        assert!(conf.lines.contains(&("Folder".into(), "/w".into())));
    }

    /// SPEC-0276 AK-4: Cancel is selected first; keys move and choose; Escape cancels.
    #[test]
    fn confirmation_keys() {
        let t = run_type(vec![command("x", &[], ".")]);
        let e = &entries(&[t], &RunAllowances::default())[0];
        let mut conf = Confirmation::new(e, Path::new("/w"));
        assert_eq!(conf.choice(), ConfirmChoice::Cancel);
        assert_eq!(conf.key("enter", false), Some(ConfirmChoice::Cancel));
        assert_eq!(conf.key("down", false), None);
        assert_eq!(conf.key("enter", false), Some(ConfirmChoice::RunOnce));
        assert_eq!(conf.key("tab", false), None);
        assert_eq!(conf.choice(), ConfirmChoice::AlwaysAllow);
        assert_eq!(
            ConfirmChoice::AlwaysAllow.label(),
            "Always Allow in This Workspace"
        );
        assert_eq!(conf.key("down", false), None);
        assert_eq!(conf.choice(), ConfirmChoice::Cancel, "wraps around");
        assert_eq!(conf.key("tab", true), None);
        assert_eq!(conf.choice(), ConfirmChoice::AlwaysAllow);
        assert_eq!(conf.key("escape", false), Some(ConfirmChoice::Cancel));
        assert_eq!(conf.key("x", false), None);
    }
}

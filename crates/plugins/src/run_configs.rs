//! Run configurations suggested by plugins (`SPEC-0276`): `run-config` actions answer commands as
//! data, the host checks them, and the user's "always allow in this workspace" is kept outside the
//! workspace for exactly the confirmed command.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    Access, ActionKind, INSPECT_FUEL, PanelContext, Permission, Plugin, PluginError, granted,
};

/// Most suggestions taken from one type (behaviour 3).
pub const MAX_SUGGESTIONS: usize = 50;
/// Longest name of a suggestion in characters.
pub const MAX_NAME: usize = 80;
/// Most arguments of one suggestion.
pub const MAX_ARGS: usize = 256;
/// Longest argument in bytes.
pub const MAX_ARG: usize = 8 << 10;
/// Most environment variables of one suggestion.
pub const MAX_ENV: usize = 64;

/// Command interpreters a plugin may not suggest as the program (behaviour 3): their argument
/// would be a command line the confirmation could not show as a list.
const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "csh",
    "tcsh",
    "cmd",
    "powershell",
    "pwsh",
];

/// What a `run-config` action gets (behaviour 2): the project folder's name and the active file.
pub type RunConfigContext = PanelContext;

/// One command a plugin suggests, as the plugin wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedCommand {
    /// Name, unique within its type.
    pub name: String,
    /// The program: a name looked up on `PATH` or a path.
    pub program: String,
    /// Its arguments, passed as they are.
    #[serde(default)]
    pub args: Vec<String>,
    /// Folder relative to the project; `.` is the project folder.
    #[serde(default = "dot")]
    pub cwd: String,
    /// Variables added to Ferris IDEA's environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

fn dot() -> String {
    ".".into()
}

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    configs: Vec<serde_json::Value>,
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// The suggestion `s` normalized (folder without `./`, `.` for the project), or why it is dropped
/// (behaviour 3).
pub fn check_suggestion(mut s: SuggestedCommand) -> Result<SuggestedCommand, String> {
    let name = s.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(format!(
            "a suggestion needs a name of 1 to {MAX_NAME} characters"
        ));
    }
    let fail = |why: String| Err(format!("{name}: {why}"));
    if has_control(&name) {
        return fail("the name has control characters".into());
    }
    if s.program.trim().is_empty() {
        return fail("the program is empty".into());
    }
    if has_control(&s.program) {
        return fail("the program has control characters".into());
    }
    let base = Path::new(&s.program)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let base = base.strip_suffix(".exe").unwrap_or(&base);
    if INTERPRETERS.contains(&base) {
        return fail(format!(
            "`{}` is a command interpreter; a plugin may only suggest a program with its arguments",
            s.program
        ));
    }
    if s.args.len() > MAX_ARGS {
        return fail(format!("more than {MAX_ARGS} arguments"));
    }
    for a in &s.args {
        if a.len() > MAX_ARG {
            return fail("an argument is longer than 8 KiB".into());
        }
        if has_control(a) {
            return fail("an argument has control characters".into());
        }
    }
    let cwd = s.cwd.trim();
    let cwd_path = Path::new(cwd);
    if has_control(cwd)
        || cwd_path.is_absolute()
        || cwd_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return fail(format!(
            "the folder `{}` must be relative to the project, without `..`",
            s.cwd
        ));
    }
    let parts: Vec<String> = cwd_path
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    s.cwd = if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    };
    if s.env.len() > MAX_ENV {
        return fail(format!("more than {MAX_ENV} environment variables"));
    }
    for (k, v) in &s.env {
        let ok_name = !k.is_empty()
            && !k.starts_with(|c: char| c.is_ascii_digit())
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok_name {
            return fail(format!("`{k}` is no environment variable name"));
        }
        if k.starts_with("DYLD_") || k.starts_with("LD_") {
            return fail(format!(
                "`{k}` changes how programs are loaded and is not allowed"
            ));
        }
        if has_control(v) {
            return fail(format!("the value of `{k}` has control characters"));
        }
    }
    s.name = name;
    Ok(s)
}

impl Plugin {
    /// Runs the `run-config` action `id` (behaviours 2–3) with a fifth of the budget: the first
    /// [`MAX_SUGGESTIONS`] suggestions that pass the checks, and why the others were dropped.
    pub fn run_config_suggestions(
        &self,
        id: &str,
        context: &RunConfigContext,
        access: Access,
    ) -> Result<(Vec<SuggestedCommand>, Vec<String>), PluginError> {
        let name = self.manifest.name.clone();
        if !self
            .manifest
            .actions
            .iter()
            .any(|a| a.id == id && a.kind == ActionKind::RunConfig)
        {
            return Err(PluginError::UnknownAction(name, id.to_owned()));
        }
        let input = serde_json::to_string(context)
            .map_err(|e| PluginError::Failed(name.clone(), e.to_string()))?;
        let output = self.run_in(id, &input, INSPECT_FUEL, access)?;
        let answer: Answer = serde_json::from_str(&output)
            .map_err(|e| PluginError::Failed(name, format!("its answer: {e}")))?;
        let mut out: Vec<SuggestedCommand> = Vec::new();
        let mut problems = Vec::new();
        for (i, value) in answer.configs.into_iter().enumerate() {
            if i >= MAX_SUGGESTIONS {
                problems.push(format!(
                    "only the first {MAX_SUGGESTIONS} suggestions are used"
                ));
                break;
            }
            let checked = serde_json::from_value::<SuggestedCommand>(value)
                .map_err(|e| format!("suggestion {}: {e}", i + 1))
                .and_then(check_suggestion);
            match checked {
                Ok(s) if out.iter().any(|o| o.name == s.name) => {
                    problems.push(format!("{}: the name is used twice", s.name));
                }
                Ok(s) => out.push(s),
                Err(e) => problems.push(e),
            }
        }
        Ok((out, problems))
    }
}

/// One run configuration type of a plugin and what it suggests now (behaviours 2–4).
#[derive(Debug, Clone, Serialize)]
pub struct RunType {
    /// The plugin's file name without `.wasm`.
    pub plugin: String,
    /// The plugin's name.
    pub plugin_name: String,
    /// The action id: the type.
    #[serde(rename = "type")]
    pub kind: String,
    /// The type's title.
    pub title: String,
    /// Whether the user granted `suggest-commands`; without it nothing was run.
    pub granted: bool,
    /// The checked suggestions.
    pub commands: Vec<SuggestedCommand>,
    /// Suggestions dropped, or why the action failed.
    pub problems: Vec<String>,
}

impl RunType {
    /// The run configuration name of suggestion `command`: `<plugin name>: <name>`.
    pub fn config_name(&self, command: &SuggestedCommand) -> String {
        format!("{}: {}", self.plugin_name, command.name)
    }

    /// What "always allow" stores for `command` (behaviour 7).
    pub fn allowance(&self, command: &SuggestedCommand) -> RunAllowance {
        RunAllowance {
            plugin: self.plugin.clone(),
            kind: self.kind.clone(),
            name: command.name.clone(),
            program: command.program.clone(),
            args: command.args.clone(),
            cwd: command.cwd.clone(),
            env: command.env.clone(),
        }
    }

    /// `command` as a run configuration that starts verbatim (behaviour 6).
    pub fn run_config(&self, command: &SuggestedCommand) -> ferris_idea_core::RunConfig {
        let mut argv = vec![command.program.clone()];
        argv.extend(command.args.iter().cloned());
        ferris_idea_core::RunConfig {
            name: self.config_name(command),
            command: argv,
            cwd: (command.cwd != ".").then(|| command.cwd.clone()),
            env: command.env.clone(),
            kind: ferris_idea_core::RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: true,
            http: None,
        }
    }
}

/// The run configuration types of `plugins` with their suggestions for the project at `root`
/// (behaviour 2): a type of a plugin without the `suggest-commands` grant in `dir` is listed but
/// not run. `active` is the active file relative to the project; it is not named to the plugins
/// when it is a file they could not read (`SPEC-0156` v4 behaviour 12). A suggestion whose folder
/// is reached through a symbolic link, lies outside the project or is no folder is dropped (v3
/// behaviour 3); one whose folder does not exist yet stays and is checked again when it starts.
pub fn run_types(
    plugins: &[Plugin],
    dir: Option<&Path>,
    root: &Path,
    active: Option<String>,
) -> Vec<RunType> {
    let context = RunConfigContext {
        project: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        active: active.filter(|a| crate::guarded(root, Path::new(a)).is_none()),
    };
    let mut out = Vec::new();
    for p in plugins {
        let file = p.file();
        let has = |perm: Permission| {
            p.manifest.permissions.contains(&perm) && dir.is_some_and(|d| granted(d, &file, perm))
        };
        let suggest = has(Permission::SuggestCommands);
        let access = Access {
            root: Some(root.to_path_buf()),
            project_read: has(Permission::ProjectRead),
            network: None,
            log: dir.map(|d| d.join(crate::FILE_AUDIT)),
        };
        for a in p
            .manifest
            .actions
            .iter()
            .filter(|a| a.kind == ActionKind::RunConfig)
        {
            let mut t = RunType {
                plugin: file.clone(),
                plugin_name: p.manifest.name.clone(),
                kind: a.id.clone(),
                title: a.title.clone(),
                granted: suggest,
                commands: Vec::new(),
                problems: Vec::new(),
            };
            if suggest {
                match p.run_config_suggestions(&a.id, &context, access.clone()) {
                    Ok((commands, problems)) => {
                        t.problems = problems;
                        for c in commands {
                            match ferris_idea_core::folder_below(root, Path::new(&c.cwd)) {
                                Ok(_) | Err(ferris_idea_core::FolderProblem::Missing) => {
                                    t.commands.push(c);
                                }
                                Err(why) => t
                                    .problems
                                    .push(format!("{}: the folder `{}` {why}", c.name, c.cwd)),
                            }
                        }
                    }
                    Err(e) => t.problems.push(e.to_string()),
                }
            }
            out.push(t);
        }
    }
    out
}

/// Name of the file in the user's settings folder that keeps "always allow" (behaviour 7).
pub const RUN_PERMISSIONS: &str = "plugin-run-permissions.toml";

/// `plugin-run-permissions.toml` in the user's Ferris IDEA settings folder.
pub fn run_permissions_file() -> Option<PathBuf> {
    ferris_idea_core::user_config_dir().map(|d| d.join(RUN_PERMISSIONS))
}

/// One command the user always allows in a workspace (behaviour 7): exactly this command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAllowance {
    /// The plugin's file name without `.wasm`.
    pub plugin: String,
    /// The run configuration type (action id).
    #[serde(rename = "type")]
    pub kind: String,
    /// The suggestion's name.
    pub name: String,
    /// The program.
    pub program: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Its folder relative to the project.
    #[serde(default = "dot")]
    pub cwd: String,
    /// Its added environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl RunAllowance {
    fn same_config(&self, other: &RunAllowance) -> bool {
        self.plugin == other.plugin && self.kind == other.kind && self.name == other.name
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowFile {
    #[serde(default)]
    workspace: Vec<AllowWorkspace>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowWorkspace {
    path: PathBuf,
    #[serde(default)]
    allow: Vec<RunAllowance>,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// What the user always allows in one workspace, and where it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunAllowances {
    /// The allowed commands.
    pub allow: Vec<RunAllowance>,
    /// The file consulted.
    pub file: Option<PathBuf>,
    /// Why nothing is allowed although the file exists, if so.
    pub problem: Option<String>,
}

impl RunAllowances {
    /// Whether exactly `command` is allowed.
    pub fn allows(&self, command: &RunAllowance) -> bool {
        self.allow.contains(command)
    }
}

/// Reads the whole file: `Ok(None)` when it does not exist.
fn read_file(file: &Path) -> Result<Option<AllowFile>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => toml::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// What `file` always allows in the workspace at `root` (behaviour 7): paths compared after
/// resolving links; a file inside the workspace and a malformed file allow nothing.
pub fn load_allowances(file: &Path, root: &Path) -> RunAllowances {
    let mut out = RunAllowances {
        file: Some(file.to_path_buf()),
        ..RunAllowances::default()
    };
    let root = canon(root);
    if canon(file).starts_with(&root) {
        out.problem = Some("the file lies inside the workspace and is ignored".into());
        return out;
    }
    match read_file(file) {
        Ok(Some(f)) => {
            for w in f.workspace {
                if canon(&w.path) == root {
                    out.allow.extend(w.allow);
                }
            }
        }
        Ok(None) => {}
        Err(e) => out.problem = Some(e),
    }
    out
}

/// Changes the file with `change`; refuses a file inside the workspace and never overwrites a
/// malformed file.
fn edit_file(
    file: &Path,
    root: &Path,
    change: impl FnOnce(&mut AllowFile, &Path) -> usize,
) -> Result<usize, String> {
    let root = canon(root);
    if canon(file).starts_with(&root) {
        return Err("the file lies inside the workspace".into());
    }
    let mut all = read_file(file)?.unwrap_or_default();
    let n = change(&mut all, &root);
    write_file(file, &all)?;
    Ok(n)
}

fn write_file(file: &Path, all: &AllowFile) -> Result<(), String> {
    let text = toml::to_string_pretty(all).map_err(|e| e.to_string())?;
    let io = |e: std::io::Error| format!("{}: {e}", file.display());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    // Written beside the file first, so a failed write never leaves half a file behind.
    let part = file.with_extension("toml.part");
    std::fs::write(&part, text).map_err(io)?;
    std::fs::rename(&part, file).map_err(io)
}

/// Always allows `command` in the workspace at `root` (the confirmation's third choice); an older
/// entry for the same plugin, type and name is replaced.
pub fn allow(file: &Path, root: &Path, command: &RunAllowance) -> Result<(), String> {
    edit_file(file, root, |all, root| {
        let w = match all.workspace.iter().position(|w| canon(&w.path) == root) {
            Some(i) => &mut all.workspace[i],
            None => {
                all.workspace.push(AllowWorkspace {
                    path: root.to_path_buf(),
                    allow: Vec::new(),
                });
                all.workspace.last_mut().expect("just pushed")
            }
        };
        w.allow.retain(|a| !a.same_config(command));
        w.allow.push(command.clone());
        1
    })
    .map(|_| ())
}

/// Forgets the entries of plugin `plugin` in the workspace at `root` (behaviour 8), all of them
/// or only those of `kind` and `name`: how many were forgotten.
pub fn revoke(
    file: &Path,
    root: &Path,
    plugin: &str,
    kind: Option<&str>,
    name: Option<&str>,
) -> Result<usize, String> {
    if read_file(file)?.is_none() {
        return Ok(0);
    }
    edit_file(file, root, |all, root| {
        let mut n = 0;
        for w in all.workspace.iter_mut().filter(|w| canon(&w.path) == root) {
            let before = w.allow.len();
            w.allow.retain(|a| {
                !(a.plugin == plugin
                    && kind.is_none_or(|k| a.kind == k)
                    && name.is_none_or(|x| a.name == x))
            });
            n += before - w.allow.len();
        }
        all.workspace.retain(|w| !w.allow.is_empty());
        n
    })
}

/// Forgets every entry of plugin `plugin` in every workspace (behaviour 1: the grant withdrawn or
/// the plugin removed): how many were forgotten.
pub fn forget_plugin(file: &Path, plugin: &str) -> Result<usize, String> {
    let Some(mut all) = read_file(file)? else {
        return Ok(0);
    };
    let mut n = 0;
    for w in &mut all.workspace {
        let before = w.allow.len();
        w.allow.retain(|a| a.plugin != plugin);
        n += before - w.allow.len();
    }
    if n > 0 {
        all.workspace.retain(|w| !w.allow.is_empty());
        write_file(file, &all)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dirs {
        base: PathBuf,
    }

    impl Dirs {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "ferris-idea-plugin-run-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("config")).unwrap();
            std::fs::create_dir_all(base.join("ws/sub")).unwrap();
            std::fs::create_dir_all(base.join("other")).unwrap();
            Self {
                base: base.canonicalize().unwrap(),
            }
        }
        fn file(&self) -> PathBuf {
            self.base.join("config").join(RUN_PERMISSIONS)
        }
        fn ws(&self) -> PathBuf {
            self.base.join("ws")
        }
        fn other(&self) -> PathBuf {
            self.base.join("other")
        }
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn entry(name: &str, args: &[&str]) -> RunAllowance {
        RunAllowance {
            plugin: "runconfig".into(),
            kind: "tasks".into(),
            name: name.into(),
            program: "make".into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: ".".into(),
            env: BTreeMap::new(),
        }
    }

    /// SPEC-0276 AK-2: stored per workspace, only the identical command, another workspace not.
    #[test]
    fn allows_exactly_the_stored_command_per_workspace() {
        let d = Dirs::new("exact");
        let build = entry("build", &["build"]);
        assert!(!load_allowances(&d.file(), &d.ws()).allows(&build));
        allow(&d.file(), &d.ws(), &build).unwrap();
        let loaded = load_allowances(&d.file(), &d.ws());
        assert!(loaded.allows(&build), "{loaded:?}");
        assert_eq!(loaded.problem, None);
        // The same name with another argument is not allowed.
        assert!(!loaded.allows(&entry("build", &["build", "install"])));
        let mut with_env = build.clone();
        with_env.env.insert("A".into(), "1".into());
        assert!(!loaded.allows(&with_env));
        // Through `..` and a link the workspace is the same; another one is not.
        assert!(load_allowances(&d.file(), &d.ws().join("sub/..")).allows(&build));
        #[cfg(unix)]
        {
            let link = d.base.join("link");
            std::os::unix::fs::symlink(d.ws(), &link).unwrap();
            assert!(load_allowances(&d.file(), &link).allows(&build));
        }
        assert!(!load_allowances(&d.file(), &d.other()).allows(&build));
        // Allowing a changed command replaces the entry of that name.
        allow(&d.file(), &d.ws(), &entry("build", &["all"])).unwrap();
        let loaded = load_allowances(&d.file(), &d.ws());
        assert_eq!(loaded.allow.len(), 1);
        assert!(loaded.allows(&entry("build", &["all"])));
        let text = std::fs::read_to_string(d.file()).unwrap();
        assert!(text.contains("[[workspace.allow]]"), "{text}");
    }

    /// SPEC-0276 AK-2: a file inside the workspace and a malformed file allow nothing.
    #[test]
    fn a_file_inside_the_workspace_or_malformed_allows_nothing() {
        let d = Dirs::new("inside");
        let build = entry("build", &["build"]);
        let inside = d.ws().join(RUN_PERMISSIONS);
        std::fs::write(
            &inside,
            format!(
                "[[workspace]]\npath = \"{}\"\n[[workspace.allow]]\nplugin = \"runconfig\"\ntype = \"tasks\"\nname = \"build\"\nprogram = \"make\"\nargs = [\"build\"]\n",
                d.ws().display()
            ),
        )
        .unwrap();
        let loaded = load_allowances(&inside, &d.ws());
        assert!(!loaded.allows(&build));
        assert!(loaded.problem.unwrap().contains("inside the workspace"));
        assert!(allow(&inside, &d.ws(), &build).is_err());
        // The same text outside works; with an unknown key it allows nothing and is not
        // overwritten.
        std::fs::copy(&inside, d.file()).unwrap();
        assert!(load_allowances(&d.file(), &d.ws()).allows(&build));
        std::fs::write(d.file(), "[[workspace]]\npath = \"x\"\nsecret = 1\n").unwrap();
        let loaded = load_allowances(&d.file(), &d.ws());
        assert!(loaded.allow.is_empty() && loaded.problem.is_some());
        assert!(allow(&d.file(), &d.ws(), &build).is_err());
        assert!(
            std::fs::read_to_string(d.file())
                .unwrap()
                .contains("secret")
        );
    }

    /// SPEC-0276 AK-2: revoking by name, by plugin in one workspace, and everywhere.
    #[test]
    fn revokes_by_name_and_by_plugin() {
        let d = Dirs::new("revoke");
        let (build, test) = (entry("build", &["build"]), entry("test", &["test"]));
        assert_eq!(revoke(&d.file(), &d.ws(), "runconfig", None, None), Ok(0));
        allow(&d.file(), &d.ws(), &build).unwrap();
        allow(&d.file(), &d.ws(), &test).unwrap();
        allow(&d.file(), &d.other(), &build).unwrap();
        assert_eq!(
            revoke(
                &d.file(),
                &d.ws(),
                "runconfig",
                Some("tasks"),
                Some("build")
            ),
            Ok(1)
        );
        let loaded = load_allowances(&d.file(), &d.ws());
        assert!(!loaded.allows(&build) && loaded.allows(&test));
        assert!(load_allowances(&d.file(), &d.other()).allows(&build));
        assert_eq!(revoke(&d.file(), &d.ws(), "other", None, None), Ok(0));
        allow(&d.file(), &d.ws(), &build).unwrap();
        assert_eq!(revoke(&d.file(), &d.ws(), "runconfig", None, None), Ok(2));
        assert!(load_allowances(&d.file(), &d.other()).allows(&build));
        allow(&d.file(), &d.ws(), &test).unwrap();
        assert_eq!(forget_plugin(&d.file(), "runconfig"), Ok(2));
        assert!(load_allowances(&d.file(), &d.other()).allow.is_empty());
        assert_eq!(forget_plugin(&d.base.join("missing.toml"), "x"), Ok(0));
    }

    fn suggestion(json: serde_json::Value) -> Result<SuggestedCommand, String> {
        check_suggestion(serde_json::from_value(json).unwrap())
    }

    /// SPEC-0276 AK-1: the checks of behaviour 3.
    #[test]
    fn checks_suggestions() {
        let ok = suggestion(serde_json::json!({"name": " build ", "program": "make", "args": ["build"], "cwd": "./sub/", "env": {"MODE": "fast"}})).unwrap();
        assert_eq!(ok.name, "build");
        assert_eq!(ok.cwd, "sub");
        assert_eq!(
            suggestion(serde_json::json!({"name": "x", "program": "./gradlew"}))
                .unwrap()
                .cwd,
            "."
        );
        for (bad, why) in [
            (serde_json::json!({"name": "", "program": "make"}), "name"),
            (
                serde_json::json!({"name": "x".repeat(81), "program": "make"}),
                "name",
            ),
            (
                serde_json::json!({"name": "a\nb", "program": "make"}),
                "control",
            ),
            (serde_json::json!({"name": "x", "program": " "}), "empty"),
            (
                serde_json::json!({"name": "x", "program": "/bin/sh"}),
                "interpreter",
            ),
            (
                serde_json::json!({"name": "x", "program": "BASH.EXE"}),
                "interpreter",
            ),
            (
                serde_json::json!({"name": "x", "program": "pwsh"}),
                "interpreter",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "args": ["a\u{1b}[2J"]}),
                "control",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "args": vec!["a"; 257]}),
                "arguments",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "args": ["a".repeat(8193)]}),
                "8 KiB",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "cwd": "../up"}),
                "folder",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "cwd": "/tmp"}),
                "folder",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "env": {"DYLD_INSERT_LIBRARIES": "a"}}),
                "loaded",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "env": {"LD_PRELOAD": "a"}}),
                "loaded",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "env": {"1A": "a"}}),
                "name",
            ),
            (
                serde_json::json!({"name": "x", "program": "make", "env": {"A-B": "a"}}),
                "name",
            ),
        ] {
            let e = suggestion(bad.clone()).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
    }
}

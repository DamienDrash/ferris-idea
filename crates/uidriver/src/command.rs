//! Window commands of external clients (`SPEC-0319` v7 behaviours 46-53): open a file of the
//! project, show, focus and close a tool window, show a Settings page, invoke an action on the
//! allow list, type text into the focused editor or popup field, read and choose in an open popup,
//! and list what can be addressed. Each command has a declared effect; this module holds the
//! checks that need no window — the command's shape, the allow list of actions, the reserved
//! Settings pages, the files that are never opened — and the window adds what only it knows
//! (which popup is open, what has the focus).
//!
//! Nothing here grants a right, answers a question, reads a secret or reaches outside the
//! project: a refused command delivers nothing.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, UiError};
use crate::security::{Actor, Grants, UI_MEANING, UI_RIGHT, effects_of_action, reserved_action};
use crate::tree::{Effect, Reserved};

/// What to do with a tool window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolWindowOp {
    /// Show it (and give it the focus, as its activation action does).
    #[default]
    Open,
    /// Show it if hidden and give it the focus.
    Focus,
    /// Hide it.
    Close,
}

impl ToolWindowOp {
    /// The op's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Focus => "focus",
            Self::Close => "close",
        }
    }
}

/// One window command (behaviour 46).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum UiCommand {
    /// Opens a file of the project in an editor tab, at a 1-based line and column.
    OpenFile {
        /// Relative to the project root, or absolute inside it.
        path: String,
        /// 1-based line.
        #[serde(default)]
        line: Option<usize>,
        /// 1-based column (needs `line`).
        #[serde(default)]
        column: Option<usize>,
    },
    /// Shows, focuses or hides a tool window by its id (`project`, `problems`, `containers`, …).
    ToolWindow {
        /// The tool window's id.
        id: String,
        /// `open` (default), `focus` or `close`.
        #[serde(default)]
        op: ToolWindowOp,
    },
    /// Opens the Settings dialog on a page by its id (`appearance`, `keymap`, `code-style`, …).
    Settings {
        /// The page's id: its title in kebab case.
        page: String,
    },
    /// Closes the Settings dialog as Escape does; nothing is saved by closing.
    CloseSettings,
    /// Invokes an action by its id, only actions on the allow list (behaviour 49).
    Invoke {
        /// The action id as the keymap writes it (`ShowIntentionActions`).
        action: String,
    },
    /// Types Unicode text into the focused editor or popup field (behaviour 50).
    TypeText {
        /// The text; logged only as its length.
        text: String,
    },
    /// Reads the visible items of the open popup or list (behaviour 51).
    Popup,
    /// Chooses an item of the open popup by its index or its exact label (behaviour 51).
    Choose {
        /// 0-based index of the item as `popup` lists it.
        #[serde(default)]
        index: Option<usize>,
        /// Or its exact label.
        #[serde(default)]
        label: Option<String>,
    },
    /// Lists the ids of the tool windows and Settings pages (behaviour 52).
    List,
    /// Closes every open popup, submenu and context menu without a choice, as Escape does
    /// (behaviour 51a).
    Dismiss,
}

impl UiCommand {
    /// The command's name, as in the audit log.
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenFile { .. } => "open_file",
            Self::ToolWindow { .. } => "tool_window",
            Self::Settings { .. } => "settings",
            Self::CloseSettings => "close_settings",
            Self::Invoke { .. } => "invoke",
            Self::TypeText { .. } => "type_text",
            Self::Popup => "popup",
            Self::Choose { .. } => "choose",
            Self::List => "list",
            Self::Dismiss => "dismiss",
        }
    }

    /// What the audit log says about the call: the command and its target, never typed text.
    pub fn audit_text(&self) -> String {
        match self {
            Self::OpenFile { path, line, column } => {
                let mut s = format!("open_file {path}");
                if let Some(l) = line {
                    s.push_str(&format!(":{l}"));
                    if let Some(c) = column {
                        s.push_str(&format!(":{c}"));
                    }
                }
                s
            }
            Self::ToolWindow { id, op } => format!("tool_window {id} {}", op.name()),
            Self::Settings { page } => format!("settings {page}"),
            Self::Invoke { action } => format!("invoke {action}"),
            Self::Choose { index, label } => match (index, label) {
                (Some(i), _) => format!("choose #{i}"),
                (None, Some(l)) => format!("choose {l:?}"),
                (None, None) => "choose".into(),
            },
            other => other.name().into(),
        }
    }

    /// The length of typed text, for the audit log.
    pub fn typed_len(&self) -> Option<usize> {
        match self {
            Self::TypeText { text } => Some(text.chars().count()),
            _ => None,
        }
    }

    /// Whether the command only reads.
    pub fn reads_only(&self) -> bool {
        matches!(self, Self::Popup | Self::List)
    }

    /// Whether the command may run while the Settings dialog is open: reading, switching to
    /// another page that is not reserved and closing it. Nothing else reaches its pages.
    pub fn allowed_in_settings(&self) -> bool {
        self.reads_only() || matches!(self, Self::Settings { .. } | Self::CloseSettings)
    }
}

/// Settings pages reserved for the user (behaviour 48): AI Assistant holds the switch of this
/// driver, the MCP server and the API key; Trusted Locations decides which code may run.
pub const RESERVED_SETTINGS_PAGES: [&str; 2] = ["ai-assistant", "trusted-locations"];

/// The id of a Settings page from its title: lower case, words joined by `-`, `&` left out
/// (`Languages & Frameworks` → `languages-frameworks`).
pub fn page_id(title: &str) -> String {
    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// The effects of showing a tool window (behaviour 47): the Terminal starts a shell.
pub fn tool_window_effects(id: &str) -> Vec<Effect> {
    match id {
        "terminal" => vec![Effect::Run],
        _ => vec![Effect::None],
    }
}

/// Words in an action id that mark it as destructive, as touching credentials or as deciding a
/// permission (behaviour 49): such actions are never on the allow list, whatever their effects.
const DENIED_WORDS: [&str; 22] = [
    "Delete",
    "Remove",
    "Revert",
    "Rollback",
    "Discard",
    "Reset",
    "Drop",
    "Truncate",
    "Purge",
    "Password",
    "Credential",
    "Token",
    "Secret",
    "Keychain",
    "Vault",
    "Login",
    "Account",
    "Trust",
    "Permission",
    "Grant",
    "Paste",
    "Settings",
];

/// Action ids denied by name although their effects alone would pass (behaviour 49).
const DENIED_IDS: [&str; 8] = [
    // Opening Settings goes through the `settings` command, which keeps its reserved pages.
    "ShowSettings",
    "ShowProjectStructureSettings",
    // Run Anything runs any typed command line.
    "RunAnything",
    // Closing projects and quitting are destructive for unsaved work.
    "CloseProject",
    "CloseAllProjects",
    "CloseOtherProjects",
    "Exit",
    "$Quit",
];

/// The effects an action may have to be on the allow list: nothing outside the window, the
/// workspace's files, starting programs and debug sessions — each still needs its right.
const ALLOWED_EFFECTS: [Effect; 4] = [Effect::None, Effect::Write, Effect::Run, Effect::Debug];

/// Whether `id` is on the allow list of `invoke`, with its effects (behaviour 49): its effects
/// are declared (`effects_of_action`), every one is in [`ALLOWED_EFFECTS`], it does not read the
/// clipboard, and neither its id nor a word of it is denied.
pub fn invokable(id: &str) -> Result<Vec<Effect>, UiError> {
    let refuse = |why: &str| {
        UiError::new(
            ErrorCode::Reserved,
            format!(
                "reserved: the action `{id}` is not on the allow list of external clients: {why}"
            ),
            "invoke actions that show, navigate or edit; destructive, credential and permission actions are the user's",
        )
    };
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        return Err(UiError::invalid(
            "invalid: an action id is 1 to 200 printable characters",
        ));
    }
    if let Some(why) = reserved_action(id) {
        return Err(refuse(why.name()));
    }
    if DENIED_IDS.contains(&id) {
        return Err(refuse("denied by name"));
    }
    if let Some(w) = DENIED_WORDS.iter().find(|w| id.contains(*w)) {
        return Err(refuse(&format!("its id contains `{w}`")));
    }
    let Some(effects) = effects_of_action(id) else {
        return Err(UiError::new(
            ErrorCode::Undeclared,
            format!(
                "undeclared: the effect of the action `{id}` is not declared, so external clients may not invoke it"
            ),
            "invoke an action whose effect is declared (SPEC-0319 behaviour 35)",
        ));
    };
    if let Some(e) = effects.iter().find(|e| !ALLOWED_EFFECTS.contains(e)) {
        return Err(refuse(&format!("its effect `{}`", e.name())));
    }
    Ok(effects)
}

/// File names and folders that are never opened for external clients (behaviour 46): the
/// project's own Ferris IDEA and Git state (which hold grants, logs and hooks) and files that
/// usually hold keys or credentials.
pub fn secret_like_file(rel: &Path) -> bool {
    let parts: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    let dir_denied = parts.iter().rev().skip(1).any(|d| {
        matches!(
            d.as_str(),
            ".ferris-idea" | ".git" | ".ssh" | ".gnupg" | ".aws" | ".docker" | ".kube"
        )
    });
    let Some(name) = parts.last() else {
        return true;
    };
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    dir_denied
        || name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".env")
        || matches!(
            name.as_str(),
            ".netrc" | ".npmrc" | ".pypirc" | ".pgpass" | ".git-credentials" | "credentials"
        )
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || name.starts_with("id_dsa")
        || name.contains("secret")
        || matches!(
            ext.as_str(),
            "pem" | "key" | "p12" | "pfx" | "jks" | "keystore" | "kdbx" | "gpg" | "asc"
        )
}

/// The file `path` names inside `root` (behaviour 46): relative paths are taken from the root;
/// `..` is refused; the canonical path (links followed) must lie inside the canonical root, be a
/// regular file and not be secret-like. Errors say why without the file's content.
pub fn project_file(root: &Path, path: &str) -> Result<PathBuf, UiError> {
    let outside = || {
        UiError::new(
            ErrorCode::Denied,
            format!("denied: {path} is not a file inside the project"),
            "open files of the project only, relative to its root",
        )
    };
    let given = Path::new(path);
    if path.is_empty()
        || given
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(outside());
    }
    let root = std::fs::canonicalize(root).map_err(|_| outside())?;
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        root.join(given)
    };
    let real = std::fs::canonicalize(&joined).map_err(|_| {
        UiError::new(
            ErrorCode::NotFound,
            format!("not_found: no file {path} in the project"),
            "give a path relative to the project root",
        )
    })?;
    let Ok(rel) = real.strip_prefix(&root) else {
        return Err(outside());
    };
    if !real.is_file() {
        return Err(UiError::invalid(format!("invalid: {path} is not a file")));
    }
    if secret_like_file(rel) {
        return Err(UiError::new(
            ErrorCode::Reserved,
            format!(
                "reserved: {path} may hold credentials or the project's agent state; it is not opened for external clients"
            ),
            "the user opens it in the window",
        ));
    }
    Ok(real)
}

/// The checks of a command before the window sees it (behaviours 34, 46-52): the right `ui`,
/// the shape of the arguments, the allow list and the reserved pages. Returns the effects known
/// so far; the window checks those it learns (a popup's choice, the focused field) with
/// [`check_effects`]. The in-process self-test passes.
pub fn authorize_command(
    actor: &Actor,
    command: &UiCommand,
    grants: &Grants,
) -> Result<Vec<Effect>, UiError> {
    let effects = static_effects(command)?;
    if !actor.is_external() {
        return Ok(effects);
    }
    if !grants.has(UI_RIGHT) {
        return Err(UiError::not_granted(UI_RIGHT, UI_MEANING));
    }
    check_effects(&effects, grants)?;
    Ok(effects)
}

/// The effects a command has before the window looks; empty where only the window can tell.
fn static_effects(command: &UiCommand) -> Result<Vec<Effect>, UiError> {
    match command {
        UiCommand::OpenFile { line, column, .. } => {
            if line == &Some(0) || column == &Some(0) {
                return Err(UiError::invalid("invalid: line and column count from 1"));
            }
            if column.is_some() && line.is_none() {
                return Err(UiError::invalid("invalid: a column needs a line"));
            }
            Ok(vec![Effect::None])
        }
        UiCommand::ToolWindow { id, op } => Ok(match op {
            ToolWindowOp::Close => vec![Effect::None],
            _ => tool_window_effects(id),
        }),
        UiCommand::Settings { page } => {
            if RESERVED_SETTINGS_PAGES.contains(&page.as_str()) {
                return Err(UiError::new(
                    ErrorCode::Reserved,
                    format!(
                        "reserved: the Settings page `{page}` is reserved for the user (agent rights, MCP, UI automation, trust)"
                    ),
                    "the user opens it in the window",
                ));
            }
            Ok(vec![Effect::None])
        }
        UiCommand::CloseSettings | UiCommand::Popup | UiCommand::List | UiCommand::Dismiss => {
            Ok(vec![Effect::None])
        }
        UiCommand::Invoke { action } => invokable(action),
        UiCommand::TypeText { text } => {
            if text.is_empty() {
                return Err(UiError::invalid("invalid: type_text needs text"));
            }
            if text.chars().count() > 10_000 {
                return Err(UiError::invalid(
                    "invalid: type_text takes at most 10000 characters",
                ));
            }
            // The window decides between the editor (write) and a popup's field (none).
            Ok(Vec::new())
        }
        UiCommand::Choose { index, label } => {
            if index.is_none() == label.is_none() {
                return Err(UiError::invalid("invalid: choose takes `index` or `label`"));
            }
            Ok(Vec::new())
        }
    }
}

/// The refusal of a command whose effect needs `right`: at once, never a question in the window
/// that a driving agent cannot answer (behaviour 54a).
pub fn not_granted_now(right: &str, meaning: &str) -> UiError {
    let mut e = UiError::not_granted(right, meaning);
    e.hint = format!(
        "window commands never wait for a question: the user adds `{right}` for this workspace to agent-permissions.toml in the settings folder (or grants it in the window for another tool call), then call again"
    );
    e
}

/// Checks each effect's right (behaviour 35); trusting the project is never a command's.
pub fn check_effects(effects: &[Effect], grants: &Grants) -> Result<(), UiError> {
    let set: BTreeSet<Effect> = effects.iter().copied().collect();
    if set.contains(&Effect::Trust) {
        return Err(UiError::new(
            ErrorCode::Reserved,
            format!(
                "reserved: trusting the project is reserved for the user ({})",
                Reserved::Trust.name()
            ),
            "the user does this in the window; nothing was delivered",
        ));
    }
    for e in &set {
        if let Some(right) = e.right()
            && !grants.has(right)
        {
            return Err(not_granted_now(right, e.meaning()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> Actor {
        Actor::External {
            client: "agent".into(),
            session: "s".into(),
        }
    }

    #[test]
    fn commands_parse_from_json() {
        let c: UiCommand = serde_json::from_value(
            serde_json::json!({"command": "open_file", "path": "src/a.rs", "line": 3}),
        )
        .unwrap();
        assert_eq!(
            c,
            UiCommand::OpenFile {
                path: "src/a.rs".into(),
                line: Some(3),
                column: None
            }
        );
        let c: UiCommand =
            serde_json::from_value(serde_json::json!({"command": "tool_window", "id": "problems"}))
                .unwrap();
        assert_eq!(
            c,
            UiCommand::ToolWindow {
                id: "problems".into(),
                op: ToolWindowOp::Open
            }
        );
        assert_eq!(c.audit_text(), "tool_window problems open");
    }

    #[test]
    fn page_ids_are_kebab_titles() {
        assert_eq!(page_id("Languages & Frameworks"), "languages-frameworks");
        assert_eq!(page_id("AI Assistant"), "ai-assistant");
        assert_eq!(page_id("Code Style"), "code-style");
    }

    #[test]
    fn the_allow_list_keeps_destructive_credential_and_permission_actions_out() {
        assert_eq!(
            invokable("ShowIntentionActions").unwrap(),
            vec![Effect::None]
        );
        assert_eq!(
            invokable("ActivateProblemsToolWindow").unwrap(),
            vec![Effect::None]
        );
        assert_eq!(invokable("ReformatCode").unwrap(), vec![Effect::Write]);
        for id in [
            "SafeDelete",
            "$Delete",
            "ChangesView.Revert",
            "Vcs.Push",
            "CheckinProject",
            "$Paste",
            "ShowSettings",
            "RunAnything",
            "Exit",
            "CloseProject",
            "FerrisIDEA.Layout.Delete:x",
        ] {
            let e = invokable(id).unwrap_err();
            assert!(matches!(e.code, ErrorCode::Reserved), "{id}: {e:?}");
        }
        assert_eq!(
            invokable("NoSuchAction").unwrap_err().code,
            ErrorCode::Undeclared
        );
    }

    #[test]
    fn reserved_pages_and_rights_are_checked_before_the_window() {
        let none = Grants::default();
        let ui = Grants::of(&["ui"]);
        let open = UiCommand::ToolWindow {
            id: "containers".into(),
            op: ToolWindowOp::Open,
        };
        assert_eq!(
            authorize_command(&agent(), &open, &none).unwrap_err().code,
            ErrorCode::NotGranted
        );
        assert!(authorize_command(&agent(), &open, &ui).is_ok());
        let term = UiCommand::ToolWindow {
            id: "terminal".into(),
            op: ToolWindowOp::Open,
        };
        let e = authorize_command(&agent(), &term, &ui).unwrap_err();
        assert_eq!(e.right.as_deref(), Some("run"));
        let ai = UiCommand::Settings {
            page: "ai-assistant".into(),
        };
        assert_eq!(
            authorize_command(&agent(), &ai, &ui).unwrap_err().code,
            ErrorCode::Reserved
        );
        let reformat = UiCommand::Invoke {
            action: "ReformatCode".into(),
        };
        assert_eq!(
            authorize_command(&agent(), &reformat, &ui)
                .unwrap_err()
                .right
                .as_deref(),
            Some("write")
        );
        assert!(authorize_command(&agent(), &reformat, &Grants::of(&["ui", "write"])).is_ok());
        // The self-test passes without rights.
        assert!(authorize_command(&Actor::SelfTest, &term, &none).is_ok());
    }

    #[test]
    fn only_project_files_that_hold_no_credentials_open() {
        let base = std::env::temp_dir().join(format!("uidriver-cmd-{}", std::process::id()));
        let root = base.join("p");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join(".env"), "X=1\n").unwrap();
        std::fs::write(root.join(".ferris-idea/agent-log.jsonl"), "\n").unwrap();
        std::fs::write(base.join("outside.txt"), "x\n").unwrap();
        assert!(project_file(&root, "src/a.rs").is_ok());
        let abs = root.join("src/a.rs");
        assert!(project_file(&root, abs.to_str().unwrap()).is_ok());
        assert_eq!(
            project_file(&root, "../outside.txt").unwrap_err().code,
            ErrorCode::Denied
        );
        let out = base.join("outside.txt");
        assert_eq!(
            project_file(&root, out.to_str().unwrap()).unwrap_err().code,
            ErrorCode::Denied
        );
        assert_eq!(
            project_file(&root, ".env").unwrap_err().code,
            ErrorCode::Reserved
        );
        assert_eq!(
            project_file(&root, ".ferris-idea/agent-log.jsonl")
                .unwrap_err()
                .code,
            ErrorCode::Reserved
        );
        assert_eq!(
            project_file(&root, "src").unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            project_file(&root, "nope.rs").unwrap_err().code,
            ErrorCode::NotFound
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside.txt"), root.join("link.txt")).unwrap();
            assert_eq!(
                project_file(&root, "link.txt").unwrap_err().code,
                ErrorCode::Denied
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn audit_text_never_holds_typed_text() {
        let t = UiCommand::TypeText {
            text: "hunter2".into(),
        };
        assert_eq!(t.audit_text(), "type_text");
        assert_eq!(t.typed_len(), Some(7));
    }
}

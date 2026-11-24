//! What AI agents may do in a workspace beyond reading (`SPEC-0053`).
//!
//! Grants live in the user's own settings folder, never in the workspace, so a cloned project cannot
//! grant itself the right to run code (IMP-0028).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Name of the grant file inside the user's Ferris IDEA settings folder.
pub const GRANT_FILE: &str = "agent-permissions.toml";
/// Log of everything agents ran, relative to the workspace root.
pub const AGENT_LOG: &str = ".ferris-idea/agent-log.jsonl";

/// A right an agent can be given (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Grant {
    /// Start programs: run configurations and commands.
    Run,
    /// Send HTTP requests.
    Http,
    /// Start debug sessions.
    Debug,
    /// Fetch, pull and push Git remotes.
    Remote,
    /// Reach SSH hosts: browse, read and write files there (`SPEC-0121`); running commands there
    /// also needs `run`.
    Ssh,
    /// Trust the project, so its code may run (`SPEC-0278` behaviour 13).
    Trust,
    /// Create, change, move or delete files of the workspace (`SPEC-0053` v4 behaviour 6).
    Write,
    /// Change the repository or the working tree through version control: commit, branch, switch,
    /// merge, stash, cherry-pick, rebase, resolve, pull, push (`SPEC-0053` v4 behaviour 6).
    #[serde(rename = "vcs-write")]
    VcsWrite,
    /// Quit the window while work is unsaved or running (`SPEC-0261` v2 behaviour 8).
    Quit,
    /// Read the window's terminal tabs, masked (`SPEC-0313` behaviour 9).
    #[serde(rename = "terminal-read")]
    TerminalRead,
    /// Forward a port of a remote project's host to this computer's loopback (`SPEC-0318`
    /// behaviour 30).
    Ports,
    /// Operate the Ferris IDEA window through the UI driver (`SPEC-0319` behaviour 34); each action
    /// also needs the rights of its effects.
    Ui,
    /// Read the text of files whose names commonly hold secrets (`.env`, keys, credential files,
    /// `http-client.private.env.json`) and see the private values of the HTTP client unmasked
    /// (`SPEC-0053` v6 behaviour 10).
    #[serde(rename = "secret-read")]
    SecretRead,
    /// Change the user's own Ferris IDEA settings, which hold for every project: IDE settings,
    /// keymap, theme, plugin grants (`SPEC-0053` v8 behaviour 12).
    Settings,
    /// Read the folders of the other projects the user opened recently, which lie outside the
    /// workspace (`SPEC-0094` v9 behaviour 23).
    #[serde(rename = "recent-projects")]
    RecentProjects,
    /// Start, prompt and stop the agents the user configured for the agent host, read their
    /// output and answer their permission requests (`SPEC-0348` behaviour 18).
    Agents,
    /// Store, generate, list and remove entries of the internal vault (`SPEC-0353` behaviour 22);
    /// never read a value. Each use of an entry still needs the person's approval in the window.
    Vault,
    /// Create tickets, comment on them, edit and move them along (`SPEC-0352` behaviour 18);
    /// closing, cancelling, priority and other people's text also need a person's confirmation
    /// for each call (behaviour 19).
    #[serde(rename = "ticket-write")]
    TicketWrite,
}

impl Grant {
    /// Name as written in the grant file.
    pub fn name(self) -> &'static str {
        match self {
            Grant::Run => "run",
            Grant::Http => "http",
            Grant::Debug => "debug",
            Grant::Remote => "remote",
            Grant::Ssh => "ssh",
            Grant::Trust => "trust",
            Grant::Write => "write",
            Grant::VcsWrite => "vcs-write",
            Grant::Quit => "quit",
            Grant::TerminalRead => "terminal-read",
            Grant::Ports => "ports",
            Grant::Ui => "ui",
            Grant::SecretRead => "secret-read",
            Grant::Settings => "settings",
            Grant::RecentProjects => "recent-projects",
            Grant::Agents => "agents",
            Grant::Vault => "vault",
            Grant::TicketWrite => "ticket-write",
        }
    }

    /// What the right allows, in words for the question in the window and refusals.
    pub fn meaning(self) -> &'static str {
        match self {
            Grant::Run => "start programs: run configurations and commands",
            Grant::Http => "send HTTP requests",
            Grant::Debug => "start debug sessions",
            Grant::Remote => "fetch, pull and push Git remotes",
            Grant::Ssh => "reach SSH hosts and their files",
            Grant::Trust => "trust the project, so its code may run",
            Grant::Write => "create, change, move or delete files of the workspace",
            Grant::VcsWrite => {
                "change the repository: commit, branch, switch, merge, stash, rebase, pull, push"
            }
            Grant::Quit => "quit Ferris IDEA or close a project's window",
            Grant::TerminalRead => "read the output of the terminal tabs, with secrets masked",
            Grant::Ports => "forward ports of a remote project's host to this computer's loopback",
            Grant::Ui => {
                "operate the Ferris IDEA window: read its interface, click, type and press keys in it"
            }
            Grant::SecretRead => {
                "read files that commonly hold secrets (.env, keys, credentials) and the private values of the HTTP client"
            }
            Grant::Settings => {
                "change your own Ferris IDEA settings, which hold for every project: IDE settings, keymap, theme, plugin grants"
            }
            Grant::RecentProjects => {
                "read the folders of the other projects you opened recently in Ferris IDEA"
            }
            Grant::Agents => {
                "start the agents you configured for Ferris IDEA's agent host, send them prompts, read their output, answer their permission requests and stop them"
            }
            Grant::Vault => {
                "store and generate credentials in Ferris IDEA's vault and list or remove its entries, never read a value; each use is approved by you"
            }
            Grant::TicketWrite => {
                "create tickets, comment on them and move them along; closing, cancelling, priority and other people's text still need your confirmation each time"
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantFile {
    #[serde(default)]
    workspace: Vec<WorkspaceGrant>,
    /// Database permissions per connection (`SPEC-0012` v3 behaviour 1).
    #[serde(default)]
    connection: Vec<ConnectionEntry>,
}

/// One `[[connection]]` table of the grant file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionEntry {
    workspace: PathBuf,
    connection: String,
    target: String,
    #[serde(default)]
    allow: Vec<String>,
}

/// The names of the four database permissions of a connection (`SPEC-0012` behaviour 1), as
/// written in the grant file.
pub const CONNECTION_PERMISSIONS: [&str; 4] =
    ["read-schema", "read-data", "modify-data", "modify-schema"];

/// What the user granted agents on one database connection of a workspace (`SPEC-0012` v3
/// behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionGrant {
    /// Id of the connection in the project's connection or store file.
    pub connection: String,
    /// The target the grant was given for; it applies only while the connection still has it.
    pub target: String,
    /// Granted permissions, each one of [`CONNECTION_PERMISSIONS`].
    pub allow: BTreeSet<String>,
}

/// The database grants of one workspace and where they came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionGrants {
    /// One entry per `[[connection]]` table of this workspace.
    pub grants: Vec<ConnectionGrant>,
    /// The grant file consulted (`None`: no settings folder known).
    pub file: Option<PathBuf>,
    /// Problem reading the file, if any; nothing is granted then.
    pub problem: Option<String>,
}

impl ConnectionGrants {
    /// The permissions granted on `connection` while it has `target`.
    pub fn allowed(&self, connection: &str, target: &str) -> BTreeSet<String> {
        self.grants
            .iter()
            .filter(|g| g.connection == connection && g.target == target)
            .flat_map(|g| g.allow.iter().cloned())
            .collect()
    }
}

/// Whether the grant `file` lies inside the workspace `root` after resolving links (such a file
/// is ignored and never written).
pub(crate) fn inside_workspace(file: &Path, root: &Path) -> bool {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    // A file that does not exist yet is placed by its folder, resolved as far as it exists.
    let placed = file
        .ancestors()
        .find_map(|a| {
            let resolved = std::fs::canonicalize(a).ok()?;
            Some(resolved.join(file.strip_prefix(a).ok()?))
        })
        .unwrap_or_else(|| file.to_path_buf());
    placed.starts_with(&root)
}

/// Reads the database grants of `root` from the grant `file` (`SPEC-0012` v3 behaviour 1):
/// `[[connection]]` tables whose `workspace` is `root` after resolving links. A file inside the
/// workspace is ignored; a malformed file or an unknown permission name grants nothing and the
/// problem is reported.
pub fn connection_grants(file: &Path, root: &Path) -> ConnectionGrants {
    let mut out = ConnectionGrants {
        file: Some(file.to_path_buf()),
        ..ConnectionGrants::default()
    };
    if inside_workspace(file, root) {
        out.problem = Some("the grant file lies inside the workspace and is ignored".into());
        return out;
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.problem = Some(e.to_string());
            return out;
        }
    };
    let parsed = match toml::from_str::<GrantFile>(&text) {
        Ok(f) => f,
        Err(e) => {
            out.problem = Some(e.to_string());
            return out;
        }
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let root = canon(root);
    for entry in &parsed.connection {
        if let Some(unknown) = entry
            .allow
            .iter()
            .find(|a| !CONNECTION_PERMISSIONS.contains(&a.as_str()))
        {
            out.grants.clear();
            out.problem = Some(format!(
                "unknown database permission `{unknown}` for connection `{}` (known: {})",
                entry.connection,
                CONNECTION_PERMISSIONS.join(", ")
            ));
            return out;
        }
        if canon(&entry.workspace) == root {
            out.grants.push(ConnectionGrant {
                connection: entry.connection.clone(),
                target: entry.target.clone(),
                allow: entry.allow.iter().cloned().collect(),
            });
        }
    }
    out
}

/// The `[[connection]]` table that grants `allow` on `connection` with `target` in the workspace
/// `root`, as text for the grant file (and for the refusal that names it).
pub fn connection_grant_entry(
    root: &Path,
    connection: &str,
    target: &str,
    allow: &[&str],
) -> String {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let text = |s: &str| toml::Value::String(s.to_owned()).to_string();
    let allow = allow.iter().map(|a| text(a)).collect::<Vec<_>>().join(", ");
    format!(
        "[[connection]]\nworkspace = {}\nconnection = {}\ntarget = {}\nallow = [{allow}]\n",
        text(&root.to_string_lossy()),
        text(connection),
        text(target),
    )
}

/// Adds `allow` on `connection` with `target` for the workspace `root` to the grant `file`, as
/// the user's answer "Always for This Connection" in the window does (`SPEC-0012` v3 behaviour
/// 9): a new `[[connection]]` entry is appended, the rest of the file stays as it was. Nothing is
/// written when everything is granted already; a file inside the workspace and an unknown
/// permission are refused.
pub fn add_connection_grant(
    file: &Path,
    root: &Path,
    connection: &str,
    target: &str,
    allow: &[&str],
) -> Result<(), String> {
    if let Some(unknown) = allow.iter().find(|a| !CONNECTION_PERMISSIONS.contains(a)) {
        return Err(format!("unknown database permission `{unknown}`"));
    }
    let current = connection_grants(file, root);
    if let Some(p) = &current.problem {
        return Err(format!("{}: {p}", file.display()));
    }
    let have = current.allowed(connection, target);
    let missing: Vec<&str> = allow
        .iter()
        .copied()
        .filter(|a| !have.contains(*a))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&connection_grant_entry(root, connection, target, &missing));
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// The line that names what a database connection reaches (`SPEC-0012` v3, "Target"): `shown`
/// (its URL without password), `#` and 16 hexadecimal digits of a SHA-256 over `parts` —
/// everything that decides where the connection leads and what runs on connecting. Two
/// connections have the same target only when every part is the same.
pub fn connection_target(shown: &str, parts: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let digest = hash.finalize();
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{shown}#{hex}")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceGrant {
    path: PathBuf,
    #[serde(default)]
    allow: Vec<Grant>,
}

/// The grants for one workspace and where they came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AgentPermissions {
    /// Granted rights.
    pub allow: BTreeSet<Grant>,
    /// The grant file consulted (`None`: no settings folder known).
    pub file: Option<PathBuf>,
    /// Problem reading the file, if any; nothing is granted then.
    pub problem: Option<String>,
}

impl AgentPermissions {
    /// Whether `grant` is given.
    pub fn allows(&self, grant: Grant) -> bool {
        self.allow.contains(&grant)
    }

    /// Explanation for a refused action, naming the exact entry to add (behaviour 3). It says
    /// "not granted" and that nobody was asked, never that the user rejected it (IMP-0007).
    pub fn refusal(&self, grant: Grant, root: &Path) -> String {
        let file = self
            .file
            .as_ref()
            .map(|f| f.display().to_string())
            .unwrap_or_else(|| format!("<settings folder>/{GRANT_FILE}"));
        let mut text = format!(
            "not granted: this workspace does not grant agents the `{}` right ({}); nobody was asked. Only the user can grant it: answer \"Always for This Workspace\" when the Ferris IDEA window asks a client of its MCP server over HTTP, or add to {file}:\n\n[[workspace]]\npath = \"{}\"\nallow = [\"{}\"]\n\nUse dry_run to see what would happen.",
            grant.name(),
            grant.meaning(),
            root.display(),
            grant.name()
        );
        if let Some(p) = &self.problem {
            text.push_str(&format!("\n(The grant file could not be read: {p})"));
        }
        text
    }
}

/// The user's Ferris IDEA settings folder: `$FERRIS_IDEA_CONFIG_DIR`, else `$XDG_CONFIG_HOME/ferris-idea`,
/// else `~/.config/ferris-idea`.
/// The variable's former name is read while it is not set (`SPEC-0356` behaviour 6).
pub fn user_config_dir() -> Option<PathBuf> {
    if let Some(d) = crate::product_var_os("FERRIS_IDEA_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    crate::takeover::config_home().map(|h| h.join(crate::takeover::CONFIG_DIR_NAME))
}

/// Reads the grants of `root` from `file` (behaviour 2). Paths are compared after resolving links; a
/// grant file inside the workspace is ignored.
pub fn load_agent_permissions(file: &Path, root: &Path) -> AgentPermissions {
    let mut out = AgentPermissions {
        file: Some(file.to_path_buf()),
        ..AgentPermissions::default()
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let inside = inside_workspace(file, root);
    let root = canon(root);
    if inside {
        out.problem = Some("the grant file lies inside the workspace and is ignored".into());
        return out;
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.problem = Some(e.to_string());
            return out;
        }
    };
    match toml::from_str::<GrantFile>(&text) {
        Ok(f) => {
            for w in f.workspace {
                if canon(&w.path) == root {
                    out.allow.extend(w.allow);
                }
            }
        }
        Err(e) => out.problem = Some(e.to_string()),
    }
    out
}

/// Adds `grant` for the workspace `root` to the grant `file`, as the user's answer "Always for This
/// Workspace" in the window does (`SPEC-0053` v4 behaviour 8): a new `[[workspace]]` entry is appended,
/// so the rest of the file (comments included) stays as it was. Nothing is written when the right is
/// granted already; a file inside the workspace is refused.
pub fn add_grant(file: &Path, root: &Path, grant: Grant) -> Result<(), String> {
    let current = load_agent_permissions(file, root);
    if let Some(p) = &current.problem {
        return Err(format!("{}: {p}", file.display()));
    }
    if current.allows(grant) {
        return Ok(());
    }
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    let path = toml::Value::String(root.to_string_lossy().into_owned());
    text.push_str(&format!(
        "[[workspace]]\npath = {path}\nallow = [\"{}\"]\n",
        grant.name()
    ));
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// Grants of `root` from the user's settings folder.
pub fn agent_permissions(root: &Path) -> AgentPermissions {
    match user_config_dir() {
        Some(dir) => load_agent_permissions(&dir.join(GRANT_FILE), root),
        None => AgentPermissions::default(),
    }
}

/// Appends one JSON line describing an action an agent took to `.ferris-idea/agent-log.jsonl`
/// (behaviour 5). Failures to log are returned, not hidden.
pub fn log_agent_action(root: &Path, entry: &serde_json::Value) -> std::io::Result<()> {
    // The log is machine-local: it stays out of version control (`SPEC-0069` behaviour 3).
    crate::local_state::refresh_local_state_ignore(root);
    let path = root.join(AGENT_LOG);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

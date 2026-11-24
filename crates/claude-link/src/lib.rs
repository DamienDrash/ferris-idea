//! The Claude Code link (`SPEC-0358`): when the user runs Anthropic's `claude` CLI in a Ferris IDEA
//! terminal, Claude Code connects to Ferris IDEA as its IDE. It sees the selection and the open
//! files, reads the diagnostics, and shows its proposed changes as diffs in the window, which the
//! user accepts or rejects.
//!
//! One [`LinkServer`] serves one project window. The process-wide hub ([`install`],
//! [`set_projects`], [`terminal_env`], [`ensure_started`]) starts a link lazily, the first time a
//! terminal of the project opens or another part of Ferris IDEA asks for it, and stops it when the
//! project window closes. Toolkit-free: the window implements [`Editor`].
//!
//! The protocol is the one Claude Code speaks with IDE integrations, as documented by the openly
//! licensed `claudecode.nvim` (MIT) and Anthropic's public documentation; the sources are listed
//! in the specification. The code here is written anew.

pub mod editor;
pub mod lockfile;
pub mod paths;
pub mod server;
pub mod tools;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub use editor::{
    Diagnostic, DiffDecision, DiffProposal, Editor, OpenEditor, OpenRequest, Position, Selection,
    Severity,
};
pub use server::{ClientState, LinkConfig, LinkServer, LinkState};

/// The name Claude Code shows for Ferris IDEA, written into the lock file.
pub const IDE_NAME: &str = "Ferris IDEA";

/// The variables a terminal of a linked project gets (behaviour 2): the port Claude Code connects
/// to and the switch that turns its IDE integration on.
pub const PORT_VARIABLE: &str = "CLAUDE_CODE_SSE_PORT";
/// See [`PORT_VARIABLE`].
pub const ENABLE_VARIABLE: &str = "ENABLE_IDE_INTEGRATION";

/// What the hub needs to start links.
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// The folder of the lock files (behaviour 3).
    pub lock_dir: PathBuf,
    /// The version Ferris IDEA reports.
    pub ide_version: String,
}

#[derive(Default)]
struct Hub {
    config: Option<HubConfig>,
    editor: Option<Arc<dyn Editor>>,
    projects: Vec<PathBuf>,
    links: HashMap<PathBuf, LinkServer>,
}

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    static HUB: OnceLock<Mutex<Hub>> = OnceLock::new();
    HUB.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Lets this process start links (the window does it once). Without it, [`terminal_env`] gives
/// nothing and [`ensure_started`] refuses.
pub fn install(config: HubConfig, editor: Arc<dyn Editor>) {
    let mut h = hub();
    h.config = Some(config);
    h.editor = Some(editor);
}

/// Whether [`install`] ran.
pub fn installed() -> bool {
    hub().config.is_some()
}

/// The open project folders. Links of folders no longer open stop (behaviour 5).
pub fn set_projects(roots: Vec<PathBuf>) {
    let roots: Vec<PathBuf> = roots
        .into_iter()
        .map(|r| std::fs::canonicalize(&r).unwrap_or(r))
        .collect();
    let stopped: Vec<LinkServer> = {
        let mut h = hub();
        h.projects = roots.clone();
        let gone: Vec<PathBuf> = h
            .links
            .keys()
            .filter(|k| !roots.contains(k))
            .cloned()
            .collect();
        gone.iter().filter_map(|k| h.links.remove(k)).collect()
    };
    // Dropped outside the lock: stopping joins threads.
    drop(stopped);
}

/// Adds an open project folder (a project window opened); [`set_projects`] later keeps the list
/// in step with the windows.
pub fn add_project(root: &Path) {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut h = hub();
    if !h.projects.contains(&root) {
        h.projects.push(root);
    }
}

/// The open project whose folder holds `path` (the deepest one).
fn project_of(projects: &[PathBuf], path: &Path) -> Option<PathBuf> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    projects
        .iter()
        .filter(|p| path.starts_with(p))
        .max_by_key(|p| p.components().count())
        .cloned()
}

/// Starts the link of the open project that holds `path`, if it does not run yet, and returns its
/// port: the hook for the terminal that runs Claude Code (behaviour 2).
pub fn ensure_started(path: &Path) -> Result<u16, String> {
    let mut h = hub();
    let (Some(config), Some(editor)) = (h.config.clone(), h.editor.clone()) else {
        return Err("the Claude Code link is not available in this process".into());
    };
    let root = project_of(&h.projects, path).ok_or("the folder belongs to no open project")?;
    if let Some(link) = h.links.get(&root) {
        return Ok(link.port());
    }
    let link = LinkServer::start(
        LinkConfig {
            workspace: root.clone(),
            lock_dir: config.lock_dir,
            ide_name: IDE_NAME.into(),
            ide_version: config.ide_version,
        },
        editor,
    )
    .map_err(|e| format!("the Claude Code link could not start: {e}"))?;
    let port = link.port();
    h.links.insert(root, link);
    Ok(port)
}

/// The variables for a terminal started in `cwd` (behaviour 2): the link of its project is started
/// and named. Empty when the link is not available or cannot start; the reason goes to stderr.
pub fn terminal_env(cwd: &Path) -> Vec<(String, String)> {
    if !installed() {
        return Vec::new();
    }
    match ensure_started(cwd) {
        Ok(port) => vec![
            (PORT_VARIABLE.into(), port.to_string()),
            (ENABLE_VARIABLE.into(), "true".into()),
        ],
        Err(e) => {
            eprintln!("warning: {e}");
            Vec::new()
        }
    }
}

/// The state of the link of the project at `root`, if it runs (behaviour 23).
pub fn state(root: &Path) -> Option<LinkState> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    hub().links.get(&root).map(LinkServer::state)
}

/// Whether Claude Code is connected to the link of the project at `root`.
pub fn connected(root: &Path) -> bool {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    hub().links.get(&root).is_some_and(LinkServer::connected)
}

/// The projects whose link has a connected client, to look for their selection.
pub fn connected_projects() -> Vec<PathBuf> {
    hub()
        .links
        .iter()
        .filter(|(_, l)| l.connected())
        .map(|(r, _)| r.clone())
        .collect()
}

/// Hands the selection of the project at `root` to its link (behaviour 14).
pub fn notify_selection(root: &Path, selection: Option<Selection>) {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Some(link) = hub().links.get(&root) {
        link.notify_selection(selection);
    }
}

/// Stops every link and removes their lock files (the app quits).
pub fn shutdown() {
    let links: Vec<LinkServer> = hub().links.drain().map(|(_, l)| l).collect();
    drop(links);
}

/// The lock folder of this process's links, if [`install`] ran.
pub fn lock_dir() -> Option<PathBuf> {
    hub().config.as_ref().map(|c| c.lock_dir.clone())
}

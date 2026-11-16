//! The window's terminal tabs for readers on other threads (`SPEC-0313` behaviour 9): the MCP tool
//! `terminal_read` lists them and reads one, with the right the user granted. The window registers
//! each shell when it starts and the registration goes with the shell; nothing is read here unless
//! a reader asks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::screen::Terminal;

struct Entry {
    id: u64,
    root: PathBuf,
    name: String,
    shared: bool,
    terminal: Weak<Mutex<Terminal>>,
}

/// A terminal tab as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    /// 0-based position among the workspace's shells.
    pub index: usize,
    /// Tab name, e.g. `Local (2)`.
    pub name: String,
    /// Whether the person shares it with the AI chat.
    pub shared: bool,
    /// Whether its shell marks prompts (OSC 133).
    pub marks: bool,
}

/// Shells of the window by workspace. Cloning shares the registry.
#[derive(Clone, Default)]
pub struct TerminalRegistry {
    entries: Arc<Mutex<Vec<Entry>>>,
    next: Arc<std::sync::atomic::AtomicU64>,
}

fn key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

impl TerminalRegistry {
    /// The registry of this process, which the window publishes to.
    pub fn global() -> &'static TerminalRegistry {
        static GLOBAL: OnceLock<TerminalRegistry> = OnceLock::new();
        GLOBAL.get_or_init(TerminalRegistry::default)
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers the screen `terminal` of a shell named `name` in the workspace at `root`.
    pub fn register(
        &self,
        root: &Path,
        name: &str,
        terminal: &Arc<Mutex<Terminal>>,
    ) -> Registration {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.entries().push(Entry {
            id,
            root: key(root),
            name: name.to_owned(),
            shared: false,
            terminal: Arc::downgrade(terminal),
        });
        Registration {
            registry: self.clone(),
            id,
        }
    }

    /// The live shells of the workspace at `root`, in the order they were registered.
    fn live(&self, root: &Path) -> Vec<(u64, String, bool, Arc<Mutex<Terminal>>)> {
        let root = key(root);
        let mut entries = self.entries();
        entries.retain(|e| e.terminal.strong_count() > 0);
        entries
            .iter()
            .filter(|e| e.root == root)
            .filter_map(|e| Some((e.id, e.name.clone(), e.shared, e.terminal.upgrade()?)))
            .collect()
    }

    /// The tabs of the workspace at `root`; no content is read.
    pub fn tabs(&self, root: &Path) -> Vec<TabInfo> {
        self.live(root)
            .into_iter()
            .enumerate()
            .map(|(index, (_, name, shared, t))| TabInfo {
                index,
                name,
                shared,
                marks: t
                    .lock()
                    .map(|t| !t.screen.prompts().is_empty())
                    .unwrap_or(false),
            })
            .collect()
    }

    /// Runs `read` on the screen of the tab at `index`, or of the first tab named `name`.
    pub fn read<R>(
        &self,
        root: &Path,
        tab: &str,
        read: impl FnOnce(&TabInfo, &Terminal) -> R,
    ) -> Option<R> {
        let live = self.live(root);
        let index = tab
            .parse::<usize>()
            .ok()
            .filter(|i| *i < live.len())
            .or_else(|| live.iter().position(|(_, n, _, _)| n == tab))?;
        let (_, name, shared, t) = &live[index];
        let t = t.lock().ok()?;
        let info = TabInfo {
            index,
            name: name.clone(),
            shared: *shared,
            marks: !t.screen.prompts().is_empty(),
        };
        Some(read(&info, &t))
    }

    fn update(&self, id: u64, change: impl FnOnce(&mut Entry)) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.id == id) {
            change(e);
        }
    }
}

/// A shell's registration: renames and sharing go through it; dropping it unregisters the shell.
pub struct Registration {
    registry: TerminalRegistry,
    id: u64,
}

impl Registration {
    /// The tab was renamed.
    pub fn set_name(&self, name: &str) {
        self.registry.update(self.id, |e| e.name = name.to_owned());
    }

    /// The person shared the tab with the chat or stopped sharing it.
    pub fn set_shared(&self, shared: bool) {
        self.registry.update(self.id, |e| e.shared = shared);
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let id = self.id;
        self.registry.entries().retain(|e| e.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_by_workspace_name_and_index_and_they_go_with_the_shell() {
        let reg = TerminalRegistry::default();
        let a = Arc::new(Mutex::new(Terminal::new(3, 20)));
        let b = Arc::new(Mutex::new(Terminal::new(3, 20)));
        let other = Arc::new(Mutex::new(Terminal::new(3, 20)));
        let ra = reg.register(Path::new("/ws"), "Local", &a);
        let rb = reg.register(Path::new("/ws"), "Local (2)", &b);
        let _ro = reg.register(Path::new("/elsewhere"), "Local", &other);
        b.lock()
            .unwrap()
            .feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07x\r\n\x1b]133;D;0\x07");
        rb.set_shared(true);
        let tabs = reg.tabs(Path::new("/ws"));
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            (tabs[1].name.as_str(), tabs[1].shared, tabs[1].marks),
            ("Local (2)", true, true)
        );
        assert!(!tabs[0].marks);
        ra.set_name("build");
        let named = reg.read(Path::new("/ws"), "build", |i, _| i.index);
        assert_eq!(named, Some(0));
        let by_index = reg.read(Path::new("/ws"), "1", |i, t| {
            (i.name.clone(), t.screen.prompts().len())
        });
        assert_eq!(by_index, Some(("Local (2)".to_owned(), 1)));
        assert!(reg.read(Path::new("/ws"), "nope", |_, _| ()).is_none());
        drop(rb);
        assert_eq!(reg.tabs(Path::new("/ws")).len(), 1);
        // A shell that went away without its registration is not listed.
        drop(a);
        assert!(reg.tabs(Path::new("/ws")).is_empty());
    }
}

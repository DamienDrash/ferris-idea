//! Integrated terminal for IDECraft.
//!
//! Implements `SPEC-0038`: a program in a pseudo terminal (`portable-pty`)
//! and a screen model fed by the `vte` escape-sequence parser; `SPEC-0062`
//! (terminal tool window) for the bytes keys send; `SPEC-0205` (saved tabs) and `SPEC-0206`
//! (search, prompt marks, shell integration); the last command and the registry of tabs for the AI
//! chat and MCP (`SPEC-0313`).

pub mod integration;
mod keys;
mod layout;
mod links;
mod mouse;
mod registry;
mod saved;
mod screen;
mod search;
mod selection;
mod session;

pub use keys::{key_bytes, paste_bytes};
pub use layout::{cell_runs, split_position};
pub use links::{Link, LinkTarget, find_links, line_links};
pub use mouse::{MouseAction, MouseButton, MouseInput, mouse_bytes};
pub use registry::{Registration, TabInfo, TerminalRegistry};
pub use saved::{SAVED_FILE, SAVED_LINES, SavedShell, SavedTab, SavedTerminals, next_tab_name};
pub use screen::{
    Cell, Color, DEFAULT_SCROLLBACK, LastCommand, MouseEncoding, MouseMode, Prompt, Screen, Style,
    Terminal,
};
pub use search::{Match, search};
pub use selection::{Point, Selection, SelectionUnit};
pub use session::{LinkRead, PtyLink, Session, SessionError, SpawnOptions};

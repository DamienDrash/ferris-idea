//! The jdtls tests that ask one project many questions, through the language server pool as the
//! window and `ferris-idea mcp` ask them: one jdtls per project instead of one per question. A test
//! program of its own beside `it` (ADR-0030 item 7), because the pool is switched on for the whole
//! process (see `it/main.rs`). Each test stops its project's servers at the end, so the tests of
//! this program do not see each other's servers. On the shared CI server a jdtls start took 15 to
//! 27 s, and Inline and Extract asked through a server per question ran past the five minutes a
//! test may take there.

use std::path::Path;

mod java_refactor;
mod java_server;
mod java_structure;
#[path = "../it/server_case.rs"]
mod server_case;

/// Switches the language server pool on for this process.
pub fn pooled() {
    ferris_idea_languages::pool::enable();
}

/// Stops the pool's servers of the workspace `root`.
pub fn done(root: &Path) {
    ferris_idea_languages::pool::restart_root(root);
}

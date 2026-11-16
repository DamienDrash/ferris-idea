//! F-020 · Editor tabs and split views: several files, split horizontally/vertically (SPEC-0059,
//! SPEC-0109).
//!
//! self-test: opening tree and tab steps, split_steps, split_move_steps
//!
//! Tabs and splits exist only in a window. Headless, the agent-visible part is the list of open
//! documents the window publishes over the HTTP transport (SPEC-0220); without a window it is empty.

use crate::harness::*;
use serde_json::json;

#[test]
fn without_a_window_no_document_is_open() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::http(&p);
    let docs = ide.ok("open_documents", json!({}));
    assert_eq!(text(&docs).matches(".py").count(), 0, "{docs}");
}

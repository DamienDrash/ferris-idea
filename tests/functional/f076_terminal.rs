//! F-076 · Integrated terminal: a shell in the project folder (SPEC-0038, SPEC-0205, SPEC-0207).
//!
//! self-test: terminal_session_steps (tabs, split, restore in a window)

use crate::harness::*;
use serde_json::json;

#[test]
fn a_command_line_runs_in_the_project_folder_as_the_terminal_shows_it() {
    let p = Project::empty();
    p.write("here.txt", "x").grant(&["run"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "terminal_command",
        json!({"command": "ls *.txt && printf '\\033[31mred\\033[0m\\rR'", "dry_run": false}),
    );
    assert_eq!(r["exit_code"], 0, "{r}");
    let out = text(&r["output"]);
    assert!(out.contains("here.txt"), "{out}");
    // Colours stripped, `\r` overwrites the line as a terminal does.
    assert!(out.contains("Red") && !out.contains('\u{1b}'), "{out:?}");
}

#[test]
fn without_the_run_right_only_a_preview() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let preview = ide.ok("terminal_command", json!({"command": "touch made.txt"}));
    assert_eq!(preview["dry_run"], true);
    ide.refused(
        "terminal_command",
        json!({"command": "touch made.txt", "dry_run": false}),
    );
    assert!(!p.path("made.txt").exists());
}

#[test]
fn escape_sequences_render_like_xterm() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "terminal_render",
        json!({"output": "\u{1b}]0;Title\u{7}abc\r\nde\u{1b}[1Df", "rows": 5, "cols": 20}),
    );
    let all = text(&r);
    assert!(all.contains("Title"), "{r}");
    assert!(all.contains("abc") && all.contains("df"), "{r}");
}

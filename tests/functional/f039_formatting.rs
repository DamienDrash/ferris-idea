//! F-039 · Code formatting: format by style rules (SPEC-0043 AK, SPEC-0075, SPEC-0329, IMP-0020).

use crate::harness::*;
use serde_json::json;

#[test]
fn preview_then_apply_a_format() {
    let p = Project::empty();
    p.write("m.c", "int add(int a,int b){return a+b;}\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.call("format_file", json!({"path": "m.c"}));
    if lsp_unavailable(&r) {
        return;
    }
    let r = r.unwrap();
    assert_eq!(r["applied"], false);
    assert_eq!(
        p.read("m.c"),
        "int add(int a,int b){return a+b;}\n",
        "preview writes nothing"
    );
    ide.ok("format_file", json!({"path": "m.c", "dry_run": false}));
    assert!(
        p.read("m.c").contains("int add(int a, int b)"),
        "{}",
        p.read("m.c")
    );
}

/// SPEC-0329: a Python file goes to the project's ruff or black, not to its language server;
/// without one the answer says what to install.
#[test]
fn python_is_formatted_by_an_installed_formatter_or_says_what_to_install() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    match ide.call("format_file", json!({"path": "src/inventory/model.py"})) {
        Ok(r) => {
            assert_eq!(r["applied"], false, "{r}");
            assert!(
                matches!(r["formatter"].as_str(), Some("ruff" | "black")),
                "{r}"
            );
        }
        Err(e) => assert!(
            e.contains("No ruff or black found") && e.contains("installs nothing itself")
                || e.starts_with("Safe mode: ruff or black"),
            "{e}"
        ),
    }
}

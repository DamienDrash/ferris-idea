//! F-038 · PHP, Go, Ruby, C/C++, HTML, CSS, Rust: language services over LSP (SPEC-0042, IMP-0019).

use crate::harness::*;
use serde_json::json;

#[test]
fn every_language_names_its_server_or_why_none_is_usable() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let servers = ide.ok("language_servers", json!({}));
    for lang in ["php", "go", "ruby", "c", "cpp", "html", "css", "rust"] {
        let entry = servers
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["language"] == lang)
            .unwrap_or_else(|| panic!("{lang} missing"));
        for c in entry["candidates"].as_array().unwrap() {
            let state = text(&c["availability"]["state"]);
            if state != "ready" {
                // A missing server says how to install it.
                assert!(!text(&c["install"]).is_empty(), "{lang}: {c}");
            }
        }
    }
}

#[test]
fn c_diagnostics_from_clangd() {
    let p = Project::empty();
    p.write("m.c", "int main(void) { return nope; }\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.call(
        "file_diagnostics",
        json!({"path": "m.c", "timeout_seconds": 120}),
    );
    if lsp_unavailable(&r) {
        return;
    }
    assert!(text(&r.unwrap()).contains("nope"));
}

#[test]
fn php_symbols_and_ruby_environment() {
    let p = Project::reference("php-basic");
    p.write(".ruby-version", "3.3.0\n");
    let mut ide = Ide::stdio(&p);
    let syms = ide.call("document_symbols", json!({"path": "src/Inventory.php"}));
    if !lsp_unavailable(&syms) {
        assert!(text(&syms.unwrap()).contains("Inventory"));
    }
    let ruby = ide.ok("ruby_environment", json!({}));
    assert!(
        text(&ruby).contains("3.3.0"),
        "requested version from .ruby-version: {ruby}"
    );
}

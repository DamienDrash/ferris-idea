//! F-080 · Plugin system: install extensions, sandbox (SPEC-0104, SPEC-0126, SPEC-0210, SPEC-0211).

use crate::harness::*;
use serde_json::json;

fn sample(name: &str) -> std::path::PathBuf {
    reference_projects().join("../plugins").join(name)
}

#[test]
fn installed_plugins_are_listed_and_their_actions_run_in_the_sandbox() {
    let p = Project::empty();
    p.write("a.txt", "hello\n");
    std::fs::create_dir_all(p.config.join("plugins")).unwrap();
    std::fs::copy(
        sample("complete.wasm"),
        p.config.join("plugins/complete.wasm"),
    )
    .unwrap();
    let mut ide = Ide::stdio(&p);
    let list = ide.ok("plugins", json!({}));
    let plugin = &list["plugins"][0];
    assert_eq!(plugin["name"], "Words", "{list}");
    let action = text(&plugin["actions"][0]["id"]);
    let r = ide.ok(
        "run_plugin_action",
        json!({"action": action, "path": "a.txt", "offset": 2}),
    );
    assert!(text(&r).contains("plugin_hello"), "{r}");
}

#[test]
fn a_workspace_plugin_file_is_inspected_but_not_installed() {
    let p = Project::empty();
    std::fs::copy(sample("status.wasm"), p.path("status.wasm")).unwrap();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("inspect_plugin_file", json!({"path": "status.wasm"}));
    assert_eq!(r["review"]["manifest"]["name"], "Line Count", "{r}");
    // A project cannot bring its own plugins.
    let list = ide.ok("plugins", json!({}));
    assert_eq!(list["plugins"], json!([]));
}

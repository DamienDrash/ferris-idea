//! F-036 · JavaScript/TypeScript and frameworks: Node, npm, frameworks (SPEC-0051, SPEC-0133,
//! SPEC-0168).

use crate::harness::*;
use serde_json::json;

#[test]
fn node_runtime_package_manager_and_missing_packages() {
    let p = Project::reference("react-basic");
    let mut ide = Ide::stdio(&p);
    let env = ide.ok("node_environment", json!({}));
    assert_eq!(env["package_manager"]["name"], "npm", "{env}");
    assert_eq!(env["installed"], false, "{env}");
    assert!(text(&env["unsatisfied"]).contains("react"), "{env}");
}

#[test]
fn package_scripts_get_run_configurations() {
    let p = Project::reference("react-basic");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("package_scripts", json!({}));
    let dev = r["scripts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "dev")
        .unwrap_or_else(|| panic!("{r}"));
    assert_eq!(dev["run"]["command"], json!(["npm", "run", "dev"]));
}

#[test]
fn typescript_errors_come_from_the_language_server() {
    let p = Project::reference("typescript-basic");
    p.write("src/bad.ts", "const n: number = \"x\";\nexport {};\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.call(
        "file_diagnostics",
        json!({"path": "src/bad.ts", "timeout_seconds": 120}),
    );
    if lsp_unavailable(&r) {
        return;
    }
    let mut r = r.unwrap();
    // Known weakness (PO-STATUS 2026-11-03, M7.4): under load typescript-language-server first
    // publishes an empty set and the tool returns it; asking again gets the real check.
    for _ in 0..5 {
        if r != json!([]) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        r = ide.ok(
            "file_diagnostics",
            json!({"path": "src/bad.ts", "timeout_seconds": 120}),
        );
    }
    assert_eq!(r[0]["severity"], "error", "{r}");
    assert_eq!(r[0]["start_line"], 0);
}

/// SPEC-0045 v4 AK-12: every test and suite of a Vitest file runs alone, with the runner's filter.
#[test]
fn tests_of_a_vitest_file_get_run_markers() {
    let p = Project::empty();
    p.write(
        "package.json",
        r#"{"name": "x", "devDependencies": {"vitest": "2.1.5"}}"#,
    )
    .write(
        "test/util.test.ts",
        "import { describe, it } from \"vitest\";\ndescribe(\"math\", () => {\n  it(\"adds (x)\", () => {});\n});\n",
    );
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("run_markers", json!({"path": "test/util.test.ts"}));
    let markers = r["markers"].as_array().expect("markers");
    assert_eq!(markers.len(), 2, "{r}");
    assert_eq!(markers[0]["line"], 2);
    assert_eq!(markers[0]["kind"], "test_class");
    assert_eq!(markers[1]["class"], "math adds (x)");
    let command: Vec<&str> = markers[1]["run"]["command"]
        .as_array()
        .expect("command")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(command[..3], ["npx", "vitest", "run"], "{r}");
    assert_eq!(
        command[command.len() - 2..],
        ["-t", "math adds \\(x\\)$"],
        "{r}"
    );
    assert_eq!(markers[1]["run"]["kind"], "test", "{r}");
    assert_eq!(
        markers[0]["run"]["command"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "math "
    );
}

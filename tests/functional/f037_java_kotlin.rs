//! F-037 · Java/Kotlin: build, run, navigation, inspections (SPEC-0052, SPEC-0097, SPEC-0125,
//! SPEC-0132, IMP-0027).

use crate::harness::*;
use serde_json::json;

#[test]
fn jdks_and_the_projects_java_version_are_read_statically() {
    let p = Project::reference("spring-basic");
    let mut ide = Ide::stdio(&p);
    let env = ide.ok("java_environment", json!({}));
    let req = &env["requirements"][0];
    assert_eq!(req["feature"], 21, "{env}");
    assert_eq!(req["source"], "pom.xml");
    // Either a JDK 21+ is chosen or the mismatch is named.
    if env["compatible"] == false {
        assert!(!env["problems"].as_array().unwrap().is_empty(), "{env}");
    }
}

#[test]
fn main_classes_get_run_markers() {
    let p = Project::reference("java-basic");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "run_markers",
        json!({"path": "src/main/java/lab/inventory/Inventory.java"}),
    );
    let main = r["markers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "main")
        .unwrap_or_else(|| panic!("{r}"));
    assert_eq!(main["class"], "Inventory");
}

#[test]
fn getters_are_generated_and_imports_optimised() {
    let p = Project::empty();
    p.write(
        "src/A.java",
        "import java.util.List;\nimport java.util.Map;\nclass A {\n    private int count;\n    List<String> names;\n}\n",
    );
    let mut ide = Ide::stdio(&p);
    let generated = ide.ok(
        "generate_code",
        json!({"path": "src/A.java", "line": 3, "kind": "getters"}),
    );
    assert!(text(&generated).contains("getCount"), "{generated}");
    let opt = ide.ok("optimize_imports", json!({"path": "src/A.java"}));
    assert_eq!(opt["removed"], json!(["import java.util.Map;"]), "{opt}");
    assert!(
        p.read("src/A.java").contains("import java.util.Map;"),
        "preview writes nothing"
    );
}

#[test]
fn kotlin_has_main_markers() {
    let p = Project::empty();
    p.write("src/Main.kt", "fun main() {\n    println(\"hi\")\n}\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("run_markers", json!({"path": "src/Main.kt"}));
    assert!(
        r["markers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "main"),
        "{r}"
    );
}

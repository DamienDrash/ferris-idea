//! F-003 · Project model from build files: modules, dependencies and SDK from the build description
//! (SPEC-0048 AK-2, AK-5).

use crate::harness::*;
use serde_json::json;

#[test]
fn maven_project_gives_module_and_dependencies() {
    let p = Project::reference("spring-basic");
    let mut ide = Ide::stdio(&p);
    let model = ide.ok("project_model", json!({}));
    let modules = model["modules"].as_array().unwrap();
    assert_eq!(modules.len(), 1, "{model}");
    let all = text(&model);
    assert!(all.contains("spring-boot-starter-web"), "{all}");
    assert!(all.to_lowercase().contains("maven"), "{all}");
}

#[test]
fn npm_and_python_projects_are_read_and_a_broken_file_is_a_problem() {
    let p = Project::reference("react-basic");
    p.write(
        "tools/pyproject.toml",
        "[project]\nname = \"tools\"\nversion = \"1.0\"\ndependencies = [\"requests>=2\"]\n",
    )
    .write("broken/package.json", "{ not json");
    let mut ide = Ide::stdio(&p);
    let model = ide.ok("project_model", json!({}));
    let all = text(&model);
    assert!(all.contains("react"), "{all}");
    assert!(all.contains("requests"), "{all}");
    assert!(!model["problems"].as_array().unwrap().is_empty(), "{model}");
}

//! F-001 · Open a folder as a project: the folder becomes the project, its file tree leaves out build
//! and VCS folders (SPEC-0001 AK-1, AK-2, AK-4).

use crate::harness::*;
use serde_json::json;

#[test]
fn folder_lists_sorted_relative_files_without_build_and_vcs_folders() {
    let p = Project::reference("python-basic");
    p.write("target/debug/out.py", "x = 1\n")
        .write("node_modules/pkg/index.js", "x\n")
        .write("zz.py", "z = 1\n");
    p.git_repo();
    let mut ide = Ide::stdio(&p);
    let files = ide.ok("list_files", json!({}));
    assert_eq!(
        files,
        json!([
            "src/inventory/__init__.py",
            "src/inventory/model.py",
            "test_model.py",
            "zz.py"
        ])
    );
    let info = ide.ok("workspace_info", json!({}));
    assert_eq!(info["root"], json!(p.root.display().to_string()));
    assert_eq!(info["languages"]["python"], 4);
}

#[test]
fn cli_opens_a_folder_and_refuses_a_file_or_missing_path() {
    let p = Project::reference("python-basic");
    let out = p.cli(&["open", p.root.to_str().unwrap()]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("files:     3"), "{text}");
    assert!(text.contains("settings:  defaults"), "{text}");

    let file = p.path("test_model.py");
    assert!(!p.cli(&["open", file.to_str().unwrap()]).status.success());
    let missing = p.path("does-not-exist");
    assert!(!p.cli(&["open", missing.to_str().unwrap()]).status.success());
}

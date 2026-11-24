//! SPEC-0042 v24 AK-46 (OBS-0075, F-011): files a refactoring wrote on disk reach the pooled
//! server at once, so a second rename right after the first still finds the usages in the other
//! file. Before v24 a pooled pyright kept the other file's old text, and the second rename changed
//! only the file it started from. A test binary of its own, because the pool is switched on for
//! the whole process. Skipped without pyright or basedpyright.

use std::time::Duration;

use ferris_idea_languages::pool;
use ferris_idea_languages::rename::{apply_edits, plan_rename_in};

#[tokio::test]
async fn a_second_rename_sees_what_the_first_wrote() {
    let python = ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Python)
        .ok()
        .filter(|r| ferris_idea_languages::python_server::is_python_server(r.candidate.program));
    if python.is_none() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: pyright-langserver"
        );
        eprintln!("skipped: no pyright or basedpyright installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-py-written-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let model = root.join("model.py");
    let usage = root.join("use.py");
    std::fs::write(&model, "def helper(x):\n    return x * 2\n").unwrap();
    std::fs::write(&usage, "from model import helper\n\nprint(helper(3))\n").unwrap();
    pool::enable();
    let wait = Duration::from_secs(60);
    for (old, new) in [("helper", "aa"), ("aa", "bb")] {
        let plan = plan_rename_in(&root, &model, None, 0, 5, new, wait)
            .await
            .unwrap();
        assert_eq!(plan.old_name, old);
        assert!(
            plan.files.iter().any(|f| f.path == usage),
            "renaming {old} to {new} changes use.py too: {:?}",
            plan.files
        );
        let mut written = Vec::new();
        for file in &plan.files {
            let before = std::fs::read_to_string(&file.path).unwrap();
            std::fs::write(&file.path, apply_edits(&before, &file.edits).unwrap()).unwrap();
            written.push(file.path.clone());
        }
        let paths: Vec<&std::path::Path> = written.iter().map(|p| p.as_path()).collect();
        pool::files_written(&root, &paths);
    }
    assert_eq!(
        std::fs::read_to_string(&usage).unwrap(),
        "from model import bb\n\nprint(bb(3))\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}

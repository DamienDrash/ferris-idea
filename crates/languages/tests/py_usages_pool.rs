//! SPEC-0042 v14 AK-22 (OBS-0034 L8): with the server pool, the first question to a freshly started
//! pyright covers the project's other files. A test binary of its own, because the pool is switched
//! on for the whole process. Skipped without pyright or basedpyright.

use std::time::Duration;

use ferris_idea_languages::find_usages::{UsageSource, find_usages_in};
use ferris_idea_languages::pool;

#[tokio::test]
async fn pooled_pyright_finds_usages_in_other_files_at_once() {
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
    let root =
        std::env::temp_dir().join(format!("ferris-idea-py-pool-usages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("model.py");
    std::fs::write(&file, "class Item:\n    pass\n").unwrap();
    std::fs::write(
        root.join("report.py"),
        "from model import Item\n\nfirst = Item()\n",
    )
    .unwrap();
    pool::enable();
    let found = find_usages_in(
        &root,
        &file,
        None,
        0,
        6,
        "Item",
        Duration::from_secs(60),
        None,
    )
    .await
    .unwrap();
    assert_eq!(found.source, UsageSource::LanguageServer);
    let other = found
        .usages
        .iter()
        .filter(|u| u.path == root.join("report.py"))
        .count();
    assert_eq!(other, 2, "{:?}", found.usages);
    let _ = std::fs::remove_dir_all(&root);
}

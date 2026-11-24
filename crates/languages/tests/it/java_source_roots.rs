//! Source roots of a Java project without a build file (`SPEC-0042` v8 behaviour 17, AK-13,
//! OBS-0011 item 2): jdtls gets the inferred roots, so a file under `src/main/java` whose package matches
//! its folders has no package error, and nothing is written into the project. Skipped without
//! `jdtls`.

use std::time::Duration;

use ferris_idea_languages::lsp::file_diagnostics;

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

/// Every path below `root`, sorted.
fn listing(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            out.push(p.strip_prefix(root).unwrap().display().to_string());
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Copies the folder `from` to `to`.
fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[tokio::test]
async fn maven_layout_without_build_file_has_no_package_error() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        skip("jdtls not installed");
        return;
    }
    // A copy of the reference project, which has no build file.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference-projects/java-basic");
    let root = std::env::temp_dir().join(format!("ferris-idea-java-roots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy(&source, &root);
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main/java/lab/inventory/Inventory.java");
    assert!(file.is_file());
    let before = listing(&root);
    let found = file_diagnostics(&root, &file, Duration::from_secs(120))
        .await
        .unwrap();
    assert!(
        !found.iter().any(|d| d.message.contains("package")),
        "{found:?}"
    );
    assert_eq!(
        listing(&root),
        before,
        "nothing is written into the project"
    );
    let _ = std::fs::remove_dir_all(ferris_idea_languages::java_roots::workspace_data_dir(&root));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0042 v9 AK-14 (OBS-0013 item 3): a project whose jdtls data was created without source
/// roots (as before v8) loses the package error on the next start through Ferris IDEA.
#[tokio::test]
async fn data_from_before_the_roots_is_renewed() {
    use ferris_idea_languages::{LanguageId, java_roots, lsp::LspClient, servers};
    let Ok(server) = servers::resolve(LanguageId::Java) else {
        skip("jdtls not installed");
        return;
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference-projects/java-basic");
    let root = std::env::temp_dir().join(format!("ferris-idea-java-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy(&source, &root);
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main/java/lab/inventory/Inventory.java");
    let text = std::fs::read_to_string(&file).unwrap();
    let data = java_roots::workspace_data_dir(&root);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let wait = Duration::from_secs(120);

    // The old start: same data folder, no source roots, nothing recorded.
    let data_arg = data.display().to_string();
    let mut args = server.args();
    args.extend(["-data", data_arg.as_str()]);
    let mut old = LspClient::spawn(&server.command.to_string_lossy(), &args, &root).unwrap();
    old.timeout = wait;
    old.init_options = serde_json::json!({
        "extendedClientCapabilities": {"classFileContentsSupport": true}
    });
    old.initialize(&root).await.unwrap();
    let stale = old
        .diagnostics(&file, LanguageId::Java, &text, wait)
        .await
        .unwrap();
    old.shutdown().await;
    eprintln!(
        "without roots: package error {}",
        stale.iter().any(|d| d.message.contains("package"))
    );

    let found = file_diagnostics(&root, &file, wait).await.unwrap();
    assert!(
        !found.iter().any(|d| d.message.contains("package")),
        "{found:?}"
    );
    assert_eq!(
        std::fs::read_to_string(data.join("ferris-idea-source-roots")).unwrap(),
        "src/main/java"
    );
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&root);
}

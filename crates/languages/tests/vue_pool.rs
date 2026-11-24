//! SPEC-0141/AK-8 (v4) through the language server pool: diagnostics asked once for a Vue file are
//! complete, though the server publishes them in steps. A test binary of its own, because the
//! pool is switched on for the whole process. Skipped when `vue-language-server` is not installed.

use std::time::Duration;

use ferris_idea_languages::{lsp, pool};

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

#[tokio::test]
async fn pooled_vue_diagnostics_are_complete() {
    let installed = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join("vue-language-server").is_file())
    });
    if !installed {
        skip("vue-language-server not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-vue-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("package.json"), "{\"name\": \"app\"}\n").unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("App.vue");
    std::fs::write(
        &file,
        "<script setup lang=\"ts\">\nconst count: number = \"three\"\n</script>\n\n<template>\n  <p>{{ count }}</p>\n</template>\n",
    )
    .unwrap();
    pool::enable();
    let wait = Duration::from_secs(120);
    let found = lsp::file_diagnostics(&root, &file, wait).await.unwrap();
    assert!(
        found
            .iter()
            .any(|d| d.start_line == 1 && d.message.contains("not assignable to type 'number'")),
        "{found:?}"
    );
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}

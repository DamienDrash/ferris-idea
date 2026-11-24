//! SPEC-0141 v4 with the real Vue language server (standalone mode): diagnostics asked once are
//! complete, hover at the start of a template name names the symbol, and — in a project with its
//! `node_modules` (`FERRIS_IDEA_TEST_VUE`, e.g. one made by `npm create vue@latest`) — hover and
//! type errors in the template. Skipped when `vue-language-server` is not installed. Without the
//! server pool; `vue_pool.rs` asks the same through the pool.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_languages::{insight, lsp};

const WAIT: Duration = Duration::from_secs(120);

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn installed(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// The component of a `npm create vue@latest` project.
const HELLO: &str = "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n}>()\n</script>\n\n<template>\n  <div class=\"greetings\">\n    <h1 class=\"green\">{{ msg }}</h1>\n  </div>\n</template>\n";

/// A project like the generated one, without `node_modules`: a solution-style `tsconfig.json`.
fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-vue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/components")).unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"name\": \"app\", \"private\": true, \"type\": \"module\"}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        "{\"files\": [], \"references\": [{\"path\": \"./tsconfig.app.json\"}]}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.app.json"),
        "{\"include\": [\"src/**/*\", \"src/**/*.vue\"], \"compilerOptions\": {\"strict\": true, \"module\": \"ESNext\", \"moduleResolution\": \"bundler\", \"target\": \"ESNext\", \"noEmit\": true}}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/components/HelloWorld.vue"), HELLO).unwrap();
    root.canonicalize().unwrap()
}

/// SPEC-0141/AK-8 (v4): a type error in the script is among the diagnostics asked once, though
/// the server's first publication for the file is the empty template step.
#[tokio::test]
async fn vue_diagnostics_are_complete() {
    if !installed("vue-language-server") {
        skip("vue-language-server not installed");
        return;
    }
    let root = project("diagnostics");
    let file = root.join("src/components/Broken.vue");
    std::fs::write(
        &file,
        "<script setup lang=\"ts\">\nconst count: number = \"three\"\n</script>\n\n<template>\n  <p>{{ count }}</p>\n</template>\n",
    )
    .unwrap();
    let found = lsp::file_diagnostics(&root, &file, WAIT).await.unwrap();
    assert!(
        found
            .iter()
            .any(|d| d.start_line == 1 && d.message.contains("not assignable to type 'number'")),
        "{found:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0141/AK-9 (v4): on the first letter of `msg` in `{{ msg }}` the hover is not the
/// server's generated context.
#[tokio::test]
async fn vue_template_hover_names_the_prop() {
    if !installed("vue-language-server") {
        skip("vue-language-server not installed");
        return;
    }
    let root = project("hover");
    let file = root.join("src/components/HelloWorld.vue");
    let hover = insight::hover_in(&root, &file, None, 8, 25, WAIT)
        .await
        .unwrap()
        .expect("a hover on msg");
    assert!(!hover.text.contains("__VLS_"), "{}", hover.text);
    let _ = std::fs::remove_dir_all(&root);
}

/// The Vue project named by `FERRIS_IDEA_TEST_VUE`, with its `node_modules`.
fn real_project() -> Option<PathBuf> {
    let root = PathBuf::from(ferris_idea_core::product_var_os("FERRIS_IDEA_TEST_VUE")?);
    root.join("node_modules/vue/package.json")
        .is_file()
        .then(|| root.canonicalize().ok())
        .flatten()
}

/// SPEC-0141/AK-9 (v4): what hand test Nr. 67 asked for, in a generated project: hover on
/// `defineProps`, on `msg` in the template (also on its first letter) and on a component tag; a
/// misspelt prop in the template is reported. The files live in a project of the test's own that
/// links to the given project's `node_modules`, so that project stays unchanged.
#[cfg(unix)]
#[tokio::test]
async fn vue_project_template_features() {
    let Some(source) = real_project() else {
        skip("FERRIS_IDEA_TEST_VUE does not name a Vue project with node_modules");
        return;
    };
    if !installed("vue-language-server") {
        skip("vue-language-server not installed");
        return;
    }
    let root = project("real");
    std::os::unix::fs::symlink(source.join("node_modules"), root.join("node_modules")).unwrap();
    let hello = root.join("src/components/HelloWorld.vue");
    let app = root.join("src/App.vue");
    std::fs::write(
        &app,
        "<script setup lang=\"ts\">\nimport HelloWorld from './components/HelloWorld.vue'\n</script>\n\n<template>\n  <HelloWorld msg=\"You did it!\" />\n</template>\n",
    )
    .unwrap();
    let ask = |file: &Path, line, character| {
        let (root, file) = (root.clone(), file.to_path_buf());
        async move {
            insight::hover_in(&root, &file, None, line, character, WAIT)
                .await
                .unwrap()
                .map(|h| h.text)
                .unwrap_or_default()
        }
    };
    let define = ask(&hello, 1, 3).await;
    assert!(define.contains("defineProps"), "{define}");
    for character in [25, 26] {
        let msg = ask(&hello, 8, character).await;
        assert!(msg.contains("msg: string"), "{character}: {msg}");
    }
    let tag = ask(&app, 5, 5).await;
    assert!(tag.contains("HelloWorld"), "{tag}");

    std::fs::write(&hello, HELLO.replace("{{ msg }}", "{{ msgg }}")).unwrap();
    let found = lsp::file_diagnostics(&root, &hello, WAIT).await.unwrap();
    assert!(
        found
            .iter()
            .any(|d| d.start_line == 8 && d.message.contains("'msgg' does not exist")),
        "{found:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

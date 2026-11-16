//! SPEC-0123 v4 behaviour 6 with the real language servers: Optimize Imports of TypeScript,
//! JavaScript, Svelte and Vue files on unsaved text (typescript-language-server 6.0.2 with
//! TypeScript 5.9.3, svelte-language-server 0.18.6, @vue/language-server 2.2.12). Each case is
//! skipped with a note without its server.

use std::path::PathBuf;
use std::time::Duration;

use idecraft_languages::LanguageId;
use idecraft_languages::js_imports::{organize_imports_in, summary};

const WAIT: Duration = Duration::from_secs(120);
const UTIL: &str = "export const a = 1\nexport const b = 2\nexport const c = 3\n";

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A project with `src/util.ts`, when the server IDECraft starts for `language` is `program`.
fn project(name: &str, language: LanguageId, program: &str) -> Option<Project> {
    if !idecraft_languages::servers::resolve(language).is_ok_and(|r| r.candidate.program == program)
    {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: {program}"
        );
        eprintln!("skipped: {program} not installed");
        return None;
    }
    let root =
        std::env::temp_dir().join(format!("idecraft-jsimports-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"name\": \"app\", \"private\": true, \"type\": \"module\"}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        "{\"compilerOptions\": {\"strict\": true, \"target\": \"es2022\", \"module\": \"esnext\", \"moduleResolution\": \"bundler\", \"jsx\": \"preserve\", \"allowJs\": true, \"noEmit\": true}, \"include\": [\"src/**/*\"]}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/util.ts"), UTIL).unwrap();
    Some(Project(root))
}

/// SPEC-0123/AK-10: on unsaved text the unused imports go, a statement keeps its used names, the
/// rest is sorted and combined; the file on disk stays; a second run has nothing to do.
#[tokio::test]
async fn typescript_imports_are_optimized_on_unsaved_text() {
    let Some(p) = project("ts", LanguageId::TypeScript, "typescript-language-server") else {
        return;
    };
    let file = p.0.join("src/main.ts");
    let saved = "export const saved = 1\n";
    std::fs::write(&file, saved).unwrap();
    let unsaved = "import {c, b} from \"./util\"\nimport * as all from \"./util\"\nimport {a} from \"./util\"\nimport \"./util\"\n\nexport const x = a + b\n";
    let done = organize_imports_in(&p.0, &file, unsaved, WAIT)
        .await
        .unwrap();
    assert_eq!(done.removed, ["c", "all"], "{}", done.text);
    assert_eq!(
        done.text,
        "import \"./util\"\nimport { a, b } from \"./util\"\n\nexport const x = a + b\n"
    );
    assert_eq!((done.by, done.sorted), ("language server", true));
    assert!(done.left.is_empty());
    assert_eq!(
        summary(&done, true),
        "Removed 2 unused imports and sorted the imports (language server)"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);

    let again = organize_imports_in(&p.0, &file, &done.text, WAIT)
        .await
        .unwrap();
    assert_eq!(again.text, done.text);
    assert!(again.removed.is_empty());
    assert_eq!(summary(&again, false), "Imports are already optimized");

    // Nothing unused: the imports are only sorted and combined.
    let unsorted =
        "import {b} from \"./util\"\nimport {a} from \"./util\"\n\nexport const x = a + b\n";
    let sorted = organize_imports_in(&p.0, &file, unsorted, WAIT)
        .await
        .unwrap();
    assert!(sorted.removed.is_empty());
    assert_eq!(
        sorted.text,
        "import { a, b } from \"./util\"\n\nexport const x = a + b\n"
    );
    assert_eq!(summary(&sorted, true), "Imports sorted (language server)");
}

/// SPEC-0123/AK-10: a `.tsx` and a `.js` file.
#[tokio::test]
async fn tsx_and_javascript_imports_are_optimized() {
    let Some(p) = project("tsx", LanguageId::TypeScript, "typescript-language-server") else {
        return;
    };
    let tsx = p.0.join("src/App.tsx");
    let text = "import {b, a} from \"./util\"\n\nexport const n = <T,>(v: T) => [v, a]\n";
    std::fs::write(&tsx, text).unwrap();
    let done = organize_imports_in(&p.0, &tsx, text, WAIT).await.unwrap();
    assert_eq!(done.removed, ["b"], "{}", done.text);
    assert_eq!(
        done.text,
        "import { a } from \"./util\"\n\nexport const n = <T,>(v: T) => [v, a]\n"
    );

    let js = p.0.join("src/tool.mjs");
    let text = "import {c, a} from \"./util\"\n\nexport const n = c\n";
    std::fs::write(&js, text).unwrap();
    let done = organize_imports_in(&p.0, &js, text, WAIT).await.unwrap();
    assert_eq!(done.removed, ["a"], "{}", done.text);
    assert_eq!(
        done.text,
        "import { c } from \"./util\"\n\nexport const n = c\n"
    );
}

/// SPEC-0123/AK-10: the unused import of a Svelte file's script block goes.
#[tokio::test]
async fn svelte_imports_are_optimized() {
    let Some(p) = project("svelte", LanguageId::Svelte, "svelteserver") else {
        return;
    };
    let file = p.0.join("src/A.svelte");
    let text = "<script lang=\"ts\">\n  import {c, b} from \"./util\"\n  import {a} from \"./util\"\n  const n = a + b\n</script>\n\n<h1>{n}</h1>\n";
    std::fs::write(&file, text).unwrap();
    let done = organize_imports_in(&p.0, &file, text, WAIT).await.unwrap();
    assert_eq!(done.removed, ["c"], "{}", done.text);
    assert!(
        done.text.contains("import { a, b } from \"./util\""),
        "{}",
        done.text
    );
    assert!(
        done.text
            .ends_with("  const n = a + b\n</script>\n\n<h1>{n}</h1>\n"),
        "{}",
        done.text
    );
}

/// SPEC-0123/AK-10: the unused import of a Vue file's script block goes.
#[tokio::test]
async fn vue_imports_are_optimized() {
    let Some(p) = project("vue", LanguageId::Vue, "vue-language-server") else {
        return;
    };
    let file = p.0.join("src/A.vue");
    let text = "<script setup lang=\"ts\">\nimport {c, b} from \"./util\"\nimport {a} from \"./util\"\nconst n = a + b\n</script>\n\n<template>\n  <h1>{{ n }}</h1>\n</template>\n";
    std::fs::write(&file, text).unwrap();
    let done = organize_imports_in(&p.0, &file, text, WAIT).await.unwrap();
    assert_eq!(done.removed, ["c"], "{}", done.text);
    assert!(
        done.text.contains("import { a, b } from \"./util\""),
        "{}",
        done.text
    );
    assert!(
        done.text.ends_with(
            "const n = a + b\n</script>\n\n<template>\n  <h1>{{ n }}</h1>\n</template>\n"
        ),
        "{}",
        done.text
    );
}

//! SPEC-0293 v4 with a real TypeScript server (typescript-language-server, or TypeScript 7's
//! `tsc --lsp`): Find Usages on an interface method lists its call, not the methods that implement
//! it (OBS-0028 T5). Skipped without a TypeScript server.

use std::path::PathBuf;
use std::time::Duration;

use ferris_idea_languages::LanguageId;
use ferris_idea_languages::context::UsageKind;
use ferris_idea_languages::find_usages::{UsageSource, find_usages_in};
use ferris_idea_languages::usages::usage_counts_in;

/// The files of OBS-0028 item 7.
const SHAPE: &str = "export interface Shape {\n  area(): number;\n}\n\nexport abstract class Base implements Shape {\n  abstract area(): number;\n\n  describe(): string {\n    return `area ${this.area()}`;\n  }\n}\n";
const CIRCLE: &str = "import { Base } from \"./shape\";\n\nexport class Circle extends Base {\n  constructor(readonly r: number) {\n    super();\n  }\n\n  area(): number {\n    return this.r * this.r * 3;\n  }\n}\n";
const SQUARE: &str = "import { Base } from \"./shape\";\n\nexport class Square extends Base {\n  area(): number {\n    return 4;\n  }\n}\n";

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn usages_of_an_interface_method_are_its_calls() {
    let Ok(server) = ferris_idea_languages::servers::resolve(LanguageId::TypeScript) else {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: a TypeScript language server"
        );
        eprintln!("skipped: no TypeScript language server");
        return;
    };
    let root = std::env::temp_dir().join(format!("ferris-idea-ts-usages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let p = Project(root.canonicalize().unwrap());
    std::fs::write(
        p.0.join("tsconfig.json"),
        "{\"compilerOptions\": {\"strict\": true, \"target\": \"es2022\"}, \"include\": [\"src\"]}\n",
    )
    .unwrap();
    std::fs::write(p.0.join("src/shape.ts"), SHAPE).unwrap();
    std::fs::write(p.0.join("src/circle.ts"), CIRCLE).unwrap();
    std::fs::write(p.0.join("src/square.ts"), SQUARE).unwrap();
    let file = p.0.join("src/shape.ts");
    // `area` in `interface Shape`, line 1.
    let found = find_usages_in(
        &p.0,
        &file,
        None,
        1,
        2,
        "area",
        Duration::from_secs(60),
        None,
    )
    .await
    .unwrap();
    let program = server.candidate.program;
    assert_eq!(
        found.source,
        UsageSource::LanguageServer,
        "{program}: {found:?}"
    );
    let places: Vec<(String, u64)> = found
        .usages
        .iter()
        .map(|u| {
            (
                u.path.file_name().unwrap().to_string_lossy().into_owned(),
                u.range.start_line,
            )
        })
        .collect();
    // The reference lists one usage, the call in `describe`; the declarations in `Base`,
    // `Circle` and `Square` are implementations.
    assert_eq!(places, [("shape.ts".to_owned(), 8)], "{program}: {found:?}");
    assert_eq!(found.details[0].kind, UsageKind::Call, "{program}");
    eprintln!("{program}: {places:?}");
}

/// OBS-0028 T8: the usage counts at the lines say what Find Usages lists — `1 usage · 3
/// implementations` at `area` of `interface Shape`, `1 usage · 2 implementations` at `abstract
/// area` in `Base` — and the types keep their counts (`1 usage · 3 inheritors` at `Shape`).
#[tokio::test(flavor = "multi_thread")]
async fn usage_counts_say_what_find_usages_lists() {
    let Ok(server) = ferris_idea_languages::servers::resolve(LanguageId::TypeScript) else {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: a TypeScript language server"
        );
        eprintln!("skipped: no TypeScript language server");
        return;
    };
    let root = std::env::temp_dir().join(format!("ferris-idea-ts-counts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let p = Project(root.canonicalize().unwrap());
    std::fs::write(
        p.0.join("tsconfig.json"),
        "{\"compilerOptions\": {\"strict\": true, \"target\": \"es2022\"}, \"include\": [\"src\"]}\n",
    )
    .unwrap();
    std::fs::write(p.0.join("src/shape.ts"), SHAPE).unwrap();
    std::fs::write(p.0.join("src/circle.ts"), CIRCLE).unwrap();
    std::fs::write(p.0.join("src/square.ts"), SQUARE).unwrap();
    let file = p.0.join("src/shape.ts");
    let counts = usage_counts_in(&p.0, &file, None, Duration::from_secs(60))
        .await
        .unwrap();
    let program = server.candidate.program;
    let label = |name: &str, line: u64| {
        counts
            .iter()
            .find(|c| c.name == name && c.line == line)
            .map(|c| c.label())
            .unwrap_or_else(|| panic!("{program}: no count for {name} at {line}: {counts:?}"))
    };
    assert_eq!(label("area", 1), "1 usage · 3 implementations", "{program}");
    assert_eq!(label("area", 5), "1 usage · 2 implementations", "{program}");
    assert_eq!(label("Shape", 0), "1 usage · 3 inheritors", "{program}");
    // Each count is the length of Find Usages' list at the same place, whatever the server
    // answers (typescript-language-server names the imports of `Base` too, TypeScript 7 not).
    for c in &counts {
        let found = find_usages_in(
            &p.0,
            &file,
            None,
            c.line,
            c.character,
            &c.name,
            Duration::from_secs(60),
            None,
        )
        .await
        .unwrap();
        assert_eq!(found.usages.len(), c.count, "{program}: {c:?} {found:?}");
    }
    eprintln!("{program}: {counts:?}");
}

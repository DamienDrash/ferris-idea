//! The gaps GO1, RS1 and PH1 of the side-by-side comparison of F-038 against real servers:
//! Find Usages with gopls (`SPEC-0293` v12 behaviours 14b and 30) and rust-analyzer (behaviour
//! 29), completion with intelephense (`SPEC-0072` v21 behaviours 56 and 57). Each test is skipped
//! when its server is not installed (and fails then under `FERRIS_IDEA_REQUIRE_TOOLS`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_languages::context::UsageKind;
use ferris_idea_languages::find_usages::{UsageSource, code_usages, find_usages_in};

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

/// Whether `program` is on `PATH` (gopls and intelephense have no `--version`).
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Whether `rust-analyzer --version` answers in `cwd`: not only the rustup proxy.
fn answers(program: &str, cwd: &Path) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .current_dir(cwd)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-f038-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, content) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }
    root.canonicalize().unwrap()
}

const WAIT: Duration = Duration::from_secs(90);

const GO: &str = "package main\n\nimport \"fmt\"\n\n// Rect is a rectangle.\ntype Rect struct{ W, H float64 }\n\nfunc (r Rect) Area() float64 { return r.W * r.H }\n\nfunc main() {\n\tr := Rect{W: 2, H: 3}\n\tfmt.Println(r.Area())\n}\n";

/// SPEC-0293 v12 AK-18 (GO1): Find Usages on `Rect` with gopls lists the receiver and the struct
/// initialization by their kinds and the mention in the doc comment as a usage in a comment,
/// counted apart; the declaration is not listed. The same from the struct initialization.
#[tokio::test]
async fn gopls_usages_by_kind_with_the_comment_mention() {
    let root = project(
        "go",
        &[("go.mod", "module shapes\n\ngo 1.21\n"), ("main.go", GO)],
    );
    if !on_path("gopls") {
        skip("gopls not installed");
        return;
    }
    let file = root.join("main.go");
    for (line, character) in [(5, 5), (10, 7)] {
        let found = find_usages_in(&root, &file, None, line, character, "Rect", WAIT, None)
            .await
            .unwrap();
        assert_eq!(found.source, UsageSource::LanguageServer);
        let shown: Vec<(u64, u64, UsageKind)> = found
            .usages
            .iter()
            .zip(&found.details)
            .map(|(u, d)| (u.range.start_line, u.range.start_character, d.kind))
            .collect();
        assert_eq!(
            shown,
            [
                (4, 3, UsageKind::Comment),
                (7, 8, UsageKind::Receiver),
                (10, 6, UsageKind::StructInit),
            ],
            "asked at {line}:{character}"
        );
        assert_eq!(code_usages(&found.details), 2);
        let d = found.target.declaration.expect("declaration");
        assert_eq!((d.range.start_line, d.range.start_character), (5, 5));
    }
    let _ = std::fs::remove_dir_all(&root);
}

const RUST: &str = "#[derive(Debug, Clone, Copy, PartialEq)]\nstruct Rect {\n    w: f64,\n    h: f64,\n}\n\nimpl Rect {\n    fn area(&self) -> f64 {\n        self.w * self.h\n    }\n}\n\nfn main() {\n    let r = Rect { w: 2.0, h: 3.0 };\n    println!(\"{}\", r.area());\n}\n";

/// SPEC-0293 v12 AK-19 (RS1): Find Usages on `struct Rect` with rust-analyzer lists `impl Rect`
/// and the literal, not the declaration, asked on the declaration and on a usage.
#[tokio::test]
async fn rust_analyzer_usages_without_the_declaration() {
    let root = project(
        "rs",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
            ("src/main.rs", RUST),
        ],
    );
    if !answers("rust-analyzer", &root) {
        skip("rust-analyzer not installed for the stable toolchain");
        return;
    }
    let file = root.join("src/main.rs");
    for (line, character) in [(1, 7), (6, 6)] {
        let found = find_usages_in(&root, &file, None, line, character, "Rect", WAIT, None)
            .await
            .unwrap();
        let places: Vec<(u64, u64)> = found
            .usages
            .iter()
            .map(|u| (u.range.start_line, u.range.start_character))
            .collect();
        assert_eq!(places, [(6, 5), (13, 12)], "asked at {line}:{character}");
        let d = found.target.declaration.expect("declaration");
        assert_eq!((d.range.start_line, d.range.start_character), (1, 7));
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0072 v21 AK-42 (PH1): with intelephense, `gr` in `echo gr|eet("you");` lists `greet`,
/// declared in the file, first, and in this call position the functions before the constants.
#[tokio::test]
async fn intelephense_ranks_the_files_function_first() {
    let text = "<?php\n\nfunction greet(string $name): string\n{\n    return \"Hello, \" . $name;\n}\n\necho greet(\"you\");\n";
    let root = project("php", &[("index.php", text)]);
    if !on_path("intelephense") {
        skip("intelephense not installed");
        return;
    }
    let file = root.join("index.php");
    let all = ferris_idea_languages::insight::complete(&root, &file, 7, 7, 500, WAIT)
        .await
        .unwrap();
    // The names starting with `gr`; fuzzy matches follow them.
    let shown: Vec<_> = ferris_idea_languages::completion_filter::filter(&all, "gr")
        .into_iter()
        .filter(|i| i.label.to_lowercase().starts_with("gr"))
        .collect();
    assert_eq!(shown.first().map(|i| i.label.as_str()), Some("greet"));
    assert_eq!(
        shown[0].declared_in,
        Some(ferris_idea_languages::insight::DeclaredIn::File)
    );
    let functions = shown
        .iter()
        .take_while(|i| i.kind.as_deref() == Some("function"))
        .count();
    assert!(
        shown[functions..]
            .iter()
            .all(|i| i.kind.as_deref() != Some("function")),
        "functions first: {:?}",
        shown
            .iter()
            .map(|i| (&i.label, &i.kind))
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&root);
}

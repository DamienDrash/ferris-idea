//! SPEC-0095 v3 behaviour 8 (D55): Extract Variable, then renaming the introduced name, with the
//! pooled server the window uses. Skipped when `clangd` is not installed. A test binary of its own,
//! because the pool is switched on for the whole process.

use std::time::Duration;

use idecraft_languages::actions::{Refactoring, introduced_name, plan_code_action_in};
use idecraft_languages::rename::{apply_edits, plan_rename_in};
use idecraft_languages::{insight, pool};

/// Skips a test whose tool is missing; with `IDECRAFT_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn range_of(text: &str, needle: &str) -> insight::Range {
    let at = text.find(needle).unwrap();
    let line = text[..at].matches('\n').count() as u64;
    let col = |o: usize| (o - text[..o].rfind('\n').map_or(0, |i| i + 1)) as u64;
    insight::Range {
        start_line: line,
        start_character: col(at),
        end_line: line,
        end_character: col(at + needle.len()),
    }
}

#[tokio::test]
async fn the_extracted_variable_is_renamed_everywhere() {
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    const SHAPES: &str = "// Handtest IDECraft: Navigation und Code-Einsicht
#include <cstdio>

/// Eine Form mit Fläche.
struct Shape {
    virtual int area() const = 1;
    virtual ~Shape() = default;
};

struct Square : Shape {
    int side = 2;
    int area() const override { return side * side; }
};

int add(int a, int b) { return a + b; }

int main() {
    Square sq;
    Shape &s = sq;
    int total = add(s.area(), 1);
    std::printf(\"%d\\n\", total + missing);
    return 0;
}
";
    let root = std::env::temp_dir().join(format!("idecraft-pool-extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("shapes.cpp");
    std::fs::write(&file, SHAPES).unwrap();
    pool::enable();
    let wait = Duration::from_secs(60);
    // The window has the file open: its text is synchronised before the refactoring.
    pool::sync_text(&root, &file, SHAPES).await.unwrap();

    let selected = range_of(SHAPES, "s.area()");
    let found = idecraft_languages::actions::refactoring_targets(
        &root,
        &file,
        Some(SHAPES),
        &selected,
        Refactoring::Variable,
        wait,
    )
    .await
    .unwrap();
    let title = found[0].actions[0].title.clone();
    let plan = plan_code_action_in(&root, &file, Some(SHAPES), &selected, &title, wait)
        .await
        .unwrap();
    let extracted = apply_edits(SHAPES, &plan[0].edits).unwrap();
    // The editor sends the changed text as one goes on typing.
    pool::sync_text(&root, &file, &extracted).await.unwrap();
    let name = introduced_name(SHAPES, &extracted).unwrap();
    let rename = plan_rename_in(
        &root,
        &file,
        Some(&extracted),
        name.line,
        name.character,
        "flaeche",
        wait,
    )
    .await
    .unwrap();
    // clangd alone renames only the declaration here: `= 1` is no pure-specifier, so `s.area()`
    // and with it the type of the new variable do not compile, and its use is no reference.
    let mut rename = rename;
    assert!(!idecraft_languages::actions::has_word(SHAPES, &name.name));
    idecraft_languages::actions::complete_introduced_rename(&mut rename, &file, &extracted);
    assert_eq!(rename.files.len(), 1, "{rename:?}");
    let renamed = apply_edits(&extracted, &rename.files[0].edits).unwrap();
    assert!(renamed.contains("auto flaeche = s.area();"), "{renamed}");
    assert!(renamed.contains("add(flaeche, 1)"), "{renamed}");
    assert!(!renamed.contains(&name.name), "{renamed}");

    pool::close_text(&file).await;
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}

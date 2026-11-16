//! Language features against a second server: rust-analyzer on a small Cargo project (F-038). The
//! same functions are tested with clangd in `clangd.rs`.

use std::path::{Path, PathBuf};
use std::time::Duration;

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

/// rust-analyzer answers, not only the rustup proxy without the component.
fn available() -> bool {
    std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

const MAIN: &str = "/// A shape with an area.
trait Shape {
    fn area(&self) -> i32;
}

struct Square {
    side: i32,
}

impl Shape for Square {
    fn area(&self) -> i32 {
        self.side * self.side
    }
}

/// Adds two numbers.
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let sq = Square { side: 2 };
    let total = add(sq.area(), 1);
    println!(\"{total}\");
}
";

/// A Cargo project whose `src/main.rs` on disk is empty: every answer comes from the editor text.
fn project(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("idecraft-ra-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main.rs");
    (root, file)
}

fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

const WAIT: Duration = Duration::from_secs(120);

#[tokio::test]
async fn rust_analyzer_navigation_and_insight() {
    use idecraft_languages::insight::{
        CallDirection, LocationKind, call_hierarchy_in, hover_in, locations_in, signature_help_in,
        symbols_in,
    };
    if !available() {
        skip("rust-analyzer not installed");
        return;
    }
    let (root, file) = project("nav");
    let text = Some(MAIN);
    let lines = |found: &[idecraft_languages::insight::Location]| -> Vec<u64> {
        let mut v: Vec<u64> = found.iter().map(|l| l.range.start_line).collect();
        v.sort();
        v
    };

    let hover = hover_in(&root, &file, text, 22, 16, WAIT)
        .await
        .unwrap()
        .expect("a hover for add");
    assert!(hover.text.contains("Adds two numbers"), "{}", hover.text);
    assert!(
        hover.text.contains("fn add(a: i32, b: i32) -> i32"),
        "{}",
        hover.text
    );

    let def = locations_in(&root, &file, text, 22, 16, LocationKind::Definition, WAIT)
        .await
        .unwrap();
    assert_eq!(lines(&def), [16]);
    let refs = locations_in(&root, &file, text, 16, 3, LocationKind::References, WAIT)
        .await
        .unwrap();
    assert_eq!(lines(&refs), [16, 22]);
    let ty = locations_in(
        &root,
        &file,
        text,
        21,
        8,
        LocationKind::TypeDefinition,
        WAIT,
    )
    .await
    .unwrap();
    assert_eq!(lines(&ty), [5]);
    let imp = locations_in(&root, &file, text, 2, 7, LocationKind::Implementation, WAIT)
        .await
        .unwrap();
    assert_eq!(lines(&imp), [10]);

    let help = signature_help_in(&root, &file, text, 22, 31, WAIT)
        .await
        .unwrap()
        .expect("a signature");
    let sig = &help.signatures[help.active_signature];
    let active = sig.active_parameter.expect("an active parameter");
    let name: String = sig
        .label
        .chars()
        .skip(sig.parameters[active].start)
        .take(sig.parameters[active].len())
        .collect();
    assert_eq!(name, "b: i32", "{sig:?}");

    let symbols = symbols_in(&root, &file, text, WAIT).await.unwrap();
    let top: Vec<&str> = symbols
        .iter()
        .filter(|s| s.depth == 0)
        .map(|s| s.name.as_str())
        .collect();
    for want in ["Shape", "Square", "add", "main"] {
        assert!(top.contains(&want), "{top:?}");
    }
    assert!(symbols.iter().any(|s| s.name == "area" && s.depth == 1));

    let callers = call_hierarchy_in(&root, &file, text, 16, 3, CallDirection::Callers, 2, WAIT)
        .await
        .unwrap()
        .expect("add is a function");
    assert_eq!(callers.name, "add");
    assert_eq!(
        callers
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["main"]
    );
    cleanup(&root);
}

#[tokio::test]
async fn rust_analyzer_editing() {
    use idecraft_languages::actions::code_actions_in;
    use idecraft_languages::format::{FormatOptions, plan_format_in};
    use idecraft_languages::insight::{Range, complete_in};
    use idecraft_languages::rename::plan_rename_in;
    if !available() {
        skip("rust-analyzer not installed");
        return;
    }
    let (root, file) = project("edit");

    let typing = MAIN.replace("let total = add(sq.area(), 1);", "let total = ad");
    let items = complete_in(&root, &file, Some(&typing), 22, 18, 50, WAIT)
        .await
        .unwrap();
    assert!(
        items.iter().any(|i| i.label.starts_with("add")),
        "{items:?}"
    );

    let plan = plan_rename_in(&root, &file, Some(MAIN), 16, 3, "plus", WAIT)
        .await
        .unwrap();
    assert_eq!(plan.old_name, "add");
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].edits.len(), 2, "{plan:?}");
    assert!(plan.files[0].edits.iter().all(|e| e.old_text == "add"));

    let messy = MAIN.replace(
        "fn add(a: i32, b: i32) -> i32 {",
        "fn add(a:i32,b:i32)->i32{",
    );
    let formatted = plan_format_in(
        &root,
        &file,
        Some(&messy),
        None,
        FormatOptions::default(),
        WAIT,
    )
    .await
    .unwrap();
    assert!(!formatted.edits.edits.is_empty());
    assert!(
        formatted
            .formatted
            .contains("fn add(a: i32, b: i32) -> i32 {"),
        "{}",
        formatted.formatted
    );

    let at = Range {
        start_line: 21,
        start_character: 8,
        end_line: 21,
        end_character: 8,
    };
    let actions = code_actions_in(&root, &file, Some(MAIN), &at, WAIT)
        .await
        .unwrap();
    assert!(!actions.is_empty(), "rust-analyzer offers assists on `sq`");
    cleanup(&root);
}

/// rust-analyzer answers with unresolved assists when the client can resolve them; the plan resolves
/// the chosen one before it is previewed (SPEC-0041 v2 AK-4).
#[tokio::test]
async fn rust_analyzer_resolves_lazy_code_actions() {
    use idecraft_languages::actions::{code_actions_in, plan_code_action_in};
    use idecraft_languages::insight::Range;
    use idecraft_languages::rename::apply_edits;
    if !available() {
        skip("rust-analyzer not installed");
        return;
    }
    let (root, file) = project("resolve");
    // `sq.area()` in `let total = add(sq.area(), 1);`.
    let at = Range {
        start_line: 22,
        start_character: 20,
        end_line: 22,
        end_character: 29,
    };
    let actions = code_actions_in(&root, &file, Some(MAIN), &at, WAIT)
        .await
        .unwrap();
    let extract = actions
        .iter()
        .find(|a| a.title.to_lowercase().contains("extract into variable"))
        .unwrap_or_else(|| panic!("no extract assist: {actions:?}"));
    let plan = plan_code_action_in(&root, &file, Some(MAIN), &at, &extract.title, WAIT)
        .await
        .unwrap();
    assert_eq!(plan.len(), 1, "{plan:?}");
    let changed = apply_edits(MAIN, &plan[0].edits).unwrap();
    assert!(changed.contains("= sq.area();"), "{changed}");
    assert!(!changed.contains("add(sq.area(), 1)"), "{changed}");
    cleanup(&root);
}

/// The range of `needle` in `text`, or with `caret` the empty range `caret` characters into it.
fn range_of(text: &str, needle: &str, caret: Option<usize>) -> idecraft_languages::insight::Range {
    let start = text.find(needle).unwrap_or_else(|| panic!("{needle:?}"));
    let (a, b) = match caret {
        Some(c) => (start + c, start + c),
        None => (start, start + needle.len()),
    };
    let pos = |o: usize| {
        let before = &text[..o];
        let line = before.matches('\n').count() as u64;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u64;
        (line, column)
    };
    let ((start_line, start_character), (end_line, end_character)) = (pos(a), pos(b));
    idecraft_languages::insight::Range {
        start_line,
        start_character,
        end_line,
        end_character,
    }
}

/// SPEC-0095 v2 AK-4 with rust-analyzer: Extract Variable on a selection and at a bare caret (the
/// expressions around it, inside out), Extract Method on a line, Inline at a caret on a variable.
#[tokio::test]
async fn rust_analyzer_refactoring_shortcuts() {
    use idecraft_languages::actions::{Refactoring, plan_code_action_in, refactoring_targets};
    use idecraft_languages::rename::apply_edits;
    if !available() {
        skip("rust-analyzer not installed");
        return;
    }
    let (root, file) = project("refactor");
    let targets = async |range: idecraft_languages::insight::Range, r| {
        refactoring_targets(&root, &file, Some(MAIN), &range, r, WAIT).await
    };

    let selected = range_of(MAIN, "sq.area()", None);
    let found = targets(selected.clone(), Refactoring::Variable)
        .await
        .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].actions.len(),
        1,
        "only Extract into variable: {found:?}"
    );
    let title = &found[0].actions[0].title;
    let plan = plan_code_action_in(&root, &file, Some(MAIN), &selected, title, WAIT)
        .await
        .unwrap();
    let changed = apply_edits(MAIN, &plan[0].edits).unwrap();
    assert!(changed.contains("= sq.area();"), "{changed}");

    // At a bare caret in `sq.area()` the call and the call of `add` around it are offered.
    let found = targets(range_of(MAIN, "sq.area()", Some(4)), Refactoring::Variable)
        .await
        .unwrap();
    let texts: Vec<&str> = found.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"sq.area()"), "{texts:?}");
    assert!(texts.contains(&"add(sq.area(), 1)"), "{texts:?}");
    assert!(texts.iter().all(|t| !t.contains(';')), "{texts:?}");

    let line = range_of(MAIN, "    let total = add(sq.area(), 1);\n", None);
    let found = targets(line, Refactoring::Method).await.unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    // The same as clangd in hand test 210 (SPEC-0095 v5 AK-12): the extracted function's name
    // is found.
    let title = &found[0].actions[0].title;
    let plan = plan_code_action_in(&root, &file, Some(MAIN), &found[0].range, title, WAIT)
        .await
        .unwrap();
    let with_function = apply_edits(MAIN, &plan[0].edits).unwrap();
    let name = idecraft_languages::actions::introduced_name(MAIN, &with_function).unwrap();
    assert_eq!(name.name, "fun_name", "{with_function}");

    // Refactor This on a name: rust-analyzer lists its own actions there but no Rename (a rename is
    // no code action), so IDECraft adds it (SPEC-0095 v5 behaviour 4a).
    let found = targets(range_of(MAIN, "total = add", Some(2)), Refactoring::Any)
        .await
        .unwrap();
    let actions: Vec<_> = found.into_iter().flat_map(|t| t.actions).collect();
    assert!(!actions.is_empty());
    assert!(
        actions
            .iter()
            .all(|a| !a.title.to_lowercase().starts_with("rename"))
    );
    let own = idecraft_languages::actions::refactor_this_own(&actions, true);
    assert_eq!(own[0].title, "Rename…", "{own:?}");

    let found = targets(range_of(MAIN, "let total", Some(5)), Refactoring::Inline)
        .await
        .unwrap();
    assert!(
        found
            .iter()
            .flat_map(|t| &t.actions)
            .any(|a| a.title.to_lowercase().starts_with("inline")),
        "{found:?}"
    );
    cleanup(&root);
}

//! SPEC-0023/AK-5 against a real language server. Skipped when `clangd` is not installed.

use std::time::Duration;

use ferris_idea_languages::lsp::file_diagnostics;

/// Skips a test whose tool is missing; with `FERRIS_IDEA_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

#[tokio::test]
async fn clangd_reports_an_undeclared_identifier() {
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-clangd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("main.c");
    std::fs::write(
        &file,
        "int main(void) {\n    int a = 1;\n    return a + missing;\n}\n",
    )
    .unwrap();

    let diagnostics = file_diagnostics(&root, &file, Duration::from_secs(30))
        .await
        .unwrap();
    let error = diagnostics
        .iter()
        .find(|d| d.severity == "error")
        .unwrap_or_else(|| panic!("{diagnostics:#?}"));
    assert_eq!(error.start_line, 2);
    assert!(error.message.contains("missing"), "{error:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0024/AK-3: hover, definition, references, completion and symbols from clangd.
#[tokio::test]
async fn clangd_code_insight() {
    use ferris_idea_languages::insight::{LocationKind, complete, hover, locations, symbols};
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-insight-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("calc.c");
    std::fs::write(
        &file,
        "int add(int a, int b) {\n    return a + b;\n}\n\nint main(void) {\n    int x = add(1, 2);\n    return add(x, 3);\n}\n",
    )
    .unwrap();
    let wait = Duration::from_secs(30);

    let h = hover(&root, &file, 5, 13, wait).await.unwrap().unwrap();
    assert!(h.text.contains("add"), "{h:?}");

    let def = locations(&root, &file, 6, 12, LocationKind::Definition, wait)
        .await
        .unwrap();
    assert_eq!(def.len(), 1, "{def:?}");
    assert_eq!(def[0].path, file);
    assert_eq!(
        (def[0].range.start_line, def[0].range.start_character),
        (0, 4)
    );

    let refs = locations(&root, &file, 0, 5, LocationKind::References, wait)
        .await
        .unwrap();
    let mut lines: Vec<u64> = refs.iter().map(|l| l.range.start_line).collect();
    lines.sort();
    assert_eq!(lines, [0, 5, 6], "{refs:?}");

    let symbols = symbols(&root, &file, wait).await.unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["add", "main"], "{symbols:?}");
    assert_eq!(symbols[0].kind, "function");

    let partial = root.join("partial.c");
    std::fs::write(
        &partial,
        "int add(int a, int b) { return a + b; }\nint main(void) { return ad; }\n",
    )
    .unwrap();
    let items = complete(&root, &partial, 1, 26, 50, wait).await.unwrap();
    assert!(
        items.iter().any(|i| i.label.starts_with("add")),
        "{items:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// SPEC-0040 AK-2: rename a C function through clangd.
#[tokio::test]
async fn clangd_renames_a_function() {
    use ferris_idea_languages::rename::{apply_plan, plan_rename};
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("calc.c");
    let source = "int add(int a, int b);\n\nint add(int a, int b) {\n    return a + b;\n}\n\nint main(void) {\n    int größe = add(1, 2);\n    return add(größe, 0);\n}\n";
    std::fs::write(&file, source).unwrap();
    let wait = Duration::from_secs(60);

    // Line 8 "    return add(größe, 0);": `add` starts at character 11.
    let plan = plan_rename(&root, &file, 8, 12, "sum", wait).await.unwrap();
    assert_eq!(
        (plan.old_name.as_str(), plan.new_name.as_str()),
        ("add", "sum")
    );
    assert_eq!(plan.files.len(), 1);
    let lines: Vec<u64> = plan.files[0]
        .edits
        .iter()
        .map(|e| e.range.start_line)
        .collect();
    assert_eq!(lines, [0, 2, 7, 8]);
    assert!(
        plan.files[0]
            .edits
            .iter()
            .all(|e| e.old_text == "add" && e.new_text == "sum")
    );

    // A variable with non-ASCII letters before the call on the same line.
    let plan_var = plan_rename(&root, &file, 7, 9, "size", wait).await.unwrap();
    assert_eq!(plan_var.old_name, "größe");
    assert_eq!(plan_var.files[0].edits.len(), 2);

    apply_plan(&plan).unwrap();
    let renamed = std::fs::read_to_string(&file).unwrap();
    assert_eq!(renamed.matches("sum(").count(), 4);
    assert!(!renamed.contains("add"));
    let compiles = std::process::Command::new("cc")
        .args(["-fsyntax-only", "-Wno-everything"])
        .arg(&file)
        .status()
        .map(|s| s.success());
    assert!(compiles.unwrap_or(true));

    // Whitespace has nothing to rename; an invalid name is refused by the server.
    assert!(plan_rename(&root, &file, 1, 0, "x", wait).await.is_err());
    let bad = plan_rename(&root, &file, 2, 5, "1bad", wait)
        .await
        .unwrap_err();
    assert!(!bad.to_string().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0113 AK-2: Change Signature with clangd finding the declaration, the prototype and the calls;
/// the result compiles.
#[tokio::test]
async fn clangd_change_signature() {
    use ferris_idea_languages::rename::apply_files;
    use ferris_idea_languages::signature::plan_change_signature_in;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-signature-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("calc.c");
    let source = "int add(int a, int b);\n\nint add(int a, int b) {\n    return a - b;\n}\n\nint main(void) {\n    int x = add(5, 2);\n    return add(x, add(1, 0));\n}\n";
    std::fs::write(&file, source).unwrap();
    let wait = Duration::from_secs(60);

    // Line 8 "    return add(x, add(1, 0));": the outer `add` at character 11.
    let current = plan_change_signature_in(&root, &file, None, 8, 12, None, wait)
        .await
        .unwrap();
    assert_eq!(
        (current.function.as_str(), current.old_parameters.as_str()),
        ("add", "int a, int b")
    );
    let plan = plan_change_signature_in(
        &root,
        &file,
        None,
        8,
        12,
        Some("int b, int a, int scale = 1"),
        wait,
    )
    .await
    .unwrap();
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    apply_files(&plan.files).unwrap();
    let changed = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        changed,
        "int add(int b, int a, int scale);\n\nint add(int b, int a, int scale) {\n    return a - b;\n}\n\nint main(void) {\n    int x = add(2, 5, 1);\n    return add(add(0, 1, 1), x, 1);\n}\n"
    );
    let compiles = std::process::Command::new("cc")
        .args(["-fsyntax-only", "-Wno-everything"])
        .arg(&file)
        .status()
        .map(|s| s.success());
    assert!(compiles.unwrap_or(true));
    let _ = std::fs::remove_dir_all(&root);
}

// SPEC-0041 AK-1: quick fix and extract refactoring through clangd.
#[tokio::test]
async fn clangd_code_actions() {
    use ferris_idea_languages::actions::{code_actions, plan_code_action};
    use ferris_idea_languages::insight::Range;
    use ferris_idea_languages::rename::apply_files;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-actions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let wait = Duration::from_secs(60);
    let compiles = |path: &std::path::Path| {
        std::process::Command::new("cc")
            .args(["-fsyntax-only", "-Wno-everything"])
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(true)
    };
    let at = |sl, sc, el, ec| Range {
        start_line: sl,
        start_character: sc,
        end_line: el,
        end_character: ec,
    };

    // Missing semicolon: the preferred quick fix inserts it.
    let fix = root.join("fix.c");
    std::fs::write(&fix, "int main(void) {\n    int x = 1\n    return x;\n}\n").unwrap();
    // clangd reports the error at the next token (`return` on line 2).
    let range = at(2, 4, 2, 4);
    let actions = code_actions(&root, &fix, &range, wait).await.unwrap();
    let quick = actions
        .iter()
        .find(|a| a.kind.as_deref() == Some("quickfix"))
        .unwrap_or_else(|| panic!("{actions:?}"));
    assert!(!quick.fixes.is_empty());
    let plan = plan_code_action(&root, &fix, &range, &quick.title, wait)
        .await
        .unwrap();
    apply_files(&plan).unwrap();
    assert!(compiles(&fix), "{}", std::fs::read_to_string(&fix).unwrap());

    // Extract a subexpression to a variable (executeCommand + applyEdit).
    let ext = root.join("extract.c");
    std::fs::write(
        &ext,
        "int main(void) {\n    int a = 2;\n    return a + a * 3;\n}\n",
    )
    .unwrap();
    let range = at(2, 15, 2, 20); // `a * 3`
    let actions = code_actions(&root, &ext, &range, wait).await.unwrap();
    let extract = actions
        .iter()
        .find(|a| a.title.to_lowercase().contains("extract"))
        .unwrap_or_else(|| panic!("{actions:?}"));
    let plan = plan_code_action(&root, &ext, &range, &extract.title, wait)
        .await
        .unwrap();
    assert_eq!(plan.len(), 1);
    assert!(plan[0].edits.len() >= 2, "{plan:?}");
    apply_files(&plan).unwrap();
    let text = std::fs::read_to_string(&ext).unwrap();
    assert!(!text.contains("a + a * 3"), "{text}");
    assert!(compiles(&ext), "{text}");

    let err = plan_code_action(&root, &ext, &range, "No such action", wait)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("available"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

// SPEC-0043 AK-2: formatting through clangd (clang-format).
#[tokio::test]
async fn clangd_formats_files_and_ranges() {
    use ferris_idea_languages::format::{FormatOptions, plan_format};
    use ferris_idea_languages::insight::Range;
    use ferris_idea_languages::rename::apply_files;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let wait = Duration::from_secs(60);
    let file = root.join("ugly.c");
    let ugly =
        "int add(int a,int b){return a+b;}\nint main(void){\nint x=add(1,2);\n  return x;}\n";
    std::fs::write(&file, ugly).unwrap();

    // Range: only the first line.
    let first = Range {
        start_line: 0,
        start_character: 0,
        end_line: 0,
        end_character: 33,
    };
    let plan = plan_format(&root, &file, Some(&first), FormatOptions::default(), wait)
        .await
        .unwrap();
    assert!(plan.whitespace_only);
    assert!(plan.edits.edits.iter().all(|e| e.range.start_line == 0));
    assert!(plan.formatted.contains("int main(void){\nint x=add(1,2);"));

    // clangd follows clang-format's style (LLVM: 2 spaces) and ignores `tabSize` ...
    let options = FormatOptions {
        tab_size: 8,
        ..FormatOptions::default()
    };
    let plan = plan_format(&root, &file, None, options, wait)
        .await
        .unwrap();
    assert!(plan.whitespace_only);
    assert!(
        plan.formatted.contains("\n  int x = add(1, 2);\n"),
        "{}",
        plan.formatted
    );
    // ... while the project's `.clang-format` takes precedence.
    std::fs::write(
        root.join(".clang-format"),
        "BasedOnStyle: LLVM\nIndentWidth: 4\n",
    )
    .unwrap();
    let plan = plan_format(&root, &file, None, options, wait)
        .await
        .unwrap();
    assert!(!plan.edits.edits.is_empty());
    assert!(
        plan.formatted.contains("\n    int x = add(1, 2);\n"),
        "{}",
        plan.formatted
    );
    apply_files(std::slice::from_ref(&plan.edits)).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), plan.formatted);
    let compiles = std::process::Command::new("cc")
        .args(["-fsyntax-only"])
        .arg(&file)
        .status()
        .map(|s| s.success())
        .unwrap_or(true);
    assert!(compiles);

    // Idempotent.
    let again = plan_format(&root, &file, None, options, wait)
        .await
        .unwrap();
    assert!(again.edits.edits.is_empty(), "{:?}", again.edits);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0072/AK-1: completion and definition on unsaved editor text.
#[tokio::test]
async fn clangd_on_unsaved_text() {
    use ferris_idea_languages::insight::{LocationKind, complete_in, locations_in};
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-unsaved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("draft.c");
    // On disk the helper does not exist yet; only the editor's text has it.
    std::fs::write(&file, "int main(void) {\n    return 0;\n}\n").unwrap();
    let text = "int helper_value(void) { return 1; }\nint main(void) {\n    return helper_v\n}\n";
    let wait = Duration::from_secs(30);
    let items = complete_in(&root, &file, Some(text), 2, 19, 50, wait)
        .await
        .unwrap();
    assert!(
        items.iter().any(|i| i.label.contains("helper_value")),
        "{items:?}"
    );
    let text =
        "int helper_value(void) { return 1; }\nint main(void) {\n    return helper_value();\n}\n";
    let def = locations_in(
        &root,
        &file,
        Some(text),
        2,
        12,
        LocationKind::Definition,
        wait,
    )
    .await
    .unwrap();
    assert_eq!(def.len(), 1, "{def:?}");
    assert_eq!(
        (def[0].range.start_line, def[0].range.start_character),
        (0, 4)
    );
    // SPEC-0074/AK-1: rename on the unsaved text.
    let plan = ferris_idea_languages::rename::plan_rename_in(
        &root,
        &file,
        Some(text),
        2,
        12,
        "value",
        wait,
    )
    .await
    .unwrap();
    assert_eq!(plan.old_name, "helper_value");
    let edits: Vec<(u64, u64)> = plan.files[0]
        .edits
        .iter()
        .map(|e| (e.range.start_line, e.range.start_character))
        .collect();
    assert_eq!(edits, [(0, 4), (2, 11)], "{plan:?}");
    // Edits refer to the unsaved text, not the file on disk (regression: old texts came from disk).
    assert!(
        plan.files[0]
            .edits
            .iter()
            .all(|e| e.old_text == "helper_value"),
        "{plan:?}"
    );
    // SPEC-0075/AK-1: formatting the unsaved text.
    let messy = "int helper_value(void) { return 1; }\nint main(void) { int a = 1; return a; }\n";
    let options = ferris_idea_languages::format::FormatOptions {
        tab_size: 2,
        insert_spaces: true,
        trim_trailing_whitespace: true,
        insert_final_newline: true,
    };
    let formatted = ferris_idea_languages::format::plan_format_in(
        &root,
        &file,
        Some(messy),
        None,
        options,
        wait,
    )
    .await
    .unwrap();
    assert!(
        formatted
            .formatted
            .contains("int main(void) {\n  int a = 1;"),
        "{formatted:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0076: actions and their edits come from the editor's text, not the disk.
#[tokio::test]
async fn clangd_code_actions_on_unsaved_text() {
    use ferris_idea_languages::actions::{code_actions_in, plan_code_action_in};
    use ferris_idea_languages::insight::Range;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-actions-unsaved-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("draft.c");
    std::fs::write(&file, "int main(void) {\n    return 0;\n}\n").unwrap();
    // Only the editor's text lacks the semicolon the server offers to insert; clangd reports the
    // error at the next token (`return` on line 3).
    let text = "int main(void) {\n    int x = 1\n    return x;\n}\n";
    let at = Range {
        start_line: 2,
        start_character: 4,
        end_line: 2,
        end_character: 4,
    };
    let wait = Duration::from_secs(30);
    let actions = code_actions_in(&root, &file, Some(text), &at, wait)
        .await
        .unwrap();
    let fix = actions
        .iter()
        .find(|a| a.kind.as_deref() == Some("quickfix"))
        .unwrap_or_else(|| panic!("{actions:?}"));
    let plan = plan_code_action_in(&root, &file, Some(text), &at, &fix.title, wait)
        .await
        .unwrap();
    assert_eq!(plan.len(), 1, "{plan:?}");
    let edit = &plan[0].edits[0];
    assert_eq!(edit.new_text, ";");
    assert_eq!((edit.range.start_line, edit.range.start_character), (1, 13));
    // At the start of the line, before the token the error is reported at, the fix is still found.
    let line_start = Range {
        start_line: 2,
        start_character: 0,
        end_line: 2,
        end_character: 0,
    };
    let actions = code_actions_in(&root, &file, Some(text), &line_start, wait)
        .await
        .unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a.kind.as_deref() == Some("quickfix")),
        "{actions:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0077: documentation of a symbol that exists only in the editor's text, as Markdown.
#[tokio::test]
async fn clangd_hover_on_unsaved_text() {
    use ferris_idea_languages::insight::hover_in;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("ferris-idea-hover-unsaved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("draft.c");
    std::fs::write(&file, "int main(void) {\n    return 0;\n}\n").unwrap();
    let text = "/// Adds one.\nint plus_one(int x) { return x + 1; }\nint main(void) {\n    return plus_one(1);\n}\n";
    let h = hover_in(&root, &file, Some(text), 3, 12, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("a hover");
    assert!(h.text.contains("```"), "{}", h.text);
    assert!(h.text.contains("int plus_one(int x)"), "{}", h.text);
    assert!(h.text.contains("Adds one."), "{}", h.text);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0079: symbols of the editor's text, nested, not of the disk.
#[tokio::test]
async fn clangd_symbols_on_unsaved_text() {
    use ferris_idea_languages::insight::symbols_in;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-symbols-unsaved-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("draft.c");
    std::fs::write(&file, "int main(void) {\n    return 0;\n}\n").unwrap();
    let text = "struct point { int x; int y; };\nint length(void) { return 0; }\nint main(void) {\n    return 0;\n}\n";
    let symbols = symbols_in(&root, &file, Some(text), Duration::from_secs(30))
        .await
        .unwrap();
    let names: Vec<(&str, usize)> = symbols.iter().map(|s| (s.name.as_str(), s.depth)).collect();
    assert_eq!(
        names,
        [("point", 0), ("x", 1), ("y", 1), ("length", 0), ("main", 0)],
        "{symbols:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0080: the signature of the call around the caret, with the parameter the caret is in.
#[tokio::test]
async fn clangd_signature_help_on_unsaved_text() {
    use ferris_idea_languages::insight::signature_help_in;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-signature-unsaved-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("draft.c");
    std::fs::write(&file, "int main(void) {\n    return 0;\n}\n").unwrap();
    let text = "int add(int a, int b) { return a + b; }\nint main(void) { return add(1, 2); }\n";
    let help = signature_help_in(&root, &file, Some(text), 1, 31, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("a signature");
    let sig = &help.signatures[help.active_signature];
    assert!(sig.label.contains("add(int a, int b)"), "{sig:?}");
    let active = sig.active_parameter.expect("an active parameter");
    let name: String = sig
        .label
        .chars()
        .skip(sig.parameters[active].start)
        .take(sig.parameters[active].len())
        .collect();
    assert_eq!(name, "int b");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0081: type declaration of a variable and implementations of a virtual method.
#[tokio::test]
async fn clangd_type_definition_and_implementations() {
    use ferris_idea_languages::insight::{LocationKind, locations_in};
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-impl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("shapes.cpp");
    std::fs::write(&file, "").unwrap();
    let text = "struct Shape { virtual int area(); };\n\
                struct Square : Shape { int area() override; };\n\
                struct Circle : Shape { int area() override; };\n\
                Square s;\n";
    let wait = Duration::from_secs(30);
    let ty = locations_in(
        &root,
        &file,
        Some(text),
        3,
        7,
        LocationKind::TypeDefinition,
        wait,
    )
    .await
    .unwrap();
    assert_eq!(
        ty.iter().map(|l| l.range.start_line).collect::<Vec<_>>(),
        [1],
        "{ty:?}"
    );
    let mut lines: Vec<u64> = locations_in(
        &root,
        &file,
        Some(text),
        0,
        27,
        LocationKind::Implementation,
        wait,
    )
    .await
    .unwrap()
    .iter()
    .map(|l| l.range.start_line)
    .collect();
    lines.sort();
    assert_eq!(lines, [1, 2]);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0082: callers and callees of the editor's text, with call sites and recursion marked.
#[tokio::test]
async fn clangd_call_hierarchy_on_unsaved_text() {
    use ferris_idea_languages::insight::{CallDirection, CallNode, call_hierarchy_in};
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-calls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("calls.c");
    std::fs::write(&file, "").unwrap();
    let text = "int leaf(void) { return 1; }\n\
                int mid(void) { return leaf() + leaf(); }\n\
                int top(void) { return mid(); }\n\
                int rec(int n) { return n ? rec(n - 1) : leaf(); }\n";
    let wait = Duration::from_secs(30);
    fn names(n: &CallNode) -> Vec<String> {
        let mut v: Vec<String> = n.children.iter().map(|c| c.name.clone()).collect();
        v.sort();
        v
    }
    let callers = call_hierarchy_in(
        &root,
        &file,
        Some(text),
        0,
        5,
        CallDirection::Callers,
        3,
        wait,
    )
    .await
    .unwrap()
    .expect("leaf is a function");
    assert_eq!(callers.name, "leaf");
    assert_eq!(names(&callers), ["mid", "rec"]);
    let mid = callers.children.iter().find(|c| c.name == "mid").unwrap();
    assert_eq!(mid.call_sites.len(), 2, "{mid:?}");
    assert!(mid.call_sites.iter().all(|r| r.start_line == 1));
    assert_eq!(names(mid), ["top"]);
    let rec = callers.children.iter().find(|c| c.name == "rec").unwrap();
    let again = rec
        .children
        .iter()
        .find(|c| c.name == "rec")
        .expect("rec calls itself");
    assert!(again.recursive && again.children.is_empty(), "{again:?}");

    let callees = call_hierarchy_in(
        &root,
        &file,
        Some(text),
        2,
        5,
        CallDirection::Callees,
        3,
        wait,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(names(&callees), ["mid"]);
    assert_eq!(names(&callees.children[0]), ["leaf"]);
    assert_eq!(callees.children[0].call_sites[0].start_line, 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// The range of `needle` in `text`, or with `caret` the empty range `caret` characters into it.
fn range_of(
    text: &str,
    needle: &str,
    caret: Option<usize>,
) -> ferris_idea_languages::insight::Range {
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
    ferris_idea_languages::insight::Range {
        start_line,
        start_character,
        end_line,
        end_character,
    }
}

/// SPEC-0095 v2 AK-4: the refactoring shortcuts against clangd on the hand test's `shapes.cpp`
/// (round 4, Nr. 14): a selected expression, a bare caret widened to the expression around it, a
/// selection clangd refuses with a message in the user's terms, Extract Method on a line, and no
/// Inline (clangd has none).
#[tokio::test]
async fn clangd_refactoring_shortcuts() {
    use ferris_idea_languages::actions::{Refactoring, plan_code_action_in, refactoring_targets};
    use ferris_idea_languages::rename::apply_edits;
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    const SHAPES: &str = "// Handtest Ferris IDEA: Navigation und Code-Einsicht
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
    let root = std::env::temp_dir().join(format!("ferris-idea-refactor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("shapes.cpp");
    // On disk an older text: every answer must come from the editor's text.
    std::fs::write(&file, "int main() { return 0; }\n").unwrap();
    let wait = Duration::from_secs(60);
    let targets = async |range: ferris_idea_languages::insight::Range, r| {
        refactoring_targets(&root, &file, Some(SHAPES), &range, r, wait).await
    };

    // A selected expression: one place, clangd's tweak, applied through executeCommand/applyEdit.
    let selected = range_of(SHAPES, "s.area()", None);
    let found = targets(selected.clone(), Refactoring::Variable)
        .await
        .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].range, selected);
    assert_eq!(found[0].text, "s.area()");
    let title = &found[0].actions[0].title;
    assert!(title.to_lowercase().contains("variable"), "{title}");
    let plan = plan_code_action_in(&root, &file, Some(SHAPES), &selected, title, wait)
        .await
        .unwrap();
    let changed = apply_edits(SHAPES, &plan[0].edits).unwrap();
    assert!(changed.contains("= s.area();"), "{changed}");
    assert!(!changed.contains("add(s.area(), 1)"), "{changed}");

    // A bare caret inside `side * side`: clangd needs a selection, so the expression around the
    // caret is offered.
    let caret = range_of(SHAPES, "side * side", Some(5));
    let found = targets(caret, Refactoring::Variable).await.unwrap();
    assert_eq!(
        found.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
        ["side * side"],
        "{found:?}"
    );

    // The whole initial value of `total`: clangd does not extract it and gives no reason.
    let initial = range_of(SHAPES, "add(s.area(), 1)", None);
    let found = targets(initial, Refactoring::Variable).await.unwrap();
    assert!(found.is_empty(), "{found:?}");
    let message = Refactoring::Variable.none_message("clangd", Some("add(s.area(), 1)"));
    assert!(
        message.starts_with("clangd cannot extract `add(s.area(), 1)` into a variable."),
        "{message}"
    );

    // A whole line: Extract Method.
    let line = range_of(SHAPES, "    int total = add(s.area(), 1);\n", None);
    let found = targets(line.clone(), Refactoring::Method).await.unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].actions[0]
            .title
            .to_lowercase()
            .contains("function"),
        "{found:?}"
    );
    // Hand test 210 step 6 (SPEC-0095 v5 AK-12): clangd's Extract to function puts `extracted`
    // above `main`; the name is found at its declaration, where the name popup opens.
    let title = &found[0].actions[0].title;
    let plan = plan_code_action_in(&root, &file, Some(SHAPES), &line, title, wait)
        .await
        .unwrap();
    let with_function = apply_edits(SHAPES, &plan[0].edits).unwrap();
    assert!(with_function.contains("extracted(s);"), "{with_function}");
    let name = ferris_idea_languages::actions::introduced_name(SHAPES, &with_function).unwrap();
    assert_eq!(name.name, "extracted");
    let declaration = with_function.lines().nth(name.line as usize).unwrap();
    assert!(
        declaration.starts_with("void extracted(") && name.character == 5,
        "{name:?}: {declaration}"
    );

    // Refactor This on a bare name (hand test 14): clangd lists nothing there, so Ferris IDEA's own
    // Rename and extractions make the list (SPEC-0095 v5 behaviour 4a).
    let on_name = range_of(SHAPES, "total + missing", Some(2));
    let found = targets(on_name, Refactoring::Any).await.unwrap();
    let actions: Vec<_> = found.into_iter().flat_map(|t| t.actions).collect();
    let own = ferris_idea_languages::actions::refactor_this_own(
        &actions,
        &ferris_idea_languages::actions::RefactorSpot {
            on_name: true,
            in_code: true,
            ..Default::default()
        },
    );
    assert_eq!(own[0].title, "Rename…", "{own:?}");
    // With `s.area()` selected clangd lists its Extract Variable; after the extraction the same
    // text is a variable's whole value and clangd lists nothing (the cause of the hand test's
    // second ⌃T message).
    let found = targets(selected.clone(), Refactoring::Any).await.unwrap();
    assert!(
        found
            .iter()
            .flat_map(|t| &t.actions)
            .any(|a| Refactoring::Variable.matches(a)),
        "{found:?}"
    );
    let in_declaration = range_of(&changed, "s.area()", None);
    let found = refactoring_targets(
        &root,
        &file,
        Some(&changed),
        &in_declaration,
        Refactoring::Any,
        wait,
    )
    .await
    .unwrap();
    assert!(found.is_empty(), "{found:?}");

    // clangd has no Inline; Refactor This at a caret in `side * side` lists its refactorings.
    let on_total = range_of(SHAPES, "total + missing", Some(2));
    let found = targets(on_total, Refactoring::Inline).await.unwrap();
    assert!(found.is_empty(), "{found:?}");
    let any = targets(range_of(SHAPES, "side * side", Some(5)), Refactoring::Any)
        .await
        .unwrap();
    assert!(!any.is_empty(), "{any:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0295/AK-7: the occurrences of a local variable within its file, with their kinds, from
/// clangd's `textDocument/documentHighlight`.
#[tokio::test]
async fn clangd_highlights_the_usages_in_a_file() {
    use ferris_idea_languages::occurrences::{Highlights, OccurrenceKind, document_highlights_in};
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-occurrences-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("sum.c");
    // `total` is declared, written and read; `other` only shares letters with nothing.
    let text = "int sum(int n) {\n    int total = 0;\n    int other = n;\n    total = total + other;\n    return total;\n}\n";
    std::fs::write(&file, text).unwrap();
    let wait = Duration::from_secs(30);
    // The caret inside `total` on the line of its assignment, on the unsaved text.
    let found = document_highlights_in(&root, &file, Some(text), 3, 6, wait, None, true)
        .await
        .unwrap();
    let Highlights::Found(found) = found else {
        panic!("clangd offers usage highlights: {found:?}");
    };
    let places: Vec<(u64, u64, u64, OccurrenceKind)> = found
        .iter()
        .map(|o| {
            (
                o.range.start_line,
                o.range.start_character,
                o.range.end_character,
                o.kind,
            )
        })
        .collect();
    assert_eq!(
        places,
        [
            // clangd gives the declaration no kind, although it has an initializer.
            (1, 8, 13, OccurrenceKind::Text),
            (3, 4, 9, OccurrenceKind::Write),
            (3, 12, 17, OccurrenceKind::Read),
            (4, 11, 16, OccurrenceKind::Read),
        ]
    );
    // White space has no symbol: an empty answer, not an error.
    let none = document_highlights_in(&root, &file, Some(text), 5, 1, wait, None, true)
        .await
        .unwrap();
    assert_eq!(none, Highlights::Found(Vec::new()));
    let _ = std::fs::remove_dir_all(&root);
}

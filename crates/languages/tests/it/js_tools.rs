//! SPEC-0123 AK-2: the project's real ESLint 9 and Prettier 3 (`js.sh install`
//! of the reference environment; folder from `FERRIS_IDEA_TEST_JSLINT`). Skipped when they are not installed.

use std::path::PathBuf;

use ferris_idea_languages::js_tools::{
    LintSeverity, eslint_check, eslint_fix, find_eslint, find_prettier, minimal_edit,
    prettier_format,
};

const APP: &str = include_str!("../../../../tests/reference-projects/js-lint/src/app.js");

fn project() -> Option<PathBuf> {
    let dir = ferris_idea_core::product_var_os("FERRIS_IDEA_TEST_JSLINT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("ReferenceEnv/projects/js-lint"))
        })?;
    if dir.join("node_modules/.bin/eslint").exists() {
        Some(dir)
    } else {
        eprintln!("skipped: run js.sh install of the reference environment");
        None
    }
}

#[test]
fn eslint_reports_and_fixes_unsaved_text() {
    let Some(dir) = project() else { return };
    let file = dir.join("src/app.js");
    let eslint = find_eslint(&dir, &file).expect("eslint of the project");
    let problems = eslint_check(&eslint, &file, APP).unwrap();
    let rules: Vec<(&str, LintSeverity, &str)> = problems
        .iter()
        .map(|p| {
            (
                p.rule.as_deref().unwrap_or(""),
                p.severity,
                &APP[p.range.clone()],
            )
        })
        .collect();
    assert_eq!(
        rules,
        [
            ("prefer-const", LintSeverity::Error, "total"),
            ("no-unused-vars", LintSeverity::Error, "unused"),
            ("eqeqeq", LintSeverity::Warning, "=="),
        ]
    );
    assert!(problems[0].fixable && !problems[1].fixable);
    let fixed = eslint_fix(&eslint, &file, APP).unwrap().unwrap();
    assert!(fixed.starts_with("const total = 0\n"), "{fixed}");
    assert_eq!(
        eslint_fix(&eslint, &file, &fixed).unwrap(),
        None,
        "nothing left to fix"
    );
    // A syntax error is a finding, not a failure.
    let broken = eslint_check(&eslint, &file, "let x = ;\n").unwrap();
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].rule, None);
}

#[test]
fn prettier_formats_with_the_project_configuration() {
    let Some(dir) = project() else { return };
    let file = dir.join("src/app.js");
    let prettier = find_prettier(&dir, &file).expect("prettier of the project");
    let formatted = prettier_format(&prettier, &file, APP).unwrap();
    assert_eq!(
        formatted,
        "let total = 0;\nconst unused = 1;\nexport function add(a, b) {\n  if (a == b) {\n    return a * 2;\n  }\n  return a + b;\n}\nconsole.log(add(total, 2));\n"
    );
    let edit = minimal_edit(APP, &formatted).unwrap();
    assert_eq!(edit.range.start, "let total = 0".len());
    assert!(
        prettier_format(&prettier, &file, "let x = ;")
            .unwrap_err()
            .starts_with("Prettier:")
    );
    // Files Prettier does not know are left to others.
    assert!(find_prettier(&dir, &dir.join("src/main.rs")).is_none());
}

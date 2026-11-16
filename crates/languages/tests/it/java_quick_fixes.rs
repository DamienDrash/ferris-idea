//! SPEC-0076 v6 AK-7 and v7 AK-8 (F-033, OBS-0027 JQ1, JQ1b and JQ2) with a real jdtls: an unused
//! import, private field, private method and local variable are weak warnings (marked unnecessary)
//! with jdtls's removal fixes; the fixes of a type error are found at its exact place, because the
//! question sends jdtls's refined diagnostics as context, not the first list of its build
//! (behaviour 10); a primitive assigned to a String gets IDECraft's Wrap using 'String.valueOf()'
//! (behaviour 11), and an unused local in a method with an error, which JDT does not check, gets
//! IDECraft's removal (behaviour 12). Skipped without `jdtls`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use idecraft_languages::actions::{code_actions_in, plan_code_action_in};
use idecraft_languages::insight::Range;
use idecraft_languages::rename::apply_edits;

const WAIT: Duration = Duration::from_secs(120);

/// Unused things in a file without errors (JDT reports unused imports only then).
const UNUSED: &str = "package lab;\n\nimport java.util.List;\n\npublic class Probe {\n    private int never;\n\n    private void helper() {\n    }\n\n    void run() {\n        int unused = 3;\n    }\n}\n";

/// The file of the comparison: the type error in a method of its own (JDT skips the unused locals
/// of a method with an error).
const MISMATCH: &str = "package lab;\n\npublic class Qf {\n    void run() {\n        int unused = 3;\n    }\n\n    void other() {\n        String text = 5;\n    }\n}\n";

/// An unused local in the same method as an unresolved type: JDT reports no unused code there.
const IN_ERROR: &str = "package lab;\n\npublic class Qf {\n    void run() {\n        int unused = 3;\n        Object list = new ArrayList<String>();\n        System.out.println(list);\n    }\n}\n";

fn caret(line: u64, character: u64) -> Range {
    Range {
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character,
    }
}

/// A project with `text` as `src/main/java/lab/<name>`, or `None` (skipped) without jdtls.
fn project(tag: &str, name: &str, text: &str) -> Option<(PathBuf, PathBuf)> {
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err() {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return None;
    }
    let root = std::env::temp_dir().join(format!("idecraft-java-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/lab")).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main/java/lab").join(name);
    std::fs::write(&file, text).unwrap();
    Some((root, file))
}

async fn titles(root: &Path, file: &Path, text: &str, at: Range) -> Vec<String> {
    code_actions_in(root, file, Some(text), &at, WAIT)
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect()
}

#[tokio::test]
async fn unused_code_is_a_weak_warning_with_a_removal_fix() {
    let Some((root, file)) = project("unused", "Probe.java", UNUSED) else {
        return;
    };
    let found = idecraft_languages::lsp::file_diagnostics(&root, &file, WAIT)
        .await
        .unwrap();
    for (line, says) in [
        (2, "The import java.util.List is never used"),
        (5, "The value of the field Probe.never is not used"),
        (
            7,
            "The method helper() from the type Probe is never used locally",
        ),
        (11, "The value of the local variable unused is not used"),
    ] {
        let d = found
            .iter()
            .find(|d| d.start_line == line)
            .unwrap_or_else(|| panic!("no problem on line {line}: {found:?}"));
        assert_eq!(
            (d.severity.as_str(), d.unnecessary, d.message.as_str()),
            ("warning", true, says),
            "{d:?}"
        );
    }

    let local = titles(&root, &file, UNUSED, caret(11, 12)).await;
    for want in [
        "Remove 'unused' and all assignments",
        "Remove 'unused', keep assignments with side effects",
    ] {
        assert!(local.iter().any(|t| t == want), "{want}: {local:?}");
    }
    let import = titles(&root, &file, UNUSED, caret(2, 10)).await;
    assert!(
        import.iter().any(|t| t == "Remove unused import"),
        "{import:?}"
    );
    let field = titles(&root, &file, UNUSED, caret(5, 16)).await;
    assert!(
        field
            .iter()
            .any(|t| t == "Remove 'never', keep assignments with side effects"),
        "{field:?}"
    );
    let method = titles(&root, &file, UNUSED, caret(7, 18)).await;
    assert!(
        method
            .iter()
            .any(|t| t.starts_with("Remove") && t.contains("helper")),
        "{method:?}"
    );

    let files = plan_code_action_in(
        &root,
        &file,
        Some(UNUSED),
        &caret(11, 12),
        "Remove 'unused' and all assignments",
        WAIT,
    )
    .await
    .unwrap();
    let after = apply_edits(UNUSED, &files[0].edits).unwrap();
    assert!(!after.contains("unused"), "{after}");
    assert!(after.contains("    void run() {\n"), "{after}");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn the_fixes_of_a_type_error_are_found_at_its_place() {
    let Some((root, file)) = project("mismatch", "Qf.java", MISMATCH) else {
        return;
    };
    // At the literal and at the start of its line (SPEC-0076 behaviour 1): jdtls's first list
    // names the error by its line only and would get no fix for it.
    for at in [caret(8, 22), caret(8, 0)] {
        let offered = titles(&root, &file, MISMATCH, at).await;
        assert!(
            offered
                .iter()
                .any(|t| t == "Change type of 'text' to 'int'"),
            "{offered:?}"
        );
        // JQ2: jdtls has no wrapping; IDECraft offers its own, once (SPEC-0076 v7 behaviour 11).
        assert_eq!(
            offered
                .iter()
                .filter(|t| *t == "Wrap using 'String.valueOf()'")
                .count(),
            1,
            "{offered:?}"
        );
    }
    let files = plan_code_action_in(
        &root,
        &file,
        Some(MISMATCH),
        &caret(8, 22),
        "Wrap using 'String.valueOf()'",
        WAIT,
    )
    .await
    .unwrap();
    let after = apply_edits(MISMATCH, &files[0].edits).unwrap();
    assert!(
        after.contains("        String text = String.valueOf(5);\n"),
        "{after}"
    );
    // The unused local of a method without an error is still offered for removal.
    let local = titles(&root, &file, MISMATCH, caret(4, 12)).await;
    assert!(
        local
            .iter()
            .any(|t| t == "Remove 'unused' and all assignments"),
        "{local:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn an_unused_local_in_a_method_with_an_error_can_be_removed() {
    let Some((root, file)) = project("in-error", "Qf.java", IN_ERROR) else {
        return;
    };
    // JDT reports the unresolved type, not the unused local, so jdtls offers no removal; IDECraft
    // offers the one of its inspection (SPEC-0076 v7 behaviour 12).
    let offered = titles(&root, &file, IN_ERROR, caret(4, 12)).await;
    assert!(
        !offered.iter().any(|t| t.starts_with("Remove 'unused'")),
        "{offered:?}"
    );
    assert!(
        offered.iter().any(|t| t == "Remove variable 'unused'"),
        "{offered:?}"
    );
    let files = plan_code_action_in(
        &root,
        &file,
        Some(IN_ERROR),
        &caret(4, 12),
        "Remove variable 'unused'",
        WAIT,
    )
    .await
    .unwrap();
    let after = apply_edits(IN_ERROR, &files[0].edits).unwrap();
    assert!(!after.contains("unused"), "{after}");
    assert!(
        after.contains("    void run() {\n        Object list"),
        "{after}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

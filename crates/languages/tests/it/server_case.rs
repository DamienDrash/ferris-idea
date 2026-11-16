//! One language's features against a real server (F-035, F-036, F-038), shared by `servers.rs`
//! and, for jdtls through the language server pool, by `java_live/java_server.rs`.

use std::path::PathBuf;
use std::time::Duration;

use idecraft_languages::insight::{
    CallDirection, Location, LocationKind, call_hierarchy_in, complete_in, hover_in, locations_in,
    signature_help_in, symbols_in,
};
use idecraft_languages::rename::plan_rename_in;

pub(crate) const WAIT: Duration = Duration::from_secs(120);

pub(crate) fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

/// Whether IDECraft would start `program`: on the `PATH` and usable (typescript-language-server
/// with TypeScript 7 next to it is not, `SPEC-0042` v12 behaviour 21).
pub(crate) fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        matches!(
            idecraft_languages::servers::find_in(&paths, program),
            idecraft_languages::servers::Availability::Ready(_)
        )
    })
}

/// One language: a project, a program and where its symbols are (0-based lines and characters).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) server: &'static str,
    /// Extra project files (path, content).
    pub(crate) files: &'static [(&'static str, &'static str)],
    pub(crate) file: &'static str,
    pub(crate) text: &'static str,
    /// The call of `add` in `main` and the definition of `add`.
    pub(crate) call: (u64, u64),
    pub(crate) definition: (u64, u64),
    /// Where the second argument of that call is, and the parameter's label.
    pub(crate) second_argument: (u64, u64),
    pub(crate) parameter: &'static str,
    /// The variable holding the square, and the line of its type (`None`: the server lacks it).
    pub(crate) variable: (u64, u64),
    pub(crate) type_line: Option<u64>,
    pub(crate) doc: &'static str,
    /// The function's name.
    pub(crate) function: &'static str,
    /// The call line with `add(…)` cut to its first two letters, and where the caret is then.
    pub(crate) typing: (&'static str, &'static str, u64, u64),
    pub(crate) top_symbols: &'static [&'static str],
    /// Whether the server offers the call hierarchy.
    pub(crate) calls: bool,
    /// The new name for `add` (`None`: the server does not rename).
    pub(crate) rename_to: Option<&'static str>,
}

pub(crate) fn project(case: &Case) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("idecraft-{}-{}", case.name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, content) in case.files {
        std::fs::write(root.join(path), content).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let file = root.join(case.file);
    std::fs::write(&file, "\n").unwrap();
    (root, file)
}

pub(crate) fn lines(found: &[Location]) -> Vec<u64> {
    let mut v: Vec<u64> = found.iter().map(|l| l.range.start_line).collect();
    v.sort();
    v.dedup();
    v
}

pub(crate) async fn check(case: &Case) {
    if !installed(case.server) {
        skip(&format!("{} not installed", case.server));
        return;
    }
    let (root, file) = project(case);
    let text = Some(case.text);
    let name = case.name;

    let hover = hover_in(&root, &file, text, case.call.0, case.call.1, WAIT)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{name}: a hover for add"));
    assert!(hover.text.contains(case.doc), "{name}: {}", hover.text);

    let (cl, cc) = case.call;
    let (dl, dc) = case.definition;
    let def = locations_in(&root, &file, text, cl, cc, LocationKind::Definition, WAIT)
        .await
        .unwrap();
    assert_eq!(lines(&def), [dl], "{name}: definition");
    let refs = locations_in(&root, &file, text, dl, dc, LocationKind::References, WAIT)
        .await
        .unwrap();
    assert_eq!(lines(&refs), [dl, cl], "{name}: references");
    if let Some(type_line) = case.type_line {
        let (vl, vc) = case.variable;
        let ty = locations_in(
            &root,
            &file,
            text,
            vl,
            vc,
            LocationKind::TypeDefinition,
            WAIT,
        )
        .await
        .unwrap();
        assert_eq!(lines(&ty), [type_line], "{name}: type definition");
    }

    let (al, ac) = case.second_argument;
    let help = signature_help_in(&root, &file, text, al, ac, WAIT)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{name}: a signature"));
    let sig = &help.signatures[help.active_signature];
    let active = sig
        .active_parameter
        .unwrap_or_else(|| panic!("{name}: an active parameter in {sig:?}"));
    let label: String = sig
        .label
        .chars()
        .skip(sig.parameters[active].start)
        .take(sig.parameters[active].len())
        .collect();
    assert_eq!(label, case.parameter, "{name}: {sig:?}");

    let symbols = symbols_in(&root, &file, text, WAIT).await.unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    for want in case.top_symbols {
        assert!(names.contains(want), "{name}: {names:?}");
    }

    if case.calls {
        let callers =
            call_hierarchy_in(&root, &file, text, dl, dc, CallDirection::Callers, 2, WAIT)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{name}: add is a function"));
        assert!(
            callers.children.iter().any(|c| c.name.contains("main")),
            "{name}: {callers:?}"
        );
    }

    let (from, to, tl, tc) = case.typing;
    let typed = case.text.replace(from, to);
    let items = complete_in(&root, &file, Some(&typed), tl, tc, 100, WAIT)
        .await
        .unwrap();
    assert!(
        items
            .iter()
            .any(|i| i.label.trim().starts_with(case.function)),
        "{name}: {items:?}"
    );

    if let Some(new_name) = case.rename_to {
        let plan = plan_rename_in(&root, &file, text, dl, dc, new_name, WAIT)
            .await
            .unwrap();
        assert_eq!(plan.files.len(), 1, "{name}: {plan:?}");
        // gopls also renames the name in the doc comment; jdtls sends one edit spanning both places.
        let edits = &plan.files[0].edits;
        let covered = |line: u64| {
            edits
                .iter()
                .any(|e| e.range.start_line <= line && line <= e.range.end_line)
        };
        assert!(covered(dl) && covered(cl), "{name}: {plan:?}");
        assert!(
            edits.iter().all(|e| e.new_text.contains(new_name)),
            "{name}: {plan:?}"
        );
    }

    // A server of the language server pool, where a test program switched it on, is stopped.
    idecraft_languages::pool::restart_root(&root);
    let _ = std::fs::remove_dir_all(&root);
}

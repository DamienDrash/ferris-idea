//! SPEC-0053 v8 AK-13: the ratchet over the tool list. Every tool has a row in `TOOLS`;
//! a tool with a changing effect checks a right; and the checks a row names are made in the
//! tool's code. A new tool cannot be added without its row, and a row cannot claim a check the
//! handler does not make.

use std::collections::{BTreeMap, BTreeSet};

use ferris_idea_mcp::effects::{Check, TOOLS};

/// The sources that hold the tool handlers and the helpers they call: every file under `src/`
/// with an `impl IdeServer` block, so a new tool module is read without being named here
/// (ADR-0035), and the window's interface and the dialogs list.
fn sources() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    walk(&src, &mut all);
    all.retain(|path| {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        text.contains("impl IdeServer")
            || path.ends_with("dialogs.rs")
            || path.ends_with("window.rs")
    });
    all
}

/// The tools that write nothing but Ferris IDEA's own records (`Check::OwnRecords`).
const OWN_RECORDS_TOOLS: [&str; 4] = [
    "local_history",
    "change_set_begin",
    "change_set_edit",
    "change_set_reject",
];

/// Calls in a handler's code that write a file directly. A tool whose row says it only reads
/// must not reach one of them.
const WRITE_CALLS: [&str; 11] = [
    "fs::write(",
    "fs::remove_file(",
    "fs::remove_dir_all(",
    "fs::rename(",
    "fs::create_dir_all(",
    "fs::copy(",
    "write_atomic(",
    "save_ide_settings(",
    "save_profiles(",
    ".save(",
    ".write(",
];

/// The name of the function a line starts, when it is a method (four spaces) or a free
/// function (no indentation): `(indentation, name)`.
fn function_start(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if !(indent.is_empty() || indent == "    ") {
        return None;
    }
    let mut rest = trimmed;
    for prefix in ["pub(crate) ", "pub(super) ", "pub ", "async "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let rest = rest.strip_prefix("fn ")?;
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| (indent, &rest[..end]))
}

/// Every function of the sources by name, with its text (a name can have several).
pub(super) fn functions() -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in sources() {
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if let Some((indent, name)) = function_start(lines[i]) {
                let close = format!("{indent}}}");
                let end = (i..lines.len())
                    .find(|j| lines[*j] == close)
                    .unwrap_or(lines.len() - 1);
                out.entry(name.to_owned())
                    .or_default()
                    .push(lines[i..=end].join("\n"));
                i = end;
            }
            i += 1;
        }
    }
    out
}

/// The methods `body` calls on the server: `self.name(` (also across a line break) and
/// `Self::name(`.
fn calls(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let name_at = |text: &str| -> Option<String> {
        let end = text
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(text.len());
        (end > 0 && text[end..].starts_with('(')).then(|| text[..end].to_owned())
    };
    for (at, _) in body.match_indices("self") {
        let rest = body[at + 4..].trim_start();
        if let Some(rest) = rest.strip_prefix('.')
            && let Some(name) = name_at(rest.trim_start())
        {
            out.insert(name);
        }
    }
    for (at, _) in body.match_indices("Self::") {
        if let Some(name) = name_at(&body[at + 6..]) {
            out.insert(name);
        }
    }
    out
}

/// The text of `tool`'s handler and of the helpers it calls, four calls deep.
pub(super) fn code_of(tool: &str, functions: &BTreeMap<String, Vec<String>>) -> String {
    let mut seen = BTreeSet::new();
    let mut text = String::new();
    let mut todo = vec![(tool.to_owned(), 0)];
    while let Some((name, depth)) = todo.pop() {
        let Some(bodies) = functions.get(&name) else {
            continue;
        };
        if !seen.insert(name) {
            continue;
        }
        for body in bodies {
            text.push_str(body);
            text.push('\n');
            if depth < 4 {
                todo.extend(calls(body).into_iter().map(|c| (c, depth + 1)));
            }
        }
    }
    text
}

/// Whether `code` names `word` as a whole word (not as the start of a longer name).
fn names(code: &str, word: &str) -> bool {
    code.match_indices(word).any(|(at, _)| {
        !code[at + word.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    })
}

/// Whether `code` makes `check`; `own` is the handler alone.
fn makes(check: Check, code: &str, own: &str) -> bool {
    match check {
        Check::Right(grant) => names(code, &format!("Grant::{grant:?}")),
        Check::Database => ["require_db(", "require_db_sql(", "require_store("]
            .iter()
            .any(|c| code.contains(c)),
        Check::Trusted => code.contains("require_trusted(") || code.contains("usable_by_agents("),
        Check::AsksUser => own.contains("ask_user("),
        Check::EmergencyStop | Check::OwnRecords => true,
    }
}

#[tokio::test]
async fn every_tool_with_a_changing_effect_checks_a_right() {
    let listed: BTreeSet<String> = super::protocol::tool_names().await.into_iter().collect();
    let functions = functions();
    let mut problems: Vec<String> = Vec::new();

    // One row per tool, and no row without a tool.
    let mut rows = BTreeSet::new();
    for row in TOOLS.iter() {
        if !rows.insert(row.tool) {
            problems.push(format!("{}: two rows in TOOLS", row.tool));
        }
        if !listed.contains(row.tool) {
            problems.push(format!("{}: a row in TOOLS, but no such tool", row.tool));
        }
    }
    for tool in &listed {
        if !rows.contains(tool.as_str()) {
            problems.push(format!(
                "{tool}: no row in TOOLS; classify the new tool by what its code does, in its module's TOOL_EFFECTS (crates/mcp/src/effects.rs for tools of lib.rs)"
            ));
        }
    }

    for row in TOOLS.iter() {
        let Some(own) = functions.get(row.tool).map(|bodies| bodies.join("\n")) else {
            problems.push(format!("{}: no handler found in the sources", row.tool));
            continue;
        };
        let code = code_of(row.tool, &functions);
        // A changing effect needs a right or a database permission; trust alone is no right.
        let guards = row
            .checks
            .iter()
            .filter(|c| !matches!(c, Check::Trusted))
            .count();
        if !row.changes.is_empty() && guards == 0 {
            problems.push(format!(
                "{}: changes {:?} without a right or database permission",
                row.tool, row.changes
            ));
        }
        // A tool that uses a database connection checks the connection's permission, also
        // when it only reads (SPEC-0012 v3 behaviour 8).
        if row.uses_database() && !row.checks.contains(&Check::Database) {
            problems.push(format!(
                "{}: uses a database connection without checking its permission",
                row.tool
            ));
        }
        // The same row decides the agent log: what changes or uses a database is logged
        // (SPEC-0053 v7 behaviour 11), so a guarded tool cannot stay out of the log.
        if !row.checks.is_empty() && row.reaches.is_empty() && row.changes.is_empty() {
            // A reading tool with a check (debug_variables, ui_snapshot, terminal_read):
            // nothing to log, nothing to guard beyond the check it makes.
        } else if (!row.changes.is_empty() || row.uses_database()) && !row.logged() {
            problems.push(format!("{}: changes something but is not logged", row.tool));
        }
        // The two exemptions are named tools: the local history and the change-set tools that
        // only propose or reject (SPEC-0340 behaviours 6 and 16).
        if row.checks.contains(&Check::OwnRecords) && !OWN_RECORDS_TOOLS.contains(&row.tool) {
            problems.push(format!(
                "{}: only {OWN_RECORDS_TOOLS:?} write nothing but Ferris IDEA's own records",
                row.tool
            ));
        }
        // The emergency stop is the one tool that changes something without a right.
        if row.checks.contains(&Check::EmergencyStop) && row.tool != "ui_driver_stop" {
            problems.push(format!(
                "{}: only ui_driver_stop is the emergency stop",
                row.tool
            ));
        }
        // Every check the row names is made in the code.
        for check in row.checks {
            if !makes(*check, &code, &own) {
                problems.push(format!(
                    "{}: the row names {check:?}, but the handler's code does not check it",
                    row.tool
                ));
            }
        }
        // A tool that only reads writes no file.
        if row.changes.is_empty() {
            for call in WRITE_CALLS {
                if code.contains(call) {
                    problems.push(format!(
                        "{}: classified as reading only, but its code calls `{call}`; give it its effect and a right",
                        row.tool
                    ));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// The ratchet catches what it is there for: a writing tool without a right, a check that is
/// not made, a write in a tool that claims to read.
#[test]
fn the_checks_of_the_ratchet_tell_code_apart() {
    let own = "async fn sample(&self) {\n    self\n        .helper(1);\n    Self::other();\n}";
    assert_eq!(
        calls(own),
        ["helper".to_owned(), "other".to_owned()]
            .into_iter()
            .collect()
    );
    let checked = "self.require(ferris_idea_core::Grant::Write).await?;";
    assert!(makes(
        Check::Right(ferris_idea_core::Grant::Write),
        checked,
        checked
    ));
    assert!(!makes(
        Check::Right(ferris_idea_core::Grant::Run),
        checked,
        checked
    ));
    // `Grant::Ui` is not found in a longer name.
    assert!(!names("Grant::UiDriver", "Grant::Ui"));
    assert!(!makes(
        Check::Database,
        "profile.ai.contains(&p)",
        "profile.ai.contains(&p)"
    ));
    assert!(makes(
        Check::Database,
        "self.require_db(&mut p, &[], \"read\", None)",
        ""
    ));
    assert_eq!(
        function_start("    pub(crate) async fn ui_act_impl("),
        Some(("    ", "ui_act_impl"))
    );
    assert_eq!(function_start("        fn nested("), None);
}

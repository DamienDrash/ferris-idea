//! IDECraft's own Java quick fixes where jdtls offers none at the place (`SPEC-0076` v7 behaviours
//! 11 and 12): wrapping a primitive value assigned to a `String` in `String.valueOf()`, and
//! removing an unused local variable in a method that JDT does not check for unused code because
//! it holds an error. Each is offered only when the server's list for the place has no action of
//! that purpose, and is applied by IDECraft.

use std::path::Path;

use serde_json::Value;

use crate::actions::CodeAction;
use crate::insight::{Encoding, Range, from_server, line_of};
use crate::python_extract::{byte_of, edit};
use crate::rename::Edit;

/// The title of the wrapping fix, as the reference names it (OBS-0027 JQ2).
pub const WRAP_VALUE_OF: &str = "Wrap using 'String.valueOf()'";

/// The types `String.valueOf` takes a value of that JDT names in its type mismatch.
const PRIMITIVES: [&str; 5] = ["int", "long", "double", "boolean", "char"];

/// One own fix: how it is listed and its edits of the file's text.
pub(crate) struct OwnFix {
    pub(crate) action: CodeAction,
    pub(crate) edits: Vec<Edit>,
}

fn is_java(file: &Path) -> bool {
    file.extension().is_some_and(|e| e == "java")
}

/// A diagnostic's range in characters of `text`.
fn range_in(text: &str, d: &Value, encoding: Encoding) -> Option<Range> {
    let at = |p: &str| -> Option<(u64, u64)> {
        let line = d.pointer(&format!("/range/{p}/line"))?.as_u64()?;
        let units = d.pointer(&format!("/range/{p}/character"))?.as_u64()?;
        Some((line, from_server(line_of(text, line), units, encoding)))
    };
    let (start, end) = (at("start")?, at("end")?);
    Some(Range {
        start_line: start.0,
        start_character: start.1,
        end_line: end.0,
        end_character: end.1,
    })
}

/// Whether the diagnostic at `found` is one the question about `asked` covers: it overlaps the
/// range, or the caret stands on its line (`SPEC-0076` v2 behaviour 1).
fn asked_about(found: &Range, asked: &Range) -> bool {
    let caret = asked.start_line == asked.end_line && asked.start_character == asked.end_character;
    let (s, e) = (
        (found.start_line, found.start_character),
        (found.end_line, found.end_character),
    );
    let (a, b) = (
        (asked.start_line, asked.start_character),
        (asked.end_line, asked.end_character),
    );
    (s <= b && e >= a)
        || (caret && found.start_line <= asked.start_line && asked.start_line <= found.end_line)
}

/// The primitive type JDT names in "Type mismatch: cannot convert from <type> to String".
fn primitive_to_string(message: &str) -> Option<&str> {
    let from = message
        .strip_prefix("Type mismatch: cannot convert from ")?
        .strip_suffix(" to String")?;
    PRIMITIVES.contains(&from).then_some(from)
}

/// The own fixes for `range` of the Java `file` whose content is `text`, given the server's
/// `diagnostics` (in its `encoding`) and the titles it `offered` there.
pub(crate) fn own_fixes(
    file: &Path,
    text: &str,
    encoding: Encoding,
    range: &Range,
    diagnostics: &[Value],
    offered: &[String],
) -> Vec<OwnFix> {
    if !is_java(file) {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Behaviour 11: a primitive value where a String is wanted.
    if !offered.iter().any(|t| t.contains("valueOf")) {
        for d in diagnostics {
            let Some(message) = d["message"].as_str() else {
                continue;
            };
            if primitive_to_string(message).is_none() {
                continue;
            }
            let Some(found) = range_in(text, d, encoding) else {
                continue;
            };
            if !asked_about(&found, range) {
                continue;
            }
            let a = byte_of(text, found.start_line, found.start_character);
            let b = byte_of(text, found.end_line, found.end_character);
            let value = &text[a..b];
            if value.trim().is_empty() || value.contains('\n') {
                continue;
            }
            out.push(OwnFix {
                action: quick_fix(WRAP_VALUE_OF, message),
                edits: vec![edit(text, a..b, format!("String.valueOf({value})"))],
            });
            break;
        }
    }
    // Behaviour 12: an unused local the server has no removal for (JDT skips a method with an
    // error), with the fix of IDECraft's inspection.
    if !offered.iter().any(|t| t.starts_with("Remove '")) {
        let at = byte_of(text, range.start_line, range.start_character);
        let found = crate::inspections::findings_at(file, text, at)
            .into_iter()
            .find(|f| f.id == "java.unused-local");
        if let Some(f) = found
            && let Some(fix) = f.fix.filter(|x| !x.edits.is_empty() && x.action.is_none())
        {
            out.push(OwnFix {
                action: quick_fix(&fix.title, &f.message),
                edits: fix
                    .edits
                    .iter()
                    .map(|e| edit(text, e.range.clone(), e.text.clone()))
                    .collect(),
            });
        }
    }
    out
}

fn quick_fix(title: &str, fixes: &str) -> CodeAction {
    CodeAction {
        title: title.to_owned(),
        kind: Some("quickfix".into()),
        preferred: false,
        disabled: None,
        fixes: vec![fixes.to_owned()],
    }
}

/// `actions` with the own fixes after the server's quick fixes, before its refactorings.
pub(crate) fn merged(mut actions: Vec<CodeAction>, own: Vec<OwnFix>) -> Vec<CodeAction> {
    let at = actions
        .iter()
        .rposition(|a| a.kind.as_deref().is_some_and(|k| k.starts_with("quickfix")))
        .map_or(0, |i| i + 1);
    actions.splice(at..at, own.into_iter().map(|f| f.action));
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MISMATCH: &str = "package lab;\n\npublic class Qf {\n    void run() {\n        int unused = 3;\n        Object list = new ArrayList<String>();\n        String text = count + 1;\n    }\n}\n";

    fn caret(line: u64, character: u64) -> Range {
        Range {
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character,
        }
    }

    fn diagnostic(line: u64, start: u64, end: u64, message: &str) -> Value {
        json!({"range": {"start": {"line": line, "character": start},
                         "end": {"line": line, "character": end}},
               "message": message, "severity": 1})
    }

    fn titles(fixes: &[OwnFix]) -> Vec<&str> {
        fixes.iter().map(|f| f.action.title.as_str()).collect()
    }

    #[test]
    fn a_primitive_assigned_to_a_string_is_wrapped() {
        let file = Path::new("Qf.java");
        let d = [diagnostic(
            6,
            22,
            31,
            "Type mismatch: cannot convert from int to String",
        )];
        // At the value and at the start of its line.
        for at in [caret(6, 25), caret(6, 0)] {
            let fixes = own_fixes(file, MISMATCH, Encoding::Utf16, &at, &d, &[]);
            assert_eq!(titles(&fixes), [WRAP_VALUE_OF]);
            let e = &fixes[0].edits[0];
            assert_eq!(e.old_text, "count + 1");
            assert_eq!(e.new_text, "String.valueOf(count + 1)");
        }
        // Elsewhere, when the server offers it, and for a type String.valueOf does not take here.
        assert!(own_fixes(file, MISMATCH, Encoding::Utf16, &caret(2, 3), &d, &[]).is_empty());
        let theirs = ["Wrap using String.valueOf()".to_owned()];
        assert!(own_fixes(file, MISMATCH, Encoding::Utf16, &caret(6, 25), &d, &theirs).is_empty());
        let object = [diagnostic(
            6,
            22,
            31,
            "Type mismatch: cannot convert from Object to String",
        )];
        assert!(own_fixes(file, MISMATCH, Encoding::Utf16, &caret(6, 25), &object, &[]).is_empty());
        // Not Java.
        assert!(
            own_fixes(
                Path::new("a.kt"),
                MISMATCH,
                Encoding::Utf16,
                &caret(6, 25),
                &d,
                &[]
            )
            .is_empty()
        );
    }

    #[test]
    fn an_unused_local_without_the_servers_removal_gets_the_inspections_fix() {
        let file = Path::new("Qf.java");
        let fixes = own_fixes(file, MISMATCH, Encoding::Utf16, &caret(4, 12), &[], &[]);
        assert_eq!(titles(&fixes), ["Remove variable 'unused'"]);
        let mut after = MISMATCH.to_owned();
        let e = &fixes[0].edits[0];
        let a = byte_of(&after, e.range.start_line, e.range.start_character);
        let b = byte_of(&after, e.range.end_line, e.range.end_character);
        after.replace_range(a..b, &e.new_text);
        assert!(!after.contains("unused"), "{after}");
        assert!(
            after.contains("    void run() {\n        Object list"),
            "{after}"
        );
        // jdtls's own removal is preferred.
        let theirs = ["Remove 'unused' and all assignments".to_owned()];
        assert!(own_fixes(file, MISMATCH, Encoding::Utf16, &caret(4, 12), &[], &theirs).is_empty());
    }

    #[test]
    fn own_fixes_follow_the_servers_quick_fixes() {
        let action = |title: &str, kind: &str| CodeAction {
            title: title.into(),
            kind: Some(kind.into()),
            preferred: false,
            disabled: None,
            fixes: Vec::new(),
        };
        let own = OwnFix {
            action: quick_fix(WRAP_VALUE_OF, "m"),
            edits: Vec::new(),
        };
        let all = merged(
            vec![
                action("Change type of 'text' to 'int'", "quickfix"),
                action("Extract to field", "refactor.extract.field"),
            ],
            vec![own],
        );
        let titles: Vec<&str> = all.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Change type of 'text' to 'int'",
                WRAP_VALUE_OF,
                "Extract to field"
            ]
        );
    }
}

//! Quick Definition (`SPEC-0081` v4 behaviours 6 and 7): the implementations of the symbol at a
//! place, else its definition, and the source that is shown for each target — the innermost
//! declaration of the file's syntax tree at or around the target's line.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::insight::{Location, LocationKind, found_in};
use crate::lsp::LspError;

/// A declaration longer than this many lines is cut (behaviour 6).
pub const MAX_LINES: usize = 40;

/// Lines shown from the target's line on when the syntax tree names no declaration there.
pub const PLAIN_LINES: usize = 12;

/// Targets shown in the popup; the rest is counted (behaviour 6).
pub const MAX_TARGETS: usize = 5;

/// The source shown for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// 0-based first line shown.
    pub line: usize,
    /// 0-based last line shown.
    pub end_line: usize,
    /// The lines, joined with `\n`.
    pub text: String,
    /// Whether the declaration goes on after `end_line`.
    pub truncated: bool,
}

/// The source for a target at 0-based `line` of `path` with content `text`: the innermost
/// declaration (class, interface, method or function, annotations and modifiers included) that
/// starts at or encloses the line, at most [`MAX_LINES`] lines of it; without one the
/// [`PLAIN_LINES`] lines from `line` on. `None` when the file has no such line.
pub fn source_at(path: &Path, text: &str, line: usize) -> Option<Source> {
    let lines: Vec<&str> = text.lines().collect();
    if line >= lines.len() {
        return None;
    }
    let last = lines.len() - 1;
    let innermost = crate::syntax::declaration_spans_of(path, text, 10_000)
        .into_iter()
        .filter(|d| d.start_line <= line && line <= d.end_line)
        .max_by_key(|d| (d.start_line, std::cmp::Reverse(d.end_line)));
    let (start, end, truncated) = match innermost {
        Some(d) => {
            let end = d.end_line.min(last);
            let cut = d.start_line + MAX_LINES - 1;
            (d.start_line, end.min(cut), end > cut)
        }
        None => (line, (line + PLAIN_LINES - 1).min(last), false),
    };
    Some(Source {
        line: start,
        end_line: end,
        text: lines[start..=end].join("\n"),
        truncated,
    })
}

/// What Quick Definition found for a symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct Targets {
    /// `implementation` or `definition`: the request that gave the targets.
    pub kind: &'static str,
    /// The places in files, sorted by path and line, each once.
    pub locations: Vec<Location>,
    /// Readable names of targets outside files (library classes).
    pub outside: Vec<String>,
}

/// The implementations of the symbol at `line`/`character` of `file` (on `text` when given),
/// else — when the server names none or does not answer that request — its definition.
pub async fn targets(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Targets, LspError> {
    let ask = |kind| found_in(root, file, text, line, character, kind, wait);
    let (kind, found) = match ask(LocationKind::Implementation).await {
        Ok(found) if !found.locations.is_empty() => ("implementation", found),
        // No server at all is the same answer for both requests.
        Err(e @ LspError::NoServer(_)) => return Err(e),
        _ => ("definition", ask(LocationKind::Definition).await?),
    };
    let mut locations = found.locations;
    locations.sort_by(|a, b| {
        (&a.path, a.range.start_line, a.range.start_character).cmp(&(
            &b.path,
            b.range.start_line,
            b.range.start_character,
        ))
    });
    locations.dedup_by(|a, b| a.path == b.path && a.range.start_line == b.range.start_line);
    Ok(Targets {
        kind,
        locations,
        outside: found.outside,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA: &str = "package shop;\n\nimport java.util.List;\n\npublic class Cart {\n    private int total;\n\n    @Deprecated\n    public int total() {\n        return total;\n    }\n\n    public void add(int cents) {\n        total += cents;\n    }\n}\n";

    /// SPEC-0081 AK-6 (v4): the innermost declaration at or around the line, annotations
    /// included; a line outside the file has no source.
    #[test]
    fn the_declaration_at_or_around_the_line() {
        let path = Path::new("Cart.java");
        // The name line of `total()`: the method with its annotation.
        let method = source_at(path, JAVA, 8).unwrap();
        assert_eq!((method.line, method.end_line), (7, 10));
        assert_eq!(
            method.text,
            "    @Deprecated\n    public int total() {\n        return total;\n    }"
        );
        assert!(!method.truncated);
        // A line inside the body of `add`: the method, not the class.
        let add = source_at(path, JAVA, 13).unwrap();
        assert_eq!((add.line, add.end_line), (12, 14));
        // The class line: the whole class.
        let class = source_at(path, JAVA, 4).unwrap();
        assert_eq!((class.line, class.end_line), (4, 15));
        assert!(class.text.starts_with("public class Cart {"));
        assert_eq!(source_at(path, JAVA, 99), None);
        assert_eq!(source_at(path, "", 0), None);
    }

    /// SPEC-0081 AK-6 (v4): a long declaration is cut at 40 lines; a line without a declaration
    /// and a file without a grammar give the 12 lines from the line on, fewer at the end.
    #[test]
    fn long_declarations_and_plain_lines() {
        let body: String = (0..60)
            .map(|i| format!("        int v{i} = {i};\n"))
            .collect();
        let long = format!("class Big {{\n    void run() {{\n{body}    }}\n}}\n");
        let cut = source_at(Path::new("Big.java"), &long, 1).unwrap();
        assert_eq!((cut.line, cut.end_line), (1, MAX_LINES));
        assert_eq!(cut.text.lines().count(), MAX_LINES);
        assert!(cut.truncated);
        // The import line of the sample is in no declaration.
        let plain = source_at(Path::new("Cart.java"), JAVA, 2).unwrap();
        assert_eq!((plain.line, plain.end_line), (2, 2 + PLAIN_LINES - 1));
        assert!(!plain.truncated);
        // No grammar: plain lines, ending with the file.
        let notes = source_at(Path::new("notes.unknown"), "a\nb\nc\n", 1).unwrap();
        assert_eq!((notes.line, notes.end_line), (1, 2));
        assert_eq!(notes.text, "b\nc");
    }
}

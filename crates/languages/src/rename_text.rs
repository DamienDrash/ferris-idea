//! Rename in comments, strings and text files (`SPEC-0148`): the occurrences of the old name that
//! the language server does not change, found as whole words in comments and string literals of
//! code files and anywhere in plain-text files of the project's own sources (v3 behaviour 7), and
//! the split of the language server's edits into those in the project's own sources and those
//! outside.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::LanguageId;
use crate::insight::Range;
use crate::rename::{Edit, FileEdits, RenamePlan};
use crate::syntax::SyntaxDocument;

/// Files larger than this are not searched.
const MAX_FILE: u64 = 2 * 1024 * 1024;

/// Which occurrences besides the code usages are renamed (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct TextOptions {
    /// In comments and string literals of code files.
    pub comments_and_strings: bool,
    /// In text files: no code language, or documentation and configuration (Markdown, XML,
    /// YAML, JSON, TOML).
    pub text_files: bool,
}

/// Where an extra occurrence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceKind {
    /// In a comment or string literal of a code file.
    CommentOrString,
    /// In a plain-text file.
    Text,
}

/// An occurrence the language server does not change (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occurrence {
    /// The file.
    pub path: PathBuf,
    /// The replacement there.
    pub edit: Edit,
    /// What kind of place it is.
    pub kind: OccurrenceKind,
    /// The whole line, for the preview.
    pub line_text: String,
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Char ranges of `word` in `chars[from..to]` as a whole word (no identifier character before or
/// after it).
fn words_in(chars: &[char], word: &[char], from: usize, to: usize, out: &mut Vec<(usize, usize)>) {
    if word.is_empty() || to < from + word.len() {
        return;
    }
    let mut i = from;
    while i + word.len() <= to {
        if chars[i..i + word.len()] == *word
            && (i == 0 || !ident(chars[i - 1]))
            && chars.get(i + word.len()).is_none_or(|c| !ident(*c))
        {
            out.push((i, i + word.len()));
            i += word.len();
        } else {
            i += 1;
        }
    }
}

/// Documentation and configuration languages, searched as text files (behaviour 2).
pub fn is_text_language(language: LanguageId) -> bool {
    matches!(
        language,
        LanguageId::Markdown
            | LanguageId::Xml
            | LanguageId::Yaml
            | LanguageId::Json
            | LanguageId::Toml
    )
}

/// Char ranges of `word` as a whole word anywhere in `text`.
pub fn word_occurrences(text: &str, word: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let word: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    words_in(&chars, &word, 0, chars.len(), &mut out);
    out
}

/// Char ranges of `word` as a whole word inside comments and string literals of `text` in
/// `language` (behaviour 2); empty where there is no grammar for it.
pub fn comment_string_occurrences(
    language: LanguageId,
    tsx: bool,
    text: &str,
    word: &str,
) -> Vec<(usize, usize)> {
    let Ok(doc) = SyntaxDocument::new(language, tsx, text) else {
        return Vec::new();
    };
    let chars: Vec<char> = text.chars().collect();
    let word: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    let mut last_end = 0;
    for span in doc.highlight() {
        let k = span.kind.as_str();
        if !(k.starts_with("comment") || k.starts_with("string")) {
            continue;
        }
        // Nested captures may repeat a range; each place counts once.
        let start = span.start.max(last_end);
        let end = span.end.min(chars.len());
        if start < end {
            words_in(&chars, &word, start, end, &mut out);
            last_end = end;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The (line, character) range of the char range `r` in `text`, and its whole line.
fn to_range(line_starts: &[usize], chars: &[char], r: (usize, usize)) -> (Range, String) {
    let line = line_starts.partition_point(|s| *s <= r.0) - 1;
    let start = line_starts[line];
    let end = chars[start..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(chars.len(), |e| start + e);
    let text: String = chars[start..end].iter().collect();
    (
        Range {
            start_line: line as u64,
            start_character: (r.0 - start) as u64,
            end_line: line as u64,
            end_character: (r.1 - start) as u64,
        },
        text.trim_end_matches('\r').to_owned(),
    )
}

fn overlaps(a: &Range, b: &Range) -> bool {
    let start = |r: &Range| (r.start_line, r.start_character);
    let end = |r: &Range| (r.end_line, r.end_character);
    start(a) < end(b) && start(b) < end(a)
}

/// The occurrences of `plan.old_name` in the own sources of the project at `root` that the plan
/// does not already change, as `options` asks for them (behaviour 2). Environments, installed
/// dependencies, build output, excluded and ignored files are not read (v3 behaviour 7). `text_of` gives the editor text of an open
/// file (unsaved changes count); other files are read from disk, binary and very large ones are
/// skipped. Sorted by path and position.
pub fn find_occurrences(
    root: &Path,
    plan: &RenamePlan,
    options: TextOptions,
    text_of: &dyn Fn(&Path) -> Option<String>,
) -> Vec<Occurrence> {
    let mut out = Vec::new();
    if plan.old_name.is_empty() || !(options.comments_and_strings || options.text_files) {
        return out;
    }
    if !root.is_dir() {
        return out;
    }
    let own = idecraft_core::OwnSources::new(root);
    let root = own.root().to_path_buf();
    let planned = |path: &Path| {
        plan.files
            .iter()
            .find(|f| f.path == path || f.path.canonicalize().is_ok_and(|p| p == path))
    };
    for rel in own.files() {
        let path = root.join(&rel);
        let language = LanguageId::detect(&path).filter(|l| !is_text_language(*l));
        let kind = match language {
            Some(_) if options.comments_and_strings => OccurrenceKind::CommentOrString,
            None if options.text_files => OccurrenceKind::Text,
            _ => continue,
        };
        let text = match text_of(&path) {
            Some(t) => t,
            None => {
                if std::fs::metadata(&path).map_or(true, |m| m.len() > MAX_FILE) {
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(t) if !t.contains('\0') => t,
                    _ => continue,
                }
            }
        };
        if !text.contains(&plan.old_name) {
            continue;
        }
        let found = match language {
            Some(l) => {
                let tsx = path.extension().is_some_and(|e| e == "tsx");
                comment_string_occurrences(l, tsx, &text, &plan.old_name)
            }
            None => word_occurrences(&text, &plan.old_name),
        };
        if found.is_empty() {
            continue;
        }
        let chars: Vec<char> = text.chars().collect();
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(
                chars
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c == '\n')
                    .map(|(i, _)| i + 1),
            )
            .collect();
        let existing = planned(&path).map(|f| f.edits.as_slice()).unwrap_or(&[]);
        for r in found {
            let (range, line_text) = to_range(&line_starts, &chars, r);
            if existing.iter().any(|e| overlaps(&e.range, &range)) {
                continue;
            }
            out.push(Occurrence {
                path: path.clone(),
                edit: Edit {
                    range,
                    old_text: plan.old_name.clone(),
                    new_text: plan.new_name.clone(),
                },
                kind,
                line_text,
            });
        }
    }
    out
}

/// The language server's edits for one file that is not the project's own (v3 behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutsideFile {
    /// The file.
    pub path: PathBuf,
    /// The edits there.
    pub edits: Vec<Edit>,
    /// Why the file is not the project's own.
    pub why: idecraft_core::NotOwn,
    /// The same in words, for the preview.
    pub reason: String,
}

/// Takes the files that are not the own sources of the project at `root` out of `plan` and
/// returns them, each with the reason (v3 behaviour 7). The file the rename was started in stays:
/// the user chose it.
pub fn split_outside(root: &Path, plan: &mut RenamePlan, started_in: &Path) -> Vec<OutsideFile> {
    let own = idecraft_core::OwnSources::new(root);
    let started = started_in.canonicalize().ok();
    let mut outside = Vec::new();
    plan.files.retain(|f| {
        if f.path == started_in || (started.is_some() && f.path.canonicalize().ok() == started) {
            return true;
        }
        match own.not_own(&f.path) {
            Some(why) => {
                outside.push(OutsideFile {
                    path: f.path.clone(),
                    edits: f.edits.clone(),
                    why,
                    reason: why.reason().to_owned(),
                });
                false
            }
            None => true,
        }
    });
    outside
}

/// Puts files taken out by [`split_outside`] back into `plan`, when they were included on
/// purpose.
pub fn merge_outside(plan: &mut RenamePlan, files: &[OutsideFile]) {
    for f in files {
        if !plan.files.iter().any(|p| p.path == f.path) {
            plan.files.push(FileEdits {
                path: f.path.clone(),
                edits: f.edits.clone(),
            });
        }
    }
    plan.files.sort_by(|a, b| a.path.cmp(&b.path));
}

/// Adds `occurrences` to `plan`'s edits, each file's edits in document order (behaviour 4).
pub fn merge(plan: &mut RenamePlan, occurrences: &[Occurrence]) {
    for o in occurrences {
        let canonical = o.path.canonicalize().unwrap_or(o.path.clone());
        let file = plan
            .files
            .iter_mut()
            .find(|f| f.path == o.path || f.path.canonicalize().is_ok_and(|p| p == canonical));
        match file {
            Some(f) => f.edits.push(o.edit.clone()),
            None => plan.files.push(FileEdits {
                path: o.path.clone(),
                edits: vec![o.edit.clone()],
            }),
        }
    }
    for f in &mut plan.files {
        f.edits
            .sort_by_key(|e| (e.range.start_line, e.range.start_character));
    }
    plan.files.sort_by(|a, b| a.path.cmp(&b.path));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_words_only() {
        assert_eq!(
            word_occurrences("total totals subtotal total_x $total total.", "total"),
            [(0, 5), (37, 42)]
        );
        assert_eq!(word_occurrences("überTotal Total", "Total"), [(10, 15)]);
    }

    #[test]
    fn comments_and_strings_but_not_code() {
        let rust = "// total of all\nfn total() -> u32 { let s = \"total\"; total2() }\n";
        let found = comment_string_occurrences(LanguageId::Rust, false, rust, "total");
        let words: Vec<String> = found
            .iter()
            .map(|(a, b)| rust.chars().skip(*a).take(b - a).collect())
            .collect();
        assert_eq!(words, ["total", "total"]);
        assert_eq!(found[0], (3, 8));
        let py =
            "def total():\n    \"\"\"Return the total.\"\"\"\n    # total here\n    return total\n";
        assert_eq!(
            comment_string_occurrences(LanguageId::Python, false, py, "total").len(),
            2
        );
    }

    #[test]
    fn workspace_occurrences_skip_planned_edits() {
        let dir = std::env::temp_dir().join(format!("idecraft-rename-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let code = "// compute total\nfn total() {}\nfn main() { total(); }\n";
        std::fs::write(dir.join("lib.rs"), code).unwrap();
        std::fs::write(dir.join("README.md"), "Call `total` for the total.\n").unwrap();
        std::fs::write(dir.join("node_modules/x.md"), "total\n").unwrap();
        let edit = |line, ch| Edit {
            range: Range {
                start_line: line,
                start_character: ch,
                end_line: line,
                end_character: ch + 5,
            },
            old_text: "total".into(),
            new_text: "sum".into(),
        };
        let mut plan = RenamePlan {
            old_name: "total".into(),
            new_name: "sum".into(),
            files: vec![FileEdits {
                path: dir.join("lib.rs"),
                edits: vec![edit(1, 3), edit(2, 12)],
            }],
        };
        let none = |_: &Path| None;
        let all = TextOptions {
            comments_and_strings: true,
            text_files: true,
        };
        let found = find_occurrences(&dir, &plan, all, &none);
        let kinds: Vec<_> = found
            .iter()
            .map(|o| (o.kind, o.edit.range.start_line))
            .collect();
        assert_eq!(
            kinds,
            [
                (OccurrenceKind::Text, 0),
                (OccurrenceKind::Text, 0),
                (OccurrenceKind::CommentOrString, 0)
            ]
        );
        assert_eq!(found[2].line_text, "// compute total");
        let only_comments = TextOptions {
            comments_and_strings: true,
            text_files: false,
        };
        assert_eq!(find_occurrences(&dir, &plan, only_comments, &none).len(), 1);
        assert!(find_occurrences(&dir, &plan, TextOptions::default(), &none).is_empty());
        // The editor's unsaved text counts.
        let open = |p: &Path| p.ends_with("lib.rs").then(|| "fn total() {}\n".to_owned());
        assert_eq!(find_occurrences(&dir, &plan, only_comments, &open).len(), 0);
        merge(&mut plan, &found);
        assert_eq!(plan.files.len(), 2);
        let lib = plan
            .files
            .iter()
            .find(|f| f.path.ends_with("lib.rs"))
            .unwrap();
        assert_eq!(lib.edits[0].range.start_line, 0);
        let text = crate::rename::apply_edits(code, &lib.edits).unwrap();
        assert_eq!(text, "// compute sum\nfn sum() {}\nfn main() { sum(); }\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn at(line: u64, ch: u64) -> Edit {
        Edit {
            range: Range {
                start_line: line,
                start_character: ch,
                end_line: line,
                end_character: ch + 5,
            },
            old_text: "total".into(),
            new_text: "amount".into(),
        }
    }

    /// A Python project with a virtual environment, installed Node packages, an ignored file and
    /// an own file; each holds `total` in a comment.
    fn project(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-rename-own-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |rel: &str, text: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(".git/HEAD", "ref: refs/heads/main\n");
        write(".gitignore", "generated.py\n");
        write("prices.py", "def total():\n    return 1\n");
        write("report.py", "# the total of all prices\n");
        write("generated.py", "# total, written by a generator\n");
        write(".venv/pyvenv.cfg", "home = /usr/bin\n");
        write(
            ".venv/lib/python3.12/site-packages/x.py",
            "# total of the collected items\ndef total():\n    pass\n",
        );
        write("node_modules/y.js", "// total\n");
        write("node_modules/pkg/README.md", "total\n");
        dir.canonicalize().unwrap()
    }

    // SPEC-0148/AK-7 (v3, OBS-0034 L14).
    #[test]
    fn occurrences_only_in_the_projects_own_sources() {
        let dir = project("text");
        let plan = RenamePlan {
            old_name: "total".into(),
            new_name: "amount".into(),
            files: vec![FileEdits {
                path: dir.join("prices.py"),
                edits: vec![at(0, 4)],
            }],
        };
        let all = TextOptions {
            comments_and_strings: true,
            text_files: true,
        };
        let found = find_occurrences(&dir, &plan, all, &|_| None);
        let places: Vec<_> = found
            .iter()
            .map(|o| (o.path.strip_prefix(&dir).unwrap().to_owned(), o.kind))
            .collect();
        assert_eq!(
            places,
            [(PathBuf::from("report.py"), OccurrenceKind::CommentOrString)]
        );
        // An open editor of a file in the environment does not bring it back.
        let open = |p: &Path| p.ends_with("x.py").then(|| "# total\n".to_owned());
        assert_eq!(find_occurrences(&dir, &plan, all, &open).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0148/AK-7 (v3).
    #[test]
    fn usages_outside_the_own_sources_are_split_off() {
        let dir = project("code");
        let files = |names: &[&str]| -> Vec<FileEdits> {
            names
                .iter()
                .map(|n| FileEdits {
                    path: dir.join(n),
                    edits: vec![at(0, 4)],
                })
                .collect()
        };
        let all = [
            ".venv/lib/python3.12/site-packages/x.py",
            "generated.py",
            "prices.py",
            "report.py",
        ];
        let mut plan = RenamePlan {
            old_name: "total".into(),
            new_name: "amount".into(),
            files: files(&all),
        };
        let outside = split_outside(&dir, &mut plan, &dir.join("prices.py"));
        let own: Vec<_> = plan.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(own, [dir.join("prices.py"), dir.join("report.py")]);
        let reasons: Vec<_> = outside
            .iter()
            .map(|f| {
                (
                    f.path.strip_prefix(&dir).unwrap().to_owned(),
                    f.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    PathBuf::from(".venv/lib/python3.12/site-packages/x.py"),
                    "in a virtual environment"
                ),
                (PathBuf::from("generated.py"), "ignored by .gitignore"),
            ]
        );
        // Included on purpose, a file comes back; twice changes nothing.
        merge_outside(&mut plan, &outside[1..]);
        merge_outside(&mut plan, &outside[1..]);
        assert_eq!(plan.files.len(), 3);
        assert_eq!(plan.files[0].path, dir.join("generated.py"));
        // The file the rename was started in stays, wherever it lies.
        let mut plan = RenamePlan {
            old_name: "total".into(),
            new_name: "amount".into(),
            files: files(&all),
        };
        let started = dir.join(".venv/lib/python3.12/site-packages/x.py");
        let outside = split_outside(&dir, &mut plan, &started);
        assert_eq!(outside.len(), 1);
        assert!(plan.files.iter().any(|f| f.path == started));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

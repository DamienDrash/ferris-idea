//! Ferris IDEA's own inspections on the syntax tree (`SPEC-0098`): rules the language servers do not
//! report, written as tree-sitter queries with a check, per language; their descriptions and
//! catalogue (`SPEC-0098` v3 behaviour 6, `SPEC-0315`), the project's profile (`SPEC-0098` v3
//! behaviour 9), Java suppression and "fix all" (`SPEC-0125` v2 behaviours 7, 8).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use crate::LanguageId;
use crate::syntax::SyntaxDocument;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// Set as error in the project's profile (`SPEC-0315`).
    Error,
    /// Probably a bug.
    Warning,
    /// Style or a weak hint.
    Weak,
    /// No highlighting, only the fix (`SPEC-0315` v2 behaviour 7): not shown in the editor, the
    /// fix stays in ⌥↩.
    Info,
}

impl Level {
    /// The level named `error`, `warning` or `weak` (also `weak-warning`, `weak warning`).
    pub fn parse(s: &str) -> Option<Level> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace(['_', ' '], "-")
            .as_str()
        {
            "error" => Some(Level::Error),
            "warning" => Some(Level::Warning),
            "weak" | "weak-warning" => Some(Level::Weak),
            "info" | "information" | "fix-only" | "no-highlighting" => Some(Level::Info),
            _ => None,
        }
    }

    /// The name used in tool output: `error`, `warning`, `weak`, `info`.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Weak => "weak",
            Level::Info => "info",
        }
    }
}

/// A quick fix of a finding: a title and byte-range edits (ascending) of the inspected text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    /// Title in the ⌥↩ list, e.g. `Remove unused import`.
    pub title: String,
    /// Edits of the inspected text.
    pub edits: Vec<crate::jvm::TextEdit>,
    /// A word to save to the project dictionary instead of editing (`SPEC-0163`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learn: Option<String>,
    /// The id of an action the fix runs instead of editing, such as `OptimizeImports`
    /// (`SPEC-0098` v9 behaviour 17).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
    /// What the fix keeps in the project instead of editing (`SPEC-0155` v3 behaviour 10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<Keep>,
    /// A key the fix adds, with an empty value, to the other files of the resource bundle that
    /// lack it, instead of editing this file (`SPEC-0163` v3 behaviour 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_key: Option<String>,
}

/// What a fix keeps in the project instead of editing the file (`SPEC-0155` v3 behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// The finding of `rule` on `text` is no longer reported in the project
    /// (`.ferris-idea/grammar-ignore.toml`).
    Ignore {
        /// Rule id.
        rule: &'static str,
        /// The finding's text, e.g. `an user`.
        text: String,
    },
    /// The inspection is turned off in the project's profile (`SPEC-0315`).
    Disable(&'static str),
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Stable id of the inspection, e.g. `java.empty-catch`.
    pub id: &'static str,
    /// Byte range in the text.
    pub start: usize,
    /// End of the byte range.
    pub end: usize,
    /// Level.
    pub level: Level,
    /// Message shown to the user.
    pub message: String,
    /// Quick fix, when the change is safe (`SPEC-0125`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

/// One inspection: languages, query (capture `@at` marks the reported node) and a check.
pub struct Inspection {
    /// Stable id.
    pub id: &'static str,
    /// Display name, as in the Inspect Code results.
    pub name: &'static str,
    languages: &'static [LanguageId],
    query: &'static str,
    level: Level,
    message: &'static str,
    check: fn(&Node, &str) -> bool,
}

fn always(_: &Node, _: &str) -> bool {
    true
}

/// A block with nothing in it, not even a comment.
fn empty_block(n: &Node, _: &str) -> bool {
    n.named_child_count() == 0
}

/// An equality where neither side is `null` or `undefined` (comparing with those by `==` is a
/// common idiom).
fn coercing_equality(n: &Node, text: &str) -> bool {
    let (Some(l), Some(r)) = (
        n.child_by_field_name("left"),
        n.child_by_field_name("right"),
    ) else {
        return false;
    };
    let nullish = |x: &Node| matches!(&text[x.byte_range()], "null" | "undefined");
    !nullish(&l) && !nullish(&r)
}

/// An `except` clause without an exception type.
fn bare_except(n: &Node, _: &str) -> bool {
    n.child_by_field_name("value").is_none()
}

/// All inspections (behaviour 2).
pub const INSPECTIONS: &[Inspection] = &[
    Inspection {
        id: "java.empty-catch",
        name: "Empty 'catch' block",
        languages: &[LanguageId::Java],
        query: "(catch_clause body: (block) @at)",
        level: Level::Warning,
        message: "Empty 'catch' block: the exception is ignored",
        check: empty_block,
    },
    Inspection {
        id: "kotlin.empty-catch",
        name: "Empty 'catch' block",
        languages: &[LanguageId::Kotlin],
        query: "(catch_block (block) @at)",
        level: Level::Warning,
        message: "Empty 'catch' block: the exception is ignored",
        check: empty_block,
    },
    Inspection {
        id: "python.bare-except",
        name: "Too broad exception clause",
        languages: &[LanguageId::Python],
        query: "(except_clause) @at",
        level: Level::Weak,
        message: "Too broad exception clause: a bare 'except:' also catches SystemExit and KeyboardInterrupt",
        check: bare_except,
    },
    Inspection {
        id: "js.coercing-equality",
        name: "Equality operator may cause type coercion",
        languages: &[LanguageId::JavaScript, LanguageId::TypeScript],
        query: "(binary_expression operator: [\"==\" \"!=\"]) @at",
        level: Level::Weak,
        message: "'==' and '!=' convert types before comparing; use '===' or '!=='",
        check: coercing_equality,
    },
    Inspection {
        id: "any.conflict-marker",
        name: "Merge conflict marker",
        languages: &[],
        query: "",
        level: Level::Warning,
        message: "Unresolved merge conflict marker",
        check: always,
    },
];

/// The findings of every inspection for `doc` (behaviours 1–3), sorted by position.
pub fn inspect(doc: &SyntaxDocument) -> Vec<Finding> {
    let text = doc.text();
    let mut out = Vec::new();
    for i in INSPECTIONS {
        if i.query.is_empty() {
            continue;
        }
        if !i.languages.contains(&doc.language()) {
            continue;
        }
        let Ok(query) = Query::new(doc.ts_language(), i.query) else {
            continue;
        };
        let Some(at) = query.capture_index_for_name("at") else {
            continue;
        };
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, doc.tree().root_node(), text.as_bytes());
        while let Some(m) = matches.next() {
            for c in m.captures.iter().filter(|c| c.index == at) {
                if (i.check)(&c.node, text) {
                    out.push(Finding {
                        id: i.id,
                        start: c.node.start_byte(),
                        end: c.node.end_byte(),
                        level: i.level,
                        message: i.message.to_owned(),
                        fix: None,
                    });
                }
            }
        }
    }
    if doc.language() == LanguageId::Java {
        out.extend(crate::java_inspections::analyze(
            doc.tree().root_node(),
            text,
        ));
    }
    if doc.language() == LanguageId::Kotlin {
        out.extend(crate::kotlin_inspections::analyze(
            doc.tree().root_node(),
            text,
        ));
    }
    if doc.language() == LanguageId::Python {
        // SPEC-0098 v5 behaviour 12.
        out.extend(crate::python_inspections::analyze(
            doc.tree().root_node(),
            text,
        ));
    }
    out.extend(prose_findings(doc));
    out.extend(conflict_markers(text));
    if doc.language() == LanguageId::Java {
        // SPEC-0125 v2 behaviour 7.
        let root = doc.tree().root_node();
        out.retain(|f| !crate::java_inspections::suppressed(root, text, f));
    }
    if doc.language() == LanguageId::Kotlin {
        // SPEC-0098 v13 behaviour 28.
        let root = doc.tree().root_node();
        out.retain(|f| !crate::kotlin_inspections::suppressed(root, text, f));
    }
    out.sort_by_key(|f| (f.start, f.end));
    out
}

/// Ids and names of the natural-language and properties checks (`SPEC-0155`).
pub const TEXT_RULES: &[(&str, &str)] = &[
    ("grazie.repeated-word", "Repeated word"),
    ("grazie.article", "Article before a vowel or consonant"),
    ("grazie.typo", "Typo"),
    (
        crate::spelling_variants::OTHER_VARIANT,
        "Spelling of the other English variant",
    ),
    (
        crate::spelling_variants::MIXED_SPELLING,
        "Mixed English spelling",
    ),
    ("properties.duplicate-key", "Duplicate property key"),
    (
        "properties.missing-in-bundle",
        "Property missing in other files of its resource bundle",
    ),
    (
        "properties.trailing-spaces",
        "Trailing spaces in property value",
    ),
];

fn grazie_findings(text: &str, prose: &[std::ops::Range<usize>]) -> Vec<Finding> {
    let known = crate::grazie::code_words(text, prose);
    let dictionary = crate::grazie::Dictionary::current();
    // American and British spellings by the chosen dictionaries (SPEC-0155 v7 behaviour 7).
    let variants = crate::grazie::variant_mode();
    crate::grazie::check_with(text, prose, dictionary.as_deref(), &known, variants)
        .into_iter()
        .map(|i| Finding {
            id: i.rule,
            start: i.range.start,
            end: i.range.end,
            level: Level::Weak,
            fix: i.replacement.map(|r| Fix {
                title: if r.is_empty() {
                    "Remove the repeated word".into()
                } else {
                    format!("Replace with '{r}'")
                },
                edits: vec![crate::jvm::TextEdit {
                    range: i.range.clone(),
                    text: r,
                }],
                learn: None,
                keep: None,
                bundle_key: None,
                action: None,
            }),
            message: i.message,
        })
        .collect()
}

/// Grammar and spelling of the comments of code and of Markdown prose (`SPEC-0155` behaviour 7).
fn prose_findings(doc: &SyntaxDocument) -> Vec<Finding> {
    // The scope of Settings › Grammar and Style (SPEC-0155 v8 behaviour 14).
    let scope = crate::grazie::grammar_settings().scope(doc.language().title());
    grazie_findings(doc.text(), &prose_ranges(doc, scope))
}

/// The byte ranges of `doc` the grammar and spelling checks read under `scope` (`SPEC-0155` v8
/// behaviour 14): nothing for a file type turned off; Markdown prose; else the comments and
/// documentation comments, the string literals and Python docstrings, each as its switch says.
pub fn prose_ranges(
    doc: &SyntaxDocument,
    scope: ferris_idea_core::GrammarScope,
) -> Vec<std::ops::Range<usize>> {
    let text = doc.text();
    if !scope.enabled {
        return Vec::new();
    }
    if doc.language() == LanguageId::Markdown {
        return crate::grazie::markdown_prose(text);
    }
    let mut prose: Vec<std::ops::Range<usize>> = doc
        .comment_ranges()
        .into_iter()
        .filter(|r| {
            if crate::syntax::doc_comment(doc.language(), &text[r.clone()]) {
                scope.documentation
            } else {
                scope.comments
            }
        })
        .collect();
    let docstrings = doc.language() == LanguageId::Python && scope.documentation;
    if scope.string_literals || docstrings {
        prose.extend(
            doc.string_ranges()
                .into_iter()
                .filter_map(|(r, docstring)| {
                    let on = if docstring {
                        scope.documentation
                    } else {
                        scope.string_literals
                    };
                    on.then_some(r)
                }),
        );
        prose.sort_by_key(|r| r.start);
        prose.dedup_by(|b, a| b.start < a.end);
    }
    prose
}

/// Checks of files without a syntax tree: properties files (duplicate keys, trailing spaces, and
/// the prose of their comments) and plain text (`SPEC-0155` behaviours 5, 7).
/// `absolute` is where the file lives, so the other files of its resource bundle are found (the
/// rules see `path`, which may be relative to the project: `SPEC-0163` v3 behaviour 3).
fn text_file_findings(path: &Path, absolute: &Path, text: &str) -> Vec<Finding> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    // A file shown as Properties by an override or pattern is checked as one (SPEC-0004 v11).
    let properties = LanguageId::detect(path) == Some(LanguageId::Properties);
    match ext.as_deref() {
        _ if properties => {
            let mut out: Vec<Finding> = crate::properties::check(text)
                .into_iter()
                .chain(crate::properties::missing_in_bundle(absolute, text))
                .map(|i| Finding {
                    id: i.id,
                    start: i.range.start,
                    end: i.range.end,
                    // As the reference (SPEC-0155 v5 behaviour 5): a duplicate key and a key
                    // missing in the bundle are errors, trailing spaces a warning.
                    level: if i.id == "properties.trailing-spaces" {
                        Level::Warning
                    } else {
                        Level::Error
                    },
                    message: i.message,
                    // "Remove trailing spaces", "Remove property" (SPEC-0155 v2 behaviour 5);
                    // "Add missing translation to <locales>" (SPEC-0163 v3 behaviour 3).
                    fix: match (i.fix, i.missing) {
                        (Some((title, range, replacement)), _) => Some(Fix {
                            learn: None,
                            keep: None,
                            bundle_key: None,
                            action: None,
                            title: title.into(),
                            edits: vec![crate::jvm::TextEdit {
                                range,
                                text: replacement,
                            }],
                        }),
                        (None, Some((key, locales))) => Some(bundle_fix(&key, &locales)),
                        (None, None) => None,
                    },
                })
                .collect();
            let mut at = 0;
            let comments: Vec<std::ops::Range<usize>> = text
                .split_inclusive('\n')
                .filter_map(|l| {
                    let start = at;
                    at += l.len();
                    let t = l.trim_start();
                    (t.starts_with('#') || t.starts_with('!')).then(|| start..start + l.len())
                })
                .collect();
            // The values of a translation in English are prose too; another language's are not
            // checked with an English list (SPEC-0159 v6 behaviour 9, IMP-0535).
            // The scope of the file type (SPEC-0155 v8 behaviour 14).
            let scope = crate::grazie::grammar_settings().scope(LanguageId::Properties.title());
            if !scope.enabled {
                return out;
            }
            let mut prose = if scope.comments { comments } else { Vec::new() };
            if crate::properties::values_in_english(absolute) {
                prose.extend(
                    crate::properties::parse(text)
                        .into_iter()
                        .map(|p| p.value_range)
                        .filter(|r| !r.is_empty()),
                );
                prose.sort_by_key(|r| r.start);
            }
            out.extend(grazie_findings(text, &prose));
            out
        }
        Some("txt") | None
            if crate::grazie::grammar_settings()
                .scope(crate::grazie::PLAIN_TEXT)
                .enabled =>
        {
            grazie_findings(text, std::slice::from_ref(&(0..text.len())))
        }
        _ => Vec::new(),
    }
}

/// Lines starting with a Git conflict marker, in any language (behaviour 2).
///
/// Only files with an opening `<<<<<<<` line count, so a Markdown heading underline `=======` alone
/// is not reported.
pub fn conflict_markers(text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let opened = text
        .lines()
        .any(|l| l == "<<<<<<<" || l.starts_with("<<<<<<< "));
    if !opened {
        return out;
    }
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let marker = ["<<<<<<< ", ">>>>>>> "].iter().any(|m| bare.starts_with(m))
            || bare == "======="
            || bare == "<<<<<<<"
            || bare == ">>>>>>>";
        if marker {
            out.push(Finding {
                id: "any.conflict-marker",
                start: at,
                end: at + bare.len(),
                level: Level::Warning,
                message: "Unresolved merge conflict marker".into(),
                fix: None,
            });
        }
        at += line.len();
    }
    out
}

/// A finding placed in a project file (Inspect Code, behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placed {
    /// Path relative to the project root.
    pub path: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column in chars.
    pub column: usize,
    /// Char offset of the start.
    pub offset: usize,
    /// Inspection id.
    pub id: &'static str,
    /// Inspection display name.
    pub name: &'static str,
    /// Level.
    pub level: Level,
    /// Message.
    pub message: String,
    /// Title of the quick fix, if the finding has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// For a spelling finding: `typo`, `other_variant` or `mixed_spelling` (`SPEC-0155` v7
    /// behaviour 9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    /// For a spelling finding: the text its quick fix writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
}

/// Files larger than this are skipped by [`inspect_project`].
pub const MAX_FILE_BYTES: u64 = 1 << 20;

/// The findings for one file's text: the syntax inspections when the language is supported, the
/// conflict markers for any text.
pub fn inspect_text(path: &Path, text: &str) -> Vec<Finding> {
    let found = inspect_text_unfiltered(path, text);
    // Words of the project dictionary are no typos (SPEC-0163), and the project's profile turns
    // inspections off or changes their level (SPEC-0315); only an absolute path finds them.
    if path.is_absolute() {
        let found = crate::project_dictionary::filter(path, text, found);
        // Grammar findings whose text the project ignores (SPEC-0155 v3 behaviour 10).
        let found = crate::grammar_ignore::filter(path, text, found);
        crate::inspection_profile::filter(path, found)
    } else {
        found
    }
}

/// [`inspect_text`] of a project file named by its path `rel` relative to the project, which
/// lives at `absolute`: the project's other files are found from there (Python's import fixes,
/// `SPEC-0098` v14 behaviour 19a), the rules and filters see `rel` as before.
pub fn inspect_text_of(rel: &Path, absolute: &Path, text: &str) -> Vec<Finding> {
    if rel.is_absolute() {
        return inspect_text(rel, text);
    }
    inspect_text_unfiltered_in(rel, absolute, text)
}

fn inspect_text_unfiltered(path: &Path, text: &str) -> Vec<Finding> {
    inspect_text_unfiltered_in(path, path, text)
}

fn inspect_text_unfiltered_in(path: &Path, absolute: &Path, text: &str) -> Vec<Finding> {
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    let language =
        LanguageId::detect(path).filter(|_| text.len() <= crate::syntax::MAX_SYNTAX_BYTES);
    match language.and_then(|l| SyntaxDocument::new(l, tsx, text).ok()) {
        Some(doc) if doc.language() == LanguageId::Python => {
            let mut out = inspect(&doc);
            // SPEC-0098 v9 behaviours 17–19: the places of Ferris IDEA's own quick fixes.
            out.extend(crate::python_quick_fixes::analyze_in(
                path,
                absolute,
                doc.tree().root_node(),
                text,
            ));
            out.sort_by_key(|f| (f.start, f.end));
            out
        }
        Some(doc) => inspect(&doc),
        None => {
            let mut out = text_file_findings(path, absolute, text);
            out.extend(conflict_markers(text));
            out.sort_by_key(|f| (f.start, f.end));
            out
        }
    }
}

/// The files of an Inspect Code run and what it leaves out (`SPEC-0098` v8 behaviour 16).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionFiles {
    /// The files to inspect, relative to the project folder and sorted.
    pub files: Vec<PathBuf>,
    /// One line on what the run leaves out, `2 folders and 1 file not inspected (libraries and
    /// ignored files)`; `None` when nothing was left out.
    pub not_inspected: Option<String>,
}

/// The files Inspect Code runs over (`SPEC-0098` v8 behaviour 16): for the whole project (`below`
/// is `None`) the project's own sources; for a file or folder named on purpose (absolute or
/// relative to `root`) that file or every file below that folder when it is not the project's
/// own, else the own files below it. A path outside the project folder or behind a symbolic link
/// is not read: no files, and the line says why.
pub fn inspection_files(root: &Path, below: Option<&Path>) -> InspectionFiles {
    match ferris_idea_core::OwnSources::new(root).chosen(below, false) {
        Ok(listing) => {
            let (folders, files) = listing.left_out_counts();
            InspectionFiles {
                not_inspected: ferris_idea_core::left_out_line(folders, files, "not inspected"),
                files: listing.files,
            }
        }
        Err(why) => InspectionFiles {
            files: Vec::new(),
            not_inspected: Some(format!(
                "{} not inspected ({})",
                below.unwrap_or(root).display(),
                why.reason()
            )),
        },
    }
}

/// Runs every inspection over `files` (relative to `root`), skipping unreadable, non-UTF-8 and
/// oversized files (behaviour 4). Sorted by path and position.
pub fn inspect_project(root: &Path, files: &[PathBuf]) -> Vec<Placed> {
    inspect_project_only(root, files, None)
}

/// [`inspect_project`] limited to the inspection `only` when given, which then runs even when the
/// profile turns it off (`SPEC-0098` v4 behaviour 10). The global inspection
/// `java.unused-declaration` (`SPEC-0316`) is added after the file findings, with the reference
/// index refreshed over every project file.
pub fn inspect_project_only(root: &Path, files: &[PathBuf], only: Option<&str>) -> Vec<Placed> {
    // The profile of `root` applies to every file (SPEC-0098 v3 behaviour 9); an unreadable
    // profile means the defaults.
    let mut profile = crate::inspection_profile::load(root).unwrap_or_default();
    if let Some(id) = only {
        profile.set(id, Some(true), None);
    }
    let mut out = Vec::new();
    for rel in files {
        let path = root.join(rel);
        if std::fs::metadata(&path).map_or(true, |m| m.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let found =
            crate::project_dictionary::filter(&path, &text, inspect_text_of(rel, &path, &text));
        // Grammar findings whose text the project ignores (SPEC-0155 v3 behaviour 10).
        let mut found = crate::grammar_ignore::filter(&path, &text, found);
        if let Some(id) = only {
            found.retain(|f| f.id == id);
        }
        out.extend(place(rel, &text, profile.apply(found)));
    }
    // Java's and, since SPEC-0316 v4 behaviour 9, Kotlin's: each follows the profile on its own.
    let globals: Vec<(&'static str, Level)> = [
        crate::reference_index::UNUSED_DECLARATION,
        crate::reference_index::KOTLIN_UNUSED_DECLARATION,
    ]
    .into_iter()
    .filter(|g| only.is_none_or(|id| id == *g))
    .filter_map(|g| {
        let (enabled, level) = profile.state(g);
        enabled.then_some((g, level))
    })
    .collect();
    if !globals.is_empty() {
        let all = crate::reference_index::project_files(root);
        let scope: Vec<PathBuf> = files
            .iter()
            .filter(|f| {
                globals
                    .iter()
                    .any(|(g, _)| crate::reference_index::unused_id(f) == *g)
            })
            .cloned()
            .collect();
        for mut p in crate::reference_index::unused_findings(root, &all, &scope) {
            let Some((_, level)) = globals.iter().find(|(g, _)| *g == p.id) else {
                continue;
            };
            p.level = *level;
            out.push(p);
        }
    }
    out
}

/// The lines around 0-based `line` of `text` for a preview (`SPEC-0092` v4 behaviour 9): the first
/// line's number and up to `radius` lines above and below, fewer at the ends of the text.
pub fn preview_lines(text: &str, line: usize, radius: usize) -> (usize, Vec<String>) {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.len().saturating_sub(1);
    let line = line.min(last);
    let first = line.saturating_sub(radius);
    let end = (line + radius).min(last);
    (
        first,
        lines[first..=end]
            .iter()
            .map(|l| l.trim_end_matches('\r').to_owned())
            .collect(),
    )
}

/// Display name of an inspection id.
pub fn name_of(id: &'static str) -> &'static str {
    known(id).map_or(id, |(_, n)| n)
}

/// The static id and display name of a known inspection id.
fn known(id: &str) -> Option<(&'static str, &'static str)> {
    INSPECTIONS
        .iter()
        .map(|i| (i.id, i.name))
        .chain(crate::java_inspections::JAVA_RULES.iter().copied())
        .chain(crate::kotlin_inspections::KOTLIN_RULES.iter().copied())
        .chain(crate::python_inspections::PYTHON_RULES.iter().copied())
        .chain(crate::python_quick_fixes::RULES.iter().copied())
        .chain(TEXT_RULES.iter().copied())
        .find(|(i, _)| *i == id)
}

/// The static id equal to `id`, if it names one of Ferris IDEA's inspections.
pub fn known_id(id: &str) -> Option<&'static str> {
    known(id).map(|(i, _)| i)
}

/// Suppression and profile names of the Java inspections (`SPEC-0125` v2 behaviour 7); an id may
/// have two (`java.empty-block`: by the block), a name may cover several ids (`unused`).
const SHORT_NAMES: &[(&str, &str)] = &[
    ("java.string-equality", "StringEquality"),
    ("java.empty-catch", "CatchMayIgnoreException"),
    ("java.unused-import", "UNUSED_IMPORT"),
    ("java.unused-local", "unused"),
    ("java.unused-private", "unused"),
    ("java.unused-parameter", "unused"),
    ("java.redundant-cast", "RedundantCast"),
    ("java.field-may-be-final", "FieldMayBeFinal"),
    ("java.null-dereference", "DataFlowIssue"),
    (
        "java.boolean-literal-comparison",
        "PointlessBooleanExpression",
    ),
    ("java.size-zero", "SizeReplaceableByIsEmpty"),
    ("java.self-assignment", "SillyAssignment"),
    ("java.empty-if", "EmptyStatementBody"),
    ("java.constant-condition", "ConstantValue"),
    ("java.duplicate-condition", "DuplicateCondition"),
    ("java.print-stack-trace", "ThrowablePrintStackTrace"),
    ("java.boxing-constructor", "CachedNumberConstructorCall"),
    ("java.empty-block", "EmptyFinallyBlock"),
    ("java.empty-block", "EmptySynchronizedStatement"),
    ("java.equals-hash-code", "EqualsAndHashcode"),
    ("java.unused-declaration", "unused"),
    ("java.anonymous-to-lambda", "Convert2Lambda"),
    ("java.statement-lambda", "CodeBlock2Expr"),
    ("java.lambda-to-method-ref", "Convert2MethodRef"),
    // Kotlin (SPEC-0098 v13 behaviour 28).
    ("kotlin.unused-declaration", "unused"),
    ("kotlin.unused-private", "unused"),
    ("kotlin.unused-parameter", "UNUSED_PARAMETER"),
];

/// The short name of an inspection (`SPEC-0125` v2 behaviour 7), used in `//noinspection`,
/// `@SuppressWarnings` and the profile file; `java.empty-block` gives `EmptyFinallyBlock`.
pub fn short_name(id: &str) -> Option<&'static str> {
    short_names(id).next()
}

/// Every short name of `id`.
pub(crate) fn short_names(id: &str) -> impl Iterator<Item = &'static str> + '_ {
    SHORT_NAMES
        .iter()
        .filter(move |(i, _)| *i == id)
        .map(|(_, n)| *n)
}

/// The ids a short name stands for.
pub(crate) fn ids_of_short_name(name: &str) -> Vec<&'static str> {
    SHORT_NAMES
        .iter()
        .filter(|(_, n)| *n == name)
        .map(|(i, _)| *i)
        .collect()
}

/// Whether `id` marks unused code, drawn grey in the editor (`SPEC-0125` v2 behaviour 6,
/// `SPEC-0070` v5 behaviour 6).
pub fn is_unused(id: &str) -> bool {
    matches!(
        id,
        "java.unused-import"
            | "java.unused-local"
            | "java.unused-private"
            | "java.unused-parameter"
            | "java.unused-declaration"
            | "kotlin.unused-private"
            | "kotlin.unused-parameter"
            | "kotlin.unused-declaration"
    )
}

/// Whether the suppression name `name` covers `id`: the id, a short name, `unused` for the
/// unused rules or `ALL` (`SPEC-0125` v2 behaviour 7).
pub fn suppression_matches(name: &str, id: &str) -> bool {
    let name = name.trim();
    name == "ALL"
        || name == id
        || (name == "unused" && is_unused(id))
        || short_names(id).any(|n| n == name)
}

/// The description of an inspection in Ferris IDEA's own words (`SPEC-0098` v3 behaviour 6).
pub fn description(id: &str) -> Option<&'static str> {
    crate::inspection_catalogue::of(id).map(|m| m.description)
}

/// The default level of an inspection (before the profile); warning for unknown ids.
pub fn default_level(id: &str) -> Level {
    crate::inspection_catalogue::of(id).map_or(Level::Warning, |m| m.level)
}

/// One inspection as the settings page and `inspection_profile` list it (`SPEC-0315`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectionInfo {
    /// Stable id.
    pub id: &'static str,
    /// Short name (`SPEC-0125` v2 behaviour 7), if any.
    pub short_name: Option<&'static str>,
    /// Display name.
    pub name: &'static str,
    /// Language group: `Java`, `Kotlin`, `JavaScript and TypeScript`, `Python`, `Properties`,
    /// `Natural language` or `General`.
    pub language: &'static str,
    /// Default level.
    pub level: Level,
}

/// Every inspection, sorted by language, then name (then id).
pub fn catalogue() -> Vec<InspectionInfo> {
    let mut out: Vec<InspectionInfo> = crate::inspection_catalogue::META
        .iter()
        .map(|m| InspectionInfo {
            id: m.id,
            short_name: short_name(m.id),
            name: known(m.id).map_or(m.id, |(_, n)| n),
            language: m.language,
            level: m.level,
        })
        .collect();
    out.sort_by(|a, b| (a.language, a.name, a.id).cmp(&(b.language, b.name, b.id)));
    out
}

/// The findings of `text` in `path` that cover byte offset `at`, after the profile and the
/// suppressions.
pub fn findings_at(path: &Path, text: &str, at: usize) -> Vec<Finding> {
    inspect_text(path, text)
        .into_iter()
        .filter(|f| f.start <= at && at <= f.end)
        .collect()
}

fn overlaps(a: &std::ops::Range<usize>, b: &std::ops::Range<usize>) -> bool {
    if a.is_empty() && b.is_empty() {
        a.start == b.start
    } else {
        a.start < b.end && b.start < a.end
    }
}

/// Whether inspection `id` has "Fix all in file": not a duplicate property key, whose places are
/// removed one by one because which to keep is the user's choice (`SPEC-0155` v2 behaviour 5).
pub fn fix_all_applies(id: &str) -> bool {
    id != "properties.duplicate-key" && id != "properties.missing-in-bundle"
}

/// The fix of a key that other files of its bundle lack (`SPEC-0163` v3 behaviour 3): "Add
/// '<key>' to default, de", carried out by whoever applies it from [`Fix::bundle_key`].
pub fn bundle_fix(key: &str, locales: &[String]) -> Fix {
    Fix {
        title: format!("Add missing translation to {}", locales.join(", ")),
        edits: Vec::new(),
        learn: None,
        action: None,
        keep: None,
        bundle_key: Some(key.to_owned()),
    }
}

/// The grammar and spelling rules whose fixes "Apply all writing suggestions in file" applies
/// (`SPEC-0155` v8 behaviour 13).
pub const WRITING_RULES: &[&str] = &[
    "grazie.repeated-word",
    "grazie.article",
    "grazie.typo",
    crate::spelling_variants::OTHER_VARIANT,
    crate::spelling_variants::MIXED_SPELLING,
];

/// The title of the ⌥↩ entry of [`writing_fix_all`].
pub const APPLY_ALL_WRITING: &str = "Apply all writing suggestions in file";

/// "Apply all writing suggestions in file" (`SPEC-0155` v8 behaviour 13, OBS-0117 G5): the first
/// suggestion of every grammar and spelling finding of `text` in `path` (as [`inspect_text`]
/// reports them, so the project dictionary, the ignored texts and the profile apply), ascending,
/// a fix overlapping an earlier one skipped. `None` when fewer than two findings have a fix: one
/// alone is its own entry already.
pub fn writing_fix_all(path: &Path, text: &str) -> Option<Fix> {
    let mut fixes: Vec<Fix> = inspect_text(path, text)
        .into_iter()
        .filter(|f| WRITING_RULES.contains(&f.id))
        .filter_map(|f| f.fix)
        .filter(|f| !f.edits.is_empty())
        .collect();
    fixes.sort_by_key(|f| f.edits.first().map(|e| (e.range.start, e.range.end)));
    let mut edits: Vec<crate::jvm::TextEdit> = Vec::new();
    for fix in fixes {
        let clash = fix
            .edits
            .iter()
            .any(|e| edits.iter().any(|o| overlaps(&o.range, &e.range)));
        if !clash {
            edits.extend(fix.edits);
        }
    }
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    (edits.len() >= 2).then(|| Fix {
        title: APPLY_ALL_WRITING.to_owned(),
        edits,
        learn: None,
        action: None,
        keep: None,
        bundle_key: None,
    })
}

/// The edits of every fix of inspection `id` in `text` of `path` (`SPEC-0125` v2 behaviour 8:
/// "Fix all in file"), ascending byte ranges; a fix overlapping an earlier one is skipped. None
/// for an inspection without "Fix all" ([`fix_all_applies`]).
pub fn fix_all(path: &Path, text: &str, id: &str) -> Vec<crate::jvm::TextEdit> {
    if !fix_all_applies(id) {
        return Vec::new();
    }
    let mut fixes: Vec<Fix> = inspect_text(path, text)
        .into_iter()
        .filter(|f| f.id == id)
        .filter_map(|f| f.fix)
        .filter(|f| !f.edits.is_empty())
        .collect();
    fixes.sort_by_key(|f| f.edits.first().map(|e| (e.range.start, e.range.end)));
    let mut out: Vec<crate::jvm::TextEdit> = Vec::new();
    for fix in fixes {
        let clash = fix
            .edits
            .iter()
            .any(|e| out.iter().any(|o| overlaps(&o.range, &e.range)));
        if !clash {
            out.extend(fix.edits);
        }
    }
    out.sort_by_key(|e| (e.range.start, e.range.end));
    out
}

/// Where a suppression goes (`SPEC-0125` v2 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressScope {
    /// `//noinspection` above the statement or member.
    Statement,
    /// `@SuppressWarnings` on the enclosing method or constructor.
    Method,
    /// `@SuppressWarnings` on the enclosing class (Kotlin: `@Suppress` on the class or object).
    Class,
    /// Kotlin: `@file:Suppress` (`SPEC-0098` v13 behaviour 28).
    File,
}

impl SuppressScope {
    /// The scope named `statement`, `method` or `class`.
    pub fn parse(s: &str) -> Option<SuppressScope> {
        match s.trim().to_ascii_lowercase().as_str() {
            // Kotlin's "for declaration" is the statement scope (SPEC-0098 v13 behaviour 28).
            "statement" | "declaration" => Some(SuppressScope::Statement),
            "file" => Some(SuppressScope::File),
            "method" => Some(SuppressScope::Method),
            "class" => Some(SuppressScope::Class),
            _ => None,
        }
    }
}

/// The edits that suppress inspection `id` for the finding at byte offset `at` of the Java `text`
/// (`SPEC-0125` v2 behaviour 8), naming it by its short name or else its id; `None` when there is
/// no such element (no method around, unparsable text) or it suppresses `id` already. A Kotlin
/// id (`kotlin.…`) reads `text` as Kotlin and writes `@Suppress` (`SPEC-0098` v13 behaviour 28).
pub fn suppress(
    text: &str,
    at: usize,
    id: &str,
    scope: SuppressScope,
) -> Option<Vec<crate::jvm::TextEdit>> {
    if id.starts_with("kotlin.") {
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, text).ok()?;
        return crate::kotlin_inspections::suppress_edits(
            doc.tree().root_node(),
            text,
            at,
            id,
            scope,
        );
    }
    let doc = SyntaxDocument::new(LanguageId::Java, false, text).ok()?;
    crate::java_inspections::suppress_edits(doc.tree().root_node(), text, at, id, scope)
}

/// The edits of "Suppress all inspections for class" (`SPEC-0125` v3 behaviour 8): `ALL` in the
/// enclosing class's `@SuppressWarnings`; `None` without a class or when it suppresses `ALL`.
pub fn suppress_all(text: &str, at: usize) -> Option<Vec<crate::jvm::TextEdit>> {
    suppress(text, at, "ALL", SuppressScope::Class)
}

/// The quick fixes of finding `f` of `text` in the order ⌥↩ lists them: its own fix; for a typo
/// "Save to project dictionary" (`SPEC-0163` behaviour 1); for a repeated word or an article
/// "Ignore '<text>'" and "Disable rule '<name>'" (`SPEC-0155` v3 behaviour 10).
pub fn fixes_of(text: &str, f: &Finding) -> Vec<Fix> {
    let mut out: Vec<Fix> = f.fix.iter().cloned().collect();
    if f.id == "grazie.typo"
        && let Some(word) = text.get(f.start..f.end)
    {
        // Up to five replacements, the common word first (SPEC-0155 v5 behaviour 7).
        // A word with a capital first letter is looked up lowercase and its replacements keep
        // the capital (SPEC-0155 v8 behaviour 7).
        let capital = word.chars().next().is_some_and(char::is_uppercase);
        let more = crate::grazie::Dictionary::current()
            .map(|d| d.suggestions(&word.to_lowercase(), crate::grazie::MAX_SUGGESTIONS))
            .unwrap_or_default()
            .into_iter()
            .map(|s| {
                if capital {
                    crate::grazie::capitalised(&s)
                } else {
                    s
                }
            });
        for s in more {
            if out.len() >= crate::grazie::MAX_SUGGESTIONS {
                break;
            }
            let title = format!("Replace with '{s}'");
            if out.iter().all(|o| o.title != title) {
                out.push(Fix {
                    title,
                    edits: vec![crate::jvm::TextEdit {
                        range: f.start..f.end,
                        text: s,
                    }],
                    learn: None,
                    keep: None,
                    bundle_key: None,
                    action: None,
                });
            }
        }
        out.push(crate::project_dictionary::learn_fix(word));
    }
    // The chosen spelling, then keeping the word (SPEC-0155 v7 behaviour 7).
    if f.id == crate::spelling_variants::OTHER_VARIANT
        && let Some(word) = text.get(f.start..f.end)
    {
        out.push(crate::project_dictionary::learn_fix(word));
    }
    if crate::grammar_ignore::RULES.contains(&f.id)
        && let Some(phrase) = crate::grazie::phrase(text, f.id, f.start..f.end)
    {
        let keep = |title: String, keep: Keep| Fix {
            title,
            edits: Vec::new(),
            learn: None,
            action: None,
            keep: Some(keep),
            bundle_key: None,
        };
        out.push(keep(
            format!("Ignore '{phrase}'"),
            Keep::Ignore {
                rule: f.id,
                text: phrase.to_owned(),
            },
        ));
        out.push(keep(
            format!("Disable rule '{}'", name_of(f.id)),
            Keep::Disable(f.id),
        ));
    }
    out
}

/// Quick fixes of the findings at byte offset `at` of `text` in `path` (`SPEC-0125` behaviour 3),
/// each finding's as [`fixes_of`] lists them; a fix that keeps something is listed once.
pub fn fixes_at(path: &Path, text: &str, at: usize) -> Vec<Fix> {
    let mut out: Vec<Fix> = Vec::new();
    let mut writing = false;
    for f in inspect_text(path, text)
        .into_iter()
        .filter(|f| f.start <= at && at <= f.end)
    {
        writing |= WRITING_RULES.contains(&f.id);
        for fix in fixes_of(text, &f) {
            if fix.keep.is_none() || out.iter().all(|o| o.title != fix.title) {
                out.push(fix);
            }
        }
    }
    // On a grammar or spelling finding, the fixes of all of them in the file (SPEC-0155 v8
    // behaviour 13), after the finding's own.
    if writing && let Some(all) = writing_fix_all(path, text) {
        out.push(all);
    }
    out
}

/// `findings` of `text` as diagnostics next to the language server's (`SPEC-0098` v5 behaviour
/// 13): source `Ferris IDEA`, the inspection id as code, a weak warning as `information`, lines and
/// characters 0-based in Unicode scalar values. Findings of the level "no highlighting" are left
/// out, as in the editor.
pub fn as_diagnostics(text: &str, mut findings: Vec<Finding>) -> Vec<crate::lsp::Diagnostic> {
    findings.sort_by_key(|f| (f.start, f.end));
    let position = |byte: usize| {
        let before = &text[..byte.min(text.len())];
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        (
            before.matches('\n').count() as u64,
            before[line_start..].chars().count() as u64,
        )
    };
    findings
        .into_iter()
        .filter(|f| f.level != Level::Info)
        .map(|f| {
            let (start_line, start_character) = position(f.start);
            let (end_line, end_character) = position(f.end);
            crate::lsp::Diagnostic {
                severity: match f.level {
                    Level::Error => "error",
                    Level::Warning => "warning",
                    Level::Weak | Level::Info => "information",
                }
                .to_owned(),
                start_line,
                start_character,
                end_line,
                end_character,
                message: f.message,
                source: Some("Ferris IDEA".to_owned()),
                code: Some(f.id.to_owned()),
                unnecessary: is_unused(f.id),
            }
        })
        .collect()
}

/// Ferris IDEA's own findings of the absolute `file` as diagnostics ([`as_diagnostics`]), after the
/// profile, the suppressions and the project dictionary; none for an unreadable file or one over
/// [`MAX_FILE_BYTES`].
pub fn file_findings_as_diagnostics(file: &Path) -> Vec<crate::lsp::Diagnostic> {
    if std::fs::metadata(file).map_or(true, |m| m.len() > MAX_FILE_BYTES) {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let found = inspect_text(file, &text);
    as_diagnostics(&text, found)
}

/// Line, column and char offset for each finding of `text`.
pub fn place(rel: &Path, text: &str, mut findings: Vec<Finding>) -> Vec<Placed> {
    findings.sort_by_key(|f| f.start);
    let (mut line, mut line_start, mut chars, mut last) = (0, 0, 0, 0);
    findings
        .into_iter()
        .map(|f| {
            for (i, ch) in text[last..f.start].char_indices() {
                if ch == '\n' {
                    line += 1;
                    line_start = last + i + 1;
                }
            }
            chars += text[last..f.start].chars().count();
            last = f.start;
            let kind = crate::spelling_variants::kind_of(f.id);
            let replacement = kind
                .and(f.fix.as_ref())
                .and_then(|x| x.edits.first())
                .map(|e| e.text.clone());
            Placed {
                path: rel.to_path_buf(),
                line,
                column: text[line_start..f.start].chars().count(),
                offset: chars,
                id: f.id,
                name: name_of(f.id),
                level: f.level,
                fix: f.fix.map(|x| x.title),
                message: f.message,
                kind,
                replacement,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ids(language: LanguageId, text: &str) -> Vec<&'static str> {
        inspect(&SyntaxDocument::new(language, false, text).unwrap())
            .into_iter()
            .map(|f| f.id)
            .collect()
    }

    #[test]
    fn each_inspection_reports_and_stays_quiet() {
        let java = "class A { void f(String s) { try { g(); } catch (Exception e) { } \
                    try { g(); } catch (Exception e) { /* ok */ } \
                    if (s == \"x\") {} if (s == null) {} if (1 == 2) {} } }";
        assert_eq!(
            ids(LanguageId::Java, java),
            ["java.empty-catch", "java.string-equality"]
        );
        let kt = "fun f() { try { g() } catch (e: Exception) { } }";
        assert_eq!(ids(LanguageId::Kotlin, kt), ["kotlin.empty-catch"]);
        let py = "try:\n    f()\nexcept:\n    pass\ntry:\n    f()\nexcept ValueError:\n    pass\n";
        assert_eq!(ids(LanguageId::Python, py), ["python.bare-except"]);
        // SPEC-0098 v5 behaviour 12.
        let py = "def f(item, other):\n    return item\n\n\nitem = 1\n";
        assert_eq!(ids(LanguageId::Python, py), ["python.shadows-name"]);
        // SPEC-0098 v6 behaviour 14.
        let py = "class A:\n    def f(self):\n        return 1\n\n    def g(self):\n        return self\n";
        assert_eq!(
            ids(LanguageId::Python, py),
            ["python.method-could-be-static"]
        );
        let js = "if (a == b) {}\nif (a == null) {}\nif (a === b) {}\n";
        assert_eq!(ids(LanguageId::JavaScript, js), ["js.coercing-equality"]);
        let conflict = "x = 1\n<<<<<<< HEAD\ny = 2\n=======\ny = 3\n>>>>>>> topic\n";
        let found = ids(LanguageId::Python, conflict);
        assert_eq!(found, ["any.conflict-marker"; 3]);
        assert!(conflict_markers("Title\n=======\n").is_empty());
    }

    #[test]
    fn project_run_places_findings() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-inspect-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/A.java"),
            "class A {\n  void f() {\n    try { g(); } catch (Exception e) {}\n  }\n}\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "Title\n=======\n").unwrap();
        let files = [PathBuf::from("README.md"), PathBuf::from("src/A.java")];
        let found = inspect_project(&dir, &files);
        std::fs::remove_dir_all(&dir).unwrap();
        // SPEC-0316/AK-3: the global inspection follows the file findings.
        let global: Vec<&str> = found
            .iter()
            .filter(|f| f.id == "java.unused-declaration")
            .map(|f| f.message.as_str())
            .collect();
        assert_eq!(
            global,
            ["Class 'A' is never used", "Method 'f()' is never used"]
        );
        let found: Vec<&Placed> = found
            .iter()
            .filter(|f| f.id != "java.unused-declaration")
            .collect();
        assert_eq!(found.len(), 1);
        let f = &found[0];
        assert_eq!((f.line, f.column, f.id), (2, 37, "java.empty-catch"));
        assert_eq!(f.path, PathBuf::from("src/A.java"));
        assert_eq!(f.name, "Empty 'catch' block");
    }

    /// SPEC-0098/AK-5: every inspection `inspect`/`inspect_text` can report has a description of
    /// at least two sentences, a language and a catalogue entry.
    #[test]
    fn every_inspection_is_described() {
        let ids: Vec<&str> = INSPECTIONS
            .iter()
            .map(|i| i.id)
            .chain(crate::java_inspections::JAVA_RULES.iter().map(|(i, _)| *i))
            .chain(
                crate::kotlin_inspections::KOTLIN_RULES
                    .iter()
                    .map(|(i, _)| *i),
            )
            .chain(
                crate::python_inspections::PYTHON_RULES
                    .iter()
                    .map(|(i, _)| *i),
            )
            .chain(crate::python_quick_fixes::RULES.iter().map(|(i, _)| *i))
            .chain(TEXT_RULES.iter().map(|(i, _)| *i))
            .collect();
        for id in &ids {
            let d = description(id).unwrap_or_else(|| panic!("{id} has no description"));
            let prose = d.split("\n\n").next().unwrap_or_default();
            let sentences = prose.matches(". ").count() + usize::from(prose.ends_with('.'));
            assert!(sentences >= 2, "{id}: {prose}");
            assert!(d.contains("Reported:"), "{id} has no example");
        }
        let all = catalogue();
        assert_eq!(all.len(), ids.len(), "every id once in the catalogue");
        let languages: BTreeSet<&str> = all.iter().map(|i| i.language).collect();
        assert_eq!(
            languages,
            BTreeSet::from([
                "General",
                "Java",
                "JavaScript and TypeScript",
                "Kotlin",
                "Natural language",
                "Properties",
                "Python",
            ])
        );
        assert!(
            all.windows(2)
                .all(|w| (w[0].language, w[0].name) <= (w[1].language, w[1].name))
        );
        let eq = all.iter().find(|i| i.id == "java.string-equality").unwrap();
        assert_eq!(
            (eq.short_name, eq.name, eq.level),
            (
                Some("StringEquality"),
                "String comparison with '=='",
                Level::Warning
            )
        );
        // A description with a fix shows the fixed code.
        assert!(
            description("java.redundant-cast").is_some_and(|d| d.contains("After the quick fix"))
        );
        assert_eq!(short_name("java.empty-block"), Some("EmptyFinallyBlock"));
        assert_eq!(short_name("java.boxed-equality"), None);
        assert!(is_unused("java.unused-parameter") && !is_unused("java.empty-if"));
        assert_eq!(Level::parse("weak warning"), Some(Level::Weak));
        assert_eq!(Level::parse("ERROR"), Some(Level::Error));
    }

    /// SPEC-0315/AK-2, SPEC-0098/AK-5: a disabled inspection is not reported (editor text and
    /// project run) and a raised severity is reported as an error.
    #[test]
    fn profile_disables_and_raises() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-inspect-prof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join(".idea/inspectionProfiles")).unwrap();
        std::fs::write(
            dir.join(".idea/inspectionProfiles/Project_Default.xml"),
            "<component name=\"InspectionProjectProfileManager\">\n  <profile version=\"1.0\">\n    <option name=\"myName\" value=\"Project Default\" />\n    <inspection_tool class=\"StringEquality\" enabled=\"false\" level=\"WARNING\" enabled_by_default=\"false\" />\n    <inspection_tool class=\"CatchMayIgnoreException\" enabled=\"true\" level=\"ERROR\" enabled_by_default=\"true\" />\n  </profile>\n</component>\n",
        )
        .unwrap();
        let text = "class A {\n  void f(String s) {\n    if (s == \"x\") {}\n    try { g(); } catch (Exception e) {}\n  }\n}\n";
        std::fs::write(dir.join("src/A.java"), text).unwrap();
        let levels = |found: Vec<Finding>| -> Vec<(&'static str, Level)> {
            found.into_iter().map(|f| (f.id, f.level)).collect()
        };
        assert_eq!(
            levels(inspect_text(Path::new("A.java"), text)),
            [
                ("java.string-equality", Level::Warning),
                ("java.empty-catch", Level::Warning)
            ],
            "a relative path has no project"
        );
        assert_eq!(
            levels(inspect_text(&dir.join("src/A.java"), text)),
            [("java.empty-catch", Level::Error)]
        );
        let run = inspect_project(&dir, &[PathBuf::from("src/A.java")]);
        let run: Vec<_> = run
            .iter()
            .filter(|p| p.id != "java.unused-declaration")
            .map(|p| (p.id, p.level))
            .collect();
        assert_eq!(run, [("java.empty-catch", Level::Error)]);
        let at = text.find("catch").unwrap();
        assert!(findings_at(&dir.join("src/A.java"), text, at).is_empty());
        let at = text.find("{}\n  }").unwrap();
        assert_eq!(
            findings_at(&dir.join("src/A.java"), text, at)[0].id,
            "java.empty-catch"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0098/AK-7: the Python shadowing rule is a weak warning and follows the profile.
    #[test]
    fn python_shadowing_follows_the_profile() {
        let text = "def f(item):\n    return item\n\n\nitem = 1\n";
        let found = inspect_text(Path::new("a.py"), text);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (
                found[0].id,
                found[0].level,
                &text[found[0].start..found[0].end]
            ),
            ("python.shadows-name", Level::Weak, "item")
        );
        assert_eq!(default_level("python.shadows-name"), Level::Weak);
        assert_eq!(
            name_of("python.shadows-name"),
            "Name shadows a name of an outer scope"
        );
        let mut profile = crate::inspection_profile::Profile::default();
        profile.set("python.shadows-name", None, Some(Level::Warning));
        let raised = profile.apply(found.clone());
        assert_eq!(raised[0].level, Level::Warning);
        profile.set("python.shadows-name", Some(false), None);
        assert!(profile.apply(found).is_empty());
    }

    /// SPEC-0098/AK-10: "Make method static" is among the fixes at the caret (what ⌥↩ lists),
    /// named by the placed finding, and applied to every method by fix all in file.
    #[test]
    fn python_make_static_at_the_caret_and_for_the_file() {
        let text = "class Shape:\n    def area(self):\n        return 0\n\n    def name(self, upper):\n        return upper\n";
        let path = Path::new("a.py");
        let at = text.find("area").unwrap();
        let fixes = fixes_at(path, text, at + 2);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].title, "Make method static");
        assert_eq!(
            crate::jvm::apply(text, &fixes[0].edits),
            "class Shape:\n    @staticmethod\n    def area():\n        return 0\n\n    def name(self, upper):\n        return upper\n"
        );
        assert!(fixes_at(path, text, text.find("return").unwrap()).is_empty());
        let placed = place(path, text, inspect_text(path, text));
        assert_eq!(placed[0].fix.as_deref(), Some("Make method static"));
        let all = fix_all(path, text, "python.method-could-be-static");
        assert_eq!(all.len(), 4);
        let new = crate::jvm::apply(text, &all);
        assert_eq!(
            new,
            "class Shape:\n    @staticmethod\n    def area():\n        return 0\n\n    @staticmethod\n    def name(upper):\n        return upper\n"
        );
        assert!(inspect_text(path, &new).is_empty());
    }

    /// SPEC-0098/AK-9: the rule for methods that do not use `self` is a weak warning and follows
    /// the profile.
    #[test]
    fn python_static_candidates_follow_the_profile() {
        let text = "class Shape:\n    def area(self):\n        return 0\n";
        let found = inspect_text(Path::new("a.py"), text);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (
                found[0].id,
                found[0].level,
                &text[found[0].start..found[0].end]
            ),
            ("python.method-could-be-static", Level::Weak, "area")
        );
        assert_eq!(default_level("python.method-could-be-static"), Level::Weak);
        assert_eq!(
            name_of("python.method-could-be-static"),
            "Method does not use 'self'"
        );
        let d = as_diagnostics(text, found.clone());
        assert_eq!(
            (
                d[0].severity.as_str(),
                d[0].code.as_deref(),
                (d[0].start_line, d[0].start_character, d[0].end_character)
            ),
            (
                "information",
                Some("python.method-could-be-static"),
                (1, 8, 12)
            )
        );
        let mut profile = crate::inspection_profile::Profile::default();
        profile.set("python.method-could-be-static", None, Some(Level::Warning));
        assert_eq!(profile.apply(found.clone())[0].level, Level::Warning);
        profile.set("python.method-could-be-static", Some(false), None);
        assert!(profile.apply(found).is_empty());
    }

    /// SPEC-0098/AK-8: own findings as diagnostics next to the language server's.
    #[test]
    fn findings_as_diagnostics() {
        let text =
            "größe = 1\n\n\ndef f(ä, größe):\n    try:\n        pass\n    except:\n        pass\n";
        let mut found = inspect_text(Path::new("a.py"), text);
        // A finding of the level "no highlighting" is not shown.
        found.push(Finding {
            id: "java.size-zero",
            start: 0,
            end: 1,
            level: Level::Info,
            message: "hidden".to_owned(),
            fix: None,
        });
        found.push(Finding {
            id: "java.unused-import",
            start: 0,
            end: 7,
            level: Level::Error,
            message: "raised".to_owned(),
            fix: None,
        });
        let d = as_diagnostics(text, found);
        let rows: Vec<_> = d
            .iter()
            .map(|d| {
                (
                    d.severity.as_str(),
                    d.code.as_deref().unwrap_or_default(),
                    (d.start_line, d.start_character, d.end_line, d.end_character),
                    d.unnecessary,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("error", "java.unused-import", (0, 0, 0, 5), true),
                ("information", "python.shadows-name", (3, 9, 3, 14), false),
                ("information", "python.bare-except", (6, 4, 7, 12), false),
            ]
        );
        assert!(d.iter().all(|d| d.source.as_deref() == Some("Ferris IDEA")));
        assert_eq!(
            d[1].message,
            "Parameter 'größe' shadows the name 'größe' of the module (line 1)"
        );
    }

    // SPEC-0098/AK-6: one inspection over a scope, also when the profile turns it off; the global
    // inspection follows the profile (SPEC-0316/AK-3).
    // SPEC-0098/AK-11 (v8, behaviour 16).
    #[test]
    fn a_project_run_inspects_the_own_sources() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-inspect-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bare = "try:\n    pass\nexcept:\n    pass\n";
        for (file, text) in [
            (".gitignore", "scratch.py\n"),
            ("app.py", bare),
            ("scratch.py", bare),
            (".venv/lib/python3.12/site-packages/x.py", bare),
            ("node_modules/y.js", "if (a == b) {}\n"),
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        // The project is its own repository, wherever the temporary folder lies.
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let run = |below: Option<&str>| {
            let scope = inspection_files(&dir, below.map(Path::new));
            let found: Vec<String> = inspect_project(&dir, &scope.files)
                .into_iter()
                .map(|p| format!("{} {}", p.path.to_string_lossy().replace('\\', "/"), p.id))
                .collect();
            (found, scope.not_inspected)
        };
        assert_eq!(
            run(None),
            (
                vec!["app.py python.bare-except".to_owned()],
                Some("2 folders and 1 file not inspected (libraries and ignored files)".to_owned())
            )
        );
        // A place named on purpose is inspected whatever it is.
        assert_eq!(
            run(Some(".venv")),
            (
                vec![".venv/lib/python3.12/site-packages/x.py python.bare-except".to_owned()],
                None
            )
        );
        assert_eq!(
            run(Some("scratch.py")),
            (vec!["scratch.py python.bare-except".to_owned()], None)
        );
        assert_eq!(
            run(Some("node_modules/y.js")),
            (
                vec!["node_modules/y.js js.coercing-equality".to_owned()],
                None
            )
        );
        let absolute = inspection_files(&dir, Some(&dir.join("scratch.py")));
        assert_eq!(absolute.files, [PathBuf::from("scratch.py")]);
        // Nothing outside the project or behind a symbolic link is read.
        assert_eq!(
            run(Some("../elsewhere")),
            (
                Vec::new(),
                Some("../elsewhere not inspected (outside the project folder)".to_owned())
            )
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join(".venv"), dir.join("link")).unwrap();
            assert_eq!(
                run(Some("link")),
                (
                    Vec::new(),
                    Some("link not inspected (behind a symbolic link)".to_owned())
                )
            );
            // The link itself is not followed by the whole-project run either.
            assert_eq!(run(None).0, ["app.py python.bare-except"]);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn one_inspection_on_a_scope() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/A.java"),
            "class A {\n  void f(String s) {\n    if (s == \"x\") {}\n    try { g(); } catch (Exception e) {}\n  }\n}\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/B.java"), "class B { A a; }\n").unwrap();
        let files = [PathBuf::from("src/A.java")];
        let mut profile = crate::inspection_profile::Profile::default();
        profile.set("java.string-equality", Some(false), None);
        crate::inspection_profile::save(&dir, &profile).unwrap();
        let ids = |only: Option<&str>| -> Vec<&'static str> {
            inspect_project_only(&dir, &files, only)
                .into_iter()
                .map(|p| p.id)
                .collect()
        };
        assert_eq!(
            ids(None),
            ["java.empty-catch", "java.unused-declaration"],
            "B uses A, f is unused"
        );
        assert_eq!(ids(Some("java.string-equality")), ["java.string-equality"]);
        assert_eq!(
            ids(Some("java.unused-declaration")),
            ["java.unused-declaration"]
        );
        profile.set("java.unused-declaration", Some(false), None);
        crate::inspection_profile::save(&dir, &profile).unwrap();
        assert_eq!(ids(None), ["java.empty-catch"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0125/AK-8, SPEC-0315/AK-6: a finding at "no highlighting, only fix" keeps its fix.
    #[test]
    fn fix_only_keeps_the_fix() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-fixonly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut profile = crate::inspection_profile::Profile::default();
        profile.set("java.boolean-literal-comparison", None, Some(Level::Info));
        crate::inspection_profile::save(&dir, &profile).unwrap();
        let text = "class A {\n  void f(boolean b) {\n    if (b == true) {}\n  }\n}\n";
        let path = dir.join("A.java");
        let at = text.find("==").unwrap();
        let found = findings_at(&path, text, at);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, Level::Info);
        assert_eq!(fixes_at(&path, text, at)[0].title, "Simplify to 'b'");
        assert_eq!(Level::parse("info"), Some(Level::Info));
        assert_eq!(Level::Info.as_str(), "info");
        let (first, lines) = preview_lines("a\nb\nc\nd\ne\n", 1, 2);
        assert_eq!((first, lines.len()), (0, 4));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0155/AK-6 (v2): both places of a duplicate key are warnings, ⌥↩ at either offers
    /// "Remove property" for that place only, and there is no "Fix all in file".
    #[test]
    fn duplicate_keys_offer_remove_property() {
        let path = Path::new("m.properties");
        let text = "greeting=Hello\ngreeting=Hi \\\n  there\nother=1\n";
        let found: Vec<(usize, Level, String)> = inspect_text(path, text)
            .into_iter()
            .filter(|f| f.id == "properties.duplicate-key")
            .map(|f| (f.start, f.level, f.message))
            .collect();
        assert_eq!(
            found,
            [
                (
                    0,
                    Level::Error,
                    "Duplicate property key `greeting` (also on line 2)".into()
                ),
                (
                    15,
                    Level::Error,
                    "Duplicate property key `greeting` (also on line 1)".into()
                ),
            ]
        );
        let second = fixes_at(path, text, 17);
        assert_eq!(second[0].title, "Remove property");
        assert_eq!(
            crate::jvm::apply(text, &second[0].edits),
            "greeting=Hello\nother=1\n"
        );
        let first = fixes_at(path, text, 3);
        assert_eq!(
            crate::jvm::apply(text, &first[0].edits),
            "greeting=Hi \\\n  there\nother=1\n"
        );
        assert!(fix_all(path, text, "properties.duplicate-key").is_empty());
        assert!(!fix_all_applies("properties.duplicate-key"));
        assert!(fix_all_applies("properties.trailing-spaces"));
    }

    /// SPEC-0155/AK-10 (v5): a duplicate key and a key missing in the bundle are errors, trailing
    /// spaces a warning, as in the reference (OBS-0091 P1).
    #[test]
    fn properties_severities_as_the_reference() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-psev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = "greeting=Hello\nfarewell=Bye  \ngreeting=Hi\numlaut=\\u00fc\n";
        std::fs::write(dir.join("messages.properties"), text).unwrap();
        std::fs::write(
            dir.join("messages_de.properties"),
            "greeting=Hallo\nfarewell=Tschüss\n",
        )
        .unwrap();
        let mut found: Vec<(&str, Level)> = inspect_text(&dir.join("messages.properties"), text)
            .into_iter()
            .filter(|f| f.id.starts_with("properties."))
            .map(|f| (f.id, f.level))
            .collect();
        found.sort_by_key(|(id, _)| *id);
        found.dedup();
        assert_eq!(
            found,
            [
                ("properties.duplicate-key", Level::Error),
                ("properties.missing-in-bundle", Level::Error),
                ("properties.trailing-spaces", Level::Warning),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0163/AK-7 (v3, OBS-0117 G3): a missing key is found by Inspect Code too (the project
    /// run reads files by their relative path), with "Add missing translation to …" carrying the key; the
    /// values of the English base file are spell-checked, the German ones not (IMP-0535).
    #[test]
    fn missing_bundle_keys_in_inspect_code_with_their_fix() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-bkeys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("res")).unwrap();
        std::fs::write(
            dir.join("res/messages.properties"),
            "greeting=Hello\nfarewell=Goodbye recieve\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("res/messages_de.properties"),
            "greeting=Hallo\nfarewell=Auf Wiedersehen\nlesen=Lesen fehlgeschlagen\n",
        )
        .unwrap();
        let rel = [
            PathBuf::from("res/messages.properties"),
            PathBuf::from("res/messages_de.properties"),
        ];
        let found = inspect_project(&dir, &rel);
        let missing: Vec<(String, usize, &str, Level, Option<&str>)> = found
            .iter()
            .filter(|p| p.id == "properties.missing-in-bundle")
            .map(|p| {
                (
                    p.path.display().to_string(),
                    p.line,
                    p.message.as_str(),
                    p.level,
                    p.fix.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            missing,
            [(
                "res/messages_de.properties".to_owned(),
                2,
                "`lesen` is missing in the bundle's default",
                Level::Error,
                Some("Add missing translation to default"),
            )]
        );
        let typos: Vec<(String, &str)> = found
            .iter()
            .filter(|p| p.id == "grazie.typo")
            .map(|p| (p.path.display().to_string(), p.message.as_str()))
            .collect();
        assert_eq!(typos.len(), 1, "{typos:?}");
        assert_eq!(typos[0].0, "res/messages.properties");
        let de = dir.join("res/messages_de.properties");
        let text = std::fs::read_to_string(&de).unwrap();
        let fixes = fixes_at(&de, &text, text.find("lesen").unwrap());
        assert_eq!(fixes.len(), 1, "{fixes:?}");
        assert_eq!(fixes[0].bundle_key.as_deref(), Some("lesen"));
        assert!(fixes[0].edits.is_empty());
        assert!(!fix_all_applies("properties.missing-in-bundle"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0155/AK-16 (v8, OBS-0117 G5): "Apply all writing suggestions in file" takes the first
    /// suggestion of every grammar and spelling finding, offered on any of them when there are at
    /// least two, and not on other findings.
    #[test]
    fn apply_all_writing_suggestions() {
        let text = "Open the the file as an user. We recieve it.\n";
        let path = Path::new("notes.txt");
        let all = writing_fix_all(path, text).expect("three findings");
        assert_eq!(all.title, APPLY_ALL_WRITING);
        assert_eq!(
            crate::jvm::apply(text, &all.edits),
            "Open the file as a user. We receive it.\n"
        );
        // A capitalised typo's replacements keep the capital.
        let capital = fixes_at(path, "Recieve it.\n", 0);
        assert_eq!(
            capital.first().map(|f| f.title.as_str()),
            Some("Replace with 'Receive'"),
            "{capital:?}"
        );
        let at_typo = fixes_at(path, text, text.find("recieve").unwrap());
        assert_eq!(
            at_typo.last().map(|f| f.title.as_str()),
            Some(APPLY_ALL_WRITING)
        );
        // One finding: its own fix is enough.
        assert!(writing_fix_all(path, "Open the the file.\n").is_none());
        let one = fixes_at(path, "Open the the file.\n", 6);
        assert!(one.iter().all(|f| f.title != APPLY_ALL_WRITING));
    }

    /// SPEC-0155/AK-19 (v8, OBS-0117 G6): the scope picks comments, documentation, string
    /// literals and Python docstrings by their switches; a file type turned off has none.
    #[test]
    fn grammar_scope_picks_the_contexts() {
        use ferris_idea_core::GrammarScope;
        let java =
            "/** Docs here. */\nclass A {\n  // A comment.\n  String s = \"Some text\";\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Java, false, java).unwrap();
        let picked = |scope: GrammarScope| -> Vec<&str> {
            prose_ranges(&doc, scope)
                .into_iter()
                .map(|r| &java[r])
                .collect()
        };
        let default = GrammarScope::default();
        assert!(
            !default.string_literals,
            "string literals are off by default"
        );
        assert_eq!(picked(default), ["/** Docs here. */", "// A comment."]);
        assert_eq!(
            picked(GrammarScope {
                documentation: false,
                string_literals: true,
                ..default
            }),
            ["// A comment.", "\"Some text\""]
        );
        assert!(
            picked(GrammarScope {
                enabled: false,
                ..default
            })
            .is_empty()
        );
        let python = "def f():\n    \"\"\"Docstring here.\"\"\"\n    return \"value\"\n";
        let doc = SyntaxDocument::new(LanguageId::Python, false, python).unwrap();
        let ranges: Vec<&str> = prose_ranges(&doc, default)
            .into_iter()
            .map(|r| &python[r])
            .collect();
        assert_eq!(ranges, ["\"\"\"Docstring here.\"\"\""]);
        let settings = ferris_idea_core::GrammarSettings::default();
        assert_eq!(settings.scope("Java"), default);
        assert!(crate::grazie::grammar_file_types().contains(&"Plain text"));
        assert!(crate::grazie::grammar_file_types().contains(&"Java"));
    }

    /// SPEC-0155/AK-11 (v5): ⌥↩ on `mesage` lists `message` first, at most five replacements, then
    /// "Save to project dictionary".
    #[test]
    fn typos_offer_up_to_five_suggestions() {
        let text = "A mesage arrived.\n";
        let fixes = fixes_at(Path::new("notes.txt"), text, text.find("mesage").unwrap());
        let replace: Vec<&str> = fixes
            .iter()
            .filter_map(|f| f.title.strip_prefix("Replace with "))
            .collect();
        assert_eq!(replace.first(), Some(&"'message'"), "{fixes:?}");
        assert!(replace.len() >= 2 && replace.len() <= 5, "{replace:?}");
        assert!(replace.contains(&"'menage'"), "{replace:?}");
        assert_eq!(fixes.len(), replace.len() + 1);
        assert_eq!(
            fixes.last().and_then(|f| f.learn.as_deref()),
            Some("mesage")
        );
    }

    /// SPEC-0155/AK-14 (v7): with the default en_US a British spelling is an other-variant
    /// finding whose fixes are the American spelling and "Save to project dictionary"; Inspect
    /// Code gives its kind and replacement; a word of the project dictionary is not reported.
    #[test]
    fn variant_findings_and_their_fixes() {
        let text = "Pick a colour for the files.\n";
        let at = text.find("colour").unwrap();
        let found = findings_at(Path::new("notes.txt"), text, at);
        assert_eq!(
            found.iter().map(|f| f.id).collect::<Vec<_>>(),
            [crate::spelling_variants::OTHER_VARIANT],
            "{found:?}"
        );
        let titles: Vec<String> = fixes_at(Path::new("notes.txt"), text, at)
            .into_iter()
            .map(|f| f.title)
            .collect();
        assert_eq!(
            titles,
            [
                "Replace with 'color'",
                "Save 'colour' to project dictionary"
            ]
        );
        let placed = place(Path::new("notes.txt"), text, found);
        assert_eq!(placed[0].kind, Some("other_variant"));
        assert_eq!(placed[0].replacement.as_deref(), Some("color"));
        let json = serde_json::to_value(&placed[0]).unwrap();
        assert_eq!(json["kind"], "other_variant");
        // Saved to the project dictionary: no longer reported.
        let dir = std::env::temp_dir().join(format!("ferris-idea-variants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, text).unwrap();
        crate::project_dictionary::learn(&file, "colour").unwrap();
        assert!(
            inspect_text(&file, text)
                .iter()
                .all(|f| f.id != crate::spelling_variants::OTHER_VARIANT)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0155/AK-7 (v3): ⌥↩ on a wrong article lists the correction, "Ignore" and "Disable
    /// rule"; a typo has no "Ignore"; an ignored text of the project is not reported again.
    #[test]
    fn grammar_findings_offer_ignore_and_disable_rule() {
        let text = "Send an user the the file. We recieve it.\n";
        let at = text.find("an user").unwrap();
        let fixes = fixes_at(Path::new("notes.txt"), text, at);
        let titles: Vec<&str> = fixes.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Replace with 'a'",
                "Ignore 'an user'",
                "Disable rule 'Article before a vowel or consonant'",
                // SPEC-0155 v8 behaviour 13: the file has more than one writing finding.
                APPLY_ALL_WRITING,
            ]
        );
        assert_eq!(
            fixes[1].keep,
            Some(Keep::Ignore {
                rule: "grazie.article",
                text: "an user".into()
            })
        );
        assert_eq!(fixes[2].keep, Some(Keep::Disable("grazie.article")));
        assert!(fixes[1].edits.is_empty() && fixes[2].edits.is_empty());
        let repeated = fixes_at(
            Path::new("notes.txt"),
            text,
            text.find(" the file").unwrap() + 2,
        );
        assert_eq!(
            repeated
                .iter()
                .map(|f| f.title.as_str())
                .collect::<Vec<_>>(),
            [
                "Remove the repeated word",
                "Ignore 'the the'",
                "Disable rule 'Repeated word'",
                APPLY_ALL_WRITING,
            ]
        );
        // A typo (with the built-in or the chosen word list) keeps its own entries.
        let typo = fixes_at(Path::new("notes.txt"), text, text.find("recieve").unwrap());
        assert!(
            typo.iter().all(|f| f.keep.is_none()) && !typo.is_empty(),
            "{typo:?}"
        );
        // Ignored in the project: the article goes, the repeated word stays.
        let dir = std::env::temp_dir().join(format!("ferris-idea-gfix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        crate::grammar_ignore::add(
            &dir.join(crate::grammar_ignore::FILE),
            "grazie.article",
            "an user",
        )
        .unwrap();
        let ids: Vec<&str> = inspect_text(&file, text)
            .into_iter()
            .filter(|f| f.id != "grazie.typo")
            .map(|f| f.id)
            .collect();
        assert_eq!(ids, ["grazie.repeated-word"]);
        // Inspect Code follows the same file.
        std::fs::write(&file, text).unwrap();
        let ids: Vec<&str> = inspect_project(&dir, &[PathBuf::from("notes.txt")])
            .into_iter()
            .filter(|p| p.id != "grazie.typo")
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, ["grazie.repeated-word"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

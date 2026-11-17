//! Find Usages with one deadline (`SPEC-0293` behaviours 2, 3, 10 and 11): the declaration and the
//! references of a symbol from the file's language server, only in project files, given up and
//! withdrawn at the deadline or on a stop flag; when the server is too slow, the whole-word text
//! occurrences in project files of the same extension instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use idecraft_core::{Finder, Query, Workspace};
use serde::Serialize;
use serde_json::json;

use crate::context::{PlaceContext, UsageKind, context_of, symbols_by_file, usage_kind};
use crate::insight::{Location, Opened, Range, Symbol, outside_locations, parse_locations};
use crate::lsp::LspError;

/// Time the whole search may take, server start included (behaviour 2).
pub const DEADLINE: Duration = Duration::from_secs(20);

/// Longest wait for the declaration within the deadline.
const DEFINITION_WAIT: Duration = Duration::from_secs(5);

/// Most text occurrences listed (behaviour 10).
pub const MAX_TEXT_OCCURRENCES: usize = 1000;

/// Where the usages came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// The language server's references.
    LanguageServer,
    /// Whole-word text occurrences, because the server did not answer in time (behaviour 10).
    Text,
}

/// The searched symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    /// The symbol's name.
    pub name: String,
    /// Its declaration in a project file, if the server named one there.
    pub declaration: Option<Location>,
    /// The server's name for a declaration outside the project (a library or JDK type).
    pub outside: Option<String>,
    /// The declaration's symbol kind (`method`, `class`, …), when its file's symbols arrived
    /// (`SPEC-0293` v3 behaviour 14).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// What a usage does and where it is (`SPEC-0293` v3 behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageDetail {
    /// Call, write, import or other.
    pub kind: UsageKind,
    /// Module, package, class and method.
    #[serde(flatten)]
    pub context: PlaceContext,
    /// Found by IDECraft's own index, not by a language server (`SPEC-0293` v5 behaviour 17).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub own_index: bool,
}

/// The outcome of a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageSearch {
    /// What was searched.
    pub target: Target,
    /// Usages in project files, sorted by path, line and character, each once.
    pub usages: Vec<Location>,
    /// For each of `usages`, its kind and context (v3).
    pub details: Vec<UsageDetail>,
    /// Where they came from.
    pub source: UsageSource,
    /// Why the list is what it is when that is not obvious (the server was too slow).
    pub note: Option<String>,
}

impl UsageSearch {
    /// Keeps only the usages in `file`, with their details (`FindUsagesInFile`, `SPEC-0293` v7
    /// behaviour 19). Paths are compared resolved through links, as the search reports them.
    pub fn only_in(mut self, file: &Path) -> Self {
        let wanted = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let same =
            |p: &Path| p == file || p == wanted || p.canonicalize().is_ok_and(|c| c == wanted);
        let keep: Vec<bool> = self.usages.iter().map(|u| same(&u.path)).collect();
        let mut kept = keep.iter();
        self.usages.retain(|_| *kept.next().unwrap_or(&false));
        // Details run alongside the usages; a search without details keeps none.
        if self.details.len() == keep.len() {
            let mut kept = keep.iter();
            self.details.retain(|_| *kept.next().unwrap_or(&false));
        }
        self
    }
}

/// Searches the usages of `symbol` at a position of `file` whose content is `text` (else the saved
/// file), within `deadline`, ending early with [`LspError::Stopped`] when `stop` is set. On a
/// class or id selector of a stylesheet the uses in the project's HTML files are added from
/// IDECraft's own index (`SPEC-0293` v5 behaviour 17).
#[allow(clippy::too_many_arguments)]
pub async fn find_usages_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    symbol: &str,
    deadline: Duration,
    stop: Option<Arc<AtomicBool>>,
) -> Result<UsageSearch, LspError> {
    let search =
        server_usages_in(root, file, text, line, character, symbol, deadline, stop).await?;
    let disk;
    let content = match text {
        Some(t) => t,
        None => match std::fs::read_to_string(file) {
            Ok(t) => {
                disk = t;
                &disk
            }
            Err(_) => return Ok(search),
        },
    };
    Ok(
        match crate::html_css::selector_uses(root, file, content, line, character) {
            Some(uses) => with_own_index(root, search, uses),
            None => search,
        },
    )
}

/// `search` with the places `uses` that IDECraft's own index found added, each once, marked and
/// counted in the note, all sorted by path, line and character (`SPEC-0293` v5 behaviour 17).
pub fn with_own_index(root: &Path, mut search: UsageSearch, uses: Vec<Location>) -> UsageSearch {
    let same = |a: &Location, b: &Location| {
        a.path == b.path
            && a.range.start_line == b.range.start_line
            && a.range.start_character == b.range.start_character
    };
    let mut rows: Vec<(Location, UsageDetail)> = std::mem::take(&mut search.usages)
        .into_iter()
        .zip(std::mem::take(&mut search.details))
        .collect();
    let mut added = 0;
    for u in project_only(root, uses) {
        if rows.iter().any(|(known, _)| same(known, &u)) {
            continue;
        }
        let detail = UsageDetail {
            kind: UsageKind::Other,
            context: context_of(
                root,
                &u.path,
                None,
                u.range.start_line,
                u.range.start_character,
            ),
            own_index: true,
        };
        rows.push((u, detail));
        added += 1;
    }
    rows.sort_by(|(a, _), (b, _)| {
        (&a.path, a.range.start_line, a.range.start_character).cmp(&(
            &b.path,
            b.range.start_line,
            b.range.start_character,
        ))
    });
    (search.usages, search.details) = rows.into_iter().unzip();
    if added > 0 {
        let own = format!(
            "{added} {} in HTML files found by IDECraft's own index, not by the language server.",
            if added == 1 { "usage" } else { "usages" }
        );
        search.note = Some(match search.note.take() {
            Some(note) => format!("{note} {own}"),
            None => own,
        });
    }
    search
}

/// The end of the note of a search answered with text occurrences (behaviours 10 and 10a).
const TEXT_SHOWN: &str = "; showing text occurrences.";

/// Why a search answered with text occurrences, from its `note`: the note without its end
/// (`The language server did not answer within 20 s`, `No usable language server for …`).
pub fn text_reason(note: &str) -> &str {
    note.strip_suffix(TEXT_SHOWN).unwrap_or(note)
}

/// The reason to answer with text occurrences when opening the file's server failed with `e`:
/// no usable server for the file, in the words of the server lookup (`SPEC-0293` v11 behaviour
/// 10a); `None` for every other failure, which ends the search (behaviour 11).
fn no_server(e: &LspError) -> Option<String> {
    matches!(e, LspError::NoServer(_)).then(|| capitalized(&e.to_string()))
}

/// The answer from the whole-word text occurrences of `symbol` instead of the server's, with
/// `reason` in the note (behaviours 10 and 10a): usages and kinds as [`usages_of`] and the syntax
/// tree give them.
fn text_answer(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    symbol: &str,
    target: Target,
    reason: &str,
) -> UsageSearch {
    let found = text_occurrences(root, file, text, symbol, MAX_TEXT_OCCURRENCES);
    let usages = usages_of(root, file, text, found, None);
    let mut details = details(root, file, text, &usages, &std::collections::HashMap::new());
    set_kinds(file, text, &usages, &mut details);
    UsageSearch {
        target,
        details,
        usages,
        source: UsageSource::Text,
        note: Some(format!("{reason}{TEXT_SHOWN}")),
    }
}

/// `s` with its first letter in upper case.
fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The search of [`find_usages_in`] with the file's language server alone.
#[allow(clippy::too_many_arguments)]
async fn server_usages_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    symbol: &str,
    deadline: Duration,
    stop: Option<Arc<AtomicBool>>,
) -> Result<UsageSearch, LspError> {
    let started = Instant::now();
    let left = || deadline.saturating_sub(started.elapsed());
    let mut target = Target {
        name: symbol.to_owned(),
        declaration: None,
        outside: None,
        kind: None,
    };
    let from_text =
        |target: Target, reason: String| Ok(text_answer(root, file, text, symbol, target, &reason));
    let too_slow = || {
        format!(
            "The language server did not answer within {} s",
            deadline.as_secs()
        )
    };
    let stopped = until_stopped(stop.clone());
    let opened = tokio::select! {
        o = tokio::time::timeout(left(), Opened::open_text(root, file, text, left())) => o,
        () = stopped => return Err(LspError::Stopped("find usages".into())),
    };
    let mut o = match opened {
        Ok(Ok(o)) => o,
        Ok(Err(LspError::Timeout(..))) | Err(_) => return from_text(target, too_slow()),
        // No server for the file: the text occurrences, with what is missing (v11 behaviour 10a).
        Ok(Err(e)) => {
            return match no_server(&e) {
                Some(reason) => from_text(target, reason),
                None => Err(e),
            };
        }
    };
    let result = async {
        let params = o.position(line, character);
        match o
            .ask_bounded(
                "textDocument/definition",
                params.clone(),
                left().min(DEFINITION_WAIT),
                stop.clone(),
            )
            .await
        {
            Ok(v) => {
                let found = parse_locations(&v, &mut o.files);
                target.declaration = project_only(root, found.clone()).into_iter().next();
                if target.declaration.is_none() {
                    target.outside = outside_locations(&v)
                        .into_iter()
                        .next()
                        .or_else(|| found.first().map(|l| l.path.display().to_string()));
                }
            }
            Err(e @ LspError::Stopped(_)) => return Err(e),
            Err(_) => {}
        }
        let mut params = params;
        params["context"] = json!({"includeDeclaration": false});
        match o
            .ask_bounded("textDocument/references", params, left(), stop.clone())
            .await
        {
            Ok(v) => {
                let usages = usages_of(
                    root,
                    file,
                    text,
                    parse_locations(&v, &mut o.files),
                    target.declaration.as_ref(),
                );
                // Module, package, class and method from each file's symbols, within the
                // deadline (v3 behaviour 14).
                let mut asked = usages.clone();
                asked.extend(target.declaration.clone());
                let symbols = symbols_by_file(&mut o, &asked, started + deadline, 100).await;
                if let Some(d) = &target.declaration {
                    target.kind = context_of(
                        root,
                        &d.path,
                        symbols.get(&d.path).map(Vec::as_slice),
                        d.range.start_line,
                        d.range.start_character,
                    )
                    .symbol_kind;
                }
                let mut details = details(root, file, text, &usages, &symbols);
                set_kinds(file, text, &usages, &mut details);
                Ok(UsageSearch {
                    target: target.clone(),
                    details,
                    usages,
                    source: UsageSource::LanguageServer,
                    note: None,
                })
            }
            Err(LspError::Timeout(..)) => from_text(target.clone(), too_slow()),
            Err(e) => Err(e),
        }
    }
    .await;
    o.close().await;
    result
}

/// The kind and context of each of `usages`, the lines read from `text` for `file` and from disk
/// for other files.
fn details(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    usages: &[Location],
    symbols: &std::collections::HashMap<PathBuf, Vec<Symbol>>,
) -> Vec<UsageDetail> {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut contents: std::collections::HashMap<PathBuf, String> = std::collections::HashMap::new();
    usages
        .iter()
        .map(|u| {
            let content = contents
                .entry(u.path.clone())
                .or_insert_with(|| match text {
                    Some(t) if u.path == file => t.to_owned(),
                    _ => std::fs::read_to_string(&u.path).unwrap_or_default(),
                });
            let line = content
                .lines()
                .nth(u.range.start_line as usize)
                .unwrap_or_default();
            UsageDetail {
                kind: usage_kind(line, u.range.start_character as usize),
                context: context_of(
                    root,
                    &u.path,
                    symbols.get(&u.path).map(Vec::as_slice),
                    u.range.start_line,
                    u.range.start_character,
                ),
                own_index: false,
            }
        })
        .collect()
}

/// The usages among the places `found` that a server or the text search gave for a symbol of
/// `file` (content `text`, else the disk): in project files only, each once, without `declaration`
/// (the symbol's own), and in JavaScript and TypeScript files without the declarations of other
/// members, which implement or override it (`SPEC-0293` v4 behaviours 3 and 16, OBS-0028 T5).
/// Find Usages lists these and the usage counts at the lines count them (`SPEC-0320` v8
/// behaviour 5a, T8), so the two say the same.
pub fn usages_of(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    found: Vec<Location>,
    declaration: Option<&Location>,
) -> Vec<Location> {
    let mut usages = project_only(root, found);
    if let Some(d) = declaration {
        let d = project_only(root, vec![d.clone()]);
        if let Some(d) = d.first() {
            usages.retain(|u| {
                u.path != d.path
                    || u.range.start_line != d.range.start_line
                    || u.range.start_character != d.range.start_character
            });
        }
    }
    // Member declarations, and in a text search the occurrences in strings and comments
    // (`SPEC-0293` v11 behaviour 10a), are no usages.
    let syntax = syntax_of(file, text, &usages);
    let mut member = syntax
        .iter()
        .map(|s| s.is_some_and(|u| u.member_declaration || u.in_text));
    usages.retain(|_| !member.next().unwrap_or(false));
    usages
}

/// How each of `usages` reads in the syntax tree of its file, for the files whose usages are read
/// from it (JavaScript and TypeScript); `None` elsewhere.
fn syntax_of(
    file: &Path,
    text: Option<&str>,
    usages: &[Location],
) -> Vec<Option<crate::script_usages::ScriptUsage>> {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut out = vec![None; usages.len()];
    let mut paths: Vec<&PathBuf> = usages
        .iter()
        .map(|u| &u.path)
        .filter(|p| crate::script_usages::supported(p))
        .collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        let content = match text {
            Some(t) if *path == file => t.to_owned(),
            _ => std::fs::read_to_string(path).unwrap_or_default(),
        };
        let at: Vec<usize> = (0..usages.len())
            .filter(|&i| usages[i].path == *path)
            .collect();
        let positions: Vec<(u64, u64)> = at
            .iter()
            .map(|&i| (usages[i].range.start_line, usages[i].range.start_character))
            .collect();
        for (&i, usage) in at
            .iter()
            .zip(crate::script_usages::classify(path, &content, &positions))
        {
            out[i] = usage;
        }
    }
    out
}

/// In JavaScript and TypeScript files the kind of each usage from the syntax tree (`SPEC-0293` v4
/// behaviour 14a); other files keep the kinds from their lines.
fn set_kinds(file: &Path, text: Option<&str>, usages: &[Location], details: &mut [UsageDetail]) {
    for (detail, usage) in details.iter_mut().zip(syntax_of(file, text, usages)) {
        if let Some(u) = usage {
            detail.kind = u.kind;
        }
    }
}

/// Waits until `stop` is set; never, without one.
async fn until_stopped(stop: Option<Arc<AtomicBool>>) {
    loop {
        if stop.as_ref().is_some_and(|s| s.load(Ordering::SeqCst)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The places inside `root` (behaviour 3), sorted by path, line and character, each once.
pub fn project_only(root: &Path, places: Vec<Location>) -> Vec<Location> {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = canonical(root);
    let mut out: Vec<Location> = places
        .into_iter()
        .map(|mut l| {
            l.path = canonical(&l.path);
            l
        })
        .filter(|l| l.path.starts_with(&root))
        .collect();
    out.sort_by(|a, b| {
        (&a.path, a.range.start_line, a.range.start_character).cmp(&(
            &b.path,
            b.range.start_line,
            b.range.start_character,
        ))
    });
    out.dedup_by(|a, b| {
        a.path == b.path
            && a.range.start_line == b.range.start_line
            && a.range.start_character == b.range.start_character
    });
    out
}

/// Whole-word, case-sensitive occurrences of `symbol` in the project files with `file`'s extension,
/// `file` itself searched in `text` when given (its unsaved content), at most `max` (behaviour 10).
pub fn text_occurrences(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    symbol: &str,
    max: usize,
) -> Vec<Location> {
    let (Ok(workspace), Ok(finder)) = (
        Workspace::open(root),
        Finder::new(&Query {
            pattern: symbol.to_owned(),
            regex: false,
            match_case: true,
            whole_words: true,
        }),
    ) else {
        return Vec::new();
    };
    if symbol.is_empty() {
        return Vec::new();
    }
    let extension = file.extension();
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut out = Vec::new();
    for relative in workspace.files().unwrap_or_default() {
        if relative.extension() != extension {
            continue;
        }
        let path: PathBuf = workspace.root().join(&relative);
        let path = path.canonicalize().unwrap_or(path);
        let content = match text {
            Some(t) if path == file => t.to_owned(),
            _ => match idecraft_core::searchable_text(&workspace, &relative) {
                Some(t) => t,
                None => continue,
            },
        };
        for m in finder.find(&content) {
            if out.len() >= max {
                return project_only(root, out);
            }
            let length = symbol.chars().count() as u64;
            out.push(Location {
                path: path.clone(),
                range: Range {
                    start_line: m.line as u64,
                    start_character: m.column as u64,
                    end_line: m.line as u64,
                    end_character: m.column as u64 + length,
                },
            });
        }
    }
    project_only(root, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0293 v7 AK-12: a search restricted to a file keeps that file's usages with their
    /// details and drops the others.
    #[test]
    fn a_search_is_restricted_to_one_file() {
        let root = std::env::temp_dir().join(format!("idecraft-in-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let root = root.canonicalize().unwrap();
        for name in ["src/a.c", "src/b.c"] {
            std::fs::write(root.join(name), "int helper(void);\nint x = helper();\n").unwrap();
        }
        let at = |file: &str, line: u64| Location {
            path: root.join(file),
            range: Range {
                start_line: line,
                start_character: 4,
                end_line: line,
                end_character: 10,
            },
        };
        let usages = vec![at("src/a.c", 0), at("src/a.c", 1), at("src/b.c", 1)];
        let search = UsageSearch {
            target: Target {
                name: "helper".into(),
                declaration: Some(at("src/a.c", 0)),
                outside: None,
                kind: None,
            },
            details: details(
                &root,
                &root.join("src/a.c"),
                None,
                &usages,
                &Default::default(),
            ),
            usages,
            source: UsageSource::LanguageServer,
            note: Some("kept".into()),
        };
        let all = search.details.clone();
        let found = search.clone().only_in(&root.join("src/a.c"));
        assert_eq!(found.usages, [at("src/a.c", 0), at("src/a.c", 1)]);
        assert_eq!(found.details, all[..2]);
        assert_eq!(found.target, search.target);
        assert_eq!(found.note.as_deref(), Some("kept"));
        let other = search.clone().only_in(&root.join("src/b.c"));
        assert_eq!(other.usages, [at("src/b.c", 1)]);
        assert_eq!(other.details, all[2..]);
        assert!(search.only_in(&root.join("src/none.c")).usages.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0293 v5 AK-10 (SPEC-0042 v17 behaviour 30): the uses the own index found are added
    /// once, marked, sorted with the server's and counted in the note.
    #[test]
    fn own_index_usages_are_added_once() {
        let root = std::env::temp_dir().join(format!("idecraft-own-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("css")).unwrap();
        let root = root.canonicalize().unwrap();
        let css = ".card {}\n.banner .card {}\n";
        std::fs::write(root.join("css/site.css"), css).unwrap();
        std::fs::write(root.join("index.html"), "<p class=\"card\"></p>\n").unwrap();
        std::fs::write(root.join("about.html"), "<p class=\"a card\"></p>\n").unwrap();
        let at = |file: &str, line: u64, character: u64| Location {
            path: root.join(file),
            range: Range {
                start_line: line,
                start_character: character,
                end_line: line,
                end_character: character + 4,
            },
        };
        let server = vec![at("css/site.css", 1, 8)];
        let search = UsageSearch {
            target: Target {
                name: "card".into(),
                declaration: Some(at("css/site.css", 0, 0)),
                outside: None,
                kind: None,
            },
            details: details(
                &root,
                &root.join("css/site.css"),
                Some(css),
                &server,
                &Default::default(),
            ),
            usages: server,
            source: UsageSource::LanguageServer,
            note: None,
        };
        let uses = crate::html_css::selector_uses(&root, &root.join("css/site.css"), css, 0, 2)
            .expect("a class selector");
        // The same place named again is not listed twice.
        let mut twice = uses.clone();
        twice.extend(uses);
        twice.push(at("css/site.css", 1, 8));
        let found = with_own_index(&root, search.clone(), twice);
        let rows: Vec<(String, u64, bool)> = found
            .usages
            .iter()
            .zip(&found.details)
            .map(|(u, d)| {
                (
                    u.path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    u.range.start_character,
                    d.own_index,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("about.html".into(), 12, true),
                ("css/site.css".into(), 8, false),
                ("index.html".into(), 10, true),
            ]
        );
        assert_eq!(
            found.note.as_deref(),
            Some(
                "2 usages in HTML files found by IDECraft's own index, not by the language server."
            )
        );
        let marked = serde_json::to_value(&found.details).unwrap();
        assert_eq!(marked[0]["own_index"], true);
        assert!(marked[1].get("own_index").is_none());
        // Nothing to add: the search stays as it was.
        assert_eq!(with_own_index(&root, search.clone(), Vec::new()), search);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0320 v8 behaviour 5a, SPEC-0293 v4 behaviour 16 (OBS-0028 T8): the one function behind
    /// Find Usages and the usage counts keeps the call and drops the symbol's own declaration, the
    /// implementing and overriding members, places outside the project and repeats.
    #[test]
    fn usages_of_keep_calls_and_drop_implementations() {
        let root = std::env::temp_dir().join(format!("idecraft-usages-of-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let root = root.canonicalize().unwrap();
        let shape = "export interface Shape {\n  area(): number;\n}\n\nexport abstract class Base implements Shape {\n  abstract area(): number;\n  describe() {\n    return this.area();\n  }\n}\n";
        std::fs::write(root.join("src/shape.ts"), shape).unwrap();
        std::fs::write(
            root.join("src/circle.ts"),
            "import { Base } from \"./shape\";\nexport class Circle extends Base {\n  area(): number {\n    return 3;\n  }\n}\n",
        )
        .unwrap();
        let at = |file: &str, line: u64, character: u64| Location {
            path: root.join(file),
            range: Range {
                start_line: line,
                start_character: character,
                end_line: line,
                end_character: character + 4,
            },
        };
        let found = vec![
            at("src/shape.ts", 5, 11),
            at("src/shape.ts", 7, 16),
            at("src/shape.ts", 7, 16),
            at("src/circle.ts", 2, 2),
            at("src/shape.ts", 1, 2),
            Location {
                path: std::env::temp_dir().join("elsewhere.ts"),
                range: at("src/shape.ts", 7, 16).range,
            },
        ];
        let own = at("src/shape.ts", 1, 2);
        let file = root.join("src/shape.ts");
        let kept = usages_of(&root, &file, None, found.clone(), Some(&own));
        assert_eq!(kept, [at("src/shape.ts", 7, 16)]);
        // The unsaved text of the file counts, not the disk: here a call stands at line 5.
        let unsaved = "export interface Shape {\n  area(): number;\n}\n\nexport abstract class Base implements Shape {\n  run() { area(); }\n";
        let kept = usages_of(
            &root,
            &file,
            Some(unsaved),
            vec![at("src/shape.ts", 5, 10)],
            None,
        );
        assert_eq!(kept.len(), 1);
        // Other languages keep every place.
        std::fs::write(root.join("src/a.py"), "def area(self):\n    pass\n").unwrap();
        let py = Location {
            path: root.join("src/a.py"),
            range: at("src/a.py", 0, 4).range,
        };
        assert_eq!(
            usages_of(&root, &root.join("src/a.py"), None, vec![py.clone()], None),
            [py]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn at(path: &Path, line: u64, character: u64) -> Location {
        Location {
            path: path.to_path_buf(),
            range: Range {
                start_line: line,
                start_character: character,
                end_line: line,
                end_character: character + 4,
            },
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "idecraft-find-usages-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        dir.canonicalize().unwrap()
    }

    /// SPEC-0293 v11 AK-17 (behaviour 10a): without a usable language server the search answers
    /// with the text occurrences, marked as such, and a note that names what is missing; in the
    /// TypeScript project of AK-9 that is the call in `describe` only, the member declarations and
    /// the occurrence in the template string's text left out. Other failures still end the search.
    #[test]
    fn without_a_server_text_occurrences_with_the_reason() {
        let root = temp("no-server");
        let shape = root.join("src/shape.ts");
        std::fs::write(
            &shape,
            "export interface Shape {\n  area(): number;\n}\n\nexport abstract class Base implements Shape {\n  abstract area(): number;\n\n  describe(): string {\n    return `area ${this.area()}`;\n  }\n}\n",
        )
        .unwrap();
        for (file, class) in [("src/circle.ts", "Circle"), ("src/square.ts", "Square")] {
            std::fs::write(
                root.join(file),
                format!("import {{ Base }} from \"./shape\";\n\nexport class {class} extends Base {{\n  area(): number {{\n    return 4;\n  }}\n}}\n"),
            )
            .unwrap();
        }
        let missing = LspError::NoServer(
            "typescript: typescript-language-server not found (install: npm i -g typescript-language-server typescript)".into(),
        );
        let reason = no_server(&missing).expect("no server gives the text occurrences");
        let target = Target {
            name: "area".into(),
            declaration: None,
            outside: None,
            kind: None,
        };
        let found = text_answer(&root, &shape, None, "area", target, &reason);
        let shown: Vec<(String, u64, UsageKind)> = found
            .usages
            .iter()
            .zip(&found.details)
            .map(|(u, d)| {
                (
                    u.path.file_name().unwrap().to_string_lossy().into_owned(),
                    u.range.start_line,
                    d.kind,
                )
            })
            .collect();
        assert_eq!(shown, [("shape.ts".into(), 8, UsageKind::Call)]);
        assert_eq!(found.source, UsageSource::Text);
        let note = found.note.unwrap();
        assert_eq!(
            note,
            "No usable language server for typescript: typescript-language-server not found (install: npm i -g typescript-language-server typescript); showing text occurrences."
        );
        assert_eq!(
            text_reason(&note),
            "No usable language server for typescript: typescript-language-server not found (install: npm i -g typescript-language-server typescript)"
        );
        // A server error or a server that cannot start is no reason for text (behaviour 11).
        for other in [
            LspError::Start {
                command: "x".into(),
                message: "denied".into(),
            },
            LspError::Io("broken pipe".into()),
        ] {
            assert_eq!(no_server(&other), None, "{other}");
        }
        // The whole search, on a file whose language has no server: the text, not an error.
        let notes = root.join("src/notes.txt");
        std::fs::write(&notes, "area\nthe area\n").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let found = rt
            .block_on(find_usages_in(
                &root,
                &notes,
                None,
                0,
                0,
                "area",
                Duration::from_secs(5),
                None,
            ))
            .expect("text occurrences, not an error");
        assert_eq!(found.source, UsageSource::Text);
        assert_eq!(found.usages.len(), 2);
        assert!(
            found
                .note
                .as_deref()
                .is_some_and(|n| n.starts_with("No usable language server for")
                    && n.ends_with("; showing text occurrences.")),
            "{:?}",
            found.note
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0293/AK-3: places outside the project are dropped, the rest sorted and each once.
    #[test]
    fn keeps_project_places_once_in_order() {
        let root = temp("project");
        let a = root.join("src/A.java");
        let b = root.join("src/B.java");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let outside = std::env::temp_dir().join("elsewhere/String.java");
        let kept = project_only(
            &root,
            vec![
                at(&b, 3, 1),
                at(&outside, 1, 1),
                at(&a, 9, 2),
                at(&a, 2, 5),
                at(&a, 2, 5),
            ],
        );
        let shown: Vec<(PathBuf, u64)> = kept
            .iter()
            .map(|l| (l.path.clone(), l.range.start_line))
            .collect();
        assert_eq!(shown, [(a.clone(), 2), (a, 9), (b, 3)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0293/AK-3: the text fallback finds whole words, case-sensitive, in files of the same
    /// extension, the current file in its unsaved text.
    #[test]
    fn text_fallback_whole_words_same_extension() {
        let root = temp("text");
        let main = root.join("src/Main.java");
        std::fs::write(&main, "class Main {}\n").unwrap();
        std::fs::write(
            root.join("src/Other.java"),
            "String s; StringBuilder b; string t;\n  String u;\n",
        )
        .unwrap();
        std::fs::write(root.join("src/notes.txt"), "String\n").unwrap();
        let found = text_occurrences(&root, &main, Some("void f(String a) {}\n"), "String", 100);
        let shown: Vec<(String, u64, u64, u64)> = found
            .iter()
            .map(|l| {
                (
                    l.path.file_name().unwrap().to_string_lossy().into_owned(),
                    l.range.start_line,
                    l.range.start_character,
                    l.range.end_character,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("Main.java".into(), 0, 7, 13),
                ("Other.java".into(), 0, 0, 6),
                ("Other.java".into(), 1, 2, 8),
            ]
        );
        assert_eq!(
            text_occurrences(&root, &main, None, "String", 2).len(),
            2,
            "cut at the maximum"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

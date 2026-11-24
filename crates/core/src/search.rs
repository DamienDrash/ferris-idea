//! Find and replace in text and in workspace files (`SPEC-0029`).

use std::path::{Component, Path, PathBuf};

use regex::{Regex, RegexBuilder};

use crate::{LeftOut, NotOwn, OwnSources, Workspace, WorkspaceError};

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// Text or regular expression.
    pub pattern: String,
    /// Treat `pattern` as a regular expression.
    pub regex: bool,
    /// Distinguish upper and lower case.
    pub match_case: bool,
    /// Only matches bounded by non-word characters.
    pub whole_words: bool,
}

/// Why a search could not run.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// The regular expression does not compile.
    #[error("invalid regular expression: {0}")]
    Regex(String),
    /// Listing the workspace failed.
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

/// One match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Start char offset.
    pub start: usize,
    /// End char offset (exclusive).
    pub end: usize,
    /// 0-based line of the start.
    pub line: usize,
    /// 0-based column (chars) of the start.
    pub column: usize,
    /// Text of the line the match starts on.
    pub line_text: String,
}

/// One replacement in a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// The match.
    pub found: Match,
    /// Matched text.
    pub old: String,
    /// Text that replaces it.
    pub new: String,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A compiled query.
#[derive(Debug, Clone)]
pub struct Finder {
    regex: Option<Regex>,
    query: Query,
}

impl Finder {
    /// Compiles `query` (`SPEC-0029` behaviour 1).
    pub fn new(query: &Query) -> Result<Self, SearchError> {
        if query.pattern.is_empty() {
            return Ok(Self {
                regex: None,
                query: query.clone(),
            });
        }
        let source = if query.regex {
            query.pattern.clone()
        } else {
            regex::escape(&query.pattern)
        };
        let regex = RegexBuilder::new(&source)
            .case_insensitive(!query.match_case)
            .multi_line(true)
            .build()
            .map_err(|e| SearchError::Regex(e.to_string()))?;
        Ok(Self {
            regex: Some(regex),
            query: query.clone(),
        })
    }

    /// Matches in `text` (`SPEC-0029` behaviours 2–3).
    pub fn find(&self, text: &str) -> Vec<Match> {
        let mut at = Locator::new(text);
        self.ranges(text)
            .into_iter()
            .map(|r| at.locate(r.start, r.end))
            .collect()
    }

    fn ranges(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        let Some(regex) = &self.regex else {
            return Vec::new();
        };
        regex
            .find_iter(text)
            .filter(|m| !m.is_empty())
            .filter(|m| {
                !self.query.whole_words
                    || (!text[..m.start()].chars().next_back().is_some_and(is_word)
                        && !text[m.end()..].chars().next().is_some_and(is_word))
            })
            .map(|m| m.range())
            .collect()
    }

    /// Replacements for every match and the resulting text (`SPEC-0029` behaviour 4).
    pub fn replace(&self, text: &str, replacement: &str) -> (Vec<Replacement>, String) {
        let Some(regex) = &self.regex else {
            return (Vec::new(), text.to_owned());
        };
        let mut out = String::with_capacity(text.len());
        let mut list = Vec::new();
        let mut at = 0;
        let mut locator = Locator::new(text);
        for range in self.ranges(text) {
            let new = if self.query.regex {
                let caps = regex
                    .captures_at(text, range.start)
                    .expect("the range came from this regex");
                let mut expanded = String::new();
                caps.expand(replacement, &mut expanded);
                expanded
            } else {
                replacement.to_owned()
            };
            out.push_str(&text[at..range.start]);
            out.push_str(&new);
            at = range.end;
            list.push(Replacement {
                found: locator.locate(range.start, range.end),
                old: text[range.clone()].to_owned(),
                new,
            });
        }
        out.push_str(&text[at..]);
        (list, out)
    }
}

/// Line, column and char offsets of byte ranges given in text order, counted on from the previous
/// range instead of from the start of the text (linear instead of quadratic in the text size).
struct Locator<'a> {
    text: &'a str,
    /// Byte offset counted up to, with its char offset and line.
    byte: usize,
    chars: usize,
    line: usize,
    /// Byte and char offset where that line starts.
    line_start: usize,
    line_start_chars: usize,
}

impl<'a> Locator<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            chars: 0,
            line: 0,
            line_start: 0,
            line_start_chars: 0,
        }
    }

    fn locate(&mut self, start: usize, end: usize) -> Match {
        let text = self.text;
        let skipped = &text[self.byte..start];
        if let Some(nl) = skipped.rfind('\n') {
            self.line += skipped.bytes().filter(|&b| b == b'\n').count();
            self.line_start = self.byte + nl + 1;
            self.line_start_chars = self.chars + skipped[..=nl].chars().count();
        }
        self.chars += skipped.chars().count();
        self.byte = start;
        let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        Match {
            start: self.chars,
            end: self.chars + text[start..end].chars().count(),
            line: self.line,
            column: self.chars - self.line_start_chars,
            line_text: text[self.line_start..line_end]
                .trim_end_matches('\r')
                .to_owned(),
        }
    }
}

/// Whether `name` matches one of the comma-separated globs in `mask` (`*`, `?`).
pub fn matches_mask(name: &str, mask: &str) -> bool {
    fn glob(p: &[char], s: &[char]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some('*'), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
            (Some('?'), Some(_)) => glob(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => glob(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let name: Vec<char> = name.chars().collect();
    mask.split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .any(|g| glob(&g.chars().collect::<Vec<_>>(), &name))
}

/// Matches of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    /// Workspace-relative path.
    pub path: PathBuf,
    /// Matches in the file.
    pub matches: Vec<Match>,
}

/// Result of [`search_workspace`] and [`search_scope`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceSearch {
    /// Files with matches, sorted by path.
    pub files: Vec<FileMatches>,
    /// Files skipped as binary or larger than 10 MiB.
    pub skipped: usize,
    /// Whether `max_matches` cut the result short.
    pub truncated: bool,
    /// What the search left out because it is not the project's own (`SPEC-0029` v2 behaviour
    /// 10): the topmost folder or the single file each, sorted.
    pub not_searched: Vec<LeftOut>,
}

impl WorkspaceSearch {
    /// How many folders and files were not searched.
    pub fn not_searched_counts(&self) -> (usize, usize) {
        let folders = self.not_searched.iter().filter(|o| o.folder).count();
        (folders, self.not_searched.len() - folders)
    }

    /// One line on what was not searched, `None` when nothing was left out (`SPEC-0029` v2
    /// behaviour 10).
    pub fn not_searched_line(&self) -> Option<String> {
        let (folders, files) = self.not_searched_counts();
        not_searched_line(folders, files)
    }
}

/// `2 folders and 1 file not searched (libraries and ignored files)`; `None` for nothing.
pub fn not_searched_line(folders: usize, files: usize) -> Option<String> {
    left_out_line(folders, files, "not searched")
}

/// `2 folders and 1 file <done> (libraries and ignored files)`; `None` for nothing.
pub fn left_out_line(folders: usize, files: usize, done: &str) -> Option<String> {
    let count = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let parts: Vec<String> = [
        count(folders, "folder", "folders"),
        count(files, "file", "files"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| {
        format!(
            "{} {done} (libraries and ignored files)",
            parts.join(" and ")
        )
    })
}

/// The place Find in Files looks in (`SPEC-0029` v2 behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SearchPlace {
    /// The whole project.
    #[default]
    Project,
    /// Below a folder (or a single file), relative to the project folder. A folder that is not
    /// the project's own is searched completely, because it was named.
    Directory(PathBuf),
    /// These files, relative to the project folder, whatever they are (the open files).
    Files(Vec<PathBuf>),
}

/// Where Find in Files looks (`SPEC-0029` v2 behaviours 5, 7 and 8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchScope {
    /// Comma-separated globs on the file name; all files when `None`.
    pub mask: Option<String>,
    /// The place.
    pub place: SearchPlace,
    /// Also what is not the project's own: libraries and ignored files.
    pub include_non_project: bool,
}

/// `path` without `.` parts when it only leads downwards; `None` for an absolute path or one
/// with `..`.
fn downwards(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::Normal(p) => out.push(p),
            _ => return None,
        }
    }
    Some(out)
}

/// The files `scope` searches, relative to the project folder and sorted, and what it leaves out
/// as not the project's own (`SPEC-0029` v2 behaviours 7, 8 and 10). The file mask is applied.
pub fn search_candidates(
    workspace: &Workspace,
    scope: &SearchScope,
) -> (Vec<PathBuf>, Vec<LeftOut>) {
    let masked = |path: &Path| {
        scope.mask.as_deref().is_none_or(|mask| {
            path.file_name()
                .is_some_and(|n| matches_mask(&n.to_string_lossy(), mask))
        })
    };
    let root = workspace.root();
    let (mut files, mut left_out) = match &scope.place {
        SearchPlace::Files(list) => {
            let mut files: Vec<PathBuf> = list
                .iter()
                .filter_map(|f| downwards(f))
                .filter(|f| std::fs::symlink_metadata(root.join(f)).is_ok_and(|m| m.is_file()))
                .collect();
            files.sort();
            files.dedup();
            (files, Vec::new())
        }
        place => {
            let below = match place {
                SearchPlace::Directory(dir) => match downwards(dir) {
                    Some(dir) if dir.as_os_str().is_empty() => None,
                    Some(dir) => Some(dir),
                    None => return (Vec::new(), Vec::new()),
                },
                _ => None,
            };
            // A named place that is not the project's own is searched completely; nothing outside
            // the project or behind a symbolic link is read.
            match OwnSources::new(root).chosen(below.as_deref(), scope.include_non_project) {
                Ok(listing) => (listing.files, listing.left_out),
                Err(_) => (Vec::new(), Vec::new()),
            }
        }
    };
    files.retain(|f| masked(f));
    left_out.retain(|o| o.folder || masked(&o.path));
    (files, left_out)
}

/// Why a replacement must not change `path` (relative to the project folder); `None` when it may
/// (`SPEC-0029` v2 behaviour 9). A file that is not the project's own is changed only with
/// `include_non_project`; version control data, Ferris IDEA's own data, files behind a symbolic link
/// and files outside the project folder never.
pub fn not_replaceable(own: &OwnSources, path: &Path, include_non_project: bool) -> Option<NotOwn> {
    let why = own.not_own(path)?;
    let never = matches!(why, NotOwn::OutsideProject | NotOwn::SymbolicLink)
        || path
            .components()
            .any(|c| crate::own_sources::never_listed(c.as_os_str()));
    (never || !include_non_project).then_some(why)
}

/// Largest file searched, in bytes.
pub const MAX_SEARCH_BYTES: u64 = 10 * 1024 * 1024;

/// Reads a workspace file as searchable text; `None` for binary, oversize or unreadable files.
pub fn searchable_text(workspace: &Workspace, relative: &std::path::Path) -> Option<String> {
    let path = workspace.root().join(relative);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > MAX_SEARCH_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Searches the project's own files matching `mask` (all when `None`) (`SPEC-0029` behaviours 5
/// and 7).
pub fn search_workspace(
    workspace: &Workspace,
    query: &Query,
    mask: Option<&str>,
    max_matches: usize,
) -> Result<WorkspaceSearch, SearchError> {
    let scope = SearchScope {
        mask: mask.map(str::to_owned),
        ..SearchScope::default()
    };
    search_scope(workspace, query, &scope, max_matches)
}

/// Searches the files of `scope` (`SPEC-0029` v2 behaviours 5 and 7 to 10).
pub fn search_scope(
    workspace: &Workspace,
    query: &Query,
    scope: &SearchScope,
    max_matches: usize,
) -> Result<WorkspaceSearch, SearchError> {
    let finder = Finder::new(query)?;
    let (candidates, not_searched) = search_candidates(workspace, scope);
    let mut result = WorkspaceSearch {
        not_searched,
        ..WorkspaceSearch::default()
    };
    let mut total = 0;
    for relative in candidates {
        let Some(text) = searchable_text(workspace, &relative) else {
            result.skipped += 1;
            continue;
        };
        let mut matches = finder.find(&text);
        if matches.is_empty() {
            continue;
        }
        if total + matches.len() > max_matches {
            matches.truncate(max_matches - total);
            result.truncated = true;
        }
        total += matches.len();
        if !matches.is_empty() {
            result.files.push(FileMatches {
                path: relative,
                matches,
            });
        }
        if result.truncated {
            break;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pattern: &str) -> Query {
        Query {
            pattern: pattern.into(),
            ..Query::default()
        }
    }

    #[test]
    fn literal_regex_case_and_positions() {
        let text = "Größe = size\nSIZE size(x)\n";
        let f = Finder::new(&q("size")).unwrap();
        let m = f.find(text);
        assert_eq!(m.len(), 3, "case-insensitive by default");
        assert_eq!((m[0].start, m[0].line, m[0].column), (8, 0, 8));
        assert_eq!(m[1].line_text, "SIZE size(x)");
        let cased = Finder::new(&Query {
            match_case: true,
            ..q("size")
        })
        .unwrap();
        assert_eq!(cased.find(text).len(), 2);
        let literal = Finder::new(&q("size(x)")).unwrap();
        assert_eq!(literal.find(text).len(), 1, "parentheses are literal");
        let multi = Finder::new(&Query {
            regex: true,
            ..q(r"size\nSIZE")
        })
        .unwrap();
        let m = multi.find(text);
        assert_eq!((m[0].start, m[0].end), (8, 17));
        let anchored = Finder::new(&Query {
            regex: true,
            ..q("^size")
        })
        .unwrap();
        assert_eq!(anchored.find(text).len(), 1, "^ is a line start");
    }

    #[test]
    fn positions_are_counted_on_from_match_to_match() {
        let text = "ä x\r\nöö x x\n\nü\r\nx ß x\nx";
        let m = Finder::new(&q("x")).unwrap().find(text);
        let naive: Vec<(usize, usize, usize, &str)> = text
            .match_indices('x')
            .map(|(b, _)| {
                let before = &text[..b];
                let start = before.rfind('\n').map_or(0, |i| i + 1);
                let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
                (
                    before.chars().count(),
                    before.matches('\n').count(),
                    text[start..b].chars().count(),
                    text[start..end].trim_end_matches('\r'),
                )
            })
            .collect();
        let found: Vec<_> = m
            .iter()
            .map(|m| (m.start, m.line, m.column, m.line_text.as_str()))
            .collect();
        assert_eq!(found, naive);
        assert!(m.iter().all(|m| m.end == m.start + 1));
        let (list, _) = Finder::new(&q("x")).unwrap().replace(text, "y");
        assert_eq!(list.last().unwrap().found, *m.last().unwrap());
    }

    #[test]
    fn whole_words_errors_and_empty() {
        let text = "foo foobar _foo foo.bar (foo)";
        let w = |p: &str| {
            Finder::new(&Query {
                whole_words: true,
                ..q(p)
            })
            .unwrap()
            .find(text)
            .len()
        };
        assert_eq!(w("foo"), 3);
        assert_eq!(w("(foo)"), 1, "punctuation at the edges");
        assert_eq!(w(".bar"), 0, "`foo.bar`: preceded by a word char");
        let err = Finder::new(&Query {
            regex: true,
            ..q("(unclosed")
        })
        .unwrap_err();
        assert!(err.to_string().starts_with("invalid regular expression"));
        assert!(Finder::new(&q("")).unwrap().find(text).is_empty());
    }

    #[test]
    fn replacement_with_groups() {
        let re = |p: &str| {
            Finder::new(&Query {
                regex: true,
                match_case: true,
                ..q(p)
            })
            .unwrap()
        };
        let (list, out) = re(r"(\w+)=(\d+)").replace("a=1, b=22", "$2:$1");
        assert_eq!(out, "1:a, 22:b");
        assert_eq!(
            (list[1].old.as_str(), list[1].new.as_str()),
            ("b=22", "22:b")
        );
        let (_, named) = re(r"(?P<k>\w+)=").replace("x=1", "${k} costs $$");
        assert_eq!(named, "x costs $1");
        let (_, literal) = Finder::new(&q("a")).unwrap().replace("a-A", "$1");
        assert_eq!(literal, "$1-$1", "no expansion without regex");
    }

    #[test]
    fn masks() {
        assert!(matches_mask("main.rs", "*.rs, *.toml"));
        assert!(matches_mask("Cargo.toml", "*.rs,*.toml"));
        assert!(matches_mask("a1.txt", "a?.txt"));
        assert!(!matches_mask("main.rsx", "*.rs"));
    }
}

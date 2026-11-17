//! Rows and selection of the Find in Files panel (`SPEC-0066` behaviours 2–4), without toolkit.

use std::path::PathBuf;

use idecraft_core::WorkspaceSearch;

/// One row of the result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultRow {
    /// A file with its number of matches.
    File {
        /// Workspace-relative path.
        path: PathBuf,
        /// Matches in the file.
        count: usize,
    },
    /// One match.
    Hit {
        /// Workspace-relative path.
        path: PathBuf,
        /// Char range in the file.
        start: usize,
        /// End of the char range.
        end: usize,
        /// 0-based line.
        line: usize,
        /// Text of the line, trimmed at the start.
        text: String,
        /// Char range of the match within `text`.
        within: std::ops::Range<usize>,
    },
}

/// The result list with the selected match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultList {
    /// File rows, each followed by its matches.
    pub rows: Vec<ResultRow>,
    /// Index of the selected match row.
    pub selected: Option<usize>,
    /// Number of matches.
    pub total: usize,
    /// Number of files with matches.
    pub files: usize,
    /// Whether the search stopped at the match limit.
    pub truncated: bool,
    /// What the search left out as not the project's own, in one line (`SPEC-0066` v4 behaviour
    /// 16); `None` when nothing was left out.
    pub not_searched: Option<String>,
}

impl ResultList {
    /// Rows of a search; the first match is selected (behaviour 2).
    pub fn from_search(search: &WorkspaceSearch) -> Self {
        let mut rows = Vec::new();
        let mut total = 0;
        for f in &search.files {
            rows.push(ResultRow::File {
                path: f.path.clone(),
                count: f.matches.len(),
            });
            for m in &f.matches {
                let trimmed = m.line_text.trim_start();
                let cut = m.line_text.chars().count() - trimmed.chars().count();
                let first_line_len = trimmed.chars().count();
                let from = m.column.saturating_sub(cut).min(first_line_len);
                let to = (from + (m.end - m.start)).min(first_line_len);
                rows.push(ResultRow::Hit {
                    path: f.path.clone(),
                    start: m.start,
                    end: m.end,
                    line: m.line,
                    text: trimmed.trim_end().to_owned(),
                    within: from..to,
                });
                total += 1;
            }
        }
        let selected = rows.iter().position(|r| matches!(r, ResultRow::Hit { .. }));
        Self {
            rows,
            selected,
            total,
            files: search.files.len(),
            truncated: search.truncated,
            not_searched: search.not_searched_line(),
        }
    }

    /// Moves the selection to the next or previous match row, skipping file rows, without wrapping
    /// (behaviour 3).
    pub fn step(&mut self, down: bool) {
        let Some(at) = self.selected else {
            return;
        };
        let is_hit = |i: &usize| matches!(self.rows[*i], ResultRow::Hit { .. });
        let next = if down {
            (at + 1..self.rows.len()).find(is_hit)
        } else {
            (0..at).rev().find(is_hit)
        };
        if let Some(n) = next {
            self.selected = Some(n);
        }
    }

    /// The selected match: path and char range.
    pub fn selected_hit(&self) -> Option<(PathBuf, std::ops::Range<usize>)> {
        match self.rows.get(self.selected?)? {
            ResultRow::Hit {
                path, start, end, ..
            } => Some((path.clone(), *start..*end)),
            ResultRow::File { .. } => None,
        }
    }

    /// Summary line: `3 matches in 2 files`, with `1000+` when truncated (behaviour 4).
    pub fn summary(&self) -> String {
        let plus = if self.truncated { "+" } else { "" };
        let matches = if self.total == 1 { "match" } else { "matches" };
        let files = if self.files == 1 { "file" } else { "files" };
        format!("{}{plus} {matches} in {} {files}", self.total, self.files)
    }
}

/// The edits that replace the matches of `finder` in `text` with `replacement` (regex groups
/// expanded), as char ranges with their new text; with `only`, just the match starting there
/// (`SPEC-0066` v2 behaviour 7).
pub fn plan_replacements(
    text: &str,
    finder: &idecraft_core::Finder,
    replacement: &str,
    only: Option<usize>,
) -> Vec<(std::ops::Range<usize>, String)> {
    finder
        .replace(text, replacement)
        .0
        .into_iter()
        .filter(|r| only.is_none_or(|start| r.found.start == start))
        .map(|r| (r.found.start..r.found.end, r.new))
        .collect()
}

/// Writes the planned edits of `file`, a file of the workspace at `root` that no editor has open,
/// to disk, with a local history revision before and after, so Show History has the old text
/// (`SPEC-0066` v2 behaviour 7). The file is written only if it still holds the text the edits
/// were planned for (`SPEC-0066` v5 behaviour 17); `Ok(false)` when it changed in between.
pub fn write_planned(
    root: &std::path::Path,
    file: &PlannedFile,
    history: Option<&mut idecraft_core::LocalHistory>,
) -> std::io::Result<bool> {
    let path = root.join(&file.path);
    if file.edits.is_empty() || std::fs::read_to_string(&path).ok().as_deref() != Some(&file.text) {
        return Ok(false);
    }
    let relative = file.path.to_string_lossy().replace('\\', "/");
    let mut history = history;
    if let Some(h) = history.as_deref_mut() {
        let _ = h.record(&relative);
    }
    std::fs::write(&path, apply_edits(&file.text, &file.edits))?;
    if let Some(h) = history {
        let _ = h.record(&relative);
    }
    Ok(true)
}

/// `text` with `edits` (sorted, non-overlapping char ranges) applied.
pub fn apply_edits(text: &str, edits: &[(std::ops::Range<usize>, String)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, new) in edits {
        out.extend(&chars[at..range.start.min(chars.len())]);
        out.push_str(new);
        at = range.end.min(chars.len());
    }
    out.extend(&chars[at..]);
    out
}

/// The files of `files` (relative to the project at `root`) a replacement may change, and those it
/// must leave with the reason (`SPEC-0029` v2 behaviour 9, `SPEC-0066` v4 behaviour 15): a file
/// that is not the project's own only with `include_non_project`; version control data,
/// IDECraft's data and files behind a symbolic link never.
pub fn split_replaceable(
    root: &std::path::Path,
    files: Vec<PathBuf>,
    include_non_project: bool,
) -> (Vec<PathBuf>, Vec<(PathBuf, idecraft_core::NotOwn)>) {
    let own = idecraft_core::OwnSources::new(root);
    let (mut change, mut leave) = (Vec::new(), Vec::new());
    for file in files {
        match idecraft_core::not_replaceable(&own, &file, include_non_project) {
            Some(why) => leave.push((file, why)),
            None => change.push(file),
        }
    }
    (change, leave)
}

/// The status text for the files a replacement left unchanged: `2 files not changed: not the
/// project's own sources (in a virtual environment)`; `None` for none.
pub fn left_unchanged_line(left: &[(PathBuf, idecraft_core::NotOwn)]) -> Option<String> {
    let (_, first) = left.first()?;
    let files = if left.len() == 1 {
        "1 file".to_owned()
    } else {
        format!("{} files", left.len())
    };
    let reason = if left.iter().all(|(_, why)| why == first) {
        format!(" ({})", first.reason())
    } else {
        String::new()
    };
    Some(format!(
        "{files} not changed: not the project's own sources{reason}"
    ))
}

/// Where Find in Files looks (`SPEC-0066` v3 behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// Every project file.
    #[default]
    Project,
    /// Files below a workspace-relative folder.
    Directory(PathBuf),
    /// The files open in editors (workspace-relative).
    OpenFiles(Vec<PathBuf>),
}

impl Scope {
    /// The place for the search (`SPEC-0029` v2 behaviour 8): a named folder and the open files
    /// are searched whatever they are.
    pub fn place(&self) -> idecraft_core::SearchPlace {
        match self {
            Scope::Project => idecraft_core::SearchPlace::Project,
            Scope::Directory(dir) => idecraft_core::SearchPlace::Directory(dir.clone()),
            Scope::OpenFiles(files) => idecraft_core::SearchPlace::Files(files.clone()),
        }
    }

    /// Whether the workspace-relative `path` lies in the scope.
    pub fn contains(&self, path: &std::path::Path) -> bool {
        match self {
            Scope::Project => true,
            Scope::Directory(dir) => dir.as_os_str().is_empty() || path.starts_with(dir),
            Scope::OpenFiles(files) => files.iter().any(|f| f == path),
        }
    }
}

/// Keeps the matches of `text`, the content of `path`, that `context` allows (comments and string
/// literals, `SPEC-0066` v3 behaviour 11). The one place the filter is applied, for the result
/// list and for a replacement alike.
fn keep_context(
    context: idecraft_languages::syntax::SearchContext,
    path: &std::path::Path,
    text: &str,
    matches: &mut Vec<idecraft_core::Match>,
) {
    if context == idecraft_languages::syntax::SearchContext::Anywhere {
        return;
    }
    let ranges: Vec<std::ops::Range<usize>> = matches.iter().map(|m| m.start..m.end).collect();
    let kept = context.filter(path, text, ranges);
    matches.retain(|m| kept.contains(&(m.start..m.end)));
}

/// Keeps the files of `search` in `scope` and the matches that `context` allows (comments and
/// string literals, `SPEC-0066` v3 behaviours 8 and 11); files are read from `root` for the context.
pub fn narrow(
    search: &mut WorkspaceSearch,
    root: &std::path::Path,
    scope: &Scope,
    context: idecraft_languages::syntax::SearchContext,
) {
    search.files.retain(|f| scope.contains(&f.path));
    if context == idecraft_languages::syntax::SearchContext::Anywhere {
        return;
    }
    for f in &mut search.files {
        let Ok(text) = std::fs::read_to_string(root.join(&f.path)) else {
            continue;
        };
        keep_context(context, &f.path, &text, &mut f.matches);
    }
    search.files.retain(|f| !f.matches.is_empty());
}

/// What Find in Files searches for and where: everything that decides the result list, and so
/// everything that decides what Replace and Replace All change (`SPEC-0066` v5 behaviour 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    /// Pattern and options.
    pub query: idecraft_core::Query,
    /// File mask as typed, empty for all files.
    pub mask: String,
    /// Project, Directory or Open Files.
    pub scope: Scope,
    /// The comments/strings filter.
    pub context: idecraft_languages::syntax::SearchContext,
    /// Also libraries and ignored files (`SPEC-0066` v4 behaviour 15).
    pub include_non_project: bool,
}

/// The files a replacement changes with their edits, and what it leaves (`SPEC-0066` v5
/// behaviour 17).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplacementPlan {
    /// Files to change, sorted by path; none without edits.
    pub files: Vec<PlannedFile>,
    /// Files with matches that must not be changed, with the reason (`SPEC-0029` v2 behaviour 9).
    pub left: Vec<(PathBuf, idecraft_core::NotOwn)>,
    /// Replace of one match: the match is no longer where the list showed it.
    pub gone: bool,
}

/// The planned edits of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Workspace-relative path.
    pub path: PathBuf,
    /// The text the edits were planned for: the editor's when `open`, else the file's on disk.
    pub text: String,
    /// Sorted, non-overlapping char ranges of `text` with their new text.
    pub edits: Vec<(std::ops::Range<usize>, String)>,
    /// Whether `text` came from an open editor.
    pub open: bool,
}

impl SearchRequest {
    /// The matches of the result list, at most `max_matches` (`SPEC-0066` behaviours 2, 8, 11, 14
    /// and 15): the scope's files by `SPEC-0029` v2, narrowed by the comments/strings filter. Read
    /// from disk.
    pub fn list(
        &self,
        workspace: &idecraft_core::Workspace,
        max_matches: usize,
    ) -> Result<WorkspaceSearch, idecraft_core::SearchError> {
        let mask = self.mask.trim();
        // Only the project's own sources unless a place is named or the toggle is on
        // (SPEC-0066 v4 behaviours 14 and 15).
        let within = idecraft_core::SearchScope {
            mask: (!mask.is_empty()).then(|| mask.to_owned()),
            place: self.scope.place(),
            include_non_project: self.include_non_project,
        };
        let mut found = idecraft_core::search_scope(workspace, &self.query, &within, max_matches)?;
        narrow(&mut found, workspace.root(), &self.scope, self.context);
        Ok(found)
    }

    /// [`Self::list`] with the matches of each listed file that is open with unsaved changes found
    /// in its editor's text (`unsaved`: workspace-relative path and text) with the same filter,
    /// so the list shows what the editors hold; a file whose editor text no longer matches leaves
    /// the list (`SPEC-0066` v7 behaviour 19).
    pub fn list_with(
        &self,
        workspace: &idecraft_core::Workspace,
        max_matches: usize,
        unsaved: &[(PathBuf, String)],
    ) -> Result<WorkspaceSearch, idecraft_core::SearchError> {
        let mut found = self.list(workspace, max_matches)?;
        if unsaved.is_empty() {
            return Ok(found);
        }
        let finder = idecraft_core::Finder::new(&self.query)?;
        for file in &mut found.files {
            if let Some((_, text)) = unsaved.iter().find(|(path, _)| *path == file.path) {
                let mut matches = finder.find(text);
                keep_context(self.context, &file.path, text, &mut matches);
                file.matches = matches;
            }
        }
        found.files.retain(|f| !f.matches.is_empty());
        Ok(found)
    }

    /// What Replace All — or, with `only` (a file and the listed match's char range), Replace —
    /// changes: exactly the matches [`Self::list`] returns now, searched again at this moment, so
    /// nothing outside the scope or the filter and nothing that no longer matches is touched
    /// (`SPEC-0066` v5 behaviour 17). `open_text` gives the text of a file open in an editor; its
    /// matches are found again in that text with the same filter. Nothing is written here.
    pub fn plan_replacement(
        &self,
        workspace: &idecraft_core::Workspace,
        replacement: &str,
        only: Option<&(PathBuf, std::ops::Range<usize>)>,
        open_text: &dyn Fn(&std::path::Path) -> Option<String>,
    ) -> Result<ReplacementPlan, idecraft_core::SearchError> {
        let finder = idecraft_core::Finder::new(&self.query)?;
        let listed = self.list(workspace, usize::MAX)?;
        let files: Vec<PathBuf> = listed
            .files
            .into_iter()
            .map(|f| f.path)
            .filter(|path| only.is_none_or(|(file, _)| file == path))
            .collect();
        let (files, left) = split_replaceable(workspace.root(), files, self.include_non_project);
        let mut plan = ReplacementPlan {
            left,
            ..ReplacementPlan::default()
        };
        for path in files {
            let (text, open) = match open_text(&path) {
                Some(text) => (text, true),
                None => match std::fs::read_to_string(workspace.root().join(&path)) {
                    Ok(text) => (text, false),
                    Err(_) => continue,
                },
            };
            // The matches the list has for this text: the same finder, the same filter.
            let mut matches = finder.find(&text);
            keep_context(self.context, &path, &text, &mut matches);
            let edits: Vec<(std::ops::Range<usize>, String)> = finder
                .replace(&text, replacement)
                .0
                .into_iter()
                .map(|r| (r.found.start..r.found.end, r.new))
                .filter(|(range, _)| matches.iter().any(|m| (m.start..m.end) == *range))
                .filter(|(range, _)| only.is_none_or(|(_, listed)| listed == range))
                .collect();
            if !edits.is_empty() {
                plan.files.push(PlannedFile {
                    path,
                    text,
                    edits,
                    open,
                });
            }
        }
        plan.gone = only.is_some() && plan.files.is_empty() && plan.left.is_empty();
        Ok(plan)
    }
}

/// The status text after a replacement (`SPEC-0066` v5 behaviour 17): `Replaced 3 occurrences in
/// 2 files`, then what was not changed and why — files that are not the project's own (`left`),
/// files that could not be written or changed in between (`failed`, each with its reason), and
/// for Replace that the match is `gone`.
pub fn replaced_line(
    count: usize,
    changed: usize,
    left: &[(PathBuf, idecraft_core::NotOwn)],
    failed: &[String],
    gone: bool,
) -> String {
    let mut parts = vec![if gone && count == 0 {
        "Nothing replaced: the match is no longer there, the file changed since the search"
            .to_owned()
    } else {
        format!(
            "Replaced {count} {} in {changed} {}",
            if count == 1 {
                "occurrence"
            } else {
                "occurrences"
            },
            if changed == 1 { "file" } else { "files" }
        )
    }];
    parts.extend(left_unchanged_line(left));
    if !failed.is_empty() {
        parts.push(format!("not changed: {}", failed.join(", ")));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_core::{FileMatches, Match};

    /// SPEC-0066 v3 AK-5: scope and comment filter narrow a search.
    #[test]
    fn scope_and_context_narrow_the_results() {
        let root = std::env::temp_dir().join(format!("idecraft-fif-scope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("a/A.java"), "// total\nlong total;\n").unwrap();
        std::fs::write(root.join("b/B.java"), "long total;\n").unwrap();
        let ws = idecraft_core::Workspace::open(&root).unwrap();
        let query = idecraft_core::Query {
            pattern: "total".into(),
            ..Default::default()
        };
        let search = idecraft_core::search_workspace(&ws, &query, None, 100).unwrap();
        assert_eq!(search.files.len(), 2);
        let mut s = search.clone();
        narrow(
            &mut s,
            ws.root(),
            &Scope::Directory("a".into()),
            idecraft_languages::syntax::SearchContext::Anywhere,
        );
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].matches.len(), 2);
        let mut s = search.clone();
        narrow(
            &mut s,
            ws.root(),
            &Scope::Project,
            idecraft_languages::syntax::SearchContext::ExceptComments,
        );
        assert_eq!(s.files.iter().map(|f| f.matches.len()).sum::<usize>(), 2);
        let mut s = search;
        narrow(
            &mut s,
            ws.root(),
            &Scope::OpenFiles(vec!["b/B.java".into()]),
            idecraft_languages::syntax::SearchContext::Anywhere,
        );
        assert_eq!(s.files.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Hand test 6 of round 4: after Replace All in a closed file, Show History has the text from
    /// before the replacement.
    #[test]
    fn replace_on_disk_keeps_the_old_text_in_the_local_history() {
        let root =
            std::env::temp_dir().join(format!("idecraft-replace-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let old = "void report(int total) {\n    print(total, twice(total));\n}\n";
        std::fs::write(root.join("util.cpp"), old).unwrap();
        let ws = idecraft_core::Workspace::open(&root).unwrap();
        let plan = request("total", Scope::Project)
            .plan_replacement(&ws, "sum", None, &|_| None)
            .unwrap();
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].edits.len(), 3);
        let mut history = idecraft_core::LocalHistory::open(&root).unwrap();
        assert!(write_planned(&root, &plan.files[0], Some(&mut history)).unwrap());
        let new = std::fs::read_to_string(root.join("util.cpp")).unwrap();
        assert_eq!(new, old.replace("total", "sum"));
        // What the history view lists, oldest first: the old text, then the new one.
        let history = idecraft_core::LocalHistory::open(&root).unwrap();
        let revisions = crate::history_model::shown(history.revisions("util.cpp"));
        let texts: Vec<String> = revisions
            .iter()
            .filter_map(|r| history.content("util.cpp", r.id).ok().flatten())
            .map(|b| String::from_utf8(b).unwrap())
            .collect();
        assert!(
            texts.iter().any(|t| t == old),
            "old text in the history: {texts:?}"
        );
        assert!(texts.contains(&new));
        // Nothing to replace: nothing planned. The plan of before is not written a second
        // time either, because the file no longer holds the text it was made for.
        let again = request("total", Scope::Project)
            .plan_replacement(&ws, "sum", None, &|_| None)
            .unwrap();
        assert!(again.files.is_empty());
        assert!(!write_planned(&root, &plan.files[0], None).unwrap());
        assert_eq!(std::fs::read_to_string(root.join("util.cpp")).unwrap(), new);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn request(pattern: &str, scope: Scope) -> SearchRequest {
        SearchRequest {
            query: idecraft_core::Query {
                pattern: pattern.into(),
                ..Default::default()
            },
            mask: String::new(),
            scope,
            context: idecraft_languages::syntax::SearchContext::Anywhere,
            include_non_project: false,
        }
    }

    /// A project folder with `files`, fresh.
    fn project(name: &str, files: &[(&str, &str)]) -> (PathBuf, idecraft_core::Workspace) {
        let root = std::env::temp_dir().join(format!("idecraft-fif-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let ws = idecraft_core::Workspace::open(&root).unwrap();
        (root, ws)
    }

    /// Plans and writes Replace All of `request` as the window does for closed files; returns
    /// the status line.
    fn replace_all(
        ws: &idecraft_core::Workspace,
        request: &SearchRequest,
        replacement: &str,
    ) -> String {
        let plan = request
            .plan_replacement(ws, replacement, None, &|_| None)
            .unwrap();
        // What is replaced is what the list shows.
        let listed = request.list(ws, usize::MAX).unwrap();
        let listed: Vec<(PathBuf, usize)> = listed
            .files
            .iter()
            .filter(|f| !plan.left.iter().any(|(left, _)| *left == f.path))
            .map(|f| (f.path.clone(), f.matches.len()))
            .collect();
        let planned: Vec<(PathBuf, usize)> = plan
            .files
            .iter()
            .map(|f| (f.path.clone(), f.edits.len()))
            .collect();
        assert_eq!(planned, listed);
        let (mut count, mut changed) = (0, 0);
        for file in &plan.files {
            assert!(write_planned(ws.root(), file, None).unwrap());
            count += file.edits.len();
            changed += 1;
        }
        replaced_line(count, changed, &plan.left, &[], plan.gone)
    }

    fn read(root: &std::path::Path, file: &str) -> String {
        std::fs::read_to_string(root.join(file)).unwrap()
    }

    /// SPEC-0066 v5 AK-7: Replace All changes the files of the Directory scope, or the open
    /// files, and nothing else; the rules of v4 for files that are not the project's own hold.
    #[test]
    fn replace_all_changes_only_what_the_scope_lists() {
        let files = [
            ("a/A.java", "long total;\nlong total2;\n"),
            ("a/deep/C.java", "long total;\n"),
            ("b/B.java", "long total;\n"),
            (".venv/lib/x.py", "total = 1\n"),
        ];
        // Directory: only below `a`.
        let (root, ws) = project("replace-dir", &files);
        let status = replace_all(&ws, &request("total", Scope::Directory("a".into())), "sum");
        assert_eq!(status, "Replaced 3 occurrences in 2 files");
        assert_eq!(read(&root, "a/A.java"), "long sum;\nlong sum2;\n");
        assert_eq!(read(&root, "a/deep/C.java"), "long sum;\n");
        assert_eq!(read(&root, "b/B.java"), "long total;\n");
        assert_eq!(read(&root, ".venv/lib/x.py"), "total = 1\n");
        // The file mask narrows the scope further.
        let mut masked = request("total", Scope::Directory("b".into()));
        masked.mask = "*.py".into();
        assert_eq!(
            replace_all(&ws, &masked, "sum"),
            "Replaced 0 occurrences in 0 files"
        );
        assert_eq!(read(&root, "b/B.java"), "long total;\n");
        // A named folder that is not the project's own is listed, but not changed without the
        // toggle; the status says so.
        let venv = request("total", Scope::Directory(".venv".into()));
        assert_eq!(venv.list(&ws, 100).unwrap().files.len(), 1);
        assert_eq!(
            replace_all(&ws, &venv, "sum"),
            "Replaced 0 occurrences in 0 files · 1 file not changed: not the project's own \
             sources (in a virtual environment)"
        );
        assert_eq!(read(&root, ".venv/lib/x.py"), "total = 1\n");
        let _ = std::fs::remove_dir_all(root);

        // Open Files: only the open file, also when the query is everywhere.
        let (root, ws) = project("replace-open", &files);
        let open = request("total", Scope::OpenFiles(vec!["b/B.java".into()]));
        assert_eq!(
            replace_all(&ws, &open, "sum"),
            "Replaced 1 occurrence in 1 file"
        );
        assert_eq!(read(&root, "b/B.java"), "long sum;\n");
        assert_eq!(read(&root, "a/A.java"), "long total;\nlong total2;\n");
        assert_eq!(read(&root, "a/deep/C.java"), "long total;\n");
        // No open file: nothing is listed and nothing changes.
        let none = request("total", Scope::OpenFiles(Vec::new()));
        assert_eq!(
            replace_all(&ws, &none, "sum"),
            "Replaced 0 occurrences in 0 files"
        );
        assert_eq!(read(&root, "a/A.java"), "long total;\nlong total2;\n");
        // Project: every own file, and never the virtual environment.
        assert_eq!(
            replace_all(&ws, &request("total", Scope::Project), "sum"),
            "Replaced 3 occurrences in 2 files"
        );
        assert_eq!(read(&root, ".venv/lib/x.py"), "total = 1\n");
        let _ = std::fs::remove_dir_all(root);
    }

    /// SPEC-0066 v5 AK-7: the comments/strings filter holds for a replacement, also for Replace
    /// of one match and in the text of an open editor.
    #[test]
    fn replace_all_keeps_what_the_filter_leaves_out() {
        use idecraft_languages::syntax::SearchContext;
        let text = "// total\nlong total;\nString s = \"total\";\n";
        let (root, ws) = project("replace-filter", &[("a/A.java", text), ("b/B.java", text)]);
        let mut code = request("total", Scope::Directory("a".into()));
        code.context = SearchContext::ExceptCommentsAndStrings;
        assert_eq!(
            replace_all(&ws, &code, "sum"),
            "Replaced 1 occurrence in 1 file"
        );
        assert_eq!(
            read(&root, "a/A.java"),
            "// total\nlong sum;\nString s = \"total\";\n"
        );
        assert_eq!(read(&root, "b/B.java"), text);
        let mut comments = request("total", Scope::Project);
        comments.context = SearchContext::InComments;
        assert_eq!(
            replace_all(&ws, &comments, "sum"),
            "Replaced 2 occurrences in 2 files"
        );
        assert_eq!(
            read(&root, "b/B.java"),
            "// sum\nlong total;\nString s = \"total\";\n"
        );
        // Replace of one match: a match the filter leaves out is not replaced even when asked
        // for by its place (the comment of b/B.java is now `sum`; this is its `total` in the
        // string).
        let strings_start = "// sum\nlong total;\nString s = \"".chars().count();
        let only = (PathBuf::from("b/B.java"), strings_start..strings_start + 5);
        let plan = code
            .plan_replacement(&ws, "sum", Some(&only), &|_| None)
            .unwrap();
        assert!(plan.files.is_empty(), "{plan:?}");
        // … because b/B.java is outside the Directory `a`; in the project it is still left
        // out, by the filter.
        code.scope = Scope::Project;
        let plan = code
            .plan_replacement(&ws, "sum", Some(&only), &|_| None)
            .unwrap();
        assert!(plan.files.is_empty() && plan.gone, "{plan:?}");
        let mut strings = code.clone();
        strings.context = SearchContext::InStrings;
        let plan = strings
            .plan_replacement(&ws, "sum", Some(&only), &|_| None)
            .unwrap();
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].edits, [(only.1.clone(), "sum".to_owned())]);
        assert!(!plan.gone);
        // An open editor: its text is searched with the same filter, unsaved changes included.
        let unsaved = "long total; // total\nlong total;\n";
        let editor = |path: &std::path::Path| {
            (path == std::path::Path::new("b/B.java")).then(|| unsaved.to_owned())
        };
        let plan = code.plan_replacement(&ws, "sum", None, &editor).unwrap();
        let b = plan
            .files
            .iter()
            .find(|f| f.path == std::path::Path::new("b/B.java"))
            .unwrap();
        assert!(b.open);
        assert_eq!(
            apply_edits(&b.text, &b.edits),
            "long sum; // total\nlong sum;\n"
        );
        // A file whose matches are only in unsaved text is not in the list (it reads the disk)
        // and is not replaced.
        let elsewhere = request("absent", Scope::Project)
            .plan_replacement(&ws, "x", None, &|_| Some("absent".to_owned()))
            .unwrap();
        assert!(elsewhere.files.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    /// SPEC-0066 v7 AK-9 (behaviour 19): a listed file open with unsaved changes shows the
    /// matches of its editor's text; after the editor's text was replaced it leaves the list.
    #[test]
    fn the_list_shows_the_matches_of_unsaved_editors() {
        let (root, ws) = project(
            "list-unsaved",
            &[
                ("a/A.java", "long total;\nlong total;\n"),
                ("b/B.java", "long total;\n"),
            ],
        );
        let all = request("total", Scope::Project);
        let disk = ResultList::from_search(&all.list(&ws, 1000).unwrap());
        assert_eq!(disk.summary(), "3 matches in 2 files");
        // One match left in the editor of A.java: its line, not the disk's two.
        let edited = vec![(
            PathBuf::from("a/A.java"),
            "long sum;\n\nlong total;\n".to_owned(),
        )];
        let found = all.list_with(&ws, 1000, &edited).unwrap();
        let a = found
            .files
            .iter()
            .find(|f| f.path == std::path::Path::new("a/A.java"))
            .unwrap();
        assert_eq!(a.matches.len(), 1);
        assert_eq!(a.matches[0].line, 2);
        assert_eq!(
            ResultList::from_search(&found).summary(),
            "2 matches in 2 files"
        );
        // Replace All in the editor, unsaved: the file is no longer listed.
        let replaced = vec![(
            PathBuf::from("a/A.java"),
            "long sum;\nlong sum;\n".to_owned(),
        )];
        let found = all.list_with(&ws, 1000, &replaced).unwrap();
        assert_eq!(
            ResultList::from_search(&found).summary(),
            "1 match in 1 file"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// SPEC-0066 v5 AK-7: a file that changed after the list was built is searched again;
    /// only what still matches inside the scope is replaced.
    #[test]
    fn replacing_searches_a_changed_file_again() {
        let (root, ws) = project(
            "replace-changed",
            &[
                ("a/A.java", "long total;\nlong total;\n"),
                ("a/Gone.java", "long total;\n"),
                ("b/B.java", "long total;\n"),
            ],
        );
        let in_a = request("total", Scope::Directory("a".into()));
        // The list as the panel shows it.
        let list = ResultList::from_search(&in_a.list(&ws, 1000).unwrap());
        assert_eq!(list.summary(), "3 matches in 2 files");
        let first = list.selected_hit().unwrap();
        assert_eq!(first, (PathBuf::from("a/A.java"), 5..10));
        // Then the files change: one match of A.java goes and the other moves, Gone.java has
        // no match any more, and b/B.java, outside the scope, gets more of them.
        std::fs::write(root.join("a/A.java"), "long count;\n  long total;\n").unwrap();
        std::fs::write(root.join("a/Gone.java"), "long count;\n").unwrap();
        std::fs::write(root.join("b/B.java"), "long total;\nlong total;\n").unwrap();
        // Replace of the selected match: it is no longer at its place, so nothing is written.
        let plan = in_a
            .plan_replacement(&ws, "sum", Some(&first), &|_| None)
            .unwrap();
        assert!(plan.files.is_empty() && plan.gone, "{plan:?}");
        assert_eq!(
            replaced_line(0, 0, &plan.left, &[], plan.gone),
            "Nothing replaced: the match is no longer there, the file changed since the search"
        );
        // Replace All: what still matches in `a`, nothing else.
        assert_eq!(
            replace_all(&ws, &in_a, "sum"),
            "Replaced 1 occurrence in 1 file"
        );
        assert_eq!(read(&root, "a/A.java"), "long count;\n  long sum;\n");
        assert_eq!(read(&root, "a/Gone.java"), "long count;\n");
        assert_eq!(read(&root, "b/B.java"), "long total;\nlong total;\n");
        // A file that changes between planning and writing is left as it is.
        let plan = request("total", Scope::Directory("b".into()))
            .plan_replacement(&ws, "sum", None, &|_| None)
            .unwrap();
        std::fs::write(root.join("b/B.java"), "int total;\n").unwrap();
        assert!(!write_planned(&root, &plan.files[0], None).unwrap());
        assert_eq!(read(&root, "b/B.java"), "int total;\n");
        assert_eq!(
            replaced_line(0, 0, &[], &["b/B.java (changed meanwhile)".into()], false),
            "Replaced 0 occurrences in 0 files · not changed: b/B.java (changed meanwhile)"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn m(start: usize, line: usize, column: usize, line_text: &str) -> Match {
        Match {
            start,
            end: start + 2,
            line,
            column,
            line_text: line_text.into(),
        }
    }

    fn search() -> WorkspaceSearch {
        WorkspaceSearch {
            files: vec![
                FileMatches {
                    path: "a.rs".into(),
                    matches: vec![m(0, 0, 0, "fn a() {}"), m(14, 1, 4, "    fn b() {}  ")],
                },
                FileMatches {
                    path: "src/c.rs".into(),
                    matches: vec![m(3, 0, 3, "// fn")],
                },
            ],
            skipped: 0,
            truncated: false,
            not_searched: Vec::new(),
        }
    }

    /// SPEC-0066 v4 AK-6: the list carries what was not searched, also without a match; a
    /// replacement stays in the project's own sources unless told otherwise.
    #[test]
    fn not_searched_line_and_replacements_in_the_own_sources() {
        let root = std::env::temp_dir().join(format!("idecraft-fif-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in [
            ("app.py", "total = 1\n"),
            (".venv/lib/python3.12/site-packages/x.py", "total = 2\n"),
            ("node_modules/y.js", "let total = 3;\n"),
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let ws = idecraft_core::Workspace::open(&root).unwrap();
        let query = |pattern: &str| idecraft_core::Query {
            pattern: pattern.into(),
            ..Default::default()
        };
        let line = "2 folders not searched (libraries and ignored files)";
        let found = idecraft_core::search_workspace(&ws, &query("total"), None, 100).unwrap();
        let list = ResultList::from_search(&found);
        assert_eq!(list.summary(), "1 match in 1 file");
        assert_eq!(list.not_searched.as_deref(), Some(line));
        let nothing = idecraft_core::search_workspace(&ws, &query("absent"), None, 100).unwrap();
        let list = ResultList::from_search(&nothing);
        assert!(list.rows.is_empty());
        assert_eq!(list.not_searched.as_deref(), Some(line));
        // The Directory scope names a place: it is searched, and nothing is left out.
        let scope = idecraft_core::SearchScope {
            place: Scope::Directory(".venv/lib".into()).place(),
            ..Default::default()
        };
        let venv = idecraft_core::search_scope(&ws, &query("total"), &scope, 100).unwrap();
        let list = ResultList::from_search(&venv);
        assert_eq!(list.summary(), "1 match in 1 file");
        assert_eq!(list.not_searched, None);
        // The open files are searched whatever they are.
        let scope = idecraft_core::SearchScope {
            place: Scope::OpenFiles(vec!["node_modules/y.js".into()]).place(),
            ..Default::default()
        };
        let open = idecraft_core::search_scope(&ws, &query("total"), &scope, 100).unwrap();
        assert_eq!(open.files.len(), 1);

        // Replacing: what the Directory scope found is left alone without the option.
        let files: Vec<PathBuf> = vec![
            "app.py".into(),
            ".venv/lib/python3.12/site-packages/x.py".into(),
            "node_modules/y.js".into(),
        ];
        let (change, leave) = split_replaceable(ws.root(), files.clone(), false);
        assert_eq!(change, [PathBuf::from("app.py")]);
        assert_eq!(leave.len(), 2);
        assert_eq!(
            left_unchanged_line(&leave).as_deref(),
            Some("2 files not changed: not the project's own sources")
        );
        assert_eq!(
            left_unchanged_line(&leave[..1]).as_deref(),
            Some("1 file not changed: not the project's own sources (in a virtual environment)")
        );
        assert_eq!(left_unchanged_line(&[]), None);
        let (change, leave) = split_replaceable(ws.root(), files, true);
        assert_eq!(change.len(), 3);
        assert!(leave.is_empty());
        let (change, leave) = split_replaceable(ws.root(), vec![".git/config".into()], true);
        assert!(change.is_empty());
        assert_eq!(leave.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn replacements_all_one_and_with_groups() {
        let q = |pattern: &str, regex: bool| {
            idecraft_core::Finder::new(&idecraft_core::Query {
                pattern: pattern.into(),
                regex,
                match_case: true,
                whole_words: false,
            })
            .unwrap()
        };
        let text = "äb total; total = b;";
        let all = plan_replacements(text, &q("total", false), "sum", None);
        assert_eq!(all, [(3..8, "sum".to_owned()), (10..15, "sum".to_owned())]);
        assert_eq!(apply_edits(text, &all), "äb sum; sum = b;");
        let one = plan_replacements(text, &q("total", false), "sum", Some(10));
        assert_eq!(apply_edits(text, &one), "äb total; sum = b;");
        let groups = plan_replacements("f(a, b)", &q(r"(\w), (\w)", true), "$2, $1", None);
        assert_eq!(apply_edits("f(a, b)", &groups), "f(b, a)");
    }

    #[test]
    fn rows_selection_and_summary() {
        let mut list = ResultList::from_search(&search());
        assert_eq!(list.rows.len(), 5);
        assert_eq!(list.selected, Some(1));
        assert_eq!(list.summary(), "3 matches in 2 files");
        match &list.rows[2] {
            ResultRow::Hit { text, within, .. } => {
                assert_eq!(text, "fn b() {}");
                assert_eq!(within, &(0..2), "match position after trimming");
            }
            other => panic!("{other:?}"),
        }
        list.step(true);
        list.step(true);
        assert_eq!(list.selected, Some(4), "file rows are skipped");
        list.step(true);
        assert_eq!(list.selected, Some(4), "no wrap at the end");
        assert_eq!(list.selected_hit(), Some(("src/c.rs".into(), 3..5)));
        list.step(false);
        assert_eq!(list.selected, Some(2));
        let empty = ResultList::from_search(&WorkspaceSearch::default());
        assert_eq!(
            (empty.selected, empty.summary()),
            (None, "0 matches in 0 files".into())
        );
    }
}

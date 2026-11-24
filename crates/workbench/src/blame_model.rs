//! Annotations of the editor (`SPEC-0277`), without toolkit: the blame of a file at HEAD mapped
//! onto the editor's lines, the column's labels, age colours and hover text, and the status texts
//! for files that cannot be annotated.
//!
//! The blame itself comes from [`ferris_idea_vcs::Repo::blame_head`] on a background thread; mapping
//! it onto the buffer after an edit ([`ferris_idea_vcs::HeadBlame::map`]) runs there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferris_idea_vcs::{BlameCommit, GitError, HeadBlame};

use crate::git_log_model::{DateStyle, LocalZone};

/// Annotate with Git Blame; closes the annotations while they are shown (behaviour 1).
pub const ANNOTATE: &str = "Annotate";
/// Close Annotations (behaviour 6).
pub const CLOSE_ANNOTATIONS: &str = "FerrisIDEA.CloseAnnotations";
/// Show the line's commit in the Git log (behaviour 6).
pub const SHOW_IN_GIT_LOG: &str = "FerrisIDEA.Annotation.ShowInGitLog";
/// Copy the line's full commit id (behaviour 6).
pub const COPY_REVISION: &str = "FerrisIDEA.Annotation.CopyRevision";

/// Label of a line changed since HEAD (behaviour 2).
pub const NOT_COMMITTED: &str = "not committed";
/// Most characters of an author's name in the column (behaviour 2).
pub const AUTHOR_CHARS: usize = 16;
/// Age colours of the scheme, newest first (behaviour 3).
pub const AGE_KEYS: [&str; 5] = [
    "VCS_ANNOTATIONS_COLOR_1",
    "VCS_ANNOTATIONS_COLOR_2",
    "VCS_ANNOTATIONS_COLOR_3",
    "VCS_ANNOTATIONS_COLOR_4",
    "VCS_ANNOTATIONS_COLOR_5",
];
/// Status text without an open editor (behaviour 9).
pub const NO_EDITOR: &str = "Open a file to annotate it";

/// The short date of `time` (seconds since the epoch) in `zone`, in the region's format like the Git
/// log (`SPEC-0277` v3 behaviour 14, OBS-0022 L18): `01.10.26`, `10/1/26`, `2026-10-01`.
pub fn date(time: i64, zone: &LocalZone, style: &DateStyle) -> String {
    style.date((time + zone.offset_at(time)).div_euclid(86_400))
}

/// Short date and time of `time` in `zone` and `style` (`01.10.26, 11:30`).
pub fn date_time(time: i64, zone: &LocalZone, style: &DateStyle) -> String {
    let local = time + zone.offset_at(time);
    let comma = if style.comma { "," } else { "" };
    format!(
        "{}{comma} {}",
        style.date(local.div_euclid(86_400)),
        style.clock(local.rem_euclid(86_400) / 60)
    )
}

/// The author's name, cut to [`AUTHOR_CHARS`] characters with `…`.
pub fn short_author(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= AUTHOR_CHARS {
        return name.to_owned();
    }
    let mut out: String = name.chars().take(AUTHOR_CHARS - 1).collect();
    out.push('…');
    out
}

/// The column's label of a commit: date and author (behaviour 2).
pub fn label(commit: &BlameCommit, zone: &LocalZone, style: &DateStyle) -> String {
    format!(
        "{} {}",
        date(commit.info.time, zone, style),
        short_author(&commit.info.author)
    )
}

/// Age bucket per commit, 0 (newest fifth by time) to 4 (oldest fifth) (behaviour 3).
pub fn age_buckets(commits: &[BlameCommit]) -> Vec<u8> {
    let mut order: Vec<usize> = (0..commits.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(commits[i].info.time));
    let n = commits.len().max(1);
    let mut out = vec![0u8; commits.len()];
    for (rank, i) in order.into_iter().enumerate() {
        out[i] = (rank * AGE_KEYS.len() / n) as u8;
    }
    out
}

/// What the column shows on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell<'a> {
    /// Nothing: a line after the end of the text (the empty line after the last line break).
    Empty,
    /// The line changed since HEAD.
    Uncommitted,
    /// The line's commit, its label, age bucket and whether it is the file's newest commit.
    Commit {
        /// The commit.
        commit: &'a BlameCommit,
        /// Date and author.
        label: &'a str,
        /// 0 (newest) to 4 (oldest).
        age: u8,
        /// The newest commit of the file.
        newest: bool,
    },
}

/// The annotations of one editor: the blame at HEAD and its commit per buffer line.
#[derive(Debug, Clone)]
pub struct Annotations {
    blame: Arc<HeadBlame>,
    /// Per buffer line, the index into the blame's commits.
    lines: Vec<Option<u32>>,
    labels: Vec<String>,
    ages: Vec<u8>,
    newest: Option<usize>,
    width: usize,
    zone: LocalZone,
    style: DateStyle,
}

impl Annotations {
    /// Annotations of `blame` with `lines` mapped onto the buffer; dates in `zone` and `style`.
    pub fn new(
        blame: Arc<HeadBlame>,
        lines: Vec<Option<u32>>,
        zone: LocalZone,
        style: DateStyle,
    ) -> Self {
        let labels: Vec<String> = blame
            .commits
            .iter()
            .map(|c| label(c, &zone, &style))
            .collect();
        let ages = age_buckets(&blame.commits);
        let newest = (0..blame.commits.len()).max_by_key(|&i| blame.commits[i].info.time);
        let width = labels
            .iter()
            .map(|l| l.chars().count())
            .chain([NOT_COMMITTED.len()])
            .max()
            .unwrap_or(0);
        Self {
            blame,
            lines,
            labels,
            ages,
            newest,
            width,
            zone,
            style,
        }
    }

    /// The blame at HEAD.
    pub fn blame(&self) -> &Arc<HeadBlame> {
        &self.blame
    }

    /// Takes the commit per buffer line after a remap (behaviour 7).
    pub fn set_lines(&mut self, lines: Vec<Option<u32>>) {
        self.lines = lines;
    }

    /// The commit per buffer line.
    pub fn lines(&self) -> &[Option<u32>] {
        &self.lines
    }

    /// Width of the column in characters: the longest label.
    pub fn width_chars(&self) -> usize {
        self.width
    }

    /// What the column shows on 0-based buffer line `line`.
    pub fn cell(&self, line: usize) -> Cell<'_> {
        match self.lines.get(line) {
            None => Cell::Empty,
            Some(None) => Cell::Uncommitted,
            Some(Some(i)) => {
                let i = *i as usize;
                match self.blame.commits.get(i) {
                    Some(commit) => Cell::Commit {
                        commit,
                        label: &self.labels[i],
                        age: self.ages[i],
                        newest: self.newest == Some(i),
                    },
                    None => Cell::Uncommitted,
                }
            }
        }
    }

    /// The commit of 0-based line `line`, if it has one.
    pub fn commit_at(&self, line: usize) -> Option<&BlameCommit> {
        match self.cell(line) {
            Cell::Commit { commit, .. } => Some(commit),
            _ => None,
        }
    }

    /// The hover card of line `line` (behaviour 4), one entry per text line; `None` after the end.
    pub fn hover(&self, line: usize) -> Option<Vec<String>> {
        match self.cell(line) {
            Cell::Empty => None,
            Cell::Uncommitted => Some(vec![
                "Not committed yet".into(),
                "This line changed since the last commit.".into(),
            ]),
            Cell::Commit { commit, .. } => {
                let i = &commit.info;
                let mut out = vec![
                    format!("{}  {} <{}>", i.short_id, i.author, i.email),
                    date_time(i.time, &self.zone, &self.style),
                    String::new(),
                ];
                out.extend(commit.message.lines().map(str::to_owned));
                out.push(String::new());
                out.push("Click to show the commit in the Git log".into());
                Some(out)
            }
        }
    }
}

/// The status text when `file` (its name) cannot be annotated (behaviour 9).
pub fn reason(error: &GitError, file: &str) -> String {
    match error {
        GitError::NotARepository(_) => {
            format!("Cannot annotate: {file} is not in a Git repository")
        }
        GitError::Untracked(_) => format!("Cannot annotate: {file} is not tracked by Git"),
        GitError::NotCommitted(_) => {
            format!("Cannot annotate: {file} has no committed version yet")
        }
        other => format!("Cannot annotate {file}: {other}"),
    }
}

/// The editor views waiting for the blame of a file (behaviours 10 and 13): one background read
/// per file serves every view that asked for it while it runs. `V` names a view (a weak handle in
/// the window, a number in tests).
#[derive(Debug)]
pub struct BlameRequests<V> {
    waiting: Vec<(PathBuf, Vec<V>)>,
}

impl<V> Default for BlameRequests<V> {
    fn default() -> Self {
        Self {
            waiting: Vec::new(),
        }
    }
}

impl<V: PartialEq> BlameRequests<V> {
    /// `view` asks for the blame of `path`; true when a read has to start, false when one runs
    /// already and will serve this view too.
    pub fn ask(&mut self, path: &Path, view: V) -> bool {
        match self.waiting.iter_mut().find(|(p, _)| p == path) {
            Some((_, views)) => {
                if !views.contains(&view) {
                    views.push(view);
                }
                false
            }
            None => {
                self.waiting.push((path.to_path_buf(), vec![view]));
                true
            }
        }
    }

    /// The read of `path` finished: the views that asked for it, in order.
    pub fn finish(&mut self, path: &Path) -> Vec<V> {
        match self.waiting.iter().position(|(p, _)| p == path) {
            Some(i) => self.waiting.remove(i).1,
            None => Vec::new(),
        }
    }

    /// Whether any read runs.
    pub fn running(&self) -> bool {
        !self.waiting.is_empty()
    }
}

/// The annotated views whose blame is stale (behaviour 8): `annotated` holds per view its file and
/// the HEAD text blamed; `heads` the HEAD text per file as the status poll just read it, or `None`
/// after a commit in Ferris IDEA, which makes every annotated view stale.
pub fn stale_views<V: Clone>(
    annotated: &[(V, PathBuf, String)],
    heads: Option<&[(PathBuf, Option<String>)]>,
) -> Vec<(PathBuf, V)> {
    annotated
        .iter()
        .filter(|(_, path, blamed)| match heads {
            None => true,
            Some(heads) => heads
                .iter()
                .find(|(p, _)| p == path)
                .is_some_and(|(_, head)| head.as_deref() != Some(blamed.as_str())),
        })
        .map(|(view, path, _)| (path.clone(), view.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_vcs::CommitInfo;

    fn commit(id: &str, author: &str, time: i64, message: &str) -> BlameCommit {
        BlameCommit {
            info: CommitInfo {
                id: id.repeat(40 / id.len()),
                short_id: id.repeat(7 / id.len() + 1)[..7].to_owned(),
                summary: message.lines().next().unwrap_or_default().to_owned(),
                author: author.into(),
                email: format!("{}@example.invalid", author.to_lowercase()),
                time,
                parents: Vec::new(),
                committer: None,
            },
            message: message.into(),
        }
    }

    const UTC: LocalZone = LocalZone::Fixed(0);

    fn blame() -> Arc<HeadBlame> {
        Arc::new(HeadBlame {
            path: "a.txt".into(),
            head_text: "one\ntwo\nthree\n".into(),
            lines: vec![Some(0), Some(1), Some(0)],
            commits: vec![
                commit("a", "Ann", 1_761_955_200, "first\n\nWhy it starts."),
                commit("b", "Bernhard Maximilian Schmidt", 1_762_041_600, "second"),
            ],
        })
    }

    #[test]
    fn labels_ages_width_and_cells() {
        let b = blame();
        let a = Annotations::new(b.clone(), b.lines.clone(), UTC, DateStyle::ISO);
        // 1_761_955_200 is 2025-11-01 00:00 UTC.
        let iso = &DateStyle::ISO;
        assert_eq!(date(1_761_955_200, &UTC, iso), "2025-11-01");
        assert_eq!(
            date(1_761_955_200, &LocalZone::Fixed(-3600), iso),
            "2025-10-31"
        );
        // Regional short dates like the Git log (SPEC-0277 v3 behaviour 14, OBS-0022 L18).
        let de = &DateStyle::for_locale("de_DE", None);
        assert_eq!(date(1_761_955_200, &UTC, de), "01.11.25");
        assert_eq!(
            date_time(1_761_955_200 + 3 * 3600 + 5 * 60, &UTC, iso),
            "2025-11-01 03:05"
        );
        assert_eq!(short_author("Ann"), "Ann");
        assert_eq!(
            short_author("Bernhard Maximilian Schmidt"),
            "Bernhard Maximi…"
        );
        assert_eq!(
            short_author("Bernhard Maximilian Schmidt").chars().count(),
            16
        );
        assert_eq!(
            a.width_chars(),
            "2025-11-02 Bernhard Maximi…".chars().count()
        );
        match a.cell(1) {
            Cell::Commit {
                label, age, newest, ..
            } => {
                assert_eq!(label, "2025-11-02 Bernhard Maximi…");
                assert_eq!(age, 0);
                assert!(newest);
            }
            other => panic!("{other:?}"),
        }
        match a.cell(0) {
            Cell::Commit {
                label, age, newest, ..
            } => {
                assert_eq!(label, "2025-11-01 Ann");
                assert!(age > 0 && !newest);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(a.cell(3), Cell::Empty, "the line after the last line break");
        assert_eq!(age_buckets(&[]), Vec::<u8>::new());
        let many: Vec<BlameCommit> = (0..10).map(|i| commit("c", "C", 1000 + i, "m")).collect();
        assert_eq!(age_buckets(&many), [4, 4, 3, 3, 2, 2, 1, 1, 0, 0]);
    }

    #[test]
    fn edits_keep_unchanged_lines_and_mark_changed_ones() {
        let b = blame();
        let mut a = Annotations::new(b.clone(), b.map("one\ntwo\nthree\n"), UTC, DateStyle::ISO);
        assert_eq!(a.lines(), [Some(0), Some(1), Some(0)]);
        // Insert a line, modify one, delete one.
        a.set_lines(b.map("zero\none\nTWO\n"));
        assert_eq!(a.lines(), [None, Some(0), None]);
        assert_eq!(a.cell(0), Cell::Uncommitted);
        assert!(a.commit_at(1).is_some_and(|c| c.info.author == "Ann"));
        assert!(a.commit_at(2).is_none());
        // Undo back to HEAD: every line has its commit again.
        a.set_lines(b.map("one\ntwo\nthree\n"));
        assert!((0..3).all(|l| a.commit_at(l).is_some()));
        // A last line without line break that was added is uncommitted.
        a.set_lines(b.map("one\ntwo\nthree\nfour"));
        assert_eq!(a.cell(3), Cell::Uncommitted);
    }

    #[test]
    fn hover_shows_commit_author_date_and_message() {
        let b = blame();
        let a = Annotations::new(b.clone(), b.map("one\nnew\n"), UTC, DateStyle::ISO);
        let card = a.hover(0).unwrap();
        assert_eq!(card[0], "aaaaaaa  Ann <ann@example.invalid>");
        assert_eq!(card[1], "2025-11-01 00:00");
        assert!(card.contains(&"Why it starts.".to_owned()));
        assert_eq!(
            card.last().unwrap(),
            "Click to show the commit in the Git log"
        );
        assert_eq!(a.hover(1).unwrap()[0], "Not committed yet");
        assert_eq!(a.hover(5), None);
    }

    #[test]
    fn reasons_name_the_file() {
        assert_eq!(
            reason(&GitError::NotARepository("/x".into()), "a.txt"),
            "Cannot annotate: a.txt is not in a Git repository"
        );
        assert_eq!(
            reason(&GitError::Untracked("src/a.txt".into()), "a.txt"),
            "Cannot annotate: a.txt is not tracked by Git"
        );
        assert_eq!(
            reason(&GitError::NotCommitted("a.txt".into()), "a.txt"),
            "Cannot annotate: a.txt has no committed version yet"
        );
        assert!(reason(&GitError::Git("boom".into()), "a.txt").contains("boom"));
        assert_eq!(NO_EDITOR, "Open a file to annotate it");
    }

    /// SPEC-0277 v2 behaviour 13 (AK-8): views of one file in a split are served by one read,
    /// each separately; refresh covers every annotated view.
    #[test]
    fn annotations_belong_to_editor_views() {
        let file = Path::new("/p/src/a.rs");
        let other = Path::new("/p/src/b.rs");
        let mut requests = BlameRequests::default();
        assert!(!requests.running());
        assert!(requests.ask(file, 2), "the second view starts a read");
        assert!(
            !requests.ask(file, 1),
            "the first view waits for the same read"
        );
        assert!(!requests.ask(file, 2), "asking twice changes nothing");
        assert!(requests.ask(other, 1), "another file reads on its own");
        assert!(requests.running());
        assert_eq!(requests.finish(file), [2, 1]);
        assert_eq!(requests.finish(file), Vec::<i32>::new());
        assert_eq!(requests.finish(other), [1]);
        assert!(!requests.running());
        // Both views of a.rs annotated, b.rs in the first view only.
        let annotated = vec![
            (1, file.to_path_buf(), "old".to_owned()),
            (2, file.to_path_buf(), "old".to_owned()),
            (1, other.to_path_buf(), "same".to_owned()),
        ];
        let heads = vec![
            (file.to_path_buf(), Some("new".to_owned())),
            (other.to_path_buf(), Some("same".to_owned())),
        ];
        assert_eq!(
            stale_views(&annotated, Some(&heads)),
            [(file.to_path_buf(), 1), (file.to_path_buf(), 2)]
        );
        assert_eq!(stale_views(&annotated, None).len(), 3, "after a commit");
    }
}

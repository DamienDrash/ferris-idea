//! Problem marks of files and folders (`SPEC-0059` v4 behaviour 10), without toolkit: which names on
//! the editor tabs and in the project tree get the wavy error underline.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::diagnostics_model::Severity;

/// The files that have at least one error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProblemFiles {
    files: BTreeSet<PathBuf>,
}

impl ProblemFiles {
    /// Collects the files of `problems` (path and severities) that have an error; warnings and
    /// weaker problems do not mark a file.
    pub fn from_severities<'a, I, S>(problems: I) -> Self
    where
        I: IntoIterator<Item = (&'a Path, S)>,
        S: IntoIterator<Item = Severity>,
    {
        let files = problems
            .into_iter()
            .filter_map(|(path, severities)| {
                let mut severities = severities.into_iter();
                severities
                    .any(|severity| severity == Severity::Error)
                    .then(|| path.to_path_buf())
            })
            .collect();
        Self { files }
    }

    /// Whether no file is marked.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether the file itself has an error.
    pub fn file(&self, path: &Path) -> bool {
        self.files.contains(path)
    }

    /// Whether a file below the folder `dir` has an error.
    pub fn folder(&self, dir: &Path) -> bool {
        // A node without a folder on disk ("External Libraries") is above nothing (OBS-0027).
        if dir.as_os_str().is_empty() {
            return false;
        }
        self.files
            .range(dir.to_path_buf()..)
            .take_while(|f| f.starts_with(dir))
            .any(|f| f != dir)
    }

    /// Whether a tree row for `path` is marked: a file with an error, or a folder above one.
    pub fn marked(&self, path: &Path, is_dir: bool) -> bool {
        if is_dir {
            self.folder(path)
        } else {
            self.file(path)
        }
    }
}

/// Whether an LSP severity name (`SPEC-0023`) is an error.
pub fn lsp_error(severity: &str) -> bool {
    severity == "error"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn only_errors_mark_a_file() {
        let a = p("/w/src/a.java");
        let b = p("/w/src/b.java");
        let c = p("/w/c.py");
        let marks = ProblemFiles::from_severities([
            (a.as_path(), vec![Severity::Warning, Severity::Error]),
            (b.as_path(), vec![Severity::Warning, Severity::Weak]),
            (c.as_path(), vec![]),
        ]);
        assert!(marks.file(&a));
        assert!(!marks.file(&b), "warnings do not mark");
        assert!(!marks.file(&c));
        assert!(lsp_error("error") && !lsp_error("warning") && !lsp_error("hint"));
    }

    #[test]
    fn folders_above_an_error_are_marked_up_to_the_root() {
        let a = p("/w/sprachen/test.java");
        let marks = ProblemFiles::from_severities([(a.as_path(), [Severity::Error])]);
        assert!(marks.marked(&p("/w/sprachen"), true));
        assert!(marks.marked(&p("/w"), true), "the root shows it too");
        assert!(
            !marks.marked(&p("/w/sprachen2"), true),
            "a sibling prefix is not a parent"
        );
        assert!(!marks.marked(&p("/w/other"), true));
        assert!(
            !marks.marked(&a, true),
            "a file path is not a folder above itself"
        );
        assert!(marks.marked(&a, false));
    }

    /// OBS-0027: the tree's nodes without a path are never marked.
    #[test]
    fn a_node_without_a_path_is_not_marked() {
        let marks =
            ProblemFiles::from_severities([(p("/w/src/A.java").as_path(), vec![Severity::Error])]);
        assert!(marks.marked(&p("/w/src"), true));
        assert!(!marks.marked(Path::new(""), true));
    }

    #[test]
    fn fixing_the_error_removes_every_mark() {
        let a = p("/w/sprachen/test.java");
        let before = ProblemFiles::from_severities([(a.as_path(), [Severity::Error])]);
        assert!(!before.is_empty());
        let after = ProblemFiles::from_severities([(a.as_path(), Vec::<Severity>::new())]);
        assert!(after.is_empty());
        assert!(!after.marked(&p("/w/sprachen"), true));
        assert!(!after.marked(&a, false));
    }
}

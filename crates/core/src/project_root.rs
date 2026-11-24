//! The project a path opens (`SPEC-0094` v7 behaviour 18): a folder is its own project; a file
//! opens the project it belongs to, never a project whose root is the file.

use std::io;
use std::path::{Path, PathBuf};

/// Folders that mark a folder opened as a project before (step 1).
const PROJECT_MARKERS: [&str; 2] = [".ferris-idea", ".idea"];

/// Version control roots (step 2): `.git` may be a folder or, in a worktree or submodule, a file.
const VCS_MARKERS: [&str; 3] = [".git", ".hg", ".svn"];

/// Build files (step 3).
const BUILD_FILES: [&str; 17] = [
    "Cargo.toml",
    "package.json",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "build.sbt",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "Gemfile",
    "composer.json",
    "Package.swift",
    "CMakeLists.txt",
    "mix.exs",
    "deno.json",
];

/// What opening a path opens: the project folder and, for a file, the file to show in an editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTarget {
    /// The project folder (canonical).
    pub root: PathBuf,
    /// The file to open in an editor, when the path was a file (canonical).
    pub file: Option<PathBuf>,
}

/// The project and file `path` opens, with the user's home folder as the limit of the walk.
pub fn open_target(path: &Path) -> io::Result<OpenTarget> {
    open_target_below(path, home_folder().as_deref())
}

/// As [`open_target`], with `home` as the user's home folder (tests).
pub fn open_target_below(path: &Path, home: Option<&Path>) -> io::Result<OpenTarget> {
    let canonical = path.canonicalize()?;
    if canonical.is_dir() {
        return Ok(OpenTarget {
            root: canonical,
            file: None,
        });
    }
    let root = project_root_of_file(&canonical, home);
    Ok(OpenTarget {
        root,
        file: Some(canonical),
    })
}

/// The project of the canonical `file` (behaviour 18): the nearest folder opened as a project,
/// else the nearest version control root, else the nearest folder with a build file, else the
/// file's own folder. `home` and the file system root are passed over by the first three steps.
pub fn project_root_of_file(file: &Path, home: Option<&Path>) -> PathBuf {
    let own = file.parent().unwrap_or(file).to_path_buf();
    let home = home.and_then(|h| h.canonicalize().ok());
    let candidates: Vec<&Path> = file
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.parent().is_some() && Some(*dir) != home.as_deref())
        .collect();
    let has = |dir: &Path, names: &[&str], need_dir: bool| {
        names.iter().any(|n| {
            let p = dir.join(n);
            if need_dir { p.is_dir() } else { p.exists() }
        })
    };
    candidates
        .iter()
        .find(|d| has(d, &PROJECT_MARKERS, true))
        .or_else(|| candidates.iter().find(|d| has(d, &VCS_MARKERS, false)))
        .or_else(|| candidates.iter().find(|d| has(d, &BUILD_FILES, false)))
        .map_or(own, |d| d.to_path_buf())
}

fn home_folder() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The message for a command that takes a folder and was given the file `path` (`SPEC-0094` v7
/// behaviour 20): names the project folder to pass instead.
pub fn file_given_for_folder(path: &Path) -> String {
    match open_target(path) {
        Ok(OpenTarget {
            root,
            file: Some(_),
        }) => format!(
            "{} is a file, not a project folder; its project is {} — pass that folder",
            path.display(),
            root.display()
        ),
        _ => format!("{} is not a project folder", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "ferris-idea-project-root-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p.canonicalize().unwrap())
        }
        fn file(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "").unwrap();
            p
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn target(path: &Path, home: &Path) -> OpenTarget {
        open_target_below(path, Some(home)).unwrap()
    }

    /// SPEC-0094 v7 AK-16: a folder is its own project, without a file to open.
    #[test]
    fn a_folder_is_its_own_project() {
        let d = Dir::new("folder");
        let folder = d.dir("home/p/queries");
        let t = target(&folder, &d.0.join("home"));
        assert_eq!(t.root, folder);
        assert_eq!(t.file, None);
    }

    /// SPEC-0094 v7 AK-16: the window check of 2026-11-15 — a file in a folder without markers
    /// opens that folder, and the file; never the file as the root.
    #[test]
    fn a_file_in_a_plain_folder_opens_its_folder() {
        let d = Dir::new("plain");
        let file = d.file("home/p/queries/file.sql");
        let t = target(&file, &d.0.join("home"));
        assert_eq!(t.root, d.0.join("home/p/queries"));
        assert_eq!(t.file, Some(file));
    }

    /// SPEC-0094 v7 AK-16: a file in a crate of a Cargo workspace inside a Git repository opens
    /// the repository, not the crate.
    #[test]
    fn the_repository_wins_over_a_nearer_build_file() {
        let d = Dir::new("repo");
        d.dir("home/repo/.git");
        d.file("home/repo/Cargo.toml");
        d.file("home/repo/crates/app/Cargo.toml");
        let file = d.file("home/repo/crates/app/src/main.rs");
        assert_eq!(target(&file, &d.0.join("home")).root, d.0.join("home/repo"));
        // A worktree's `.git` is a file.
        d.file("home/wt/.git");
        let file = d.file("home/wt/src/lib.rs");
        assert_eq!(target(&file, &d.0.join("home")).root, d.0.join("home/wt"));
    }

    /// SPEC-0094 v7 AK-16: a folder opened as a project wins over a nearer Git root and a nearer
    /// build file.
    #[test]
    fn a_project_folder_wins_over_version_control_and_build_files() {
        let d = Dir::new("marked");
        d.dir("home/mono/.idea");
        d.dir("home/mono/sub/.git");
        d.file("home/mono/sub/pkg/package.json");
        let file = d.file("home/mono/sub/pkg/index.js");
        assert_eq!(target(&file, &d.0.join("home")).root, d.0.join("home/mono"));
        d.dir("home/other/.ferris-idea");
        d.file("home/other/deep/go.mod");
        let file = d.file("home/other/deep/main.go");
        assert_eq!(
            target(&file, &d.0.join("home")).root,
            d.0.join("home/other")
        );
    }

    /// SPEC-0094 v7 AK-16: without version control the nearest build file decides.
    #[test]
    fn a_build_file_decides_without_version_control() {
        let d = Dir::new("build");
        d.file("home/svc/pyproject.toml");
        let file = d.file("home/svc/src/pkg/mod.py");
        assert_eq!(target(&file, &d.0.join("home")).root, d.0.join("home/svc"));
    }

    /// SPEC-0094 v7 AK-16: the home folder is passed over — a dotfiles repository there does not
    /// make the whole home folder the project; a file directly in it still opens it.
    #[test]
    fn the_home_folder_is_passed_over() {
        let d = Dir::new("home");
        d.dir("home/.git");
        d.dir("home/.idea");
        let file = d.file("home/notes/todo.md");
        assert_eq!(
            target(&file, &d.0.join("home")).root,
            d.0.join("home/notes")
        );
        let file = d.file("home/scratch.sql");
        assert_eq!(target(&file, &d.0.join("home")).root, d.0.join("home"));
        // Nothing above the home folder is looked at either.
        d.dir(".git");
        let file = d.file("home/a/b.txt");
        assert_eq!(target(&file, &d.0.join("home")).root, d.0.join("home/a"));
    }

    /// SPEC-0094 v7 AK-16: a link to a file opens the project of the file it points to.
    #[cfg(unix)]
    #[test]
    fn a_link_opens_the_project_of_its_target() {
        let d = Dir::new("link");
        d.dir("home/real/.git");
        let file = d.file("home/real/q/file.sql");
        let link = d.dir("home/links").join("file.sql");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let t = target(&link, &d.0.join("home"));
        assert_eq!(t.root, d.0.join("home/real"));
        assert_eq!(t.file, Some(file));
    }

    /// SPEC-0094 v7 AK-17: the refusal for commands that take a folder names the project.
    #[test]
    fn the_refusal_names_the_project() {
        let d = Dir::new("refusal");
        let file = d.file("q/file.sql");
        let message = file_given_for_folder(&file);
        assert!(message.contains("is a file"), "{message}");
        assert!(
            message.contains(&d.0.join("q").display().to_string()),
            "{message}"
        );
    }
}

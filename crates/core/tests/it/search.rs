//! SPEC-0029/AK-4, AK-6 and AK-7: find in files on a real directory.

use std::path::PathBuf;

use std::path::Path;

use idecraft_core::{
    NotOwn, OwnSources, Query, SearchPlace, SearchScope, Workspace, not_replaceable, search_scope,
    search_workspace,
};

#[test]
fn find_in_files_with_mask_exclusions_and_limits() {
    let root = std::env::temp_dir().join(format!("idecraft-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["src", "target", ".git"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("src/b.rs"), "fn todo() {}\n// TODO later\n").unwrap();
    std::fs::write(root.join("src/a.rs"), "// todo: one\n").unwrap();
    std::fs::write(root.join("notes.md"), "todo in markdown\n").unwrap();
    std::fs::write(root.join("target/out.rs"), "todo excluded\n").unwrap();
    std::fs::write(root.join(".git/HEAD.rs"), "todo excluded\n").unwrap();
    std::fs::write(root.join("src/blob.rs"), b"todo\0binary").unwrap();
    let big = vec![b'a'; 10 * 1024 * 1024 + 1];
    std::fs::write(root.join("src/big.rs"), big).unwrap();
    let ws = Workspace::open(&root).unwrap();
    let query = Query {
        pattern: "todo".into(),
        ..Query::default()
    };

    let all = search_workspace(&ws, &query, None, 1000).unwrap();
    let paths: Vec<PathBuf> = all.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(
        paths,
        [
            PathBuf::from("notes.md"),
            PathBuf::from("src/a.rs"),
            PathBuf::from("src/b.rs")
        ]
    );
    assert_eq!(all.skipped, 2, "binary and oversize");
    assert!(!all.truncated);
    assert_eq!(all.files[2].matches.len(), 2);
    assert_eq!(all.files[2].matches[1].line, 1);

    let rust = search_workspace(&ws, &query, Some("*.rs"), 1000).unwrap();
    assert_eq!(rust.files.len(), 2);
    let limited = search_workspace(&ws, &query, Some("*.rs"), 2).unwrap();
    assert!(limited.truncated);
    assert_eq!(
        limited.files.iter().map(|f| f.matches.len()).sum::<usize>(),
        2
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A project with an own file, a virtual environment, installed dependencies, an ignored file,
/// version control data and IDECraft's data; every file holds `needle`.
fn mixed_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-search-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (file, text) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".git/needle.txt", "needle\n"),
        (".idecraft/history/1", "needle\n"),
        (".gitignore", "*.log\n"),
        ("src/own.py", "needle = 1\n"),
        ("src/notes.log", "needle\n"),
        (".venv/pyvenv.cfg", "home = /usr/bin\n"),
        (".venv/lib/python3.12/site-packages/x.py", "needle = 2\n"),
        ("node_modules/y.js", "const needle = 3;\n"),
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

fn paths(search: &idecraft_core::WorkspaceSearch) -> Vec<String> {
    search
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

// SPEC-0029/AK-6 (v2).
#[test]
fn find_in_files_searches_only_the_own_sources() {
    let root = mixed_project("own");
    let ws = Workspace::open(&root).unwrap();
    let query = Query {
        pattern: "needle".into(),
        ..Query::default()
    };
    let search = |scope: SearchScope| search_scope(&ws, &query, &scope, 1000).unwrap();

    // By default only the project's own file, and the result says what was left out.
    let default = search_workspace(&ws, &query, None, 1000).unwrap();
    assert_eq!(paths(&default), ["src/own.py"]);
    let left: Vec<(String, bool, NotOwn)> = default
        .not_searched
        .iter()
        .map(|o| (o.path.to_string_lossy().replace('\\', "/"), o.folder, o.why))
        .collect();
    assert_eq!(
        left,
        [
            (".venv".to_owned(), true, NotOwn::VirtualEnvironment),
            ("node_modules".to_owned(), true, NotOwn::Dependencies),
            ("src/notes.log".to_owned(), false, NotOwn::Ignored),
        ]
    );
    assert_eq!(default.not_searched_counts(), (2, 1));
    assert_eq!(
        default.not_searched_line().as_deref(),
        Some("2 folders and 1 file not searched (libraries and ignored files)")
    );
    // A file the mask does not match is not named as left out.
    let masked = search(SearchScope {
        mask: Some("*.py".into()),
        ..SearchScope::default()
    });
    assert_eq!(masked.not_searched_counts(), (2, 0));

    // With the option: everything but version control and IDECraft's data.
    let everything = search(SearchScope {
        include_non_project: true,
        ..SearchScope::default()
    });
    assert_eq!(
        paths(&everything),
        [
            ".venv/lib/python3.12/site-packages/x.py",
            "node_modules/y.js",
            "src/notes.log",
            "src/own.py",
        ]
    );
    assert!(everything.not_searched.is_empty());
    assert_eq!(everything.not_searched_line(), None);

    // A named directory that is not the project's own is searched without the option …
    let directory = |dir: &str, include_non_project: bool| {
        search(SearchScope {
            place: SearchPlace::Directory(dir.into()),
            include_non_project,
            ..SearchScope::default()
        })
    };
    let venv = directory(".venv/lib", false);
    assert_eq!(paths(&venv), [".venv/lib/python3.12/site-packages/x.py"]);
    assert!(venv.not_searched.is_empty());
    assert_eq!(
        paths(&directory("./node_modules/", false)),
        ["node_modules/y.js"]
    );
    // … a single file too …
    assert_eq!(paths(&directory("src/notes.log", false)), ["src/notes.log"]);
    // … and one that is the project's own by the rule, unless the option is set.
    let src = directory("src", false);
    assert_eq!(paths(&src), ["src/own.py"]);
    assert_eq!(src.not_searched_counts(), (0, 1));
    assert_eq!(
        paths(&directory("src", true)),
        ["src/notes.log", "src/own.py"]
    );
    assert_eq!(paths(&directory("", false)), ["src/own.py"]);
    // Nothing outside the project folder, nothing that is missing.
    for outside in ["..", "../elsewhere", "/etc", "missing"] {
        assert!(directory(outside, true).files.is_empty(), "{outside}");
    }

    // A list of files is searched whatever the files are; what does not exist is dropped.
    let files = search(SearchScope {
        place: SearchPlace::Files(vec![
            "node_modules/y.js".into(),
            "src/own.py".into(),
            "missing.py".into(),
            "../elsewhere.py".into(),
        ]),
        ..SearchScope::default()
    });
    assert_eq!(paths(&files), ["node_modules/y.js", "src/own.py"]);
    let _ = std::fs::remove_dir_all(&root);
}

// SPEC-0029/AK-6 (v2): symbolic links are followed by no search.
#[cfg(unix)]
#[test]
fn find_in_files_follows_no_symbolic_link() {
    let root = mixed_project("links");
    let other = mixed_project("links-target");
    std::os::unix::fs::symlink(other.join("src"), root.join("linked")).unwrap();
    std::os::unix::fs::symlink(other.join("src/own.py"), root.join("src/link.py")).unwrap();
    let ws = Workspace::open(&root).unwrap();
    let query = Query {
        pattern: "needle".into(),
        ..Query::default()
    };
    for (place, include_non_project) in [
        (SearchPlace::Project, false),
        (SearchPlace::Project, true),
        (SearchPlace::Directory("linked".into()), true),
        (SearchPlace::Files(vec!["src/link.py".into()]), true),
    ] {
        let scope = SearchScope {
            place,
            include_non_project,
            ..SearchScope::default()
        };
        let found = search_scope(&ws, &query, &scope, 1000).unwrap();
        assert!(
            !paths(&found)
                .iter()
                .any(|p| p.starts_with("linked") || p == "src/link.py"),
            "{scope:?}: {:?}",
            paths(&found)
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}

// SPEC-0029/AK-7 (v2).
#[test]
fn replacing_stays_in_the_own_sources() {
    let root = mixed_project("replace");
    let own = OwnSources::new(&root);
    let check = |path: &str, option: bool| not_replaceable(&own, Path::new(path), option);
    assert_eq!(check("src/own.py", false), None);
    assert_eq!(
        check(".venv/lib/python3.12/site-packages/x.py", false),
        Some(NotOwn::VirtualEnvironment)
    );
    assert_eq!(
        check("node_modules/y.js", false),
        Some(NotOwn::Dependencies)
    );
    assert_eq!(check("src/notes.log", false), Some(NotOwn::Ignored));
    // The option allows libraries and ignored files …
    assert_eq!(check(".venv/lib/python3.12/site-packages/x.py", true), None);
    assert_eq!(check("node_modules/y.js", true), None);
    assert_eq!(check("src/notes.log", true), None);
    // … never version control data, IDECraft's data or a file outside the project.
    assert_eq!(check(".git/needle.txt", true), Some(NotOwn::VersionControl));
    assert_eq!(check(".idecraft/history/1", true), Some(NotOwn::IdeData));
    assert_eq!(
        check("node_modules/dep/.git/config", true),
        Some(NotOwn::Dependencies)
    );
    assert_eq!(check("../elsewhere.py", true), Some(NotOwn::OutsideProject));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("src/own.py"), root.join("src/link.py")).unwrap();
        assert_eq!(check("src/link.py", true), Some(NotOwn::SymbolicLink));
    }
    let _ = std::fs::remove_dir_all(&root);
}

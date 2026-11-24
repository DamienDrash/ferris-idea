//! SPEC-0094 AK-1: the recent projects list.

use ferris_idea_core::{RECENT_PROJECTS_LIMIT, load_recent_projects, remember_project};

#[test]
fn most_recent_first_without_duplicates_or_missing_folders() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-recent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The list names folders by their canonical path (v8); the temporary folder is a link on macOS.
    let dir = std::fs::canonicalize(&dir).unwrap();
    let projects: Vec<_> = (0..RECENT_PROJECTS_LIMIT + 2)
        .map(|i| dir.join(format!("p{i}")))
        .collect();
    for p in &projects {
        std::fs::create_dir_all(p).unwrap();
    }
    let file = dir.join("config/recent-projects");
    assert!(
        load_recent_projects(&file).is_empty(),
        "no file, no projects"
    );
    remember_project(&file, &projects[0]).unwrap();
    remember_project(&file, &projects[1]).unwrap();
    remember_project(&file, &projects[0]).unwrap();
    assert_eq!(
        load_recent_projects(&file),
        [projects[0].clone(), projects[1].clone()]
    );
    std::fs::remove_dir_all(&projects[1]).unwrap();
    assert_eq!(
        load_recent_projects(&file),
        [projects[0].clone()],
        "gone folders are left out"
    );
    for p in &projects[2..] {
        remember_project(&file, p).unwrap();
    }
    let list = load_recent_projects(&file);
    assert_eq!(list.len(), RECENT_PROJECTS_LIMIT);
    assert_eq!(list[0], *projects.last().unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0094 v8 AK-19: different spellings of one folder are one entry, by its canonical path, at
/// the place of the most recent; a link to a folder counts as that folder.
#[test]
fn spellings_of_one_folder_are_one_entry() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-recent-spell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (a, b) = (dir.join("a"), dir.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let file = dir.join("recent-projects");
    let (ca, cb) = (
        std::fs::canonicalize(&a).unwrap(),
        std::fs::canonicalize(&b).unwrap(),
    );
    std::fs::write(
        &file,
        format!(
            "{}/\n{}\n{}/../a\n{}\n",
            a.display(),
            b.display(),
            b.display(),
            ca.display()
        ),
    )
    .unwrap();
    assert_eq!(load_recent_projects(&file), [ca.clone(), cb.clone()]);
    #[cfg(unix)]
    {
        let link = dir.join("link-to-b");
        std::os::unix::fs::symlink(&b, &link).unwrap();
        remember_project(&file, &link).unwrap();
        assert_eq!(load_recent_projects(&file), [cb.clone(), ca.clone()]);
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, format!("{}\n{}\n", cb.display(), ca.display()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0094 v10 AK-21, SPEC-0355 AK-2: a folder that is gone stays in the file and in the
/// Welcome window's entries, marked, until it is removed; the other readers leave it out.
#[test]
fn missing_folders_stay_until_removed() {
    use ferris_idea_core::{RecentEntry, forget_project, load_recent_entries};
    let dir = std::env::temp_dir().join(format!("ferris-idea-recent-gone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let (a, gone, c) = (dir.join("a"), dir.join("gone"), dir.join("c"));
    for p in [&a, &gone, &c] {
        std::fs::create_dir_all(p).unwrap();
    }
    let file = dir.join("recent-projects");
    remember_project(&file, &a).unwrap();
    remember_project(&file, &gone).unwrap();
    std::fs::remove_dir_all(&gone).unwrap();
    // Remembering another project keeps the missing one.
    remember_project(&file, &c).unwrap();
    assert_eq!(load_recent_projects(&file), [c.clone(), a.clone()]);
    assert_eq!(
        load_recent_entries(&file),
        [
            RecentEntry {
                path: c.clone(),
                exists: true
            },
            RecentEntry {
                path: gone.clone(),
                exists: false
            },
            RecentEntry {
                path: a.clone(),
                exists: true
            },
        ]
    );
    // Removed by another spelling of the same line; the folder `a` stays where it is.
    assert!(forget_project(&file, &dir.join("gone/")).unwrap());
    assert!(!forget_project(&file, &gone).unwrap(), "only once");
    assert!(forget_project(&file, &dir.join("c/../a")).unwrap());
    assert!(a.is_dir(), "removing never touches the folder");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("{}\n", c.display())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

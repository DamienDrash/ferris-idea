//! SPEC-0094 AK-1: the recent projects list.

use idecraft_core::{RECENT_PROJECTS_LIMIT, load_recent_projects, remember_project};

#[test]
fn most_recent_first_without_duplicates_or_missing_folders() {
    let dir = std::env::temp_dir().join(format!("idecraft-recent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

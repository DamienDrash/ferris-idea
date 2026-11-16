//! SPEC-0026/AK-2: the watcher on a real directory.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use idecraft_core::{FileChange, Workspace, watch};

/// Collects batches until `want` shows up (or two seconds pass).
fn wait_for(rx: &Receiver<Vec<FileChange>>, want: &FileChange) -> Vec<FileChange> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.extend(batch);
            if seen.contains(want) {
                return seen;
            }
        }
    }
    panic!("{want:?} not reported; saw {seen:?}");
}

#[test]
fn watcher_reports_create_modify_rename_delete() {
    let root = std::env::temp_dir().join(format!("idecraft-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    let ws = Workspace::open(&root).unwrap();
    let (handle, rx) = watch(ws, Duration::from_millis(50)).unwrap();

    std::fs::write(root.join("src/a.txt"), "one").unwrap();
    wait_for(&rx, &FileChange::Created(PathBuf::from("src/a.txt")));

    std::fs::write(root.join("target/ignored.o"), "x").unwrap();
    std::fs::write(root.join("src/a.txt"), "one and two").unwrap();
    let seen = wait_for(&rx, &FileChange::Modified(PathBuf::from("src/a.txt")));
    assert!(
        seen.iter().all(|c| !c.path().starts_with("target")),
        "excluded directory: {seen:?}"
    );

    std::fs::rename(root.join("src/a.txt"), root.join("b.txt")).unwrap();
    wait_for(
        &rx,
        &FileChange::Renamed {
            from: PathBuf::from("src/a.txt"),
            to: PathBuf::from("b.txt"),
        },
    );

    std::fs::remove_file(root.join("b.txt")).unwrap();
    wait_for(&rx, &FileChange::Deleted(PathBuf::from("b.txt")));

    drop(handle);
    std::thread::sleep(Duration::from_millis(200));
    while rx.try_recv().is_ok() {}
    std::fs::write(root.join("late.txt"), "x").unwrap();
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "stopped after the handle was dropped"
    );
    let _ = std::fs::remove_dir_all(&root);
}

//! `SPEC-0108` v13 AK-25 (OBS-0083 L8), in a window of the test harness: the connection dialog
//! offers Create File for SQLite and DuckDB; Test Connection on a missing file creates nothing;
//! Create File makes the empty database once and refuses an existing file.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::{Dialect, WorkbenchView, semantics};
use gpui::{TestAppContext, VisualTestContext, px};

#[gpui::test]
fn create_file_makes_the_database_only_when_asked(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("ferris-idea-create-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(885.)));
    bench.update_in(cx, |b, window, cx| {
        b.open_connection_dialog(None, window, cx)
    });
    cx.run_until_parked();
    let dialog = bench
        .read_with(cx, |b, _| b.connection_dialog())
        .expect("the dialog is open");
    let shown = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, _| {
            semantics::tree(semantics::window_id(window))
                .is_some_and(|t| t.iter().any(|n| n.test_id() == Some("conn-create-file")))
        })
    };
    assert!(!shown(cx), "not for a server");
    dialog.update(cx, |d, cx| {
        d.set_dialect(Dialect::Sqlite, cx);
        d.set_field("Name", "notes", cx);
        d.set_field("File", "notes.db", cx);
    });
    assert!(shown(cx), "offered for SQLite");
    let file = root.join("notes.db");
    // Testing a missing file fails and creates nothing.
    dialog.update(cx, |d, cx| d.test(cx));
    for _ in 0..50 {
        cx.run_until_parked();
        if dialog.read_with(cx, |d, _| d.note().is_some_and(|(n, _)| n != "Connecting…")) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        dialog.read_with(cx, |d, _| d.note().is_some_and(|(_, error)| *error)),
        "{:?}",
        dialog.read_with(cx, |d, _| d.note().cloned())
    );
    assert!(!file.exists(), "Test Connection created the file");
    let wait = |cx: &mut VisualTestContext| {
        for _ in 0..50 {
            cx.run_until_parked();
            if dialog.read_with(cx, |d, _| {
                d.note().is_some_and(|(n, _)| n != "Creating the file…")
            }) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        dialog.read_with(cx, |d, _| d.note().cloned()).unwrap()
    };
    dialog.update(cx, |d, cx| d.create_file(cx));
    let (note, error) = wait(cx);
    assert!(!error && note.starts_with("Created "), "{note}");
    assert!(file.is_file());
    // A second click never touches the file.
    std::fs::write(&file, b"kept").unwrap();
    dialog.update(cx, |d, cx| d.create_file(cx));
    let (note, error) = wait(cx);
    assert!(error && note.contains("already exists"), "{note}");
    assert_eq!(std::fs::read(&file).unwrap(), b"kept");
    // DuckDB too.
    dialog.update(cx, |d, cx| {
        d.set_dialect(Dialect::DuckDb, cx);
        d.set_field("File", "facts.duckdb", cx);
    });
    assert!(shown(cx), "offered for DuckDB");
    dialog.update(cx, |d, cx| d.create_file(cx));
    let (note, error) = wait(cx);
    assert!(!error, "{note}");
    assert!(root.join("facts.duckdb").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

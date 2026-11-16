//! `SPEC-0089` v10.1 behaviour 22, headless on SQLite: after saving or removing a connection the
//! tree selects the kept row again once it is loaded, unless a row was chosen meanwhile. Found by
//! the window self-test of 2026-11-14 15:15: the second Delete on a just saved connection removed
//! nothing, because the kept selection came back while the question was open.

#![cfg(target_os = "macos")]

use gpui::{Entity, TestAppContext, VisualTestContext};
use idecraft_workbench::WorkbenchView;

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// A workspace with the SQLite connections `shop` (two tables) and `notes`. `None` without the
/// `sqlite3` program.
fn workspace() -> Option<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("idecraft-db-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let (shop, notes) = (root.join("shop.db"), root.join("notes.db"));
    let made = sqlite(
        &shop,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY); CREATE TABLE orders (id INTEGER);",
    ) && sqlite(&notes, "CREATE TABLE note (id INTEGER);");
    if !made {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".idecraft/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n\n\
             [[connection]]\nid = \"notes\"\nname = \"notes\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            shop.display(),
            notes.display()
        ),
    )
    .unwrap();
    Some(root)
}

/// Runs the background reads until `done` holds; panics after five seconds.
fn wait(
    view: &Entity<idecraft_workbench::DatabaseView>,
    cx: &mut VisualTestContext,
    what: &str,
    done: impl Fn(&idecraft_workbench::DatabaseView) -> bool,
) {
    for _ in 0..200 {
        cx.run_until_parked();
        if view.read_with(cx, |v, _| done(v)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{what}");
}

#[gpui::test]
fn a_row_chosen_after_a_reload_stays_chosen(cx: &mut TestAppContext) {
    let Some(root) = workspace() else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    let view = bench
        .read_with(cx, |b, _| b.database_view())
        .expect("the Database window is open");
    let row = |v: &idecraft_workbench::DatabaseView, name: &str| {
        v.visible().iter().position(|(_, n, _)| n == name)
    };
    // A table of shop is selected: its row is back only once shop has been read again.
    view.update(cx, |v, cx| {
        v.expand(idecraft_workbench::db_tree_model::Node::Conn(0), cx)
    });
    wait(&view, cx, "shop is read", |v| row(v, "tables").is_some());
    view.update(cx, |v, cx| v.expand_all_tables(cx));
    wait(&view, cx, "shop lists its tables", |v| {
        row(v, "customers").is_some()
    });
    view.update(cx, |v, cx| {
        let at = row(v, "customers").expect("customers is listed");
        v.select(at, cx);
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.selected_table()),
        Some("customers".into())
    );

    // Saving a connection reads the file again; before shop is loaded, notes is chosen.
    view.update(cx, |v, cx| {
        v.reload_keeping(cx);
        let at = row(v, "notes").expect("notes is listed");
        v.select(at, cx);
    });
    wait(&view, cx, "shop is read again", |v| {
        row(v, "customers").is_some()
    });
    let chosen = view.read_with(cx, |v, _| v.selected_profile().map(|p| p.id));
    assert_eq!(
        chosen.as_deref(),
        Some("notes"),
        "the row chosen last stays"
    );

    // Without a choice in between the kept row is selected again (behaviour 22).
    view.update(cx, |v, cx| {
        let at = row(v, "customers").expect("customers is listed");
        v.select(at, cx);
        v.reload_keeping(cx);
    });
    wait(&view, cx, "shop is read again", |v| {
        row(v, "customers").is_some()
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.selected_table()),
        Some("customers".into())
    );
    let _ = std::fs::remove_dir_all(&root);
}

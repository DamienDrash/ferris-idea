//! `SPEC-0089` v11 behaviour 23, `SPEC-0192` v4 behaviour 11 (OBS-0051 L7, re-check of PR #780),
//! headless on SQLite: the way of the re-check. An open `.sql` file names a table that does not
//! exist; the table is created; the Refresh button of the Database window's toolbar clears the
//! unknown-table mark without an edit, and an edit afterwards does not bring it back. Until v11
//! the button refreshed the tree only and the editors kept the old schema.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext};

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// A workspace with the one SQLite connection `shop` and `q.sql`; `None` without `sqlite3`.
fn workspace() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let root = std::env::temp_dir().join(format!("ferris-idea-sql-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let shop = root.join("shop.db");
    if !sqlite(&shop, "CREATE TABLE item (id INTEGER PRIMARY KEY);") {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            shop.display()
        ),
    )
    .unwrap();
    std::fs::write(root.join("q.sql"), "select * from fresh;\n").unwrap();
    Some((root, shop))
}

fn unknown(editor: &Entity<EditorView>, cx: &mut VisualTestContext, table: &str) -> bool {
    let message = format!("unknown table `{table}`");
    editor.read_with(cx, |e, _| {
        e.problems().iter().any(|p| p.message.contains(&message))
    })
}

/// Runs timers and background reads until `done` holds; panics after five seconds.
fn wait(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..200 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{what}");
}

#[gpui::test]
fn the_toolbar_refresh_makes_a_new_table_known(cx: &mut TestAppContext) {
    let Some((root, shop)) = workspace() else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let file = root.join("q.sql");
    bench.update_in(cx, |b, window, cx| b.open(&file, window, cx));
    let editor = bench
        .read_with(cx, |b, cx| b.editor_for(&file, cx))
        .expect("q.sql is open");
    wait(cx, "`fresh` is marked unknown", |cx| {
        unknown(&editor, cx, "fresh")
    });

    // The table is created outside the editor; the tree's Refresh button is pressed.
    assert!(sqlite(&shop, "CREATE TABLE fresh (id INTEGER);"));
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    let view = bench
        .read_with(cx, |b, _| b.database_view())
        .expect("the Database window is open");
    view.update(cx, |v, cx| v.refresh_from_toolbar(cx));
    wait(cx, "the mark goes without an edit", |cx| {
        !unknown(&editor, cx, "fresh")
    });

    // An edit afterwards is inspected with the new schema: `gone` is marked, `fresh` stays known.
    editor.update(cx, |e, cx| {
        e.replace_text("select * from fresh;\nselect * from gone;\n", cx);
    });
    wait(cx, "the edit is inspected", |cx| {
        unknown(&editor, cx, "gone")
    });
    assert!(
        !unknown(&editor, cx, "fresh"),
        "an edit after Refresh keeps the new table known"
    );
    let _ = std::fs::remove_dir_all(&root);
}

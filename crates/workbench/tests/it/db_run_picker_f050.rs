//! F-050 gap G1 of the window check (OBS-0070), in the harness's window on SQLite: the Database
//! window's tree has no selected row until the user selects one (`SPEC-0192` v7 behaviour 3b), so
//! ⌘⏎ in an unbound file opens the data-source popup also while the Database window is open. Before,
//! the tree's first row counted as selected from the start and the file ran on that connection.

#![cfg(target_os = "macos")]

use std::path::Path;

use gpui::{Entity, TestAppContext, VisualTestContext};
use idecraft_workbench::{DatabaseView, WorkbenchView};

use crate::db_refactor_retest::{has_sqlite, until, window, workspace};

/// A workspace with the connections `duck` (DuckDB, listed first) and `shop` (SQLite), and the
/// files `files`.
fn two_connections(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root = workspace(tag, "create table item (id integer primary key);", files);
    std::fs::write(
        root.join(".idecraft/connections.toml"),
        format!(
            "[[connection]]\nid = \"duck\"\nname = \"duck\"\ndialect = \"duckdb\"\npath = \"{}\"\n\n\
             [[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            root.join("other.duckdb").display(),
            root.join("shop.db").display()
        ),
    )
    .unwrap();
    root
}

/// Opens the Database window and returns its view.
fn open_database(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> Entity<DatabaseView> {
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.database_view())
        .expect("the Database window is open")
}

fn selected_id(db: &Entity<DatabaseView>, cx: &mut VisualTestContext) -> Option<String> {
    db.read_with(cx, |v, _| v.selected_profile().map(|p| p.id))
}

/// Selects all of `file`'s text, runs it with ⌘⏎ (`execute_sql`) and lets the window settle.
fn run(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, cx| b.editor_for(file, cx))
        .expect("the file is open");
    let len = std::fs::read_to_string(file).unwrap().trim_end().len();
    editor.update(cx, |e, cx| e.select_range(0..len, cx));
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        assert!(b.execute_sql(window, cx), "the .sql file runs");
    });
    cx.run_until_parked();
}

fn bound(root: &Path, file: &Path) -> Option<String> {
    let profiles = idecraft_database::load_profiles(root).unwrap();
    idecraft_database::bound_connection(root, file, &profiles)
}

/// The shown result's first cell, once a run is over.
fn first_cell(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| {
        b.services_view().and_then(|v| {
            v.read(cx)
                .shown_result()
                .and_then(|r| r.rows.first().and_then(|r| r[0].clone()))
        })
    })
}

/// The model side of G1: an untouched tree names no connection and no target; ↓ selects the
/// first row, a click (here `select_label`) the row the user chose.
#[gpui::test]
fn an_untouched_database_tree_selects_nothing(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let root = two_connections("g1-tree", &[]);
    let (bench, cx) = window(&root, cx);
    let db = open_database(&bench, cx);
    assert_eq!(selected_id(&db, cx), None, "nothing is selected at first");
    db.read_with(cx, |v, _| {
        assert_eq!(v.selected_row_label(), None);
        assert_eq!(v.selected_label(), None);
        assert_eq!(
            v.target(),
            idecraft_workbench::context_menu::DbTarget::Nothing
        );
        assert!(v.quick_doc().is_none());
    });
    // Delete and F4 without a selection do nothing.
    cx.update(|window, cx| window.focus(&db.read(cx).focus()));
    cx.simulate_keystrokes("delete");
    assert_eq!(
        db.read_with(cx, |v, _| v.removing().map(str::to_owned)),
        None
    );
    // ↓ from no selection selects the first row.
    cx.simulate_keystrokes("down");
    assert_eq!(selected_id(&db, cx).as_deref(), Some("duck"));
    db.update(cx, |v, cx| assert!(v.select_label("shop", cx)));
    assert_eq!(selected_id(&db, cx).as_deref(), Some("shop"));
    // Reading the file again forgets the rows; the selection comes back only as it was.
    db.update(cx, |v, _| v.reload());
    assert_eq!(selected_id(&db, cx), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// G1 in the window: with the Database window open and untouched, ⌘⏎ in an unbound file opens the
/// data-source popup with the SQLite connection preselected, and Enter runs on it. Once the user
/// selects a connection in the tree, an unbound file runs on that one without asking.
#[gpui::test]
fn an_unbound_file_asks_for_its_data_source_while_the_database_window_is_open(
    cx: &mut TestAppContext,
) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let root = two_connections(
        "g1-run",
        &[
            ("q.sql", "pragma user_version;\nselect 42 as answer;\n"),
            ("r.sql", "select 7 as seven;\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let db = open_database(&bench, cx);
    assert_eq!(selected_id(&db, cx), None);

    let q = root.join("q.sql");
    run(&bench, cx, &q);
    let popup = bench
        .read_with(cx, |b, _| b.sql_popup())
        .expect("⌘⏎ opens the data-source popup although the Database window is open");
    popup.read_with(cx, |p, _| {
        assert!(p.title().contains("SQLite"), "{}", p.title());
        assert_eq!(p.items().len(), 2);
        assert_eq!(p.selected(), 1, "the SQLite connection is preselected");
    });
    assert_eq!(bound(&root, &q), None, "nothing ran before the choice");
    // Enter binds the file and goes on with the same run: two statements bring the choice of
    // behaviour 4, its first answer runs the statement at the caret or the whole script.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    if bench.read_with(cx, |b, _| b.sql_popup().is_some()) {
        cx.simulate_keystrokes("enter");
    }
    until(cx, "the file ran on shop", |cx| {
        first_cell(&bench, cx).is_some()
    });
    assert_eq!(bound(&root, &q).as_deref(), Some("shop"));

    // A connection the user selects in the tree is the data source of the next unbound file.
    db.update(cx, |v, cx| assert!(v.select_label("shop", cx)));
    let r = root.join("r.sql");
    run(&bench, cx, &r);
    assert!(
        bench.read_with(cx, |b, _| b.sql_popup().is_none()),
        "the selected connection is used without asking"
    );
    until(cx, "r.sql ran", |cx| {
        first_cell(&bench, cx).as_deref() == Some("7")
    });
    assert_eq!(bound(&root, &r).as_deref(), Some("shop"));
    let _ = std::fs::remove_dir_all(&root);
}

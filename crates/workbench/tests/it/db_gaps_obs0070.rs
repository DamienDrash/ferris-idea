//! OBS-0070 V1, V2 and V4 in the harness's window, on SQLite:
//! - `SPEC-0192` v6 behaviour 3b: ⌘⏎ in a file without a data source opens the data-source popup at
//!   once, with the connection of the file's dialect preselected, and runs after the choice.
//! - `SPEC-0190` v7 behaviour 1: running one statement after the script replaces only its tab.
//! - `SPEC-0164` v5 behaviour 16: ⌘Z after a rename says what the database keeps, and the way back
//!   opens the dialog; nothing runs before Refactor there.

#![cfg(target_os = "macos")]

use std::path::Path;

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::db_refactor_retest::{
    has_sqlite, open_with, rename_to, select_in_tree, sqlite, text, until, window, workspace,
};

/// Opens `file` in the window's editor, focused; the editor.
fn open_file(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    file: &Path,
) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, cx| b.editor_for(file, cx))
        .expect("the file is open")
}

/// The statement texts of the result tabs and their titles.
fn tabs(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> (Vec<String>, Vec<String>) {
    bench.read_with(cx, |b, cx| {
        b.services_view().map_or_else(Default::default, |v| {
            let v = v.read(cx);
            (v.result_statements(), v.tab_titles())
        })
    })
}

/// Number of Output lines, to tell when a run is over.
fn output(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> usize {
    bench.read_with(cx, |b, cx| {
        b.services_view()
            .map_or(0, |v| v.read(cx).output_lines().len())
    })
}

#[gpui::test]
fn running_an_unbound_file_asks_for_its_data_source(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    // `pragma` reads like SQLite; the DuckDB connection comes first in the list.
    let text = "pragma user_version;\nselect 42 as answer;\n";
    let root = workspace(
        "v1",
        "create table item (id integer primary key);",
        &[("q.sql", text)],
    );
    let duck = root.join("other.duckdb");
    let shop = root.join("shop.db");
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"duck\"\nname = \"duck\"\ndialect = \"duckdb\"\npath = \"{}\"\n\n\
             [[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            duck.display(),
            shop.display()
        ),
    )
    .unwrap();
    let (bench, cx) = window(&root, cx);
    let file = root.join("q.sql");
    let editor = open_file(&bench, cx, &file);
    let start = text.find("select").unwrap();
    let end = text.rfind(';').unwrap();
    let bound = || {
        let profiles = ferris_idea_database::load_profiles(&root).unwrap();
        ferris_idea_database::bound_connection(&root, &file, &profiles)
    };
    let ask = |cx: &mut VisualTestContext| {
        editor.update(cx, |e, cx| e.select_range(start..end, cx));
        bench.update_in(cx, |b, window, cx| {
            b.focus_editor(window, cx);
            assert!(b.execute_sql(window, cx), "the .sql file runs");
        });
        cx.run_until_parked();
        let popup = bench
            .read_with(cx, |b, _| b.sql_popup())
            .expect("⌘⏎ opens the data-source popup at once");
        popup.read_with(cx, |p, _| {
            assert!(p.title().contains("SQLite"), "{}", p.title());
            assert_eq!(p.items().len(), 2);
            assert_eq!(p.selected(), 1, "the SQLite connection is preselected");
        });
    };

    // Escape runs nothing and leaves the file unbound.
    ask(cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.sql_popup().is_none()));
    assert_eq!(bound(), None);
    assert!(bench.read_with(cx, |b, _| b.services_view().is_none()));

    // Enter binds the file to the preselected connection and runs the selection at once.
    ask(cx);
    cx.simulate_keystrokes("enter");
    until(cx, "the selected statement ran", |cx| {
        bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| {
                v.read(cx)
                    .shown_result()
                    .is_some_and(|r| r.rows.first().and_then(|r| r[0].as_deref()) == Some("42"))
            })
        })
    });
    assert_eq!(bound().as_deref(), Some("shop"));
    assert_eq!(tabs(&bench, cx).0, ["select 42 as answer"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn one_statement_replaces_only_its_own_result_tab(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let text = "select 1 as a;\nselect 2 as b;\nselect 3 as c;\n";
    let root = workspace(
        "v2",
        "create table item (id integer primary key);",
        &[("q.sql", text)],
    );
    let (bench, cx) = window(&root, cx);
    let file = root.join("q.sql");
    let editor = open_file(&bench, cx, &file);
    let all = ["select 1 as a", "select 2 as b", "select 3 as c"];
    let run_selected = |cx: &mut VisualTestContext, range: std::ops::Range<usize>| {
        let before = output(&bench, cx);
        editor.update(cx, |e, cx| e.select_range(range.clone(), cx));
        bench.update_in(cx, |b, window, cx| {
            b.focus_editor(window, cx);
            assert!(b.execute_sql(window, cx));
        });
        until(cx, "the run is over", |cx| output(&bench, cx) > before);
    };

    // The whole script: three tabs.
    run_selected(cx, 0..text.len());
    assert_eq!(tabs(&bench, cx).0, all);

    // The caret in the second statement, the statement chosen: still three tabs, the second one
    // new and shown.
    let caret = text.find("select 2").unwrap() + 3;
    let before = output(&bench, cx);
    editor.update(cx, |e, cx| e.select_range(caret..caret, cx));
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        assert!(b.execute_sql(window, cx));
    });
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.sql_popup().is_some()),
        "statement or whole script"
    );
    cx.simulate_keystrokes("enter");
    until(cx, "the statement ran", |cx| output(&bench, cx) > before);
    let (statements, titles) = tabs(&bench, cx);
    assert_eq!(statements, all, "only the second tab was replaced");
    assert_eq!(titles, ["Result 1", "Result 2", "Result 3"]);
    let shown = bench.read_with(cx, |b, cx| {
        b.services_view().map(|v| v.read(cx).selected_tab())
    });
    assert_eq!(shown.as_deref(), Some("Result 2"));

    // The first two statements, selected: their tabs are replaced, the third stays.
    run_selected(cx, 0..text.find("select 3").unwrap());
    assert_eq!(tabs(&bench, cx).0, all);

    // The whole script again replaces all three.
    run_selected(cx, 0..text.len());
    assert_eq!(
        tabs(&bench, cx),
        (
            all.map(String::from).to_vec(),
            vec![
                "Result 1".to_owned(),
                "Result 2".to_owned(),
                "Result 3".to_owned()
            ]
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn undo_after_a_rename_says_what_the_database_keeps(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let sql = "select qty from item;\n";
    let root = workspace(
        "v4",
        "create table item (id integer primary key, qty integer not null);",
        &[("q.sql", sql)],
    );
    let db = root.join("shop.db");
    let (bench, cx) = window(&root, cx);
    let file = root.join("q.sql");
    open_with(&bench, cx, &file, "shop");
    select_in_tree(&bench, cx, "qty");
    cx.simulate_keystrokes("shift-f6");
    rename_to(&bench, cx, "amount");
    assert_eq!(text(&bench, cx), "select amount from item;\n");
    assert!(sqlite(&db, ".schema item").contains("amount"));

    // ⌘Z takes back the file only; the status bar and the popup say so, Keep preselected.
    let undo = |cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        assert_eq!(text(&bench, cx), sql);
        let status = bench
            .read_with(cx, |b, cx| b.status_message(cx))
            .unwrap_or_default();
        assert!(
            status.contains("Undo took back q.sql only")
                && status.contains("the column is still item.amount"),
            "{status}"
        );
        let popup = bench
            .read_with(cx, |b, _| b.sql_popup())
            .expect("the way back is offered");
        popup.read_with(cx, |p, _| {
            assert_eq!(
                p.items(),
                ["Keep the database as it is", "Rename back in the database…"]
            );
            assert_eq!(p.selected(), 0);
        });
        popup
    };
    undo(cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.sql_popup().is_none()));
    assert!(sqlite(&db, ".schema item").contains("amount"), "kept");

    // Redo, undo again, and take the way back: the dialog renames amount back to qty and runs only
    // on Refactor.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-shift-z");
    cx.run_until_parked();
    assert_eq!(text(&bench, cx), "select amount from item;\n");
    let popup = undo(cx);
    popup.update(cx, |p, cx| {
        p.select(1);
        cx.emit(p.accept());
    });
    until(cx, "the dialog for the way back opens", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.db_rename_dialog()).unwrap();
    until(cx, "the preview of the way back", |cx| {
        dialog.read_with(cx, |d, cx| d.new_name(cx) == "qty" && d.preview().is_ok())
    });
    let (title, ddl) = dialog.read_with(cx, |d, _| {
        (d.title(), d.preview().map(|p| p.ddl.clone()).unwrap())
    });
    assert_eq!(title, "Rename column item.amount");
    assert!(ddl.contains("RENAME COLUMN amount TO qty"), "{ddl}");
    assert!(
        sqlite(&db, ".schema item").contains("amount"),
        "nothing ran yet"
    );
    cx.update(|window, cx| window.focus(&dialog.read(cx).focus(cx)));
    cx.simulate_keystrokes("enter");
    until(cx, "the way back ran", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_none())
    });
    let schema = sqlite(&db, ".schema item");
    assert!(
        schema.contains("qty") && !schema.contains("amount"),
        "{schema}"
    );
    assert_eq!(text(&bench, cx), sql, "file and database agree");
    let _ = std::fs::remove_dir_all(&root);
}

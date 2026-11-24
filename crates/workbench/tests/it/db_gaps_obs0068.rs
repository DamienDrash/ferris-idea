//! OBS-0068 L2 and L4 in the harness's window, on SQLite:
//! - `SPEC-0192` v5 behaviour 11a: a table the script's DDL makes (here by renaming) is known in the
//!   same file once the run is over, without a refresh. A table the script creates with
//!   `CREATE TABLE` is known before the run already (v7 behaviour 11b).
//! - `SPEC-0152` v3 behaviour 6a: the menu's Inline (the action `Inline`, also ⌥⌘N) inlines the
//!   `WITH` query at the caret of a `.sql` file. Other refactorings name no language server.

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

/// A workspace with the one SQLite connection `shop` and `q.sql` holding `text`; `None` without
/// `sqlite3`.
fn workspace(name: &str, text: &str) -> Option<std::path::PathBuf> {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-obs0068-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let shop = root.join("shop.db");
    if !sqlite(
        &shop,
        "CREATE TABLE item (id INTEGER PRIMARY KEY, body TEXT);",
    ) {
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
    std::fs::write(root.join("q.sql"), text).unwrap();
    Some(root)
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<EditorView>,
    &'static mut VisualTestContext,
) {
    cx.executor().allow_parking();
    let dir = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let file = root.join("q.sql");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, cx| b.editor_for(&file, cx))
        .expect("q.sql is open");
    (bench, editor, cx)
}

fn unknown(editor: &Entity<EditorView>, cx: &mut VisualTestContext, table: &str) -> bool {
    let message = format!("unknown table `{table}`");
    editor.read_with(cx, |e, _| {
        e.problems().iter().any(|p| p.message.contains(&message))
    })
}

/// Runs timers and background work until `done` holds; panics after 200 rounds.
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
fn a_table_the_script_renames_is_known_in_the_same_file(cx: &mut TestAppContext) {
    let text = "alter table item rename to fresh;\nselect * from fresh;\n";
    let Some(root) = workspace("ddl", text) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, editor, cx) = open(&root, cx);
    wait(cx, "`fresh` is marked before the run", |cx| {
        unknown(&editor, cx, "fresh")
    });
    // The whole script, selected, runs without a question.
    editor.update(cx, |e, cx| e.run_action("$SelectAll", cx));
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        assert!(b.execute_sql(window, cx), "the .sql file runs");
    });
    wait(cx, "after the run `fresh` is known in the file", |cx| {
        !unknown(&editor, cx, "fresh")
    });
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn the_menus_inline_inlines_a_with_query(cx: &mut TestAppContext) {
    let text = "with big as (select id, body from item where id > 1)\nselect b.body from big b;\n";
    let Some(root) = workspace("inline", text) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, editor, cx) = open(&root, cx);
    // The caret on `big` of the definition.
    editor.update(cx, |e, cx| e.go_to(0, 6, cx));
    bench.update_in(cx, |b, window, cx| b.menu_action("Inline", window, cx));
    cx.run_until_parked();
    let inlined = editor.read_with(cx, |e, _| e.state.buffer.text());
    assert!(
        !inlined.contains("with big")
            && inlined.contains("from (select id, body from item where id > 1) b;"),
        "{inlined}"
    );
    // Another refactoring says it is not offered here, naming no language server.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ExtractMethod", window, cx)
    });
    let status = bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default();
    assert!(
        status.contains("not offered in SQL files") && !status.contains("language server"),
        "{status}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

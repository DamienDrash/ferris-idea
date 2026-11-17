//! The open hand tests of priority workflow (b) "SQL against PostgreSQL or MySQL" walked in a
//! window of the test harness with clicks and keys, without the screen and without the keychain,
//! before the beta (the internal hand-test log, Runde 4):
//!
//! - 11 (SPEC-0093): the statement at the caret runs, a failing one is told in red in Output, ⌘8
//!   hides and shows the Services window.
//! - 22 (SPEC-0099): header clicks sort (twice: descending, ⇧-click: second rank), the filter
//!   field filters at once, the pager turns 500-row pages.
//! - 32 (SPEC-0107): a console result is edited in place (double click, Enter, "+ Row", ⌘↩); a row
//!   changed by someone else meanwhile is not written and the note says so.
//! - 113 (SPEC-0190): the data editor of a 1200-row table: pages, `WHERE` and `ORDER BY` on the
//!   server, one `UPDATE` in the preview, Manual transaction visible to others only after Commit.
//! - 114 (SPEC-0191): F5 copies a table with key, NOT NULL, defaults and checks; a second copy says
//!   "exists already" in the dialog.
//! - 115 (SPEC-0021 v3, IMP-0061, D42): ⇧-click selection, "Copy as" CSV and Markdown with header,
//!   pasted into an editor; the export dialog's preview follows format and header, its Export
//!   button stays inside a low window and writes what the preview shows.
//! - D39: a click on a row of the Database window selects it.
//!
//! On SQLite always; 113, 114 and 115 also on PostgreSQL with `IDECRAFT_TEST_PG` and its password
//! file (skipped without).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_workbench::{ServicesView, WorkbenchView, hit_targets, semantics};

use crate::db_refactor_retest::{has_sqlite, pg_run, reference, sqlite, until, window};

/// The inventory of the hand tests on SQLite: a key, a default, a check, a NULL and a value with
/// a semicolon.
const ITEM: &str = "CREATE TABLE item (id INTEGER PRIMARY KEY, sku TEXT NOT NULL DEFAULT 'new', \
     name TEXT, unit_price_cents INTEGER NOT NULL CHECK (unit_price_cents >= 0)); \
     INSERT INTO item VALUES (1, 'A-1', 'Bolt; M4', 25), (2, 'A-2', 'Nut', 10), (3, 'B-1', NULL, 5);";

/// `m56_hand` of hand test 113 (1200 rows, `n = id % 7`) and `nums` of hand test 22.
const BIG: &str = "CREATE TABLE m56_hand (id INTEGER PRIMARY KEY, n INTEGER); \
     WITH RECURSIVE g(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM g WHERE i < 1200) \
     INSERT INTO m56_hand SELECT i, i % 7 FROM g;";

/// A workspace with the SQLite connection `shop` (`item`, `m56_hand`) and `query.sql` holding
/// `sql`. `None` without the `sqlite3` program.
fn workspace(tag: &str, sql: &str) -> Option<PathBuf> {
    if !has_sqlite() {
        return None;
    }
    let root = std::env::temp_dir().join(format!("idecraft-sqlbeta-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let root = root.canonicalize().unwrap();
    let db = root.join("shop.db");
    sqlite(&db, &format!("{ITEM} {BIG}"));
    std::fs::write(
        root.join(".idecraft/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            db.display()
        ),
    )
    .unwrap();
    std::fs::write(root.join("query.sql"), sql).unwrap();
    std::fs::write(root.join("paste.txt"), "").unwrap();
    Some(root)
}

/// What a PostgreSQL workspace holds: its folder, the profile, the scratch schema, and the test
/// secret with the lock that keeps the PostgreSQL tests of this module apart.
type PgWorkspace = (
    PathBuf,
    idecraft_database::ConnectionProfile,
    String,
    (
        crate::db_refactor_retest::SecretFile,
        std::sync::MutexGuard<'static, ()>,
    ),
);

/// A workspace on the reference PostgreSQL as connection `pg`, working in the fresh schema
/// `scratch` (made here, with `item` and `m56_hand` like on SQLite). `None` without
/// `IDECRAFT_TEST_PG`.
fn pg_workspace(tag: &str, sql: &str) -> Option<PgWorkspace> {
    // The secret file is removed before the next test takes the lock (fields drop in order).
    let guard = PG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // An id of its own, so the secret is not the one of other modules' PostgreSQL tests.
    let (mut profile, secret) = reference("IDECRAFT_TEST_PG", "pgbeta")?;
    profile.name = "pg".into();
    let scratch = format!("ht{tag}_{}", std::process::id());
    pg_run(
        &profile,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch}; \
             set search_path to {scratch}; \
             create table item (id integer primary key, sku text not null default 'new', \
             name text, unit_price_cents integer not null check (unit_price_cents >= 0)); \
             insert into item values (1, 'A-1', 'Bolt; M4', 25), (2, 'A-2', 'Nut', 10), \
             (3, 'B-1', null, 5); \
             create table m56_hand as select g as id, g % 7 as n from generate_series(1, 1200) g; \
             alter table m56_hand add primary key (id);"
        ),
    );
    profile.options.startup_script = Some(format!("set search_path to {scratch}"));
    profile.options.schemas = vec![scratch.clone()];
    let root =
        std::env::temp_dir().join(format!("idecraft-sqlbeta-pg{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    idecraft_database::save_profiles(&root, std::slice::from_ref(&profile)).unwrap();
    std::fs::write(root.join("query.sql"), sql).unwrap();
    std::fs::write(root.join("paste.txt"), "").unwrap();
    Some((root, profile, scratch, (secret, guard)))
}

/// The PostgreSQL tests one after the other: they share this process's test secret.
static PG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Draws a frame and the one it asks for, so the marks are where the controls are.
fn paint(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
    }
}

/// Where the control `name` was last painted: the visible bounds of the element with that test id
/// in the window's semantic tree (`SPEC-0319` behaviour 42; the last one painted, as it is on top).
fn rect(cx: &mut VisualTestContext, name: &str) -> idecraft_ui::tool_windows::Rect {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    paint(cx);
    drawn(cx, name).unwrap_or_else(|| panic!("{name} is drawn"))
}

/// Where the element with the test id `name` is visible in the window's last semantic tree, if
/// it is (without drawing again).
fn drawn(cx: &mut VisualTestContext, name: &str) -> Option<idecraft_ui::tool_windows::Rect> {
    let tree = cx.update(|window, _| {
        semantics::tree(semantics::window_id(window)).expect("a tree is built")
    });
    tree.iter()
        .filter(|n| n.test_id() == Some(name))
        .filter_map(|n| n.visible_bounds)
        .last()
        .map(|r| idecraft_ui::tool_windows::Rect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        })
}

fn spot(cx: &mut VisualTestContext, name: &str) -> gpui::Point<gpui::Pixels> {
    let r = rect(cx, name);
    point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
}

fn click_with(cx: &mut VisualTestContext, name: &str, modifiers: Modifiers) {
    let at = spot(cx, name);
    cx.simulate_click(at, modifiers);
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, name: &str) {
    click_with(cx, name, Modifiers::none());
}

fn shift_click(cx: &mut VisualTestContext, name: &str) {
    click_with(
        cx,
        name,
        Modifiers {
            shift: true,
            ..Default::default()
        },
    );
}

fn double_click(cx: &mut VisualTestContext, name: &str) {
    let position = spot(cx, name);
    for click_count in [1, 2] {
        cx.simulate_event(gpui::MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: gpui::MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: gpui::MouseButton::Left,
            click_count,
        });
    }
    cx.run_until_parked();
}

fn services(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<ServicesView> {
    bench
        .read_with(cx, |b, _| b.services_view())
        .expect("the Services window is shown")
}

fn clipboard(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| cx.read_from_clipboard())
        .and_then(|c| c.text())
        .unwrap_or_default()
}

/// Opens `query.sql` and the Database window with the connection `name` selected; the editor has
/// the keyboard.
fn open(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, root: &Path, name: &str) {
    crate::db_refactor_retest::open_with(bench, cx, &root.join("query.sql"), name);
}

/// Puts the caret at `at` in the editor and runs the statement there (⌘↩); waits until a result
/// of `sql` or the run's end is shown.
fn run_at(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, at: usize, sql: &str) {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-enter");
    for _ in 0..5 {
        cx.run_until_parked();
    }
    // In a file of several statements ⌘↩ asks first; Enter takes the statement at the caret.
    if let Some(popup) = bench.read_with(cx, |b, _| b.sql_popup()) {
        let first = popup.read_with(cx, |p, _| p.items().first().cloned());
        assert!(
            first.is_some_and(|f| f.starts_with("Statement at caret: ") && f.contains(sql)),
            "the chooser offers the statement first"
        );
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
    }
    let done = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| {
                let v = v.read(cx);
                !v.running() && v.result_statements().iter().any(|s| s.contains(sql))
            })
        })
    };
    for _ in 0..1200 {
        if done(cx) {
            return;
        }
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let state = bench.read_with(cx, |b, cx| {
        b.services_view().map(|v| {
            let v = v.read(cx);
            (v.running(), v.result_statements(), v.output_lines())
        })
    });
    panic!("{sql} did not run: {state:?}");
}

/// Hand test 115 on the connection of `query.sql`'s workspace: `item` has the columns `id`, `sku`,
/// `name` first and the rows `(1, A-1, Bolt; M4)`, `(2, A-2, Nut)`.
fn copy_formats_and_export(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, root: &Path) {
    run_at(bench, cx, 0, "select * from item");
    let view = services(bench, cx);
    // The rows come back in key order; cells are named by row and column.
    click(cx, "services-cell-0-0");
    shift_click(cx, "services-cell-1-2");
    let selection = view.read_with(cx, |v, _| v.selection()).unwrap();
    assert_eq!((selection.anchor, selection.cursor), ((0, 0), (1, 2)));

    // Copy as → CSV, ⌘C: with the header, the semicolon needs no quotes.
    click(cx, "services-copy-as");
    click(cx, "services-format-CSV");
    cx.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    let csv = "id,sku,name\r\n1,A-1,Bolt; M4\r\n2,A-2,Nut\r\n";
    assert_eq!(clipboard(cx), csv);

    // Markdown: the header too, the numeric column aligned right.
    click(cx, "services-copy-as");
    click(cx, "services-format-Markdown");
    cx.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    let markdown =
        "| id | sku | name |\n| ---: | --- | --- |\n| 1 | A-1 | Bolt; M4 |\n| 2 | A-2 | Nut |\n";
    assert_eq!(clipboard(cx), markdown);

    // Pasted into an editor.
    let paste = root.join("paste.txt");
    bench.update_in(cx, |b, window, cx| {
        b.open(&paste, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert_eq!(
        bench.read_with(cx, |b, cx| b.active_text(cx)).as_deref(),
        Some(markdown)
    );

    // Export…: the preview follows the format and the header switch.
    click(cx, "services-export");
    assert!(view.read_with(cx, |v, _| v.exporting()));
    let preview = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.export_preview());
    assert_eq!(preview(cx), markdown, "the selection in the copy format");
    click(cx, "export-format-JSON");
    assert!(preview(cx).starts_with('['), "{}", preview(cx));
    click(cx, "export-format-CSV");
    assert_eq!(preview(cx), csv);
    click(cx, "export-header");
    assert_eq!(preview(cx), "1,A-1,Bolt; M4\r\n2,A-2,Nut\r\n");
    click(cx, "export-header");
    assert_eq!(preview(cx), csv);
    click(cx, "export-file");

    // D42: in a low window the Export button is still inside the Services panel and the window.
    cx.simulate_resize(gpui::size(px(1372.), px(560.)));
    let button = rect(cx, "export-run");
    let (footer, panel) = view.read_with(cx, |v, _| v.export_edges());
    assert!(
        // The mark covers the button inside its 1 px border.
        button.h >= idecraft_workbench::metrics::CONTROL_HEIGHT - 2.5
            && button.y + button.h <= 560.
            && footer <= panel + 0.5,
        "Export is fully shown: {button:?}, footer {footer}, panel {panel}"
    );
    let before: Vec<PathBuf> = csv_files(root);
    click(cx, "export-run");
    until(cx, "the export is written", |_| {
        csv_files(root).len() > before.len()
    });
    let written = csv_files(root)
        .into_iter()
        .find(|p| !before.contains(p))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&written).unwrap(),
        csv,
        "the file holds what the preview showed"
    );
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    paint(cx);
}

fn csv_files(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "csv"))
                .collect()
        })
        .unwrap_or_default()
}

/// Hand test 115 (SPEC-0021 v3, IMP-0061, D42) on SQLite.
#[gpui::test]
fn hand_test_115_copy_formats_and_export(cx: &mut TestAppContext) {
    let Some(root) = workspace("115", "select * from item;\n") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    copy_formats_and_export(&bench, cx, &root);
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 115 on PostgreSQL.
#[gpui::test]
fn hand_test_115_copy_formats_and_export_on_postgres(cx: &mut TestAppContext) {
    let Some((root, profile, scratch, secret)) =
        pg_workspace("115", "select * from item order by id;\n")
    else {
        eprintln!("skipped: IDECRAFT_TEST_PG or its password file is not set");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "pg");
    copy_formats_and_export(&bench, cx, &root);
    pg_run(&profile, &format!("drop schema {scratch} cascade"));
    drop(secret);
    let _ = std::fs::remove_dir_all(&root);
}

/// Expands the connection and its tables in the Database window and selects the row `label`
/// with a click; the window has the keyboard.
fn click_tree_row(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, label: &str) {
    let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    until(cx, "the tables are read", |cx| {
        db.update(cx, |v, cx| {
            if !v.visible().iter().any(|(_, n, _)| n == label) {
                v.expand(idecraft_workbench::db_tree_model::Node::Conn(0), cx);
                v.expand_all_tables(cx);
            }
            v.visible().iter().any(|(_, n, _)| n == label)
        })
    });
    click(cx, &format!("db-row-{label}"));
}

/// Hand test 114: F5 on `item`, the target, the preview with key, NOT NULL, default and check,
/// Copy (Enter); the row count in Output; a second copy says "exists already" in the dialog.
/// `schema` is typed into the target schema field when given.
fn copy_table(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    schema: Option<&str>,
) -> String {
    for round in 0..2 {
        click_tree_row(bench, cx, "item");
        let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
        assert_eq!(
            db.read_with(cx, |v, _| v.selected_table()).as_deref(),
            Some("item")
        );
        cx.simulate_keystrokes("f5");
        cx.run_until_parked();
        let dialog = bench
            .read_with(cx, |b, _| b.copy_table_dialog())
            .expect("F5 opens the dialog");
        assert_eq!(
            dialog.read_with(cx, |d, _| d.title()),
            "Copy Table item To…"
        );
        if let Some(schema) = schema {
            click(cx, "copy-table-Target schema");
            cx.simulate_input(schema);
            cx.run_until_parked();
        }
        until(cx, "the preview", |cx| {
            dialog.read_with(cx, |d, cx| d.preview(cx).is_ok())
        });
        let target = dialog.read_with(cx, |d, cx| d.target(cx));
        assert_eq!(target.schema.as_deref(), schema);
        let preview = dialog
            .read_with(cx, |d, cx| d.preview(cx))
            .unwrap()
            .0
            .join(";\n")
            .to_uppercase();
        for part in ["PRIMARY KEY", "NOT NULL", "DEFAULT", "CHECK"] {
            assert!(preview.contains(part), "{part} in {preview}");
        }
        cx.simulate_keystrokes("enter");
        if round == 0 {
            until(cx, "the copy ran", |cx| {
                bench.read_with(cx, |b, _| b.copy_table_dialog().is_none())
            });
            let output = services(bench, cx).read_with(cx, |v, _| v.output_lines().join("\n"));
            assert!(output.contains("Table copied: 3 row(s)"), "{output}");
        } else {
            until(cx, "the refusal", |cx| {
                dialog.read_with(cx, |d, _| {
                    d.note()
                        .is_some_and(|(t, e)| *e && t.contains("exists already"))
                })
            });
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            assert!(bench.read_with(cx, |b, _| b.copy_table_dialog().is_none()));
            return target.table;
        }
    }
    unreachable!()
}

/// Hand test 114 (SPEC-0191) on SQLite: the copy goes next to the table as `item_copy`.
#[gpui::test]
fn hand_test_114_copy_table(cx: &mut TestAppContext) {
    let Some(root) = workspace("114", "select 1;\n") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    let table = copy_table(&bench, cx, None);
    assert_eq!(table, "item_copy");
    let db = root.join("shop.db");
    let ddl = sqlite(&db, ".schema item_copy").to_uppercase();
    for part in ["PRIMARY KEY", "NOT NULL", "DEFAULT", "CHECK"] {
        assert!(ddl.contains(part), "{part} in {ddl}");
    }
    assert_eq!(sqlite(&db, "select count(*) from item_copy").trim(), "3");
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 114 on PostgreSQL: the copy goes into the schema `m56_copy…`.
#[gpui::test]
fn hand_test_114_copy_table_on_postgres(cx: &mut TestAppContext) {
    let Some((root, profile, scratch, secret)) = pg_workspace("114", "select 1;\n") else {
        eprintln!("skipped: IDECRAFT_TEST_PG or its password file is not set");
        return;
    };
    let target = format!("m56_copy_{}", std::process::id());
    pg_run(
        &profile,
        &format!("drop schema if exists {target} cascade; create schema {target}"),
    );
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "pg");
    let table = copy_table(&bench, cx, Some(&target));
    assert_eq!(table, "item");
    let defs = pg_run(
        &profile,
        &format!(
            "select string_agg(pg_get_constraintdef(oid), ' | ' order by conname) from \
             pg_constraint where conrelid = '{target}.item'::regclass"
        ),
    );
    let defs = defs[0][0].clone().unwrap_or_default().to_uppercase();
    assert!(
        defs.contains("PRIMARY KEY") && defs.contains("CHECK"),
        "{defs}"
    );
    let columns = pg_run(
        &profile,
        &format!(
            "select string_agg(column_name || ' ' || is_nullable || ' ' || \
             coalesce(column_default, '-'), ', ' order by ordinal_position) from \
             information_schema.columns where table_schema = '{target}' and table_name = 'item'"
        ),
    );
    let columns = columns[0][0].clone().unwrap_or_default();
    assert!(
        columns.contains("sku NO 'new'"),
        "NOT NULL and the default: {columns}"
    );
    let rows = pg_run(&profile, &format!("select count(*) from {target}.item"));
    assert_eq!(rows[0][0].as_deref(), Some("3"));
    pg_run(
        &profile,
        &format!("drop schema {target} cascade; drop schema {scratch} cascade"),
    );
    drop(secret);
    let _ = std::fs::remove_dir_all(&root);
}

/// D39: a click on a row of the Database window selects it; nothing is selected before.
#[gpui::test]
fn d39_a_click_selects_a_row_of_the_database_window(cx: &mut TestAppContext) {
    let Some(root) = workspace("d39", "select 1;\n") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    cx.run_until_parked();
    let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_profile().map(|p| p.id)),
        None,
        "no row is selected before the user selects one (#909)"
    );
    click(cx, "db-row-shop");
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_profile().map(|p| p.id))
            .as_deref(),
        Some("shop"),
        "the click selects the connection"
    );
    // The arrow expands; a click on a table row selects the table.
    click(cx, "db-arrow-shop");
    until(cx, "shop is read", |cx| {
        db.update(cx, |v, cx| {
            v.expand_all_tables(cx);
            v.visible().iter().any(|(_, n, _)| n == "m56_hand")
        })
    });
    click(cx, "db-row-m56_hand");
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_table()).as_deref(),
        Some("m56_hand")
    );
    click(cx, "db-row-item");
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_table()).as_deref(),
        Some("item")
    );
    // The window has the keyboard after the click: ↑ moves the selection.
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    assert_ne!(
        db.read_with(cx, |v, _| v.selected_table()).as_deref(),
        Some("item")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 11 (SPEC-0093): two statements, the caret in the second; a failing one; ⌘8.
#[gpui::test]
fn hand_test_11_console_error_and_cmd_8(cx: &mut TestAppContext) {
    let sql = "select 1 as one;\nselect * from item;\nselect * from no_such_table;\n";
    let Some(root) = workspace("11", sql) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    run_at(&bench, cx, sql.find("item").unwrap(), "select * from item");
    let view = services(&bench, cx);
    let (columns, null, label) = view.read_with(cx, |v, _| {
        (
            v.shown_result()
                .map(|r| r.columns.clone())
                .unwrap_or_default(),
            v.cell_text_at(2, 2),
            v.page_text(),
        )
    });
    assert_eq!(columns, ["id", "sku", "name", "unit_price_cents"]);
    assert_eq!(null.as_deref(), Some("<null>"), "NULL is shown as <null>");
    assert_eq!(label.as_deref(), Some("3 rows"));

    // The failing statement: its error is red in Output, which is shown.
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    let at = sql.find("no_such").unwrap();
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.sql_popup().is_some()));
    cx.simulate_keystrokes("enter");
    until(cx, "the error is told", |cx| {
        view.read_with(cx, |v, _| {
            !v.running()
                && v.output_entries()
                    .iter()
                    .any(|(t, failed)| *failed && t.contains("no_such_table"))
        })
    });
    assert_eq!(view.read_with(cx, |v, _| v.selected_tab()), "Output");

    // ⌘8 from the editor gives the shown window the keys (as for every tool window, hand test 7
    // of round 4); ⌘8 in the focused window hides it.
    cx.simulate_keystrokes("cmd-8");
    cx.run_until_parked();
    let focused = cx.update(|window, cx| view.read(cx).focus().is_focused(window));
    assert!(focused, "⌘8 from the editor focuses the Services window");
    cx.simulate_keystrokes("cmd-8");
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.services_view().is_none()),
        "⌘8 hides the Services window"
    );
    cx.simulate_keystrokes("cmd-8");
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.services_view().is_some()),
        "⌘8 shows it again"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 22 (SPEC-0099): sorting by header clicks, the filter field, the pager.
#[gpui::test]
fn hand_test_22_sort_filter_and_pages(cx: &mut TestAppContext) {
    let sql = "select n, id from m56_hand;\n";
    let Some(root) = workspace("22", sql) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    run_at(&bench, cx, 0, "select n, id from m56_hand");
    let view = services(&bench, cx);
    let page = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.page_text()).unwrap();
    let first = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| v.shown_rows().first().cloned())
            .unwrap()
    };
    assert_eq!(page(cx), "1–500 of 1200");

    // n ascending, then descending; ⇧-click on id adds it as the second rank.
    click(cx, "services-column-0");
    assert_eq!(first(cx), ["0", "7"]);
    click(cx, "services-column-0");
    assert_eq!(first(cx), ["6", "6"]);
    shift_click(cx, "services-column-1");
    assert_eq!(first(cx), ["6", "6"]);
    shift_click(cx, "services-column-1");
    assert_eq!(first(cx), ["6", "1196"], "id descending within n = 6");

    // Pages of 500.
    click(cx, "services-next");
    assert_eq!(page(cx), "501–1000 of 1200");
    click(cx, "services-last");
    assert_eq!(page(cx), "1001–1200 of 1200");
    click(cx, "services-first");
    assert_eq!(page(cx), "1–500 of 1200");

    // The filter acts while typing.
    click(cx, "services-field-Filter");
    cx.simulate_input("1199");
    cx.run_until_parked();
    let rows = view.read_with(cx, |v, _| v.shown_rows());
    assert_eq!(rows, [["2", "1199"]], "only the matching row");
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 32 (SPEC-0107): edit the console result, add a row, ⌘↩; a row changed meanwhile.
#[gpui::test]
fn hand_test_32_edit_the_console_result(cx: &mut TestAppContext) {
    let sql = "select * from item;\n";
    let Some(root) = workspace("32", sql) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let db = root.join("shop.db");
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    run_at(&bench, cx, 0, "select * from item");
    let view = services(&bench, cx);

    // Double click on Nut's name, a new name, Enter: the cell is marked changed.
    double_click(cx, "services-cell-1-2");
    assert!(
        view.read_with(cx, |v, _| v.editing()),
        "the double click edits"
    );
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("Washer");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.editing()));
    assert_eq!(
        view.read_with(cx, |v, _| v.shown_edits().map(|e| e.count())),
        Some(1)
    );

    // + Row: the new row's cells are typed in; ⌘↩ writes both in one go and reads again.
    click(cx, "services-add-row");
    for (k, text) in ["4", "C-1", "Screw", "7"].into_iter().enumerate() {
        let cell = format!("services-cell-3-{k}");
        double_click(cx, &cell);
        cx.simulate_input(text);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
    }
    cx.simulate_keystrokes("cmd-enter");
    until(cx, "the changes are written", |cx| {
        view.read_with(cx, |v, _| {
            !v.running()
                && v.shown_edits().is_some_and(|e| e.is_empty())
                && v.shown_rows().len() == 4
        })
    });
    assert_eq!(
        sqlite(&db, "select name from item where id in (2, 4) order by id").trim(),
        "Washer\nScrew"
    );

    // Someone else changes the row; the same row edited here again is not written.
    sqlite(&db, "update item set name = 'Other' where id = 2");
    double_click(cx, "services-cell-1-2");
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("Mine");
    cx.simulate_keystrokes("enter cmd-enter");
    until(cx, "the conflict is told", |cx| {
        view.read_with(cx, |v, _| {
            v.note()
                .is_some_and(|n| n.contains("changed or deleted since it was loaded"))
        })
    });
    assert_eq!(
        sqlite(&db, "select name from item where id = 2").trim(),
        "Other",
        "nothing written"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 113 on the open workspace: F4 on `m56_hand`, ⏭ ⏮, `WHERE n = 3`, ORDER BY by header
/// clicks, a cell changed, ⇧↩, Preview, Manual, ⌘↩ (not yet seen by `peek`), Commit (seen).
fn table_data_editor(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    quote: &str,
    peek: &dyn Fn() -> String,
) {
    click_tree_row(bench, cx, "m56_hand");
    cx.simulate_keystrokes("f4");
    let view = |cx: &mut VisualTestContext| services(bench, cx);
    until(cx, "the data editor loads", |cx| {
        bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| {
                v.read(cx)
                    .table_state()
                    .is_some_and(|(t, _, _, loading)| t.ends_with("m56_hand") && !loading)
            })
        })
    });
    let v = view(cx);
    let idle = |cx: &mut VisualTestContext| {
        let v = v.clone();
        until(cx, "the page loads", move |cx| {
            v.read_with(cx, |v, _| v.table_state().is_some_and(|s| !s.3))
        });
    };
    let page = |cx: &mut VisualTestContext| v.read_with(cx, |v, _| v.page_text()).unwrap();
    assert_eq!(page(cx), "1–500 of 1200");
    click(cx, "services-last");
    idle(cx);
    assert_eq!(page(cx), "1001–1200 of 1200");
    click(cx, "services-first");
    idle(cx);
    assert_eq!(page(cx), "1–500 of 1200");

    // WHERE n = 3 + Enter: the server counts 172 rows.
    click(cx, "services-field-WHERE");
    cx.simulate_input("n = 3");
    cx.simulate_keystrokes("enter");
    idle(cx);
    assert_eq!(page(cx), "172 rows");

    // Header n, then ⇧-click on id: ORDER BY on the server.
    click(cx, "services-column-1");
    idle(cx);
    shift_click(cx, "services-column-0");
    idle(cx);
    let order = v.read_with(cx, |v, _| v.table_state().unwrap().2);
    assert_eq!(order, format!("{q}n{q}, {q}id{q}", q = quote));

    // The first row (id 3) gets n = 99; ⇧↩ shows the record; Preview shows one UPDATE.
    double_click(cx, "services-cell-0-1");
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("99");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_keystrokes("shift-enter");
    cx.run_until_parked();
    assert!(v.read_with(cx, |v, _| v.side_view().is_some()));
    let record = v.read_with(cx, |v, _| v.record());
    assert!(
        record.iter().any(|(k, val)| k == "n" && val == "99"),
        "{record:?}"
    );
    click(cx, "services-preview");
    let preview = v.read_with(cx, |v, _| v.preview_sql()).unwrap();
    assert_eq!(preview.len(), 1, "{preview:?}");
    assert!(preview[0].starts_with("UPDATE"), "{preview:?}");

    // Tx: Manual; ⌘↩ applies in the open transaction; Commit makes it visible.
    click(cx, "services-tx");
    click(cx, "services-tx-mode-1");
    assert_eq!(v.read_with(cx, |v, _| v.transaction()), Some((true, None)));
    cx.simulate_keystrokes("cmd-enter");
    until(cx, "applied in the transaction", |cx| {
        v.read_with(cx, |v, _| v.transaction() == Some((true, Some(1))))
    });
    assert_eq!(peek(), "3", "others do not see it before Commit");
    click(cx, "services-commit");
    until(cx, "committed", |cx| {
        v.read_with(cx, |v, _| v.transaction() == Some((true, None)))
    });
    assert_eq!(peek(), "99");
}

/// Hand test 113 (SPEC-0190) on SQLite.
#[gpui::test]
fn hand_test_113_table_data_editor(cx: &mut TestAppContext) {
    let Some(root) = workspace("113", "select 1;\n") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let db = root.join("shop.db");
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    table_data_editor(&bench, cx, "\"", &|| {
        sqlite(&db, "select n from m56_hand where id = 3")
            .trim()
            .to_owned()
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 113 on PostgreSQL.
#[gpui::test]
fn hand_test_113_table_data_editor_on_postgres(cx: &mut TestAppContext) {
    let Some((root, profile, scratch, secret)) = pg_workspace("113", "select 1;\n") else {
        eprintln!("skipped: IDECRAFT_TEST_PG or its password file is not set");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "pg");
    let p = profile.clone();
    let s = scratch.clone();
    table_data_editor(&bench, cx, "\"", &move || {
        pg_run(&p, &format!("select n from {s}.m56_hand where id = 3"))[0][0]
            .clone()
            .unwrap_or_default()
    });
    pg_run(&profile, &format!("drop schema {scratch} cascade"));
    drop(secret);
    let _ = std::fs::remove_dir_all(&root);
}

/// B1 (SPEC-0021 v4 behaviour 11, AK-11): at the Services window's default height the export
/// preview shows every line of a short preview in full; a long one scrolls.
#[gpui::test]
fn b1_the_export_preview_shows_its_lines(cx: &mut TestAppContext) {
    let Some(root) = workspace("b1", "select * from item;\n") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "shop");
    run_at(&bench, cx, 0, "select * from item");
    let view = services(&bench, cx);
    click(cx, "services-cell-0-0");
    shift_click(cx, "services-cell-1-2");
    click(cx, "services-export");
    click(cx, "export-format-CSV");
    let preview = view.read_with(cx, |v, _| v.export_preview());
    assert_eq!(
        preview.lines().count(),
        3,
        "the header and two rows: {preview}"
    );
    let area = rect(cx, "export-preview");
    let lines: Vec<_> = (0..3)
        .map(|n| drawn(cx, &format!("export-preview-line-{n}")))
        .collect();
    let first = lines[0].expect("the first line is shown");
    for (n, line) in lines.iter().enumerate() {
        let line = line.unwrap_or_else(|| panic!("line {n} is shown: {area:?} {lines:?}"));
        assert!(
            (line.h - first.h).abs() < 0.5 && line.y + line.h <= area.y + area.h + 0.5,
            "line {n} is shown in full: {line:?} in {area:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// B2 and B3 (SPEC-0191 v2 behaviour 6, SPEC-0089 v14 behaviour 26): a console ran on `other`
/// before; Copy Table on `shop`'s `item` names `shop` in the Services header, and the selection in
/// the Database window stays on `item` while `shop` is read again. A selected table dropped
/// outside gives the selection to its parent after a refresh.
#[gpui::test]
fn b2_b3_copy_table_names_its_connection_and_keeps_the_selection(cx: &mut TestAppContext) {
    let Some(root) = workspace("b23", "select 1;\n") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    // A second connection after `shop`: the row that took `item`'s place in the defect.
    let other = root.join("other.db");
    sqlite(&other, "CREATE TABLE note (id INTEGER PRIMARY KEY);");
    let mut profiles = std::fs::read_to_string(root.join(".idecraft/connections.toml")).unwrap();
    profiles.push_str(&format!(
        "\n[[connection]]\nid = \"other\"\nname = \"other\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
        other.display()
    ));
    std::fs::write(root.join(".idecraft/connections.toml"), profiles).unwrap();
    hit_targets::enable();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root, "other");
    run_at(&bench, cx, 0, "select 1");
    let view = services(&bench, cx);
    assert_eq!(
        view.read_with(cx, |v, _| v.connection().to_owned()),
        "other"
    );

    click_tree_row(&bench, cx, "item");
    let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_table()).as_deref(),
        Some("item")
    );
    cx.simulate_keystrokes("f5");
    cx.run_until_parked();
    let dialog = bench
        .read_with(cx, |b, _| b.copy_table_dialog())
        .expect("F5 opens the dialog");
    until(cx, "the preview", |cx| {
        dialog.read_with(cx, |d, cx| d.preview(cx).is_ok())
    });
    cx.simulate_keystrokes("enter");
    until(cx, "the copy ran and shop is read again", |cx| {
        bench.read_with(cx, |b, _| b.copy_table_dialog().is_none())
            && db.read_with(cx, |v, _| {
                v.visible().iter().any(|(_, n, _)| n == "item_copy")
            })
    });
    // B2: the output is labelled with the connection the copy ran on.
    let (connection, output) = view.read_with(cx, |v, _| {
        (v.connection().to_owned(), v.output_lines().join("\n"))
    });
    assert!(output.contains("Table copied: 3 row(s)"), "{output}");
    assert_eq!(connection, "shop", "the header names the copy's connection");
    // B3: the selection stayed on `item`, not on the row that took its place meanwhile.
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_row_label()).as_deref(),
        Some("item"),
        "{:?}",
        db.read_with(cx, |v, _| v.visible())
    );

    // A table that is gone after a refresh: its parent, the `tables` group of `shop`, is selected.
    click(cx, "db-row-item_copy");
    assert_eq!(
        db.read_with(cx, |v, _| v.selected_table()).as_deref(),
        Some("item_copy")
    );
    sqlite(&root.join("shop.db"), "DROP TABLE item_copy;");
    db.update(cx, |v, cx| v.refresh_connection("shop", cx));
    until(cx, "shop is read again", |cx| {
        db.read_with(cx, |v, _| {
            let rows = v.visible();
            rows.iter().any(|(_, n, _)| n == "item")
                && !rows.iter().any(|(_, n, _)| n == "item_copy")
        })
    });
    assert_eq!(
        db.read_with(cx, |v, _| (
            v.selected_row_label(),
            v.selected_profile().map(|p| p.id)
        )),
        (Some("tables".to_owned()), Some("shop".to_owned())),
        "the nearest parent is selected: {:?}",
        db.read_with(cx, |v, _| v.visible())
    );
    let _ = std::fs::remove_dir_all(&root);
}

//! F-060 window checks (`OBS-0120`, `SPEC-0017` v2, `SPEC-0099`, `SPEC-0107`, `SPEC-0116`,
//! `SPEC-0142`, `SPEC-0319`): the database result grid driven with real mouse and key events
//! through the UI driver (`ferris_idea_uidriver::Engine`) against a local throwaway SQLite database
//! and checked in the semantic tree:
//! - column types on `ColumnHeader` nodes (declared types on editable tables, `numeric`/`text` on
//!   ad-hoc queries),
//! - numbers compared as numbers (numeric sorting in the grid and dialect DML previews for Oracle,
//!   MariaDB and SQL Server),
//! - editing cells with double-click + typing + Enter and submitting with ⌘↩, plus conflict
//!   detection,
//! - NULL display (`<null>`) and setting cells to NULL with Backspace,
//! - paging (`services-page-size`, `services-page-label`, `services-first`/`previous`/`next`/`last`
//!   and their disabled states).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ferris_idea_database::{Column, Dialect, EditTarget, GridEdits, Table};
use ferris_idea_uidriver::action::{Action, Button, Input, KeyPress, Modifier};
use ferris_idea_uidriver::engine::{Command as DriverCommand, Engine, Keys, Op, Request, Target};
use ferris_idea_uidriver::locator::Locator;
use ferris_idea_uidriver::tree::{ElementInfo, View};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{ConsoleRun, ResultSet, ServicesView, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use serde_json::Value;

type Bench = Entity<WorkbenchView>;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct NoKeys;

impl Keys for NoKeys {
    fn actions_for(&self, _: &KeyPress) -> Vec<String> {
        Vec::new()
    }
}

fn has_sqlite() -> bool {
    Command::new("sqlite3")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn sqlite(db: &Path, sql: &str) -> String {
    let out = Command::new("sqlite3").arg(db).arg(sql).output().unwrap();
    assert!(
        out.status.success(),
        "sqlite3 failed on `{sql}`: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let dir = root.join(".test-settings");
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    cx.executor().allow_parking();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    draw(cx);
    (bench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        draw(cx);
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn gpui_button(b: Button) -> MouseButton {
    match b {
        Button::Left => MouseButton::Left,
        Button::Right => MouseButton::Right,
        Button::Middle => MouseButton::Middle,
    }
}

fn gpui_modifiers(mods: &[Modifier]) -> Modifiers {
    let mut m = Modifiers::none();
    for k in mods {
        match k {
            Modifier::Cmd => m.platform = true,
            Modifier::Ctrl => m.control = true,
            Modifier::Alt => m.alt = true,
            Modifier::Shift => m.shift = true,
            Modifier::Fn => m.function = true,
        }
    }
    m
}

/// Delivers the real pointer and key events planned by `ferris_idea_uidriver::Engine` into the window.
fn deliver(cx: &mut VisualTestContext, inputs: Vec<Input>) {
    for input in inputs {
        match input {
            Input::PointerMove {
                x,
                y,
                pressed,
                modifiers,
            } => {
                cx.simulate_mouse_move(
                    point(px(x), px(y)),
                    pressed.map(gpui_button),
                    gpui_modifiers(&modifiers),
                );
            }
            Input::PointerDown {
                x,
                y,
                button,
                count,
                modifiers,
            } => {
                cx.simulate_event(gpui::MouseDownEvent {
                    position: point(px(x), px(y)),
                    modifiers: gpui_modifiers(&modifiers),
                    button: gpui_button(button),
                    click_count: usize::from(count),
                    first_mouse: false,
                });
            }
            Input::PointerUp {
                x,
                y,
                button,
                count,
                modifiers,
            } => {
                cx.simulate_event(gpui::MouseUpEvent {
                    position: point(px(x), px(y)),
                    modifiers: gpui_modifiers(&modifiers),
                    button: gpui_button(button),
                    click_count: usize::from(count),
                });
            }
            Input::Key(press) => {
                let text = press.to_text();
                cx.simulate_keystrokes(&text);
            }
            Input::Text { text } => {
                cx.simulate_input(&text);
            }
            other => panic!("unexpected driver input in test: {other:?}"),
        }
    }
    draw(cx);
}

/// Runs one `ferris_idea_uidriver::Engine` operation against the window's semantic tree, delivering
/// any mouse/key events it plans into `cx`.
fn drive(cx: &mut VisualTestContext, op: Op) -> Value {
    let mut engine = Engine::new();
    let wid = cx.update(|window, _| semantics::window_id(window));
    let id = engine
        .submit(wid, Request::selftest(op.clone()), Instant::now())
        .unwrap_or_else(|e| panic!("submit {op:?}: {e}"));
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        draw(cx);
        let tree = cx
            .update(|window, _| semantics::tree(semantics::window_id(window)))
            .expect("semantic tree built for frame");
        let out = engine.frame(wid, &tree, Instant::now(), &NoKeys);
        for cmd in out.commands {
            if let DriverCommand::Deliver { inputs, .. } = cmd {
                deliver(cx, inputs);
            }
        }
        if let Some(done) = out.done.into_iter().find(|d| d.op == id) {
            return done
                .reply
                .result
                .unwrap_or_else(|e| panic!("driver op {op:?} failed: {e}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("driver op timed out: {op:?}");
}

fn driver_click(cx: &mut VisualTestContext, test_id: &str) {
    drive(
        cx,
        Op::Act {
            target: Target::Locator(Locator::id(test_id)),
            action: Action::Click {
                button: Button::Left,
                count: 1,
                modifiers: Vec::new(),
                position: None,
            },
            level: None,
            force: false,
        },
    );
}

fn driver_double_click(cx: &mut VisualTestContext, test_id: &str) {
    drive(
        cx,
        Op::Act {
            target: Target::Locator(Locator::id(test_id)),
            action: Action::Click {
                button: Button::Left,
                count: 2,
                modifiers: Vec::new(),
                position: None,
            },
            level: None,
            force: false,
        },
    );
}

fn driver_press(cx: &mut VisualTestContext, keys: &str) {
    drive(
        cx,
        Op::Act {
            target: Target::Window,
            action: Action::Press {
                keys: keys.to_owned(),
            },
            level: None,
            force: false,
        },
    );
}

fn find_id(cx: &mut VisualTestContext, test_id: &str) -> Option<ElementInfo> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let view = View::in_process();
        tree.iter()
            .find(|n| n.test_id() == Some(test_id))
            .map(|n| ElementInfo::of(&tree, n, &view))
    })
}

fn column_cells_in_paint_order(cx: &mut VisualTestContext, col: usize) -> Vec<String> {
    draw(cx);
    let suffix = format!("-{col}");
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("semantic tree");
        let view = View::in_process();
        tree.iter()
            .filter(|n| {
                n.test_id()
                    .is_some_and(|id| id.starts_with("services-cell-") && id.ends_with(&suffix))
            })
            .map(|n| ElementInfo::of(&tree, n, &view).value.unwrap_or_default())
            .collect()
    })
}

fn run_sql(bench: &Bench, cx: &mut VisualTestContext, sql: &str) -> Entity<ServicesView> {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("editor open");
    editor.update(cx, |e, cx| e.set_text(sql, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    draw(cx);
    driver_press(cx, "cmd-a");
    driver_press(cx, "cmd-enter");
    until(cx, "query finished", |cx| {
        bench.read_with(cx, |b, cx| {
            b.services_view()
                .is_some_and(|v| !v.read(cx).running() && v.read(cx).shown_result().is_some())
        })
    });
    bench
        .read_with(cx, |b, _| b.services_view())
        .expect("services view")
}

/// F-060 window check against a local throwaway SQLite database driven through the UI driver:
/// column types, numbers compared as numbers (sorting), NULL display, cell editing + submit +
/// conflict detection, read-only ad-hoc query types, and paging.
#[gpui::test]
fn f060_result_grid_in_window_via_ui_driver(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-f060-grid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let root = root.canonicalize().unwrap();
    let _cleanup = TempDir(root.clone());

    let db = root.join("f060_throwaway.db");
    let mut seed = String::from(
        "CREATE TABLE item (\
             id INTEGER PRIMARY KEY,\
             price NUMERIC(8,2),\
             whole DECIMAL(38,0),\
             label TEXT\
         );\
         INSERT INTO item VALUES (1, 0.05, 4, 'Bolt');\
         INSERT INTO item VALUES (2, -1.5, NULL, 'Nut');\
         INSERT INTO item VALUES (3, 10, 0, 'Washer');\
         INSERT INTO item VALUES (4, 2, -7, NULL);\
         INSERT INTO item VALUES (5, NULL, 12, 'Pin');\
         CREATE TABLE paged (id INTEGER PRIMARY KEY, n INTEGER, label TEXT);",
    );
    for i in 1..=25 {
        seed.push_str(&format!(
            "INSERT INTO paged VALUES ({i}, {}, 'row-{i}');",
            i * 3
        ));
    }
    sqlite(&db, &seed);

    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"local\"\nname = \"local\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            db.display()
        ),
    )
    .unwrap();
    let file = root.join("query.sql");
    std::fs::write(
        &file,
        "SELECT id, price, whole, label FROM item ORDER BY id;\n",
    )
    .unwrap();
    let _ = ferris_idea_database::bind_file(&root, &file, Some("local"));

    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.toggle_database(window, cx);
    });
    draw(cx);
    let db_view = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    db_view.update(cx, |v, cx| assert!(v.select_label("local", cx)));
    draw(cx);

    // 1. Column types in the semantic tree (editable table query).
    let services = run_sql(
        &bench,
        cx,
        "SELECT id, price, whole, label FROM item ORDER BY id;\n",
    );
    until(cx, "result is editable", |cx| {
        services.read_with(cx, |v, _| v.shown_edits().is_some())
    });

    for (k, expected_name, expected_type) in [
        (0, "id", "INTEGER"),
        (1, "price", "NUMERIC(8,2)"),
        (2, "whole", "DECIMAL(38,0)"),
        (3, "label", "TEXT"),
    ] {
        let col = find_id(cx, &format!("services-column-{k}"))
            .unwrap_or_else(|| panic!("column header {k} in semantic tree"));
        assert_eq!(col.role, "columnheader");
        assert_eq!(col.name.as_deref(), Some(expected_name));
        assert_eq!(
            col.description.as_deref(),
            Some(expected_type),
            "column {expected_name} exposes its declared type"
        );
    }

    // 2. NULL display and integer-valued DECIMAL(38,0) display in the semantic tree.
    let c_price = find_id(cx, "services-cell-0-1").expect("cell 0-1");
    assert_eq!(c_price.value.as_deref(), Some("0.05"));
    assert!(!c_price.states.contains(&"read-only".to_owned()));

    let c_whole = find_id(cx, "services-cell-0-2").expect("cell 0-2");
    assert_eq!(c_whole.value.as_deref(), Some("4"));

    for null_cell in [
        "services-cell-1-2",
        "services-cell-3-3",
        "services-cell-4-1",
    ] {
        let cell = find_id(cx, null_cell).unwrap_or_else(|| panic!("{null_cell} in tree"));
        assert_eq!(cell.name.as_deref(), Some("<null>"), "{null_cell}");
        assert_eq!(cell.value.as_deref(), Some("<null>"), "{null_cell}");
    }

    // 3. Numbers compared as numbers: sorting `price` via UI driver clicks on the column header.
    driver_click(cx, "services-column-1");
    assert_eq!(
        column_cells_in_paint_order(cx, 1),
        ["<null>", "-1.5", "0.05", "2", "10"],
        "ascending numeric sort puts 2 before 10 and NULL first"
    );
    driver_click(cx, "services-column-1");
    assert_eq!(
        column_cells_in_paint_order(cx, 1),
        ["10", "2", "0.05", "-1.5", "<null>"],
        "descending numeric sort puts 10 before 2 and NULL last"
    );
    driver_click(cx, "services-column-1");
    assert_eq!(
        column_cells_in_paint_order(cx, 1),
        ["0.05", "-1.5", "10", "2", "<null>"],
        "third click clears sort"
    );

    // 4. Editing a numeric cell (< 1) and setting a text cell to NULL via real mouse/key events.
    driver_double_click(cx, "services-cell-0-1");
    assert!(
        services.read_with(cx, |v, _| v.editing()),
        "double click opens cell editor"
    );
    driver_press(cx, "cmd-a");
    cx.simulate_input("0.07");
    draw(cx);
    driver_press(cx, "enter");

    let edited_price = find_id(cx, "services-cell-0-1").expect("edited cell 0-1");
    assert_eq!(edited_price.value.as_deref(), Some("0.07"));
    assert_eq!(edited_price.description.as_deref(), Some("changed"));

    driver_click(cx, "services-cell-0-3");
    let selected_label = find_id(cx, "services-cell-0-3").expect("selected cell 0-3");
    assert!(selected_label.states.contains(&"selected".to_owned()));
    driver_press(cx, "backspace");

    let nulled_label = find_id(cx, "services-cell-0-3").expect("nulled cell 0-3");
    assert_eq!(nulled_label.value.as_deref(), Some("<null>"));
    assert_eq!(nulled_label.description.as_deref(), Some("changed"));

    // Submit both edits with ⌘↩ and verify persistence in SQLite and in the semantic tree.
    driver_press(cx, "cmd-enter");
    until(cx, "edits committed and reloaded", |cx| {
        services.read_with(cx, |v, _| {
            !v.running() && v.shown_edits().is_some_and(|e| e.is_empty())
        })
    });
    let committed_price = find_id(cx, "services-cell-0-1").expect("committed cell 0-1");
    assert_eq!(committed_price.value.as_deref(), Some("0.07"));
    assert_eq!(committed_price.description, None);
    let committed_label = find_id(cx, "services-cell-0-3").expect("committed cell 0-3");
    assert_eq!(committed_label.value.as_deref(), Some("<null>"));
    assert_eq!(committed_label.description, None);
    assert_eq!(
        sqlite(
            &db,
            "SELECT price, COALESCE(label, '<NULL>') FROM item WHERE id = 1;"
        )
        .trim(),
        "0.07|<NULL>"
    );

    // Conflict detection: external change on row 1 prevents overwriting it.
    sqlite(&db, "UPDATE item SET price = 0.09 WHERE id = 1;");
    driver_double_click(cx, "services-cell-0-1");
    driver_press(cx, "cmd-a");
    cx.simulate_input("0.11");
    draw(cx);
    driver_press(cx, "enter");
    driver_press(cx, "cmd-enter");
    until(cx, "conflict reported", |cx| {
        services.read_with(cx, |v, _| {
            v.note()
                .is_some_and(|n| n.contains("changed or deleted since it was loaded"))
        })
    });
    assert_eq!(
        sqlite(&db, "SELECT price FROM item WHERE id = 1;").trim(),
        "0.09",
        "external value kept on conflict"
    );

    // 5. Computed column alongside table columns: `next_price` is inferred numeric and read-only,
    // while `label` keeps its declared `TEXT` type and stays editable.
    let _ = run_sql(
        &bench,
        cx,
        "SELECT id, price + 1 AS next_price, label FROM item ORDER BY id;\n",
    );
    let col_next = find_id(cx, "services-column-1").expect("computed column 1");
    assert_eq!(col_next.name.as_deref(), Some("next_price"));
    assert_eq!(col_next.description.as_deref(), Some("numeric"));
    let col_lbl = find_id(cx, "services-column-2").expect("table column 2");
    assert_eq!(col_lbl.name.as_deref(), Some("label"));
    assert_eq!(col_lbl.description.as_deref(), Some("TEXT"));
    let ro_cell = find_id(cx, "services-cell-0-1").expect("read-only computed cell");
    assert!(ro_cell.states.contains(&"read-only".to_owned()));
    let rw_cell = find_id(cx, "services-cell-0-2").expect("editable table cell");
    assert!(!rw_cell.states.contains(&"read-only".to_owned()));

    // 6. Paging through the UI driver and checking pager buttons, label and rows in the tree.
    let _ = run_sql(&bench, cx, "SELECT id, n, label FROM paged ORDER BY id;\n");
    driver_click(cx, "services-page-size");
    driver_click(cx, "services-size-0"); // 10 rows per page

    let label = find_id(cx, "services-page-label").expect("page label");
    assert_eq!(label.name.as_deref(), Some("1–10 of 25"));
    assert!(
        find_id(cx, "services-first")
            .unwrap()
            .states
            .contains(&"disabled".to_owned())
    );
    assert!(
        find_id(cx, "services-previous")
            .unwrap()
            .states
            .contains(&"disabled".to_owned())
    );
    assert!(
        !find_id(cx, "services-next")
            .unwrap()
            .states
            .contains(&"disabled".to_owned())
    );
    assert_eq!(
        find_id(cx, "services-cell-0-0")
            .and_then(|c| c.value)
            .as_deref(),
        Some("1")
    );
    assert!(find_id(cx, "services-cell-10-0").is_none());

    driver_click(cx, "services-next");
    assert_eq!(
        find_id(cx, "services-page-label")
            .and_then(|n| n.name)
            .as_deref(),
        Some("11–20 of 25")
    );
    assert_eq!(
        find_id(cx, "services-cell-10-0")
            .and_then(|c| c.value)
            .as_deref(),
        Some("11")
    );
    assert!(
        !find_id(cx, "services-previous")
            .unwrap()
            .states
            .contains(&"disabled".to_owned())
    );

    driver_click(cx, "services-last");
    assert_eq!(
        find_id(cx, "services-page-label")
            .and_then(|n| n.name)
            .as_deref(),
        Some("21–25 of 25")
    );
    assert_eq!(
        find_id(cx, "services-cell-20-0")
            .and_then(|c| c.value)
            .as_deref(),
        Some("21")
    );
    assert!(
        find_id(cx, "services-next")
            .unwrap()
            .states
            .contains(&"disabled".to_owned())
    );
    assert!(
        find_id(cx, "services-last")
            .unwrap()
            .states
            .contains(&"disabled".to_owned())
    );

    driver_click(cx, "services-previous");
    assert_eq!(
        find_id(cx, "services-page-label")
            .and_then(|n| n.name)
            .as_deref(),
        Some("11–20 of 25")
    );

    driver_click(cx, "services-first");
    assert_eq!(
        find_id(cx, "services-page-label")
            .and_then(|n| n.name)
            .as_deref(),
        Some("1–10 of 25")
    );
}

fn dialect_table(types: [&str; 4]) -> Table {
    let names = ["id", "price", "weight", "whole"];
    Table {
        name: "item".into(),
        kind: "table".into(),
        columns: names
            .iter()
            .zip(types)
            .map(|(name, ty)| Column {
                name: (*name).into(),
                data_type: ty.into(),
                nullable: *name != "id",
                default: None,
                generated: None,
                expression: None,
                identity: None,
                comment: None,
            })
            .collect(),
        primary_key: vec!["id".into()],
        ..Table::default()
    }
}

/// F-060 (`OBS-0120` X1 & X3) in the window's data grid for Oracle, MariaDB and SQL Server without
/// external servers: column types, `<null>` and whole decimals (`4`) in the semantic tree, and
/// editing a number below 1 (`0.05` → `0.07`) + setting a float to NULL via UI driver events,
/// checked in the DML preview (`services-preview-line-0`) so numbers are compared as numbers.
#[gpui::test]
fn f060_dialect_numeric_editing_preview_in_grid(cx: &mut TestAppContext) {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-f060-dialects-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let _cleanup = TempDir(root.clone());

    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| b.toggle_services(window, cx));
    draw(cx);
    let services = bench
        .read_with(cx, |b, _| b.services_view())
        .expect("services view");

    let cases = [
        (
            Dialect::Oracle,
            ["NUMBER(10)", "NUMBER(8,2)", "BINARY_FLOAT", "NUMBER(38,0)"],
            "UPDATE \"item\" SET \"price\" = 0.07, \"weight\" = NULL WHERE \"id\" = 7 \
             AND DECODE(\"price\", 0.05, 1, 0) = 1 \
             AND DECODE(\"weight\", CAST(1.5 AS BINARY_FLOAT), 1, 0) = 1;",
        ),
        (
            Dialect::MariaDb,
            ["int", "decimal(8,2)", "float", "decimal(38,0)"],
            "UPDATE `item` SET `price` = 0.07, `weight` = NULL WHERE `id` = 7 \
             AND `price` <=> 0.05 AND CAST(`weight` AS CHAR) <=> '1.5';",
        ),
        (
            Dialect::SqlServer,
            ["int", "decimal(8,2)", "real", "decimal(38,0)"],
            "UPDATE [item] SET [price] = 0.07, [weight] = NULL WHERE [id] = 7 \
             AND [price] = 0.05 AND [weight] = CAST(1.5 AS real);",
        ),
    ];

    for (dialect, types, expected_sql) in cases {
        let table = dialect_table(types);
        let columns: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        let edits =
            GridEdits::new(EditTarget::new(&table, dialect).unwrap(), columns.clone()).unwrap();
        let run = ConsoleRun {
            output: vec![(format!("{dialect:?} ok"), false)],
            results: vec![ResultSet {
                columns,
                rows: vec![
                    vec![
                        Some("7".into()),
                        Some("0.05".into()),
                        Some("1.5".into()),
                        Some("4".into()),
                    ],
                    vec![Some("8".into()), None, Some("-0.5".into()), None],
                ],
                time: Duration::from_millis(1),
                sql: "SELECT id, price, weight, whole FROM item".into(),
            }],
            edits: vec![Some(edits)],
            ran: 1,
            ..ConsoleRun::default()
        };
        services.update(cx, |v, cx| v.finish(run, cx));
        draw(cx);

        for (k, expected_type) in types.iter().enumerate() {
            let col = find_id(cx, &format!("services-column-{k}")).expect("column header");
            assert_eq!(
                col.description.as_deref(),
                Some(*expected_type),
                "{dialect:?} column {k}"
            );
        }
        assert_eq!(
            find_id(cx, "services-cell-0-3")
                .and_then(|c| c.value)
                .as_deref(),
            Some("4"),
            "{dialect:?}: integer-valued decimal has no .0"
        );
        assert_eq!(
            find_id(cx, "services-cell-1-1")
                .and_then(|c| c.value)
                .as_deref(),
            Some("<null>"),
            "{dialect:?}: NULL displays as <null>"
        );

        // Edit 0.05 -> 0.07 and set float 1.5 -> NULL via real UI driver events.
        driver_double_click(cx, "services-cell-0-1");
        driver_press(cx, "cmd-a");
        cx.simulate_input("0.07");
        draw(cx);
        driver_press(cx, "enter");

        driver_click(cx, "services-cell-0-2");
        driver_press(cx, "backspace");

        // Open DML preview via UI driver click and check SQL in the semantic tree.
        driver_click(cx, "services-preview");
        let preview_line = find_id(cx, "services-preview-line-0")
            .unwrap_or_else(|| panic!("{dialect:?}: preview line in semantic tree"));
        assert_eq!(
            preview_line.name.as_deref(),
            Some(expected_sql),
            "{dialect:?}: numbers compared as numbers in DML preview"
        );
        // Close preview before the next dialect.
        driver_press(cx, "escape");
    }
}

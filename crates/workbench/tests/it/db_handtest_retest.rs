//! Retest of the failed hand tests 37, 120 and 121 (`docs/testing/DEFECTS-2026-11-05.md`) in a
//! window of the test harness, headless on SQLite, without the screen and without the keychain:
//!
//! - 37 (F-059, D13): the plan with its full-scan hint, then Compare Schemas asks for the target
//!   instead of taking another connection, the Schema Diff is listed and Open Migration Script opens
//!   the script bound to the target.
//! - 120 (F-056, D43): the ER boxes hold name and type as the font draws them, the diagram is
//!   fitted when it opens, ⌘0 then ⇧⌘0 fits again, moved boxes keep their lines, Layout resets,
//!   SVG and PNG are written with whole names and types.
//! - 121 (F-058, D44): an unchanged view is not dropped and created again in either direction, the
//!   data-loss question wraps with Include and Cancel inside the window, the script drops the view
//!   before its table, runs on the target and a second comparison finds no differences.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const WIDTH: f32 = 1372.;
const HEIGHT: f32 = 860.;

fn sqlite(db: &Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// The inventory of the hand tests on SQLite: `item`, `warehouse`, `movement` and the view
/// `stock_value`, which names its columns.
const INVENTORY: &str = "CREATE TABLE item (id INTEGER PRIMARY KEY, sku TEXT NOT NULL, \
     unit_price_cents integer NOT NULL); \
     CREATE TABLE warehouse (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
     CREATE TABLE movement (id INTEGER PRIMARY KEY, \
     item_id INTEGER NOT NULL REFERENCES item(id), \
     warehouse_id integer NOT NULL REFERENCES warehouse(id), quantity INTEGER NOT NULL, \
     moved_at \"timestamp without time zone\" NOT NULL); \
     CREATE VIEW stock_value AS SELECT m.warehouse_id, i.sku, m.quantity * i.unit_price_cents \
     AS value_cents FROM movement m JOIN item i ON i.id = m.item_id;";

/// A workspace with the connections `refdb` (the inventory) and `compare_b` (the inventory with
/// `item.discontinued`, a table `extra` and the view `extra_v` on it). `None` without `sqlite3`.
fn workspace(tag: &str) -> Option<PathBuf> {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-dbretest-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let (a, b) = (root.join("refdb.db"), root.join("compare_b.db"));
    let made = sqlite(&a, INVENTORY)
        && sqlite(
            &b,
            &format!(
                "{INVENTORY} ALTER TABLE item ADD COLUMN discontinued INTEGER; \
                 CREATE TABLE extra (id INTEGER); CREATE VIEW extra_v AS SELECT id FROM extra;"
            ),
        );
    if !made {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"refdb\"\nname = \"refdb\"\ndialect = \"sqlite\"\npath = \"{}\"\n\n\
             [[connection]]\nid = \"compare_b\"\nname = \"compare_b\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            a.display(),
            b.display()
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("query.sql"),
        "select m.quantity, w.name from movement m join warehouse w on w.id = m.warehouse_id;\n",
    )
    .unwrap();
    Some(root)
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(WIDTH), px(HEIGHT)));
    cx.run_until_parked();
    (bench, cx)
}

/// Opens `query.sql` and the Database window with `name` selected.
fn open_with(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, root: &Path, name: &str) {
    let file = root.join("query.sql");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        if b.database_view().is_none() {
            b.toggle_database(window, cx);
        }
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        let view = b.database_view().expect("the Database window is open");
        view.update(cx, |v, cx| {
            let row = v
                .visible()
                .iter()
                .position(|(_, n, _)| n == name)
                .expect("the connection is listed");
            v.select(row, cx);
        });
        b.focus_editor(window, cx);
    });
}

fn until(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    what: &str,
    done: impl Fn(&WorkbenchView, &gpui::App) -> bool,
) {
    for _ in 0..400 {
        cx.run_until_parked();
        if bench.read_with(cx, |b, cx| done(b, cx)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("timed out: {what}");
}

fn compared(b: &WorkbenchView, cx: &gpui::App) -> bool {
    !b.reading_schemas()
        && b.services_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.running() && v.schema_diff().is_some()
        })
}

/// Compare Schemas from the editor: the popup lists `target` alone; Enter compares.
fn compare_to(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, target: &str) {
    let sides = |b: &WorkbenchView, cx: &gpui::App| {
        b.services_view().and_then(|v| {
            v.read(cx)
                .schema_diff()
                .and_then(|d| d.choice.as_ref())
                .map(|c| (c.source.clone(), c.target.clone()))
        })
    };
    let before = bench.read_with(cx, |b, cx| sides(b, cx));
    bench.update_in(cx, |b, window, cx| b.compare_schemas(window, cx));
    cx.run_until_parked();
    let (items, now) = bench.read_with(cx, |b, cx| {
        (
            b.compare_popup()
                .map(|p| p.read(cx).items().to_vec())
                .unwrap_or_default(),
            sides(b, cx),
        )
    });
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].starts_with(&format!("{target} · ")), "{items:?}");
    assert_eq!(
        now, before,
        "nothing is compared before the target is chosen (D13)"
    );
    cx.simulate_keystrokes("enter");
    let target = target.to_owned();
    until(bench, cx, "the comparison", move |b, cx| {
        compared(b, cx) && sides(b, cx).is_some_and(|(_, t)| t == target)
    });
}

fn choice(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> ferris_idea_workbench::db_compare_model::MigrationChoice {
    bench.read_with(cx, |b, cx| {
        b.services_view()
            .and_then(|v| v.read(cx).schema_diff().and_then(|d| d.choice.clone()))
            .expect("the statements are listed")
    })
}

/// Hand test 37 (SPEC-0110 v3 AK-7, D13).
#[gpui::test]
fn hand_test_37_plan_then_compare_asks_for_the_target(cx: &mut TestAppContext) {
    let Some(root) = workspace("37") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = window(&root, cx);
    open_with(&bench, cx, &root, "refdb");

    // The plan of the join, its full scan named with a hint.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.ExplainPlan", window, cx)
    });
    until(&bench, cx, "the plan", |b, cx| {
        b.services_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.running() && v.plan().is_some()
        })
    });
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    assert_eq!(services.read_with(cx, |v, _| v.selected_tab()), "Plan");
    let plan = services.read_with(cx, |v, _| v.plan().cloned()).unwrap();
    assert!(plan.raw.contains("SCAN"), "{}", plan.raw);
    assert!(
        plan.hints.iter().any(|h| h.starts_with("full scan of")),
        "{:?}",
        plan.hints
    );

    // Compare Schemas asks, even with one candidate; the Schema Diff lists the differences.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    compare_to(&bench, cx, "compare_b");
    let c = choice(&bench, cx);
    assert_eq!(
        (c.source.as_str(), c.target.as_str()),
        ("refdb", "compare_b")
    );
    assert!(!c.statements.is_empty());

    // Open Migration Script: the script opens, bound to the target.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.OpenMigrationScript", window, cx)
    });
    cx.run_until_parked();
    let script = bench
        .read_with(cx, |b, _| b.migration_script().map(Path::to_path_buf))
        .expect("the script was written");
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(script.clone())
    );
    let profiles = ferris_idea_database::load_profiles(&root).unwrap();
    assert_eq!(
        ferris_idea_database::bound_connection(&root, &script, &profiles).as_deref(),
        Some("compare_b"),
        "the script runs on the target"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 120 (SPEC-0111 v3 AK-8, v4; D43).
#[gpui::test]
fn hand_test_120_boxes_hold_their_text_and_the_diagram_fits(cx: &mut TestAppContext) {
    let Some(root) = workspace("120") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = window(&root, cx);
    open_with(&bench, cx, &root, "refdb");
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.ShowErDiagram", window, cx)
    });
    until(&bench, cx, "the diagram", |b, _| b.er_view().is_some());
    let er = bench.read_with(cx, |b, _| b.er_view()).unwrap();
    for _ in 0..20 {
        cx.run_until_parked();
    }

    // Fitted when it opens: the whole canvas is inside the diagram area, so no box is cut off.
    let fits = |cx: &mut VisualTestContext| {
        er.read_with(cx, |v, _| {
            let (vw, vh) = v.viewport();
            let l = v.layout();
            let fit = ferris_idea_database::er_layout::fit_zoom(l.width, l.height, vw, vh);
            (
                v.zoom(),
                fit,
                l.width * v.zoom() <= vw + 0.5,
                l.height * v.zoom() <= vh + 0.5,
            )
        })
    };
    let (zoom, fit, inside_w, inside_h) = fits(cx);
    assert!(!er.read_with(cx, |v, _| v.fit_pending()));
    assert!((zoom - fit).abs() < 1e-3, "opened at {zoom}, fit is {fit}");
    assert!(inside_w && inside_h, "the canvas is inside the area");
    let names: Vec<String> = er.read_with(cx, |v, _| {
        v.layout().boxes.iter().map(|b| b.name.clone()).collect()
    });
    for t in ["item", "movement", "stock_value", "warehouse"] {
        assert!(names.iter().any(|n| n == t), "{names:?}");
    }
    assert!(
        er.read_with(cx, |v, _| v
            .layout()
            .boxes
            .iter()
            .any(|b| b.view && b.name == "stock_value")),
        "the view is drawn as one"
    );

    // Name and type side by side as the diagram font draws them: no name is shortened, and the
    // measured text ends one gap before the type (D43).
    let boxes = er.read_with(cx, |v, _| v.layout().boxes.clone());
    let family = ferris_idea_workbench::fonts::editor_family();
    let measure = |cx: &mut VisualTestContext, s: &str| -> f32 {
        cx.update(|window, _| {
            let run = gpui::TextRun {
                len: s.len(),
                font: gpui::font(family.clone()),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(s.to_owned().into(), px(12.), &[run], None)
                    .width,
            )
        })
    };
    use ferris_idea_database::er_layout::INSET;
    for b in &boxes {
        for (_, name, ty, _) in &b.lines {
            assert_eq!(
                &ferris_idea_database::er_layout::fitted_name(b, name, ty),
                name,
                "{name} is shortened in {}",
                b.name
            );
            let name_end = INSET + b.key_width + INSET + measure(cx, name);
            let type_start = b.width - INSET - measure(cx, ty);
            assert!(
                name_end + INSET <= type_start + 0.5,
                "{name} / {ty} overlap in {} ({name_end} + gap > {type_start})",
                b.name
            );
        }
    }

    // ⌘= zooms in, ⌘0 is 100 %, ⇧⌘0 right after fits again (the 125 % of D43 was ⇧0 read as ⌘=).
    cx.update(|window, cx| window.focus(&er.read(cx).focus()));
    cx.simulate_keystrokes("cmd-=");
    assert!(er.read_with(cx, |v, _| v.zoom()) > fit);
    cx.simulate_keystrokes("cmd-0");
    assert_eq!(er.read_with(cx, |v, _| v.zoom()), 1.);
    cx.simulate_keystrokes("cmd-shift-0");
    cx.run_until_parked();
    let (zoom, fit, _, _) = fits(cx);
    assert!((zoom - fit).abs() < 1e-3, "⇧⌘0 gave {zoom}, fit is {fit}");

    // A moved box keeps its lines; Layout (⌥L) puts it back.
    let before = er.read_with(cx, |v, _| v.layout().clone());
    let movement = before
        .boxes
        .iter()
        .position(|b| b.name == "movement")
        .unwrap();
    er.update(cx, |v, cx| v.move_box(movement, 900., 700., cx));
    let moved = er.read_with(cx, |v, _| v.layout().clone());
    assert_eq!(moved.boxes[movement].x, 900.);
    assert_ne!(moved.lines, before.lines, "the lines follow the box");
    assert_eq!(moved.lines.len(), before.lines.len());
    cx.simulate_keystrokes("alt-l");
    assert_eq!(
        er.read_with(cx, |v, _| v.layout().boxes.clone()),
        before.boxes
    );

    // SVG and PNG with whole names and types.
    for export in [
        ferris_idea_workbench::ErExport::Svg,
        ferris_idea_workbench::ErExport::Png(1.),
    ] {
        let path = er
            .update(cx, |v, cx| v.export(export, cx))
            .expect("exported");
        assert!(
            path.starts_with(root.join(".ferris-idea/diagrams")),
            "{path:?}"
        );
        assert!(std::fs::metadata(&path).is_ok_and(|m| m.len() > 0));
    }
    let svg = std::fs::read_to_string(root.join(".ferris-idea/diagrams/refdb.svg")).unwrap();
    for text in [
        ">unit_price_cents<",
        ">moved_at<",
        ">timestamp without time zone<",
        ">warehouse_id<",
    ] {
        assert!(svg.contains(text), "{text} in the SVG");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 121 (SPEC-0018 v3, SPEC-0110 v3 AK-7; D44 and the re-created view).
#[gpui::test]
fn hand_test_121_views_and_the_data_loss_question(cx: &mut TestAppContext) {
    let Some(root) = workspace("121") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    ferris_idea_workbench::hit_targets::enable();
    ferris_idea_workbench::semantics::set_consumer(
        ferris_idea_workbench::semantics::Consumer::SelfTest,
        true,
    );
    let (bench, cx) = window(&root, cx);

    // compare_b → refdb: the new column, table and view are added; stock_value stays.
    open_with(&bench, cx, &root, "compare_b");
    compare_to(&bench, cx, "refdb");
    let c = choice(&bench, cx);
    let sql: Vec<&str> = c.statements.iter().map(|s| s.sql.as_str()).collect();
    assert!(
        sql.iter()
            .any(|s| s.contains("CREATE VIEW") && s.contains("extra_v")),
        "{sql:?}"
    );
    assert!(
        !sql.iter().any(|s| s.contains("stock_value")),
        "the unchanged view stays: {sql:?}"
    );

    // refdb → compare_b: DROP VIEW extra_v before DROP TABLE extra, both unchecked with their loss.
    // SQLite drops a column by rebuilding `item`, so stock_value is re-created around the rebuild
    // (on PostgreSQL it stays: `ddl::tests::views_that_do_not_use_a_dropped_column_stay`).
    open_with(&bench, cx, &root, "refdb");
    compare_to(&bench, cx, "compare_b");
    let c = choice(&bench, cx);
    let sql: Vec<&str> = c.statements.iter().map(|s| s.sql.as_str()).collect();
    let at = |what: &str| sql.iter().position(|s| s.contains(what));
    let (view, table) = (
        at("DROP VIEW \"extra_v\"").unwrap(),
        at("DROP TABLE \"extra\"").unwrap(),
    );
    assert!(view < table, "{sql:?}");
    for (s, on) in c.statements.iter().zip(&c.selected) {
        assert!(s.destructive.is_none() || !on, "{} starts unchecked", s.sql);
    }

    // The question: it wraps, Include and Cancel are inside the window; Enter includes.
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.IncludeDestructiveChanges", window, cx)
    });
    for _ in 0..5 {
        cx.run_until_parked();
    }
    assert!(services.read_with(cx, |v, _| v.confirming_migration()));
    cx.update(|window, cx| window.draw(cx).clear());
    let tree = cx.update(|window, _| {
        ferris_idea_workbench::semantics::tree(ferris_idea_workbench::semantics::window_id(window))
            .expect("a tree is built")
    });
    let get = |name: &str| {
        tree.iter()
            .find(|n| n.test_id() == Some(name))
            .and_then(|n| n.visible_bounds)
            .unwrap_or_else(|| panic!("{name} is drawn"))
    };
    let (text, include, cancel) = (
        get("migration-confirmation"),
        get("migration-include"),
        get("migration-cancel"),
    );
    for r in [text, include, cancel] {
        assert!(
            r.x >= 0. && r.x + r.w <= WIDTH && r.y + r.h <= HEIGHT,
            "{r:?} in the window"
        );
    }
    assert!(
        include.y >= text.y + text.h - 0.5,
        "the buttons sit below the question"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!services.read_with(cx, |v, _| v.confirming_migration()));
    let c = choice(&bench, cx);
    assert!(c.selected.iter().all(|on| *on), "everything included");

    // The script is one transaction; run on the target it leaves no differences.
    let script = bench
        .read_with(cx, |b, _| b.migration_script().map(Path::to_path_buf))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    assert!(
        script.contains("BEGIN") && script.contains("COMMIT"),
        "{script}"
    );
    // Run on the target as the console would, read from standard input (it starts with `--`).
    let run = {
        use std::io::Write;
        let mut child = std::process::Command::new("sqlite3")
            .args(["-bail"])
            .arg(root.join("compare_b.db"))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait().is_ok_and(|s| s.success())
    };
    assert!(run, "the script runs: {script}");
    // Compared again (the other way round, so the new result is told from the last one).
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    open_with(&bench, cx, &root, "compare_b");
    compare_to(&bench, cx, "refdb");
    let c = choice(&bench, cx);
    assert!(
        c.statements.is_empty(),
        "no differences: {:?}",
        c.statements
    );
    let _ = std::fs::remove_dir_all(&root);
}

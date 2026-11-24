//! Window check of `SPEC-0110` v5, headless on SQLite: the plan is read by keyboard (Tree, Raw,
//! step details, copy), Compare Schemas runs from the Database window's menu, the statement list
//! answers to ↑/↓ and Space, Swap exchanges the sides, and a cancelled comparison shows nothing.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::{MenuPlace, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, point, px};

const QUERY: &str = "select c.name, o.id from customers c join orders o on o.customer_id = c.id;\n";

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// Two SQLite connections: `shop` with `customers` and `orders`, `copy` with `customers` and a
/// table `extra` that `shop` lacks. `None` without the `sqlite3` program.
fn workspace(tag: &str) -> Option<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("ferris-idea-dbecp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let (shop, copy) = (root.join("shop.db"), root.join("copy.db"));
    let made = sqlite(
        &shop,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));",
    ) && sqlite(
        &copy,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE extra (id INTEGER);",
    );
    if !made {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n\n\
             [[connection]]\nid = \"copy\"\nname = \"copy\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            shop.display(),
            copy.display()
        ),
    )
    .unwrap();
    std::fs::write(root.join("query.sql"), QUERY).unwrap();
    Some(root)
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = root.to_path_buf();
    let file = root.join("query.sql");
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.toggle_database(window, cx);
    });
    cx.run_until_parked();
    select_shop(&bench, cx);
    (bench, cx)
}

fn select_shop(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        let view = b.database_view().expect("the Database window is open");
        view.update(cx, |v, cx| {
            let row = v
                .visible()
                .iter()
                .position(|(_, name, _)| name == "shop")
                .expect("shop is listed");
            v.select(row, cx);
        });
    });
}

/// Waits for the background work of the Services window.
fn settle(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    done: fn(&WorkbenchView, &gpui::App) -> bool,
) {
    for _ in 0..400 {
        cx.run_until_parked();
        if bench.read_with(cx, done) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("the background work did not finish");
}

fn compared(b: &WorkbenchView, cx: &gpui::App) -> bool {
    !b.reading_schemas()
        && b.services_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.running() && v.schema_diff().is_some()
        })
}

/// SPEC-0110/AK-11 (v5; four views since v13).
#[gpui::test]
fn the_plan_is_read_by_keyboard(cx: &mut TestAppContext) {
    let Some(root) = workspace("plan") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        b.menu_action("Sql.ExplainPlan", window, cx);
    });
    settle(&bench, cx, |b, cx| {
        b.services_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.running() && v.plan().is_some()
        })
    });
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    assert_eq!(services.read_with(cx, |v, _| v.selected_tab()), "Plan");
    assert!(!services.read_with(cx, |v, _| v.plan_raw()), "Tree first");
    cx.update(|window, cx| window.focus(&services.read(cx).focus()));
    cx.simulate_keystrokes("down");
    assert_eq!(services.read_with(cx, |v, _| v.plan_step()), Some(0));
    // ⌥R goes Tree → Diagram → Flame → Raw (SPEC-0110 v13 behaviour 19).
    cx.simulate_keystrokes("alt-r alt-r alt-r");
    let raw = services.read_with(cx, |v, _| {
        (
            v.plan_raw(),
            v.plan().map(|p| p.raw.clone()).unwrap_or_default(),
        )
    });
    assert!(raw.0, "⌥R shows the raw plan");
    assert!(
        raw.1.contains("SCAN") || raw.1.contains("SEARCH"),
        "{}",
        raw.1
    );
    cx.simulate_keystrokes("cmd-c");
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some(raw.1.as_str()));
    cx.simulate_keystrokes("alt-r");
    assert!(
        !services.read_with(cx, |v, _| v.plan_raw()),
        "⌥R goes back to Tree"
    );
    cx.simulate_keystrokes("cmd-c");
    let tree = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert!(
        tree.lines().count() >= 2 && tree.contains("SCAN  o") && tree.contains("SEARCH  c"),
        "{tree}"
    );
    assert_eq!(
        bench.read_with(cx, |b, cx| b.active_text(cx)).as_deref(),
        Some(QUERY),
        "no key reached the editor"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0110/AK-12 (v5).
#[gpui::test]
fn the_comparison_swaps_and_answers_to_the_keyboard(cx: &mut TestAppContext) {
    let Some(root) = workspace("swap") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, _, cx| {
        b.open_context_menu(MenuPlace::Database, None, point(px(200.), px(200.)), cx)
    });
    let chosen = bench.update_in(cx, |b, window, cx| {
        b.choose_context_item("Compare Schemas…", window, cx)
    });
    assert!(chosen, "the Database window's menu has Compare Schemas…");
    cx.run_until_parked();
    let items = bench.read_with(cx, |b, cx| {
        b.compare_popup()
            .map(|p| p.read(cx).items().to_vec())
            .unwrap_or_default()
    });
    assert!(
        items.len() == 1 && items[0].starts_with("copy · "),
        "{items:?}"
    );
    cx.simulate_keystrokes("enter");
    settle(&bench, cx, compared);
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    let choice = |cx: &mut VisualTestContext| {
        services.read_with(cx, |v, _| {
            v.schema_diff().and_then(|d| d.choice.clone()).unwrap()
        })
    };
    let first = choice(cx);
    assert_eq!(
        (first.source.as_str(), first.target.as_str()),
        ("shop", "copy")
    );
    let destructive = first
        .statements
        .iter()
        .position(|s| s.destructive.is_some())
        .expect("dropping `extra` is destructive");
    assert!(!first.selected[destructive]);

    cx.update(|window, cx| window.focus(&services.read(cx).focus()));
    for _ in 0..=destructive {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(
        services.read_with(cx, |v, _| v.statement_mark()),
        Some(destructive)
    );
    cx.simulate_keystrokes("space");
    assert!(
        services.read_with(cx, |v, _| v.confirming_migration()),
        "Space asks first"
    );
    cx.simulate_keystrokes("escape");
    assert!(!choice(cx).selected[destructive], "Escape leaves it out");
    cx.simulate_keystrokes("space");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        choice(cx).selected[destructive],
        "Space and Enter include it"
    );
    let script = services.read_with(cx, |v, _| v.schema_diff().unwrap().script.clone());
    assert!(script.contains("DROP TABLE"), "{script}");

    // Swap: the other direction, read again, every destructive statement unchecked.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.SwapComparison", window, cx)
    });
    settle(&bench, cx, compared);
    let swapped = choice(cx);
    assert_eq!(
        (swapped.source.as_str(), swapped.target.as_str()),
        ("copy", "shop")
    );
    let lines = services.read_with(cx, |v, _| v.schema_diff().unwrap().lines.clone());
    let text: Vec<String> = lines
        .iter()
        .map(|l| format!("{} {}", l.mark.sign(), l.text))
        .collect();
    assert!(
        text.iter().any(|l| l.contains("+ ") && l.contains("extra")),
        "{text:?}"
    );
    assert!(
        text.iter()
            .any(|l| l.contains("orders") && !l.starts_with("+")),
        "{text:?}"
    );
    assert!(
        swapped
            .statements
            .iter()
            .zip(&swapped.selected)
            .all(|(s, on)| s.destructive.is_none() || !on),
        "the choice starts over"
    );
    let file = bench.read_with(cx, |b, _| {
        b.migration_script().map(std::path::Path::to_path_buf)
    });
    assert!(
        file.as_ref()
            .and_then(|f| f.file_name())
            .is_some_and(|n| n == "copy-to-shop.sql"),
        "{file:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0110/AK-13 (v5).
#[gpui::test]
fn a_cancelled_comparison_shows_nothing(cx: &mut TestAppContext) {
    let Some(root) = workspace("cancel") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        // Without a comparison the status bar says so.
        b.menu_action("Sql.CancelComparison", window, cx);
        b.compare_schemas(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    settle(&bench, cx, compared);
    // Swap starts reading again; it is given up before its result can arrive.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.SwapComparison", window, cx);
        assert!(b.reading_schemas(), "the comparison runs");
        b.menu_action("Sql.CancelComparison", window, cx);
        assert!(!b.reading_schemas());
    });
    for _ in 0..40 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    let (diff, output, running) = services.read_with(cx, |v, _| {
        (v.schema_diff().is_some(), v.output_lines(), v.running())
    });
    assert!(
        !diff,
        "no Schema Diff tab comes from a cancelled comparison"
    );
    assert!(!running, "the Services window stops waiting");
    assert!(
        output
            .iter()
            .any(|l| l.contains("comparison was cancelled")),
        "{output:?}"
    );
    assert!(
        !root
            .join(".ferris-idea/migrations/copy-to-shop.sql")
            .exists(),
        "a cancelled comparison writes no script"
    );
    let _ = std::fs::remove_dir_all(&root);
}

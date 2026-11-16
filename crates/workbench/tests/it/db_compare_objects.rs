//! Window check of `SPEC-0110` v12 behaviour 18 (OBS-0068 L8), headless on SQLite: Compare With…
//! on a table in the Database window compares it with another table of the same file; the Schema
//! Diff tab names both objects, the drop of a column starts unchecked, the script changes only the
//! target table, and Swap exchanges the objects.

#![cfg(target_os = "macos")]

use gpui::{Entity, TestAppContext, VisualTestContext, point, px};
use idecraft_workbench::{DatabaseView, MenuPlace, WorkbenchView};

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// One SQLite connection `shop` with `item`, `item_copy` (another column) and `other`; `None`
/// without the `sqlite3` program.
fn workspace() -> Option<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("idecraft-dbcmpobj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let shop = root.join("shop.db");
    let made = sqlite(
        &shop,
        "CREATE TABLE item (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price INTEGER); \
         CREATE TABLE item_copy (id INTEGER PRIMARY KEY, name TEXT NOT NULL, legacy TEXT); \
         CREATE TABLE other (id INTEGER PRIMARY KEY);",
    );
    if !made {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".idecraft/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            shop.display()
        ),
    )
    .unwrap();
    Some(root)
}

fn wait(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..400 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{what}");
}

fn popup_items(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.compare_popup()
            .map(|p| p.read(cx).items().to_vec())
            .unwrap_or_default()
    })
}

#[gpui::test]
fn compare_with_compares_two_tables(cx: &mut TestAppContext) {
    let Some(root) = workspace() else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    let view: Entity<DatabaseView> = bench
        .read_with(cx, |b, _| b.database_view())
        .expect("the Database window is open");
    let row = |v: &DatabaseView, name: &str| v.visible().iter().position(|(_, n, _)| n == name);
    view.update(cx, |v, cx| {
        v.expand(idecraft_workbench::db_tree_model::Node::Conn(0), cx)
    });
    wait(cx, "shop is read", |cx| {
        view.read_with(cx, |v, _| row(v, "tables").is_some())
    });
    view.update(cx, |v, cx| v.expand_all_tables(cx));
    wait(cx, "shop lists its tables", |cx| {
        view.read_with(cx, |v, _| row(v, "item").is_some())
    });
    view.update(cx, |v, cx| {
        let at = row(v, "item").expect("item is listed");
        v.select(at, cx);
    });

    // Compare With… from the table's context menu.
    bench.update_in(cx, |b, _, cx| {
        b.open_context_menu(MenuPlace::Database, None, point(px(200.), px(200.)), cx)
    });
    let chosen = bench.update_in(cx, |b, window, cx| {
        b.choose_context_item("Compare With…", window, cx)
    });
    assert!(chosen, "a table's menu has Compare With…");
    cx.run_until_parked();
    let connections = popup_items(&bench, cx);
    assert_eq!(connections, ["shop · this connection"], "{connections:?}");
    cx.simulate_keystrokes("enter");
    wait(cx, "the tables to compare with are listed", |cx| {
        !popup_items(&bench, cx).is_empty()
    });
    let objects = popup_items(&bench, cx);
    assert_eq!(
        objects,
        ["item_copy", "other"],
        "the source itself is left out"
    );
    cx.simulate_keystrokes("enter");
    wait(cx, "the comparison is done", |cx| {
        bench.read_with(cx, |b, cx| {
            !b.reading_schemas()
                && b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running() && v.schema_diff().is_some()
                })
        })
    });
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    let tab = services.read_with(cx, |v, _| v.schema_diff().cloned().unwrap());
    let choice = tab.choice.clone().unwrap();
    assert_eq!(
        (choice.source.as_str(), choice.target.as_str()),
        ("shop.item", "shop.item_copy")
    );
    let destructive = choice
        .statements
        .iter()
        .position(|s| s.destructive.is_some())
        .expect("dropping legacy is destructive");
    assert!(!choice.selected[destructive], "it starts unchecked");
    assert!(tab.script.contains("item_copy"), "{}", tab.script);
    assert!(!tab.script.contains("\"other\""), "{}", tab.script);

    // Swap exchanges the objects with their sides.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.SwapComparison", window, cx)
    });
    wait(cx, "the swapped comparison is done", |cx| {
        services.read_with(cx, |v, _| {
            !v.running()
                && v.schema_diff()
                    .and_then(|d| d.choice.clone())
                    .is_some_and(|c| c.source == "shop.item_copy")
        })
    });
    let _ = std::fs::remove_dir_all(&root);
}

//! `SPEC-0089` v13 behaviour 18, headless on SQLite: the Database window lists a connection's
//! triggers in their own folder, and Show DDL on one opens its definition read-only.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext};

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// A workspace with the SQLite connection `shop`: a table and a trigger. `None` without the
/// `sqlite3` program.
fn workspace(tag: &str) -> Option<std::path::PathBuf> {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-db-defs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let shop = root.join("shop.db");
    let made = sqlite(
        &shop,
        "CREATE TABLE stock (sku TEXT, qty INTEGER); \
         CREATE TRIGGER stock_check BEFORE INSERT ON stock BEGIN SELECT 1; END;",
    );
    if !made {
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
    Some(root)
}

/// Runs the background reads until `done` holds; panics after five seconds.
fn wait(
    view: &Entity<ferris_idea_workbench::DatabaseView>,
    cx: &mut VisualTestContext,
    what: &str,
    done: impl Fn(&ferris_idea_workbench::DatabaseView) -> bool,
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

/// SPEC-0089 AK-28 (v13): the trigger folder with its count, the trigger with table and timing,
/// Show DDL opens `stock_check (DDL).sql` read-only with SQLite's statement.
#[gpui::test]
fn show_ddl_opens_the_definition_of_a_trigger(cx: &mut TestAppContext) {
    let Some(root) = workspace("ddl") else {
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
    let row = |v: &ferris_idea_workbench::DatabaseView, name: &str| {
        v.visible().iter().position(|(_, n, _)| n == name)
    };
    view.update(cx, |v, cx| {
        v.expand(ferris_idea_workbench::db_tree_model::Node::Conn(0), cx)
    });
    wait(&view, cx, "shop lists its triggers", |v| {
        row(v, "triggers").is_some()
    });
    view.update(cx, |v, cx| {
        v.expand(
            ferris_idea_workbench::db_tree_model::Node::Objects(0, "trigger".into()),
            cx,
        )
    });
    let (_, _, detail) = view.read_with(cx, |v, _| {
        v.visible()
            .into_iter()
            .find(|(_, n, _)| n == "stock_check")
            .expect("the trigger is listed")
    });
    assert_eq!(detail, "stock · BEFORE INSERT");
    view.update(cx, |v, cx| {
        let at = row(v, "stock_check").unwrap();
        v.select(at, cx);
        v.show_ddl(cx);
    });
    let mut opened = None;
    for _ in 0..200 {
        cx.run_until_parked();
        opened = bench.read_with(cx, |b, _| b.active_path());
        if opened
            .as_ref()
            .is_some_and(|p| p.ends_with("stock_check (DDL).sql"))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let path = opened.expect("a tab is open");
    assert!(
        path.ends_with("stock_check (DDL).sql"),
        "{}",
        path.display()
    );
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    let text = editor.read_with(cx, |e, _| e.state.buffer.text());
    assert!(
        text.starts_with("CREATE TRIGGER stock_check BEFORE INSERT ON stock"),
        "{text}"
    );
    assert!(editor.read_with(cx, |e, _| e.is_read_only()));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0089 AK-32 (v17, OBS-0120 X2): a trigger a script creates shows in the open tree once the
/// script ran, without Refresh; the open trigger folder stays open.
#[gpui::test]
fn objects_a_script_creates_appear_in_the_tree(cx: &mut TestAppContext) {
    let Some(root) = workspace("run") else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    std::fs::write(
        root.join("query.sql"),
        "CREATE TRIGGER stock_gone AFTER DELETE ON stock BEGIN SELECT 1; END",
    )
    .unwrap();
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    let view = bench
        .read_with(cx, |b, _| b.database_view())
        .expect("the Database window is open");
    let listed = |v: &ferris_idea_workbench::DatabaseView, name: &str| {
        v.visible().iter().any(|(_, n, _)| n == name)
    };
    view.update(cx, |v, cx| {
        v.expand(ferris_idea_workbench::db_tree_model::Node::Conn(0), cx)
    });
    wait(&view, cx, "shop lists its triggers", |v| {
        listed(v, "triggers")
    });
    view.update(cx, |v, cx| {
        v.expand(
            ferris_idea_workbench::db_tree_model::Node::Objects(0, "trigger".into()),
            cx,
        )
    });
    wait(&view, cx, "the trigger folder is open", |v| {
        listed(v, "stock_check")
    });
    assert!(view.read_with(cx, |v, _| !listed(v, "stock_gone")));
    crate::db_refactor_retest::open_with(&bench, cx, &root.join("query.sql"), "shop");
    cx.simulate_keystrokes("cmd-enter");
    wait(&view, cx, "the new trigger is listed after the run", |v| {
        listed(v, "stock_gone") && listed(v, "stock_check")
    });
    let _ = std::fs::remove_dir_all(&root);
}

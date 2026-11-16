//! Window check of hand tests 37 and 121 (`SPEC-0110` v3 and v4, D13, D44), headless on SQLite:
//! Compare Schemas asks for the target also when there is only one, and the data-loss question of
//! Include Destructive Changes is brought to the front, takes the keyboard from the editor and
//! answers to Escape and Enter, only while it is on screen.

#![cfg(target_os = "macos")]

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::WorkbenchView;

const QUERY: &str = "select * from customers;\n";

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// A workspace with two SQLite connections: `shop`, and `copy` with a table and a view on it that
/// `shop` does not have. `None` without the `sqlite3` program.
fn workspace() -> Option<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("idecraft-ht121-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let (shop, copy) = (root.join("shop.db"), root.join("copy.db"));
    let made = sqlite(
        &shop,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
    ) && sqlite(
        &copy,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE extra (id INTEGER); CREATE VIEW extra_v AS SELECT id FROM extra;",
    );
    if !made {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".idecraft/connections.toml"),
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

/// Lets the comparison, which reads both databases in the background, finish.
fn settle(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) {
    for _ in 0..200 {
        cx.run_until_parked();
        let done = bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| {
                let v = v.read(cx);
                !v.running() && v.schema_diff().is_some()
            })
        });
        if done {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("the comparison did not finish");
}

#[gpui::test]
fn the_data_loss_question_takes_the_keyboard(cx: &mut TestAppContext) {
    let Some(root) = workspace() else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    cx.executor().allow_parking();
    let file = root.join("query.sql");
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.toggle_database(window, cx);
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        let view = b.database_view().expect("the Database window is open");
        view.update(cx, |v, cx| {
            let row = v
                .visible()
                .iter()
                .position(|(_, name, _)| name == "shop")
                .expect("shop is listed");
            v.select(row, cx);
        });
        b.focus_editor(window, cx);
        b.compare_schemas(window, cx);
    });
    cx.run_until_parked();

    // D13 (hand test 37): the only candidate is offered with where it points, nothing is compared.
    let (items, compared) = bench.read_with(cx, |b, cx| {
        (
            b.compare_popup()
                .map(|p| p.read(cx).items().to_vec())
                .unwrap_or_default(),
            b.services_view()
                .is_some_and(|v| v.read(cx).schema_diff().is_some()),
        )
    });
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].starts_with("copy · "), "{items:?}");
    assert!(!compared, "nothing is compared before the target is chosen");
    cx.simulate_keystrokes("enter");
    settle(&bench, cx);
    let services = bench
        .read_with(cx, |b, _| b.services_view())
        .expect("the Services window is open");
    let included = |cx: &mut VisualTestContext| {
        services.read_with(cx, |v, _| {
            let c = v
                .schema_diff()
                .and_then(|d| d.choice.clone())
                .expect("the statements are listed");
            c.statements
                .iter()
                .zip(&c.selected)
                .filter(|(s, on)| s.destructive.is_some() && **on)
                .count()
        })
    };
    assert_eq!(included(cx), 0, "destructive statements start unchecked");

    // D44 (hand test 121): the action is called from the editor with another tab in front, as
    // from Search Everywhere.
    let ask = |cx: &mut VisualTestContext| {
        services.update(cx, |v, cx| v.show_output(cx));
        bench.update_in(cx, |b, window, cx| {
            b.focus_editor(window, cx);
            b.menu_action("Sql.IncludeDestructiveChanges", window, cx);
        });
        cx.run_until_parked();
        let (tab, asking) =
            services.read_with(cx, |v, _| (v.selected_tab(), v.confirming_migration()));
        assert_eq!(tab, "Schema Diff", "the question is brought to the front");
        assert!(asking);
        let focused = cx.update(|window, cx| services.read(cx).focus().is_focused(window));
        assert!(focused, "the Services window has the keyboard");
    };
    let text = |cx: &mut VisualTestContext| bench.read_with(cx, |b, cx| b.active_text(cx));

    ask(cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!services.read_with(cx, |v, _| v.confirming_migration()));
    assert_eq!(included(cx), 0, "Escape cancels");

    // A question that is not on screen is not answered: Enter on the Output tab leaves it open.
    ask(cx);
    services.update(cx, |v, cx| v.show_output(cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(services.read_with(cx, |v, _| v.confirming_migration()));
    assert_eq!(included(cx), 0, "Enter on another tab includes nothing");

    ask(cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!services.read_with(cx, |v, _| v.confirming_migration()));
    assert_eq!(included(cx), 2, "Enter includes DROP VIEW and DROP TABLE");
    assert_eq!(
        text(cx).as_deref(),
        Some(QUERY),
        "no key reached the editor"
    );
    let script = bench
        .read_with(cx, |b, _| {
            b.migration_script().map(std::path::Path::to_path_buf)
        })
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    assert!(
        script.contains("DROP VIEW") && !script.contains("-- Left out"),
        "{script}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

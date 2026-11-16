//! Retest of hand test 68 of F-060 (Oracle through the Instant Client, `SPEC-0142`) in a window of
//! the test harness, with keys, without the screen and without the keychain, against the Oracle
//! Free of the reference environment (`IDECRAFT_TEST_ORACLE` with its password file; skipped
//! without): Test Connection names the version and the encryption, the tree shows the table with
//! its key and index, a script with a table, a PL/SQL block, `COMMIT` and `ROLLBACK` runs, an error
//! names `ORA-…` and its place, Explain Plan shows a tree, and the same connection read-only refuses
//! DML and blocks before sending them.

#![cfg(target_os = "macos")]

use std::path::Path;

use gpui::{Entity, VisualTestContext};
use idecraft_workbench::WorkbenchView;

use crate::db_refactor_retest::{open_with, reference, until, window};

fn output(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.services_view()
            .map(|v| v.read(cx).output_lines().join("\n"))
            .unwrap_or_default()
    })
}

fn idle(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, cx| {
        b.services_view().is_some_and(|v| !v.read(cx).running())
    })
}

/// Selects everything in the editor and runs it (⌘A, ⌘↩); waits until the run ended.
fn run_all(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-a cmd-enter");
    for _ in 0..10 {
        cx.run_until_parked();
    }
    until(cx, "the script ran", |cx| idle(bench, cx));
}

/// Replaces the editor's text with `sql`.
fn set_text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, sql: &str) {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| e.set_text(sql, cx));
    cx.run_until_parked();
}

/// Hand test 68 (SPEC-0142).
#[gpui::test]
fn hand_test_68_oracle_through_the_instant_client(cx: &mut gpui::TestAppContext) {
    let Some((mut profile, secret)) = reference("IDECRAFT_TEST_ORACLE", "ora") else {
        eprintln!("skipped: IDECRAFT_TEST_ORACLE or its password file is not set");
        return;
    };
    // Consoles of this connection start in manual commit, so COMMIT and ROLLBACK decide.
    profile.options.auto_commit = false;
    let mut read_only = profile.clone();
    read_only.id = "ora_ro".into();
    read_only.name = "ora_ro".into();
    read_only.read_only = true;
    let root = std::env::temp_dir().join(format!("idecraft-dbdialect-68-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    idecraft_database::save_profiles(&root, &[profile.clone(), read_only]).unwrap();
    let file = root.join("ora.sql");
    std::fs::write(&file, "select 1 from dual;\n").unwrap();
    let (bench, cx) = window(&root, cx);
    open_with(&bench, cx, &file, "ora");

    // Test Connection (F4 on the connection, then Test): version and encryption.
    let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    cx.update(|window, cx| window.focus(&db.read(cx).focus()));
    cx.simulate_keystrokes("f4");
    until(cx, "the connection dialog", |cx| {
        bench.read_with(cx, |b, _| b.connection_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.connection_dialog()).unwrap();
    dialog.update(cx, |d, cx| d.test(cx));
    until(cx, "the test result", |cx| {
        dialog.read_with(cx, |d, _| !d.test_details().is_empty())
    });
    let result = dialog
        .read_with(cx, |d, _| d.test_text())
        .unwrap_or_default();
    assert!(result.contains("Oracle"), "{result}");
    assert!(
        result.to_lowercase().contains("encrypt") || result.to_lowercase().contains("tls"),
        "the encryption is named: {result}"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // A script with a table, a PL/SQL block, COMMIT and ROLLBACK.
    let table = format!("HT68_ITEM_{}", std::process::id());
    let script = format!(
        "create table {table} (id number primary key, name varchar2(20) not null);\n\
         create index {table}_NAME_IX on {table}(name);\n\
         insert into {table} values (1, 'a');\n\
         commit;\n\
         begin\n  insert into {table} values (2, 'b');\nend;\n/\n\
         rollback;\n\
         select count(*) as n from {table};\n"
    );
    set_text(&bench, cx, &script);
    // The connection starts its consoles in Manual, shown before the first run.
    assert_eq!(bench.read_with(cx, |b, cx| b.console_mode(cx)), (true, 0));
    run_all(&bench, cx);
    let out = output(&bench, cx);
    assert!(!out.contains("ORA-"), "{out}");
    let rows = bench.read_with(cx, |b, cx| {
        b.services_view()
            .map(|v| v.read(cx).shown_rows())
            .unwrap_or_default()
    });
    assert_eq!(
        rows,
        [vec!["1".to_owned()]],
        "the block was rolled back: {out}"
    );

    // The tree shows the table with its key and index.
    db.update(cx, |v, cx| v.refresh_connection("ora", cx));
    until(cx, "the table is listed", |cx| {
        db.update(cx, |v, cx| {
            v.expand_all_tables(cx);
            let rows = v.visible();
            rows.iter().any(|(_, n, _)| n.eq_ignore_ascii_case(&table))
                && rows
                    .iter()
                    .any(|(_, n, _)| n.eq_ignore_ascii_case(&format!("{table}_NAME_IX")))
        })
    });

    // An error names ORA-… and its place.
    set_text(&bench, cx, "select nosuch from dual;\n");
    run_all(&bench, cx);
    let out = output(&bench, cx);
    assert!(out.contains("ORA-00904"), "{out}");
    assert!(
        out.lines()
            .any(|l| l.contains("ORA-00904") && l.contains("(at column 8)")),
        "the error names its place: {out}"
    );

    // Explain Plan shows the plan as a tree.
    set_text(
        &bench,
        cx,
        &format!("select name from {table} where id = 1;\n"),
    );
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        b.menu_action("Sql.ExplainPlan", window, cx);
    });
    until(cx, "the plan", |cx| {
        bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| {
                let v = v.read(cx);
                !v.running() && v.plan().is_some()
            })
        })
    });
    let plan = bench
        .read_with(cx, |b, cx| {
            b.services_view().and_then(|v| v.read(cx).plan().cloned())
        })
        .unwrap();
    assert!(
        plan.nodes.iter().any(|n| !n.children.is_empty()),
        "a tree: {}",
        plan.raw
    );

    // Read-only: DML and a block are refused before they are sent.
    open_with(&bench, cx, &file, "ora_ro");
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    let _ = idecraft_database::bind_file(&root, &file, Some("ora_ro"));
    for refused in [
        format!("insert into {table} values (3, 'c');\n"),
        format!("begin\n  delete from {table};\nend;\n/\n"),
    ] {
        editor.update(cx, |e, cx| e.set_text(&refused, cx));
        run_all(&bench, cx);
        let out = output(&bench, cx);
        assert!(out.to_lowercase().contains("read-only"), "{refused}: {out}");
    }
    let check =
        crate::db_refactor_retest::pg_run(&profile, &format!("select count(*) from {table}"));
    assert_eq!(check, [vec![Some("1".to_owned())]], "nothing was sent");

    crate::db_refactor_retest::pg_run(&profile, &format!("drop table {table} purge"));
    drop(secret);
    let _ = std::fs::remove_dir_all(Path::new(&root));
}

/// SPEC-0093 v7 behaviour 10 (SPEC-0108 v4 behaviour 10), found by the retest of hand test 68: a
/// connection with Auto-commit off starts its consoles in Manual, so ROLLBACK takes back what the
/// console ran; switched to Auto by hand, the console stays Auto. On SQLite, always.
#[gpui::test]
fn consoles_of_a_connection_without_auto_commit_start_in_manual(cx: &mut gpui::TestAppContext) {
    let has_sqlite = std::process::Command::new("sqlite3")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_sqlite {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-dbdialect-tx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let root = root.canonicalize().unwrap();
    let db = root.join("shop.db");
    assert!(
        std::process::Command::new("sqlite3")
            .arg(&db)
            .arg("create table t (id integer primary key);")
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        root.join(".idecraft/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{db}\"\n\n\
             [connection.options]\nauto_commit = false\n\n\
             [[connection]]\nid = \"shop_ro\"\nname = \"shop_ro\"\ndialect = \"sqlite\"\npath = \"{db}\"\n\
             read_only = true\n\n\
             [connection.options]\nauto_commit = false\n",
            db = db.display()
        ),
    )
    .unwrap();
    let file = root.join("tx.sql");
    std::fs::write(&file, "").unwrap();
    let (bench, cx) = window(&root, cx);
    open_with(&bench, cx, &file, "shop");
    let count = |cx: &mut VisualTestContext| {
        set_text(&bench, cx, "select count(*) from t;\n");
        run_all(&bench, cx);
        bench.read_with(cx, |b, cx| {
            b.services_view()
                .map(|v| v.read(cx).shown_rows())
                .unwrap_or_default()
        })
    };

    assert_eq!(bench.read_with(cx, |b, cx| b.console_mode(cx)), (true, 0));
    set_text(&bench, cx, "insert into t values (1);\nrollback;\n");
    run_all(&bench, cx);
    assert_eq!(count(cx), [vec!["0".to_owned()]], "{}", output(&bench, cx));

    // Switched to Auto, it commits each statement and stays Auto.
    bench.update_in(cx, |b, window, cx| b.set_console_manual(false, window, cx));
    assert_eq!(bench.read_with(cx, |b, cx| b.console_mode(cx)), (false, 0));
    set_text(&bench, cx, "insert into t values (2);\nrollback;\n");
    run_all(&bench, cx);
    assert_eq!(bench.read_with(cx, |b, cx| b.console_mode(cx)), (false, 0));
    assert_eq!(count(cx), [vec!["1".to_owned()]], "{}", output(&bench, cx));

    // Manual again with a statement in its transaction; then the file's data source becomes the
    // read-only connection: the old session is left, rolled back, and the insert is refused.
    bench.update_in(cx, |b, window, cx| b.set_console_manual(true, window, cx));
    set_text(&bench, cx, "insert into t values (3);\n");
    run_all(&bench, cx);
    assert_eq!(
        bench.read_with(cx, |b, _| b.console_transaction()),
        (true, 1)
    );
    idecraft_database::bind_file(&root, &file, Some("shop_ro")).unwrap();
    set_text(&bench, cx, "insert into t values (4);\n");
    run_all(&bench, cx);
    let out = output(&bench, cx);
    assert!(
        out.contains("rolled back (1 statement(s)): the data source is now shop_ro"),
        "{out}"
    );
    // SQLite opens the file of a read-only connection read-only.
    assert!(out.to_lowercase().contains("readonly"), "{out}");
    assert_eq!(count(cx), [vec!["1".to_owned()]], "{out}");
    let _ = std::fs::remove_dir_all(&root);
}

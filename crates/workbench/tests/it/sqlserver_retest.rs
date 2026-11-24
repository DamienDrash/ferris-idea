//! Retest of hand test 43 of F-060 (SQL Server, `SPEC-0116`) in a window of the test harness, with
//! keys, without the screen and without the keychain, against the SQL Server of the reference
//! environment (`FERRIS_IDEA_TEST_SQLSERVER` with its password file, `mssql.sh start` and `init`;
//! skipped without): Test Connection, the tree with tables and keys, a script whose `GO` line
//! separates two batches with the error of the second one and its line, a cell of a result changed
//! and submitted, Explain Plan with costs and the whole index name in its column (the side finding
//! of hand test 43: `PK__wa…` was cut), Explain Analyze with measured rows.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

/// The copy of the test password in the test secrets folder, removed when dropped (also when the
/// test fails).
struct SecretFile(PathBuf);

impl Drop for SecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The server of `FERRIS_IDEA_TEST_SQLSERVER` as connection `mssql`, its password copied from the
/// file named by `FERRIS_IDEA_TEST_SQLSERVER_PASSWORD_FILE` into this process's test secrets folder
/// (0700, the file 0600): the window never asks the keychain.
fn reference() -> Option<(ferris_idea_database::ConnectionProfile, SecretFile)> {
    use ferris_idea_database::SecretStore;
    let url = ferris_idea_core::product_var("FERRIS_IDEA_TEST_SQLSERVER").ok()?;
    let file = ferris_idea_core::product_var("FERRIS_IDEA_TEST_SQLSERVER_PASSWORD_FILE").ok()?;
    let password = std::fs::read_to_string(file).ok()?;
    let base =
        std::env::temp_dir().join(format!("ferris-idea-mssql-secrets-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let dir = ferris_idea_database::test_store::use_dir(base);
    let account = format!("connection:mssql-{}", std::process::id());
    let reference = ferris_idea_database::SecretRef {
        service: "ferris-idea".into(),
        account: account.clone(),
    };
    ferris_idea_database::test_store::DirStore::new(&dir)
        .set(
            &reference,
            &ferris_idea_database::Secret::new(password.trim_end()),
        )
        .unwrap();
    let mut profile = ferris_idea_database::ConnectionProfile::from_url("mssql", &url).unwrap();
    profile.id = "mssql".into();
    profile.secret = Some(reference);
    Some((profile, SecretFile(dir.join("ferris-idea").join(account))))
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..800 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("timed out: {what}");
}

fn output(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.services_view()
            .map(|v| v.read(cx).output_lines().join("\n"))
            .unwrap_or_default()
    })
}

/// Replaces the editor's text with `sql`, selects it all and runs it (⌘A, ⌘↩).
fn run(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, sql: &str) {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| e.set_text(sql, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-a cmd-enter");
    for _ in 0..10 {
        cx.run_until_parked();
    }
    until(cx, "the run ended", |cx| {
        bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| !v.read(cx).running())
        })
    });
}

fn plan_of(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    analyze: bool,
) -> ferris_idea_database::Plan {
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        b.menu_action(
            if analyze {
                "Sql.ExplainAnalyze"
            } else {
                "Sql.ExplainPlan"
            },
            window,
            cx,
        );
    });
    for _ in 0..5 {
        cx.run_until_parked();
    }
    until(cx, "the plan", |cx| {
        bench.read_with(cx, |b, cx| {
            b.services_view().is_some_and(|v| {
                let v = v.read(cx);
                !v.running() && v.plan().is_some_and(|p| p.analyzed == analyze)
            })
        })
    });
    bench
        .read_with(cx, |b, cx| {
            b.services_view().and_then(|v| v.read(cx).plan().cloned())
        })
        .unwrap()
}

/// Hand test 43 (SPEC-0116), the SQL Server part.
#[gpui::test]
fn hand_test_43_sql_server(cx: &mut TestAppContext) {
    let Some((profile, secret)) = reference() else {
        eprintln!("skipped: FERRIS_IDEA_TEST_SQLSERVER or its password file is not set");
        return;
    };
    let root = std::env::temp_dir().join(format!("ferris-idea-mssql-43-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    ferris_idea_database::save_profiles(&root, std::slice::from_ref(&profile)).unwrap();
    let file = root.join("ms.sql");
    std::fs::write(&file, "select 1;\n").unwrap();
    cx.executor().allow_parking();
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
    let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    db.update(cx, |v, cx| assert!(v.select_label("mssql", cx)));

    // Test Connection (F4 on the connection, Test).
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
    assert!(result.contains("SQL Server"), "{result}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // The tree with the tables and their keys.
    cx.update(|window, cx| window.focus(&db.read(cx).focus()));
    cx.simulate_keystrokes("right");
    until(cx, "the tables and keys are listed", |cx| {
        db.update(cx, |v, cx| {
            v.expand_all_tables(cx);
            let rows = v.visible();
            ["warehouse", "item", "movement"]
                .iter()
                .all(|t| rows.iter().any(|(_, n, _)| n == t))
                && rows.iter().any(|(_, n, _)| n.starts_with("PK__"))
        })
    });

    // GO separates the batches; the error of the second names its line.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    run(
        &bench,
        cx,
        "select count(*) as n from warehouse\nGO\nselect nosuch from warehouse\n",
    );
    let out = output(&bench, cx);
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("select count(*) as n from warehouse — 1 row")),
        "{out}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("select nosuch from warehouse —") && l.contains("207")),
        "{out}"
    );

    // A cell of the result changed and submitted; the seed's value is put back afterwards.
    let seed = "update warehouse set city = N'Leipzig' where id = 1\n";
    run(&bench, cx, seed);
    run(&bench, cx, "select id, city from warehouse order by id\n");
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    assert_eq!(
        services
            .read_with(cx, |v, _| v.cell_text_at(0, 1))
            .as_deref(),
        Some("Leipzig")
    );
    until(cx, "the result is editable", |cx| {
        services.read_with(cx, |v, _| v.shown_edits().is_some())
    });
    let changed = format!("Hand test 43 · {}", std::process::id());
    cx.update(|window, cx| window.focus(&services.read(cx).focus()));
    services.update_in(cx, |v, window, cx| {
        v.select_cell(0, 1, cx);
        v.start_edit(window, cx);
    });
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(&changed);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        services.read_with(cx, |v, _| v.shown_edits().map(|e| e.count())),
        Some(1)
    );
    services.update(cx, |v, cx| v.submit(cx));
    until(cx, "the change is committed", |cx| {
        services.read_with(cx, |v, _| v.note() == Some("1 change(s) committed"))
    });
    run(&bench, cx, "select city from warehouse where id = 1\n");
    let now = services.read_with(cx, |v, _| v.cell_text_at(0, 0));
    run(&bench, cx, seed);
    assert_eq!(now, Some(changed), "the change was saved");

    // Explain Plan: the steps with their costs, the index name whole in its column.
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| {
        e.set_text("select city from warehouse where id = 1\n", cx)
    });
    let plan = plan_of(&bench, cx, false);
    let rows = ferris_idea_workbench::db_compare_model::plan_rows(&plan);
    assert!(rows.iter().any(|r| !r.estimate.is_empty()), "{}", plan.raw);
    let seek = rows
        .iter()
        .find(|r| r.object.contains(" using PK__"))
        .unwrap_or_else(|| panic!("the key's index: {rows:?}"));
    let index = seek.object.split(" using ").nth(1).unwrap().to_owned();
    run(
        &bench,
        cx,
        "select name from sys.indexes where object_id = object_id('warehouse') and is_primary_key = 1\n",
    );
    assert_eq!(
        services.read_with(cx, |v, _| v.cell_text_at(0, 0)),
        Some(index.clone()),
        "the plan names the whole index"
    );
    let width = ferris_idea_workbench::db_compare_model::plan_object_width(&rows, 7.8);
    let measured = cx.update(|window, _| {
        let font = gpui::font(ferris_idea_workbench::fonts::editor_family());
        let run = gpui::TextRun {
            len: seek.object.len(),
            font,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        f32::from(
            window
                .text_system()
                .shape_line(
                    seek.object.clone().into(),
                    px(ferris_idea_workbench::fonts::EDITOR_FONT_SIZE),
                    &[run],
                    None,
                )
                .width,
        )
    });
    assert!(
        measured + 16. <= width + 0.5,
        "`{}` is {measured} px, its column {width} px",
        seek.object
    );

    // Explain Analyze: measured rows.
    editor.update(cx, |e, cx| {
        e.set_text("select city from warehouse where id = 1\n", cx)
    });
    let analyzed = plan_of(&bench, cx, true);
    let rows = ferris_idea_workbench::db_compare_model::plan_rows(&analyzed);
    assert!(
        rows.iter().any(|r| r.actual.contains("rows")),
        "{}",
        analyzed.raw
    );

    drop(secret);
    let _ = std::fs::remove_dir_all(Path::new(&root));
}

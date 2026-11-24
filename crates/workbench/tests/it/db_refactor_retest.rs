//! Retest of the hand tests of F-057 (database refactorings) in a window of the test harness, with
//! keys, without the screen and without the keychain:
//!
//! - 55 (SPEC-0128): ⇧F6 on a column and a table in the Database window, the preview, Enter renames
//!   in the database and in the SQL files, ⌘Z takes the open file's change back in one step.
//! - 78 (SPEC-0152): ⇧F6 on an aliased column in the SQL editor with the index named after it,
//!   whose name in the file follows; ⌥↩ "Expand column list" on `*`.
//! - 122 (SPEC-0019 v2/v3, D45): the column's own definition and check in `CREATE TABLE` of a file
//!   are renamed with it.
//! - 90 (SPEC-0164, D32): on PostgreSQL (`FERRIS_IDEA_TEST_PG` with its password file; skipped
//!   without): ⇧F6 on a view, ⌘F6 changes a column's type in the database, a refused change is told
//!   in the dialog and the status bar and the dialog stays answerable.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

pub(crate) fn sqlite(db: &Path, sql: &str) -> String {
    let out = std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .output()
        .expect("sqlite3 runs");
    assert!(out.status.success(), "{sql}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub(crate) fn has_sqlite() -> bool {
    std::process::Command::new("sqlite3")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A workspace with one SQLite connection `shop` made by `schema`, and the SQL files `files`.
pub(crate) fn workspace(tag: &str, schema: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-dbrefactor-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let root = root.canonicalize().unwrap();
    let db = root.join("shop.db");
    sqlite(&db, schema);
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            db.display()
        ),
    )
    .unwrap();
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

pub(crate) fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

pub(crate) fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("timed out: {what}");
}

/// Opens `file` in the editor and the Database window with connection `name` selected.
pub(crate) fn open_with(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    file: &Path,
    name: &str,
) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        if b.database_view().is_none() {
            b.toggle_database(window, cx);
        }
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        let view = b.database_view().expect("the Database window is open");
        view.update(cx, |v, cx| {
            assert!(v.select_label(name, cx), "{name} is listed");
        });
        b.focus_editor(window, cx);
    });
}

/// Expands every table of the loaded connections and selects the row `label` in the Database
/// window, which takes the keyboard.
pub(crate) fn select_in_tree(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    label: &str,
) {
    let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
    // The connection's tables are read in the background when it is expanded.
    cx.update(|window, cx| window.focus(&db.read(cx).focus()));
    cx.simulate_keystrokes("right");
    until(cx, "the tables are read", |cx| {
        db.update(cx, |v, cx| {
            v.expand_all_tables(cx);
            v.visible().iter().any(|(_, n, _)| n == label)
        })
    });
    db.update(cx, |v, cx| assert!(v.select_label(label, cx)));
    cx.update(|window, cx| window.focus(&db.read(cx).focus()));
    cx.run_until_parked();
}

/// Types `name` over the dialog's field and presses Enter; waits until the dialog closed.
pub(crate) fn rename_to(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, name: &str) {
    until(cx, "the rename dialog opens", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.db_rename_dialog()).unwrap();
    let focused = cx.update(|window, cx| dialog.read(cx).focus(cx).is_focused(window));
    assert!(focused, "the name field has the keyboard");
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(name);
    cx.run_until_parked();
    until(cx, "the preview follows the name", |cx| {
        dialog.read_with(cx, |d, cx| d.new_name(cx) == name && d.preview().is_ok())
    });
    cx.simulate_keystrokes("enter");
    until(cx, "the rename ran and the dialog closed", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_none())
    });
}

pub(crate) fn text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

/// Hand test 55 (SPEC-0128).
#[gpui::test]
fn hand_test_55_rename_in_the_database_window(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let sql = "select menge from ht_rename;\nupdate ht_rename set menge = menge + 1;\nselect r.menge from ht_rename r;\n";
    let root = workspace(
        "55",
        "create table ht_rename (id integer primary key, menge integer not null);",
        &[("sql/rename.sql", sql)],
    );
    let (bench, cx) = window(&root, cx);
    let file = root.join("sql/rename.sql");
    open_with(&bench, cx, &file, "shop");

    // ⇧F6 on the column: the preview names the statement and the four places.
    select_in_tree(&bench, cx, "menge");
    cx.simulate_keystrokes("shift-f6");
    until(cx, "the dialog", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.db_rename_dialog()).unwrap();
    dialog.update(cx, |d, cx| d.set_new_name("anzahl", cx));
    let (ddl, edits) = dialog.read_with(cx, |d, _| {
        let p = d.preview().expect("a preview");
        (p.ddl.clone(), p.edit_count())
    });
    assert!(
        ddl.contains("RENAME COLUMN") && ddl.contains("anzahl"),
        "{ddl}"
    );
    assert_eq!(edits, 4, "four places in sql/rename.sql");
    rename_to(&bench, cx, "anzahl");
    let schema = sqlite(&root.join("shop.db"), ".schema ht_rename");
    assert!(
        schema.contains("anzahl") && !schema.contains("menge"),
        "{schema}"
    );
    assert_eq!(text(&bench, cx), sql.replace("menge", "anzahl"));
    until(cx, "the tree follows", |cx| {
        let db = bench.read_with(cx, |b, _| b.database_view()).unwrap();
        db.update(cx, |v, cx| {
            v.expand_all_tables(cx);
            v.visible().iter().any(|(_, n, _)| n == "anzahl")
        })
    });

    // ⌘Z in the file takes its change back in one step; the database keeps the new name, and the
    // popup that says so is closed with Escape (SPEC-0164 v5 behaviour 16).
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-z");
    cx.run_until_parked();
    assert_eq!(text(&bench, cx), sql);
    assert!(bench.read_with(cx, |b, _| b.sql_popup().is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-shift-z");
    cx.run_until_parked();

    // ⇧F6 on the table.
    select_in_tree(&bench, cx, "ht_rename");
    cx.simulate_keystrokes("shift-f6");
    rename_to(&bench, cx, "ht_bestand");
    let tables = sqlite(&root.join("shop.db"), ".tables");
    assert!(
        tables.contains("ht_bestand") && !tables.contains("ht_rename"),
        "{tables}"
    );
    assert_eq!(
        text(&bench, cx),
        sql.replace("menge", "anzahl")
            .replace("ht_rename", "ht_bestand")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 78 (SPEC-0152) and the index name of hand test 78 and D45 of hand test 122.
#[gpui::test]
fn hand_test_78_and_122_rename_in_the_editor_and_expand_columns(cx: &mut TestAppContext) {
    if !has_sqlite() {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    let schema = "create table ht_a (id integer primary key, name text not null);\n\
         create table ht_b (id integer primary key, a_id integer references ht_a(id), \
         menge integer not null check (menge >= 0));\n\
         create index ht_b_menge_idx on ht_b(menge);\n";
    let joins = "select b.menge, a.name from ht_b b join ht_a a on a.id = b.a_id;\n\
         select * from ht_a join ht_b on ht_b.a_id = ht_a.id;\n";
    let root = workspace(
        "78",
        schema,
        &[("joins.sql", joins), ("schema.sql", schema)],
    );
    let (bench, cx) = window(&root, cx);
    let file = root.join("joins.sql");
    open_with(&bench, cx, &file, "shop");

    // ⇧F6 on `menge` of `b.menge`.
    let at = joins.find("b.menge").unwrap() + 3;
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    cx.simulate_keystrokes("shift-f6");
    until(cx, "the dialog", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.db_rename_dialog()).unwrap();
    dialog.update(cx, |d, cx| d.set_new_name("anzahl", cx));
    // SQLite cannot rename an index: the one named after the column is listed with the reason and
    // not chosen (SPEC-0019 v2 behaviour 9; renamed on PostgreSQL in the live test below).
    let derived = dialog.read_with(cx, |d, _| {
        d.preview().map(|p| p.derived.clone()).unwrap_or_default()
    });
    let index = derived
        .iter()
        .find(|(r, _)| r.old == "ht_b_menge_idx")
        .unwrap_or_else(|| panic!("the index is listed: {derived:?}"));
    assert_eq!(index.0.new, "ht_b_anzahl_idx");
    assert!(
        index
            .0
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("SQLite"))
    );
    assert!(!index.1, "not chosen");
    rename_to(&bench, cx, "anzahl");
    assert!(text(&bench, cx).starts_with("select b.anzahl, a.name from ht_b b"));
    let db = sqlite(&root.join("shop.db"), ".schema ht_b");
    assert!(
        db.contains("anzahl") && db.contains("ht_b_menge_idx"),
        "{db}"
    );
    // D45 (hand test 122): the file's CREATE TABLE renames the column's definition and its check;
    // its CREATE INDEX keeps the name the database kept.
    let file_schema = std::fs::read_to_string(root.join("schema.sql")).unwrap();
    assert!(
        file_schema.contains("anzahl integer not null check (anzahl >= 0)"),
        "{file_schema}"
    );
    assert!(
        file_schema.contains("create index ht_b_menge_idx on ht_b(anzahl)"),
        "{file_schema}"
    );

    // ⌥↩ on `*`: Expand column list gives the qualified columns.
    let text_now = text(&bench, cx);
    let star = text_now.find("select *").unwrap() + 7;
    editor.update(cx, |e, cx| e.select_range(star..star, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("alt-enter");
    until(cx, "the context actions", |cx| {
        bench.read_with(cx, |b, cx| {
            b.intentions_popup().is_some_and(|p| {
                p.read(cx)
                    .titles()
                    .iter()
                    .any(|t| t == "Expand column list")
            })
        })
    });
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    let titles = popup.read_with(cx, |p, _| p.titles());
    let i = titles
        .iter()
        .position(|t| t == "Expand column list")
        .unwrap();
    for _ in 0..i {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    until(cx, "the list is expanded", |cx| {
        !text(&bench, cx).contains("select *")
    });
    let expanded = text(&bench, cx);
    assert!(
        expanded.contains(
            "select ht_a.id, ht_a.name, ht_b.id, ht_b.a_id, ht_b.anzahl from ht_a join ht_b"
        ),
        "{expanded}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The copy of a test password in the test secrets folder: removed when dropped, also when the
/// test fails before its end.
pub(crate) struct SecretFile(PathBuf);

impl Drop for SecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The reference server of the URL in `url_var` (e.g. `FERRIS_IDEA_TEST_PG`) as connection `id`, with
/// its password read from the file named by `<url_var>_PASSWORD_FILE` into this process's test
/// secrets folder (0700, a file 0600, removed when the returned guard is dropped): the window
/// never asks the keychain. `None` without them.
pub(crate) fn reference(
    url_var: &str,
    id: &str,
) -> Option<(ferris_idea_database::ConnectionProfile, SecretFile)> {
    use ferris_idea_database::SecretStore;
    let url = std::env::var(url_var).ok()?;
    let file = std::env::var(format!("{url_var}_PASSWORD_FILE")).ok()?;
    let password = std::fs::read_to_string(file).ok()?;
    let base = std::env::temp_dir().join(format!(
        "ferris-idea-dbrefactor-secrets-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&base).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let dir = ferris_idea_database::test_store::use_dir(base);
    let account = format!("connection:{id}-{}", std::process::id());
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
    let mut profile = ferris_idea_database::ConnectionProfile::from_url(id, &url).unwrap();
    profile.id = id.into();
    profile.secret = Some(reference);
    Some((profile, SecretFile(dir.join("ferris-idea").join(account))))
}

/// Drops the PostgreSQL schemas `<prefix><pid>` that runs ended early left behind, before a test
/// makes its own (OBS-0083 L11): only those of processes no longer running, never this run's.
pub(crate) fn pg_drop_leftovers(profile: &ferris_idea_database::ConnectionProfile, prefix: &str) {
    for row in pg_run(profile, "select nspname from pg_namespace") {
        let Some(name) = row.into_iter().next().flatten() else {
            continue;
        };
        let Some(pid) = name
            .strip_prefix(prefix)
            .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        let alive = pid == std::process::id()
            || std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
        if !alive {
            pg_run(profile, &format!("drop schema if exists {name} cascade"));
        }
    }
}

/// Runs `sql` on `profile` outside the window; the rows of the last result.
pub(crate) fn pg_run(
    profile: &ferris_idea_database::ConnectionProfile,
    sql: &str,
) -> Vec<Vec<Option<String>>> {
    let credentials =
        ferris_idea_database::resolve_credentials(profile, &ferris_idea_database::SystemStore)
            .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let s = ferris_idea_database::Session::connect(profile, &credentials)
            .await
            .unwrap();
        let run = s.run_script(sql, ferris_idea_database::OnError::Stop).await;
        let mut rows = Vec::new();
        for o in run.outcomes {
            match o.outcome {
                ferris_idea_database::Outcome::Error(e) => panic!("{e:?} in {sql}"),
                ferris_idea_database::Outcome::ResultSet { rows: r, .. } => rows = r,
                _ => {}
            }
        }
        rows
    })
}

/// Hand test 90 (SPEC-0164, D32) and the derived index name of hand test 78, on PostgreSQL.
#[gpui::test]
fn hand_test_90_views_types_and_refusals_on_postgres(cx: &mut TestAppContext) {
    let Some((mut profile, secret)) = reference("FERRIS_IDEA_TEST_PG", "pg") else {
        eprintln!("skipped: FERRIS_IDEA_TEST_PG or its password file is not set");
        return;
    };
    let scratch = format!("ht90_{}", std::process::id());
    pg_drop_leftovers(&profile, "ht90_");
    pg_run(
        &profile,
        &format!(
            "drop schema if exists {scratch} cascade; create schema {scratch}; \
             set search_path to {scratch}; \
             create table ht_a (id integer primary key, name varchar(20) not null); \
             create table ht_b (id integer primary key, a_id integer, menge integer not null); \
             create index ht_b_menge_idx on ht_b(menge); \
             create view ht_v as select id, name from ht_a"
        ),
    );
    // The window works in the scratch schema.
    profile.options.startup_script = Some(format!("set search_path to {scratch}"));
    let sql = "select name from ht_a;\nselect a_id from ht_b;\nselect * from ht_v;\nselect b.menge from ht_b b;\n";
    let root =
        std::env::temp_dir().join(format!("ferris-idea-dbrefactor-90-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    ferris_idea_database::save_profiles(&root, std::slice::from_ref(&profile)).unwrap();
    std::fs::write(root.join("ht.sql"), sql).unwrap();
    let (bench, cx) = window(&root, cx);
    open_with(&bench, cx, &root.join("ht.sql"), "pg");
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    let caret = |cx: &mut VisualTestContext, at: usize| {
        editor.update(cx, |e, cx| e.select_range(at..at, cx));
        bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    };
    let q = |what: &str| pg_run(&profile, &format!("set search_path to {scratch}; {what}"));
    let one = |rows: Vec<Vec<Option<String>>>| {
        rows.first()
            .and_then(|r| r.first().cloned())
            .flatten()
            .unwrap_or_default()
    };

    // ⇧F6 on the view.
    caret(cx, sql.find("ht_v").unwrap() + 1);
    cx.simulate_keystrokes("shift-f6");
    rename_to(&bench, cx, "ht_view");
    assert_eq!(
        one(q(&format!(
            "select count(*) from pg_views where schemaname = '{scratch}' and viewname = 'ht_view'"
        ))),
        "1"
    );

    // ⌘F6 on a_id: the type changes in the database (D32: before, nothing ran).
    let at = text(&bench, cx).find("a_id").unwrap() + 1;
    caret(cx, at);
    cx.simulate_keystrokes("cmd-f6");
    rename_to(&bench, cx, "bigint");
    assert_eq!(
        one(q(&format!(
            "select data_type from information_schema.columns where table_schema = '{scratch}' \
             and table_name = 'ht_b' and column_name = 'a_id'"
        ))),
        "bigint"
    );

    // ⌘F6 on name, used by the view: refused; the dialog and the status bar say so, Escape closes.
    let at = text(&bench, cx).find("name").unwrap() + 1;
    caret(cx, at);
    cx.simulate_keystrokes("cmd-f6");
    until(cx, "the dialog", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.db_rename_dialog()).unwrap();
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("varchar(50)");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    until(cx, "the refusal is told", |cx| {
        dialog.read_with(cx, |d, _| {
            d.note().is_some_and(|(t, e)| e && t.contains("refused"))
        })
    });
    let status = bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default();
    assert!(status.contains("refused"), "{status}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_none()),
        "Escape closes it"
    );
    assert_eq!(
        one(q(&format!(
            "select character_maximum_length from information_schema.columns where \
             table_schema = '{scratch}' and table_name = 'ht_a' and column_name = 'name'"
        ))),
        "20",
        "nothing changed"
    );

    // Hand test 78 on PostgreSQL: the index named after the column is renamed with it.
    let at = text(&bench, cx).find("b.menge").unwrap() + 3;
    caret(cx, at);
    cx.simulate_keystrokes("shift-f6");
    until(cx, "the dialog", |cx| {
        bench.read_with(cx, |b, _| b.db_rename_dialog().is_some())
    });
    let dialog = bench.read_with(cx, |b, _| b.db_rename_dialog()).unwrap();
    dialog.update(cx, |d, cx| d.set_new_name("anzahl", cx));
    until(cx, "the index is offered and chosen", |cx| {
        dialog.read_with(cx, |d, _| {
            d.preview().is_ok_and(|p| {
                p.chosen_derived()
                    .any(|s| s == "ALTER INDEX ht_b_menge_idx RENAME TO ht_b_anzahl_idx")
            })
        })
    });
    rename_to(&bench, cx, "anzahl");
    assert_eq!(
        one(q(&format!(
            "select indexname from pg_indexes where schemaname = '{scratch}' \
             and tablename = 'ht_b' and indexname like 'ht_b_%_idx'"
        ))),
        "ht_b_anzahl_idx"
    );

    q(&format!("drop schema {scratch} cascade"));
    drop(secret);
    let _ = std::fs::remove_dir_all(&root);
}

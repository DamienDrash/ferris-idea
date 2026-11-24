//! `SPEC-0089` v16 AK-31 (behaviour 28, gap DS1 of OBS-0109), in a window of the test harness: the
//! data sources of the project's `.idea/dataSources.xml` are offered in the Database window until
//! added; Add… only fills the connection dialog, its Save writes the connection.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::{Dialect, WorkbenchView};
use gpui::{TestAppContext, VisualTestContext, px};

const SOURCES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="DataSourceManagerImpl" format="xml" multifile-model="true">
    <data-source source="LOCAL" name="inventory" uuid="5e0d1c2a-0000-4000-8000-000000000001">
      <driver-ref>sqlite.xerial</driver-ref>
      <synchronize>true</synchronize>
      <jdbc-driver>org.sqlite.JDBC</jdbc-driver>
      <jdbc-url>jdbc:sqlite:$PROJECT_DIR$/inventory.sqlite</jdbc-url>
      <working-dir>$ProjectFileDir$</working-dir>
    </data-source>
    <data-source source="LOCAL" name="legacy" uuid="5e0d1c2a-0000-4000-8000-000000000002">
      <driver-ref>oracle.19</driver-ref>
      <jdbc-url>jdbc:oracle:thin:@h:1521/x</jdbc-url>
    </data-source>
  </component>
</project>
"#;

#[gpui::test]
fn idea_data_sources_are_offered_until_added(cx: &mut TestAppContext) {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-idea-sources-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idea")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join(".idea/dataSources.xml"), SOURCES).unwrap();
    std::fs::write(root.join("inventory.sqlite"), b"").unwrap();
    cx.executor().allow_parking();
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(885.)));
    bench.update_in(cx, |b, window, cx| b.toggle_database(window, cx));
    cx.run_until_parked();
    let view = bench
        .read_with(cx, |b, _| b.database_view())
        .expect("the Database window is open");
    let listed = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| {
            v.idea_sources()
                .iter()
                .map(|s| (s.name.clone(), s.offered(), s.reason.clone()))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        listed(cx),
        [
            ("inventory".to_owned(), true, None),
            (
                "legacy".to_owned(),
                false,
                Some("driver `oracle.19` is not supported".to_owned())
            ),
        ]
    );
    let connections = root.join(".ferris-idea/connections.toml");
    assert!(!connections.exists(), "nothing is added on its own");

    // Add… on the one without a reason does nothing; on the offered one it fills the dialog.
    view.update(cx, |v, cx| v.add_idea_source("legacy", cx));
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.connection_dialog()).is_none());
    view.update(cx, |v, cx| v.add_idea_source("inventory", cx));
    cx.run_until_parked();
    let dialog = bench
        .read_with(cx, |b, _| b.connection_dialog())
        .expect("Add… opens the dialog");
    let form = dialog.read_with(cx, |d, cx| d.form(cx));
    assert_eq!(form.name, "inventory");
    assert_eq!(form.dialect, Dialect::Sqlite);
    assert_eq!(form.path, "inventory.sqlite");
    assert!(form.user.is_empty());
    assert!(
        dialog.read_with(cx, |d, _| d.note().is_some_and(|(n, error)| !error
            && n.contains(".idea")
            && !n.contains("user"))),
        "the dialog says where the fields came from, and asks for no user of a SQLite file \
         (SPEC-0049 v10 AK-19)"
    );
    assert!(!connections.exists(), "the dialog saves only on Save");

    // Save writes the connection; the data source is no longer offered.
    dialog.update(cx, |d, cx| d.save(cx));
    for _ in 0..200 {
        cx.run_until_parked();
        if connections.exists() && view.read_with(cx, |v, _| v.idea_sources().len() == 1) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let saved = std::fs::read_to_string(&connections).unwrap();
    assert!(saved.contains("name = \"inventory\""), "{saved}");
    assert!(saved.contains("path = \"inventory.sqlite\""), "{saved}");
    assert_eq!(
        listed(cx).into_iter().map(|l| l.0).collect::<Vec<_>>(),
        ["legacy"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

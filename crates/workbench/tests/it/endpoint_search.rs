//! The endpoint search of Search Everywhere in a window of the test harness (`SPEC-0067` v9
//! behaviour 20): `GotoUrlAction` opens the popup on its Endpoints tab, typing finds a row of the
//! Endpoints model and choosing it opens the handler's declaration.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::everywhere_model::{Filter, GOTO_ENDPOINT, Row, SELECT_ITEM};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-endpoint-search-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    std::fs::write(root.join("notes.txt"), "notes\n").unwrap();
    std::fs::write(
        root.join("src/main/java/demo/OrderApi.java"),
        "package demo;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api/orders\")\nclass OrderApi {\n  @GetMapping\n  String list() { return \"\"; }\n\n  @PostMapping\n  void create() {}\n\n  @GetMapping(\"/{id}\")\n  String get() { return \"\"; }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("app.py"),
        "from fastapi import FastAPI\napp = FastAPI()\n\n@app.get(\"/health\")\ndef health():\n    return {}\n",
    )
    .unwrap();
    // The directory as the workbench names it, so paths compare equal.
    root.canonicalize().unwrap()
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join("notes.txt");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

/// The rows of the open popup as `METHODS path handler`.
fn rows(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        let popup = b.everywhere().expect("the popup");
        popup
            .read(cx)
            .results()
            .rows
            .iter()
            .map(|row| match row {
                Row::Endpoint(e) => e.endpoint.row_text(),
                other => format!("{other:?}"),
            })
            .collect()
    })
}

/// Waits for the background scan and search of the popup (they read files on real threads).
fn settle(bench: &Entity<WorkbenchView>, wanted: usize, cx: &mut VisualTestContext) {
    for _ in 0..400 {
        cx.run_until_parked();
        if rows(bench, cx).len() == wanted {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// SPEC-0067 AK-10 (v9): the action opens the Endpoints tab with the rows of the Endpoints
/// model; a method and a part of the path select the row; choosing it opens the handler's file
/// with the caret on the mapping's line.
#[gpui::test]
fn goto_url_opens_the_endpoints_tab_and_enter_opens_the_handler(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = workspace("open");
    let (bench, cx) = open(&root, cx);
    assert!(bench.read_with(cx, |b, _| b.everywhere().is_none()));

    bench.update_in(cx, |b, window, cx| b.menu_action(GOTO_ENDPOINT, window, cx));
    settle(&bench, 4, cx);
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("the popup");
    assert_eq!(popup.read_with(cx, |v, _| v.filter()), Filter::Endpoints);
    assert_eq!(popup.read_with(cx, |v, _| v.endpoint_count()), Some(4));
    // Every framework of the model, in the tool window's order.
    assert_eq!(
        rows(&bench, cx),
        [
            "GET /api/orders OrderApi.list",
            "POST /api/orders OrderApi.create",
            "GET /api/orders/{id} OrderApi.get",
            "GET /health health",
        ]
    );

    popup.update(cx, |v, cx| v.set_query("post ord", cx));
    settle(&bench, 1, cx);
    assert_eq!(rows(&bench, cx), ["POST /api/orders OrderApi.create"]);
    let footer = popup.read_with(cx, |v, _| v.footer());
    assert!(footer.ends_with("OrderApi.java"), "{footer}");

    // The handler's name finds it too.
    popup.update(cx, |v, cx| v.set_query("health", cx));
    settle(&bench, 1, cx);
    assert_eq!(rows(&bench, cx), ["GET /health health"]);
    popup.update(cx, |v, cx| v.set_query("post ord", cx));
    settle(&bench, 1, cx);
    assert_eq!(rows(&bench, cx), ["POST /api/orders OrderApi.create"]);

    // Choosing the row (Enter in the bundled keymaps) opens the declaration.
    assert!(popup.update(cx, |v, cx| v.press(SELECT_ITEM, cx)));
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.everywhere().is_none()));
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_file()),
        Some(root.join("src/main/java/demo/OrderApi.java"))
    );
    // `@PostMapping` stands on the eleventh line.
    assert_eq!(
        bench.read_with(cx, |b, cx| b.caret_position(cx).map(|(line, _)| line)),
        Some(11)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0067 AK-10 (v9): in a project without endpoints the tab is empty and says nothing is
/// selected; the action in an open popup switches to the tab.
#[gpui::test]
fn a_project_without_endpoints_and_switching_to_the_tab(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-endpoint-search-none-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "notes\n").unwrap();
    let root = root.canonicalize().unwrap();
    let (bench, cx) = open(&root, cx);

    bench.update_in(cx, |b, window, cx| b.menu_action("GotoAction", window, cx));
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("the popup");
    assert_eq!(popup.read_with(cx, |v, _| v.filter()), Filter::Actions);

    bench.update_in(cx, |b, window, cx| b.menu_action(GOTO_ENDPOINT, window, cx));
    for _ in 0..400 {
        cx.run_until_parked();
        if popup.read_with(cx, |v, _| v.endpoint_count()).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(popup.read_with(cx, |v, _| v.filter()), Filter::Endpoints);
    assert_eq!(popup.read_with(cx, |v, _| v.endpoint_count()), Some(0));
    assert!(rows(&bench, cx).is_empty());
    assert!(popup.read_with(cx, |v, _| v.results().selected.is_none()));
    // Choosing with nothing selected does nothing: the popup stays.
    assert!(popup.update(cx, |v, cx| v.press(SELECT_ITEM, cx)));
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.everywhere().is_some()));
    let _ = std::fs::remove_dir_all(&root);
}

//! `SPEC-0072` v20 AK-41 (behaviour 13; gap TC1 of the side-by-side comparison of 2026-11-18):
//! in a window of the test harness with typescript-language-server, Code Completion after `xs.`
//! opens with `at` selected and shows its documentation without a key press; narrowing by typing
//! shows the documentation of the item then selected. The language server pool is on, as in the
//! application (typescript-language-server resolves only items of the list its own process
//! handed out), so this test has a process of its own. Skipped without
//! `typescript-language-server` on `PATH`.
//!
//! The defect was a race (the resolved details changed the key the documentation was kept under),
//! so this window check guards the behaviour with the real server; the order of the two answers
//! is pinned down in `completion_model::tests::the_first_items_documentation_survives_its_details`.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

const MAIN_TS: &str = "const xs = [1, 2, 3];\nxs.\n";

fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(program))
            .find(|f| f.is_file())
    })
}

fn workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-tc1-doc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_TS).unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        "{ \"compilerOptions\": { \"target\": \"es2022\", \"lib\": [\"es2022\"] } }\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    ferris_idea_core::trust::trust_for_process(&root);
    ferris_idea_languages::pool::enable();
    root
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let dir = root.join(".test-settings");
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    cx.executor().allow_parking();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

/// The selected row's label and the documentation beside the list.
fn selected(bench: &Bench, cx: &mut VisualTestContext) -> Option<(String, Option<String>)> {
    bench.read_with(cx, |b, _| {
        let label = b.completion_selected_label()?;
        let (_, doc) = b.completion_extras()?;
        Some((label, doc))
    })
}

/// SPEC-0072 AK-41 (v20, behaviour 13): the first item's documentation is shown at once.
#[gpui::test]
fn the_first_items_documentation_shows_at_once(cx: &mut TestAppContext) {
    if on_path("typescript-language-server").is_none() {
        eprintln!("skipped: typescript-language-server not on PATH");
        return;
    }
    let root = workspace();
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("main.ts"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open");
    editor.update(cx, |e, cx| e.go_to(1, 3, cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CodeCompletion", window, cx)
    });
    until(cx, 60, "the list opens with `at` selected", |cx| {
        selected(&bench, cx).is_some_and(|(l, _)| l == "at")
    });
    // No key press: the documentation of `at` comes by itself.
    until(cx, 30, "the documentation of `at` is shown", |cx| {
        selected(&bench, cx).is_some_and(|(l, d)| l == "at" && d.is_some())
    });
    let (_, doc) = selected(&bench, cx).unwrap();
    assert!(doc.unwrap().to_lowercase().contains("index"));

    // Narrowed by typing: the item then selected shows its documentation too.
    cx.simulate_input("fil");
    until(cx, 10, "the list narrows to `fill` and `filter`", |cx| {
        selected(&bench, cx).is_some_and(|(l, _)| l.starts_with("fil"))
    });
    until(
        cx,
        30,
        "the narrowed selection's documentation is shown",
        |cx| selected(&bench, cx).is_some_and(|(l, d)| l.starts_with("fil") && d.is_some()),
    );
    let _ = std::fs::remove_dir_all(&root);
    ferris_idea_languages::pool::shutdown();
}

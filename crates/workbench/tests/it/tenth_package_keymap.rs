//! The tenth package of actions taken from the keymap (F-018), in a window of the test harness and
//! with keys only: Insert Live Template (⌘J, `SPEC-0072` v14 behaviour 50) with a real
//! rust-analyzer, Next and Previous Template Variable (Tab, ⇧Tab; behaviour 51) and Iterate Recent
//! Files in the Switcher (⌘E, `SPEC-0059` v9.10 behaviour 27h).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_languages::insight::CompletionItem;
use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-tenth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    cx.executor().allow_parking();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
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

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// The text the active editor has selected.
fn selected(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        let e = b.active_editor_entity().unwrap();
        let e = e.read(cx);
        e.selection_range()
            .map(|r| {
                e.state
                    .buffer
                    .text()
                    .chars()
                    .skip(r.start)
                    .take(r.len())
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// SPEC-0072 AK-33 (v14): ⌘J in a file without a language server says that no template applies;
/// in a Rust file rust-analyzer's templates are listed alone, Enter inserts one, Tab goes to its
/// end and ⇧Tab back to its name (skipped without rust-analyzer).
#[gpui::test]
fn insert_live_template_lists_the_servers_templates(cx: &mut TestAppContext) {
    let root = workspace(
        "templates",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("src/main.rs", "fn main() {}\n\n"),
            ("notes.txt", "notes\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.txt"));
    keys(cx, "cmd-j");
    assert_eq!(status(&bench, cx), "No live templates here");
    assert_eq!(bench.read_with(cx, |b, _| b.completion_items()), None);

    let usable = std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !usable {
        eprintln!("skipped: rust-analyzer is not on PATH");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    ferris_idea_core::trust::trust_for_process(&root);
    open(&bench, cx, &root.join("src/main.rs"));
    keys(cx, "cmd-down");
    keys(cx, "down down");
    let mut shown = Vec::new();
    until(cx, 240, "⌘J lists rust-analyzer's templates", |cx| {
        keys(cx, "escape");
        keys(cx, "cmd-j");
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            cx.run_until_parked();
            if let Some(items) = bench.read_with(cx, |b, _| b.completion_items()) {
                shown = items;
                break;
            }
            if status(&bench, cx) == "No live templates here" {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        shown.iter().any(|l| l.contains("tfn"))
    });
    assert!(
        !shown.iter().any(|l| l.contains("main")),
        "only templates: {shown:?}"
    );
    // The list's own filter narrows to the test function template.
    cx.simulate_input("tfn");
    cx.run_until_parked();
    keys(cx, "enter");
    until(cx, 10, "the template is inserted", |cx| {
        text(&bench, cx).contains("#[test]")
    });
    let name = selected(&bench, cx);
    assert!(
        !name.is_empty(),
        "the name is selected: {}",
        text(&bench, cx)
    );
    keys(cx, "tab");
    assert_eq!(selected(&bench, cx), "", "Tab goes to the body");
    keys(cx, "shift-tab");
    assert_eq!(selected(&bench, cx), name, "⇧Tab goes back to the name");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0072 AK-34 (v14): Tab and ⇧Tab walk the places of an inserted template both ways, also
/// after typing into one; Escape ends it and ⇧Tab is then the editor's.
#[gpui::test]
fn tab_and_shift_tab_walk_a_templates_places(cx: &mut TestAppContext) {
    let root = workspace("stops", &[("src/main.rs", "fn main() {\n    \n}\n")]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("src/main.rs"));
    keys(cx, "down end");
    let template = CompletionItem {
        label: "pair".into(),
        insert: "let (${1:left}, ${2:right}) = (1, 2);$0".into(),
        kind: Some("snippet".into()),
        snippet: true,
        ..Default::default()
    };
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![template], false, cx))
    });
    keys(cx, "enter");
    assert_eq!(
        text(&bench, cx),
        "fn main() {\n    let (left, right) = (1, 2);\n}\n"
    );
    assert_eq!(selected(&bench, cx), "left");
    cx.simulate_input("width");
    cx.run_until_parked();
    keys(cx, "tab");
    assert_eq!(selected(&bench, cx), "right");
    keys(cx, "shift-tab");
    assert_eq!(
        selected(&bench, cx),
        "width",
        "⇧Tab selects what was typed there"
    );
    keys(cx, "tab");
    cx.simulate_input("height");
    cx.run_until_parked();
    keys(cx, "tab");
    assert_eq!(
        text(&bench, cx),
        "fn main() {\n    let (width, height) = (1, 2);\n}\n"
    );
    keys(cx, "shift-tab");
    assert_eq!(selected(&bench, cx), "height", "back from the end");
    keys(cx, "escape");
    keys(cx, "tab");
    assert!(
        text(&bench, cx).contains("let (width, "),
        "after Escape Tab is the editor's: {}",
        text(&bench, cx)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0059 AK-19 (v9.10): in the Switcher ⌘E moves the selection down as ⌃⇥ does.
#[gpui::test]
fn command_e_iterates_the_switcher(cx: &mut TestAppContext) {
    let root = workspace(
        "switcher",
        &[("one.txt", "1\n"), ("two.txt", "2\n"), ("three.txt", "3\n")],
    );
    let (bench, cx) = window(&root, cx);
    for f in ["one.txt", "two.txt", "three.txt"] {
        open(&bench, cx, &root.join(f));
    }
    let chosen = |bench: &Bench, cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.switcher_state(cx)
                .map(|s| s["selected"].as_str().unwrap_or_default().to_owned())
        })
    };
    keys(cx, "ctrl-tab");
    assert_eq!(chosen(&bench, cx).as_deref(), Some("two.txt"));
    keys(cx, "ctrl-cmd-e");
    assert_eq!(
        chosen(&bench, cx).as_deref(),
        Some("one.txt"),
        "⌘E walks on"
    );
    keys(cx, "enter");
    let active = bench.read_with(cx, |b, _| b.active_path());
    assert!(active.is_some_and(|p| p.ends_with("one.txt")));
    let _ = std::fs::remove_dir_all(&root);
}

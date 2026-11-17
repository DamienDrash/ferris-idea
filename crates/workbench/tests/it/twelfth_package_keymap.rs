//! The twelfth package of actions taken from the keymap (F-018), in a window of the test harness
//! and with keys only: Focus Gutter (⌥⇧6 F, `EditorFocusGutter`, `SPEC-0058` v5 behaviour 13).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::WorkbenchView;

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-twelfth-{tag}-{}", std::process::id()));
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

/// SPEC-0058 AK-8 (v5): ⌥⇧6 F puts the keys on the gutter: ↓ goes to the next line with an icon,
/// Enter on a breakpoint removes it as a click does; in a file without icons the status bar says so.
#[gpui::test]
fn the_gutter_icons_answer_to_keys(cx: &mut TestAppContext) {
    let root = workspace("gutter", &[("notes.txt", "one\ntwo\nthree\nfour\nfive\n")]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.txt"));
    keys(cx, "alt-shift-6 f");
    until(cx, 5, "the status", |cx| !status(&bench, cx).is_empty());
    assert!(
        status(&bench, cx).contains("No gutter icons"),
        "{}",
        status(&bench, cx)
    );
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| e.set_breakpoints(vec![1, 3], cx));
    cx.run_until_parked();
    keys(cx, "alt-shift-6 f");
    assert_eq!(editor.read_with(cx, |e, _| e.gutter_focus()), Some((1, 0)));
    assert!(
        editor
            .read_with(cx, |e, _| e.focused_gutter_tip())
            .is_some(),
        "the focused icon has its tooltip"
    );
    keys(cx, "down");
    assert_eq!(editor.read_with(cx, |e, _| e.gutter_focus()), Some((3, 0)));
    keys(cx, "enter");
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoints().to_vec()),
        vec![1]
    );
    assert_eq!(editor.read_with(cx, |e, _| e.gutter_focus()), None);
    keys(cx, "alt-shift-6 f");
    keys(cx, "escape");
    assert_eq!(
        editor.read_with(cx, |e, _| e.gutter_focus()),
        None,
        "Escape gives the keys back"
    );
    assert_eq!(
        text(&bench, cx),
        "one\ntwo\nthree\nfour\nfive\n",
        "no key reached the text"
    );
    let _ = std::fs::remove_dir_all(&root);
}

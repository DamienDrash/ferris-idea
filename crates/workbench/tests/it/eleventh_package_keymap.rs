//! The eleventh package of actions taken from the keymap (F-018), in a window of the test harness
//! and with keys only: Tab out of a pair (`BraceOrQuoteOut`, `SPEC-0056` v6 behaviour 22), a Python
//! template expanded by Tab (`ExpandLiveTemplateByTab`, `SPEC-0072` v16 behaviour 53), Find Usages
//! with a choice of scope (⌥⇧⌘F7, `SPEC-0293` v10 behaviour 28) and External Documentation without
//! a link (⇧F1, `SPEC-0077` v8 behaviour 7).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::WorkbenchView;

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-eleventh-{tag}-{}", std::process::id()));
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

/// SPEC-0056 AK-17 (v6), SPEC-0072 AK-37 (v16): Tab inside `print("x")` steps over the quote
/// and the bracket; `main` and Tab in a Python file becomes the template.
#[gpui::test]
fn tab_steps_out_of_pairs_and_expands_a_template(cx: &mut TestAppContext) {
    let root = workspace("tab", &[("app.py", "print(\"x\")\nmain\n")]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("app.py"));
    // The caret after `x`: Tab steps over `"`, then over `)`.
    keys(cx, "right right right right right right right right");
    keys(cx, "tab");
    keys(cx, "tab");
    keys(cx, "Y");
    assert_eq!(
        text(&bench, cx),
        "print(\"x\")Y\nmain\n",
        "the caret left both pairs"
    );
    keys(cx, "down end");
    keys(cx, "tab");
    let t = text(&bench, cx);
    assert!(
        t.contains("if __name__ == \"__main__\":"),
        "main expanded by Tab: {t}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0293 AK-16 (v10): ⌥⇧⌘F7 on a name asks for the scope; External Documentation (⇧F1)
/// without a server says that there is no link.
#[gpui::test]
fn find_usages_asks_for_the_scope(cx: &mut TestAppContext) {
    let root = workspace("scope", &[("notes.txt", "alpha beta\n")]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.txt"));
    keys(cx, "shift-f1");
    until(cx, 5, "the status", |cx| !status(&bench, cx).is_empty());
    assert!(
        status(&bench, cx).contains("No external documentation"),
        "{}",
        status(&bench, cx)
    );
    let _ = std::fs::remove_dir_all(&root);

    let root = workspace(
        "scope-rs",
        &[("lib.rs", "fn alpha() {}\nfn beta() { alpha(); }\n")],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("lib.rs"));
    keys(cx, "right right right right");
    keys(cx, "alt-shift-cmd-f7");
    let popup = bench.read_with(cx, |b, cx| {
        b.jvm_popup()
            .map(|p| (p.read(cx).title().to_owned(), p.read(cx).items().to_vec()))
    });
    let (title, items) = popup.expect("the scope choice is open");
    assert_eq!(title, "Find Usages of alpha");
    assert_eq!(items, ["Project Files", "This File"]);
    let _ = std::fs::remove_dir_all(&root);
}

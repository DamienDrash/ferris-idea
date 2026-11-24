//! Reformat Code through a language server in a window of the test harness (feature row F-039,
//! `SPEC-0075` v7 behaviour 2): the selection and every caret stay on their text, and the result
//! is one undo step. With clangd; skipped without it.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;
type Editor = Entity<EditorView>;

const CODE: &str = "int a(int x){\nreturn x+1;\n}\nint b(int y){\nreturn y*2;\n}\n";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-f039-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("shop.c"), CODE).unwrap();
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Editor {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

fn text(editor: &Editor, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

/// The text with the carets marked: `|` for a caret, `[`…`]` for a selection.
fn marked(editor: &Editor, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| {
        let text: Vec<char> = e.state.buffer.text().chars().collect();
        let carets = e.state.carets.carets();
        let mut out = String::new();
        for i in 0..=text.len() {
            for c in carets {
                if c.anchor == c.head && c.head == i {
                    out.push('|');
                } else if c.anchor != c.head && c.range().end == i {
                    out.push(']');
                }
            }
            for c in carets {
                if c.anchor != c.head && c.range().start == i {
                    out.push('[');
                }
            }
            if let Some(ch) = text.get(i) {
                out.push(*ch);
            }
        }
        out
    })
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// Runs Reformat Code and waits until the status bar has its answer.
fn reformat(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ReformatCode", window, cx)
    });
    for _ in 0..600 {
        cx.run_until_parked();
        let now = status(bench, cx);
        if !now.is_empty() && now != "Reformatting…" {
            return now;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("no answer: {}", status(bench, cx));
}

fn clangd() -> bool {
    let found = std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !found {
        eprintln!("skipped: no clangd");
    }
    found
}

/// `SPEC-0075` v7 AK-10: a selection formatted through clangd stays selected over its text, the
/// lines outside it stay, and one ⌘Z gives the text and the selection back. Before v7 the
/// selection was dropped for a caret at its end.
#[gpui::test]
fn a_reformatted_selection_stays_selected(cx: &mut TestAppContext) {
    if !clangd() {
        return;
    }
    let root = workspace("selection");
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("shop.c"));
    let start = CODE.find("return x").unwrap();
    let end = start + "return x+1;".len();
    e.update(cx, |e, cx| e.select_range(start..end, cx));
    let before = marked(&e, cx);
    let said = reformat(&bench, cx);
    assert!(said.starts_with("Reformatted"), "{said}");
    let after = marked(&e, cx);
    assert!(after.contains("[return x + 1;]"), "{after:?}");
    assert!(
        after.contains("\nreturn y*2;\n"),
        "outside the selection: {after:?}"
    );
    cx.simulate_keystrokes("cmd-z");
    cx.run_until_parked();
    assert_eq!(
        marked(&e, cx),
        before,
        "one step, the selection where it was"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0075` v7 AK-10: formatting the whole file through clangd keeps every caret on its text.
/// Before v7 only one caret was left.
#[gpui::test]
fn every_caret_stays_on_its_text(cx: &mut TestAppContext) {
    if !clangd() {
        return;
    }
    let root = workspace("carets");
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("shop.c"));
    let x = CODE.find("x+1").unwrap() + 1;
    let y = CODE.find("y*2").unwrap() + 1;
    e.update(cx, |e, cx| e.select_ranges(&[x..x, y..y], cx));
    let said = reformat(&bench, cx);
    assert!(said.starts_with("Reformatted"), "{said}");
    let after = marked(&e, cx);
    assert!(
        after.contains("return x |+ 1;") && after.contains("return y |* 2;"),
        "{after:?}"
    );
    assert!(!text(&e, cx).contains("x+1"));
    let _ = std::fs::remove_dir_all(&root);
}

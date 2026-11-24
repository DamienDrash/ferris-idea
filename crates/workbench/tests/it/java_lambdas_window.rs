//! Window check of the Java lambda simplifications (`SPEC-0098` v11 AK-15, OBS-0050): a file like
//! the reference's `Fold.java` shows three weak warnings and no warning in the editor's counter,
//! and ⌥↩ on the anonymous class replaces it by a lambda in one undo step. No language server
//! takes part.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const FOLD: &str = "import java.util.ArrayList;
import java.util.List;
import java.util.Map;

public class Fold {
    private final List<String> names = new ArrayList<>();
    private int count;

    public int getCount() {
        return count;
    }

    public void add(String name) {
        for (int i = 0; i < 2; i++) {
            names.add(name + i);
        }
        count++;
    }

    Runnable task = new Runnable() {
        @Override
        public void run() {
            names.forEach(n -> {
                System.out.println(n);
            });
        }
    };

    static class Inner {
        Map<String, Integer> sizes = Map.of(\"a\", 1);
    }
}
";

fn workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-lambdas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Fold.java"), FOLD).unwrap();
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    (bench, cx)
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn counts(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize, usize) {
    editor.read_with(cx, |e, _| e.status().problems)
}

#[gpui::test]
fn java_lambda_simplifications_in_the_window(cx: &mut TestAppContext) {
    let root = workspace();
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("src/Fold.java"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    until(cx, "the file's own findings", |cx| {
        counts(&editor, cx).2 > 0
    });
    // Three weak warnings: the anonymous class, the block lambda, the call. The warnings are the
    // unused declarations of the project's index (SPEC-0098 v12 behaviour 26), which arrive when
    // the index is ready; they are checked in `java_unused_window.rs`.
    let (errors, _, weak) = counts(&editor, cx);
    assert_eq!((errors, weak), (0, 3));

    // ⌥↩ on the anonymous class: "Replace with lambda" gives the expression lambda at once.
    let at = FOLD.find("new Runnable()").unwrap();
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ShowIntentionActions", window, cx)
    });
    until(cx, "the list of context actions opens", |cx| {
        bench.read_with(cx, |b, _| b.intentions_popup().is_some())
    });
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    let titles = popup.read_with(cx, |p, _| p.titles());
    assert!(
        titles.iter().any(|t| t == "Replace with lambda"),
        "{titles:?}"
    );
    popup.update(cx, |p, cx| p.choose_title("Replace with lambda", cx));
    until(cx, "the anonymous class is replaced", |cx| {
        editor.read_with(cx, |e, _| e.state.buffer.text()) != FOLD
    });
    let text = editor.read_with(cx, |e, _| e.state.buffer.text());
    assert!(
        text.contains(
            "    Runnable task = () -> names.forEach(n -> {\n            System.out.println(n);\n        });\n"
        ),
        "{text}"
    );
    // The block lambda and its call remain; the anonymous class is gone.
    until(cx, "the findings follow the edit", |cx| {
        let (errors, _, weak) = counts(&editor, cx);
        (errors, weak) == (0, 2)
    });

    // One undo step brings the anonymous class back.
    editor.update(cx, |e, cx| e.run_action("$Undo", cx));
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.state.buffer.text()), FOLD);
    let _ = std::fs::remove_dir_all(&root);
}

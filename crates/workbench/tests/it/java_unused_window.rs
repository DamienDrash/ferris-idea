//! Window check of the unused declarations in the editor (`SPEC-0098` v12 AK-16, `SPEC-0316` v3
//! AK-9, OBS-0050 W1, W2, W6, W7): a file like the reference's `Fold.java` shows the four warnings
//! of the project's reference index once the index is ready, the entry points of a second file
//! show none, a use typed into the file takes its warning away, and ⌥↩ on a finding offers Safe
//! Delete and suppresses it. No language server takes part.

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

/// Entry points only: `main` and its class, an annotated method, `toString`, the
/// serialization field.
const APP: &str = "public class App {
    private static final long serialVersionUID = 1L;

    public static void main(String[] args) {
        System.out.println(new App());
    }

    @Deprecated
    public void old() {
    }

    public String toString() {
        return \"app\";
    }
}
";

fn workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-unused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Fold.java"), FOLD).unwrap();
    std::fs::write(root.join("src/App.java"), APP).unwrap();
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

/// The messages of the editor's unused-declaration warnings, in text order.
fn unused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |e, _| {
        e.problems()
            .iter()
            .filter(|p| p.inspection == Some("java.unused-declaration"))
            .map(|p| p.message.clone())
            .collect()
    })
}

fn open(
    bench: &Entity<WorkbenchView>,
    file: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

#[gpui::test]
fn unused_declarations_appear_in_the_editor(cx: &mut TestAppContext) {
    let root = workspace();
    let (bench, cx) = window(&root, cx);
    let app = open(&bench, &root.join("src/App.java"), cx);
    let editor = open(&bench, &root.join("src/Fold.java"), cx);
    until(cx, "the index is built and the file checked", |cx| {
        !bench.read_with(cx, |b, _| b.reference_index_running()) && !unused(&editor, cx).is_empty()
    });
    // The four warnings of OBS-0050; not the top-level class, not the private or used members.
    assert_eq!(
        unused(&editor, cx),
        [
            "Ferris IDEA: Method 'getCount()' is never used",
            "Ferris IDEA: Field 'task' is never used",
            "Ferris IDEA: Class 'Inner' is never used",
            "Ferris IDEA: Field 'sizes' is never used",
        ]
    );
    assert_eq!(editor.read_with(cx, |e, _| e.status().problems.1), 4);
    // The entry points of the other file are not reported.
    assert_eq!(unused(&app, cx), Vec::<String>::new());

    // A use typed into the file (unsaved) takes the warning of `getCount()` away.
    let typed = FOLD.replace("        count++;", "        getCount();\n        count++;");
    editor.update(cx, |e, cx| e.apply_agent_edit(&typed, false, cx).unwrap());
    until(cx, "the check follows the edit", |cx| {
        unused(&editor, cx).len() == 3
    });
    assert!(
        !unused(&editor, cx).iter().any(|m| m.contains("getCount")),
        "{:?}",
        unused(&editor, cx)
    );

    // ⌥↩ on `task`: Safe Delete, and in its options the suppressions but no "Fix all".
    let text = editor.read_with(cx, |e, _| e.state.buffer.text());
    let at = text.find("task =").unwrap() + 1;
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
    assert_eq!(
        titles.first().map(String::as_str),
        Some("Safe delete 'task'"),
        "{titles:?}"
    );
    popup.update(cx, |p, cx| p.open_submenu(cx));
    let labels = popup
        .read_with(cx, |p, _| p.submenu_labels())
        .unwrap_or_default();
    assert!(
        labels.iter().any(|l| l == "Suppress for statement"),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("Fix all")),
        "{labels:?}"
    );
    popup.update(cx, |p, cx| p.choose_option("Suppress for statement", cx));
    until(
        cx,
        "the suppression is written and the warning goes",
        |cx| {
            editor
                .read_with(cx, |e, _| e.state.buffer.text())
                .contains("//noinspection unused\n    Runnable task")
                && unused(&editor, cx).len() == 2
        },
    );
    let _ = std::fs::remove_dir_all(&root);
}

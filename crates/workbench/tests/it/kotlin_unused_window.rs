//! Window check of the unused declarations of Kotlin files in the editor (`SPEC-0098` v13 AK-17,
//! `SPEC-0316` v4 AK-10): a Kotlin file next to a Java file shows the warnings of the project's
//! reference index for its unused function, properties and nested class once the index is ready,
//! none for the function the Java file calls and none for the entry points of a second Kotlin
//! file; a use typed into the file takes its warning away, and ⌥↩ on a finding offers Safe Delete
//! and the three Kotlin suppressions. No language server takes part.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{EditorView, WorkbenchView};

const FOLD: &str = "package demo

class Fold {
    private var count = 0

    fun countValue(): Int {
        return count
    }

    fun add(name: String) {
        count += name.length
    }

    val label = \"fold\"

    class Inner {
        val sizes = mapOf(\"a\" to 1)
    }

    fun calledFromJava() {
    }
}
";

/// Entry points only: `main`, a data class's properties, an operator, an override, a test.
const ENTRY: &str = "package demo

fun main() {
    println(Point(1, 2) + Point(3, 4))
}

data class Point(val x: Int, val y: Int) {
    operator fun plus(o: Point) = Point(x + o.x, y + o.y)

    override fun toString() = \"($x, $y)\"
}

class EntryTest {
    @Test
    fun works() {
    }
}
";

/// Java calls into the Kotlin file.
const APP: &str = "package demo;

public class App {
    public static void main(String[] args) {
        Fold f = new Fold();
        f.add(\"x\");
        f.calledFromJava();
    }
}
";

fn workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-kt-unused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/demo")).unwrap();
    std::fs::write(root.join("src/demo/Fold.kt"), FOLD).unwrap();
    std::fs::write(root.join("src/demo/Entry.kt"), ENTRY).unwrap();
    std::fs::write(root.join("src/demo/App.java"), APP).unwrap();
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

/// The messages of the editor's Kotlin unused-declaration warnings, in text order.
fn unused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |e, _| {
        e.problems()
            .iter()
            .filter(|p| p.inspection == Some("kotlin.unused-declaration"))
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
fn kotlin_unused_declarations_appear_in_the_editor(cx: &mut TestAppContext) {
    let root = workspace();
    let (bench, cx) = window(&root, cx);
    let entry = open(&bench, &root.join("src/demo/Entry.kt"), cx);
    let editor = open(&bench, &root.join("src/demo/Fold.kt"), cx);
    until(cx, "the index is built and the file checked", |cx| {
        !bench.read_with(cx, |b, _| b.reference_index_running()) && !unused(&editor, cx).is_empty()
    });
    // Not the top-level class, not the private property, not what Java calls.
    assert_eq!(
        unused(&editor, cx),
        [
            "IDECraft: Function 'countValue' is never used",
            "IDECraft: Property 'label' is never used",
            "IDECraft: Class 'Inner' is never used",
            "IDECraft: Property 'sizes' is never used",
        ]
    );
    // The entry points of the other Kotlin file are not reported.
    assert_eq!(unused(&entry, cx), Vec::<String>::new());

    // A use typed into the file (unsaved) takes the warning of `countValue` away.
    let typed = FOLD.replace(
        "        count += name.length",
        "        count += name.length + countValue()",
    );
    editor.update(cx, |e, cx| e.apply_agent_edit(&typed, false, cx).unwrap());
    until(cx, "the check follows the edit", |cx| {
        unused(&editor, cx).len() == 3
    });
    assert!(
        !unused(&editor, cx).iter().any(|m| m.contains("countValue")),
        "{:?}",
        unused(&editor, cx)
    );

    // ⌥↩ on `label`: Safe Delete, and in its options the Kotlin suppressions but no "Fix all".
    let text = editor.read_with(cx, |e, _| e.state.buffer.text());
    let at = text.find("label =").unwrap() + 1;
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
        Some("Safe delete 'label'"),
        "{titles:?}"
    );
    popup.update(cx, |p, cx| p.open_submenu(cx));
    let labels = popup
        .read_with(cx, |p, _| p.submenu_labels())
        .unwrap_or_default();
    for wanted in [
        "Suppress for declaration",
        "Suppress for class",
        "Suppress for file",
    ] {
        assert!(labels.iter().any(|l| l == wanted), "{wanted}: {labels:?}");
    }
    assert!(
        !labels
            .iter()
            .any(|l| l.starts_with("Fix all") || l == "Suppress for statement"),
        "{labels:?}"
    );
    popup.update(cx, |p, cx| p.choose_option("Suppress for declaration", cx));
    until(
        cx,
        "the suppression is written and the warning goes",
        |cx| {
            editor
                .read_with(cx, |e, _| e.state.buffer.text())
                .contains("    @Suppress(\"unused\")\n    val label")
                && unused(&editor, cx).len() == 2
        },
    );
    let _ = std::fs::remove_dir_all(&root);
}

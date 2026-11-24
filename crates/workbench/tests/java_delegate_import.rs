//! Delegate Methods and the import hint in a window with a real jdtls (`SPEC-0122` v5 AK-17;
//! OBS-0027 L36, L37): typing narrows the method list and Enter adds the method left; Show Context
//! Actions on a hint with one class adds the import at once. Inline (⌥⌘N) on a method's name
//! lists the Inline Method variants (`SPEC-0122` v9 AK-23, AK-24; the window of 2026-11-13 stayed
//! at "Inline…"). The language server pool is on, as in the application, so these tests have a
//! process of their own. Skipped without `jdtls`.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::popup_place;
use ferris_idea_workbench::{ChoiceView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const DEADLINE: Duration = Duration::from_secs(300);

fn write(root: &Path, name: &str, text: &str) -> PathBuf {
    let file = root.join(format!("src/main/java/shop/{name}.java"));
    std::fs::write(&file, text).unwrap();
    file
}

fn go_to(bench: &Entity<WorkbenchView>, line: usize, column: usize, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().expect("a file is open");
        editor.update(cx, |e, cx| e.go_to(line, column, cx));
    });
    cx.run_until_parked();
}

fn text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn list<T>(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    read: impl FnOnce(&ChoiceView) -> T,
) -> T {
    bench.read_with(cx, |b, cx| {
        read(b.jvm_popup().expect("a list is open").read(cx))
    })
}

fn closed(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, _| b.jvm_popup().is_none())
}

/// Lets the workbench's timers run for a moment of real time, so the language server can answer.
fn wait(cx: &mut VisualTestContext) {
    std::thread::sleep(Duration::from_millis(500));
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
}

#[gpui::test]
fn typing_narrows_the_delegate_list_and_the_hint_imports(cx: &mut TestAppContext) {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-delegate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    let root = root.canonicalize().unwrap();
    ferris_idea_core::trust::trust_for_process(&root);
    ferris_idea_languages::pool::enable();
    write(
        &root,
        "Engine",
        "package shop;\n\npublic class Engine {\n    public void start(int rpm) {}\n\n    public String label() {\n        return \"\";\n    }\n}\n",
    );
    let car = write(
        &root,
        "Car",
        "package shop;\n\npublic class Car {\n    private Engine engine;\n}\n",
    );
    let main = write(
        &root,
        "Main",
        "package shop;\n\npublic class Main {\n    void m() {\n        Object o = new LinkedList<String>();\n    }\n}\n",
    );

    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&car, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    go_to(&bench, 3, 20, cx);

    // Delegate Methods: jdtls lists the field's methods once it knows the project.
    let start = Instant::now();
    loop {
        bench.update_in(cx, |b, window, cx| b.delegate_methods(window, cx));
        cx.run_until_parked();
        if !closed(&bench, cx) {
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no Delegate list: {}",
            status(&bench, cx)
        );
        wait(cx);
    }
    let (title, mut items) = list(&bench, cx, |p| (p.title().to_owned(), p.items().to_vec()));
    assert_eq!(title, "Delegate to engine");
    items.sort();
    assert_eq!(items, ["label()", "start(int)"]);
    // The list stands under the caret's line (OBS-0027 L10).
    let caret = bench.read_with(cx, |b, cx| {
        let editor = b.active_editor_entity().unwrap();
        let e = editor.read(cx);
        e.caret_spot(e.caret()).expect("the editor was drawn").0
    });
    let (place, _) = bench
        .read_with(cx, |b, cx| b.caret_list_placement(cx))
        .expect("the list stands at the caret");
    assert_eq!(
        (place.x, place.y, place.above),
        (caret.x, caret.bottom + popup_place::GAP, false)
    );
    // Typing narrows the methods; Enter adds the one left (OBS-0027 L36).
    cx.simulate_keystrokes("s t a");
    assert_eq!(list(&bench, cx, |p| p.shown_items()), ["start(int)"]);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(closed(&bench, cx));
    let added = text(&bench, cx);
    assert!(added.contains("engine.start("), "{added}");
    assert!(!added.contains("engine.label("), "{added}");

    // The import hint over the unresolved LinkedList: jdtls's problem and its one class.
    bench.update_in(cx, |b, window, cx| b.open(&main, window, cx));
    cx.run_until_parked();
    let start = Instant::now();
    'hint: loop {
        // The hint asks once per stay on the line, so the caret comes back for another try.
        go_to(&bench, 3, 0, cx);
        wait(cx);
        go_to(&bench, 4, 30, cx);
        for _ in 0..10 {
            wait(cx);
            if bench.read_with(cx, |b, cx| b.import_hint_is_shown(cx)) {
                break 'hint;
            }
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no import hint: {}",
            status(&bench, cx)
        );
    }
    let hint = bench.read_with(cx, |b, _| b.import_hint_text()).unwrap();
    assert!(hint.starts_with("Import java.util.LinkedList? "), "{hint}");
    // One class: Show Context Actions imports it at once, no list (OBS-0027 L37).
    cx.simulate_keystrokes("alt-enter");
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.intentions_popup().is_none()
            && b.jvm_popup().is_none()),
        "no list opens for the one class"
    );
    let imported = text(&bench, cx);
    assert!(
        imported.contains("import java.util.LinkedList;"),
        "{imported}"
    );
    assert!(
        !bench.read_with(cx, |b, cx| b.import_hint_is_shown(cx)),
        "the hint is gone"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The probe file of OBS-0027 J2.
const INVENTORY: &str = "package lab.inventory;\n\nimport java.util.List;\n\npublic final class Inventory {\n    public record Item(String sku, String name, int qty, int unitPriceCents) {\n        int totalCents() {\n            return qty * unitPriceCents;\n        }\n    }\n\n    public static int totalValue(List<Item> items) {\n        return items.stream().mapToInt(Item::totalCents).sum();\n    }\n\n    public static void main(String[] args) {\n        System.out.println(totalValue(List.of(new Item(\"A-1\", \"Bolt\", 3, 25))));\n    }\n}\n";

#[gpui::test]
fn inline_on_a_method_lists_its_variants(cx: &mut TestAppContext) {
    use ferris_idea_languages::java_refactor::{INLINE_ALL_KEEP, INLINE_ALL_REMOVE};
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("ferris-idea-inline-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/lab/inventory")).unwrap();
    let root = root.canonicalize().unwrap();
    ferris_idea_core::trust::trust_for_process(&root);
    ferris_idea_languages::pool::enable();
    let file = root.join("src/main/java/lab/inventory/Inventory.java");
    std::fs::write(&file, INVENTORY).unwrap();

    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    // The caret in `totalValue` of the declaration, line 12 column 25 as the window counts.
    go_to(&bench, 11, 24, cx);

    // Pressed right after the file was opened, while jdtls is still loading the project
    // (OBS-0027 W1b): the status bar says at every moment what is asked or that jdtls is loading,
    // and the question is repeated by itself when jdtls is ready (SPEC-0095 v10 behaviours 11a
    // and 12). Only a final line (jdtls did not load in time, or had nothing yet) is answered
    // with another keystroke.
    let limit = ferris_idea_workbench::intentions_model::REFACTORING_LIMIT;
    let start = Instant::now();
    let mut presses = 0;
    let titles = 'list: loop {
        bench.update_in(cx, |b, window, cx| b.menu_action("Inline", window, cx));
        presses += 1;
        let asked = Instant::now();
        let said = status(&bench, cx);
        assert!(said.starts_with("Inline…"), "at the keystroke: {said:?}");
        loop {
            wait(cx);
            if let Some(popup) = bench.read_with(cx, |b, _| b.intentions_popup()) {
                break 'list popup.read_with(cx, |p, _| p.titles());
            }
            let said = status(&bench, cx);
            assert!(!said.is_empty(), "neither a list nor a line");
            let waiting = said.starts_with("Inline…") || said.contains("is asked again");
            if !waiting {
                eprintln!("after {:?}: {said}", start.elapsed());
                break;
            }
            // Neither line stays: the question and the wait for the server have their limit.
            assert!(
                asked.elapsed() < limit * 2 + Duration::from_secs(15),
                "still {said} after {:?}",
                asked.elapsed()
            );
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no Inline list: {}",
            status(&bench, cx)
        );
    };
    eprintln!(
        "Inline list after {presses} keystroke(s), {:?}",
        start.elapsed()
    );
    assert_eq!(titles, [INLINE_ALL_REMOVE, INLINE_ALL_KEEP]);
    assert_eq!(status(&bench, cx), "");
    let _ = std::fs::remove_dir_all(&root);
}

//! Find Usages where the caret is on no name, in a window of the test harness (`SPEC-0293` v8
//! behaviour 20): Find Usages, Find Usages in File, Show Usages and Highlight Usages in File say
//! so in the status bar at once; no tool window, no popup and no mark appears, and no language
//! server is asked (none is installed for the test, and none is needed).

#![cfg(target_os = "macos")]

use ferris_idea_workbench::usages_model::NO_NAME_AT_CARET;
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const CART: &str = "package shop;

class Cart {
    @Override
    public String toString() { // text of the cart
        return \"cart\";
    }

}
";

fn open(
    tag: &str,
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<EditorView>,
    &'static mut VisualTestContext,
) {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-usages-without-symbol-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Cart.java"), CART).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("Cart.java");
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open");
    (bench, editor, cx)
}

/// SPEC-0293 AK-13 (v8): each of the four actions, at each kind of place without a name.
#[gpui::test]
fn the_four_usage_actions_say_at_once_that_there_is_no_name(cx: &mut TestAppContext) {
    let (bench, editor, cx) = open("four", cx);
    // 0-based line and column: before the `@` of the annotation (the first column of its
    // text), in the indentation, between the signs of a comment, on an empty line.
    let places = [(3, 4), (3, 0), (4, 32), (7, 0)];
    for id in [
        "FindUsagesInFile",
        "FindUsages",
        "ShowUsages",
        "HighlightUsagesInFile",
    ] {
        for (line, column) in places {
            editor.update(cx, |e, cx| e.go_to(line, column, cx));
            assert_eq!(
                editor.read_with(cx, |e, _| e.name_at_caret()),
                None,
                "{line}:{column} is on no name"
            );
            // Another note first, so an older one cannot pass for the new one: a grouping
            // action without a result says so (SPEC-0293 v6 behaviour 18).
            bench.update_in(cx, |b, window, cx| {
                b.menu_action("UsageGrouping.Module", window, cx);
                assert_eq!(
                    b.status_note(),
                    Some(ferris_idea_workbench::keymap_actions::NO_USAGES_TO_GROUP)
                );
                b.menu_action(id, window, cx);
            });
            cx.run_until_parked();
            bench.read_with(cx, |b, _| {
                assert_eq!(
                    b.status_note(),
                    Some(NO_NAME_AT_CARET),
                    "{id} at {line}:{column}"
                );
                assert!(
                    b.find_tool_window().is_none(),
                    "{id} at {line}:{column} opens no Find tool window"
                );
                assert!(b.usages_popup().is_none(), "{id} at {line}:{column}");
            });
        }
    }
}

/// SPEC-0293 AK-13 (v8): on a name the search starts as before — the window opens in its
/// searching state for Find Usages in File and names the symbol.
#[gpui::test]
fn on_a_name_the_search_still_starts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (bench, editor, cx) = open("name", cx);
    // Right after `toString`: next to a name counts as on it.
    editor.update(cx, |e, cx| e.go_to(4, 26, cx));
    assert_eq!(
        editor.read_with(cx, |e, _| e.name_at_caret()).as_deref(),
        Some("toString")
    );
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("FindUsagesInFile", window, cx)
    });
    let view = bench
        .read_with(cx, |b, _| b.find_tool_window())
        .expect("the Find tool window opens");
    view.read_with(cx, |v, _| {
        assert_eq!(v.symbol(), "toString");
        assert_eq!(v.title(), "toString in Cart.java");
    });
    // Hiding the window stops the search; nothing is left running for the next test.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("HideActiveWindow", window, cx)
    });
    cx.run_until_parked();
}

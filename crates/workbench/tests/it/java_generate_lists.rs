//! The lists of Generate and Delegate Methods in a window (`SPEC-0122` v5 AK-17, `SPEC-0070` v10
//! AK-10; OBS-0027 L10, L36, L37): they open at the caret, typed text narrows them, a shown import
//! hint imports at once and the hint outside a class passes. Generate works from the syntax tree,
//! so nothing here needs a language server; Delegate Methods and the import hint with a real jdtls
//! are in `java_delegate_import.rs`.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_languages::java_generate::{DelegateField, DelegateMethod};
use ferris_idea_workbench::nav_model::Place;
use ferris_idea_workbench::popup_place::{self, Area, CaretSpot};
use ferris_idea_workbench::{ChoiceView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const PERSON: &str =
    "package shop;\n\npublic class Person {\n    private String name;\n    private int age;\n}\n";
const OUTSIDE: &str = "Put the caret inside a class to generate code";

fn workspace(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-genlists-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    root.canonicalize().unwrap()
}

fn write(root: &Path, name: &str, text: &str) -> PathBuf {
    let file = root.join(format!("src/main/java/shop/{name}.java"));
    std::fs::write(&file, text).unwrap();
    file
}

/// A workbench window with `file` open and the caret at `line`, `column`.
fn open(
    root: &Path,
    file: &Path,
    line: usize,
    column: usize,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    let file = file.to_path_buf();
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    go_to(&bench, line, column, cx);
    (bench, cx)
}

fn go_to(bench: &Entity<WorkbenchView>, line: usize, column: usize, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().expect("a file is open");
        editor.update(cx, |e, cx| e.go_to(line, column, cx));
    });
    cx.run_until_parked();
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

/// Reads the open list, or panics with `what`.
fn list<T>(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    what: &str,
    read: impl FnOnce(&ChoiceView) -> T,
) -> T {
    bench.read_with(cx, |b, cx| {
        let popup = b.jvm_popup().unwrap_or_else(|| panic!("no list: {what}"));
        read(popup.read(cx))
    })
}

fn closed(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, _| b.jvm_popup().is_none())
}

/// The caret's line and the editor area as the editor drew them.
fn spot(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> (CaretSpot, Area) {
    bench.read_with(cx, |b, cx| {
        let editor = b.active_editor_entity().unwrap();
        let e = editor.read(cx);
        e.caret_spot(e.caret()).expect("the editor was drawn")
    })
}

/// Where the open list stands and how many rows fit; checks that it keeps to the editor area.
fn placed(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> (popup_place::Place, usize) {
    let (_, area) = spot(bench, cx);
    let (place, rows) = bench
        .read_with(cx, |b, cx| b.caret_list_placement(cx))
        .expect("the list stands at the caret");
    let (shown, limit, height) = list(bench, cx, "placed", |p| {
        (p.shown().len(), p.limit(), p.full_height())
    });
    assert_eq!(limit, Some(rows), "the list takes the rows that fit");
    let height = height.min(
        ferris_idea_workbench::choice_view::CHROME
            + rows.min(shown.max(1)) as f32 * ferris_idea_workbench::metrics::MENU_ITEM_HEIGHT,
    );
    let (top, bottom) = if place.above {
        (place.y - height, place.y)
    } else {
        (place.y, place.y + height)
    };
    assert!(
        top >= area.top + popup_place::MARGIN && bottom <= area.bottom - popup_place::MARGIN,
        "the list {top}..{bottom} keeps inside {area:?}"
    );
    assert!(
        place.x >= area.left + popup_place::MARGIN
            && place.x + ferris_idea_workbench::choice_view::WIDTH
                <= area.right - popup_place::MARGIN,
        "the list at x {} keeps inside {area:?}",
        place.x
    );
    (place, rows)
}

#[gpui::test]
fn the_generate_list_opens_at_the_caret_and_filters(cx: &mut TestAppContext) {
    let root = workspace("filter");
    let file = write(&root, "Person", PERSON);
    let (bench, cx) = open(&root, &file, 3, 4, cx);

    cx.simulate_keystrokes("cmd-n");
    let items = list(&bench, cx, "⌘N", |p| p.items().to_vec());
    assert_eq!(
        items,
        [
            "Constructor",
            "Getter",
            "Setter",
            "Getter and Setter",
            "equals() and hashCode()",
            "toString()",
            // SPEC-0122 v10 behaviour 3d (OBS-0119 K2): no supertype, so no Implement Methods…;
            // no copyright profile, so no Copyright.
            "Override Methods…",
            "Delegate Methods…",
            "Test…",
        ]
    );
    // Under the caret's line, its left edge at the caret (OBS-0027 L10).
    let (caret, _) = spot(&bench, cx);
    let (place, _) = placed(&bench, cx);
    assert!(!place.above);
    assert_eq!(
        (place.x, place.y),
        (caret.x, caret.bottom + popup_place::GAP)
    );

    // Typed text narrows the rows; the row starting with it is selected (L36).
    cx.simulate_keystrokes("s e t");
    let (filter, shown, selected) = list(&bench, cx, "set", |p| {
        (
            p.filter().map(str::to_owned),
            p.shown_items(),
            p.items()[p.selected()].clone(),
        )
    });
    assert_eq!(filter.as_deref(), Some("set"));
    assert_eq!(shown, ["Setter", "Getter and Setter"]);
    assert_eq!(selected, "Setter");
    // Upper case finds the same rows, and ↓ stays among them.
    cx.simulate_keystrokes("backspace backspace backspace shift-g");
    assert_eq!(
        list(&bench, cx, "G", |p| p.shown_items()),
        [
            "Getter",
            "Getter and Setter",
            "toString()",
            "Delegate Methods…"
        ]
    );
    assert_eq!(list(&bench, cx, "G", |p| p.selected()), 1);
    cx.simulate_keystrokes("down down down down");
    assert_eq!(
        list(&bench, cx, "↓↓↓↓", |p| p.selected()),
        1,
        "↓ wraps among the shown rows"
    );
    cx.simulate_keystrokes("up");
    assert_eq!(list(&bench, cx, "↑", |p| p.selected()), 7);
    // Backspace widens again.
    cx.simulate_keystrokes("backspace s e t t");
    assert_eq!(
        list(&bench, cx, "sett", |p| p.shown_items()),
        ["Setter", "Getter and Setter"]
    );
    cx.simulate_keystrokes("backspace backspace backspace");
    assert_eq!(
        list(&bench, cx, "s", |p| p.shown_items()),
        [
            "Constructor",
            "Setter",
            "Getter and Setter",
            "equals() and hashCode()",
            "toString()",
            "Override Methods…",
            "Delegate Methods…",
            "Test…",
        ]
    );
    // A text no row has: the list stays and Enter does nothing.
    cx.simulate_keystrokes("q enter");
    assert!(list(&bench, cx, "sq", |p| p.shown().is_empty()));
    // Escape first clears the text …
    cx.simulate_keystrokes("escape");
    let (filter, shown) = list(&bench, cx, "Escape", |p| {
        (p.filter().map(str::to_owned), p.shown().len())
    });
    assert_eq!((filter.as_deref(), shown), (Some(""), 9));
    // … and Enter takes the row a text leaves: the field list opens at the same place.
    cx.simulate_keystrokes("s e t t enter");
    let (title, fields) = list(&bench, cx, "Enter", |p| {
        (p.title().to_owned(), p.items().to_vec())
    });
    assert_eq!(title, "Choose Fields");
    assert_eq!(fields, ["name: String", "age: int"]);
    let (again, _) = placed(&bench, cx);
    assert_eq!((again.x, again.y, again.above), (place.x, place.y, false));
    // Without a text Escape closes.
    cx.simulate_keystrokes("escape");
    assert!(closed(&bench, cx));
    assert_eq!(text(&bench, cx), PERSON, "nothing was generated");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn a_long_list_keeps_to_the_room(cx: &mut TestAppContext) {
    let root = workspace("room");
    // A class long enough to fill the editor, so the caret can stand at its bottom.
    let mut class = String::from("package shop;\n\npublic class Wide {\n");
    for i in 0..90 {
        class.push_str(&format!("    private int f{i};\n"));
    }
    class.push_str("}\n");
    let file = write(&root, "Wide", &class);

    // Near the top there is room under the line.
    let (bench, cx) = open(&root, &file, 3, 4, cx);
    cx.simulate_keystrokes("cmd-n");
    let (place, _) = placed(&bench, cx);
    assert!(!place.above);
    cx.simulate_keystrokes("escape");
    assert!(closed(&bench, cx));

    // On the last line of the editor the short Generate list stands over the line.
    go_to(&bench, 92, 4, cx);
    let (caret, area) = spot(&bench, cx);
    cx.simulate_keystrokes("cmd-n");
    let (place, rows) = placed(&bench, cx);
    let height = list(&bench, cx, "Generate", |p| p.full_height());
    assert!(
        area.bottom - caret.bottom < height,
        "the caret stands at the bottom: {caret:?} in {area:?}"
    );
    assert!(place.above, "{place:?}");
    assert_eq!(place.y, caret.top - popup_place::GAP);
    assert!(rows >= 6, "all six rows fit over the line");

    // The field list with 90 rows fits nowhere: the roomier side, limited and scrolling.
    cx.simulate_keystrokes("g e t t e r enter");
    let all = list(&bench, cx, "Choose Fields", |p| p.shown().len());
    assert_eq!(all, 90);
    let (place, rows) = placed(&bench, cx);
    assert!(place.above, "{place:?}");
    assert!(rows < 90, "{rows} rows of 90 are shown at once");
    assert_eq!(list(&bench, cx, "top", |p| p.top()), 0);
    for _ in 0..rows + 3 {
        cx.simulate_keystrokes("down");
    }
    let (top, selected) = list(&bench, cx, "scrolled", |p| (p.top(), p.selected()));
    assert_eq!(selected, rows + 3);
    assert_eq!(top, 4, "the list scrolled with the selection");
    // A text shortens the list; it still keeps to the room.
    cx.simulate_keystrokes("f 8 9");
    assert_eq!(list(&bench, cx, "f89", |p| p.shown_items()), ["f89: int"]);
    placed(&bench, cx);
    cx.simulate_keystrokes("escape escape");
    assert!(closed(&bench, cx));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn checked_rows_count_also_when_hidden(cx: &mut TestAppContext) {
    let root = workspace("checks");
    let file = write(&root, "Person", PERSON);
    let (bench, cx) = open(&root, &file, 3, 4, cx);

    cx.simulate_keystrokes("cmd-n g e t t e r enter");
    let (title, checked) = list(&bench, cx, "Getter", |p| {
        (p.title().to_owned(), p.checked())
    });
    assert_eq!(title, "Choose Fields");
    assert_eq!(checked, [0, 1], "both fields are checked at first");
    // "na" leaves the name; Space unchecks it and is not part of the text.
    cx.simulate_keystrokes("n a space");
    let (filter, shown, checked, hidden) = list(&bench, cx, "na", |p| {
        (
            p.filter().map(str::to_owned),
            p.shown_items(),
            p.checked(),
            p.hidden_checked(),
        )
    });
    assert_eq!(filter.as_deref(), Some("na"));
    assert_eq!(shown, ["name: String"]);
    assert_eq!(checked, [1]);
    assert_eq!(hidden, 1, "the checked age is not shown");
    // Enter takes every checked row, also the hidden one; then the usual template.
    cx.simulate_keystrokes("enter");
    assert_eq!(
        list(&bench, cx, "Template", |p| p.title().to_owned()),
        "Template"
    );
    cx.simulate_keystrokes("enter");
    assert!(closed(&bench, cx));
    let generated = text(&bench, cx);
    assert!(generated.contains("public int getAge()"), "{generated}");
    assert!(!generated.contains("getName"), "{generated}");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn the_hint_outside_a_class_passes(cx: &mut TestAppContext) {
    let root = workspace("hint");
    let file = write(&root, "Person", PERSON);
    let (bench, cx) = open(&root, &file, 0, 3, cx);

    // ⌘N on the package line: nothing to generate, the status bar says why.
    cx.simulate_keystrokes("cmd-n");
    assert!(closed(&bench, cx));
    assert_eq!(status(&bench, cx), OUTSIDE);
    assert_eq!(status(&bench, cx), OUTSIDE, "it stays while nothing moves");
    // The caret moves into the class: the hint is gone, also with a list open (OBS-0027).
    go_to(&bench, 3, 4, cx);
    assert_eq!(status(&bench, cx), "");
    cx.simulate_keystrokes("cmd-n");
    assert!(!closed(&bench, cx));
    assert_eq!(status(&bench, cx), "");
    cx.simulate_keystrokes("escape");
    // It does not come back at the old place.
    go_to(&bench, 0, 3, cx);
    assert_eq!(status(&bench, cx), "");
    assert_eq!(
        bench.read_with(cx, |b, _| b.status_note().map(str::to_owned)),
        None
    );

    // A list that opens while the caret rests removes the hint as well.
    cx.simulate_keystrokes("cmd-n");
    assert_eq!(status(&bench, cx), OUTSIDE);
    let place = Place {
        path: file.clone(),
        line: 0,
        column: 3,
    };
    bench.update_in(cx, |b, window, cx| {
        b.delegate_chooser(place, field(&["start(int)"]), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        list(&bench, cx, "Delegate", |p| p.title().to_owned()),
        "Delegate to engine"
    );
    assert_eq!(status(&bench, cx), "");
    cx.simulate_keystrokes("escape");
    assert_eq!(status(&bench, cx), "", "and it stays away after the list");
    let _ = std::fs::remove_dir_all(&root);
}

/// A field `engine` with methods labelled `labels` (`name(Type)`), as jdtls would list them.
fn field(labels: &[&str]) -> DelegateField {
    DelegateField {
        name: "engine".into(),
        ty: "Engine".into(),
        methods: labels
            .iter()
            .map(|l| {
                let (name, rest) = l.split_once('(').unwrap();
                DelegateMethod {
                    name: name.into(),
                    parameters: vec![rest.trim_end_matches(')').to_owned()],
                    raw: serde_json::Value::Null,
                }
            })
            .collect(),
        raw: serde_json::Value::Null,
    }
}

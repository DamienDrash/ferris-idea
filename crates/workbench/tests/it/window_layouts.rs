//! Window checks of the named tool window layouts (`SPEC-0333` AK-8): saving the arrangement
//! under a name through the name field, rearranging and restoring, the default layout, applying,
//! renaming and deleting a layout with its question, and a layouts file that cannot be read.
//! The layouts are kept beside the test workspace, never in the user's settings folder; nothing
//! is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_ui::tool_windows::{Layout, Side, ToolWindow};
use ferris_idea_ui::window_layouts::{Layouts, Problem};
use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn workspace(tag: &str) -> (PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-layouts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("project")).unwrap();
    std::fs::create_dir_all(root.join("settings")).unwrap();
    std::fs::write(root.join("project/notes.txt"), "one\ntwo\n").unwrap();
    let root = root.canonicalize().unwrap();
    (
        root.join("project"),
        root.join("settings/window-layouts.json"),
    )
}

fn window(
    root: &Path,
    layouts: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    let file = root.join("notes.txt");
    let layouts = layouts.to_path_buf();
    bench.update_in(cx, |b, window, cx| {
        b.set_window_layouts_file(layouts, cx);
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

fn act(bench: &Entity<WorkbenchView>, id: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

/// Types `name` into the open name field and presses Enter.
fn name(bench: &Entity<WorkbenchView>, name: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| {
        b.set_name_field(name, cx);
        b.finish_name_prompt(true, window, cx);
    });
    cx.run_until_parked();
}

fn prompt(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> Option<(String, Option<String>)> {
    bench.read_with(cx, |b, _| b.name_prompt_state())
}

fn note(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn shown(bench: &Entity<WorkbenchView>, w: ToolWindow, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, _| b.tool_window_shown(w))
}

fn side(bench: &Entity<WorkbenchView>, w: ToolWindow, cx: &mut VisualTestContext) -> Side {
    bench.read_with(cx, |b, _| b.tool_layout().side(w))
}

/// Shows Problems (hidden at the start) and docks it to the right.
fn problems_to_the_right(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) {
    if !shown(bench, ToolWindow::Problems, cx) {
        act(bench, "ActivateProblemsViewToolWindow", cx);
    }
    bench.update(cx, |b, cx| {
        b.move_tool_window(ToolWindow::Problems, Side::Right, cx)
    });
    cx.run_until_parked();
    assert!(shown(bench, ToolWindow::Problems, cx));
}

/// SPEC-0333 AK-8 (behaviours 4, 5, 7, 8, 9): the arrangement is saved under a name, comes back
/// after rearranging, and the default layout neither changes nor goes away.
#[gpui::test]
fn a_layout_is_saved_rearranged_and_restored(cx: &mut TestAppContext) {
    let (root, file) = workspace("save");
    let (bench, cx) = window(&root, &file, cx);
    assert!(shown(&bench, ToolWindow::Project, cx));
    problems_to_the_right(&bench, cx);

    // Save Current Layout as New…: a name that breaks a rule keeps the field open.
    act(&bench, "StoreNewLayout", cx);
    let (title, error) = prompt(&bench, cx).expect("the name field is open");
    assert_eq!(title, "Save Current Layout as New");
    assert_eq!(error, None);
    name(&bench, "a/b", cx);
    let (_, error) = prompt(&bench, cx).expect("the name field stays open");
    assert_eq!(
        error.as_deref(),
        Some("A layout name cannot contain / or \\")
    );
    assert!(!file.exists());
    name(&bench, "default", cx);
    assert!(prompt(&bench, cx).unwrap().1.is_some());
    name(&bench, "  Review ", cx);
    assert_eq!(prompt(&bench, cx), None);
    assert_eq!(note(&bench, cx), "Layout \"Review\" saved");
    let saved = Layouts::load(&file);
    assert_eq!(saved.names(), ["Review"]);
    assert_eq!(saved.selected(), Some("Review"));
    let (_, review) = saved.get("Review").unwrap();
    assert_eq!(review.layout.side(ToolWindow::Problems), Side::Right);
    assert!(review.shown.contains(&ToolWindow::Project));
    assert!(review.shown.contains(&ToolWindow::Problems));

    // Rearranging changes the window and the project's file, not the saved layout.
    bench.update_in(cx, |b, window, cx| {
        b.move_tool_window(ToolWindow::Problems, Side::Bottom, cx);
        b.hide_all_tool_windows(window, cx);
        b.toggle_tool_window_bars(cx);
    });
    cx.run_until_parked();
    assert!(!shown(&bench, ToolWindow::Problems, cx));
    assert!(!shown(&bench, ToolWindow::Project, cx));
    assert!(!bench.read_with(cx, |b, _| b.tool_layout().stripes));
    assert_eq!(Layouts::load(&file), saved);

    // Restore Current Layout brings the saved arrangement back.
    act(&bench, "RestoreDefaultLayout", cx);
    assert_eq!(note(&bench, cx), "Layout \"Review\" restored");
    assert_eq!(side(&bench, ToolWindow::Problems, cx), Side::Right);
    assert!(bench.read_with(cx, |b, _| b.tool_layout().stripes));
    assert!(shown(&bench, ToolWindow::Problems, cx));
    assert!(shown(&bench, ToolWindow::Project, cx));
    assert_eq!(
        Layout::load(&root).side(ToolWindow::Problems),
        Side::Right,
        "the project's layout file follows"
    );

    // Save Changes in Current Layout writes the window's arrangement into it.
    bench.update(cx, |b, cx| {
        b.move_tool_window(ToolWindow::Problems, Side::Bottom, cx)
    });
    act(&bench, "StoreDefaultLayout", cx);
    assert_eq!(note(&bench, cx), "Layout \"Review\" saved");
    let changed = Layouts::load(&file);
    assert_eq!(
        changed
            .get("Review")
            .unwrap()
            .1
            .layout
            .side(ToolWindow::Problems),
        Side::Bottom
    );

    // Default selects the default layout: sides back, the shown windows stay.
    bench.update(cx, |b, cx| {
        b.move_tool_window(ToolWindow::Problems, Side::Right, cx)
    });
    act(&bench, "RestoreFactoryDefaultLayout", cx);
    assert_eq!(side(&bench, ToolWindow::Problems, cx), Side::Bottom);
    assert!(shown(&bench, ToolWindow::Problems, cx));
    assert_eq!(
        bench.read_with(cx, |b, _| b.tool_layout().clone()),
        Layout::default()
    );
    let after = Layouts::load(&file);
    assert_eq!(after.selected(), None);
    assert_eq!(after.names(), ["Review"]);

    // The default layout is not saved over.
    act(&bench, "StoreDefaultLayout", cx);
    assert!(
        note(&bench, cx).starts_with("The default layout cannot be changed"),
        "{}",
        note(&bench, cx)
    );
    assert_eq!(Layouts::load(&file), after);
    // Escape in the name field saves nothing.
    act(&bench, "StoreNewLayout", cx);
    bench.update_in(cx, |b, window, cx| b.finish_name_prompt(false, window, cx));
    assert_eq!(prompt(&bench, cx), None);
    assert_eq!(Layouts::load(&file), after);
}

/// SPEC-0333 AK-8 (behaviours 6, 10, 11): Apply selects and arranges, Rename… keeps the
/// selection, Delete… asks and refuses the selected layout.
#[gpui::test]
fn layouts_are_applied_renamed_and_deleted(cx: &mut TestAppContext) {
    let (root, file) = workspace("apply");
    let (bench, cx) = window(&root, &file, cx);
    problems_to_the_right(&bench, cx);
    act(&bench, "StoreNewLayout", cx);
    name(&bench, "One", cx);
    // A second layout: nothing shown, no stripes, Problems at the bottom.
    bench.update_in(cx, |b, window, cx| {
        b.move_tool_window(ToolWindow::Problems, Side::Bottom, cx);
        b.hide_all_tool_windows(window, cx);
        b.toggle_tool_window_bars(cx);
    });
    cx.run_until_parked();
    act(&bench, "StoreNewLayout", cx);
    name(&bench, "Two", cx);
    let saved = Layouts::load(&file);
    assert_eq!(saved.names(), ["One", "Two"]);
    assert_eq!(saved.selected(), Some("Two"));
    assert!(saved.get("Two").unwrap().1.shown.is_empty());

    act(&bench, "FerrisIDEA.Layout.Apply:One", cx);
    assert_eq!(note(&bench, cx), "Layout \"One\" applied");
    assert_eq!(Layouts::load(&file).selected(), Some("One"));
    assert_eq!(side(&bench, ToolWindow::Problems, cx), Side::Right);
    assert!(shown(&bench, ToolWindow::Problems, cx));
    assert!(shown(&bench, ToolWindow::Project, cx));
    assert!(bench.read_with(cx, |b, _| b.tool_layout().stripes));
    let now = bench.read_with(cx, |b, cx| {
        let _ = cx;
        b.tool_layout().clone()
    });
    assert_eq!(
        Layout::parse(&now.to_json()),
        saved.get("One").unwrap().1.layout
    );

    act(&bench, "FerrisIDEA.Layout.Apply:Two", cx);
    assert!(!shown(&bench, ToolWindow::Problems, cx));
    assert!(!shown(&bench, ToolWindow::Project, cx));
    assert!(!bench.read_with(cx, |b, _| b.tool_layout().stripes));
    act(&bench, "FerrisIDEA.Layout.Apply:Gone", cx);
    assert_eq!(note(&bench, cx), "There is no layout named \"Gone\"");
    assert_eq!(Layouts::load(&file).selected(), Some("Two"));

    // Rename…: the field starts with the old name; a taken name is refused.
    act(&bench, "FerrisIDEA.Layout.Rename:Two", cx);
    let (title, _) = prompt(&bench, cx).expect("the name field is open");
    assert_eq!(title, "Rename Layout Two");
    assert_eq!(
        bench
            .read_with(cx, |b, cx| b.name_field_text(cx))
            .as_deref(),
        Some("Two")
    );
    name(&bench, "one", cx);
    assert_eq!(
        prompt(&bench, cx).unwrap().1.as_deref(),
        Some("A layout named \"One\" already exists")
    );
    name(&bench, "Zwei", cx);
    assert_eq!(prompt(&bench, cx), None);
    let renamed = Layouts::load(&file);
    assert_eq!(renamed.names(), ["One", "Zwei"]);
    assert_eq!(renamed.selected(), Some("Zwei"));

    // Delete…: not the selected layout; another one after its question.
    act(&bench, "FerrisIDEA.Layout.Delete:Zwei", cx);
    assert!(bench.read_with(cx, |b, _| b.drawn_question().is_none()));
    assert_eq!(
        note(&bench, cx),
        "Layout \"Zwei\" is selected; select another layout before deleting it"
    );
    act(&bench, "FerrisIDEA.Layout.Delete:One", cx);
    let question = bench
        .read_with(cx, |b, _| b.drawn_question().cloned())
        .expect("the question is asked");
    assert_eq!(question.message, "Delete layout \"One\"?");
    assert_eq!(question.buttons, ["Delete", "Cancel"]);
    assert_eq!(question.answer_for_key("enter"), Some(1));
    assert_eq!(question.answer_for_key("escape"), Some(1));
    bench.update_in(cx, |b, window, cx| b.answer_drawn_question(1, window, cx));
    assert_eq!(Layouts::load(&file).names(), ["One", "Zwei"]);
    act(&bench, "FerrisIDEA.Layout.Delete:One", cx);
    bench.update_in(cx, |b, window, cx| b.answer_drawn_question(0, window, cx));
    cx.run_until_parked();
    assert_eq!(note(&bench, cx), "Layout \"One\" deleted");
    assert_eq!(Layouts::load(&file).names(), ["Zwei"]);
}

/// SPEC-0333 AK-8 (behaviour 18): a layouts file that cannot be read leaves the default layout
/// and says so; a broken one is kept once, a newer one is never overwritten.
#[gpui::test]
fn a_broken_layouts_file_leaves_the_default_layout(cx: &mut TestAppContext) {
    let (root, file) = workspace("broken");
    std::fs::write(&file, "{ \"version\": 1, \"layouts\": [").unwrap();
    let (bench, cx) = window(&root, &file, cx);
    problems_to_the_right(&bench, cx);
    act(&bench, "RestoreDefaultLayout", cx);
    let said = note(&bench, cx);
    assert!(
        said.starts_with("The saved layouts could not be read")
            && said.ends_with("the default layout is used"),
        "{said}"
    );
    assert_eq!(side(&bench, ToolWindow::Problems, cx), Side::Bottom);
    assert!(shown(&bench, ToolWindow::Problems, cx));

    // Saving a layout replaces the broken file and keeps it once.
    act(&bench, "StoreNewLayout", cx);
    name(&bench, "Mine", cx);
    assert_eq!(prompt(&bench, cx), None);
    assert_eq!(Layouts::load(&file).names(), ["Mine"]);
    let kept = file.with_file_name("window-layouts.json.bak");
    assert_eq!(
        std::fs::read_to_string(&kept).unwrap(),
        "{ \"version\": 1, \"layouts\": ["
    );

    // A file of a newer version: the default layout, and nothing is written over it.
    let newer = "{\"version\": 7, \"layouts\": [{\"name\": \"Later\"}]}";
    std::fs::write(&file, newer).unwrap();
    assert_eq!(Layouts::load(&file).problem, Some(Problem::Newer(7)));
    act(&bench, "StoreNewLayout", cx);
    name(&bench, "Other", cx);
    let (_, error) = prompt(&bench, cx).expect("the name field stays open");
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.contains("newer Ferris IDEA")),
        "{error:?}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
    bench.update_in(cx, |b, window, cx| b.finish_name_prompt(false, window, cx));
    act(&bench, "RestoreFactoryDefaultLayout", cx);
    assert!(
        note(&bench, cx).contains("newer Ferris IDEA"),
        "{}",
        note(&bench, cx)
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
}

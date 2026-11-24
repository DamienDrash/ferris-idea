//! The steps of hand tests 56 and 76 that needed a real window (`SPEC-0129` v5 AK-9, `SPEC-0150`
//! v4 AK-9), in a window of the test harness, by clicks on controls found through the semantic tree
//! (`SPEC-0319`) and by real keys: the font size set to 16 and the line spacing to 1.4 with the +
//! buttons of Editor › Font, applied to the editor at once and saved; the shell of Settings ›
//! Terminal used by a new terminal tab; protected branches typed on the Version Control page and
//! saved; the saved file read again, as at a start, holds every value. The test has a settings file
//! of its own; the user's settings are never read or written. The editor font size is one value for
//! the whole process, so the test has a process of its own, as `font_size_actions.rs`. Further
//! tests check the gaps of OBS-0062: S1–S5 (`SPEC-0105` v7) and S6, S7 (`SPEC-0105` v8: Select
//! All in the fields, the rolling page list and page), and S8 of the window check of 2026-11-18
//! (`SPEC-0105` v10: a page's note goes with the page).

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::settings_model::Page;
use ferris_idea_workbench::{SettingsView, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

/// Both tests set the editor font size, one value for the whole process: they run one after the
/// other, never side by side (a parallel run saw the other test's size).
static FONT_SIZE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-settings-window-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

/// The node `id` of the semantic tree: its centre and value, if it is in view.
fn node(id: &str, cx: &mut VisualTestContext) -> Option<(gpui::Point<gpui::Pixels>, String)> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let n = tree.iter().find(|n| n.test_id() == Some(id))?;
        let r = n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
        Some((
            point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)),
            n.raw_value().unwrap_or_default().to_owned(),
        ))
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let (at, _) = node(id, cx).unwrap_or_else(|| panic!("no control {id} in view"));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn value(id: &str, cx: &mut VisualTestContext) -> String {
    node(id, cx).map(|n| n.1).unwrap_or_default()
}

fn dialog(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<SettingsView> {
    bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("Settings is open")
}

#[gpui::test]
fn font_shell_and_protected_branches_in_the_window(cx: &mut TestAppContext) {
    let _serial = FONT_SIZE.lock().unwrap_or_else(|e| e.into_inner());
    let settings = folder("settings");
    let file = settings.join("settings.toml");
    let root = folder("project");
    std::fs::write(root.join("notes.txt"), "alpha\n").unwrap();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("notes.txt"), window, cx)
    });
    cx.run_until_parked();

    // Editor › Font: + until the size is 16 and the line spacing 1.4 (hand test 76).
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Font, window, cx));
    cx.run_until_parked();
    let size: f32 = value("settings-font-value-1", cx).parse().expect("a size");
    assert!(size <= 16.0, "starts at {size}");
    for _ in 0..(16.0 - size) as usize {
        click("settings-font-more-1", cx);
    }
    assert_eq!(value("settings-font-value-1", cx), "16");
    for _ in 0..20 {
        if value("settings-font-value-2", cx) == "1.4" {
            break;
        }
        let now: f32 = value("settings-font-value-2", cx).parse().unwrap();
        click(
            if now < 1.4 {
                "settings-font-more-2"
            } else {
                "settings-font-less-2"
            },
            cx,
        );
    }
    assert_eq!(value("settings-font-value-2", cx), "1.4");
    // Applied at once and saved.
    assert_eq!(ferris_idea_workbench::fonts::editor_font_size(), 16.0);
    assert!((ferris_idea_workbench::fonts::editor_line_height() - 16.0 * 1.4).abs() < 1.0);
    let saved = ferris_idea_core::load_ide_settings(&file);
    assert_eq!(saved.editor.font_size, 16.0);
    assert!((saved.editor.line_spacing - 1.4).abs() < 1e-3, "{saved:?}");

    // Settings › Terminal: the shell; a new terminal tab starts it (hand test 56).
    let view = dialog(&bench, cx);
    view.update_in(cx, |v, window, cx| {
        v.show(Page::Terminal, cx);
        v.set_shell_text("/bin/bash", cx);
        v.focus_shell(window, cx);
    });
    cx.run_until_parked();
    keys(cx, "enter");

    // Version Control › Protected branches: typed and saved with Enter (hand test 76).
    view.update(cx, |v, cx| v.show(Page::VersionControl, cx));
    cx.run_until_parked();
    assert!(
        node("settings-protected-branches", cx).is_some(),
        "the field is in view"
    );
    click("settings-protected-branches", cx);
    keys(cx, "cmd-a");
    cx.simulate_input("main, release/*");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(
        ferris_idea_core::load_ide_settings(&file)
            .vcs
            .protected_branches,
        "main, release/*"
    );
    assert_eq!(value("settings-protected-branches", cx), "main, release/*");
    keys(cx, "escape");
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));

    bench.update_in(cx, |b, window, cx| b.new_terminal_tab(window, cx));
    cx.run_until_parked();
    let terminal = bench
        .read_with(cx, |b, _| b.terminal())
        .expect("a terminal tab");
    assert_eq!(
        terminal.read_with(cx, |t, _| t.command().to_vec()),
        ["/bin/bash"]
    );

    // The saved file read again, as at a start, holds every value.
    let again = ferris_idea_core::load_ide_settings(&file);
    assert_eq!(again.editor.font_size, 16.0);
    assert!((again.editor.line_spacing - 1.4).abs() < 1e-3);
    assert_eq!(again.terminal.shell.as_deref(), Some("/bin/bash"));
    assert_eq!(again.vcs.protected_branches, "main, release/*");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}

/// Every node of the semantic tree with test id `id`: its name.
fn names(id: &str, cx: &mut VisualTestContext) -> Vec<String> {
    cx.run_until_parked();
    cx.update(|window, _| {
        semantics::tree(semantics::window_id(window))
            .map(|t| {
                t.iter()
                    .filter(|n| n.test_id() == Some(id))
                    .map(|n| n.name().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// `SPEC-0105` v6 AK-12, `SPEC-0129` v7 AK-10 and `SPEC-0337` v2 AK-7 (OBS-0062 S1–S5): the
/// search field shows the text and the page marks the label it found; Revert changes and Restore
/// defaults act on the page shown; a page changed from its defaults has a dot in the list; the
/// Font page takes the family from a list, the size from its field and keeps a text of one's own
/// in the preview; Advanced Settings has its own search and "Show modified only".
#[gpui::test]
fn search_revert_defaults_font_and_advanced(cx: &mut TestAppContext) {
    let _serial = FONT_SIZE.lock().unwrap_or_else(|e| e.into_inner());
    let settings = folder("gaps");
    let file = settings.join("settings.toml");
    let root = folder("gaps-project");
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Appearance, window, cx)
    });
    cx.run_until_parked();
    let view = dialog(&bench, cx);

    // S1: the search field shows what is typed; the page found marks the label.
    assert_eq!(value("settings-search", cx), "");
    cx.simulate_input("spacing");
    cx.run_until_parked();
    assert_eq!(value("settings-search", cx), "spacing");
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Font);
    assert_eq!(names("settings-hit", cx), ["Line spacing"]);
    for _ in 0.."spacing".len() {
        keys(cx, "backspace");
    }
    assert_eq!(value("settings-search", cx), "");
    assert!(
        names("settings-hit", cx).is_empty(),
        "no mark without a search"
    );

    // S2, S3: a change offers Revert changes and Restore defaults and puts a dot after the page.
    assert!(node("settings-revert", cx).is_none());
    assert!(node("settings-page-modified-Font", cx).is_none());
    let size = ferris_idea_core::IdeSettings::default().editor.font_size;
    // Back to the page shown before the search; the Font page by a click in the list.
    click("settings-page-Font", cx);
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Font);
    click("settings-font-more-1", cx);
    click("settings-font-more-1", cx);
    assert_eq!(
        value("settings-font-value-1", cx),
        format!("{}", size + 2.0)
    );
    assert!(node("settings-revert", cx).is_some());
    assert!(node("settings-restore-defaults", cx).is_some());
    assert!(node("settings-page-modified-Font", cx).is_some());
    click("settings-revert", cx);
    assert_eq!(value("settings-font-value-1", cx), format!("{size}"));
    assert!(
        node("settings-revert", cx).is_none(),
        "nothing left to revert"
    );
    assert_eq!(
        ferris_idea_core::load_ide_settings(&file).editor.font_size,
        size
    );

    // S4: the size typed into its field; Restore defaults takes it back.
    click("settings-font-value-1", cx);
    keys(cx, "cmd-a");
    cx.simulate_input("18");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(
        ferris_idea_core::load_ide_settings(&file).editor.font_size,
        18.0
    );
    assert_eq!(value("settings-font-value-1", cx), "18");
    click("settings-restore-defaults", cx);
    assert_eq!(
        ferris_idea_core::load_ide_settings(&file).editor.font_size,
        size
    );
    assert!(node("settings-page-modified-Font", cx).is_none());
    // S8 (`SPEC-0105` v10 AK-17): the note belongs to the Font page; Tools › Terminal, chosen in
    // the list, shows none, and neither does Font again.
    assert_eq!(
        view.read_with(cx, |v, _| v.note().map(str::to_owned)),
        Some("Font: defaults restored".to_owned())
    );
    click("settings-page-Terminal", cx);
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Terminal);
    assert_eq!(view.read_with(cx, |v, _| v.note().map(str::to_owned)), None);
    click("settings-page-Font", cx);
    assert_eq!(view.read_with(cx, |v, _| v.note().map(str::to_owned)), None);
    // The family from the list.
    let fonts = view.read_with(cx, |v, _| v.font_choices().to_vec());
    if fonts.len() > 1 {
        click("settings-font-family-1", cx);
        assert_eq!(
            ferris_idea_core::load_ide_settings(&file)
                .editor
                .font_family,
            fonts[1]
        );
        click("settings-font-family-0", cx);
    }
    // The preview keeps a text of one's own.
    click("settings-font-preview", cx);
    keys(cx, "cmd-a");
    cx.simulate_input("Grüße 0O1lI");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, cx| v.font_preview_text(cx)),
        "Grüße 0O1lI"
    );
    keys(cx, "escape");

    // S5: Advanced Settings, its own search and "Show modified only".
    view.update(cx, |v, cx| v.show(Page::Advanced, cx));
    cx.run_until_parked();
    let all = view.read_with(cx, |v, _| v.advanced_rows().len());
    assert!(all > 1);
    click("settings-advanced-modified-only", cx);
    assert_eq!(
        view.read_with(cx, |v, _| v.advanced_rows().len()),
        0,
        "nothing modified"
    );
    click("settings-advanced-modified-only", cx);
    click("settings-advanced-search", cx);
    cx.simulate_input("report");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.advanced_query().to_owned()),
        "report"
    );
    let found = view.read_with(cx, |v, _| v.advanced_rows().len());
    assert!(found >= 1 && found < all, "{found} of {all}");
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}

/// Draws the window once more, so the semantic tree shows what the wheel or a drag did.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

/// The node `id`: its bounds unclipped (x, y, width, height), whether it is wholly in view, and
/// whether it is selected.
type Shown = ((f32, f32, f32, f32), bool, bool);

fn shown(id: &str, cx: &mut VisualTestContext) -> Option<Shown> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let n = tree.iter().find(|n| n.test_id() == Some(id))?;
        let b = n.bounds();
        let whole = n
            .visible_bounds
            .is_some_and(|v| b.h > 0. && v.h >= b.h - 0.5 && v.w >= b.w - 0.5);
        Some(((b.x, b.y, b.w, b.h), whole, n.selected() == Some(true)))
    })
}

fn in_view(id: &str, cx: &mut VisualTestContext) -> bool {
    shown(id, cx).is_some_and(|s| s.1)
}

/// Turns the wheel `dy` points (negative: towards the end) over the node `id`.
fn wheel(id: &str, dy: f32, cx: &mut VisualTestContext) {
    let (at, _) = node(id, cx).unwrap_or_else(|| panic!("no control {id} in view"));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(cx);
}

/// `SPEC-0105` v8 AK-14 and `SPEC-0129` v8 AK-11 (OBS-0062 S6, S7): ⌘A and the menu bar's Select
/// All act on the search and on the focused field, never on the editor behind; the page list and a
/// tall page roll by the wheel, the scroll bar and ↓, and in a window 901 points high the whole
/// list is in view.
#[gpui::test]
fn select_all_and_rolling_lists(cx: &mut TestAppContext) {
    use gpui::MouseButton;
    let _serial = FONT_SIZE.lock().unwrap_or_else(|e| e.into_inner());
    let settings = folder("rolling");
    let file = settings.join("settings.toml");
    let root = folder("rolling-project");
    std::fs::write(root.join("notes.txt"), "alpha\nbeta\n").unwrap();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    // The window of the recheck (OBS-0062 S7).
    cx.simulate_resize(gpui::size(px(1300.), px(901.)));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("notes.txt"), window, cx)
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor");
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Appearance, window, cx)
    });
    cx.run_until_parked();
    let view = dialog(&bench, cx);

    // S7: the dialog takes the window's height, so the whole list is in view.
    assert!(
        in_view("settings-page-Advanced Settings", cx),
        "the last page is in view without rolling"
    );

    // S6: ⌘A marks the search, ⌫ empties it and every page is listed again.
    cx.simulate_input("external");
    cx.run_until_parked();
    assert_eq!(value("settings-search", cx), "external");
    assert!(view.read_with(cx, |v, _| v.pages().len()) < Page::ALL.len());
    keys(cx, "cmd-a");
    assert!(view.read_with(cx, |v, _| v.search_marked()));
    assert!(
        shown("settings-search", cx).is_some_and(|s| s.2),
        "the field's node is selected while its text is marked"
    );
    keys(cx, "backspace");
    assert_eq!(value("settings-search", cx), "");
    assert_eq!(view.read_with(cx, |v, _| v.pages().len()), Page::ALL.len());
    // Text typed over the mark replaces it.
    cx.simulate_input("font");
    cx.run_until_parked();
    keys(cx, "cmd-a");
    cx.simulate_input("keymap");
    cx.run_until_parked();
    assert_eq!(value("settings-search", cx), "keymap");
    // The menu bar takes ⌘A first on macOS: its Select All marks the search as well, and the
    // status bar does not call it unavailable.
    bench.update_in(cx, |b, window, cx| b.menu_action("$SelectAll", window, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.search_marked()));
    let status = bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned());
    assert!(!status.contains("not available"), "{status}");
    keys(cx, "backspace");
    assert_eq!(value("settings-search", cx), "");

    // In the Size field the menu bar's Select All selects the field's text; "20" replaces it, and
    // the editor behind keeps its caret without a selection (SPEC-0129 v8 behaviour 11).
    view.update(cx, |v, cx| v.show(Page::Font, cx));
    cx.run_until_parked();
    click("settings-font-value-1", cx);
    bench.update_in(cx, |b, window, cx| b.menu_action("$SelectAll", window, cx));
    cx.run_until_parked();
    cx.simulate_input("20");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(
        ferris_idea_core::load_ide_settings(&file).editor.font_size,
        20.0
    );
    assert!(
        !editor.read_with(cx, |e, _| e.has_selection()),
        "Select All never reached the editor behind the dialog"
    );
    click("settings-restore-defaults", cx);

    // S7 in a low window: the list is cut and rolls by the wheel.
    view.update(cx, |v, cx| v.show(Page::Appearance, cx));
    cx.simulate_resize(gpui::size(px(1300.), px(460.)));
    draw(cx);
    assert!(
        !in_view("settings-page-Advanced Settings", cx),
        "the list does not fit"
    );
    wheel("settings-page-Keymap", -2000., cx);
    assert!(
        in_view("settings-page-Advanced Settings", cx),
        "the wheel rolls the list to its end"
    );
    assert!(!in_view("settings-page-Appearance", cx));
    // The thumb, now at the foot of the bar, dragged back to the top.
    let ((bar_x, bar_y, bar_w, bar_h), _, _) =
        shown("settings-pages-scrollbar", cx).expect("a scroll bar");
    let x = px(bar_x + bar_w / 2.);
    let from = point(x, px(bar_y + bar_h - 6.));
    cx.simulate_mouse_move(from, None, Modifiers::none());
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(x, px(bar_y + bar_h - 16.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    let to = point(x, px(bar_y - 100.));
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert!(
        in_view("settings-page-Appearance", cx),
        "the thumb dragged to the top rolls the list back"
    );
    // A click on a page gives the list the keys: ↓ goes page by page and keeps the page in view.
    click("settings-page-Appearance", cx);
    for _ in 1..Page::ALL.len() {
        keys(cx, "down");
    }
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Advanced);
    assert!(
        in_view("settings-page-Advanced Settings", cx),
        "the list follows ↓"
    );
    assert!(
        shown("settings-page-Advanced Settings", cx).is_some_and(|s| s.2),
        "the page shown is the selected row"
    );

    // A page taller than its area rolls: the Font page's preview, cut at first.
    view.update(cx, |v, cx| v.show(Page::Font, cx));
    draw(cx);
    assert!(
        !in_view("settings-font-preview", cx),
        "the preview is cut in a low window"
    );
    assert!(shown("settings-page-scrollbar", cx).is_some());
    wheel("settings-font-less-1", -1000., cx);
    assert!(
        in_view("settings-font-preview", cx),
        "the wheel rolls the page"
    );
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}

/// `SPEC-0105` v10 AK-18 (window check of 2026-11-18, K1): with a search typed on the keymap page,
/// a click on a row or Tab from the search gives the keys to the list: ⌫ removes the row's
/// shortcuts instead of shortening the search, ⇧⌫ restores them, ↓ moves; Escape gives the keys
/// back to the search instead of closing the dialog.
#[gpui::test]
fn keymap_rows_take_the_keys_after_a_click_or_tab(cx: &mut TestAppContext) {
    let settings = folder("keymap-keys");
    let file = settings.join("settings.toml");
    let root = folder("keymap-keys-project");
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        // The test's own keymap file: the user's is never written.
        cx.set_global(ferris_idea_workbench::keymaps::UserKeymapFile(
            settings.join("keymaps/FerrisIDEA.toml"),
        ));
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Keymap, window, cx)
    });
    cx.run_until_parked();
    let view = dialog(&bench, cx);
    cx.simulate_input("reformat");
    cx.run_until_parked();
    assert_eq!(value("settings-search", cx), "reformat");
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Keymap);
    // A row found by the search that has a shortcut.
    let row = view
        .read_with(cx, |v, _| {
            v.rows().iter().position(|r| !r.shortcuts.is_empty())
        })
        .expect("an action with a shortcut matches \"reformat\"");
    let title = view.read_with(cx, |v, _| v.rows()[row].title.clone());
    let note = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.note().map(str::to_owned));

    // A click on the row, then ⌫: the shortcuts go, the search stays.
    click(&format!("settings-keymap-row-{row}"), cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected()), row);
    keys(cx, "backspace");
    assert_eq!(
        value("settings-search", cx),
        "reformat",
        "⌫ left the search alone"
    );
    assert_eq!(note(cx), Some(format!("{title}: shortcuts removed")));
    // The row stays chosen without shortcuts; ⇧⌫ restores them.
    assert_eq!(
        view.read_with(cx, |v, _| v.rows()[v.selected()].title.clone()),
        title
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.rows()[v.selected()].shortcuts.clone()),
        ""
    );
    keys(cx, "shift-backspace");
    assert_eq!(
        note(cx),
        Some(format!("{title}: bundled shortcuts restored"))
    );
    assert_eq!(value("settings-search", cx), "reformat");
    // ↓ goes on in the list (when there is a next row).
    assert_ne!(
        view.read_with(cx, |v, _| v.rows()[v.selected()].shortcuts.clone()),
        ""
    );
    let n = view.read_with(cx, |v, _| v.rows().len());
    keys(cx, "down");
    assert_eq!(
        view.read_with(cx, |v, _| v.selected()),
        (row + 1).min(n - 1)
    );
    keys(cx, "up");
    assert_eq!(view.read_with(cx, |v, _| v.selected()), row);

    // Escape gives the keys back to the search; the dialog stays, ⌫ shortens the search.
    keys(cx, "escape");
    assert!(
        bench.read_with(cx, |b, _| b.settings_dialog()).is_some(),
        "Escape on the list keeps the dialog"
    );
    keys(cx, "backspace");
    assert_eq!(value("settings-search", cx), "reforma");
    cx.simulate_input("t");
    cx.run_until_parked();
    assert_eq!(value("settings-search", cx), "reformat");

    // Tab from the search gives the keys to the list again: ⌫ removes.
    keys(cx, "tab");
    assert_eq!(
        view.read_with(cx, |v, _| v.page()),
        Page::Keymap,
        "Tab stays on Keymap"
    );
    let selected = view.read_with(cx, |v, _| v.selected());
    let chosen = view.read_with(cx, |v, _| v.rows()[selected].title.clone());
    keys(cx, "backspace");
    assert_eq!(value("settings-search", cx), "reformat");
    assert_eq!(note(cx), Some(format!("{chosen}: shortcuts removed")));
    keys(cx, "shift-backspace");
    assert_eq!(
        note(cx),
        Some(format!("{chosen}: bundled shortcuts restored"))
    );

    // Escape twice: back to the search, then the dialog closes.
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog()).is_some());
    keys(cx, "escape");
    assert!(
        bench.read_with(cx, |b, _| b.settings_dialog()).is_none(),
        "Escape from the search closes the dialog"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}

/// The checked state and the selection of the semantic tree's node `id`.
fn check_state(id: &str, cx: &mut VisualTestContext) -> (Option<bool>, Option<bool>) {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a semantic tree");
        let n = tree
            .iter()
            .find(|n| n.test_id() == Some(id))
            .unwrap_or_else(|| panic!("no node {id}"));
        (n.checked(), n.selected())
    })
}

/// `SPEC-0105` v11 AK-19 (window check of 2026-11-18, G1): on Editor › General and System
/// Settings a click on an option's label switches it and selects its row; Space and Enter then
/// switch the selected option and never type into the search, which stays empty; with a search
/// typed, Space switches after the click and leaves the search as typed, Escape gives the keys
/// back to the search.
#[gpui::test]
fn options_switch_by_a_click_on_the_label_and_then_by_keys(cx: &mut TestAppContext) {
    // Opening the workbench sets the process's editor font size: one after the other with the
    // font tests.
    let _serial = FONT_SIZE.lock().unwrap_or_else(|e| e.into_inner());
    let settings = folder("option-labels");
    let file = settings.join("settings.toml");
    let root = folder("option-labels-project");
    std::fs::write(root.join("notes.txt"), "alpha\n").unwrap();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        // The test's own settings file: the user's is never read or written.
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    // An editor behind the dialog, as in the window: keys that leave the dialog would type there.
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("notes.txt"), window, cx)
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Editor, window, cx)
    });
    cx.run_until_parked();
    let view = dialog(&bench, cx);
    let strip = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| v.ide().editor.strip_trailing_spaces)
    };
    let before = strip(cx);
    assert_eq!(value("settings-search", cx), "");

    // A click on the label of "Remove trailing spaces on save": switched and selected.
    click("settings-editor-0-label", cx);
    assert_eq!(
        strip(cx),
        !before,
        "a click on the label switches the option"
    );
    assert_eq!(view.read_with(cx, |v, _| v.selected()), 0);
    assert_eq!(
        check_state("settings-editor-0", cx),
        (Some(!before), Some(true))
    );
    // Space and Enter switch the selected option; the search stays empty.
    keys(cx, "space");
    assert_eq!(strip(cx), before, "Space switches the selected option");
    assert_eq!(
        value("settings-search", cx),
        "",
        "Space typed nothing into the search"
    );
    keys(cx, "enter");
    assert_eq!(strip(cx), !before, "Enter switches the selected option");
    assert_eq!(value("settings-search", cx), "");
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Editor);
    // The value went to the test's settings file.
    let saved = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        saved.contains(&format!("strip_trailing_spaces = {}", !before)),
        "saved: {saved}"
    );

    // A click on another label moves the selection there; Space switches that one only.
    let hints =
        |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.ide().editor.parameter_hints);
    let hints_before = hints(cx);
    click("settings-editor-3-label", cx);
    assert_eq!(hints(cx), !hints_before);
    assert_eq!(view.read_with(cx, |v, _| v.selected()), 3);
    assert_eq!(check_state("settings-editor-0", cx).1, Some(false));
    keys(cx, "space");
    assert_eq!(hints(cx), hints_before);
    assert_eq!(strip(cx), !before, "the first option stays as it was");

    // System Settings: the same for its options.
    click("settings-page-System Settings", cx);
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::SystemSettings);
    let reopen =
        |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.ide().system.reopen_last_project);
    let reopen_before = reopen(cx);
    click("settings-system-1-label", cx);
    assert_eq!(reopen(cx), !reopen_before);
    keys(cx, "space");
    assert_eq!(reopen(cx), reopen_before);
    assert_eq!(value("settings-search", cx), "");

    // With a search typed: the click and Space switch, the search stays as typed.
    click("settings-search", cx);
    cx.simulate_input("trailing");
    cx.run_until_parked();
    assert_eq!(value("settings-search", cx), "trailing");
    assert_eq!(view.read_with(cx, |v, _| v.page()), Page::Editor);
    let now = strip(cx);
    click("settings-editor-0-label", cx);
    assert_eq!(strip(cx), !now);
    keys(cx, "space");
    assert_eq!(
        strip(cx),
        now,
        "Space after the click switches, also with a search typed"
    );
    assert_eq!(value("settings-search", cx), "trailing");
    // Escape gives the keys back to the search; the dialog stays and typing goes on there.
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog()).is_some());
    keys(cx, "backspace");
    assert_eq!(value("settings-search", cx), "trailin");
    // The editor behind never got a key.
    let text = bench.read_with(cx, |b, cx| b.active_text(cx));
    assert_eq!(text.as_deref(), Some("alpha\n"));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}

//! The pages of SPEC-0337 in a window (AK-5): the outline with Appearance & Behavior first,
//! System Settings switched and saved, the question before quitting with nothing pending, saving
//! when the window loses the focus, Notifications keeping failures only, and Advanced Settings
//! searched, switched, reset and edited. The test has a settings file of its own; the user's
//! settings are never read or written. Nothing is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::notifications::Kind;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::settings_model::Page;
use ferris_idea_workbench::{SettingsView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-settings-pages-{tag}-{}",
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

fn dialog(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<SettingsView> {
    bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("Settings is open")
}

fn page(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Page {
    let view = dialog(bench, cx);
    view.read_with(cx, |v, _| v.page())
}

fn saved(file: &Path) -> ferris_idea_core::IdeSettings {
    ferris_idea_core::load_ide_settings(file)
}

/// Shows `note` in the status bar and lets the window draw it (the Notifications log records it).
fn note(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, note: &str) {
    bench.update(cx, |b, cx| b.hold_status(note.into(), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

#[gpui::test]
fn settings_pages_of_appearance_and_behavior(cx: &mut TestAppContext) {
    let settings = folder("settings");
    let file = settings.join("settings.toml");
    let root = folder("project");
    std::fs::write(root.join("notes.txt"), "alpha\n").unwrap();
    // This test's own settings and theme file, before anything reads or writes them.
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
    cx.update(|_, cx| {
        ferris_idea_workbench::project_windows::register_workbench(bench.downgrade(), cx)
    });
    bench.update_in(cx, |b, window, cx| b.watch_window(window, cx));

    // The outline (behaviour 1).
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Appearance, window, cx)
    });
    cx.run_until_parked();
    let list = dialog(&bench, cx).read_with(cx, |v, _| v.pages());
    assert_eq!(
        list[..5],
        [
            Page::Appearance,
            Page::SystemSettings,
            Page::Notifications,
            Page::TrustedLocations,
            Page::Keymap
        ]
    );
    assert_eq!(list.last(), Some(&Page::Advanced));
    assert!(
        list[..4]
            .iter()
            .all(|p| p.group() == Some("Appearance & Behavior"))
    );

    // Appearance › Startup animation, the last row: on by default, Enter switches it and saves
    // it (SPEC-0355 v2 behaviour 23).
    let last = ferris_idea_workbench::palette::ThemeId::ALL.len() + 3;
    dialog(&bench, cx).update(cx, |v, cx| v.select_row(last, cx));
    keys(cx, "enter");
    assert!(!saved(&file).appearance.startup_animation);
    keys(cx, "enter");
    assert!(saved(&file).appearance.startup_animation);

    // System Settings: Tab goes there; Space switches each row and saves it (behaviour 2).
    keys(cx, "tab");
    assert_eq!(page(&bench, cx), Page::SystemSettings);
    keys(cx, "space");
    assert!(saved(&file).system.confirm_quit);
    keys(cx, "down space");
    assert!(!saved(&file).system.reopen_last_project);
    keys(cx, "space");
    assert!(
        saved(&file).system.reopen_last_project,
        "Space again switches it back"
    );
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));

    // Quit with "Confirm before quitting" on asks; Cancel keeps the window. The open documents are
    // known process-wide, so another test of the same run may hold a modified file while this one
    // quits, and its window may close between the question and the look at the pending work:
    // the plain question is only expected while nothing was pending before and after it. Until
    // then the question is cancelled and asked again, for at most twenty seconds.
    let nothing_pending = |cx: &mut VisualTestContext| {
        let (unsaved, running) = cx.update(|_, cx| WorkbenchView::pending_work(cx));
        unsaved.is_empty() && running.is_empty()
    };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let before = nothing_pending(cx);
        cx.update(|_, cx| WorkbenchView::quit(cx));
        cx.run_until_parked();
        let after = nothing_pending(cx);
        let question = bench
            .read_with(cx, |b, _| b.drawn_question().cloned())
            .expect("quitting asks first");
        let cancel = question.cancel.expect("a Cancel answer");
        bench.update_in(cx, |b, window, cx| {
            b.answer_drawn_question(cancel, window, cx)
        });
        cx.run_until_parked();
        assert!(bench.read_with(cx, |b, _| b.drawn_question().is_none()));
        if before && after {
            assert_eq!(question.message, "Quit Ferris IDEA?");
            assert_eq!(question.buttons, ["Quit", "Cancel"]);
            break;
        }
        if std::time::Instant::now() > end {
            // Other tests kept files modified the whole time: quitting asked, which is the point.
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Saving when the window loses the focus: off, a modified file stays modified.
    let notes = root.join("notes.txt");
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.open(&notes, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_input("x");
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    cx.deactivate_window();
    assert!(
        editor.read_with(cx, |e, _| e.is_modified()),
        "off by default: nothing is saved"
    );
    assert_eq!(std::fs::read_to_string(&notes).unwrap(), "alpha\n");
    // On, losing the focus saves it as Save All does.
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::SystemSettings, window, cx)
    });
    cx.run_until_parked();
    keys(cx, "down down space escape");
    assert!(saved(&file).system.save_on_frame_deactivation);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.deactivate_window();
    cx.run_until_parked();
    assert!(!editor.read_with(cx, |e, _| e.is_modified()), "saved");
    assert_eq!(std::fs::read_to_string(&notes).unwrap(), "xalpha\n");

    // Notifications off: a failure is kept, an informational note is not (behaviour 3).
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Notifications, window, cx)
    });
    cx.run_until_parked();
    keys(cx, "space escape");
    assert!(!saved(&file).notifications.log_info);
    note(&bench, cx, "Copied 3 lines");
    note(&bench, cx, "Cannot open missing.txt: no such file");
    let listed: Vec<(String, Kind)> = bench.read_with(cx, |b, _| {
        b.notifications()
            .into_iter()
            .map(|n| (n.text, n.kind))
            .collect()
    });
    assert!(
        listed
            .iter()
            .any(|(t, k)| t.starts_with("Cannot open") && *k == Kind::Error),
        "{listed:?}"
    );
    assert!(
        !listed.iter().any(|(t, _)| t == "Copied 3 lines"),
        "{listed:?}"
    );

    // Advanced Settings: the search by a word of a row, Enter switches it, Delete resets it.
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Appearance, window, cx)
    });
    cx.run_until_parked();
    // The search takes text through the input handler of the drawn dialog in the active window.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.simulate_input("packages");
    cx.run_until_parked();
    assert_eq!(page(&bench, cx), Page::Advanced);
    let view = dialog(&bench, cx);
    let rows: Vec<&str> =
        view.read_with(cx, |v, _| v.advanced_rows().iter().map(|a| a.key).collect());
    assert_eq!(rows, ["project_view.compact_middle_packages"]);
    keys(cx, "enter");
    assert!(!saved(&file).project_view.compact_middle_packages);
    keys(cx, "delete");
    assert!(saved(&file).project_view.compact_middle_packages, "reset");

    // A text row is edited in the field: an http address is refused, an https one saved.
    keys(
        cx,
        "backspace backspace backspace backspace backspace backspace backspace backspace",
    );
    // The emptied search lists every page; the page is chosen again in the list.
    assert_eq!(view.read_with(cx, |v, _| v.pages()), Page::ALL);
    view.update(cx, |v, cx| v.show(Page::Advanced, cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.advanced_rows().len()),
        ferris_idea_core::advanced_settings::ADVANCED.len()
    );
    let at = view.read_with(cx, |v, _| {
        v.advanced_rows()
            .iter()
            .position(|a| a.key == "database.report_url")
            .unwrap()
    });
    view.update(cx, |v, cx| v.select_row(at, cx));
    keys(cx, "enter");
    view.update(cx, |v, cx| {
        v.set_advanced_text("http://example.test/{title}/{body}", cx)
    });
    keys(cx, "enter");
    assert_eq!(saved(&file).database.report_url, "", "refused");
    assert!(
        view.read_with(cx, |v, _| v.note().map(str::to_owned))
            .is_some_and(|n| n.contains("https://")),
        "the note says why"
    );
    let good = "https://example.test/new?t={title}&b={body}";
    view.update(cx, |v, cx| v.set_advanced_text(good, cx));
    keys(cx, "enter");
    assert_eq!(saved(&file).database.report_url, good);

    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&root);
}

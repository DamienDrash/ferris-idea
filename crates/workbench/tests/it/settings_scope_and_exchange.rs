//! SPEC-0105 v9 AK-16 in a window of the test harness: each settings page names where its
//! settings are kept, project pages are marked in the list, and File › Manage IDE Settings
//! exports a settings file that the import preview brings back only after confirmation. The test
//! has a settings folder and a project of its own; the user's settings are never read or written.
//! Nothing is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::keymaps::UserKeymapFile;
use idecraft_workbench::settings_model::Page;
use idecraft_workbench::{WorkbenchView, semantics};

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "idecraft-settings-exchange-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// The name of the node `id` of the semantic tree, drawn in view or not.
fn node_name(id: &str, cx: &mut VisualTestContext) -> Option<String> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let n = tree.iter().find(|n| n.test_id() == Some(id))?;
        Some(n.name().unwrap_or_default().to_owned())
    })
}

fn act(bench: &Entity<WorkbenchView>, id: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn saved(file: &Path) -> idecraft_core::IdeSettings {
    idecraft_core::load_ide_settings(file)
}

fn set_protected(branches: &str, cx: &mut VisualTestContext) {
    let branches = branches.to_owned();
    cx.update(|_, cx| {
        let mut s = idecraft_workbench::appearance::ide_settings(cx);
        s.vcs.protected_branches = branches;
        s.terminal.shell = Some("/bin/zsh".into());
        idecraft_workbench::appearance::set_ide_settings(s, cx).unwrap();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn pages_name_their_scope_and_settings_go_out_and_come_back(cx: &mut TestAppContext) {
    let settings = folder("settings");
    let file = settings.join("settings.toml");
    let root = folder("project");
    std::fs::write(root.join("notes.txt"), "alpha\n").unwrap();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    // This test's own settings, theme and keymap files, before anything reads or writes them.
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(UserKeymapFile(settings.join("keymaps/IDECraft.toml")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(901.)));
    cx.run_until_parked();

    // Behaviour 13: the bar names where the page's settings are kept; only Inspections is a
    // page of the project.
    bench.update_in(cx, |b, window, cx| b.open_settings(Page::Font, window, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let scope = node_name("settings-page-scope", cx).expect("the scope text of Font");
    assert_eq!(scope, "IDE · settings.toml, the same for every project");
    assert!(node_name("settings-page-project-Inspections", cx).is_some());
    assert!(node_name("settings-page-project-Font", cx).is_none());
    assert!(node_name("settings-page-project-Keymap", cx).is_none());
    let view = bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("Settings is open");
    view.update(cx, |v, cx| v.show(Page::Inspections, cx));
    cx.update(|window, cx| window.draw(cx).clear());
    let scope = node_name("settings-page-scope", cx).expect("the scope text of Inspections");
    assert!(
        scope.starts_with("Project · .idea/inspectionProfiles/Project_Default.xml"),
        "{scope}"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.settings_dialog().is_none()));

    // Behaviour 14: Export Settings writes the shared sections into the file chosen in the save
    // dialog, which starts in the project's folder; the terminal stays out.
    set_protected("main, release/*", cx);
    let exported = root.join("team.idcsettings");
    act(&bench, "ExportSettings", cx);
    let target = exported.clone();
    let start = root.clone();
    cx.simulate_new_path_selection(move |dir| {
        assert_eq!(
            dir,
            start.as_path(),
            "the save dialog starts in the project"
        );
        Some(target)
    });
    cx.run_until_parked();
    let text = std::fs::read_to_string(&exported).expect("the settings file is written");
    assert!(text.contains("release/*"), "{text}");
    assert!(
        !text.contains("/bin/zsh"),
        "the terminal is not shared: {text}"
    );
    let note = status(&bench, cx);
    assert!(
        note.starts_with(&format!("Exported the settings to {}", exported.display()))
            && note.contains("terminal"),
        "{note}"
    );

    // Import: the preview changes nothing until Import is confirmed; then the exported value is
    // back in the settings file and in the window.
    set_protected("main", cx);
    assert_eq!(saved(&file).vcs.protected_branches, "main");
    bench.update_in(cx, |b, window, cx| b.open_import(&exported, window, cx));
    cx.run_until_parked();
    let preview = bench
        .read_with(cx, |b, _| b.import_preview())
        .expect("the import preview is shown");
    assert!(preview.read_with(cx, |p, _| p.preview().problems.is_empty()));
    assert_eq!(saved(&file).vcs.protected_branches, "main", "nothing yet");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.import_preview().is_none()));
    assert_eq!(saved(&file).vcs.protected_branches, "main, release/*");
    let live = cx.update(|_, cx| idecraft_workbench::appearance::ide_settings(cx));
    assert_eq!(live.vcs.protected_branches, "main, release/*");
    assert_eq!(
        live.terminal.shell.as_deref(),
        Some("/bin/zsh"),
        "the import leaves the terminal alone"
    );

    // A file that cannot be written: the reason in the status bar, nothing written.
    let nowhere = root.join("missing-folder/out.idcsettings");
    let written = bench.update(cx, |b, cx| b.export_settings_to(&nowhere, cx));
    assert!(!written && !nowhere.exists());
    let note = status(&bench, cx);
    assert!(note.starts_with("Settings not exported to"), "{note}");
}

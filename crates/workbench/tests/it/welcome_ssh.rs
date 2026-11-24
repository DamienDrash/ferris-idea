//! The Welcome window's section Remote Development › SSH in the test harness (`SPEC-0355` v3
//! AK-8): the recent SSH projects and the configured host from the test's own settings folder,
//! remote state folder and SSH configuration; the search; the New Project form from a host row
//! and with ⌘N, its check of the port; going back; removing a recent project from the file.
//! Nothing connects: `ssh` only resolves the test's configuration (`ssh -G -F <its own>`), and a
//! read of the user's `~/.ssh/config` fails the test (`isolate_ssh`). Connecting to a real host,
//! the folder chooser after it and the project window are left to the window check (the code of
//! `SPEC-0317` AK-3 and AK-7). Nothing is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use ferris_idea_workbench::appearance::{IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::keymaps::UserKeymapFile;
use ferris_idea_workbench::project_windows::{
    OnLastProjectClosed, QuitsInHarness, RecentProjectsFile,
};
use ferris_idea_workbench::welcome_model::Section;
use ferris_idea_workbench::welcome_ssh_view::{Stage, WelcomeSshArgs};
use ferris_idea_workbench::welcome_view::{WelcomePluginsDir, show, welcome_window};
use ferris_idea_workbench::{RemoteStateDir, WelcomeView};
use gpui::{AnyWindowHandle, TestAppContext, WindowHandle};

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-welcome-ssh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// This test's own files and SSH configuration, set before anything reads them.
fn isolate(cx: &mut TestAppContext, dir: &std::path::Path) {
    ferris_idea_remote::testing::isolate_ssh();
    let config = dir.join("ssh_config");
    std::fs::write(
        &config,
        "Host staging\n  HostName 10.0.0.9\n  User deploy\n  Port 22\n",
    )
    .unwrap();
    let dir = dir.to_path_buf();
    cx.update(move |cx| {
        cx.set_global(RecentProjectsFile(dir.join("recent-projects")));
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(UserKeymapFile(dir.join("keymap.toml")));
        cx.set_global(WelcomePluginsDir(dir.join("plugins")));
        cx.set_global(RemoteStateDir(dir.join("remote")));
        cx.set_global(WelcomeSshArgs(vec![
            "-F".into(),
            config.display().to_string(),
        ]));
        cx.set_global(QuitsInHarness::default());
        cx.set_global(OnLastProjectClosed(std::rc::Rc::new(show)));
    });
}

fn texts(handle: WindowHandle<WelcomeView>, cx: &mut TestAppContext) -> Vec<String> {
    handle
        .update(cx, |view, _, cx| view.ssh_section().read(cx).shown_texts())
        .unwrap()
}

fn stage(handle: WindowHandle<WelcomeView>, cx: &mut TestAppContext) -> Stage {
    handle
        .update(cx, |view, _, cx| view.ssh_section().read(cx).stage())
        .unwrap()
}

/// SPEC-0355 v3 AK-8 (behaviours 12a, 12b, 12d, 12f).
#[gpui::test]
fn the_ssh_section_lists_fills_the_form_and_removes(cx: &mut TestAppContext) {
    let dir = folder();
    isolate(cx, &dir);
    let recent = ferris_idea_remote::recent_remote_file(&dir.join("remote"));
    for (host, folder) in [
        ("docker://web", "/app"),
        ("me@box:2222", "/home/me/site"),
        ("build", "/srv/shop"),
    ] {
        ferris_idea_remote::remember_remote(&recent, host, folder).unwrap();
    }

    cx.update(show);
    cx.run_until_parked();
    let handle = cx
        .update(|cx| welcome_window(cx))
        .expect("the Welcome window");
    let any: AnyWindowHandle = handle.into();

    // ⌘2 shows SSH: the recent SSH projects (not the container's), then the configured host.
    cx.simulate_keystrokes(any, "cmd-2");
    cx.run_until_parked();
    let section = handle.update(cx, |view, _, _| view.section()).unwrap();
    assert_eq!(section, Section::Ssh);
    assert_eq!(
        texts(handle, cx),
        [
            "shop — build:/srv/shop",
            "site — me@box:2222:/home/me/site",
            "staging (host) — deploy@10.0.0.9",
        ]
    );

    // The search filters; Enter on the host shows the form with its alias.
    cx.simulate_keystrokes(any, "s t a g");
    assert_eq!(texts(handle, cx), ["staging (host) — deploy@10.0.0.9"]);
    cx.simulate_keystrokes(any, "enter");
    cx.run_until_parked();
    assert_eq!(stage(handle, cx), Stage::Form);
    let form = handle
        .update(cx, |view, _, cx| view.ssh_section().read(cx).form_texts(cx))
        .unwrap();
    assert_eq!(form, ["staging".to_owned(), String::new(), String::new()]);

    // A port of 70000 is refused in the form; nothing connects.
    handle
        .update(cx, |view, _, cx| {
            view.ssh_section()
                .update(cx, |s, cx| s.fill("staging", "", "70000", cx))
        })
        .unwrap();
    cx.simulate_keystrokes(any, "enter");
    cx.run_until_parked();
    let (error, chooser, stage_now) = handle
        .update(cx, |view, _, cx| {
            let s = view.ssh_section().read(cx);
            (
                s.form_error().map(str::to_owned),
                s.chooser().is_some(),
                s.stage(),
            )
        })
        .unwrap();
    assert!(
        error.as_deref().unwrap_or("").starts_with("Port"),
        "{error:?}"
    );
    assert!(!chooser, "no connection was started");
    assert_eq!(stage_now, Stage::Form);

    // Editing a field drops the error; Escape goes back to the list.
    handle
        .update(cx, |view, _, cx| {
            view.ssh_section()
                .update(cx, |s, cx| s.fill("staging", "", "22", cx))
        })
        .unwrap();
    cx.run_until_parked();
    let error = handle
        .update(cx, |view, _, cx| {
            view.ssh_section().read(cx).form_error().map(str::to_owned)
        })
        .unwrap();
    assert_eq!(error, None);
    cx.simulate_keystrokes(any, "escape");
    cx.run_until_parked();
    assert_eq!(stage(handle, cx), Stage::List);

    // Back in the list with the keys: Escape empties the search, ⌘⌫ removes the first recent
    // project from the file; the folder on the host is not touched (nothing connects).
    cx.simulate_keystrokes(any, "escape");
    assert_eq!(texts(handle, cx).len(), 3);
    cx.simulate_keystrokes(any, "cmd-backspace");
    assert_eq!(
        texts(handle, cx),
        [
            "site — me@box:2222:/home/me/site",
            "staging (host) — deploy@10.0.0.9",
        ]
    );
    let left: Vec<String> = ferris_idea_remote::load_recent_remote(&recent)
        .into_iter()
        .map(|r| r.host)
        .collect();
    assert_eq!(
        left,
        ["me@box:2222", "docker://web"],
        "the container's stays"
    );

    // A configured host cannot be removed here.
    cx.simulate_keystrokes(any, "down cmd-backspace");
    let note = handle
        .update(cx, |view, _, cx| {
            view.ssh_section().read(cx).note().map(str::to_owned)
        })
        .unwrap();
    assert!(note.unwrap_or_default().contains("SSH configuration"));
    assert_eq!(texts(handle, cx).len(), 2);

    // ⌘N shows the empty form.
    cx.simulate_keystrokes(any, "cmd-n");
    cx.run_until_parked();
    assert_eq!(stage(handle, cx), Stage::Form);
    let form = handle
        .update(cx, |view, _, cx| view.ssh_section().read(cx).form_texts(cx))
        .unwrap();
    assert_eq!(form, [String::new(), String::new(), String::new()]);
    assert_eq!(
        cx.update(|cx| cx.windows().len()),
        1,
        "no other window opened"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

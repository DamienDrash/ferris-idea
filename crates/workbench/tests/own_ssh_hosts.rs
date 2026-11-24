//! `SPEC-0121` v5 AK-8, window part: Ferris IDEA's own SSH hosts in the Remote Host window of the
//! test harness, with real keys and clicks on controls found through the semantic tree. Add Host…
//! opens a form; a refused alias stays in the form with the values typed; the corrected host is
//! listed as Ferris IDEA's, connected to the test `sshd` through the one start point, and its home
//! folder shown; Remove Host asks and removes it; a host of the user's configuration is listed with
//! its source and cannot be removed. The host files, the home folder the file includes and the
//! settings folder are the test's own (`isolate_ssh` fails the test on a read of the user's
//! `~/.ssh/config`). Ferris IDEA's host files are one setting for the whole process, so the test has a
//! process of its own.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{WorkbenchView, semantics};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

type Bench = Entity<WorkbenchView>;

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn click(id: &str, cx: &mut VisualTestContext) {
    cx.run_until_parked();
    let at = cx
        .update(|window, _| {
            let tree = semantics::tree(semantics::window_id(window))?;
            let n = tree.iter().find(|n| n.test_id() == Some(id))?;
            let r = n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
            Some(point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)))
        })
        .unwrap_or_else(|| {
            let ids = cx.update(|window, _| {
                semantics::tree(semantics::window_id(window))
                    .map(|t| {
                        t.iter()
                            .filter_map(|n| {
                                n.test_id().map(|i| format!("{i}:{:?}", n.visible_bounds))
                            })
                            .filter(|i| i.starts_with("remote"))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            });
            panic!("no control {id} in view: {ids:?}")
        });
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn texts(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

/// Waits up to five minutes of real time (the test host is slow under load) until `done` holds;
/// an error the window shows ends the wait at once.
fn until(
    bench: &Bench,
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(300);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        let failed = bench.read_with(cx, |b, cx| {
            b.remote_view()
                .and_then(|v| v.read(cx).error().map(str::to_owned))
        });
        if let Some(e) = failed {
            panic!("{what}: the window reports {e:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}; rows {:?}", texts(bench, cx));
}

/// Moves the selection with ↑/↓ to the row containing `name`, focus on the list.
fn select(bench: &Bench, cx: &mut VisualTestContext, name: &str) {
    let view = bench.read_with(cx, |b, _| b.remote_view()).unwrap();
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    cx.run_until_parked();
    for _ in 0..50 {
        let (rows, selected) = view.read_with(cx, |v, _| (v.texts(), v.selected_text()));
        if selected.as_deref().is_some_and(|s| s.contains(name)) {
            return;
        }
        let want = rows
            .iter()
            .position(|t| t.contains(name))
            .unwrap_or_else(|| panic!("no row {name:?}: {rows:?}"));
        let at = selected
            .and_then(|s| rows.iter().position(|t| *t == s))
            .unwrap_or(0);
        keys(cx, if want > at { "down" } else { "up" });
    }
    panic!("the selection does not reach {name:?}");
}

fn folder(base: &Path, name: &str) -> PathBuf {
    let d = base.join(name);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

#[gpui::test]
fn own_hosts_are_added_connected_and_removed_in_the_window(cx: &mut TestAppContext) {
    ferris_idea_remote::testing::isolate_ssh();
    ferris_idea_remote::testing::stretch_limits(4);
    let base =
        std::env::temp_dir().join(format!("ferris-idea-own-ssh-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    // The sessions' home folder is the test's own, so the window never lists the user's.
    let session_home = folder(&base, "session-home");
    let Some(host) = ferris_idea_remote::testing::TestHost::start_with_env(
        &base.join("sshd"),
        &[("HOME", &session_home.display().to_string())],
    ) else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    host.wait_ready(Duration::from_secs(300))
        .expect("the test host lets a session in");
    cx.executor().allow_parking();
    let port: u16 = std::fs::read_to_string(host.dir.join("ssh_config"))
        .unwrap()
        .lines()
        .find_map(|l| l.trim().strip_prefix("Port "))
        .and_then(|p| p.trim().parse().ok())
        .unwrap();

    // The user's configuration of this test: `Host *` defaults (the test's known hosts, no agent)
    // and one host of the user's own.
    let user_file = base.join("user_ssh_config");
    std::fs::write(
        &user_file,
        format!(
            "Host theirs\n  HostName 192.0.2.8\n\nHost *\n  UserKnownHostsFile {}\n  IdentityAgent none\n",
            host.dir.join("known_hosts").display()
        ),
    )
    .unwrap();
    let user_before = std::fs::read(&user_file).unwrap();
    let files = ferris_idea_remote::HostFiles {
        own: base.join("settings/ssh/config"),
        user: user_file.clone(),
    };
    ferris_idea_remote::set_host_files(Some(files.clone()));

    let settings = folder(&base, "ide-settings");
    let root = folder(&base, "project");
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
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
        // As in the product: no `-F`, so both lists are shown.
        b.set_remote_ssh(Vec::new());
        b.toggle_remote(window, cx);
    });
    until(
        &bench,
        cx,
        "the user's host is listed with its source",
        |cx| {
            texts(&bench, cx)
                .iter()
                .any(|t| t.starts_with("theirs") && t.ends_with("· ~/.ssh/config"))
        },
    );

    // Add Host…: a refused alias stays in the form with what was typed.
    click("remote-add-host", cx);
    let view = bench.read_with(cx, |b, _| b.remote_view()).unwrap();
    assert!(view.read_with(cx, |v, _| v.adding_host()));
    let key = host.dir.join("client_key").display().to_string();
    let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
    cx.simulate_input("bad alias");
    keys(cx, "tab");
    cx.simulate_input("127.0.0.1");
    keys(cx, "tab");
    cx.simulate_input(&port.to_string());
    keys(cx, "tab");
    cx.simulate_input(&user);
    keys(cx, "tab");
    cx.simulate_input(&key);
    keys(cx, "enter");
    until(&bench, cx, "the refusal is shown in the form", |cx| {
        view.read_with(cx, |v, _| v.add_host_error().is_some())
    });
    let (error, values) = view.read_with(cx, |v, cx| {
        (
            v.add_host_error().unwrap().to_owned(),
            v.add_host_values(cx),
        )
    });
    assert!(error.starts_with("Alias:"), "{error}");
    assert_eq!(values[0], "bad alias");
    assert_eq!(values[4], key);
    assert!(!files.own.exists(), "nothing is written for a refusal");

    // Tab back to the alias, correct it, and add with the Add button.
    keys(cx, "tab");
    keys(cx, "cmd-a");
    cx.simulate_input("ferris-idea-own-win");
    click("remote-add-host-ok", cx);
    until(&bench, cx, "the host is listed as Ferris IDEA's", |cx| {
        texts(&bench, cx).iter().any(|t| {
            t.starts_with("ferris-idea-own-win")
                && t.contains("127.0.0.1")
                && t.ends_with("· Ferris IDEA")
        })
    });
    assert!(!view.read_with(cx, |v, _| v.adding_host()));
    let text = std::fs::read_to_string(&files.own).unwrap();
    assert!(text.contains("Host ferris-idea-own-win\n"));
    assert!(text.contains(&format!("IdentityFile {key}")));

    // Enter connects through Ferris IDEA's file and shows the session's home folder.
    select(&bench, cx, "ferris-idea-own-win");
    keys(cx, "enter");
    until(
        &bench,
        cx,
        "the home folder of the own host is shown",
        |cx| {
            view.read_with(cx, |v, _| {
                v.place().is_some_and(|(h, p)| {
                    h.contains("ferris-idea-own-win") && Path::new(p) == session_home
                })
            })
        },
    );
    click("remote-hosts", cx);
    until(&bench, cx, "back to the hosts", |cx| {
        view.read_with(cx, |v, _| v.place().is_none())
    });

    // A host of the user's configuration cannot be removed.
    select(&bench, cx, "theirs");
    keys(cx, "cmd-backspace");
    let refused = view.read_with(cx, |v, _| v.error().map(str::to_owned));
    assert!(
        refused
            .as_deref()
            .is_some_and(|e| e.contains("never changes")),
        "{refused:?}"
    );
    assert!(view.read_with(cx, |v, _| v.pending().is_none()));

    // Remove Host on Ferris IDEA's host asks, Enter removes it.
    select(&bench, cx, "ferris-idea-own-win");
    click("remote-remove-host", cx);
    assert!(view.read_with(cx, |v, _| matches!(
        v.pending(),
        Some(ferris_idea_workbench::RemotePending::RemoveHost(a)) if a == "ferris-idea-own-win"
    )));
    assert!(
        files.own.is_file()
            && std::fs::read_to_string(&files.own)
                .unwrap()
                .contains("ferris-idea-own-win")
    );
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    keys(cx, "enter");
    until(&bench, cx, "the host is gone from the list", |cx| {
        !texts(&bench, cx)
            .iter()
            .any(|t| t.contains("ferris-idea-own-win"))
    });
    assert!(
        !std::fs::read_to_string(&files.own)
            .unwrap()
            .contains("ferris-idea-own-win")
    );
    assert_eq!(std::fs::read(&user_file).unwrap(), user_before);

    ferris_idea_remote::set_host_files(None);
    drop(host);
    let _ = std::fs::remove_dir_all(&base);
}

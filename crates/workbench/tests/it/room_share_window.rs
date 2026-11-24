//! `SPEC-0323` v6 AK-23 to AK-25 in a real remote project window of the test harness: the backend
//! runs in this process with the room bound, the window is the owner, and another SSH user on
//! the same backend is a second person. Starting the room from the Share dialog, making an expiring
//! editor link, copying it only on a click (the harness's own clipboard, never the real one), the
//! other user joining with exactly that link, revoking it, removing that user, and the room widget
//! in the status bar.

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_remote::inprocess::{InProcessProject, Setup};
use ferris_idea_remote::{LiveProject, Timing};
use ferris_idea_session::{Backend, ClientSession, ErrorCode, Peer, Right, in_process};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{RemoteStateDir, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, px};
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-room-share-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), "hello\n").unwrap();
    dir
}

/// Lets the window's frame loop and the backend's threads run until `done` holds.
fn wait_for(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.executor().advance_clock(Duration::from_millis(50));
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting until {what}");
}

/// Draws a frame, then the middle of what is shown of the node with test id `id` in its semantic
/// tree, if it is in view.
fn spot(id: &str, cx: &mut VisualTestContext) -> Option<gpui::Point<gpui::Pixels>> {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        tree.iter()
            .filter(|n| n.test_id() == Some(id))
            .find_map(|n| {
                let r = n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
                Some(gpui::point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)))
            })
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let at = spot(id, cx).unwrap_or_else(|| panic!("{id} is drawn"));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn drawn(id: &str, cx: &mut VisualTestContext) -> bool {
    spot(id, cx).is_some()
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// Another server user `name` with editor rights, on a session of its own.
fn device(backend: &Backend, folder: &str, name: &str) -> ClientSession {
    let (side, server) = in_process();
    let b = backend.clone();
    let peer = Peer {
        name: format!("ssh:{name}@10.0.0.7"),
        rights: vec![Right::View, Right::Edit, Right::VcsWrite],
    };
    std::thread::spawn(move || b.serve_as(server, peer));
    ClientSession::open(side, folder, "t", "h", None).unwrap()
}

#[gpui::test]
fn the_owner_shares_revokes_and_removes_from_the_window(cx: &mut TestAppContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let dir = scratch("owner");
    let settings = dir.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
        cx.set_global(RemoteStateDir(dir.join("remote")));
    });
    let backend: Arc<Mutex<Option<Backend>>> = Arc::default();
    let kept = Arc::clone(&backend);
    let setup: Setup = Arc::new(move |b| {
        ferris_idea_session::room::bind(b, ferris_idea_session::room::RoomConfig::default());
        *kept.lock().unwrap() = Some(b.clone());
    });
    let project = InProcessProject::open_fixed_with(
        "room-share-host",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        None,
        Duration::ZERO,
        Some(setup),
    )
    .unwrap();
    let live = LiveProject::start(Box::new(project), Timing::default());
    let handle = cx
        .update(|cx| ferris_idea_workbench::open_live_window(live.clone(), cx))
        .unwrap();
    let window: WindowHandle<WorkbenchView> = handle.downcast().unwrap();
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(900.)));
    let b = backend
        .lock()
        .unwrap()
        .clone()
        .expect("the backend is bound");
    let folder = dir.join("project").display().to_string();

    // No room yet: no widget; the Share button is there and opens the dialog.
    assert!(!drawn("room-state-widget", cx), "no widget without a room");
    assert!(live.window_part("room").is_none_or(|p| p.is_null()));
    click("room-share-button", cx);
    wait_for(cx, "the dialog knows there is no room", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model()
                .is_some_and(|m| m.known() && m.room_name().is_none())
        })
    });

    // Start Room: the window is the admin and shows the room.
    click("room-share-start", cx);
    wait_for(cx, "the room exists and the window is in it", |cx| {
        bench.read_with(cx, |b, _| {
            b.room_view().is_some() && b.share_model().is_some_and(|m| m.is_admin())
        })
    });
    assert_eq!(
        bench.read_with(cx, |b, _| b
            .share_model()
            .unwrap()
            .room_name()
            .map(str::to_owned)),
        Some("project".to_owned())
    );

    // Editor, 7 days, Create Link: shown, not copied.
    let before = clipboard(cx);
    click("room-share-role-editor", cx);
    click("room-share-life-week", cx);
    click("room-share-create", cx);
    wait_for(cx, "the link is shown", |cx| {
        bench.read_with(cx, |b, _| b.share_model().unwrap().fresh().is_some())
    });
    let fresh = bench.read_with(cx, |b, _| b.share_model().unwrap().fresh().unwrap().clone());
    assert!(
        fresh.link.starts_with("ferris-idea://room/"),
        "{}",
        fresh.link
    );
    assert_eq!(fresh.role, "editor");
    let secret = fresh.link.rsplit_once('#').unwrap().1.to_owned();
    assert_eq!(secret.len(), 64);
    assert_eq!(
        clipboard(cx),
        before,
        "nothing reaches the clipboard without Copy"
    );
    let links = live.call("room.links", json!({})).unwrap();
    let link = &links["links"][0];
    assert_eq!(link["role"], "editor");
    assert_eq!(link["audience"], "organisation");
    assert_eq!(
        link["expires"].as_u64().unwrap() - link["created"].as_u64().unwrap(),
        604_800_000,
        "seven days, always with an end"
    );
    let link_id = link["id"].as_str().unwrap().to_owned();
    wait_for(cx, "the link is in force in the dialog", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model()
                .unwrap()
                .links()
                .iter()
                .any(|l| l.id == link_id)
        })
    });

    // Copy: exactly the link.
    click("room-share-copy", cx);
    assert_eq!(clipboard(cx).as_deref(), Some(fresh.link.as_str()));

    // The secret is nowhere else: status bar, what agents read.
    assert!(!status(&bench, cx).contains(&secret));
    let part = live
        .window_part("room")
        .expect("the window publishes the room");
    assert!(!part.to_string().contains(&secret), "{part}");
    assert_eq!(part["widget"]["text"], "Room · 1");

    // Bob joins with the copied link and shows in the list with his role.
    let bob = device(&b, &folder, "bob");
    bob.call("room.join", json!({ "link": clipboard(cx).unwrap() }))
        .unwrap();
    wait_for(cx, "Bob is listed", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model()
                .unwrap()
                .people()
                .iter()
                .any(|p| p.name == "bob" && p.role == "editor" && p.removable)
        })
    });
    let own = bench.read_with(cx, |b, _| b.share_model().unwrap().people()[0].clone());
    assert!(own.you && !own.removable, "{own:?}");
    wait_for(cx, "the widget counts Bob", |_| {
        live.window_part("room")
            .is_some_and(|p| p["widget"]["text"] == "Room · 2")
    });

    // Revoke: the link goes, and it admits nobody any more.
    click(&format!("room-share-revoke-{link_id}"), cx);
    wait_for(cx, "the link is revoked", |cx| {
        bench.read_with(cx, |b, _| b.share_model().unwrap().links().is_empty())
    });
    let carl = device(&b, &folder, "carl");
    let refused = carl
        .call("room.join", json!({ "link": fresh.link }))
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotGranted);

    // Remove asks in the row; Keep keeps, Remove removes.
    click("room-share-remove-ssh-bob", cx);
    click("room-share-remove-keep-ssh-bob", cx);
    assert!(
        bob.call("session.ping", json!({})).is_ok(),
        "Keep keeps Bob"
    );
    click("room-share-remove-ssh-bob", cx);
    click("room-share-remove-yes-ssh-bob", cx);
    wait_for(cx, "Bob's session is closed", |_| {
        bob.call("session.ping", json!({})).is_err()
    });
    wait_for(cx, "Bob is out of the list", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model()
                .unwrap()
                .people()
                .iter()
                .all(|p| p.name != "bob")
        })
    });

    // Closing forgets the link; opening again shows none.
    click("room-share-close", cx);
    assert!(bench.read_with(cx, |b, _| b.share_model().is_none()));
    wait_for(cx, "the room widget is drawn", |cx| {
        drawn("room-state-widget", cx)
    });
    click("room-state-widget", cx);
    wait_for(cx, "the widget opens the dialog", |cx| {
        bench.read_with(cx, |b, _| b.share_model().is_some_and(|m| m.is_admin()))
    });
    assert!(bench.read_with(cx, |b, _| b.share_model().unwrap().fresh().is_none()));

    // Escape closes it too.
    cx.simulate_keystrokes("escape");
    assert!(bench.read_with(cx, |b, _| b.share_model().is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn share_project_on_a_local_project_says_what_it_needs(cx: &mut TestAppContext) {
    let dir = scratch("local").join("project");
    let settings = dir.parent().unwrap().join("settings-local");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = dir.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Remote.ShareProject", window, cx)
    });
    assert_eq!(
        status(&bench, cx),
        "Sharing needs a project opened from a host (File › Remote Development…)"
    );
    assert!(bench.read_with(cx, |b, _| b.share_model().is_none()));
}

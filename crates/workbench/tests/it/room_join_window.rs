//! `SPEC-0323` v7 AK-29 in a real project window of the test harness: the backend runs in this
//! process with the room bound and the window is its owner. The Share dialog makes a link that
//! leads to its host; File › Join Project by Link… reads a link, checks its host key against a
//! `known_hosts` in a temporary folder (never `~/.ssh`), refuses a different key, explains an
//! unknown one, and with the matching key joins another SSH user of the same backend, whose
//! remote project window opens in the room. The harness's own clipboard, never the real one.

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ferris_idea_remote::inprocess::{InProcessProject, Setup};
use ferris_idea_remote::{HostKeyCheck, LiveProject, ProjectLink, RoomLink, Timing};
use ferris_idea_session::{Backend, Peer, Right};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::room_join_model::JoinFailure;
use ferris_idea_workbench::{HostKeySource, JoinOpener, RemoteStateDir, WorkbenchView, semantics};
use gpui::{
    ClipboardItem, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle,
    px,
};
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-room-join-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), "hello\n").unwrap();
    dir
}

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

fn paste(text: &str, cx: &mut VisualTestContext) {
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned())));
    click("room-join-paste", cx);
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// A made-up Ed25519 public key blob.
fn blob(seed: u8) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&11u32.to_be_bytes());
    b.extend_from_slice(b"ssh-ed25519");
    b.extend_from_slice(&32u32.to_be_bytes());
    b.extend(std::iter::repeat_n(seed, 32));
    b
}

fn key_line(host: &str, seed: u8) -> String {
    format!(
        "{host} ssh-ed25519 {}\n",
        base64::engine::general_purpose::STANDARD.encode(blob(seed))
    )
}

/// The connections the test opener made: host label and the extra `ssh` arguments.
type Opened = Arc<Mutex<Vec<(String, Vec<String>)>>>;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[gpui::test]
fn a_link_joins_only_a_host_whose_key_matches(cx: &mut TestAppContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let dir = scratch("join");
    let settings = dir.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    let known_hosts = dir.join("known_hosts");
    // The owner's host is known; the host of the links is not yet.
    std::fs::write(&known_hosts, key_line("room-join-owner", 1)).unwrap();
    let fp = ferris_idea_remote::known_hosts::fingerprint_of_blob(&blob(1));

    let backend: Arc<Mutex<Option<Backend>>> = Arc::default();
    let kept = Arc::clone(&backend);
    let setup: Setup = Arc::new(move |b| {
        ferris_idea_session::room::bind(b, ferris_idea_session::room::RoomConfig::default());
        *kept.lock().unwrap() = Some(b.clone());
    });
    let project = InProcessProject::open_fixed_with(
        "room-join-owner",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        None,
        Duration::ZERO,
        Some(setup),
    )
    .unwrap();
    let b = backend
        .lock()
        .unwrap()
        .clone()
        .expect("the backend is bound");
    let folder = dir.join("project").display().to_string();

    // Joining connects another SSH user to the same backend instead of SSH; the host decides who.
    let opened: Opened = Arc::default();
    let seen = Arc::clone(&opened);
    let on = b.clone();
    let at = dir.join("project");
    let opener: ferris_idea_workbench::JoinOpen =
        Arc::new(move |link: &RoomLink, extra: &[String]| {
            let label = ferris_idea_workbench::link_label(link);
            seen.lock().unwrap().push((label.clone(), extra.to_vec()));
            let who = if link.host == "join-host.test" {
                "bob"
            } else {
                "carol"
            };
            let peer = Peer {
                name: format!("ssh:{who}@10.0.0.7"),
                rights: vec![Right::View, Right::Edit, Right::VcsWrite],
            };
            InProcessProject::open_on(&on, &label, &at, peer)
                .map(|p| Box::new(p) as Box<dyn ProjectLink>)
        });
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
        cx.set_global(RemoteStateDir(dir.join("remote")));
        cx.set_global(HostKeySource::Files(vec![known_hosts.clone()]));
        cx.set_global(JoinOpener(opener));
    });
    let live = LiveProject::start(Box::new(project), Timing::default());
    let handle = cx
        .update(|cx| ferris_idea_workbench::open_live_window(live.clone(), cx))
        .unwrap();
    let window: WindowHandle<WorkbenchView> = handle.downcast().unwrap();
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(900.)));

    // The Share dialog makes a link that leads to its host (behaviour 44).
    click("room-share-button", cx);
    wait_for(cx, "the dialog knows there is no room", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model()
                .is_some_and(|m| m.known() && m.room_name().is_none())
        })
    });
    click("room-share-start", cx);
    wait_for(cx, "the window is the room's admin", |cx| {
        bench.read_with(cx, |b, _| b.share_model().is_some_and(|m| m.is_admin()))
    });
    click("room-share-role-editor", cx);
    click("room-share-create", cx);
    wait_for(cx, "the link leads to its host", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model()
                .and_then(|m| m.fresh())
                .is_some_and(|f| f.link.contains("?host="))
        })
    });
    let shared = bench.read_with(cx, |b, _| {
        b.share_model().unwrap().fresh().unwrap().link.clone()
    });
    let read = RoomLink::parse(&shared, now()).unwrap();
    assert_eq!(read.host, "room-join-owner");
    assert_eq!(read.port, 22);
    assert_eq!(read.folder, folder);
    assert_eq!(read.fingerprint, fp);
    assert_eq!(read.role, "editor");
    assert!(read.expires.is_some_and(|e| e > now()));
    let secret = shared.rsplit_once('#').unwrap().1.to_owned();
    click("room-share-close", cx);

    // The same link as it would lead to another host and port: the joiner's.
    let short = format!("ferris-idea://room/{}#{secret}", read.room);
    let link = RoomLink::compose(
        &short,
        "join-host.test",
        2222,
        &folder,
        &fp,
        "editor",
        read.expires,
    )
    .unwrap()
    .text();

    // File › Join Project by Link…, Paste: the parts before anything connects, secret masked.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Remote.JoinByLink", window, cx)
    });
    assert!(bench.read_with(cx, |b, _| b.join_model().is_some()));
    paste(&link, cx);
    wait_for(cx, "the host key was looked up", |cx| {
        bench.read_with(cx, |b, _| {
            b.join_model()
                .is_some_and(|m| m.check().is_some() && !m.busy)
        })
    });
    let rows = bench.read_with(cx, |b, _| b.join_model().unwrap().rows(now()));
    let row = |k: &str| rows.iter().find(|r| r.0 == k).unwrap().1.clone();
    assert_eq!(row("Host"), "join-host.test:2222");
    assert_eq!(row("Project"), folder);
    assert_eq!(row("Host key"), fp);
    assert_eq!(row("Role"), "Editor");
    assert_eq!(row("Link"), "ends in 24 h");
    assert!(rows.iter().all(|(_, v)| !v.contains(&secret)));

    // Unknown: the steps, no Join, nothing connected.
    let m = bench.read_with(cx, |b, _| b.join_model().unwrap().clone());
    assert_eq!(m.check(), Some(&HostKeyCheck::Unknown));
    assert!(m.steps()[0].contains("ssh -p 2222 join-host.test"));
    assert!(!m.can_join());
    assert!(!drawn("room-join-go", cx));
    assert!(opened.lock().unwrap().is_empty());

    // A different key for the host: refused as a possible attack.
    std::fs::write(
        &known_hosts,
        key_line("room-join-owner", 1) + &key_line("[join-host.test]:2222", 2),
    )
    .unwrap();
    click("room-join-check", cx);
    wait_for(cx, "the different key is seen", |cx| {
        bench.read_with(cx, |b, _| {
            b.join_model().is_some_and(|m| {
                matches!(m.check(), Some(HostKeyCheck::Different { .. })) && !m.busy
            })
        })
    });
    let m = bench.read_with(cx, |b, _| b.join_model().unwrap().clone());
    assert_eq!(m.failure(), Some(&JoinFailure::WrongHostKey));
    assert!(m.check_line().unwrap().0.contains("attack"));
    assert!(!m.can_join() && !drawn("room-join-go", cx));
    assert!(opened.lock().unwrap().is_empty(), "nothing connects");

    // The matching key: Join opens Bob's remote project window in the room as an editor.
    std::fs::write(
        &known_hosts,
        key_line("room-join-owner", 1)
            + &key_line("[join-host.test]:2222", 1)
            + &key_line("join-host2.test", 1),
    )
    .unwrap();
    click("room-join-check", cx);
    wait_for(cx, "Join is offered", |cx| {
        bench.read_with(cx, |b, _| b.join_model().is_some_and(|m| m.can_join()))
    });
    click("room-join-go", cx);
    wait_for(cx, "the dialog closed after joining", |cx| {
        bench.read_with(cx, |b, _| b.join_model().is_none())
    });
    let note = status(&bench, cx);
    assert!(note.contains("Joined the room"), "{note}");
    assert!(note.contains("editor"), "{note}");
    assert!(!note.contains(&secret));
    {
        let opened = opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].0, "join-host.test:2222");
        assert!(
            opened[0]
                .1
                .iter()
                .any(|a| a == "HostKeyAlgorithms=ssh-ed25519"),
            "{:?}",
            opened[0].1
        );
    }
    let joined = ferris_idea_remote::live::find("join-host.test:2222", &folder)
        .expect("Bob's remote project window is open");
    let mine = joined.call("room.status", json!({})).unwrap();
    assert_eq!(mine["id"], read.room);
    let status_now = live.call("room.status", json!({})).unwrap();
    assert!(
        status_now["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "bob" && p["role"] == "editor"),
        "{status_now}"
    );
    for part in [live.window_part("room"), joined.window_part("room")]
        .into_iter()
        .flatten()
    {
        assert!(!part.to_string().contains(&secret), "{part}");
    }

    // An ended link: refused before anything connects.
    let ended = RoomLink::compose(
        &short,
        "join-host2.test",
        22,
        &folder,
        &fp,
        "editor",
        Some(now() - 7200),
    )
    .unwrap()
    .text();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Remote.JoinByLink", window, cx)
    });
    paste(&ended, cx);
    let m = bench.read_with(cx, |b, _| b.join_model().unwrap().clone());
    assert!(matches!(m.failure(), Some(JoinFailure::Expired(_))));
    assert!(m.error.as_deref().unwrap().starts_with("This link ended"));
    assert!(m.link().is_none());

    // A revoked link: the host does not admit Carol, and nothing of her project stays open.
    let links = live.call("room.links", json!({})).unwrap();
    let id = links["links"][0]["id"].as_str().unwrap().to_owned();
    live.call("room.revoke_link", json!({ "id": id })).unwrap();
    let revoked = RoomLink::compose(
        &short,
        "join-host2.test",
        22,
        &folder,
        &fp,
        "editor",
        read.expires,
    )
    .unwrap()
    .text();
    paste(&revoked, cx);
    wait_for(cx, "Join is offered for the revoked link", |cx| {
        bench.read_with(cx, |b, _| b.join_model().is_some_and(|m| m.can_join()))
    });
    click("room-join-go", cx);
    wait_for(cx, "the host refused the link", |cx| {
        bench.read_with(cx, |b, _| {
            b.join_model()
                .is_some_and(|m| m.failure() == Some(&JoinFailure::NotAdmitted) && !m.busy)
        })
    });
    let m = bench.read_with(cx, |b, _| b.join_model().unwrap().clone());
    assert!(
        m.error
            .as_deref()
            .unwrap()
            .contains("Ask an admin of the room for a new link")
    );
    assert_eq!(opened.lock().unwrap().len(), 2, "Carol was connected once");
    assert!(ferris_idea_remote::live::find("join-host2.test", &folder).is_none());
    let status_after = live.call("room.status", json!({})).unwrap();
    assert!(
        status_after["participants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["name"] != "carol"),
        "{status_after}"
    );
    assert!(!status(&bench, cx).contains(&secret));

    // Escape closes the dialog.
    cx.simulate_keystrokes("escape");
    assert!(bench.read_with(cx, |b, _| b.join_model().is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}

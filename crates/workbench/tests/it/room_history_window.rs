//! `SPEC-0323` v9 AK-39 in a real remote project window of the test harness: the backend runs in
//! this process with the room bound, the window is the owner, and another SSH user on the same
//! backend is a second person who changes two files at once. The Room History dialog lists the
//! rows, shows the diffs, follows new changes, and restores a version as one change that one undo
//! of the window's session takes back.

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_remote::inprocess::{InProcessProject, Setup};
use ferris_idea_remote::{LiveProject, Timing};
use ferris_idea_session::{Backend, ClientSession, Peer, Right, in_process};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::room_history_model::{FileState, HISTORY_NEEDS_ROOM};
use ferris_idea_workbench::{RemoteStateDir, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, px};
use serde_json::{Value, json};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-room-history-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), "alpha\n").unwrap();
    std::fs::write(dir.join("project/b.txt"), "beta\n").unwrap();
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

fn row_ids(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<(u64, String)> {
    bench.read_with(cx, |b, _| {
        b.room_history_model()
            .map(|m| m.rows().iter().map(|r| (r.id, r.summary())).collect())
            .unwrap_or_default()
    })
}

#[gpui::test]
fn the_history_lists_shows_and_restores_from_the_window(cx: &mut TestAppContext) {
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
        "room-history-host",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        None,
        Duration::ZERO,
        Some(setup),
    )
    .unwrap();
    let live = LiveProject::start(Box::new(project), Timing::default());
    // The window's own session makes the room and a link for Bob before the window opens.
    live.call("room.create", json!({"name": "demo"})).unwrap();
    let link = live.call("room.share", json!({"role": "editor"})).unwrap()["link"]
        .as_str()
        .unwrap()
        .to_owned();
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
    let text = |name: &str| b.document(name).unwrap().text;

    // Bob changes both files in one change set, then both again.
    let bob = device(&b, &folder, "bob");
    bob.call("room.join", json!({ "link": link })).unwrap();
    let apply = |a: &str, bb: &str| {
        bob.call(
            "room.apply",
            json!({"changes": [
                {"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": a}]},
                {"kind": "edit", "path": "b.txt", "ops": [{"at": 0, "insert": bb}]},
            ]}),
        )
        .unwrap();
    };
    apply("one ", "two ");
    apply("three ", "four ");
    wait_for(cx, "the window is in the room", |cx| {
        bench.read_with(cx, |b, _| b.room_view().is_some())
    });

    // Room History from the action: newest first, Bob's name, both files.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Room.ShowHistory", window, cx)
    });
    wait_for(cx, "the rows arrive", |cx| row_ids(&bench, cx).len() == 2);
    let rows = row_ids(&bench, cx);
    assert_eq!(rows[0].1, "Edited a.txt, b.txt");
    assert!(rows[0].0 > rows[1].0, "newest first: {rows:?}");
    let first_set = rows[1].0;
    let author = bench.read_with(cx, |b, _| {
        b.room_history_model().unwrap().rows()[0].author.clone()
    });
    assert_eq!(author, "bob");
    assert!(drawn(&format!("room-history-row-{first_set}"), cx));

    // Selecting the first change set shows both diffs.
    click(&format!("room-history-row-{first_set}"), cx);
    wait_for(cx, "both diffs are shown", |cx| {
        bench.read_with(cx, |b, _| {
            let files = b.room_history_model().unwrap().files();
            files.len() == 2
                && files
                    .iter()
                    .all(|f| matches!(f.state, FileState::Shown { .. }))
        })
    });
    let files = bench.read_with(cx, |b, _| b.room_history_model().unwrap().files().to_vec());
    assert_eq!(files[0].doc, "a.txt");
    let FileState::Shown {
        lines,
        added,
        removed,
    } = &files[0].state
    else {
        unreachable!()
    };
    assert_eq!((*added, *removed), (1, 1));
    assert_eq!(
        lines,
        &vec![
            ferris_idea_workbench::room_history_model::DiffLine::Line('−', "alpha".into()),
            ferris_idea_workbench::room_history_model::DiffLine::Line('+', "one alpha".into()),
        ]
    );
    assert!(drawn("room-history-file-1", cx));

    // A new change of Bob appears without reopening; the selection stays.
    bob.call(
        "room.apply",
        json!({"changes": [{"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "five "}]}]}),
    )
    .unwrap();
    wait_for(cx, "the new row appears", |cx| {
        row_ids(&bench, cx).len() == 3
    });
    assert_eq!(row_ids(&bench, cx)[0].1, "Edited a.txt");
    assert_eq!(
        bench.read_with(cx, |b, _| b
            .room_history_model()
            .unwrap()
            .selected()
            .map(|r| r.id)),
        Some(first_set)
    );

    // Restore asks first; Keep changes nothing.
    click("room-history-restore", cx);
    assert_eq!(
        bench.read_with(cx, |b, _| b.room_history_model().unwrap().question()),
        "Restore 2 files to how they were after this change?"
    );
    click("room-history-restore-keep", cx);
    assert_eq!(text("a.txt"), "five three one alpha\n");

    // Restore, then Restore: both files as one group.
    let before = live.call("room.status", json!({})).unwrap()["seq"]
        .as_u64()
        .unwrap();
    click("room-history-restore", cx);
    click("room-history-restore-yes", cx);
    wait_for(cx, "the restore is reported", |cx| {
        bench.read_with(cx, |b, _| {
            b.room_history_model().unwrap().note.as_deref()
                == Some("Restored 2 files. The room's undo takes it back.")
        })
    });
    assert_eq!(text("a.txt"), "one alpha\n");
    assert_eq!(text("b.txt"), "two beta\n");
    let new: Vec<Value> = live.call("room.since", json!({"seq": before})).unwrap()["entries"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(new.len(), 2, "{new:?}");
    assert_eq!(new[0]["group"], new[1]["group"], "one group");
    wait_for(cx, "the restore is a row of its own", |cx| {
        row_ids(&bench, cx).len() == 4
    });

    // One undo of the window's session takes the restore back.
    live.call("room.undo", json!({})).unwrap();
    assert_eq!(text("a.txt"), "five three one alpha\n");
    assert_eq!(text("b.txt"), "four two beta\n");

    // Escape closes the dialog.
    cx.simulate_keystrokes("escape");
    assert!(bench.read_with(cx, |b, _| b.room_history_model().is_none()));

    // The Share dialog leads there too.
    click("room-share-button", cx);
    wait_for(cx, "the Share dialog knows the room", |cx| {
        bench.read_with(cx, |b, _| {
            b.share_model().is_some_and(|m| m.room_name().is_some())
        })
    });
    click("room-share-history", cx);
    assert!(bench.read_with(cx, |b, _| b.share_model().is_none()
        && b.room_history_model().is_some()));
    click("room-history-close", cx);
    assert!(bench.read_with(cx, |b, _| b.room_history_model().is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn room_history_on_a_local_project_says_what_it_needs(cx: &mut TestAppContext) {
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
        b.menu_action("Room.ShowHistory", window, cx)
    });
    assert_eq!(status(&bench, cx), HISTORY_NEEDS_ROOM);
    assert!(bench.read_with(cx, |b, _| b.room_history_model().is_none()));
}

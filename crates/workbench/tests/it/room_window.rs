//! `SPEC-0323` v5 AK-18 to AK-20 in a real remote project window of the test harness: the backend
//! runs in this process with the room bound, the window is one participant and a second session
//! on the same backend is another person. The other's avatar in the toolbar, their caret and
//! selection in the editor, following them by a click on the avatar, being taken along, stopping
//! by a key, the window's own place reaching the other, and their leaving.

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_remote::inprocess::{InProcessProject, Setup};
use ferris_idea_remote::{LiveProject, Timing};
use ferris_idea_session::{Backend, ClientSession, in_process};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::room_model::PALETTE;
use ferris_idea_workbench::{RemoteStateDir, WorkbenchView};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, px};
use serde_json::{Value, json};

/// The agent's name in the room: its own and the person's it acts for.
const BOB: &str = "Bob Stone (room-window-host)";

const A: &str = "hello world\nsecond line\nthird line\n";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-room-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), A).unwrap();
    std::fs::write(dir.join("project/b.txt"), "other file\n").unwrap();
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

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn active(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> Option<(String, String, usize)> {
    bench.read_with(cx, |b, cx| {
        let e = b.active_editor_entity()?;
        let path = b.active_path()?;
        let e = e.read(cx);
        Some((
            path.file_name()?.to_string_lossy().into_owned(),
            e.state.buffer.text(),
            e.state.carets.primary().head,
        ))
    })
}

fn click(name: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    let r =
        ferris_idea_workbench::hit_targets::get(name).unwrap_or_else(|| panic!("{name} is drawn"));
    let at = gpui::point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn another_person_is_shown_followed_and_left(cx: &mut TestAppContext) {
    ferris_idea_workbench::hit_targets::enable();
    let dir = scratch();
    let settings = dir.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    let remote_state = dir.join("remote");
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
        cx.set_global(RemoteStateDir(remote_state.clone()));
    });

    // The backend with the room, kept for the second person.
    let backend: Arc<Mutex<Option<Backend>>> = Arc::default();
    let kept = Arc::clone(&backend);
    let setup: Setup = Arc::new(move |b| {
        ferris_idea_session::room::bind(b, ferris_idea_session::room::RoomConfig::default());
        *kept.lock().unwrap() = Some(b.clone());
    });
    let project = InProcessProject::open_with(
        "room-window-host",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        None,
        Duration::ZERO,
        Some(setup),
    )
    .unwrap();
    let live = LiveProject::start(Box::new(project), Timing::default());
    live.call("room.create", json!({"name": "demo"})).unwrap();

    // Bob: an agent of the same user joining as a participant of its own (behaviour 23), on a
    // session of its own.
    let b = backend
        .lock()
        .unwrap()
        .clone()
        .expect("the backend is bound");
    let (side, server) = in_process();
    std::thread::spawn(move || b.serve(server));
    let folder = dir.join("project").display().to_string();
    let mut bob = ClientSession::open(side, &folder, "t", "h", None).unwrap();
    let seen = bob.listen_host("room.");
    bob.call("room.join", json!({"agent": "Bob Stone"}))
        .unwrap();
    let bob_client = bob.info.client.clone();

    // The window.
    let handle = cx
        .update(|cx| ferris_idea_workbench::open_live_window(live.clone(), cx))
        .unwrap();
    let window: WindowHandle<WorkbenchView> = handle.downcast().unwrap();
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));

    wait_for(cx, "the window shows Bob in the room", |cx| {
        bench.read_with(cx, |b, _| {
            b.room_view()
                .is_some_and(|v| v.participant(&bob_client).is_some())
        })
    });
    let avatars = bench.read_with(cx, |b, _| b.room_view().unwrap().avatars());
    assert_eq!(avatars.shown.len(), 1, "only Bob, never the window itself");
    let avatar = &avatars.shown[0];
    assert_eq!(avatar.title, BOB);
    assert_eq!(avatar.initials, "BS");
    assert!(
        avatar.details.contains(&"Role: editor".to_owned()),
        "{:?}",
        avatar.details
    );
    let colour = bench.read_with(cx, |b, _| {
        b.room_view()
            .unwrap()
            .participant(&bob_client)
            .unwrap()
            .color
    });
    assert_eq!(avatar.rgb, PALETTE[colour]);

    // The window opens b.txt and a.txt; b.txt is in front.
    for name in ["a.txt", "b.txt"] {
        bench.update_in(cx, |b, window, cx| {
            let p = b.remote_local_path(name).unwrap();
            b.open(&p, window, cx);
        });
        wait_for(cx, &format!("{name} opens"), |cx| {
            active(&bench, cx).is_some_and(|(n, _, _)| n == name)
        });
    }

    // Bob's caret and selection in a.txt, in his colour, with his name.
    bob.call(
        "room.presence",
        json!({"doc": "a.txt", "caret": 6, "selection": [0, 5]}),
    )
    .unwrap();
    let a_editor = bench.read_with(cx, |b, cx| {
        b.editor_for(&b.remote_local_path("a.txt").unwrap(), cx)
            .expect("a.txt has an editor")
    });
    wait_for(cx, "Bob's caret shows in a.txt", |cx| {
        a_editor.read_with(cx, |e, _| e.remote_carets().iter().any(|c| c.caret == 6))
    });
    let shown = a_editor.read_with(cx, |e, _| e.remote_carets().to_vec());
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].selection, Some(0..5));
    assert_eq!(shown[0].rgb, PALETTE[colour]);
    assert_eq!(shown[0].name, BOB);
    assert_eq!(shown[0].label, 1.0, "the label stands right after a move");
    let b_editor = bench.read_with(cx, |b, cx| {
        b.editor_for(&b.remote_local_path("b.txt").unwrap(), cx)
            .unwrap()
    });
    assert!(
        b_editor.read_with(cx, |e, _| e.remote_carets().is_empty()),
        "no caret in a document Bob is not in"
    );

    // A click on Bob's avatar: a.txt to the front with the caret at Bob's.
    click(&format!("room-avatar-{bob_client}"), cx);
    assert_eq!(
        status(&bench, cx),
        format!("Following {BOB}. Type or move the caret to stop.")
    );
    wait_for(cx, "the window follows Bob to a.txt", |cx| {
        active(&bench, cx).is_some_and(|(n, _, at)| n == "a.txt" && at == 6)
    });
    // Bob moves on: the window goes along.
    bob.call("room.presence", json!({"doc": "a.txt", "caret": 14}))
        .unwrap();
    wait_for(cx, "the window goes along with Bob", |cx| {
        active(&bench, cx).is_some_and(|(_, _, at)| at == 14)
    });
    assert_eq!(
        bench.read_with(cx, |b, _| b
            .room_view()
            .unwrap()
            .following()
            .map(str::to_owned)),
        Some(bob_client.clone())
    );

    // Bob learns where the window is and that it follows him.
    let own = bench.read_with(cx, |b, _| b.room_view().unwrap().own().to_owned());
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..300 {
        cx.executor().advance_clock(Duration::from_millis(50));
        cx.run_until_parked();
        bob.pump(Duration::from_millis(10));
        while let Ok((m, p)) = seen.try_recv() {
            if m == "room.presence" && p["client"] == own.as_str() {
                got.push(p);
            }
        }
        if got.iter().any(|p| p["presence"]["caret"] == 14) {
            break;
        }
    }
    let last = got
        .iter()
        .rfind(|p| p["presence"]["caret"] == 14)
        .unwrap_or_else(|| panic!("Bob gets the window's place: {got:?}"));
    assert_eq!(last["presence"]["doc"], "a.txt");
    assert_eq!(last["presence"]["follows"], bob_client.as_str());

    // A key of the user's own: following stops with a hint.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("right");
    wait_for(cx, "following stops", |cx| {
        bench.read_with(cx, |b, _| b.room_view().unwrap().following().is_none())
    });
    assert_eq!(status(&bench, cx), format!("Stopped following {BOB}"));
    bob.call("room.presence", json!({"doc": "a.txt", "caret": 2}))
        .unwrap();
    wait_for(cx, "Bob's caret moves", |cx| {
        a_editor.read_with(cx, |e, _| e.remote_carets().iter().any(|c| c.caret == 2))
    });
    assert_eq!(
        active(&bench, cx).map(|(_, _, at)| at),
        Some(15),
        "the window stays where the user put it"
    );

    // What agents see and ask (`room_presence`, `room_follow`, behaviour 36).
    let ask = |value: Value, cx: &mut VisualTestContext| {
        let id = live.ask_window("room_follow", value);
        let mut answer = None;
        wait_for(cx, "the window answers the agent", |_| {
            answer = live.take_window_answer(id);
            answer.is_some()
        });
        answer.unwrap()
    };
    let part = live
        .window_part("room")
        .expect("the window publishes the room");
    assert_eq!(part["you"], own.as_str());
    assert_eq!(part["participants"][0]["client"], bob_client.as_str());
    assert_eq!(part["participants"][0]["caret"], 2);
    let answer = ask(json!({ "client": bob_client }), cx);
    assert_eq!(answer["following"], bob_client.as_str(), "{answer}");
    wait_for(cx, "the agent's follow takes the window to Bob", |cx| {
        active(&bench, cx).is_some_and(|(_, _, at)| at == 2)
    });
    let refused = ask(json!({ "client": own }), cx);
    assert_eq!(refused["error"], "a window cannot follow itself");
    let stopped = ask(json!({}), cx);
    assert_eq!(stopped["following"], Value::Null, "{stopped}");

    // Following again, then Bob leaves.
    bench.update_in(cx, |b, window, cx| {
        b.room_avatar_clicked(&bob_client, window, cx)
    });
    assert!(status(&bench, cx).starts_with(&format!("Following {BOB}")));
    let _ = bob.call("room.leave", json!({}));
    wait_for(cx, "Bob is gone", |cx| {
        bench.read_with(cx, |b, _| b.room_view().unwrap().avatars().shown.is_empty())
    });
    assert_eq!(
        status(&bench, cx),
        format!("{BOB} left the room; stopped following")
    );
    assert!(
        a_editor.read_with(cx, |e, _| e.remote_carets().is_empty()),
        "his caret goes with him"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

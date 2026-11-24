//! `SPEC-0323` v8 AK-34 in a real remote project window of the test harness: the backend runs in
//! this process with the room bound, the window is the room's admin and a second session on the
//! same backend is an agent participant (as in `room_window.rs`). Its comment shows as a mark,
//! the popover opens on a click, a reply, Resolve and Reopen reach the room, Add Comment starts a
//! thread on the caret's line, marks follow inserted lines, and deleting one's own takes two
//! clicks.

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferris_idea_remote::inprocess::{InProcessProject, Setup};
use ferris_idea_remote::{LiveProject, Timing};
use ferris_idea_session::{Backend, ClientSession, in_process};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::room_comments_model::CommentMark;
use ferris_idea_workbench::{EditorView, RemoteStateDir, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, px};
use serde_json::{Value, json};

const A: &str = "hello world\nsecond line\nthird line\n";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-room-comments-window-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("project/a.txt"), A).unwrap();
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

/// Clicks the control with test id `name`: a button of the popover from the semantic tree
/// (`SPEC-0319` behaviour 44), a gutter mark from its tooltip box.
fn click(name: &str, cx: &mut VisualTestContext) {
    // Twice: the marks' boxes are placed in one frame and drawn as controls in the next.
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());
    let from_tree = cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        tree.iter()
            .filter(|n| n.test_id() == Some(name))
            .find_map(|n| n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.))
            .map(|r| (r.x, r.y, r.w, r.h))
    });
    let (x, y, w, h) = from_tree
        .or_else(|| ferris_idea_workbench::hit_targets::get(name).map(|r| (r.x, r.y, r.w, r.h)))
        .unwrap_or_else(|| panic!("{name} is drawn"));
    let at = gpui::point(px(x + w / 2.), px(y + h / 2.));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn marks(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<CommentMark> {
    editor.read_with(cx, |e, _| e.comment_marks().to_vec())
}

fn mark(line: usize, comments: usize, threads: usize, open: bool) -> CommentMark {
    CommentMark {
        line,
        comments,
        threads,
        open,
    }
}

fn listed(bob: &ClientSession) -> Vec<Value> {
    bob.call("room.comments", json!({"doc": "a.txt"})).unwrap()["comments"]
        .as_array()
        .unwrap()
        .clone()
}

#[gpui::test]
fn comments_show_open_answer_resolve_and_follow_their_lines(cx: &mut TestAppContext) {
    ferris_idea_workbench::hit_targets::enable();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let dir = scratch();
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
    let project = InProcessProject::open_with(
        "room-comments-host",
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

    // Bob: an agent participant of its own (behaviour 23), role editor.
    let b = backend
        .lock()
        .unwrap()
        .clone()
        .expect("the backend is bound");
    let (side, server) = in_process();
    std::thread::spawn(move || b.serve(server));
    let folder = dir.join("project").display().to_string();
    let bob = ClientSession::open(side, &folder, "t", "h", None).unwrap();
    bob.call("room.join", json!({"agent": "Bob Stone"}))
        .unwrap();

    let handle = cx
        .update(|cx| ferris_idea_workbench::open_live_window(live.clone(), cx))
        .unwrap();
    let window: WindowHandle<WorkbenchView> = handle.downcast().unwrap();
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    wait_for(cx, "the window is in the room and may comment", |cx| {
        bench.read_with(cx, |b, _| b.room_can_comment() == Some(true))
    });

    bench.update_in(cx, |b, window, cx| {
        let p = b.remote_local_path("a.txt").unwrap();
        b.open(&p, window, cx);
    });
    let editor = {
        let mut found = None;
        wait_for(cx, "a.txt opens", |cx| {
            found = bench.read_with(cx, |b, cx| {
                b.editor_for(&b.remote_local_path("a.txt").unwrap(), cx)
            });
            found.is_some()
        });
        found.unwrap()
    };

    // Bob comments on "second line": the window shows a mark there without being asked.
    let first = bob
        .call(
            "room.comment",
            json!({"doc": "a.txt", "at": 12, "text": "Rename <b>this</b> [link](https://example.com)"}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    wait_for(cx, "Bob's comment shows as a mark", |cx| {
        marks(&editor, cx) == vec![mark(1, 1, 1, true)]
    });

    // A click on the mark opens the popover with Bob's thread; the window replies.
    click("room-comment-2", cx);
    let popup = bench
        .read_with(cx, |b, _| b.room_comment_popup())
        .expect("the popover opens");
    popup.read_with(cx, |p, _| {
        assert_eq!(p.place(), ("a.txt", 1));
        assert_eq!(p.threads().len(), 1);
        assert_eq!(p.threads()[0].first.name, "Bob Stone (room-comments-host)");
        assert_eq!(
            p.threads()[0].first.text,
            "Rename <b>this</b> [link](https://example.com)",
            "kept as plain text"
        );
        assert_eq!(p.reply_target(), Some(first));
    });
    popup.update(cx, |p, cx| p.set_input("  ", cx));
    click("room-comment-submit", cx);
    assert_eq!(
        popup.read_with(cx, |p, _| p.note().map(str::to_owned)),
        Some("Write a comment first".to_owned()),
        "nothing empty is sent"
    );
    popup.update(cx, |p, cx| p.set_input("Agreed", cx));
    click("room-comment-submit", cx);
    wait_for(
        cx,
        "the reply is in the thread and the mark counts two",
        |cx| marks(&editor, cx) == vec![mark(1, 2, 1, true)],
    );
    let reply = listed(&bob)
        .into_iter()
        .find(|c| c["text"] == "Agreed")
        .expect("Bob sees the reply");
    assert_eq!(reply["reply_to"], first);

    // Resolve turns the mark grey, for Bob too; Reopen opens it again.
    click(&format!("room-comment-resolve-{first}"), cx);
    wait_for(cx, "the thread is resolved", |cx| {
        marks(&editor, cx) == vec![mark(1, 2, 1, false)]
    });
    assert!(
        listed(&bob)
            .iter()
            .any(|c| c["id"] == first && c["resolved"] == true)
    );
    click(&format!("room-comment-resolve-{first}"), cx);
    wait_for(cx, "the thread is open again", |cx| {
        marks(&editor, cx) == vec![mark(1, 2, 1, true)]
    });

    // Add Comment on the caret's line ("third line") from the editor's menu entry.
    popup.update(cx, |_, cx| {
        cx.emit(ferris_idea_workbench::room_comment_popup::CommentPopupEvent::Close)
    });
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.room_comment_popup().is_none()));
    editor.update(cx, |e, cx| e.select_range(27..27, cx));
    bench.update_in(cx, |b, window, cx| b.room_add_comment(window, cx));
    let popup = bench.read_with(cx, |b, _| b.room_comment_popup()).unwrap();
    assert!(popup.read_with(cx, |p, _| p.writes_new() && p.place() == ("a.txt", 2)));
    popup.update(cx, |p, cx| {
        p.set_input("New thread", cx);
        p.submit(cx);
    });
    wait_for(cx, "the new thread shows on line 3", |cx| {
        marks(&editor, cx) == vec![mark(1, 2, 1, true), mark(2, 1, 1, true)]
    });
    let own = listed(&bob)
        .into_iter()
        .find(|c| c["text"] == "New thread")
        .expect("Bob sees the new thread");
    assert_eq!(own["at"], 24, "at the start of the caret's line");
    let own = own["id"].as_u64().unwrap();

    // Two lines inserted at the top: the marks move down with their lines.
    editor.update(cx, |e, cx| {
        e.replace_ranges(&[(0..0, "x\ny\n".to_owned())], cx)
    });
    wait_for(cx, "the marks follow their lines", |cx| {
        marks(&editor, cx) == vec![mark(3, 2, 1, true), mark(4, 1, 1, true)]
    });
    wait_for(cx, "the room moved the comments too", |_| {
        listed(&bob).iter().any(|c| c["id"] == own && c["at"] == 28)
    });

    // Deleting one's own comment takes two clicks; Bob's new comment arrives by itself.
    let popup = bench.read_with(cx, |b, _| b.room_comment_popup()).unwrap();
    wait_for(cx, "the popover follows its thread", |cx| {
        popup.read_with(cx, |p, _| {
            p.place() == ("a.txt", 4) && p.threads().len() == 1
        })
    });
    click(&format!("room-comment-delete-{own}"), cx);
    assert_eq!(popup.read_with(cx, |p, _| p.confirming()), Some(own));
    assert!(listed(&bob).iter().any(|c| c["id"] == own), "not yet");
    click("room-comment-delete-confirm", cx);
    wait_for(cx, "the comment is gone", |cx| {
        marks(&editor, cx) == vec![mark(3, 2, 1, true)]
    });
    assert!(!listed(&bob).iter().any(|c| c["id"] == own));
    bob.call(
        "room.comment",
        json!({"doc": "a.txt", "at": 0, "text": "Top"}),
    )
    .unwrap();
    wait_for(cx, "Bob's new comment arrives", |cx| {
        marks(&editor, cx) == vec![mark(0, 1, 1, true), mark(3, 2, 1, true)]
    });

    let _ = std::fs::remove_dir_all(&dir);
}

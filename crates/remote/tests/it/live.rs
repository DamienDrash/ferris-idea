//! SPEC-0317 v3: the window's live handle on a remote project against a backend in this process —
//! automatic reconnection on the schedule without losing an edit (behaviours 12, 14, 15), the
//! reconciliation of an editor's text with concurrent changes (behaviour 9), recovery copies
//! after a crash of the window (behaviour 13) and typing latency with 50 and 300 ms of added
//! round trip (AK-8).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_remote::inprocess::InProcessProject;
use ferris_idea_remote::{ConnState, DocSync, Follow, LiveProject, Restored, Synced, Timing};
use ferris_idea_session::{ErrorCode, Op, TextOp};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-live-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn fast() -> Timing {
    Timing {
        delays: [40, 80, 160].map(Duration::from_millis).to_vec(),
        then: Duration::from_millis(200),
        reattach: Duration::from_secs(600),
        heartbeat: Duration::from_millis(200),
        heartbeat_limit: Duration::from_secs(5),
        tick: Duration::from_millis(10),
    }
}

fn open(dir: &Path, reattach: Duration, delay: Duration, timing: Timing) -> LiveProject {
    let p = InProcessProject::open(
        "testhost",
        &dir.join("project"),
        &dir.join("state"),
        reattach,
        Some(dir.join("window/recovery")),
        delay,
    )
    .unwrap();
    LiveProject::start(Box::new(p), timing)
}

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    f()
}

/// An editor as the window has one: its text, kept in step with the session through `sync_doc`.
struct Editor {
    text: String,
    sync: DocSync,
}

impl Editor {
    fn open(live: &LiveProject, path: &str) -> Self {
        let (_, text, _, sync) = live.open_doc(path).unwrap();
        Self { text, sync }
    }

    fn type_at(&mut self, at: usize, s: &str) {
        let mut chars: Vec<char> = self.text.chars().collect();
        let at = at.min(chars.len());
        chars.splice(at..at, s.chars());
        self.text = chars.into_iter().collect();
    }

    fn sync(&mut self, live: &LiveProject) -> Synced {
        let r = live.sync_doc(&mut self.sync, &self.text).unwrap();
        if let Synced::Apply(ops) = &r {
            let len = self.text.chars().count();
            self.text = TextOp::from_ops(len, ops)
                .unwrap()
                .apply_str(&self.text)
                .unwrap();
            assert_eq!(self.text, self.sync.synced);
        }
        r
    }

    /// Syncs until the editor and the replica agree and nothing waits.
    fn settle(&mut self, live: &LiveProject) {
        assert!(
            wait_until(Duration::from_secs(10), || {
                self.sync(live);
                self.text == self.sync.synced && live.pending_edits() == Some(0)
            }),
            "did not settle: {:?}",
            live.state()
        );
    }
}

#[test]
fn a_break_reconnects_by_itself_and_typing_goes_on_without_loss() {
    let dir = scratch("break");
    std::fs::write(dir.join("project/a.txt"), "hello\n").unwrap();
    let live = open(&dir, Duration::from_secs(600), Duration::ZERO, fast());
    let mut e = Editor::open(&live, "a.txt");
    e.type_at(5, " world");
    e.sync(&live);
    e.settle(&live);
    assert!(matches!(live.state(), ConnState::Connected { .. }));

    // The network breaks; typing goes on into the queue.
    live.break_connection();
    let mut seen = Vec::new();
    e.type_at(0, "> ");
    e.sync(&live);
    let refused = live.save("a.txt", false);
    if let Err(err) = &refused {
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert!(
            err.message
                .starts_with("Not saved — reconnecting to testhost")
        );
    }
    let resumed = wait_until(Duration::from_secs(10), || {
        let s = live.state();
        if !seen.contains(&s.label()) {
            seen.push(s.label());
        }
        e.type_at(e.text.chars().count(), "x");
        e.sync(&live);
        // Connected again after a resume: the log says so. The `Reconnecting` state itself can
        // pass between two looks when the machine is busy, so it is not required to be seen.
        matches!(s, ConnState::Connected { .. }) && live.log().iter().any(|l| l.contains("Resumed"))
    });
    assert!(resumed, "states seen: {seen:?}; log: {:?}", live.log());
    e.settle(&live);
    // The save asked for during the break was carried out after the resume, and saving now
    // takes the rest.
    live.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/a.txt")).unwrap(),
        e.text
    );
    assert!(e.text.starts_with("> hello world\n") && e.text.ends_with('x'));
    assert!(live.log().iter().any(|l| l.contains("Resumed")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_changes_meet_the_editor_without_moving_its_text() {
    let dir = scratch("meet");
    std::fs::write(dir.join("project/m.rs"), "fn main() {}\n").unwrap();
    let p = InProcessProject::open(
        "testhost",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        None,
        Duration::ZERO,
    )
    .unwrap();
    let second = p.connect();
    let live = LiveProject::start(Box::new(p), fast());
    let mut e = Editor::open(&live, "m.rs");
    // The host changes the unmodified file: the editor follows and stays unmodified.
    std::fs::write(dir.join("project/m.rs"), "// head\nfn main() {}\n").unwrap();
    assert!(wait_until(Duration::from_secs(5), || {
        e.sync(&live);
        e.text == "// head\nfn main() {}\n"
    }));
    assert!(!e.sync.modified);
    // A second client (an agent) edits while the editor types something else that the session
    // has not seen yet: both changes stay, in both texts.
    let mut agent = ferris_idea_session::ClientSession::open(
        second,
        &dir.join("project").display().to_string(),
        "agent",
        "testhost",
        None,
    )
    .unwrap();
    agent.open_doc("m.rs").unwrap();
    e.type_at(e.text.chars().count(), "// tail\n");
    agent
        .edit(
            "m.rs",
            &[Op {
                at: 0,
                delete: 0,
                insert: "// top\n".into(),
            }],
        )
        .unwrap();
    assert!(agent.settle(Duration::from_secs(5)));
    assert!(wait_until(Duration::from_secs(5), || {
        live.with(|l| l.session().pump(Duration::ZERO));
        live.with(|l| {
            l.session_ref()
                .doc("m.rs")
                .unwrap()
                .text()
                .starts_with("// top")
        })
    }));
    e.settle(&live);
    assert_eq!(e.text, "// top\n// head\nfn main() {}\n// tail\n");
    assert!(e.sync.modified);
    assert!(wait_until(Duration::from_secs(5), || {
        agent.pump(Duration::from_millis(10));
        agent.doc("m.rs").unwrap().text() == e.text
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_restarted_host_gets_a_new_backend_and_the_text_stays() {
    let dir = scratch("restart");
    std::fs::write(dir.join("project/same.txt"), "same\n").unwrap();
    std::fs::write(dir.join("project/theirs.txt"), "theirs\n").unwrap();
    let p = InProcessProject::open(
        "testhost",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_secs(600),
        Some(dir.join("window/recovery")),
        Duration::ZERO,
    )
    .unwrap();
    let host = p.switch();
    let live = LiveProject::start(Box::new(p), fast());
    let mut a = Editor::open(&live, "same.txt");
    let mut b = Editor::open(&live, "theirs.txt");
    a.type_at(0, "mine ");
    b.type_at(0, "mine ");
    a.settle(&live);
    b.settle(&live);
    // The host goes away; attempts fail on the schedule while typing goes on.
    host.set_reachable(false);
    live.break_connection();
    assert!(wait_until(Duration::from_secs(5), || {
        a.type_at(0, "+");
        a.sync(&live);
        matches!(live.state(), ConnState::Reconnecting { attempt: 3.., .. })
    }));
    assert!(
        live.log()
            .iter()
            .any(|l| l.contains("attempt 1 failed") && l.contains("cannot reach"))
    );
    // It comes back restarted, and one file changed there meanwhile.
    std::fs::write(dir.join("project/theirs.txt"), "changed there\n").unwrap();
    host.restart();
    host.set_reachable(true);
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    assert!(live.log().iter().any(|l| l.contains("NewBackend")));
    a.settle(&live);
    b.settle(&live);
    // Unchanged on the host: the edits followed. Changed: the editor kept the window's text and
    // saving it needs a confirmation (the host's version is in its local history afterwards).
    assert!(a.text.ends_with("mine same\n") && a.text.starts_with('+'));
    assert_eq!(b.text, "mine theirs\n");
    // `doc_states` never waits for the session (it serves the window); asked until it is free.
    let mut states = None;
    assert!(wait_until(Duration::from_secs(10), || {
        states = live.doc_states();
        states.is_some()
    }));
    let states = states.unwrap();
    let theirs = states["theirs.txt"];
    assert!(theirs.modified && theirs.disk_changed && theirs.pending == 0);
    assert!(!states["same.txt"].disk_changed);
    live.save("theirs.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/theirs.txt")).unwrap(),
        "mine theirs\n"
    );
    let history = ferris_idea_core::LocalHistory::open(&dir.join("project")).unwrap();
    assert!(!history.revisions("theirs.txt").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_reattach_period_ends_automatic_attempts_and_reconnect_starts_a_new_backend() {
    let dir = scratch("expire");
    std::fs::write(dir.join("project/n.txt"), "n\n").unwrap();
    let mut timing = fast();
    timing.reattach = Duration::from_millis(500);
    let p = InProcessProject::open(
        "testhost",
        &dir.join("project"),
        &dir.join("state"),
        Duration::from_millis(300),
        None,
        Duration::ZERO,
    )
    .unwrap();
    let host = p.switch();
    let live = LiveProject::start(Box::new(p), timing);
    let mut e = Editor::open(&live, "n.txt");
    e.type_at(0, "kept ");
    e.settle(&live);
    // The host stays unreachable longer than it keeps the session: the window stops trying.
    host.set_reachable(false);
    live.break_connection();
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Disconnected { ref reason } if reason.contains("reattach period")
    )));
    let attempts = |live: &LiveProject| live.log().iter().filter(|l| l.contains("failed")).count();
    let before = attempts(&live);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(attempts(&live), before);
    // Reconnect gets a new backend; the edit made before the break is kept.
    host.set_reachable(true);
    live.reconnect_now();
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    assert!(live.log().iter().any(|l| l.contains("NewBackend")));
    e.type_at(0, "more ");
    e.settle(&live);
    live.save("n.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/n.txt")).unwrap(),
        "more kept n\n"
    );
    // Disconnect stops the attempts until Reconnect: the window's supervisor takes it in its own
    // time (slow under load), so wait for the state, then check that it stays.
    live.disconnect();
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Disconnected { .. }
    )));
    let before = attempts(&live);
    std::thread::sleep(Duration::from_millis(300));
    assert!(matches!(live.state(), ConnState::Disconnected { .. }));
    assert_eq!(attempts(&live), before, "no attempt while disconnected");
    live.reconnect_now();
    assert!(wait_until(Duration::from_secs(10), || matches!(
        live.state(),
        ConnState::Connected { .. }
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recovery_copies_survive_a_crash_of_the_window_and_restore_on_request() {
    let dir = scratch("crash");
    std::fs::write(dir.join("project/r.txt"), "base\n").unwrap();
    std::fs::write(dir.join("project/s.txt"), "base s\n").unwrap();
    {
        let live = open(&dir, Duration::from_secs(600), Duration::ZERO, fast());
        let mut r = Editor::open(&live, "r.txt");
        let mut s = Editor::open(&live, "s.txt");
        r.type_at(4, " typed");
        s.type_at(0, "S: ");
        r.settle(&live);
        s.settle(&live);
        // While the session is busy elsewhere, the editor's newest text goes to the copy.
        // The holder says when it holds the session; a fixed wait was too short under load,
        // and the sync then found the session free.
        let busy = live.clone();
        let (held, holding) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            busy.with(|_| {
                held.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(300));
            });
        });
        holding.recv_timeout(Duration::from_secs(30)).unwrap();
        r.type_at(0, "late ");
        assert_eq!(r.sync(&live), Synced::Busy);
        holder.join().unwrap();
        // The window "crashes": nothing is saved or closed, and its half of the session is gone
        // (a crashed process writes no more copies).
        drop(live);
    }
    std::fs::write(dir.join("project/s.txt"), "changed on the host\n").unwrap();
    let live = open(&dir, Duration::from_secs(600), Duration::ZERO, fast());
    let copies = live.recovery_copies();
    let paths: Vec<&str> = copies.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, ["r.txt", "s.txt"]);
    let text = |p: &str| copies.iter().find(|c| c.path == p).unwrap().text.clone();
    assert_eq!(text("r.txt"), "late base typed\n");
    let outcome = live.restore(&copies);
    assert_eq!(
        outcome,
        [
            ("r.txt".to_owned(), Restored::Followed),
            ("s.txt".to_owned(), Restored::HostChanged)
        ]
    );
    let r = Editor::open(&live, "r.txt");
    assert_eq!(r.text, "late base typed\n");
    assert!(r.sync.modified);
    live.save("r.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("project/r.txt")).unwrap(),
        "late base typed\n"
    );
    let left: Vec<String> = live.recovery_copies().into_iter().map(|c| c.path).collect();
    assert_eq!(left, ["s.txt"]);
    live.discard_copies(&live.recovery_copies());
    assert!(live.recovery_copies().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// CPU time this thread has used.
fn thread_cpu() -> Duration {
    let t = nix::time::clock_gettime(nix::time::ClockId::CLOCK_THREAD_CPUTIME_ID).unwrap();
    Duration::new(t.tv_sec() as u64, t.tv_nsec() as u32)
}

fn percentile(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

/// AK-8: typing never waits for the backend, so its synchronous work stays within a local
/// keystroke's budget with 50 and 300 ms of added round trip.
#[test]
fn typing_latency_does_not_follow_the_round_trip() {
    for delay_ms in [0u64, 50, 300] {
        let dir = scratch(&format!("latency-{delay_ms}"));
        std::fs::write(dir.join("project/t.rs"), "fn main() {\n}\n".repeat(200)).unwrap();
        let live = open(
            &dir,
            Duration::from_secs(600),
            Duration::from_millis(delay_ms),
            fast(),
        );
        let mut e = Editor::open(&live, "t.rs");
        let (mut work, mut wall) = (Vec::new(), Vec::new());
        for i in 0..150 {
            e.type_at(11 + i, "x");
            let (c0, t0) = (thread_cpu(), Instant::now());
            e.sync(&live);
            work.push(thread_cpu().saturating_sub(c0));
            wall.push(t0.elapsed());
            std::thread::sleep(Duration::from_millis(2));
        }
        let (p50, p95) = (percentile(&mut work, 0.5), percentile(&mut work, 0.95));
        let (wall50, wall95) = (percentile(&mut wall, 0.5), percentile(&mut wall, 0.95));
        eprintln!(
            "added round trip {delay_ms} ms: keystroke work p50 {p50:?}, p95 {p95:?}; wall p50 {wall50:?}, p95 {wall95:?}"
        );
        // SPEC-0260: a keystroke's painted frame within 8 / 16 ms at p50 / p95. The synchronous
        // work of a remote keystroke (the typing thread's CPU time, so a busy machine that
        // preempts the thread does not count) stays below it in a debug build.
        assert!(
            p95 < Duration::from_millis(16),
            "work p95 {p95:?} at {delay_ms} ms"
        );
        assert!(
            p50 < Duration::from_millis(8),
            "work p50 {p50:?} at {delay_ms} ms"
        );
        // And it does not wait for the round trip: waiting would put the median keystroke at the
        // added delay; it stays below half of it (wall time, where waiting shows).
        if delay_ms > 0 {
            assert!(
                wall50 < Duration::from_millis(delay_ms) / 2,
                "typing waited for the round trip: wall p50 {wall50:?} at {delay_ms} ms"
            );
        }
        e.settle(&live);
        live.save("t.rs", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("project/t.rs")).unwrap(),
            e.text
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn editing_ops_are_plain_ranges() {
    // The ranges handed to the editor are those of its current text (sorted, not overlapping).
    let op = TextOp::diff("abc", "aXc");
    assert_eq!(
        op.to_ops(),
        [Op {
            at: 1,
            delete: 1,
            insert: "X".into()
        }]
    );
}

/// The window's frame loop for one document, as `remote_tick` runs it.
struct Frames {
    editor: Editor,
    follow: Follow,
    editor_rev: u64,
    states: std::collections::HashMap<String, ferris_idea_remote::DocState>,
    /// The last revision for which another request took the session between the look at the
    /// documents' states and the meeting (the frame loop's unlucky moment).
    held_at: u64,
}

impl Frames {
    fn tick(&mut self, live: &LiveProject) {
        if let Some(states) = live.doc_states() {
            self.states = states;
        }
        let state = self.states.get(&self.editor.sync.doc).copied();
        if self
            .follow
            .due(&self.editor.sync, Some(self.editor_rev), state.as_ref())
        {
            // A folder listing started by the same host change holds the session just now.
            let revision = state.map_or(0, |s| s.revision);
            let holder = (revision != self.held_at).then(|| {
                self.held_at = revision;
                let (held, is_held) = std::sync::mpsc::channel();
                let (release, released) = std::sync::mpsc::channel::<()>();
                let other = live.clone();
                let thread = std::thread::spawn(move || {
                    other.with(|_| {
                        held.send(()).unwrap();
                        let _ = released.recv();
                    });
                });
                is_held.recv().unwrap();
                (release, thread)
            });
            let outcome = live.sync_doc(&mut self.editor.sync, &self.editor.text);
            if let Some((release, thread)) = holder {
                assert_eq!(outcome, Ok(Synced::Busy));
                release.send(()).unwrap();
                thread.join().unwrap();
            }
            if let Ok(Synced::Apply(ops)) = &outcome {
                let len = self.editor.text.chars().count();
                self.editor.text = TextOp::from_ops(len, ops)
                    .unwrap()
                    .apply_str(&self.editor.text)
                    .unwrap();
                self.editor_rev += 1;
            }
            self.follow.met(
                &outcome,
                &mut self.editor.sync,
                self.editor_rev,
                state.as_ref(),
            );
        }
    }

    fn until(&mut self, live: &LiveProject, what: &str, mut done: impl FnMut(&Self) -> bool) {
        let t0 = Instant::now();
        while !done(self) {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "{what}: the editor shows {:?}, the session {:?}, connection {:?}",
                self.editor.text,
                live.with(|l| l.session_ref().doc("m.txt").map(|d| d.text())),
                live.state()
            );
            self.tick(live);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// SPEC-0317 behaviour 9 and AK-7: a change made on the host during a break always reaches the
/// open editor after the reconnection, also while other requests of the window (the tree listing
/// folders after the same change) hold the session at the moment the change arrives.
#[test]
fn a_host_change_during_a_break_always_reaches_the_editor() {
    let dir = scratch("flake");
    let file = dir.join("project/m.txt");
    std::fs::write(&file, "start\n").unwrap();
    let live = open(&dir, Duration::from_secs(600), Duration::ZERO, fast());
    let editor = Editor::open(&live, "m.txt");
    let follow = Follow::new(0);
    let mut f = Frames {
        editor,
        follow,
        editor_rev: 0,
        states: Default::default(),
        held_at: 1,
    };
    // The tree: a host change makes the window list folders, which holds the session.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lister = {
        let (live, stop) = (live.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = live.list(".");
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    for round in 0..40 {
        live.break_connection();
        let typed = round % 2 == 1;
        let line = format!("host change {round}\n");
        std::fs::write(&file, &line).unwrap();
        f.until(&live, &format!("round {round}, host change"), |f| {
            f.editor.text == line
        });
        // Typing only after the backend has seen the change: an edit that reaches it first makes
        // the document modified, and a disk change then is the window's decision (docs.rs).
        if typed {
            live.break_connection();
            f.editor.type_at(0, "#");
            f.editor_rev += 1;
            f.tick(&live);
        }
        let expected = if typed {
            format!("#{line}")
        } else {
            line.clone()
        };
        f.until(&live, &format!("round {round}"), |f| {
            f.editor.text == expected
        });
        if typed {
            // The text was typed locally; saving means something once the backend has it.
            f.until(&live, &format!("round {round}, typing arrived"), |f| {
                f.states.get("m.txt").is_some_and(|s| s.modified)
            });
            let saved = (0..200).find_map(|_| {
                std::thread::sleep(Duration::from_millis(5));
                live.save("m.txt", false).ok()
            });
            assert!(saved.is_some(), "round {round}: not saved");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), expected);
        }
        f.until(&live, &format!("round {round}, settled"), |f| {
            !f.states.get("m.txt").is_some_and(|s| s.modified)
        });
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    lister.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

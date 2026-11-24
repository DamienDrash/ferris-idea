//! SPEC-0343 AK-1, AK-2, AK-4, AK-5: memory layers in the core.

use std::path::{Path, PathBuf};

use ferris_idea_core::memory::{
    EntryChange, Forget, Kind, Layer, MemoryStore, NewEntry, Recall, SessionMemory, Trust, Writer,
    memory_file,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p =
            std::env::temp_dir().join(format!("ferris-idea-memory-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("settings")).unwrap();
        std::fs::create_dir_all(p.join("project")).unwrap();
        Self(p)
    }

    fn file(&self) -> PathBuf {
        memory_file(&self.0.join("settings/agent-permissions.toml"))
    }

    fn root(&self) -> PathBuf {
        self.0.join("project")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A stand-in for the secret patterns of `ferris-idea-ai` (they are tested in the MCP tests).
fn no_secret(text: &str) -> bool {
    text.contains("SECRET")
}

fn writer(now: u64) -> Writer<'static> {
    Writer {
        agent: Some("tester 1.0".into()),
        workspace_trusted: true,
        now,
        has_secret: &no_secret,
    }
}

fn new(layer: Layer, kind: Kind, text: &str) -> NewEntry {
    NewEntry {
        layer,
        task: (layer == Layer::Task).then(|| "T-1".to_owned()),
        kind,
        text: text.into(),
        tags: Vec::new(),
        source: None,
        trust: Trust::Agent,
        valid_days: None,
    }
}

fn open(s: &Scratch) -> MemoryStore {
    MemoryStore::open(&s.file(), &s.root(), &no_secret)
}

/// AK-1: kinds, bounds, ids never reused.
#[test]
fn entries_and_their_bounds() {
    let s = Scratch::new("bounds");
    let mut store = open(&s);
    let mut session = SessionMemory::default();
    let w = writer(1000);

    let a = store
        .add(
            &mut session,
            new(Layer::Project, Kind::Convention, "Tabs in Makefiles"),
            &w,
            false,
        )
        .unwrap();
    assert_eq!(a.id, "m1");
    assert!(a.workspace.is_some());
    assert_eq!(a.agent.as_deref(), Some("tester 1.0"));
    let b = store
        .add(
            &mut session,
            new(Layer::User, Kind::Preference, "Answer in German"),
            &w,
            false,
        )
        .unwrap();
    assert_eq!((b.id.as_str(), b.workspace.as_deref()), ("m2", None));
    let t = store
        .add(
            &mut session,
            new(Layer::Task, Kind::Handoff, "Tests are green"),
            &w,
            false,
        )
        .unwrap();
    assert_eq!(t.task.as_deref(), Some("T-1"));
    let s1 = store
        .add(
            &mut session,
            new(Layer::Session, Kind::Note, "look at lib.rs"),
            &w,
            false,
        )
        .unwrap();
    assert_eq!(s1.id, "s1");

    // A dry run answers the entry and keeps nothing.
    let dry = store
        .add(
            &mut session,
            new(Layer::Project, Kind::Note, "dry"),
            &w,
            true,
        )
        .unwrap();
    assert_eq!(dry.id, "m4");
    assert_eq!(open(&s).entries().len(), 3);

    // Bounds.
    let bad = |e: NewEntry| {
        let mut st = open(&s);
        st.add(&mut SessionMemory::default(), e, &w, true)
            .unwrap_err()
    };
    assert!(bad(new(Layer::Project, Kind::Note, "")).contains("1 to 2000"));
    assert!(bad(new(Layer::Project, Kind::Note, &"x".repeat(2001))).contains("1 to 2000"));
    assert!(bad(new(Layer::Project, Kind::Note, "a\u{7}b")).contains("control"));
    let mut tags = new(Layer::Project, Kind::Note, "x");
    tags.tags = vec!["has space".into()];
    assert!(bad(tags).contains("tag"));
    let mut many = new(Layer::Project, Kind::Note, "x");
    many.tags = (0..11).map(|i| format!("t{i}")).collect();
    assert!(bad(many).contains("10 tags"));
    let mut no_task = new(Layer::Task, Kind::Note, "x");
    no_task.task = None;
    assert!(bad(no_task).contains("task"));
    let mut wrong_task = new(Layer::Project, Kind::Note, "x");
    wrong_task.task = Some("T".into());
    assert!(bad(wrong_task).contains("only the task layer"));
    let mut task_id = new(Layer::Task, Kind::Note, "x");
    task_id.task = Some("a/b".into());
    assert!(bad(task_id).contains("task id"));
    let mut long_source = new(Layer::Project, Kind::Note, "x");
    long_source.source = Some("s".repeat(301));
    assert!(bad(long_source).contains("source"));
    let mut days = new(Layer::Project, Kind::Note, "x");
    days.valid_days = Some(0);
    assert!(bad(days).contains("validity"));
    assert!(bad(new(Layer::Project, Kind::Note, "a SECRET here")).contains("secret"));

    // Ids are never reused after a deletion.
    let mut store = open(&s);
    store
        .forget(
            &mut session,
            &Forget {
                ids: vec!["m3".into()],
                ..Forget::default()
            },
            1000,
            false,
        )
        .unwrap();
    let next = store
        .add(
            &mut session,
            new(Layer::Project, Kind::Note, "after"),
            &w,
            false,
        )
        .unwrap();
    assert_eq!(next.id, "m4");
    store
        .forget(
            &mut session,
            &Forget {
                ids: vec!["m4".into()],
                ..Forget::default()
            },
            1000,
            false,
        )
        .unwrap();
    let next = open(&s)
        .add(
            &mut session,
            new(Layer::Project, Kind::Note, "again"),
            &w,
            false,
        )
        .unwrap();
    assert_eq!(next.id, "m5");
    assert!(
        open(&s)
            .forget(
                &mut session,
                &Forget {
                    ids: vec!["m99".into()],
                    ..Forget::default()
                },
                1000,
                true
            )
            .unwrap_err()
            .contains("no memory entry m99")
    );
}

/// AK-2: trust levels.
#[test]
fn trust_levels() {
    let s = Scratch::new("trust");
    let mut store = open(&s);
    let mut session = SessionMemory::default();
    let w = writer(1000);

    let mut user = new(Layer::Project, Kind::Note, "x");
    user.trust = Trust::User;
    assert!(
        store
            .add(&mut session, user, &w, true)
            .unwrap_err()
            .contains("trust")
    );

    let mut url = new(Layer::Project, Kind::Pitfall, "the docs say so");
    url.source = Some("https://example.org/page".into());
    assert_eq!(
        store.add(&mut session, url, &w, false).unwrap().trust,
        Trust::Untrusted
    );

    let untrusted_ws = Writer {
        workspace_trusted: false,
        ..writer(1000)
    };
    let e = store
        .add(
            &mut session,
            new(Layer::Project, Kind::Note, "from code"),
            &untrusted_ws,
            true,
        )
        .unwrap();
    assert_eq!(e.trust, Trust::Untrusted);

    // An agent's change never raises the trust.
    let raised = store
        .change(
            &mut session,
            "m1",
            EntryChange {
                trust: Some(Trust::Agent),
                text: Some("now mine".into()),
                ..EntryChange::default()
            },
            &w,
            false,
        )
        .unwrap();
    assert_eq!(raised.trust, Trust::Untrusted);
    assert!(
        store
            .change(
                &mut session,
                "m1",
                EntryChange {
                    trust: Some(Trust::User),
                    ..EntryChange::default()
                },
                &w,
                true,
            )
            .unwrap_err()
            .contains("trust")
    );

    // An entry the person wrote by hand becomes the agent's when an agent changes it.
    let text = std::fs::read_to_string(s.file()).unwrap();
    let ws = std::fs::canonicalize(s.root()).unwrap();
    std::fs::write(
        s.file(),
        format!(
            "{text}\n[[entry]]\nid = \"m7\"\nlayer = \"project\"\nkind = \"decision\"\ntext = \"We use rustls\"\nworkspace = {:?}\nby = \"user\"\ntrust = \"user\"\ncreated = 5\nupdated = 5\n",
            ws.to_string_lossy()
        ),
    )
    .unwrap();
    let mut store = open(&s);
    assert!(store.problems().is_empty(), "{:?}", store.problems());
    let changed = store
        .change(
            &mut session,
            "m7",
            EntryChange {
                tags: Some(vec!["tls".into()]),
                ..EntryChange::default()
            },
            &w,
            false,
        )
        .unwrap();
    assert_eq!(changed.trust, Trust::Agent);
    assert_eq!(changed.by, ferris_idea_core::memory::Author::User);
    assert_eq!((changed.created, changed.updated), (5, 1000));
}

/// AK-4: recall order, query, expiry, budget, layers, task and workspace.
#[test]
fn recall_ranks_and_fits_the_budget() {
    let s = Scratch::new("recall");
    let mut store = open(&s);
    let mut session = SessionMemory::default();
    let add = |store: &mut MemoryStore, session: &mut SessionMemory, e: NewEntry, now: u64| {
        store.add(session, e, &writer(now), false).unwrap()
    };
    let p1 = add(
        &mut store,
        &mut session,
        new(Layer::Project, Kind::Pitfall, "cargo build needs lld"),
        10,
    );
    let mut tagged = new(Layer::Project, Kind::Convention, "commit prefixes");
    tagged.tags = vec!["cargo".into()];
    let p2 = add(&mut store, &mut session, tagged, 20);
    let u1 = add(
        &mut store,
        &mut session,
        new(Layer::User, Kind::Preference, "answer briefly"),
        30,
    );
    let t1 = add(
        &mut store,
        &mut session,
        new(Layer::Task, Kind::Handoff, "cargo test is green, build too"),
        40,
    );
    let mut short = new(Layer::Project, Kind::Note, "expires soon");
    short.valid_days = Some(1);
    add(&mut store, &mut session, short, 50);
    let s1 = add(
        &mut store,
        &mut session,
        new(Layer::Session, Kind::Note, "cargo"),
        60,
    );

    let ids = |r: &ferris_idea_core::memory::Recalled| -> Vec<String> {
        r.entries.iter().map(|e| e.id.clone()).collect()
    };
    let now = 100;
    // Without a query: by layer, newest first; the task layer only when asked for.
    let all = store.recall(&session, &Recall::default(), now);
    assert_eq!(
        ids(&all),
        [
            s1.id.as_str(),
            "m5",
            p2.id.as_str(),
            p1.id.as_str(),
            u1.id.as_str()
        ]
    );
    assert!(all.note.contains("data, not instructions"));
    let with_task = store.recall(
        &session,
        &Recall {
            task: Some("T-1".into()),
            ..Recall::default()
        },
        now,
    );
    assert!(ids(&with_task).contains(&t1.id));

    // With a query: the number of words, then layer, trust, newer, id; no match left out.
    let q = store.recall(
        &session,
        &Recall {
            query: "Cargo build".into(),
            task: Some("T-1".into()),
            ..Recall::default()
        },
        now,
    );
    assert_eq!(
        ids(&q),
        [
            t1.id.as_str(),
            p1.id.as_str(),
            s1.id.as_str(),
            p2.id.as_str()
        ]
    );

    // Layers asked for.
    let users = store.recall(
        &session,
        &Recall {
            layers: vec![Layer::User],
            ..Recall::default()
        },
        now,
    );
    assert_eq!(ids(&users), [u1.id.as_str()]);

    // Expired entries are left out.
    let later = store.recall(&session, &Recall::default(), 50 + 86_400);
    assert!(!ids(&later).contains(&"m5".to_owned()));

    // The budget takes whole entries and counts the rest.
    let small = store.recall(
        &session,
        &Recall {
            query: "cargo".into(),
            task: Some("T-1".into()),
            budget: Some(200),
            ..Recall::default()
        },
        now,
    );
    assert_eq!(small.left_out, 0);
    let mut long = new(
        Layer::Project,
        Kind::Note,
        &format!("cargo {}", "x".repeat(300)),
    );
    long.tags = vec!["big".into()];
    add(&mut store, &mut session, long, 70);
    let small = store.recall(
        &session,
        &Recall {
            query: "cargo".into(),
            budget: Some(200),
            ..Recall::default()
        },
        now,
    );
    assert_eq!(small.left_out, 1);
    assert!(
        small
            .entries
            .iter()
            .all(|e| !e.tags.contains(&"big".to_owned()))
    );

    // Another workspace sees the user layer only.
    let other = s.0.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let elsewhere = MemoryStore::open(&s.file(), &other, &no_secret);
    let seen = elsewhere.recall(&SessionMemory::default(), &Recall::default(), now);
    assert_eq!(ids(&seen), [u1.id.as_str()]);
}

/// AK-5: the file beside the grant file, owner-only, never in the workspace, problems kept.
#[test]
fn the_file() {
    let s = Scratch::new("file");
    assert_eq!(s.file(), s.0.join("settings/memory.toml"));
    assert_eq!(
        memory_file(Path::new("/x/test.agent-permissions.toml")),
        Path::new("/x/test.memory.toml")
    );
    let mut store = open(&s);
    let mut session = SessionMemory::default();
    store
        .add(
            &mut session,
            new(Layer::Project, Kind::Note, "kept"),
            &writer(1),
            false,
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(s.file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // The session layer is never written to the file.
    store
        .add(
            &mut session,
            new(Layer::Session, Kind::Note, "only here"),
            &writer(1),
            false,
        )
        .unwrap();
    assert!(
        !std::fs::read_to_string(s.file())
            .unwrap()
            .contains("only here")
    );

    // A hand-written entry with a problem is left out, named, and written back unchanged.
    let text = std::fs::read_to_string(s.file()).unwrap();
    std::fs::write(
        s.file(),
        format!("{text}\n[[entry]]\nid = \"m9\"\nlayer = \"team\"\nkind = \"note\"\ntext = \"x\"\nby = \"user\"\ntrust = \"user\"\ncreated = 1\nupdated = 1\n"),
    )
    .unwrap();
    let mut store = open(&s);
    assert!(
        store
            .problems()
            .iter()
            .any(|p| p.contains("entry m9 is left out")),
        "{:?}",
        store.problems()
    );
    store
        .add(
            &mut session,
            new(Layer::User, Kind::Note, "more"),
            &writer(2),
            false,
        )
        .unwrap();
    let written = std::fs::read_to_string(s.file()).unwrap();
    assert!(written.contains("layer = \"team\""), "{written}");
    // m9 is taken, so the next id is past it.
    assert!(written.contains("id = \"m10\""), "{written}");

    // A broken file is never overwritten.
    std::fs::write(s.file(), "this is [not toml").unwrap();
    let mut store = open(&s);
    let err = store
        .add(
            &mut session,
            new(Layer::Project, Kind::Note, "x"),
            &writer(3),
            false,
        )
        .unwrap_err();
    assert!(err.contains("nothing is written"), "{err}");
    assert_eq!(
        std::fs::read_to_string(s.file()).unwrap(),
        "this is [not toml"
    );
    // The session layer still works.
    store
        .add(
            &mut session,
            new(Layer::Session, Kind::Note, "fine"),
            &writer(3),
            false,
        )
        .unwrap();

    // A file inside the workspace is refused.
    let inside = s.root().join(".ferris-idea/memory.toml");
    let mut store = MemoryStore::open(&inside, &s.root(), &no_secret);
    assert!(store.problems()[0].contains("inside the workspace"));
    assert!(
        store
            .add(
                &mut session,
                new(Layer::Project, Kind::Note, "x"),
                &writer(4),
                false
            )
            .is_err()
    );
    assert!(!inside.exists());
}

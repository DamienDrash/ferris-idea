//! SPEC-0352 AK-1 to AK-8: the work item model, its files, its transition table, keys, merges,
//! the rights of agents, full-text search and where the store lives.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferris_idea_core::tickets::{
    Actor, Criterion, Filter, ItemChange, Kind, Link, LinkKind, Loaded, NewItem, Priority,
    STORE_GITATTRIBUTES, State, Store, StoreKind, choice_file, id_from, is_id, item_toml,
    key_chars, load_store_choice, parse_item, parse_rfc3339, private_store_dir, rfc3339,
    save_store_choice, search, workspace_store,
};

const NOW: u64 = 1_795_000_000;

fn temp(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-tickets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn person() -> Actor {
    Actor::person("ada")
}

fn agent() -> Actor {
    Actor::agent("claude-code 2.1")
}

/// Creates an item by `actor` and returns the store's items afterwards with its id.
fn create(store: &Store, new: NewItem, actor: &Actor, now: u64) -> (Loaded, String) {
    let loaded = store.load();
    let plan = store.plan_create(&loaded, new, actor, now).unwrap();
    store.apply(&plan, false).unwrap();
    (store.load(), plan.item.id)
}

fn update(store: &Store, key: &str, change: ItemChange, actor: &Actor) -> Result<(), String> {
    let loaded = store.load();
    let plan = store.plan_update(&loaded, key, change, actor, NOW + 60)?;
    store.apply(&plan, false)
}

#[test]
fn times_round_trip() {
    for t in [0, 951_782_400, NOW, 4_102_444_799] {
        assert_eq!(parse_rfc3339(&rfc3339(t)), Some(t), "{}", rfc3339(t));
    }
    assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
    assert_eq!(parse_rfc3339("2026-11-18 09:30:00"), None);
}

/// AK-1: the files of an item, written deterministically and read back.
#[test]
fn an_item_is_written_as_mergeable_text_and_read_back() {
    let dir = temp("files");
    let store = Store::new(dir.join("store"), StoreKind::Private);
    let mut new = NewItem::new(Kind::Bug, "Login fails with \"quotes\"");
    new.description = "Steps:\n1. open\n2. click".into();
    new.labels = ["ui".to_owned(), "auth".to_owned()].into();
    new.criteria = vec![Criterion {
        text: "a test covers it".into(),
        met: false,
    }];
    new.links = [
        Link {
            kind: LinkKind::Commit,
            target: "abc1234".into(),
        },
        Link {
            kind: LinkKind::Branch,
            target: "fix/login".into(),
        },
    ]
    .into();
    let (loaded, id) = create(&store, new, &person(), NOW);
    let t = loaded.resolve(&id).unwrap();
    assert_eq!(t.item.state, State::Proposed);
    assert_eq!(t.description, "Steps:\n1. open\n2. click");
    assert!(t.state_verified);

    let text =
        std::fs::read_to_string(store.root().join("items").join(&id).join("item.toml")).unwrap();
    assert!(text.starts_with("[ferris-idea]\nkind = \"work-item\"\nformat = 1\n\n[item]\nid = "));
    // Fixed order, a blank line between fields, sorted lists one entry per line.
    let order: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split(" = ").next().filter(|_| l.contains(" = ")))
        .collect();
    assert_eq!(
        order,
        [
            "kind",
            "format",
            "id",
            "kind",
            "title",
            "state",
            "priority",
            "created",
            "created_by",
            "labels",
            "text",
            "met",
            "kind",
            "target",
            "kind",
            "target"
        ]
    );
    assert!(
        text.contains("labels = [\n  \"auth\",\n  \"ui\",\n]"),
        "{text}"
    );
    assert!(text.contains("title = \"Login fails with \\\"quotes\\\"\"\n\nstate"));
    assert!(!text.contains("updated"), "no field changes on every write");
    assert_eq!(item_toml(&parse_item(&text).unwrap()), text);

    // A comment is its own file; every change is one event line.
    let plan = store
        .plan_comment(&loaded, &t.key, "Seen on macOS too.", &agent(), NOW + 5)
        .unwrap();
    store.apply(&plan, false).unwrap();
    update(
        &store,
        &id,
        ItemChange {
            state: Some(State::Ready),
            add_labels: vec!["mac".into()],
            ..Default::default()
        },
        &person(),
    )
    .unwrap();
    let loaded = store.load();
    let t = loaded.resolve(&id).unwrap();
    assert_eq!(t.comments.len(), 1);
    assert_eq!(t.comments[0].author, agent());
    assert_eq!(t.comments[0].text, "Seen on macOS too.");
    let comment_files: Vec<_> =
        std::fs::read_dir(store.root().join("items").join(&id).join("comments"))
            .unwrap()
            .collect();
    assert_eq!(comment_files.len(), 1);
    let fields: Vec<&str> = t.events.iter().map(|e| e.field.as_str()).collect();
    assert_eq!(fields, ["created", "comment", "state", "labels"]);
    assert!(t.state_verified);
    assert!(t.item.labels.contains("mac"));
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let store_toml = std::fs::read_to_string(store.root().join("store.toml")).unwrap();
    assert!(store_toml.contains("kind = \"work-store\""));
    // A newer format and a wrong kind are refused.
    let newer = text.replace("format = 1", "format = 2");
    assert!(
        parse_item(&newer)
            .unwrap_err()
            .contains("newer Ferris IDEA")
    );
    let wrong = text.replace("kind = \"work-item\"", "kind = \"run\"");
    assert!(
        parse_item(&wrong)
            .unwrap_err()
            .contains("not a `work-item`")
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// AK-2: the transition table, its guards, and states edited by hand.
#[test]
fn states_follow_the_table_and_hand_edits_read_as_unverified() {
    let dir = temp("states");
    let store = Store::new(dir.join("s"), StoreKind::Private);
    let mut new = NewItem::new(Kind::Task, "Write the parser");
    new.criteria = vec![Criterion {
        text: "tests pass".into(),
        met: false,
    }];
    let (_, id) = create(&store, new, &person(), NOW);
    let to = |s: State| ItemChange {
        state: Some(s),
        ..Default::default()
    };
    let err = update(&store, &id, to(State::Done), &person()).unwrap_err();
    assert!(err.contains("`proposed` cannot go to `done`"), "{err}");
    assert!(err.contains("ready, blocked, cancelled"), "{err}");
    update(&store, &id, to(State::Ready), &person()).unwrap();
    let err = update(&store, &id, to(State::Assigned), &person()).unwrap_err();
    assert!(err.contains("needs an assignee"), "{err}");
    update(
        &store,
        &id,
        ItemChange {
            state: Some(State::Assigned),
            assignee: Some(Some(agent())),
            ..Default::default()
        },
        &person(),
    )
    .unwrap();
    update(&store, &id, to(State::InProgress), &person()).unwrap();
    update(&store, &id, to(State::InReview), &person()).unwrap();
    let err = update(&store, &id, to(State::Done), &person()).unwrap_err();
    assert!(err.contains("not met: tests pass"), "{err}");
    update(
        &store,
        &id,
        ItemChange {
            state: Some(State::Done),
            criteria: Some(vec![Criterion {
                text: "tests pass".into(),
                met: true,
            }]),
            ..Default::default()
        },
        &person(),
    )
    .unwrap();
    let loaded = store.load();
    let t = loaded.resolve(&id).unwrap();
    assert_eq!(t.item.state, State::Done);
    assert!(t.state_verified);

    // An item edited by hand to a state the events do not reach.
    let (_, other) = create(
        &store,
        NewItem::new(Kind::Task, "Hand edited"),
        &person(),
        NOW,
    );
    let file = store.root().join("items").join(&other).join("item.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        text.replace("state = \"proposed\"", "state = \"done\""),
    )
    .unwrap();
    let loaded = store.load();
    assert!(!loaded.resolve(&other).unwrap().state_verified);
    assert!(loaded.resolve(&id).unwrap().state_verified);
    // A broken event line is a problem, not a failure.
    let events = store.root().join("items").join(&id).join("events.jsonl");
    let mut text = std::fs::read_to_string(&events).unwrap();
    text.push_str("not json\n");
    std::fs::write(&events, text).unwrap();
    let loaded = store.load();
    assert!(
        loaded
            .problems
            .iter()
            .any(|p| p.contains("events.jsonl line")),
        "{:?}",
        loaded.problems
    );
    assert!(loaded.resolve(&id).unwrap().state_verified);
    let _ = std::fs::remove_dir_all(dir);
}

/// AK-3: bounds, references and links are checked.
#[test]
fn bounds_references_and_links_are_checked() {
    let dir = temp("bounds");
    let store = Store::new(dir.join("s"), StoreKind::Private);
    let (_, a) = create(&store, NewItem::new(Kind::Task, "A"), &person(), NOW);
    let mut b_new = NewItem::new(Kind::Task, "B");
    b_new.depends_on = [a.clone()].into();
    let (loaded, b) = create(&store, b_new, &person(), NOW + 1);
    let refuse = |new: NewItem, says: &str| {
        let err = store
            .plan_create(&loaded, new, &person(), NOW + 2)
            .unwrap_err();
        assert!(err.contains(says), "{says}: {err}");
    };
    refuse(NewItem::new(Kind::Task, ""), "`title`");
    refuse(NewItem::new(Kind::Task, "x".repeat(201)), "`title`");
    refuse(NewItem::new(Kind::Task, "two\nlines"), "one line");
    let mut n = NewItem::new(Kind::Task, "t");
    n.labels = ["has space".to_owned()].into();
    refuse(n, "label");
    let mut n = NewItem::new(Kind::Task, "t");
    n.labels = (0..21).map(|i| format!("l{i}")).collect::<BTreeSet<_>>();
    refuse(n, "20 labels");
    let mut n = NewItem::new(Kind::Task, "t");
    n.description = "x".repeat(64 * 1024 + 1);
    refuse(n, "`description`");
    let ghost = id_from(1, [9; 10]);
    let mut n = NewItem::new(Kind::Task, "t");
    n.depends_on = [ghost.clone()].into();
    refuse(n, "names no item");
    let mut n = NewItem::new(Kind::Task, "t");
    n.parent = Some(ghost);
    refuse(n, "names no item");
    for (kind, target, says) in [
        (LinkKind::Commit, "xyz", "hexadecimal"),
        (LinkKind::Attachment, "../secret", "inside the workspace"),
        (LinkKind::Attachment, "/etc/passwd", "inside the workspace"),
        (
            LinkKind::Attachment,
            "http://example.com/a",
            "inside the workspace",
        ),
        (LinkKind::Ticket, "IC-123456", "by its id"),
    ] {
        let mut n = NewItem::new(Kind::Task, "t");
        n.links = [Link {
            kind,
            target: target.into(),
        }]
        .into();
        refuse(n, says);
    }
    let mut n = NewItem::new(Kind::Task, "t");
    n.links = [Link {
        kind: LinkKind::Attachment,
        target: "https://example.com/screenshot.png".into(),
    }]
    .into();
    assert!(store.plan_create(&loaded, n, &person(), NOW + 2).is_ok());
    // A cycle through dependencies and through parents.
    let err = update(
        &store,
        &a,
        ItemChange {
            add_depends_on: vec![b.clone()],
            ..Default::default()
        },
        &person(),
    )
    .unwrap_err();
    assert!(err.contains("cycle"), "{err}");
    update(
        &store,
        &b,
        ItemChange {
            parent: Some(Some(a.clone())),
            ..Default::default()
        },
        &person(),
    )
    .unwrap();
    let err = update(
        &store,
        &a,
        ItemChange {
            parent: Some(Some(b.clone())),
            ..Default::default()
        },
        &person(),
    )
    .unwrap_err();
    assert!(err.contains("cycle"), "{err}");
    let err = update(
        &store,
        &a,
        ItemChange {
            add_depends_on: vec![a.clone()],
            ..Default::default()
        },
        &person(),
    )
    .unwrap_err();
    assert!(err.contains("itself"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

/// AK-4: ids, keys, their lengthening and their resolution.
#[test]
fn keys_are_short_unique_and_resolve() {
    let id = id_from(NOW * 1000, [0xff; 10]);
    assert!(is_id(&id), "{id}");
    assert_eq!(&id[14..15], "7");
    assert!(!is_id("not-an-id"));
    assert!(!is_id(&id.to_uppercase()));
    assert_eq!(key_chars(&id).len(), 15);

    let dir = temp("keys");
    let store = Store::new(dir.join("s"), StoreKind::Private);
    // Two ids sharing their first eight Base32 characters, one different.
    let a = id_from(NOW * 1000, [0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let b = id_from(NOW * 1000, [0, 0, 0, 0, 0, 0, 0, 1, 0, 0]);
    let c = id_from(NOW * 1000, [0x0f, 0xff, 0x3f, 0xff, 0, 0, 0, 0, 0, 0]);
    assert_eq!(key_chars(&a)[..8], key_chars(&b)[..8]);
    for (i, id) in [&a, &b, &c].into_iter().enumerate() {
        let loaded = store.load();
        let plan = store
            .plan_create_with_id(
                &loaded,
                NewItem::new(Kind::Task, format!("item {i}")),
                &person(),
                NOW + i as u64,
                id.clone(),
            )
            .unwrap();
        store.apply(&plan, false).unwrap();
    }
    let loaded = store.load();
    let key = |id: &str| loaded.resolve(id).unwrap().key.clone();
    assert_eq!(key(&c), format!("T-{}", &key_chars(&c)[..6]));
    assert!(key(&a).len() > 8, "{}", key(&a));
    assert_ne!(key(&a), key(&b));
    // By key, without prefix, in lower case, with a longer start.
    let kc = key(&c);
    assert_eq!(loaded.resolve(&kc).unwrap().item.id, c);
    assert_eq!(loaded.resolve(&kc[2..]).unwrap().item.id, c);
    assert_eq!(loaded.resolve(&kc.to_lowercase()).unwrap().item.id, c);
    assert_eq!(loaded.resolve(&key_chars(&c)[..10]).unwrap().item.id, c);
    let err = loaded.resolve(&key_chars(&a)[..6]).unwrap_err();
    assert!(err.contains("names 2 items"), "{err}");
    assert!(loaded.resolve("T-ZZ").is_err());
    // The prefix comes from the store.
    std::fs::write(
        store.root().join("store.toml"),
        "[ferris-idea]\nkind = \"work-store\"\nformat = 1\n\n[store]\nprefix = \"ic\"\n",
    )
    .unwrap();
    let loaded = store.load();
    assert!(loaded.resolve(&c).unwrap().key.starts_with("IC-"));
    // A folder that is not an id is a problem.
    std::fs::create_dir_all(store.root().join("items/not-an-id")).unwrap();
    assert!(
        store
            .load()
            .problems
            .iter()
            .any(|p| p.contains("not-an-id"))
    );
    let _ = std::fs::remove_dir_all(dir);
}

fn git_merge_file(ours: &Path, base: &Path, theirs: &Path) -> Option<(bool, String)> {
    let out = std::process::Command::new("git")
        .args(["merge-file", "-p"])
        .args([ours, base, theirs])
        .output()
        .ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// AK-5: two branches change neighbouring fields and comment; Git merges `item.toml` line by
/// line without a conflict and the event logs by union, and the result is verified.
#[test]
fn two_branches_merge_without_conflict() {
    let dir = temp("merge");
    let base_store = Store::new(dir.join("base"), StoreKind::Repository);
    let (_, id) = create(
        &base_store,
        NewItem::new(Kind::Task, "Shared"),
        &person(),
        NOW,
    );
    let copy = |name: &str| {
        let to = dir.join(name);
        let status = std::process::Command::new("cp")
            .args(["-R"])
            .arg(dir.join("base"))
            .arg(&to)
            .status();
        assert!(status.is_ok_and(|s| s.success()));
        Store::new(to, StoreKind::Repository)
    };
    let ours = copy("ours");
    let theirs = copy("theirs");
    // Ours: the state (and a comment); theirs: the title and the priority, the neighbours of
    // the state line.
    update(
        &ours,
        &id,
        ItemChange {
            state: Some(State::Ready),
            ..Default::default()
        },
        &person(),
    )
    .unwrap();
    let loaded = ours.load();
    let plan = ours
        .plan_comment(&loaded, &id, "from ours", &person(), NOW + 70)
        .unwrap();
    ours.apply(&plan, false).unwrap();
    update(
        &theirs,
        &id,
        ItemChange {
            title: Some("Shared, renamed".into()),
            priority: Some(Priority::High),
            ..Default::default()
        },
        &Actor::person("ana"),
    )
    .unwrap();
    let file = |s: &Store, f: &str| s.root().join("items").join(&id).join(f);
    let Some((clean, merged)) = git_merge_file(
        &file(&ours, "item.toml"),
        &file(&base_store, "item.toml"),
        &file(&theirs, "item.toml"),
    ) else {
        eprintln!("git is not available; the merge is not checked");
        return;
    };
    assert!(clean, "conflict:\n{merged}");
    std::fs::write(file(&ours, "item.toml"), &merged).unwrap();
    // `merge=union` keeps both sides' lines: ours, then theirs' new ones.
    let base_events = std::fs::read_to_string(file(&base_store, "events.jsonl")).unwrap();
    let theirs_events = std::fs::read_to_string(file(&theirs, "events.jsonl")).unwrap();
    let mut ours_events = std::fs::read_to_string(file(&ours, "events.jsonl")).unwrap();
    ours_events.push_str(&theirs_events[base_events.len()..]);
    std::fs::write(file(&ours, "events.jsonl"), ours_events).unwrap();
    let loaded = ours.load();
    let t = loaded.resolve(&id).unwrap();
    assert_eq!(t.item.state, State::Ready);
    assert_eq!(t.item.title, "Shared, renamed");
    assert_eq!(t.item.priority, Priority::High);
    assert_eq!(t.comments.len(), 1);
    assert!(t.state_verified);
    assert_eq!(
        std::fs::read_to_string(base_store.root().join(".gitattributes")).unwrap(),
        STORE_GITATTRIBUTES
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// AK-6: what an agent may do alone, what needs a person's confirmation, and its hourly limit.
#[test]
fn agents_need_a_person_to_close_cancel_and_reprioritise() {
    let dir = temp("agents");
    let store = Store::new(dir.join("s"), StoreKind::Private);
    let loaded = store.load();
    // A finding at normal priority, assigned to itself: no confirmation.
    let mut n = NewItem::new(Kind::Finding, "Unused import in main.rs");
    n.assignee = Some(agent());
    let plan = store.plan_create(&loaded, n, &agent(), NOW).unwrap();
    assert!(
        plan.approval_needed.is_empty(),
        "{:?}",
        plan.approval_needed
    );
    store.apply(&plan, false).unwrap();
    let id = plan.item.id.clone();
    // An epic at high priority for someone else: three reasons, refused without confirmation.
    let mut n = NewItem::new(Kind::Epic, "Rewrite everything");
    n.priority = Priority::High;
    n.assignee = Some(person());
    let plan = store.plan_create(&store.load(), n, &agent(), NOW).unwrap();
    assert_eq!(plan.approval_needed.len(), 3, "{:?}", plan.approval_needed);
    let err = store.apply(&plan, false).unwrap_err();
    assert!(err.contains("needs a person's confirmation"), "{err}");
    assert_eq!(store.load().tickets.len(), 1);

    // Moving along is fine; done and cancelled, priority and another's text are not.
    for s in [State::Ready, State::InProgress, State::InReview] {
        update(
            &store,
            &id,
            ItemChange {
                state: Some(s),
                ..Default::default()
            },
            &agent(),
        )
        .unwrap();
    }
    for (change, says) in [
        (
            ItemChange {
                state: Some(State::Done),
                ..Default::default()
            },
            "to `done`",
        ),
        (
            ItemChange {
                state: Some(State::Cancelled),
                ..Default::default()
            },
            "to `cancelled`",
        ),
        (
            ItemChange {
                priority: Some(Priority::Critical),
                ..Default::default()
            },
            "priority",
        ),
        (
            ItemChange {
                assignee: Some(Some(Actor::agent("other"))),
                ..Default::default()
            },
            "assigning agent:other",
        ),
    ] {
        let err = update(&store, &id, change, &agent()).unwrap_err();
        assert!(err.contains(says), "{says}: {err}");
    }
    let (_, theirs) = create(
        &store,
        NewItem::new(Kind::Bug, "A person's bug"),
        &person(),
        NOW,
    );
    let err = update(
        &store,
        &theirs,
        ItemChange {
            title: Some("Rewritten".into()),
            ..Default::default()
        },
        &agent(),
    )
    .unwrap_err();
    assert!(err.contains("an item person:ada created"), "{err}");
    // With the person's confirmation: written and recorded.
    let loaded = store.load();
    let plan = store
        .plan_update(
            &loaded,
            &id,
            ItemChange {
                state: Some(State::Done),
                ..Default::default()
            },
            &agent(),
            NOW + 100,
        )
        .unwrap();
    store.apply(&plan, true).unwrap();
    let loaded = store.load();
    let t = loaded.resolve(&id).unwrap();
    assert_eq!(t.item.state, State::Done);
    assert_eq!(
        t.events.last().unwrap().approved_by.as_deref(),
        Some("person:window")
    );
    assert!(t.state_verified);
    // A person needs no confirmation.
    let plan = store
        .plan_update(
            &loaded,
            &theirs,
            ItemChange {
                priority: Some(Priority::Critical),
                state: Some(State::Cancelled),
                ..Default::default()
            },
            &person(),
            NOW,
        )
        .unwrap();
    assert!(plan.approval_needed.is_empty());

    // The hourly limit, counted from the store.
    let limit_store = Store::new(dir.join("limit"), StoreKind::Private);
    for i in 0..50 {
        let loaded = limit_store.load();
        let plan = limit_store
            .plan_create(
                &loaded,
                NewItem::new(Kind::Task, format!("t{i}")),
                &agent(),
                NOW + i,
            )
            .unwrap();
        limit_store.apply(&plan, false).unwrap();
    }
    let loaded = limit_store.load();
    let err = limit_store
        .plan_create(
            &loaded,
            NewItem::new(Kind::Task, "one more"),
            &agent(),
            NOW + 60,
        )
        .unwrap_err();
    assert!(err.contains("the limit is 50"), "{err}");
    assert!(err.contains(&rfc3339(NOW + 3600)), "{err}");
    assert!(
        limit_store
            .plan_create(
                &loaded,
                NewItem::new(Kind::Task, "later"),
                &agent(),
                NOW + 3600
            )
            .is_ok()
    );
    assert!(
        limit_store
            .plan_create(
                &loaded,
                NewItem::new(Kind::Task, "person"),
                &person(),
                NOW + 60
            )
            .is_ok()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// AK-7: full-text search with every term, ranking, filters.
#[test]
fn search_finds_items_with_every_term() {
    let dir = temp("search");
    let store = Store::new(dir.join("s"), StoreKind::Private);
    let mut n = NewItem::new(Kind::Bug, "Crash when saving");
    n.description = "The editor crashes on save of a large file.".into();
    n.labels = ["editor".to_owned()].into();
    let (_, crash) = create(&store, n, &person(), NOW);
    let mut n = NewItem::new(Kind::Task, "Document saving");
    n.description = "Explain autosave.".into();
    let (_, docs) = create(&store, n, &person(), NOW + 1);
    let (loaded, other) = create(
        &store,
        NewItem::new(Kind::Question, "Which font?"),
        &person(),
        NOW + 2,
    );
    let plan = store
        .plan_comment(
            &loaded,
            &other,
            "Maybe JetBrains Mono, see the editor settings",
            &agent(),
            NOW + 3,
        )
        .unwrap();
    store.apply(&plan, false).unwrap();
    let loaded = store.load();
    let all = Filter::default();
    let ids = |q: &str, f: &Filter| -> Vec<String> {
        search(&loaded.tickets, q, f)
            .iter()
            .map(|h| h.ticket.item.id.clone())
            .collect()
    };
    // The title counts three times.
    assert_eq!(ids("SAVING", &all), [docs.clone(), crash.clone()]);
    assert_eq!(ids("saving crash", &all), [crash.as_str()]);
    assert_eq!(ids("editor", &all), [crash.clone(), other.clone()]);
    assert_eq!(ids("mono", &all), [other.as_str()]);
    assert!(ids("saving nothing", &all).is_empty());
    let bugs = Filter {
        kinds: vec![Kind::Bug],
        ..Default::default()
    };
    assert_eq!(ids("editor", &bugs), [crash.as_str()]);
    let labelled = Filter {
        label: Some("editor".into()),
        ..Default::default()
    };
    assert_eq!(ids("", &labelled), [crash.as_str()]);
    let hits = search(&loaded.tickets, "large", &all);
    assert!(
        hits[0].snippet.contains("large file"),
        "{}",
        hits[0].snippet
    );
    let closed = Filter {
        open: Some(false),
        ..Default::default()
    };
    assert!(ids("", &closed).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// AK-8: the store choice beside the grant file, private by default.
#[test]
fn the_store_is_private_unless_the_user_chose_the_repository() {
    let dir = temp("choice");
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let grant = dir.join("x.agent-permissions.toml");
    assert_eq!(choice_file(&grant), dir.join("x.tickets.toml"));
    let (store, problem) = workspace_store(&grant, &root);
    assert!(problem.is_none());
    assert_eq!(store.kind(), StoreKind::Private);
    assert_eq!(store.root(), private_store_dir(&grant, &root));
    assert!(store.root().starts_with(dir.join("x.tickets")));
    assert!(!store.root().starts_with(&root));
    // A repository store found in the project is not used without the choice.
    std::fs::create_dir_all(root.join(".ferris-idea/work/items")).unwrap();
    assert_eq!(workspace_store(&grant, &root).0.kind(), StoreKind::Private);
    save_store_choice(&choice_file(&grant), &root, StoreKind::Repository).unwrap();
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    save_store_choice(&choice_file(&grant), &other, StoreKind::Private).unwrap();
    let (store, _) = workspace_store(&grant, &root);
    assert_eq!(store.kind(), StoreKind::Repository);
    assert_eq!(store.root(), root.join(".ferris-idea/work"));
    let (_, id) = create(&store, NewItem::new(Kind::Task, "Shared"), &person(), NOW);
    assert!(
        root.join(".ferris-idea/work/items")
            .join(id)
            .join("item.toml")
            .is_file()
    );
    assert!(root.join(".ferris-idea/work/.gitattributes").is_file());
    assert_eq!(workspace_store(&grant, &other).0.kind(), StoreKind::Private);
    // A choice file inside the workspace is ignored and never written.
    let inside = root.join("x.tickets.toml");
    std::fs::write(
        &inside,
        format!(
            "[[workspace]]\npath = {:?}\nstore = \"repository\"\n",
            root.display().to_string()
        ),
    )
    .unwrap();
    let (kind, problem) = load_store_choice(&inside, &root);
    assert_eq!(kind, StoreKind::Private);
    assert!(problem.unwrap().contains("inside the workspace"));
    assert!(save_store_choice(&inside, &root, StoreKind::Repository).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

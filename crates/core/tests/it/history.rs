//! SPEC-0044 AK-1 – AK-4 and AK-6 (v2).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_core::{
    COMPACT_AT, FileChange, LocalHistory, MAX_RECORDED_BYTES, MAX_REVISIONS_PER_FILE, RevisionKind,
};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let p =
            std::env::temp_dir().join(format!("ferris-idea-history-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, path: &str, text: &str) {
        let p = self.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(path)).ok()
    }
    fn root(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const DAY: u64 = 24 * 60 * 60 * 1000;

#[test]
fn recording_dedup_and_large_files() {
    let d = Dir::new("record");
    let mut h = LocalHistory::open(d.root()).unwrap();
    assert!(!h.record_at("a.txt", 1).unwrap(), "unknown and absent");
    d.write("a.txt", "one");
    assert!(h.record_at("a.txt", 1).unwrap());
    assert!(!h.record_at("a.txt", 2).unwrap(), "unchanged");
    d.write("a.txt", "two");
    assert!(h.record_at("a.txt", 3).unwrap());
    d.write("b.txt", "one");
    assert!(h.record_at("b.txt", 4).unwrap());
    assert_eq!(h.object_count(), 2, "identical contents share one object");
    std::fs::remove_file(d.root().join("a.txt")).unwrap();
    assert!(h.record_at("a.txt", 5).unwrap());
    let revs = h.revisions("a.txt");
    assert_eq!(revs.len(), 3);
    assert!(revs[0].deleted);
    assert_eq!(h.content("a.txt", revs[1].id).unwrap().unwrap(), b"two");
    assert_eq!(h.content("a.txt", revs[0].id).unwrap(), None);

    d.write("big.bin", &"x".repeat(MAX_RECORDED_BYTES as usize + 1));
    assert!(!h.record_at("big.bin", 6).unwrap());

    // Reopening reads the same history.
    let again = LocalHistory::open(d.root()).unwrap();
    assert_eq!(again.revisions("a.txt"), revs);
}

#[test]
fn labels_and_listing() {
    let d = Dir::new("labels");
    let mut h = LocalHistory::open(d.root()).unwrap();
    d.write("src/x.rs", "fn a() {}");
    h.record_at("src/x.rs", 10).unwrap();
    h.put_label_at(None, "before refactoring", 20).unwrap();
    d.write("src/x.rs", "fn b() {}");
    h.put_label_at(Some("src/x.rs"), "renamed", 30).unwrap();
    let revs = h.revisions("src/x.rs");
    let view: Vec<_> = revs
        .iter()
        .map(|r| (r.time, r.kind, r.label.as_deref(), r.workspace_label))
        .collect();
    assert_eq!(
        view,
        [
            (30, RevisionKind::Label, Some("renamed"), false),
            (30, RevisionKind::Change, None, false),
            (20, RevisionKind::Label, Some("before refactoring"), true),
            (10, RevisionKind::Change, None, false),
        ]
    );
    // A label is not a content revision.
    assert!(h.content("src/x.rs", revs[0].id).is_err());
    // Files recorded only after a workspace label do not show it.
    d.write("late.txt", "x");
    h.record_at("late.txt", 40).unwrap();
    assert_eq!(h.revisions("late.txt").len(), 1);
}

#[test]
fn restore_is_recorded_and_undoable() {
    let d = Dir::new("restore");
    let mut h = LocalHistory::open(d.root()).unwrap();
    d.write("f.txt", "v1");
    h.record_at("f.txt", 1).unwrap();
    d.write("f.txt", "v2 unsaved work");
    let v1 = h.revisions("f.txt")[0].id;
    h.restore_at("f.txt", v1, 2).unwrap();
    assert_eq!(d.read("f.txt").as_deref(), Some("v1"));
    let revs = h.revisions("f.txt");
    assert_eq!(revs[0].kind, RevisionKind::Restore);
    let before_restore = revs[1].id;
    assert_eq!(
        h.content("f.txt", before_restore).unwrap().unwrap(),
        b"v2 unsaved work"
    );
    // Undo the restore.
    h.restore_at("f.txt", before_restore, 3).unwrap();
    assert_eq!(d.read("f.txt").as_deref(), Some("v2 unsaved work"));

    // Restoring a deletion deletes the file.
    std::fs::remove_file(d.root().join("f.txt")).unwrap();
    h.record_at("f.txt", 4).unwrap();
    let deletion = h.revisions("f.txt")[0].id;
    d.write("f.txt", "back again");
    h.restore_at("f.txt", deletion, 5).unwrap();
    assert_eq!(d.read("f.txt"), None);
    assert!(h.restore_at("f.txt", 999, 6).is_err());
}

#[test]
fn pruning_damage_and_watcher_changes() {
    let d = Dir::new("prune");
    let mut h = LocalHistory::open(d.root()).unwrap();
    d.write("old.txt", "a");
    h.record_at("old.txt", 0).unwrap();
    d.write("old.txt", "b");
    h.record_at("old.txt", DAY).unwrap();
    d.write("new.txt", "c");
    h.record_at("new.txt", 40 * DAY).unwrap();
    assert_eq!(h.object_count(), 3);
    let removed = h
        .prune_at(Duration::from_secs(30 * 24 * 3600), 40 * DAY)
        .unwrap();
    assert_eq!(removed, 1, "only the oldest revision of old.txt goes");
    assert_eq!(h.revisions("old.txt").len(), 1, "the latest one stays");
    assert_eq!(h.object_count(), 2, "its content is removed");

    // A damaged line is skipped.
    let log = d.root().join(".ferris-idea/history/revisions.jsonl");
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.insert_str(0, "{not json\n");
    std::fs::write(&log, text).unwrap();
    let mut h = LocalHistory::open(d.root()).unwrap();
    assert_eq!(h.revisions("new.txt").len(), 1);

    // Watcher changes, including a rename.
    std::fs::rename(d.root().join("new.txt"), d.root().join("moved.txt")).unwrap();
    d.write("added.txt", "z");
    let changes = [
        FileChange::Created(PathBuf::from("added.txt")),
        FileChange::Renamed {
            from: PathBuf::from("new.txt"),
            to: PathBuf::from("moved.txt"),
        },
    ];
    assert_eq!(h.record_changes(&changes).unwrap(), 3);
    assert!(h.revisions("new.txt")[0].deleted);
    assert_eq!(h.revisions("moved.txt").len(), 1);
}

/// Lines of the log that belong to `path`.
fn log_lines(d: &Dir, path: &str) -> usize {
    let log =
        std::fs::read_to_string(d.root().join(".ferris-idea/history/revisions.jsonl")).unwrap();
    let needle = format!("\"path\":\"{path}\"");
    log.lines().filter(|l| l.contains(&needle)).count()
}

#[test]
fn bounded_log_and_stable_ids() {
    let d = Dir::new("bounded");
    let mut h = LocalHistory::open(d.root()).unwrap();
    d.write("other.txt", "kept");
    h.record_at("other.txt", 1).unwrap();
    let other = h.revisions("other.txt");
    let mut early = None;
    let mut late = None;
    for n in 0..3000u64 {
        d.write("hot.txt", &format!("round {n}"));
        assert!(h.record_at("hot.txt", 10 + n).unwrap());
        let newest = h.revisions("hot.txt")[0].id;
        if n == 5 {
            early = Some(newest);
        }
        if n == 2990 {
            late = Some(newest);
        }
        assert!(log_lines(&d, "hot.txt") <= COMPACT_AT, "round {n}");
    }
    let revs = h.revisions("hot.txt");
    assert!(revs.len() >= MAX_REVISIONS_PER_FILE && revs.len() <= COMPACT_AT);
    assert_eq!(
        h.content("hot.txt", revs[0].id).unwrap().unwrap(),
        b"round 2999"
    );
    // Ids survive compactions; removed ids are refused instead of naming another revision.
    assert_eq!(
        h.content("hot.txt", late.unwrap()).unwrap().unwrap(),
        b"round 2990"
    );
    assert!(h.content("hot.txt", early.unwrap()).is_err());
    assert!(revs.windows(2).all(|w| w[0].id > w[1].id));
    // Other files keep their history; unreferenced contents are gone.
    assert_eq!(h.revisions("other.txt"), other);
    assert!(h.object_count() <= COMPACT_AT + 1);
    // Reopening reads the same bounded history with the same ids.
    let again = LocalHistory::open(d.root()).unwrap();
    assert_eq!(again.revisions("hot.txt"), revs);
}

#[test]
fn logs_without_ids_number_their_lines() {
    let d = Dir::new("legacy");
    d.write("a.txt", "x");
    let dir = d.root().join(".ferris-idea/history");
    std::fs::create_dir_all(dir.join("objects")).unwrap();
    std::fs::write(
        dir.join("revisions.jsonl"),
        "{\"path\":\"a.txt\",\"time\":1,\"hash\":null,\"kind\":\"change\"}\n\
         damaged\n\
         {\"path\":\"a.txt\",\"time\":2,\"hash\":null,\"kind\":\"change\"}\n",
    )
    .unwrap();
    let mut h = LocalHistory::open(d.root()).unwrap();
    let ids: Vec<usize> = h.revisions("a.txt").iter().map(|r| r.id).collect();
    assert_eq!(ids, [1, 0]);
    h.record_at("a.txt", 3).unwrap();
    assert_eq!(h.revisions("a.txt")[0].id, 2);
    assert_eq!(h.content("a.txt", 2).unwrap().unwrap(), b"x");
}

#[test]
fn refresh_sees_other_holders() {
    let d = Dir::new("refresh");
    let mut a = LocalHistory::open(d.root()).unwrap();
    let mut b = LocalHistory::open(d.root()).unwrap();
    d.write("f.txt", "one");
    a.record_at("f.txt", 1).unwrap();
    b.refresh().unwrap();
    assert_eq!(b.revisions("f.txt"), a.revisions("f.txt"));
    // Appends of either holder number on from the other's.
    d.write("f.txt", "two");
    b.record_at("f.txt", 2).unwrap();
    d.write("f.txt", "three");
    a.record_at("f.txt", 3).unwrap();
    let ids: Vec<usize> = a.revisions("f.txt").iter().map(|r| r.id).collect();
    assert_eq!(ids, [2, 1, 0]);
    // A compaction by one holder (a rewrite) makes the other read the log again.
    for n in 0..COMPACT_AT as u64 {
        d.write("f.txt", &format!("round {n}"));
        a.record_at("f.txt", 10 + n).unwrap();
    }
    b.refresh().unwrap();
    assert_eq!(b.revisions("f.txt"), a.revisions("f.txt"));
    assert!(b.revisions("f.txt").len() <= COMPACT_AT);
}

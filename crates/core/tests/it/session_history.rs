//! SPEC-0344: the session history of agents in the core: redaction, the file format, private
//! files, limits and reading back.

use std::path::PathBuf;

use ferris_idea_core::session_history::{
    Event, Outcome, REDACTED, Retention, SessionWriter, ToolCall, history_dir, list_sessions,
    no_mask, prune, read_session, redact_arguments, valid_id, workspace_dir,
};
use serde_json::{Map, Value, json};

/// A fresh folder for one test: the workspace and its grant file beside it.
fn base(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "ferris-idea-session-history-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let dir = history_dir(&base.join("x.agent-permissions.toml"));
    (base, root, dir)
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn call(tool: &str, a: Value, outcome: Outcome, ms: u64) -> Event {
    Event::ToolCall(ToolCall::new(tool, Some(&args(a)), &no_mask).finished(
        12,
        ms,
        outcome,
        (outcome != Outcome::Ok).then_some("failed: token=abc123secret"),
        &no_mask,
    ))
}

/// SPEC-0344/AK-1 (behaviour 9).
#[test]
fn secrets_are_redacted_before_writing() {
    let input = json!({
        "path": "src/main.rs",
        "password": "hunter2",
        "apiKey": "k-123",
        "API-KEY": 42,
        "client_secret": ["a", "b"],
        "save_password": true,
        "token": null,
        "author": "Ann",
        "max_tokens_hint": "plain",
        "url": "postgres://ann:s3cr3tpw@db.local:5432/app",
        "header": "Authorization: Bearer abcdefghijklmnop",
        "text": "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123 and sk-ant-abcdefghijklmnopqrstuvwx and AKIAABCDEFGHIJKLMNOP",
        "env": "export API_TOKEN=xyz789 DEBUG=1",
        // Made up and assembled at run time, so secret scanners do not take the test value for a
        // real token.
        "jwt": (["eyJhbGciOiJIUzI1NiJ9", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", "c2lnbmF0dXJlc2ln"].join(".")),
        "key": "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----",
        "nested": {"deeper": [{"Password": "p4ss"}]},
        "long": "x".repeat(5000),
    });
    let (out, n) = redact_arguments(&args(input), &no_mask);
    let text = out.to_string();
    for secret in [
        "hunter2",
        "k-123",
        "42",
        "s3cr3tpw",
        "abcdefghijklmnop",
        "ghp_ABC",
        "sk-ant",
        "AKIAABCD",
        "xyz789",
        "eyJhbGci",
        "MIIEow",
        "p4ss",
    ] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    assert_eq!(out["password"], REDACTED);
    assert_eq!(out["apiKey"], REDACTED);
    assert_eq!(out["API-KEY"], REDACTED);
    assert_eq!(out["client_secret"], REDACTED);
    assert_eq!(out["save_password"], true);
    assert_eq!(out["token"], Value::Null);
    assert_eq!(out["author"], "Ann");
    assert_eq!(out["path"], "src/main.rs");
    assert_eq!(
        out["url"],
        format!("postgres://ann:{REDACTED}@db.local:5432/app")
    );
    assert!(out["env"].as_str().unwrap().contains("DEBUG=1"));
    assert_eq!(out["nested"]["deeper"][0]["Password"], REDACTED);
    let long = out["long"].as_str().unwrap();
    assert!(long.starts_with(&"x".repeat(200)) && long.contains("4800 more characters"));
    assert!(n >= 13, "{n} redacted");

    // Deep nesting is left out; the extra masker runs on every text and counts.
    let mut deep = json!("bottom");
    for _ in 0..40 {
        deep = json!({ "d": deep });
    }
    let (out, _) = redact_arguments(&args(json!({ "deep": deep })), &no_mask);
    assert!(out.to_string().contains("nested too deep"));
    let mask = |t: &str| {
        (
            t.replace("internal", "‹masked›"),
            t.matches("internal").count(),
        )
    };
    let (out, n) = redact_arguments(&args(json!({"a": "an internal name"})), &mask);
    assert_eq!(out["a"], "an ‹masked› name");
    assert_eq!(n, 1);

    // Arguments too large after cutting are replaced by their size.
    let many: Map<String, Value> = (0..200)
        .map(|i| (format!("k{i}"), json!("y".repeat(900))))
        .collect();
    let (out, _) = redact_arguments(&many, &no_mask);
    assert!(
        out.as_str()
            .unwrap()
            .contains("bytes of arguments left out")
    );
}

/// SPEC-0344/AK-2 (behaviours 5 to 8, 13).
#[test]
fn a_session_is_written_and_read_back_in_order() {
    let (base, root, dir) = base("format");
    let mut w =
        SessionWriter::start(&dir, &root, "agent 1.0", 1_000_000, Retention::default()).unwrap();
    assert!(valid_id(w.id()));
    assert!(
        w.record(
            call("read_file", json!({"path": "a"}), Outcome::Ok, 5),
            1_000_010
        )
        .unwrap()
    );
    assert!(
        w.record(
            call(
                "write_file",
                json!({"path": "b", "password": "pw1"}),
                Outcome::Refused,
                1
            ),
            1_000_020
        )
        .unwrap()
    );
    assert!(
        w.record(call("run", json!({}), Outcome::Error, 30), 1_000_050)
            .unwrap()
    );
    for e in [
        Event::Context {
            target: "fn main".into(),
            pieces: 3,
            chars: 900,
        },
        Event::ChangeSet {
            id: "c1".into(),
            files: vec!["a".into()],
            state: "applied".into(),
        },
        Event::CheckGate {
            gate: "format".into(),
            passed: true,
            findings: 0,
        },
        Event::Cost {
            model: "m".into(),
            input_tokens: 10,
            output_tokens: 5,
            cost_micros: 7,
        },
    ] {
        assert!(w.record(e, 1_000_060).unwrap());
    }
    // A line this reader does not know, and a broken one, are counted, never fatal.
    let path = w.path().to_path_buf();
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, r#"{{"seq":8,"time":1000070,"kind":"future_kind"}}"#).unwrap();
        writeln!(f, "not json").unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("pw1") && !text.contains("abc123secret"),
        "{text}"
    );

    let id = w.id().to_owned();
    let page = read_session(&dir, &root, &id, 0, 100, &[]).unwrap();
    assert_eq!(page.session.client, "agent 1.0");
    assert_eq!(page.session.tool_calls, 3);
    assert_eq!(page.session.errors, 1);
    assert_eq!(page.session.refused, 1);
    assert_eq!(page.session.duration_ms, 36);
    assert_eq!(page.session.unreadable, 2);
    assert_eq!(page.session.lines, 8);
    assert!(!page.session.truncated);
    assert_eq!(page.lines.len(), 8);
    assert_eq!(page.next, None);
    let seqs: Vec<u64> = page.lines.iter().map(|l| l.line.seq).collect();
    assert_eq!(seqs, (0..8).collect::<Vec<_>>());
    assert_eq!(page.lines[0].offset_ms, 0);
    assert_eq!(page.lines[3].offset_ms, 50);
    let Event::ToolCall(c) = &page.lines[2].line.event else {
        panic!("not a call")
    };
    assert_eq!(c.tool, "write_file");
    assert_eq!(c.outcome, Outcome::Refused);
    assert_eq!(c.arguments["password"], REDACTED);
    assert_eq!(c.redacted, 1);
    assert_eq!(c.result_bytes, 12);
    assert_eq!(
        c.error.as_deref(),
        Some(&*format!("failed: token={REDACTED}"))
    );
    let json = serde_json::to_value(&page.lines[1]).unwrap();
    for key in [
        "seq",
        "time",
        "kind",
        "offset_ms",
        "tool",
        "arguments",
        "redacted",
        "result_bytes",
        "duration_ms",
        "outcome",
    ] {
        assert!(json.get(key).is_some(), "{key} missing in {json}");
    }

    // Paging and kinds.
    let page = read_session(&dir, &root, &id, 2, 2, &[]).unwrap();
    assert_eq!(
        page.lines.iter().map(|l| l.line.seq).collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(page.next, Some(4));
    let page = read_session(
        &dir,
        &root,
        &id,
        0,
        10,
        &["tool_call".into(), "cost".into()],
    )
    .unwrap();
    assert_eq!(page.lines.len(), 4);
    assert!(
        page.lines
            .iter()
            .all(|l| matches!(l.line.event, Event::ToolCall(_) | Event::Cost { .. }))
    );

    // Listing: newest first, with the summary.
    let w2 = SessionWriter::start(&dir, &root, "other 2", 2_000_000, Retention::default()).unwrap();
    drop(w2);
    let list = list_sessions(&dir, &root, 0, 10);
    assert_eq!(list.total, 2);
    assert_eq!(list.sessions[0].client, "other 2");
    assert_eq!(list.sessions[1].id, id);
    assert_eq!(list.sessions[1].tool_calls, 3);
    assert_eq!(list_sessions(&dir, &root, 1, 10).sessions.len(), 1);
    // Another workspace sees none of them.
    let other = base.join("other");
    std::fs::create_dir_all(&other).unwrap();
    assert_eq!(list_sessions(&dir, &other, 0, 10).total, 0);
    assert!(
        read_session(&dir, &other, &id, 0, 10, &[])
            .unwrap_err()
            .contains("no session")
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0344/AK-3 (behaviours 2 and 3).
#[test]
fn files_are_private_and_never_follow_links() {
    let (base, root, dir) = base("private");
    let w = SessionWriter::start(&dir, &root, "c", 5_000, Retention::default()).unwrap();
    let folder = workspace_dir(&dir, &root);
    assert_eq!(w.path().parent().unwrap(), folder);
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains("project") && name.len() == 24, "{name}");
    assert!(dir.ends_with("x.session-history"));
    assert!(!w.path().starts_with(&root));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&folder), 0o700);
        assert_eq!(mode(w.path()), 0o600);

        // A session folder that is a link is refused, and so is reading through a link.
        let root2 = base.join("project2");
        std::fs::create_dir_all(&root2).unwrap();
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, workspace_dir(&dir, &root2)).unwrap();
        let err = SessionWriter::start(&dir, &root2, "c", 6_000, Retention::default()).unwrap_err();
        assert!(err.to_string().contains("link"), "{err}");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        let target = base.join("target.jsonl");
        std::fs::write(&target, "{}\n").unwrap();
        let fake = "0000000000001-abcdef01";
        std::os::unix::fs::symlink(&target, folder.join(format!("{fake}.jsonl"))).unwrap();
        assert!(
            read_session(&dir, &root, fake, 0, 10, &[])
                .unwrap_err()
                .contains("link")
        );
    }
    // Ids not of the form are refused before any file is touched.
    for bad in [
        "../x",
        "123",
        "0000000000001-ABCDEF01",
        "0000000000001-abcdef0g",
        "",
    ] {
        assert!(!valid_id(bad));
        assert!(
            read_session(&dir, &root, bad, 0, 10, &[])
                .unwrap_err()
                .contains("no session id")
        );
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0344/AK-4 (behaviours 8, 10 and 11).
#[test]
fn limits_prune_and_truncate() {
    let (base, root, dir) = base("limits");
    let day = 24 * 60 * 60 * 1000;
    let now = 100 * day;
    let folder = workspace_dir(&dir, &root);
    std::fs::create_dir_all(&folder).unwrap();
    // 40 days old, then 105 recent ones, and a file that is not a session.
    std::fs::write(
        folder.join(format!("{:013}-00000000.jsonl", now - 40 * day)),
        "",
    )
    .unwrap();
    for i in 0..105u64 {
        std::fs::write(
            folder.join(format!("{:013}-{:08x}.jsonl", now - day + i, i)),
            "",
        )
        .unwrap();
    }
    std::fs::write(folder.join("notes.txt"), "keep").unwrap();
    let r = Retention::default();
    // Room is made for the new session: 100 kept in all with it.
    assert_eq!(prune(&folder, now, &r), 7);
    assert_eq!(list_sessions(&dir, &root, 0, 1000).total, 99);
    assert!(folder.join("notes.txt").exists());
    let w = SessionWriter::start(&dir, &root, "c", now, r).unwrap();
    assert_eq!(list_sessions(&dir, &root, 0, 1000).total, 100);
    // The newest of the old ones stayed.
    assert!(
        folder
            .join(format!("{:013}-{:08x}.jsonl", now - day + 104, 104))
            .exists()
    );
    assert!(
        !folder
            .join(format!("{:013}-{:08x}.jsonl", now - day, 0))
            .exists()
    );
    drop(w);

    // The line limit ends with one `truncated` line.
    let small = Retention {
        max_events: 4,
        ..Retention::default()
    };
    let mut w = SessionWriter::start(&dir, &root, "c", now + 1, small).unwrap();
    assert!(
        w.record(call("a", json!({}), Outcome::Ok, 1), now + 2)
            .unwrap()
    );
    assert!(
        w.record(call("b", json!({}), Outcome::Ok, 1), now + 3)
            .unwrap()
    );
    assert!(
        !w.record(call("c", json!({}), Outcome::Ok, 1), now + 4)
            .unwrap()
    );
    assert!(w.ended());
    assert!(
        !w.record(call("d", json!({}), Outcome::Ok, 1), now + 5)
            .unwrap()
    );
    let page = read_session(&dir, &root, w.id(), 0, 100, &[]).unwrap();
    assert_eq!(page.lines.len(), 4);
    assert!(page.session.truncated);
    assert!(matches!(&page.lines[3].line.event, Event::Truncated { reason } if reason == "events"));

    // The byte limit too.
    let tiny = Retention {
        max_bytes: 600,
        ..Retention::default()
    };
    let mut w = SessionWriter::start(&dir, &root, "c", now + 10, tiny).unwrap();
    let mut written = 0;
    while w
        .record(
            call("t", json!({"p": "y".repeat(50)}), Outcome::Ok, 1),
            now + 11,
        )
        .unwrap()
    {
        written += 1;
    }
    assert!(written >= 1);
    let page = read_session(&dir, &root, w.id(), 0, 100, &[]).unwrap();
    assert!(
        matches!(&page.lines.last().unwrap().line.event, Event::Truncated { reason } if reason == "bytes")
    );
    assert!(std::fs::metadata(w.path()).unwrap().len() < 800);
    let _ = std::fs::remove_dir_all(&base);
}

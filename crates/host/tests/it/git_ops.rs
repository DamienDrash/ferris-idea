//! SPEC-0318 v5 AK-15, AK-16 and AK-18 against the in-process backend: Git over the network with
//! the host's own credentials, rollback, stash, the log's actions, the comments/strings filter and
//! the preview of hits.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use idecraft_host::HostOptions;
use idecraft_session::{
    Backend, BackendConfig, ClientSession, ErrorCode, Right, RpcError, Transport, in_process,
};
use serde_json::{Value, json};

struct Host {
    session: ClientSession,
}

impl Host {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.session
            .call_within(method, params, Duration::from_secs(120))
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idc-gitops-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("p")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn connect(b: &Backend) -> Transport {
    let (window, server) = in_process();
    let b = b.clone();
    std::thread::spawn(move || b.serve(server));
    window
}

fn host(dir: &Path, rights: Vec<Right>) -> Host {
    let backend = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        peer: "test:window".into(),
        rights,
        ..BackendConfig::default()
    });
    idecraft_host::bind(
        &backend,
        HostOptions {
            language_servers: false,
            poll: Duration::from_millis(200),
            shell: Some("/bin/sh".into()),
            ..HostOptions::default()
        },
    );
    let session = ClientSession::open(
        connect(&backend),
        &dir.join("p").display().to_string(),
        "test",
        "testhost",
        None,
    )
    .unwrap();
    Host { session }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A repository in `p` with one commit, and an identity for the commits the host makes.
fn repo(p: &Path) {
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.name", "T"]);
    git(p, &["config", "user.email", "t@example.org"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "one\n").unwrap();
    git(p, &["add", "a.txt"]);
    git(p, &["commit", "-q", "-m", "first"]);
}

fn code(r: Result<Value, RpcError>) -> ErrorCode {
    r.expect_err("the call should be refused").code
}

// SPEC-0318 v5 AK-15.
#[test]
fn network_git_runs_on_the_host_and_reports_missing_credentials() {
    let dir = scratch("net");
    let p = dir.join("p");
    repo(&p);
    let bare = dir.join("origin.git");
    git(&dir, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git(&p, &["remote", "add", "origin", bare.to_str().unwrap()]);
    let h = host(&dir, Right::ALL.to_vec());

    // Push with an upstream, then Git's own view agrees.
    let out = h
        .call("git.outgoing", json!({}))
        .expect("outgoing before the first push");
    assert_eq!(out["commits"].as_array().unwrap().len(), 1, "{out}");
    let pushed = h.call("git.push", json!({ "set_upstream": true })).unwrap();
    assert_eq!(pushed["ok"], true, "{pushed}");
    assert_eq!(
        git(&bare, &["rev-parse", "main"]),
        git(&p, &["rev-parse", "HEAD"])
    );

    // Another clone moves the branch: fetch sees it, pull brings it.
    git(&dir, &["clone", "-q", bare.to_str().unwrap(), "other"]);
    let other = dir.join("other");
    git(&other, &["config", "user.name", "T"]);
    git(&other, &["config", "user.email", "t@example.org"]);
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    git(&other, &["add", "b.txt"]);
    git(&other, &["commit", "-q", "-m", "from other"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let fetched = h.call("git.fetch", json!({ "remote": "origin" })).unwrap();
    assert_eq!(fetched["ok"], true, "{fetched}");
    assert!(!p.join("b.txt").exists());
    let pulled = h.call("git.pull", json!({})).unwrap();
    assert_eq!(pulled["fetch"]["ok"], true, "{pulled}");
    assert!(p.join("b.txt").exists(), "{pulled}");

    // A push the remote has moved past is reported, not forced.
    git(&other, &["pull", "-q", "origin", "main"]);
    std::fs::write(other.join("c.txt"), "c\n").unwrap();
    git(&other, &["add", "c.txt"]);
    git(&other, &["commit", "-q", "-m", "second from other"]);
    git(&other, &["push", "-q", "origin", "main"]);
    std::fs::write(p.join("d.txt"), "d\n").unwrap();
    git(&p, &["add", "d.txt"]);
    git(&p, &["commit", "-q", "-m", "diverged"]);
    let rejected = h.call("git.push", json!({})).unwrap();
    assert_eq!(rejected["ok"], false, "{rejected}");

    // A remote is a configured one; never a URL or an option.
    assert_eq!(
        code(h.call("git.fetch", json!({ "remote": "ext::sh -c id" }))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(h.call("git.push", json!({ "target": "--force" }))),
        ErrorCode::Invalid
    );

    // A remote that needs a credential the host lacks: reported, never asked for, and the
    // password in the URL does not come back.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in server.incoming().take(8) {
            let Ok(mut s) = stream else { break };
            let mut seen = [0u8; 2048];
            let _ = s.read(&mut seen);
            let _ = s.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    git(
        &p,
        &[
            "remote",
            "add",
            "locked",
            &format!("http://me:s3cret-value@127.0.0.1:{port}/r.git"),
        ],
    );
    let refused = h.call("git.fetch", json!({ "remote": "locked" })).unwrap();
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["auth_required"], true, "{refused}");
    assert!(
        !refused.to_string().contains("s3cret-value"),
        "the password left the host: {refused}"
    );

    // Rights (SEC-4): reading only is not enough, and a push needs its own right.
    let dir2 = scratch("net-rights");
    let p2 = dir2.join("p");
    repo(&p2);
    let view = host(&dir2, vec![Right::View]);
    assert_eq!(
        code(view.call("git.fetch", json!({}))),
        ErrorCode::NotGranted
    );
    assert_eq!(
        code(view.call("git.pull", json!({}))),
        ErrorCode::NotGranted
    );
    let write = host(&dir2, vec![Right::View, Right::VcsWrite]);
    let e = write.call("git.push", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted, "{e:?}");
    assert!(e.message.contains("push"), "{e:?}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

// SPEC-0318 v5 AK-16.
#[test]
fn rollback_stash_and_log_actions_run_on_the_host() {
    let dir = scratch("ops");
    let p = dir.join("p");
    repo(&p);
    std::fs::write(p.join("a.txt"), "changed\n").unwrap();
    std::fs::write(p.join("added.txt"), "new\n").unwrap();
    git(&p, &["add", "added.txt"]);
    let h = host(&dir, Right::ALL.to_vec());

    // Rollback.
    assert_eq!(
        code(h.call("git.rollback", json!({ "paths": ["../outside"] }))),
        ErrorCode::Invalid
    );
    let plan = h
        .call(
            "git.rollback",
            json!({ "paths": ["a.txt"], "dry_run": true }),
        )
        .unwrap();
    assert_eq!(plan["restored"], json!(["a.txt"]), "{plan}");
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "changed\n"
    );
    let done = h
        .call(
            "git.rollback",
            json!({ "paths": ["a.txt", "added.txt"], "delete_added": true }),
        )
        .unwrap();
    assert_eq!(done["done"], true, "{done}");
    assert_eq!(done["deleted"], json!(["added.txt"]), "{done}");
    assert_eq!(std::fs::read_to_string(p.join("a.txt")).unwrap(), "one\n");
    assert!(!p.join("added.txt").exists());

    // Stash.
    std::fs::write(p.join("a.txt"), "stash me\n").unwrap();
    let pushed = h
        .call("git.stash_push", json!({ "message": "wip on host" }))
        .unwrap();
    assert_eq!(pushed["ok"], true, "{pushed}");
    assert_eq!(std::fs::read_to_string(p.join("a.txt")).unwrap(), "one\n");
    let list = h.call("git.stashes", json!({})).unwrap();
    assert!(
        list["stashes"][0]["message"]
            .as_str()
            .unwrap()
            .contains("wip on host")
    );
    let files = h.call("git.stash_files", json!({ "index": 0 })).unwrap();
    assert_eq!(files["files"][0]["path"], "a.txt", "{files}");
    assert_eq!(files["files"][0]["new"], "stash me\n", "{files}");
    h.call("git.stash_apply", json!({ "index": 0, "pop": false }))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(p.join("a.txt")).unwrap(),
        "stash me\n"
    );
    h.call("git.stash_drop", json!({ "index": 0 })).unwrap();
    assert_eq!(
        h.call("git.stashes", json!({})).unwrap()["stashes"],
        json!([])
    );
    git(&p, &["checkout", "-q", "--", "a.txt"]);

    // The log's actions.
    let action = |a: &str, id: &str, extra: Value| {
        let mut v = json!({ "action": a, "id": id });
        if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
            m.extend(e);
        }
        h.call("git.commit_action", v)
    };
    git(&p, &["checkout", "-q", "-b", "side"]);
    std::fs::write(p.join("s.txt"), "side\n").unwrap();
    git(&p, &["add", "s.txt"]);
    git(&p, &["commit", "-q", "-m", "on side"]);
    let side = git(&p, &["rev-parse", "HEAD"]);
    git(&p, &["checkout", "-q", "main"]);
    let picked = action("cherry-pick", &side, json!({})).unwrap();
    assert_eq!(picked["ok"], true, "{picked}");
    assert!(p.join("s.txt").exists());
    let head = git(&p, &["rev-parse", "HEAD"]);
    let reverted = action("revert", &head, json!({})).unwrap();
    assert_eq!(reverted["ok"], true, "{reverted}");
    assert!(!p.join("s.txt").exists());
    action("new-tag", &head, json!({ "name": "v1" })).unwrap();
    assert_eq!(git(&p, &["rev-parse", "v1"]), head);
    action("new-branch", &head, json!({ "name": "from-log" })).unwrap();
    assert_eq!(git(&p, &["rev-parse", "from-log"]), head);
    std::fs::write(p.join("a.txt"), "kept by the hard reset\n").unwrap();
    let reset = action("reset", &head, json!({ "mode": "hard" })).unwrap();
    assert_eq!(reset["ok"], true, "{reset}");
    assert_eq!(git(&p, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        h.call("git.stashes", json!({})).unwrap()["stashes"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "the hard reset keeps the changes in a stash"
    );

    // Reading a commit.
    let patch = h.call("git.patch", json!({ "id": head })).unwrap();
    assert!(
        patch["patch"].as_str().unwrap().contains("s.txt"),
        "{patch}"
    );
    let diff = h
        .call("git.commit_diff", json!({ "id": head, "path": "s.txt" }))
        .unwrap();
    assert!(diff["rows"].to_string().contains("side"), "{diff}");
    let files = h.call("git.files_at", json!({ "id": head })).unwrap();
    assert!(files["files"].to_string().contains("s.txt"), "{files}");
    let text = h
        .call("git.text_at", json!({ "id": head, "path": "s.txt" }))
        .unwrap();
    assert_eq!(text["text"], "side\n", "{text}");

    // Nothing that reads like an option or leaves the project.
    assert_eq!(
        code(action("cherry-pick", "--abort", json!({}))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(action("not-an-action", &head, json!({}))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(h.call("git.text_at", json!({ "id": head, "path": "../x" }))),
        ErrorCode::Invalid
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC-0318 v5 AK-18.
#[test]
fn search_filters_by_context_and_previews_on_the_host() {
    let dir = scratch("search");
    let p = dir.join("p");
    std::fs::write(
        p.join("a.rs"),
        "// needle in a comment\nfn main() {\n    let needle = \"needle in a string\";\n}\n",
    )
    .unwrap();
    let mut h = host(&dir, Right::ALL.to_vec());
    let lines = |context: &str| -> Vec<u64> {
        let v = h
            .call(
                "search.text",
                json!({ "query": "needle", "context": context }),
            )
            .unwrap();
        v["files"][0]["matches"]
            .as_array()
            .map(|m| m.iter().map(|m| m["line"].as_u64().unwrap()).collect())
            .unwrap_or_default()
    };
    assert_eq!(lines("anywhere"), vec![0, 2, 2]);
    assert_eq!(lines("in_comments"), vec![0]);
    assert_eq!(lines("in_strings"), vec![2]);
    assert_eq!(lines("except_comments"), vec![2, 2]);
    assert_eq!(lines("except_strings"), vec![0, 2]);
    assert_eq!(lines("except_comments_and_strings"), vec![2]);
    assert_eq!(
        code(h.call(
            "search.text",
            json!({ "query": "x", "context": "somewhere" })
        )),
        ErrorCode::Invalid
    );

    // The filter counts before `max`.
    let v = h
        .call(
            "search.text",
            json!({ "query": "needle", "context": "in_comments", "max": 1 }),
        )
        .unwrap();
    assert_eq!(v["matches"], 1, "{v}");
    assert_eq!(v["truncated"], false, "{v}");

    // The preview around a line.
    let v = h
        .call(
            "search.excerpt",
            json!({ "path": "a.rs", "line": 2, "context": 1 }),
        )
        .unwrap();
    assert_eq!(v["first"], 1, "{v}");
    assert_eq!(v["lines"][0], "fn main() {", "{v}");
    assert_eq!(v["lines"].as_array().unwrap().len(), 3, "{v}");
    // A window's unsaved text wins over the file on disk.
    h.session.open_doc("a.rs").unwrap();
    h.session
        .edit(
            "a.rs",
            &[idecraft_session::Op {
                at: 0,
                delete: 0,
                insert: "// unsaved first line\n".into(),
            }],
        )
        .unwrap();
    assert!(h.session.settle(Duration::from_secs(5)));
    let v = h
        .call("search.excerpt", json!({ "path": "a.rs", "line": 0 }))
        .unwrap();
    assert_eq!(v["lines"][0], "// unsaved first line", "{v}");
    assert_eq!(
        code(h.call("search.excerpt", json!({ "path": "../x", "line": 0 }))),
        ErrorCode::Invalid
    );
    std::fs::write(p.join("bin.dat"), [0u8, 1, 2]).unwrap();
    assert_eq!(
        code(h.call("search.excerpt", json!({ "path": "bin.dat", "line": 0 }))),
        ErrorCode::NotFound
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC-0318 v6 AK-21.
#[test]
fn branch_actions_run_on_the_host() {
    let dir = scratch("branches");
    let p = dir.join("p");
    repo(&p);
    git(&p, &["checkout", "-b", "side"]);
    std::fs::write(p.join("s.txt"), "side\n").unwrap();
    git(&p, &["add", "."]);
    git(&p, &["commit", "-m", "side work"]);
    git(&p, &["checkout", "main"]);
    let h = host(&dir, Right::ALL.to_vec());

    // Merge brings the branch in.
    let done = h.call("git.merge", json!({ "source": "side" })).unwrap();
    assert_eq!(done["ok"], true, "{done}");
    assert!(p.join("s.txt").is_file());

    // Delete of a merged branch, rename, a name like an option refused.
    let done = h
        .call("git.delete_branch", json!({ "name": "side" }))
        .unwrap();
    assert_eq!(done["ok"], true, "{done}");
    assert!(!git(&p, &["branch"]).contains("side"));
    git(&p, &["branch", "old"]);
    let done = h
        .call("git.rename_branch", json!({ "old": "old", "new": "new" }))
        .unwrap();
    assert_eq!(done["ok"], true, "{done}");
    assert!(git(&p, &["branch"]).contains("new"));
    assert_eq!(
        code(h.call("git.delete_branch", json!({ "name": "--force" }))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(h.call("git.merge", json!({ "source": "--abort" }))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(h.call("git.rename_branch", json!({ "old": "new" }))),
        ErrorCode::Invalid
    );

    // Rebase onto a branch, and a stop on conflicts is reported, not an error.
    git(&p, &["checkout", "-b", "topic"]);
    std::fs::write(p.join("t.txt"), "topic\n").unwrap();
    git(&p, &["add", "."]);
    git(&p, &["commit", "-m", "topic work"]);
    git(&p, &["checkout", "main"]);
    std::fs::write(p.join("m.txt"), "main\n").unwrap();
    git(&p, &["add", "."]);
    git(&p, &["commit", "-m", "main work"]);
    let done = h
        .call(
            "git.rebase_onto",
            json!({ "onto": "main", "branch": "topic" }),
        )
        .unwrap();
    assert_eq!(done["ok"], true, "{done}");
    assert!(git(&p, &["log", "--format=%s", "-3"]).contains("main work"));
    git(&p, &["checkout", "main"]);
    git(&p, &["checkout", "-b", "clash"]);
    std::fs::write(p.join("m.txt"), "clash\n").unwrap();
    git(&p, &["add", "."]);
    git(&p, &["commit", "-m", "clash"]);
    git(&p, &["checkout", "main"]);
    std::fs::write(p.join("m.txt"), "ours\n").unwrap();
    git(&p, &["add", "."]);
    git(&p, &["commit", "-m", "ours"]);
    let done = h.call("git.merge", json!({ "source": "clash" })).unwrap();
    assert_eq!(done["conflicts"][0], "m.txt", "{done}");
    assert_eq!(done["in_progress"], "merge", "{done}");
    git(&p, &["merge", "--abort"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC-0318 v6 AK-22 (the trust half is in `trust.rs`).
#[test]
fn branch_actions_need_the_right() {
    let dir = scratch("branch-right");
    repo(&dir.join("p"));
    let h = host(&dir, vec![Right::View]);
    for (method, params) in [
        ("git.merge", json!({ "source": "main" })),
        ("git.rebase_onto", json!({ "onto": "main" })),
        ("git.rename_branch", json!({ "old": "a", "new": "c" })),
        ("git.delete_branch", json!({ "name": "a" })),
    ] {
        assert_eq!(
            code(h.call(method, params)),
            ErrorCode::NotGranted,
            "{method}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC-0318 v6 AK-23.
#[test]
fn log_path_filter_runs_on_the_host() {
    let dir = scratch("touching");
    // The repository is the outer folder; the project is its sub-folder `p`.
    let root = dir.clone();
    git(&root, &["init", "-q", "-b", "main"]);
    let p = root.join("p");
    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(p.join("src/a.txt"), "1\n").unwrap();
    std::fs::write(root.join("outside.txt"), "x\n").unwrap();
    git(&root, &["add", "p", "outside.txt"]);
    git(&root, &["commit", "-m", "one"]);
    let first = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::write(root.join("outside.txt"), "y\n").unwrap();
    git(&root, &["add", "outside.txt"]);
    git(&root, &["commit", "-m", "two"]);
    let second = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::remove_file(p.join("src/a.txt")).unwrap();
    git(&root, &["add", "p"]);
    git(&root, &["commit", "-m", "three"]);
    let third = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();

    // Paths are project-relative.
    let h = host(&dir, Right::ALL.to_vec());
    let touching = |paths: Value| {
        h.call(
            "git.commits_touching",
            json!({ "ids": [third, second, first], "paths": paths }),
        )
    };
    // A file, a folder and a deleted file are found; the commit outside the path is not.
    let v = touching(json!(["src/a.txt"])).unwrap();
    assert_eq!(v["ids"], json!([third, first]), "{v}");
    let v = touching(json!(["src"])).unwrap();
    assert_eq!(v["ids"], json!([third, first]), "{v}");
    assert_eq!(
        code(touching(json!(["../outside.txt"]))),
        ErrorCode::Invalid
    );
    assert_eq!(code(touching(json!(["/etc/passwd"]))), ErrorCode::Invalid);
    assert_eq!(
        code(h.call("git.commits_touching", json!({ "paths": ["a"] }))),
        ErrorCode::Invalid
    );
    let _ = std::fs::remove_dir_all(&dir);
}

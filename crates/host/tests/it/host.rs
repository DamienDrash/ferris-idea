//! SPEC-0318 AK-1, AK-3–AK-8 (and the URI part of AK-2) against the in-process backend: the host
//! families end to end through a window half, with the security tests SEC-4, SEC-7 and SEC-14
//! that concern them.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ferris_idea_host::HostOptions;
use ferris_idea_session::forward::LocalForward;
use ferris_idea_session::{
    Backend, BackendConfig, ClientSession, Connection, ErrorCode, Right, Transport, in_process,
};
use serde_json::{Value, json};

struct Host {
    backend: Backend,
    session: ClientSession,
    _dir: PathBuf,
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idc-host-{tag}-{}", std::process::id()));
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

fn options() -> HostOptions {
    HostOptions {
        language_servers: false,
        poll: Duration::from_millis(200),
        shell: Some("/bin/sh".into()),
        ..HostOptions::default()
    }
}

fn host(dir: &Path, rights: Vec<Right>, options: HostOptions) -> Host {
    let backend = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        peer: "test:window".into(),
        rights,
        ..BackendConfig::default()
    });
    ferris_idea_host::bind(&backend, options);
    let session = ClientSession::open(
        connect(&backend),
        &dir.join("p").display().to_string(),
        "test",
        "testhost",
        None,
    )
    .unwrap();
    Host {
        backend,
        session,
        _dir: dir.to_path_buf(),
    }
}

impl Host {
    fn call(&self, method: &str, params: Value) -> Result<Value, ferris_idea_session::RpcError> {
        self.session
            .call_within(method, params, Duration::from_secs(60))
    }

    fn conn(&self) -> Connection {
        self.session.connection().expect("connected")
    }

    /// Waits for a host notification `method` whose parameters satisfy `want`.
    fn event(&mut self, method: &str, want: impl Fn(&Value) -> bool, limit: Duration) -> Value {
        let t0 = Instant::now();
        loop {
            self.session.pump(Duration::from_millis(50));
            if let Some(i) = self
                .session
                .host_events
                .iter()
                .position(|(m, p)| m == method && want(p))
            {
                return self.session.host_events.remove(i).1;
            }
            assert!(
                t0.elapsed() < limit,
                "no {method} within {limit:?}; got {:?}",
                self.session.host_events
            );
        }
    }

    fn reconnect(&mut self) {
        let t = connect(&self.backend);
        self.session.resume(t).unwrap();
    }
}

/// Reads stream `id` until its text contains `needle`; returns everything read.
fn read_until(conn: &Connection, id: u32, needle: &str, limit: Duration) -> String {
    let t0 = Instant::now();
    let mut all = Vec::new();
    while t0.elapsed() < limit {
        let (data, done) = conn.read_stream(id, Duration::from_millis(100));
        all.extend(data);
        if String::from_utf8_lossy(&all).contains(needle) || done.is_some() {
            break;
        }
    }
    String::from_utf8_lossy(&all).into_owned()
}

fn code(r: Result<Value, ferris_idea_session::RpcError>) -> ErrorCode {
    r.expect_err("refused").code
}

#[test]
fn families_are_offered_and_each_right_is_checked() {
    let dir = scratch("rights");
    std::fs::write(dir.join("p/a.txt"), "alpha\n").unwrap();
    let mut h = host(&dir, vec![Right::View, Right::Agents], options());
    for family in ["lsp", "search", "git", "term", "run", "port", "dap"] {
        assert!(
            h.session.info.capabilities.iter().any(|c| c == family),
            "{family} missing in {:?}",
            h.session.info.capabilities
        );
    }
    h.session.open_doc("a.txt").unwrap();
    // SEC-4: every method behind a right this session lacks is refused, naming it.
    for (method, params, right) in [
        ("term.open", json!({}), "run"),
        ("run.start", json!({ "name": "x" }), "run"),
        ("run.trust", json!({}), "run"),
        ("git.commit", json!({ "message": "m" }), "vcs-write"),
        ("git.checkout", json!({ "name": "x" }), "vcs-write"),
        ("port.forward", json!({ "port": 8080 }), "ports"),
        ("port.connect", json!({ "forward": "f1" }), "ports"),
        ("dap.adapters", json!({}), "debug"),
        ("dap.start", json!({ "adapter": "lldb-dap" }), "debug"),
    ] {
        let e = h.call(method, params).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotGranted, "{method}");
        assert!(e.message.contains(right), "{method}: {}", e.message);
    }
    // Applying a rename needs `edit` on top of `view`.
    let e = h
        .call(
            "lsp.rename",
            json!({ "doc": "a.txt", "revision": 1, "line": 0, "character": 0, "new_name": "b", "apply": true }),
        )
        .unwrap_err();
    assert!(
        matches!(e.code, ErrorCode::NotGranted | ErrorCode::Unavailable),
        "{e:?}"
    );
    // View methods work.
    assert!(h.call("search.text", json!({ "query": "alpha" })).is_ok());
    assert!(h.call("term.list", json!({})).is_ok());
    let audit = std::fs::read_to_string(dir.join("state/remote/audit.jsonl")).unwrap();
    assert!(audit.contains("\"method\":\"term.open\""), "{audit}");
    assert!(audit.contains("not_granted"), "{audit}");
    // SEC-14: a URI or path outside the project is refused before anything looks at it.
    for params in [
        json!({ "uri": "file:///etc/passwd" }),
        json!({ "uri": "file://elsewhere/etc/passwd" }),
        json!({ "doc": "../outside.c" }),
        json!({ "doc": "/etc/passwd" }),
    ] {
        assert_eq!(
            code(h.call("lsp.symbols", params.clone())),
            ErrorCode::Invalid,
            "{params}"
        );
        assert_eq!(
            code(h.call("git.base", json!({ "path": params["doc"] }))),
            ErrorCode::Invalid
        );
    }
    let _ = h.session.bye();
}

#[test]
fn search_uses_unsaved_text_and_relative_paths() {
    let dir = scratch("search");
    std::fs::create_dir_all(dir.join("p/src")).unwrap();
    std::fs::write(dir.join("p/src/one.rs"), "fn needle_one() {}\n").unwrap();
    std::fs::write(dir.join("p/src/two.rs"), "fn other() {}\n").unwrap();
    std::fs::write(dir.join("p/notes.md"), "needle in notes\n").unwrap();
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    h.session.open_doc("src/two.rs").unwrap();
    h.session
        .edit(
            "src/two.rs",
            &[ferris_idea_session::Op {
                at: 0,
                delete: 0,
                insert: "// needle unsaved\n".into(),
            }],
        )
        .unwrap();
    assert!(h.session.settle(Duration::from_secs(5)));
    let r = h.call("search.text", json!({ "query": "needle" })).unwrap();
    let docs: Vec<&str> = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["doc"].as_str().unwrap())
        .collect();
    assert_eq!(docs.len(), 3, "{r}");
    assert!(
        docs.contains(&"src/two.rs") && docs.contains(&"notes.md"),
        "{r}"
    );
    let two = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["doc"] == "src/two.rs")
        .unwrap();
    assert_eq!(two["unsaved"], true);
    assert_eq!(two["matches"][0]["line"], 0);
    let masked = h
        .call("search.text", json!({ "query": "needle", "mask": "*.md" }))
        .unwrap();
    assert_eq!(masked["files"].as_array().unwrap().len(), 1);
    let capped = h
        .call("search.text", json!({ "query": "needle", "max": 1 }))
        .unwrap();
    assert_eq!(capped["truncated"], true);
    assert_eq!(capped["matches"], 1);
    assert_eq!(
        code(h.call("search.text", json!({ "query": "(", "regex": true }))),
        ErrorCode::Invalid
    );
    let files = h.call("search.files", json!({ "query": "one" })).unwrap();
    assert_eq!(files["files"][0]["path"], "src/one.rs", "{files}");
    let every = h
        .call(
            "search.everywhere",
            json!({ "query": "needle_one", "categories": ["symbols"] }),
        )
        .unwrap();
    let text = every.to_string();
    assert!(text.contains("src/one.rs"), "{text}");
    assert!(
        !text.contains(&dir.display().to_string()),
        "host path in {text}"
    );
}

// SPEC-0318 v8 behaviour 14, SPEC-0031/AK-12 (v5): the host lists the project's own files by
// default, the rest with `include_non_project`, the window's recent files in any case.
#[test]
fn go_to_file_on_the_host_lists_the_projects_own_files() {
    let dir = scratch("own-files");
    for (file, text) in [
        ("p/.gitignore", "*.log\n"),
        ("p/src/shop.py", "class Shop:\n    pass\n"),
        ("p/src/shop.log", "shop\n"),
        ("p/.venv/lib/shop_venv.py", "class ShopVenv:\n    pass\n"),
        (
            "p/node_modules/shop/shop_module.js",
            "function shopModule() {}\n",
        ),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let h = host(&dir, Right::ALL.to_vec(), options());
    let paths = |v: &Value| -> Vec<String> {
        let mut out: Vec<String> = v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_owned())
            .collect();
        out.sort();
        out
    };
    let own = h.call("search.files", json!({ "query": "shop" })).unwrap();
    assert_eq!(paths(&own), ["src/shop.py"], "{own}");
    assert_eq!(
        own["not_listed"],
        "2 folders and 1 file not listed (libraries and ignored files)"
    );
    let all = h
        .call(
            "search.files",
            json!({ "query": "shop", "include_non_project": true }),
        )
        .unwrap();
    assert_eq!(paths(&all).len(), 4, "{all}");
    assert!(all.get("not_listed").is_none(), "{all}");
    let recent = h
        .call(
            "search.everywhere",
            json!({ "query": "shop", "categories": ["files", "classes"],
                "recent": [".venv/lib/shop_venv.py"] }),
        )
        .unwrap();
    assert_eq!(
        paths(&recent),
        [".venv/lib/shop_venv.py", "src/shop.py"],
        "{recent}"
    );
    assert!(
        recent["classes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "ShopVenv"),
        "classes are not limited: {recent}"
    );
    // A recent path outside the project is refused.
    assert_eq!(
        code(h.call(
            "search.files",
            json!({ "query": "shop", "recent": ["../secret.txt"] })
        )),
        ErrorCode::Invalid
    );
}

// SPEC-0029/AK-11 (v5), SPEC-0318 v8 behaviour 13: Find in Files on the host searches the
// project's own sources; the rest with `include_non_project`, a named folder or named files.
#[test]
fn find_in_files_on_the_host_searches_the_projects_own_files() {
    let dir = scratch("own-text");
    for (file, text) in [
        ("p/.gitignore", "scratch.py\n"),
        ("p/app.py", "needle = 1\n"),
        ("p/scratch.py", "needle = 2\n"),
        ("p/.venv/lib/python3.12/site-packages/x.py", "needle = 3\n"),
        ("p/node_modules/y.js", "let needle = 4;\n"),
        ("p/.git/description", "needle\n"),
        ("outside.py", "needle = 5\n"),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    let docs = |v: &Value| -> Vec<String> {
        v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["doc"].as_str().unwrap().to_owned())
            .collect()
    };
    let line = "2 folders and 1 file not searched (libraries and ignored files)";
    let own = h.call("search.text", json!({ "query": "needle" })).unwrap();
    assert_eq!(docs(&own), ["app.py"], "{own}");
    assert_eq!(own["not_searched"], line, "{own}");
    // The line is there without a match too, and the file mask narrows what it counts.
    let none = h.call("search.text", json!({ "query": "absent" })).unwrap();
    assert_eq!(none["not_searched"], line, "{none}");
    let masked = h
        .call("search.text", json!({ "query": "needle", "mask": "*.js" }))
        .unwrap();
    assert!(docs(&masked).is_empty(), "{masked}");
    assert_eq!(
        masked["not_searched"],
        "2 folders not searched (libraries and ignored files)"
    );
    let all = h
        .call(
            "search.text",
            json!({ "query": "needle", "include_non_project": true }),
        )
        .unwrap();
    assert_eq!(
        docs(&all),
        [
            ".venv/lib/python3.12/site-packages/x.py",
            "app.py",
            "node_modules/y.js",
            "scratch.py"
        ],
        "version control data is searched in no case: {all}"
    );
    assert!(all.get("not_searched").is_none(), "{all}");
    // A named folder wins; an own folder is searched by the rule.
    let venv = h
        .call(
            "search.text",
            json!({ "query": "needle", "folder": ".venv/lib" }),
        )
        .unwrap();
    assert_eq!(
        docs(&venv),
        [".venv/lib/python3.12/site-packages/x.py"],
        "{venv}"
    );
    assert!(venv.get("not_searched").is_none(), "{venv}");
    // Named files (the Open Files scope) are searched whatever they are, and only they.
    let named = h
        .call(
            "search.text",
            json!({ "query": "needle", "files": ["node_modules/y.js", "scratch.py", "gone.py"] }),
        )
        .unwrap();
    assert_eq!(docs(&named), ["node_modules/y.js", "scratch.py"], "{named}");
    assert!(named.get("not_searched").is_none(), "{named}");
    let nothing = h
        .call("search.text", json!({ "query": "needle", "files": [] }))
        .unwrap();
    assert!(docs(&nothing).is_empty(), "{nothing}");
    // Unsaved text of an open document that is not the project's own is found when it is named.
    h.session.open_doc("scratch.py").unwrap();
    h.session
        .edit(
            "scratch.py",
            &[ferris_idea_session::Op {
                at: 0,
                delete: 0,
                insert: "# needle unsaved\n".into(),
            }],
        )
        .unwrap();
    assert!(h.session.settle(Duration::from_secs(5)));
    let unsaved = h
        .call(
            "search.text",
            json!({ "query": "needle", "files": ["scratch.py"] }),
        )
        .unwrap();
    assert_eq!(unsaved["matches"], 2, "{unsaved}");
    assert_eq!(unsaved["files"][0]["unsaved"], true);
    let still_own = h.call("search.text", json!({ "query": "needle" })).unwrap();
    assert_eq!(docs(&still_own), ["app.py"], "{still_own}");
    // Paths from the window stay inside the project (ADR-0025).
    for params in [
        json!({ "query": "needle", "folder": ".." }),
        json!({ "query": "needle", "folder": "/etc" }),
        json!({ "query": "needle", "files": ["../outside.py"] }),
        json!({ "query": "needle", "files": [7] }),
    ] {
        assert_eq!(
            code(h.call("search.text", params.clone())),
            ErrorCode::Invalid,
            "{params}"
        );
    }
    // Nothing behind a symbolic link is read, named or not.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("outside.py"), dir.join("p/link.py")).unwrap();
        std::os::unix::fs::symlink(&dir, dir.join("p/up")).unwrap();
        std::os::unix::fs::symlink(&dir, dir.join("p/node_modules/up")).unwrap();
        for params in [
            json!({ "query": "needle", "files": ["link.py", "up/outside.py"] }),
            json!({ "query": "needle", "files": ["node_modules/up/outside.py"] }),
            json!({ "query": "needle", "folder": "up" }),
            json!({ "query": "needle", "folder": "node_modules/up" }),
        ] {
            let linked = h.call("search.text", params.clone()).unwrap();
            assert!(docs(&linked).is_empty(), "{params}: {linked}");
        }
        let everything = h
            .call(
                "search.text",
                json!({ "query": "needle", "include_non_project": true }),
            )
            .unwrap();
        assert_eq!(docs(&everything).len(), 4, "{everything}");
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.org",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
}

#[test]
fn git_runs_on_the_host_repository() {
    let dir = scratch("git");
    let p = dir.join("p");
    git(&p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("a.txt"), "one\ntwo\n").unwrap();
    git(&p, &["add", "a.txt"]);
    git(&p, &["commit", "-q", "-m", "first"]);
    std::fs::write(p.join("a.txt"), "one\nTWO\n").unwrap();
    std::fs::write(p.join("new.txt"), "n\n").unwrap();
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    let st = h.call("git.status", json!({})).unwrap();
    assert_eq!(st["branch"], "main", "{st}");
    let paths: Vec<&str> = st["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.contains(&"a.txt") && paths.contains(&"new.txt"),
        "{st}"
    );
    let base = h.call("git.base", json!({ "path": "a.txt" })).unwrap();
    assert_eq!(base["text"], "one\ntwo\n");
    let diff = h.call("git.diff", json!({ "path": "a.txt" })).unwrap();
    assert!(diff["rows"].to_string().contains("TWO"), "{diff}");
    let log = h.call("git.log", json!({ "limit": 5 })).unwrap();
    assert_eq!(log["commits"].as_array().unwrap().len(), 1);
    let c = h
        .call(
            "git.commit",
            json!({ "message": "second", "paths": ["a.txt", "new.txt"] }),
        )
        .unwrap();
    assert!(c.to_string().contains("second"), "{c}");
    assert_eq!(
        h.call("git.log", json!({})).unwrap()["commits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    h.call("git.create_branch", json!({ "name": "feature" }))
        .unwrap();
    let b = h.call("git.branches", json!({})).unwrap();
    assert!(b.to_string().contains("feature"), "{b}");
    // A commit by another program on the host is announced (after the watcher looked once).
    std::thread::sleep(Duration::from_millis(600));
    std::fs::write(p.join("b.txt"), "b\n").unwrap();
    git(&p, &["add", "b.txt"]);
    git(&p, &["commit", "-q", "-m", "outside"]);
    h.event("git.changed", |_| true, Duration::from_secs(10));
    assert_eq!(
        code(h.call("git.commit", json!({ "message": " " }))),
        ErrorCode::Invalid
    );
}

#[test]
fn a_host_terminal_echoes_resizes_survives_a_break_and_ends_with_the_session() {
    let dir = scratch("term");
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    assert_eq!(
        code(h.call("term.open", json!({ "env": { "lower": "x" } }))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(h.call("term.open", json!({ "cwd": "../" }))),
        ErrorCode::Invalid
    );
    let t = h
        .call(
            "term.open",
            json!({ "cols": 80, "rows": 24, "env": { "IDC_MARK": "m42" } }),
        )
        .unwrap();
    let (term, stream) = (
        t["term"].as_str().unwrap().to_owned(),
        t["stream"].as_u64().unwrap() as u32,
    );
    let conn = h.conn();
    conn.open_sending(stream);
    conn.send_data(
        stream,
        b"echo hi-$((40+2)) $IDC_MARK\n",
        Duration::from_secs(5),
    )
    .unwrap();
    let out = read_until(&conn, stream, "hi-42 m42", Duration::from_secs(10));
    assert!(out.contains("hi-42 m42"), "{out}");
    h.call(
        "term.resize",
        json!({ "term": term, "cols": 100, "rows": 30 }),
    )
    .unwrap();
    conn.send_data(stream, b"stty size\n", Duration::from_secs(5))
        .unwrap();
    let out = read_until(&conn, stream, "30 100", Duration::from_secs(10));
    assert!(out.contains("30 100"), "{out}");
    // A break: the shell goes on, its output is kept and comes after term.attach.
    conn.send_data(
        stream,
        b"sleep 1; echo after-the-break\n",
        Duration::from_secs(5),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    h.session.drop_connection();
    std::thread::sleep(Duration::from_millis(1500));
    h.reconnect();
    let a = h.call("term.attach", json!({ "term": term })).unwrap();
    let s2 = a["stream"].as_u64().unwrap() as u32;
    let conn = h.conn();
    conn.open_sending(s2);
    let out = read_until(&conn, s2, "after-the-break", Duration::from_secs(10));
    assert!(out.contains("after-the-break"), "{out}");
    let list = h.call("term.list", json!({})).unwrap();
    assert_eq!(list["terminals"][0]["running"], true, "{list}");
    conn.send_data(s2, b"exit 3\n", Duration::from_secs(5))
        .unwrap();
    let ex = h.event(
        "term.exited",
        |p| p["term"] == term.as_str(),
        Duration::from_secs(10),
    );
    assert_eq!(ex["code"], 3, "{ex}");
    // A second terminal ends with the session.
    let t2 = h.call("term.open", json!({})).unwrap();
    let marker = dir.join("p/ended");
    let s3 = t2["stream"].as_u64().unwrap() as u32;
    conn.open_sending(s3);
    // The session ends only once the shell has set its trap: it says so (flake of 2026-11-14,
    // where a fixed 300 ms were too short under load and the shell died before `trap` ran). The
    // word is put together by the shell, so the echo of the typed line does not count.
    conn.send_data(
        s3,
        format!(
            "trap 'touch {}' HUP; echo trap-''set; while true; do sleep 0.1; done\n",
            marker.display()
        )
        .as_bytes(),
        Duration::from_secs(5),
    )
    .unwrap();
    let out = read_until(&conn, s3, "trap-set", Duration::from_secs(30));
    assert!(out.contains("trap-set"), "{out}");
    let _ = h.session.bye();
    let t0 = Instant::now();
    while !marker.exists() && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        marker.exists(),
        "the shell did not get SIGHUP when the session ended"
    );
}

#[test]
fn runs_stream_their_output_report_tests_stop_and_survive_a_break() {
    let dir = scratch("run");
    std::fs::create_dir_all(dir.join("p/.ferris-idea")).unwrap();
    std::fs::write(
        dir.join("p/.ferris-idea/run.toml"),
        r#"[[run]]
name = "hello"
command = ["sh", "-c", "echo to-out; echo to-err >&2"]

[[run]]
name = "tests"
kind = "test"
report = "tap"
command = ["sh", "-c", "echo 1..2; echo ok 1 - first; echo not ok 2 - second"]

[[run]]
name = "slow"
command = ["sh", "-c", "sleep 1; echo late-line; sleep 30"]
"#,
    )
    .unwrap();
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    let configs = h.call("run.configs", json!({})).unwrap();
    let names: Vec<&str> = configs["configs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"hello") && names.contains(&"slow"),
        "{configs}"
    );
    let r = h.call("run.start", json!({ "name": "hello" })).unwrap();
    let id = r["run"].as_str().unwrap().to_owned();
    let conn = h.conn();
    let out = read_until(
        &conn,
        r["stdout"].as_u64().unwrap() as u32,
        "to-out",
        Duration::from_secs(10),
    );
    let err = read_until(
        &conn,
        r["stderr"].as_u64().unwrap() as u32,
        "to-err",
        Duration::from_secs(10),
    );
    assert!(out.contains("to-out") && !out.contains("to-err"), "{out}");
    assert!(err.contains("to-err"), "{err}");
    h.event(
        "run.event",
        |p| p["run"] == id.as_str() && p["kind"] == "started",
        Duration::from_secs(10),
    );
    let fin = h.event(
        "run.event",
        |p| p["run"] == id.as_str() && p["kind"] == "finished",
        Duration::from_secs(10),
    );
    // SPEC-0324 behaviour 13: the end of a run is a notice for every session.
    let notices = h.call("notice.list", json!({})).unwrap();
    assert!(
        notices["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "run" && n["title"].as_str().unwrap().contains("hello")),
        "{notices}"
    );
    assert_eq!(fin["exit_code"], 0, "{fin}");
    let t = h.call("run.start", json!({ "name": "tests" })).unwrap();
    let tid = t["run"].as_str().unwrap().to_owned();
    let tests = h.event(
        "run.event",
        |p| p["run"] == tid.as_str() && p["kind"] == "tests",
        Duration::from_secs(10),
    );
    assert!(tests["report"].to_string().contains("second"), "{tests}");
    let s = h.call("run.start", json!({ "name": "slow" })).unwrap();
    let sid = s["run"].as_str().unwrap().to_owned();
    h.session.drop_connection();
    std::thread::sleep(Duration::from_millis(1500));
    h.reconnect();
    let a = h.call("run.attach", json!({ "run": sid })).unwrap();
    let conn = h.conn();
    let out = read_until(
        &conn,
        a["stdout"].as_u64().unwrap() as u32,
        "late-line",
        Duration::from_secs(10),
    );
    assert!(out.contains("late-line"), "{out}");
    let t0 = Instant::now();
    h.call("run.stop", json!({ "run": sid })).unwrap();
    let fin = h.event(
        "run.event",
        |p| p["run"] == sid.as_str() && p["kind"] == "finished",
        Duration::from_secs(10),
    );
    assert_eq!(fin["stopped"], true, "{fin}");
    // Asked to end first (SPEC-0318 v4), not killed outright.
    assert_eq!(fin["exit_code"], 143, "{fin}");
    assert!(t0.elapsed() < Duration::from_secs(10));
    assert_eq!(
        code(h.call("run.start", json!({ "name": "missing" }))),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(h.call("run.stop", json!({ "run": "r99" }))),
        ErrorCode::NotFound
    );
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The TCP listening addresses of this process (`lsof`), if `lsof` is there.
fn own_listeners() -> Option<Vec<String>> {
    let out = Command::new("lsof")
        .args([
            "-nP",
            "-a",
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fn",
            "-p",
            &std::process::id().to_string(),
        ])
        .output()
        .ok()?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.strip_prefix('n').map(str::to_owned))
            .collect(),
    )
}

#[test]
fn ports_are_offered_forwarded_on_loopback_only_and_debug_ports_refused() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipped: no python3");
        return;
    }
    let dir = scratch("port");
    std::fs::write(dir.join("p/index.html"), "hello from the host\n").unwrap();
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    let port = free_port();
    let t = h.call("term.open", json!({})).unwrap();
    let stream = t["stream"].as_u64().unwrap() as u32;
    let conn = h.conn();
    conn.open_sending(stream);
    conn.send_data(
        stream,
        format!("exec python3 -m http.server {port} --bind 127.0.0.1\n").as_bytes(),
        Duration::from_secs(5),
    )
    .unwrap();
    let found = h.event(
        "port.detected",
        |p| p["port"] == port,
        Duration::from_secs(20),
    );
    assert_eq!(found["kind"], "service", "{found}");
    assert!(
        found["command"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .starts_with("python"),
        "{found}"
    );
    let listed = h.call("port.list", json!({})).unwrap();
    assert!(listed["detected"].to_string().contains(&port.to_string()));
    let f = h.call("port.forward", json!({ "port": port })).unwrap();
    let forward = f["forward"].as_str().unwrap().to_owned();
    assert_eq!(f["target"], format!("127.0.0.1:{port}"));
    let local = LocalForward::start(h.conn(), &forward, 0).unwrap();
    assert!(
        local.addresses.iter().all(|a| a.ip().is_loopback()),
        "{:?}",
        local.addresses
    );
    let mut c = std::net::TcpStream::connect(("127.0.0.1", local.port())).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(b"GET /index.html HTTP/1.0\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut answer = String::new();
    let _ = c.read_to_string(&mut answer);
    assert!(
        answer.contains("200") && answer.contains("hello from the host"),
        "{answer}"
    );
    // SEC-7: every TCP listener of this process (the window side, the backend in process) is on
    // loopback.
    if let Some(addrs) = own_listeners() {
        assert!(!addrs.is_empty());
        for a in addrs {
            assert!(
                a.starts_with("127.0.0.1:") || a.starts_with("[::1]:"),
                "listener on {a}"
            );
        }
    }
    // Debug ports are not forwarded; database ports only after the user's confirmation.
    let e = h.call("port.forward", json!({ "port": 5005 })).unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
    assert!(e.message.contains("debug"), "{e:?}");
    let e = h.call("port.forward", json!({ "port": 5432 })).unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
    assert!(e.message.contains("every user"), "{e:?}");
    let db = h
        .call("port.forward", json!({ "port": 5432, "confirm": true }))
        .unwrap();
    assert_eq!(db["kind"], "database");
    assert_eq!(
        code(h.call("port.forward", json!({ "port": 0 }))),
        ErrorCode::Invalid
    );
    assert_eq!(
        code(h.call("port.forward", json!({ "port": 80, "target_host": "a b" }))),
        ErrorCode::Invalid
    );
    h.call("port.close", json!({ "forward": forward })).unwrap();
    local.close();
    assert_eq!(
        code(h.call("port.connect", json!({ "forward": forward }))),
        ErrorCode::NotFound
    );
    let _ = h.session.bye();
}

/// SPEC-0322 v7 behaviour 10: a Delve server is talked to directly, and only on the host's
/// loopback (its port is not protected); nothing is started for it.
#[test]
fn a_delve_server_outside_the_hosts_loopback_is_refused() {
    let dir = scratch("dlv-remote");
    let mut h = host(&dir, Right::ALL.to_vec(), options());
    let attach = |host: &str| {
        json!({ "adapter": "dlv", "mode": "attach",
                "attach": { "kind": "address", "host": host, "port": 2345 } })
    };
    let refused = h.call("dap.start", attach("10.0.0.7")).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Denied, "{refused:?}");
    assert!(refused.message.contains("loopback"), "{refused:?}");
    // On the loopback the backend connects at once; with nothing listening that fails as such.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut params = attach("127.0.0.1");
    params["attach"]["port"] = json!(port);
    let failed = h.call("dap.start", params).unwrap_err();
    assert_eq!(failed.code, ErrorCode::Unavailable, "{failed:?}");
    assert!(failed.message.contains("on the host"), "{failed:?}");
    let _ = h.session.bye();
}

#[test]
fn a_debug_adapter_starts_on_the_host_over_a_stream_without_a_port() {
    let dir = scratch("dap");
    let mut opts = options();
    opts.extra_adapters = vec![("echo-adapter".into(), vec!["cat".into()])];
    let mut h = host(&dir, Right::ALL.to_vec(), opts);
    let list = h.call("dap.adapters", json!({})).unwrap();
    assert!(
        list["adapters"].to_string().contains("echo-adapter"),
        "{list}"
    );
    assert!(
        list["adapters"]
            .to_string()
            .contains("\"transport\":\"stdio\""),
        "{list}"
    );
    assert!(list["root"].as_str().is_some(), "{list}");
    let s = h
        .call(
            "dap.start",
            json!({ "adapter": "echo-adapter", "mode": "attach", "attach": { "kind": "address", "port": 1 } }),
        )
        .unwrap();
    let (session, stream) = (
        s["session"].as_str().unwrap().to_owned(),
        s["stream"].as_u64().unwrap() as u32,
    );
    let conn = h.conn();
    conn.open_sending(stream);
    let msg = b"Content-Length: 13\r\n\r\n{\"seq\":1,\"a\"}";
    conn.send_data(stream, msg, Duration::from_secs(5)).unwrap();
    let back = read_until(&conn, stream, "{\"seq\":1", Duration::from_secs(10));
    assert!(back.contains("Content-Length: 13"), "{back}");
    if let Some(addrs) = own_listeners() {
        assert!(
            addrs
                .iter()
                .all(|a| a.starts_with("127.0.0.1:") || a.starts_with("[::1]:")),
            "{addrs:?}"
        );
    }
    h.call("dap.stop", json!({ "session": session })).unwrap();
    h.event(
        "dap.exited",
        |p| p["session"] == session.as_str(),
        Duration::from_secs(10),
    );
    assert_eq!(
        code(h.call("dap.start", json!({ "adapter": "nope" }))),
        ErrorCode::NotFound
    );
}

fn fake_program(dir: &Path, name: &str, script: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.display().to_string()
}

fn has_python() -> bool {
    Command::new("python3").arg("-V").output().is_ok()
}

const FAKE_DLV: &str = r#"#!/usr/bin/env python3
import socket, sys
assert sys.argv[1] == "--listen=127.0.0.1:0"
s = socket.socket()
s.bind(("127.0.0.1", 0))
s.listen(1)
print("DAP server listening at: 127.0.0.1:%d" % s.getsockname()[1], flush=True)
c, _ = s.accept()
while True:
    d = c.recv(4096)
    if not d:
        break
    c.sendall(d)
"#;

/// SPEC-0322 AK-4: a TCP adapter is relayed; the backend opens no listener of its own and the port
/// does not go through `port.forward`.
#[test]
fn a_tcp_adapter_is_relayed_through_the_session() {
    if !has_python() {
        return;
    }
    let dir = scratch("dap-tcp");
    ferris_idea_core::trust::trust_for_process(&dir.join("p"));
    let mut opts = options();
    let fake = fake_program(&dir, "fake-adapter", FAKE_DLV);
    opts.extra_tcp_adapters = vec![("fake-adapter".into(), vec![fake])];
    let h = host(&dir, Right::ALL.to_vec(), opts);
    let list = h.call("dap.adapters", json!({})).unwrap();
    assert!(
        list["adapters"]
            .to_string()
            .contains("\"transport\":\"tcp\""),
        "{list}"
    );
    let s = h
        .call("dap.start", json!({ "adapter": "fake-adapter" }))
        .unwrap();
    let stream = s["stream"].as_u64().unwrap() as u32;
    let conn = h.conn();
    conn.open_sending(stream);
    conn.send_data(stream, b"ping-through-relay", Duration::from_secs(5))
        .unwrap();
    let back = read_until(&conn, stream, "ping-through-relay", Duration::from_secs(10));
    assert!(back.contains("ping-through-relay"), "{back}");
    if let Some(addrs) = own_listeners() {
        assert!(
            addrs
                .iter()
                .all(|a| a.starts_with("127.0.0.1:") || a.starts_with("[::1]:")),
            "{addrs:?}"
        );
    }
    // The adapter's port is refused while it is open, although the port list has not seen it.
    let port = s["adapter_port"].as_u64().unwrap();
    assert!(port > 0, "{s}");
    let e = h.call("port.forward", json!({ "port": port })).unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied, "{e:?}");
    assert!(e.message.contains("debugger"), "{e:?}");
    h.call("dap.stop", json!({ "session": s["session"] }))
        .unwrap();
    let after = h.call("port.forward", json!({ "port": port })).unwrap();
    assert_ne!(after["kind"], "debug", "{after}");
    h.call("port.close", json!({ "forward": after["forward"] }))
        .unwrap();
}

/// SPEC-0322 AK-6, AK-7: `dap.attach` answers the offsets for an exact resend, and tells when
/// more was produced than kept.
#[test]
fn dap_attach_offsets_are_exact_and_a_loss_is_told() {
    let dir = scratch("dap-attach");
    let mut opts = options();
    opts.extra_adapters = vec![("cat-adapter".into(), vec!["cat".into()])];
    let h = host(&dir, Right::ALL.to_vec(), opts);
    let s = h
        .call(
            "dap.start",
            json!({ "adapter": "cat-adapter", "mode": "attach", "attach": { "kind": "pid", "pid": 1 } }),
        )
        .unwrap();
    let (id, stream) = (
        s["session"].as_str().unwrap().to_owned(),
        s["stream"].as_u64().unwrap() as u32,
    );
    let conn = h.conn();
    conn.open_sending(stream);
    conn.send_data(stream, b"hello", Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        read_until(&conn, stream, "hello", Duration::from_secs(5)),
        "hello"
    );
    let a = h
        .call("dap.attach", json!({ "session": id, "from": 2 }))
        .unwrap();
    assert_eq!(a["start"], 2);
    assert_eq!(a["received"], 5);
    let new = a["stream"].as_u64().unwrap() as u32;
    assert_eq!(read_until(&conn, new, "llo", Duration::from_secs(5)), "llo");
    conn.open_sending(new);
    conn.send_data(new, &vec![b'x'; 3 << 20], Duration::from_secs(20))
        .unwrap();
    let t0 = Instant::now();
    let mut got = 0;
    while got < 3 << 20 && t0.elapsed() < Duration::from_secs(20) {
        let (data, _) = conn.read_stream(new, Duration::from_millis(50));
        got += data.len();
    }
    assert_eq!(got, 3 << 20);
    let lost = h
        .call("dap.attach", json!({ "session": id, "from": 0 }))
        .unwrap();
    assert!(lost["start"].as_u64().unwrap() > 0, "{lost}");
    h.call("dap.stop", json!({ "session": id })).unwrap();
}

/// SPEC-0322 AK-9, AK-10: docker through the `inspect` command and a pod through a tunnel.
#[test]
fn container_and_pod_targets_resolve_on_the_host() {
    let dir = scratch("dap-targets");
    let mut opts = options();
    opts.extra_adapters = vec![("cat-adapter".into(), vec!["cat".into()])];
    let project = dir.join("p");
    opts.docker_program = Some(fake_program(
        &dir,
        "fake-docker",
        &format!(
            "#!/bin/sh\necho '[{{\"NetworkSettings\":{{\"Ports\":{{\"5678/tcp\":[{{\"HostIp\":\"127.0.0.1\",\"HostPort\":\"15678\"}}]}}}},\"Mounts\":[{{\"Type\":\"bind\",\"Source\":\"{}\",\"Destination\":\"/app\"}}]}}]'\n",
            project.display()
        ),
    ));
    opts.kubectl_program = Some(fake_program(
        &dir,
        "fake-kubectl",
        "#!/bin/sh\necho \"args: $*\"\necho 'Forwarding from 127.0.0.1:43211 -> 5005'\nexec sleep 30\n",
    ));
    let h = host(&dir, Right::ALL.to_vec(), opts);
    let d = h
        .call(
            "dap.start",
            json!({ "adapter": "cat-adapter", "mode": "attach",
                    "attach": { "kind": "docker", "container": "web", "port": 5678 } }),
        )
        .unwrap();
    assert_eq!(d["address"], json!({ "host": "127.0.0.1", "port": 15678 }));
    assert_eq!(
        d["mappings"],
        json!([[project.display().to_string(), "/app"]])
    );
    assert_eq!(
        code(h.call(
            "dap.start",
            json!({ "adapter": "cat-adapter", "mode": "attach",
                    "attach": { "kind": "docker", "container": "a;b", "port": 1 } }),
        )),
        ErrorCode::Invalid
    );
    let p = h
        .call(
            "dap.start",
            json!({ "adapter": "cat-adapter", "mode": "attach",
                    "attach": { "kind": "pod", "pod": "api-1", "namespace": "prod", "port": 5005 } }),
        )
        .unwrap();
    assert_eq!(p["address"], json!({ "host": "127.0.0.1", "port": 43211 }));
    h.call("dap.stop", json!({ "session": d["session"] }))
        .unwrap();
    h.call("dap.stop", json!({ "session": p["session"] }))
        .unwrap();
}

/// SPEC-0322 AK-12: the plan is made on the host; a page is refused; process arguments stay out.
#[test]
fn the_debug_plan_and_host_processes() {
    let dir = scratch("dap-plan");
    std::fs::create_dir_all(dir.join("p/.ferris-idea")).unwrap();
    std::fs::write(
        dir.join("p/.ferris-idea/run.toml"),
        "[[run]]\nname = \"app\"\ncommand = [\"python3\", \"app.py\", \"--x\"]\n\n[[run]]\nname = \"web\"\ncommand = []\nurl = \"http://localhost:1/\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("p/app.py"), "print(1)\n").unwrap();
    let h = host(&dir, Right::ALL.to_vec(), options());
    let plan = h.call("dap.plan", json!({ "name": "app" })).unwrap();
    assert_eq!(plan["kind"], "python", "{plan}");
    assert_eq!(plan["adapter"], "debugpy");
    assert!(plan["program"].as_str().unwrap().ends_with("/app.py"));
    assert_eq!(plan["args"], json!(["--x"]));
    assert!(plan["root"].as_str().is_some());
    assert_eq!(
        code(h.call("dap.plan", json!({ "name": "web" }))),
        ErrorCode::Unavailable
    );
    assert_eq!(
        code(h.call("dap.plan", json!({ "name": "none" }))),
        ErrorCode::NotFound
    );
    let list = h.call("dap.processes", json!({})).unwrap();
    assert!(list["processes"].is_array(), "{list}");
    for p in list["processes"].as_array().unwrap() {
        assert!(
            !p["command"].as_str().unwrap_or_default().contains(' '),
            "{p}"
        );
    }
}

/// SPEC-0318 v2 AK-14: attaching at an offset sends exactly what follows it; `search.text` matches
/// carry their character offset; `git.show` answers with project-relative paths.
#[test]
fn reattaching_at_an_offset_search_offsets_and_git_show() {
    let dir = scratch("v2");
    let p = dir.join("p");
    git(&p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("a.txt"), "äb\nfind me\n").unwrap();
    git(&p, &["add", "a.txt"]);
    git(&p, &["commit", "-q", "-m", "first"]);
    let h = host(&dir, Right::ALL.to_vec(), options());

    // The match's offset counts characters ("ä" is one).
    let found = h.call("search.text", json!({ "query": "find" })).unwrap();
    assert_eq!(found["files"][0]["doc"], "a.txt", "{found}");
    assert_eq!(found["files"][0]["matches"][0]["start"], 3, "{found}");

    let shown = h.call("git.show", json!({ "id": "HEAD" })).unwrap();
    assert_eq!(shown["info"]["summary"], "first", "{shown}");
    assert_eq!(shown["changes"][0]["path"], "a.txt", "{shown}");
    assert_eq!(
        code(h.call("git.show", json!({ "id": "--output=/tmp/x" }))),
        ErrorCode::Invalid
    );

    // A terminal: the window has read N bytes; after the break it gets what follows N only.
    let mut h = h;
    let t = h
        .call("term.open", json!({ "cols": 80, "rows": 24 }))
        .unwrap();
    let (term, stream) = (
        t["term"].as_str().unwrap().to_owned(),
        t["stream"].as_u64().unwrap() as u32,
    );
    let conn = h.conn();
    conn.open_sending(stream);
    conn.send_data(stream, b"echo first-$((1+1))\n", Duration::from_secs(5))
        .unwrap();
    let before = read_until(&conn, stream, "first-2\r\n", Duration::from_secs(10));
    assert!(before.contains("first-2\r\n"), "{before}");
    // Settle: the prompt after the output.
    std::thread::sleep(Duration::from_millis(300));
    let (rest, _) = conn.read_stream(stream, Duration::from_millis(100));
    let seen = (before.len() + rest.len()) as u64;
    h.session.drop_connection();
    h.reconnect();
    let a = h
        .call("term.attach", json!({ "term": term, "from": seen }))
        .unwrap();
    assert_eq!(a["from"], seen, "{a}");
    let s2 = a["stream"].as_u64().unwrap() as u32;
    let conn = h.conn();
    conn.open_sending(s2);
    conn.send_data(s2, b"echo second-$((1+2))\n", Duration::from_secs(5))
        .unwrap();
    let after = read_until(&conn, s2, "second-3\r\n", Duration::from_secs(10));
    assert!(after.contains("second-3"), "{after}");
    assert!(
        !after.contains("first-2"),
        "nothing before the offset: {after}"
    );
    // Without an offset everything kept comes again (v1).
    let all = h.call("term.attach", json!({ "term": term })).unwrap();
    assert_eq!(all["from"], 0, "{all}");
    let s3 = all["stream"].as_u64().unwrap() as u32;
    let again = read_until(&h.conn(), s3, "second-3\r\n", Duration::from_secs(10));
    assert!(
        again.contains("first-2") && again.contains("second-3"),
        "{again}"
    );
}

/// What a window may call changed: raise `ferris_idea_session::hello::MINOR` so that a backend without
/// it is refused when connecting, then put the new minor and hash here (`ADR-0026` §4). A new `mcp.*`
/// family is left out: it is bound only with the MCP feature.
const METHOD_SURFACE: (u32, &str) = (1, "91e7e8ea5344a7e8");

#[test]
fn a_changed_method_surface_raises_the_minor() {
    let dir = scratch("surface");
    let h = host(&dir, Right::ALL.to_vec(), options());
    let names = h.backend.method_names();
    assert!(names.iter().any(|m| m == "dap.plan"), "{names:?}");
    let hash = ferris_idea_session::ot::str_hash(&names.join("\n"));
    assert_eq!(
        (ferris_idea_session::hello::MINOR, hash.as_str()),
        METHOD_SURFACE,
        "the backend's methods or MINOR changed; raise MINOR when a method was added"
    );
}

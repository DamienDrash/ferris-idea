//! SPEC-0318 AK-2 against a real language server (clangd, as in CI): the backend keeps the
//! server's copy of a document in step with the session's edits, so diagnostics follow typing
//! without the window sending any text; completion, hover, definition, semantic tokens and
//! rename run on the backend's text; places outside the project come back without a host path
//! (SEC-14). A test binary of its own, because the language server pool is per process. Skipped
//! without clangd, failing under `IDECRAFT_REQUIRE_TOOLS`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use idecraft_host::HostOptions;
use idecraft_session::{
    Backend, BackendConfig, ClientSession, ErrorCode, Op, Right, Transport, in_process,
};
use serde_json::{Value, json};

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn connect(b: &Backend) -> Transport {
    let (window, server) = in_process();
    let b = b.clone();
    std::thread::spawn(move || b.serve(server));
    window
}

fn wait_diagnostics(s: &mut ClientSession, revision: u64, want: impl Fn(&Value) -> bool) -> Value {
    let t0 = Instant::now();
    loop {
        s.pump(Duration::from_millis(100));
        if let Some(i) = s.host_events.iter().position(|(m, p)| {
            m == "lsp.diagnostics" && p["revision"] == revision && want(&p["items"])
        }) {
            return s.host_events.remove(i).1;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "no diagnostics for revision {revision}: {:?}",
            s.host_events
        );
    }
}

#[test]
fn language_servers_on_the_host_follow_the_session_documents() {
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let dir: PathBuf = std::env::temp_dir().join(format!("idc-host-lsp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("p")).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    let main = "#include <stdio.h>\n\nint add(int a, int b) {\n    return a + b;\n}\n\nint main(void) {\n    int x = add(1, 2);\n    printf(\"%d\\n\", add(x, 3));\n    return 0;\n}\n";
    std::fs::write(dir.join("p/main.c"), main).unwrap();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        rights: Right::ALL.to_vec(),
        ..BackendConfig::default()
    });
    idecraft_host::bind(
        &backend,
        HostOptions {
            language_servers: true,
            ..HostOptions::default()
        },
    );
    let mut s = ClientSession::open(
        connect(&backend),
        &dir.join("p").display().to_string(),
        "test",
        "testhost",
        None,
    )
    .unwrap();
    let call = |s: &ClientSession, m: &str, p: Value| s.call_within(m, p, Duration::from_secs(90));
    s.open_doc("main.c").unwrap();
    // A clean file first, then an undeclared name typed through the session only.
    wait_diagnostics(&mut s, 1, |items| {
        !items
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["severity"] == "error")
    });
    let at = main.find("return 0").unwrap();
    s.edit(
        "main.c",
        &[Op {
            at,
            delete: 0,
            insert: "missing_name = 1;\n    ".into(),
        }],
    )
    .unwrap();
    assert!(s.settle(Duration::from_secs(10)));
    let d = wait_diagnostics(&mut s, 2, |items| {
        items.to_string().contains("missing_name")
    });
    assert_eq!(d["doc"], "main.c");
    // Questions name the revision they were computed on.
    let e = call(
        &s,
        "lsp.hover",
        json!({ "doc": "main.c", "revision": 1, "line": 7, "character": 13 }),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::StaleRevision);
    let h = call(
        &s,
        "lsp.hover",
        json!({ "doc": "main.c", "revision": 2, "line": 7, "character": 13 }),
    )
    .unwrap();
    assert!(h["hover"]["text"].as_str().unwrap().contains("add"), "{h}");
    let c = call(
        &s,
        "lsp.complete",
        json!({ "doc": "main.c", "revision": 2, "line": 7, "character": 13, "limit": 50 }),
    )
    .unwrap();
    assert!(c["items"].to_string().contains("\"add\""), "{c}");
    let def = call(
        &s,
        "lsp.locations",
        json!({ "doc": "main.c", "revision": 2, "line": 7, "character": 13, "kind": "definition" }),
    )
    .unwrap();
    assert_eq!(def["locations"][0]["doc"], "main.c", "{def}");
    assert_eq!(def["locations"][0]["range"]["start_line"], 2, "{def}");
    // printf is declared outside the project: only a name, never a host path (SEC-14).
    let out = call(
        &s,
        "lsp.locations",
        json!({ "doc": "main.c", "revision": 2, "line": 8, "character": 6, "kind": "definition" }),
    )
    .unwrap();
    assert!(out["locations"].as_array().unwrap().is_empty(), "{out}");
    let text = out.to_string();
    assert!(text.contains("outside the project"), "{text}");
    assert!(
        !text.contains("/usr/") && !text.contains("/Library/"),
        "{text}"
    );
    let sem = call(
        &s,
        "lsp.semantic",
        json!({ "doc": "main.c", "revision": 2 }),
    )
    .unwrap();
    assert!(!sem["tokens"].as_array().unwrap().is_empty(), "{sem}");
    // Rename: planned, then applied to the open document as an edit of the backend.
    let plan = call(
        &s,
        "lsp.rename",
        json!({ "doc": "main.c", "revision": 2, "line": 2, "character": 5, "new_name": "sum" }),
    )
    .unwrap();
    assert_eq!(plan["files"][0]["doc"], "main.c", "{plan}");
    assert_eq!(plan["files"][0]["edits"], 3, "{plan}");
    let applied = call(
        &s,
        "lsp.rename",
        json!({ "doc": "main.c", "revision": 2, "line": 2, "character": 5, "new_name": "sum", "apply": true }),
    )
    .unwrap();
    assert_eq!(applied["changed"][0], "main.c", "{applied}");
    let t0 = Instant::now();
    while !s.doc("main.c").unwrap().text().contains("int sum(") {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the rename did not reach the window"
        );
        s.pump(Duration::from_millis(50));
    }
    assert_eq!(s.doc("main.c").unwrap().text().matches("sum(").count(), 3);
    let status = call(&s, "lsp.status", json!({})).unwrap();
    assert!(status["running"].as_u64().unwrap() >= 1, "{status}");
    assert!(!status.to_string().contains("/usr/bin"), "{status}");
    let _ = s.bye();
    idecraft_languages::pool::shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

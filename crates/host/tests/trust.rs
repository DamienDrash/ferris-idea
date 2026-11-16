//! SPEC-0318 behaviour 5 and AK-6: in safe mode an untrusted host project runs nothing until
//! `run.trust` adds it to the host's trust list. A test binary of its own, because safe mode is
//! switched on for the whole process.

use std::time::Duration;

use idecraft_host::HostOptions;
use idecraft_session::{
    Backend, BackendConfig, ClientSession, ErrorCode, Right, Transport, in_process,
};
use serde_json::json;

fn connect(b: &Backend) -> Transport {
    let (window, server) = in_process();
    let b = b.clone();
    std::thread::spawn(move || b.serve(server));
    window
}

#[test]
fn an_untrusted_host_project_runs_nothing_until_it_is_trusted() {
    let dir = std::env::temp_dir().join(format!("idc-host-trust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("p/.idecraft")).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    std::fs::write(
        dir.join("p/.idecraft/run.toml"),
        "[[run]]\nname = \"hello\"\ncommand = [\"sh\", \"-c\", \"echo hi\"]\n",
    )
    .unwrap();
    idecraft_core::trust::enforce();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        rights: Right::ALL.to_vec(),
        ..BackendConfig::default()
    });
    let trust_file = dir.join("settings/trusted-locations.toml");
    idecraft_host::bind(
        &backend,
        HostOptions {
            language_servers: false,
            trust_file: Some(trust_file.clone()),
            ..HostOptions::default()
        },
    );
    let s = ClientSession::open(
        connect(&backend),
        &dir.join("p").display().to_string(),
        "test",
        "testhost",
        None,
    )
    .unwrap();
    let call = |m: &str, p| s.call_within(m, p, Duration::from_secs(30));
    assert_eq!(call("run.configs", json!({})).unwrap()["trusted"], false);
    let e = call("run.start", json!({ "name": "hello" })).unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
    assert!(e.message.contains("Safe mode"), "{e:?}");
    assert!(
        e.hint.as_deref().unwrap_or_default().contains("run.trust"),
        "{e:?}"
    );
    call("run.trust", json!({})).unwrap();
    let list = std::fs::read_to_string(&trust_file).unwrap();
    assert!(
        list.contains(&dir.join("p").display().to_string()),
        "{list}"
    );
    assert_eq!(call("run.configs", json!({})).unwrap()["trusted"], true);
    assert!(call("run.start", json!({ "name": "hello" })).is_ok());
    let audit = std::fs::read_to_string(dir.join("state/remote/audit.jsonl")).unwrap();
    assert!(audit.contains("\"method\":\"run.trust\""), "{audit}");
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC-0318 v5 AK-17: an untrusted project starts no git process at all.
#[test]
fn git_processes_stay_off_for_an_untrusted_project() {
    let dir = std::env::temp_dir().join(format!("idc-host-trust-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("p")).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    let p = dir.join("p");
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
                .args(args)
                .current_dir(&p)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(p.join("a.txt"), "one\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "first"]);
    // A hook that leaves a mark when anything runs it.
    let mark = dir.join("hook-ran");
    let hook = p.join(".git/hooks/pre-push");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", mark.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    idecraft_core::trust::enforce();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        rights: Right::ALL.to_vec(),
        ..BackendConfig::default()
    });
    idecraft_host::bind(
        &backend,
        HostOptions {
            language_servers: false,
            ..HostOptions::default()
        },
    );
    let s = ClientSession::open(
        connect(&backend),
        &p.display().to_string(),
        "test",
        "testhost",
        None,
    )
    .unwrap();
    let call = |m: &str, a| s.call_within(m, a, Duration::from_secs(30));
    for (method, params) in [
        ("git.fetch", json!({})),
        ("git.pull", json!({})),
        ("git.push", json!({})),
        ("git.outgoing", json!({})),
        ("git.reset_upstream", json!({})),
        ("git.stashes", json!({})),
        ("git.stash_push", json!({})),
        (
            "git.commit_action",
            json!({ "action": "revert", "id": "HEAD" }),
        ),
        ("git.patch", json!({ "id": "HEAD" })),
        ("git.rebase_todo", json!({ "onto": "HEAD" })),
        // SPEC-0318 v6 AK-22.
        ("git.merge", json!({ "source": "HEAD" })),
        ("git.rebase_onto", json!({ "onto": "HEAD" })),
        ("git.rename_branch", json!({ "old": "main", "new": "m" })),
        ("git.delete_branch", json!({ "name": "main" })),
    ] {
        let e = call(method, params).expect_err(method);
        assert_eq!(e.code, ErrorCode::Denied, "{method}: {e:?}");
        assert!(e.message.contains("Safe mode"), "{method}: {e:?}");
    }
    assert!(!mark.exists(), "a hook ran for an untrusted project");
    // The Git library still answers.
    assert_eq!(call("git.status", json!({})).unwrap()["branch"], "main");
    assert!(
        call(
            "git.commits_touching",
            json!({ "ids": [], "paths": ["a.txt"] })
        )
        .is_ok()
    );
    assert_eq!(
        call("git.log", json!({})).unwrap()["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0322 AK-5: an untrusted host project launches no debuggee; attaching stays allowed.
#[test]
fn an_untrusted_project_refuses_a_debug_launch_not_an_attach() {
    let dir = std::env::temp_dir().join(format!("idc-host-dap-trust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("p")).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    idecraft_core::trust::enforce();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(dir.join("state")),
        rights: Right::ALL.to_vec(),
        ..BackendConfig::default()
    });
    idecraft_host::bind(
        &backend,
        HostOptions {
            language_servers: false,
            extra_adapters: vec![("cat-adapter".into(), vec!["cat".into()])],
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
    let launch = s.call_within(
        "dap.start",
        json!({ "adapter": "cat-adapter" }),
        Duration::from_secs(30),
    );
    assert_eq!(launch.unwrap_err().code, ErrorCode::Denied);
    let prepare = s.call_within(
        "dap.start",
        json!({ "adapter": "cat-adapter", "mode": "attach", "prepare": "true",
                "attach": { "kind": "address", "port": 1 } }),
        Duration::from_secs(30),
    );
    assert_eq!(prepare.unwrap_err().code, ErrorCode::Denied);
    let a = s
        .call_within(
            "dap.start",
            json!({ "adapter": "cat-adapter", "mode": "attach",
                    "attach": { "kind": "pid", "pid": 1 } }),
            Duration::from_secs(30),
        )
        .unwrap();
    s.call_within(
        "dap.stop",
        json!({ "session": a["session"] }),
        Duration::from_secs(30),
    )
    .unwrap();
    let _ = s.bye();
}

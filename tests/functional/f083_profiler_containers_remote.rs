//! F-083 · Profiler, Docker, Kubernetes, remote development (SPEC-0119, SPEC-0120, SPEC-0121,
//! SPEC-0158, SPEC-0251, SPEC-0253).
//!
//! self-test: profiler_steps, containers_steps, remote_steps
//!
//! Live Docker, Kubernetes and SSH hosts belong to the user; here only previews, refusals and what
//! the machine itself shows.

use crate::harness::*;
use serde_json::json;

#[test]
fn profiler_lists_own_processes_and_never_the_reference() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("profile_processes", json!({}));
    for proc in r["processes"].as_array().unwrap() {
        // Only the program name: arguments may hold secrets.
        let cmd = proc["command"].as_str().unwrap_or_default();
        assert!(!cmd.contains(' ') && !cmd.contains('/'), "{proc}");
    }
    // A refused process still says why (the reference, behaviour 2).
    for proc in r["processes"].as_array().unwrap() {
        if !proc["refused"].is_null() {
            assert!(text(&proc["refused"]).contains("JetBrains"), "{proc}");
        }
    }
    p.write(
        ".idecraft/run.toml",
        "[[run]]\nname = \"w\"\ncommand = [\"sh\", \"-c\", \"true\"]\n",
    );
    let preview = ide.ok("profile_run", json!({"name": "w"}));
    assert!(text(&preview).contains("w"), "{preview}");
    ide.refused("profile_run", json!({"name": "w", "dry_run": false}));
}

#[test]
fn docker_compose_services_of_the_project() {
    let p = Project::empty();
    p.write("compose.yaml", "services:\n  db:\n    image: postgres:17\n  web:\n    image: nginx:1.29-alpine\n    depends_on: [db]\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.call("docker_compose", json!({"action": "services"}));
    match r {
        Ok(v) => {
            let all = text(&v);
            assert!(all.contains("db") && all.contains("web"), "{v}");
        }
        Err(e) => skip_optional(&format!("Docker Compose not available: {e}")),
    }
}

#[test]
fn ssh_hosts_need_the_ssh_right() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let reason = ide.refused("remote_list", json!({"host": "func-test-unknown-host"}));
    assert!(reason.contains("ssh"), "{reason}");
    let preview = ide.ok(
        "remote_exec",
        json!({"host": "func-test-unknown-host", "command": ["uname"]}),
    );
    assert!(text(&preview).contains("uname"), "{preview}");
}

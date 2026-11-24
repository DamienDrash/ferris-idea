//! SPEC-0347 AK-5: the Landlock sandbox on Linux (this module is compiled on Linux only). On a
//! kernel without Landlock ABI 9 or without user namespaces the launch must be refused, and the
//! test checks that instead.

use std::net::TcpListener;

use ferris_idea_sandbox::SandboxError;

use super::child::{policy, run};

#[test]
fn landlock_confines_writes_and_connections_or_refuses() {
    let work = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let work_dir = std::fs::canonicalize(work.path()).unwrap();
    let out_dir = std::fs::canonicalize(outside.path()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let inside = work_dir.join("inside.txt");
    let escaped = out_dir.join("escaped.txt");
    let actions = vec![
        format!("write:{}", inside.display()),
        format!("write:{}", escaped.display()),
        format!("connect:{addr}"),
    ];
    match run(policy(std::slice::from_ref(&work_dir), &[]), &actions) {
        Ok(r) => {
            assert_eq!(r[&actions[0]], "ok", "{r:?}");
            assert!(r[&actions[1]].starts_with("denied"), "{r:?}");
            assert!(r[&actions[2]].starts_with("denied"), "{r:?}");
            assert!(inside.exists());
        }
        Err(SandboxError::Refused(m)) => {
            assert!(m.contains("ferris-idea-sandbox: refused:"), "{m}");
            eprintln!("sandbox not available here, the launch was refused: {m}");
            assert!(!inside.exists());
        }
        Err(e) => panic!("{e}"),
    }
    assert!(!escaped.exists());
    // Hosts cannot be enforced on Linux yet: refused before anything starts.
    let hosts = policy(&[work_dir], &[addr.to_string()]);
    assert!(matches!(
        run(hosts, &actions),
        Err(SandboxError::Refused(_))
    ));
}

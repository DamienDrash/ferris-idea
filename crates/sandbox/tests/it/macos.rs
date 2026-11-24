//! SPEC-0347 AK-4: the Seatbelt sandbox on macOS (this module is compiled on macOS only).

use std::net::TcpListener;

use super::child::{policy, run};

#[test]
fn seatbelt_confines_writes_reads_and_connections() {
    let work = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let work_dir = std::fs::canonicalize(work.path()).unwrap();
    let out_dir = std::fs::canonicalize(outside.path()).unwrap();
    let secret = out_dir.join("secret.txt");
    std::fs::write(&secret, "secret").unwrap();
    let allowed = TcpListener::bind("127.0.0.1:0").unwrap();
    let blocked = TcpListener::bind("127.0.0.1:0").unwrap();
    let (a, b) = (allowed.local_addr().unwrap(), blocked.local_addr().unwrap());
    let accept = |l: TcpListener| {
        std::thread::spawn(move || {
            for c in l.incoming().take(2) {
                drop(c);
            }
        })
    };
    let _ta = accept(allowed);
    let _tb = accept(blocked);

    let inside = work_dir.join("inside.txt");
    let escaped = out_dir.join("escaped.txt");
    let actions = vec![
        format!("write:{}", inside.display()),
        format!("write:{}", escaped.display()),
        format!("read:{}", secret.display()),
        format!("connect:{b}"),
        format!("proxy:{a}"),
        format!("proxy:{b}"),
    ];
    let p = policy(std::slice::from_ref(&work_dir), &[a.to_string()]);
    let r = run(p, &actions).expect("sandbox-exec is available on macOS");
    assert_eq!(r[&actions[0]], "ok", "{r:?}");
    assert!(r[&actions[1]].starts_with("denied"), "{r:?}");
    assert!(r[&actions[2]].starts_with("denied"), "{r:?}");
    assert!(r[&actions[3]].starts_with("denied"), "{r:?}");
    assert_eq!(r[&actions[4]], "ok", "{r:?}");
    assert!(r[&actions[5]].contains("403"), "{r:?}");
    assert!(inside.exists());
    assert!(!escaped.exists());

    // Without hosts there is no proxy and no network at all.
    let none = policy(std::slice::from_ref(&work_dir), &[]);
    let actions = vec![format!("connect:{a}")];
    let r = run(none, &actions).unwrap();
    assert!(r[&actions[0]].starts_with("denied"), "{r:?}");
}

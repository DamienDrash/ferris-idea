//! Against a real `sshd` started for the test (`ferris_idea_remote::testing`).

use std::time::Duration;

use ferris_idea_remote::testing::TestHost;
use ferris_idea_remote::{EntryKind, NewEntry, Remote, RemoteError, cksum};

fn host(name: &str) -> Option<TestHost> {
    let dir = std::env::temp_dir().join(format!("idc-ssh-{name}-{}", std::process::id()));
    let host = TestHost::start(&dir);
    if host.is_none() {
        eprintln!("skipped: no sshd/ssh-keygen");
    }
    host
}

#[test]
fn files_on_the_host() {
    let Some(h) = host("files") else { return };
    let r = Remote::new(h.target.clone());
    let base = h.dir.join("project");
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::write(base.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(base.join(".env"), "A=1\n").unwrap();
    std::fs::write(base.join("it's here.txt"), "q").unwrap();
    std::os::unix::fs::symlink(base.join("src"), base.join("code")).unwrap();
    let dir = base.display().to_string();

    assert_eq!(r.home().unwrap(), std::env::var("HOME").unwrap());
    assert_eq!(r.resolve_dir(&format!("{dir}/src/..")).unwrap(), dir);
    let entries = r.list(&dir).unwrap();
    let names: Vec<(&str, EntryKind, bool)> = entries
        .iter()
        .map(|e| (e.name.as_str(), e.kind, e.link))
        .collect();
    assert_eq!(
        names,
        [
            ("code", EntryKind::Dir, true),
            ("src", EntryKind::Dir, false),
            (".env", EntryKind::File, false),
            ("it's here.txt", EntryKind::File, false),
        ]
    );
    assert!(r.list(&format!("{dir}/nope")).is_err());

    let path = format!("{dir}/src/main.rs");
    let file = r.read(&path).unwrap();
    assert_eq!(file.bytes, b"fn main() {}\n");
    assert_eq!(file.stamp, cksum(b"fn main() {}\n"));
    assert!(r.read(&format!("{dir}/src")).is_err());

    // Saving keeps the file's mode and checks that nobody changed it meanwhile.
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        base.join("src/main.rs"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    let stamp = r
        .write(&path, b"fn main() { run() }\n", Some(&file.stamp))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(base.join("src/main.rs")).unwrap(),
        "fn main() { run() }\n"
    );
    let mode = std::fs::metadata(base.join("src/main.rs"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o640);
    std::fs::write(base.join("src/main.rs"), "changed by someone else\n").unwrap();
    // SPEC-0121 v7 AK-10: the refusal names both stamps; the host's stamp then overwrites.
    assert_eq!(
        r.write(&path, b"mine\n", Some(&stamp)),
        Err(RemoteError::Changed {
            path: path.clone(),
            expected: stamp.clone(),
            host: cksum(b"changed by someone else\n"),
        })
    );
    let text = r
        .write(&path, b"mine\n", Some(&stamp))
        .unwrap_err()
        .to_string();
    assert!(
        text.starts_with("conflict:") && text.contains(&stamp),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(base.join("src/main.rs")).unwrap(),
        "changed by someone else\n"
    );
    r.write(&path, b"mine\n", Some(&cksum(b"changed by someone else\n")))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(base.join("src/main.rs")).unwrap(),
        "mine\n"
    );
    // A file deleted on the host: the refusal says `absent`, and nothing is written.
    let gone = format!("{dir}/gone.txt");
    std::fs::write(base.join("gone.txt"), "x\n").unwrap();
    let gone_stamp = r.read(&gone).unwrap().stamp;
    std::fs::remove_file(base.join("gone.txt")).unwrap();
    assert!(matches!(
        r.write(&gone, b"y\n", Some(&gone_stamp)),
        Err(RemoteError::Changed { host, .. }) if host == "absent"
    ));
    assert!(!base.join("gone.txt").exists());
    // A new file must not exist yet; no temporary files are left behind.
    r.write(&format!("{dir}/new.txt"), b"n", None).unwrap();
    assert!(r.write(&format!("{dir}/new.txt"), b"n", None).is_err());
    assert!(!std::fs::read_dir(base.join("src")).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains("ferris-idea-")
    }));
}

#[test]
fn commands_quoting_and_limits() {
    let Some(h) = host("exec") else { return };
    let r = Remote::new(h.target.clone());
    let out = r
        .exec(
            &[
                "printf".into(),
                "%s|".into(),
                "a b".into(),
                "$(id)".into(),
                "it's".into(),
            ],
            None,
            Duration::from_secs(20),
        )
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a b|$(id)|it's|");
    let out = r
        .exec(
            &["sh".into(), "-c".into(), "cat; echo err >&2; exit 4".into()],
            Some(b"in"),
            Duration::from_secs(20),
        )
        .unwrap();
    assert_eq!(
        (out.exit_code, out.stdout.as_slice(), out.stderr.trim()),
        (Some(4), &b"in"[..], "err")
    );
    // A limit ends the call (stretched in tests for a machine under load, still below 10 s).
    assert!(matches!(
        r.exec(&["sleep".into(), "10".into()], None, Duration::from_secs(1)),
        Err(RemoteError::TimedOut(_, n)) if n < 10
    ));
}

#[test]
fn unknown_and_changed_host_keys_are_refused() {
    let Some(h) = host("keys") else { return };
    let unknown = Remote::new(h.unknown_key_target()).home().unwrap_err();
    assert!(
        matches!(unknown, RemoteError::HostKeyUnknown(_)),
        "{unknown:?}"
    );
    let changed = Remote::new(h.changed_key_target()).home().unwrap_err();
    assert!(
        matches!(changed, RemoteError::HostKeyChanged(_)),
        "{changed:?}"
    );
    // Nothing was added to the known hosts.
    assert_eq!(
        std::fs::read_to_string(h.dir.join("known_hosts_unknown")).unwrap(),
        ""
    );
    let mut nobody = h.target.clone();
    nobody
        .extra
        .extend(["-o".into(), "User=ferris-idea-nobody".into()]);
    let refused = Remote::new(nobody).home().unwrap_err();
    assert!(
        matches!(refused, RemoteError::AuthRequired(_)),
        "{refused:?}"
    );
    let mut closed = ferris_idea_remote::SshTarget::new("127.0.0.1");
    closed.port = Some(1);
    assert!(matches!(
        Remote::new(closed).home(),
        Err(RemoteError::Unreachable(_, _))
    ));
}

#[test]
fn creates_renames_and_deletes() {
    let Some(h) = host("ops") else { return };
    let r = Remote::new(h.target.clone());
    let base = h.dir.join("ops");
    std::fs::create_dir_all(&base).unwrap();
    let dir = base.canonicalize().unwrap().display().to_string();
    let at = |name: &str| format!("{dir}/{name}");

    r.create(&at("new file.txt"), NewEntry::File).unwrap();
    assert!(base.join("new file.txt").is_file());
    let again = r.create(&at("new file.txt"), NewEntry::File).unwrap_err();
    assert!(again.to_string().contains("already exists"), "{again}");
    r.create(&at("sub"), NewEntry::Dir).unwrap();
    assert!(base.join("sub").is_dir());
    assert!(r.create(&at("sub"), NewEntry::Dir).is_err());

    // Names with a space, a quote and a `$` stay names.
    r.rename(&at("new file.txt"), &at("it's $HOME.txt"))
        .unwrap();
    assert!(base.join("it's $HOME.txt").is_file());
    std::fs::write(base.join("other.txt"), "x").unwrap();
    let taken = r
        .rename(&at("other.txt"), &at("it's $HOME.txt"))
        .unwrap_err();
    assert!(taken.to_string().contains("already exists"), "{taken}");
    assert!(
        r.rename(&at("other.txt"), &at("missing/other.txt"))
            .is_err()
    );
    r.rename(&at("other.txt"), &at("sub/other.txt")).unwrap();
    assert!(base.join("sub/other.txt").is_file());

    assert_eq!(r.count_entries(&at("sub")).unwrap(), 1);
    assert_eq!(r.count_entries(&at("it's $HOME.txt")).unwrap(), 0);
    r.delete(&at("it's $HOME.txt"), false).unwrap();
    assert!(!base.join("it's $HOME.txt").exists());
    let full = r.delete(&at("sub"), false).unwrap_err();
    assert!(full.to_string().contains("not empty"), "{full}");
    assert!(base.join("sub/other.txt").exists());
    // A link to a folder is removed, not followed.
    std::os::unix::fs::symlink(base.join("sub"), base.join("link")).unwrap();
    r.delete(&at("link"), true).unwrap();
    assert!(base.join("sub/other.txt").exists());
    r.delete(&at("sub"), true).unwrap();
    assert!(!base.join("sub").exists());
    assert!(r.delete(&at("gone"), false).is_err());

    // The home folder is checked without ever sending a delete for it: the test host's home is
    // the real one.
    let home = r.home().unwrap();
    assert!(ferris_idea_remote::protects_home(&home, &home));
    assert!(ferris_idea_remote::protects_home(
        &home,
        &format!("{home}/..")
    ));
    assert!(r.delete("/", true).is_err());
    assert!(r.delete("relative", true).is_err());
}

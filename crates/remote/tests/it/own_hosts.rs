//! Ferris IDEA's own SSH hosts against a real `sshd` (`SPEC-0121` v5 AK-8): a host added to the list
//! in a settings folder of the test's own is listed with its source, resolved by `ssh -G` and
//! reached through the one start point with `-F <Ferris IDEA's file>`; an unknown host key is still
//! refused; removing it takes it off the list. The home folder whose `~/.ssh/config` the file
//! includes is the test's own too, so the user's `~/.ssh` and settings folder are never read or
//! written.

use std::time::Duration;

use ferris_idea_remote::testing::TestHost;
use ferris_idea_remote::{HostFiles, HostSource, OwnHost, Remote, RemoteError, SshTarget};

#[test]
fn an_own_host_is_listed_reached_and_removed() {
    let base = std::env::temp_dir().join(format!("idc-own-hosts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    let Some(h) = TestHost::start(&base.join("sshd")) else {
        eprintln!("skipped: no sshd/ssh-keygen");
        return;
    };
    let config = std::fs::read_to_string(h.dir.join("ssh_config")).unwrap();
    let port: u16 = config
        .lines()
        .find_map(|l| l.trim().strip_prefix("Port "))
        .and_then(|p| p.trim().parse().ok())
        .unwrap();
    // The test's own home folder: its ~/.ssh/config gives every host the test's known hosts and
    // no agent, as a user's `Host *` defaults would.
    let home = base.join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(
        home.join(".ssh/config"),
        format!(
            "Host *\n  UserKnownHostsFile {}\n  IdentityAgent none\n",
            h.dir.join("known_hosts").display()
        ),
    )
    .unwrap();
    let files = HostFiles::in_folders(&base.join("settings"), &home);
    let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
    let add = |alias: &str, hostname: &str| {
        ferris_idea_remote::add_host(
            &files,
            &OwnHost {
                alias: alias.into(),
                hostname: hostname.into(),
                port: Some(port),
                user: Some(user.clone()),
                identity_file: Some(h.dir.join("client_key").display().to_string()),
            },
            false,
        )
        .unwrap()
    };
    add("ferris-idea-own-it", "127.0.0.1");
    // The same server under a name its known hosts do not hold.
    add("ferris-idea-own-unknown", "localhost");
    ferris_idea_remote::set_host_files(Some(files.clone()));

    let list = ferris_idea_remote::host_list(&[]);
    let own = list
        .iter()
        .find(|i| i.alias == "ferris-idea-own-it")
        .expect("the own host is listed");
    assert_eq!(own.source, HostSource::FerrisIdea);
    assert_eq!(
        own.hostname.as_deref(),
        Some("127.0.0.1"),
        "ssh -G resolved it"
    );
    assert_eq!(own.port, Some(port));
    assert_eq!(own.user.as_deref(), Some(user.as_str()));
    assert_eq!(
        own.config.as_deref(),
        Some(files.own.display().to_string().as_str())
    );

    // Reached through the start point, with the shared options and Ferris IDEA's file.
    let r = Remote::new(SshTarget::parse("ferris-idea-own-it", &[]));
    let out = r
        .exec(
            &["echo".into(), "reached".into()],
            None,
            Duration::from_secs(120),
        )
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "reached");

    // Host key checking stays strict: an unknown key is refused as before.
    let unknown = Remote::new(SshTarget::parse("ferris-idea-own-unknown", &[]));
    match unknown.exec(&["true".into()], None, Duration::from_secs(120)) {
        Err(RemoteError::HostKeyUnknown(_)) => {}
        other => panic!("an unknown host key must be refused: {other:?}"),
    }

    ferris_idea_remote::remove_host(&files, "ferris-idea-own-it", false).unwrap();
    ferris_idea_remote::remove_host(&files, "ferris-idea-own-unknown", false).unwrap();
    assert!(
        !ferris_idea_remote::host_list(&[])
            .iter()
            .any(|i| i.alias.starts_with("ferris-idea-own-"))
    );
    ferris_idea_remote::set_host_files(None);
    assert!(files.own.starts_with(&base) && files.user.starts_with(&base));
    drop(h);
    let _ = std::fs::remove_dir_all(&base);
}

//! Jump hosts with the post-quantum key exchange on every hop (ADR-0025 v5 section 1a), against
//! real `sshd`s started for the test: a post-quantum hop, a classical hop and two post-quantum
//! targets. One test function, because the setting `ssh.allowOwnProxyCommand` is process-wide.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use idecraft_remote::testing::TestHost;
use idecraft_remote::{Remote, RemoteError, SshCommand, SshPurpose, SshTarget};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("idc-jump-{name}-{}", std::process::id()))
}

/// The client block of `h` under `alias`, with `more` lines.
fn block(h: &TestHost, alias: &str, more: &[&str]) -> String {
    let mut b = std::fs::read_to_string(h.dir.join("ssh_config"))
        .unwrap()
        .replace("Host idecraft-test", &format!("Host {alias}"));
    for m in more {
        b.push_str(&format!("  {m}\n"));
    }
    b
}

/// One connection through the start point to `alias` that runs `true`, without a shared
/// connection (so no earlier success is reused); the refusal classified like every remote call.
fn connect(config: &Path, alias: &str, extra: &[&str]) -> Result<Output, RemoteError> {
    let mut args = vec!["-F".to_owned(), config.display().to_string()];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let out = SshCommand::new(SshPurpose::Host)
        .option("ControlMaster=no")
        .option("ControlPath=none")
        .extra(&args)
        .remote(["true"])
        .build(alias)?
        .command()
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| RemoteError::NoSsh(e.to_string()))?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(idecraft_remote::classify(
            alias,
            &String::from_utf8_lossy(&out.stderr),
        ))
    }
}

/// Runs a stand-in (Git's `GIT_SSH_COMMAND` or the Compose CLI's `ssh`) to `alias`.
fn stand_in(command: &str, config: &Path, alias: &str) -> Output {
    Command::new("sh")
        .arg("-c")
        .arg(format!("{command} -F \"$1\" -- \"$2\" true"))
        .args(["sh", &config.display().to_string(), alias])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn every_hop_needs_the_post_quantum_key_exchange() {
    let base = scratch("all");
    let _ = std::fs::remove_dir_all(&base);
    let (Some(a), Some(c), Some(t), Some(t2)) = (
        TestHost::start(&base.join("hop-pq")),
        TestHost::start_classical(&base.join("hop-classic")),
        TestHost::start(&base.join("target")),
        TestHost::start(&base.join("target2")),
    ) else {
        eprintln!("skipped: no sshd/ssh-keygen");
        return;
    };
    if SshCommand::check_client("ssh").is_err() {
        eprintln!("skipped: the local ssh has no post-quantum key exchange");
        return;
    }
    idecraft_remote::testing::redirect_logs(&base.join("logs"));
    let config = base.join("ssh_config");
    let me = |alias: &str| -> String {
        // The user's own ProxyCommand: plain OpenSSH through the post-quantum hop.
        format!(
            "ProxyCommand ssh -F {} -o BatchMode=yes -W %h:%p {alias}",
            config.display()
        )
    };
    let text = [
        block(&a, "idc-a", &[]),
        block(&c, "idc-c", &[]),
        block(&a, "idc-a-via-c", &["ProxyJump idc-c"]),
        block(&a, "idc-a-own", &[&me("idc-a")]),
        block(&t, "idc-t", &[]),
        block(&t, "idc-t-c", &["ProxyJump idc-c"]),
        block(&t, "idc-t-ac", &["ProxyJump idc-a,idc-c"]),
        block(&t, "idc-t-ca", &["ProxyJump idc-c,idc-a"]),
        block(&t, "idc-t-avc", &["ProxyJump idc-a-via-c"]),
        block(&t, "idc-t-aown", &["ProxyJump idc-a-own"]),
        block(&t, "idc-t-a", &["ProxyJump idc-a"]),
        block(&t, "idc-t-aa", &["ProxyJump idc-a,idc-a"]),
        block(&t, "idc-t-none", &["ProxyJump none"]),
        block(&t2, "idc-t2-own", &[&me("idc-a")]),
    ]
    .concat();
    std::fs::write(&config, text).unwrap();

    // Without IDECraft, OpenSSH opens the classical hop with its defaults and reaches the target:
    // the gap this closes.
    let plain = Command::new("ssh")
        .args(["-F", &config.display().to_string(), "-o", "BatchMode=yes"])
        .args(["-o", "ControlMaster=no", "--", "idc-t-c", "true"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        plain.status.success(),
        "{}",
        String::from_utf8_lossy(&plain.stderr)
    );

    // A classical hop is refused, wherever it sits in the chain and however it is named.
    let marker = base.join("ran");
    let remote = Remote::new(SshTarget {
        host: "idc-t-c".into(),
        port: None,
        extra: vec!["-F".into(), config.display().to_string()],
        program: None,
    });
    let e = remote
        .sh(
            "touch \"$1\"",
            &[&marker.display().to_string()],
            None,
            idecraft_remote::TIMEOUT,
        )
        .unwrap_err();
    assert_eq!(
        e,
        RemoteError::HopNoPqKex("idc-c".into(), "idc-t-c".into()),
        "{e:?}"
    );
    let text = e.to_string();
    assert!(text.contains("jump host idc-c"), "{text}");
    assert!(text.contains("ADR-0025"), "{text}");
    assert!(!marker.exists(), "nothing may run behind a refused hop");
    for alias in ["idc-t-ac", "idc-t-ca", "idc-t-avc"] {
        let e = connect(&config, alias, &[]).unwrap_err();
        assert!(
            matches!(&e, RemoteError::HopNoPqKex(_, host) if host == alias),
            "{alias}: {e:?}"
        );
    }
    // `-J` on the command line is resolved the same way.
    let e = connect(&config, "idc-t", &["-J", "idc-c"]).unwrap_err();
    assert!(matches!(e, RemoteError::HopNoPqKex(..)), "{e:?}");

    // A ProxyCommand of the user's own is refused by default, for the target and for a hop.
    let e = connect(&config, "idc-t2-own", &[]).unwrap_err();
    assert_eq!(e, RemoteError::ProxyCommandRefused("idc-t2-own".into()));
    let e = connect(&config, "idc-t-aown", &[]).unwrap_err();
    assert_eq!(e, RemoteError::ProxyCommandRefused("idc-a-own".into()));

    // Git and the Compose CLI start ssh themselves: their stand-ins refuse the classical hop too.
    let git = SshCommand::git_ssh_command();
    let compose = SshCommand::stand_in_dir()
        .unwrap()
        .join("ssh")
        .display()
        .to_string();
    for command in [&git, &idecraft_remote::quote(&compose)] {
        let out = stand_in(command, &config, "idc-t-c");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{command}: {stderr}");
        assert!(
            stderr.contains("no matching key exchange method"),
            "{command}: {stderr}"
        );
        let out = stand_in(command, &config, "idc-t2-own");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{command}: {stderr}");
        // Named by the host it resolves to.
        assert!(
            stderr.contains("idecraft-ssh-hop: refused 127.0.0.1: ProxyCommand"),
            "{command}: {stderr}"
        );
    }

    // Post-quantum hops are accepted, one or several, by the start point and by the stand-ins.
    for alias in ["idc-t-a", "idc-t-aa", "idc-t-none"] {
        connect(&config, alias, &[]).unwrap_or_else(|e| panic!("{alias}: {e}"));
    }
    connect(&config, "idc-t", &["-J", "idc-a"]).unwrap();
    for command in [&git, &idecraft_remote::quote(&compose)] {
        let out = stand_in(command, &config, "idc-t-aa");
        assert!(
            out.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let remote = Remote::new(SshTarget {
        host: "idc-t-a".into(),
        port: None,
        extra: vec!["-F".into(), config.display().to_string()],
        program: None,
    });
    assert!(remote.home().unwrap().starts_with('/'));

    // The setting allows a ProxyCommand of the user's own, with a log line.
    idecraft_remote::set_own_proxy_command(true);
    let own = connect(&config, "idc-t2-own", &[]);
    let hop_own = connect(&config, "idc-t-aown", &[]);
    idecraft_remote::set_own_proxy_command(false);
    own.unwrap();
    hop_own.unwrap();
    let log = std::fs::read_to_string(
        base.join("logs")
            .join(idecraft_remote::OWN_PROXY_COMMAND_LOG),
    )
    .unwrap();
    assert!(log.contains("host=idc-t2-own"), "{log}");
    assert!(
        !log.contains("ProxyCommand ssh"),
        "the command itself is not logged"
    );
    // Off again: refused again.
    assert_eq!(
        connect(&config, "idc-t2-own", &[]).unwrap_err(),
        RemoteError::ProxyCommandRefused("idc-t2-own".into())
    );
    drop((a, c, t, t2));
    let _ = std::fs::remove_dir_all(&base);
}

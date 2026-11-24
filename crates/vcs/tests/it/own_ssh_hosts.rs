//! Git reaches Ferris IDEA's own SSH hosts (`SPEC-0055` v5 AK-8, `SPEC-0121` v6 AK-9): against the
//! test `sshd`, with a settings folder and a home folder of the test's own, a clone from
//! `ssh://<alias>/<path>`, a push and a fetch to `user@<alias>:<path>` through Ferris IDEA's Git
//! operations reach a repository on the host; an own alias whose host key is unknown fails in Git
//! with `auth_required`, and the start point's refusal names `ssh -F '<file>' <alias>`. The user's
//! `~/.ssh` and settings folder are never read or written: Git's stand-in asks for the destination
//! with `-F /dev/null` and reaches own aliases with Ferris IDEA's file, which includes the test's home.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ferris_idea_remote::testing::TestHost;
use ferris_idea_remote::{HostFiles, OwnHost, Remote, RemoteError, SshTarget};
use ferris_idea_vcs::{Repo, check_clone_url, clone_repository};

const WAIT: Duration = Duration::from_secs(120);

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn configure(dir: &Path) {
    git(dir, &["config", "user.name", "Test Author"]);
    git(dir, &["config", "user.email", "test@example.invalid"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

#[test]
fn git_reaches_an_own_ssh_host() {
    let base = std::env::temp_dir().join(format!("idc-git-own-{}", std::process::id()));
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
    add("ferris-idea-own-git", "127.0.0.1");
    // The same server under a name its known hosts do not hold.
    add("ferris-idea-own-git-unknown", "localhost");
    ferris_idea_remote::set_host_files(Some(files.clone()));

    // The repository "on the host" (the test host runs commands on this machine).
    let server = base.join("server");
    std::fs::create_dir_all(&server).unwrap();
    git(
        &server,
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    git(&server, &["clone", "-q", "remote.git", "seed"]);
    let seed = server.join("seed");
    configure(&seed);
    git(&seed, &["checkout", "-q", "-B", "main"]);
    commit(&seed, "readme.txt", "one\n");
    git(&seed, &["push", "-q", "origin", "main"]);
    let bare = server.join("remote.git");

    // Clone from ssh://<alias>/<path> through Ferris IDEA's clone.
    let url = format!("ssh://ferris-idea-own-git{}", bare.display());
    let source = check_clone_url(&url).unwrap();
    let target = base.join("clone");
    let cloned = clone_repository(&source, &target, &AtomicBool::new(false), WAIT, |_| {});
    assert!(cloned.is_ok(), "clone over an own alias: {cloned:?}");
    assert_eq!(
        std::fs::read_to_string(target.join("readme.txt")).unwrap(),
        "one\n"
    );
    configure(&target);

    // Push and fetch with the scp form user@<alias>:<path>.
    git(
        &target,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("{user}@ferris-idea-own-git:{}", bare.display()),
        ],
    );
    commit(&target, "two.txt", "two\n");
    let repo = Repo::discover(&target).unwrap();
    let pushed = repo.push(None, None, false, None, WAIT).unwrap();
    assert!(pushed.ok, "push over an own alias: {pushed:?}");
    let tip = git(&target, &["rev-parse", "HEAD"]);
    assert_eq!(git(&bare, &["rev-parse", "main"]), tip);

    git(&seed, &["pull", "-q", "origin", "main"]);
    commit(&seed, "three.txt", "three\n");
    git(&seed, &["push", "-q", "origin", "main"]);
    let fetched = repo.fetch(Some("origin"), WAIT).unwrap();
    assert!(fetched.ok, "fetch over an own alias: {fetched:?}");
    assert_eq!(
        git(&target, &["rev-parse", "origin/main"]),
        git(&bare, &["rev-parse", "main"])
    );

    // Host keys stay strict: an own alias whose key is unknown fails in Git as auth_required.
    git(
        &target,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("ssh://ferris-idea-own-git-unknown{}", bare.display()),
        ],
    );
    let refused = repo.fetch(Some("origin"), WAIT).unwrap();
    assert!(!refused.ok, "{refused:?}");
    assert!(refused.auth_required, "{refused:?}");
    assert!(
        refused
            .messages
            .to_lowercase()
            .contains("host key verification failed"),
        "{refused:?}"
    );
    // The start point's refusal names the terminal command that reaches the alias.
    let unknown = Remote::new(SshTarget::parse("ferris-idea-own-git-unknown", &[]));
    match unknown.exec(&["true".into()], None, WAIT) {
        Err(e @ RemoteError::HostKeyUnknown(_)) => {
            let file = files.own.display().to_string();
            let text = e.to_string();
            assert!(
                text.contains(&format!("`ssh -F '{file}' ferris-idea-own-git-unknown`")),
                "{text}"
            );
        }
        other => panic!("an unknown host key must be refused: {other:?}"),
    }

    ferris_idea_remote::set_host_files(None);
    assert!(files.own.starts_with(&base) && files.user.starts_with(&base));
    drop(h);
    let _ = std::fs::remove_dir_all(&base);
}

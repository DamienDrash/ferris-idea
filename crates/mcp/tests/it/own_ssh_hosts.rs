//! `SPEC-0121` v5 AK-8, MCP part: Ferris IDEA's own SSH hosts. `remote_hosts` names each host's
//! source; `remote_host_add` checks every value in a dry run that writes nothing, and applying it
//! is refused without the user-granted `settings` and `ssh` rights; `remote_host_remove` needs
//! `settings` and never removes a host of `~/.ssh/config`. The host files lie in the test's own
//! folder (never the user's `~/.ssh` or settings folder).

use ferris_idea_core::Grant;
use serde_json::json;

use super::protocol::{connect_exact, error_message, is_error, json_text};

#[tokio::test]
async fn own_hosts_are_added_and_removed_only_with_the_users_rights() {
    let base =
        std::env::temp_dir().join(format!("ferris-idea-mcp-own-hosts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home/.ssh")).unwrap();
    let base = base.canonicalize().unwrap();
    let files =
        ferris_idea_remote::HostFiles::in_folders(&base.join("settings"), &base.join("home"));
    // No alias of the user's own yet: listing it would ask `ssh -G` without a test config.
    std::fs::write(&files.user, "Host *\n  IdentityAgent none\n").unwrap();
    ferris_idea_remote::set_host_files(Some(files.clone()));
    let add = json!({"alias": "test-vm", "hostname": "192.0.2.7", "port": 2222, "user": "dev"});

    // A dry run needs no right and writes nothing.
    let (mut none, _) = connect_exact(&[], |s, _| s).await;
    let dry = json_text(&none.call("remote_host_add", add.clone()).await);
    assert_eq!(dry["applied"], false, "{dry}");
    assert_eq!(dry["host"]["alias"], "test-vm");
    assert_eq!(dry["config"], files.own.display().to_string());
    assert!(!files.own.exists());
    for (field, value) in [
        ("alias", "-oProxyCommand=sh"),
        ("alias", "a b"),
        ("hostname", "h\nProxyCommand sh"),
        ("user", "-l"),
    ] {
        let mut bad = add.clone();
        bad[field] = json!(value);
        let r = none.call("remote_host_add", bad).await;
        assert!(is_error(&r), "{field} {value:?}: {r}");
    }

    // Applying it: refused without `settings`, then without `ssh`; nothing is written.
    let mut apply = add.clone();
    apply["dry_run"] = json!(false);
    let r = none.call("remote_host_add", apply.clone()).await;
    assert!(error_message(&r).contains("`settings`"), "{r}");
    let (mut settings_only, _) = connect_exact(&[Grant::Settings], |s, _| s).await;
    let r = settings_only.call("remote_host_add", apply.clone()).await;
    assert!(error_message(&r).contains("`ssh`"), "{r}");
    assert!(!files.own.exists(), "a refused change writes nothing");

    // With both rights it is added, and listed with its source.
    let (mut both, root2) = connect_exact(&[Grant::Settings, Grant::Ssh], |s, _| s).await;
    let added = json_text(&both.call("remote_host_add", apply.clone()).await);
    assert_eq!(added["applied"], true, "{added}");
    assert!(files.own.is_file());
    let r = both.call("remote_host_add", apply.clone()).await;
    assert!(error_message(&r).contains("already a host"), "{r}");
    let listed = json_text(&both.call("remote_hosts", json!({})).await);
    let host = &listed["hosts"][0];
    assert_eq!(host["alias"], "test-vm", "{listed}");
    assert_eq!(host["source"], "ferris-idea");
    assert_eq!(host["config"], files.own.display().to_string());

    // Removing: refused without `settings`; a host of ~/.ssh/config is never removed.
    std::fs::write(&files.user, "Host theirs\n  HostName 192.0.2.8\n").unwrap();
    let user_before = std::fs::read(&files.user).unwrap();
    let remove = json!({"alias": "test-vm", "dry_run": false});
    let (mut ssh_only, _) = connect_exact(&[Grant::Ssh], |s, _| s).await;
    let r = ssh_only.call("remote_host_remove", remove.clone()).await;
    assert!(error_message(&r).contains("`settings`"), "{r}");
    let r = both
        .call(
            "remote_host_remove",
            json!({"alias": "theirs", "dry_run": false}),
        )
        .await;
    assert!(error_message(&r).contains("never changes"), "{r}");
    let dry = json_text(
        &both
            .call("remote_host_remove", json!({"alias": "test-vm"}))
            .await,
    );
    assert_eq!(dry["applied"], false);
    assert!(
        std::fs::read_to_string(&files.own)
            .unwrap()
            .contains("Host test-vm")
    );
    let done = json_text(&both.call("remote_host_remove", remove).await);
    assert_eq!(done["applied"], true, "{done}");
    assert!(
        !std::fs::read_to_string(&files.own)
            .unwrap()
            .contains("Host test-vm")
    );
    assert_eq!(std::fs::read(&files.user).unwrap(), user_before);

    let log = std::fs::read_to_string(root2.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(log.contains("\"tool\":\"remote_host_add\"") && log.contains("\"alias\":\"test-vm\""));
    assert!(log.contains("\"tool\":\"remote_host_remove\""));
    ferris_idea_remote::set_host_files(None);
    let _ = std::fs::remove_dir_all(&base);
}

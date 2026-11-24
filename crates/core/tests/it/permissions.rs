//! SPEC-0053 AK-1, AK-2, AK-4.

use std::path::PathBuf;

use ferris_idea_core::{AGENT_LOG, Grant, load_agent_permissions, log_agent_action};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("ferris-idea-perm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("ws")).unwrap();
        std::fs::create_dir_all(p.join("config")).unwrap();
        Self(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn grants_per_workspace_from_the_user_file() {
    let s = Scratch::new("grants");
    let ws = s.0.join("ws");
    let file = s.0.join("config/agent-permissions.toml");
    // No file: nothing granted, no problem.
    let p = load_agent_permissions(&file, &ws);
    assert!(p.allow.is_empty() && p.problem.is_none());

    std::fs::create_dir_all(s.0.join("other")).unwrap();
    // The workspace is named through a path with `..`; it still matches after resolving.
    std::fs::write(
        &file,
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"run\"]\n\n[[workspace]]\npath = \"{}/other/../ws\"\nallow = [\"http\"]\n\n[[workspace]]\npath = \"{}\"\nallow = [\"debug\"]\n",
            ws.display(),
            s.0.display(),
            s.0.join("other").display()
        ),
    )
    .unwrap();
    let p = load_agent_permissions(&file, &ws);
    assert!(p.allows(Grant::Run) && p.allows(Grant::Http));
    assert!(
        !p.allows(Grant::Debug),
        "another workspace's grant must not apply"
    );

    let refusal = p.refusal(Grant::Debug, &ws);
    assert!(refusal.contains("`debug` right"));
    assert!(refusal.contains(&file.display().to_string()));
    assert!(refusal.contains(&format!("path = \"{}\"", ws.display())));

    // A malformed file grants nothing and says why.
    std::fs::write(&file, "[[workspace]]\npath = 1\n").unwrap();
    let p = load_agent_permissions(&file, &ws);
    assert!(p.allow.is_empty());
    assert!(p.problem.is_some());
    // Unknown rights are rejected too.
    std::fs::write(
        &file,
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"root\"]\n",
            ws.display()
        ),
    )
    .unwrap();
    assert!(load_agent_permissions(&file, &ws).problem.is_some());
}

#[test]
fn a_grant_file_inside_the_workspace_is_ignored() {
    let s = Scratch::new("inside");
    let ws = s.0.join("ws");
    let file = ws.join(".ferris-idea/agent-permissions.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"run\"]\n",
            ws.display()
        ),
    )
    .unwrap();
    let p = load_agent_permissions(&file, &ws);
    assert!(!p.allows(Grant::Run));
    assert_eq!(
        p.problem.as_deref(),
        Some("the grant file lies inside the workspace and is ignored")
    );
}

#[test]
fn actions_are_appended_to_the_log() {
    let s = Scratch::new("log");
    let ws = s.0.join("ws");
    log_agent_action(&ws, &serde_json::json!({"tool": "a"})).unwrap();
    log_agent_action(&ws, &serde_json::json!({"tool": "b"})).unwrap();
    let text = std::fs::read_to_string(ws.join(AGENT_LOG)).unwrap();
    assert_eq!(text, "{\"tool\":\"a\"}\n{\"tool\":\"b\"}\n");
}

/// SPEC-0053 v4 AK-6/AK-7: the new rights round-trip through the file; the answer "Always for This
/// Workspace" appends an entry and keeps the rest; refusals say "not granted" and nobody was asked.
#[test]
fn write_rights_and_always_for_this_workspace() {
    use ferris_idea_core::add_grant;
    let s = Scratch::new("always");
    let ws = s.0.join("ws");
    let file = s.0.join("config/agent-permissions.toml");
    std::fs::write(
        &file,
        format!(
            "# my grants\n[[workspace]]\npath = \"{}\"\nallow = [\"vcs-write\"]",
            ws.display()
        ),
    )
    .unwrap();
    let p = load_agent_permissions(&file, &ws);
    assert!(p.problem.is_none(), "{:?}", p.problem);
    assert!(p.allows(Grant::VcsWrite) && !p.allows(Grant::Write));

    let refusal = p.refusal(Grant::Write, &ws);
    assert!(refusal.starts_with("not granted:"), "{refusal}");
    assert!(refusal.contains("nobody was asked"));
    assert!(refusal.contains("allow = [\"write\"]"));
    assert!(!refusal.contains("rejected") && !refusal.contains("denied"));

    add_grant(&file, &ws, Grant::Write).unwrap();
    add_grant(&file, &ws, Grant::Quit).unwrap();
    // Already granted: nothing is appended.
    add_grant(&file, &ws, Grant::Write).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("# my grants\n"), "{text}");
    assert_eq!(text.matches("[[workspace]]").count(), 3, "{text}");
    let p = load_agent_permissions(&file, &ws);
    assert!(p.allows(Grant::Write) && p.allows(Grant::VcsWrite) && p.allows(Grant::Quit));
    assert!(!p.allows(Grant::Run));

    // A grant file inside the workspace is never written.
    let inside = ws.join(".ferris-idea/agent-permissions.toml");
    assert!(add_grant(&inside, &ws, Grant::Write).is_err());
    assert!(!inside.exists());
}

/// SPEC-0053 v8 AK-14 and SPEC-0012 v3 AK-8: database permissions are kept in the grant file per
/// workspace, connection and target; `settings` is a right like the others.
#[test]
fn connection_grants_are_kept_per_workspace_connection_and_target() {
    use ferris_idea_core::{
        add_connection_grant, add_grant, connection_grant_entry, connection_grants,
        connection_target,
    };
    let s = Scratch::new("connections");
    let ws = s.0.join("ws");
    let other = s.0.join("other-ws");
    std::fs::create_dir_all(&other).unwrap();
    let file = s.0.join("config/agent-permissions.toml");

    // The target changes with every part that decides where a connection leads.
    let parts = |host: &str, script: &str| vec![host.to_owned(), script.to_owned()];
    let target = connection_target("postgresql://app@db/shop", &parts("db", ""));
    assert!(target.starts_with("postgresql://app@db/shop#"), "{target}");
    assert_eq!(target.len(), "postgresql://app@db/shop#".len() + 16);
    assert_eq!(
        target,
        connection_target("postgresql://app@db/shop", &parts("db", ""))
    );
    assert_ne!(
        target,
        connection_target("postgresql://app@db/shop", &parts("db", "set role admin"))
    );
    // Parts are not run together: ("ab", "c") is not ("a", "bc").
    assert_ne!(
        connection_target("x", &parts("ab", "c")),
        connection_target("x", &parts("a", "bc"))
    );

    // Nothing without a file; "Always for This Connection" appends an entry and keeps the rest.
    assert!(connection_grants(&file, &ws).grants.is_empty());
    add_grant(&file, &ws, Grant::Settings).unwrap();
    add_connection_grant(&file, &ws, "shop", &target, &["read-schema", "read-data"]).unwrap();
    add_connection_grant(&file, &ws, "shop", &target, &["read-data", "modify-data"]).unwrap();
    // Already granted: nothing is appended.
    add_connection_grant(&file, &ws, "shop", &target, &["read-data"]).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.matches("[[connection]]").count(), 2, "{text}");
    assert!(text.contains("allow = [\"modify-data\"]"), "{text}");
    let granted = connection_grants(&file, &ws);
    assert!(granted.problem.is_none(), "{:?}", granted.problem);
    let allowed: Vec<String> = granted.allowed("shop", &target).into_iter().collect();
    assert_eq!(allowed, ["modify-data", "read-data", "read-schema"]);
    // Another connection, another target and another workspace get nothing.
    assert!(granted.allowed("other", &target).is_empty());
    assert!(
        granted
            .allowed("shop", "postgresql://app@evil/shop#0000000000000000")
            .is_empty()
    );
    assert!(connection_grants(&file, &other).grants.is_empty());
    // The rights of the same file are read as before, `settings` among them.
    let rights = load_agent_permissions(&file, &ws);
    assert!(rights.problem.is_none(), "{:?}", rights.problem);
    assert!(rights.allows(Grant::Settings) && !rights.allows(Grant::Write));
    assert!(
        rights
            .refusal(Grant::Settings, &ws)
            .contains("allow = [\"settings\"]")
    );
    // The entry a refusal names is the one that is written.
    let entry = connection_grant_entry(&ws, "shop", &target, &["read-schema"]);
    assert!(entry.starts_with("[[connection]]\nworkspace = "), "{entry}");
    assert!(entry.contains("connection = \"shop\""), "{entry}");

    // An unknown permission is refused when writing, and in a file it grants nothing at all.
    assert!(add_connection_grant(&file, &ws, "shop", &target, &["everything"]).is_err());
    std::fs::write(
        &file,
        format!("{text}\n[[connection]]\nworkspace = \"{}\"\nconnection = \"x\"\ntarget = \"t\"\nallow = [\"everything\"]\n", ws.display()),
    )
    .unwrap();
    let broken = connection_grants(&file, &ws);
    assert!(broken.grants.is_empty());
    assert!(broken.problem.unwrap().contains("everything"));
    // A grant file inside the workspace is ignored and never written.
    let inside = ws.join(".ferris-idea/agent-permissions.toml");
    assert!(add_connection_grant(&inside, &ws, "shop", &target, &["read-data"]).is_err());
    assert!(!inside.exists());
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    std::fs::write(
        &inside,
        connection_grant_entry(&ws, "shop", &target, &["read-data"]),
    )
    .unwrap();
    let ignored = connection_grants(&inside, &ws);
    assert!(ignored.grants.is_empty() && ignored.problem.is_some());
}

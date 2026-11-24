//! Passwords of the local reference servers for the live tests. Like `oracle.rs` and `sqlserver.rs`:
//! `FERRIS_IDEA_TEST_<X>_PASSWORD`, or a file named by `FERRIS_IDEA_TEST_<X>_PASSWORD_FILE`
//! (`scripts/ci-local.sh` sets these), where `<X>` is `PG`, `MYSQL`, `MARIADB` or `SQLSERVER` after the
//! profile's dialect. Without one the server is expected to need no password (GitHub's service
//! containers).

use ferris_idea_database::{ConnectionProfile, Credentials, Dialect, Secret};

/// The credentials for `profile`: its server's test password when one is set, else none needed.
pub fn credentials(profile: &ConnectionProfile) -> Credentials {
    let var = match profile.dialect {
        Dialect::PostgreSql => "FERRIS_IDEA_TEST_PG",
        Dialect::MySql => "FERRIS_IDEA_TEST_MYSQL",
        Dialect::MariaDb => "FERRIS_IDEA_TEST_MARIADB",
        Dialect::SqlServer => "FERRIS_IDEA_TEST_SQLSERVER",
        _ => return Credentials::NotNeeded,
    };
    match password(var) {
        Some(password) => Credentials::Available(Secret::new(password)),
        None => Credentials::NotNeeded,
    }
}

/// `<var>_PASSWORD`, else the content of the file named by `<var>_PASSWORD_FILE`. A named file that
/// cannot be read fails the test instead of silently connecting without a password.
fn password(var: &str) -> Option<String> {
    if let Ok(password) = std::env::var(format!("{var}_PASSWORD"))
        && !password.is_empty()
    {
        return Some(password);
    }
    let file = std::env::var(format!("{var}_PASSWORD_FILE")).ok()?;
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{var}_PASSWORD_FILE {file}: {e}"));
    Some(text.trim().to_owned())
}

/// Whether `name` is a throwaway namespace `<prefix>[<word>_…]<pid>[_<word>…]` of a test process that has
/// ended: not this one, and none `kill -0` still finds. A name without such a number, or of a
/// process still running (another worktree's run), is no leftover.
pub fn stale_namespace(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    let parts: Vec<&str> = rest.split('_').collect();
    if !parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return false;
    }
    // The process id is the last number; words may follow it (`…_<pid>_b`).
    let Some(pid) = parts
        .iter()
        .rev()
        .find(|p| p.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|p| p.parse::<u32>().ok())
    else {
        return false;
    };
    pid != std::process::id()
        && !std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

/// Drops the throwaway schemas (PostgreSQL) or databases (MySQL, MariaDB) with one of `prefixes`
/// that runs ended early left behind (an assertion that fails skips the drop at the end), before a
/// test makes its own (OBS-0083 L11). Only names of [`stale_namespace`]; nothing else is touched.
pub async fn drop_leftovers(s: &ferris_idea_database::Session, prefixes: &[&str]) {
    use ferris_idea_database::{OnError, Outcome};
    let (list, drop): (&str, fn(&str) -> String) = match s.dialect() {
        Dialect::PostgreSql => ("select nspname from pg_namespace", |n| {
            format!("drop schema if exists {n} cascade")
        }),
        Dialect::MySql | Dialect::MariaDb => {
            ("select schema_name from information_schema.schemata", |n| {
                format!("drop database if exists {n}")
            })
        }
        _ => return,
    };
    let names: Vec<String> = s
        .run_script(list, OnError::Stop)
        .await
        .outcomes
        .into_iter()
        .flat_map(|o| match o.outcome {
            Outcome::ResultSet { rows, .. } => rows
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    for name in names {
        if prefixes.iter().any(|p| stale_namespace(&name, p)) {
            let _ = s.run_script(&drop(&name), OnError::Continue).await;
        }
    }
}

#[test]
fn stale_namespaces_are_only_those_of_ended_runs() {
    let mine = std::process::id();
    assert!(!stale_namespace(
        &format!("ferris_idea_cat_{mine}"),
        "ferris_idea_cat_"
    ));
    assert!(!stale_namespace("ferris_idea_cat_x", "ferris_idea_cat_"));
    assert!(!stale_namespace("inventory", "ferris_idea_cat_"));
    assert!(!stale_namespace(
        "ferris_idea_catalog_5",
        "ferris_idea_cat_"
    ));
    // 2^31 - 2 is never a running process.
    assert!(stale_namespace(
        "ferris_idea_cat_2147483646",
        "ferris_idea_cat_"
    ));
    assert!(stale_namespace("m57_mig_src_2147483646", "m57_"));
    assert!(!stale_namespace("m57_mig__2147483646", "m57_"));
    assert!(stale_namespace(
        "ferris_idea_d4_2147483646_b",
        "ferris_idea_d4_"
    ));
    assert!(!stale_namespace("ferris_idea_d4_b", "ferris_idea_d4_"));
}

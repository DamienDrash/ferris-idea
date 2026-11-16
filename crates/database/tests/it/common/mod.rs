//! Passwords of the local reference servers for the live tests. Like `oracle.rs` and `sqlserver.rs`:
//! `IDECRAFT_TEST_<X>_PASSWORD`, or a file named by `IDECRAFT_TEST_<X>_PASSWORD_FILE`
//! (`scripts/ci-local.sh` sets these), where `<X>` is `PG`, `MYSQL` or `MARIADB` after the
//! profile's dialect. Without one the server is expected to need no password (GitHub's service
//! containers).

use idecraft_database::{ConnectionProfile, Credentials, Dialect, Secret};

/// The credentials for `profile`: its server's test password when one is set, else none needed.
pub fn credentials(profile: &ConnectionProfile) -> Credentials {
    let var = match profile.dialect {
        Dialect::PostgreSql => "IDECRAFT_TEST_PG",
        Dialect::MySql => "IDECRAFT_TEST_MYSQL",
        Dialect::MariaDb => "IDECRAFT_TEST_MARIADB",
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

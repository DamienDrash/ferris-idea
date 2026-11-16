//! SPEC-0016 against the reference databases (local PostgreSQL without TLS, MySQL 8.4 with its
//! self-signed certificate). Uses `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL`; the SSL mode in those
//! URLs is replaced per test.

use crate::common;

use idecraft_database::{ConnectionProfile, Session, SessionError, SslMode};

fn profile(var: &str, mode: SslMode) -> Option<ConnectionProfile> {
    let url = std::env::var(var).ok()?;
    let mut p = ConnectionProfile::from_url("reference", &url).unwrap();
    p.ssl = mode;
    Some(p)
}

async fn encrypted(p: &ConnectionProfile) -> Option<bool> {
    let session = Session::connect(p, &common::credentials(p)).await.unwrap();
    session.encrypted().await
}

#[tokio::test]
async fn postgres_prefer_falls_back_and_require_fails() {
    let Some(prefer) = profile("IDECRAFT_TEST_PG", SslMode::Prefer) else {
        return;
    };
    // The reference PostgreSQL has no TLS; CI's may have. Either way prefer connects.
    assert!(encrypted(&prefer).await.is_some());
    if encrypted(&prefer).await == Some(false) {
        let require = profile("IDECRAFT_TEST_PG", SslMode::Require).unwrap();
        let err = Session::connect(&require, &common::credentials(&require))
            .await
            .err()
            .unwrap();
        assert!(
            matches!(
                err,
                SessionError::Tls {
                    mode: "require",
                    ..
                }
            ),
            "{err}"
        );
    }
}

#[tokio::test]
async fn mysql_modes() {
    let Some(require) = profile("IDECRAFT_TEST_MYSQL", SslMode::Require) else {
        return;
    };
    assert_eq!(encrypted(&require).await, Some(true));
    let prefer = profile("IDECRAFT_TEST_MYSQL", SslMode::Prefer).unwrap();
    assert_eq!(encrypted(&prefer).await, Some(true));
    let disable = profile("IDECRAFT_TEST_MYSQL", SslMode::Disable).unwrap();
    assert_eq!(encrypted(&disable).await, Some(false));
    // The server certificate is self-signed, so verification must fail.
    let verify = profile("IDECRAFT_TEST_MYSQL", SslMode::VerifyFull).unwrap();
    let err = Session::connect(&verify, &common::credentials(&verify))
        .await
        .err()
        .unwrap();
    assert!(
        matches!(
            err,
            SessionError::Tls {
                mode: "verify-full",
                ..
            }
        ),
        "{err}"
    );
}

/// The MySQL server's own CA file (`IDECRAFT_TEST_MYSQL_CA`, set by `scripts/ci-local.sh` from the
/// reference server's data folder).
fn mysql_ca() -> Option<std::path::PathBuf> {
    let ca = std::path::PathBuf::from(std::env::var("IDECRAFT_TEST_MYSQL_CA").ok()?);
    ca.is_file().then_some(ca)
}

fn temp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("idecraft-tlsfiles-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A CA of our own that never signed the server's certificate, made with `openssl`.
fn foreign_ca(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let ok = std::process::Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        ])
        .args(["-subj", "/CN=not-the-server-ca", "-keyout"])
        .arg(dir.join("foreign.key"))
        .arg("-out")
        .arg(dir.join("foreign.pem"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| dir.join("foreign.pem"))
}

/// SPEC-0016 v2 AK-8 (live): `verify-ca` with the server's CA file connects encrypted;
/// `verify-full` with it still fails on the host name (the server's certificate is not for
/// 127.0.0.1); a CA that did not sign the certificate fails the check — no fallback in any case.
#[tokio::test]
async fn mysql_ca_files_are_checked() {
    let (Some(mut p), Some(ca)) = (
        profile("IDECRAFT_TEST_MYSQL", SslMode::VerifyCa),
        mysql_ca(),
    ) else {
        return;
    };
    p.tls.ca_file = Some(ca.clone());
    match Session::connect(&p, &common::credentials(&p)).await {
        Ok(s) => assert_eq!(s.encrypted().await, Some(true)),
        // macOS accepts no server certificate valid for more than 825 days; MySQL's own
        // certificate is valid for ten years, so the system refuses it even with its CA.
        Err(e) if cfg!(target_os = "macos") => {
            assert!(e.to_string().contains("validity period"), "{e}")
        }
        Err(e) => panic!("{e}"),
    }
    p.ssl = SslMode::VerifyFull;
    let err = Session::connect(&p, &common::credentials(&p))
        .await
        .err()
        .unwrap();
    assert!(
        matches!(
            err,
            SessionError::Tls {
                mode: "verify-full",
                ..
            }
        ),
        "{err}"
    );
    let dir = temp("foreign");
    let Some(foreign) = foreign_ca(&dir) else {
        return;
    };
    p.ssl = SslMode::VerifyCa;
    p.tls.ca_file = Some(foreign);
    let err = Session::connect(&p, &common::credentials(&p))
        .await
        .err()
        .unwrap();
    assert!(
        matches!(
            err,
            SessionError::Tls {
                mode: "verify-ca",
                ..
            }
        ),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0016 v2 AK-7 (live): a client key file that others can read is refused before anything
/// is sent, with the path and the mode.
#[cfg(unix)]
#[tokio::test]
async fn readable_client_keys_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let Some(mut p) = profile("IDECRAFT_TEST_PG", SslMode::Require) else {
        return;
    };
    let dir = temp("key");
    let (cert, key) = (dir.join("client.crt"), dir.join("client.key"));
    std::fs::write(
        &cert,
        "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    std::fs::write(
        &key,
        "-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----\n",
    )
    .unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
    p.tls.cert_file = Some(cert);
    p.tls.key_file = Some(key.clone());
    let err = Session::connect(&p, &common::credentials(&p))
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(
        err.contains(&key.display().to_string()) && err.contains("0640"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

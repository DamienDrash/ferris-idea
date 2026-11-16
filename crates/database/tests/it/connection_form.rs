//! SPEC-0108 AK-2: testing a connection from the dialog against a SQLite file (missing, then
//! present) and a server that is not there.

use crate::common;

use idecraft_database::{ConnectionForm, ConnectionProfile, Credentials, Dialect, test_connection};

#[tokio::test]
async fn test_connection_reports_success_and_failure() {
    let dir = std::env::temp_dir().join(format!("idecraft-conn-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.db");
    let ok = ConnectionForm {
        name: "local".into(),
        path: file.display().to_string(),
        ..ConnectionForm::new(Dialect::Sqlite)
    }
    .to_profile("local", None, None)
    .unwrap();
    let missing = test_connection(&ok, &Credentials::NotNeeded)
        .await
        .unwrap_err();
    assert!(missing.contains("does not exist"), "{missing}");
    // An empty file is an empty SQLite database.
    std::fs::write(&file, b"").unwrap();
    let note = test_connection(&ok, &Credentials::NotNeeded).await.unwrap();
    assert_eq!(note, "Connected to local");
    // SPEC-0108 v3: the SQLite version, the file and a measured round trip; no TLS line.
    assert_eq!(note.product, "SQLite");
    assert!(
        note.server_version
            .as_deref()
            .is_some_and(|v| v.starts_with('3')),
        "{note:?}"
    );
    assert_eq!(note.file.as_deref(), Some(file.as_path()));
    assert_eq!(note.tls, None);
    assert!(note.ping > std::time::Duration::ZERO);
    // Nothing listens on port 1 of the loopback address.
    let down = ConnectionForm {
        name: "down".into(),
        port: "1".into(),
        host: "127.0.0.1".into(),
        ..ConnectionForm::new(Dialect::PostgreSql)
    }
    .to_profile("down", None, None)
    .unwrap();
    let err = test_connection(&down, &Credentials::NotNeeded)
        .await
        .unwrap_err();
    assert!(!err.is_empty());
    // SPEC-0142: Oracle goes through Instant Client; nothing listens on port 1.
    let oracle = ConnectionForm {
        name: "ora".into(),
        host: "127.0.0.1".into(),
        ..ConnectionForm::new(Dialect::Oracle)
    }
    .to_profile("ora", None, None)
    .unwrap();
    assert_eq!(oracle.effective_port(), Some(1521));
    let down = ConnectionForm {
        name: "ora".into(),
        host: "127.0.0.1".into(),
        port: "1".into(),
        database: "FREEPDB1".into(),
        user: "scott".into(),
        ..ConnectionForm::new(Dialect::Oracle)
    }
    .to_profile("ora", None, None)
    .unwrap();
    let err = test_connection(&down, &Credentials::NotNeeded)
        .await
        .unwrap_err();
    assert!(err.contains("127.0.0.1:1/FREEPDB1"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0108 v3 AK-7 (live, `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL`; skipped without them): the
/// test names the server version, measures the round trip and says whether TLS is in use.
async fn live_details(var: &str, product: &str) {
    let Ok(url) = std::env::var(var) else {
        return;
    };
    let profile = ConnectionProfile::from_url("reference", &url).unwrap();
    let details = test_connection(&profile, &common::credentials(&profile))
        .await
        .unwrap_or_else(|e| panic!("{var}: {e}"));
    eprintln!("{var}: {}", details.to_text());
    assert!(details.product.starts_with(product), "{details:?}");
    assert!(
        details
            .server_version
            .as_deref()
            .is_some_and(|v| !v.is_empty()),
        "{details:?}"
    );
    assert!(details.ping > std::time::Duration::ZERO);
    // The reference URLs disable TLS (scripts/ci-local.sh); other modes may encrypt.
    if profile.ssl == idecraft_database::SslMode::Disable {
        assert_eq!(details.tls, Some(false), "{details:?}");
    } else {
        assert!(details.tls.is_some(), "{details:?}");
    }
    assert!(
        details
            .detail_lines()
            .iter()
            .any(|l| l.starts_with("TLS: "))
    );
    assert!(details.file.is_none());
}

#[tokio::test]
async fn live_postgres_test_names_version_ping_and_tls() {
    live_details("IDECRAFT_TEST_PG", "PostgreSQL").await;
}

#[tokio::test]
async fn live_mysql_test_names_version_ping_and_tls() {
    live_details("IDECRAFT_TEST_MYSQL", "MySQL").await;
}

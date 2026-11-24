//! SPEC-0011 against a temporary database built from the reference seed files.

use std::path::{Path, PathBuf};

use ferris_idea_database::{ConnectionProfile, Credentials, OnError, Outcome, SqliteSession};

struct TempDb(PathBuf);

impl TempDb {
    /// Builds `inventory.sqlite` from `tests/reference-projects/sql-basic`.
    fn reference(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-sqlite-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sql =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
        let script: String = ["schema.sql", "seed.sql", "seed-movements.sqlite.sql"]
            .iter()
            .map(|f| std::fs::read_to_string(sql.join(f)).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let conn = rusqlite::Connection::open(dir.join("inventory.sqlite")).unwrap();
        conn.execute_batch(&script).unwrap();
        Self(dir)
    }

    fn file(&self) -> PathBuf {
        self.0.join("inventory.sqlite")
    }

    async fn session(&self, read_only: bool) -> SqliteSession {
        let mut profile =
            ConnectionProfile::from_url("ref-sqlite", &format!("sqlite:{}", self.file().display()))
                .unwrap();
        profile.read_only = read_only;
        SqliteSession::connect(&profile, &Credentials::NotNeeded)
            .await
            .unwrap()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn values(outcome: &Outcome) -> Vec<Vec<Option<String>>> {
    match outcome {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("expected rows, got {other:?}"),
    }
}

fn message(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Error(e) => e.message.clone(),
        other => panic!("expected error, got {other:?}"),
    }
}

#[tokio::test]
async fn reference_counts_match() {
    let db = TempDb::reference("counts");
    let run = db
        .session(false)
        .await
        .run_script("select count(*), sum(delta) from movement", OnError::Stop)
        .await;
    assert_eq!(
        values(&run.outcomes[0].outcome),
        [[Some("10000".to_owned()), Some("-30".to_owned())]]
    );
}

#[tokio::test]
async fn two_selects_give_two_result_sets() {
    let db = TempDb::reference("two");
    let run = db
        .session(false)
        .await
        .run_script("select 1 as a; select 'x;y' as b", OnError::Stop)
        .await;
    assert_eq!(run.outcomes.len(), 2);
    assert_eq!(values(&run.outcomes[1].outcome), [[Some("x;y".to_owned())]]);
}

#[tokio::test]
async fn error_points_at_the_bad_token() {
    let db = TempDb::reference("error");
    let script = "select 1;\nselect no_such_column from movement";
    let run = db
        .session(false)
        .await
        .run_script(script, OnError::Stop)
        .await;
    let failed = &run.outcomes[1];
    assert!(message(&failed.outcome).starts_with("SQLITE_ERROR"));
    let offset = failed.error_offset.expect("position mapped");
    assert!(
        script[offset..].starts_with("no_such_column"),
        "{}",
        &script[offset..]
    );
}

#[tokio::test]
async fn insert_reports_update_count() {
    let db = TempDb::reference("insert");
    let run = db
        .session(false)
        .await
        .run_script(
            "create table t (v int); insert into t values (1), (2), (3)",
            OnError::Stop,
        )
        .await;
    assert_eq!(run.outcomes[1].outcome, Outcome::UpdateCount(3));
}

#[tokio::test]
async fn read_only_profile_refuses_changes() {
    let db = TempDb::reference("ro");
    let before = std::fs::read(db.file()).unwrap();
    let run = db
        .session(true)
        .await
        .run_script("update movement set delta = 0 where id = 1", OnError::Stop)
        .await;
    assert!(message(&run.outcomes[0].outcome).starts_with("SQLITE_READONLY"));
    assert_eq!(std::fs::read(db.file()).unwrap(), before);
}

#[tokio::test]
async fn values_by_storage_class() {
    let db = TempDb::reference("values");
    let run = db
        .session(false)
        .await
        .run_script("select x'ff00', 0.1, 1e300, null", OnError::Stop)
        .await;
    assert_eq!(
        values(&run.outcomes[0].outcome),
        [[
            Some("0xff00".to_owned()),
            Some("0.1".to_owned()),
            Some("1e300".to_owned()),
            None
        ]]
    );
}

#[tokio::test]
async fn missing_file_is_refused_and_not_created() {
    let db = TempDb::reference("missing");
    let missing = db.0.join("typo.sqlite");
    let profile =
        ConnectionProfile::from_url("typo", &format!("sqlite:{}", missing.display())).unwrap();
    let err = SqliteSession::connect(&profile, &Credentials::NotNeeded)
        .await
        .err()
        .unwrap();
    assert!(err.to_string().contains("typo.sqlite"), "{err}");
    assert!(!missing.exists());
}

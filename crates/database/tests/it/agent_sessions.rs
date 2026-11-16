//! SPEC-0012 v3 AK-13: a session opened for an agent reaches no file outside the workspace
//! through a file database, and a session of the window keeps what it could do.

use std::path::{Path, PathBuf};

use idecraft_database::{ConnectionProfile, Credentials, OnError, Outcome, Session};

/// A workspace folder with a sibling folder outside it.
struct Place(PathBuf);

impl Place {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("idecraft-agent-db-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ws")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        Self(dir.canonicalize().unwrap())
    }

    fn root(&self) -> PathBuf {
        self.0.join("ws")
    }

    fn outside(&self, name: &str) -> PathBuf {
        self.0.join("outside").join(name)
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sqlite_file(path: &Path, sql: &str) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

fn error_of(run: &idecraft_database::ScriptRun) -> Option<String> {
    run.outcomes.iter().find_map(|o| match &o.outcome {
        Outcome::Error(e) => Some(e.to_string()),
        _ => None,
    })
}

fn first_rows(run: &idecraft_database::ScriptRun) -> Vec<Vec<Option<String>>> {
    match &run.outcomes[0].outcome {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_agents_sqlite_session_attaches_nothing_outside_the_workspace() {
    let place = Place::new("sqlite");
    let root = place.root();
    sqlite_file(&root.join("app.db"), "create table t(v int);");
    sqlite_file(
        &root.join("second.db"),
        "create table inside(v text); insert into inside values ('in');",
    );
    let secret = place.outside("other.db");
    sqlite_file(
        &secret,
        "create table o(v text); insert into o values ('out');",
    );
    let profile =
        ConnectionProfile::from_url("app", &format!("sqlite:{}", root.join("app.db").display()))
            .unwrap();

    let agent = Session::connect_for_agent(&profile, &Credentials::NotNeeded, &root)
        .await
        .unwrap();
    // A file outside is refused before anything of the script runs.
    let run = agent
        .run_script(
            &format!(
                "insert into t values (1); attach database '{}' as x; select v from x.o",
                secret.display()
            ),
            OnError::Continue,
        )
        .await;
    let error = error_of(&run).unwrap();
    assert!(error.contains("refused for agents"), "{error}");
    assert!(error.contains("not inside the workspace"), "{error}");
    assert_eq!((run.outcomes.len(), run.not_run), (1, 2));
    let count = agent
        .run_script("select count(*) from t", OnError::Stop)
        .await;
    assert_eq!(first_rows(&count), [[Some("0".to_owned())]]);
    let run = agent
        .run_script(
            &format!("vacuum into '{}'", root.join("copy.db").display()),
            OnError::Stop,
        )
        .await;
    assert!(error_of(&run).unwrap().contains("VACUUM INTO"));
    assert!(!root.join("copy.db").exists());
    // A file inside the workspace attaches.
    let run = agent
        .run_script(
            &format!(
                "attach database '{}' as s; select v from s.inside",
                root.join("second.db").display()
            ),
            OnError::Stop,
        )
        .await;
    assert_eq!(error_of(&run), None);

    // The window's own session is unchanged: it attaches what the user names.
    let window = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    let run = window
        .run_script(
            &format!(
                "attach database '{}' as x; select v from x.o",
                secret.display()
            ),
            OnError::Stop,
        )
        .await;
    assert_eq!(error_of(&run), None);

    // A startup script that attaches a file outside keeps an agent's session from opening.
    let mut started = profile.clone();
    started.options.startup_script = Some(format!("attach database '{}' as x", secret.display()));
    started.options.startup_allowed = true;
    let error = Session::connect_for_agent(&started, &Credentials::NotNeeded, &root)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("startup script"), "{error}");
    assert!(error.contains("not inside the workspace"), "{error}");
    assert!(
        Session::connect(&started, &Credentials::NotNeeded)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn an_agents_duckdb_session_reaches_no_other_file() {
    let place = Place::new("duckdb");
    let root = place.root();
    let file = root.join("shop.duckdb");
    duckdb::Connection::open(&file)
        .unwrap()
        .execute_batch("create table t(v integer); insert into t values (7);")
        .unwrap();
    let note = place.outside("note.txt");
    std::fs::write(&note, "outside text").unwrap();
    let inside = root.join("inside.txt");
    std::fs::write(&inside, "inside text").unwrap();
    let other = place.outside("other.duckdb");
    duckdb::Connection::open(&other).unwrap();
    let profile =
        ConnectionProfile::from_url("shop", &format!("duckdb:{}", file.display())).unwrap();

    let agent = Session::connect_for_agent(&profile, &Credentials::NotNeeded, &root)
        .await
        .unwrap();
    // The database itself works, reading and writing.
    let run = agent
        .run_script(
            "insert into t values (8); select sum(v) from t",
            OnError::Stop,
        )
        .await;
    assert_eq!(error_of(&run), None);
    for sql in [
        format!("select * from read_text('{}')", note.display()),
        format!("select * from read_text('{}')", inside.display()),
        format!("attach '{}' as o", other.display()),
        format!("copy t to '{}' (format csv)", root.join("t.csv").display()),
        "set enable_external_access = true".to_owned(),
        "set lock_configuration = false".to_owned(),
    ] {
        let run = agent.run_script(&sql, OnError::Stop).await;
        assert!(error_of(&run).is_some(), "not refused: {sql}");
    }
    assert!(!root.join("t.csv").exists());
    drop(agent);

    // The window's own session is unchanged.
    let window = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    let run = window
        .run_script(
            &format!("select content from read_text('{}')", note.display()),
            OnError::Stop,
        )
        .await;
    assert_eq!(first_rows(&run), [[Some("outside text".to_owned())]]);
    drop(window);

    // A startup script that opens another file keeps an agent's session from opening.
    let mut started = profile.clone();
    started.options.startup_script = Some(format!("attach '{}' as o", other.display()));
    started.options.startup_allowed = true;
    let error = Session::connect_for_agent(&started, &Credentials::NotNeeded, &root)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("startup script"), "{error}");
}

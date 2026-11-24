//! SPEC-0013 v3, SPEC-0089 v7 behaviour 11, SPEC-0011 v2 behaviour 6 and SPEC-0008 v5 behaviour
//! 16 against the catalogue sample of `tests/reference-projects/sqlite-catalogue`: every object
//! is read as SQLite's own pragmas report it, the shown DDL recreates an equivalent object,
//! reading changes nothing on disk, and files that cannot be opened say why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ferris_idea_database::{
    ConnectionProfile, Credentials, Dialect, OnError, Outcome, Session, Table, complete,
    create_script, inspect, introspect, introspect_schema, show_ddl, show_ddl_in,
};

struct Sample(PathBuf);

impl Sample {
    /// A new folder with `catalogue.db` built from the sample's SQL.
    fn catalogue(tag: &str) -> Self {
        let sample = Self::empty(tag);
        let sql = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/reference-projects/sqlite-catalogue/catalogue.sql");
        let conn = rusqlite::Connection::open(sample.file("catalogue.db")).unwrap();
        conn.execute_batch(&std::fs::read_to_string(sql).unwrap())
            .unwrap();
        sample
    }

    fn empty(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-sqlite-catalogue-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn profile(&self, name: &str, read_only: bool) -> ConnectionProfile {
        let mut profile =
            ConnectionProfile::from_url("sample", &format!("sqlite:{}", self.file(name).display()))
                .unwrap();
        profile.read_only = read_only;
        profile
    }

    async fn session(&self, name: &str, read_only: bool) -> Session {
        Session::connect(&self.profile(name, read_only), &Credentials::NotNeeded)
            .await
            .unwrap()
    }

    /// Names and sizes of everything in the folder.
    fn listing(&self) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.path().is_file())
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect()
    }
}

impl Drop for Sample {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in std::fs::read_dir(&self.0).into_iter().flatten().flatten() {
                if entry.path().is_dir() {
                    let _ = std::fs::set_permissions(
                        entry.path(),
                        std::fs::Permissions::from_mode(0o755),
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn table<'a>(tables: &'a [Table], name: &str) -> &'a Table {
    tables
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no table {name}"))
}

fn rows(conn: &rusqlite::Connection, sql: &str) -> Vec<Vec<String>> {
    let mut statement = conn.prepare(sql).unwrap();
    let width = statement.column_count();
    statement
        .query_map([], |row| {
            Ok((0..width)
                .map(|i| match row.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Null => "∅".to_owned(),
                    rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                    rusqlite::types::ValueRef::Real(r) => r.to_string(),
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                    rusqlite::types::ValueRef::Blob(_) => "blob".to_owned(),
                })
                .collect())
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// What SQLite's pragmas say about one table or view; names of automatic indexes left out.
fn facts(conn: &rusqlite::Connection, name: &str) -> Vec<String> {
    let lit = format!("'{}'", name.replace('\'', "''"));
    let mut out = Vec::new();
    for (label, sql) in [
        (
            "column",
            format!(
                "select name, type, \"notnull\", dflt_value, pk, hidden from pragma_table_xinfo({lit})"
            ),
        ),
        (
            "foreign key",
            format!(
                "select \"table\", \"from\", \"to\", on_update, on_delete, seq \
                 from pragma_foreign_key_list({lit}) order by \"from\", seq"
            ),
        ),
        (
            "index",
            format!(
                "select case when l.name like 'sqlite_autoindex%' then '' else l.name end, l.\"unique\", \
                 l.origin, l.partial, \
                 (select group_concat(coalesce(name, '<expression>') || ' ' || \"desc\" || ' ' || coll, ', ') \
                  from pragma_index_xinfo(l.name) where key = 1) \
                 from pragma_index_list({lit}) l order by 5, 1"
            ),
        ),
        (
            "kind",
            format!(
                "select type, wr, strict from pragma_table_list where schema = 'main' and name = {lit}"
            ),
        ),
        (
            "trigger",
            format!(
                "select name from sqlite_master where type = 'trigger' and tbl_name = {lit} order by 1"
            ),
        ),
    ] {
        for row in rows(conn, &sql) {
            out.push(format!("{label}: {}", row.join(" | ")));
        }
    }
    out
}

#[tokio::test]
async fn the_catalogue_is_read_as_the_pragmas_report_it() {
    let sample = Sample::catalogue("read");
    let session = sample.session("catalogue.db", true).await;
    let tables = introspect(&session).await.unwrap();
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();

    // Every table and view once; the storage tables of the virtual tables are internal.
    for expected in [
        "affinity",
        "generated",
        "warehouse_slot",
        "strict_reading",
        "ticket",
        "constrained",
        "child_actions",
        "indexed",
        "indexed_city_count",
        "doc_search",
        "area",
        "order items",
        "select",
        "wei\"rd",
        "Straße",
        "顧客",
        "view with space",
    ] {
        assert!(names.contains(&expected), "{expected} missing in {names:?}");
    }
    for shadow in [
        "doc_search_data",
        "doc_search_idx",
        "area_node",
        "area_rowid",
    ] {
        assert!(!names.contains(&shadow), "{shadow} listed");
    }
    assert_eq!(table(&tables, "indexed_city_count").kind, "view");
    assert_eq!(table(&tables, "doc_search").kind, "table");

    // Generated columns are columns, marked; the hidden columns of a virtual table are not.
    let generated = table(&tables, "generated");
    let marks: Vec<(&str, Option<&str>)> = generated
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c.generated.as_deref()))
        .collect();
    assert_eq!(
        marks,
        [
            ("id", None),
            ("net", None),
            ("tax", None),
            ("gross", Some("virtual")),
            ("gross_kept", Some("stored")),
            ("tag", Some("stored")),
        ]
    );
    assert_eq!(generated.columns[3].data_type, "INTEGER");
    let search: Vec<&str> = table(&tables, "doc_search")
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(search, ["title", "body"]);

    // Types, NOT NULL and defaults as written.
    let affinity = table(&tables, "affinity");
    let column = |name: &str| affinity.columns.iter().find(|c| c.name == name).unwrap();
    assert_eq!(column("short_name").data_type, "varchar(40)");
    assert_eq!(column("short_name").default.as_deref(), Some("'it''s'"));
    assert!(!column("label").nullable);
    assert_eq!(column("untyped").data_type, "");
    assert_eq!(
        column("stamp").default.as_deref(),
        Some("strftime('%s', 'now')")
    );

    // Keys: composite primary key in key order, composite and self references, and a reference
    // written without columns points at the primary key.
    assert_eq!(
        table(&tables, "warehouse_slot").primary_key,
        ["warehouse", "aisle", "slot"]
    );
    let composite = &table(&tables, "child_composite").foreign_keys[0];
    assert_eq!(composite.columns, ["region", "number"]);
    assert_eq!(composite.referenced_columns, ["region", "number"]);
    assert_eq!(
        table(&tables, "employee").foreign_keys[0].referenced_table,
        "employee"
    );
    let implicit = table(&tables, "child_actions")
        .foreign_keys
        .iter()
        .find(|f| f.columns == ["implicit_pk"])
        .unwrap();
    assert_eq!(implicit.referenced_columns, ["id"]);
    assert_eq!(table(&tables, "child_actions").foreign_keys.len(), 10);

    // Indexes: a name with a comma stays one name, expressions show their text.
    let index = |t: &str, name: &str| {
        table(&tables, t)
            .indexes
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("no index {name}"))
            .clone()
    };
    assert_eq!(
        index("order items", "idx comma").columns,
        ["comma,name", "quo\"te"]
    );
    assert_eq!(
        index("indexed", "indexed_lower_name").columns,
        ["lower(name)"]
    );
    assert_eq!(
        index("indexed", "indexed_mixed").columns,
        ["city", "amount * 2", "name"]
    );
    assert!(index("indexed", "indexed_email_unique").unique);
    assert_eq!(
        index("indexed", "indexed_created_desc").columns,
        ["created", "id"]
    );

    // Completion finds the columns of a quoted table through its alias (SPEC-0014 v4).
    let sql = "select * from \"order items\" o where o.";
    let offered: Vec<String> = complete(sql, sql.len(), Dialect::Sqlite, &tables)
        .into_iter()
        .map(|c| c.insert_text)
        .collect();
    assert!(offered.contains(&"\"group by\"".to_owned()), "{offered:?}");
    assert!(
        offered.contains(&"\"comma,name\"".to_owned()),
        "{offered:?}"
    );
}

#[tokio::test]
async fn the_shown_ddl_recreates_an_equivalent_object() {
    let sample = Sample::catalogue("ddl");
    let session = sample.session("catalogue.db", true).await;
    let tables = introspect(&session).await.unwrap();
    let original = rusqlite::Connection::open(sample.file("catalogue.db")).unwrap();
    let copy = rusqlite::Connection::open_in_memory().unwrap();
    copy.execute_batch("pragma foreign_keys = off").unwrap();

    let mut shown = Vec::new();
    for kind in ["table", "view"] {
        for t in tables.iter().filter(|t| t.kind == kind) {
            let object = show_ddl(&session, &t.name)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("no DDL for {}", t.name));
            assert_eq!(object.kind, kind, "{}", t.name);
            copy.execute_batch(&object.ddl)
                .unwrap_or_else(|e| panic!("{}: {e}\n{}", t.name, object.ddl));
            shown.push((t.name.clone(), object.ddl));
        }
    }
    assert_eq!(shown.len(), tables.len());
    for (name, ddl) in &shown {
        assert_eq!(facts(&copy, name), facts(&original, name), "{name}\n{ddl}");
    }

    // What no pragma reports is in the text: checks, deferred keys, AUTOINCREMENT.
    let ddl = |name: &str| &shown.iter().find(|(n, _)| n == name).unwrap().1;
    assert!(ddl("constrained").contains("constraint score_range check (score between 0 and 100)"));
    assert!(ddl("child_composite").contains("deferrable initially deferred"));
    assert!(ddl("ticket").contains("autoincrement"));
    assert!(ddl("ticket").contains("CREATE TRIGGER ticket_before_insert"));
    assert!(ddl("doc_search").starts_with("CREATE VIRTUAL TABLE doc_search using fts5"));
    assert!(ddl("order items").contains("CREATE INDEX \"idx comma\""));
    assert!(show_ddl(&session, "no such table").await.unwrap().is_none());

    // The generator of the schema comparison (SPEC-0018) writes statements SQLite accepts: an
    // expression default in brackets, a reference with its columns.
    let generated = rusqlite::Connection::open_in_memory().unwrap();
    for statement in create_script(&tables, Dialect::Sqlite) {
        if statement
            .sql
            .to_ascii_uppercase()
            .contains("CREATE TABLE \"AFFINITY\"")
            || statement.sql.contains("\"child_actions\"")
            || statement.sql.contains("CREATE TABLE \"generated\"")
        {
            generated
                .execute_batch(&statement.sql)
                .unwrap_or_else(|e| panic!("{e}\n{}", statement.sql));
            // A generated column is written as an ordinary one, and says so.
            assert_eq!(
                statement.sql.contains("\"generated\""),
                statement
                    .sql
                    .contains("/* generated (virtual) in the source"),
                "{}",
                statement.sql
            );
        }
    }
    let defaults = rows(
        &generated,
        "select dflt_value from pragma_table_info('affinity') where name in ('stamp', 'ratio', 'payload')",
    );
    assert_eq!(
        defaults,
        [["1e3"], ["x'00ff'"], ["strftime('%s', 'now')"]].map(|r| r.map(str::to_owned).to_vec())
    );
}

#[tokio::test]
async fn reading_the_schema_changes_nothing_on_disk() {
    let sample = Sample::catalogue("untouched");
    let before = sample.listing();
    assert_eq!(before.len(), 1);
    {
        // The profile allows writing; reading the schema still must not.
        let session = sample.session("catalogue.db", false).await;
        introspect(&session).await.unwrap();
        show_ddl(&session, "indexed").await.unwrap().unwrap();
        ferris_idea_database::list_schemas(&session).await.unwrap();
        ferris_idea_database::schema_fingerprint(&session)
            .await
            .unwrap();
    }
    assert_eq!(
        sample.listing(),
        before,
        "a file was changed or left behind"
    );

    // A read-only connection is read-only for SQLite itself, not only for Ferris IDEA's checks.
    let session = sample.session("catalogue.db", true).await;
    for sql in [
        "create table sneaked (a)",
        "insert into ticket (title) values ('x')",
        "pragma user_version = 7",
        "vacuum",
    ] {
        let run = session.run_script(sql, OnError::Stop).await;
        let Outcome::Error(e) = &run.outcomes[0].outcome else {
            panic!("{sql} ran on a read-only connection");
        };
        assert!(
            e.message.contains("SQLITE_READONLY"),
            "{sql}: {}",
            e.message
        );
    }
    // An attached file inherits the read-only flag: a missing one is not created.
    let attach = format!(
        "attach database '{}' as sneaked",
        sample.file("sneaked.db").display()
    );
    let run = session.run_script(&attach, OnError::Stop).await;
    assert!(matches!(run.outcomes[0].outcome, Outcome::Error(_)));
    drop(session);
    assert_eq!(sample.listing(), before);
}

#[cfg(unix)]
#[tokio::test]
async fn a_wal_database_in_a_read_only_folder_is_read_without_side_files() {
    use std::os::unix::fs::PermissionsExt;
    let sample = Sample::empty("ro-wal");
    let folder = sample.file("readonly");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("ro-wal.db");
    {
        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.execute_batch(
            "pragma journal_mode = wal; create table kept (id integer primary key, body text not null); \
             insert into kept (body) values ('do not change');",
        )
        .unwrap();
    }
    for side in ["ro-wal.db-wal", "ro-wal.db-shm"] {
        let _ = std::fs::remove_file(folder.join(side));
    }
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let bytes = std::fs::read(&file).unwrap();

    for read_only in [true, false] {
        let session = sample.session("readonly/ro-wal.db", read_only).await;
        let tables = introspect(&session).await.unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "kept");
        let run = session
            .run_script("select body from kept", OnError::Stop)
            .await;
        assert!(
            matches!(&run.outcomes[0].outcome, Outcome::ResultSet { rows, .. } if rows.len() == 1)
        );
        let run = session
            .run_script("insert into kept (body) values ('x')", OnError::Stop)
            .await;
        assert!(matches!(run.outcomes[0].outcome, Outcome::Error(_)));
    }
    let left: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["ro-wal.db"]);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
}

#[tokio::test]
async fn a_view_over_a_dropped_table_does_not_hide_the_rest() {
    let sample = Sample::empty("broken-view");
    {
        let conn = rusqlite::Connection::open(sample.file("broken.db")).unwrap();
        conn.execute_batch(
            "create table base (id integer primary key, v text); \
             create table other (id integer primary key, v text); \
             create view on_base as select id, v from base; \
             create view on_other as select v from other; \
             drop table base;",
        )
        .unwrap();
    }
    let session = sample.session("broken.db", true).await;
    let tables = introspect(&session).await.unwrap();
    let summary: Vec<(&str, &str, usize)> = tables
        .iter()
        .map(|t| (t.name.as_str(), t.kind.as_str(), t.columns.len()))
        .collect();
    assert_eq!(
        summary,
        [
            ("on_base", "view", 0),
            ("on_other", "view", 1),
            ("other", "table", 2)
        ]
    );
    // SPEC-0013 v4 AK-11: the view carries SQLite's message, the objects that were read none.
    assert_eq!(
        tables[0].unreadable.as_deref(),
        Some("no such table: main.base")
    );
    assert!(tables[1..].iter().all(|t| t.unreadable.is_none()));
    let ddl = show_ddl(&session, "on_base").await.unwrap().unwrap();
    assert!(ddl.ddl.contains("select id, v from base"));
}

/// SPEC-0013 v4 AK-10: the condition of a partial index and the collation of an index term,
/// also for the tables of an attached database.
#[tokio::test]
async fn partial_indexes_and_collations_are_read() {
    let sample = Sample::catalogue("partial");
    let session = sample.session("catalogue.db", true).await;
    let tables = introspect(&session).await.unwrap();
    let indexed = tables.iter().find(|t| t.name == "indexed").unwrap();
    let index = |name: &str| indexed.indexes.iter().find(|i| i.name == name).unwrap();
    let active = index("indexed_active");
    assert_eq!(active.columns, ["city"]);
    assert_eq!(active.condition.as_deref(), Some("deleted = 0"));
    assert!(active.collations.is_empty());
    let nocase = index("indexed_city_nocase");
    assert_eq!(nocase.collations, [Some("nocase".to_owned())]);
    assert_eq!(nocase.terms(), ["city collate nocase"]);
    assert_eq!(nocase.condition, None);
    let mixed = index("indexed_mixed");
    assert_eq!(mixed.columns, ["city", "amount * 2", "name"]);
    assert_eq!(mixed.collations, [None, None, Some("nocase".to_owned())]);
    let plain = index("indexed_name");
    assert!(plain.condition.is_none() && plain.collations.is_empty());
    assert!(index("indexed_email_unique").is_key());
    // Every index SQLite calls partial has a condition, and no other.
    for table in &tables {
        for found in &table.indexes {
            let partial = rusqlite::Connection::open(sample.file("catalogue.db"))
                .unwrap()
                .query_row(
                    "select partial from pragma_index_list(?1) where name = ?2",
                    [&table.name, &found.name],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap();
            assert_eq!(found.condition.is_some(), partial == 1, "{}", found.name);
            assert_ne!(found.condition.as_deref(), Some("<condition>"));
        }
    }

    // An attached database is read the same way; a condition over several lines and a column
    // with a collation of its own.
    let other = Sample::empty("partial-attached");
    {
        let conn = rusqlite::Connection::open(other.file("side.db")).unwrap();
        conn.execute_batch(
            "create table person (id integer primary key, nick text collate nocase, gone integer); \
             create unique index person_nick on person (nick)\n  where gone = 0\n    and nick <> ';';",
        )
        .unwrap();
    }
    let session = other.session("side.db", false).await;
    let attach = format!(
        "attach database '{}' as side",
        other.file("side.db").display()
    );
    session.run_script(&attach, OnError::Stop).await;
    for schema in [None, Some("side")] {
        let tables = introspect_schema(&session, schema).await.unwrap();
        let nick = &tables[0].indexes[0];
        assert_eq!(nick.name, "person_nick");
        assert_eq!(
            nick.condition.as_deref(),
            Some("gone = 0\n    and nick <> ';'")
        );
        assert_eq!(nick.collations, [Some("nocase".to_owned())]);
        assert!(nick.unique && !nick.is_key());
    }
}

#[tokio::test]
async fn files_that_cannot_be_opened_say_why() {
    let sample = Sample::empty("errors");
    std::fs::write(
        sample.file("notadb.db"),
        "This is a text file.\n".repeat(20),
    )
    .unwrap();
    let error = |name: &str| {
        let profile = sample.profile(name, true);
        async move {
            Session::connect(&profile, &Credentials::NotNeeded)
                .await
                .err()
                .unwrap_or_else(|| panic!("{} opened", profile.name))
                .to_string()
        }
    };
    let not_a_database = error("notadb.db").await;
    assert!(
        not_a_database.contains("is not a SQLite database")
            && not_a_database.contains("SQLITE_NOTADB"),
        "{not_a_database}"
    );
    assert_eq!(
        std::fs::read_to_string(sample.file("notadb.db")).unwrap(),
        "This is a text file.\n".repeat(20)
    );
    let missing = error("missing.db").await;
    assert!(missing.contains("file does not exist"), "{missing}");
    assert!(!sample.file("missing.db").exists());
    std::fs::create_dir_all(sample.file("folder.db")).unwrap();
    let folder = error("folder.db").await;
    assert!(folder.contains("a folder, not a database file"), "{folder}");

    // Another connection holds a write lock: the reason and what to do, after SQLite's wait.
    {
        let conn = rusqlite::Connection::open(sample.file("locked.db")).unwrap();
        conn.execute_batch("create table held (id integer primary key)")
            .unwrap();
    }
    let holder = rusqlite::Connection::open(sample.file("locked.db")).unwrap();
    holder
        .execute_batch("begin exclusive; insert into held default values;")
        .unwrap();
    let locked = error("locked.db").await;
    assert!(
        locked.contains("the database is locked") && locked.contains("try again"),
        "{locked}"
    );
    holder.execute_batch("rollback").unwrap();
    let session = sample.session("locked.db", true).await;
    assert_eq!(introspect(&session).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_relative_file_is_found_from_the_workspace_and_attached_databases_are_read() {
    let sample = Sample::catalogue("relative");
    {
        let sql = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/reference-projects/sqlite-catalogue/attached.sql");
        let conn = rusqlite::Connection::open(sample.file("attached.db")).unwrap();
        conn.execute_batch(&std::fs::read_to_string(sql).unwrap())
            .unwrap();
    }
    std::fs::create_dir_all(sample.file(".ferris-idea")).unwrap();
    std::fs::write(
        sample.file(".ferris-idea/connections.toml"),
        format!(
            "[ferris-idea]\nkind = \"connections\"\nformat = 1\n\n[[connection]]\nid = \"c\"\nname = \"c\"\n\
             dialect = \"sqlite\"\npath = \"catalogue.db\"\n\n[connection.options]\n\
             startup_script = \"attach database '{}' as archive\"\n",
            sample.file("attached.db").display()
        ),
    )
    .unwrap();
    let profiles = ferris_idea_database::load_profiles(&sample.0).unwrap();
    assert_eq!(profiles[0].path.as_deref(), Some(Path::new("catalogue.db")));
    assert_eq!(profiles[0].file_path(), sample.file("catalogue.db"));
    // Saving writes the path as it was, never the folder it was read from.
    ferris_idea_database::save_profiles(&sample.0, &profiles).unwrap();
    let written = std::fs::read_to_string(sample.file(".ferris-idea/connections.toml")).unwrap();
    assert!(written.contains("path = \"catalogue.db\""), "{written}");
    assert!(!written.contains("base ="), "{written}");

    let mut profile = profiles[0].clone();
    profile.options.startup_allowed = true;
    let session = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    assert!(!introspect(&session).await.unwrap().is_empty());
    let archive = introspect_schema(&session, Some("archive")).await.unwrap();
    let names: Vec<&str> = archive.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["archive notes", "archived_ticket", "archived_titles"]
    );
    assert_eq!(
        table(&archive, "archive notes").foreign_keys[0].referenced_table,
        "archived_ticket"
    );
    assert_eq!(table(&archive, "archived_ticket").indexes.len(), 1);
}

/// SPEC-0005 v2 AK-9, SPEC-0089 v8 AK-19 and SPEC-0015 v3 AK-10 against a real SQLite file: a
/// trigger with `BEGIN … END` runs as one statement and fires; a table, its index and a view of
/// an attached database are described and shown as DDL with their schema; FTS5's hidden `rank`
/// and the column named after the table are no unknown columns, a misspelt one still is.
#[tokio::test]
async fn triggers_attached_objects_and_hidden_columns() {
    let sample = Sample::empty("triggers");
    drop(rusqlite::Connection::open(sample.file("main.db")).unwrap());
    {
        let other = rusqlite::Connection::open(sample.file("other.db")).unwrap();
        other
            .execute_batch(
                "create table items(id integer primary key, name text not null);\n\
                 create index items_name on items(name);\n\
                 create view item_names as select name from items;",
            )
            .unwrap();
    }
    let mut profile = sample.profile("main.db", false);
    profile.options.startup_script = Some(format!(
        "attach database '{}' as other",
        sample.file("other.db").display()
    ));
    profile.options.startup_allowed = true;
    let session = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();

    let script = "create table stock(id integer primary key, qty int);\n\
        create table log(id int, size text);\n\
        create trigger stock_log after insert on stock\n\
        begin\n  insert into log values (new.id, case when new.qty > 9 then 'big' else 'small' end);\n  \
        update stock set qty = qty + 1 where id = new.id;\nend;\n\
        insert into stock values (1, 12);\n\
        create virtual table docs using fts5(title, body);\n";
    let run = session.run_script(script, OnError::Stop).await;
    let errors: Vec<String> = run
        .outcomes
        .iter()
        .filter_map(|o| match &o.outcome {
            Outcome::Error(e) => Some(e.message.clone()),
            _ => None,
        })
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(run.outcomes.len(), 5, "the trigger is one statement");
    {
        let conn = rusqlite::Connection::open(sample.file("main.db")).unwrap();
        assert_eq!(rows(&conn, "select id, size from log"), [["1", "big"]]);
        assert_eq!(rows(&conn, "select qty from stock"), [["13"]]);
    }

    // The attached database's objects, by their schema.
    let other = introspect_schema(&session, Some("other")).await.unwrap();
    let items = table(&other, "items");
    assert_eq!(items.indexes[0].name, "items_name");
    let ddl = show_ddl_in(&session, Some("other"), "items")
        .await
        .unwrap()
        .expect("the attached table");
    assert_eq!(ddl.kind, "table");
    assert!(ddl.ddl.starts_with("CREATE TABLE items("), "{}", ddl.ddl);
    assert!(
        ddl.ddl.contains("CREATE INDEX items_name on items(name);"),
        "{}",
        ddl.ddl
    );
    let view = show_ddl_in(&session, Some("other"), "item_names")
        .await
        .unwrap()
        .expect("the attached view");
    assert_eq!(view.kind, "view");
    assert!(show_ddl(&session, "items").await.unwrap().is_none());

    // FTS5's hidden columns.
    let main = introspect(&session).await.unwrap();
    let docs = table(&main, "docs");
    let names: Vec<&str> = docs.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["title", "body"]);
    assert_eq!(docs.hidden_columns, ["docs", "rank"]);
    let query = "select title, rank from docs where docs match 'sql' order by rank;\n\
                 select docs.rank, d.title from docs d;\n\
                 select titel from docs;";
    let problems: Vec<String> = inspect(query, Dialect::Sqlite, &main)
        .into_iter()
        .map(|p| p.message)
        .collect();
    assert_eq!(problems, ["unknown column `titel`"]);
}

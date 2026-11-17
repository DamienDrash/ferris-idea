//! F-060 step by step against the servers of the reference environment, without a window: SQL
//! Server (`SPEC-0116`), MariaDB (`SPEC-0116`) and Oracle (`SPEC-0142`, `SPEC-0135`). Each test
//! runs a throw-away script of `pd_*` objects as a user would select it all and run it, then
//! checks Test Connection, the catalog, an edited cell, plans and a comparison of two schemas
//! where the system has them, and drops its objects again. Without `IDECRAFT_TEST_SQLSERVER`,
//! `IDECRAFT_TEST_MARIADB` or `IDECRAFT_TEST_ORACLE` (and their password files) the test of that
//! system passes without doing anything.

use crate::common;

use idecraft_database::{
    Applied, ConnectionProfile, Credentials, EditSet, EditTarget, OnError, Outcome, ScriptRun,
    Secret, Session, Values, compare, explain, introspect, introspect_schema, test_connection,
};

/// The profile of `var` and its password from `<var>_PASSWORD` or `<var>_PASSWORD_FILE`.
fn reference(var: &str) -> Option<(ConnectionProfile, Credentials)> {
    let url = std::env::var(var).ok()?;
    let profile = ConnectionProfile::from_url("reference", &url).unwrap();
    let password = std::env::var(format!("{var}_PASSWORD")).ok().or_else(|| {
        let file = std::env::var(format!("{var}_PASSWORD_FILE")).ok()?;
        Some(std::fs::read_to_string(file).ok()?.trim().to_owned())
    });
    let credentials = match password {
        Some(p) => Credentials::Available(Secret::new(p)),
        None => common::credentials(&profile),
    };
    Some((profile, credentials))
}

fn rows(o: &Outcome) -> Vec<Vec<Option<String>>> {
    match o {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("not a result set: {other:?}"),
    }
}

fn text(rows: &[&[&str]]) -> Vec<Vec<Option<String>>> {
    rows.iter()
        .map(|r| r.iter().map(|v| Some((*v).to_owned())).collect())
        .collect()
}

/// The 1-based line of the script on which outcome `i` of `run` starts.
fn line_of(script: &str, run: &ScriptRun, i: usize) -> usize {
    script[..run.outcomes[i].range.start].matches('\n').count() + 1
}

fn show(script: &str, run: &ScriptRun) -> String {
    run.outcomes
        .iter()
        .map(|o| format!("{:?} → {:?}", &script[o.range.clone()], o.outcome))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Changes the body of row 2 of `pd_note` through an edit set, as the result grid submits it, and
/// reads it back. `id` and `body` are the column names as the catalog has them.
async fn edit_a_cell(s: &Session, table: &idecraft_database::Table, [id, body]: [&str; 2]) {
    let target = EditTarget::new(table, s.dialect()).unwrap();
    let mut edits = EditSet::new(target);
    let row: Values = vec![
        (id.into(), Some("2".into())),
        (body.into(), Some("plain".into())),
    ];
    edits
        .set_cell(&row, body, Some("it's changed".into()))
        .unwrap();
    let select = "select body from pd_note where id = 2";
    assert_eq!(edits.apply(s).await, Applied::Committed(1));
    let back = s.run_script(select, OnError::Stop).await;
    assert_eq!(
        rows(&back.outcomes[0].outcome),
        text(&[&["it's changed"]]),
        "{back:?}"
    );
}

/// S1–S3: `GO` batches, a failing batch with its code and line, Test Connection with version and
/// encryption, the key under the name `sys.indexes` has, an edited cell, both plans, two result
/// sets of one procedure and two schemas compared.
#[tokio::test]
async fn sql_server_steps() {
    let Some((profile, credentials)) = reference("IDECRAFT_TEST_SQLSERVER") else {
        eprintln!("skipped: IDECRAFT_TEST_SQLSERVER not set");
        return;
    };
    let s = Session::connect(&profile, &credentials).await.unwrap();
    let cleanup = "drop procedure if exists pd_two_sets;\ndrop table if exists pd_b.pd_note;\n\
                   drop schema if exists pd_b;\ndrop table if exists pd_note;";
    let _ = s.run_script(cleanup, OnError::Continue).await;

    // S1: the script of the plan, all selected and run.
    let script = "-- F-060 on SQL Server: GO batches; the third batch fails on purpose.\n\
                  create table pd_note (id int primary key, body nvarchar(40) not null);\n\
                  insert into pd_note values (1, N'semi; colon'), (2, N'plain');\n\
                  GO\n\
                  select id, body from pd_note order by id;\n\
                  select count(*) as n from pd_note;\n\
                  GO\n\
                  select no_such_column from pd_note;\n\
                  GO\n";
    let run = s.run_script(script, OnError::Stop).await;
    eprintln!("S1:\n{}", show(script, &run));
    assert_eq!(run.outcomes.len(), 5, "{}", show(script, &run));
    assert_eq!(run.outcomes[1].outcome, Outcome::UpdateCount(2));
    assert_eq!(
        rows(&run.outcomes[2].outcome),
        text(&[&["1", "semi; colon"], &["2", "plain"]])
    );
    assert_eq!(rows(&run.outcomes[3].outcome), text(&[&["2"]]));
    match &run.outcomes[4].outcome {
        Outcome::Error(e) => {
            assert!(e.message.contains("207"), "{e:?}");
            assert!(e.message.contains("no_such_column"), "{e:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(line_of(script, &run, 4), 8);

    // S2: Test Connection names the system, its version and the encryption.
    let test = test_connection(&profile, &credentials).await.unwrap();
    eprintln!("S2: {}", test.to_text());
    assert_eq!(test.product, "SQL Server");
    assert!(
        test.server_version
            .as_deref()
            .is_some_and(|v| v.starts_with("16.")),
        "{test:?}"
    );
    // The login has no right to see the server's connections; the driver's agreement answers:
    // `prefer` is encrypted exactly when an encrypted session can be opened, `disable` is not.
    let mut strict = profile.clone();
    strict.ssl = idecraft_database::SslMode::Require;
    let encrypts = Session::connect(&strict, &credentials).await.is_ok();
    assert_eq!(test.tls, Some(encrypts), "{test:?}");
    let mut plain = profile.clone();
    plain.ssl = idecraft_database::SslMode::Disable;
    let test = test_connection(&plain, &credentials).await.unwrap();
    assert_eq!(test.tls, Some(false), "{test:?}");

    // S2: the tree's key is the index `sys.indexes` names.
    let tables = introspect(&s).await.unwrap();
    let note = tables.iter().find(|t| t.name == "pd_note").unwrap();
    assert_eq!(note.primary_key, ["id"]);
    let pk = s
        .run_script(
            "select name from sys.indexes where object_id = object_id('pd_note') and is_primary_key = 1",
            OnError::Stop,
        )
        .await;
    let pk = rows(&pk.outcomes[0].outcome)[0][0].clone().unwrap();
    assert!(pk.starts_with("PK__pd_note__"), "{pk}");
    assert!(
        note.indexes.iter().any(|i| i.name == pk && i.unique),
        "{:?}",
        note.indexes
    );

    // S3: one cell changed, committed and read back; both plans of a query on the table.
    edit_a_cell(&s, note, ["id", "body"]).await;
    let query = "select id, body from pd_note where id = 2";
    let plan = explain(&s, query, false).await.unwrap();
    eprintln!("S3 plan:\n{:#?}", plan.nodes);
    let all = |p: &idecraft_database::Plan| {
        let mut out = Vec::new();
        let mut stack: Vec<_> = p.nodes.iter().collect();
        while let Some(n) = stack.pop() {
            stack.extend(n.children.iter());
            out.push(n.clone());
        }
        out
    };
    assert!(all(&plan).iter().any(|n| n.cost.is_some()), "{}", plan.raw);
    assert!(
        all(&plan)
            .iter()
            .any(|n| n.object.as_deref().is_some_and(|o| o.contains(&pk))),
        "the plan names {pk}: {:#?}",
        plan.nodes
    );
    let measured = explain(&s, query, true).await.unwrap();
    assert!(measured.analyzed);
    assert!(
        all(&measured).iter().any(|n| n.actual_rows == Some(1.0)),
        "{:#?}",
        measured.nodes
    );

    // SPEC-0005 v4 behaviour 18: a procedure with two SELECTs gives both result sets.
    let made = s
        .run_script(
            "create procedure pd_two_sets as\nbegin\n  select id, body from pd_note order by id;\n  \
             select count(*) as n from pd_note;\nend\nGO\n",
            OnError::Stop,
        )
        .await;
    assert!(
        !matches!(made.outcomes[0].outcome, Outcome::Error(_)),
        "{made:?}"
    );
    let call = "exec pd_two_sets";
    let sets = s.run_script(call, OnError::Stop).await;
    eprintln!("EXEC:\n{}", show(call, &sets));
    assert_eq!(sets.outcomes.len(), 2, "{}", show(call, &sets));
    assert_eq!(
        rows(&sets.outcomes[0].outcome),
        text(&[&["1", "semi; colon"], &["2", "it's changed"]])
    );
    assert_eq!(rows(&sets.outcomes[1].outcome), text(&[&["2"]]));
    assert_eq!(sets.outcomes[0].range, sets.outcomes[1].range);

    // SPEC-0005 v6 behaviour 11a: a variable is read in its batch, `TRY … CATCH` answers from
    // `CATCH`, two queries without `;` and the queries of an `IF` block give their result sets;
    // a failure after a result set keeps the set.
    let batches = "declare @n int = (select count(*) from pd_note);\nselect @n as n;\nGO\n\
                   begin try\n  select 1/0 as x;\nend try\nbegin catch\n  \
                   select error_number() as e;\nend catch\nGO\n\
                   select 'a' as a\nselect 'b' as b\nGO\n\
                   if 1 = 1\nbegin\n  select 'c' as c;\n  select 'd' as d;\nend\nGO\n\
                   select 'e' as e\nselect 1/0 as z\nGO\nselect 'f' as f";
    let run = s.run_script(batches, OnError::Continue).await;
    eprintln!("batches:\n{}", show(batches, &run));
    let got: Vec<_> = run
        .outcomes
        .iter()
        .map(|o| match &o.outcome {
            Outcome::ResultSet { columns, rows } => match rows.first() {
                Some(row) => row[0].clone().unwrap_or_default(),
                None => format!("no rows of {}", columns.join(", ")),
            },
            Outcome::UpdateCount(n) => format!("{n} rows"),
            Outcome::Error(e) => e.message.clone(),
        })
        .collect();
    // The server sends the columns of `x` and `z` before the division fails: empty sets, as in
    // `sqlcmd`. `CATCH` answers; the uncaught failure keeps the sets before it, the next batch runs.
    let failure = "SQL Server error 8134 (line 2): Divide by zero error encountered.";
    assert_eq!(
        got,
        [
            "2",
            "no rows of x",
            "8134",
            "a",
            "b",
            "c",
            "d",
            "e",
            "no rows of z",
            failure,
            "f"
        ],
        "{}",
        show(batches, &run)
    );
    let failed = &run.outcomes[9];
    assert!(batches[failed.error_offset.unwrap()..].starts_with("select 1/0 as z"));

    // Two schemas of one database compared: `pd_b.pd_note` has one column more.
    let other = s
        .run_script(
            "create schema pd_b\nGO\ncreate table pd_b.pd_note (id int primary key, \
             body nvarchar(40) not null, pd_discontinued bit null)",
            OnError::Stop,
        )
        .await;
    assert!(
        other
            .outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{other:?}"
    );
    let b = introspect_schema(&s, Some("pd_b")).await.unwrap();
    assert_eq!(
        b.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["pd_note"]
    );
    let dbo: Vec<_> = tables.into_iter().filter(|t| t.name == "pd_note").collect();
    let diff = compare(&b, &dbo, s.dialect());
    eprintln!("compare: {diff:?}");
    assert_eq!(diff.changed.len(), 1, "{diff:?}");
    assert_eq!(
        diff.changed[0]
            .columns_added
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["pd_discontinued"]
    );

    let gone = s.run_script(cleanup, OnError::Continue).await;
    assert!(
        gone.outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{gone:?}"
    );
}

/// M2, M3: Test Connection names MariaDB and its version; `DELIMITER` and both result sets of the
/// procedure as on MySQL; the key in the catalog; an edited cell; two databases compared.
#[tokio::test]
async fn mariadb_steps() {
    let Some((profile, credentials)) = reference("IDECRAFT_TEST_MARIADB") else {
        eprintln!("skipped: IDECRAFT_TEST_MARIADB not set");
        return;
    };
    let test = test_connection(&profile, &credentials).await.unwrap();
    eprintln!("M2: {}", test.to_text());
    assert_eq!(test.product, "MariaDB");
    // SPEC-0108 v12 behaviour 22: the version number alone, the product line names MariaDB.
    assert!(
        test.server_version
            .as_deref()
            .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()) && !v.contains("MariaDB")),
        "{test:?}"
    );

    let s = Session::connect(&profile, &credentials).await.unwrap();
    let cleanup = "drop procedure if exists pd_two_sets;\ndrop table if exists pd_note;\n\
                   drop database if exists pd_b;";
    let _ = s.run_script(cleanup, OnError::Continue).await;
    let script = "-- F-060 on MySQL and MariaDB: DELIMITER and two result sets.\n\
                  create table pd_note (id int primary key, body varchar(40) not null);\n\
                  insert into pd_note values (1, 'semi; colon'), (2, 'plain');\n\
                  \n\
                  DELIMITER //\n\
                  create procedure pd_two_sets()\n\
                  begin\n  \
                    select id, body from pd_note order by id;\n  \
                    select count(*) as n from pd_note;\n\
                  end //\n\
                  DELIMITER ;\n\
                  \n\
                  call pd_two_sets();\n\
                  select body from pd_note where body like '%;%';\n";
    let run = s.run_script(script, OnError::Stop).await;
    eprintln!("M2 script:\n{}", show(script, &run));
    assert_eq!(run.outcomes.len(), 6, "{}", show(script, &run));
    assert_eq!(run.outcomes[1].outcome, Outcome::UpdateCount(2));
    assert_eq!(
        rows(&run.outcomes[3].outcome),
        text(&[&["1", "semi; colon"], &["2", "plain"]])
    );
    assert_eq!(rows(&run.outcomes[4].outcome), text(&[&["2"]]));
    assert_eq!(run.outcomes[3].range, run.outcomes[4].range);
    assert_eq!(line_of(script, &run, 3), 13);
    assert_eq!(rows(&run.outcomes[5].outcome), text(&[&["semi; colon"]]));

    // M3: the table with its key; a cell changed, committed and read back.
    let tables = introspect(&s).await.unwrap();
    let note = tables.iter().find(|t| t.name == "pd_note").unwrap();
    assert_eq!(note.primary_key, ["id"]);
    edit_a_cell(&s, note, ["id", "body"]).await;

    // An error names MariaDB, its code and where the statement is.
    let bad = "select 1;\nselect no_such_column\n  from pd_note;";
    let failed = s.run_script(bad, OnError::Stop).await;
    eprintln!("M error:\n{}", show(bad, &failed));
    match &failed.outcomes[1].outcome {
        Outcome::Error(e) => assert!(e.message.starts_with("MariaDB error 1054"), "{e:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(line_of(bad, &failed, 1), 2);

    // Two databases of one server compared.
    let other = s
        .run_script(
            "create database pd_b;\ncreate table pd_b.pd_note (id int primary key, \
             body varchar(40) not null, pd_discontinued tinyint null)",
            OnError::Stop,
        )
        .await;
    assert!(
        other
            .outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{other:?}"
    );
    let b = introspect_schema(&s, Some("pd_b")).await.unwrap();
    let here: Vec<_> = tables.into_iter().filter(|t| t.name == "pd_note").collect();
    let diff = compare(&b, &here, s.dialect());
    eprintln!("compare: {diff:?}");
    assert_eq!(diff.changed.len(), 1, "{diff:?}");
    assert_eq!(
        diff.changed[0]
            .columns_added
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["pd_discontinued"]
    );

    let gone = s.run_script(cleanup, OnError::Continue).await;
    assert!(
        gone.outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{gone:?}"
    );
}

/// Oracle `/`: PL/SQL blocks and a procedure end at their `/` line, the `;` inside them stay;
/// Test Connection names Oracle and its version; errors with `ORA-` code and position; the key in
/// the catalog; an edited cell; Explain Plan.
#[tokio::test]
async fn oracle_steps() {
    let Some((profile, credentials)) = reference("IDECRAFT_TEST_ORACLE") else {
        eprintln!("skipped: IDECRAFT_TEST_ORACLE not set");
        return;
    };
    let test = test_connection(&profile, &credentials).await.unwrap();
    eprintln!("O: {}", test.to_text());
    assert_eq!(test.product, "Oracle");
    assert!(test.server_version.is_some(), "{test:?}");

    let s = Session::connect(&profile, &credentials).await.unwrap();
    let cleanup = "drop procedure pd_fill;\ndrop procedure pd_broken;\ndrop table pd_note purge";
    let _ = s.run_script(cleanup, OnError::Continue).await;
    let script = "-- F-060 on Oracle: PL/SQL blocks end at a line holding only /.\n\
                  create table pd_note (id number(10) primary key, body varchar2(40) not null);\n\
                  insert into pd_note values (1, 'semi; colon');\n\
                  begin\n  \
                    insert into pd_note values (2, 'plain');\n  \
                    null;\n\
                  end;\n\
                  /\n\
                  create or replace procedure pd_fill (p_id number) as\n\
                  begin\n  \
                    insert into pd_note values (p_id, q'[it's; /]');\n\
                  end;\n\
                  /\n\
                  call pd_fill(3);\n\
                  select id, body from pd_note order by id;\n\
                  select no_such_column from pd_note;\n";
    let run = s.run_script(script, OnError::Continue).await;
    eprintln!("O script:\n{}", show(script, &run));
    assert_eq!(run.outcomes.len(), 7, "{}", show(script, &run));
    for o in &run.outcomes[..6] {
        assert!(
            !matches!(o.outcome, Outcome::Error(_)),
            "{}",
            show(script, &run)
        );
    }
    assert_eq!(line_of(script, &run, 2), 4);
    assert_eq!(line_of(script, &run, 3), 9);
    assert_eq!(
        rows(&run.outcomes[5].outcome),
        text(&[&["1", "semi; colon"], &["2", "plain"], &["3", "it's; /"]])
    );
    match &run.outcomes[6].outcome {
        Outcome::Error(e) => {
            assert!(e.message.contains("ORA-00904"), "{e:?}");
            assert_eq!(e.position, Some(8));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(line_of(script, &run, 6), 16);

    // A procedure that does not compile is reported as an error, not as done.
    let broken = "create or replace procedure pd_broken as\nbegin\n  no_such_call;\nend;\n/\n";
    let compiled = s.run_script(broken, OnError::Stop).await;
    eprintln!("O broken:\n{}", show(broken, &compiled));
    match &compiled.outcomes[0].outcome {
        Outcome::Error(e) => {
            assert!(e.message.starts_with("ORA-24344"), "{e:?}");
            assert!(e.message.contains("\nline 3, column 3: PLS-00201"), "{e:?}");
        }
        other => panic!("{other:?}"),
    }
    let at = compiled.outcomes[0].error_offset.expect("position");
    assert!(broken[at..].starts_with("no_such_call"), "{compiled:?}");

    let tables = introspect(&s).await.unwrap();
    let note = tables.iter().find(|t| t.name == "PD_NOTE").unwrap();
    assert_eq!(note.primary_key, ["ID"]);
    edit_a_cell(&s, note, ["ID", "BODY"]).await;
    let plan = explain(&s, "select id, body from pd_note where id = 2", false)
        .await
        .unwrap();
    eprintln!("O plan:\n{:#?}", plan.nodes);
    assert!(plan.nodes[0].operation.starts_with("SELECT STATEMENT"));

    let gone = s.run_script(cleanup, OnError::Continue).await;
    eprintln!("O cleanup:\n{}", show(cleanup, &gone));
}

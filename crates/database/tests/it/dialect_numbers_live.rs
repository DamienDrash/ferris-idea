//! OBS-0120 X1–X3 and O4 against the servers of the reference environment: numbers in edited
//! cells (`SPEC-0017` v2 behaviour 7), decimals with their scale (`SPEC-0116` v10 behaviour 16,
//! `SPEC-0142` v5 behaviour 3b), routines a script creates (`SPEC-0089` v17 behaviour 29) and
//! Oracle's transactions (`SPEC-0142` v5 behaviour 3c). Each test works on a throw-away table
//! `pd_num_<pid>` of the test account and drops it again. Without `FERRIS_IDEA_TEST_PG`,
//! `FERRIS_IDEA_TEST_MYSQL`, `FERRIS_IDEA_TEST_MARIADB`, `FERRIS_IDEA_TEST_SQLSERVER` or
//! `FERRIS_IDEA_TEST_ORACLE` (and their password files) the system is skipped.

use crate::dialect_steps_live::reference;

use ferris_idea_database::{
    Applied, Dialect, EditSet, EditTarget, Failure, Isolation, OnError, Outcome, ScriptRun,
    Session, Table, Values, begin_statements, introspect, schema_objects,
};

const SYSTEMS: [&str; 5] = [
    "FERRIS_IDEA_TEST_PG",
    "FERRIS_IDEA_TEST_MYSQL",
    "FERRIS_IDEA_TEST_MARIADB",
    "FERRIS_IDEA_TEST_SQLSERVER",
    "FERRIS_IDEA_TEST_ORACLE",
];

async fn connect(var: &str) -> Option<Session> {
    let (profile, credentials) = reference(var)?;
    Some(Session::connect(&profile, &credentials).await.unwrap())
}

async fn run(s: &Session, sql: &str) -> ScriptRun {
    let run = s.run_script(sql, OnError::Stop).await;
    for o in &run.outcomes {
        if let Outcome::Error(e) = &o.outcome {
            panic!("{sql}: {e}");
        }
    }
    run
}

fn rows(run: &ScriptRun) -> Vec<Vec<Option<String>>> {
    match &run.outcomes.last().expect("a statement ran").outcome {
        Outcome::ResultSet { rows, .. } => rows.clone(),
        other => panic!("not a result set: {other:?}"),
    }
}

/// The table's DDL in `d`: a key, a small and a large-scale decimal, a single- and a
/// double-precision float, and an integer-valued decimal that may be NULL.
fn create(d: Dialect, t: &str) -> String {
    let (int, small, big, single, double, whole) = match d {
        Dialect::PostgreSql => (
            "int",
            "numeric(8,2)",
            "numeric(38,10)",
            "real",
            "double precision",
            "numeric(38,0)",
        ),
        Dialect::MySql | Dialect::MariaDb => (
            "int",
            "decimal(8,2)",
            "decimal(38,10)",
            "float",
            "double",
            "decimal(38,0)",
        ),
        Dialect::SqlServer => (
            "int",
            "decimal(8,2)",
            "decimal(38,10)",
            "real",
            "float",
            "decimal(38,0)",
        ),
        Dialect::Oracle => (
            "number(10)",
            "number(8,2)",
            "number(38,10)",
            "binary_float",
            "binary_double",
            "number(38,0)",
        ),
        other => panic!("{other:?}"),
    };
    format!(
        "create table {t} (id {int} primary key, small {small}, big {big}, single {single}, \
         twice {double}, whole {whole})"
    )
}

const VALUES: [&str; 3] = [
    "(1, 0.05, 1234567890123456789012345678.1234567891, 0.1, 0.1, null)",
    "(2, -0.5, -0.0000000001, -1.5, 0.00001, 4)",
    "(3, 4, 0, 16777216, 123456.789, -7)",
];

/// The loaded row `id` as the grid holds it, with the catalog's column names.
fn row_of(table: &Table, loaded: &[Vec<Option<String>>], id: &str) -> Values {
    let row = loaded
        .iter()
        .find(|r| r[0].as_deref() == Some(id))
        .unwrap_or_else(|| panic!("row {id} in {loaded:?}"));
    table
        .columns
        .iter()
        .map(|c| c.name.clone())
        .zip(row.iter().cloned())
        .collect()
}

async fn edit(s: &Session, table: &Table, row: &Values, column: &str, value: &str) -> Applied {
    let mut edits = EditSet::new(EditTarget::new(table, s.dialect()).unwrap());
    edits.set_cell(row, column, Some(value.into())).unwrap();
    edits.apply(s).await
}

/// SPEC-0017 v2 AK-8 (OBS-0120 X1): every numeric cell — below 1, negative, of a large scale,
/// single and double floats, integers, NULL — can be edited with its loaded value, without a
/// false conflict; a value changed meanwhile, even in the last digit of a large-scale decimal or
/// a float, is a conflict. Deleting a row compares every loaded value the same way. Decimals keep
/// their scale (SPEC-0116 v10 AK-14, SPEC-0142 v5 AK-9).
#[tokio::test]
async fn numbers_in_edited_cells_and_their_scale() {
    for var in SYSTEMS {
        let Some(s) = connect(var).await else {
            continue;
        };
        let d = s.dialect();
        let t = format!("pd_num_{}", std::process::id());
        let _ = s
            .run_script(&format!("drop table {t}"), OnError::Continue)
            .await;
        run(&s, &create(d, &t)).await;
        for v in VALUES {
            run(&s, &format!("insert into {t} values {v}")).await;
        }
        if d == Dialect::Oracle {
            run(&s, "commit").await;
        }
        let tables = introspect(&s).await.unwrap();
        let table = tables
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(&t))
            .expect("the table is in the catalog")
            .clone();
        let name = |i: usize| table.columns[i].name.clone();
        let select = format!("select * from {t} order by 1");
        let loaded = rows(&run(&s, &select).await);
        // Decimals keep their scale; 0.05 has its zero (X3, and Oracle's `.05`).
        let small: Vec<_> = loaded.iter().map(|r| r[1].clone().unwrap()).collect();
        assert_eq!(small, ["0.05", "-0.50", "4.00"], "{var}");
        assert_eq!(loaded[1][5].as_deref(), Some("4"), "{var}");
        assert_eq!(loaded[0][5], None, "{var}");
        assert_eq!(
            loaded[0][2].as_deref(),
            Some("1234567890123456789012345678.1234567891"),
            "{var}"
        );
        // No false conflict, cell by cell.
        for id in ["1", "2", "3"] {
            let row = row_of(&table, &loaded, id);
            for (i, value) in [
                (1, "9.99"),
                (2, "-3.25"),
                (3, "2.5"),
                (4, "0.25"),
                (5, "11"),
            ] {
                assert_eq!(
                    edit(&s, &table, &row, &name(i), value).await,
                    Applied::Committed(1),
                    "{var}: row {id}, {} loaded as {:?}",
                    name(i),
                    row[i].1
                );
            }
        }
        // Real conflicts: the last digit of a large-scale decimal, a float, a decimal below 1.
        run(&s, &format!("delete from {t}")).await;
        for v in VALUES {
            run(&s, &format!("insert into {t} values {v}")).await;
        }
        if d == Dialect::Oracle {
            run(&s, "commit").await;
        }
        let loaded = rows(&run(&s, &select).await);
        let row = row_of(&table, &loaded, "1");
        run(
            &s,
            &format!(
                "update {t} set big = 1234567890123456789012345678.1234567892, single = 0.2, \
                 small = 0.06 where id = 1"
            ),
        )
        .await;
        if d == Dialect::Oracle {
            run(&s, "commit").await;
        }
        for i in [1, 2, 3] {
            assert_eq!(
                edit(&s, &table, &row, &name(i), "1").await,
                Applied::RolledBack {
                    index: Some(0),
                    failure: Failure::Conflict(0)
                },
                "{var}: {} changed meanwhile",
                name(i)
            );
        }
        // Deleting compares every loaded value, NULL included.
        for id in ["2", "3"] {
            let mut edits = EditSet::new(EditTarget::new(&table, d).unwrap());
            edits.delete_row(&row_of(&table, &loaded, id)).unwrap();
            assert_eq!(
                edits.apply(&s).await,
                Applied::Committed(1),
                "{var}: delete {id}"
            );
        }
        if d == Dialect::SqlServer {
            // SCOPE_IDENTITY() is a DECIMAL(38,0).
            let one = rows(
                &run(
                    &s,
                    "select cast(4 as decimal(38,0)), cast(1.5 as decimal(9,3))",
                )
                .await,
            );
            assert_eq!(one[0], [Some("4".into()), Some("1.500".into())]);
        }
        run(&s, &format!("drop table {t}")).await;
    }
}

/// SPEC-0089 v17 AK-33 (OBS-0120 X2): routines a script creates are in the catalog the tree
/// reads, as its tables are — MariaDB and MySQL between `DELIMITER` lines, SQL Server in its own
/// `GO` batch, Oracle ended with `/`, PostgreSQL in dollar quotes.
#[tokio::test]
async fn routines_a_script_creates_are_listed() {
    for var in SYSTEMS {
        let Some(s) = connect(var).await else {
            continue;
        };
        let d = s.dialect();
        let p = format!("pd_proc_{}", std::process::id());
        let f = format!("pd_func_{}", std::process::id());
        let script = match d {
            Dialect::MySql | Dialect::MariaDb => format!(
                "DELIMITER //\ncreate procedure {p}() begin select 1; end //\n\
                 create function {f}() returns int deterministic begin return 1; end //\nDELIMITER ;\n"
            ),
            Dialect::SqlServer => format!(
                "create procedure {p} as select 1\nGO\ncreate function {f}() returns int as begin return 1 end\nGO\n"
            ),
            Dialect::Oracle => format!(
                "create procedure {p} as begin null; end;\n/\n\
                 create function {f} return number as begin return 1; end;\n/\n"
            ),
            Dialect::PostgreSql => format!(
                "create procedure {p}() language plpgsql as $$ begin null; end $$;\n\
                 create function {f}() returns int language sql as $$ select 1 $$;\n"
            ),
            _ => continue,
        };
        run(&s, &script).await;
        let objects = schema_objects(&s, None).await.unwrap();
        let has = |name: &str, kind: &str| {
            objects
                .iter()
                .any(|o| o.name.eq_ignore_ascii_case(name) && o.kind == kind)
        };
        let ok = has(&p, "procedure") && has(&f, "function");
        let drop = format!("drop procedure {p};\ndrop function {f}");
        let _ = s.run_script(&drop, OnError::Continue).await;
        assert!(ok, "{var}: {objects:?}");
    }
}

/// SPEC-0142 v5 AK-10 (OBS-0120 O4): in auto-commit each statement is committed when it ran and
/// a ROLLBACK says it had nothing to undo; in a transaction opened as Manual opens it, changes
/// stay invisible to other sessions until COMMIT, and ROLLBACK undoes them.
#[tokio::test]
async fn oracle_transactions_auto_and_manual() {
    let (Some(s), Some(other)) = (
        connect("FERRIS_IDEA_TEST_ORACLE").await,
        connect("FERRIS_IDEA_TEST_ORACLE").await,
    ) else {
        return;
    };
    let t = format!("pd_tx_{}", std::process::id());
    let _ = s
        .run_script(&format!("drop table {t}"), OnError::Continue)
        .await;
    run(
        &s,
        &format!("create table {t} (id number primary key, v number)"),
    )
    .await;
    run(&s, &format!("insert into {t} values (1, 10)")).await;
    let seen = |sql: String| {
        let other = &other;
        async move { rows(&run(other, &sql).await)[0][0].clone() }
    };
    let value = || format!("select v from {t} where id = 1");
    // Auto: committed at once; ROLLBACK has nothing to undo and says so.
    let auto = run(
        &s,
        &format!("update {t} set v = 20 where id = 1;\nrollback"),
    )
    .await;
    assert_eq!(seen(value()).await.as_deref(), Some("20"));
    assert!(
        auto.outcomes[1]
            .notices
            .iter()
            .any(|n| n.contains("auto-commit is on")),
        "{auto:?}"
    );
    assert!(auto.outcomes[0].notices.is_empty());
    // Manual: kept until COMMIT, undone by ROLLBACK.
    let begin = begin_statements(Dialect::Oracle, Isolation::Default).unwrap();
    for sql in &begin {
        run(&s, sql).await;
    }
    run(&s, &format!("update {t} set v = 30 where id = 1")).await;
    assert_eq!(rows(&run(&s, &value()).await)[0][0].as_deref(), Some("30"));
    assert_eq!(
        seen(value()).await.as_deref(),
        Some("20"),
        "not visible before COMMIT"
    );
    let back = run(&s, "rollback").await;
    assert!(back.outcomes[0].notices.is_empty(), "{back:?}");
    assert_eq!(rows(&run(&s, &value()).await)[0][0].as_deref(), Some("20"));
    for sql in &begin {
        run(&s, sql).await;
    }
    run(&s, &format!("update {t} set v = 40 where id = 1")).await;
    assert_eq!(seen(value()).await.as_deref(), Some("20"));
    run(&s, "commit").await;
    assert_eq!(
        seen(value()).await.as_deref(),
        Some("40"),
        "visible after COMMIT"
    );
    run(&s, &format!("drop table {t}")).await;
}

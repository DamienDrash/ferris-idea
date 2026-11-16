//! SPEC-0020 against the reference databases: SQLite always, PostgreSQL/MySQL when
//! `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB` are set.

use crate::common;

use std::path::Path;

use idecraft_database::{
    ConnectionProfile, ExplainError, OnError, Outcome, PlanNode, Session, explain,
};

const GRID: &str = "select m.id, w.city, m.sku, i.name, m.delta from movement m \
    join warehouse w on w.id = m.warehouse_id join item i on i.sku = m.sku order by m.id";

async fn connect(url: &str) -> Session {
    let profile = ConnectionProfile::from_url("plan", url).unwrap();
    Session::connect(&profile, &common::credentials(&profile))
        .await
        .unwrap()
}

fn all(nodes: &[PlanNode]) -> Vec<&PlanNode> {
    let mut out = Vec::new();
    for n in nodes {
        out.push(n);
        out.extend(all(&n.children));
    }
    out
}

async fn delta_of_first(s: &Session) -> Option<String> {
    let run = s
        .run_script("select delta from movement where id = 1", OnError::Stop)
        .await;
    match &run.outcomes[0].outcome {
        Outcome::ResultSet { rows, .. } => rows[0][0].clone(),
        other => panic!("{other:?}"),
    }
}

async fn refusals(s: &Session) {
    assert_eq!(
        explain(s, "select 1; select 2", false).await.unwrap_err(),
        ExplainError::NotOneStatement(2)
    );
}

#[tokio::test]
async fn sqlite_plans() {
    let dir = std::env::temp_dir().join(format!("idecraft-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sql =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
    let seed: String = ["schema.sql", "seed.sql", "seed-movements.sqlite.sql"]
        .iter()
        .map(|f| std::fs::read_to_string(sql.join(f)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let file = dir.join("ref.sqlite");
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch(&seed)
        .unwrap();
    let s = connect(&format!("sqlite:{}", file.display())).await;
    let plan = explain(&s, GRID, false).await.unwrap();
    let nodes = all(&plan.nodes);
    assert!(
        nodes
            .iter()
            .any(|n| n.full_scan && n.object.as_deref() == Some("m")),
        "{plan:#?}"
    );
    assert!(nodes.iter().any(|n| n.operation == "SEARCH"), "{plan:#?}");
    assert!(!plan.hints.is_empty());
    assert_eq!(
        explain(&s, GRID, true).await.unwrap_err(),
        ExplainError::AnalyzeUnsupported
    );
    refusals(&s).await;
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

async fn server_plans(s: &Session) {
    server_plans_with(s, GRID).await;
}

/// `query` must make the optimizer read a big table whole.
async fn server_plans_with(s: &Session, query: &str) {
    let plan = explain(s, query, false).await.unwrap();
    assert!(!plan.analyzed);
    assert!(
        plan.hints.iter().any(|h| h.contains("full scan")),
        "{plan:#?}"
    );
    let measured = explain(s, "select count(*) from movement where delta > 5", true)
        .await
        .unwrap();
    assert!(measured.analyzed && !measured.rolled_back);
    let scan = all(&measured.nodes)
        .into_iter()
        .find(|n| n.full_scan)
        .cloned()
        .unwrap_or_else(|| panic!("{measured:#?}"));
    assert!(scan.actual_rows.is_some() && scan.actual_ms.is_some() && scan.loops.is_some());

    let before = delta_of_first(s).await;
    let update = explain(
        s,
        "update movement set delta = delta + 100 where id = 1",
        true,
    )
    .await
    .unwrap();
    assert!(update.rolled_back);
    assert_eq!(delta_of_first(s).await, before);
    assert_eq!(
        explain(s, "drop table movement", true).await.unwrap_err(),
        ExplainError::AnalyzeSchemaChange
    );
    refusals(s).await;
}

#[tokio::test]
async fn postgres_plans() {
    if let Ok(url) = std::env::var("IDECRAFT_TEST_PG") {
        server_plans(&connect(&url).await).await;
    }
}

#[tokio::test]
async fn mysql_plans() {
    if let Ok(url) = std::env::var("IDECRAFT_TEST_MYSQL") {
        let s = connect(&url).await;
        // With the reference schema's foreign keys (since #414) MySQL, like MariaDB, reaches the
        // grid's movements through `movement_warehouse_fk`, so the full scan is checked on a filter.
        s.run_script("analyze table movement", OnError::Stop).await;
        server_plans_with(&s, "select id, delta from movement where delta > 5").await;
    }
}

#[tokio::test]
async fn mariadb_plans() {
    if let Ok(url) = std::env::var("IDECRAFT_TEST_MARIADB") {
        let s = connect(&url).await;
        // MariaDB starts the grid's join from the two warehouses and reaches movements by index,
        // so the full scan is checked on a filter; fresh statistics give the row estimate.
        s.run_script("analyze table movement", OnError::Stop).await;
        server_plans_with(&s, "select id, delta from movement where delta > 5").await;
    }
}

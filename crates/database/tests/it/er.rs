//! SPEC-0022/AK-5: ER model of the SQLite reference schema; AK-8: golden Mermaid and PlantUML
//! exports of `sql-basic` (set `UPDATE_GOLDEN=1` to rewrite them after a reviewed change).

use std::path::Path;

use idecraft_database::{
    ConnectionProfile, Credentials, Session, er_model, er_model_with_views, introspect, to_mermaid,
    to_plantuml, view_definitions,
};

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(actual, expected, "{} differs", path.display());
}

#[tokio::test]
async fn sqlite_reference_er_model() {
    let dir = std::env::temp_dir().join(format!("idecraft-er-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sql =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
    let script: String = ["schema.sql", "seed.sql", "seed-movements.sqlite.sql"]
        .iter()
        .map(|f| std::fs::read_to_string(sql.join(f)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let file = dir.join("ref.sqlite");
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch(&script)
        .unwrap();
    let profile =
        ConnectionProfile::from_url("ref", &format!("sqlite:{}", file.display())).unwrap();
    let s = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    let model = er_model(&introspect(&s).await.unwrap(), None, 0).unwrap();

    let entities: Vec<(&str, usize)> = model
        .entities
        .iter()
        .map(|e| (e.name.as_str(), e.layer))
        .collect();
    assert_eq!(
        entities,
        [("item", 0), ("warehouse", 0), ("movement", 1), ("stock", 1)],
        "the view stock_value is not an entity"
    );
    let mut edges: Vec<(&str, &str)> = model
        .relationships
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();
    edges.sort();
    assert_eq!(
        edges,
        [
            ("movement", "item"),
            ("movement", "warehouse"),
            ("stock", "item"),
            ("stock", "warehouse")
        ]
    );
    assert!(model.relationships.iter().all(|r| r.many && !r.optional));
    assert!(model.outside.is_empty());
    assert!(to_mermaid(&model).contains("    warehouse ||--o{ stock : \"warehouse_id\"\n"));

    let tables = introspect(&s).await.unwrap();
    let views = view_definitions(&s).await.unwrap();
    assert_eq!(views.len(), 1);
    assert!(views[0].definition.starts_with("select s.warehouse_id"));
    let full = er_model_with_views(&tables, &views, None, 0).unwrap();
    let deps: Vec<(&str, &str)> = full
        .dependencies
        .iter()
        .map(|d| (d.view.as_str(), d.uses.as_str()))
        .collect();
    assert_eq!(deps, [("stock_value", "item"), ("stock_value", "stock")]);
    golden("sql-basic.mmd", &to_mermaid(&full));
    golden("sql-basic.puml", &to_plantuml(&full));
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

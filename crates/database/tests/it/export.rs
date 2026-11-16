//! SPEC-0021/AK-4: export round trips on the SQLite reference database.

use std::path::Path;

use idecraft_database::{
    ConnectionProfile, Credentials, Dialect, ExportFormat, ExportOptions, OnError, Outcome,
    Session, export,
};

type Rows = Vec<Vec<Option<String>>>;

async fn query(s: &Session, sql: &str) -> (Vec<String>, Rows) {
    let run = s.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { columns, rows }) => (columns, rows),
        other => panic!("{other:?}"),
    }
}

/// Minimal RFC 4180 reader: an unquoted empty field is NULL, `""` is the empty string.
fn parse_csv(text: &str) -> Rows {
    let mut rows = Vec::new();
    let (mut row, mut field, mut quoted, mut in_quotes) = (Vec::new(), String::new(), false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' => (in_quotes, quoted) = (true, true),
            ',' | '\r' => {
                if c == '\r' {
                    assert_eq!(chars.next(), Some('\n'));
                }
                let value = std::mem::take(&mut field);
                row.push((quoted || !value.is_empty()).then_some(value));
                quoted = false;
                if c == '\r' {
                    rows.push(std::mem::take(&mut row));
                }
            }
            c => field.push(c),
        }
    }
    rows
}

#[tokio::test]
async fn sqlite_export_round_trips() {
    let dir = std::env::temp_dir().join(format!("idecraft-export-{}", std::process::id()));
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
    let profile =
        ConnectionProfile::from_url("ref", &format!("sqlite:{}", file.display())).unwrap();
    let s = Session::connect(&profile, &Credentials::NotNeeded)
        .await
        .unwrap();
    let options = |format, table: Option<&str>| ExportOptions {
        format,
        header: true,
        table: table.map(str::to_owned),
        key: Vec::new(),
        dialect: Dialect::Sqlite,
    };

    // NULL and empty string side by side, plus every movement row.
    let (columns, rows) = query(
        &s,
        "select id, sku, null as n, '' as e, 'x,\"y\"' as q from movement order by id",
    )
    .await;
    let csv = export(&columns, &rows, &options(ExportFormat::Csv, None)).unwrap();
    let parsed = parse_csv(&csv);
    assert_eq!(parsed.len(), 10_001);
    assert_eq!(
        parsed[0],
        ["id", "sku", "n", "e", "q"].map(|c| Some(c.to_owned()))
    );
    assert_eq!(parsed[1..], rows[..]);

    // item as INSERTs into an empty copy.
    let (columns, rows) = query(
        &s,
        "select sku, name, unit_price_cents from item order by sku",
    )
    .await;
    let inserts = export(
        &columns,
        &rows,
        &options(ExportFormat::SqlInsert, Some("item_copy")),
    )
    .unwrap();
    let run = s
        .run_script(
            &format!("create table item_copy as select * from item where 0; {inserts}"),
            OnError::Stop,
        )
        .await;
    assert!(
        run.outcomes
            .iter()
            .all(|o| !matches!(o.outcome, Outcome::Error(_))),
        "{run:?}"
    );
    let (_, copied) = query(
        &s,
        "select sku, name, unit_price_cents from item_copy order by sku",
    )
    .await;
    assert_eq!(copied, rows);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

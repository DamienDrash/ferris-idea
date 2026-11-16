//! SPEC-0005 against the own reference project `tests/reference-projects/sql-basic`.

use idecraft_database::{Dialect, split_statements};

fn read(name: &str) -> String {
    let path = format!(
        "{}/../../tests/reference-projects/sql-basic/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn schema_has_four_statements() {
    let statements = split_statements(&read("schema.sql"), Dialect::PostgreSql);
    assert_eq!(statements.len(), 4);
    assert!(statements[0].text.starts_with("create table warehouse"));
    assert!(statements[3].text.starts_with("create view stock_value"));
}

#[test]
fn seed_keeps_literals_and_drops_trailing_comments() {
    let statements = split_statements(&read("seed.sql"), Dialect::Sqlite);
    assert_eq!(statements.len(), 3);
    assert!(statements[1].text.contains("'Bolt; zinc'"));
    assert!(statements[1].text.ends_with("10)"));
    assert!(statements[2].text.starts_with("insert into stock"));
}

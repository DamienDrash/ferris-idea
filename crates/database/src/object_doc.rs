//! Quick documentation of a table, view or column in the Database tool window (`SPEC-0089` v6
//! behaviour 13): what the tree already read, as a few lines, without asking the database again
//! and without waiting for any language engine (IMP-0367).

use crate::{ConnectionProfile, Index, Table};

/// The lines of the quick documentation of `table` (or of its column `column`) of `profile`:
/// a title (`table stock`, `column stock.qty`), where it lives, then columns or the column's
/// details, keys and indexes. `None` when the table or column is not in `tables`.
pub fn object_doc(
    profile: &ConnectionProfile,
    tables: &[Table],
    table: &str,
    column: Option<&str>,
) -> Option<Vec<String>> {
    let t = tables.iter().find(|t| t.name == table)?;
    let kind = if t.kind == "view" { "view" } else { "table" };
    let place = match &profile.database {
        Some(db) => format!("{} · {db}", profile.name),
        None => profile.name.clone(),
    };
    let fk_of = |c: &str| {
        t.foreign_keys
            .iter()
            .find(|fk| fk.columns.iter().any(|x| x == c))
            .map(|fk| {
                format!(
                    "→ {}({})",
                    fk.referenced_table,
                    fk.referenced_columns.join(", ")
                )
            })
    };
    if let Some(column) = column {
        let c = t.columns.iter().find(|c| c.name == column)?;
        let mut lines = vec![
            format!("column {}.{}", t.name, c.name),
            format!("{kind} {} in {place}", t.name),
            format!(
                "Type: {}{}",
                c.data_type,
                if c.nullable { "" } else { " NOT NULL" }
            ),
        ];
        if let Some(d) = &c.default {
            lines.push(format!("Default: {d}"));
        }
        if let Some(kind) = &c.generated {
            lines.push(format!("Generated ({kind})"));
        }
        if t.primary_key.iter().any(|k| k == &c.name) {
            lines.push(format!("Primary key ({})", t.primary_key.join(", ")));
        }
        if let Some(fk) = fk_of(&c.name) {
            lines.push(format!("Foreign key {fk}"));
        }
        for i in t.indexes.iter().filter(|i| i.columns.contains(&c.name)) {
            lines.push(index_line(i));
        }
        return Some(lines);
    }
    let mut lines = vec![
        format!("{kind} {}", t.name),
        format!("in {place} ({})", profile.dialect.label()),
    ];
    match &t.unreadable {
        // v9 behaviour 21: the database refused to describe it, so there are no columns.
        Some(reason) => lines.push(format!("Cannot be read: {reason}")),
        None => lines.push(format!("Columns ({}):", t.columns.len())),
    }
    for c in &t.columns {
        let mut marks = Vec::new();
        if t.primary_key.iter().any(|k| k == &c.name) {
            marks.push("PK".to_owned());
        }
        if let Some(fk) = fk_of(&c.name) {
            marks.push(fk);
        }
        if !c.nullable {
            marks.push("NOT NULL".into());
        }
        if let Some(d) = &c.default {
            marks.push(format!("default {d}"));
        }
        if let Some(kind) = &c.generated {
            marks.push(format!("generated ({kind})"));
        }
        let marks = if marks.is_empty() {
            String::new()
        } else {
            format!("  {}", marks.join(" · "))
        };
        lines.push(format!("  {} {}{marks}", c.name, c.data_type));
    }
    if !t.primary_key.is_empty() {
        lines.push(format!("Primary key: {}", t.primary_key.join(", ")));
    }
    for fk in &t.foreign_keys {
        lines.push(format!(
            "Foreign key {}({}) → {}({})",
            fk.name.as_deref().unwrap_or(""),
            fk.columns.join(", "),
            fk.referenced_table,
            fk.referenced_columns.join(", ")
        ));
    }
    let referenced: Vec<String> = tables
        .iter()
        .filter(|o| {
            o.foreign_keys
                .iter()
                .any(|fk| fk.referenced_table == t.name)
        })
        .map(|o| o.name.clone())
        .collect();
    if !referenced.is_empty() {
        lines.push(format!("Referenced by: {}", referenced.join(", ")));
    }
    for i in &t.indexes {
        lines.push(index_line(i));
    }
    Some(lines)
}

/// One index as a line: `Unique index name (a, b collate nocase) where condition` (v9
/// behaviour 20).
fn index_line(i: &Index) -> String {
    format!(
        "{} {} ({}){}",
        if i.unique { "Unique index" } else { "Index" },
        i.name,
        i.terms().join(", "),
        i.condition_line()
            .map(|c| format!(" where {c}"))
            .unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, ForeignKey};

    fn tables() -> Vec<Table> {
        let col = |name: &str, ty: &str, nullable: bool, default: Option<&str>| Column {
            name: name.into(),
            data_type: ty.into(),
            nullable,
            default: default.map(Into::into),
            generated: None,
        };
        vec![
            Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name: "item".into(),
                kind: "table".into(),
                columns: vec![col("sku", "text", false, None)],
                primary_key: vec!["sku".into()],
                foreign_keys: vec![],
                indexes: vec![],
                unreadable: None,
                constraints: Vec::new(),
            },
            Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name: "stock".into(),
                kind: "table".into(),
                columns: vec![
                    col("sku", "text", false, None),
                    col("qty", "integer", false, Some("0")),
                    Column {
                        generated: Some("stored".into()),
                        ..col("double_qty", "integer", true, None)
                    },
                ],
                primary_key: vec!["sku".into()],
                foreign_keys: vec![ForeignKey {
                    name: Some("stock_sku_fkey".into()),
                    columns: vec!["sku".into()],
                    referenced_table: "item".into(),
                    referenced_columns: vec!["sku".into()],
                }],
                indexes: vec![
                    Index {
                        name: "stock_qty".into(),
                        unique: false,
                        columns: vec!["qty".into()],
                        condition: None,
                        collations: Vec::new(),
                    },
                    Index {
                        name: "stock_open".into(),
                        unique: true,
                        columns: vec!["sku".into(), "qty".into()],
                        condition: Some("qty > 0\n  and sku <> ''".into()),
                        collations: vec![Some("NOCASE".into()), None],
                    },
                ],
                unreadable: None,
                constraints: Vec::new(),
            },
        ]
    }

    /// SPEC-0089 v6 AK-13: table and column documentation from what the tree read.
    #[test]
    fn documents_tables_and_columns() {
        let p = ConnectionProfile::from_url("ref-pg", "postgresql://u@h/inventory").unwrap();
        let doc = object_doc(&p, &tables(), "stock", None).unwrap();
        assert_eq!(doc[0], "table stock");
        assert_eq!(doc[1], "in ref-pg · inventory (PostgreSQL)");
        assert!(doc.contains(&"  sku text  PK · → item(sku) · NOT NULL".to_owned()));
        assert!(doc.contains(&"  qty integer  NOT NULL · default 0".to_owned()));
        // SPEC-0089 v7 AK-18: a generated column says so.
        assert!(doc.contains(&"  double_qty integer  generated (stored)".to_owned()));
        assert_eq!(
            object_doc(&p, &tables(), "stock", Some("double_qty")).unwrap()[2..],
            ["Type: integer", "Generated (stored)"]
        );
        assert!(doc.contains(&"Foreign key stock_sku_fkey(sku) → item(sku)".to_owned()));
        assert!(doc.contains(&"Index stock_qty (qty)".to_owned()));
        // SPEC-0089 v9 AK-21: collation and condition of an index, on one line.
        assert!(
            doc.contains(
                &"Unique index stock_open (sku collate nocase, qty) where qty > 0 and sku <> ''"
                    .to_owned()
            )
        );
        let item = object_doc(&p, &tables(), "item", None).unwrap();
        assert!(item.contains(&"Referenced by: stock".to_owned()));
        let qty = object_doc(&p, &tables(), "stock", Some("qty")).unwrap();
        assert_eq!(
            qty,
            [
                "column stock.qty",
                "table stock in ref-pg · inventory",
                "Type: integer NOT NULL",
                "Default: 0",
                "Index stock_qty (qty)",
                "Unique index stock_open (sku collate nocase, qty) where qty > 0 and sku <> ''",
            ]
        );
        assert!(object_doc(&p, &tables(), "nope", None).is_none());
        assert!(object_doc(&p, &tables(), "stock", Some("nope")).is_none());
    }

    /// SPEC-0089 v9 AK-22: a view the database refuses to describe says why instead of
    /// listing no columns.
    #[test]
    fn a_view_that_cannot_be_read_says_so() {
        let p = ConnectionProfile::from_url("lite", "sqlite:///tmp/shop.db").unwrap();
        let broken = Table {
            storage: Vec::new(),
            name: "orphan".into(),
            kind: "view".into(),
            columns: vec![],
            primary_key: vec![],
            foreign_keys: vec![],
            indexes: vec![],
            unreadable: Some("no such table: main.gone".into()),
            constraints: Vec::new(),
            hidden_columns: vec![],
        };
        let doc = object_doc(&p, &[broken], "orphan", None).unwrap();
        assert_eq!(doc[0], "view orphan");
        assert_eq!(doc[2..], ["Cannot be read: no such table: main.gone"]);
    }
}

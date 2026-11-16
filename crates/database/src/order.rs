//! Stable result order and identifier quoting (`SPEC-0007`, behaviour 7–8).
//!
//! Sorting by columns that are not unique leaves ties in arbitrary order, so
//! paging through such a result can repeat or skip rows. A unique key of the
//! source is appended as a tie-breaker.

use crate::Dialect;

/// Sort direction of one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Smallest first.
    Asc,
    /// Largest first.
    Desc,
}

/// One `ORDER BY` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    /// Column name, unquoted.
    pub column: String,
    /// Sort direction.
    pub direction: Direction,
}

impl SortKey {
    /// Ascending sort on `column`.
    pub fn asc(column: &str) -> Self {
        Self {
            column: column.to_owned(),
            direction: Direction::Asc,
        }
    }

    /// Descending sort on `column`.
    pub fn desc(column: &str) -> Self {
        Self {
            column: column.to_owned(),
            direction: Direction::Desc,
        }
    }
}

/// A sort order and whether it is guaranteed to be total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableOrder {
    /// Requested keys followed by any tie-breaker keys.
    pub keys: Vec<SortKey>,
    /// `false` when no unique key was known, so ties may stay unordered.
    pub stable: bool,
}

/// Extends `requested` with a tie-breaker taken from the source's unique keys.
///
/// `unique_keys` lists the column sets of the primary key first, then other
/// unique keys. If the requested columns already contain one of them, nothing
/// is added. Otherwise the first key's missing columns are appended in
/// ascending order.
pub fn stable_order(requested: &[SortKey], unique_keys: &[Vec<String>]) -> StableOrder {
    let sorted = |column: &String| requested.iter().any(|k| &k.column == column);
    if unique_keys.iter().any(|key| key.iter().all(sorted)) {
        return StableOrder {
            keys: requested.to_vec(),
            stable: true,
        };
    }
    match unique_keys.first() {
        Some(key) => {
            let mut keys = requested.to_vec();
            keys.extend(
                key.iter()
                    .filter(|column| !sorted(column))
                    .map(|column| SortKey::asc(column)),
            );
            StableOrder { keys, stable: true }
        }
        None => StableOrder {
            keys: requested.to_vec(),
            stable: false,
        },
    }
}

/// Quotes an identifier for `dialect`, doubling embedded quote characters.
pub fn quote_identifier(name: &str, dialect: Dialect) -> String {
    let (open, close) = match dialect {
        Dialect::MySql | Dialect::MariaDb => ('`', '`'),
        Dialect::SqlServer => ('[', ']'),
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::Oracle | Dialect::DuckDb => ('"', '"'),
    };
    let mut out = String::with_capacity(name.len() + 2);
    out.push(open);
    for c in name.chars() {
        if c == close {
            out.push(close);
        }
        out.push(c);
    }
    out.push(close);
    out
}

/// Renders `keys` as an `ORDER BY` clause; empty keys give an empty string.
pub fn order_by_clause(keys: &[SortKey], dialect: Dialect) -> String {
    if keys.is_empty() {
        return String::new();
    }
    let items: Vec<String> = keys
        .iter()
        .map(|k| {
            let column = quote_identifier(&k.column, dialect);
            match k.direction {
                Direction::Asc => column,
                Direction::Desc => format!("{column} desc"),
            }
        })
        .collect();
    format!("order by {}", items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(columns: &[&str]) -> Vec<String> {
        columns.iter().map(|c| (*c).to_owned()).collect()
    }

    #[test]
    fn appends_primary_key_as_tie_breaker() {
        let order = stable_order(&[SortKey::asc("sku")], &[key(&["id"])]);
        assert_eq!(order.keys, [SortKey::asc("sku"), SortKey::asc("id")]);
        assert!(order.stable);

        let by_id = stable_order(&[SortKey::desc("id")], &[key(&["id"])]);
        assert_eq!(by_id.keys, [SortKey::desc("id")]);

        let composite = stable_order(&[SortKey::asc("sku")], &[key(&["warehouse_id", "sku"])]);
        assert_eq!(
            composite.keys,
            [SortKey::asc("sku"), SortKey::asc("warehouse_id")]
        );
    }

    #[test]
    fn any_covered_unique_key_is_enough() {
        // Sorting on a unique column that is not the primary key adds nothing.
        let order = stable_order(&[SortKey::asc("email")], &[key(&["id"]), key(&["email"])]);
        assert_eq!(order.keys, [SortKey::asc("email")]);
        assert!(order.stable);
    }

    #[test]
    fn without_unique_key_order_is_marked_unstable() {
        let order = stable_order(&[SortKey::asc("sku")], &[]);
        assert_eq!(order.keys, [SortKey::asc("sku")]);
        assert!(!order.stable);
    }

    #[test]
    fn quotes_identifiers_per_dialect() {
        assert_eq!(quote_identifier("sku", Dialect::PostgreSql), "\"sku\"");
        assert_eq!(quote_identifier("a\"b", Dialect::Sqlite), "\"a\"\"b\"");
        assert_eq!(quote_identifier("a`b", Dialect::MySql), "`a``b`");
        assert_eq!(quote_identifier("a`b", Dialect::MariaDb), "`a``b`");
        assert_eq!(quote_identifier("a]b", Dialect::SqlServer), "[a]]b]");
        assert_eq!(
            order_by_clause(&[SortKey::asc("sku"), SortKey::desc("id")], Dialect::MySql),
            "order by `sku`, `id` desc"
        );
        assert_eq!(order_by_clause(&[], Dialect::PostgreSql), "");
    }
}

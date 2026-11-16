//! The query of a table data editor (`SPEC-0190` behaviours 3–4): `WHERE` and `ORDER BY` from the
//! user's fields, pages of rows and the count, for the window and for agents.

use crate::{Dialect, Direction, quote_identifier};

/// Rows per page of a table data editor.
pub const TABLE_PAGE: usize = 500;

/// Why a field's text is not used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClauseError {
    /// A `;` outside quotes would end the statement.
    #[error("`{0}` must not contain `;`: it would end the statement")]
    Semicolon(&'static str),
    /// A comment would hide the rest of the query.
    #[error("`{0}` must not contain a comment (`--` or `/*`)")]
    Comment(&'static str),
    /// A quote or bracket is not closed.
    #[error("`{0}` has an unclosed quote or parenthesis")]
    Unbalanced(&'static str),
}

/// Checks the text of the `WHERE` or `ORDER BY` field (`field`) (behaviour 3): no `;` and no
/// comment outside quotes, every quote and parenthesis closed.
pub fn check_clause(text: &str, field: &'static str, dialect: Dialect) -> Result<(), ClauseError> {
    let mut chars = text.chars().peekable();
    let mut depth = 0i32;
    while let Some(c) = chars.next() {
        let close = match c {
            '\'' | '"' => Some(c),
            '`' if dialect.mysql_like() => Some('`'),
            '[' if dialect == Dialect::SqlServer => Some(']'),
            _ => None,
        };
        if let Some(close) = close {
            // A doubled closing character stays inside the quoted text.
            loop {
                match chars.next() {
                    None => return Err(ClauseError::Unbalanced(field)),
                    Some('\\') if close == '\'' && dialect.mysql_like() => {
                        chars.next();
                    }
                    Some(ch) if ch == close => {
                        if chars.peek() == Some(&close) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    Some(_) => {}
                }
            }
            continue;
        }
        match c {
            ';' => return Err(ClauseError::Semicolon(field)),
            '-' if chars.peek() == Some(&'-') => return Err(ClauseError::Comment(field)),
            '/' if chars.peek() == Some(&'*') => return Err(ClauseError::Comment(field)),
            '#' if dialect.mysql_like() => return Err(ClauseError::Comment(field)),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err(ClauseError::Unbalanced(field));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(ClauseError::Unbalanced(field));
    }
    Ok(())
}

/// What a data editor shows of its table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableQuery {
    /// Schema, when not the connection's current one.
    pub schema: Option<String>,
    /// Table or view name.
    pub table: String,
    /// Body of the `WHERE` clause; empty for none.
    pub filter: String,
    /// Body of the `ORDER BY` clause; empty for the key order.
    pub order: String,
    /// Key columns, the order when `order` is empty (behaviour 4).
    pub key: Vec<String>,
}

impl TableQuery {
    /// The query of `table` with its `key`.
    pub fn new(table: &str, key: Vec<String>) -> Self {
        Self {
            table: table.to_owned(),
            key,
            ..Self::default()
        }
    }

    /// The quoted, possibly schema-qualified table name.
    pub fn table_name(&self, d: Dialect) -> String {
        match &self.schema {
            Some(s) if !s.is_empty() => {
                format!(
                    "{}.{}",
                    quote_identifier(s, d),
                    quote_identifier(&self.table, d)
                )
            }
            _ => quote_identifier(&self.table, d),
        }
    }

    /// Whether pages are stable: an explicit order (the user's choice) or the key.
    pub fn stable(&self) -> bool {
        !self.order.trim().is_empty() || !self.key.is_empty()
    }

    fn checked(&self, d: Dialect) -> Result<(String, String), ClauseError> {
        check_clause(&self.filter, "WHERE", d)?;
        check_clause(&self.order, "ORDER BY", d)?;
        let filter = match self.filter.trim() {
            "" => String::new(),
            w => format!(" WHERE {w}"),
        };
        let order = match self.order.trim() {
            "" if self.key.is_empty() => String::new(),
            "" => format!(
                " ORDER BY {}",
                self.key
                    .iter()
                    .map(|k| quote_identifier(k, d))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            o => format!(" ORDER BY {o}"),
        };
        Ok((filter, order))
    }

    /// All rows with the filter and order, for exports (SPEC-0021 v2 behaviour 11).
    pub fn all_sql(&self, d: Dialect) -> Result<String, ClauseError> {
        let (filter, order) = self.checked(d)?;
        Ok(format!(
            "SELECT * FROM {}{filter}{order}",
            self.table_name(d)
        ))
    }

    /// Page `page` (0-based) of `size` rows (behaviour 4).
    pub fn page_sql(&self, d: Dialect, page: usize, size: usize) -> Result<String, ClauseError> {
        let (filter, mut order) = self.checked(d)?;
        let offset = page * size;
        let limit = match d {
            Dialect::SqlServer | Dialect::Oracle => {
                if order.is_empty() && d == Dialect::SqlServer {
                    order = " ORDER BY (SELECT NULL)".into();
                }
                format!(" OFFSET {offset} ROWS FETCH NEXT {size} ROWS ONLY")
            }
            _ => format!(" LIMIT {size} OFFSET {offset}"),
        };
        Ok(format!(
            "SELECT * FROM {}{filter}{order}{limit}",
            self.table_name(d)
        ))
    }

    /// The number of rows with the filter (behaviour 4).
    pub fn count_sql(&self, d: Dialect) -> Result<String, ClauseError> {
        let (filter, _) = self.checked(d)?;
        Ok(format!(
            "SELECT COUNT(*) FROM {}{filter}",
            self.table_name(d)
        ))
    }
}

/// The `ORDER BY` text for header clicks (behaviour 3): sort keys as quoted columns with `DESC`
/// where descending.
pub fn order_text(columns: &[String], sort: &[(usize, Direction)], d: Dialect) -> String {
    sort.iter()
        .filter_map(|(k, dir)| {
            let name = quote_identifier(columns.get(*k)?, d);
            Some(match dir {
                Direction::Asc => name,
                Direction::Desc => format!("{name} DESC"),
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Number of pages for `total` rows, at least one.
pub fn page_count(total: u64, size: usize) -> usize {
    (total as usize).div_ceil(size.max(1)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_per_dialect() {
        let mut q = TableQuery::new("t", vec!["id".into()]);
        assert_eq!(
            q.page_sql(Dialect::PostgreSql, 1, 500).unwrap(),
            "SELECT * FROM \"t\" ORDER BY \"id\" LIMIT 500 OFFSET 500"
        );
        q.filter = "name = 'a;b'".into();
        q.order = "`name` DESC".into();
        assert_eq!(
            q.page_sql(Dialect::MySql, 0, 500).unwrap(),
            "SELECT * FROM `t` WHERE name = 'a;b' ORDER BY `name` DESC LIMIT 500 OFFSET 0"
        );
        assert_eq!(
            q.count_sql(Dialect::MySql).unwrap(),
            "SELECT COUNT(*) FROM `t` WHERE name = 'a;b'"
        );
        let mut plain = TableQuery::new("t", Vec::new());
        assert!(!plain.stable());
        assert_eq!(
            plain.page_sql(Dialect::SqlServer, 2, 10).unwrap(),
            "SELECT * FROM [t] ORDER BY (SELECT NULL) OFFSET 20 ROWS FETCH NEXT 10 ROWS ONLY"
        );
        assert_eq!(
            plain.page_sql(Dialect::Oracle, 0, 10).unwrap(),
            "SELECT * FROM \"t\" OFFSET 0 ROWS FETCH NEXT 10 ROWS ONLY"
        );
        plain.schema = Some("other".into());
        assert_eq!(
            plain.all_sql(Dialect::Sqlite).unwrap(),
            "SELECT * FROM \"other\".\"t\""
        );
        assert_eq!(page_count(0, 500), 1);
        assert_eq!(page_count(1200, 500), 3);
    }

    #[test]
    fn fields_that_would_end_or_hide_the_query_are_refused() {
        let d = Dialect::PostgreSql;
        assert_eq!(
            check_clause("1=1; drop table t", "WHERE", d),
            Err(ClauseError::Semicolon("WHERE"))
        );
        assert_eq!(
            check_clause("1=1 --", "WHERE", d),
            Err(ClauseError::Comment("WHERE"))
        );
        assert_eq!(
            check_clause("a /* x */", "WHERE", d),
            Err(ClauseError::Comment("WHERE"))
        );
        assert_eq!(
            check_clause("name = 'it''s", "WHERE", d),
            Err(ClauseError::Unbalanced("WHERE"))
        );
        assert_eq!(
            check_clause("(a = 1", "WHERE", d),
            Err(ClauseError::Unbalanced("WHERE"))
        );
        assert!(check_clause("name = 'a;b' and x = 'it''s -- no'", "WHERE", d).is_ok());
        assert!(check_clause("\"we;ird\" > 1", "WHERE", d).is_ok());
        assert!(check_clause("a = 'x\\'; y'", "WHERE", Dialect::MySql).is_ok());
        assert!(check_clause("a # b", "WHERE", Dialect::MySql).is_err());
        let mut q = TableQuery::new("t", vec![]);
        q.order = "1; drop table t".into();
        assert!(q.page_sql(d, 0, 500).is_err());
    }

    #[test]
    fn header_clicks_become_order_text() {
        let columns = vec!["id".to_owned(), "name".to_owned()];
        assert_eq!(
            order_text(
                &columns,
                &[(1, Direction::Desc), (0, Direction::Asc)],
                Dialect::MySql
            ),
            "`name` DESC, `id`"
        );
        assert_eq!(order_text(&columns, &[], Dialect::MySql), "");
    }
}

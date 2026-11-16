//! Value summaries of a result column with an explicit scope (`SPEC-0007`, behaviour 9–10).

use std::collections::HashMap;
use std::fmt;

/// Which rows a summary was computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Only the rows loaded into the grid.
    Loaded {
        /// Number of loaded rows.
        loaded: usize,
        /// Total rows of the result, if known.
        total: Option<u64>,
    },
    /// All rows, counted by the database.
    All,
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Loaded {
                loaded,
                total: Some(total),
            } => write!(f, "{loaded} loaded rows of {total}"),
            Scope::Loaded {
                loaded,
                total: None,
            } => write!(f, "{loaded} loaded rows"),
            Scope::All => f.write_str("all rows"),
        }
    }
}

/// Distinct values of a column with their counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueSummary {
    /// Rows the counts are based on.
    pub scope: Scope,
    /// Values (`None` is NULL) with counts, most frequent first, then by value.
    pub values: Vec<(Option<String>, u64)>,
}

/// Summarises the loaded values of one column. `total` is the result's row
/// count if it is known.
pub fn summarize_loaded(values: &[Option<String>], total: Option<u64>) -> ValueSummary {
    let mut counts: HashMap<&Option<String>, u64> = HashMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    let mut values: Vec<(Option<String>, u64)> = counts
        .into_iter()
        .map(|(value, count)| (value.clone(), count))
        .collect();
    values.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ValueSummary {
        scope: Scope::Loaded {
            loaded: values.iter().map(|(_, c)| *c as usize).sum(),
            total,
        },
        values,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_summary_states_scope_and_sorts_by_count_then_value() {
        let mut column: Vec<Option<String>> = Vec::new();
        column.extend(std::iter::repeat_n(Some("A-1".to_owned()), 300));
        column.extend(std::iter::repeat_n(Some("B-2".to_owned()), 100));
        column.extend(std::iter::repeat_n(Some("C-3".to_owned()), 50));
        column.extend(std::iter::repeat_n(None, 50));

        let summary = summarize_loaded(&column, Some(10_000));
        assert_eq!(summary.scope.to_string(), "500 loaded rows of 10000");
        assert_eq!(
            summary.values,
            [
                (Some("A-1".to_owned()), 300),
                (Some("B-2".to_owned()), 100),
                (None, 50),
                (Some("C-3".to_owned()), 50),
            ]
        );
        assert_eq!(
            summarize_loaded(&column[..10], None).scope.to_string(),
            "10 loaded rows"
        );
        assert_eq!(Scope::All.to_string(), "all rows");
    }
}

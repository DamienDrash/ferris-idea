//! Editing a query result in place (`SPEC-0107`): changed cells, added and deleted rows of one
//! result, kept apart from the loaded rows until they are submitted as an [`EditSet`].

use std::collections::{BTreeMap, BTreeSet};

use crate::{EditError, EditSet, EditTarget, Values};

/// How a cell of the grid is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellState {
    /// As loaded.
    Loaded,
    /// Changed, not yet submitted.
    Changed,
    /// In a row that will be deleted.
    Deleted,
    /// In a row that will be added.
    Added,
}

/// Pending changes of one result whose rows come from one table (behaviours 2–5).
#[derive(Debug, Clone)]
pub struct GridEdits {
    target: EditTarget,
    columns: Vec<String>,
    /// New values by (loaded row, column).
    changed: BTreeMap<(usize, usize), Option<String>>,
    deleted: BTreeSet<usize>,
    /// Rows to add, one value per result column (`None` leaves the column to its default).
    added: Vec<Vec<Option<String>>>,
}

impl GridEdits {
    /// Edits of a result with `columns` from `target`'s table. Every key column must be in the
    /// result, so each row can be found again (behaviour 1).
    pub fn new(target: EditTarget, columns: Vec<String>) -> Result<Self, EditError> {
        if let Some(missing) = target.key.iter().find(|k| !columns.contains(k)) {
            return Err(EditError::MissingKey(missing.clone()));
        }
        Ok(Self {
            target,
            columns,
            changed: BTreeMap::new(),
            deleted: BTreeSet::new(),
            added: Vec::new(),
        })
    }

    /// The table, dialect and row identity of the edits.
    pub fn target(&self) -> &EditTarget {
        &self.target
    }

    /// The table being edited.
    pub fn table(&self) -> &str {
        &self.target.table
    }

    /// Whether nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.deleted.is_empty() && self.added.is_empty()
    }

    /// Number of pending changes: changed rows, deleted rows and added rows.
    pub fn count(&self) -> usize {
        let rows: BTreeSet<usize> = self
            .changed
            .keys()
            .map(|(r, _)| *r)
            .filter(|r| !self.deleted.contains(r))
            .collect();
        rows.len() + self.deleted.len() + self.added.len()
    }

    /// Rows to add, after the loaded rows.
    pub fn added(&self) -> &[Vec<Option<String>>] {
        &self.added
    }

    /// The value shown in loaded row `row`, column `col`: the pending one if changed.
    pub fn value<'a>(
        &'a self,
        loaded: &'a [Vec<Option<String>>],
        row: usize,
        col: usize,
    ) -> Option<&'a Option<String>> {
        if let Some(v) = self.changed.get(&(row, col)) {
            return Some(v);
        }
        loaded.get(row).and_then(|r| r.get(col))
    }

    /// How cell (`row`, `col`) is shown; rows past `loaded_rows` are added ones.
    pub fn state(&self, loaded_rows: usize, row: usize, col: usize) -> CellState {
        if row >= loaded_rows {
            CellState::Added
        } else if self.deleted.contains(&row) {
            CellState::Deleted
        } else if self.changed.contains_key(&(row, col)) {
            CellState::Changed
        } else {
            CellState::Loaded
        }
    }

    /// Whether column `col` can be edited: it is a column of the table (computed columns are not).
    pub fn editable(&self, col: usize) -> bool {
        self.columns
            .get(col)
            .is_some_and(|c| self.target.has_column(c))
    }

    /// The declared type of result column `col`, if it is a column of the table.
    pub fn column_type(&self, col: usize) -> Option<&str> {
        let name = self.columns.get(col)?;
        self.target.column_type(name)
    }

    /// Sets cell (`row`, `col`) to `value`; rows past the loaded ones are added rows. Setting a
    /// loaded cell back to its loaded value drops the change (behaviour 2).
    pub fn set(
        &mut self,
        loaded: &[Vec<Option<String>>],
        row: usize,
        col: usize,
        value: Option<String>,
    ) -> Result<(), EditError> {
        if !self.editable(col) {
            return Err(EditError::UnknownColumn {
                table: self.target.table.clone(),
                column: self.columns.get(col).cloned().unwrap_or_default(),
            });
        }
        match row.checked_sub(loaded.len()) {
            Some(i) => {
                if let Some(r) = self.added.get_mut(i) {
                    r[col] = value;
                }
            }
            None => {
                if loaded[row].get(col) == Some(&value) {
                    self.changed.remove(&(row, col));
                } else {
                    self.changed.insert((row, col), value);
                }
            }
        }
        Ok(())
    }

    /// Adds an empty row and returns its grid row (behaviour 3).
    pub fn add_row(&mut self, loaded_rows: usize) -> usize {
        self.added.push(vec![None; self.columns.len()]);
        loaded_rows + self.added.len() - 1
    }

    /// Marks loaded row `row` for deletion, or forgets an added row; again unmarks it
    /// (behaviour 4).
    pub fn toggle_delete(&mut self, loaded_rows: usize, row: usize) {
        match row.checked_sub(loaded_rows) {
            Some(i) if i < self.added.len() => {
                self.added.remove(i);
            }
            Some(_) => {}
            None => {
                if !self.deleted.remove(&row) {
                    self.deleted.insert(row);
                }
            }
        }
    }

    /// Drops every pending change (behaviour 5).
    pub fn revert(&mut self) {
        self.changed.clear();
        self.deleted.clear();
        self.added.clear();
    }

    fn row_values(&self, loaded: &[Vec<Option<String>>], row: usize) -> Values {
        self.columns
            .iter()
            .enumerate()
            .filter(|(_, c)| self.target.has_column(c))
            .map(|(i, c)| (c.clone(), loaded[row].get(i).cloned().flatten()))
            .collect()
    }

    /// The pending changes as one [`EditSet`]: deletes, then updates, then inserts (v2, `SPEC-0190`
    /// behaviour 7). Updates and deletes match the loaded values, so rows changed by someone else
    /// meanwhile are conflicts (behaviour 5).
    pub fn edit_set(&self, loaded: &[Vec<Option<String>>]) -> Result<EditSet, EditError> {
        let mut set = EditSet::new(self.target.clone());
        for &row in &self.deleted {
            set.delete_row(&self.row_values(loaded, row))?;
        }
        for (&(row, col), value) in &self.changed {
            if !self.deleted.contains(&row) {
                set.set_cell(
                    &self.row_values(loaded, row),
                    &self.columns[col],
                    value.clone(),
                )?;
            }
        }
        for values in &self.added {
            let given: Values = self
                .columns
                .iter()
                .zip(values)
                .filter(|(_, v)| v.is_some())
                .map(|(c, v)| (c.clone(), v.clone()))
                .collect();
            set.insert_row(given)?;
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, Dialect, Table};

    fn table() -> Table {
        let column = |name: &str, nullable: bool| Column {
            name: name.into(),
            data_type: "text".into(),
            nullable,
            default: None,
            generated: None,
            expression: None,
            identity: None,
            comment: None,
        };
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: "users".into(),
            kind: "table".into(),
            columns: vec![column("id", false), column("name", true)],
            primary_key: vec!["id".into()],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            unreadable: None,
            constraints: Vec::new(),
            comment: None,
        }
    }

    fn some(s: &str) -> Option<String> {
        Some(s.to_owned())
    }

    #[test]
    fn pending_changes_become_one_edit_set() {
        let target = EditTarget::new(&table(), Dialect::Sqlite).unwrap();
        let columns = vec!["id".to_owned(), "name".to_owned(), "upper".to_owned()];
        assert!(
            GridEdits::new(target.clone(), vec!["name".into()]).is_err(),
            "key missing"
        );
        let loaded = vec![
            vec![some("1"), some("ann"), some("ANN")],
            vec![some("2"), some("bob"), some("BOB")],
        ];
        let mut g = GridEdits::new(target, columns).unwrap();
        assert!(!g.editable(2), "computed column");
        assert!(g.set(&loaded, 0, 2, some("X")).is_err());
        g.set(&loaded, 0, 1, some("anna")).unwrap();
        assert_eq!(g.state(2, 0, 1), CellState::Changed);
        assert_eq!(g.value(&loaded, 0, 1), Some(&some("anna")));
        g.set(&loaded, 0, 1, some("ann")).unwrap();
        assert!(g.is_empty(), "back to the loaded value");
        g.set(&loaded, 0, 1, None).unwrap();
        g.toggle_delete(2, 1);
        assert_eq!(g.state(2, 1, 0), CellState::Deleted);
        let added = g.add_row(2);
        assert_eq!(added, 2);
        g.set(&loaded, added, 0, some("3")).unwrap();
        g.set(&loaded, added, 1, some("cy")).unwrap();
        assert_eq!(g.state(2, 2, 0), CellState::Added);
        assert_eq!(g.count(), 3);
        let sql = g.edit_set(&loaded).unwrap().sql();
        assert_eq!(sql.len(), 3, "{sql:?}");
        // Deletes first, then updates, then inserts (SPEC-0190 behaviour 7).
        assert!(sql[0].starts_with("DELETE FROM \"users\""), "{sql:?}");
        assert!(
            sql[1].starts_with("UPDATE \"users\" SET \"name\" = NULL"),
            "{sql:?}"
        );
        assert!(sql[2].starts_with("INSERT INTO \"users\""), "{sql:?}");
        g.toggle_delete(2, 2);
        assert!(g.added().is_empty(), "deleting an added row forgets it");
        g.revert();
        assert!(g.is_empty());
    }
}

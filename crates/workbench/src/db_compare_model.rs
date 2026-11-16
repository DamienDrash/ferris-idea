//! Query plans and schema comparison in the window (`SPEC-0110`): the rows of the Plan tab, the lines
//! of the Schema Diff tab, the migration script and its file. v2: which statements go into the
//! script, destructive ones only after a confirmation that names what is lost.

use std::path::{Path, PathBuf};

use idecraft_database::{
    Column, ConnectionProfile, DdlStatement, Dialect, ForeignKey, Index, Plan, PlanNode,
    SchemaDiff, TableConstraint, comment_text, migration_script_in, toggle_statement,
};

/// One step of a plan as a row of the Plan tab (behaviour 2).
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRow {
    /// Nesting depth, 0 for a top-level step.
    pub depth: usize,
    /// Operation, e.g. `Seq Scan`.
    pub operation: String,
    /// Table or index, or empty.
    pub object: String,
    /// Conditions and keys joined by `; `.
    pub details: String,
    /// The same conditions and keys one by one (v5 behaviour 11).
    pub detail_lines: Vec<String>,
    /// Estimated rows and cost, e.g. `rows 10 000 · cost 180.5`, or empty.
    pub estimate: String,
    /// Measured rows, time and loops, or empty when not measured.
    pub actual: String,
    /// Whether the step reads a whole table.
    pub full_scan: bool,
}

/// The plan's steps in tree order, parents before children.
pub fn plan_rows(plan: &Plan) -> Vec<PlanRow> {
    fn walk(node: &PlanNode, depth: usize, out: &mut Vec<PlanRow>) {
        let mut estimate = Vec::new();
        if let Some(r) = node.estimated_rows {
            estimate.push(format!("rows {}", number(r)));
        }
        if let Some(c) = node.cost {
            estimate.push(format!("cost {}", number(c)));
        }
        let mut actual = Vec::new();
        if let Some(r) = node.actual_rows {
            actual.push(format!("rows {}", number(r)));
        }
        if let Some(ms) = node.actual_ms {
            actual.push(format!("{} ms", number(ms)));
        }
        if let Some(l) = node.loops {
            actual.push(format!("loops {}", number(l)));
        }
        out.push(PlanRow {
            depth,
            operation: node.operation.clone(),
            object: node.object.clone().unwrap_or_default(),
            details: node.details.join("; "),
            detail_lines: node.details.clone(),
            estimate: estimate.join(" · "),
            actual: actual.join(" · "),
            full_scan: node.full_scan,
        });
        for child in &node.children {
            walk(child, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for node in &plan.nodes {
        walk(node, 0, &mut out);
    }
    out
}

/// Width of the Tree view's object column for `rows` with characters `char_width` wide: as wide as
/// its longest text plus 16 px, between 180 and 480 px, so an index name such as
/// `PK__warehous__3213E83F…` stays whole (`SPEC-0110` behaviour 2, hand test 43).
pub fn plan_object_width(rows: &[PlanRow], char_width: f32) -> f32 {
    rows.iter()
        .map(|r| r.object.chars().count() as f32 * char_width + 16.)
        .fold(180f32, f32::max)
        .min(480.)
}

/// The plan as copied from the Tree view (`SPEC-0110` v5 behaviour 11): one line per step,
/// indented by two spaces per level, with object, estimate, measured values and details.
pub fn plan_text(plan: &Plan) -> String {
    let mut out = String::new();
    for r in plan_rows(plan) {
        let mut line = format!("{}{}", "  ".repeat(r.depth), r.operation);
        for part in [&r.object, &r.estimate] {
            if !part.is_empty() {
                line.push_str(&format!("  {part}"));
            }
        }
        if !r.actual.is_empty() {
            line.push_str(&format!("  actual {}", r.actual));
        }
        if !r.details.is_empty() {
            line.push_str(&format!("  [{}]", r.details));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The selected step written out in full (`SPEC-0110` v5 behaviour 11): operation and object,
/// every detail on its own line, then the estimate and the measured values.
pub fn step_lines(row: &PlanRow) -> Vec<String> {
    let mut out = vec![if row.object.is_empty() {
        row.operation.clone()
    } else {
        format!("{} on {}", row.operation, row.object)
    }];
    out.extend(row.detail_lines.iter().map(|d| format!("  {d}")));
    if !row.estimate.is_empty() {
        out.push(format!("Estimated: {}", row.estimate));
    }
    if !row.actual.is_empty() {
        out.push(format!("Measured: {}", row.actual));
    }
    if row.full_scan {
        out.push("Reads the whole table".into());
    }
    out
}

/// The step or statement a marker moves to with ↓ (`down`) or ↑ in a list of `len` entries:
/// from none ↓ marks the first and ↑ the last; the ends hold (`SPEC-0110` v5 behaviours 11, 14).
pub fn move_marker(at: Option<usize>, len: usize, down: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (at, down) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1).min(len - 1),
        (Some(i), false) => i.saturating_sub(1).min(len - 1),
    })
}

/// A number with thin grouping and at most two decimals: `10000.0` → `10 000`, `0.125` → `0.13`.
fn number(v: f64) -> String {
    let text = if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    };
    let (int, frac) = text
        .split_once('.')
        .map_or((text.as_str(), None), |(i, f)| (i, Some(f)));
    let (sign, digits) = int.strip_prefix('-').map_or(("", int), |d| ("-", d));
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// How a line of the Schema Diff tab changes the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Only in the source: the script adds it.
    Added,
    /// Only in the target: the script removes it.
    Removed,
    /// In both, different.
    Changed,
}

impl Mark {
    /// The sign in front of the line.
    pub fn sign(self) -> &'static str {
        match self {
            Mark::Added => "+",
            Mark::Removed => "−",
            Mark::Changed => "~",
        }
    }
}

/// One line of the Schema Diff tab (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// 0 for a table, 1 for a part of a table.
    pub depth: usize,
    /// Kind of change.
    pub mark: Mark,
    /// What changed, e.g. `column qty integer NOT NULL`.
    pub text: String,
}

fn column_text(c: &Column) -> String {
    let mut s = format!("{} {}", c.name, c.data_type);
    if !c.nullable {
        s.push_str(" NOT NULL");
    }
    if let Some(d) = &c.default {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    s
}

fn index_text(i: &Index) -> String {
    format!(
        "{}index {} ({}){}",
        if i.unique { "unique " } else { "" },
        i.name,
        i.terms().join(", "),
        i.condition_line()
            .map(|c| format!(" where {c}"))
            .unwrap_or_default()
    )
}

fn foreign_key_text(f: &ForeignKey) -> String {
    format!(
        "foreign key ({}) → {} ({})",
        f.columns.join(", "),
        f.referenced_table,
        f.referenced_columns.join(", ")
    )
}

/// The differences as lines: added, removed, then changed tables with their parts.
pub fn diff_lines(diff: &SchemaDiff) -> Vec<DiffLine> {
    let line = |depth, mark, text: String| DiffLine { depth, mark, text };
    let mut out = Vec::new();
    for t in &diff.added {
        out.push(line(0, Mark::Added, format!("table {t}")));
    }
    for t in &diff.removed {
        out.push(line(0, Mark::Removed, format!("table {t}")));
    }
    for v in &diff.views_added {
        out.push(line(0, Mark::Added, format!("view {v}")));
    }
    for v in &diff.views_removed {
        out.push(line(0, Mark::Removed, format!("view {v}")));
    }
    for v in &diff.views_changed {
        out.push(line(0, Mark::Changed, format!("view {v} (definition)")));
    }
    for t in &diff.changed {
        out.push(line(0, Mark::Changed, format!("table {}", t.name)));
        for c in &t.columns_added {
            out.push(line(1, Mark::Added, format!("column {}", column_text(c))));
        }
        for c in &t.columns_removed {
            out.push(line(1, Mark::Removed, format!("column {}", column_text(c))));
        }
        for c in &t.columns_changed {
            out.push(line(
                1,
                Mark::Changed,
                format!("column {} → {}", column_text(&c.from), column_text(&c.to)),
            ));
        }
        if let Some((from, to)) = &t.primary_key {
            out.push(line(
                1,
                Mark::Changed,
                format!("primary key ({}) → ({})", from.join(", "), to.join(", ")),
            ));
        }
        // SPEC-0018 v9 behaviour 21.
        if let Some((from, to)) = &t.storage {
            let list = |p: &[String]| {
                if p.is_empty() {
                    "none".to_owned()
                } else {
                    p.join(", ")
                }
            };
            out.push(line(
                1,
                Mark::Changed,
                format!("storage ({}) → ({})", list(from), list(to)),
            ));
        }
        for i in &t.indexes_added {
            out.push(line(1, Mark::Added, index_text(i)));
        }
        for i in &t.indexes_removed {
            out.push(line(1, Mark::Removed, index_text(i)));
        }
        for f in &t.foreign_keys_added {
            out.push(line(1, Mark::Added, foreign_key_text(f)));
        }
        for f in &t.foreign_keys_removed {
            out.push(line(1, Mark::Removed, foreign_key_text(f)));
        }
        // Checks and unique constraints (SPEC-0110 v10 behaviour 4).
        for c in &t.constraints_added {
            out.push(line(1, Mark::Added, constraint_text(c)));
        }
        for c in &t.constraints_removed {
            out.push(line(1, Mark::Removed, constraint_text(c)));
        }
    }
    out
}

fn constraint_text(c: &TableConstraint) -> String {
    format!("constraint {} {}", c.name, c.definition)
}

/// The migration script: a header naming both sides on one line, then the statements as
/// `migration_script_in` writes them for the `selected` ones, pinned to `target_schema` when it is
/// known (behaviours 4, 7 and 16; SPEC-0018 v7 behaviour 19).
pub fn script_text(
    source: &str,
    target: &str,
    statements: &[DdlStatement],
    selected: &[bool],
    dialect: Dialect,
    target_schema: Option<&str>,
) -> String {
    let mut out = format!(
        "-- Turns `{}` into `{}` (generated by IDECraft, not run).\n",
        comment_text(target),
        comment_text(source)
    );
    if statements.is_empty() {
        out.push_str("-- No differences.\n");
    }
    out.push_str(&migration_script_in(
        statements,
        selected,
        dialect,
        target_schema,
    ));
    out
}

/// The statements of a comparison and which of them go into the script (behaviour 7).
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationChoice {
    /// Source connection.
    pub source: String,
    /// Target connection.
    pub target: String,
    /// Dialect of both.
    pub dialect: Dialect,
    /// The migration's statements in order.
    pub statements: Vec<DdlStatement>,
    /// Which are in the script.
    pub selected: Vec<bool>,
    /// Statements waiting for the confirmation of their data loss.
    pub pending: Option<Vec<usize>>,
    /// The target's schema, which the script pins (`SPEC-0110` v11 behaviour 16, `SPEC-0018` v7
    /// behaviour 19), also when it is the target connection's current one.
    pub target_schema: Option<String>,
}

impl MigrationChoice {
    /// Everything that is not destructive is selected.
    pub fn new(
        source: &str,
        target: &str,
        dialect: Dialect,
        statements: Vec<DdlStatement>,
    ) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            dialect,
            selected: statements
                .iter()
                .map(DdlStatement::selected_by_default)
                .collect(),
            statements,
            pending: None,
            target_schema: None,
        }
    }

    /// The same choice whose script pins the target schema `name` (`SPEC-0110` v11 behaviour 16):
    /// the schema chosen for the target, or the name of its connection's current one.
    pub fn in_schema(mut self, name: &str) -> Self {
        self.target_schema = Some(name.to_owned());
        self
    }

    /// A click on statement `i`: a selected one (and its group) is left out; an unselected one is
    /// selected, unless it is destructive, which waits for [`Self::confirm`]. True when the
    /// selection changed.
    pub fn click(&mut self, i: usize) -> bool {
        let Some(s) = self.statements.get(i) else {
            return false;
        };
        if self.selected[i] {
            self.selected = toggle_statement(&self.statements, &self.selected, i, false);
            self.pending = None;
            return true;
        }
        if s.destructive.is_some() {
            let group = s.group;
            self.pending = Some(
                (0..self.statements.len())
                    .filter(|k| *k == i || (group.is_some() && self.statements[*k].group == group))
                    .filter(|k| !self.selected[*k])
                    .collect(),
            );
            return false;
        }
        self.selected = toggle_statement(&self.statements, &self.selected, i, true);
        true
    }

    /// Asks to include every destructive statement left out (`Sql.IncludeDestructiveChanges`).
    pub fn include_all(&mut self) {
        let left: Vec<usize> = (0..self.statements.len())
            .filter(|k| !self.selected[*k] && self.statements[*k].destructive.is_some())
            .collect();
        self.pending = (!left.is_empty()).then_some(left);
    }

    /// The confirmation line for the pending statements: what each loses.
    pub fn confirmation(&self) -> Option<String> {
        let pending = self.pending.as_ref()?;
        let mut lost: Vec<String> = Vec::new();
        for k in pending {
            let s = &self.statements[*k];
            let first = s.sql.lines().next().unwrap_or_default();
            if let Some(what) = &s.destructive
                && (s.data_loss || s.sql.starts_with("DROP VIEW") || s.group.is_none())
            {
                lost.push(format!("{first}: {what}"));
            }
        }
        if lost.is_empty() {
            lost.push("the statements change data".into());
        }
        Some(format!("Include? This loses data — {}", lost.join("; ")))
    }

    /// Includes the pending statements.
    pub fn confirm(&mut self) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        for k in pending {
            self.selected[k] = true;
        }
        true
    }

    /// Drops the pending request.
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// The script for the current choice.
    pub fn script(&self) -> String {
        script_text(
            &self.source,
            &self.target,
            &self.statements,
            &self.selected,
            self.dialect,
            self.target_schema.as_deref(),
        )
    }
}

/// One side of a comparison: a connection and a schema, `None` for the connection's current one
/// (`SPEC-0110` v7 behaviour 16).
#[derive(Debug, Clone, PartialEq)]
pub struct CompareSide {
    /// The connection.
    pub profile: ConnectionProfile,
    /// The schema; `None` is the connection's current schema.
    pub schema: Option<String>,
    /// One table or view of the schema compared alone (`SPEC-0110` v12 behaviour 18); `None`
    /// compares the whole schema.
    pub object: Option<String>,
}

impl CompareSide {
    /// The connection's current schema.
    pub fn current(profile: ConnectionProfile) -> Self {
        Self {
            profile,
            schema: None,
            object: None,
        }
    }

    /// The same side narrowed to `object` (`SPEC-0110` v12 behaviour 18).
    pub fn with_object(mut self, object: impl Into<String>) -> Self {
        self.object = Some(object.into());
        self
    }

    /// How the side is named: `<connection>` or `<connection>.<schema>`, followed by `.<object>`
    /// for a single object.
    pub fn label(&self) -> String {
        let side = match &self.schema {
            Some(schema) => format!("{}.{schema}", self.profile.name),
            None => self.profile.name.clone(),
        };
        match &self.object {
            Some(object) => format!("{side}.{object}"),
            None => side,
        }
    }
}

/// The target connections of Compare With… (`SPEC-0110` v12 behaviour 18): the source's own
/// connection first, then the other connections of its dialect.
pub fn object_targets<'a>(
    profiles: &'a [ConnectionProfile],
    source: &'a ConnectionProfile,
) -> Vec<&'a ConnectionProfile> {
    let mut out = vec![source];
    out.extend(compare_targets(profiles, source));
    out
}

/// The objects offered as the target of Compare With… (`SPEC-0110` v12 behaviour 18): the tables
/// or views of `kind` in the target schema, one named like the source first, the source object
/// itself left out when both sides are the same schema.
pub fn object_choices(
    tables: &[idecraft_database::Table],
    kind: &str,
    source_object: &str,
    same_schema: bool,
) -> Vec<String> {
    let mut names: Vec<String> = tables
        .iter()
        .filter(|t| t.kind == kind)
        .filter(|t| !(same_schema && t.name == source_object))
        .map(|t| t.name.clone())
        .collect();
    names.sort_by_key(|n| n != source_object);
    names
}

/// Whether two schemas of one connection can be compared in `dialect` (`SPEC-0110` v7 behaviour
/// 16).
pub fn schemas_comparable(dialect: Dialect) -> bool {
    dialect.switches_schema()
}

/// A target of the second popup (`SPEC-0110` v7 behaviour 16).
#[derive(Debug, Clone, PartialEq)]
pub enum CompareTarget {
    /// Another schema of the source's own connection.
    OwnConnection,
    /// Another connection of the same dialect.
    Connection(Box<ConnectionProfile>),
}

/// The targets with their popup labels: with `other_schemas`, the source's own connection first,
/// then the other connections of its dialect (behaviours 3 and 16).
pub fn target_choices(
    profiles: &[ConnectionProfile],
    source: &ConnectionProfile,
    other_schemas: bool,
) -> Vec<(String, CompareTarget)> {
    let mut out = Vec::new();
    if other_schemas && schemas_comparable(source.dialect) {
        out.push((
            format!("{} · other schema", source.name),
            CompareTarget::OwnConnection,
        ));
    }
    out.extend(compare_targets(profiles, source).into_iter().map(|p| {
        (
            target_label(p),
            CompareTarget::Connection(Box::new(p.clone())),
        )
    }));
    out
}

/// The status line when there is nothing to compare `source` with (`SPEC-0110` v7 behaviour 17).
pub fn nothing_to_compare(source: &ConnectionProfile) -> String {
    format!(
        "Nothing to compare `{}` with: no other {} connection and no other schema",
        source.name,
        source.dialect.label()
    )
}

/// The connections a schema can be compared with: same dialect as `source`, other name (behaviour 3).
pub fn compare_targets<'a>(
    profiles: &'a [ConnectionProfile],
    source: &ConnectionProfile,
) -> Vec<&'a ConnectionProfile> {
    profiles
        .iter()
        .filter(|p| p.dialect == source.dialect && p.name != source.name)
        .collect()
}

/// How a compare target is listed: its name and where it points, e.g. `staging · db.example:5432/app`
/// or `copy · /w/copy.db` (SPEC-0110 v3 behaviour 3).
pub fn target_label(p: &ConnectionProfile) -> String {
    let place = match (&p.path, &p.host) {
        (Some(path), _) => path.display().to_string(),
        (None, Some(host)) => {
            let port = p.port.map(|n| format!(":{n}")).unwrap_or_default();
            let db = p
                .database
                .as_deref()
                .map(|d| format!("/{d}"))
                .unwrap_or_default();
            format!("{host}{port}{db}")
        }
        (None, None) => String::new(),
    };
    if place.is_empty() {
        p.name.clone()
    } else {
        format!("{} · {place}", p.name)
    }
}

/// Where the script goes: `.idecraft/migrations/<source>-to-<target>.sql` with characters other than
/// letters, digits, `-` and `_` replaced by `_` (behaviour 5).
pub fn script_path(root: &Path, source: &str, target: &str) -> PathBuf {
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    root.join(".idecraft").join("migrations").join(format!(
        "{}-to-{}.sql",
        safe(source),
        safe(target)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_database::{ColumnChange, TableDiff};

    /// SPEC-0110 v12 AK-22 (OBS-0068 L8): the targets and objects of Compare With….
    #[test]
    fn object_targets_and_labels() {
        let a = profile("a", "postgresql://u@localhost/a");
        let b = profile("b", "postgresql://u@localhost/b");
        let lite = profile("lite", "sqlite:///tmp/x.db");
        let all = [a.clone(), b.clone(), lite];
        let names: Vec<&str> = object_targets(&all, &a)
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["a", "b"], "own connection first, same dialect only");
        let side = CompareSide::current(a).with_object("item");
        assert_eq!(side.label(), "a.item");
        let t = |name: &str, kind: &str| idecraft_database::Table {
            name: name.into(),
            kind: kind.into(),
            ..Default::default()
        };
        let tables = [
            t("archive", "table"),
            t("item", "table"),
            t("item_v", "view"),
            t("zeta", "table"),
        ];
        assert_eq!(
            object_choices(&tables, "table", "item", false),
            ["item", "archive", "zeta"]
        );
        assert_eq!(
            object_choices(&tables, "table", "item", true),
            ["archive", "zeta"]
        );
        assert_eq!(
            object_choices(&tables, "view", "item_v", true),
            Vec::<String>::new()
        );
    }

    /// SPEC-0110 v7 AK-16.
    #[test]
    fn sides_are_named_with_their_schema() {
        let pg = profile("pg", "postgresql://u@localhost/db");
        assert_eq!(CompareSide::current(pg.clone()).label(), "pg");
        let side = CompareSide {
            profile: pg,
            schema: Some("audit".into()),
            object: None,
        };
        assert_eq!(side.label(), "pg.audit");
        assert_eq!(
            script_path(Path::new("/w"), &side.label(), "pg"),
            Path::new("/w/.idecraft/migrations/pg_audit-to-pg.sql")
        );
    }

    /// SPEC-0110 v7 AK-16, v11 AK-21: the script pins its target schema, also the target
    /// connection's current one.
    #[test]
    fn the_script_starts_in_the_target_schema() {
        let create = || {
            vec![DdlStatement {
                sql: "CREATE TABLE extra (id integer)".into(),
                data_loss: false,
                destructive: None,
                group: None,
            }]
        };
        let choice = MigrationChoice::new("pg.new", "pg.old", Dialect::PostgreSql, create())
            .in_schema("old");
        let script = choice.script();
        let lines: Vec<&str> = script.lines().collect();
        assert!(
            lines[0].starts_with("-- Turns `pg.old` into `pg.new`"),
            "{script}"
        );
        assert_eq!(
            lines[1..4],
            ["", "BEGIN;", "SET LOCAL search_path TO \"old\", pg_temp;"],
            "{script}"
        );
        // The current schema of the target connection (`pg`, read as `public`) too (v11).
        let current =
            MigrationChoice::new("pg2", "pg", Dialect::PostgreSql, create()).in_schema("public");
        assert!(
            current
                .script()
                .contains("\nBEGIN;\nSET LOCAL search_path TO \"public\", pg_temp;\n"),
            "{}",
            current.script()
        );
        let my = MigrationChoice::new("a", "b", Dialect::MySql, create()).in_schema("shop");
        let my_script = my.script();
        let lines: Vec<&str> = my_script.lines().collect();
        assert!(
            lines[1].starts_with("-- This database commits"),
            "{my_script}"
        );
        assert!(
            lines[2].starts_with("-- The next statement makes the target schema current"),
            "{my_script}"
        );
        assert_eq!(lines[3], "USE `shop`;");
        // Without a known schema (as before a comparison read it) and on SQLite: none.
        let unknown = MigrationChoice::new("a", "b", Dialect::PostgreSql, create());
        assert!(!unknown.script().contains("search_path"));
        let lite = MigrationChoice::new("a", "b", Dialect::Sqlite, create()).in_schema("main");
        assert!(!lite.script().contains("main"), "{}", lite.script());
        // A line break in a side's name stays inside the head line.
        let odd = MigrationChoice::new("a", "pg.x\nDROP TABLE t;", Dialect::PostgreSql, create())
            .in_schema("old");
        let text = odd.script();
        assert!(
            text.starts_with("-- Turns `pg.x DROP TABLE t;` into `a`"),
            "{text}"
        );
        assert!(!text.lines().any(|l| l.starts_with("DROP")), "{text}");
    }

    /// SPEC-0110 v7 AK-16.
    #[test]
    fn targets_offer_the_own_connection_first() {
        let pg = profile("pg", "postgresql://u@localhost/db");
        let profiles = [
            pg.clone(),
            profile("staging", "postgresql://u@stage/db"),
            profile("my", "mysql://u@localhost/shop"),
        ];
        let labels: Vec<String> = target_choices(&profiles, &pg, true)
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(labels, ["pg · other schema", "staging · stage/db"]);
        assert_eq!(target_choices(&profiles, &pg, false).len(), 1);
        let lite = profile("lite", "sqlite:///tmp/lite.db");
        assert!(target_choices(std::slice::from_ref(&lite), &lite, true).is_empty());
    }

    /// SPEC-0110 v7 AK-17.
    #[test]
    fn nothing_to_compare_names_the_dialect() {
        assert_eq!(
            nothing_to_compare(&profile("pg", "postgresql://u@localhost/db")),
            "Nothing to compare `pg` with: no other PostgreSQL connection and no other schema"
        );
    }

    fn node(op: &str, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            operation: op.into(),
            children,
            ..PlanNode::default()
        }
    }

    #[test]
    fn plan_rows_follow_the_tree() {
        let scan = PlanNode {
            object: Some("movement m".into()),
            details: vec!["m.qty > 5".into(), "rows removed".into()],
            estimated_rows: Some(10_000.0),
            cost: Some(180.5),
            actual_rows: Some(9_990.0),
            actual_ms: Some(1.234),
            loops: Some(1.0),
            full_scan: true,
            ..node("Seq Scan", vec![])
        };
        let plan = Plan {
            nodes: vec![node(
                "Hash Join",
                vec![scan, node("Hash", vec![node("Index Scan", vec![])])],
            )],
            analyzed: true,
            rolled_back: false,
            hints: vec![],
            raw: String::new(),
        };
        let rows = plan_rows(&plan);
        let shape: Vec<_> = rows
            .iter()
            .map(|r| (r.depth, r.operation.as_str()))
            .collect();
        assert_eq!(
            shape,
            [
                (0, "Hash Join"),
                (1, "Seq Scan"),
                (1, "Hash"),
                (2, "Index Scan")
            ]
        );
        let scan = &rows[1];
        assert_eq!(scan.object, "movement m");
        assert_eq!(scan.details, "m.qty > 5; rows removed");
        assert_eq!(scan.estimate, "rows 10 000 · cost 180.50");
        assert_eq!(scan.actual, "rows 9 990 · 1.23 ms · loops 1");
        assert!(scan.full_scan);
        assert_eq!(rows[0].estimate, "");
        assert_eq!(rows[0].actual, "");
        assert_eq!(number(1_234_567.0), "1 234 567");
        assert_eq!(number(-1500.0), "-1 500");
    }

    /// SPEC-0110/AK-10 (v5).
    #[test]
    fn plan_text_and_step_lines() {
        let scan = PlanNode {
            object: Some("movement m".into()),
            details: vec!["m.qty > 5".into(), "rows removed".into()],
            estimated_rows: Some(10_000.0),
            cost: Some(180.5),
            actual_rows: Some(9_990.0),
            actual_ms: Some(1.5),
            loops: Some(1.0),
            full_scan: true,
            ..node("Seq Scan", vec![])
        };
        let plan = Plan {
            nodes: vec![node("Hash Join", vec![scan])],
            analyzed: true,
            rolled_back: false,
            hints: vec![],
            raw: String::new(),
        };
        assert_eq!(
            plan_text(&plan),
            "Hash Join\n  Seq Scan  movement m  rows 10 000 · cost 180.50  actual rows 9 990 · 1.50 ms · loops 1  [m.qty > 5; rows removed]\n"
        );
        let rows = plan_rows(&plan);
        assert_eq!(
            step_lines(&rows[1]),
            [
                "Seq Scan on movement m",
                "  m.qty > 5",
                "  rows removed",
                "Estimated: rows 10 000 · cost 180.50",
                "Measured: rows 9 990 · 1.50 ms · loops 1",
                "Reads the whole table",
            ]
        );
        assert_eq!(step_lines(&rows[0]), ["Hash Join"]);
    }

    /// SPEC-0110/AK-10 (v5).
    #[test]
    fn the_statement_marker_moves_within_the_list() {
        assert_eq!(move_marker(None, 3, true), Some(0));
        assert_eq!(move_marker(None, 3, false), Some(2));
        assert_eq!(move_marker(Some(0), 3, true), Some(1));
        assert_eq!(move_marker(Some(2), 3, true), Some(2));
        assert_eq!(move_marker(Some(0), 3, false), Some(0));
        assert_eq!(move_marker(Some(5), 3, false), Some(2));
        assert_eq!(move_marker(None, 0, true), None);
    }

    fn col(name: &str, ty: &str, nullable: bool) -> Column {
        Column {
            name: name.into(),
            data_type: ty.into(),
            nullable,
            default: None,
            generated: None,
        }
    }

    #[test]
    fn diff_lines_list_tables_and_their_parts() {
        let diff = SchemaDiff {
            added: vec!["audit".into()],
            removed: vec!["legacy".into()],
            changed: vec![TableDiff {
                name: "item".into(),
                columns_added: vec![Column {
                    default: Some("0".into()),
                    generated: None,
                    ..col("qty", "integer", false)
                }],
                columns_removed: vec![col("note", "text", true)],
                columns_changed: vec![ColumnChange {
                    from: col("name", "varchar(20)", true),
                    to: col("name", "varchar(80)", false),
                }],
                primary_key: Some((vec!["id".into()], vec!["id".into(), "sku".into()])),
                storage: Some((Vec::new(), vec!["autovacuum_enabled=false".into()])),
                indexes_added: vec![Index {
                    name: "item_sku".into(),
                    unique: true,
                    columns: vec!["sku".into()],
                    condition: None,
                    collations: Vec::new(),
                }],
                indexes_removed: vec![],
                foreign_keys_added: vec![ForeignKey {
                    name: None,
                    columns: vec!["warehouse_id".into()],
                    referenced_table: "warehouse".into(),
                    referenced_columns: vec!["id".into()],
                }],
                foreign_keys_removed: vec![],
                constraints_added: vec![TableConstraint {
                    name: "item_qty_check".into(),
                    kind: "check".into(),
                    columns: vec![],
                    definition: "CHECK (qty >= 0)".into(),
                }],
                constraints_removed: vec![TableConstraint {
                    name: "item_name_key".into(),
                    kind: "unique".into(),
                    columns: vec!["name".into()],
                    definition: "UNIQUE (name)".into(),
                }],
            }],
            views_added: vec!["totals".into()],
            views_removed: vec!["old_totals".into()],
            views_changed: vec!["stock_value".into()],
        };
        let lines: Vec<String> = diff_lines(&diff)
            .iter()
            .map(|l| format!("{}{} {}", "  ".repeat(l.depth), l.mark.sign(), l.text))
            .collect();
        assert_eq!(
            lines,
            [
                "+ table audit",
                "− table legacy",
                "+ view totals",
                "− view old_totals",
                "~ view stock_value (definition)",
                "~ table item",
                "  + column qty integer NOT NULL DEFAULT 0",
                "  − column note text",
                "  ~ column name varchar(20) → name varchar(80) NOT NULL",
                "  ~ primary key (id) → (id, sku)",
                "  ~ storage (none) → (autovacuum_enabled=false)",
                "  + unique index item_sku (sku)",
                "  + foreign key (warehouse_id) → warehouse (id)",
                "  + constraint item_qty_check CHECK (qty >= 0)",
                "  − constraint item_name_key UNIQUE (name)",
            ]
        );
        assert!(diff_lines(&SchemaDiff::default()).is_empty());
    }

    fn statements() -> Vec<DdlStatement> {
        vec![
            DdlStatement {
                sql: "DROP VIEW totals".into(),
                data_loss: false,
                destructive: Some("view totals (its definition; it holds no rows)".into()),
                group: Some(0),
            },
            DdlStatement {
                sql: "DROP TABLE legacy".into(),
                data_loss: true,
                destructive: Some("all rows of table legacy".into()),
                group: Some(0),
            },
            DdlStatement {
                sql: "CREATE TABLE audit (id integer)".into(),
                data_loss: false,
                destructive: None,
                group: None,
            },
        ]
    }

    #[test]
    fn script_marks_data_loss_and_ends_statements() {
        let st = statements();
        assert_eq!(
            script_text(
                "dev",
                "prod",
                &st,
                &[true, true, true],
                Dialect::MySql,
                None
            ),
            "-- Turns `prod` into `dev` (generated by IDECraft, not run).\n\
             -- This database commits each DDL statement on its own; these statements cannot be rolled back together.\n\n\
             -- Can lose data: view totals (its definition; it holds no rows).\nDROP VIEW totals;\n\n\
             -- Can lose data: all rows of table legacy.\nDROP TABLE legacy;\n\n\
             CREATE TABLE audit (id integer);\n"
        );
        assert!(
            script_text("a", "b", &[], &[], Dialect::Sqlite, None)
                .ends_with("-- No differences.\n")
        );
    }

    /// SPEC-0110/AK-4: default choice, confirmation naming what is lost, script per choice.
    #[test]
    fn destructive_statements_need_a_confirmation() {
        let mut c = MigrationChoice::new("dev", "prod", Dialect::PostgreSql, statements());
        assert_eq!(c.selected, [false, false, true]);
        let script = c.script();
        assert!(
            script
                .contains("-- Left out (loses all rows of table legacy):\n-- DROP TABLE legacy\n")
        );
        assert!(
            script.contains("\nBEGIN;\n")
                && script.ends_with("CREATE TABLE audit (id integer);\n\nCOMMIT;\n")
        );
        assert!(!c.click(1), "a destructive statement waits");
        assert_eq!(c.pending, Some(vec![0, 1]), "with its group");
        let text = c.confirmation().unwrap();
        assert!(
            text.contains("DROP TABLE legacy: all rows of table legacy"),
            "{text}"
        );
        assert!(text.contains("DROP VIEW totals: view totals"), "{text}");
        c.cancel();
        assert_eq!(c.selected, [false, false, true]);
        c.click(0);
        assert!(c.confirm());
        assert_eq!(c.selected, [true, true, true]);
        assert!(
            c.script()
                .contains("-- Can lose data: all rows of table legacy.\nDROP TABLE legacy;")
        );
        assert!(c.click(1), "leaving one out leaves its group out");
        assert_eq!(c.selected, [false, false, true]);
        assert!(
            c.click(2) && c.selected == [false, false, false],
            "plain statements toggle at once"
        );
        assert!(c.click(2));
        c.include_all();
        assert_eq!(c.pending, Some(vec![0, 1]));
        c.confirm();
        assert_eq!(c.selected, [true, true, true]);
        c.include_all();
        assert_eq!(c.pending, None, "nothing left to include");
    }

    /// SPEC-0110 v10 AK-20 (OBS-0068 L6): the foreign key drop of a dropped table, and a kept
    /// table's key to it, wait for the table's question and go with it.
    #[test]
    fn statements_tied_to_a_drop_ask_with_it() {
        use idecraft_database::{Table, migration};
        let table = |name: &str, fk: Option<&str>| Table {
            name: name.into(),
            kind: "table".into(),
            columns: vec![col("id", "integer", false), col("ref_id", "integer", true)],
            primary_key: vec!["id".into()],
            foreign_keys: fk
                .map(|t| ForeignKey {
                    name: Some(format!("{name}_ref_fkey")),
                    columns: vec!["ref_id".into()],
                    referenced_table: t.into(),
                    referenced_columns: vec!["id".into()],
                })
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let source = [table("item", None), table("kept", None)];
        let target = [
            table("item", None),
            table("kept", Some("movement")),
            table("movement", Some("item")),
        ];
        let statements = migration(&source, &target, Dialect::PostgreSql);
        let sql: Vec<&str> = statements.iter().map(|s| s.sql.as_str()).collect();
        assert_eq!(
            sql,
            [
                "ALTER TABLE \"movement\" DROP CONSTRAINT \"movement_ref_fkey\"",
                "ALTER TABLE \"kept\" DROP CONSTRAINT \"kept_ref_fkey\"",
                "DROP TABLE \"movement\"",
            ]
        );
        let mut c = MigrationChoice::new("pd_b", "public", Dialect::PostgreSql, statements);
        assert_eq!(
            c.selected,
            [false, false, false],
            "nothing weakens a kept table"
        );
        assert!(
            c.script().contains(
                "-- Left out (loses all rows of table movement):\n-- ALTER TABLE \"kept\""
            ),
            "{}",
            c.script()
        );
        assert!(!c.click(1), "the kept table's key waits for the question");
        assert_eq!(c.pending, Some(vec![0, 1, 2]));
        let text = c.confirmation().unwrap();
        assert_eq!(
            text,
            "Include? This loses data — DROP TABLE \"movement\": all rows of table movement"
        );
        assert!(c.confirm());
        assert_eq!(c.selected, [true, true, true]);
        assert!(c.click(0), "leaving one out leaves the group out");
        assert_eq!(c.selected, [false, false, false]);
    }

    fn profile(name: &str, url: &str) -> ConnectionProfile {
        ConnectionProfile::from_url(name, url).unwrap()
    }

    #[test]
    fn targets_share_the_dialect_and_paths_are_file_safe() {
        let profiles = [
            profile("dev", "sqlite:///tmp/dev.db"),
            profile("pg", "postgresql://u@localhost/db"),
            profile("copy", "sqlite:///tmp/copy.db"),
        ];
        let names: Vec<_> = compare_targets(&profiles, &profiles[0])
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["copy"]);
        // D13: the popup names where each target points.
        assert_eq!(target_label(&profiles[2]), "copy · /tmp/copy.db");
        assert_eq!(target_label(&profiles[1]), "pg · localhost/db");
        assert_eq!(
            script_path(Path::new("/w"), "my db", "prod/eu"),
            Path::new("/w/.idecraft/migrations/my_db-to-prod_eu.sql")
        );
    }
}

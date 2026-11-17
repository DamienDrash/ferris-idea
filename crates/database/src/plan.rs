//! Query plans (`SPEC-0020`).
//!
//! Each dialect reports its plan differently; all are read into one tree with
//! costs, estimates and, when measured, actual rows and times.

use serde::Serialize;
use serde_json::Value;

use crate::{
    DbError, Dialect, OnError, Outcome, Session, StatementClass, classify, split_statements,
};

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PlanNode {
    /// What the step does, e.g. `Seq Scan`, `Table scan`, `SEARCH`.
    pub operation: String,
    /// Table (with alias) or index the step works on.
    pub object: Option<String>,
    /// Conditions, keys and other details.
    pub details: Vec<String>,
    /// Estimated rows.
    pub estimated_rows: Option<f64>,
    /// Estimated total cost (database units).
    pub cost: Option<f64>,
    /// Measured rows per loop.
    pub actual_rows: Option<f64>,
    /// Measured time in milliseconds.
    pub actual_ms: Option<f64>,
    /// How often the step ran.
    pub loops: Option<f64>,
    /// Whether the step reads a whole table.
    pub full_scan: bool,
    /// Estimated start-up cost, where the database reports one (`SPEC-0020` v2 behaviour 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_cost: Option<f64>,
    /// Measured time until the first row, in milliseconds (`SPEC-0020` v2 behaviour 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_startup_ms: Option<f64>,
    /// Every field the database reports for the step, by its own name and in its order
    /// (`SPEC-0020` v2 behaviour 7).
    pub properties: Vec<Property>,
    /// The step's total cost as a part (0 to 1) of the whole plan's (`SPEC-0020` v2 behaviour 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_share: Option<f64>,
    /// The hints about this step (`SPEC-0020` v2 behaviour 9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// Steps feeding this one.
    pub children: Vec<PlanNode>,
}

/// One field of a plan step as the database reports it (`SPEC-0020` v2 behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Property {
    /// The database's name for it, e.g. `Plan Rows`.
    pub name: String,
    /// The value as text; a list joined by `, `.
    pub value: String,
}

impl Property {
    fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// A JSON value as the text of a property: strings as they are, a list joined by `, `.
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(value_text).collect::<Vec<_>>().join(", "),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// The scalar and list fields of a JSON object, in order, leaving out `skip` and nested objects.
fn json_properties(v: &Value, skip: &[&str]) -> Vec<Property> {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !skip.contains(&k.as_str()))
                .filter(|(_, x)| {
                    !x.is_object() && !x.as_array().is_some_and(|a| a.iter().any(Value::is_object))
                })
                .map(|(k, x)| Property::new(k.clone(), value_text(x)))
                .collect()
        })
        .unwrap_or_default()
}

/// A plan with hints.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    /// Top-level steps (SQLite can report several).
    pub nodes: Vec<PlanNode>,
    /// Whether the statement was run and measured.
    pub analyzed: bool,
    /// Whether the measured statement's changes were rolled back.
    pub rolled_back: bool,
    /// Parts that usually explain a slow statement.
    pub hints: Vec<String>,
    /// The plan as the database reported it.
    pub raw: String,
}

/// Why no plan was produced.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExplainError {
    /// Not exactly one statement.
    #[error("expected exactly one statement, got {0}")]
    NotOneStatement(usize),
    /// SQLite cannot measure plans.
    #[error(
        "SQLite reports no costs or measurements; only the plan (without analyze) is available"
    )]
    AnalyzeUnsupported,
    /// Oracle plans are read without measuring (`SPEC-0142`).
    #[error(
        "Oracle: measured plans need DBMS_XPLAN.DISPLAY_CURSOR and rights on V$ views; only the plan (without analyze) is available"
    )]
    AnalyzeUnsupportedOracle,
    /// The text starts with a word that begins no statement — a selected clause such as `FROM`
    /// (`SPEC-0110` v6 behaviour 15).
    #[error("`{0}` does not start a statement; select a whole statement or put the caret in one")]
    NotAStatement(String),
    /// Schema changes are never run for measuring.
    #[error("schema-changing statements are not run for measuring; explain them without analyze")]
    AnalyzeSchemaChange,
    /// The connector names no measuring plan statement (`SPEC-0300`).
    #[error("{0} has no measured plans; only the plan (without analyze) is available")]
    AnalyzeUnsupportedBy(String),
    /// The database refused.
    #[error("{0}")]
    Db(DbError),
    /// The plan could not be read.
    #[error("could not read the plan: {0}")]
    Parse(String),
}

/// Words that begin a clause but never a statement: text starting with one is part of a statement
/// (`SPEC-0110` v6 behaviour 15), so its refusal says that instead of guessing a statement kind.
const CLAUSE_WORDS: &[&str] = &[
    "from",
    "where",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "outer",
    "cross",
    "on",
    "group",
    "order",
    "having",
    "limit",
    "offset",
    "union",
    "intersect",
    "except",
    "and",
    "or",
    "not",
    "as",
    "by",
    "into",
];

/// The first word of `sql` in its original spelling when it is one of [`CLAUSE_WORDS`].
fn clause_start(sql: &str, dialect: Dialect) -> Option<String> {
    let first = crate::script::code_words(sql, dialect).into_iter().next()?;
    CLAUSE_WORDS.contains(&first.as_str()).then(|| {
        let at = sql.to_ascii_lowercase().find(&first).unwrap_or(0);
        sql[at..at + first.len()].to_owned()
    })
}

/// The statements around a measured statement (`SPEC-0020` v2 behaviour 4): every measured
/// statement runs in a transaction that is rolled back, also a read, which can call a function
/// that writes. `None` when nothing is measured.
pub fn wrapped(dialect: Dialect, analyze: bool) -> Option<(&'static str, &'static str)> {
    if !analyze {
        return None;
    }
    Some(match dialect {
        d if d.mysql_like() => ("START TRANSACTION", "ROLLBACK"),
        Dialect::SqlServer => ("BEGIN TRANSACTION", "ROLLBACK"),
        _ => ("BEGIN", "ROLLBACK"),
    })
}

/// The hint for a measured change on MySQL or MariaDB: tables without transactions keep it
/// (`SPEC-0020` v2 behaviour 4).
pub const NON_TRANSACTIONAL_HINT: &str = "MySQL and MariaDB roll back only tables with transactions (InnoDB); a MyISAM or MEMORY table keeps what the statement changed";

/// Explains one statement (`SPEC-0020`).
pub async fn explain(session: &Session, sql: &str, analyze: bool) -> Result<Plan, ExplainError> {
    let dialect = session.dialect();
    let mut plan = explain_steps(session, sql, analyze).await?;
    fill_steps(&mut plan.nodes);
    if analyze && dialect.mysql_like() {
        let statements = split_statements(sql, dialect);
        if statements
            .first()
            .is_some_and(|s| classify(&s.text, dialect) == StatementClass::ModifyData)
        {
            plan.hints.push(NON_TRANSACTIONAL_HINT.into());
        }
    }
    Ok(plan)
}

async fn explain_steps(session: &Session, sql: &str, analyze: bool) -> Result<Plan, ExplainError> {
    let dialect = session.dialect();
    let statements = split_statements(sql, dialect);
    if statements.len() != 1 {
        return Err(ExplainError::NotOneStatement(statements.len()));
    }
    let statement = statements[0].text.trim_end_matches(';').trim();
    if let Some(word) = clause_start(statement, dialect) {
        return Err(ExplainError::NotAStatement(word));
    }
    let class = classify(statement, dialect);
    if analyze {
        if dialect == Dialect::Sqlite {
            return Err(ExplainError::AnalyzeUnsupported);
        }
        if dialect == Dialect::Oracle {
            return Err(ExplainError::AnalyzeUnsupportedOracle);
        }
        if class == StatementClass::ModifySchema {
            return Err(ExplainError::AnalyzeSchemaChange);
        }
    }
    let rollback = analyze;
    // A connector's own plan statement (`SPEC-0300` behaviour 9).
    if let Some(system) = session.system()
        && system.sql.explain.is_some()
    {
        let template = if analyze {
            system
                .sql
                .explain_analyze
                .clone()
                .ok_or_else(|| ExplainError::AnalyzeUnsupportedBy(system.name.clone()))?
        } else {
            system.sql.explain.clone().unwrap_or_default()
        };
        if system.sql.plan_format == crate::connector::PlanFormat::Text {
            let text = template.replace("{statement}", statement);
            let around = wrapped(dialect, rollback);
            if let Some((begin, _)) = around {
                rows(session, begin).await?;
            }
            let result = rows(session, &text).await;
            if let Some((_, end)) = around {
                let _ = rows(session, end).await;
            }
            let nodes = text_plan(&result?);
            let raw = nodes
                .iter()
                .map(|n| n.operation.clone())
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(Plan {
                hints: Vec::new(),
                nodes,
                analyzed: analyze,
                rolled_back: rollback,
                raw,
            });
        }
    }
    if let crate::Driver::SqlServer(server) = &session.driver {
        return explain_sql_server(server, statement, analyze, rollback).await;
    }
    if dialect == Dialect::Oracle {
        return explain_oracle(session, statement).await;
    }
    let text = match (dialect, analyze) {
        (Dialect::PostgreSql, false) => format!("EXPLAIN (FORMAT JSON) {statement}"),
        (Dialect::PostgreSql, true) => format!("EXPLAIN (FORMAT JSON, ANALYZE) {statement}"),
        (Dialect::MySql, false) => format!("EXPLAIN FORMAT=TREE {statement}"),
        (Dialect::MySql, true) => format!("EXPLAIN ANALYZE {statement}"),
        (Dialect::MariaDb, false) => format!("EXPLAIN FORMAT=JSON {statement}"),
        (Dialect::MariaDb, true) => format!("ANALYZE FORMAT=JSON {statement}"),
        (Dialect::Sqlite, _) => format!("EXPLAIN QUERY PLAN {statement}"),
        (Dialect::DuckDb, false) => format!("EXPLAIN {statement}"),
        (Dialect::DuckDb, true) => format!("EXPLAIN ANALYZE {statement}"),
        (Dialect::SqlServer, _) => unreachable!("SQL Server plans are read above"),
        (Dialect::Oracle, _) => unreachable!("Oracle plans are read above"),
    };
    let around = wrapped(dialect, rollback);
    if let Some((begin, _)) = around {
        rows(session, begin).await?;
    }
    let result = rows(session, &text).await;
    if let Some((_, end)) = around {
        // Runs even when EXPLAIN failed, so nothing stays changed.
        let _ = rows(session, end).await;
    }
    let rows = result?;
    let (nodes, raw) = match dialect {
        Dialect::PostgreSql => {
            let raw = cell(&rows, 0, 0);
            (parse_postgres(&raw)?, raw)
        }
        Dialect::MySql => {
            let raw = cell(&rows, 0, 0);
            (parse_mysql(&raw), raw)
        }
        Dialect::MariaDb => {
            let raw = cell(&rows, 0, 0);
            (parse_mariadb(&raw)?, raw)
        }
        Dialect::SqlServer => unreachable!("SQL Server plans are read above"),
        Dialect::Oracle => unreachable!("Oracle plans are read above"),
        Dialect::DuckDb => {
            // The plan is the last cell, drawn as boxes over several lines.
            let raw = rows
                .iter()
                .filter_map(|r| r.last().cloned().flatten())
                .collect::<Vec<_>>()
                .join("\n");
            let lines: Vec<Vec<Option<String>>> =
                raw.lines().map(|l| vec![Some(l.to_owned())]).collect();
            (text_plan(&lines), raw)
        }
        Dialect::Sqlite => {
            let raw = rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|v| v.clone().unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .collect::<Vec<_>>()
                .join("\n");
            (parse_sqlite(&rows), raw)
        }
    };
    Ok(Plan {
        hints: hints(&nodes),
        nodes,
        analyzed: analyze,
        rolled_back: rollback,
        raw,
    })
}

type Rows = Vec<Vec<Option<String>>>;

/// A text plan: one step per result row, its cells joined by two spaces (`SPEC-0300`
/// `plan_format = "text"`).
pub fn text_plan(rows: &Rows) -> Vec<PlanNode> {
    rows.iter()
        .map(|r| PlanNode {
            operation: r
                .iter()
                .map(|v| v.clone().unwrap_or_default())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join("  "),
            ..PlanNode::default()
        })
        .filter(|n| !n.operation.trim().is_empty())
        .collect()
}

/// SQL Server reports plans as XML through session options: `SHOWPLAN_XML` compiles without
/// running, `STATISTICS XML` runs and adds a result set with the measured plan. Each option is a
/// batch of its own and is switched off again whatever happens.
/// The plan of `statement` through `EXPLAIN PLAN` and the user's `PLAN_TABLE` (`SPEC-0142`).
async fn explain_oracle(session: &Session, statement: &str) -> Result<Plan, ExplainError> {
    const ID: &str = "IDECRAFT";
    let crate::Driver::Oracle(oracle) = &session.driver else {
        unreachable!("Oracle plans need an Oracle session")
    };
    let rows = oracle
        .plan_rows(
            statement,
            ID,
            &format!(
                "SELECT id, parent_id, operation, options, object_name, cost, cardinality, \
                 access_predicates, filter_predicates FROM plan_table WHERE statement_id = '{ID}' \
                 ORDER BY id"
            ),
        )
        .await
        .map_err(ExplainError::Db)?;
    let raw = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| v.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let nodes = parse_oracle(&rows)?;
    Ok(Plan {
        hints: hints(&nodes),
        nodes,
        analyzed: false,
        rolled_back: false,
        raw,
    })
}

/// A tree from `PLAN_TABLE` rows: id, parent id, operation, options, object, cost, cardinality,
/// access and filter predicates.
pub fn parse_oracle(rows: &[Vec<Option<String>>]) -> Result<Vec<PlanNode>, ExplainError> {
    let cell = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten();
    let mut nodes: Vec<(u32, Option<u32>, PlanNode)> = Vec::new();
    for r in rows {
        let id: u32 = cell(r, 0)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| ExplainError::Parse("plan row without id".into()))?;
        let parent = cell(r, 1).and_then(|v| v.parse().ok());
        let operation = cell(r, 2).unwrap_or_default();
        let options = cell(r, 3).unwrap_or_default();
        let mut details = Vec::new();
        if let Some(a) = cell(r, 7) {
            details.push(format!("access: {a}"));
        }
        if let Some(f) = cell(r, 8) {
            details.push(format!("filter: {f}"));
        }
        nodes.push((
            id,
            parent,
            PlanNode {
                full_scan: operation == "TABLE ACCESS" && options == "FULL",
                operation: format!("{operation} {options}").trim().to_owned(),
                object: cell(r, 4),
                details,
                estimated_rows: cell(r, 6).and_then(|v| v.parse().ok()),
                cost: cell(r, 5).and_then(|v| v.parse().ok()),
                ..PlanNode::default()
            },
        ));
    }
    fn build(id: u32, all: &mut Vec<(u32, Option<u32>, PlanNode)>) -> Option<PlanNode> {
        let at = all.iter().position(|(i, _, _)| *i == id)?;
        let (_, _, mut node) = all.remove(at);
        let kids: Vec<u32> = all
            .iter()
            .filter(|(_, p, _)| *p == Some(id))
            .map(|(i, _, _)| *i)
            .collect();
        node.children = kids.into_iter().filter_map(|k| build(k, all)).collect();
        Some(node)
    }
    let roots: Vec<u32> = nodes
        .iter()
        .filter(|(_, p, _)| p.is_none())
        .map(|(i, _, _)| *i)
        .collect();
    let mut all = nodes;
    Ok(roots
        .into_iter()
        .filter_map(|r| build(r, &mut all))
        .collect())
}

async fn explain_sql_server(
    server: &crate::SqlServerSession,
    statement: &str,
    analyze: bool,
    rollback: bool,
) -> Result<Plan, ExplainError> {
    let option = if analyze {
        "STATISTICS XML"
    } else {
        "SHOWPLAN_XML"
    };
    let db = |e: DbError| ExplainError::Db(e);
    if rollback {
        server.result_sets("BEGIN TRANSACTION").await.map_err(db)?;
    }
    let result = match server.result_sets(&format!("SET {option} ON")).await {
        Ok(_) => {
            let sets = server.result_sets(statement).await;
            let _ = server.result_sets(&format!("SET {option} OFF")).await;
            sets
        }
        Err(e) => Err(e),
    };
    if rollback {
        // Runs even when the statement failed, so nothing stays changed.
        let _ = server.result_sets("ROLLBACK").await;
    }
    let sets = result.map_err(db)?;
    let raw = sets
        .iter()
        .rev()
        .find(|(columns, _)| columns.len() == 1 && columns[0].contains("Showplan"))
        .and_then(|(_, rows)| rows.first()?.first().cloned().flatten())
        .ok_or_else(|| ExplainError::Parse("no XML showplan in the result".into()))?;
    let nodes = parse_sql_server(&raw)?;
    Ok(Plan {
        hints: hints(&nodes),
        nodes,
        analyzed: analyze,
        rolled_back: rollback,
        raw,
    })
}

/// Reads MariaDB's `EXPLAIN FORMAT=JSON` / `ANALYZE FORMAT=JSON` output: every `table` becomes a
/// step, blocks such as `filesort` or `nested_loop` become steps above their tables.
pub fn parse_mariadb(json: &str) -> Result<Vec<PlanNode>, ExplainError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| ExplainError::Parse(e.to_string()))?;
    if value.get("query_block").is_none() {
        return Err(ExplainError::Parse("no `query_block` in the JSON".into()));
    }
    Ok(maria_walk(&value))
}

fn maria_walk(v: &Value) -> Vec<PlanNode> {
    let mut out = Vec::new();
    match v {
        Value::Array(items) => items.iter().for_each(|i| out.extend(maria_walk(i))),
        Value::Object(map) => {
            for (key, value) in map {
                match key.as_str() {
                    "table" => out.push(maria_table(value)),
                    "query_block"
                    | "filesort"
                    | "temporary_table"
                    | "nested_loop"
                    | "union_result"
                    | "materialized"
                    | "duplicates_removal"
                    | "read_sorted_file"
                    | "block-nl-join"
                    | "window_functions_computation" => {
                        let f = |k: &str| value.get(k).and_then(Value::as_f64);
                        let mut node = PlanNode {
                            properties: json_properties(value, &[]),
                            operation: maria_label(key),
                            cost: f("cost").or_else(|| f("query_cost")),
                            actual_ms: f("r_total_time_ms"),
                            loops: f("r_loops"),
                            children: maria_walk(value),
                            ..PlanNode::default()
                        };
                        if let Some(keys) = value.get("sort_key").and_then(Value::as_str) {
                            node.details.push(format!("Sort Key: {keys}"));
                        }
                        // A block around one step adds nothing to read.
                        if key == "query_block" && node.children.len() == 1 && node.cost.is_none() {
                            out.extend(node.children);
                        } else {
                            out.push(node);
                        }
                    }
                    _ if value.is_object() || value.is_array() => out.extend(maria_walk(value)),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

fn maria_label(key: &str) -> String {
    match key {
        "query_block" => "Query block",
        "filesort" => "Sort",
        "temporary_table" => "Temporary table",
        "nested_loop" => "Nested loop",
        "union_result" => "Union",
        "materialized" => "Materialize",
        "duplicates_removal" => "Remove duplicates",
        "read_sorted_file" => "Read sorted",
        "block-nl-join" => "Block nested loop",
        "window_functions_computation" => "Window functions",
        other => other,
    }
    .to_owned()
}

fn maria_table(v: &Value) -> PlanNode {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let access = s("access_type").unwrap_or_default();
    let operation = match access.as_str() {
        "ALL" => "Table scan",
        "index" => "Index scan",
        "range" | "index_merge" => "Index range scan",
        "ref" | "eq_ref" | "ref_or_null" | "const" | "system" | "fulltext" => "Index lookup",
        "" => "Table",
        other => other,
    }
    .to_owned();
    let object = s("table_name").map(|t| match s("key") {
        Some(key) => format!("{t} using {key}"),
        None => t,
    });
    let mut details = Vec::new();
    if !access.is_empty() {
        details.push(format!("Access: {access}"));
    }
    if let Some(r) = v.get("ref").and_then(Value::as_array) {
        let r: Vec<&str> = r.iter().filter_map(Value::as_str).collect();
        details.push(format!("Ref: {}", r.join(", ")));
    }
    if let Some(c) = s("attached_condition") {
        details.push(format!("Filter: {c}"));
    }
    let children = v
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(_, x)| x.is_object() || x.is_array())
                .flat_map(|(k, x)| {
                    if k == "ref" || k == "possible_keys" || k == "used_key_parts" {
                        Vec::new()
                    } else {
                        maria_walk(&serde_json::json!({ k: x }))
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    PlanNode {
        full_scan: access == "ALL" || access == "index",
        operation,
        object,
        details,
        estimated_rows: f("rows"),
        cost: f("cost"),
        actual_rows: f("r_rows"),
        actual_ms: f("r_table_time_ms").or_else(|| f("r_total_time_ms")),
        loops: f("r_loops"),
        properties: json_properties(v, &[]),
        children,
        ..PlanNode::default()
    }
}

/// Reads SQL Server's XML showplan (`SHOWPLAN_XML` or `STATISTICS XML`): every `RelOp` is a step.
pub fn parse_sql_server(xml: &str) -> Result<Vec<PlanNode>, ExplainError> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| ExplainError::Parse(e.to_string()))?;
    let plans: Vec<roxmltree::Node> = doc
        .descendants()
        .filter(|n| n.has_tag_name("QueryPlan"))
        .collect();
    if plans.is_empty() {
        return Err(ExplainError::Parse("no `QueryPlan` in the XML".into()));
    }
    Ok(plans
        .into_iter()
        .flat_map(|plan| own_relops(plan).into_iter().map(relop))
        .collect())
}

/// `RelOp` elements below `node` whose nearest `RelOp` ancestor is `node` itself (or none).
fn own_relops<'a, 'i>(node: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    node.descendants()
        .skip(1)
        .filter(|n| n.has_tag_name("RelOp"))
        .filter(|n| {
            n.ancestors()
                .skip(1)
                .find(|a| a.has_tag_name("RelOp") || *a == node)
                == Some(node)
        })
        .collect()
}

/// Elements named `tag` that belong to `relop` itself, not to a nested `RelOp`.
fn own<'a, 'i>(relop: roxmltree::Node<'a, 'i>, tag: &str) -> Vec<roxmltree::Node<'a, 'i>> {
    relop
        .descendants()
        .skip(1)
        .filter(|n| n.has_tag_name(tag))
        .filter(|n| n.ancestors().skip(1).find(|a| a.has_tag_name("RelOp")) == Some(relop))
        .collect()
}

fn relop(node: roxmltree::Node) -> PlanNode {
    let attr = |n: roxmltree::Node, k: &str| n.attribute(k).map(str::to_owned);
    let num = |n: roxmltree::Node, k: &str| n.attribute(k).and_then(|v| v.parse::<f64>().ok());
    let unbracket = |s: &str| s.trim_start_matches('[').trim_end_matches(']').to_owned();
    let operation = attr(node, "PhysicalOp").unwrap_or_default();
    let object = own(node, "Object").first().and_then(|o| {
        let table = unbracket(o.attribute("Table")?);
        let mut text = match o.attribute("Alias").map(unbracket) {
            Some(alias) if alias != table => format!("{table} {alias}"),
            _ => table,
        };
        if let Some(index) = o.attribute("Index") {
            text.push_str(&format!(" using {}", unbracket(index)));
        }
        Some(text)
    });
    let mut details = Vec::new();
    if let Some(logical) = attr(node, "LogicalOp").filter(|l| *l != operation) {
        details.push(logical);
    }
    for (tag, label) in [("SeekPredicates", "Seek"), ("Predicate", "Filter")] {
        if let Some(text) = own(node, tag).first().and_then(|p| {
            p.descendants()
                .find(|n| n.has_tag_name("ScalarOperator"))?
                .attribute("ScalarString")
        }) {
            details.push(format!("{label}: {text}"));
        }
    }
    let counters = own(node, "RunTimeCountersPerThread");
    let (rows, executions, elapsed) = counters.iter().fold((None, None, None), |acc, c| {
        let add = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
        (
            add(acc.0, num(*c, "ActualRows")),
            add(acc.1, num(*c, "ActualExecutions")),
            match (acc.2, num(*c, "ActualElapsedms")) {
                (Some(a), Some(b)) => Some(f64::max(a, b)),
                (a, b) => a.or(b),
            },
        )
    });
    PlanNode {
        full_scan: matches!(
            operation.as_str(),
            "Table Scan" | "Clustered Index Scan" | "Index Scan"
        ),
        operation,
        object,
        details,
        estimated_rows: num(node, "EstimateRows"),
        cost: num(node, "EstimatedTotalSubtreeCost"),
        actual_rows: rows.map(|r| r / executions.unwrap_or(1.0).max(1.0)),
        actual_ms: elapsed,
        loops: executions,
        properties: node
            .attributes()
            .map(|a| Property::new(a.name(), a.value()))
            .collect(),
        children: own_relops(node).into_iter().map(relop).collect(),
        ..PlanNode::default()
    }
}

async fn rows(session: &Session, sql: &str) -> Result<Rows, ExplainError> {
    let run = session.run_script(sql, OnError::Stop).await;
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { rows, .. }) => Ok(rows),
        Some(Outcome::UpdateCount(_)) => Ok(Vec::new()),
        Some(Outcome::Error(e)) => Err(ExplainError::Db(e)),
        None => Err(ExplainError::Parse("no result".into())),
    }
}

fn cell(rows: &Rows, row: usize, column: usize) -> String {
    rows.get(row)
        .and_then(|r| r.get(column))
        .cloned()
        .flatten()
        .unwrap_or_default()
}

/// Reads PostgreSQL's `EXPLAIN (FORMAT JSON)` output.
pub fn parse_postgres(json: &str) -> Result<Vec<PlanNode>, ExplainError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| ExplainError::Parse(e.to_string()))?;
    let plan = value
        .get(0)
        .and_then(|v| v.get("Plan"))
        .ok_or_else(|| ExplainError::Parse("no `Plan` in the JSON".into()))?;
    Ok(vec![pg_node(plan)])
}

fn pg_node(v: &Value) -> PlanNode {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let operation = s("Node Type").unwrap_or_default();
    let object = match (s("Relation Name"), s("Alias"), s("Index Name")) {
        (Some(rel), alias, index) => {
            let mut o = match alias {
                Some(a) if a != rel => format!("{rel} {a}"),
                _ => rel,
            };
            if let Some(index) = index {
                o.push_str(&format!(" using {index}"));
            }
            Some(o)
        }
        (None, _, Some(index)) => Some(index),
        (None, alias, None) => s("CTE Name").or(alias),
    };
    let mut details = Vec::new();
    for key in [
        "Join Type",
        "Hash Cond",
        "Merge Cond",
        "Index Cond",
        "Recheck Cond",
        "Join Filter",
        "Filter",
    ] {
        if let Some(value) = s(key) {
            details.push(format!("{key}: {value}"));
        }
    }
    for key in ["Sort Key", "Group Key"] {
        if let Some(keys) = v.get(key).and_then(Value::as_array) {
            let keys: Vec<&str> = keys.iter().filter_map(Value::as_str).collect();
            details.push(format!("{key}: {}", keys.join(", ")));
        }
    }
    PlanNode {
        // An index scan without a condition still reads every row, only in index order.
        full_scan: operation == "Seq Scan"
            || (matches!(operation.as_str(), "Index Scan" | "Index Only Scan")
                && s("Index Cond").is_none()),
        operation,
        object,
        details,
        estimated_rows: f("Plan Rows"),
        cost: f("Total Cost"),
        startup_cost: f("Startup Cost"),
        actual_rows: f("Actual Rows"),
        actual_ms: f("Actual Total Time"),
        actual_startup_ms: f("Actual Startup Time"),
        loops: f("Actual Loops"),
        properties: json_properties(v, &["Plans"]),
        cost_share: None,
        hints: Vec::new(),
        children: v
            .get("Plans")
            .and_then(Value::as_array)
            .map(|c| c.iter().map(pg_node).collect())
            .unwrap_or_default(),
    }
}

/// Reads MySQL's `EXPLAIN FORMAT=TREE` / `EXPLAIN ANALYZE` text.
pub fn parse_mysql(text: &str) -> Vec<PlanNode> {
    // (depth, node) in text order, folded into a tree afterwards.
    let mut flat: Vec<(usize, PlanNode)> = Vec::new();
    for line in text.lines() {
        let Some(arrow) = line.find("-> ") else {
            continue;
        };
        let depth = arrow / 4;
        let mut body = line[arrow + 3..].trim_end().to_owned();
        let mut node = PlanNode::default();
        let mut measured = Vec::new();
        let mut estimated = Vec::new();
        if let Some(start) = body.find(" (actual time=") {
            let part = body[start + 2..].trim_end_matches(')').to_owned();
            body.truncate(start);
            for item in part.trim_start_matches("actual ").split(' ') {
                match item.split_once('=') {
                    Some(("time", t)) => {
                        node.actual_startup_ms = t.split("..").next().and_then(|t| t.parse().ok());
                        node.actual_ms = t.split("..").last().and_then(|t| t.parse().ok());
                        measured.push(Property::new("actual time", t));
                    }
                    Some(("rows", r)) => {
                        node.actual_rows = r.parse().ok();
                        measured.push(Property::new("actual rows", r));
                    }
                    Some(("loops", l)) => {
                        node.loops = l.parse().ok();
                        measured.push(Property::new("loops", l));
                    }
                    _ => {}
                }
            }
        }
        let body = body.trim_end();
        let body = if let Some(start) = body.rfind("  (cost=").or_else(|| body.rfind("  (rows=")) {
            let inner = &body[start + 3..];
            let part = inner.strip_suffix(')').unwrap_or(inner).to_owned();
            for item in part.split(' ') {
                match item.split_once('=') {
                    Some(("cost", c)) => {
                        node.cost = c.parse().ok();
                        estimated.push(Property::new("cost", c));
                    }
                    Some(("rows", r)) => {
                        node.estimated_rows = r.parse().ok();
                        estimated.push(Property::new("rows", r));
                    }
                    _ => {}
                }
            }
            &body[..start]
        } else {
            body
        };
        let (head, detail) = match body.split_once(": ") {
            Some((h, d)) => (h, Some(d)),
            None => (body, None),
        };
        match head.split_once(" on ") {
            Some((op, object)) => {
                node.operation = op.to_owned();
                node.object = Some(object.to_owned());
            }
            None => node.operation = head.to_owned(),
        }
        node.details = detail.map(str::to_owned).into_iter().collect();
        node.full_scan = node.operation == "Table scan";
        node.properties
            .push(Property::new("operation", node.operation.clone()));
        if let Some(object) = &node.object {
            node.properties.push(Property::new("on", object.clone()));
        }
        if let Some(d) = detail {
            node.properties.push(Property::new("condition", d));
        }
        node.properties.extend(estimated);
        node.properties.extend(measured);
        flat.push((depth, node));
    }
    fold(flat)
}

/// Builds a tree from nodes with depths in pre-order.
fn fold(flat: Vec<(usize, PlanNode)>) -> Vec<PlanNode> {
    let mut stack: Vec<(usize, PlanNode)> = Vec::new();
    let mut roots = Vec::new();
    let close = |stack: &mut Vec<(usize, PlanNode)>, roots: &mut Vec<PlanNode>| {
        let (_, node) = stack.pop().expect("stack not empty");
        match stack.last_mut() {
            Some((_, parent)) => parent.children.push(node),
            None => roots.push(node),
        }
    };
    for (depth, node) in flat {
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            close(&mut stack, &mut roots);
        }
        stack.push((depth, node));
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    roots
}

/// Reads SQLite's `EXPLAIN QUERY PLAN` rows (id, parent, notused, detail).
pub fn parse_sqlite(rows: &[Vec<Option<String>>]) -> Vec<PlanNode> {
    let get = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten().unwrap_or_default();
    // Parents always come before their children; depth follows from the parent chain.
    let mut depth_of: Vec<(String, usize)> = Vec::new();
    let mut flat = Vec::new();
    for r in rows {
        let (id, parent, detail) = (get(r, 0), get(r, 1), get(r, 3));
        let depth = depth_of
            .iter()
            .find(|(i, _)| *i == parent)
            .map_or(0, |(_, d)| d + 1);
        depth_of.push((id, depth));
        let mut words = detail.splitn(3, ' ');
        let first = words.next().unwrap_or_default();
        let node = if matches!(first, "SCAN" | "SEARCH") {
            let object = words.next().map(str::to_owned);
            let rest = words.next().map(str::to_owned);
            PlanNode {
                full_scan: first == "SCAN" && rest.as_deref().is_none_or(|r| !r.contains("INDEX")),
                operation: first.to_owned(),
                object,
                details: rest.into_iter().collect(),
                ..PlanNode::default()
            }
        } else {
            PlanNode {
                operation: detail.clone(),
                ..PlanNode::default()
            }
        };
        flat.push((depth, node));
    }
    fold(flat)
}

/// Full scans of big tables and estimates that are off by 10× or more.
pub fn hints(nodes: &[PlanNode]) -> Vec<String> {
    fn walk(n: &PlanNode, out: &mut Vec<String>) {
        out.extend(step_hints(n));
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    for n in nodes {
        walk(n, &mut out);
    }
    out
}

/// The properties of a step the database reported nothing more for (SQLite, Oracle, text plans):
/// the values it has (`SPEC-0020` v2 behaviour 7).
fn known_properties(n: &PlanNode) -> Vec<Property> {
    let mut out = vec![Property::new("Operation", n.operation.clone())];
    if let Some(o) = &n.object {
        out.push(Property::new("Object", o.clone()));
    }
    for d in &n.details {
        match d.split_once(": ") {
            Some((k, v)) => out.push(Property::new(k, v)),
            None => out.push(Property::new("Detail", d.clone())),
        }
    }
    let number = |name: &str, v: Option<f64>, out: &mut Vec<Property>| {
        if let Some(v) = v {
            out.push(Property::new(name, v.to_string()));
        }
    };
    number("Estimated rows", n.estimated_rows, &mut out);
    number("Cost", n.cost, &mut out);
    number("Actual rows", n.actual_rows, &mut out);
    number("Actual time (ms)", n.actual_ms, &mut out);
    number("Loops", n.loops, &mut out);
    out
}

/// Gives every step its share of the plan's cost, its hints, and properties where the parser had
/// none (`SPEC-0020` v2 behaviours 7–9).
pub fn fill_steps(nodes: &mut [PlanNode]) {
    fn walk(n: &mut PlanNode, total: Option<f64>) {
        if n.properties.is_empty() {
            n.properties = known_properties(n);
        }
        n.cost_share = match (n.cost, total) {
            (Some(c), Some(t)) if t > 0.0 => Some((c / t).clamp(0.0, 1.0)),
            _ => None,
        };
        n.hints = step_hints(n);
        for c in &mut n.children {
            walk(c, total);
        }
    }
    let costs: Vec<f64> = nodes.iter().filter_map(|n| n.cost).collect();
    let total = (!costs.is_empty()).then(|| costs.iter().sum::<f64>());
    for n in nodes {
        walk(n, total);
    }
}

/// The hints about one step: a full scan of a big table, an estimate off by 10× or more.
pub fn step_hints(n: &PlanNode) -> Vec<String> {
    let mut out = Vec::new();
    let name = n.object.as_deref().unwrap_or(&n.operation);
    let rows = n.actual_rows.or(n.estimated_rows);
    if n.full_scan {
        match rows {
            Some(r) if r >= 1000.0 => out.push(format!(
                "full scan of `{name}` (about {r:.0} rows); an index on the filtered or joined columns may help"
            )),
            None => out.push(format!(
                "full scan of `{name}`; an index on the filtered or joined columns may help"
            )),
            _ => {}
        }
    }
    if let (Some(e), Some(a)) = (n.estimated_rows, n.actual_rows) {
        let (lo, hi) = if e < a { (e, a) } else { (a, e) };
        if hi >= 10.0 * lo.max(1.0) {
            // The step's name once: `Seq Scan item i`, or `Sort` for a step without an object
            // (SPEC-0110 v9 behaviour 2, OBS-0068 L11).
            let step = match &n.object {
                Some(object) => format!("{} {object}", n.operation),
                None => n.operation.clone(),
            };
            out.push(format!(
                "`{step}`: estimated {e:.0} rows, actual {a:.0}; table statistics may be stale (run ANALYZE)"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0020 v2 AK-7: every field of a step, start-up values, shares and hints at the step.
    #[test]
    fn steps_keep_every_field() {
        let json = r#"[{"Plan": {"Node Type": "Hash Join", "Parallel Aware": false, "Join Type": "Inner",
            "Startup Cost": 21.5, "Total Cost": 200.0, "Plan Rows": 2380, "Plan Width": 40,
            "Actual Startup Time": 0.5, "Actual Total Time": 3.25, "Actual Rows": 2380, "Actual Loops": 1,
            "Hash Cond": "(m.sku = i.sku)", "Output": ["m.sku", "i.name"], "Plans": [
            {"Node Type": "Seq Scan", "Relation Name": "movement", "Alias": "m", "Startup Cost": 0.0,
             "Total Cost": 150.0, "Plan Rows": 10000, "Actual Rows": 10000, "Actual Loops": 1,
             "Shared Hit Blocks": 64},
            {"Node Type": "Seq Scan", "Relation Name": "item", "Alias": "i", "Total Cost": 20.0,
             "Plan Rows": 5, "Actual Rows": 850, "Actual Loops": 1}]}}]"#;
        let mut nodes = parse_postgres(json).unwrap();
        fill_steps(&mut nodes);
        let top = &nodes[0];
        let names: Vec<&str> = top.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Node Type",
                "Parallel Aware",
                "Join Type",
                "Startup Cost",
                "Total Cost",
                "Plan Rows",
                "Plan Width",
                "Actual Startup Time",
                "Actual Total Time",
                "Actual Rows",
                "Actual Loops",
                "Hash Cond",
                "Output"
            ],
            "every key but Plans, in order"
        );
        assert_eq!(top.properties[1].value, "false");
        assert_eq!(top.properties[12].value, "m.sku, i.name");
        assert_eq!(top.startup_cost, Some(21.5));
        assert_eq!(top.actual_startup_ms, Some(0.5));
        assert_eq!(top.cost_share, Some(1.0));
        assert_eq!(top.children[0].cost_share, Some(0.75));
        assert_eq!(top.children[1].cost_share, Some(0.1));
        assert!(
            top.children[0]
                .properties
                .iter()
                .any(|p| p.name == "Shared Hit Blocks" && p.value == "64")
        );
        assert!(
            top.children[0].hints[0].starts_with("full scan of `movement m`"),
            "{:?}",
            top.children[0].hints
        );
        assert!(
            top.children[1]
                .hints
                .iter()
                .any(|h| h.contains("estimated 5 rows, actual 850"))
        );
        assert!(top.hints.is_empty());

        let mysql = parse_mysql(
            "-> Filter: (m.delta > 5)  (cost=1012.25 rows=3333) (actual time=0.051..4.5 rows=2380 loops=1)\n    -> Table scan on m  (cost=1012.25 rows=10000) (actual time=0.04..3.9 rows=10000 loops=1)",
        );
        let p: Vec<(&str, &str)> = mysql[0]
            .properties
            .iter()
            .map(|p| (p.name.as_str(), p.value.as_str()))
            .collect();
        assert_eq!(
            p,
            [
                ("operation", "Filter"),
                ("condition", "(m.delta > 5)"),
                ("cost", "1012.25"),
                ("rows", "3333"),
                ("actual time", "0.051..4.5"),
                ("actual rows", "2380"),
                ("loops", "1")
            ]
        );
        assert_eq!(mysql[0].actual_startup_ms, Some(0.051));
        assert_eq!(mysql[0].children[0].properties[1], Property::new("on", "m"));

        // SQLite: the values it has.
        let r = |v: &[&str]| v.iter().map(|c| Some((*c).to_owned())).collect::<Vec<_>>();
        let mut lite = parse_sqlite(&[r(&["2", "0", "0", "SCAN movement"])]);
        fill_steps(&mut lite);
        assert_eq!(lite[0].properties[0], Property::new("Operation", "SCAN"));
        assert_eq!(lite[0].properties[1], Property::new("Object", "movement"));
        assert_eq!(lite[0].cost_share, None);
    }

    /// SPEC-0020 v2 AK-8: every measured statement in a rolled-back transaction.
    #[test]
    fn every_measured_statement_is_rolled_back() {
        assert_eq!(wrapped(Dialect::PostgreSql, false), None);
        assert_eq!(
            wrapped(Dialect::PostgreSql, true),
            Some(("BEGIN", "ROLLBACK"))
        );
        assert_eq!(wrapped(Dialect::DuckDb, true), Some(("BEGIN", "ROLLBACK")));
        assert_eq!(
            wrapped(Dialect::MySql, true),
            Some(("START TRANSACTION", "ROLLBACK"))
        );
        assert_eq!(
            wrapped(Dialect::MariaDb, true),
            Some(("START TRANSACTION", "ROLLBACK"))
        );
        assert_eq!(
            wrapped(Dialect::SqlServer, true),
            Some(("BEGIN TRANSACTION", "ROLLBACK"))
        );
        assert!(NON_TRANSACTIONAL_HINT.contains("MyISAM"));
    }

    /// SPEC-0110 v6 AK-15: a selected clause is named as no statement, a real one is not.
    #[test]
    fn a_clause_is_no_statement() {
        let d = Dialect::PostgreSql;
        assert_eq!(clause_start("FROM", d).as_deref(), Some("FROM"));
        assert_eq!(
            clause_start("/* x */ where a = 1", d).as_deref(),
            Some("where")
        );
        assert_eq!(clause_start("select 1 from t", d), None);
        assert_eq!(clause_start("create table fromage (a int)", d), None);
        assert_eq!(
            ExplainError::NotAStatement("FROM".into()).to_string(),
            "`FROM` does not start a statement; select a whole statement or put the caret in one"
        );
    }

    const PG: &str = r#"[{"Plan": {"Node Type": "Hash Join", "Join Type": "Inner", "Total Cost": 224.41,
        "Plan Rows": 2380, "Hash Cond": "(m.sku = i.sku)", "Plans": [
        {"Node Type": "Seq Scan", "Relation Name": "movement", "Alias": "m", "Total Cost": 189.0,
         "Plan Rows": 2380, "Filter": "(delta > 5)", "Actual Rows": 2380.0, "Actual Total Time": 1.078,
         "Actual Loops": 1},
        {"Node Type": "Hash", "Total Cost": 18.5, "Plan Rows": 850, "Plans": [
          {"Node Type": "Seq Scan", "Relation Name": "item", "Alias": "i", "Total Cost": 18.5,
           "Plan Rows": 850, "Actual Rows": 2.0, "Actual Loops": 1}]},
        {"Node Type": "Index Scan", "Relation Name": "warehouse", "Index Name": "warehouse_pkey",
         "Plan Rows": 1, "Index Cond": "(id = 1)"}]}}]"#;

    #[test]
    fn oracle_plan_rows() {
        let r = |v: &[Option<&str>]| v.iter().map(|c| c.map(str::to_owned)).collect::<Vec<_>>();
        let rows = vec![
            r(&[
                Some("0"),
                None,
                Some("SELECT STATEMENT"),
                None,
                None,
                Some("3"),
                Some("2"),
                None,
                None,
            ]),
            r(&[
                Some("1"),
                Some("0"),
                Some("TABLE ACCESS"),
                Some("FULL"),
                Some("IDE_LINE"),
                Some("3"),
                Some("2"),
                None,
                Some("\"QTY\">3"),
            ]),
        ];
        let nodes = parse_oracle(&rows).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].operation, "SELECT STATEMENT");
        let scan = &nodes[0].children[0];
        assert_eq!(scan.operation, "TABLE ACCESS FULL");
        assert!(scan.full_scan);
        assert_eq!(scan.object.as_deref(), Some("IDE_LINE"));
        assert_eq!(scan.details, ["filter: \"QTY\">3"]);
        assert!(parse_oracle(&[r(&[None])]).is_err());
    }

    #[test]
    fn postgres_json() {
        let nodes = parse_postgres(PG).unwrap();
        let join = &nodes[0];
        assert_eq!(join.operation, "Hash Join");
        assert_eq!(
            join.details,
            ["Join Type: Inner", "Hash Cond: (m.sku = i.sku)"]
        );
        assert_eq!(join.cost, Some(224.41));
        let scan = &join.children[0];
        assert_eq!(scan.object.as_deref(), Some("movement m"));
        assert!(scan.full_scan);
        assert_eq!(scan.actual_ms, Some(1.078));
        assert_eq!(
            join.children[1].children[0].object.as_deref(),
            Some("item i")
        );
        let lookup = &join.children[2];
        assert_eq!(
            lookup.object.as_deref(),
            Some("warehouse using warehouse_pkey")
        );
        assert!(!lookup.full_scan, "index scan with a condition");
        assert_eq!(
            hints(&nodes),
            [
                "full scan of `movement m` (about 2380 rows); an index on the filtered or joined columns may help",
                "`Seq Scan item i`: estimated 850 rows, actual 2; table statistics may be stale (run ANALYZE)",
            ]
        );
        assert!(parse_postgres("[]").is_err());
    }

    /// SPEC-0110 v9 AK-19 (OBS-0068 L11): a step without an object is named once, not `Sort Sort`.
    #[test]
    fn a_hint_names_a_step_once() {
        let sort = PlanNode {
            operation: "Sort".into(),
            estimated_rows: Some(10.0),
            actual_rows: Some(5000.0),
            ..PlanNode::default()
        };
        assert_eq!(
            hints(&[sort]),
            ["`Sort`: estimated 10 rows, actual 5000; table statistics may be stale (run ANALYZE)"]
        );
    }

    /// M8.2 fuzz finding: a line ending in a multibyte character after `  (cost=` cut it apart.
    #[test]
    fn mysql_tree_without_closing_parenthesis() {
        let nodes = parse_mysql("-> Table scan on t  (cost=1 rows=2é\n");
        assert_eq!(nodes[0].cost, Some(1.0));
        assert_eq!(nodes[0].estimated_rows, None);
    }

    #[test]
    fn mysql_tree() {
        let text = "-> Inner hash join (m.sku = i.sku)  (cost=754 rows=22.5)\n    -> Filter: (m.delta > 5)  (cost=360 rows=337)\n        -> Table scan on m  (cost=360 rows=10107)\n    -> Hash\n        -> Table scan on i  (cost=0.45 rows=2)\n";
        let nodes = parse_mysql(text);
        assert_eq!(nodes.len(), 1);
        let join = &nodes[0];
        assert_eq!(join.operation, "Inner hash join (m.sku = i.sku)");
        assert_eq!((join.cost, join.estimated_rows), (Some(754.0), Some(22.5)));
        assert_eq!(join.children.len(), 2);
        let filter = &join.children[0];
        assert_eq!(filter.operation, "Filter");
        assert_eq!(filter.details, ["(m.delta > 5)"]);
        let scan = &filter.children[0];
        assert_eq!(
            (scan.operation.as_str(), scan.object.as_deref()),
            ("Table scan", Some("m"))
        );
        assert!(scan.full_scan);
        assert_eq!(join.children[1].children[0].object.as_deref(), Some("i"));
        assert_eq!(hints(&nodes).len(), 1, "only m is big");

        let analyzed = parse_mysql(
            "-> Aggregate: count(0)  (cost=1355 rows=1) (actual time=3.84..3.84 rows=1 loops=1)\n    -> Table scan on movement  (cost=1018 rows=10107) (actual time=0.164..3 rows=10000 loops=1)\n",
        );
        let scan = &analyzed[0].children[0];
        assert_eq!(analyzed[0].details, ["count(0)"]);
        assert_eq!(
            (scan.actual_rows, scan.actual_ms, scan.loops),
            (Some(10000.0), Some(3.0), Some(1.0))
        );
        assert_eq!(scan.estimated_rows, Some(10107.0));
    }

    #[test]
    fn sqlite_rows() {
        let row = |id: &str, parent: &str, detail: &str| {
            vec![
                Some(id.into()),
                Some(parent.into()),
                Some("0".into()),
                Some(detail.into()),
            ]
        };
        let nodes = parse_sqlite(&[
            row("1", "0", "COMPOUND QUERY"),
            row("2", "1", "LEFT-MOST SUBQUERY"),
            row("4", "2", "SEARCH t USING INDEX tb (b=?)"),
            row("7", "1", "UNION USING TEMP B-TREE"),
            row("9", "7", "SCAN t"),
            row("12", "0", "SCAN u USING COVERING INDEX ux"),
        ]);
        assert_eq!(nodes.len(), 2);
        let search = &nodes[0].children[0].children[0];
        assert_eq!(search.operation, "SEARCH");
        assert_eq!(search.object.as_deref(), Some("t"));
        assert_eq!(search.details, ["USING INDEX tb (b=?)"]);
        assert!(!search.full_scan);
        assert!(nodes[0].children[1].children[0].full_scan);
        assert!(!nodes[1].full_scan);
        assert_eq!(
            hints(&nodes),
            ["full scan of `t`; an index on the filtered or joined columns may help"]
        );
    }

    /// Shaped after the public showplan schema: a hash join over a scan and a seek, measured.
    const SHOWPLAN: &str = r#"<?xml version="1.0" encoding="utf-16"?>
<ShowPlanXML xmlns="http://schemas.microsoft.com/sqlserver/2004/07/showplan" Version="1.564">
 <BatchSequence><Batch><Statements><StmtSimple StatementText="select ...">
  <QueryPlan>
   <RelOp NodeId="0" PhysicalOp="Hash Match" LogicalOp="Inner Join" EstimateRows="2380" EstimatedTotalSubtreeCost="0.2441">
    <RunTimeInformation>
     <RunTimeCountersPerThread Thread="0" ActualRows="2380" ActualExecutions="1" ActualElapsedms="4"/>
    </RunTimeInformation>
    <Hash>
     <RelOp NodeId="1" PhysicalOp="Clustered Index Scan" LogicalOp="Clustered Index Scan" EstimateRows="850" EstimatedTotalSubtreeCost="0.0185">
      <RunTimeInformation>
       <RunTimeCountersPerThread Thread="0" ActualRows="2" ActualExecutions="1" ActualElapsedms="0"/>
      </RunTimeInformation>
      <IndexScan Ordered="false">
       <Object Database="[shop]" Schema="[dbo]" Table="[item]" Index="[PK_item]" Alias="[i]"/>
       <Predicate><ScalarOperator ScalarString="[shop].[dbo].[item].[qty] as [i].[qty]&gt;(5)"/></Predicate>
      </IndexScan>
     </RelOp>
     <RelOp NodeId="2" PhysicalOp="Index Seek" LogicalOp="Index Seek" EstimateRows="1" EstimatedTotalSubtreeCost="0.0033">
      <RunTimeInformation>
       <RunTimeCountersPerThread Thread="0" ActualRows="3" ActualExecutions="2" ActualElapsedms="1"/>
       <RunTimeCountersPerThread Thread="1" ActualRows="1" ActualExecutions="2" ActualElapsedms="2"/>
      </RunTimeInformation>
      <IndexScan Ordered="true">
       <Object Database="[shop]" Schema="[dbo]" Table="[movement]" Index="[ix_sku]"/>
       <SeekPredicates><SeekPredicateNew><SeekKeys><Prefix ScanType="EQ"><RangeColumns/>
        <RangeExpressions><ScalarOperator ScalarString="[shop].[dbo].[item].[sku]"/></RangeExpressions>
       </Prefix></SeekKeys></SeekPredicateNew></SeekPredicates>
      </IndexScan>
     </RelOp>
    </Hash>
   </RelOp>
  </QueryPlan>
 </StmtSimple></Statements></Batch></BatchSequence>
</ShowPlanXML>"#;

    #[test]
    fn reads_sql_server_showplan() {
        let nodes = parse_sql_server(SHOWPLAN).unwrap();
        assert_eq!(nodes.len(), 1);
        let join = &nodes[0];
        assert_eq!(join.operation, "Hash Match");
        assert_eq!(join.details, ["Inner Join"]);
        assert_eq!(join.estimated_rows, Some(2380.0));
        assert_eq!(join.cost, Some(0.2441));
        assert_eq!(
            (join.actual_rows, join.actual_ms),
            (Some(2380.0), Some(4.0))
        );
        assert_eq!(join.children.len(), 2);
        let scan = &join.children[0];
        assert_eq!(scan.object.as_deref(), Some("item i using PK_item"));
        assert!(scan.full_scan);
        assert_eq!(
            scan.details,
            ["Filter: [shop].[dbo].[item].[qty] as [i].[qty]>(5)"]
        );
        let seek = &join.children[1];
        assert!(!seek.full_scan);
        assert_eq!(seek.object.as_deref(), Some("movement using ix_sku"));
        assert_eq!(seek.details, ["Seek: [shop].[dbo].[item].[sku]"]);
        // Two threads: 4 rows over 4 executions, the slower thread's time.
        assert_eq!(
            (seek.actual_rows, seek.loops, seek.actual_ms),
            (Some(1.0), Some(4.0), Some(2.0))
        );
        assert!(parse_sql_server("<a/>").is_err());
        assert!(parse_sql_server("not xml").is_err());
    }

    /// `ANALYZE FORMAT=JSON` of MariaDB 12.3 on the reference data, shortened.
    const MARIADB: &str = r#"{"query_optimization": {"r_total_time_ms": 0.07},
      "query_block": {"select_id": 1, "cost": 0.0185, "r_loops": 1, "r_total_time_ms": 4.36,
        "filesort": {"sort_key": "m.moved_at", "r_loops": 1, "r_total_time_ms": 0.09,
          "temporary_table": {"nested_loop": [
            {"table": {"table_name": "i", "access_type": "ALL", "possible_keys": ["PRIMARY"],
              "r_loops": 1, "rows": 2, "r_rows": 2, "cost": 0.0111, "r_table_time_ms": 0.0087,
              "r_engine_stats": {"pages_accessed": 1}}},
            {"table": {"table_name": "m", "access_type": "ref", "possible_keys": ["sku"],
              "key": "sku", "used_key_parts": ["sku"], "ref": ["inventory.i.sku"], "r_loops": 2,
              "rows": 1, "r_rows": 5000, "cost": 0.0061, "r_table_time_ms": 3.66,
              "attached_condition": "m.delta > 5"}}]}}}}"#;

    #[test]
    fn reads_mariadb_json() {
        let nodes = parse_mariadb(MARIADB).unwrap();
        assert_eq!(nodes.len(), 1);
        let block = &nodes[0];
        assert_eq!(
            (block.operation.as_str(), block.cost),
            ("Query block", Some(0.0185))
        );
        let sort = &block.children[0];
        assert_eq!(sort.operation, "Sort");
        assert_eq!(sort.details, ["Sort Key: m.moved_at"]);
        let loop_ = &sort.children[0].children[0];
        assert_eq!(sort.children[0].operation, "Temporary table");
        assert_eq!(loop_.operation, "Nested loop");
        let (i, m) = (&loop_.children[0], &loop_.children[1]);
        assert_eq!((i.operation.as_str(), i.full_scan), ("Table scan", true));
        assert_eq!(i.object.as_deref(), Some("i"));
        assert_eq!(m.operation, "Index lookup");
        assert_eq!(m.object.as_deref(), Some("m using sku"));
        assert_eq!(
            m.details,
            ["Access: ref", "Ref: inventory.i.sku", "Filter: m.delta > 5"]
        );
        assert_eq!(
            (m.estimated_rows, m.actual_rows, m.loops),
            (Some(1.0), Some(5000.0), Some(2.0))
        );
        assert!(
            hints(&nodes)
                .iter()
                .any(|h| h.contains("estimated 1 rows, actual 5000"))
        );
        assert!(parse_mariadb("{}").is_err());
    }
}

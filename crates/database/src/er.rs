//! Entity-relationship model (`SPEC-0022`).
//!
//! Builds entities and relationships from the schema, places them in layers
//! (referenced tables left of referencing ones) and renders Mermaid, PlantUML
//! or DOT. Views are entities too when their definitions are given (v2); what
//! they use comes from those definitions.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{Table, ViewDef};

/// A diagram of tables and their foreign keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErModel {
    /// Tables, sorted by layer, then row.
    pub entities: Vec<Entity>,
    /// Foreign keys between included tables.
    pub relationships: Vec<Relationship>,
    /// Foreign keys from included tables to tables outside the selection.
    pub outside: Vec<Relationship>,
    /// What the included views use (v2 behaviour 7), sorted.
    pub dependencies: Vec<Dependency>,
}

/// A view using a table or another view.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Dependency {
    /// The view.
    pub view: String,
    /// The table or view it uses.
    pub uses: String,
}

/// One table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entity {
    /// Table name.
    pub name: String,
    /// Columns in definition order.
    pub columns: Vec<ErColumn>,
    /// Layer (0 = references nothing in the diagram).
    pub layer: usize,
    /// Position within the layer.
    pub row: usize,
    /// A view, not a table.
    pub view: bool,
}

/// One column with key marks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErColumn {
    /// Column name.
    pub name: String,
    /// Type as the database spells it.
    pub data_type: String,
    /// Whether NULL is allowed.
    pub nullable: bool,
    /// `PK`, `FK`, `UK`.
    pub keys: Vec<String>,
}

/// One foreign key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Relationship {
    /// Referencing table.
    pub from: String,
    /// Referencing columns.
    pub from_columns: Vec<String>,
    /// Referenced table.
    pub to: String,
    /// Referenced columns.
    pub to_columns: Vec<String>,
    /// Many rows of `from` per row of `to` (otherwise at most one).
    pub many: bool,
    /// A `from` row may reference nothing (a key column is nullable).
    pub optional: bool,
}

/// Why no model was built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErError {
    /// A requested table is not in the schema.
    #[error("unknown table `{0}`")]
    UnknownTable(String),
}

/// Builds the model of the tables of `schema` (all when `tables` is `None`) plus neighbours up to
/// `depth` foreign keys away; views are left out.
pub fn er_model(
    schema: &[Table],
    tables: Option<&[String]>,
    depth: usize,
) -> Result<ErModel, ErError> {
    build(schema, None, tables, depth)
}

/// Like [`er_model`], with the views of `schema` whose definitions are in `views` as entities and
/// what they use as dependencies (v2 behaviours 1 and 7). With a table selection, a view is included
/// when it uses an included table.
pub fn er_model_with_views(
    schema: &[Table],
    views: &[ViewDef],
    tables: Option<&[String]>,
    depth: usize,
) -> Result<ErModel, ErError> {
    build(schema, Some(views), tables, depth)
}

fn build(
    schema: &[Table],
    views: Option<&[ViewDef]>,
    tables: Option<&[String]>,
    depth: usize,
) -> Result<ErModel, ErError> {
    let all: BTreeMap<&str, &Table> = schema
        .iter()
        .filter(|t| t.kind == "table")
        .map(|t| (t.name.as_str(), t))
        .collect();
    let mut selected: BTreeSet<&str> = match tables {
        None => all.keys().copied().collect(),
        Some(names) => {
            let mut set = BTreeSet::new();
            for n in names {
                let (key, _) = all
                    .get_key_value(n.as_str())
                    .ok_or_else(|| ErError::UnknownTable(n.clone()))?;
                set.insert(*key);
            }
            for _ in 0..depth {
                let mut next = set.clone();
                for t in all.values() {
                    for f in &t.foreign_keys {
                        let Some((to, _)) = all.get_key_value(f.referenced_table.as_str()) else {
                            continue;
                        };
                        if set.contains(t.name.as_str()) {
                            next.insert(*to);
                        }
                        if set.contains(to) {
                            next.insert(t.name.as_str());
                        }
                    }
                }
                set = next;
            }
            set
        }
    };
    let view_tables: BTreeMap<&str, &Table> = schema
        .iter()
        .filter(|t| t.kind == "view" && views.is_some_and(|v| v.iter().any(|d| d.name == t.name)))
        .map(|t| (t.name.as_str(), t))
        .collect();
    let mut dependencies = Vec::new();
    if let Some(views) = views {
        let names: Vec<&str> = all.keys().chain(view_tables.keys()).copied().collect();
        let deps = view_dependencies(views, &names);
        // A view joins when it uses something included, repeated for views on views.
        loop {
            let before = selected.len();
            for d in &deps {
                if selected.contains(d.uses.as_str())
                    && let Some((v, _)) = view_tables.get_key_value(d.view.as_str())
                {
                    selected.insert(*v);
                }
            }
            if tables.is_none() {
                selected.extend(view_tables.keys().copied());
            }
            if selected.len() == before {
                break;
            }
        }
        dependencies = deps
            .into_iter()
            .filter(|d| selected.contains(d.view.as_str()) && selected.contains(d.uses.as_str()))
            .collect();
    }
    let all: BTreeMap<&str, &Table> = all.into_iter().chain(view_tables).collect();

    let mut relationships = Vec::new();
    let mut outside = Vec::new();
    for name in &selected {
        let t = all[name];
        for f in &t.foreign_keys {
            let unique = f.columns == t.primary_key
                || t.indexes
                    .iter()
                    .any(|i| i.is_key() && i.columns == f.columns);
            let optional = f
                .columns
                .iter()
                .any(|c| t.columns.iter().any(|col| &col.name == c && col.nullable));
            let r = Relationship {
                from: t.name.clone(),
                from_columns: f.columns.clone(),
                to: f.referenced_table.clone(),
                to_columns: f.referenced_columns.clone(),
                many: !unique,
                optional,
            };
            if selected.contains(f.referenced_table.as_str()) {
                relationships.push(r);
            } else {
                outside.push(r);
            }
        }
    }

    // Layer = longest chain of foreign keys; edges back into the current path are ignored.
    fn layer_of<'a>(
        t: &'a str,
        edges: &BTreeMap<&'a str, Vec<&'a str>>,
        done: &mut BTreeMap<&'a str, usize>,
        path: &mut Vec<&'a str>,
    ) -> usize {
        if let Some(l) = done.get(t) {
            return *l;
        }
        path.push(t);
        let mut layer = 0;
        for &to in edges.get(t).into_iter().flatten() {
            if to != t && !path.contains(&to) {
                layer = layer.max(layer_of(to, edges, done, path) + 1);
            }
        }
        path.pop();
        done.insert(t, layer);
        layer
    }
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in &relationships {
        edges.entry(&r.from).or_default().push(&r.to);
    }
    for d in &dependencies {
        edges.entry(&d.view).or_default().push(&d.uses);
    }
    let mut layers = BTreeMap::new();
    for name in &selected {
        layer_of(name, &edges, &mut layers, &mut Vec::new());
    }

    let mut rows: BTreeMap<usize, usize> = BTreeMap::new();
    let mut entities: Vec<Entity> = selected
        .iter()
        .map(|name| {
            let t = all[name];
            let layer = layers[name];
            let row = rows.entry(layer).or_default();
            let entity = Entity {
                name: t.name.clone(),
                columns: t
                    .columns
                    .iter()
                    .map(|c| {
                        let mut keys = Vec::new();
                        if t.primary_key.contains(&c.name) {
                            keys.push("PK".to_owned());
                        }
                        if t.foreign_keys.iter().any(|f| f.columns.contains(&c.name)) {
                            keys.push("FK".to_owned());
                        }
                        if t.indexes.iter().any(|i| {
                            i.is_key() && i.columns != t.primary_key && i.columns.contains(&c.name)
                        }) {
                            keys.push("UK".to_owned());
                        }
                        ErColumn {
                            name: c.name.clone(),
                            data_type: c.data_type.clone(),
                            nullable: c.nullable,
                            keys,
                        }
                    })
                    .collect(),
                layer,
                row: *row,
                view: t.kind == "view",
            };
            *row += 1;
            entity
        })
        .collect();
    entities.sort_by_key(|e| (e.layer, e.row));
    Ok(ErModel {
        entities,
        relationships,
        outside,
        dependencies,
    })
}

/// What each view uses among `names` (v2 behaviour 7): the names that follow `FROM`, `JOIN` or a
/// comma of a `FROM` list in the view's definition, by their last part, outside literals and
/// comments, compared case-insensitively. Sorted, without duplicates or self-references.
pub fn view_dependencies(views: &[ViewDef], names: &[&str]) -> Vec<Dependency> {
    let mut out = BTreeSet::new();
    for v in views {
        for used in from_names(&v.definition) {
            if let Some(n) = names.iter().find(|n| n.eq_ignore_ascii_case(&used))
                && !n.eq_ignore_ascii_case(&v.name)
            {
                out.insert(Dependency {
                    view: v.name.clone(),
                    uses: (*n).to_owned(),
                });
            }
        }
    }
    out.into_iter().collect()
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Dot,
    Comma,
    Open,
    Close,
    Other,
}

fn tokens(sql: &str) -> Vec<Token> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c.is_whitespace() {
            i += 1;
        } else if c == '-' && next == Some('-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == '\'' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' && chars.get(i + 1) == Some(&'\'') {
                    i += 2;
                } else if chars[i] == '\'' {
                    break;
                } else {
                    i += 1;
                }
            }
            i += 1;
            out.push(Token::Other);
        } else if matches!(c, '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            let start = i + 1;
            i = start;
            while i < chars.len() && chars[i] != close {
                i += 1;
            }
            out.push(Token::Quoted(
                chars[start..i.min(chars.len())].iter().collect(),
            ));
            i += 1;
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            out.push(Token::Word(chars[start..i].iter().collect()));
        } else {
            out.push(match c {
                '.' => Token::Dot,
                ',' => Token::Comma,
                '(' => Token::Open,
                ')' => Token::Close,
                _ => Token::Other,
            });
            i += 1;
        }
    }
    out
}

fn from_names(sql: &str) -> Vec<String> {
    const END: [&str; 12] = [
        "where",
        "group",
        "order",
        "having",
        "limit",
        "union",
        "except",
        "intersect",
        "on",
        "using",
        "window",
        "select",
    ];
    let t = tokens(sql);
    let mut out = Vec::new();
    // Depths at which a FROM list is open.
    let mut from_at: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    let mut expect = false;
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Token::Open => {
                depth += 1;
                i += 1;
                continue;
            }
            Token::Close => {
                from_at.retain(|d| *d < depth);
                depth = depth.saturating_sub(1);
                expect = false;
            }
            Token::Comma => expect = from_at.last() == Some(&depth),
            Token::Word(w) => {
                let lower = w.to_ascii_lowercase();
                if lower == "from" || lower == "join" {
                    from_at.retain(|d| *d != depth);
                    from_at.push(depth);
                    expect = true;
                } else if expect && matches!(lower.as_str(), "only" | "lateral") {
                } else if END.contains(&lower.as_str()) {
                    from_at.retain(|d| *d != depth);
                    expect = false;
                } else if expect {
                    let (name, end) = chain(&t, i);
                    // A name followed by `(` is a function in the FROM list.
                    if !matches!(t.get(end), Some(Token::Open)) {
                        out.push(name);
                    }
                    expect = false;
                    i = end;
                    continue;
                }
            }
            Token::Quoted(_) if expect => {
                let (name, end) = chain(&t, i);
                out.push(name);
                expect = false;
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The last part of the dotted name starting at `i`, and the index after it.
fn chain(t: &[Token], mut i: usize) -> (String, usize) {
    let mut last = String::new();
    while let Some(Token::Word(w) | Token::Quoted(w)) = t.get(i) {
        last = w.clone();
        i += 1;
        if t.get(i) == Some(&Token::Dot) {
            i += 1;
        } else {
            break;
        }
    }
    (last, i)
}

fn safe(text: &str, extra: &[char]) -> String {
    let mut out: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || extra.contains(&c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Mermaid `erDiagram` text.
pub fn to_mermaid(model: &ErModel) -> String {
    let mut out = String::from("erDiagram\n");
    for e in &model.entities {
        out.push_str(&format!("    {} {{\n", safe(&e.name, &[])));
        if e.view {
            out.push_str("        %% view\n");
        }
        for c in &e.columns {
            let name = safe(&c.name, &[]);
            // A view column computed by an expression may have no type.
            let data_type = if c.data_type.is_empty() {
                "unknown".to_owned()
            } else {
                safe(&c.data_type, &['(', ')', '[', ']', '-'])
            };
            out.push_str(&format!("        {data_type} {name}"));
            if !c.keys.is_empty() {
                out.push_str(&format!(" {}", c.keys.join(", ")));
            }
            let mut notes = Vec::new();
            if name != c.name {
                notes.push(format!("name: {}", c.name));
            }
            if data_type != c.data_type && !c.data_type.is_empty() {
                notes.push(format!("type: {}", c.data_type));
            }
            if !notes.is_empty() {
                out.push_str(&format!(" \"{}\"", notes.join("; ").replace('"', "'")));
            }
            out.push('\n');
        }
        out.push_str("    }\n");
    }
    for r in &model.relationships {
        out.push_str(&format!(
            "    {} {}--{} {} : \"{}\"\n",
            safe(&r.to, &[]),
            if r.optional { "|o" } else { "||" },
            if r.many { "o{" } else { "o|" },
            safe(&r.from, &[]),
            r.from_columns.join(", ").replace('"', "'")
        ));
    }
    for d in &model.dependencies {
        out.push_str(&format!(
            "    {} ||..o{{ {} : \"used by view\"\n",
            safe(&d.uses, &[]),
            safe(&d.view, &[])
        ));
    }
    out
}

/// PlantUML entity diagram (v2 behaviour 10).
pub fn to_plantuml(model: &ErModel) -> String {
    let mut out = String::from("@startuml\nhide circle\nskinparam linetype ortho\n");
    let pk = |c: &ErColumn| c.keys.iter().any(|k| k == "PK");
    for e in &model.entities {
        out.push_str(&format!(
            "\nentity \"{}\" as {}{} {{\n",
            e.name.replace('"', "'"),
            safe(&e.name, &[]),
            if e.view { " <<view>>" } else { "" }
        ));
        let line = |c: &ErColumn| {
            let mut l = format!("    {}", c.name);
            if !c.data_type.is_empty() {
                l.push_str(&format!(" : {}", c.data_type));
            }
            for k in &c.keys {
                l.push_str(&format!(" <<{k}>>"));
            }
            if !c.nullable {
                l.push_str(" NOT NULL");
            }
            l.push('\n');
            l
        };
        let keys: Vec<&ErColumn> = e.columns.iter().filter(|c| pk(c)).collect();
        for c in &keys {
            out.push_str(&line(c));
        }
        if !keys.is_empty() {
            out.push_str("    --\n");
        }
        for c in e.columns.iter().filter(|c| !pk(c)) {
            out.push_str(&line(c));
        }
        out.push_str("}\n");
    }
    if !model.relationships.is_empty() || !model.dependencies.is_empty() {
        out.push('\n');
    }
    for r in &model.relationships {
        out.push_str(&format!(
            "{} {}--{} {} : {}\n",
            safe(&r.to, &[]),
            if r.optional { "|o" } else { "||" },
            if r.many { "o{" } else { "o|" },
            safe(&r.from, &[]),
            r.from_columns.join(", ")
        ));
    }
    for d in &model.dependencies {
        out.push_str(&format!(
            "{} ..> {} : uses\n",
            safe(&d.view, &[]),
            safe(&d.uses, &[])
        ));
    }
    out.push_str("@enduml\n");
    out
}

fn html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn dot_id(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Graphviz DOT text.
pub fn to_dot(model: &ErModel) -> String {
    let mut out = String::from("digraph er {\n    rankdir=RL;\n    node [shape=plaintext];\n");
    for e in &model.entities {
        let mut label = format!(
            "<table border=\"0\" cellborder=\"1\" cellspacing=\"0\"><tr><td bgcolor=\"lightgrey\"><b>{}</b>{}</td></tr>",
            html(&e.name),
            if e.view { " (view)" } else { "" }
        );
        for c in &e.columns {
            let keys = if c.keys.is_empty() {
                String::new()
            } else {
                format!(" ({})", c.keys.join(", "))
            };
            label.push_str(&format!(
                "<tr><td align=\"left\">{} : {}{}</td></tr>",
                html(&c.name),
                html(&c.data_type),
                keys
            ));
        }
        label.push_str("</table>");
        out.push_str(&format!("    {} [label=<{label}>];\n", dot_id(&e.name)));
    }
    for r in &model.relationships {
        out.push_str(&format!(
            "    {} -> {} [label={}, dir=both, arrowtail={}, arrowhead={}];\n",
            dot_id(&r.from),
            dot_id(&r.to),
            dot_id(&r.from_columns.join(", ")),
            if r.many { "crow" } else { "tee" },
            if r.optional { "teeodot" } else { "tee" }
        ));
    }
    for d in &model.dependencies {
        out.push_str(&format!(
            "    {} -> {} [label=\"uses\", style=dashed];\n",
            dot_id(&d.view),
            dot_id(&d.uses)
        ));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, ForeignKey, Index};

    fn table(name: &str, columns: &[(&str, bool)], pk: &[&str], fks: &[(&[&str], &str)]) -> Table {
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: name.into(),
            kind: "table".into(),
            columns: columns
                .iter()
                .map(|(c, nullable)| Column {
                    name: c.to_string(),
                    data_type: "integer".into(),
                    nullable: *nullable,
                    default: None,
                    generated: None,
                })
                .collect(),
            primary_key: pk.iter().map(|c| c.to_string()).collect(),
            foreign_keys: fks
                .iter()
                .map(|(cols, to)| ForeignKey {
                    name: None,
                    columns: cols.iter().map(|c| c.to_string()).collect(),
                    referenced_table: to.to_string(),
                    referenced_columns: vec!["id".into()],
                })
                .collect(),
            indexes: vec![],
            unreadable: None,
            constraints: Vec::new(),
        }
    }

    /// customer ← order (customer nullable) ← line; profile 1:1 customer; order → outside `region`.
    fn shop() -> Vec<Table> {
        let mut profile = table(
            "profile",
            &[("cid", false)],
            &["cid"],
            &[(&["cid"], "customer")],
        );
        profile.indexes.push(Index {
            name: "p".into(),
            unique: true,
            columns: vec!["cid".into()],
            condition: None,
            collations: Vec::new(),
        });
        let mut customer = table("customer", &[("id", false), ("email", false)], &["id"], &[]);
        customer.indexes.push(Index {
            name: "email_key".into(),
            unique: true,
            columns: vec!["email".into()],
            condition: None,
            collations: Vec::new(),
        });
        vec![
            customer,
            table(
                "order",
                &[("id", false), ("customer_id", true), ("region_id", false)],
                &["id"],
                &[(&["customer_id"], "customer"), (&["region_id"], "region")],
            ),
            table(
                "line",
                &[("order_id", false), ("n", false)],
                &["order_id", "n"],
                &[(&["order_id"], "order")],
            ),
            profile,
        ]
    }

    #[test]
    fn keys_cardinalities_and_outside_references() {
        let m = er_model(&shop(), None, 0).unwrap();
        let customer = m.entities.iter().find(|e| e.name == "customer").unwrap();
        assert_eq!(customer.columns[0].keys, ["PK"]);
        assert_eq!(customer.columns[1].keys, ["UK"]);
        let profile = m.entities.iter().find(|e| e.name == "profile").unwrap();
        assert_eq!(profile.columns[0].keys, ["PK", "FK"]);
        let rel = |from: &str| m.relationships.iter().find(|r| r.from == from).unwrap();
        assert!(rel("order").many && rel("order").optional);
        assert!(rel("line").many && !rel("line").optional);
        assert!(!rel("profile").many, "foreign key = primary key → one");
        assert_eq!(m.outside.len(), 1);
        assert_eq!(m.outside[0].to, "region");
    }

    #[test]
    fn selection_by_depth() {
        let names = |m: ErModel| {
            m.entities
                .into_iter()
                .map(|e| e.name)
                .collect::<BTreeSet<_>>()
        };
        let pick = |d| er_model(&shop(), Some(&["line".to_owned()]), d).unwrap();
        assert_eq!(names(pick(0)), BTreeSet::from(["line".to_owned()]));
        assert_eq!(names(pick(1)), ["line", "order"].map(String::from).into());
        assert_eq!(
            names(pick(2)),
            ["customer", "line", "order"].map(String::from).into()
        );
        assert_eq!(
            er_model(&shop(), Some(&["nope".to_owned()]), 0),
            Err(ErError::UnknownTable("nope".into()))
        );
    }

    #[test]
    fn layers_for_chain_diamond_cycle_and_self_reference() {
        let layers = |schema: &[Table]| {
            er_model(schema, None, 0)
                .unwrap()
                .entities
                .into_iter()
                .map(|e| (e.name, e.layer, e.row))
                .collect::<Vec<_>>()
        };
        let s = |n: &str, to: &[&str]| {
            let fks: Vec<(&[&str], &str)> = to.iter().map(|t| (&["x"][..], *t)).collect();
            table(n, &[("x", false)], &[], &fks)
        };
        assert_eq!(
            layers(&shop()),
            [
                ("customer".into(), 0, 0),
                ("order".into(), 1, 0),
                ("profile".into(), 1, 1),
                ("line".into(), 2, 0)
            ]
        );
        assert_eq!(
            layers(&[
                s("a", &["b", "c"]),
                s("b", &["d"]),
                s("c", &["d"]),
                s("d", &[])
            ]),
            [
                ("d".into(), 0, 0),
                ("b".into(), 1, 0),
                ("c".into(), 1, 1),
                ("a".into(), 2, 0)
            ]
        );
        let cycle = layers(&[s("a", &["b"]), s("b", &["a"]), s("me", &["me"])]);
        assert_eq!(cycle.len(), 3, "no endless loop");
        assert!(cycle.contains(&("me".into(), 0, 1)) || cycle.contains(&("me".into(), 0, 0)));
    }

    #[test]
    fn mermaid_and_dot_text() {
        let mut t = table(
            "my item",
            &[("id", false), ("1st name", true)],
            &["id"],
            &[],
        );
        t.columns[1].data_type = "character varying(20)".into();
        let child = table(
            "tag",
            &[("item_id", true)],
            &[],
            &[(&["item_id"], "my item")],
        );
        let m = er_model(&[t, child], None, 0).unwrap();
        assert_eq!(
            to_mermaid(&m),
            "erDiagram\n    my_item {\n        integer id PK\n        character_varying(20) _1st_name \"name: 1st name; type: character varying(20)\"\n    }\n    tag {\n        integer item_id FK\n    }\n    my_item |o--o{ tag : \"item_id\"\n"
        );
        let dot = to_dot(&m);
        assert!(dot.starts_with("digraph er {\n    rankdir=RL;\n"));
        assert!(dot.contains("\"my item\" [label=<<table"));
        assert!(dot.contains("<b>my item</b>"));
        assert!(dot.contains(
            "\"tag\" -> \"my item\" [label=\"item_id\", dir=both, arrowtail=crow, arrowhead=teeodot];"
        ));
        let mut odd = table("a<b>&\"c\"", &[("x", false)], &[], &[]);
        odd.columns[0].data_type = "t<1>".into();
        let dot = to_dot(&er_model(&[odd], None, 0).unwrap());
        assert!(dot.contains("\"a<b>&\\\"c\\\"\" [label=<"));
        assert!(dot.contains("<b>a&lt;b&gt;&amp;&quot;c&quot;</b>"));
        assert!(dot.contains("x : t&lt;1&gt;"));
    }

    fn view(name: &str, definition: &str) -> ViewDef {
        ViewDef {
            name: name.into(),
            definition: definition.into(),
        }
    }

    /// SPEC-0022/AK-7: dependencies from definitions.
    #[test]
    fn view_dependencies_from_definitions() {
        let names = ["item", "stock", "warehouse", "v1", "Order"];
        let deps = |def: &str| {
            view_dependencies(&[view("v2", def)], &names)
                .into_iter()
                .map(|d| d.uses)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            deps(
                " SELECT s.warehouse_id, sum(s.qty * i.unit_price_cents) AS value_cents\n   FROM stock s\n     JOIN item i ON i.sku = s.sku\n  GROUP BY s.warehouse_id;"
            ),
            ["item", "stock"]
        );
        assert_eq!(
            deps(
                "select `s`.`warehouse_id` AS `warehouse_id`,sum((`s`.`qty` * `i`.`x`)) AS `v` from (`inventory`.`stock` `s` join `inventory`.`item` `i` on((`i`.`sku` = `s`.`sku`))) group by `s`.`warehouse_id`"
            ),
            ["item", "stock"],
            "MySQL: quoted, qualified, parenthesised"
        );
        assert_eq!(
            deps(
                "select 'from warehouse' as t, item /* from stock */ from \"public\".\"Order\" o, v1 -- from item\n where x in (select 1 from generate_series(1, 2))"
            ),
            ["Order", "v1"],
            "literals, comments, comma lists, functions; columns named like tables are not uses"
        );
        assert_eq!(
            deps("select * from v2"),
            Vec::<String>::new(),
            "no self-reference"
        );
    }

    /// SPEC-0022/AK-7: views as entities, right of what they use, also on views; Mermaid, PlantUML, DOT.
    #[test]
    fn views_are_entities_with_dependencies() {
        let mut schema = shop();
        for n in ["big_orders", "biggest"] {
            let mut v = table(n, &[("id", true)], &[], &[]);
            v.kind = "view".into();
            schema.push(v);
        }
        let views = [
            view(
                "big_orders",
                "select o.id from \"order\" o join customer c on c.id = o.customer_id",
            ),
            view("biggest", "select id from big_orders"),
        ];
        let m = er_model_with_views(&schema, &views, None, 0).unwrap();
        let layer = |n: &str| m.entities.iter().find(|e| e.name == n).unwrap().layer;
        assert_eq!(layer("big_orders"), 2);
        assert_eq!(layer("biggest"), 3);
        assert!(
            m.entities
                .iter()
                .find(|e| e.name == "biggest")
                .unwrap()
                .view
        );
        assert_eq!(m.dependencies.len(), 3);
        let only_line =
            er_model_with_views(&schema, &views, Some(&["line".to_owned()]), 0).unwrap();
        assert!(only_line.dependencies.is_empty(), "no view uses line");
        let customer =
            er_model_with_views(&schema, &views, Some(&["customer".to_owned()]), 0).unwrap();
        let names: Vec<&str> = customer.entities.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["customer", "big_orders", "biggest"],
            "views using the selection join it"
        );
        assert!(
            er_model(&schema, None, 0)
                .unwrap()
                .entities
                .iter()
                .all(|e| !e.view)
        );
        let mermaid = to_mermaid(&m);
        assert!(mermaid.contains("    biggest {\n        %% view\n"));
        assert!(mermaid.contains("    big_orders ||..o{ biggest : \"used by view\"\n"));
        let puml = to_plantuml(&m);
        assert!(puml.starts_with("@startuml\nhide circle\nskinparam linetype ortho\n"));
        assert!(puml.contains("entity \"biggest\" as biggest <<view>> {\n"));
        assert!(puml.contains(
            "entity \"profile\" as profile {\n    cid : integer <<PK>> <<FK>> NOT NULL\n    --\n}\n"
        ));
        assert!(puml.contains("customer |o--o{ order : customer_id\n"));
        assert!(puml.contains("customer ||--o| profile : cid\n"));
        assert!(puml.contains("biggest ..> big_orders : uses\n"));
        assert!(puml.ends_with("@enduml\n"));
        assert!(
            to_dot(&m).contains("\"biggest\" -> \"big_orders\" [label=\"uses\", style=dashed];")
        );
    }
}

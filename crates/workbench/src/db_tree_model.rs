//! Rows of the Database tool window (`SPEC-0089`, v3: other schemas and the error node, IMP-0051,
//! IMP-0052), free of the UI toolkit.

use std::collections::{HashMap, HashSet};

use idecraft_database::{ConnectionProfile, Dialect, Index, Table};

/// What is known about a connection's schema.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    /// Not asked yet (the connection was never expanded).
    #[default]
    NotLoaded,
    /// Being read in the background.
    Loading,
    /// Tables and views of the current schema.
    Loaded(Vec<Table>),
    /// Why it could not be read.
    Failed(String),
}

/// One connection with what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conn {
    /// The profile from `.idecraft/connections.toml`.
    pub profile: ConnectionProfile,
    /// Its schema.
    pub load: Load,
    /// The other non-system schemas of its database, listed when the current one is read, each
    /// read on expand (v3 behaviour 8).
    pub schemas: Vec<(String, Load)>,
    /// The sequences of its current schema, read with it (v5 behaviour 12; every dialect with sequences, SPEC-0116 v3).
    pub sequences: Vec<String>,
    /// Whether the session that read the schema was encrypted (v12 behaviour 24); `None` before
    /// the read and when the server does not say.
    pub tls: Option<bool>,
    /// The routines, triggers and types of its current schema, read with it (`SPEC-0089` v13
    /// behaviour 18, `SPEC-0013` v6 behaviour 18).
    pub objects: Vec<idecraft_database::SchemaObject>,
}

/// What is known about one level of a store's tree (`SPEC-0310` behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StoreLoad {
    /// Not asked yet.
    #[default]
    NotLoaded,
    /// Being read in the background.
    Loading,
    /// The children.
    Loaded(Vec<idecraft_nosql::BrowseItem>),
    /// Why it could not be read.
    Failed(String),
}

/// A non-relational store (`.idecraft/stores.toml`) with the levels read so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConn {
    /// The store profile.
    pub profile: idecraft_nosql::StoreProfile,
    /// Children by tree path (`[]` is the store itself).
    pub levels: HashMap<Vec<String>, StoreLoad>,
}

/// Which section of a table a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// Columns.
    Columns,
    /// Primary and foreign keys.
    Keys,
    /// Indexes.
    Indexes,
}

/// A node that can be expanded.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    /// A connection (index in the list).
    Conn(usize),
    /// The tables (`false`) or views (`true`) of a connection.
    Group(usize, bool),
    /// The sequences of a connection (v5 behaviour 12).
    Sequences(usize),
    /// A table or view of a connection.
    Table(usize, String),
    /// A section of a table.
    Section(usize, String, Section),
    /// A column of a table: connection, table, column (`SPEC-0128`: renamed with ⇧F6).
    Column(usize, String, String),
    /// An index of a table: connection, table, index (`SPEC-0164` v3 behaviour 10).
    Index(usize, String, String),
    /// A named foreign key of a table: connection, table, constraint (`SPEC-0164` v3).
    Key(usize, String, String),
    /// A sequence of a connection (`SPEC-0164` v3 behaviour 10).
    Sequence(usize, String),
    /// The objects of one kind of a connection, e.g. its functions (`SPEC-0089` v13 behaviour 25).
    Objects(usize, String),
    /// A routine, trigger or type of a connection (`SPEC-0089` v13 behaviour 25).
    Object(usize, idecraft_database::SchemaObject),
    /// Read the connection's schema again (v3 behaviour 9).
    Retry(usize),
    /// Another schema of a connection (v3 behaviour 8).
    Schema(usize, String),
    /// A table of another schema: connection, schema, table.
    SchemaTable(usize, String, String),
    /// Read another schema again.
    SchemaRetry(usize, String),
    /// A level of a non-relational store: store index, tree path (`SPEC-0310`).
    Store(usize, Vec<String>),
}

impl Node {
    /// Index of the connection the node belongs to.
    pub fn conn(&self) -> usize {
        match self {
            Node::Conn(i)
            | Node::Group(i, _)
            | Node::Sequences(i)
            | Node::Table(i, _)
            | Node::Section(i, ..)
            | Node::Column(i, ..)
            | Node::Index(i, ..)
            | Node::Key(i, ..)
            | Node::Sequence(i, _)
            | Node::Objects(i, _)
            | Node::Object(i, _)
            | Node::Retry(i)
            | Node::Schema(i, _)
            | Node::SchemaTable(i, ..)
            | Node::SchemaRetry(i, _) => *i,
            // Not an SQL connection.
            Node::Store(..) => usize::MAX,
        }
    }
}

impl Node {
    /// The same node for the connection (or store) now at index `to`.
    pub fn moved_to(&self, to: usize) -> Node {
        match self.clone() {
            Node::Conn(_) => Node::Conn(to),
            Node::Group(_, v) => Node::Group(to, v),
            Node::Sequences(_) => Node::Sequences(to),
            Node::Table(_, t) => Node::Table(to, t),
            Node::Section(_, t, s) => Node::Section(to, t, s),
            Node::Column(_, t, c) => Node::Column(to, t, c),
            Node::Index(_, t, n) => Node::Index(to, t, n),
            Node::Key(_, t, n) => Node::Key(to, t, n),
            Node::Sequence(_, n) => Node::Sequence(to, n),
            Node::Objects(_, k) => Node::Objects(to, k),
            Node::Object(_, o) => Node::Object(to, o),
            Node::Retry(_) => Node::Retry(to),
            Node::Schema(_, n) => Node::Schema(to, n),
            Node::SchemaTable(_, n, t) => Node::SchemaTable(to, n, t),
            Node::SchemaRetry(_, n) => Node::SchemaRetry(to, n),
            Node::Store(_, p) => Node::Store(to, p),
        }
    }
}

/// `node` after the connections were read again (`SPEC-0089` v10 behaviour 22): connections and
/// stores are found by their id, so a node of a connection that moved in the list moves with
/// it; `None` for one whose connection or store is gone.
pub fn carried_over(
    node: &Node,
    old_conns: &[String],
    new_conns: &[String],
    old_stores: &[String],
    new_stores: &[String],
) -> Option<Node> {
    let (index, old, new) = match node {
        Node::Store(i, _) => (*i, old_stores, new_stores),
        n => (n.conn(), old_conns, new_conns),
    };
    let id = old.get(index)?;
    let to = new.iter().position(|n| n == id)?;
    Some(node.moved_to(to))
}

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The node, if the row can be expanded or acted on.
    pub node: Option<Node>,
    /// Indentation level.
    pub depth: usize,
    /// Name.
    pub label: String,
    /// Grey text after the name (type, target, columns).
    pub detail: String,
    /// Icon key.
    pub icon: &'static str,
    /// Whether it has children.
    pub has_children: bool,
    /// Whether its children are shown.
    pub expanded: bool,
    /// Whether the label is a message (loading, error) rather than an object.
    pub message: bool,
}

/// Grey text of a connection: dialect and where it points.
pub fn connection_detail(p: &ConnectionProfile) -> String {
    let dialect = p.dialect.label();
    let place = match p.dialect {
        Dialect::Sqlite => p
            .path
            .as_ref()
            .and_then(|f| f.file_name())
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => {
            let host = p.host.clone().unwrap_or_else(|| "localhost".into());
            match (&p.database, p.effective_port()) {
                (Some(db), Some(port)) => format!("{host}:{port}/{db}"),
                (Some(db), None) => format!("{host}/{db}"),
                (None, Some(port)) => format!("{host}:{port}"),
                (None, None) => host,
            }
        }
    };
    if place.is_empty() {
        dialect.into()
    } else {
        format!("{dialect} · {place}")
    }
}

/// The end of a read connection's detail: whether its session is encrypted (v12 behaviour 24,
/// SPEC-0016 v3). Nothing before the read, after a failed one or when the server does not say.
fn tls_note(conn: &Conn) -> &'static str {
    match (&conn.load, conn.tls) {
        (Load::Loaded(_), Some(true)) => " · TLS",
        (Load::Loaded(_), Some(false)) => " · TLS: no",
        _ => "",
    }
}

fn row(node: Option<Node>, depth: usize, label: String, detail: String, icon: &'static str) -> Row {
    Row {
        node,
        depth,
        label,
        detail,
        icon,
        has_children: false,
        expanded: false,
        message: false,
    }
}

fn message(depth: usize, text: String) -> Row {
    Row {
        message: true,
        ..row(None, depth, text, String::new(), "")
    }
}

/// A load error without the leading "connection `name`: " (the row sits under that connection),
/// starting with a capital letter (SPEC-0089 v2).
pub fn short_error(error: &str) -> String {
    let rest = error
        .strip_prefix("connection `")
        .and_then(|r| r.split_once('`'))
        .map_or(error, |(_, r)| {
            let r = r.trim_start();
            let r = if r.starts_with('(') {
                r.split_once("):").map_or(r, |(_, t)| t)
            } else {
                r
            };
            r.strip_prefix(':').unwrap_or(r).trim_start()
        });
    let mut chars = rest.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The visible rows for `conns` with `expanded` nodes open (behaviours 2–4).
pub fn rows(conns: &[Conn], expanded: &HashSet<Node>) -> Vec<Row> {
    let mut out = Vec::new();
    for (ci, conn) in conns.iter().enumerate() {
        let node = Node::Conn(ci);
        let open = expanded.contains(&node);
        out.push(Row {
            has_children: true,
            expanded: open,
            ..row(
                Some(node),
                0,
                conn.profile.name.clone(),
                connection_detail(&conn.profile) + tls_note(conn),
                "nodes/dataSchema",
            )
        });
        if !open {
            continue;
        }
        let tables = match &conn.load {
            Load::NotLoaded | Load::Loading => {
                // D21: a password in the keychain can make the system ask first; say where to look
                // instead of a bare "Loading…" that seems to hang.
                let text = if conn.profile.secret.is_some() {
                    "Loading… (if the system asks for keychain access, allow it)"
                } else {
                    "Loading…"
                };
                out.push(message(1, text.into()));
                continue;
            }
            Load::Failed(e) => {
                error_rows(1, e, Node::Retry(ci), &mut out);
                continue;
            }
            Load::Loaded(tables) => tables,
        };
        if tables.is_empty() {
            out.push(message(1, "No tables".into()));
        }
        for views in [false, true] {
            let group: Vec<&Table> = tables
                .iter()
                .filter(|t| (t.kind == "view") == views)
                .collect();
            if group.is_empty() {
                continue;
            }
            let node = Node::Group(ci, views);
            let open = expanded.contains(&node);
            out.push(Row {
                has_children: true,
                expanded: open,
                ..row(
                    Some(node),
                    1,
                    if views { "views" } else { "tables" }.into(),
                    group.len().to_string(),
                    "nodes/folder",
                )
            });
            if open {
                for t in group {
                    table_rows(ci, t, expanded, &mut out);
                }
            }
        }
        if !conn.sequences.is_empty() {
            let node = Node::Sequences(ci);
            let open = expanded.contains(&node);
            out.push(Row {
                has_children: true,
                expanded: open,
                ..row(
                    Some(node),
                    1,
                    "sequences".into(),
                    conn.sequences.len().to_string(),
                    "nodes/folder",
                )
            });
            if open {
                for name in &conn.sequences {
                    out.push(row(
                        Some(Node::Sequence(ci, name.clone())),
                        2,
                        name.clone(),
                        String::new(),
                        "nodes/constant",
                    ));
                }
            }
        }
        object_rows(ci, conn, expanded, &mut out);
        for (name, load) in &conn.schemas {
            schema_rows(ci, name, load, expanded, &mut out);
        }
    }
    out
}

/// The folder of a kind of schema object (`SPEC-0089` v13 behaviour 25).
pub fn kind_folder(kind: &str) -> String {
    match kind {
        "function" => "functions".into(),
        "procedure" => "procedures".into(),
        "aggregate" => "aggregates".into(),
        "trigger" => "triggers".into(),
        "type" => "types".into(),
        other => format!("{other}s"),
    }
}

/// How a routine, trigger or type is shown: label, grey text and icon. A PostgreSQL routine
/// carries its arguments in the label, so overloads differ; a trigger shows its table and when it
/// fires, a type what it is.
pub fn object_row_text(o: &idecraft_database::SchemaObject) -> (String, String, &'static str) {
    match o.kind.as_str() {
        "trigger" => {
            let mut detail = o.table.clone().unwrap_or_default();
            if let Some(when) = &o.detail {
                if !detail.is_empty() {
                    detail.push_str(" · ");
                }
                detail.push_str(when);
            }
            (o.name.clone(), detail, "nodes/method")
        }
        "type" => (
            o.name.clone(),
            o.detail.clone().unwrap_or_default(),
            if o.detail.as_deref() == Some("enum") {
                "nodes/enum"
            } else {
                "nodes/class"
            },
        ),
        _ => (
            format!("{}{}", o.name, o.detail.as_deref().unwrap_or_default()),
            String::new(),
            "nodes/function",
        ),
    }
}

/// The folders of routines, triggers and types of a connection, in the order of
/// `idecraft_database::object_kinds`, each with its count.
fn object_rows(ci: usize, conn: &Conn, expanded: &HashSet<Node>, out: &mut Vec<Row>) {
    for kind in idecraft_database::object_kinds(conn.profile.dialect) {
        let of_kind: Vec<&idecraft_database::SchemaObject> =
            conn.objects.iter().filter(|o| o.kind == *kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        let node = Node::Objects(ci, (*kind).to_owned());
        let open = expanded.contains(&node);
        out.push(Row {
            has_children: true,
            expanded: open,
            ..row(
                Some(node),
                1,
                kind_folder(kind),
                of_kind.len().to_string(),
                "nodes/folder",
            )
        });
        if open {
            for o in of_kind {
                let (label, detail, icon) = object_row_text(o);
                out.push(row(
                    Some(Node::Object(ci, o.clone())),
                    2,
                    label,
                    detail,
                    icon,
                ));
            }
        }
    }
}

/// Every row of the Database tool window: SQL connections, then the stores.
pub fn all_rows(conns: &[Conn], stores: &[StoreConn], expanded: &HashSet<Node>) -> Vec<Row> {
    let mut out = rows(conns, expanded);
    out.extend(store_rows(stores, expanded));
    out
}

/// The names speed search looks at (v5 behaviour 10): every visible row that is an object or a
/// folder; loading and error messages are skipped (`None`).
pub fn search_labels(rows: &[Row]) -> Vec<Option<&str>> {
    rows.iter()
        .map(|r| (!r.message).then_some(r.label.as_str()))
        .collect()
}

/// Depth, name and grey detail of `rows` (what tests and the self-test read).
pub fn visible(rows: &[Row]) -> Vec<(usize, String, String)> {
    rows.iter()
        .map(|r| (r.depth, r.label.clone(), r.detail.clone()))
        .collect()
}

/// Rows of the non-relational stores, after the SQL connections (`SPEC-0310` behaviour 5).
pub fn store_rows(stores: &[StoreConn], expanded: &HashSet<Node>) -> Vec<Row> {
    let mut out = Vec::new();
    for (si, store) in stores.iter().enumerate() {
        let node = Node::Store(si, Vec::new());
        let open = expanded.contains(&node);
        out.push(Row {
            has_children: true,
            expanded: open,
            ..row(
                Some(node),
                0,
                store.profile.name.clone(),
                store.profile.summary(),
                "nodes/dataSchema",
            )
        });
        if open {
            level_rows(si, store, &[], 1, expanded, &mut out);
        }
    }
    out
}

fn level_rows(
    si: usize,
    store: &StoreConn,
    path: &[String],
    depth: usize,
    expanded: &HashSet<Node>,
    out: &mut Vec<Row>,
) {
    match store.levels.get(path).cloned().unwrap_or_default() {
        StoreLoad::NotLoaded | StoreLoad::Loading => {
            let text = if store.profile.secret.is_some() {
                "Loading… (if the system asks for keychain access, allow it)"
            } else {
                "Loading…"
            };
            out.push(message(depth, text.into()));
        }
        StoreLoad::Failed(e) => out.push(Row {
            icon: "status/warning",
            node: Some(Node::Store(si, path.to_vec())),
            ..message(depth, format!("Could not read it: {}", short_error(&e)))
        }),
        StoreLoad::Loaded(items) if items.is_empty() => out.push(message(depth, "Empty".into())),
        StoreLoad::Loaded(items) => {
            for item in items {
                let icon = match item.kind {
                    "database" => "nodes/dataSchema",
                    "collection" | "view" => "nodes/dataTables",
                    "group" => "nodes/folder",
                    "more" => "",
                    _ => "nodes/dataColumn",
                };
                if item.kind == "more" {
                    out.push(message(depth, format!("{} ({})", item.label, item.detail)));
                    continue;
                }
                let node = Node::Store(si, item.path.clone());
                let open = item.expandable && expanded.contains(&node);
                out.push(Row {
                    has_children: item.expandable,
                    expanded: open,
                    ..row(
                        Some(node),
                        depth,
                        item.label.clone(),
                        item.detail.clone(),
                        icon,
                    )
                });
                if open {
                    level_rows(si, store, &item.path, depth + 1, expanded, out);
                }
            }
        }
    }
}

/// The error row of a schema that could not be read, and a row that reads it again (v3 behaviour
/// 9, IMP-0051).
fn error_rows(depth: usize, error: &str, retry: Node, out: &mut Vec<Row>) {
    out.push(Row {
        icon: "status/warning",
        node: Some(retry.clone()),
        ..message(
            depth,
            format!("Could not read the schema: {}", short_error(error)),
        )
    });
    out.push(row(
        Some(retry),
        depth,
        "Retry".into(),
        "Enter".into(),
        "general/refresh",
    ));
}

/// Another schema with, when open, its tables and their columns (v3 behaviour 8, IMP-0052).
fn schema_rows(ci: usize, name: &str, load: &Load, expanded: &HashSet<Node>, out: &mut Vec<Row>) {
    let node = Node::Schema(ci, name.to_owned());
    let open = expanded.contains(&node);
    out.push(Row {
        has_children: true,
        expanded: open,
        ..row(
            Some(node),
            1,
            name.to_owned(),
            "schema".into(),
            "nodes/dataSchema",
        )
    });
    if !open {
        return;
    }
    let tables = match load {
        Load::NotLoaded | Load::Loading => {
            out.push(message(2, "Loading…".into()));
            return;
        }
        Load::Failed(e) => {
            error_rows(2, e, Node::SchemaRetry(ci, name.to_owned()), out);
            return;
        }
        Load::Loaded(tables) => tables,
    };
    if tables.is_empty() {
        out.push(message(2, "No tables".into()));
    }
    for t in tables {
        let node = Node::SchemaTable(ci, name.to_owned(), t.name.clone());
        let open = expanded.contains(&node);
        let kind = if t.kind == "view" { "view" } else { "" };
        let detail = match unreadable_detail(t) {
            Some(reason) if kind.is_empty() => reason,
            Some(reason) => format!("{kind} · {reason}"),
            None => kind.to_owned(),
        };
        let has_children = !t.columns.is_empty();
        let open = open && has_children;
        out.push(Row {
            has_children,
            expanded: open,
            ..row(Some(node), 2, t.name.clone(), detail, "nodes/dataTables")
        });
        if open {
            for c in &t.columns {
                out.push(row(
                    None,
                    3,
                    c.name.clone(),
                    c.data_type.to_lowercase(),
                    "nodes/dataColumn",
                ));
            }
        }
    }
}

/// The grey detail of an object the database refused to describe (v9 behaviour 21), on one line.
fn unreadable_detail(t: &Table) -> Option<String> {
    t.unreadable.as_ref().map(|reason| {
        let reason = reason.split_whitespace().collect::<Vec<_>>().join(" ");
        format!("cannot be read: {reason}")
    })
}

/// The detail of an index row (v9 behaviour 20): its terms, each with its collation, then
/// "unique" and the condition of a partial index.
fn index_detail(ix: &Index) -> String {
    let mut detail = format!("({})", ix.terms().join(", "));
    if ix.unique {
        detail.push_str(" · unique");
    }
    if let Some(condition) = ix.condition_line() {
        detail.push_str(&format!(" · where {condition}"));
    }
    // SPEC-0089 v15 behaviour 27: an access method other than the default one.
    if let Some(method) = &ix.method {
        detail.push_str(&format!(" · {method}"));
    }
    detail
}

fn table_rows(ci: usize, t: &Table, expanded: &HashSet<Node>, out: &mut Vec<Row>) {
    let node = Node::Table(ci, t.name.clone());
    let keys = usize::from(!t.primary_key.is_empty()) + t.foreign_keys.len();
    // Without columns, keys and indexes there is nothing to open (v9 behaviour 21): no arrow,
    // also when the row was expanded before the schema was read again.
    let has_children = !t.columns.is_empty() || keys > 0 || !t.indexes.is_empty();
    let open = has_children && expanded.contains(&node);
    out.push(Row {
        has_children,
        expanded: open,
        ..row(
            Some(node),
            2,
            t.name.clone(),
            unreadable_detail(t).unwrap_or_default(),
            "nodes/dataTables",
        )
    });
    if !open {
        return;
    }
    for (section, label, count) in [
        (Section::Columns, "columns", t.columns.len()),
        (Section::Keys, "keys", keys),
        (Section::Indexes, "indexes", t.indexes.len()),
    ] {
        if count == 0 {
            continue;
        }
        let node = Node::Section(ci, t.name.clone(), section);
        let open = expanded.contains(&node);
        out.push(Row {
            has_children: true,
            expanded: open,
            ..row(
                Some(node),
                3,
                label.into(),
                count.to_string(),
                "nodes/folder",
            )
        });
        if !open {
            continue;
        }
        match section {
            Section::Columns => {
                for c in &t.columns {
                    let mut detail = c.data_type.to_lowercase();
                    if t.primary_key.contains(&c.name) {
                        detail.push_str(" · primary key");
                    } else if !c.nullable {
                        detail.push_str(" · not null");
                    }
                    if let Some(d) = &c.default {
                        detail.push_str(&format!(" = {d}"));
                    }
                    if let Some(kind) = &c.generated {
                        detail.push_str(&format!(" · generated ({kind})"));
                    }
                    out.push(row(
                        Some(Node::Column(ci, t.name.clone(), c.name.clone())),
                        4,
                        c.name.clone(),
                        detail,
                        "nodes/dataColumn",
                    ));
                }
            }
            Section::Keys => {
                if !t.primary_key.is_empty() {
                    out.push(row(
                        None,
                        4,
                        "primary key".into(),
                        format!("({})", t.primary_key.join(", ")),
                        "idecraft/dbKeyPrimary",
                    ));
                }
                for fk in &t.foreign_keys {
                    out.push(row(
                        fk.name.clone().map(|n| Node::Key(ci, t.name.clone(), n)),
                        4,
                        fk.name.clone().unwrap_or_else(|| "foreign key".into()),
                        // SPEC-0089 v15 behaviour 27: with its actions.
                        format!(
                            "({}) → {}({}){}",
                            fk.columns.join(", "),
                            fk.referenced_table,
                            fk.referenced_columns.join(", "),
                            fk.actions_sql().to_lowercase()
                        ),
                        "idecraft/dbKeyForeign",
                    ));
                }
            }
            Section::Indexes => {
                for ix in &t.indexes {
                    out.push(row(
                        Some(Node::Index(ci, t.name.clone(), ix.name.clone())),
                        4,
                        ix.name.clone(),
                        index_detail(ix),
                        "idecraft/dbIndex",
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;

    /// SPEC-0089 v10 AK-24: after reading the connections again an open node keeps to its
    /// connection by id, also when the list order changed; a removed connection's nodes go.
    #[test]
    fn open_nodes_keep_to_their_connection() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (old, new) = (ids(&["a", "b", "c"]), ids(&["c", "a"]));
        let none: Vec<String> = Vec::new();
        let table = Node::Table(0, "orders".into());
        assert_eq!(
            carried_over(&table, &old, &new, &none, &none),
            Some(Node::Table(1, "orders".into()))
        );
        assert_eq!(
            carried_over(&Node::Conn(2), &old, &new, &none, &none),
            Some(Node::Conn(0))
        );
        assert_eq!(carried_over(&Node::Conn(1), &old, &new, &none, &none), None);
        let stores = ids(&["cache"]);
        assert_eq!(
            carried_over(
                &Node::Store(0, vec!["db0".into()]),
                &old,
                &new,
                &stores,
                &ids(&["other", "cache"])
            ),
            Some(Node::Store(1, vec!["db0".into()]))
        );
    }

    #[test]
    fn a_saved_redis_store_is_a_visible_row_with_system_and_address() {
        // SPEC-0310 AK-9: what the self-test reads after Save, without a window.
        let redis = idecraft_database::catalogue().get("redis").unwrap().clone();
        let mut form = idecraft_database::ConnectionForm::for_connector(&redis);
        form.name = "cache".into();
        form.host = "127.0.0.1".into();
        form.port = "1".into();
        form.database = "2".into();
        let profile = idecraft_nosql::form_profile(&form, "new").unwrap().unwrap();
        let dir = std::env::temp_dir().join(format!("idecraft-store-rows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        idecraft_nosql::save_store(
            &dir,
            profile,
            None,
            None,
            &idecraft_database::MemoryStore::default(),
        )
        .unwrap();
        let mut stores: Vec<StoreConn> = idecraft_nosql::load_stores(&dir)
            .unwrap()
            .into_iter()
            .map(|profile| StoreConn {
                profile,
                levels: HashMap::new(),
            })
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        let mut open = HashSet::new();
        let shown = visible(&all_rows(&[], &stores, &open));
        // New connections use TLS by default (SPEC-0016 v2, IMP-0370).
        assert_eq!(
            shown,
            [(
                0,
                "cache".to_owned(),
                "Redis · 127.0.0.1:1/2 · TLS".to_owned()
            )]
        );
        // A refused connection: the reason is shown; the address stays on the store row.
        open.insert(Node::Store(0, Vec::new()));
        stores[0].levels.insert(
            Vec::new(),
            StoreLoad::Failed(
                "connection `cache` (127.0.0.1:1): Connection refused (os error 61)".into(),
            ),
        );
        let shown = visible(&all_rows(&[], &stores, &open));
        assert_eq!(shown[0].2, "Redis · 127.0.0.1:1/2 · TLS");
        assert_eq!(
            shown[1].1,
            "Could not read it: Connection refused (os error 61)"
        );
    }

    #[test]
    fn store_rows_show_system_address_and_levels() {
        // SPEC-0310 AK-9 (model).
        let profile =
            idecraft_nosql::StoreProfile::from_url("Cache", "redis://localhost:6380/2").unwrap();
        let mut store = StoreConn {
            profile,
            levels: HashMap::new(),
        };
        let mut open = HashSet::new();
        let r = store_rows(std::slice::from_ref(&store), &open);
        assert_eq!(
            (r[0].label.as_str(), r[0].detail.as_str()),
            ("Cache", "Redis · localhost:6380/2")
        );
        open.insert(Node::Store(0, Vec::new()));
        assert_eq!(
            store_rows(std::slice::from_ref(&store), &open)[1].label,
            "Loading…"
        );
        let item = |label: &str, kind: &'static str, path: &[&str], expandable: bool| {
            idecraft_nosql::BrowseItem {
                label: label.into(),
                detail: String::new(),
                path: path.iter().map(|s| (*s).to_owned()).collect(),
                expandable,
                kind,
            }
        };
        store.levels.insert(
            Vec::new(),
            StoreLoad::Loaded(vec![item("db2", "database", &["2"], true)]),
        );
        store.levels.insert(
            vec!["2".into()],
            StoreLoad::Loaded(vec![
                item("user:", "group", &["2", "user:"], true),
                item("more…", "more", &["2"], false),
            ]),
        );
        open.insert(Node::Store(0, vec!["2".into()]));
        let r = store_rows(std::slice::from_ref(&store), &open);
        let shown: Vec<(usize, &str)> = r.iter().map(|r| (r.depth, r.label.as_str())).collect();
        assert_eq!(
            shown,
            [(0, "Cache"), (1, "db2"), (2, "user:"), (2, "more… ()")]
        );
        store.levels.insert(
            vec!["2".into()],
            StoreLoad::Failed("connection refused".into()),
        );
        let r = store_rows(std::slice::from_ref(&store), &open);
        assert!(r[2].label.starts_with("Could not read it"));
        assert_eq!(r[2].node, Some(Node::Store(0, vec!["2".into()])));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_database::{Column, ForeignKey};

    fn profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::from_url(name, "sqlite:///tmp/shop.db").unwrap()
    }

    fn tables() -> Vec<Table> {
        let col = |n: &str, t: &str, nullable: bool| Column {
            name: n.into(),
            data_type: t.into(),
            nullable,
            default: None,
            generated: None,
            expression: None,
            identity: None,
            comment: None,
        };
        vec![
            Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name: "orders".into(),
                kind: "table".into(),
                columns: vec![
                    col("id", "INTEGER", false),
                    col("customer_id", "INTEGER", false),
                    col("note", "TEXT", true),
                    Column {
                        generated: Some("virtual".into()),
                        ..col("note_length", "INTEGER", true)
                    },
                ],
                primary_key: vec!["id".into()],
                foreign_keys: vec![ForeignKey {
                    name: None,
                    columns: vec!["customer_id".into()],
                    referenced_table: "customers".into(),
                    referenced_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
                indexes: vec![Index {
                    name: "orders_by_customer".into(),
                    unique: false,
                    columns: vec!["customer_id".into()],
                    condition: None,
                    collations: Vec::new(),
                    method: None,
                    orders: Vec::new(),
                    prefixes: Vec::new(),
                }],
                unreadable: None,
                constraints: Vec::new(),
                comment: None,
            },
            Table {
                storage: Vec::new(),
                hidden_columns: Vec::new(),
                name: "big_orders".into(),
                kind: "view".into(),
                columns: vec![col("id", "INTEGER", true)],
                primary_key: vec![],
                foreign_keys: vec![],
                indexes: vec![],
                unreadable: None,
                constraints: Vec::new(),
                comment: None,
            },
        ]
    }

    fn labels(rows: &[Row]) -> Vec<(usize, String, String)> {
        rows.iter()
            .map(|r| (r.depth, r.label.clone(), r.detail.clone()))
            .collect()
    }

    /// SPEC-0089 v12 AK-26: "· TLS" or "· TLS: no" once read, nothing before or when unknown.
    #[test]
    fn a_read_connection_says_whether_it_is_encrypted() {
        let pg =
            ConnectionProfile::from_url("orders", "postgresql://u@localhost:5432/shop").unwrap();
        let mut conns = vec![Conn {
            profile: pg,
            load: Load::NotLoaded,
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: Some(false),
            objects: Vec::new(),
        }];
        let open = HashSet::new();
        let detail = |conns: &[Conn]| rows(conns, &open)[0].detail.clone();
        assert_eq!(detail(&conns), "PostgreSQL · localhost:5432/shop");
        conns[0].load = Load::Loaded(Vec::new());
        assert_eq!(detail(&conns), "PostgreSQL · localhost:5432/shop · TLS: no");
        conns[0].tls = Some(true);
        assert_eq!(detail(&conns), "PostgreSQL · localhost:5432/shop · TLS");
        conns[0].tls = None;
        assert_eq!(detail(&conns), "PostgreSQL · localhost:5432/shop");
        conns[0].tls = Some(false);
        conns[0].load = Load::Failed("refused".into());
        assert_eq!(detail(&conns), "PostgreSQL · localhost:5432/shop");
    }

    #[test]
    fn connection_states() {
        let mut conns = vec![Conn {
            profile: profile("shop"),
            load: Load::NotLoaded,
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let mut open = HashSet::new();
        assert_eq!(
            labels(&rows(&conns, &open)),
            [(0, "shop".into(), "SQLite · shop.db".into())]
        );
        open.insert(Node::Conn(0));
        assert_eq!(rows(&conns, &open)[1].label, "Loading…");
        // D21: with a keychain entry, the row says the system may ask first.
        conns[0].profile.secret = Some(idecraft_database::SecretRef {
            service: "idecraft".into(),
            account: "shop".into(),
        });
        assert!(rows(&conns, &open)[1].label.contains("keychain"));
        conns[0].profile.secret = None;
        conns[0].load = Load::Failed("password missing".into());
        assert!(rows(&conns, &open)[1].message);
        assert_eq!(
            short_error("connection `staging`: password missing in keychain entry a/b"),
            "Password missing in keychain entry a/b"
        );
        assert_eq!(
            short_error("connection `pg` (127.0.0.1:1): connection refused"),
            "Connection refused"
        );
        assert_eq!(short_error("no such file"), "No such file");
        conns[0].load = Load::Loaded(vec![]);
        assert_eq!(rows(&conns, &open)[1].label, "No tables");
    }

    #[test]
    fn tables_sections_and_details() {
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(tables()),
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let mut open: HashSet<Node> =
            [Node::Conn(0), Node::Group(0, false), Node::Group(0, true)].into();
        assert_eq!(
            labels(&rows(&conns, &open))[1..],
            [
                (1, "tables".into(), "1".into()),
                (2, "orders".into(), String::new()),
                (1, "views".into(), "1".into()),
                (2, "big_orders".into(), String::new()),
            ]
        );
        open.insert(Node::Table(0, "orders".into()));
        for s in [Section::Columns, Section::Keys, Section::Indexes] {
            open.insert(Node::Section(0, "orders".into(), s));
        }
        let all = labels(&rows(&conns, &open));
        assert!(all.contains(&(4, "id".into(), "integer · primary key".into())));
        assert!(all.contains(&(4, "customer_id".into(), "integer · not null".into())));
        assert!(all.contains(&(4, "note".into(), "text".into())));
        // SPEC-0089 v7 AK-18.
        assert!(all.contains(&(
            4,
            "note_length".into(),
            "integer · generated (virtual)".into()
        )));
        assert!(all.contains(&(
            4,
            "foreign key".into(),
            "(customer_id) → customers(id)".into()
        )));
        assert!(all.contains(&(4, "orders_by_customer".into(), "(customer_id)".into())));
    }

    /// SPEC-0089 v15 AK-30: a foreign key's row names its actions, an index's row its method.
    #[test]
    fn foreign_key_actions_and_index_methods_in_rows() {
        let mut t = tables();
        t[0].foreign_keys[0].on_delete = Some("CASCADE".into());
        t[0].foreign_keys[0].on_update = Some("SET NULL".into());
        t[0].indexes[0].method = Some("gin".into());
        t[0].indexes[0].condition = Some("NOT deleted".into());
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(t),
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let mut open: HashSet<Node> = [
            Node::Conn(0),
            Node::Group(0, false),
            Node::Table(0, "orders".into()),
        ]
        .into();
        for s in [Section::Keys, Section::Indexes] {
            open.insert(Node::Section(0, "orders".into(), s));
        }
        let all = labels(&rows(&conns, &open));
        assert!(all.contains(&(
            4,
            "foreign key".into(),
            "(customer_id) → customers(id) on delete cascade on update set null".into()
        )));
        assert!(all.contains(&(
            4,
            "orders_by_customer".into(),
            "(customer_id) · where NOT deleted · gin".into()
        )));
    }

    /// SPEC-0089 v9 AK-21: the condition of a partial index and the collation of a term.
    #[test]
    fn index_rows_show_condition_and_collation() {
        let index =
            |unique, columns: &[&str], condition: Option<&str>, collations: &[Option<&str>]| {
                index_detail(&Index {
                    name: "i".into(),
                    unique,
                    columns: columns.iter().map(|c| (*c).to_owned()).collect(),
                    condition: condition.map(str::to_owned),
                    collations: collations.iter().map(|c| c.map(str::to_owned)).collect(),
                    method: None,
                    orders: Vec::new(),
                    prefixes: Vec::new(),
                })
            };
        assert_eq!(index(false, &["city"], None, &[]), "(city)");
        assert_eq!(index(true, &["city"], None, &[]), "(city) · unique");
        assert_eq!(
            index(false, &["city"], Some("deleted = 0"), &[]),
            "(city) · where deleted = 0"
        );
        assert_eq!(
            index(true, &["sku"], Some("deleted = 0"), &[]),
            "(sku) · unique · where deleted = 0"
        );
        assert_eq!(
            index(false, &["city", "name"], None, &[Some("NOCASE"), None]),
            "(city collate nocase, name)"
        );
        assert_eq!(
            index(
                false,
                &["a"],
                Some("a > 1\n   and b = ' '"),
                &[Some("rtrim")]
            ),
            "(a collate rtrim) · where a > 1 and b = ' '"
        );
        // In the tree, under the table's indexes.
        let mut all = tables();
        all[0].indexes[0].condition = Some("note is not null".into());
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(all),
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let open: HashSet<Node> = [
            Node::Conn(0),
            Node::Group(0, false),
            Node::Table(0, "orders".into()),
            Node::Section(0, "orders".into(), Section::Indexes),
        ]
        .into();
        assert!(labels(&rows(&conns, &open)).contains(&(
            4,
            "orders_by_customer".into(),
            "(customer_id) · where note is not null".into()
        )));
    }

    /// SPEC-0089 v9 AK-22: a view the database refuses to describe says why in its detail and
    /// has no arrow, also when its node is still in the expanded set; the rest is as usual.
    #[test]
    fn a_view_that_cannot_be_read_says_why_and_does_not_expand() {
        let mut all = tables();
        all.push(Table {
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
            comment: None,
        });
        let mut conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(all.clone()),
            schemas: vec![("side".into(), Load::Loaded(all))],
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let open: HashSet<Node> = [
            Node::Conn(0),
            Node::Group(0, true),
            Node::Table(0, "orphan".into()),
            Node::Table(0, "big_orders".into()),
            Node::Schema(0, "side".into()),
            Node::SchemaTable(0, "side".into(), "orphan".into()),
            Node::SchemaTable(0, "side".into(), "big_orders".into()),
        ]
        .into();
        let r = rows(&conns, &open);
        let at = |depth: usize, label: &str| {
            r.iter()
                .position(|row| row.depth == depth && row.label == label)
                .unwrap_or_else(|| panic!("no row {label}"))
        };
        // Current schema: the group says "views", so the detail is the reason alone.
        let broken = &r[at(2, "orphan")];
        assert_eq!(broken.detail, "cannot be read: no such table: main.gone");
        assert!(!broken.has_children && !broken.expanded);
        assert_eq!(broken.node, Some(Node::Table(0, "orphan".into())));
        let sound = at(2, "big_orders");
        assert!(r[sound].has_children && r[sound].expanded);
        assert_eq!(r[sound].detail, "");
        assert_eq!(
            (r[sound + 1].depth, r[sound + 1].label.as_str()),
            (3, "columns")
        );
        // Nothing hangs below the broken view: the next row is not deeper.
        let i = at(2, "orphan");
        assert!(r.get(i + 1).is_none_or(|next| next.depth <= 2));
        // Another schema: the row already says "view".
        let side = r.iter().rposition(|row| row.label == "orphan").unwrap();
        assert!(side > i);
        assert_eq!(
            r[side].detail,
            "view · cannot be read: no such table: main.gone"
        );
        assert!(!r[side].has_children && !r[side].expanded);
        let side_sound = r.iter().rposition(|row| row.label == "big_orders").unwrap();
        assert!(r[side_sound].has_children && r[side_sound].expanded);
        assert_eq!(r[side_sound].detail, "view");
        // A message over several lines stays one row of text.
        if let Load::Loaded(tables) = &mut conns[0].load {
            tables[2].unreadable = Some("no such column:\n  nope".into());
        }
        let r = rows(&conns, &open);
        assert!(
            r.iter()
                .any(|row| row.detail == "cannot be read: no such column: nope")
        );
    }

    #[test]
    fn error_node_with_retry() {
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Failed("connection `shop`: no such file".into()),
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let open: HashSet<Node> = [Node::Conn(0)].into();
        let r = rows(&conns, &open);
        assert_eq!(r[1].label, "Could not read the schema: No such file");
        assert_eq!(r[1].icon, "status/warning");
        assert_eq!(r[1].node, Some(Node::Retry(0)));
        assert_eq!(
            (r[2].label.as_str(), r[2].node.clone()),
            ("Retry", Some(Node::Retry(0)))
        );
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn other_schemas_load_on_expand() {
        let mut conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(tables()),
            schemas: vec![
                ("audit".into(), Load::NotLoaded),
                ("staging".into(), Load::Failed("permission denied".into())),
            ],
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let mut open: HashSet<Node> = [Node::Conn(0)].into();
        assert_eq!(
            labels(&rows(&conns, &open))[3..],
            [
                (1, "audit".into(), "schema".into()),
                (1, "staging".into(), "schema".into()),
            ]
        );
        open.insert(Node::Schema(0, "audit".into()));
        open.insert(Node::Schema(0, "staging".into()));
        let r = rows(&conns, &open);
        assert_eq!(r[4].label, "Loading…");
        assert_eq!(r[6].label, "Could not read the schema: Permission denied");
        assert_eq!(r[7].node, Some(Node::SchemaRetry(0, "staging".into())));
        conns[0].schemas[0].1 = Load::Loaded(tables());
        open.insert(Node::SchemaTable(0, "audit".into(), "orders".into()));
        let all = labels(&rows(&conns, &open));
        assert!(all.contains(&(2, "orders".into(), String::new())));
        assert!(all.contains(&(3, "customer_id".into(), "integer".into())));
        assert!(all.contains(&(2, "big_orders".into(), "view".into())));
        assert_eq!(Node::SchemaTable(0, "a".into(), "b".into()).conn(), 0);
        assert_eq!(Node::Store(0, Vec::new()).conn(), usize::MAX);
    }

    // SPEC-0089 v5 AK-11: a PostgreSQL connection lists its sequences with their count.
    #[test]
    fn sequences_group_with_count() {
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(tables()),
            schemas: vec![("audit".into(), Load::NotLoaded)],
            sequences: vec!["orders_id_seq".into()],
            tls: None,
            objects: Vec::new(),
        }];
        let mut open: HashSet<Node> = [Node::Conn(0)].into();
        assert_eq!(
            labels(&rows(&conns, &open))[1..],
            [
                (1, "tables".into(), "1".into()),
                (1, "views".into(), "1".into()),
                (1, "sequences".into(), "1".into()),
                (1, "audit".into(), "schema".into()),
            ]
        );
        open.insert(Node::Sequences(0));
        let r = rows(&conns, &open);
        assert_eq!((r[4].depth, r[4].label.as_str()), (2, "orders_id_seq"));
        assert_eq!(Node::Sequences(3).conn(), 3);
    }

    // SPEC-0089 v13 AK-27: routines, triggers and types in folders of their kind, in the order of
    // the dialect's kinds, overloads told apart by their arguments.
    #[test]
    fn routines_triggers_and_types_in_folders() {
        let object = |kind: &str, name: &str, table: Option<&str>, detail: Option<&str>| {
            idecraft_database::SchemaObject {
                name: name.into(),
                kind: kind.into(),
                table: table.map(str::to_owned),
                detail: detail.map(str::to_owned),
            }
        };
        let mut p = profile("shop");
        p.dialect = idecraft_database::Dialect::PostgreSql;
        let conns = vec![Conn {
            profile: p,
            load: Load::Loaded(Vec::new()),
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: vec![
                object("type", "mood", None, Some("enum")),
                object(
                    "trigger",
                    "stock_check",
                    Some("stock"),
                    Some("BEFORE INSERT"),
                ),
                object("function", "add_tax", None, Some("()")),
                object("function", "add_tax", None, Some("(amount numeric)")),
            ],
        }];
        let mut open: HashSet<Node> = [Node::Conn(0)].into();
        assert_eq!(
            labels(&rows(&conns, &open))[2..],
            [
                (1, "functions".into(), "2".into()),
                (1, "triggers".into(), "1".into()),
                (1, "types".into(), "1".into()),
            ]
        );
        for kind in ["function", "trigger", "type"] {
            open.insert(Node::Objects(0, kind.into()));
        }
        let r = rows(&conns, &open);
        let shown: Vec<(usize, String, String, &str)> = r[2..]
            .iter()
            .map(|r| (r.depth, r.label.clone(), r.detail.clone(), r.icon))
            .collect();
        assert_eq!(
            shown,
            [
                (1, "functions".into(), "2".into(), "nodes/folder"),
                (2, "add_tax()".into(), String::new(), "nodes/function"),
                (
                    2,
                    "add_tax(amount numeric)".into(),
                    String::new(),
                    "nodes/function"
                ),
                (1, "triggers".into(), "1".into(), "nodes/folder"),
                (
                    2,
                    "stock_check".into(),
                    "stock · BEFORE INSERT".into(),
                    "nodes/method"
                ),
                (1, "types".into(), "1".into(), "nodes/folder"),
                (2, "mood".into(), "enum".into(), "nodes/enum"),
            ]
        );
        assert!(
            matches!(&r[3].node, Some(Node::Object(0, o)) if o.detail.as_deref() == Some("()"))
        );
        assert_eq!(Node::Objects(1, "type".into()).moved_to(4).conn(), 4);
    }

    // SPEC-0164 v3 AK-7: index, sequence and named foreign key rows carry their object, so ⇧F6
    // can rename them; an unnamed foreign key is no object.
    #[test]
    fn index_sequence_and_key_rows_are_objects() {
        let mut t = tables();
        let fk = t[0].foreign_keys[0].clone();
        t[0].foreign_keys.push(fk);
        t[0].foreign_keys[1].name = Some("orders_customer_fk".into());
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(t),
            schemas: Vec::new(),
            sequences: vec!["orders_id_seq".into()],
            tls: None,
            objects: Vec::new(),
        }];
        let open: HashSet<Node> = [
            Node::Conn(0),
            Node::Group(0, false),
            Node::Sequences(0),
            Node::Table(0, "orders".into()),
            Node::Section(0, "orders".into(), Section::Keys),
            Node::Section(0, "orders".into(), Section::Indexes),
        ]
        .into();
        let nodes: Vec<Node> = rows(&conns, &open)
            .into_iter()
            .filter_map(|r| r.node)
            .collect();
        assert!(nodes.contains(&Node::Index(
            0,
            "orders".into(),
            "orders_by_customer".into()
        )));
        assert!(nodes.contains(&Node::Key(0, "orders".into(), "orders_customer_fk".into())));
        assert!(nodes.contains(&Node::Sequence(0, "orders_id_seq".into())));
        assert_eq!(
            nodes.iter().filter(|n| matches!(n, Node::Key(..))).count(),
            1
        );
        assert_eq!(Node::Sequence(2, "s".into()).moved_to(5).conn(), 5);
    }

    // SPEC-0089 v5 AK-8: speed search skips message rows and finds `orders` from the top.
    #[test]
    fn speed_search_finds_visible_rows() {
        let conns = vec![Conn {
            profile: profile("shop"),
            load: Load::Loaded(tables()),
            schemas: Vec::new(),
            sequences: Vec::new(),
            tls: None,
            objects: Vec::new(),
        }];
        let open: HashSet<Node> = [Node::Conn(0), Node::Group(0, false)].into();
        let r = rows(&conns, &open);
        let labels = search_labels(&r);
        assert_eq!(crate::speed_search::first_match(&labels, "ORD", 0), Some(2));
        // big_orders is in the closed views group: not visible, not found.
        assert_eq!(crate::speed_search::step(&labels, "ord", 2, true), Some(2));
        let loading = vec![Conn {
            load: Load::Loading,
            ..conns[0].clone()
        }];
        let r = rows(&loading, &open);
        assert_eq!(search_labels(&r)[1], None);
    }
}

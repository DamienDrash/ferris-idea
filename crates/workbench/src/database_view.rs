//! The Database tool window (`SPEC-0089`): the workspace's connections with their tables, views,
//! columns, keys and indexes, read in the background when a connection is expanded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use idecraft_database::{
    ConnectionProfile, PROFILE_FILE, Session, SystemStore, Table, introspect, load_profiles,
    resolve_credentials,
};

use crate::db_tree_model::{Conn, Load, Node, Row};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// What the tool window asks the workbench to do.
pub enum DatabaseEvent {
    /// Hide the tool window.
    Hide,
    /// Open the connection dialog for a new connection (`SPEC-0108` behaviour 1).
    NewConnection,
    /// Open the connection dialog for this connection.
    EditConnection(ConnectionProfile),
    /// Remove the connection with this id.
    RemoveConnection(String),
    /// Show the ER diagram of the selection (`SPEC-0111` behaviour 1).
    ShowDiagram,
    /// Rename the selected table or column (`SPEC-0128` behaviour 1).
    Rename(RenameTarget),
    /// Reading the schema of the connection with this id failed: drop what is cached about it
    /// (`SPEC-0089` v3 behaviour 9).
    SchemaFailed(String),
    /// Open the data editor of a table or view (`SPEC-0190` behaviour 2).
    OpenTable(ConnectionProfile, String),
    /// Copy a table to another schema or name (`SPEC-0191` behaviour 1).
    CopyTable(RenameTarget),
    /// A secondary click at this window position: show the tree's context menu (`SPEC-0275`
    /// behaviour 15).
    ContextMenu(gpui::Point<gpui::Pixels>),
    /// The schema of the connection with this id (every connection for `None`) was read again by
    /// Refresh: what the SQL editors cached about it is out of date (D31, `SPEC-0089` v4).
    Refreshed(Option<String>),
    /// Show the definition of this table or view as SQL in a read-only tab (`SPEC-0089` v5
    /// behaviour 11).
    ShowDdl(ConnectionProfile, String),
    /// Show DDL on a routine, trigger or type of the connection's current schema (`SPEC-0089` v13
    /// behaviour 18).
    ShowObjectDdl(ConnectionProfile, idecraft_database::SchemaObject),
    /// Open a new console of this connection (`SPEC-0089` v6 behaviour 14).
    NewConsole(ConnectionProfile),
    /// Show this text in the status bar (`SPEC-0164` v3 behaviour 10).
    Note(String),
}

/// A table or column to rename, with what the rename needs to know.
#[derive(Clone)]
pub struct RenameTarget {
    /// The connection.
    pub profile: ConnectionProfile,
    /// Its tables as read (for the analysis of references).
    pub tables: Vec<Table>,
    /// The table.
    pub table: String,
    /// The column, when a column is renamed or its type changed.
    pub column: Option<String>,
    /// What is done (`SPEC-0164`); renaming the table or column by default.
    pub what: idecraft_database::Refactoring,
    /// Named constraints of the connection, for derived names (`SPEC-0019` v2); read when the
    /// dialog opens.
    pub constraints: Vec<idecraft_database::ConstraintName>,
    /// The schema the connection works in, where a renamed sequence lives (`SPEC-0164` v5
    /// behaviour 8); read when the dialog opens, `None` until then.
    pub home_schema: Option<String>,
}

impl RenameTarget {
    /// What the field starts with: the current name, or the column's current type.
    pub fn current(&self) -> String {
        use idecraft_database::Refactoring as R;
        match &self.what {
            R::RenameIndex(n) | R::RenameRoutine(n) | R::RenameSequence(n) => n.clone(),
            R::RenameConstraint { name, .. } => name.clone(),
            R::ChangeType => self
                .tables
                .iter()
                .find(|t| t.name == self.table)
                .and_then(|t| {
                    t.columns
                        .iter()
                        .find(|c| Some(&c.name) == self.column.as_ref())
                })
                .map(|c| c.data_type.clone())
                .unwrap_or_default(),
            R::Rename => self.column.clone().unwrap_or_else(|| self.table.clone()),
        }
    }

    /// The dialog's title, e.g. `Rename column stock.qty`.
    pub fn title(&self) -> String {
        use idecraft_database::Refactoring as R;
        let kind = self
            .tables
            .iter()
            .find(|t| t.name == self.table)
            .map_or("table", |t| if t.kind == "view" { "view" } else { "table" });
        match (&self.what, &self.column) {
            (R::RenameIndex(n), _) => format!("Rename index {n} of {}", self.table),
            (R::RenameRoutine(n), _) => format!("Rename routine {n}"),
            (R::RenameSequence(n), _) => format!("Rename sequence {n}"),
            (R::RenameConstraint { name, kind }, _) => {
                format!("Rename {kind} {name} of {}", self.table)
            }
            (R::ChangeType, Some(c)) => format!("Change type of column {}.{c}", self.table),
            (R::ChangeType, None) => format!("Change column type in {}", self.table),
            (R::Rename, Some(c)) => format!("Rename column {}.{c}", self.table),
            (R::Rename, None) => format!("Rename {kind} {}", self.table),
        }
    }
}

/// The selected row's node and the nodes of the rows it hangs under, nearest first: what the
/// selection follows while rows come and go (`SPEC-0089` v14 behaviour 26).
type Selected = (Node, Vec<Node>);

/// Connections and the state of the tree.
pub struct DatabaseView {
    root: PathBuf,
    conns: Vec<Conn>,
    /// Non-relational stores of `.idecraft/stores.toml` (`SPEC-0310`).
    stores: Vec<crate::db_tree_model::StoreConn>,
    /// Why `connections.toml` could not be read, if so.
    problem: Option<String>,
    expanded: HashSet<Node>,
    /// The node to select again once its row is back (after [`Self::reload_keeping`] or while its
    /// connection is read again), with the nodes it hangs under, nearest first (`SPEC-0089` v14
    /// behaviour 26).
    keep_selection: Option<Selected>,
    /// The row the user selected (a click, the keys, an action that selects a node); none until
    /// then, so an untouched tree names no connection (`SPEC-0192` v7 behaviour 3b, OBS-0070 G1).
    selected: Option<usize>,
    focus: FocusHandle,
    /// Delete was pressed once on this connection; a second press removes it.
    removing: Option<String>,
    /// The speed search's text while it is open (`SPEC-0089` v5 behaviour 10).
    search: Option<String>,
    /// Whether connections show only the schemas of their schema filter (v6 behaviour 16).
    filter_schemas: bool,
    /// The quick documentation shown for the selection (v6 behaviour 13).
    doc: Option<Vec<String>>,
}

impl EventEmitter<DatabaseEvent> for DatabaseView {}

/// What reading a schema gives.
struct ReadSchema {
    /// The tables and views.
    tables: Vec<Table>,
    /// The names of the other schemas.
    others: Vec<String>,
    /// The sequences.
    sequences: Vec<String>,
    /// The routines, triggers and types (`SPEC-0089` v13 behaviour 25).
    objects: Vec<idecraft_database::SchemaObject>,
    /// Whether the session was encrypted (`SPEC-0089` v12 behaviour 24); `None` when the server
    /// does not say.
    tls: Option<bool>,
}

/// Reads the tables and views of `profile`'s current schema, the names of its other schemas and
/// its sequences (behaviours 3, 8 and v5 behaviour 12; a failed listing leaves the names out), or
/// the tables and views of schema `other` alone.
/// Runs on a background thread: it reads the keychain (OBS-0026 L26).
fn read_schema(profile: &ConnectionProfile, other: Option<&str>) -> Result<ReadSchema, String> {
    let credentials = resolve_credentials(profile, &SystemStore).map_err(|e| e.to_string())?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let session = Session::connect(profile, &credentials)
            .await
            .map_err(|e| e.to_string())?;
        if other.is_some() {
            let tables = idecraft_database::introspect_schema(&session, other)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(ReadSchema {
                tables,
                others: Vec::new(),
                sequences: Vec::new(),
                objects: Vec::new(),
                tls: None,
            });
        }
        let tables = introspect(&session).await.map_err(|e| e.to_string())?;
        let others = idecraft_database::list_schemas(&session)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.current)
            .map(|s| s.name)
            .collect();
        let sequences = idecraft_database::sequences(&session, None)
            .await
            .unwrap_or_default();
        // SPEC-0089 v13 behaviour 25: routines, triggers and types; sequences have their folder.
        let objects = idecraft_database::schema_objects(&session, None)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|o| o.kind != "sequence")
            .collect();
        Ok(ReadSchema {
            tables,
            others,
            sequences,
            objects,
            tls: session.encrypted().await,
        })
    })
}

/// The quick documentation of a connection (`SPEC-0089` v6 behaviour 13): system and address,
/// tunnel, SSL mode, options and comment — never a password.
pub fn connection_doc(p: &ConnectionProfile) -> Vec<String> {
    let mut lines = vec![
        format!("connection {}", p.name),
        format!("{} · {}", p.dialect.label(), p.to_url()),
    ];
    if let Some(ssh) = &p.ssh {
        lines.push(format!("SSH tunnel through {}", ssh.host));
    }
    if !p.dialect.file_based() {
        lines.push(format!(
            "SSL: {}",
            match p.ssl {
                idecraft_database::SslMode::Disable => "disable",
                idecraft_database::SslMode::Prefer => "prefer",
                idecraft_database::SslMode::Require => "require",
                idecraft_database::SslMode::VerifyCa => "verify-ca",
                idecraft_database::SslMode::VerifyFull => "verify-full",
            }
        ));
    }
    if p.read_only {
        lines.push("Read-only".into());
    }
    if !p.options.auto_commit {
        lines.push("Consoles and editors start in Manual".into());
    }
    if let Some(s) = p.options.query_timeout_secs {
        lines.push(format!("Statements end after {s} s"));
    }
    if p.options.startup_script.is_some() {
        lines.push(if p.options.startup_allowed {
            "Runs a startup script after connecting".into()
        } else {
            "Startup script not run: the project is not trusted".into()
        });
    }
    if !p.options.schemas.is_empty() {
        lines.push(format!("Schemas: {}", p.options.schemas.join(", ")));
    }
    if let Some(c) = &p.comment {
        lines.push(c.clone());
    }
    lines
}

impl DatabaseView {
    /// The tool window for the workspace at `root`.
    pub fn new(root: &Path, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            root: root.to_path_buf(),
            conns: Vec::new(),
            stores: Vec::new(),
            problem: None,
            expanded: HashSet::new(),
            keep_selection: None,
            selected: None,
            focus: cx.focus_handle(),
            removing: None,
            search: None,
            filter_schemas: true,
            doc: None,
        };
        view.reload();
        view
    }

    /// The quick documentation of the selected table, view or column (`SPEC-0089` v6 behaviour
    /// 13), from what the tree already read: nothing is asked of the database and nothing waits
    /// for a language engine (IMP-0367). For a connection: system, address, comment, options.
    pub fn quick_doc(&self) -> Option<Vec<String>> {
        let rows = self.rows();
        let node = rows.get(self.selected?)?.node.clone()?;
        let conn = self.conns.get(node.conn())?;
        let tables = |load: &Load| match load {
            Load::Loaded(t) => Some(t.clone()),
            _ => None,
        };
        match &node {
            Node::Conn(_) => Some(connection_doc(&conn.profile)),
            Node::Table(_, t) => {
                idecraft_database::object_doc(&conn.profile, &tables(&conn.load)?, t, None)
            }
            Node::Column(_, t, c) => {
                idecraft_database::object_doc(&conn.profile, &tables(&conn.load)?, t, Some(c))
            }
            Node::SchemaTable(_, s, t) => {
                let load = &conn.schemas.iter().find(|(n, _)| n == s)?.1;
                idecraft_database::object_doc(&conn.profile, &tables(load)?, t, None)
            }
            _ => None,
        }
    }

    /// F1 / ⌃J: shows or hides the quick documentation of the selection (v6 behaviour 13).
    pub fn toggle_doc(&mut self, cx: &mut Context<Self>) {
        self.doc = match self.doc.take() {
            Some(_) => None,
            None => Some(self.quick_doc().unwrap_or_else(|| {
                vec!["No documentation: select a connection, table, view or column".into()]
            })),
        };
        cx.notify();
    }

    /// The shown quick documentation (tests).
    pub fn doc_lines(&self) -> Option<&[String]> {
        self.doc.as_deref()
    }

    /// Collapses every row (v6 behaviour 14).
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let rows = self.rows();
        let conn = self
            .selected
            .and_then(|i| rows.get(i))
            .and_then(|r| r.node.as_ref())
            .map(|n| n.conn());
        self.expanded.clear();
        let shown = self.rows();
        self.selected = conn.and_then(|c| {
            shown
                .iter()
                .position(|r| matches!(r.node, Some(Node::Conn(i)) if i == c))
        });
        self.keep_selection = None;
        self.doc = None;
        cx.notify();
    }

    /// Whether connections show only the schemas of their filter (v6 behaviour 16).
    pub fn filters_schemas(&self) -> bool {
        self.filter_schemas
    }

    /// Switches between the connections' schema filters and all schemas, and reads the schema
    /// lists again (v6 behaviour 16).
    pub fn toggle_schema_filter(&mut self, cx: &mut Context<Self>) {
        self.filter_schemas = !self.filter_schemas;
        self.reload();
        cx.emit(DatabaseEvent::Refreshed(None));
        cx.notify();
    }

    /// Reads `.idecraft/connections.toml` again and forgets every loaded schema (behaviour 5).
    pub fn reload(&mut self) {
        match load_profiles(&self.root) {
            Ok(profiles) => {
                self.problem = None;
                self.conns = profiles
                    .into_iter()
                    .map(|profile| Conn {
                        profile,
                        load: Load::NotLoaded,
                        schemas: Vec::new(),
                        sequences: Vec::new(),
                        tls: None,
                        objects: Vec::new(),
                    })
                    .collect();
            }
            Err(e) => {
                self.problem = Some(format!("{PROFILE_FILE}: {e}"));
                self.conns.clear();
            }
        }
        match idecraft_nosql::load_stores(&self.root) {
            Ok(stores) => {
                self.stores = stores
                    .into_iter()
                    .map(|profile| crate::db_tree_model::StoreConn {
                        profile,
                        levels: Default::default(),
                    })
                    .collect();
            }
            Err(e) => {
                let text = e.to_string();
                self.problem = Some(match self.problem.take() {
                    Some(p) => format!("{p}; {text}"),
                    None => text,
                });
                self.stores.clear();
            }
        }
        self.expanded.clear();
        self.selected = None;
        self.keep_selection = None;
        self.search = None;
    }

    /// [`Self::reload`], keeping the open nodes and the selection (`SPEC-0089` v10 behaviour 22):
    /// Refresh and saving a connection read the file again without collapsing the tree. An open
    /// connection or schema is read again; nodes of a connection that is gone are dropped.
    pub fn reload_keeping(&mut self, cx: &mut Context<Self>) {
        let old_conns: Vec<String> = self.conns.iter().map(|c| c.profile.id.clone()).collect();
        let old_stores: Vec<String> = self.stores.iter().map(|s| s.profile.id.clone()).collect();
        let open: Vec<Node> = self.expanded.iter().cloned().collect();
        let selected = self.selection_path();
        let search = self.search.take();
        self.reload();
        self.search = search;
        let new_conns: Vec<String> = self.conns.iter().map(|c| c.profile.id.clone()).collect();
        let new_stores: Vec<String> = self.stores.iter().map(|s| s.profile.id.clone()).collect();
        let carry = |n: &Node| {
            crate::db_tree_model::carried_over(n, &old_conns, &new_conns, &old_stores, &new_stores)
        };
        let mut open: Vec<Node> = open.iter().filter_map(carry).collect();
        // Connections and schemas first: they are read again, the nodes below show once loaded.
        open.sort_by_key(|n| !matches!(n, Node::Conn(_) | Node::Schema(..)));
        for node in open {
            match node {
                Node::Conn(_) | Node::Schema(..) | Node::Store(..) => self.expand(node, cx),
                other => {
                    self.expanded.insert(other);
                }
            }
        }
        // A parent of a connection that is gone is dropped with it; the connection row stays.
        let selected = selected.and_then(|(node, parents)| {
            Some((carry(&node)?, parents.iter().filter_map(carry).collect()))
        });
        self.reselect(selected, true);
        // Every refresh of the tree tells the SQL editors (SPEC-0089 v11 behaviour 23): until v11
        // the toolbar button refreshed the tree alone and the editors kept the old schema.
        cx.emit(DatabaseEvent::Refreshed(None));
        cx.notify();
    }

    /// What the toolbar's Refresh button runs (`SPEC-0089` v10 behaviour 22, v11 behaviour 23): the
    /// whole tree is read again, keeping it open, and the SQL editors drop their schemas.
    pub fn refresh_from_toolbar(&mut self, cx: &mut Context<Self>) {
        self.reload_keeping(cx);
    }

    /// Selects the node kept by [`Self::reload_keeping`] once its row is there again.
    fn select_kept(&mut self) {
        let kept = self.keep_selection.take();
        self.reselect(kept, true);
    }

    /// The selected row's node with the nodes of the rows it hangs under, nearest first; `None`
    /// without a selection or on a message row.
    fn selection_path(&self) -> Option<Selected> {
        let rows = self.rows();
        let at = self.selected?;
        let row = rows.get(at)?;
        let node = row.node.clone()?;
        let mut depth = row.depth;
        let mut parents = Vec::new();
        for r in rows[..at].iter().rev() {
            if r.depth < depth {
                depth = r.depth;
                parents.extend(r.node.clone());
            }
        }
        Some((node, parents))
    }

    /// Selects `path`'s node again after the rows changed (`SPEC-0089` v14 behaviour 26): the
    /// selection is the node, not the row number it had. While the node has no row, its nearest
    /// parent that has one is selected; with `keep` the node is selected again once its row is back
    /// (a connection being read again), unless the user selects another row first.
    fn reselect(&mut self, path: Option<Selected>, keep: bool) {
        let Some((node, parents)) = path else {
            return;
        };
        let rows = self.rows();
        let at = |n: &Node| rows.iter().position(|r| r.node.as_ref() == Some(n));
        if let Some(i) = at(&node) {
            self.selected = Some(i);
            self.keep_selection = None;
            return;
        }
        self.selected = parents.iter().find_map(at);
        self.keep_selection = keep.then_some((node, parents));
    }

    /// Runs `change`, which adds or removes rows, keeping the selection on its node.
    fn keeping_selection<R>(&mut self, keep: bool, change: impl FnOnce(&mut Self) -> R) -> R {
        let path = self.selection_path();
        let out = change(self);
        if self.keep_selection.is_some() {
            self.select_kept();
        } else {
            self.reselect(path, keep);
        }
        out
    }

    /// The node of the selected row, if the user selected one.
    fn selected_node(&self) -> Option<Node> {
        self.rows().get(self.selected?)?.node.clone()
    }

    /// The profile of the connection the selected row belongs to (the SQL console's connection,
    /// SPEC-0093 behaviour 2).
    pub fn selected_profile(&self) -> Option<ConnectionProfile> {
        let rows = self.rows();
        let conn = rows.get(self.selected?)?.node.as_ref()?.conn();
        self.conns.get(conn).map(|c| c.profile.clone())
    }

    /// The table selected in the tree, if a table row is selected.
    pub fn selected_table(&self) -> Option<String> {
        match self.rows().get(self.selected?)?.node.as_ref()? {
            Node::Table(_, name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The table or column selected in the tree with its connection's schema, if loaded.
    pub fn selected_object(&self) -> Option<RenameTarget> {
        use idecraft_database::Refactoring as R;
        // SPEC-0164 v3 behaviour 10: also index, sequence and named foreign key rows.
        let (conn, table, column, what) = match self.selected_node()? {
            Node::Table(i, t) => (i, t, None, R::Rename),
            Node::Column(i, t, c) => (i, t, Some(c), R::Rename),
            Node::Index(i, t, n) => (i, t, None, R::RenameIndex(n)),
            Node::Key(i, t, n) => (
                i,
                t,
                None,
                R::RenameConstraint {
                    name: n,
                    kind: "foreign key".into(),
                },
            ),
            Node::Sequence(i, n) => (i, String::new(), None, R::RenameSequence(n)),
            _ => return None,
        };
        let c = self.conns.get(conn)?;
        let Load::Loaded(tables) = &c.load else {
            return None;
        };
        Some(RenameTarget {
            profile: c.profile.clone(),
            tables: tables.clone(),
            table,
            column,
            what,
            constraints: Vec::new(),
            home_schema: None,
        })
    }

    /// The selected column as a type change (⌘F6, `SPEC-0164` v3 behaviour 10).
    pub fn selected_type_change(&self) -> Option<RenameTarget> {
        let mut target = self.selected_object()?;
        if target.column.is_none() || target.what != idecraft_database::Refactoring::Rename {
            return None;
        }
        target.what = idecraft_database::Refactoring::ChangeType;
        Some(target)
    }

    /// ⌘F6 / "Change Column Type…" on a column row; elsewhere the status bar says what it needs.
    pub fn change_type_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected_type_change() {
            Some(target) => cx.emit(DatabaseEvent::Rename(target)),
            None => cx.emit(DatabaseEvent::Note(
                "Select a column of a loaded table to change its type".into(),
            )),
        }
    }

    /// Reads the schema of the connection with `id` again, keeping the tree open (after a rename).
    /// The selection stays on its node: on the connection while it is read, back on the node once
    /// its row is there again, on its nearest parent if it is gone (`SPEC-0089` v14 behaviour 26).
    pub fn refresh_connection(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.conns.iter().position(|c| c.profile.id == id) {
            self.keeping_selection(true, |this| {
                this.conns[i].load = Load::NotLoaded;
                this.expand_rows(Node::Conn(i), cx);
            });
        }
    }

    /// Selects the first visible row named `label` (tests).
    pub fn select_label(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        match self.rows().iter().position(|r| r.label == label) {
            Some(i) => {
                self.select(i, cx);
                true
            }
            None => false,
        }
    }

    /// Focus handle of the tree.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Visible rows: depth, name and grey detail — the same rows the window paints, stores
    /// included (`SPEC-0310` behaviour 5).
    pub fn visible(&self) -> Vec<(usize, String, String)> {
        crate::db_tree_model::visible(&self.rows())
    }

    fn rows(&self) -> Vec<Row> {
        crate::db_tree_model::all_rows(&self.conns, &self.stores, &self.expanded)
    }

    /// Reads one level of a store in the background (`SPEC-0310` behaviour 5).
    fn expand_store(&mut self, i: usize, path: Vec<String>, cx: &mut Context<Self>) {
        use crate::db_tree_model::StoreLoad;
        let Some(store) = self.stores.get_mut(i) else {
            return;
        };
        let state = store.levels.entry(path.clone()).or_default();
        if matches!(state, StoreLoad::NotLoaded | StoreLoad::Failed(_)) {
            *state = StoreLoad::Loading;
            let profile = store.profile.clone();
            let at = path.clone();
            cx.spawn(async move |this, cx| {
                let read = cx
                    .background_spawn(async move {
                        let password = profile.password(&SystemStore).map_err(|e| e.to_string())?;
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| e.to_string())?;
                        rt.block_on(idecraft_nosql::browse(&profile, password, &at, 0))
                            .map(|level| level.items)
                            .map_err(|e| e.to_string())
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.keeping_selection(true, |this| {
                        if let Some(store) = this.stores.get_mut(i) {
                            store.levels.insert(
                                path,
                                match read {
                                    Ok(items) => StoreLoad::Loaded(items),
                                    Err(e) => StoreLoad::Failed(e),
                                },
                            );
                        }
                    });
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Expands `node`; a connection or another schema is read in the background the first time
    /// (behaviours 3 and 8); a retry row reads it again (v3 behaviour 9). The selection stays on
    /// its node (v14 behaviour 26).
    pub fn expand(&mut self, node: Node, cx: &mut Context<Self>) {
        self.keeping_selection(true, |this| this.expand_rows(node, cx));
    }

    /// [`Self::expand`] without keeping the selection.
    fn expand_rows(&mut self, node: Node, cx: &mut Context<Self>) {
        if let Node::Store(i, path) = &node {
            self.expand_store(*i, path.clone(), cx);
            self.expanded.insert(node);
            cx.notify();
            return;
        }
        match node {
            Node::Retry(i) => {
                if let Some(conn) = self.conns.get_mut(i) {
                    conn.load = Load::NotLoaded;
                }
                return self.expand_rows(Node::Conn(i), cx);
            }
            Node::SchemaRetry(i, name) => {
                if let Some(entry) = self
                    .conns
                    .get_mut(i)
                    .and_then(|c| c.schemas.iter_mut().find(|(n, _)| *n == name))
                {
                    entry.1 = Load::NotLoaded;
                }
                return self.expand_rows(Node::Schema(i, name), cx);
            }
            _ => {}
        }
        let read = match &node {
            Node::Conn(i) => self
                .conns
                .get_mut(*i)
                .filter(|c| matches!(c.load, Load::NotLoaded | Load::Failed(_)))
                .map(|c| {
                    c.load = Load::Loading;
                    (c.profile.clone(), None)
                }),
            Node::Schema(i, name) => self.conns.get_mut(*i).and_then(|c| {
                let profile = c.profile.clone();
                let entry = c.schemas.iter_mut().find(|(n, _)| n == name)?;
                matches!(entry.1, Load::NotLoaded | Load::Failed(_)).then(|| {
                    entry.1 = Load::Loading;
                    (profile, Some(name.clone()))
                })
            }),
            _ => None,
        };
        if let Some((profile, other)) = read {
            let i = node.conn();
            cx.spawn(async move |this, cx| {
                let schema = other.clone();
                let read = cx
                    .background_spawn(async move { read_schema(&profile, schema.as_deref()) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    // The rows the read adds come before the rows of later connections: the
                    // selected node stays selected, not the row that takes its place (v14
                    // behaviour 26).
                    let chosen = this.selection_path();
                    let Some(conn) = this.conns.get_mut(i) else {
                        return;
                    };
                    let id = conn.profile.id.clone();
                    let failed = read.is_err();
                    match other {
                        None => match read {
                            Ok(ReadSchema {
                                tables,
                                others,
                                sequences,
                                objects,
                                tls,
                            }) => {
                                conn.load = Load::Loaded(tables);
                                conn.sequences = sequences;
                                conn.objects = objects;
                                conn.tls = tls;
                                // SPEC-0089 v6 behaviour 16: the connection's schema filter.
                                let filter = this.filter_schemas;
                                let options = conn.profile.options.clone();
                                conn.schemas = others
                                    .into_iter()
                                    .filter(|n| !filter || options.shows_schema(n))
                                    .map(|n| (n, Load::NotLoaded))
                                    .collect();
                            }
                            Err(e) => conn.load = Load::Failed(e),
                        },
                        Some(name) => {
                            if let Some(entry) = conn.schemas.iter_mut().find(|(n, _)| *n == name) {
                                entry.1 = match read {
                                    Ok(ReadSchema { tables, .. }) => Load::Loaded(tables),
                                    Err(e) => Load::Failed(e),
                                };
                            }
                        }
                    }
                    if failed {
                        cx.emit(DatabaseEvent::SchemaFailed(id));
                    }
                    if this.keep_selection.is_some() {
                        this.select_kept();
                    } else {
                        this.reselect(chosen, true);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        self.expanded.insert(node);
        cx.notify();
    }

    /// Expands the whole first level of every loaded connection (groups and tables), for checks.
    pub fn expand_all_tables(&mut self, cx: &mut Context<Self>) {
        let mut nodes = Vec::new();
        for (i, conn) in self.conns.iter().enumerate() {
            if let Load::Loaded(tables) = &conn.load {
                nodes.push(Node::Group(i, false));
                nodes.push(Node::Group(i, true));
                for t in tables {
                    nodes.push(Node::Table(i, t.name.clone()));
                    for s in [
                        crate::db_tree_model::Section::Columns,
                        crate::db_tree_model::Section::Keys,
                        crate::db_tree_model::Section::Indexes,
                    ] {
                        nodes.push(Node::Section(i, t.name.clone(), s));
                    }
                }
            }
        }
        self.keeping_selection(true, |this| this.expanded.extend(nodes));
        cx.notify();
    }

    /// The rows as the shared tree behaviour sees them; a retry row expands (reads again).
    fn nav_rows(rows: &[Row]) -> Vec<crate::tree_nav::NavRow> {
        rows.iter()
            .map(|r| {
                let retry = matches!(r.node, Some(Node::Retry(_) | Node::SchemaRetry(..)));
                crate::tree_nav::NavRow::new(r.depth, r.has_children || retry, r.expanded)
            })
            .collect()
    }

    /// Carries out a command of the shared tree behaviour (`SPEC-0275` behaviours 8–11).
    fn apply(&mut self, command: crate::tree_nav::TreeCommand, cx: &mut Context<Self>) {
        use crate::tree_nav::TreeCommand as C;
        let rows = self.rows();
        let node = |i: usize| rows.get(i).and_then(|r| r.node.clone());
        match command {
            C::None => {}
            C::Select(i) => {
                if Some(i) != self.selected {
                    self.removing = None;
                }
                self.selected = Some(i.min(rows.len().saturating_sub(1)));
                self.keep_selection = None;
            }
            C::Expand(i) => {
                if let Some(n) = node(i) {
                    self.expand(n, cx);
                }
            }
            // Everything below the row that is read already; what a node reads when it opens
            // comes with the next use (SPEC-0275 v13 behaviour 32).
            C::ExpandFully(i) => {
                crate::tree_nav::fully_expand(
                    self,
                    i,
                    |t| Self::nav_rows(&t.rows()),
                    |t, row| {
                        if let Some(n) = t.rows().get(row).and_then(|r| r.node.clone()) {
                            t.expand(n, cx);
                        }
                    },
                );
            }
            // A selected row inside the collapsed node gives the selection to its nearest
            // shown parent (v14 behaviour 26).
            C::Collapse(i) => {
                if let Some(n) = node(i) {
                    self.keeping_selection(false, |this| this.expanded.remove(&n));
                }
            }
            C::Open(i) => {
                self.selected = Some(i);
                self.keep_selection = None;
                self.properties(cx);
            }
        }
        cx.notify();
    }

    /// What the selected row is, for the context menu.
    pub fn target(&self) -> crate::context_menu::DbTarget {
        use crate::context_menu::DbTarget as T;
        match self.selected_node() {
            None | Some(Node::Retry(_) | Node::SchemaRetry(..) | Node::Store(..)) => T::Nothing,
            Some(Node::Conn(_)) => T::Connection,
            Some(
                Node::Group(..)
                | Node::Sequences(_)
                | Node::Objects(..)
                | Node::Section(..)
                | Node::Schema(..),
            ) => T::Folder,
            Some(Node::Object(..)) => T::Definition,
            Some(Node::Table(..) | Node::SchemaTable(..)) => T::Table,
            Some(Node::Column(..)) => T::Column,
            Some(Node::Index(..) | Node::Key(..) | Node::Sequence(..)) => T::Object,
        }
    }

    /// The name of the selected row (Copy).
    pub fn selected_label(&self) -> Option<String> {
        self.rows()
            .get(self.selected?)
            .filter(|r| r.node.is_some())
            .map(|r| r.label.clone())
    }

    /// Whether the row labelled `label` is expanded (tests).
    pub fn is_expanded(&self, label: &str) -> bool {
        self.rows().iter().any(|r| r.label == label && r.expanded)
    }

    /// The label of the selected row (tests).
    pub fn selected_row_label(&self) -> Option<String> {
        self.rows().get(self.selected?).map(|r| r.label.clone())
    }

    /// F4: a table's data (`SPEC-0190`), else the connection's properties.
    pub fn properties(&mut self, cx: &mut Context<Self>) {
        if let (Some(p), Some(t)) = (self.selected_profile(), self.selected_table()) {
            cx.emit(DatabaseEvent::OpenTable(p, t));
        } else if let Some(p) = self.selected_profile() {
            cx.emit(DatabaseEvent::EditConnection(p));
        }
    }

    /// Show DDL on the selected table or view of a connection's current schema (v5 behaviour 11).
    pub fn show_ddl(&mut self, cx: &mut Context<Self>) {
        if let (Some(p), Some(t)) = (self.selected_profile(), self.selected_table()) {
            cx.emit(DatabaseEvent::ShowDdl(p, t));
        } else if let (Some(p), Some(Node::Object(_, object))) =
            (self.selected_profile(), self.selected_node())
        {
            // SPEC-0089 v13 behaviour 25: a routine's, trigger's or type's definition.
            cx.emit(DatabaseEvent::ShowObjectDdl(p, object));
        }
    }

    /// The speed search's text while it is open (v5 behaviour 10).
    pub fn search_text(&self) -> Option<&str> {
        self.search.as_deref()
    }

    /// Opens, changes or closes (`None`) the speed search; with text the first visible match at or
    /// after the selection is selected (v5 behaviour 10).
    fn set_search(&mut self, query: Option<String>, cx: &mut Context<Self>) {
        if let Some(q) = &query {
            let rows = self.rows();
            let labels = crate::db_tree_model::search_labels(&rows);
            let from = self.selected.unwrap_or(0);
            if let Some(i) = crate::speed_search::first_match(&labels, q, from) {
                self.selected = Some(i);
                self.keep_selection = None;
                self.removing = None;
            }
        }
        self.search = query;
        cx.notify();
    }

    /// Keys while the speed search is open: Escape closes, Backspace edits, ↑/↓ go to the
    /// previous or next match, letters extend it; other keys close it and act as usual. A typed
    /// letter opens it. Whether the key was used.
    fn search_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let k = &event.keystroke;
        let plain = !k.modifiers.control && !k.modifiers.alt && !k.modifiers.platform;
        let current = self.search.clone().unwrap_or_default();
        let typed = k
            .key_char
            .clone()
            .filter(|_| plain)
            .filter(|_| !matches!(k.key.as_str(), "enter" | "tab" | "escape" | "backspace"))
            .filter(|ch| crate::speed_search::types_into(&current, ch));
        let Some(query) = self.search.clone() else {
            return false;
        };
        match k.key.as_str() {
            "escape" if plain => self.set_search(None, cx),
            "backspace" if plain => {
                let mut q = query;
                q.pop();
                self.set_search((!q.is_empty()).then_some(q), cx);
            }
            "up" | "down" if plain && !k.modifiers.shift => {
                let rows = self.rows();
                let labels = crate::db_tree_model::search_labels(&rows);
                let forward = k.key == "down";
                let from = self.selected.unwrap_or(0);
                if let Some(i) = crate::speed_search::step(&labels, &query, from, forward) {
                    self.selected = Some(i);
                    self.keep_selection = None;
                }
                cx.notify();
            }
            // The text itself arrives through the platform's text input (`take_committed`).
            _ if typed.is_some() => return false,
            _ => {
                self.search = None;
                cx.notify();
                return false;
            }
        }
        true
    }

    /// Whether a key types text the speed search takes from the platform's text input
    /// (`SPEC-0004` v7 behaviour 17a): printable, without ⌘/⌃/⌥, none of the search's own keys.
    fn types_text(event: &KeyDownEvent) -> bool {
        let k = &event.keystroke;
        let m = &k.modifiers;
        !matches!(k.key.as_str(), "enter" | "tab" | "escape" | "backspace")
            && crate::committed_text::leave_to_text_input(
                k.key_char.as_deref(),
                m.control || m.alt || m.platform,
            )
    }

    /// Takes text the platform's text input committed: it opens the speed search or extends it
    /// (`SPEC-0004` v7 behaviour 17a, SPEC-0089 v5 behaviour 10).
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        let current = self.search.clone().unwrap_or_default();
        if crate::speed_search::types_into(&current, committed) {
            self.set_search(Some(format!("{current}{committed}")), cx);
        }
    }

    /// The typed text of the speed search, if open.
    pub fn speed_search(&self) -> Option<&str> {
        self.search.as_deref()
    }

    /// ⇧F6 on a table or column.
    pub fn rename_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected_object() {
            Some(target) => cx.emit(DatabaseEvent::Rename(target)),
            None => cx.emit(DatabaseEvent::Note(
                "Rename works on a table, view, column, index, sequence or named foreign key of a loaded connection".into(),
            )),
        }
    }

    /// Reads the selected connection again, or every connection when none is selected.
    pub fn refresh_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected_profile() {
            Some(p) => {
                let id = p.id.clone();
                self.refresh_connection(&id, cx);
                cx.emit(DatabaseEvent::Refreshed(Some(id)));
            }
            // reload_keeping tells the SQL editors (v11 behaviour 23).
            None => self.reload_keeping(cx),
        }
        cx.notify();
    }

    /// Selects visible row `row`.
    pub fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selected = Some(row.min(self.rows().len().saturating_sub(1)));
        self.keep_selection = None;
        self.removing = None;
        cx.notify();
    }

    /// Whether Delete was pressed once on the selected connection.
    pub fn removing(&self) -> Option<&str> {
        self.removing.as_deref()
    }

    /// Delete on a connection asks first; a second Delete removes it (`SPEC-0108` behaviour 5).
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_profile().map(|p| p.id) else {
            return;
        };
        if self.removing.as_deref() == Some(id.as_str()) {
            self.removing = None;
            cx.emit(DatabaseEvent::RemoveConnection(id));
        } else {
            self.removing = Some(id);
        }
        cx.notify();
    }

    /// ↑/↓ select, →/Enter expand, ← collapse, Escape hides (behaviour 2); ⌘N (Ctrl+N) adds a
    /// connection, F4 edits the selected one (or opens a selected table's data, `SPEC-0190`), F5
    /// copies a table (`SPEC-0191`), Delete removes a connection (`SPEC-0108` behaviour 1).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let cmd = if cfg!(target_os = "macos") {
            k.modifiers.platform
        } else {
            k.modifiers.control
        };
        // Speed search (SPEC-0089 v5 behaviour 10).
        if self.search_key(event, cx) {
            cx.stop_propagation();
            return;
        }
        // Typed text goes to the text input, which hands it to the speed search.
        if Self::types_text(event) {
            return;
        }
        if cmd && k.key == "n" {
            cx.emit(DatabaseEvent::NewConnection);
            cx.stop_propagation();
            return;
        }
        // ⌥⇧⌘U (Ctrl+Alt+Shift+U) draws the selection as an ER diagram (SPEC-0111 behaviour 1).
        if cmd && k.modifiers.alt && k.modifiers.shift && k.key.eq_ignore_ascii_case("u") {
            cx.emit(DatabaseEvent::ShowDiagram);
            cx.stop_propagation();
            return;
        }
        // ⇧F6 renames the selected table or column (SPEC-0128 behaviour 1).
        if k.modifiers.shift && !cmd && !k.modifiers.alt && k.key == "f6" {
            self.rename_selected(cx);
            cx.stop_propagation();
            return;
        }
        // ⌘F6 (Ctrl+F6) changes the selected column's type (SPEC-0164 v3 behaviour 10).
        if cmd && !k.modifiers.shift && !k.modifiers.alt && k.key == "f6" {
            self.change_type_selected(cx);
            cx.stop_propagation();
            return;
        }
        // F1 or ⌃J shows the quick documentation of the selection (SPEC-0089 v6 behaviour 13).
        let plain_f1 = k.key == "f1"
            && !k.modifiers.control
            && !k.modifiers.alt
            && !k.modifiers.shift
            && !k.modifiers.platform;
        let ctrl_j = k.key == "j"
            && k.modifiers.control
            && !k.modifiers.alt
            && !k.modifiers.shift
            && !k.modifiers.platform;
        if plain_f1 || ctrl_j {
            self.toggle_doc(cx);
            cx.stop_propagation();
            return;
        }
        if k.key == "escape" && self.doc.is_some() {
            self.doc = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        // ⌘↓ (Ctrl+↓) opens like F4 (SPEC-0275 behaviour 10).
        if cmd && !k.modifiers.alt && !k.modifiers.shift && k.key == "down" {
            self.properties(cx);
            cx.stop_propagation();
            return;
        }
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift {
            return;
        }
        if k.key != "delete" && k.key != "backspace" {
            self.removing = None;
        }
        match k.key.as_str() {
            // F4 opens a table's data (SPEC-0190 behaviour 2), else edits the connection.
            "f4" => {
                self.properties(cx);
                cx.stop_propagation();
                return;
            }
            // F5 copies the selected table (SPEC-0191 behaviour 1).
            "f5" => {
                if let Some(target) = self.selected_object().filter(|t| t.column.is_none()) {
                    cx.emit(DatabaseEvent::CopyTable(target));
                }
                cx.stop_propagation();
                return;
            }
            "delete" | "backspace" => {
                self.remove_selected(cx);
                cx.stop_propagation();
                return;
            }
            _ => {}
        }
        if k.key == "escape" {
            cx.emit(DatabaseEvent::Hide);
            cx.stop_propagation();
            return;
        }
        // The shared tree keys (SPEC-0275 behaviour 10).
        // Expanding and collapsing are the keymap's actions (SPEC-0275 v13 behaviour 32).
        let Some(key) = crate::keymaps::tree_action_key(k)
            .or_else(|| crate::tree_nav::tree_key(&k.key, false, false))
        else {
            return;
        };
        let rows = self.rows();
        // Without a selected row the key starts at the first row (SPEC-0192 v7 behaviour 3b).
        for command in crate::tree_nav::on_key_from(&Self::nav_rows(&rows), self.selected, key) {
            self.apply(command, cx);
        }
        cx.stop_propagation();
    }
}

impl DatabaseView {
    /// New, Edit and Refresh before Options in the shared header (SPEC-0061 v5 behaviour 18).
    pub fn header_toolbar(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        vec![
            crate::metrics::header_button("database-new")
                .tip(declared!(DATABASE_TOOLBAR, "database-new"))
                .child("+")
                .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                    cx.emit(DatabaseEvent::NewConnection)
                }))
                .into_any_element(),
            crate::metrics::header_button("database-edit")
                .tip(declared!(DATABASE_TOOLBAR, "database-edit"))
                .child("✎")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    if let Some(p) = this.selected_profile() {
                        cx.emit(DatabaseEvent::EditConnection(p));
                    }
                }))
                .into_any_element(),
            crate::metrics::header_button("database-refresh")
                .tip(declared!(DATABASE_TOOLBAR, "database-refresh"))
                .child(icon("general/refresh"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.refresh_from_toolbar(cx);
                }))
                .into_any_element(),
            // SPEC-0089 v5 behaviour 11.
            crate::metrics::header_button("database-ddl")
                .tip(declared!(DATABASE_TOOLBAR, "database-ddl"))
                .text_xs()
                .child("DDL")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.show_ddl(cx)))
                .into_any_element(),
            // SPEC-0089 v6 behaviour 14: console, table data, documentation, filter, collapse.
            crate::metrics::header_button("database-console")
                .tip(declared!(DATABASE_TOOLBAR, "database-console"))
                .text_xs()
                .child("SQL")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    if let Some(p) = this.selected_profile() {
                        cx.emit(DatabaseEvent::NewConsole(p));
                    }
                }))
                .into_any_element(),
            crate::metrics::header_button("database-table-data")
                .tip(declared!(DATABASE_TOOLBAR, "database-table-data"))
                .child(icon("nodes/dataTables"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    if let (Some(p), Some(t)) = (this.selected_profile(), this.selected_table()) {
                        cx.emit(DatabaseEvent::OpenTable(p, t));
                    }
                }))
                .into_any_element(),
            crate::metrics::header_button("database-doc")
                .tip(declared!(DATABASE_TOOLBAR, "database-doc"))
                .text_xs()
                .child("?")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_doc(cx)))
                .into_any_element(),
            crate::metrics::header_button("database-filter")
                .tip(declared!(DATABASE_TOOLBAR, "database-filter"))
                .when(self.filter_schemas, |d| {
                    d.text_color(c(Palette::current().link))
                })
                .text_xs()
                .child("▽")
                .on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_schema_filter(cx)),
                )
                .into_any_element(),
            crate::metrics::header_button("database-collapse")
                .tip(declared!(DATABASE_TOOLBAR, "database-collapse"))
                .child(icon("general/collapseAll"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.collapse_all(cx)))
                .into_any_element(),
        ]
    }
}

crate::committed_text::committed_text_input!(DatabaseView, take_committed);

impl Render for DatabaseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        // The quick documentation follows the selection while it is open (v6 behaviour 13).
        if self.doc.is_some() {
            self.doc = Some(self.quick_doc().unwrap_or_else(|| {
                vec!["No documentation: select a connection, table, view or column".into()]
            }));
        }
        let doc = self.doc.clone();
        let colors: Vec<Option<u32>> = self
            .conns
            .iter()
            .map(|c| c.profile.color.map(|c| c.rgb()))
            .collect();
        let rows = self.rows();
        let nav = std::rc::Rc::new(Self::nav_rows(&rows));
        let selected = self.selected;
        // Speed search (v5 behaviour 10): the typed text, in the error colour without a match.
        let search = self.search.clone();
        let found = search.as_ref().is_some_and(|q| {
            crate::speed_search::first_match(&crate::db_tree_model::search_labels(&rows), q, 0)
                .is_some()
        });
        let hit_bg = gpui::rgb(
            p.scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
                .unwrap_or(p.selection),
        );
        let empty = if let Some(id) = &self.removing {
            Some(format!("Press Delete again to remove connection `{id}`"))
        } else if let Some(problem) = &self.problem {
            Some(problem.clone())
        } else if self.conns.is_empty() && self.stores.is_empty() {
            Some(format!("No connections yet: add them to {PROFILE_FILE}"))
        } else {
            None
        };
        div()
            .id("database-panel")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v7 behaviour 17a).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            // Title and toolbar are in the shared header (SPEC-0061 v5 behaviour 18).
            .children(search.clone().map(|text| {
                div()
                    .id("database-speed-search")
                    .tip(declared!(DATABASE_TOOLBAR, "database-speed-search"))
                    .mx_2()
                    .my_1()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(p.focus))
                    .child(icon("general/search"))
                    .child(
                        div()
                            .when(!found, |d| d.text_color(c(p.error_text)))
                            .child(text),
                    )
            }))
            .children(empty.map(|text| {
                div()
                    .px_3()
                    .py_2()
                    .text_color(c(p.ui_muted))
                    .whitespace_normal()
                    .child(text)
            }))
            // SPEC-0089 v6 behaviour 13: quick documentation of the selection.
            .children(doc.map(|lines| {
                div()
                    .id("database-doc-panel")
                    .mx_2()
                    .my_1()
                    .p_2()
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(p.popup_border))
                    .bg(c(p.popup))
                    .text_xs()
                    .flex()
                    .flex_col()
                    .children(lines.into_iter().enumerate().map(|(i, l)| {
                        div()
                            .when(i == 0, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                            .whitespace_normal()
                            .child(l)
                    }))
            }))
            .child(
                crate::mouse::on_secondary_click(
                    div().id("database-tree"),
                    cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| {
                        window.focus(&this.focus);
                        cx.emit(DatabaseEvent::ContextMenu(e.position));
                    }),
                )
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_1()
                .children(rows.into_iter().enumerate().map(|(i, row)| {
                    let (nav, arrow_nav) = (nav.clone(), nav.clone());
                    let chevron = if !row.has_children {
                        None
                    } else if row.expanded {
                        Some("general/chevronDown")
                    } else {
                        Some("general/chevronRight")
                    };
                    let node = row.node.clone();
                    let on = Some(i) == selected;
                    // The matched part of the name on the search-hit background (v5 behaviour 10).
                    let mark = search
                        .as_deref()
                        .filter(|_| !row.message)
                        .and_then(|q| crate::speed_search::match_range(&row.label, q));
                    let name =
                        gpui::StyledText::new(row.label.clone()).with_highlights(mark.map(|r| {
                            (
                                r,
                                gpui::HighlightStyle {
                                    color: Some(c(p.link)),
                                    background_color: Some(hit_bg.into()),
                                    ..Default::default()
                                },
                            )
                        }));
                    div()
                        .id(("database-row", i))
                        .when(!row.message, |d| d.h(px(24.)).whitespace_nowrap())
                        .when(row.message, |d| d.min_h(px(24.)).py_1().whitespace_normal())
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl(px(crate::metrics::tree_indent(row.depth)))
                        .pr_2()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .when(on, |d| d.bg(c(p.tree_selection)))
                        .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .relative()
                        .children(crate::hit_targets::mark(format!("db-row-{}", row.label)))
                        .child(match chevron {
                            // One click on the arrow toggles (SPEC-0275 behaviour 8).
                            Some(key) => div()
                                .id(("database-arrow", i))
                                .flex_none()
                                .relative()
                                .children(crate::hit_targets::mark(format!(
                                    "db-arrow-{}",
                                    row.label
                                )))
                                .child(icon(key))
                                .on_click(cx.listener(
                                    move |this, _: &gpui::ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        window.focus(&this.focus);
                                        for c in crate::tree_nav::on_click(&arrow_nav, i, true, 1) {
                                            this.apply(c, cx);
                                        }
                                    },
                                ))
                                .into_any_element(),
                            None => div().w(px(16.)).flex_none().into_any_element(),
                        })
                        .when(!row.icon.is_empty(), |d| {
                            d.child(div().flex_none().child(icon(row.icon)))
                        })
                        // SPEC-0108 v4 behaviour 9: the connection's colour.
                        .when_some(
                            match &row.node {
                                Some(Node::Conn(ci)) => colors.get(*ci).copied().flatten(),
                                _ => None,
                            },
                            |d, rgb| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .size(px(8.))
                                        .rounded(px(4.))
                                        .bg(gpui::rgb(rgb)),
                                )
                            },
                        )
                        // Names keep their room; long names and details end in "…"
                        // (SPEC-0089 v2).
                        .child(
                            div()
                                .flex_shrink()
                                .min_w_0()
                                .when(!row.message, |d| d.text_ellipsis())
                                .when(row.message, |d| d.text_color(c(p.ui_muted)))
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .text_color(c(if on { p.ui_muted } else { p.ui_dim }))
                                .child(row.detail.clone()),
                        )
                        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                            window.focus(&this.focus);
                            let clicks = e.click_count().min(2);
                            for c in crate::tree_nav::on_click(&nav, i, false, clicks) {
                                // A double click on a table opens its data (SPEC-0190).
                                let c = match (c, &node) {
                                    (
                                        crate::tree_nav::TreeCommand::Expand(_)
                                        | crate::tree_nav::TreeCommand::Collapse(_),
                                        Some(Node::Table(..)),
                                    ) => crate::tree_nav::TreeCommand::Open(i),
                                    (c, _) => c,
                                };
                                this.apply(c, cx);
                            }
                        }))
                        .map(|d| {
                            crate::mouse::on_secondary_click(
                                d,
                                cx.listener(move |this, e: &gpui::MouseDownEvent, window, cx| {
                                    window.focus(&this.focus);
                                    this.apply(crate::tree_nav::TreeCommand::Select(i), cx);
                                    cx.emit(DatabaseEvent::ContextMenu(e.position));
                                }),
                            )
                        })
                })),
            )
    }
}

//! The MongoDB console language (`SPEC-0311` behaviours 1–4, 9): a defined subset of shell forms
//! with relaxed JSON / Extended JSON arguments. Not a script engine.

use mongodb::bson::{self, Bson, Document, oid::ObjectId, spec::BinarySubtype};

use crate::Class;

/// Forms the console accepts, for refusals.
pub const ACCEPTED: &str = "accepted: show dbs, show collections, use <db>, \
db.<coll>.find/findOne/countDocuments/estimatedDocumentCount/distinct/aggregate/insertOne/\
insertMany/updateOne/updateMany/replaceOne/deleteOne/deleteMany/createIndex/dropIndex/drop(…), \
db.getCollection(\"<name>\").<method>(…), db.createCollection(\"<name>\"), db.runCommand({…})";

/// A parse error with its character offset into the console text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("at {offset}: {message}")]
pub struct ShellError {
    /// Character offset.
    pub offset: usize,
    /// What is wrong.
    pub message: String,
}

/// Update body: a document of operators or a pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateBody {
    /// `{ $set: … }`
    Operators(Document),
    /// `[ { $set: … } ]`
    Pipeline(Vec<Document>),
}

/// One parsed console command.
#[derive(Debug, Clone, PartialEq)]
pub enum MongoCommand {
    /// `show dbs`
    ShowDatabases,
    /// `show collections`
    ShowCollections,
    /// `use <db>`
    Use(String),
    /// `find` / `findOne` with cursor modifiers.
    Find {
        /// Collection.
        collection: String,
        /// Filter.
        filter: Document,
        /// Projection.
        projection: Option<Document>,
        /// `.sort()`
        sort: Option<Document>,
        /// `.limit()`
        limit: Option<i64>,
        /// `.skip()`
        skip: Option<u64>,
        /// `findOne`
        one: bool,
    },
    /// `countDocuments` / `estimatedDocumentCount`
    Count {
        /// Collection.
        collection: String,
        /// Filter (empty for the estimate).
        filter: Document,
        /// `estimatedDocumentCount`
        estimated: bool,
    },
    /// `distinct`
    Distinct {
        /// Collection.
        collection: String,
        /// Field.
        field: String,
        /// Filter.
        filter: Document,
    },
    /// `aggregate`
    Aggregate {
        /// Collection.
        collection: String,
        /// Stages.
        pipeline: Vec<Document>,
    },
    /// `insertOne` / `insertMany`
    Insert {
        /// Collection.
        collection: String,
        /// Documents.
        documents: Vec<Document>,
        /// `insertMany`
        many: bool,
    },
    /// `updateOne` / `updateMany`
    Update {
        /// Collection.
        collection: String,
        /// Filter.
        filter: Document,
        /// Update.
        update: UpdateBody,
        /// `updateMany`
        many: bool,
    },
    /// `replaceOne`
    Replace {
        /// Collection.
        collection: String,
        /// Filter.
        filter: Document,
        /// New document.
        document: Document,
    },
    /// `deleteOne` / `deleteMany`
    Delete {
        /// Collection.
        collection: String,
        /// Filter.
        filter: Document,
        /// `deleteMany`
        many: bool,
    },
    /// `createIndex`
    CreateIndex {
        /// Collection.
        collection: String,
        /// Keys.
        keys: Document,
        /// Options (`unique`, `name`, `expireAfterSeconds`).
        options: Option<Document>,
    },
    /// `dropIndex`
    DropIndex {
        /// Collection.
        collection: String,
        /// Index name.
        name: String,
    },
    /// `drop`
    Drop {
        /// Collection.
        collection: String,
    },
    /// `db.createCollection`
    CreateCollection(String),
    /// `db.runCommand`
    RunCommand(Document),
}

/// `runCommand` names that only read (behaviour 9).
const READ_COMMANDS: &[&str] = &[
    "ping",
    "buildinfo",
    "serverstatus",
    "listcollections",
    "listindexes",
    "dbstats",
    "collstats",
    "count",
    "distinct",
    "find",
    "hello",
    "ismaster",
];

impl MongoCommand {
    /// Class for rights and read-only profiles (behaviour 9).
    pub fn class(&self) -> Class {
        match self {
            MongoCommand::ShowDatabases | MongoCommand::ShowCollections => Class::ReadSchema,
            MongoCommand::Use(_)
            | MongoCommand::Find { .. }
            | MongoCommand::Count { .. }
            | MongoCommand::Distinct { .. } => Class::ReadData,
            MongoCommand::Aggregate { pipeline, .. } => {
                if pipeline
                    .iter()
                    .any(|s| s.contains_key("$out") || s.contains_key("$merge"))
                {
                    Class::ModifyData
                } else {
                    Class::ReadData
                }
            }
            MongoCommand::Insert { .. }
            | MongoCommand::Update { .. }
            | MongoCommand::Replace { .. }
            | MongoCommand::Delete { .. } => Class::ModifyData,
            MongoCommand::CreateIndex { .. }
            | MongoCommand::DropIndex { .. }
            | MongoCommand::Drop { .. }
            | MongoCommand::CreateCollection(_) => Class::ModifySchema,
            MongoCommand::RunCommand(doc) => match doc.keys().next() {
                Some(name) if READ_COMMANDS.contains(&name.to_ascii_lowercase().as_str()) => {
                    // `find`/`count`/`distinct` as commands may not write anything.
                    Class::ReadData
                }
                _ => Class::ModifySchema,
            },
        }
    }

    /// Collection the command works on, if any.
    pub fn collection(&self) -> Option<&str> {
        match self {
            MongoCommand::Find { collection, .. }
            | MongoCommand::Count { collection, .. }
            | MongoCommand::Distinct { collection, .. }
            | MongoCommand::Aggregate { collection, .. }
            | MongoCommand::Insert { collection, .. }
            | MongoCommand::Update { collection, .. }
            | MongoCommand::Replace { collection, .. }
            | MongoCommand::Delete { collection, .. }
            | MongoCommand::CreateIndex { collection, .. }
            | MongoCommand::DropIndex { collection, .. }
            | MongoCommand::Drop { collection } => Some(collection),
            MongoCommand::CreateCollection(c) => Some(c),
            _ => None,
        }
    }

    /// A `deleteMany`/`updateMany` with an empty filter needs confirmation (behaviour 9).
    pub fn needs_confirmation(&self) -> bool {
        matches!(
            self,
            MongoCommand::Delete { filter, many: true, .. }
            | MongoCommand::Update { filter, many: true, .. } if filter.is_empty()
        ) || matches!(self, MongoCommand::Drop { .. })
    }

    /// Short name for messages (`db.items.deleteMany`).
    pub fn describe(&self) -> String {
        let method = match self {
            MongoCommand::ShowDatabases => return "show dbs".into(),
            MongoCommand::ShowCollections => return "show collections".into(),
            MongoCommand::Use(db) => return format!("use {db}"),
            MongoCommand::CreateCollection(_) => return "db.createCollection".into(),
            MongoCommand::RunCommand(d) => {
                return format!(
                    "db.runCommand({})",
                    d.keys().next().map(String::as_str).unwrap_or("")
                );
            }
            MongoCommand::Find { one: true, .. } => "findOne",
            MongoCommand::Find { .. } => "find",
            MongoCommand::Count {
                estimated: true, ..
            } => "estimatedDocumentCount",
            MongoCommand::Count { .. } => "countDocuments",
            MongoCommand::Distinct { .. } => "distinct",
            MongoCommand::Aggregate { .. } => "aggregate",
            MongoCommand::Insert { many: false, .. } => "insertOne",
            MongoCommand::Insert { .. } => "insertMany",
            MongoCommand::Update { many: false, .. } => "updateOne",
            MongoCommand::Update { .. } => "updateMany",
            MongoCommand::Replace { .. } => "replaceOne",
            MongoCommand::Delete { many: false, .. } => "deleteOne",
            MongoCommand::Delete { .. } => "deleteMany",
            MongoCommand::CreateIndex { .. } => "createIndex",
            MongoCommand::DropIndex { .. } => "dropIndex",
            MongoCommand::Drop { .. } => "drop",
        };
        format!("db.{}.{method}", self.collection().unwrap_or(""))
    }
}

/// Parses one console command.
pub fn parse_command(text: &str) -> Result<MongoCommand, ShellError> {
    let mut p = Parser::new(text);
    p.skip_ws();
    let start = p.pos;
    let word = p.ident();
    let cmd = match word.as_str() {
        "show" => {
            p.skip_ws();
            let what_at = p.pos;
            match p.ident().as_str() {
                "dbs" | "databases" => MongoCommand::ShowDatabases,
                "collections" | "tables" => MongoCommand::ShowCollections,
                _ => return Err(p.err_at(what_at, "expected `dbs` or `collections` after `show`")),
            }
        }
        "use" => {
            p.skip_ws();
            let at = p.pos;
            let mut name = String::new();
            while let Some(c) = p.peek() {
                if c.is_whitespace() || c == ';' {
                    break;
                }
                name.push(c);
                p.bump();
            }
            if name.is_empty() {
                return Err(p.err_at(at, "expected a database name after `use`"));
            }
            MongoCommand::Use(name)
        }
        "db" => p.db_command()?,
        "" => return Err(p.err_at(start, &format!("expected a command; {ACCEPTED}"))),
        other => {
            return Err(p.err_at(
                start,
                &format!("`{other}` is not supported (no JavaScript); {ACCEPTED}"),
            ));
        }
    };
    p.skip_ws();
    if p.peek() == Some(';') {
        p.bump();
        p.skip_ws();
    }
    if p.peek().is_some() {
        return Err(p.err_here("unexpected text after the command (one command per run)"));
    }
    Ok(cmd)
}

/// Parses one relaxed JSON value (for edits typed as JSON).
pub fn parse_value(text: &str) -> Result<Bson, ShellError> {
    let mut p = Parser::new(text);
    let v = p.value()?;
    p.skip_ws();
    if p.peek().is_some() {
        return Err(p.err_here("unexpected text after the value"));
    }
    Ok(v)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        self.pos += 1;
        c
    }

    fn err_at(&self, offset: usize, message: &str) -> ShellError {
        ShellError {
            offset,
            message: message.to_owned(),
        }
    }

    fn err_here(&self, message: &str) -> ShellError {
        self.err_at(self.pos, message)
    }

    fn skip_ws(&mut self) {
        loop {
            match (self.peek(), self.peek_at(1)) {
                (Some(c), _) if c.is_whitespace() => {
                    self.pos += 1;
                }
                (Some('/'), Some('/')) => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                (Some('/'), Some('*')) => {
                    self.pos += 2;
                    while self.peek().is_some()
                        && !(self.peek() == Some('*') && self.peek_at(1) == Some('/'))
                    {
                        self.pos += 1;
                    }
                    self.pos += 2;
                }
                _ => break,
            }
        }
    }

    fn expect(&mut self, c: char) -> Result<(), ShellError> {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err_here(&format!("expected `{c}`")))
        }
    }

    fn ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        s
    }

    /// After `db`: `.coll[.more].method(args)[.sort()…]` or a db-level method.
    fn db_command(&mut self) -> Result<MongoCommand, ShellError> {
        let mut parts: Vec<(usize, String)> = Vec::new();
        let mut collection: Option<String> = None;
        loop {
            self.skip_ws();
            if self.peek() != Some('.') {
                break;
            }
            self.bump();
            self.skip_ws();
            let at = self.pos;
            let name = self.ident();
            if name.is_empty() {
                return Err(self.err_here("expected a name after `.`"));
            }
            self.skip_ws();
            if self.peek() == Some('(') {
                // A method call.
                if collection.is_none() && parts.is_empty() {
                    match name.as_str() {
                        "getCollection" => {
                            self.bump();
                            let arg_at = self.pos;
                            let n = self.value()?;
                            self.expect(')')?;
                            let Bson::String(n) = n else {
                                return Err(
                                    self.err_at(arg_at, "getCollection takes a name in quotes")
                                );
                            };
                            collection = Some(n);
                            continue;
                        }
                        "runCommand" | "createCollection" => {
                            let args = self.args()?;
                            return self.db_method(&name, at, args);
                        }
                        _ => {
                            return Err(self.err_at(
                                at,
                                &format!("`db.{name}()` is not supported; {ACCEPTED}"),
                            ));
                        }
                    }
                }
                if !crate::completion::MONGO_METHODS.contains(&name.as_str()) {
                    return Err(self.err_at(at, &format!("`{name}` is not supported; {ACCEPTED}")));
                }
                let coll = match collection.take() {
                    Some(c) => c,
                    None => parts
                        .iter()
                        .map(|(_, s)| s.as_str())
                        .collect::<Vec<_>>()
                        .join("."),
                };
                if coll.is_empty() {
                    return Err(self.err_at(at, "missing collection"));
                }
                let args = self.args()?;
                let mut cmd = self.coll_method(coll, &name, at, args)?;
                self.cursor_modifiers(&mut cmd)?;
                return Ok(cmd);
            }
            if collection.is_some() {
                return Err(self.err_at(at, "expected a method call after getCollection(…)"));
            }
            parts.push((at, name));
        }
        Err(self.err_here(&format!("expected a method call; {ACCEPTED}")))
    }

    fn args(&mut self) -> Result<Vec<(usize, Bson)>, ShellError> {
        self.expect('(')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(')') {
                self.bump();
                return Ok(out);
            }
            let at = self.pos;
            out.push((at, self.value()?));
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(')') => {}
                _ => return Err(self.err_here("expected `,` or `)`")),
            }
        }
    }

    fn db_method(
        &self,
        name: &str,
        at: usize,
        args: Vec<(usize, Bson)>,
    ) -> Result<MongoCommand, ShellError> {
        let mut args = args.into_iter();
        match name {
            "runCommand" => match args.next() {
                Some((_, Bson::Document(d))) if !d.is_empty() => Ok(MongoCommand::RunCommand(d)),
                Some((a, _)) => Err(self.err_at(a, "runCommand takes a command document")),
                None => Err(self.err_at(at, "runCommand takes a command document")),
            },
            _ => match args.next() {
                Some((_, Bson::String(s))) => Ok(MongoCommand::CreateCollection(s)),
                Some((a, _)) => Err(self.err_at(a, "createCollection takes a name in quotes")),
                None => Err(self.err_at(at, "createCollection takes a name in quotes")),
            },
        }
    }

    fn coll_method(
        &self,
        collection: String,
        name: &str,
        at: usize,
        args: Vec<(usize, Bson)>,
    ) -> Result<MongoCommand, ShellError> {
        let max = |n: usize| -> Result<(), ShellError> {
            if args.len() > n {
                Err(self.err_at(args[n].0, &format!("{name} takes at most {n} argument(s)")))
            } else {
                Ok(())
            }
        };
        let doc = |i: usize, what: &str| -> Result<Option<Document>, ShellError> {
            match args.get(i) {
                None => Ok(None),
                Some((_, Bson::Document(d))) => Ok(Some(d.clone())),
                Some((a, _)) => Err(self.err_at(*a, &format!("{what} must be a document"))),
            }
        };
        let required = |i: usize, what: &str| -> Result<Document, ShellError> {
            doc(i, what)?.ok_or_else(|| self.err_at(at, &format!("{name} needs {what}")))
        };
        let docs = |i: usize, what: &str| -> Result<Vec<Document>, ShellError> {
            match args.get(i) {
                Some((a, Bson::Array(items))) => items
                    .iter()
                    .map(|b| match b {
                        Bson::Document(d) => Ok(d.clone()),
                        _ => Err(self.err_at(*a, &format!("{what} must hold documents"))),
                    })
                    .collect(),
                Some((a, _)) => Err(self.err_at(*a, &format!("{what} must be an array"))),
                None => Err(self.err_at(at, &format!("{name} needs {what}"))),
            }
        };
        let string = |i: usize, what: &str| -> Result<String, ShellError> {
            match args.get(i) {
                Some((_, Bson::String(s))) => Ok(s.clone()),
                Some((a, _)) => Err(self.err_at(*a, &format!("{what} must be a string"))),
                None => Err(self.err_at(at, &format!("{name} needs {what}"))),
            }
        };
        Ok(match name {
            "find" | "findOne" => {
                max(2)?;
                MongoCommand::Find {
                    collection,
                    filter: doc(0, "the filter")?.unwrap_or_default(),
                    projection: doc(1, "the projection")?,
                    sort: None,
                    limit: None,
                    skip: None,
                    one: name == "findOne",
                }
            }
            "countDocuments" => {
                max(1)?;
                MongoCommand::Count {
                    collection,
                    filter: doc(0, "the filter")?.unwrap_or_default(),
                    estimated: false,
                }
            }
            "estimatedDocumentCount" => {
                max(0)?;
                MongoCommand::Count {
                    collection,
                    filter: Document::new(),
                    estimated: true,
                }
            }
            "distinct" => {
                max(2)?;
                MongoCommand::Distinct {
                    collection,
                    field: string(0, "the field name")?,
                    filter: doc(1, "the filter")?.unwrap_or_default(),
                }
            }
            "aggregate" => {
                max(1)?;
                MongoCommand::Aggregate {
                    collection,
                    pipeline: docs(0, "the pipeline")?,
                }
            }
            "insertOne" => {
                max(1)?;
                MongoCommand::Insert {
                    collection,
                    documents: vec![required(0, "a document")?],
                    many: false,
                }
            }
            "insertMany" => {
                max(1)?;
                MongoCommand::Insert {
                    collection,
                    documents: docs(0, "an array of documents")?,
                    many: true,
                }
            }
            "updateOne" | "updateMany" => {
                max(2)?;
                let filter = required(0, "a filter")?;
                let update = match args.get(1) {
                    Some((_, Bson::Document(d))) => {
                        if !d.keys().all(|k| k.starts_with('$')) || d.is_empty() {
                            return Err(self.err_at(
                                args[1].0,
                                "an update needs operators such as $set; use replaceOne to replace a document",
                            ));
                        }
                        UpdateBody::Operators(d.clone())
                    }
                    Some(_) => UpdateBody::Pipeline(docs(1, "the update pipeline")?),
                    None => return Err(self.err_at(at, &format!("{name} needs an update"))),
                };
                MongoCommand::Update {
                    collection,
                    filter,
                    update,
                    many: name == "updateMany",
                }
            }
            "replaceOne" => {
                max(2)?;
                let filter = required(0, "a filter")?;
                let document = required(1, "a replacement document")?;
                if document.keys().any(|k| k.starts_with('$')) {
                    return Err(
                        self.err_at(args[1].0, "a replacement document cannot hold operators")
                    );
                }
                MongoCommand::Replace {
                    collection,
                    filter,
                    document,
                }
            }
            "deleteOne" | "deleteMany" => {
                max(1)?;
                MongoCommand::Delete {
                    collection,
                    filter: required(0, "a filter")?,
                    many: name == "deleteMany",
                }
            }
            "createIndex" => {
                max(2)?;
                MongoCommand::CreateIndex {
                    collection,
                    keys: required(0, "the index keys")?,
                    options: doc(1, "the options")?,
                }
            }
            "dropIndex" => {
                max(1)?;
                MongoCommand::DropIndex {
                    collection,
                    name: string(0, "the index name")?,
                }
            }
            "drop" => {
                max(0)?;
                MongoCommand::Drop { collection }
            }
            other => {
                return Err(self.err_at(at, &format!("`{other}` is not supported; {ACCEPTED}")));
            }
        })
    }

    fn cursor_modifiers(&mut self, cmd: &mut MongoCommand) -> Result<(), ShellError> {
        loop {
            self.skip_ws();
            if self.peek() != Some('.') {
                return Ok(());
            }
            self.bump();
            self.skip_ws();
            let at = self.pos;
            let name = self.ident();
            let MongoCommand::Find {
                sort,
                limit,
                skip,
                one: false,
                ..
            } = cmd
            else {
                return Err(self.err_at(at, &format!("`.{name}()` only follows find(…)")));
            };
            let args = self.args()?;
            let single = |what: &str| -> Result<Bson, ShellError> {
                match args.as_slice() {
                    [(_, b)] => Ok(b.clone()),
                    _ => Err(self.err_at(at, &format!("{name} takes {what}"))),
                }
            };
            match name.as_str() {
                "sort" => match single("one document")? {
                    Bson::Document(d) => *sort = Some(d),
                    _ => return Err(self.err_at(at, "sort takes one document")),
                },
                "limit" => {
                    *limit = Some(
                        integer(&single("a number")?)
                            .ok_or_else(|| self.err_at(at, "limit takes a number"))?,
                    )
                }
                "skip" => {
                    let n = integer(&single("a number")?)
                        .filter(|n| *n >= 0)
                        .ok_or_else(|| self.err_at(at, "skip takes a number of at least 0"))?;
                    *skip = Some(n as u64);
                }
                other => {
                    return Err(self.err_at(
                        at,
                        &format!(
                            "`.{other}()` is not supported after find; use sort, limit or skip"
                        ),
                    ));
                }
            }
        }
    }

    // ---- relaxed JSON -------------------------------------------------------------------------

    fn value(&mut self) -> Result<Bson, ShellError> {
        self.skip_ws();
        let at = self.pos;
        match self.peek() {
            None => Err(self.err_here("expected a value")),
            Some('{') => {
                self.bump();
                let mut doc = Document::new();
                loop {
                    self.skip_ws();
                    if self.peek() == Some('}') {
                        self.bump();
                        break;
                    }
                    let key = match self.peek() {
                        Some('"') | Some('\'') => self.string()?,
                        _ => {
                            let mut k = String::new();
                            while let Some(c) = self.peek() {
                                if c.is_alphanumeric() || matches!(c, '_' | '$' | '.') {
                                    k.push(c);
                                    self.bump();
                                } else {
                                    break;
                                }
                            }
                            if k.is_empty() {
                                return Err(self.err_here("expected a field name"));
                            }
                            k
                        }
                    };
                    self.expect(':')?;
                    let v = self.value()?;
                    doc.insert(key, v);
                    self.skip_ws();
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some('}') => {}
                        _ => return Err(self.err_here("expected `,` or `}`")),
                    }
                }
                extended(doc).map_err(|m| self.err_at(at, &m))
            }
            Some('[') => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.peek() == Some(']') {
                        self.bump();
                        break;
                    }
                    items.push(self.value()?);
                    self.skip_ws();
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some(']') => {}
                        _ => return Err(self.err_here("expected `,` or `]`")),
                    }
                }
                Ok(Bson::Array(items))
            }
            Some('"') | Some('\'') => Ok(Bson::String(self.string()?)),
            Some('/') => self.regex(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_alphabetic() => {
                let word = self.ident();
                match word.as_str() {
                    "true" => Ok(Bson::Boolean(true)),
                    "false" => Ok(Bson::Boolean(false)),
                    "null" | "undefined" => Ok(Bson::Null),
                    "new" => {
                        self.skip_ws();
                        let at = self.pos;
                        let ctor = self.ident();
                        self.constructor(&ctor, at)
                    }
                    _ => self.constructor(&word, at),
                }
            }
            Some(c) => Err(self.err_here(&format!("unexpected `{c}`"))),
        }
    }

    fn constructor(&mut self, name: &str, at: usize) -> Result<Bson, ShellError> {
        self.skip_ws();
        if self.peek() != Some('(') {
            return Err(self.err_at(
                at,
                &format!("`{name}` is not a value (variables are not supported)"),
            ));
        }
        let args = self.args()?;
        let text = || -> Option<String> {
            match args.first().map(|(_, b)| b) {
                Some(Bson::String(s)) => Some(s.clone()),
                Some(Bson::Int32(n)) => Some(n.to_string()),
                Some(Bson::Int64(n)) => Some(n.to_string()),
                Some(Bson::Double(n)) => Some(n.to_string()),
                _ => None,
            }
        };
        let bad = |what: &str| self.err_at(at, &format!("{name}(…) needs {what}"));
        match name {
            "ObjectId" => match text() {
                None if args.is_empty() => Ok(Bson::ObjectId(ObjectId::new())),
                Some(s) => ObjectId::parse_str(&s)
                    .map(Bson::ObjectId)
                    .map_err(|_| bad("24 hexadecimal digits")),
                None => Err(bad("24 hexadecimal digits")),
            },
            "ISODate" | "Date" => match text() {
                None if args.is_empty() => Ok(Bson::DateTime(bson::DateTime::now())),
                Some(s) => parse_date(&s).ok_or_else(|| bad("an ISO-8601 date")),
                None => Err(bad("an ISO-8601 date")),
            },
            "NumberLong" => text()
                .and_then(|s| s.parse::<i64>().ok())
                .map(Bson::Int64)
                .ok_or_else(|| bad("an integer")),
            "NumberInt" => text()
                .and_then(|s| s.parse::<i32>().ok())
                .map(Bson::Int32)
                .ok_or_else(|| bad("a 32-bit integer")),
            "NumberDecimal" => text()
                .and_then(|s| s.parse::<bson::Decimal128>().ok())
                .map(Bson::Decimal128)
                .ok_or_else(|| bad("a decimal number")),
            "UUID" => text()
                .and_then(|s| hex_bytes(&s.replace('-', "")))
                .filter(|b| b.len() == 16)
                .map(|bytes| {
                    Bson::Binary(bson::Binary {
                        subtype: BinarySubtype::Uuid,
                        bytes,
                    })
                })
                .ok_or_else(|| bad("32 hexadecimal digits")),
            other => Err(self.err_at(
                at,
                &format!("`{other}(…)` is not supported; values: ObjectId, ISODate, Date, NumberLong, NumberInt, NumberDecimal, UUID"),
            )),
        }
    }

    fn string(&mut self) -> Result<String, ShellError> {
        let at = self.pos;
        let quote = self.bump().unwrap_or('"');
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err_at(at, "unclosed string")),
                Some(c) if c == quote => return Ok(s),
                Some('\\') => match self.bump() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('b') => s.push('\u{8}'),
                    Some('f') => s.push('\u{c}'),
                    Some('u') => {
                        let hex: String = (0..4).filter_map(|_| self.bump()).collect();
                        let c = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| self.err_here("invalid \\u escape"))?;
                        s.push(c);
                    }
                    Some(c) => s.push(c),
                    None => return Err(self.err_at(at, "unclosed string")),
                },
                Some(c) => s.push(c),
            }
        }
    }

    fn regex(&mut self) -> Result<Bson, ShellError> {
        let at = self.pos;
        self.bump();
        let mut pattern = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(self.err_at(at, "unclosed regular expression")),
                Some('/') => break,
                Some('\\') => {
                    pattern.push('\\');
                    if let Some(c) = self.bump() {
                        pattern.push(c);
                    }
                }
                Some(c) => pattern.push(c),
            }
        }
        let mut options: Vec<char> = Vec::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                if !"imsxlu".contains(c) {
                    return Err(self.err_here(&format!("unknown regular expression flag `{c}`")));
                }
                options.push(c);
                self.bump();
            } else {
                break;
            }
        }
        options.sort_unstable();
        Ok(Bson::RegularExpression(bson::Regex {
            pattern,
            options: options.into_iter().collect(),
        }))
    }

    fn number(&mut self) -> Result<Bson, ShellError> {
        let at = self.pos;
        let mut s = String::new();
        if self.peek() == Some('-') {
            s.push('-');
            self.bump();
        }
        let mut float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
            } else if c == '.' || c == 'e' || c == 'E' {
                float = true;
                s.push(c);
            } else if (c == '+' || c == '-') && matches!(s.chars().last(), Some('e' | 'E')) {
                s.push(c);
            } else {
                break;
            }
            self.bump();
        }
        if !float {
            if let Ok(n) = s.parse::<i32>() {
                return Ok(Bson::Int32(n));
            }
            if let Ok(n) = s.parse::<i64>() {
                return Ok(Bson::Int64(n));
            }
        }
        s.parse::<f64>()
            .map(Bson::Double)
            .map_err(|_| self.err_at(at, &format!("invalid number `{s}`")))
    }
}

fn integer(b: &Bson) -> Option<i64> {
    match b {
        Bson::Int32(n) => Some(i64::from(*n)),
        Bson::Int64(n) => Some(*n),
        Bson::Double(f) if f.fract() == 0.0 => Some(*f as i64),
        _ => None,
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn parse_date(s: &str) -> Option<Bson> {
    let full = if s.len() == 10 {
        format!("{s}T00:00:00Z")
    } else if !s.ends_with('Z') && !s[10..].contains(['+', '-']) {
        format!("{s}Z")
    } else {
        s.to_owned()
    };
    bson::DateTime::parse_rfc3339_str(&full)
        .ok()
        .map(Bson::DateTime)
}

/// Canonical/relaxed Extended JSON wrappers (`{"$oid": …}`) to BSON values.
fn extended(doc: Document) -> Result<Bson, String> {
    if doc.len() != 1
        && !(doc.len() == 2 && doc.contains_key("$regex") && doc.contains_key("$options"))
    {
        return Ok(Bson::Document(doc));
    }
    let (key, value) = doc
        .iter()
        .next()
        .map(|(k, v)| (k.clone(), v.clone()))
        .unwrap_or_default();
    let text = |v: &Bson| match v {
        Bson::String(s) => Some(s.clone()),
        _ => None,
    };
    let converted = match key.as_str() {
        "$oid" => text(&value)
            .and_then(|s| ObjectId::parse_str(s).ok())
            .map(Bson::ObjectId),
        "$date" => match &value {
            Bson::String(s) => parse_date(s),
            Bson::Int32(n) => Some(Bson::DateTime(bson::DateTime::from_millis(i64::from(*n)))),
            Bson::Int64(n) => Some(Bson::DateTime(bson::DateTime::from_millis(*n))),
            Bson::Document(d) => d
                .get_str("$numberLong")
                .ok()
                .and_then(|s| s.parse().ok())
                .map(|n| Bson::DateTime(bson::DateTime::from_millis(n))),
            _ => None,
        },
        "$numberLong" => text(&value).and_then(|s| s.parse().ok()).map(Bson::Int64),
        "$numberInt" => text(&value).and_then(|s| s.parse().ok()).map(Bson::Int32),
        "$numberDouble" => text(&value).and_then(|s| s.parse().ok()).map(Bson::Double),
        "$numberDecimal" => text(&value)
            .and_then(|s| s.parse::<bson::Decimal128>().ok())
            .map(Bson::Decimal128),
        _ => return Ok(Bson::Document(doc)),
    };
    converted.ok_or_else(|| format!("invalid Extended JSON value for `{key}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    fn find(
        text: &str,
    ) -> (
        String,
        Document,
        Option<Document>,
        Option<Document>,
        Option<i64>,
        Option<u64>,
    ) {
        match parse_command(text).unwrap() {
            MongoCommand::Find {
                collection,
                filter,
                projection,
                sort,
                limit,
                skip,
                ..
            } => (collection, filter, projection, sort, limit, skip),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_form_parses() {
        assert_eq!(
            parse_command("show dbs").unwrap(),
            MongoCommand::ShowDatabases
        );
        assert_eq!(
            parse_command(" show databases ;").unwrap(),
            MongoCommand::ShowDatabases
        );
        assert_eq!(
            parse_command("show collections").unwrap(),
            MongoCommand::ShowCollections
        );
        assert_eq!(
            parse_command("use shop").unwrap(),
            MongoCommand::Use("shop".into())
        );

        let (c, f, p, s, l, k) = find(
            "db.items.find({price: {$gt: 5}}, {name: 1}).sort({price: -1}).limit(10).skip(20);",
        );
        assert_eq!(c, "items");
        assert_eq!(f, doc! {"price": {"$gt": 5}});
        assert_eq!(p, Some(doc! {"name": 1}));
        assert_eq!(s, Some(doc! {"price": -1}));
        assert_eq!((l, k), (Some(10), Some(20)));
        assert_eq!(find("db.items.find()").1, Document::new());
        assert_eq!(find("db.audit.log.find()").0, "audit.log");
        assert_eq!(find("db.getCollection('my coll').find({})").0, "my coll");

        let cases = [
            "db.items.findOne({_id: ObjectId('65a1b2c3d4e5f60718293a4b')})",
            "db.items.countDocuments({a: 1})",
            "db.items.estimatedDocumentCount()",
            "db.items.distinct('tags', {a: 1})",
            "db.items.aggregate([{$match: {a: 1}}, {$group: {_id: '$b', n: {$sum: 1}}}])",
            "db.items.insertOne({a: 1})",
            "db.items.insertMany([{a: 1}, {a: 2},])",
            "db.items.updateOne({a: 1}, {$set: {b: 2}})",
            "db.items.updateMany({}, [{$set: {b: 2}}])",
            "db.items.replaceOne({a: 1}, {a: 1, b: 3})",
            "db.items.deleteOne({a: 1})",
            "db.items.deleteMany({a: {$lt: 0}})",
            "db.items.createIndex({a: 1}, {unique: true})",
            "db.items.dropIndex('a_1')",
            "db.items.drop()",
            "db.createCollection('logs')",
            "db.runCommand({ping: 1})",
        ];
        for c in cases {
            parse_command(c).unwrap_or_else(|e| panic!("{c}: {e}"));
        }
    }

    #[test]
    fn relaxed_json_with_constructors_comments_and_trailing_commas() {
        let v = parse_value(
            r#"{
              // a comment
              "id": ObjectId("65a1b2c3d4e5f60718293a4b"),
              at: ISODate("2024-05-01T10:00:00Z"), day: new Date('2024-05-01'),
              big: NumberLong("9007199254740993"), small: NumberInt(7), n: 3000000000,
              f: 1.5, e: -2e3, price: NumberDecimal("9.99"),
              u: UUID("123e4567-e89b-12d3-a456-426614174000"),
              re: /^ab+c/i, ok: true, none: null, /* block */ list: [1, 'two', {x: 1},],
              oid2: {"$oid": "65a1b2c3d4e5f60718293a4b"}, d2: {$date: {$numberLong: "0"}},
              esc: "a\"b\né",
            }"#,
        )
        .unwrap();
        let Bson::Document(d) = v else { panic!() };
        assert!(matches!(d.get("id"), Some(Bson::ObjectId(_))));
        assert_eq!(d.get("id"), d.get("oid2"));
        assert!(matches!(d.get("at"), Some(Bson::DateTime(_))));
        assert_eq!(
            d.get_datetime("day")
                .unwrap()
                .try_to_rfc3339_string()
                .unwrap(),
            "2024-05-01T00:00:00Z"
        );
        assert_eq!(d.get("big"), Some(&Bson::Int64(9_007_199_254_740_993)));
        assert_eq!(d.get("small"), Some(&Bson::Int32(7)));
        assert_eq!(d.get("n"), Some(&Bson::Int64(3_000_000_000)));
        assert_eq!(d.get("f"), Some(&Bson::Double(1.5)));
        assert_eq!(d.get("e"), Some(&Bson::Double(-2000.0)));
        assert!(matches!(d.get("price"), Some(Bson::Decimal128(_))));
        assert!(matches!(d.get("u"), Some(Bson::Binary(b)) if b.subtype == BinarySubtype::Uuid));
        assert!(
            matches!(d.get("re"), Some(Bson::RegularExpression(r)) if r.pattern == "^ab+c" && r.options == "i")
        );
        assert_eq!(d.get("none"), Some(&Bson::Null));
        assert_eq!(d.get_array("list").unwrap().len(), 3);
        assert_eq!(d.get_datetime("d2").unwrap().timestamp_millis(), 0);
        assert_eq!(d.get_str("esc").unwrap(), "a\"b\né");
    }

    #[test]
    fn scripts_and_unknown_methods_are_refused_with_offsets() {
        let e = parse_command("var x = 1").unwrap_err();
        assert_eq!(e.offset, 0);
        assert!(
            e.message.contains("no JavaScript") && e.message.contains("accepted:"),
            "{e}"
        );
        let e = parse_command("for (i = 0; i < 3; i++) db.a.drop()").unwrap_err();
        assert_eq!(e.offset, 0);
        let e = parse_command("db.items.mapReduce(f, g)").unwrap_err();
        assert_eq!(e.offset, 9);
        let e = parse_command("db.items.find({a: x})").unwrap_err();
        assert_eq!(e.offset, 18, "{e}");
        let e = parse_command("db.items.find({a: 1}").unwrap_err();
        assert!(e.message.contains("`,` or `)`"), "{e}");
        let e = parse_command("db.items.find().toArray()").unwrap_err();
        assert!(e.message.contains("toArray"), "{e}");
        let e = parse_command("db.items.find(); db.items.drop()").unwrap_err();
        assert!(e.message.contains("one command per run"), "{e}");
        let e = parse_command("db.items.updateOne({}, {b: 1})").unwrap_err();
        assert!(e.message.contains("replaceOne"), "{e}");
        assert!(parse_command("db.eval('1')").is_err());
    }

    #[test]
    fn classification_and_confirmation() {
        let class = |t: &str| parse_command(t).unwrap().class();
        assert_eq!(class("show collections"), Class::ReadSchema);
        assert_eq!(class("db.a.find()"), Class::ReadData);
        assert_eq!(class("db.a.aggregate([{$match: {}}])"), Class::ReadData);
        assert_eq!(
            class("db.a.aggregate([{$match: {}}, {$out: 'b'}])"),
            Class::ModifyData
        );
        assert_eq!(
            class("db.a.aggregate([{$merge: {into: 'b'}}])"),
            Class::ModifyData
        );
        assert_eq!(class("db.a.insertOne({})"), Class::ModifyData);
        assert_eq!(class("db.a.createIndex({x: 1})"), Class::ModifySchema);
        assert_eq!(class("db.a.drop()"), Class::ModifySchema);
        assert_eq!(class("db.runCommand({ping: 1})"), Class::ReadData);
        assert_eq!(class("db.runCommand({dbStats: 1})"), Class::ReadData);
        assert_eq!(
            class("db.runCommand({dropDatabase: 1})"),
            Class::ModifySchema
        );
        assert_eq!(class("db.runCommand({shutdown: 1})"), Class::ModifySchema);

        let confirm = |t: &str| parse_command(t).unwrap().needs_confirmation();
        assert!(confirm("db.a.deleteMany({})"));
        assert!(confirm("db.a.updateMany({}, {$set: {x: 1}})"));
        assert!(confirm("db.a.drop()"));
        assert!(!confirm("db.a.deleteMany({x: 1})"));
        assert!(!confirm("db.a.deleteOne({})"));
        assert_eq!(
            parse_command("db.a.deleteMany({})").unwrap().describe(),
            "db.a.deleteMany"
        );
    }
}

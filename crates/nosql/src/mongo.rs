//! MongoDB sessions (`SPEC-0310` behaviours 4–5, `SPEC-0311` behaviours 5, 7, 9, 10) on the
//! official `mongodb` driver.

use std::collections::BTreeSet;

use idecraft_database::Secret;
use mongodb::bson::{self, Bson, Document, doc};
use mongodb::error::{ErrorKind, WriteFailure};
use mongodb::options::{ClientOptions, FindOneOptions, FindOptions, IndexOptions};
use mongodb::results::CollectionType;
use mongodb::{Client, Collection, Cursor, IndexModel};

use crate::edit::MongoEdit;
use crate::mongo_shell::{MongoCommand, UpdateBody, parse_command};
use crate::{CONNECT_TIMEOUT, Class, Permission, StoreError, StoreProfile, check_class};

/// A database of the server.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DatabaseInfo {
    /// Name.
    pub name: String,
    /// Size on disk in bytes.
    pub size: u64,
}

/// A collection or view.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CollectionInfo {
    /// Name.
    pub name: String,
    /// `collection`, `view` or `timeseries`.
    pub kind: &'static str,
    /// Estimated document count (not for views).
    pub count: Option<u64>,
}

/// An index.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct IndexInfo {
    /// Name.
    pub name: String,
    /// Keys as relaxed JSON (`{"price":-1}`).
    pub keys: String,
    /// Unique.
    pub unique: bool,
    /// TTL in seconds.
    pub ttl: Option<u64>,
}

/// Result of a console command (behaviour 5).
#[derive(Debug, Clone, PartialEq)]
pub enum MongoReply {
    /// A page of documents.
    Documents {
        /// Documents of the page.
        documents: Vec<Document>,
        /// Page number (0-based).
        page: usize,
        /// Whether another page exists.
        more: bool,
    },
    /// A single value (counts, distinct, command replies, `show dbs`).
    Value(Bson),
    /// A message (`switched to db shop`, `no document`).
    Message(String),
    /// Write counts and inserted ids.
    Written(Document),
}

impl MongoReply {
    /// As JSON (relaxed Extended JSON) for MCP.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            MongoReply::Documents {
                documents,
                page,
                more,
            } => json!({
                "documents": documents.iter().map(|d| Bson::Document(d.clone()).into_relaxed_extjson()).collect::<Vec<_>>(),
                "page": page, "more": more,
            }),
            MongoReply::Value(v) => json!({ "value": v.clone().into_relaxed_extjson() }),
            MongoReply::Message(m) => json!({ "message": m }),
            MongoReply::Written(d) => {
                json!({ "written": Bson::Document(d.clone()).into_relaxed_extjson() })
            }
        }
    }
}

/// An open MongoDB session.
pub struct MongoSession {
    client: Client,
    profile: StoreProfile,
    database: String,
}

fn server_error(e: mongodb::error::Error) -> StoreError {
    let code = match e.kind.as_ref() {
        ErrorKind::Command(c) => Some(c.code.to_string()),
        ErrorKind::Write(WriteFailure::WriteError(w)) => Some(w.code.to_string()),
        ErrorKind::Write(WriteFailure::WriteConcernError(w)) => Some(w.code.to_string()),
        _ => None,
    };
    let message = match e.kind.as_ref() {
        ErrorKind::Command(c) => c.message.clone(),
        ErrorKind::Write(WriteFailure::WriteError(w)) => w.message.clone(),
        _ => e.to_string(),
    };
    StoreError::Server { code, message }
}

impl MongoSession {
    /// Connects and pings within [`CONNECT_TIMEOUT`] (SPEC-0310 behaviour 4).
    pub async fn connect(
        profile: &StoreProfile,
        password: Option<Secret>,
    ) -> Result<Self, StoreError> {
        let endpoint = profile.endpoint()?;
        let fail = |message: String| StoreError::Connect {
            profile: profile.name.clone(),
            address: endpoint.address(),
            message,
        };
        let parse = tokio::time::timeout(CONNECT_TIMEOUT, ClientOptions::parse(&profile.url))
            .await
            .map_err(|_| fail("timed out resolving the URL".into()))?;
        let mut options = parse.map_err(|e| fail(e.to_string()))?;
        if let Some(pw) = password {
            match options.credential.as_mut() {
                Some(c) => c.password = Some(pw.expose().to_owned()),
                None => {
                    return Err(fail(
                        "a password is stored but the URL names no user".into(),
                    ));
                }
            }
        }
        options.connect_timeout = Some(CONNECT_TIMEOUT);
        options.server_selection_timeout = Some(CONNECT_TIMEOUT);
        options.app_name = Some("IDECraft".into());
        let client = Client::with_options(options).map_err(|e| fail(e.to_string()))?;
        let database = endpoint.database.clone().unwrap_or_else(|| "test".into());
        let admin = client.database("admin");
        let ping = admin.run_command(doc! {"ping": 1});
        match tokio::time::timeout(CONNECT_TIMEOUT, ping).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(fail(e.to_string())),
            Err(_) => return Err(fail("timed out".into())),
        }
        Ok(Self {
            client,
            profile: profile.clone(),
            database,
        })
    }

    /// The profile.
    pub fn profile(&self) -> &StoreProfile {
        &self.profile
    }

    /// Server version (`buildInfo`).
    pub async fn server_version(&self) -> Result<String, StoreError> {
        let info = self
            .client
            .database("admin")
            .run_command(doc! {"buildInfo": 1})
            .await
            .map_err(server_error)?;
        Ok(info.get_str("version").unwrap_or("?").to_owned())
    }

    /// Current database of the console.
    pub fn database(&self) -> &str {
        &self.database
    }

    fn coll(&self, db: &str, name: &str) -> Collection<Document> {
        self.client.database(db).collection(name)
    }

    /// Databases (tree level 1).
    pub async fn databases(&self) -> Result<Vec<DatabaseInfo>, StoreError> {
        let mut out: Vec<DatabaseInfo> = self
            .client
            .list_databases()
            .await
            .map_err(server_error)?
            .into_iter()
            .map(|d| DatabaseInfo {
                name: d.name,
                size: d.size_on_disk,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Collections and views of `db` with estimated counts (tree level 2).
    pub async fn collections(&self, db: &str) -> Result<Vec<CollectionInfo>, StoreError> {
        let mut cursor = self
            .client
            .database(db)
            .list_collections()
            .await
            .map_err(server_error)?;
        let mut out = Vec::new();
        while cursor.advance().await.map_err(server_error)? {
            let spec = cursor.deserialize_current().map_err(server_error)?;
            let kind = match spec.collection_type {
                CollectionType::View => "view",
                CollectionType::Timeseries => "timeseries",
                _ => "collection",
            };
            out.push(CollectionInfo {
                name: spec.name,
                kind,
                count: None,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        for c in &mut out {
            if c.kind != "view" {
                c.count = self.coll(db, &c.name).estimated_document_count().await.ok();
            }
        }
        Ok(out)
    }

    /// Indexes of a collection.
    pub async fn indexes(&self, db: &str, collection: &str) -> Result<Vec<IndexInfo>, StoreError> {
        let mut cursor = self
            .coll(db, collection)
            .list_indexes()
            .await
            .map_err(server_error)?;
        let mut out = Vec::new();
        while cursor.advance().await.map_err(server_error)? {
            let model: IndexModel = cursor.deserialize_current().map_err(server_error)?;
            let opts = model.options.unwrap_or_default();
            out.push(IndexInfo {
                name: opts.name.unwrap_or_default(),
                keys: crate::view::compact(&Bson::Document(model.keys)),
                unique: opts.unique.unwrap_or(false),
                ttl: opts.expire_after.map(|d| d.as_secs()),
            });
        }
        Ok(out)
    }

    /// Field paths from a sample of at most 100 documents (tree "fields").
    pub async fn sample_fields(
        &self,
        db: &str,
        collection: &str,
    ) -> Result<Vec<crate::FieldInfo>, StoreError> {
        let cursor = self
            .coll(db, collection)
            .aggregate(vec![doc! {"$sample": {"size": 100}}])
            .await
            .map_err(server_error)?;
        Ok(crate::tree::field_paths(
            &collect(cursor, usize::MAX).await?,
        ))
    }

    /// Parses, checks and runs one console command (behaviours 1–5, 9). `rights` are the agent's
    /// (`None` for a person at the console); `confirmed` answers the question for empty-filter
    /// deletes and updates and `drop()`.
    pub async fn run(
        &mut self,
        text: &str,
        page: usize,
        page_size: usize,
        confirmed: bool,
        rights: Option<&BTreeSet<Permission>>,
    ) -> Result<MongoReply, StoreError> {
        let cmd = parse_command(text)?;
        check_class(&self.profile, cmd.class(), rights, &cmd.describe())?;
        if cmd.needs_confirmation() && !confirmed {
            let coll = cmd.collection().unwrap_or_default().to_owned();
            let n = self
                .coll(&self.database, &coll)
                .estimated_document_count()
                .await
                .unwrap_or(0);
            return Err(StoreError::Confirm(format!(
                "{} affects all {n} documents of `{coll}` in `{}`; confirm to run it",
                cmd.describe(),
                self.database
            )));
        }
        self.execute(&cmd, page, page_size.clamp(1, 1000)).await
    }

    async fn execute(
        &mut self,
        cmd: &MongoCommand,
        page: usize,
        size: usize,
    ) -> Result<MongoReply, StoreError> {
        let db = self.database.clone();
        let written = |d: Document| Ok(MongoReply::Written(d));
        match cmd {
            MongoCommand::ShowDatabases => {
                let dbs = self.databases().await?;
                Ok(MongoReply::Value(Bson::Array(
                    dbs.into_iter()
                        .map(|d| Bson::Document(doc! {"name": d.name, "sizeOnDisk": d.size as i64}))
                        .collect(),
                )))
            }
            MongoCommand::ShowCollections => {
                let cs = self.collections(&db).await?;
                Ok(MongoReply::Value(Bson::Array(
                    cs.into_iter().map(|c| Bson::String(c.name)).collect(),
                )))
            }
            MongoCommand::Use(name) => {
                self.database = name.clone();
                Ok(MongoReply::Message(format!("switched to db {name}")))
            }
            MongoCommand::Find {
                collection,
                filter,
                projection,
                sort,
                limit,
                skip,
                one: true,
            } => {
                let _ = (sort, limit, skip);
                let found = self
                    .coll(&db, collection)
                    .find_one(filter.clone())
                    .with_options(
                        FindOneOptions::builder()
                            .projection(projection.clone())
                            .build(),
                    )
                    .await
                    .map_err(server_error)?;
                Ok(match found {
                    Some(d) => MongoReply::Documents {
                        documents: vec![d],
                        page: 0,
                        more: false,
                    },
                    None => MongoReply::Message("no document".into()),
                })
            }
            MongoCommand::Find {
                collection,
                filter,
                projection,
                sort,
                limit,
                skip,
                one: false,
            } => {
                let user_limit = limit.map(|l| l.unsigned_abs() as usize).filter(|l| *l > 0);
                let offset = page * size;
                let remaining = user_limit.map(|l| l.saturating_sub(offset));
                if remaining == Some(0) {
                    return Ok(MongoReply::Documents {
                        documents: Vec::new(),
                        page,
                        more: false,
                    });
                }
                let want = remaining.map_or(size + 1, |r| r.min(size + 1));
                let cursor = self
                    .coll(&db, collection)
                    .find(filter.clone())
                    .with_options(
                        FindOptions::builder()
                            .projection(projection.clone())
                            .sort(sort.clone())
                            .skip(skip.unwrap_or(0) + offset as u64)
                            .limit(want as i64)
                            .build(),
                    )
                    .await
                    .map_err(server_error)?;
                let mut documents = collect(cursor, want).await?;
                let more = documents.len() > size;
                documents.truncate(size);
                Ok(MongoReply::Documents {
                    documents,
                    page,
                    more,
                })
            }
            MongoCommand::Count {
                collection,
                filter,
                estimated,
            } => {
                let c = self.coll(&db, collection);
                let n = if *estimated {
                    c.estimated_document_count().await
                } else {
                    c.count_documents(filter.clone()).await
                }
                .map_err(server_error)?;
                Ok(MongoReply::Value(Bson::Int64(n as i64)))
            }
            MongoCommand::Distinct {
                collection,
                field,
                filter,
            } => {
                let values = self
                    .coll(&db, collection)
                    .distinct(field, filter.clone())
                    .await
                    .map_err(server_error)?;
                Ok(MongoReply::Value(Bson::Array(values)))
            }
            MongoCommand::Aggregate {
                collection,
                pipeline,
            } => {
                let writes = cmd.class() == Class::ModifyData;
                let mut stages = pipeline.clone();
                if !writes {
                    stages.push(doc! {"$skip": (page * size) as i64});
                    stages.push(doc! {"$limit": (size + 1) as i64});
                }
                let cursor = self
                    .coll(&db, collection)
                    .aggregate(stages)
                    .await
                    .map_err(server_error)?;
                let mut documents = collect(cursor, size + 1).await?;
                let more = !writes && documents.len() > size;
                documents.truncate(size);
                Ok(MongoReply::Documents {
                    documents,
                    page,
                    more,
                })
            }
            MongoCommand::Insert {
                collection,
                documents,
                ..
            } => {
                let c = self.coll(&db, collection);
                let ids: Vec<Bson> = if documents.len() == 1 {
                    vec![
                        c.insert_one(documents[0].clone())
                            .await
                            .map_err(server_error)?
                            .inserted_id,
                    ]
                } else {
                    let r = c
                        .insert_many(documents.clone())
                        .await
                        .map_err(server_error)?;
                    let mut ids: Vec<(usize, Bson)> = r.inserted_ids.into_iter().collect();
                    ids.sort_by_key(|(i, _)| *i);
                    ids.into_iter().map(|(_, id)| id).collect()
                };
                written(doc! {"insertedCount": ids.len() as i64, "insertedIds": ids})
            }
            MongoCommand::Update {
                collection,
                filter,
                update,
                many,
            } => {
                let c = self.coll(&db, collection);
                let r = match (update, many) {
                    (UpdateBody::Operators(u), false) => {
                        c.update_one(filter.clone(), u.clone()).await
                    }
                    (UpdateBody::Operators(u), true) => {
                        c.update_many(filter.clone(), u.clone()).await
                    }
                    (UpdateBody::Pipeline(p), false) => {
                        c.update_one(filter.clone(), p.clone()).await
                    }
                    (UpdateBody::Pipeline(p), true) => {
                        c.update_many(filter.clone(), p.clone()).await
                    }
                }
                .map_err(server_error)?;
                written(
                    doc! {"matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64},
                )
            }
            MongoCommand::Replace {
                collection,
                filter,
                document,
            } => {
                let r = self
                    .coll(&db, collection)
                    .replace_one(filter.clone(), document.clone())
                    .await
                    .map_err(server_error)?;
                written(
                    doc! {"matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64},
                )
            }
            MongoCommand::Delete {
                collection,
                filter,
                many,
            } => {
                let c = self.coll(&db, collection);
                let r = if *many {
                    c.delete_many(filter.clone()).await
                } else {
                    c.delete_one(filter.clone()).await
                }
                .map_err(server_error)?;
                written(doc! {"deletedCount": r.deleted_count as i64})
            }
            MongoCommand::CreateIndex {
                collection,
                keys,
                options,
            } => {
                let options: Option<IndexOptions> =
                    match options {
                        Some(o) => Some(bson::from_document(o.clone()).map_err(|e| {
                            StoreError::Server {
                                code: None,
                                message: format!("index options: {e}"),
                            }
                        })?),
                        None => None,
                    };
                let model = IndexModel::builder()
                    .keys(keys.clone())
                    .options(options)
                    .build();
                let r = self
                    .coll(&db, collection)
                    .create_index(model)
                    .await
                    .map_err(server_error)?;
                Ok(MongoReply::Value(Bson::String(r.index_name)))
            }
            MongoCommand::DropIndex { collection, name } => {
                self.coll(&db, collection)
                    .drop_index(name)
                    .await
                    .map_err(server_error)?;
                Ok(MongoReply::Message(format!("dropped index {name}")))
            }
            MongoCommand::Drop { collection } => {
                self.coll(&db, collection)
                    .drop()
                    .await
                    .map_err(server_error)?;
                Ok(MongoReply::Message(format!("dropped {collection}")))
            }
            MongoCommand::CreateCollection(name) => {
                self.client
                    .database(&db)
                    .create_collection(name)
                    .await
                    .map_err(server_error)?;
                Ok(MongoReply::Message(format!("created {name}")))
            }
            MongoCommand::RunCommand(command) => {
                let r = self
                    .client
                    .database(&db)
                    .run_command(command.clone())
                    .await
                    .map_err(server_error)?;
                Ok(MongoReply::Value(Bson::Document(r)))
            }
        }
    }

    /// Every document of a `find` or read-only `aggregate` (export, behaviour 10).
    pub async fn read_all(
        &self,
        text: &str,
        rights: Option<&BTreeSet<Permission>>,
    ) -> Result<Vec<Document>, StoreError> {
        let cmd = parse_command(text)?;
        check_class(&self.profile, cmd.class(), rights, &cmd.describe())?;
        if !cmd.class().is_read() {
            return Err(StoreError::Edit(
                "only find and aggregate results can be exported".into(),
            ));
        }
        let db = &self.database;
        match cmd {
            MongoCommand::Find {
                collection,
                filter,
                projection,
                sort,
                limit,
                skip,
                ..
            } => {
                let cursor = self
                    .coll(db, &collection)
                    .find(filter)
                    .with_options(
                        FindOptions::builder()
                            .projection(projection)
                            .sort(sort)
                            .skip(skip)
                            .limit(limit)
                            .build(),
                    )
                    .await
                    .map_err(server_error)?;
                collect(cursor, usize::MAX).await
            }
            MongoCommand::Aggregate {
                collection,
                pipeline,
            } => {
                let cursor = self
                    .coll(db, &collection)
                    .aggregate(pipeline)
                    .await
                    .map_err(server_error)?;
                collect(cursor, usize::MAX).await
            }
            _ => Err(StoreError::Edit(
                "only find and aggregate results can be exported".into(),
            )),
        }
    }

    /// Applies a planned edit (behaviour 7); nothing is written when the document changed since it
    /// was read.
    pub async fn apply_edit(
        &self,
        edit: &MongoEdit,
        rights: Option<&BTreeSet<Permission>>,
    ) -> Result<(), StoreError> {
        check_class(&self.profile, Class::ModifyData, rights, &edit.preview())?;
        let c = self.coll(&self.database, &edit.collection);
        let r = if edit.replace {
            c.replace_one(edit.filter.clone(), edit.update.clone())
                .await
        } else {
            c.update_one(edit.filter.clone(), edit.update.clone()).await
        }
        .map_err(server_error)?;
        if r.matched_count == 0 {
            return Err(StoreError::Edit(
                "the document changed since it was read (or was deleted); nothing was written"
                    .into(),
            ));
        }
        Ok(())
    }
}

async fn collect(mut cursor: Cursor<Document>, max: usize) -> Result<Vec<Document>, StoreError> {
    let mut out = Vec::new();
    while out.len() < max && cursor.advance().await.map_err(server_error)? {
        out.push(cursor.deserialize_current().map_err(server_error)?);
    }
    Ok(out)
}

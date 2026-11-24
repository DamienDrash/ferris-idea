//! Non-relational data sources (ADR-0022): MongoDB (document family) and Redis (key-value family).
//!
//! Implements `SPEC-0310` (store profiles, URLs, TLS, browser tree, export, MCP surface),
//! `SPEC-0311` (MongoDB console language, completion, result views, edits) and `SPEC-0312` (Redis
//! console, value views, edits). SQL dialects stay in `ferris-idea-database`; this crate reuses its
//! keychain store and agent permissions.

mod browse;
mod completion;
mod edit;
mod export;
mod form;
mod mongo;
mod mongo_shell;
mod profile;
mod redis_cli;
mod redis_session;
mod tree;
mod view;

use std::collections::BTreeSet;
use std::time::Duration;

pub use ferris_idea_database::Permission;

pub use browse::{BrowseItem, BrowseLevel, KEY_PAGE, browse, browse_mongo, browse_redis};
pub use completion::{
    MONGO_METHODS, MongoContext, StoreCandidate, complete_mongo, complete_redis, scan_pattern,
};
pub use edit::{FieldChange, MongoEdit, RedisEdit, diff_documents, plan_replace, plan_update};
pub use export::{ExportFormat, export_documents, export_keys, key_json, write_export};
pub use form::{
    form_profile, form_url, remove_store, save_store, store_system, test_store, test_store_form,
};
pub use mongo::{CollectionInfo, DatabaseInfo, IndexInfo, MongoReply, MongoSession};
pub use mongo_shell::{ACCEPTED, MongoCommand, ShellError, UpdateBody, parse_command, parse_value};
pub use mongodb::bson;
pub use profile::{
    Endpoint, STORE_FILE, STORE_FORMAT, STORE_KIND, StoreProfile, System, Tls, load_stores,
    parse_stores, render_stores, save_stores, sql_connection_ids, unique_store_id, validate_stores,
};
pub use redis_cli::{
    COMMANDS, CommandInfo, KEYS_LIMIT, Reply, bytes_display, bytes_json, classify_redis,
    command_info, render_reply, tokenize, tokenize_script,
};
pub use redis_session::{KeyInfo, KeyValue, RedisSession, ValueData};
pub use tree::{FieldInfo, KeyNode, field_paths, key_tree};
pub use view::{TreeRow, bson_type, cell_text, grid, to_json, tree_rows};

/// Time allowed for connecting and the first command (SPEC-0310 behaviour 4).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What a command does, for read-only profiles and agent rights (SPEC-0311 b9, SPEC-0312 b5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Lists databases, collections, indexes.
    ReadSchema,
    /// Only reads data.
    ReadData,
    /// Changes documents or keys.
    ModifyData,
    /// Administration, schema changes, or not known to be harmless.
    ModifySchema,
}

impl Class {
    /// Permission an agent needs.
    pub fn permission(self) -> Permission {
        match self {
            Class::ReadSchema => Permission::ReadSchema,
            Class::ReadData => Permission::ReadData,
            Class::ModifyData => Permission::ModifyData,
            Class::ModifySchema => Permission::ModifySchema,
        }
    }

    /// Whether the command only reads.
    pub fn is_read(self) -> bool {
        matches!(self, Class::ReadSchema | Class::ReadData)
    }
}

/// Refuses `class` on a read-only profile or, when `rights` is given (agents), without the right.
pub fn check_class(
    profile: &StoreProfile,
    class: Class,
    rights: Option<&BTreeSet<Permission>>,
    what: &str,
) -> Result<(), StoreError> {
    if profile.read_only && !class.is_read() {
        return Err(StoreError::ReadOnly {
            profile: profile.name.clone(),
            what: what.to_owned(),
        });
    }
    if let Some(rights) = rights {
        let needed = class.permission();
        if !rights.contains(&needed) {
            return Err(StoreError::Refused {
                profile: profile.name.clone(),
                needed,
                what: what.to_owned(),
            });
        }
    }
    Ok(())
}

/// Errors of store profiles, sessions and commands.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The URL could not be understood.
    #[error("invalid store URL: {0}")]
    Url(String),
    /// The URL carried a password.
    #[error("connection URLs must not contain a password; store it in the system keychain instead")]
    PasswordInUrl,
    /// A profile is invalid.
    #[error("store `{name}`: {reason}")]
    Invalid {
        /// Profile name.
        name: String,
        /// What is wrong.
        reason: String,
    },
    /// The store file could not be read, parsed or written.
    #[error("{0}")]
    File(String),
    /// The keychain entry is missing.
    #[error("connection `{profile}`: password missing in keychain entry {service}/{account}")]
    MissingCredentials {
        /// Profile name.
        profile: String,
        /// Keychain service.
        service: String,
        /// Keychain account.
        account: String,
    },
    /// The keychain itself failed.
    #[error("{0}")]
    Keychain(String),
    /// Connecting failed.
    #[error("connection `{profile}` ({address}): {message}")]
    Connect {
        /// Profile name.
        profile: String,
        /// Address tried.
        address: String,
        /// Driver message.
        message: String,
    },
    /// The profile is read-only.
    #[error("connection `{profile}` is read-only: {what} changes data")]
    ReadOnly {
        /// Profile name.
        profile: String,
        /// The command.
        what: String,
    },
    /// An agent lacks a right.
    #[error("connection `{profile}`: {what} needs the `{needed}` right")]
    Refused {
        /// Profile name.
        profile: String,
        /// Missing right.
        needed: Permission,
        /// The command.
        what: String,
    },
    /// The command needs a confirmation first.
    #[error("{0}")]
    Confirm(String),
    /// The console text could not be parsed.
    #[error("{0}")]
    Shell(#[from] ShellError),
    /// The server answered with an error.
    #[error("{message}")]
    Server {
        /// Server error code or prefix (`11000`, `WRONGTYPE`).
        code: Option<String>,
        /// Message.
        message: String,
    },
    /// An edit could not be applied.
    #[error("{0}")]
    Edit(String),
}

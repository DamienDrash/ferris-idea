//! Plugins for Ferris IDEA (`SPEC-0104`, ADR-0016): WebAssembly modules run in a sandbox without access
//! to files, network or the clock. A plugin describes itself with a JSON manifest and offers actions
//! that turn the selected text into new text or a message.
//!
//! The module exports `memory`, `ferris_idea_alloc(len: i32) -> i32`, `ferris_idea_manifest() -> i64` and
//! `ferris_idea_run(action_ptr, action_len, input_ptr, input_len: i32) -> i64`. A returned `i64` packs a
//! pointer (high 32 bits) and a length (low 32 bits) of UTF-8 in `memory`; a negative value is an
//! error code.
//!
//! The plugin API (`SPEC-0156`): `edit` actions get the editor's context as JSON and answer with
//! edits, a selection, a message or a file to open; a plugin may import the host functions
//! `ferris-idea.list_files() -> i64` and `ferris-idea.read_file(path_ptr, path_len: i32) -> i64` of the
//! open project, which answer only when its manifest asks for `project-read` and the user granted
//! it (else `-1`). Both give only the project's own sources; `read_file` answers `-2` for anything
//! else and for a missing file alike, `-5` for a name that commonly holds secrets, `-3` for a file
//! that is too large, and logs every refusal (`SPEC-0156` v3, module `files`).
//!
//! Extension points (`SPEC-0210`): `complete`, `status` and `panel` actions and declared file
//! types; reviewing and updating plugins (`SPEC-0211`); run configuration types that suggest
//! commands for the user to confirm (`SPEC-0276`, permission `suggest-commands`).
//!
//! Connector plugins (`SPEC-0314`, ADR-0024, permission `profile-network`): host functions
//! `ferris-idea.net_connect`, `net_connect_tls`, `net_send`, `net_recv` and `net_close` reach only the
//! host and port of the connection the user picked, after a grant per plugin and host; every
//! connection is logged locally. A connection without TLS only where the connection's SSL mode
//! allows plain text, elsewhere TLS the host makes (`SPEC-0314` v2 behaviour 10).

use std::path::{Path, PathBuf};

mod extensions;
mod files;
mod network;
mod review;
mod run_configs;

pub use extensions::{
    CompleteContext, CompleteItem, CompleteResult, FileType, MAX_ITEMS, MAX_KEYWORDS, MAX_ROWS,
    MAX_STATUS, PanelContext, PanelResult, PanelRow, StatusResult, TokenKind, contributions,
    file_type_for, highlight,
};
pub use files::{
    FILE_AUDIT, FILE_NO_ACCESS, FILE_NO_MEMORY, FILE_NOT_READABLE, FILE_SECRET, FILE_TOO_LARGE,
    FileRefusal, GuardQuestion, MAX_FILE_AUDIT, MAX_LOGGED, MAX_PATH, Refusal, file_answer_meaning,
    file_refusals, guard_question, guard_refusal, guarded, note_passed_on, openable, project_file,
    secret_family,
};
pub use network::{
    AuditRecord, CONNECT_TIMEOUT, ConnectorAnswer, ConnectorRequest, Holder, IO_TIMEOUT,
    MAX_CONNECTIONS, MAX_RECV, NET_BAD, NET_FAILED, NET_NEEDS_TLS, NET_NO_ACCESS, NET_REFUSED,
    NET_TLS_FAILED, NET_TOO_MANY, NETWORK_AUDIT, NETWORK_GRANTS, NetAccess, NetMode, NetTls,
    NetworkGrant, NetworkWarning, Readiness, Target, audit_records, grant_network, is_loopback,
    network_granted, network_grants, network_warning, now_rfc3339, readiness, revoke_network,
    status_meaning,
};
pub use review::{Review, ReviewKind, compare_versions, install_reviewed, review, update_grants};
pub use run_configs::{
    MAX_ARG, MAX_ARGS, MAX_ENV, MAX_NAME, MAX_SUGGESTIONS, RUN_PERMISSIONS, RunAllowance,
    RunAllowances, RunConfigContext, RunType, SuggestedCommand, allow, check_suggestion,
    forget_plugin, load_allowances, revoke, run_permissions_file, run_types,
};

use serde::{Deserialize, Serialize};
use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

/// Instructions one call may execute before it is stopped (a fraction of a second in a release
/// build).
pub const FUEL: u64 = 50_000_000;
/// Largest memory a plugin may use.
pub const MAX_MEMORY: usize = 64 << 20;
/// Largest plugin file read.
pub const MAX_MODULE: u64 = 16 << 20;
/// Largest input or output text.
pub const MAX_TEXT: usize = 4 << 20;

/// What went wrong with a plugin.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The file could not be read.
    #[error("{0}: {1}")]
    Io(String, String),
    /// Not a valid module, or the exports are missing.
    #[error("{0}: not a Ferris IDEA plugin: {1}")]
    Invalid(String, String),
    /// The plugin trapped, ran out of fuel or memory.
    #[error("{0}: {1}")]
    Failed(String, String),
    /// No such action.
    #[error("{0} has no action `{1}`")]
    UnknownAction(String, String),
}

/// What an action's output does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    /// The output replaces the selection (or the whole text without a selection).
    #[default]
    Replace,
    /// The output is shown as a message.
    Message,
    /// The input is the editor's context as JSON, the output an [`EditResult`] as JSON
    /// (`SPEC-0156`).
    Edit,
    /// An inspection (`SPEC-0166`): the input is an [`InspectContext`] as JSON, the output an
    /// [`InspectResult`]; Ferris IDEA runs it on the files it applies to after typing pauses.
    Inspect,
    /// A completion contributor (`SPEC-0210`): a [`CompleteContext`] in, [`CompleteResult`] out.
    Complete,
    /// A status bar widget (`SPEC-0210`): an [`EditorContext`] in, a [`StatusResult`] out.
    Status,
    /// A section of the Plugins tool window (`SPEC-0210`): a [`PanelContext`] in, a
    /// [`PanelResult`] out.
    Panel,
    /// A run configuration type (`SPEC-0276`): a [`RunConfigContext`] in, suggested commands out;
    /// needs the permission `suggest-commands`.
    RunConfig,
    /// A database connector (`SPEC-0314`): a [`ConnectorRequest`] in, a [`ConnectorAnswer`]
    /// out; needs the permission `profile-network`.
    Connector,
}

impl ActionKind {
    /// Whether Ferris IDEA runs actions of this kind by itself instead of offering them as commands.
    pub fn is_extension(self) -> bool {
        matches!(
            self,
            Self::Inspect
                | Self::Complete
                | Self::Status
                | Self::Panel
                | Self::RunConfig
                | Self::Connector
        )
    }
}

/// The file an `inspect` action checks (`SPEC-0166`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectContext {
    /// File path relative to the project.
    pub path: String,
    /// Language id (e.g. `rust`), if known.
    pub language: Option<String>,
    /// The whole text.
    pub text: String,
}

/// How serious a plugin's finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingLevel {
    /// An error.
    Error,
    /// A warning.
    #[default]
    Warning,
    /// A weak hint.
    Weak,
}

/// A quick fix of a plugin's finding: edits of the inspected text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginFix {
    /// Title in the ⌥↩ list.
    pub title: String,
    /// Edits (character offsets of the inspected text).
    pub edits: Vec<PluginEdit>,
}

/// One finding of an `inspect` action: characters `start..end` of the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginFinding {
    /// First character.
    pub start: usize,
    /// Character after the last one.
    pub end: usize,
    /// Message shown.
    pub message: String,
    /// Level; a warning by default.
    #[serde(default)]
    pub level: FindingLevel,
    /// A quick fix.
    #[serde(default)]
    pub fix: Option<PluginFix>,
}

/// What an `inspect` action answers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InspectResult {
    /// Its findings.
    #[serde(default)]
    pub findings: Vec<PluginFinding>,
}

/// Instruction budget of one inspection run (a fifth of an action's: it runs while typing).
pub const INSPECT_FUEL: u64 = FUEL / 5;

/// Most findings taken from one inspection run.
pub const MAX_FINDINGS: usize = 500;

/// What a plugin may ask the user for (`SPEC-0156` behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// List and read the files of the open project (never write them).
    ProjectRead,
    /// Suggest commands that Ferris IDEA shows and starts only after the user confirms them
    /// (`SPEC-0276`).
    SuggestCommands,
    /// Connect to the host and port of a connection the user picks (`SPEC-0314`).
    ProfileNetwork,
}

impl Permission {
    /// Every permission there is.
    pub const ALL: [Permission; 3] = [
        Self::ProjectRead,
        Self::SuggestCommands,
        Self::ProfileNetwork,
    ];

    /// Name as in manifests and the grants file.
    pub fn name(self) -> &'static str {
        match self {
            Self::ProjectRead => "project-read",
            Self::SuggestCommands => "suggest-commands",
            Self::ProfileNetwork => "profile-network",
        }
    }

    /// What it allows, as the Plugins page says it.
    pub fn describe(self) -> &'static str {
        match self {
            Self::ProjectRead => "reads the project",
            Self::SuggestCommands => "suggests commands to run",
            Self::ProfileNetwork => "connects to the host and port of a connection you pick",
        }
    }
}

/// The editor an `edit` action works on (`SPEC-0156` behaviour 1). Offsets count characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorContext {
    /// File path relative to the project.
    pub path: String,
    /// Language id (e.g. `rust`), if known.
    pub language: Option<String>,
    /// The whole text.
    pub text: String,
    /// Selection start (the caret without a selection).
    pub selection_start: usize,
    /// Selection end.
    pub selection_end: usize,
    /// 0-based caret line.
    pub line: usize,
    /// 0-based caret column.
    pub column: usize,
}

/// One edit of an `edit` action: characters `start..end` of the context's text become `text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEdit {
    /// First character replaced.
    pub start: usize,
    /// Character after the last one replaced.
    pub end: usize,
    /// New text.
    pub text: String,
}

/// A file an `edit` action asks to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    /// Path relative to the project.
    pub path: String,
    /// 0-based line.
    #[serde(default)]
    pub line: Option<usize>,
}

/// What an `edit` action answers (`SPEC-0156` behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EditResult {
    /// Edits of the context's text, applied as one undo step.
    #[serde(default)]
    pub edits: Vec<PluginEdit>,
    /// Selection afterwards (character offsets in the edited text).
    #[serde(default)]
    pub select: Option<[usize; 2]>,
    /// Shown in the status bar.
    #[serde(default)]
    pub message: Option<String>,
    /// A project file to open.
    #[serde(default)]
    pub open: Option<OpenRequest>,
}

/// What the host gives one call: the project and whether the user granted reading it.
#[derive(Debug, Clone, Default)]
pub struct Access {
    /// The open project's folder.
    pub root: Option<PathBuf>,
    /// The user granted `project-read` to this plugin.
    pub project_read: bool,
    /// The one host and port a `connector` call may reach (`SPEC-0314`); none for every other
    /// call.
    pub network: Option<NetAccess>,
    /// The log of refused file requests ([`FILE_AUDIT`] in the plugins folder, `SPEC-0156` v3
    /// behaviour 10); none logs nothing.
    pub log: Option<PathBuf>,
}

/// One action a plugin offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Stable id within the plugin.
    pub id: String,
    /// Title shown in Search Everywhere.
    pub title: String,
    /// What the output does.
    #[serde(default)]
    pub kind: ActionKind,
    /// For `inspect` actions: the file extensions it checks (e.g. `["rs", "md"]`); empty for all
    /// files (`SPEC-0166`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

impl ActionInfo {
    /// Whether this `inspect` action checks `path`.
    pub fn inspects(&self, path: &Path) -> bool {
        self.kind == ActionKind::Inspect && self.applies_to(path)
    }

    /// Whether the action's `files` include `path` (all files when empty).
    pub fn applies_to(&self, path: &Path) -> bool {
        self.files.is_empty()
            || path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                self.files
                    .iter()
                    .any(|f| f.trim_start_matches('.').eq_ignore_ascii_case(e))
            })
    }
}

/// A plugin's description of itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Name.
    pub name: String,
    /// Version.
    #[serde(default)]
    pub version: String,
    /// One line about it.
    #[serde(default)]
    pub description: String,
    /// Who made it, as the plugin says (`SPEC-0314` behaviour 1); not verified.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// Its actions.
    #[serde(default)]
    pub actions: Vec<ActionInfo>,
    /// What it asks the user for (`SPEC-0156`).
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// File types it declares (`SPEC-0210` behaviour 3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_types: Vec<FileType>,
}

/// A loaded plugin.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// The module file.
    pub path: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
    module: Module,
}

struct Host {
    limits: StoreLimits,
    access: Access,
    sockets: network::Sockets,
    /// The project's own sources, read once per call.
    own: Option<ferris_idea_core::OwnSources>,
    refusals: files::Refusals,
}

impl Host {
    /// The own sources of the project at `root`.
    fn own(&mut self, root: &Path) -> &ferris_idea_core::OwnSources {
        self.own
            .get_or_insert_with(|| ferris_idea_core::OwnSources::new(root))
    }
}

/// Largest number of files `list_files` returns.
pub const MAX_LISTED: usize = 50_000;

/// The product's prefix of the functions a plugin exports.
pub const EXPORT_PREFIX: &str = "ferris_idea_";

/// The prefix under the product's former name (`SPEC-0356` behaviour 12).
const FORMER_EXPORT_PREFIX: &str = "idecraft_"; // rebrand:keep

/// The export `name`, or the name it had under the product's former name when the plugin exports
/// only that one, so plugins built for older versions keep working (`SPEC-0356` behaviour 12).
fn exported(name: &str, exports: impl Fn(&str) -> bool) -> String {
    if !exports(name)
        && let Some(rest) = name.strip_prefix(EXPORT_PREFIX)
    {
        let old = format!("{FORMER_EXPORT_PREFIX}{rest}");
        if exports(&old) {
            return old;
        }
    }
    name.to_owned()
}

/// Writes `bytes` into the calling plugin's memory through its `ferris_idea_alloc`; the packed result.
fn give(caller: &mut wasmi::Caller<'_, Host>, bytes: &[u8]) -> i64 {
    let name = exported("ferris_idea_alloc", |n| caller.get_export(n).is_some());
    let Some(alloc) = caller
        .get_export(&name)
        .and_then(wasmi::Extern::into_func)
        .and_then(|f| f.typed::<i32, i32>(&*caller).ok())
    else {
        return -4;
    };
    let Some(memory) = caller
        .get_export("memory")
        .and_then(wasmi::Extern::into_memory)
    else {
        return -4;
    };
    let Ok(len) = i32::try_from(bytes.len()) else {
        return -3;
    };
    let Ok(ptr) = alloc.call(&mut *caller, len) else {
        return -4;
    };
    let Ok(at) = usize::try_from(ptr) else {
        return -4;
    };
    if memory.write(&mut *caller, at, bytes).is_err() {
        return -4;
    }
    (i64::from(ptr) << 32) | i64::from(len)
}

/// The module plugins import the host functions from (`SPEC-0156` behaviour 4).
pub const HOST_MODULE: &str = "ferris-idea";

/// The module's name under the product's former name: plugins built for older versions keep
/// linking (`SPEC-0356` behaviour 12).
const FORMER_HOST_MODULE: &str = "idecraft"; // rebrand:keep

/// The host functions of module [`HOST_MODULE`] (`SPEC-0156` behaviour 4), and the same functions
/// under [`FORMER_HOST_MODULE`].
fn host_functions(linker: &mut Linker<Host>) -> Result<(), wasmi::errors::LinkerError> {
    host_functions_in(linker, HOST_MODULE)?;
    if FORMER_HOST_MODULE != HOST_MODULE {
        host_functions_in(linker, FORMER_HOST_MODULE)?;
    }
    Ok(())
}

/// The host functions, in `module`.
fn host_functions_in(
    linker: &mut Linker<Host>,
    module: &'static str,
) -> Result<(), wasmi::errors::LinkerError> {
    linker.func_wrap(
        module,
        "list_files",
        |mut caller: wasmi::Caller<'_, Host>| -> i64 {
            let access = caller.data().access.clone();
            let (Some(root), true) = (access.root, access.project_read) else {
                let host = caller.data_mut();
                host.refusals.note("list_files", Some(""), Refusal::NoGrant);
                return Refusal::NoGrant.code();
            };
            // Only the project's own sources (SPEC-0156 v2 behaviour 6).
            let files: Vec<String> = caller
                .data_mut()
                .own(&root)
                .files()
                .into_iter()
                .take(MAX_LISTED)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect();
            let json = serde_json::to_string(&files).unwrap_or_else(|_| "[]".into());
            give(&mut caller, json.as_bytes())
        },
    )?;
    linker.func_wrap(
        module,
        "read_file",
        |mut caller: wasmi::Caller<'_, Host>, ptr: i32, len: i32| -> i64 {
            let access = caller.data().access.clone();
            // SPEC-0156 v3 behaviours 7 to 10: only an own source that is no secret by its
            // name; one answer for "not yours" and "not there"; every refusal logged.
            let rel = bytes_of(&caller, ptr, len, files::MAX_PATH)
                .and_then(|bytes| String::from_utf8(bytes).ok());
            let host = caller.data_mut();
            let (Some(root), true) = (access.root, access.project_read) else {
                host.refusals
                    .note("read_file", rel.as_deref(), Refusal::NoGrant);
                return Refusal::NoGrant.code();
            };
            let read = match &rel {
                Some(rel) => {
                    let own = host.own(&root);
                    files::own_file(own, rel).and_then(|f| files::read(own, &f))
                }
                None => Err(Refusal::Form),
            };
            match read {
                Ok(bytes) => give(&mut caller, &bytes),
                Err(refusal) => {
                    host.refusals.note("read_file", rel.as_deref(), refusal);
                    refusal.code()
                }
            }
        },
    )?;
    network_functions(linker, module)
}

/// Reads `len` bytes at `ptr` of the calling plugin's memory.
fn bytes_of(caller: &wasmi::Caller<'_, Host>, ptr: i32, len: i32, max: usize) -> Option<Vec<u8>> {
    let memory = caller
        .get_export("memory")
        .and_then(wasmi::Extern::into_memory)?;
    let (at, n) = (usize::try_from(ptr).ok()?, usize::try_from(len).ok()?);
    if n > max {
        return None;
    }
    let mut buf = vec![0; n];
    memory.read(caller, at, &mut buf).ok()?;
    Some(buf)
}

/// The network host functions, in `module` (`SPEC-0314` behaviour 3).
fn network_functions(
    linker: &mut Linker<Host>,
    module: &'static str,
) -> Result<(), wasmi::errors::LinkerError> {
    linker.func_wrap(
        module,
        "net_connect",
        |mut caller: wasmi::Caller<'_, Host>, ptr: i32, len: i32| -> i32 {
            if caller.data().access.network.is_none() {
                return NET_NO_ACCESS;
            }
            let Some(text) =
                bytes_of(&caller, ptr, len, 1024).and_then(|b| String::from_utf8(b).ok())
            else {
                return NET_BAD;
            };
            caller.data_mut().sockets.connect(&text)
        },
    )?;
    linker.func_wrap(
        module,
        "net_connect_tls",
        |mut caller: wasmi::Caller<'_, Host>, ptr: i32, len: i32| -> i32 {
            if caller.data().access.network.is_none() {
                return NET_NO_ACCESS;
            }
            let Some(text) =
                bytes_of(&caller, ptr, len, 1024).and_then(|b| String::from_utf8(b).ok())
            else {
                return NET_BAD;
            };
            caller.data_mut().sockets.connect_tls(&text)
        },
    )?;
    linker.func_wrap(
        module,
        "net_send",
        |mut caller: wasmi::Caller<'_, Host>, handle: i32, ptr: i32, len: i32| -> i32 {
            if caller.data().access.network.is_none() {
                return NET_NO_ACCESS;
            }
            let Some(bytes) = bytes_of(&caller, ptr, len, MAX_TEXT) else {
                return NET_BAD;
            };
            caller.data_mut().sockets.send(handle, &bytes)
        },
    )?;
    linker.func_wrap(
        module,
        "net_recv",
        |mut caller: wasmi::Caller<'_, Host>, handle: i32, max: i32| -> i64 {
            if caller.data().access.network.is_none() {
                return i64::from(NET_NO_ACCESS);
            }
            match caller.data_mut().sockets.recv(handle, max) {
                Ok(bytes) => give(&mut caller, &bytes),
                Err(code) => i64::from(code),
            }
        },
    )?;
    linker.func_wrap(
        module,
        "net_close",
        |mut caller: wasmi::Caller<'_, Host>, handle: i32| -> i32 {
            if caller.data().access.network.is_none() {
                return NET_NO_ACCESS;
            }
            caller.data_mut().sockets.close(handle)
        },
    )?;
    Ok(())
}

fn engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    Engine::new(&config)
}

fn label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One sandboxed instance for a single call.
struct Call {
    store: Store<Host>,
    instance: wasmi::Instance,
    memory: wasmi::Memory,
    name: String,
}

impl Call {
    fn new(
        name: &str,
        file: &str,
        module: &Module,
        fuel: u64,
        access: Access,
    ) -> Result<Self, PluginError> {
        let fail = |e: &dyn std::fmt::Display| PluginError::Invalid(name.to_owned(), e.to_string());
        let limits = StoreLimitsBuilder::new()
            .memory_size(MAX_MEMORY)
            .instances(1)
            .build();
        let sockets = network::Sockets::new(access.network.clone());
        let refusals = files::Refusals::new(access.log.clone(), file);
        let mut store = Store::new(
            module.engine(),
            Host {
                limits,
                access,
                sockets,
                own: None,
                refusals,
            },
        );
        store.limiter(|h| &mut h.limits);
        store.set_fuel(fuel).map_err(|e| fail(&e))?;
        // Only the host functions of `ferris-idea`: a plugin that imports anything else does not link.
        let mut linker = Linker::<Host>::new(module.engine());
        host_functions(&mut linker).map_err(|e| fail(&e))?;
        let instance = linker
            .instantiate_and_start(&mut store, module)
            .map_err(|e| fail(&e))?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| fail(&"no exported memory"))?;
        Ok(Self {
            store,
            instance,
            memory,
            name: name.to_owned(),
        })
    }

    fn failed(&self, e: impl std::fmt::Display) -> PluginError {
        let text = e.to_string();
        let text = if text.contains("fuel") {
            "stopped: it ran too long".to_owned()
        } else {
            text
        };
        PluginError::Failed(self.name.clone(), text)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<i32, PluginError> {
        let name = exported("ferris_idea_alloc", |n| {
            self.instance.get_export(&self.store, n).is_some()
        });
        let alloc = self
            .instance
            .get_typed_func::<i32, i32>(&self.store, &name)
            .map_err(|e| PluginError::Invalid(self.name.clone(), e.to_string()))?;
        let len = i32::try_from(bytes.len()).map_err(|e| self.failed(e))?;
        let ptr = alloc
            .call(&mut self.store, len)
            .map_err(|e| self.failed(e))?;
        let at = usize::try_from(ptr).map_err(|e| self.failed(e))?;
        self.memory
            .write(&mut self.store, at, bytes)
            .map_err(|e| self.failed(e))?;
        Ok(ptr)
    }

    fn read(&self, packed: i64) -> Result<String, PluginError> {
        if packed < 0 {
            return Err(self.failed(format!("the plugin reported error {packed}")));
        }
        let ptr = (packed >> 32) as usize;
        let len = (packed & 0xffff_ffff) as usize;
        if len > MAX_TEXT {
            return Err(self.failed("the output is too large"));
        }
        let mut buf = vec![0; len];
        self.memory
            .read(&self.store, ptr, &mut buf)
            .map_err(|e| self.failed(e))?;
        String::from_utf8(buf).map_err(|e| self.failed(e))
    }
}

/// Why `bytes` are no module, in words: a file that is neither a WebAssembly binary nor its text
/// form says so instead of the text parser's raw complaint (hand test 27: "expected `(` -->
/// <anon>:1:1").
fn module_problem(bytes: &[u8], error: &dyn std::fmt::Display) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
    if bytes.starts_with(b"\0asm") || text.trim_start().starts_with('(') {
        error.to_string()
    } else {
        "the file is not a WebAssembly module (it does not start with the \\0asm header)".into()
    }
}

impl Plugin {
    /// Loads the module at `path` and reads its manifest (behaviour 1).
    pub fn load(path: &Path) -> Result<Self, PluginError> {
        let bytes = read_module(path)?;
        Self::from_bytes(path, &bytes)
    }

    /// Like [`Plugin::load`], from the module's bytes.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self, PluginError> {
        let name = label(path);
        let module = Module::new(&engine(), bytes)
            .map_err(|e| PluginError::Invalid(name.clone(), module_problem(bytes, &e)))?;
        let mut call = Call::new(&name, "", &module, FUEL, Access::default())?;
        let export = exported("ferris_idea_manifest", |n| {
            call.instance.get_export(&call.store, n).is_some()
        });
        let manifest_fn = call
            .instance
            .get_typed_func::<(), i64>(&call.store, &export)
            .map_err(|e| PluginError::Invalid(name.clone(), e.to_string()))?;
        let packed = manifest_fn
            .call(&mut call.store, ())
            .map_err(|e| call.failed(e))?;
        let text = call.read(packed)?;
        let manifest: Manifest = serde_json::from_str(&text)
            .map_err(|e| PluginError::Invalid(name.clone(), format!("manifest: {e}")))?;
        extensions::check_manifest(&manifest).map_err(|e| PluginError::Invalid(name, e))?;
        Ok(Self {
            path: path.to_path_buf(),
            manifest,
            module,
        })
    }

    /// Runs action `id` on `input` in a fresh instance (behaviour 3): the output text.
    pub fn run(&self, id: &str, input: &str) -> Result<String, PluginError> {
        self.run_with_fuel(id, input, FUEL)
    }

    /// Like [`Plugin::run`] with another instruction budget.
    pub fn run_with_fuel(&self, id: &str, input: &str, fuel: u64) -> Result<String, PluginError> {
        self.run_in(id, input, fuel, Access::default())
    }

    /// Runs action `id` with what `access` gives the host functions.
    pub fn run_in(
        &self,
        id: &str,
        input: &str,
        fuel: u64,
        access: Access,
    ) -> Result<String, PluginError> {
        let name = self.manifest.name.clone();
        if !self.manifest.actions.iter().any(|a| a.id == id) {
            return Err(PluginError::UnknownAction(name, id.to_owned()));
        }
        if input.len() > MAX_TEXT {
            return Err(PluginError::Failed(name, "the input is too large".into()));
        }
        let mut call = Call::new(&name, &self.file(), &self.module, fuel, access)?;
        let export = exported("ferris_idea_run", |n| {
            call.instance.get_export(&call.store, n).is_some()
        });
        let run = call
            .instance
            .get_typed_func::<(i32, i32, i32, i32), i64>(&call.store, &export)
            .map_err(|e| PluginError::Invalid(name.clone(), e.to_string()))?;
        let action_ptr = call.write(id.as_bytes())?;
        let input_ptr = call.write(input.as_bytes())?;
        let packed = run
            .call(
                &mut call.store,
                (action_ptr, id.len() as i32, input_ptr, input.len() as i32),
            )
            .map_err(|e| call.failed(e))?;
        call.read(packed)
    }
}

impl Plugin {
    /// Runs the `edit` action `id` on `context` (`SPEC-0156` behaviours 1–2): the answer, with its
    /// edits checked to lie in the text and not to overlap (sorted).
    pub fn run_edit(
        &self,
        id: &str,
        context: &EditorContext,
        access: Access,
    ) -> Result<EditResult, PluginError> {
        let name = self.manifest.name.clone();
        let input = serde_json::to_string(context)
            .map_err(|e| PluginError::Failed(name.clone(), e.to_string()))?;
        let output = self.run_in(id, &input, FUEL, access)?;
        let mut result: EditResult = serde_json::from_str(&output)
            .map_err(|e| PluginError::Failed(name.clone(), format!("its answer: {e}")))?;
        let len = context.text.chars().count();
        result.edits.sort_by_key(|e| (e.start, e.end));
        let mut last = 0;
        for e in &result.edits {
            if e.start > e.end || e.end > len || e.start < last {
                return Err(PluginError::Failed(
                    name,
                    format!(
                        "edit {}..{} is outside the text or overlaps",
                        e.start, e.end
                    ),
                ));
            }
            last = e.end;
        }
        Ok(result)
    }

    /// Runs the `inspect` action `id` on `context` (`SPEC-0166` behaviour 1) with the smaller
    /// budget: its findings, checked to lie in the text (the first [`MAX_FINDINGS`], sorted), with
    /// fixes whose edits lie in the text and do not overlap.
    pub fn run_inspect(
        &self,
        id: &str,
        context: &InspectContext,
        access: Access,
    ) -> Result<Vec<PluginFinding>, PluginError> {
        let name = self.manifest.name.clone();
        let input = serde_json::to_string(context)
            .map_err(|e| PluginError::Failed(name.clone(), e.to_string()))?;
        let output = self.run_in(id, &input, INSPECT_FUEL, access)?;
        let result: InspectResult = serde_json::from_str(&output)
            .map_err(|e| PluginError::Failed(name.clone(), format!("its answer: {e}")))?;
        let len = context.text.chars().count();
        let mut out = Vec::new();
        for mut f in result.findings.into_iter().take(MAX_FINDINGS) {
            if f.start > f.end || f.end > len {
                return Err(PluginError::Failed(
                    name,
                    format!("finding {}..{} is outside the text", f.start, f.end),
                ));
            }
            if let Some(fix) = &mut f.fix {
                fix.edits.sort_by_key(|e| (e.start, e.end));
                let mut last = 0;
                for e in &fix.edits {
                    if e.start > e.end || e.end > len || e.start < last {
                        return Err(PluginError::Failed(
                            name,
                            format!(
                                "fix edit {}..{} is outside the text or overlaps",
                                e.start, e.end
                            ),
                        ));
                    }
                    last = e.end;
                }
            }
            out.push(f);
        }
        out.sort_by_key(|f| (f.start, f.end));
        Ok(out)
    }

    /// File name without `.wasm`, as the grants file names it.
    pub fn file(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// File in the plugins folder that keeps what the user granted to each plugin.
pub const GRANTS: &str = "permissions.json";

/// What the user granted, per plugin file (`SPEC-0156` behaviour 3).
pub fn grants(dir: &Path) -> std::collections::BTreeMap<String, Vec<Permission>> {
    std::fs::read_to_string(dir.join(GRANTS))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Whether plugin `file` of `dir` may use `permission`.
pub fn granted(dir: &Path, file: &str, permission: Permission) -> bool {
    grants(dir)
        .get(file)
        .is_some_and(|p| p.contains(&permission))
}

/// Grants or withdraws `permission` for plugin `file` of `dir`.
pub fn set_granted(
    dir: &Path,
    file: &str,
    permission: Permission,
    on: bool,
) -> Result<(), PluginError> {
    let mut all = grants(dir);
    let list = all.entry(file.to_owned()).or_default();
    list.retain(|p| *p != permission);
    if on {
        list.push(permission);
    }
    all.retain(|_, v| !v.is_empty());
    if permission == Permission::ProfileNetwork && !on {
        // Withdrawing the capability takes every host grant with it (SPEC-0314 behaviour 5).
        revoke_network(dir, file, None, None)?;
    }
    let io = |e: std::io::Error| PluginError::Io(GRANTS.into(), e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let text = serde_json::to_string_pretty(&all)
        .map_err(|e| PluginError::Io(GRANTS.into(), e.to_string()))?;
    std::fs::write(dir.join(GRANTS), text).map_err(io)
}

/// The folder user plugins are installed in: `FERRIS_IDEA_PLUGINS`, else `plugins` in the user's
/// Ferris IDEA settings folder. Workspaces cannot bring plugins of their own (behaviour 2).
pub fn plugins_dir() -> Option<PathBuf> {
    ferris_idea_core::product_var_os("FERRIS_IDEA_PLUGINS")
        .map(PathBuf::from)
        .or_else(|| ferris_idea_core::user_config_dir().map(|d| d.join("plugins")))
}

/// The plugins in `dir` (`*.wasm`, sorted by file name) and the files that failed to load.
pub fn load_dir(dir: &Path) -> (Vec<Plugin>, Vec<String>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "wasm"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut plugins = Vec::new();
    let mut problems = Vec::new();
    for f in files {
        match Plugin::load(&f) {
            Ok(p) => plugins.push(p),
            Err(e) => problems.push(e.to_string()),
        }
    }
    (plugins, problems)
}

/// The action id as Search Everywhere knows it: `Plugin.<file stem>.<action>`.
pub fn action_id(plugin: &Plugin, action: &str) -> String {
    let stem = plugin
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("Plugin.{stem}.{action}")
}

/// Every action of `plugins` as Search Everywhere offers it: id and "plugin name: title".
pub fn action_list(plugins: &[Plugin]) -> Vec<(String, String)> {
    plugins
        .iter()
        .flat_map(|p| {
            // Inspections and other extension points run by themselves (SPEC-0166, SPEC-0210);
            // they are no commands.
            p.manifest
                .actions
                .iter()
                .filter(|a| !a.kind.is_extension())
                .map(move |a| {
                    (
                        action_id(p, &a.id),
                        format!("{}: {}", p.manifest.name, a.title),
                    )
                })
        })
        .collect()
}

/// The plugin and action behind a Search Everywhere id, if installed.
pub fn find_action<'a>(plugins: &'a [Plugin], id: &str) -> Option<(&'a Plugin, &'a ActionInfo)> {
    plugins.iter().find_map(|p| {
        p.manifest
            .actions
            .iter()
            .find(|a| action_id(p, &a.id) == id)
            .map(|a| (p, a))
    })
}

/// Suffix of a disabled plugin's file: `name.wasm.disabled` is kept but not loaded.
pub const DISABLED: &str = ".wasm.disabled";

/// A plugin file in the plugins folder as the Plugins settings page lists it (SPEC-0126).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    /// File name without `.wasm` / `.wasm.disabled`.
    pub file: String,
    /// Whether it is loaded (not disabled).
    pub enabled: bool,
    /// Its manifest, if the module loads.
    pub manifest: Option<Manifest>,
    /// Why it does not load.
    pub problem: Option<String>,
}

/// Every plugin file of `dir`, enabled and disabled, sorted by file name.
pub fn installed(dir: &Path) -> Vec<Installed> {
    let mut out: Vec<Installed> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    let name = label(&path);
                    let (file, enabled) = if let Some(stem) = name.strip_suffix(DISABLED) {
                        (stem.to_owned(), false)
                    } else {
                        (name.strip_suffix(".wasm")?.to_owned(), true)
                    };
                    let (manifest, problem) = match Plugin::load(&path) {
                        Ok(p) => (Some(p.manifest), None),
                        Err(e) => (None, Some(e.to_string())),
                    };
                    Some(Installed {
                        file,
                        enabled,
                        manifest,
                        problem,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Installs the module at `source` into `dir` under its file name, after checking that it loads
/// as a plugin; an installed plugin of the same name (enabled or not) is replaced. The file is read
/// once and the bytes checked are the bytes installed (`SPEC-0211` v3 behaviour 7). The manifest.
pub fn install(dir: &Path, source: &Path) -> Result<Manifest, PluginError> {
    install_reviewed(dir, &review(dir, source)?)
}

/// Reads a module file of at most [`MAX_MODULE`] bytes, never more, also when it grows while it
/// is read.
pub(crate) fn read_module(path: &Path) -> Result<Vec<u8>, PluginError> {
    use std::io::Read;
    let name = label(path);
    let io = |e: std::io::Error| PluginError::Io(name.clone(), e.to_string());
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(io)?
        .take(MAX_MODULE + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_MODULE {
        return Err(PluginError::Invalid(
            name,
            "the file is larger than 16 MiB".into(),
        ));
    }
    Ok(bytes)
}

/// Writes `bytes` as `name` into `dir` through a new file beside it, so a failed write never
/// leaves half a module behind and nothing already lying there (a link) is written through.
pub(crate) fn write_new_module(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), PluginError> {
    use std::io::Write;
    let io = |e: std::io::Error| PluginError::Io(name.to_owned(), e.to_string());
    let part = dir.join(format!("{name}.part"));
    let _ = std::fs::remove_file(&part);
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(io)?;
    let written = out.write_all(bytes).and_then(|()| out.sync_all());
    drop(out);
    if let Err(e) = written.and_then(|()| std::fs::rename(&part, dir.join(name))) {
        let _ = std::fs::remove_file(&part);
        return Err(io(e));
    }
    Ok(())
}

/// Enables or disables the plugin `file` of `dir` (renames between `.wasm` and `.wasm.disabled`).
pub fn set_enabled(dir: &Path, file: &str, enabled: bool) -> Result<(), PluginError> {
    let on = dir.join(format!("{file}.wasm"));
    let off = dir.join(format!("{file}{DISABLED}"));
    let (from, to) = if enabled { (off, on) } else { (on, off) };
    if !from.exists() {
        return if to.exists() {
            Ok(())
        } else {
            Err(PluginError::Io(file.to_owned(), "not installed".into()))
        };
    }
    std::fs::rename(&from, &to).map_err(|e| PluginError::Io(file.to_owned(), e.to_string()))
}

/// Removes the plugin `file` from `dir`, enabled or disabled.
pub fn uninstall(dir: &Path, file: &str) -> Result<(), PluginError> {
    let mut removed = false;
    for name in [format!("{file}.wasm"), format!("{file}{DISABLED}")] {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| PluginError::Io(file.to_owned(), e.to_string()))?;
            removed = true;
        }
    }
    if removed {
        // A plugin installed again later asks again.
        for p in Permission::ALL {
            let _ = set_granted(dir, file, p, false);
        }
        let _ = revoke_network(dir, file, None, None);
        Ok(())
    } else {
        Err(PluginError::Io(file.to_owned(), "not installed".into()))
    }
}

#[cfg(test)]
mod load_tests {
    // Hand test 27: a text file says it is no WebAssembly module, not the parser's complaint.
    #[test]
    fn a_text_file_is_named_as_no_module() {
        let err = super::Plugin::from_bytes(std::path::Path::new("broken.wasm"), b"hello\n")
            .err()
            .unwrap()
            .to_string();
        assert_eq!(
            err,
            "broken.wasm: not a Ferris IDEA plugin: the file is not a WebAssembly module (it does not start with the \\0asm header)"
        );
    }
}

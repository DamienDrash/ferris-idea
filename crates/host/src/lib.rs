//! The functions of a remote project that run on the host (`SPEC-0318`, M6.12 step 4): the
//! session families `lsp`, `search`, `git`, `term`, `run`, `port`, `dap` and `mcp`, bound into a
//! [`Backend`] of `ferris-idea serve` (over SSH or in process).
//!
//! Every path on the wire is relative to the project folder (behaviour 3); host paths outside it
//! never leave the host, and paths or URIs from the window that leave it are refused before any
//! server, shell or Git sees them (`ADR-0025` T5, SEC-14). Nothing here opens a TCP listener:
//! language servers and debug adapters talk over standard input and output, forwarded targets are
//! reached by connecting on the host (`ADR-0025` §6, SEC-7).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ferris_idea_session::{Backend, Call, ErrorCode, Handler, Right, RpcError};
use serde_json::Value;

pub mod dap;
pub mod git;
pub mod git_ops;
pub mod lsp;
pub mod mcp;
pub mod paths;
pub mod port;
mod ring;
pub mod run;
pub mod search;
pub mod term;

pub use mcp::McpBridge;

/// How the host functions run.
#[derive(Clone)]
pub struct HostOptions {
    /// Further debug adapters by id with their command (tests; `dap.start` takes only known ids).
    pub extra_adapters: Vec<(String, Vec<String>)>,
    /// Further TCP adapters with the argument shape of `dlv dap` (tests).
    pub extra_tcp_adapters: Vec<(String, Vec<String>)>,
    /// The `docker` program for container targets (default: `docker`, then `podman`; tests).
    pub docker_program: Option<String>,
    /// The `kubectl` program for pod targets (default: `kubectl`; tests).
    pub kubectl_program: Option<String>,
    /// How often listening ports and Git state are checked (1 s).
    pub poll: Duration,
    /// Runs the host's MCP tools for `mcp.*` (`ferris-idea serve` passes Ferris IDEA's MCP server).
    pub mcp: Option<Arc<dyn McpBridge>>,
    /// Start the language server pool and follow the documents (off in tests without servers).
    pub language_servers: bool,
    /// The shell of `term.open` (default: `$SHELL` of the host account, else `/bin/sh`).
    pub shell: Option<String>,
    /// The host's trust list that `run.trust` adds to (default: the settings folder's).
    pub trust_file: Option<PathBuf>,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            extra_adapters: Vec::new(),
            extra_tcp_adapters: Vec::new(),
            docker_program: None,
            kubectl_program: None,
            poll: Duration::from_secs(1),
            mcp: None,
            language_servers: true,
            shell: None,
            trust_file: None,
        }
    }
}

/// Binds every family of `SPEC-0318` into `backend`.
pub fn bind(backend: &Backend, options: HostOptions) {
    lsp::bind(backend, &options);
    search::bind(backend);
    git::bind(backend, &options);
    git_ops::bind(backend);
    term::bind(backend, &options);
    run::bind(backend, &options);
    port::bind(backend, &options);
    dap::bind(backend, &options);
    mcp::bind(backend, &options);
}

/// A handler from a closure.
pub(crate) fn handler(
    f: impl Fn(&Call) -> Result<Value, RpcError> + Send + Sync + 'static,
) -> Handler {
    Arc::new(f)
}

/// Registers `method` behind `right`; `audit` for methods that change or run something.
pub(crate) fn add(
    backend: &Backend,
    family: &str,
    method: &str,
    right: Right,
    audit: bool,
    f: impl Fn(&Call) -> Result<Value, RpcError> + Send + Sync + 'static,
) {
    if audit {
        backend.register_audited(family, method, right, handler(f));
    } else {
        backend.register(family, method, right, handler(f));
    }
}

/// `invalid` with `message`.
pub(crate) fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Invalid, message)
}

/// `internal` with `message`.
pub(crate) fn internal(message: impl std::fmt::Display) -> RpcError {
    RpcError::new(ErrorCode::Internal, message.to_string())
}

/// A string parameter.
pub(crate) fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params[key]
        .as_str()
        .ok_or_else(|| invalid(format!("`{key}` is missing")))
}

/// A number parameter.
pub(crate) fn number(params: &Value, key: &str) -> Result<u64, RpcError> {
    params[key]
        .as_u64()
        .ok_or_else(|| invalid(format!("`{key}` is missing or not a number")))
}

/// The backend a call came through.
pub(crate) fn backend_of(call: &Call) -> Result<Backend, RpcError> {
    call.session
        .backend_now()
        .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "the backend is ending"))
}

/// Runs `future` on a runtime of its own (language questions are async).
pub(crate) fn block_on<F: std::future::Future>(future: F) -> Result<F::Output, RpcError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(internal)?;
    Ok(rt.block_on(future))
}

/// The host file of a relative path from the window (refused when it leaves the project).
pub(crate) fn resolve(root: &Path, rel: &str) -> Result<PathBuf, RpcError> {
    ferris_idea_session::paths::resolve(root, rel)
}

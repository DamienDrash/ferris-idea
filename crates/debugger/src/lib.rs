//! Debugging for IDECraft, built on the Debug Adapter Protocol.
//!
//! Implements `SPEC-0039`: base protocol, adapter client, session model and
//! detection of known adapters; Go programs run under Delve (`delve`).

mod adapters;
mod attach;
mod browser;
mod client;
mod delve;
mod expr;
pub mod extras;
mod kinds;
mod kotlin_classes;
mod kotlin_gate;
pub mod lifetime;
mod paths;
mod protocol;
pub mod saved;
mod session;
pub mod values;

pub use adapters::{
    Adapter, JsDebugServer, browser_launch, can_debug_unattended, chrome, delve_binary,
    js_debug_server, known_adapters, kotlin_debug_adapter, lldb_dap, node_launch, strips_types,
};
pub use attach::{
    Process, Runtime, Target, attach, attach_arguments, classify, parse_host_target, processes,
    with_source_map,
};
pub use browser::{BrowserProfile, OLD_PROFILE, OldProfile, ProfileLock, profile_name};
pub use client::{
    AdapterHost, DapClient, DapConnect, ExceptionTypes, FnHost, HostSettings, HostSources,
};
pub use delve::{
    DelveServer, GoCommand, attach_go, go_command, go_launch, is_loopback, launch_go, link_mapping,
};
pub use extras::{Lang, ThreadInfo, ViewAs};
pub use kinds::Kind;
pub use paths::PathMap;
pub use protocol::{frame, read_message, write_message};
pub use session::{
    BLOCK_STEPS, Breakpoint, BreakpointRules, CLIENT_SIDE_NOTE, DebugSession, EXCEPTION_LANGUAGES,
    EvaluatedBy, Event, FieldWatchpoint, Frame, FunctionBreakpoint, HitCondition,
    LanguageExceptions, PendingBreakpoint, SourceBreakpoint, SpecialBreakpoints, StopState,
    Stopped, Suspend, Variable, breakpoint_place, completion_texts, exception_filters,
    exception_language, exception_types, java_method_name, set_variable_supported, step_targets,
    supported, target_method, truth, unbound_hint,
};

/// What went wrong talking to a debug adapter.
#[derive(Debug, thiserror::Error)]
pub enum DapError {
    /// The adapter could not be started.
    #[error("cannot start the debug adapter {0}")]
    Start(String),
    /// Reading or writing failed.
    #[error("debug adapter I/O: {0}")]
    Io(String),
    /// The adapter sent something that is not DAP.
    #[error("debug adapter protocol: {0}")]
    Protocol(String),
    /// A request failed.
    #[error("{command} failed: {message}")]
    Failed {
        /// The request.
        command: String,
        /// The adapter's message.
        message: String,
    },
    /// No answer in time.
    #[error("the debug adapter did not answer in time")]
    Timeout,
    /// The adapter ended.
    #[error("the debug adapter ended")]
    Closed,
}

impl DapError {
    fn io<E: std::fmt::Display>(e: E) -> Self {
        Self::Io(e.to_string())
    }
}

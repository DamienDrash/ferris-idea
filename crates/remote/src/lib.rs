//! Remote hosts for Ferris IDEA (`SPEC-0121`): browse, edit and run on a machine reached by SSH.
//!
//! Everything goes through the system `ssh`, so the user's config, keys, agent and known hosts
//! apply; on the host only POSIX tools are used. Opened files are copied into the local cache and
//! written back on save with a check that nobody changed them on the host meanwhile. A folder can
//! also be opened as a remote project (`SPEC-0317`): `ferris-idea serve` runs on the host and owns
//! the documents; the session protocol (`ferris-idea-session`) runs over the SSH channel. A project
//! window holds it as a [`live::LiveProject`], which reconnects by itself and never lets typing
//! wait (`SPEC-0317` v3).

mod backend;
pub mod bootstrap;
mod config;
mod files;
pub mod host_debug;
pub mod host_run;
pub mod host_term;
pub mod inprocess;
mod jump;
pub mod known_hosts;
mod launch;
pub mod live;
mod mirror;
pub mod own_hosts;
pub mod paired;
pub mod ports;
mod pq;
mod pq_git;
mod project;
mod recent;
pub mod room_link;
pub mod route;
mod ssh;
#[cfg(feature = "test-host")]
mod test_host;
pub mod testing;

pub use bootstrap::{HostFamily, HostPlatform};
pub use host_run::{HostRun, RunTarget, start_host_run};
pub use host_term::{HostTerminal, TermOutput, TermPhase};
pub use known_hosts::{HostKeyCheck, KnownHostsLookup, KnownKey};
pub use live::{
    ConnState, DocState, DocSync, Follow, LiveProject, ProjectLink, Restored, Synced, Timing,
};
pub use paired::PairedProject;
pub use recent::{
    MAX_RECENT, RecentRemote, forget_remote, load_recent_remote, recent_remote_file,
    remember_remote,
};
pub use room_link::{LinkError, RoomLink};
pub use route::{Engine, ExecRoute, HostSpec, Route as HostRoute};

pub use backend::{
    BACKEND_DIR, Backend, Installed, RunStep, backend_path, ensure_backend, fingerprint,
    host_platform, local_platform, run_script, run_steps, same_platform,
};
pub use config::{
    HostInfo, HostSource, config_hosts, config_hosts_in, describe, host_list, ssh_config_file,
};
pub use files::{
    EntryKind, MAX_FILE, NewEntry, RemoteEntry, RemoteFile, check_path, cksum, entry_order,
    protects_home, sort_entries,
};
pub use jump::{
    HOP_MARKER, MAX_HOPS, OWN_PROXY_COMMAND_LOG, OWN_PROXY_COMMAND_SETTING, Route, hop_script,
    jump_proxy_command, own_proxy_command_setting, route_lib, set_own_proxy_command,
};
pub use launch::{SSH_PROGRAM, SshCommand, SshLine, SshPurpose};
pub use mirror::{cache_dir, host_folder, join, mirror_path, parent};
pub use own_hosts::{
    HostFileError, HostFiles, OwnHost, add_host, host_files, remove_host, set_host_files,
};
pub use pq::{
    KEX_POLICY_ENV, KexPolicy, PQ_KEX, PQ_KEX_ML_KEM, kex_refusal, negotiating_peer, usable_kex,
};
pub use pq_git::{
    AZURE_CLASSIC_KEX, AZURE_CLASSIC_SETTING, AZURE_DEVOPS_HOSTS, KEX_MARKER, KexReport,
    azure_classic_kex_setting, azure_exception_active, classic_kex_notice, git_kex,
    git_shim_script, is_azure_devops_host, set_azure_classic_kex, take_kex_reports,
};
pub use project::{
    RemoteProject, ServeOptions, Unsaved, attach_line, checksum_limit, serve_line, sha256_file,
    verify_backend,
};
pub use ssh::{Remote, RemoteError, RemoteOutput, SshTarget, TIMEOUT, classify, quote};

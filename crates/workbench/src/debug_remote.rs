//! Debugging on the host of a remote project (`SPEC-0322`): the part of a Debug session that
//! differs from a local one. The adapter starts through the backend; every command, variable and
//! stop then flows through the normal [`DebugSession`] over the session stream.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use idecraft_debugger::{
    Breakpoint, DebugSession, Kind, PathMap, Runtime, SourceBreakpoint, Target, attach_arguments,
    with_source_map,
};
use idecraft_remote::LiveProject;
use idecraft_remote::host_debug::{HostLink, java_exception_types, java_host, start_host_debug};
use serde_json::{Value, json};

use crate::debug_model::{Launch, Update};

/// Where a remote Debug session runs.
#[derive(Clone)]
pub struct RemoteLaunch {
    /// The project's connection.
    pub live: LiveProject,
    /// The project's stand-in folder in the window.
    pub stand_in: PathBuf,
    /// The host project folder.
    pub root: String,
    /// The `attach` parameter of an attach (`SPEC-0322` behaviour 10), else `None`.
    pub attach: Option<Value>,
    /// The folder in the target named by the user, which replaces detected mappings.
    pub target_root: Option<String>,
}

impl std::fmt::Debug for RemoteLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RemoteLaunch({})", self.root)
    }
}

impl PartialEq for RemoteLaunch {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.attach == other.attach
    }
}

impl Eq for RemoteLaunch {}

/// The placed breakpoints per file of a started session.
type Started = (DebugSession, BTreeMap<PathBuf, Vec<Breakpoint>>);

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "::1" | "localhost")
}

/// Starts the adapter on the host and launches or attaches.
pub fn start(
    launch: &Launch,
    remote: &RemoteLaunch,
    mut arguments: Value,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    send: &dyn Fn(Update),
) -> Result<Started, String> {
    let link = HostLink::Live(remote.live.clone());
    // An attach does not know the host root before it asks (`dap.plan` gave it for a launch).
    let root = if remote.root.is_empty() {
        link.call("dap.adapters", json!({}), Duration::from_secs(30))
            .ok()
            .and_then(|a| a["root"].as_str().map(str::to_owned))
            .ok_or("the host named no project folder")?
    } else {
        remote.root.clone()
    };
    let paths = Some(PathMap::new(&remote.stand_in, &root));
    let attaching = remote.attach.is_some();
    let runtime = launch.kind.runtime();
    let adapter = match (attaching, runtime) {
        (true, Some(r)) => r.adapter_name(),
        _ => launch.kind.adapter_name(),
    };
    let mut params = json!({ "mode": if attaching { "attach" } else { "launch" } });
    if let Some(attach) = &remote.attach {
        params["attach"] = attach.clone();
    }
    if !attaching && launch.kind == Kind::Java {
        params["main_class"] = json!(launch.program.to_string_lossy());
    }
    if !launch.prepare.is_empty() {
        send(Update::Output(format!("$ {}\n", launch.prepare)));
        params["prepare"] = json!(launch.prepare);
    }
    let debug = start_host_debug(&link, adapter, params, paths)
        .map_err(|e| format!("{}: {}", e.code.as_str(), e.message))?;
    if let Some(text) = debug.adapter.prepared.as_deref().filter(|t| !t.is_empty()) {
        send(Update::Output(text.to_owned()));
    }
    let info = debug.adapter;
    let mut client = debug.client;
    let id = launch_id(launch);
    if launch.kind == Kind::Java {
        client = client
            .with_exception_types(java_exception_types(&link, &info.session))
            .with_host(Box::new(java_host(&link, &info.session, &remote.stand_in)));
    }
    if attaching {
        let runtime = runtime.ok_or("this kind of program is not attached to")?;
        let target = match (&info.address, info.pid) {
            (Some((host, port)), _) => {
                if !is_loopback(host) {
                    send(Update::Output(format!(
                        "[IDECraft] The traffic between the host and {host}:{port} is not protected by IDECraft.\n"
                    )));
                }
                Target::Address {
                    host: host.clone(),
                    port: *port,
                }
            }
            (None, Some(pid)) => Target::Pid(pid),
            (None, None) => return Err("the host named no attach target".into()),
        };
        let mut arguments = attach_arguments(runtime, &target, &PathBuf::from(&root))?;
        let mappings = match &remote.target_root {
            Some(folder) => vec![(root.clone(), folder.clone())],
            None => info.mappings.clone(),
        };
        with_source_map(runtime, &mut arguments, &mappings);
        return if runtime == Runtime::Node {
            DebugSession::launch_js_on(
                debug.connect,
                "attach",
                arguments,
                breakpoints,
                &launch.special,
                Box::new(client),
            )
        } else {
            DebugSession::attach_on(client, &id, arguments, breakpoints, &launch.special)
        }
        .map_err(|e| format!("cannot attach: {e}"));
    }
    if launch.kind == Kind::Java
        && let Some(c) = &info.classpath
    {
        arguments["modulePaths"] = c["modules"].clone();
        arguments["classPaths"] = c["classes"].clone();
        if !c["project"].is_null() {
            arguments["projectName"] = c["project"].clone();
        }
    }
    if matches!(launch.kind, Kind::Node | Kind::PackageScript) {
        DebugSession::launch_js_on(
            debug.connect,
            "launch",
            arguments,
            breakpoints,
            &launch.special,
            Box::new(client),
        )
    } else {
        DebugSession::launch_on(client, &id, arguments, breakpoints, &launch.special)
    }
    .map_err(|e| e.to_string())
}

fn launch_id(launch: &Launch) -> String {
    match launch.kind {
        Kind::Java => "java".into(),
        k => k.adapter_name().to_owned(),
    }
}

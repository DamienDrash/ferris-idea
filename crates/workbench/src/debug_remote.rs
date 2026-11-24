//! Debugging on the host of a remote project (`SPEC-0322`): the part of a Debug session that
//! differs from a local one. The adapter starts through the backend; every command, variable and
//! stop then flows through the normal [`DebugSession`] over the session stream.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ferris_idea_debugger::{
    Breakpoint, DebugSession, Kind, PathMap, Runtime, SourceBreakpoint, Target, attach_arguments,
    with_source_map,
};
use ferris_idea_remote::LiveProject;
use ferris_idea_remote::host_debug::{HostLink, java_exception_types, java_host, start_host_debug};
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

/// What the Debug console says before an attach to `host:port` from the host (`SPEC-0322`
/// behaviour 11): an address outside the host's loopback is not protected by Ferris IDEA.
fn address_note(host: &str, port: u16) -> Option<String> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    (!ferris_idea_debugger::is_loopback(bare)).then(|| {
        format!(
            "[Ferris IDEA] The traffic between the host and {host}:{port} is not protected by Ferris IDEA.\n"
        )
    })
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
                if let Some(note) = address_note(host, *port) {
                    send(Update::Output(note));
                }
                // What Stop does to the program is the Delve server's doing, so it is said.
                if runtime == Runtime::Go {
                    send(Update::Output(
                        ferris_idea_debugger::DELVE_SERVER_NOTE.to_owned(),
                    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0322 behaviour 11 (AK-14): only an address outside the host's loopback gets the note.
    #[test]
    fn an_address_outside_the_hosts_loopback_is_said_to_be_unprotected() {
        for host in ["127.0.0.1", "127.0.0.2", "::1", "[::1]", "localhost"] {
            assert_eq!(address_note(host, 5005), None, "{host}");
        }
        let note = address_note("10.0.0.7", 5005).unwrap();
        assert!(
            note.contains("10.0.0.7:5005") && note.contains("not protected"),
            "{note}"
        );
        assert!(address_note("build.example", 2345).is_some());
    }
}

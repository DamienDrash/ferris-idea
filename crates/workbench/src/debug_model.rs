//! The debug session behind the Debug tool window (`SPEC-0091`): a thread owning the adapter
//! session that takes commands and reports what happened, and the variables tree.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use idecraft_debugger::{Breakpoint, DebugSession, Frame, SourceBreakpoint, StopState, Variable};
use serde_json::json;

pub use idecraft_debugger::saved::*;

/// Frames fetched when the program stops.
const FRAMES: usize = 50;

/// How long the text of the frame's object may take (`SPEC-0091` v9 behaviour 14).
const OBJECT_TEXT: Duration = Duration::from_secs(1);

/// Longest text of the frame's object, in characters.
const OBJECT_TEXT_CHARS: usize = 100;

/// The frame's object, its fields, the fields the paused function uses and the object's text
/// (`SPEC-0091` v8 behaviour 13, v9 behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectFields {
    /// The frame they belong to.
    pub frame: i64,
    /// The object's variables reference.
    pub owner: i64,
    /// All its fields.
    pub fields: Vec<Variable>,
    /// Names of the fields the function's code names, in the object's order.
    pub used: Vec<String>,
    /// The object's value with its text, when the adapter gave one in time.
    pub text: Option<String>,
}

/// What the window asks of the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Resume (`Resume`).
    Continue,
    /// Step over the line (`StepOver`).
    StepOver,
    /// Step into the call (`StepInto`).
    StepInto,
    /// Step out of the function (`StepOut`).
    StepOut,
    /// End the program and the adapter (`Stop`).
    Stop,
    /// Replace the line breakpoints of a file (1-based lines, with their conditions).
    Breakpoints(PathBuf, Vec<SourceBreakpoint>),
    /// Replace the exception and method breakpoints (`SPEC-0146`).
    Special(idecraft_debugger::SpecialBreakpoints),
    /// Children of a structured variable.
    Children(i64),
    /// Local variables of another frame.
    Frame(i64),
    /// Evaluate an expression in a frame (`EvaluateExpression`, `SPEC-0091` v3 behaviour 8).
    Evaluate(String, i64),
    /// Evaluate the watches in a frame (`SPEC-0091` v5 behaviour 11).
    Watches(Vec<String>, i64),
    /// Show the call stack of another thread; stepping then acts on it (v5 behaviour 12).
    Thread(i64),
    /// Pause the running program (`Pause`, `SPEC-0194` behaviour 1).
    Pause,
    /// Run to this 1-based line of this file (`RunToCursor`, `SPEC-0194` behaviour 2).
    RunToLine(PathBuf, u64),
    /// Change a variable (`XDebugger.SetValue`, `SPEC-0194` behaviour 3): frame, container (0 for
    /// the frame's locals), name and new value.
    SetValue(i64, i64, String, String),
    /// List the calls on the line of this frame for Smart Step Into (`SmartStepInto`, `SPEC-0091`
    /// v11 behaviour 18).
    StepTargets(i64),
    /// Step into the call with this id from [`Command::StepTargets`].
    StepIntoTarget(i64),
    /// Run this frame again from its start (`Debugger.PopFrame` "Reset Frame", v11 behaviour 19).
    ResetFrame(i64),
    /// Step over without stopping at breakpoints on the way (`ForceStepOver`, v11 behaviour 20).
    ForceStepOver,
    /// Run to this 1-based line of this file without stopping at breakpoints on the way
    /// (`ForceRunToCursor`, v11 behaviour 20).
    ForceRunToLine(PathBuf, u64),
    /// Completions for the text with the caret at this 1-based column in this frame (v11
    /// behaviour 22).
    Completions(String, usize, i64),
    /// Step Out of Code Block (`StepOutOfBlock`, v12 behaviour 25).
    StepOutOfBlock,
    /// Resume All Threads (v12 behaviour 27).
    ResumeAll,
    /// Pause All: every thread, also the others while one is paused (v12 behaviour 27).
    PauseAll,
    /// Force Return in this frame (v12 behaviour 29; refused with a reason).
    ForceReturn(i64),
    /// Throw Exception in this frame (v12 behaviour 29; refused with a reason).
    ThrowException(i64),
    /// The threads with state and group (`false`), or a thread dump into the console (`true`),
    /// read in this frame (v12 behaviours 26, 28).
    Threads(bool, i64),
    /// Evaluates the expression for an Inspect tab or a referring-objects tab (`true`) under this
    /// key, in this frame (v12 behaviours 30, 31).
    Inspect(String, String, bool, i64),
    /// Values for the views and renderers of rows: the row (container, name) and the expression,
    /// in this frame (v12 behaviours 32, 33).
    Render(Vec<((i64, String), String)>, i64),
    /// Evaluates a code fragment in this frame (v12 behaviour 36).
    Fragment(String, i64),
    /// Runs the console's input in this frame, DAP context `repl` (v12 behaviour 38).
    Repl(String, i64),
    /// Debug settings for the adapter's host (java-debug `showStaticVariables`; v12 behaviour 34).
    Settings(serde_json::Value),
    /// The data breakpoint id of field `name` in this container, for a field watchpoint
    /// (`SPEC-0146` v3).
    FieldWatch(i64, String),
    /// These sources changed: build them for hot swap (v12 behaviour 37).
    SourcesChanged(Vec<PathBuf>),
    /// Reload the changed classes (`Hotswap`, v12 behaviour 37).
    ReloadClasses,
}

/// What the session reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// The program runs (after the start or a resume).
    Running,
    /// The program stopped: why, the call stack and the locals of the top frame.
    Stopped {
        /// `breakpoint`, `step`, …
        reason: String,
        /// Call stack, top first.
        frames: Vec<Frame>,
        /// Local variables of the top frame.
        locals: Vec<Variable>,
    },
    /// The program ended (exit code if reported) or was stopped.
    Exited(Option<i64>),
    /// The debugger detached from a program it had attached to; the program goes on
    /// (`SPEC-0147` behaviour 5).
    Detached,
    /// More program output.
    Output(String),
    /// Children of a variable.
    Children(i64, Vec<Variable>),
    /// Locals of a frame.
    Locals(i64, Vec<Variable>),
    /// New values of a frame's locals after a field changed; the tree keeps its shape.
    Values(i64, Vec<Variable>),
    /// Breakpoints as the adapter placed them in a file.
    Placed(PathBuf, Vec<Breakpoint>),
    /// The value of an expression, or why it has none.
    Evaluated(String, Result<String, String>),
    /// The values of the watches, in order.
    Watched(Vec<(String, Result<String, String>)>),
    /// The threads of the paused program and the one shown.
    Threads(Vec<(i64, String)>, i64),
    /// The call stack and top-frame locals of the thread now shown.
    Frames(Vec<Frame>, Vec<Variable>),
    /// A variable's new value after Set Value, or why it was not changed (`SPEC-0194`).
    ValueSet(String, Result<String, String>),
    /// The fields of the shown frame's object (`this`/`self`; `SPEC-0091` v8 behaviour 13, v9
    /// behaviour 14).
    Fields(ObjectFields),
    /// The calls Smart Step Into can enter, id and label (`SPEC-0091` v11 behaviour 18).
    StepTargets(Vec<(i64, String)>),
    /// Completions for the expression text they were asked for (v11 behaviour 22).
    Completions(String, Vec<String>),
    /// The threads with state and group (v12 behaviour 28).
    ThreadInfos(Vec<idecraft_debugger::ThreadInfo>),
    /// A thread dump for the console (v12 behaviour 26).
    Dump(String),
    /// The value of an Inspect or referring-objects tab under its key (v12 behaviours 30, 31).
    Inspected(String, Result<Variable, String>),
    /// Values for views and renderers of rows (v12 behaviours 32, 33).
    Rendered(Vec<((i64, String), Result<Variable, String>)>),
    /// A breakpoint removed after its hit: file and 1-based line (`SPEC-0127` v3 behaviour 9).
    Removed(PathBuf, u64),
    /// Changed sources were built; their classes can be reloaded (v12 behaviour 37).
    CodeChanged(Vec<PathBuf>),
    /// The reloaded classes, or why nothing was reloaded.
    Reloaded(Result<Vec<String>, String>),
    /// The data breakpoint id of a field, or why it cannot be watched (`SPEC-0146` v3).
    FieldWatch(Result<String, String>),
    /// The session could not start or broke.
    Failed(String),
}

pub use idecraft_debugger::Kind;
pub use idecraft_languages::debug_plan::{plan, plan_run_config, program_of};

/// What to debug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Native program or Python script.
    pub kind: Kind,
    /// The debug adapter command.
    pub adapter: Vec<String>,
    /// The Python interpreter that runs a script (`None`: the adapter's own).
    pub interpreter: Option<PathBuf>,
    /// Program to run.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Extra environment.
    pub env: BTreeMap<String, String>,
    /// Line breakpoints per file (1-based, with their conditions).
    pub breakpoints: BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    /// Shell command that builds the program first (Kotlin); empty for none.
    pub prepare: String,
    /// Exception and method breakpoints (`SPEC-0146`).
    pub special: idecraft_debugger::SpecialBreakpoints,
    /// Attach to this running program instead of starting `program` (`SPEC-0147`).
    pub attach: Option<idecraft_debugger::Target>,
    /// The adapter and the program run on the host of a remote project (`SPEC-0322`); `program`,
    /// `cwd`, `interpreter` are then host paths.
    pub remote: Option<crate::debug_remote::RemoteLaunch>,
    /// Chrome's profile folder for a page (`SPEC-0133` v3 behaviour 2): one per project, outside
    /// it; `None` for everything else.
    pub browser: Option<idecraft_debugger::BrowserProfile>,
}

/// How long a page session waits for the previous session on the same profile to close its
/// browser (Rerun, `SPEC-0133` v3 behaviour 2).
const PROFILE_WAIT: Duration = Duration::from_secs(15);

/// A running session: send commands, take updates.
pub struct Controller {
    commands: Sender<Command>,
    updates: Receiver<Update>,
}

impl Controller {
    /// Starts the adapter and the program on a thread of their own.
    pub fn start(launch: Launch) -> Self {
        let (commands, command_rx) = channel();
        let (update_tx, updates) = channel();
        std::thread::Builder::new()
            .name("idecraft-debug".into())
            .spawn(move || run(launch, &command_rx, &update_tx))
            .expect("a debug session needs a thread");
        Self { commands, updates }
    }

    /// Sends a command; ignored once the session has ended.
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// The updates since the last call.
    pub fn poll(&self) -> Vec<Update> {
        self.updates.try_iter().collect()
    }
}

/// Starts jdtls with java-debug, resolves the class path of the main class and launches it.
fn java_session(
    launch: &Launch,
    mut arguments: serde_json::Value,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
) -> Result<(DebugSession, BTreeMap<PathBuf, Vec<Breakpoint>>), String> {
    let server = idecraft_languages::java_debug::JavaDebugServer::start(&launch.cwd)?;
    // Attaching needs no class path: the JVM already runs (SPEC-0147 behaviour 3).
    if let Some(target) = &launch.attach {
        // java-debug evaluates nothing without the project's name: no expression, no breakpoint
        // condition, no log message (SPEC-0147 v2 behaviour 3a).
        let sources: Vec<PathBuf> = breakpoints.keys().cloned().collect();
        if arguments["projectName"].as_str().is_none_or(str::is_empty)
            && let Some(project) = server.project_of_sources(&sources, Duration::from_secs(20))
        {
            arguments["projectName"] = json!(project);
        }
        let client = java_client(server)?;
        return DebugSession::attach_on(client, "java", arguments, breakpoints, &launch.special)
            .map_err(|e| format!("cannot attach to {target}: {e}"));
    }
    let class = launch.program.to_string_lossy().into_owned();
    // A single source file joins jdtls's workspace only once it is opened.
    let simple = class.rsplit('.').next().unwrap_or(&class);
    let relative = format!("{}.java", class.replace('.', "/"));
    // Also below the inferred source roots of a folder without a build file (`java/src/main/java`):
    // the debugger's jdtls has its own data folder and creates the folder's project only for an
    // opened file (SPEC-0097 v2).
    let candidates: Vec<PathBuf> = [
        launch.cwd.join(&relative),
        launch.cwd.join(format!("{simple}.java")),
    ]
    .into_iter()
    .chain(
        idecraft_languages::java_roots::source_roots(&launch.cwd)
            .iter()
            .map(|r| launch.cwd.join(r).join(&relative)),
    )
    .collect();
    if let Some(file) = candidates.iter().find(|f| f.is_file()) {
        server.open(file)?;
    }
    let (modules, classes) = server.classpath(&class, Duration::from_secs(120))?;
    // The classes are built before the JVM looks for them (SPEC-0091 v13).
    let _ = server.build();
    arguments["modulePaths"] = json!(modules);
    arguments["classPaths"] = json!(classes);
    if let Some(project) = server.project_name(&class) {
        arguments["projectName"] = json!(project);
    }
    let client = java_client(server)?;
    DebugSession::launch_on(client, "java", arguments, breakpoints, &launch.special)
        .map_err(|e| e.to_string())
}

/// The adapter of `server`, kept alive with the connection; exception types go to jdtls's debug
/// settings (D30).
fn java_client(
    server: idecraft_languages::java_debug::JavaDebugServer,
) -> Result<idecraft_debugger::DapClient, String> {
    let address = server.dap_address();
    let server = std::sync::Arc::new(std::sync::Mutex::new(server));
    let settings = server.clone();
    let (host, sources) = (server.clone(), server.clone());
    Ok(
        idecraft_debugger::DapClient::connect(&address, Some(Box::new(server)))
            .map_err(|e| e.to_string())?
            .with_exception_types(Box::new(move |types| {
                settings
                    .lock()
                    .map_err(|_| "jdtls is gone".to_owned())?
                    .set_exception_types(types)
            }))
            // Debug settings and the build of changed sources for hot swap (SPEC-0091 v12).
            .with_host(Box::new(idecraft_debugger::FnHost {
                settings: Box::new(move |s| {
                    host.lock()
                        .map_err(|_| "jdtls is gone".to_owned())?
                        .update_settings(s)
                }),
                sources: Box::new(move |files| {
                    sources
                        .lock()
                        .map_err(|_| "jdtls is gone".to_owned())?
                        .sources_changed(files)
                }),
            })),
    )
}

/// The local variables of frame `id`: the first scope named like "Locals", else the first scope.
fn top_locals(
    session: &mut DebugSession,
    id: i64,
) -> Result<Vec<Variable>, idecraft_debugger::DapError> {
    let scopes = session.scopes(id)?;
    let (locals, mut extra) = frame_scopes(&scopes);
    let mut vars = match locals {
        Some(r) => session.variables(r)?,
        None => Vec::new(),
    };
    vars.append(&mut extra);
    Ok(vars)
}

/// The scope whose variables are the frame's locals (the first named like "Locals", else the
/// first), and the closure and block scopes after them as rows that open (hand test 73: a Node.js
/// callback's captured `n` was not shown).
pub fn frame_scopes(scopes: &[(String, i64)]) -> (Option<i64>, Vec<Variable>) {
    let locals = scopes
        .iter()
        .position(|(n, _)| n.to_lowercase().starts_with("local"))
        .or((!scopes.is_empty()).then_some(0));
    let extra = scopes
        .iter()
        .enumerate()
        .filter(|(i, (n, r))| {
            let n = n.to_lowercase();
            Some(*i) != locals && *r != 0 && (n.starts_with("closure") || n.starts_with("block"))
        })
        .map(|(_, (name, r))| Variable {
            name: name.clone(),
            value: String::new(),
            kind: None,
            reference: *r,
            evaluate_name: None,
        })
        .collect();
    (locals.map(|i| scopes[i].1), extra)
}

/// Ends `session` on Stop: a started program ends, an attached one is detached and goes on.
fn end(session: DebugSession) -> Update {
    let attached = session.attached();
    let _ = session.disconnect();
    if attached {
        Update::Detached
    } else {
        Update::Exited(None)
    }
}

/// Takes the page session's profile folder, waiting for a session still closing its browser, and
/// prepares it: mode 0700, a profile an earlier version kept in the project moved out or named in
/// the console (`SPEC-0133` v3 behaviour 2).
fn browser_profile(
    launch: &Launch,
    send: &dyn Fn(Update),
) -> Result<idecraft_debugger::ProfileLock, String> {
    let profile = launch
        .browser
        .as_ref()
        .ok_or("no folder for the browser profile (no home folder)")?;
    let lock = idecraft_debugger::ProfileLock::acquire(&profile.folder, PROFILE_WAIT)
        .ok_or("the browser of the previous session of this project is still closing")?;
    let old = profile
        .prepare()
        .map_err(|e| format!("the browser profile folder: {e}"))?;
    if let Some(note) = profile.note(&old) {
        send(Update::Output(note));
    }
    Ok(lock)
}

fn run(launch: Launch, commands: &Receiver<Command>, updates: &Sender<Update>) {
    let send = |u: Update| {
        let _ = updates.send(u);
    };
    let arguments = arguments(&launch);
    let mut breakpoints = launch.breakpoints.clone();
    // kotlin-debug-adapter maps breakpoints to classes of its project; files of other programs
    // keep it from stopping at all (SPEC-0132 behaviour 2).
    if launch.kind == Kind::Kotlin && launch.remote.is_none() {
        breakpoints.retain(|path, _| {
            path.starts_with(&launch.cwd)
                && path.extension().is_some_and(|e| e == "kt" || e == "java")
        });
    }
    // Starting a program of an untrusted project runs its code: not in safe mode (SPEC-0278
    // behaviour 5). Attaching to a running program does not.
    if launch.attach.is_none()
        && launch.remote.is_none()
        && let Err(why) = idecraft_core::trust::require_trusted(&launch.cwd)
    {
        send(Update::Failed(why));
        return;
    }
    // A page: one browser per profile folder, so Rerun's new session waits until the old one has
    // closed its browser; held until this session and its browser have ended (SPEC-0133 v3
    // behaviour 2).
    let _profile = if launch.kind == Kind::Browser {
        match browser_profile(&launch, &send) {
            Ok(lock) => Some(lock),
            Err(why) => {
                send(Update::Failed(why));
                return;
            }
        }
    } else {
        None
    };
    // Go: what Delve cannot debug (`go build`, several packages) is said before anything starts
    // (SPEC-0039 v7 behaviour 11).
    if launch.kind == Kind::Go
        && launch.attach.is_none()
        && let Err(why) = idecraft_debugger::go_command(&launch.args)
    {
        send(Update::Failed(why));
        return;
    }
    let adapter_id = launch.kind.adapter_name();
    // Kotlin: the build tool compiles first; the adapter does not (SPEC-0132 behaviour 2).
    if !launch.prepare.is_empty() && launch.remote.is_none() {
        send(Update::Output(format!("$ {}\n", launch.prepare)));
        let built = std::process::Command::new("sh")
            .args(["-c", &launch.prepare])
            .current_dir(&launch.cwd)
            .envs(&launch.env)
            .output();
        match built {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout).into_owned()
                    + &String::from_utf8_lossy(&o.stderr);
                if !text.is_empty() {
                    send(Update::Output(text));
                }
                if !o.status.success() {
                    send(Update::Failed(
                        "the build failed; nothing was debugged".into(),
                    ));
                    return;
                }
            }
            Err(e) => {
                send(Update::Failed(format!("the build could not start: {e}")));
                return;
            }
        }
    }
    let attach_runtime = launch.attach.as_ref().map(|_| launch.kind.runtime());
    let started = if let Some(remote) = &launch.remote {
        crate::debug_remote::start(&launch, remote, arguments.clone(), &breakpoints, &send)
    } else if let Some(target) = &launch.attach
        && let Some(Some(runtime)) = attach_runtime
        && runtime != idecraft_debugger::Runtime::Java
    {
        idecraft_debugger::attach(
            runtime,
            target,
            &launch.adapter,
            &launch.cwd,
            &breakpoints,
            &launch.special,
        )
    } else if let Some(Some(idecraft_debugger::Runtime::Java)) = attach_runtime {
        match launch.attach.as_ref().map(|t| {
            idecraft_debugger::attach_arguments(idecraft_debugger::Runtime::Java, t, &launch.cwd)
        }) {
            Some(Ok(arguments)) => java_session(&launch, arguments, &breakpoints),
            Some(Err(e)) => Err(e),
            None => Err("nothing to attach to".into()),
        }
    } else if launch.attach.is_some() {
        Err("pages are not attached to; debug the page's run configuration".into())
    } else if launch.kind == Kind::Java {
        java_session(&launch, arguments, &breakpoints)
    } else if matches!(
        launch.kind,
        Kind::Node | Kind::PackageScript | Kind::Browser
    ) {
        idecraft_debugger::JsDebugServer::start(&launch.adapter)
            .and_then(|server| {
                let address = server.address();
                DebugSession::launch_js(
                    &address,
                    "launch",
                    arguments,
                    &breakpoints,
                    &launch.special,
                    Box::new(server),
                )
            })
            .map_err(|e| e.to_string())
    } else if launch.kind == Kind::Go {
        // Delve serves the session on its own socket and ends with it.
        idecraft_debugger::launch_go(&launch.adapter, arguments, &breakpoints, &launch.special)
            .map_err(|e| e.to_string())
    } else {
        DebugSession::launch(
            &launch.adapter,
            adapter_id,
            arguments,
            &breakpoints,
            &launch.special,
        )
        .map_err(|e| e.to_string())
    };
    let (mut session, placed) = match started {
        Ok(s) => s,
        Err(e) => {
            send(Update::Failed(e.to_string()));
            return;
        }
    };
    for (path, list) in placed {
        send(Update::Placed(path, list));
    }
    send(Update::Running);
    let mut sent = 0;
    // Program output and the debug console (logpoints, the note that IDECraft evaluates
    // conditions, SPEC-0178 behaviour 6) go to the same console, in the order they came (D17).
    let mut flush = |session: &mut DebugSession| {
        let all = session.transcript();
        if all.len() > sent {
            send(Update::Output(all[sent..].to_owned()));
            sent = all.len();
        }
    };
    let mut thread = 0;
    let mut running = true;
    // The call stack shown, for the fields of another frame's object.
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        // Breakpoints removed after their hit (SPEC-0127 v3 behaviour 9).
        for (path, line) in session.take_removed() {
            send(Update::Removed(path, line));
        }
        // Breakpoints the adapter bound or unbound since (SPEC-0127 v5 behaviour 14).
        for (path, list) in session.take_rebound() {
            send(Update::Placed(path, list));
        }
        if running {
            match session.wait_state(Duration::from_millis(200), FRAMES) {
                Ok(StopState::Running) => {}
                Ok(StopState::Stopped {
                    reason,
                    thread_id,
                    frames,
                    locals,
                    ..
                }) => {
                    flush(&mut session);
                    thread = thread_id;
                    running = false;
                    // A stop on an exception names it (SPEC-0146 behaviour 5).
                    let reason = match (reason == "exception")
                        .then(|| session.exception_info(thread_id))
                        .flatten()
                    {
                        Some(info) => format!("exception: {info}"),
                        None => reason,
                    };
                    stack = frames.clone();
                    let object = locals.clone();
                    send(Update::Stopped {
                        reason,
                        frames,
                        locals,
                    });
                    if let Some(top) = stack.first() {
                        send_fields(&mut session, launch.kind, top, &object, &send);
                    }
                    if let Ok(threads) = session.threads() {
                        send(Update::Threads(threads, thread));
                    }
                }
                Ok(StopState::Exited { exit_code, .. }) => {
                    flush(&mut session);
                    send(Update::Exited(exit_code));
                    let _ = session.disconnect();
                    return;
                }
                Err(e) => {
                    flush(&mut session);
                    send(Update::Failed(e.to_string()));
                    let _ = session.disconnect();
                    return;
                }
            }
            // Just stopped: what was sent since the stop was reported (Resume pressed at once)
            // is for the stopped program and must not be dropped below (SPEC-0132 v6
            // behaviour 4d).
            if !running {
                continue;
            }
            flush(&mut session);
            // While running only Stop and breakpoint changes apply.
            loop {
                match commands.try_recv() {
                    Ok(Command::Stop) | Err(TryRecvError::Disconnected) => {
                        send(end(session));
                        return;
                    }
                    Ok(Command::Breakpoints(path, lines)) => {
                        if let Ok(list) = session.set_breakpoints(&path, &lines) {
                            send(Update::Placed(path, list));
                        }
                    }
                    Ok(Command::Special(special)) => {
                        if let Err(e) = session.set_special_breakpoints(&special) {
                            send(Update::Output(format!("[IDECraft] {e}\n")));
                        }
                    }
                    Ok(Command::Pause) => {
                        if let Err(e) = session.pause(thread) {
                            send(Update::Output(format!("[IDECraft] Pause: {e}\n")));
                        }
                    }
                    // SPEC-0091 v12 behaviour 27.
                    Ok(Command::PauseAll) => {
                        if let Err(e) = session.pause_all(thread, false) {
                            send(Update::Output(format!("[IDECraft] Pause All: {e}\n")));
                        }
                    }
                    // Hot swap works while the program runs (v12 behaviour 37).
                    Ok(Command::SourcesChanged(files)) => {
                        send(build_changed(&mut session, files));
                    }
                    Ok(Command::ReloadClasses) => {
                        send(Update::Reloaded(
                            session
                                .reload_classes()
                                .map_err(|e| error_summary(&e.to_string())),
                        ));
                    }
                    Ok(Command::Settings(settings)) => {
                        let _ = session.update_settings(&settings);
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => break,
                }
            }
            continue;
        }
        let Ok(command) = commands.recv() else {
            let _ = session.disconnect();
            return;
        };
        let resumed = match command {
            Command::Continue => session.resume_continue(thread),
            Command::StepOver => session.next(thread),
            Command::ForceStepOver => session.force_next(thread),
            // The session steps until the chosen call is entered and returns stopped there.
            Command::StepIntoTarget(target) => {
                send(Update::Running);
                let done = session.step_in_target(thread, target);
                flush(&mut session);
                running = report_steps(
                    &mut session,
                    launch.kind,
                    thread,
                    done,
                    "Smart Step Into",
                    &mut stack,
                    &send,
                );
                continue;
            }
            // Steps until the block is left (SPEC-0091 v12 behaviour 25).
            Command::StepOutOfBlock => {
                send(Update::Running);
                let done = session.step_out_of_block(thread);
                flush(&mut session);
                running = report_steps(
                    &mut session,
                    launch.kind,
                    thread,
                    done,
                    "Step Out of Code Block",
                    &mut stack,
                    &send,
                );
                continue;
            }
            Command::ResumeAll => session.resume_all(thread),
            Command::PauseAll => {
                if let Err(e) = session.pause_all(thread, true) {
                    send(Update::Output(format!("[IDECraft] Pause All: {e}\n")));
                }
                if let Ok(threads) = session.threads() {
                    send(Update::Threads(threads, thread));
                }
                continue;
            }
            Command::ForceReturn(frame) => {
                if let Err(e) = session.force_return(frame) {
                    send(Update::Output(format!(
                        "[IDECraft] Force Return: {}\n",
                        error_summary(&e.to_string())
                    )));
                }
                continue;
            }
            Command::ThrowException(frame) => {
                if let Err(e) = session.throw_exception(frame, "") {
                    send(Update::Output(format!(
                        "[IDECraft] Throw Exception: {}\n",
                        error_summary(&e.to_string())
                    )));
                }
                continue;
            }
            Command::Threads(dump, frame) => {
                let frames = if dump { FRAMES } else { 0 };
                match session.thread_dump(Some(frame), frames) {
                    Ok(threads) if dump => send(Update::Dump(dump_text(&threads))),
                    Ok(threads) => {
                        send(Update::ThreadInfos(
                            threads.into_iter().map(|(t, _)| t).collect(),
                        ));
                    }
                    Err(e) => send(Update::Output(format!(
                        "[IDECraft] Threads: {}\n",
                        error_summary(&e.to_string())
                    ))),
                }
                continue;
            }
            Command::Inspect(key, expression, referring, frame) => {
                let value = if referring {
                    session.referring_objects(&expression, frame)
                } else {
                    session.evaluate_value(&expression, frame, "watch")
                };
                send(Update::Inspected(
                    key,
                    value.map_err(|e| error_summary(&e.to_string())),
                ));
                continue;
            }
            Command::Render(rows, frame) => {
                let values = rows
                    .into_iter()
                    .map(|(key, expression)| {
                        let v = session
                            .evaluate_value(&expression, frame, "watch")
                            .map_err(|e| error_summary(&e.to_string()));
                        (key, v)
                    })
                    .collect();
                send(Update::Rendered(values));
                continue;
            }
            Command::Fragment(code, frame) => {
                let result = session
                    .evaluate_fragment(&code, frame)
                    .map_err(|e| error_summary(&e.to_string()));
                send(Update::Evaluated(code, result));
                continue;
            }
            Command::Repl(text, frame) => {
                send(Update::Output(format!(">>> {text}\n")));
                let result = session.evaluate_value(&text, frame, "repl");
                // Output the input printed comes first (D17's order).
                flush(&mut session);
                match result {
                    Ok(v) if v.value.is_empty() => {}
                    Ok(v) => send(Update::Output(format!("{}\n", v.value))),
                    Err(e) => send(Update::Output(format!(
                        "{}\n",
                        error_summary(&e.to_string())
                    ))),
                }
                continue;
            }
            Command::Settings(settings) => {
                if let Err(e) = session.update_settings(&settings) {
                    send(Update::Output(format!(
                        "[IDECraft] {}\n",
                        error_summary(&e.to_string())
                    )));
                }
                continue;
            }
            Command::FieldWatch(reference, name) => {
                send(Update::FieldWatch(
                    session
                        .field_watch_id(reference, &name)
                        .map_err(|e| error_summary(&e.to_string())),
                ));
                continue;
            }
            Command::SourcesChanged(files) => {
                send(build_changed(&mut session, files));
                continue;
            }
            Command::ReloadClasses => {
                send(Update::Reloaded(
                    session
                        .reload_classes()
                        .map_err(|e| error_summary(&e.to_string())),
                ));
                continue;
            }
            Command::ResetFrame(frame) => match session.restart_frame(frame) {
                Ok(()) => Ok(()),
                Err(e) => {
                    send(Update::Output(format!(
                        "[IDECraft] Reset Frame: {}\n",
                        error_summary(&e.to_string())
                    )));
                    continue;
                }
            },
            Command::StepTargets(frame) => {
                let line = stack
                    .iter()
                    .find(|f| f.id == frame)
                    .and_then(|f| {
                        let text = std::fs::read_to_string(f.path.as_ref()?).ok()?;
                        let at = usize::try_from(f.line - 1).ok()?;
                        text.lines().nth(at).map(str::to_owned)
                    })
                    .unwrap_or_default();
                match session
                    .step_in_targets(frame)
                    .map(|t| targets_on_line(t, &line))
                {
                    // A line with one call or none is a plain Step Into, as in the reference.
                    Ok(targets) if targets.len() < 2 => match session.step_in(thread) {
                        Ok(()) => {
                            running = true;
                            send(Update::Running);
                        }
                        Err(e) => send(Update::Failed(e.to_string())),
                    },
                    Ok(targets) => send(Update::StepTargets(targets)),
                    Err(e) => send(Update::Output(format!(
                        "[IDECraft] Smart Step Into: {}\n",
                        error_summary(&e.to_string())
                    ))),
                }
                continue;
            }
            Command::Completions(text, column, frame) => {
                // An adapter without completion offers none (behaviour 22).
                let items = session
                    .completions(frame, &text, column)
                    .unwrap_or_default();
                send(Update::Completions(text, items));
                continue;
            }
            Command::ForceRunToLine(path, line) => {
                match session.force_run_to_line(thread, &path, line) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        send(Update::Output(format!(
                            "[IDECraft] Force Run to Cursor: {e}\n"
                        )));
                        continue;
                    }
                }
            }
            Command::StepInto => session.step_in(thread),
            Command::StepOut => session.step_out(thread),
            Command::Stop => {
                send(end(session));
                return;
            }
            Command::Breakpoints(path, lines) => {
                if let Ok(list) = session.set_breakpoints(&path, &lines) {
                    send(Update::Placed(path, list));
                }
                continue;
            }
            Command::Pause => continue,
            Command::RunToLine(path, line) => match session.run_to_line(thread, &path, line) {
                Ok(()) => Ok(()),
                Err(e) => {
                    send(Update::Output(format!("[IDECraft] Run to Cursor: {e}\n")));
                    continue;
                }
            },
            Command::SetValue(frame, parent, name, value) => {
                let container = match parent {
                    0 => session.locals_reference(frame).ok().flatten(),
                    r => Some(r),
                };
                let result = match container {
                    Some(r) => session
                        .set_variable(r, &name, &value)
                        .map_err(|e| error_summary(&e.to_string())),
                    None => Err("the frame has no variables".into()),
                };
                let changed = result.is_ok();
                send(Update::ValueSet(name, result));
                if changed {
                    match parent {
                        0 => {
                            if let Ok(v) = top_locals(&mut session, frame) {
                                send(Update::Locals(frame, v));
                            }
                        }
                        r => {
                            if let Ok(v) = session.variables(r) {
                                send(Update::Children(r, v));
                            }
                            // The summary of the changed field's object (hand test 125).
                            if let Ok(v) = top_locals(&mut session, frame) {
                                send(Update::Values(frame, v));
                            }
                        }
                    }
                }
                continue;
            }
            Command::Special(special) => {
                if let Err(e) = session.set_special_breakpoints(&special) {
                    send(Update::Output(format!("[IDECraft] {e}\n")));
                }
                continue;
            }
            Command::Children(reference) => {
                match session.variables(reference) {
                    Ok(v) => send(Update::Children(reference, v)),
                    Err(e) => send(Update::Failed(e.to_string())),
                }
                continue;
            }
            Command::Evaluate(expression, frame) => {
                let result = session
                    .evaluate(&expression, frame)
                    .map_err(|e| error_summary(&e.to_string()));
                send(Update::Evaluated(expression, result));
                continue;
            }
            Command::Watches(expressions, frame) => {
                let values = expressions
                    .into_iter()
                    .map(|e| {
                        let v = session
                            .evaluate(&e, frame)
                            .map_err(|e| error_summary(&e.to_string()));
                        (e, v)
                    })
                    .collect();
                send(Update::Watched(values));
                continue;
            }
            Command::Thread(id) => {
                let shown = session.stack_trace(id).and_then(|mut frames| {
                    frames.truncate(FRAMES);
                    let locals = match frames.first() {
                        Some(top) => top_locals(&mut session, top.id)?,
                        None => Vec::new(),
                    };
                    Ok((frames, locals))
                });
                match shown {
                    Ok((frames, locals)) => {
                        thread = id;
                        stack = frames.clone();
                        let object = locals.clone();
                        send(Update::Frames(frames, locals));
                        if let Some(top) = stack.first() {
                            send_fields(&mut session, launch.kind, top, &object, &send);
                        }
                    }
                    Err(e) => send(Update::Failed(e.to_string())),
                }
                continue;
            }
            Command::Frame(id) => {
                match top_locals(&mut session, id) {
                    Ok(v) => {
                        let object = v.clone();
                        send(Update::Locals(id, v));
                        if let Some(frame) = stack.iter().find(|f| f.id == id) {
                            send_fields(&mut session, launch.kind, frame, &object, &send);
                        }
                    }
                    Err(e) => send(Update::Failed(e.to_string())),
                }
                continue;
            }
        };
        match resumed {
            Ok(()) => {
                running = true;
                send(Update::Running);
            }
            Err(e) => send(Update::Failed(e.to_string())),
        }
    }
}

/// Reports where steps that return stopped ended (Smart Step Into, Step Out of Code Block,
/// `SPEC-0091` v11 behaviour 18, v12 behaviour 25); `true` when the program runs again.
fn report_steps(
    session: &mut DebugSession,
    kind: Kind,
    thread: i64,
    done: Result<(), idecraft_debugger::DapError>,
    what: &str,
    stack: &mut Vec<Frame>,
    send: &dyn Fn(Update),
) -> bool {
    let stopped = done.and_then(|()| session.stopped_state("step", thread, FRAMES));
    match stopped {
        Ok(StopState::Stopped {
            reason,
            frames,
            locals,
            ..
        }) => {
            *stack = frames.clone();
            let object = locals.clone();
            send(Update::Stopped {
                reason,
                frames,
                locals,
            });
            if let Some(top) = stack.first() {
                send_fields(session, kind, top, &object, send);
            }
            if let Ok(threads) = session.threads() {
                send(Update::Threads(threads, thread));
            }
            false
        }
        Ok(_) => false,
        Err(e) => {
            send(Update::Output(format!(
                "[IDECraft] {what}: {}\n",
                error_summary(&e.to_string())
            )));
            // Show where the program is now; if it ended, the wait reports that.
            match session.stopped_state("step", thread, FRAMES) {
                Ok(StopState::Stopped { frames, locals, .. }) => {
                    *stack = frames.clone();
                    send(Update::Stopped {
                        reason: "step".into(),
                        frames,
                        locals,
                    });
                    false
                }
                _ => true,
            }
        }
    }
}

/// How long the build of changed sources may take for hot swap (`SPEC-0091` v12 behaviour 37).
const HOT_SWAP_BUILD: Duration = Duration::from_secs(60);

/// Builds changed sources for hot swap: [`Update::CodeChanged`] when built, else a console line
/// with the reason (`SPEC-0091` v12 behaviour 37).
fn build_changed(session: &mut DebugSession, files: Vec<PathBuf>) -> Update {
    match session.build_changed(&files, HOT_SWAP_BUILD) {
        Ok(()) => Update::CodeChanged(files),
        Err(e) => Update::Output(format!(
            "[IDECraft] Hot swap: {}\n",
            error_summary(&e.to_string())
        )),
    }
}

/// The file name of a frame's source: a JDK class from jdtls's `jdt://` address ends in
/// `%3Cjava.lang.ref(Reference.class`; the dump shows `Reference.class` (v13).
fn source_file_name(name: &str) -> String {
    name.rsplit('(').next().unwrap_or(name).to_owned()
}

/// The thread dump's text for the console (`SPEC-0091` v12 behaviour 26): a header, then per
/// thread `"name" STATE group` and its frames, `running` for a thread that is not suspended.
pub fn dump_text(threads: &[(idecraft_debugger::ThreadInfo, Vec<Frame>)]) -> String {
    let mut out = format!("Thread dump: {} threads\n", threads.len());
    for (t, frames) in threads {
        let mut head = format!("\"{}\"", t.name);
        if !t.state.is_empty() {
            head.push(' ');
            head.push_str(&t.state);
        }
        if !t.group.is_empty() {
            head.push_str(&format!(" (group {})", t.group));
        }
        out.push_str(&head);
        out.push('\n');
        if !t.suspended {
            out.push_str("\trunning\n");
        }
        for f in frames {
            let file = f
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| source_file_name(&n.to_string_lossy()))
                .unwrap_or_default();
            out.push_str(&format!("\tat {} ({file}:{})\n", f.name, f.line));
        }
        out.push('\n');
    }
    out
}

static BROWSER_HEADLESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Debugs pages in Chrome without a window (the self-test).
pub fn set_browser_headless(on: bool) {
    BROWSER_HEADLESS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The `launch` arguments for the adapter of `launch.kind`.
pub fn arguments(launch: &Launch) -> serde_json::Value {
    match launch.kind {
        Kind::Native => {
            let env: Vec<String> = launch.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            json!({
                "program": launch.program,
                "args": launch.args,
                "cwd": launch.cwd,
                "env": env,
                "stopOnEntry": false,
            })
        }
        Kind::Python => {
            let mut a = json!({
                "type": "python",
                "request": "launch",
                "program": launch.program,
                "args": launch.args,
                "cwd": launch.cwd,
                "env": launch.env,
                "console": "internalConsole",
                "justMyCode": true,
                "stopOnEntry": false,
            });
            if let Some(python) = &launch.interpreter {
                a["python"] = json!(python);
            }
            a
        }
        // `classPaths` and `modulePaths` are added once jdtls has resolved them.
        Kind::Java => json!({
            "type": "java",
            "request": "launch",
            "mainClass": launch.program,
            "args": launch.args.join(" "),
            "cwd": launch.cwd,
            "env": launch.env,
            "console": "internalConsole",
            "stopOnEntry": false,
        }),
        // The adapter resolves the class path itself with Gradle or Maven.
        Kind::Kotlin => json!({
            "projectRoot": launch.cwd,
            "mainClass": launch.program,
            "cwd": launch.cwd,
        }),
        // Chrome with the project's own profile outside the project, never the user's; without a
        // window for the self-test or when `IDECRAFT_BROWSER_HEADLESS` is set.
        Kind::Browser => idecraft_debugger::browser_launch(
            &launch.program.to_string_lossy(),
            &launch.cwd,
            &idecraft_debugger::chrome().unwrap_or_default(),
            &launch
                .browser
                .as_ref()
                .map(|b| b.folder.clone())
                .unwrap_or_default(),
            BROWSER_HEADLESS.load(std::sync::atomic::Ordering::Relaxed)
                || std::env::var_os("IDECRAFT_BROWSER_HEADLESS").is_some(),
        ),
        Kind::Node => idecraft_debugger::node_launch(
            &launch.program,
            None,
            &launch.args,
            &launch.cwd,
            &launch.env,
        ),
        Kind::PackageScript => idecraft_debugger::node_launch(
            &launch.program,
            Some(&launch.program),
            &launch.args,
            &launch.cwd,
            &launch.env,
        ),
        // The words after `go`; a command Delve cannot debug is refused before this is used.
        Kind::Go => idecraft_debugger::go_launch(&launch.args, &launch.cwd, &launch.env)
            .unwrap_or_else(|_| json!({})),
    }
}

/// The reference of the frame's object among `locals`: the variable `this` or `self` with
/// children, not for JavaScript, where `this` is often the global object (`SPEC-0091` v8
/// behaviour 13).
pub fn object_of(kind: Kind, locals: &[Variable]) -> Option<i64> {
    if matches!(kind, Kind::Node | Kind::PackageScript | Kind::Browser) {
        return None;
    }
    locals
        .iter()
        .find(|v| (v.name == "this" || v.name == "self") && v.reference != 0)
        .map(|v| v.reference)
}

/// Fetches the fields of the frame's object, finds those the function's code uses (the file is
/// read from disk) and, for Java and Kotlin, the object's text; reports them, nothing without an
/// object (`SPEC-0091` v8 behaviour 13, v9 behaviour 14).
fn send_fields(
    session: &mut DebugSession,
    kind: Kind,
    frame: &Frame,
    locals: &[Variable],
    send: &dyn Fn(Update),
) {
    let Some(owner) = object_of(kind, locals) else {
        return;
    };
    let Ok(fields) = session.variables(owner) else {
        return;
    };
    let text = frame
        .path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let exec = usize::try_from(frame.line - 1).unwrap_or(0);
    let range = method_lines(&lines, exec, function_name(&frame.name));
    let used = if lines.is_empty() {
        Vec::new()
    } else {
        fields_used(&lines, range, &fields, locals)
    };
    let this = locals.iter().find(|v| v.reference == owner);
    let text = match (kind, this) {
        (Kind::Java | Kind::Kotlin, Some(this)) if this.name == "this" => {
            object_text_of_this(session, frame.id).and_then(|t| object_text(&this.value, &t))
        }
        _ => None,
    };
    send(Update::Fields(ObjectFields {
        frame: frame.id,
        owner,
        fields,
        used,
        text,
    }));
}

thread_local! {
    /// Waits left in this session's thread for java-debug to know the program's project.
    static PROJECT_WAITS: std::cell::Cell<u8> = const { std::cell::Cell::new(PROJECT_WAITS_MAX) };
}

/// Seconds a session waits at most, once, for jdtls to import the project before `this.toString()`.
const PROJECT_WAITS_MAX: u8 = 8;

/// `this.toString()` in a frame. Right after the start jdtls may not have imported the program's
/// project yet and java-debug refuses to evaluate ("specify projectName"); the session then waits
/// up to [`PROJECT_WAITS_MAX`] seconds once, not at every stop.
fn object_text_of_this(session: &mut DebugSession, frame: i64) -> Option<String> {
    loop {
        match session.evaluate_within("this.toString()", frame, OBJECT_TEXT) {
            Ok(t) => {
                PROJECT_WAITS.with(|w| w.set(0));
                return Some(t);
            }
            Err(e) if e.to_string().contains("projectName") => {
                let left = PROJECT_WAITS.with(std::cell::Cell::get);
                if left == 0 {
                    return None;
                }
                PROJECT_WAITS.with(|w| w.set(left - 1));
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(_) => return None,
        }
    }
}

/// Whether `line` names `name` as a whole word.
fn names_word(line: &str, name: &str) -> bool {
    line.match_indices(name).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + name.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The function of a frame name without class, package and parameters: `Inventory$Item.totalCents()`
/// → `totalCents`, `ns::twice(int)` → `twice` (`SPEC-0091` v9 behaviour 14).
pub fn function_name(frame: &str) -> &str {
    let head = frame.split('(').next().unwrap_or(frame).trim();
    head.rsplit(['.', ':', '$', ' ']).next().unwrap_or(head)
}

/// The 0-based lines of the function paused at `exec`: from its declaration (the nearest line at
/// or above `exec`, at most 300 up, naming `function` followed by `(`) to the matching closing
/// brace, or for code without braces the following lines indented deeper; without a declaration
/// the 50 lines above `exec` through it (`SPEC-0091` v9 behaviour 14).
pub fn method_lines(
    lines: &[&str],
    exec: usize,
    function: &str,
) -> std::ops::RangeInclusive<usize> {
    let exec = exec.min(lines.len().saturating_sub(1));
    let fallback = exec.saturating_sub(50)..=exec;
    if lines.is_empty() || function.is_empty() || !function.chars().all(is_word) {
        return fallback;
    }
    let declares = |line: &str| {
        line.match_indices(function).any(|(at, _)| {
            let before = line[..at].chars().next_back();
            !before.is_some_and(is_word)
                && line[at + function.len()..].trim_start().starts_with('(')
        })
    };
    let Some(start) = (exec.saturating_sub(300)..=exec)
        .rev()
        .find(|&l| declares(lines[l]))
    else {
        return fallback;
    };
    let mut depth = 0i32;
    let mut opened = false;
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return start..=i.max(exec);
        }
        // The body's brace is on the declaration or the line after it, or there is none.
        if !opened && i > start {
            break;
        }
    }
    if opened {
        return start..=lines.len() - 1;
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let base = indent(lines[start]);
    let mut end = start;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if indent(line) <= base {
            break;
        }
        end = i;
    }
    start..=end.max(exec)
}

/// Names of the `fields` that `lines[range]` name as a whole word and no local hides, in the
/// fields' order (`SPEC-0091` v9 behaviour 14).
pub fn fields_used(
    lines: &[&str],
    range: std::ops::RangeInclusive<usize>,
    fields: &[Variable],
    locals: &[Variable],
) -> Vec<String> {
    let last = lines.len().saturating_sub(1);
    let code = &lines[(*range.start()).min(last)..=(*range.end()).min(last)];
    fields
        .iter()
        .filter(|f| !f.name.is_empty() && f.name.chars().all(is_word))
        .filter(|f| !locals.iter().any(|l| l.name == f.name))
        .filter(|f| code.iter().any(|l| names_word(l, &f.name)))
        .map(|f| f.name.clone())
        .collect()
}

/// The object's value followed by its text in quotes, when the text says more than the value;
/// one pair of quotes around the adapter's answer is dropped, a long text is cut (`SPEC-0091` v9
/// behaviour 14).
pub fn object_text(value: &str, text: &str) -> Option<String> {
    let text = text.trim();
    // kotlin-debug-adapter cannot evaluate calls and answers `unknown`, without quotes (v16
    // behaviour 42); a text "unknown" of an object comes in quotes.
    if text == "unknown" {
        return None;
    }
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    // java-debug already shows the text itself with `showToString` (v11 behaviour 17).
    if text.is_empty() || text == value || value.contains(&format!("\"{text}")) {
        return None;
    }
    let mut shown: String = text.chars().take(OBJECT_TEXT_CHARS).collect();
    if text.chars().count() > OBJECT_TEXT_CHARS {
        shown.push('…');
    }
    Some(format!("{value} \"{shown}\""))
}

/// Names and values for the editor: the top-level variables, then the fields of the frame's
/// object that no local of the same name hides (`SPEC-0091` v8 behaviour 13).
pub fn editor_values(top: &[(String, String)], fields: &[Variable]) -> Vec<(String, String)> {
    let mut out = top.to_vec();
    for f in fields {
        if !out.iter().any(|(n, _)| *n == f.name) {
            out.push((f.name.clone(), f.value.clone()));
        }
    }
    out
}

/// Whether `frame` is a library frame, folded in the call stack (`SPEC-0091` v10 behaviour 15):
/// marked by the adapter, without a source file in `workspace`, or with a generated or JDK name.
/// With `files_on_host`, `workspace` is a remote project's stand-in folder, which holds no sources:
/// a path below it counts as a source file there (`SPEC-0322` v5 behaviour 14).
pub fn library_frame(frame: &Frame, workspace: &std::path::Path, files_on_host: bool) -> bool {
    if frame.deemphasized {
        return true;
    }
    let name = frame.name.as_str();
    if ["$$Lambda", "access$", "$deserializeLambda$", "/0x"]
        .iter()
        .any(|m| name.contains(m))
        || ["java.", "javax.", "jdk.", "sun.", "com.sun."]
            .iter()
            .any(|p| name.starts_with(p))
    {
        return true;
    }
    let Some(path) = &frame.path else {
        return true;
    };
    if files_on_host {
        return !path.starts_with(workspace);
    }
    if !path.is_file() {
        return true;
    }
    let inside = path.starts_with(workspace)
        || matches!(
            (path.canonicalize(), workspace.canonicalize()),
            (Ok(p), Ok(w)) if p.starts_with(&w)
        );
    !inside
}

/// One row of the call stack (`SPEC-0091` v10 behaviour 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRow {
    /// The frame at this index.
    Frame(usize),
    /// `count` folded library frames from index `first`.
    Hidden {
        /// Index of the first folded frame.
        first: usize,
        /// How many.
        count: usize,
    },
}

/// The rows of a call stack whose frames are library frames where `library` says so: runs of
/// them fold into one row unless the run starting at that index was opened; the top frame and
/// the `selected` one never fold.
pub fn frame_rows(
    count: usize,
    library: &[bool],
    selected: usize,
    opened: &HashSet<usize>,
) -> Vec<FrameRow> {
    // Rows follow the frames: library marks of an older stack beyond `count` are ignored and
    // missing marks count as project frames, so a row never names a frame that is gone.
    let folds = |i: usize| library.get(i).copied().unwrap_or(false) && i != 0 && i != selected;
    let mut rows = Vec::new();
    let mut i = 0;
    while i < count {
        if !folds(i) {
            rows.push(FrameRow::Frame(i));
            i += 1;
            continue;
        }
        let first = i;
        while i < count && folds(i) {
            i += 1;
        }
        if opened.contains(&first) {
            rows.extend((first..i).map(FrameRow::Frame));
        } else {
            rows.push(FrameRow::Hidden {
                first,
                count: i - first,
            });
        }
    }
    rows
}

/// The text of a folded row.
pub fn hidden_label(count: usize) -> String {
    if count == 1 {
        "1 hidden frame".to_owned()
    } else {
        format!("{count} hidden frames")
    }
}

/// Which buttons of the Debug window's toolbar are enabled (`SPEC-0194` v3 behaviour 6); View and
/// Mute Breakpoints always are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Toolbar {
    /// Rerun: a session was started in the window.
    pub rerun: bool,
    /// Resume, the steps and Run to Cursor: paused.
    pub paused: bool,
    /// Pause: the program runs.
    pub pause: bool,
    /// Stop: a session is going.
    pub stop: bool,
}

/// The toolbar's states for a window that has `started` a session, which is `active` (starting,
/// running or paused), `running` or `paused`.
pub fn toolbar(started: bool, active: bool, running: bool, paused: bool) -> Toolbar {
    Toolbar {
        rerun: started,
        paused,
        pause: running,
        stop: active,
    }
}

/// What a session gets while breakpoints are muted: nothing (`SPEC-0194` v3 behaviour 8).
pub fn unless_muted<T: Default>(muted: bool, breakpoints: T) -> T {
    if muted { T::default() } else { breakpoints }
}

/// Longest value shown inline in the editor, in characters.
const INLINE_VALUE: usize = 40;

/// Values of `locals` to show after the lines of the paused function (`SPEC-0091` v5 behaviour
/// 13): each variable at the last of the lines `exec_line - 50 ..= exec_line` (0-based) that names it
/// as a whole word, as `name: value`, several on a line joined with two spaces. Long values are cut.
pub fn inline_values(
    lines: &[&str],
    exec_line: usize,
    locals: &[(String, String)],
) -> Vec<(usize, String)> {
    let names_it = names_word;
    let first = exec_line.saturating_sub(50);
    let mut at: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (name, value) in locals {
        if name.is_empty() || !name.chars().all(is_word) {
            continue;
        }
        let Some(line) = (first..=exec_line.min(lines.len().saturating_sub(1)))
            .rev()
            .find(|&l| names_it(lines[l], name))
        else {
            continue;
        };
        let mut shown: String = value.chars().take(INLINE_VALUE).collect();
        if value.chars().count() > INLINE_VALUE {
            shown.push('…');
        }
        at.entry(line).or_default().push(format!("{name}: {shown}"));
    }
    at.into_iter().map(|(l, v)| (l, v.join("  "))).collect()
}

/// One row of the variables tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRow {
    /// Nesting depth (0 = local variable).
    pub depth: usize,
    /// The variable.
    pub var: Variable,
    /// Whether it has children and they are shown.
    pub expanded: bool,
    /// Reference of the variable holding it; 0 for a local variable (`SPEC-0194` behaviour 3).
    pub parent: i64,
}

/// The step targets whose call is written in `line` (`SPEC-0091` v11 behaviour 18): debugpy also
/// offers calls of other lines of the function (OBS-0025 Nachprüfung). Without a line text, or when
/// no label matches, the adapter's list stays.
pub fn targets_on_line(targets: Vec<(i64, String)>, line: &str) -> Vec<(i64, String)> {
    let called = |label: &str| {
        let name = idecraft_debugger::target_method(label);
        !name.is_empty()
            && line.match_indices(name).any(|(at, _)| {
                let before = line[..at].chars().next_back();
                let after = line[at + name.len()..].trim_start().chars().next();
                !before.is_some_and(is_word) && after == Some('(')
            })
    };
    if !targets.iter().any(|(_, l)| called(l)) {
        return targets;
    }
    targets.into_iter().filter(|(_, l)| called(l)).collect()
}

/// Completions shown at most under the expression field (`SPEC-0091` v11 behaviour 22).
pub const COMPLETIONS: usize = 12;

/// The word before the end of `text` that a completion replaces: letters, digits and `_`.
pub fn completion_word(text: &str) -> &str {
    let start = text
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(text.len(), |(i, _)| i);
    &text[start..]
}

/// `text` with its last word replaced by `item` (v11 behaviour 22).
pub fn complete(text: &str, item: &str) -> String {
    let word = completion_word(text);
    format!("{}{item}", &text[..text.len() - word.len()])
}

/// A value on one line for a row of the Variables list (`SPEC-0091` v14 behaviour 40): line breaks
/// and the white space around them become one space; the full value stays in the variable.
pub fn one_line(value: &str) -> String {
    if !value.contains(['\n', '\r']) {
        return value.to_owned();
    }
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A value for a row, on one line, and the bytes of its object id (`@566` of `ArrayList@566 size =
/// 1`), which the row draws dimmed (`SPEC-0091` v16 behaviour 42); no range for values without an
/// id.
pub fn row_value(value: &str) -> (String, Option<std::ops::Range<usize>>) {
    let text = one_line(value);
    let id = idecraft_debugger::values::id_range(&text);
    (text, id)
}

/// The id of the object a value shows, for Mark Object (`SPEC-0091` v11 behaviour 24): Java's
/// `Class@id` (java-debug, the JDWP object id) or a hexadecimal address `0x…` (Python's and the
/// native debuggers' default texts); `None` for values without one (numbers, strings).
pub fn object_id(value: &str) -> Option<String> {
    idecraft_debugger::extras::object_id_of(value)
}

/// The expression of row `i` for Evaluate and Add to Watches (`SPEC-0091` v11 behaviour 23): a
/// top-level variable's name, a field after its parent's expression with a dot, an element
/// (`[0]`, `0`) in brackets.
pub fn row_expression(rows: &[VarRow], i: usize) -> String {
    let Some(row) = rows.get(i) else {
        return String::new();
    };
    // The adapter's own expression where it gives one (java-debug `items.get(0)`, debugpy
    // `items[0]`; SPEC-0091 v12).
    if let Some(e) = &row.var.evaluate_name {
        return e.clone();
    }
    let name = row.var.name.trim_start_matches('[').trim_end_matches(']');
    if row.depth == 0 {
        return row.var.name.clone();
    }
    let parent = rows[..i]
        .iter()
        .rposition(|r| r.depth + 1 == row.depth && r.var.reference == row.parent);
    let Some(parent) = parent else {
        return row.var.name.clone();
    };
    let base = row_expression(rows, parent);
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
        format!("{base}[{name}]")
    } else {
        format!("{base}.{name}")
    }
}

/// The variables of the selected frame with the children fetched so far (`SPEC-0091` behaviour 5),
/// followed by the fields the function uses (v9 behaviour 14).
#[derive(Debug, Clone, Default)]
pub struct VarTree {
    roots: Vec<Variable>,
    children: HashMap<i64, Vec<Variable>>,
    expanded: HashSet<i64>,
    /// The frame's object (0 for none) and its fields shown at the top level.
    owner: i64,
    fields: Vec<Variable>,
    /// Which rows are hidden (`SPEC-0091` v12 behaviour 34).
    filters: idecraft_debugger::extras::VarFilters,
    /// Values of rows shown in another view or by a renderer, by the row's container and name:
    /// the text and, for the element views, the reference whose children the row then shows
    /// (v12 behaviours 32, 33).
    overrides: HashMap<(i64, String), (String, Option<i64>)>,
}

impl VarTree {
    /// A tree on `locals`, all collapsed.
    pub fn new(locals: Vec<Variable>) -> Self {
        Self {
            roots: locals,
            ..Self::default()
        }
    }

    /// The visible rows: the locals, then the object's fields the function uses, whose container
    /// is the object.
    pub fn rows(&self) -> Vec<VarRow> {
        let mut out = Vec::new();
        self.push(&self.roots, 0, 0, &mut out);
        self.push(&self.fields, 0, self.owner, &mut out);
        out
    }

    fn push(&self, vars: &[Variable], depth: usize, parent: i64, out: &mut Vec<VarRow>) {
        for v in vars {
            if idecraft_debugger::extras::hidden_row(&self.filters, &v.name, &v.value) {
                continue;
            }
            let mut var = v.clone();
            if let Some((value, reference)) = self.overrides.get(&(parent, v.name.clone())) {
                var.value = value.clone();
                if let Some(r) = reference {
                    var.reference = *r;
                }
            }
            let expanded = var.reference != 0 && self.expanded.contains(&var.reference);
            let reference = var.reference;
            out.push(VarRow {
                depth,
                var,
                expanded,
                parent,
            });
            if expanded && let Some(kids) = self.children.get(&reference) {
                self.push(kids, depth + 1, reference, out);
            }
        }
    }

    /// Hides rows by `filters` (`SPEC-0091` v12 behaviour 34).
    pub fn set_filters(&mut self, filters: idecraft_debugger::extras::VarFilters) {
        self.filters = filters;
    }

    /// Shows the row `key` (container, name) as `value`, with the children of `reference` when
    /// given (v12 behaviours 32, 33); `None` shows the adapter's value again.
    pub fn set_override(&mut self, key: (i64, String), value: Option<(String, Option<i64>)>) {
        match value {
            Some(v) => {
                self.overrides.insert(key, v);
            }
            None => {
                self.overrides.remove(&key);
            }
        }
    }

    /// Opens the structured value `reference` (an Inspect tab's root, v12 behaviour 30); returns
    /// the reference to fetch when its children are not known yet.
    pub fn expand(&mut self, reference: i64) -> Option<i64> {
        if reference == 0 || self.expanded.contains(&reference) {
            return None;
        }
        self.toggle(reference)
    }

    /// Opens or closes a structured variable; returns the reference whose children must be fetched
    /// first, if they are not known yet.
    pub fn toggle(&mut self, reference: i64) -> Option<i64> {
        if reference == 0 {
            return None;
        }
        if !self.expanded.remove(&reference) {
            self.expanded.insert(reference);
            if !self.children.contains_key(&reference) {
                return Some(reference);
            }
        }
        None
    }

    /// Children fetched for `reference`; the fields shown at the top level take the object's
    /// new values (after Set Value on one of them).
    pub fn set_children(&mut self, reference: i64, vars: Vec<Variable>) {
        if reference != 0 && reference == self.owner {
            for f in &mut self.fields {
                if let Some(v) = vars.iter().find(|v| v.name == f.name) {
                    *f = v.clone();
                }
            }
        }
        self.children.insert(reference, vars);
    }

    /// Shows the fields of `object` named in `used` after the locals (a local of the same name
    /// hides its field) and puts the object's `text` in place of its value (v9 behaviour 14).
    pub fn set_object(&mut self, object: &ObjectFields) {
        self.owner = object.owner;
        self.fields = object
            .fields
            .iter()
            .filter(|f| object.used.contains(&f.name))
            .filter(|f| !self.roots.iter().any(|r| r.name == f.name))
            .cloned()
            .collect();
        if let Some(text) = &object.text
            && let Some(root) = self.roots.iter_mut().find(|r| r.reference == object.owner)
        {
            root.value = text.clone();
        }
    }

    /// Takes the values and types of `fresh` for the top-level variables of the same name and
    /// keeps what is open (hand test 125: an object's summary kept the value from before one of
    /// its fields was set).
    pub fn refresh_values(&mut self, fresh: &[Variable]) {
        for root in &mut self.roots {
            if let Some(f) = fresh.iter().find(|f| f.name == root.name) {
                // The object keeps its text (v9 behaviour 14).
                if root.reference == self.owner && root.value.starts_with(&f.value) {
                    continue;
                }
                root.value = f.value.clone();
                root.kind = f.kind.clone();
            }
        }
    }
}

/// A file called `name` under `root`, searched breadth first at most `depth` folders deep;
/// hidden folders and build output (`target`, `build`, `node_modules`, `out`) are left out
/// (`SPEC-0091` v12 behaviour 38).
pub fn find_file_named(root: &std::path::Path, name: &str, depth: usize) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let mut level = vec![root.to_path_buf()];
    for _ in 0..=depth {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for e in entries {
                let file = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                if path.is_dir() {
                    if !file.starts_with('.')
                        && !["target", "build", "node_modules", "out"].contains(&file.as_str())
                    {
                        next.push(path);
                    }
                } else if file == name {
                    return Some(path);
                }
            }
        }
        level = next;
    }
    None
}

/// Breakpoint lines (0-based, sorted) after an edit at `line` that removed `removed` and added
/// `added` line breaks: later lines shift, lines that disappeared give their breakpoint to `line`
/// (`SPEC-0091` behaviour 1).
pub fn shift_lines(lines: &[usize], line: usize, removed: usize, added: usize) -> Vec<usize> {
    let mut moved: Vec<usize> = lines
        .iter()
        .map(|&b| {
            if b <= line {
                b
            } else if b <= line + removed {
                line
            } else {
                b + added - removed
            }
        })
        .collect();
    moved.dedup();
    moved
}

/// One readable line of a debugger's error: the line with `error:` (lldb prints the expression and
/// a caret below it), without the `<user expression N>:L:C:` place; else the first line (hand test
/// 5, 2026-11-01).
pub fn error_summary(message: &str) -> String {
    let lines: Vec<&str> = message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let line = lines
        .iter()
        .find_map(|l| l.find("error:").map(|at| &l[at + "error:".len()..]))
        .or(lines.first().copied())
        .unwrap_or_default()
        .trim();
    let line = match line.strip_prefix('<') {
        Some(rest) => rest
            .split_once(">:")
            .and_then(|(_, after)| after.splitn(3, ':').nth(2))
            .unwrap_or(line)
            .trim(),
        None => line,
    };
    line.to_owned()
}

/// `text` without the first line that repeats `echo`, the command line the console starts with:
/// adapters such as js-debug print the command they launch once more (hand test 95). Quotes and
/// runs of spaces do not count; the echo is used up by its first match.
pub fn drop_echo(echo: &mut Option<String>, text: &str) -> String {
    fn plain(s: &str) -> String {
        s.split_whitespace()
            .map(|w| w.trim_matches(|c| c == '"' || c == '\''))
            .collect::<Vec<_>>()
            .join(" ")
    }
    let Some(line) = echo.as_deref().map(plain) else {
        return text.to_owned();
    };
    let mut out = String::with_capacity(text.len());
    let mut dropped = false;
    for l in text.split_inclusive('\n') {
        if !dropped && !line.is_empty() && plain(l) == line {
            dropped = true;
        } else {
            out.push_str(l);
        }
    }
    if dropped {
        *echo = None;
    }
    out
}

/// The breakpoints of `path` the session reports as not bound, by the 0-based line asked for,
/// with the reason the tooltip names: the adapter's, else IDECraft's hint (`SPEC-0127` v5
/// behaviour 14). Function breakpoints (no line asked for) are left out.
pub fn unbound_lines(path: &std::path::Path, placed: &[Breakpoint]) -> BTreeMap<usize, String> {
    placed
        .iter()
        .filter(|b| !b.verified)
        .filter_map(|b| {
            let line = usize::try_from(b.requested_line?.checked_sub(1)?).ok()?;
            let reason = b
                .reason
                .clone()
                .unwrap_or_else(|| idecraft_debugger::unbound_hint(path));
            Some((line, reason))
        })
        .collect()
}

/// What the status bar says when Quick Evaluate Expression finds the program running
/// (`SPEC-0091` v19 behaviour 44).
pub const NOT_PAUSED: &str = "Cannot evaluate: the program is not paused";
/// What it says without an expression at the caret.
pub const NOTHING_TO_EVALUATE: &str = "Nothing to evaluate at the caret";

/// The hint of Quick Evaluate Expression: `expression = value` or `expression: error`.
pub fn quick_text(expression: &str, result: &Result<String, String>) -> String {
    match result {
        Ok(value) => format!("{expression} = {value}"),
        Err(why) => format!("{expression}: {why}"),
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0091 AK-24 (v19): the hint reads as one line for a value and an error.
    #[test]
    fn quick_evaluation_reads_as_one_line() {
        use super::*;
        assert_eq!(quick_text("a + 1", &Ok("3".into())), "a + 1 = 3");
        assert_eq!(
            quick_text("nope", &Err("name 'nope' is not defined".into())),
            "nope: name 'nope' is not defined"
        );
        assert_ne!(NOT_PAUSED, NOTHING_TO_EVALUATE);
    }

    /// SPEC-0127 v5 AK-9: the unbound lines are the lines asked for, with the adapter's reason or
    /// IDECraft's hint; bound ones and function breakpoints are left out.
    #[test]
    fn unbound_lines_by_requested_line() {
        use idecraft_debugger::{Breakpoint, EvaluatedBy};
        let bp = |verified: bool, requested: Option<u64>, reason: Option<&str>| Breakpoint {
            id: None,
            verified,
            line: requested.map(|l| l + 1),
            requested_line: requested,
            reason: reason.map(str::to_owned),
            evaluated_by: EvaluatedBy::Adapter,
        };
        let missing = std::path::Path::new("/nonexistent/idecraft/Main.kt");
        let lines = super::unbound_lines(
            missing,
            &[
                bp(true, Some(3), None),
                bp(false, Some(7), Some("no code here")),
                bp(false, Some(9), None),
                bp(false, None, Some("function")),
            ],
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[&6], "no code here");
        assert_eq!(lines[&8], "the file does not exist");
        assert!(super::unbound_lines(missing, &[]).is_empty());
    }

    /// SPEC-0091 v14 AK-19: a function's source is one row; a single line keeps its spaces.
    #[test]
    fn a_value_with_line_breaks_is_one_line() {
        assert_eq!(
            super::one_line("function greet(name) {\n  return name;\r\n}"),
            "function greet(name) { return name; }"
        );
        assert_eq!(super::one_line("\"a  b\""), "\"a  b\"");
    }

    /// Hand test 95: js-debug's echo of the command line is not shown a second time.
    #[test]
    fn the_adapters_echo_of_the_command_line_is_dropped_once() {
        let mut echo = Some("/Users/x/.local/bin/npm run dev".to_owned());
        assert_eq!(super::drop_echo(&mut echo, "$ npm ci\n"), "$ npm ci\n");
        assert!(echo.is_some());
        assert_eq!(
            super::drop_echo(
                &mut echo,
                "\"/Users/x/.local/bin/npm\"  run dev\nSumme: 12\n"
            ),
            "Summe: 12\n"
        );
        assert!(echo.is_none());
        assert_eq!(
            super::drop_echo(&mut echo, "/Users/x/.local/bin/npm run dev\n"),
            "/Users/x/.local/bin/npm run dev\n"
        );
    }
    /// SPEC-0132: the run command of a Kotlin module is debugged through kotlin-debug-adapter after
    /// its build step.
    #[test]
    fn kotlin_commands_are_planned_for_the_kotlin_adapter() {
        let command: Vec<String> = [
            "sh",
            "-c",
            "gradle -q classes && kotlin -cp build/classes/kotlin/main demo.MainKt",
        ]
        .map(str::to_owned)
        .to_vec();
        let (kind, _, program, args) = plan(&command, std::path::Path::new("/tmp")).unwrap();
        assert_eq!(kind, Kind::Kotlin);
        assert_eq!(program, PathBuf::from("demo.MainKt"));
        assert_eq!(args, ["gradle -q classes"]);
        assert_eq!(Kind::Kotlin.adapter_name(), "kotlin-debug-adapter");
    }

    use super::*;

    fn var(name: &str, reference: i64) -> Variable {
        Variable {
            name: name.into(),
            value: "1".into(),
            kind: Some("int".into()),
            reference,
            evaluate_name: None,
        }
    }

    #[test]
    fn closures_follow_the_locals() {
        let s = |n: &str, r: i64| (n.to_owned(), r);
        let (locals, extra) = frame_scopes(&[
            s("Local", 1),
            s("Closure (setInterval)", 2),
            s("Block", 3),
            s("Global", 4),
        ]);
        assert_eq!(locals, Some(1));
        assert_eq!(
            extra
                .iter()
                .map(|v| (v.name.as_str(), v.reference))
                .collect::<Vec<_>>(),
            [("Closure (setInterval)", 2), ("Block", 3)]
        );
        assert_eq!(frame_scopes(&[s("Locals", 7)]), (Some(7), Vec::new()));
        assert_eq!(frame_scopes(&[]), (None, Vec::new()));
    }

    #[test]
    fn variables_tree_fetches_children_once() {
        let mut t = VarTree::new(vec![var("a", 0), var("point", 7)]);
        assert_eq!(t.rows().len(), 2);
        assert_eq!(t.toggle(0), None);
        assert_eq!(t.toggle(7), Some(7));
        t.set_children(7, vec![var("x", 0), var("y", 0)]);
        let rows = t.rows();
        assert_eq!(
            rows.iter()
                .map(|r| (r.depth, r.var.name.as_str()))
                .collect::<Vec<_>>(),
            [(0, "a"), (0, "point"), (1, "x"), (1, "y")]
        );
        // Set Value changes a local in the frame's scope, a field in its parent (SPEC-0194 AK-2).
        assert_eq!(
            rows.iter().map(|r| r.parent).collect::<Vec<_>>(),
            [0, 0, 7, 7]
        );
        assert!(rows[1].expanded);
        assert_eq!(t.toggle(7), None);
        assert_eq!(t.rows().len(), 2);
        assert_eq!(t.toggle(7), None, "children known: no second fetch");
        assert_eq!(t.rows().len(), 4);
        // After a field was set the summary follows; the open object stays open.
        let mut point = var("point", 99);
        point.value = "Point(x=50, y=2)".into();
        t.refresh_values(&[var("a", 0), point]);
        let rows = t.rows();
        assert_eq!(rows[1].var.value, "Point(x=50, y=2)");
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn breakpoints_move_with_inserted_and_deleted_lines() {
        assert_eq!(
            shift_lines(&[2, 7], 4, 0, 2),
            [2, 9],
            "two lines added at 4"
        );
        assert_eq!(
            shift_lines(&[2, 7], 0, 1, 0),
            [1, 6],
            "first line break removed"
        );
        assert_eq!(
            shift_lines(&[3, 5, 9], 3, 3, 0),
            [3, 6],
            "lines 4–6 joined into 3"
        );
        assert_eq!(
            shift_lines(&[3], 3, 0, 1),
            [3],
            "a break at the line keeps it"
        );
    }

    /// SPEC-0050 v2 AK-5: debugging a `python3` configuration runs the program with the
    /// workspace's selected interpreter; an explicit interpreter stays.
    #[test]
    fn python_debug_uses_the_selected_interpreter() {
        if std::env::var_os("VIRTUAL_ENV").is_some() || std::env::var_os("CONDA_PREFIX").is_some() {
            eprintln!("skipped: an environment is active in the test process");
            return;
        }
        let root = std::env::temp_dir().join(format!("idecraft-pydebug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join(".venv/pyvenv.cfg"), "version = 3.12.4\n").unwrap();
        std::fs::write(root.join(".venv/bin/python"), "").unwrap();
        let config = |command: &[&str]| idecraft_core::RunConfig {
            name: "app".into(),
            command: command.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            env: BTreeMap::new(),
            kind: Default::default(),
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
        };
        let (kind, interpreter, program, args) =
            plan_run_config(&root, &config(&["python3", "app.py", "-v"]), &root)
                .unwrap()
                .unwrap();
        assert_eq!(kind, Kind::Python);
        assert_eq!(interpreter, Some(root.join(".venv/bin/python")));
        assert_eq!(
            (program, args),
            (root.join("app.py"), vec!["-v".to_owned()])
        );
        let (_, interpreter, _, _) =
            plan_run_config(&root, &config(&["/usr/bin/python3", "app.py"]), &root)
                .unwrap()
                .unwrap();
        assert_eq!(interpreter, Some(PathBuf::from("/usr/bin/python3")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_scripts_and_native_programs() {
        let cwd = std::path::Path::new("/work");
        let cmd = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (kind, interpreter, script, args) =
            plan(&cmd(&["python3", "app.py", "-v"]), cwd).unwrap();
        assert_eq!(kind, Kind::Python);
        assert!(interpreter.is_some_and(|i| i.ends_with("python3")));
        assert_eq!(
            (script, args),
            (PathBuf::from("/work/app.py"), vec!["-v".to_owned()])
        );
        let (kind, interpreter, script, _) = plan(&cmd(&["tools/run.py"]), cwd).unwrap();
        assert_eq!(
            (kind, interpreter, script),
            (Kind::Python, None, PathBuf::from("/work/tools/run.py"))
        );
        assert_eq!(
            plan(&cmd(&["./build/app"]), cwd).map(|p| p.0),
            Some(Kind::Native)
        );
        assert_eq!(
            plan(&cmd(&["python3"]), cwd),
            None,
            "an interpreter without a script"
        );
        let python = arguments(&Launch {
            kind: Kind::Python,
            adapter: vec![],
            interpreter: Some(PathBuf::from("/usr/bin/python3")),
            program: PathBuf::from("/work/app.py"),
            args: vec![],
            cwd: PathBuf::from("/work"),
            env: BTreeMap::from([("A".into(), "1".into())]),
            breakpoints: BTreeMap::new(),
            prepare: String::new(),
            special: Default::default(),
            attach: None,
            browser: None,
            remote: None,
        });
        assert_eq!(python["python"], "/usr/bin/python3");
        assert_eq!(python["env"]["A"], "1");
        assert_eq!(python["console"], "internalConsole");
    }

    #[test]
    fn programs_relative_to_the_working_directory() {
        let cwd = std::path::Path::new("/work");
        let cmd = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            program_of(&cmd(&["./build/app", "-v"]), cwd),
            Some((PathBuf::from("/work/./build/app"), vec!["-v".into()]))
        );
        assert_eq!(
            program_of(&cmd(&["/bin/ls"]), cwd),
            Some((PathBuf::from("/bin/ls"), vec![]))
        );
        assert_eq!(
            program_of(&cmd(&["sh"]), cwd).map(|p| p.0.is_absolute()),
            Some(true)
        );
        assert_eq!(program_of(&[], cwd), None);
    }

    #[test]
    fn breakpoints_file_round_trip() {
        let root = std::env::temp_dir().join(format!("idecraft-bp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let parsed = parse_breakpoints(
            &root,
            "src/main.c:7,3,3\nbroken line\nnotes.py:0,x\n/abs/lib.c:12\n",
        );
        assert_eq!(parsed.len(), 2, "malformed and empty lines are skipped");
        assert_eq!(parsed[&root.join("src/main.c")], [2, 6]);
        assert_eq!(parsed[&PathBuf::from("/abs/lib.c")], [11]);
        assert_eq!(
            format_breakpoints(&root, &parsed),
            "/abs/lib.c:12\nsrc/main.c:3,7\n"
        );
        save_breakpoints(&root, &parsed);
        assert_eq!(load_breakpoints(&root), parsed);
        save_breakpoints(&root, &BTreeMap::new());
        assert!(
            !root.join(BREAKPOINTS_FILE).exists(),
            "no breakpoints, no file"
        );
        assert!(load_breakpoints(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn inline_values_after_their_last_line() {
        let lines = [
            "int twice(int x) {",
            "    int y = x * 2;",
            "    int xy = y;",
            "    return y;",
            "}",
            "int later = 1;",
        ];
        let locals = [
            ("x".to_owned(), "21".to_owned()),
            ("y".to_owned(), "42".to_owned()),
            ("later".to_owned(), "0".to_owned()),
            ("long".to_owned(), "z".repeat(50)),
            ("this".to_owned(), "{…}".to_owned()),
        ];
        assert_eq!(
            inline_values(&lines, 3, &locals),
            [(1, "x: 21".to_owned()), (3, "y: 42".to_owned())],
            "`xy` does not name `x`; `later` comes after the execution line"
        );
        let lines = ["f(long, long)"];
        let v = inline_values(&lines, 0, &locals[3..4]);
        assert_eq!(v[0].1.chars().count(), "long: ".len() + INLINE_VALUE + 1);
    }

    #[test]
    fn inline_values_take_the_fields_of_this() {
        let lines = [
            "record Item(int qty, int unitPriceCents) {",
            "    int totalCents() {",
            "        return qty * unitPriceCents;",
            "    }",
        ];
        let field = |n: &str, v: &str| Variable {
            name: n.into(),
            value: v.into(),
            kind: Some("int".into()),
            reference: 0,
            evaluate_name: None,
        };
        let top = [("this".to_owned(), "Inventory$Item@15".to_owned())];
        let values = editor_values(&top, &[field("qty", "3"), field("unitPriceCents", "25")]);
        assert_eq!(
            inline_values(&lines, 2, &values),
            [(2, "qty: 3  unitPriceCents: 25".to_owned())],
            "the record's components show on the line that uses them"
        );
        let top = [("qty".to_owned(), "7".to_owned())];
        assert_eq!(
            editor_values(&top, &[field("qty", "3")]),
            [("qty".to_owned(), "7".to_owned())],
            "a local hides the field of the same name"
        );
    }

    /// Smart Step Into lists only the calls of the line (`SPEC-0091` v11 behaviour 18).
    #[test]
    fn step_targets_of_the_line_only() {
        let targets = |names: &[&str]| -> Vec<(i64, String)> {
            names
                .iter()
                .enumerate()
                .map(|(i, n)| (i64::try_from(i).unwrap(), (*n).to_owned()))
                .collect()
        };
        let line = "    r = add(twice(total), square(3))";
        let kept = targets_on_line(
            targets(&["twice", "square", "add", "fact", "total", "sum"]),
            line,
        );
        let names: Vec<&str> = kept.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, ["twice", "square", "add"]);
        // java-debug labels with a class and arguments.
        let kept = targets_on_line(targets(&["Main.twice(int)", "Main.square(int)"]), line);
        assert_eq!(kept.len(), 2);
        // Nothing matches (or no line text): the adapter's list stays.
        assert_eq!(targets_on_line(targets(&["a", "b"]), "").len(), 2);
    }

    /// Completion words, Mark Object's ids and the expressions of nested rows (`SPEC-0091` v11
    /// behaviours 22–24).
    #[test]
    fn completion_words_object_ids_and_row_expressions() {
        assert_eq!(completion_word("stock.ge"), "ge");
        assert_eq!(completion_word("stock."), "");
        assert_eq!(complete("stock.ge", "get"), "stock.get");
        assert_eq!(complete("a + su", "sum"), "a + sum");
        assert_eq!(object_id("Main$Item@33"), Some("Main$Item@33".into()));
        assert_eq!(
            object_id("Main$Item@33 \"Item[name=pen]\""),
            Some("Main$Item@33".into())
        );
        assert_eq!(
            object_id("ArrayList@10 size=2"),
            Some("ArrayList@10".into())
        );
        assert_eq!(
            object_id("<__main__.Item object at 0x10a722e70>"),
            Some("0x10a722e70".into())
        );
        assert_eq!(object_id("42"), None);
        assert_eq!(object_id("\"a@b\""), None);
        assert_eq!(object_id("user@host x"), None);
        let row = |depth: usize, name: &str, reference: i64, parent: i64| VarRow {
            depth,
            var: Variable {
                name: name.into(),
                value: String::new(),
                kind: None,
                reference,
                evaluate_name: None,
            },
            expanded: reference != 0,
            parent,
        };
        let rows = vec![
            row(0, "items", 5, 0),
            row(1, "0", 6, 5),
            row(2, "name", 0, 6),
            row(1, "1", 7, 5),
            row(0, "sum", 0, 0),
        ];
        assert_eq!(row_expression(&rows, 0), "items");
        assert_eq!(row_expression(&rows, 1), "items[0]");
        assert_eq!(row_expression(&rows, 2), "items[0].name");
        assert_eq!(row_expression(&rows, 3), "items[1]");
        assert_eq!(row_expression(&rows, 4), "sum");
    }

    #[test]
    fn toolbar_states() {
        assert_eq!(toolbar(false, false, false, false), Toolbar::default());
        let running = toolbar(true, true, true, false);
        assert!(running.rerun && running.pause && running.stop && !running.paused);
        let paused = toolbar(true, true, false, true);
        assert!(paused.paused && !paused.pause && paused.stop);
        let ended = toolbar(true, false, false, false);
        assert!(ended.rerun && !ended.stop && !ended.pause && !ended.paused);
    }

    #[test]
    fn muted_breakpoints_are_not_sent() {
        let mut all = BTreeMap::new();
        all.insert(PathBuf::from("/w/a.c"), vec![SourceBreakpoint::from(3)]);
        assert!(unless_muted(true, all.clone()).is_empty());
        assert_eq!(unless_muted(false, all.clone()), all);
        let special = idecraft_debugger::SpecialBreakpoints {
            caught: true,
            ..Default::default()
        };
        assert!(unless_muted(true, special.clone()).is_empty());
        assert_eq!(
            unless_muted(true, vec![SourceBreakpoint::from(3)]),
            Vec::<SourceBreakpoint>::new(),
            "a file's breakpoints go as an empty list"
        );
    }

    #[test]
    fn library_frames_by_hint_source_and_name() {
        let root = std::env::temp_dir().join(format!("idecraft-frames-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let own = root.join("Dbg.java");
        std::fs::write(&own, "class Dbg {}\n").unwrap();
        let frame = |name: &str, path: Option<PathBuf>, deemphasized: bool| Frame {
            id: 1,
            name: name.into(),
            path,
            line: 1,
            deemphasized,
        };
        let outside =
            std::env::temp_dir().join(format!("idecraft-lib-{}.java", std::process::id()));
        std::fs::write(&outside, "class Lib {}\n").unwrap();
        assert!(!library_frame(
            &frame("Dbg.main(String[])", Some(own.clone()), false),
            &root,
            false
        ));
        assert!(
            library_frame(&frame("Dbg.main", Some(own.clone()), true), &root, false),
            "hint"
        );
        assert!(library_frame(
            &frame("ReferencePipeline$4$1.accept(Object)", None, false),
            &root,
            false
        ));
        assert!(
            library_frame(
                &frame(
                    "IntPipeline.sum()",
                    Some("jdt://contents/rt.jar/IntPipeline.class".into()),
                    false
                ),
                &root,
                false
            ),
            "not a file"
        );
        assert!(
            library_frame(
                &frame("Lib.run", Some(outside.clone()), false),
                &root,
                false
            ),
            "outside"
        );
        for generated in [
            "Dbg$$Lambda/0x000001.applyAsInt(Object)",
            "Dbg.access$000(Dbg)",
            "java.util.stream.IntPipeline.sum()",
            "jdk.internal.reflect.Method.invoke",
        ] {
            assert!(
                library_frame(&frame(generated, Some(own.clone()), false), &root, false),
                "{generated}"
            );
        }
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_remote_projects_frames_are_project_frames_without_a_local_file() {
        let stand_in = std::path::Path::new("/nonexistent/stand-in/app");
        let frame = |path: &str| Frame {
            id: 1,
            name: "main".into(),
            path: Some(path.into()),
            line: 1,
            deemphasized: false,
        };
        let own = frame("/nonexistent/stand-in/app/src/main.py");
        assert!(
            !library_frame(&own, stand_in, true),
            "no file here, still ours"
        );
        assert!(
            library_frame(&own, stand_in, false),
            "locally it needs the file"
        );
        assert!(
            library_frame(&frame("/usr/lib/python3/os.py"), stand_in, true),
            "a host path outside the project"
        );
    }

    #[test]
    fn hidden_frames_fold_and_expand() {
        let library = [true, true, true, false, true, false];
        let none = HashSet::new();
        assert_eq!(
            frame_rows(library.len(), &library, 0, &none),
            [
                FrameRow::Frame(0),
                FrameRow::Hidden { first: 1, count: 2 },
                FrameRow::Frame(3),
                FrameRow::Hidden { first: 4, count: 1 },
                FrameRow::Frame(5),
            ],
            "the top frame never folds"
        );
        assert_eq!(
            frame_rows(library.len(), &library, 2, &none),
            [
                FrameRow::Frame(0),
                FrameRow::Hidden { first: 1, count: 1 },
                FrameRow::Frame(2),
                FrameRow::Frame(3),
                FrameRow::Hidden { first: 4, count: 1 },
                FrameRow::Frame(5),
            ],
            "nor the selected one"
        );
        let opened: HashSet<usize> = [1].into();
        assert_eq!(
            frame_rows(library.len(), &library, 0, &opened)[..4],
            [
                FrameRow::Frame(0),
                FrameRow::Frame(1),
                FrameRow::Frame(2),
                FrameRow::Frame(3)
            ]
        );
        // The self-test crashed (index out of bounds) when the frames were cleared while the
        // library marks of the previous stack were still there: rows follow the frames.
        assert!(frame_rows(0, &library, 0, &none).is_empty());
        assert_eq!(
            frame_rows(2, &library, 0, &none),
            [FrameRow::Frame(0), FrameRow::Hidden { first: 1, count: 1 }]
        );
        assert_eq!(
            frame_rows(3, &[], 0, &none),
            [FrameRow::Frame(0), FrameRow::Frame(1), FrameRow::Frame(2)]
        );
        assert_eq!(hidden_label(1), "1 hidden frame");
        assert_eq!(hidden_label(7), "7 hidden frames");
    }

    #[test]
    fn method_lines_by_braces_and_indentation() {
        let java = [
            "record Item(int qty, int unitPriceCents) {",
            "    int totalCents() {",
            "        int sum = 0;",
            "        return qty * unitPriceCents;",
            "    }",
            "    int other() { return sku; }",
            "}",
        ];
        assert_eq!(function_name("Inventory$Item.totalCents()"), "totalCents");
        assert_eq!(function_name("ns::twice(int)"), "twice");
        assert_eq!(function_name("twice"), "twice");
        assert_eq!(method_lines(&java, 3, "totalCents"), 1..=4);
        let python = [
            "class Item:",
            "    def total(self):",
            "        n = self.qty",
            "",
            "        return n * self.price",
            "    def other(self):",
            "        return self.sku",
        ];
        assert_eq!(method_lines(&python, 2, "total"), 1..=4);
        assert_eq!(
            method_lines(&java, 3, "missing"),
            0..=3,
            "without a declaration the lines above count"
        );
        let brace_below = ["void f()", "{", "    x = 1;", "}", "y = 2;"];
        assert_eq!(method_lines(&brace_below, 2, "f"), 0..=3);
    }

    #[test]
    fn fields_the_method_uses() {
        let lines = [
            "    int totalCents() {",
            "        return qty * unitPriceCents;",
            "    }",
            "    String label() { return sku; }",
        ];
        let fields = [
            var("sku", 0),
            var("qty", 0),
            var("unitPriceCents", 0),
            var("qt", 0),
        ];
        assert_eq!(
            fields_used(&lines, 0..=2, &fields, &[var("this", 4)]),
            ["qty", "unitPriceCents"],
            "whole words of the method only, in the object's order"
        );
        assert_eq!(
            fields_used(&lines, 0..=2, &fields, &[var("qty", 0)]),
            ["unitPriceCents"],
            "a local hides the field"
        );
    }

    #[test]
    fn the_object_id_is_the_dimmed_part() {
        let (text, id) = row_value("ArrayList@566 size = 1");
        assert_eq!(&text[id.unwrap()], "@566");
        let (text, id) = row_value("Item@1f\n\"Item[\n  a@b]\"");
        assert_eq!(text, "Item@1f \"Item[ a@b]\"");
        assert_eq!(&text[id.unwrap()], "@1f");
        assert_eq!(row_value("\"Rex\"").1, None);
        assert_eq!(row_value("300").1, None);
    }

    #[test]
    fn object_text_after_the_value() {
        assert_eq!(
            object_text("Inventory$Item@15", "\"Item[qty=3, unitPriceCents=25]\""),
            Some("Inventory$Item@15 \"Item[qty=3, unitPriceCents=25]\"".to_owned())
        );
        assert_eq!(object_text("Dbg@3", "Dbg@3"), None, "nothing new");
        // kotlin-debug-adapter's answer to a call it cannot evaluate; an object's own text
        // "unknown" comes in quotes and is shown.
        assert_eq!(object_text("Dog@568", "unknown"), None);
        assert_eq!(
            object_text("Dog@568", "\"unknown\"").as_deref(),
            Some("Dog@568 \"unknown\"")
        );
        assert_eq!(object_text("Dbg@3", "\"\""), None);
        // The adapter's value already carries the text (java-debug `showToString`, v11).
        assert_eq!(
            object_text("Item@4 \"Item[qty=3]\"", "\"Item[qty=3]\""),
            None
        );
        let long = object_text("A@1", &"x".repeat(150)).unwrap();
        assert!(long.ends_with("…\""));
        assert_eq!(long.chars().count(), "A@1 \"".len() + OBJECT_TEXT_CHARS + 2);
    }

    #[test]
    fn object_fields_at_the_top_level() {
        let mut tree = VarTree::new(vec![var("this", 4), var("n", 0)]);
        let field = |n: &str, v: &str| Variable {
            name: n.into(),
            value: v.into(),
            kind: Some("int".into()),
            reference: 0,
            evaluate_name: None,
        };
        tree.set_object(&ObjectFields {
            frame: 1,
            owner: 4,
            fields: vec![field("sku", "\"A-1\""), field("qty", "3"), field("n", "9")],
            used: vec!["qty".into(), "n".into()],
            text: Some("Item@4 \"Item[qty=3]\"".into()),
        });
        let rows = tree.rows();
        let shown: Vec<(&str, &str, i64)> = rows
            .iter()
            .map(|r| (r.var.name.as_str(), r.var.value.as_str(), r.parent))
            .collect();
        assert_eq!(
            shown,
            [
                ("this", "Item@4 \"Item[qty=3]\"", 0),
                ("n", "1", 0),
                ("qty", "3", 4),
            ],
            "used fields after the locals, the object as their container, the local n hides its field"
        );
        tree.set_children(4, vec![field("sku", "\"A-1\""), field("qty", "42")]);
        assert!(
            tree.rows()
                .iter()
                .any(|r| r.var.name == "qty" && r.var.value == "42")
        );
        let mut fresh = var("this", 4);
        fresh.value = "Item@4".into();
        tree.refresh_values(&[fresh]);
        assert_eq!(
            tree.rows()[0].var.value,
            "Item@4 \"Item[qty=3]\"",
            "the text stays"
        );
    }

    #[test]
    fn object_fields_only_for_this_or_self() {
        let locals = [var("x", 0), var("this", 5)];
        assert_eq!(object_of(Kind::Java, &locals), Some(5));
        assert_eq!(object_of(Kind::Python, &[var("self", 6)]), Some(6));
        assert_eq!(object_of(Kind::Node, &locals), None, "not JavaScript");
        assert_eq!(
            object_of(Kind::Java, &[var("this", 0)]),
            None,
            "no children"
        );
        assert_eq!(object_of(Kind::Native, &[var("other", 3)]), None);
    }

    #[test]
    fn node_scripts() {
        let cmd = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let dir = std::path::Path::new("/work");
        let (kind, _, program, args) = plan(&cmd(&["node", "src/app.js", "x"]), dir).unwrap();
        assert_eq!(
            (kind, program, args),
            (
                Kind::Node,
                PathBuf::from("/work/src/app.js"),
                vec!["x".to_owned()]
            )
        );
        assert!(plan(&cmd(&["node", "--version"]), dir).is_none());
        let launch = Launch {
            kind: Kind::Node,
            adapter: Vec::new(),
            interpreter: None,
            program: PathBuf::from("/work/src/app.js"),
            args: Vec::new(),
            cwd: PathBuf::from("/work"),
            env: Default::default(),
            breakpoints: Default::default(),
            prepare: String::new(),
            special: Default::default(),
            attach: None,
            browser: None,
            remote: None,
        };
        let a = arguments(&launch);
        assert_eq!(a["type"], "pwa-node");
        assert_eq!(a["name"], "app.js");
    }

    /// SPEC-0168 AK-1: package scripts and TypeScript files.
    #[test]
    fn package_scripts_and_typescript() {
        let cmd = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let dir = std::path::Path::new("/work");
        let (kind, _, program, _) = plan(&cmd(&["node", "src/app.ts"]), dir).unwrap();
        assert_eq!(
            (kind, program),
            (Kind::Node, PathBuf::from("/work/src/app.ts"))
        );
        for (command, args) in [
            (&["npm", "run", "dev"][..], &["run", "dev"][..]),
            (
                &["pnpm", "run", "build", "--", "-w"],
                &["run", "build", "--", "-w"],
            ),
            (&["yarn", "run", "serve"], &["run", "serve"]),
            (&["bun", "run", "dev"], &["run", "dev"]),
            (&["npm", "start"], &["run", "start"]),
            (&["npm", "test", "--", "x"], &["run", "test", "--", "x"]),
        ] {
            let (kind, _, program, got) = plan(&cmd(command), dir).unwrap();
            assert_eq!(kind, Kind::PackageScript, "{command:?}");
            assert_eq!(got, cmd(args), "{command:?}");
            assert!(program.ends_with(command[0]), "{program:?}");
        }
        for command in [&["npm", "install"][..], &["npm", "run"], &["yarn", "start"]] {
            assert!(
                plan(&cmd(command), dir).is_none_or(|p| p.0 != Kind::PackageScript),
                "{command:?}"
            );
        }
        let launch = Launch {
            kind: Kind::PackageScript,
            adapter: Vec::new(),
            interpreter: None,
            program: PathBuf::from("/bin/npm"),
            args: cmd(&["run", "dev"]),
            cwd: PathBuf::from("/work"),
            env: Default::default(),
            breakpoints: Default::default(),
            prepare: String::new(),
            special: Default::default(),
            attach: None,
            browser: None,
            remote: None,
        };
        let a = arguments(&launch);
        assert_eq!(a["runtimeExecutable"], "/bin/npm");
        assert_eq!(a["runtimeArgs"], json!(["run", "dev"]));
        assert_eq!(a["name"], "npm run dev");
        assert!(a.get("program").is_none());
    }

    #[test]
    fn java_main_classes() {
        let dir = std::env::temp_dir().join(format!("idecraft-javaplan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Dbg.java"), "package demo;\nclass Dbg {}\n").unwrap();
        let cmd = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (kind, _, class, args) = plan(&cmd(&["java", "Dbg.java", "x"]), &dir).unwrap();
        assert_eq!(
            (kind, class, args),
            (Kind::Java, PathBuf::from("demo.Dbg"), vec!["x".to_owned()])
        );
        let (kind, _, class, _) = plan(
            &cmd(&[
                "mvn",
                "-q",
                "compile",
                "exec:java",
                "-Dexec.mainClass=a.App",
            ]),
            &dir,
        )
        .unwrap();
        assert_eq!((kind, class), (Kind::Java, PathBuf::from("a.App")));
        let launch = Launch {
            kind: Kind::Java,
            adapter: vec![],
            interpreter: None,
            program: PathBuf::from("a.App"),
            args: vec!["1".into(), "2".into()],
            cwd: dir.clone(),
            env: BTreeMap::new(),
            breakpoints: BTreeMap::new(),
            prepare: String::new(),
            special: Default::default(),
            attach: None,
            browser: None,
            remote: None,
        };
        let a = arguments(&launch);
        assert_eq!(
            (a["mainClass"].as_str(), a["args"].as_str()),
            (Some("a.App"), Some("1 2"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debugger_errors_shrink_to_one_line() {
        let lldb = "evaluate failed: error: <user expression 0>:1:9: expected expression\n    1 | breite +\n      |         ^\n    2 | ;";
        assert_eq!(error_summary(lldb), "expected expression");
        assert_eq!(
            error_summary("evaluate failed: NameError: name 'x' is not defined"),
            "evaluate failed: NameError: name 'x' is not defined"
        );
        assert_eq!(error_summary(""), "");
    }

    /// SPEC-0091 v12 behaviours 26, 32–34, 38: the dump's text, filters and overrides of the
    /// variables tree, the adapter's expression of a row, files named in console lines.
    #[test]
    fn dump_filters_overrides_and_files() {
        let frame = Frame {
            id: 1,
            name: "Hot.main(String[])".into(),
            path: Some(PathBuf::from("/w/Hot.java")),
            line: 15,
            deemphasized: false,
        };
        let worker = idecraft_debugger::ThreadInfo {
            id: 9,
            name: "worker-1".into(),
            state: "WAIT".into(),
            group: "main".into(),
            suspended: false,
        };
        let main = idecraft_debugger::ThreadInfo {
            id: 1,
            name: "main".into(),
            suspended: true,
            ..Default::default()
        };
        let text = dump_text(&[(main, vec![frame]), (worker, Vec::new())]);
        assert_eq!(
            text,
            "Thread dump: 2 threads\n\"main\"\n\tat Hot.main(String[]) (Hot.java:15)\n\n\"worker-1\" WAIT (group main)\n\trunning\n\n"
        );
        assert_eq!(
            idecraft_debugger::extras::stack_location("\tat Hot.main(String[]) (Hot.java:15)"),
            Some(("Hot.java".into(), 15))
        );
        // v13: a JDK frame names its class file, not jdtls's address.
        assert_eq!(
            source_file_name("Reference.class?=java.base/%3Cjava.lang.ref(Reference.class"),
            "Reference.class"
        );
        assert_eq!(source_file_name("Hot.java"), "Hot.java");
        let mut none = var("next", 0);
        none.value = "null".into();
        let mut items = var("items", 5);
        items.evaluate_name = Some("items".into());
        let mut tree = VarTree::new(vec![var("this$0", 0), none, items]);
        tree.set_filters(idecraft_debugger::extras::VarFilters {
            statics: true,
            synthetic: false,
            nulls: false,
        });
        let names: Vec<String> = tree.rows().into_iter().map(|r| r.var.name).collect();
        assert_eq!(names, ["items"]);
        tree.set_override((0, "items".into()), Some(("[1, 2]".into(), Some(9))));
        let rows = tree.rows();
        assert_eq!(
            (rows[0].var.value.as_str(), rows[0].var.reference),
            ("[1, 2]", 9)
        );
        assert_eq!(tree.expand(9), Some(9), "the view's children are fetched");
        tree.set_children(9, vec![var("0", 0)]);
        let rows = tree.rows();
        assert_eq!(rows.len(), 2);
        let mut child = rows[1].clone();
        child.var.evaluate_name = Some("items.get(0)".into());
        assert_eq!(row_expression(&[rows[0].clone(), child], 1), "items.get(0)");
        tree.set_override((0, "items".into()), None);
        assert_eq!(tree.rows()[0].var.value, "1");

        let root = std::env::temp_dir().join(format!("idecraft-findfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/dbg")).unwrap();
        std::fs::create_dir_all(root.join("target/x")).unwrap();
        std::fs::write(root.join("target/x/Hot.java"), "").unwrap();
        std::fs::write(root.join("src/dbg/Hot.java"), "").unwrap();
        assert_eq!(
            find_file_named(&root, "Hot.java", 6),
            Some(root.join("src/dbg/Hot.java"))
        );
        assert_eq!(find_file_named(&root, "Hot.java", 1), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Self-test steps "paused in a record's method …": against real jdtls with java-debug, the
    /// stop in a record's method reports the record's fields, the ones the line uses and the
    /// record's text. Skipped without jdtls or the plugin.
    #[test]
    fn a_java_stop_in_a_records_method_reports_the_fields_it_uses() {
        if idecraft_languages::java_debug::java_debug_plugin().is_none()
            || !idecraft_languages::java_debug::java_debug_usable()
            || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
        {
            eprintln!("skipped: jdtls or java-debug is missing");
            return;
        }
        let dir = std::env::temp_dir().join(format!("idecraft-javarecord-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.canonicalize().unwrap();
        idecraft_core::trust::trust_for_process(&root);
        let file = root.join("Dbg.java");
        std::fs::write(
            &file,
            "public class Dbg {\n    record Item(int qty, int unitPriceCents) {\n        int totalCents() {\n            return qty * unitPriceCents;\n        }\n    }\n    public static void main(String[] args) {\n        System.out.println(\"result \" + java.util.stream.Stream.of(new Item(3, 25)).mapToInt(Item::totalCents).sum());\n    }\n}\n",
        )
        .unwrap();
        let mut breakpoints = BTreeMap::new();
        breakpoints.insert(
            file.clone(),
            vec![SourceBreakpoint {
                line: 4,
                ..Default::default()
            }],
        );
        let c = Controller::start(Launch {
            kind: Kind::Java,
            adapter: Vec::new(),
            interpreter: None,
            program: PathBuf::from("Dbg"),
            args: Vec::new(),
            cwd: root.clone(),
            env: BTreeMap::new(),
            breakpoints,
            prepare: String::new(),
            special: Default::default(),
            attach: None,
            browser: None,
            remote: None,
        });
        let until = std::time::Instant::now() + Duration::from_secs(180);
        let mut seen = Vec::new();
        let fields = loop {
            assert!(std::time::Instant::now() < until, "no fields: {seen:#?}");
            let mut found = None;
            for u in c.poll() {
                if let Update::Failed(e) = &u {
                    panic!("failed: {e}\n{seen:#?}");
                }
                if let Update::Fields(f) = &u {
                    found = Some(f.clone());
                }
                seen.push(u);
            }
            if let Some(f) = found {
                break f;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        c.send(Command::Stop);
        let values: Vec<(&str, &str)> = fields
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.value.as_str()))
            .collect();
        assert!(values.contains(&("qty", "3")), "{fields:#?}\n{seen:#?}");
        assert_eq!(fields.used, ["qty", "unitPriceCents"], "{fields:#?}");
        // The record's text comes either with java-debug's value of `this` or from `toString()`
        // in `fields.text` (v11 behaviour 17); the rows below show it either way.
        // What the window shows from it: the editor's values after the line and the variables.
        let locals = seen
            .iter()
            .find_map(|u| match u {
                Update::Stopped { locals, .. } => Some(locals.clone()),
                _ => None,
            })
            .unwrap();
        let mut tree = VarTree::new(locals);
        tree.set_object(&fields);
        let texts: Vec<String> = tree
            .rows()
            .iter()
            .map(|r| format!("{} = {}", r.var.name, r.var.value))
            .collect();
        assert!(texts.iter().any(|t| t == "qty = 3"), "{texts:?}");
        assert!(
            texts.iter().any(|t| t == "unitPriceCents = 25"),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("this = ") && t.contains("\"Item[qty=3")),
            "{texts:?}"
        );
        let top: Vec<(String, String)> = tree
            .rows()
            .iter()
            .filter(|r| r.depth == 0)
            .map(|r| (r.var.name.clone(), r.var.value.clone()))
            .collect();
        let source = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = source.lines().collect();
        assert!(
            inline_values(&lines, 3, &editor_values(&top, &fields.fields))
                .contains(&(3, "qty: 3  unitPriceCents: 25".to_owned()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

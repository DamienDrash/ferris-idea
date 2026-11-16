//! Command-line entry point. Until the UI exists (M3), `idecraft` opens a
//! workspace headlessly (`open`) or serves it to LLM agents over MCP on
//! stdin/stdout (`mcp`, ADR-0008).

// The self-test chains its steps of every feature into one iterator, deeper than the default.
#![recursion_limit = "256"]

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod app_icon;
mod drive;
#[cfg(all(test, target_os = "macos"))]
mod f084_handtests;
mod launch;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod mcp_http;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native_click;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod no_window;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod selftest_groups;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod spike;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod spike_editor;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod spike_input;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod spike_layout;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod termination;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod title_bar;
mod traffic_lights;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod ui_driver;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod workbench;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use idecraft_core::{Layer, LayeredConfig, Workspace};
use idecraft_languages::LanguageId;

const DEFAULT_SETTINGS: &str = "[editor]\ntab_size = 4\ninsert_spaces = true\n";

fn main() -> ExitCode {
    let mut args = launch::without_launch_arguments(std::env::args().skip(1).collect());
    // Started from the app bundle (Finder, Dock) there is no command line (SPEC-0282 behaviour 7).
    if args.is_empty() && launch::started_from_bundle() {
        args.push("launch".into());
    }
    adopt_login_shell_environment(&args);
    // `--trust` after `ui <folder>` or `mcp <folder>` trusts that folder for this run only
    // (SPEC-0278 behaviour 10).
    let trust_flag = matches!(args.first().map(String::as_str), Some("ui" | "mcp"))
        && args.iter().any(|a| a == "--trust");
    if trust_flag {
        args.retain(|a| a != "--trust");
        let folder = args
            .get(1)
            .filter(|a| !a.starts_with("--"))
            .map_or(".", String::as_str);
        // `ui <file> --trust` trusts the file's project, the folder the window opens
        // (SPEC-0094 v7 behaviour 18).
        let folder = match idecraft_core::project_root::open_target(Path::new(folder)) {
            Ok(target) if args[0] == "ui" => target.root,
            _ => Path::new(folder).to_path_buf(),
        };
        idecraft_core::trust::trust_for_process(&folder);
    }
    match args.as_slice() {
        [cmd, flag, file] if cmd == "open" && flag == "--print" => print_preview(Path::new(file)),
        [cmd, path]
            if cmd == "open" && idecraft_core::format::is_exchange_file(Path::new(path)) =>
        {
            open_file(Path::new(path))
        }
        [cmd, path] if cmd == "open" => match open(Path::new(path)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        [cmd, path] if cmd == "mcp" => match mcp(Path::new(path)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        [cmd, rest @ ..] if cmd == "trust" => trust_cli(rest),
        [cmd, rest @ ..] if cmd == "serve" => idecraft_serve::serve_cli(rest),
        [cmd, rest @ ..] if cmd == "device" => idecraft_serve::device::run(rest),
        [cmd, rest @ ..] if cmd == "quit" => quit_cli(rest),
        // The UI driver for scripts (SPEC-0319 behaviour 30).
        [cmd, rest @ ..] if cmd == "drive" => drive::drive_cli(rest),
        [cmd, folder, tool, rest @ ..] if cmd == "call" && rest.len() <= 1 => {
            call_cli(folder, tool, rest.first().map(String::as_str))
        }
        [cmd, rest @ ..] if cmd == "launch" && rest.len() <= 1 => {
            gui(|| workbench::launch(rest.first().map(Path::new)))
        }
        [cmd, engine, container, ssh_extra @ ..] if cmd == "container-exec" => {
            container_exec(engine, container, ssh_extra)
        }
        [cmd, rest @ ..] if cmd == "spike-scroll" && rest.len() <= 1 => spike_scroll(rest.first()),
        [cmd] if cmd == "spike-input" => spike_input(),
        // The self-test's groups with their step counts, without a window.
        [cmd, selftest, list]
            if cmd == "ui" && selftest == "--selftest" && list == "--list-groups" =>
        {
            gui(workbench::list_groups)
        }
        [cmd, rest @ ..] if cmd == "ui" && rest.len() <= 1 => {
            let selftest = rest.first().is_some_and(|a| a == "--selftest");
            let root = rest
                .first()
                .filter(|_| !selftest)
                .map_or(".", String::as_str);
            gui(|| workbench::run(Path::new(root), selftest))
        }
        [cmd, file] if cmd == "edit" => gui(|| spike_editor::edit(Path::new(file))),
        [cmd, rest @ ..] if cmd == "spike-editor" && rest.len() <= 1 => {
            gui(|| spike_editor::measure(rest.first().map(Path::new)))
        }
        [cmd, rest @ ..] if cmd == "spike-layout" && rest.len() <= 2 => spike_layout(rest),
        [flag] if flag == "--version" => {
            println!("{}", idecraft_core::release::version_line());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: idecraft open <directory> | idecraft open [--print] <file.idc…> | idecraft mcp <directory> [--trust] | idecraft serve [<directory>] --stdio [--state-dir <dir>] [--run-dir <dir>] [--reattach-minutes <n>] | idecraft serve [<directory>] --pair [--listen <addr>:<port>] [--allow-network] [--new-device] [--grant <rights>] [--key-file] | idecraft serve --devices | idecraft serve --revoke <fingerprint> | idecraft serve --attach <session> [--run-dir <dir>] | idecraft trust status|add|list|remove … | idecraft quit [--save-all | --discard | --force] [--pid <pid>] | idecraft call <directory> <tool> [<json arguments>] | idecraft drive [--project <folder>] [--json] <verb> … | idecraft launch [folder] | idecraft container-exec <engine> <container> | idecraft spike-scroll [file] | idecraft ui [folder [--trust] | --selftest [--list-groups]] | idecraft edit <file> | idecraft spike-input | idecraft spike-editor [file] | idecraft spike-layout [--interactive] [directory] | idecraft --version"
            );
            ExitCode::from(2)
        }
    }
}

/// Gives a window that was not started from a shell the environment of the user's login shell,
/// so tools are found as in a terminal (`SPEC-0328`). Must stay the first thing `main` does after
/// reading its arguments.
fn adopt_login_shell_environment(args: &[String]) {
    use idecraft_core::shell_env::{self, Request, Source};
    let mut request = Request::for_process(launch::opens_window(args));
    #[cfg(unix)]
    {
        request.new_session = Some(|shell| {
            use std::os::unix::process::CommandExt;
            // SAFETY: `setsid` is async-signal-safe and the closure does nothing else, which is
            // all that may run between fork and exec. In a session of its own the shell has no
            // terminal to take over and is not stopped for trying.
            unsafe {
                shell.pre_exec(|| {
                    nix::unistd::setsid()
                        .map(drop)
                        .map_err(std::io::Error::from)
                });
            }
        });
    }
    let resolution = shell_env::resolve(&request);
    for (name, value) in &resolution.set {
        // SAFETY: `main` calls this before anything else could have started a thread, and
        // `resolve` reads the shell's answer on this thread, so nothing reads or writes the
        // environment at the same time.
        unsafe { std::env::set_var(name, value) };
    }
    let report = resolution.report;
    if report.source != Source::Inherited {
        // Source, duration and counts only; never a value (behaviour 9).
        let entries = std::env::var_os("PATH").map_or(0, |p| std::env::split_paths(&p).count());
        if let Some(dir) = idecraft_core::logs::log_dir() {
            let line = shell_env::log_line(&report, entries, std::time::SystemTime::now());
            let _ = idecraft_core::logs::append_line(&dir.join(shell_env::ENVIRONMENT_LOG), &line);
        }
        if report.source == Source::Fallback {
            eprintln!("warning: {}", report.note);
        }
    }
    shell_env::record(report);
}

/// An interactive shell inside a container on this terminal, bridged over the Engine API
/// (SPEC-0240 behaviour 2); the Containers window runs it in a terminal tab.
fn container_exec(engine: &str, container: &str, ssh_extra: &[String]) -> ExitCode {
    let Some(host) = idecraft_containers::DockerHost::parse(engine, ssh_extra.to_vec()) else {
        eprintln!("error: not a Docker engine address: {engine}");
        return ExitCode::from(2);
    };
    #[cfg(unix)]
    {
        let code = idecraft_containers::exec::run_in_terminal(&host, container);
        ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(1))
    }
    #[cfg(not(unix))]
    {
        let _ = (host, container);
        eprintln!("error: container shells need macOS or Linux");
        ExitCode::FAILURE
    }
}

/// Runs a desktop window (macOS and Linux only).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gui(run: impl FnOnce() -> Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn gui(_run: impl FnOnce() -> Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    eprintln!("error: the desktop views run on macOS and Linux");
    ExitCode::FAILURE
}

/// M3.4 spike: scroll a large file in a GPUI window and print frame timings (macOS and Linux).
fn spike_scroll(file: Option<&String>) -> ExitCode {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match spike::scroll(file.map(Path::new)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = file;
        eprintln!("error: the GPUI spike runs on macOS and Linux");
        ExitCode::FAILURE
    }
}

/// M3.4 spike: text field for input methods (umlauts, dead keys, IME) with latency measurement.
fn spike_input() -> ExitCode {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match spike_input::input() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        eprintln!("error: the GPUI spike runs on macOS and Linux");
        ExitCode::FAILURE
    }
}

/// M3.4 spike: two windows with split panes, a file tree and a 100 000-row table.
fn spike_layout(args: &[String]) -> ExitCode {
    let interactive = args.iter().any(|a| a == "--interactive");
    let root = args
        .iter()
        .find(|a| *a != "--interactive")
        .map_or(".", String::as_str);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match spike_layout::layout(Path::new(root), interactive) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (root, interactive);
        eprintln!("error: the GPUI spike runs on macOS and Linux");
        ExitCode::FAILURE
    }
}

/// The folder argument of `mcp`, `call` and `open` (`SPEC-0094` v7 behaviour 20): a file is
/// refused with its project named, never served as a project and never widened to one.
fn folder_argument(path: &Path) -> Result<Workspace, String> {
    if path.is_file() {
        return Err(idecraft_core::project_root::file_given_for_folder(path));
    }
    Workspace::open(path).map_err(|e| e.to_string())
}

fn open(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = folder_argument(path)?;
    let mut config = LayeredConfig::new();
    config.set_layer_toml(Layer::Defaults, DEFAULT_SETTINGS)?;
    let has_settings = workspace.load_settings(&mut config)?;
    let files = workspace.files()?;

    let mut by_language: BTreeMap<&'static str, usize> = BTreeMap::new();
    for file in &files {
        let key = LanguageId::detect(file).map_or("other", LanguageId::lsp_id);
        *by_language.entry(key).or_default() += 1;
    }

    println!(
        "workspace: {} ({})",
        workspace.name(),
        workspace.root().display()
    );
    println!(
        "settings:  {}, editor.tab_size = {}",
        if has_settings {
            "workspace file loaded"
        } else {
            "defaults"
        },
        config.get::<i64>("editor.tab_size")?.unwrap_or_default()
    );
    println!("files:     {}", files.len());
    for (language, count) in by_language {
        println!("  {language:<12} {count}");
    }
    Ok(())
}

/// `idecraft open --print <file>`: the import preview of an exchange file as text, against the
/// current folder and the user's settings (`SPEC-0303` behaviour 17). Exit 0 without problems, 1
/// with problems, 2 when the file is not an exchange file.
fn print_preview(file: &Path) -> ExitCode {
    if !idecraft_core::format::is_exchange_file(file) {
        eprintln!(
            "error: {} is not an IDECraft exchange file (.idcrun, .idckeymap, .idctheme, .idcsources, .idcsettings, …)",
            file.display()
        );
        return ExitCode::from(2);
    }
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let preview = idecraft_formats::preview_file(file, &idecraft_formats::Targets::user(&root));
    for line in preview.lines() {
        println!("{line}");
    }
    if preview.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `idecraft open <file>`: an exchange file's preview in the running window, else in a window this
/// starts (`SPEC-0303` behaviour 17). Other files are refused as before (`open` takes folders).
fn open_file(file: &Path) -> ExitCode {
    let Ok(path) = file.canonicalize() else {
        eprintln!("error: {}: cannot be read", file.display());
        return ExitCode::FAILURE;
    };
    let windows = idecraft_core::user_config_dir()
        .map(|c| idecraft_mcp::client::running_windows(&c))
        .unwrap_or_default();
    if let Some(window) = windows.last() {
        let shown =
            idecraft_mcp::client::Client::connect(window, "idecraft-cli").and_then(|mut c| {
                c.call(
                    "import_file",
                    serde_json::json!({ "path": path.display().to_string(), "show": true }),
                )
            });
        match shown {
            Ok(reply) => {
                println!(
                    "shown in IDECraft (pid {}, {}); confirm the import there",
                    window.pid,
                    reply["project"].as_str().unwrap_or_default()
                );
                return ExitCode::SUCCESS;
            }
            Err(e) => eprintln!("warning: the running window did not take it ({e}); starting one"),
        }
    }
    gui(|| workbench::launch_with_import(&path))
}

fn mcp(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = folder_argument(path)?;
    // Untrusted projects are served in safe mode (SPEC-0278 behaviour 11).
    idecraft_core::trust::enforce();
    let runtime = tokio::runtime::Runtime::new()?;
    // Agents asking about code reuse running language servers (SPEC-0090).
    idecraft_languages::pool::enable();
    let served = runtime.block_on(idecraft_mcp::serve_stdio(workspace));
    idecraft_languages::pool::shutdown();
    served.map_err(|e| e.to_string().into())
}

/// `idecraft call <folder> <tool> [<json>]`: one MCP tool call from a shell, with the rights and
/// safe mode of `idecraft mcp` (SPEC-0281 behaviour 5). Exit 0 with the result on standard output,
/// 1 when the tool refuses or fails, 2 for wrong arguments or a folder that cannot be opened.
fn call_cli(folder: &str, tool: &str, arguments: Option<&str>) -> ExitCode {
    use idecraft_mcp::call::{Answer, call_once};
    let arguments = match arguments.map(serde_json::from_str::<serde_json::Value>) {
        None => serde_json::json!({}),
        Some(Ok(v)) if v.is_object() => v,
        _ => {
            eprintln!("error: the arguments must be a JSON object, e.g. '{{\"path\": \"a.txt\"}}'");
            return ExitCode::from(2);
        }
    };
    let workspace = match folder_argument(Path::new(folder)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("error: {folder}: {e}");
            return ExitCode::from(2);
        }
    };
    // Untrusted projects are served in safe mode (SPEC-0278 behaviour 11).
    idecraft_core::trust::enforce();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    idecraft_languages::pool::enable();
    let server = idecraft_mcp::IdeServer::new(workspace);
    let answer = runtime.block_on(call_once(server, tool, arguments));
    idecraft_languages::pool::shutdown();
    match answer {
        Ok(Answer::Ok(value)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
            ExitCode::SUCCESS
        }
        Ok(Answer::ToolError(message)) | Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `idecraft quit [--save-all | --discard | --force] [--pid <pid>]`: quits the running window through
/// its MCP server over HTTP (SPEC-0261 v2 behaviour 11). Exit status 0 when it quit, 1 when it did not
/// (no window, refused, cancelled), 2 for pending work without a terminal to ask on.
fn quit_cli(args: &[String]) -> ExitCode {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    let terminal = stdin.is_terminal();
    let (mut input, mut out, mut err) = (stdin.lock(), std::io::stdout(), std::io::stderr());
    ExitCode::from(quit_with(
        args,
        idecraft_core::user_config_dir().as_deref(),
        &mut Console {
            terminal,
            input: &mut input,
            out: &mut out,
            err: &mut err,
        },
    ))
}

/// The terminal `idecraft quit` talks to: the process's own streams, or a scripted one in the
/// window tests of the hand tests (`f084_handtests`).
pub(crate) struct Console<'a> {
    /// Whether standard input is a terminal the command may ask on.
    pub(crate) terminal: bool,
    /// Where the answer is read from.
    pub(crate) input: &'a mut dyn std::io::BufRead,
    /// Standard output.
    pub(crate) out: &'a mut dyn std::io::Write,
    /// Standard error.
    pub(crate) err: &'a mut dyn std::io::Write,
}

/// [`quit_cli`] with the settings folder `config` (where the windows' discovery files are) and
/// `console`; returns the exit status.
pub(crate) fn quit_with(args: &[String], config: Option<&Path>, console: &mut Console) -> u8 {
    let usage = |console: &mut Console| {
        let _ = writeln!(
            console.err,
            "usage: idecraft quit [--save-all | --discard | --force] [--pid <pid>]"
        );
        2
    };
    let mut pending = "ask";
    let mut pid: Option<u32> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--save-all" => pending = "save_all",
            "--discard" | "--force" => pending = "discard",
            "--pid" => match it.next().and_then(|p| p.parse().ok()) {
                Some(p) => pid = Some(p),
                None => return usage(console),
            },
            _ => return usage(console),
        }
    }
    let fail = |m: String, err: &mut dyn std::io::Write| {
        let _ = writeln!(err, "error: {m}");
        1
    };
    let Some(config) = config else {
        return fail("no settings folder (HOME unset)".into(), console.err);
    };
    let windows = idecraft_mcp::client::running_windows(config);
    let window = match (pid, windows.as_slice()) {
        (Some(p), _) => match windows.iter().find(|w| w.pid == p) {
            Some(w) => w,
            None => {
                return fail(
                    format!("no running IDECraft window with pid {p}"),
                    console.err,
                );
            }
        },
        (None, [one]) => one,
        (None, []) => {
            return fail(
                "no running IDECraft window serves MCP over HTTP (Settings › AI Assistant › \"Serve MCP to local agents over HTTP\")".into(),
                console.err,
            );
        }
        (None, many) => {
            let list: Vec<String> = many
                .iter()
                .map(|w| format!("{} ({})", w.pid, w.projects.join(", ")))
                .collect();
            return fail(
                format!(
                    "several IDECraft windows run; pass --pid with one of: {}",
                    list.join("; ")
                ),
                console.err,
            );
        }
    };
    let mut client = match idecraft_mcp::client::Client::connect(window, "idecraft-cli") {
        Ok(c) => c,
        Err(e) => return fail(e, console.err),
    };
    loop {
        let reply = match client.call("quit_ide", serde_json::json!({ "pending": pending })) {
            Ok(r) => r,
            Err(e) => return fail(e, console.err),
        };
        let list = |key: &str| -> Vec<String> {
            reply[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let out = &mut *console.out;
        if reply["quit"] == serde_json::json!(true) {
            let _ = writeln!(out, "IDECraft (pid {}) quit", window.pid);
            // What the quit saved and ended (SPEC-0261 v3 behaviour 11).
            for (key, title) in [("saved", "Saved:"), ("stopped", "Stopped:")] {
                let items = list(key);
                if !items.is_empty() {
                    let _ = writeln!(out, "{title}");
                    for i in &items {
                        let _ = writeln!(out, "  {i}");
                    }
                }
            }
            return 0;
        }
        let (unsaved, running) = (list("unsaved"), list("running"));
        if !unsaved.is_empty() {
            let _ = writeln!(out, "Unsaved files:");
            for f in &unsaved {
                let _ = writeln!(out, "  {f}");
            }
        }
        if !running.is_empty() {
            let _ = writeln!(out, "Running:");
            for r in &running {
                let _ = writeln!(out, "  {r}");
            }
        }
        if pending != "ask" {
            return fail("IDECraft did not quit".into(), console.err);
        }
        if !console.terminal {
            let _ = writeln!(
                console.err,
                "IDECraft did not quit: pass --save-all to save and quit, or --discard to quit without saving"
            );
            return 2;
        }
        let _ = write!(out, "[s]ave all and quit, [d]iscard and quit, [c]ancel? ");
        let _ = out.flush();
        let mut answer = String::new();
        if console.input.read_line(&mut answer).is_err() {
            return fail("no answer".into(), console.err);
        }
        pending = match answer.trim().to_ascii_lowercase().as_str() {
            "s" | "save" | "save all" => "save_all",
            "d" | "discard" => "discard",
            _ => {
                let _ = writeln!(console.out, "Cancelled; IDECraft keeps running");
                return 1;
            }
        };
    }
}

/// `idecraft trust …`: reads and changes the remembered trusted locations (SPEC-0278 behaviour 10).
fn trust_cli(args: &[String]) -> ExitCode {
    use idecraft_core::trust::{self, TrustKind, TrustSettings};
    let Some(file) = trust::trust_file() else {
        eprintln!("error: no settings folder (set HOME or IDECRAFT_CONFIG_DIR)");
        return ExitCode::FAILURE;
    };
    let fail = |e: String| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    };
    match args {
        [cmd, dir] if cmd == "status" => {
            let settings = TrustSettings {
                enforced: true,
                ..TrustSettings::process()
            };
            let status = settings.status(Path::new(dir));
            match &status.source {
                Some(source) => {
                    println!("trusted: {} ({})", status.path.display(), source.describe())
                }
                None => println!("safe mode: {} is not trusted", status.path.display()),
            }
            if let Some(p) = &status.problem {
                println!("problem: {p}");
            }
            let found = trust::findings(&status.path);
            if found.is_empty() {
                println!("found: no build files or run configurations");
            }
            for f in found {
                println!("found: {} — {}", f.file, f.runs);
            }
            ExitCode::SUCCESS
        }
        [cmd, dir, rest @ ..] if cmd == "add" && rest.iter().all(|a| a == "--folder") => {
            let kind = if rest.is_empty() {
                TrustKind::Project
            } else {
                TrustKind::Folder
            };
            if !Path::new(dir).is_dir() {
                return fail(format!("not a folder: {dir}"));
            }
            match trust::add_location(&file, Path::new(dir), kind) {
                Ok(entry) => {
                    println!("trusted {:?}: {}", entry.kind, entry.path.display());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        [cmd] if cmd == "list" => match trust::load_locations(&file) {
            Ok(list) => {
                for l in list {
                    println!("{:?}\t{}", l.kind, l.path.display());
                }
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        },
        [cmd, path] if cmd == "remove" => match trust::remove_location(&file, Path::new(path)) {
            Ok(true) => {
                println!("removed: {path}");
                ExitCode::SUCCESS
            }
            Ok(false) => fail(format!("no trusted location {path}")),
            Err(e) => fail(e),
        },
        _ => {
            eprintln!(
                "usage: idecraft trust status <folder> | idecraft trust add <folder> [--folder] | idecraft trust list | idecraft trust remove <path>"
            );
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0094 v7 AK-17: `mcp`, `call` and `open` refuse a file and name its project; a folder
    /// still opens.
    #[test]
    fn a_file_is_refused_with_its_project() {
        let dir = std::env::temp_dir()
            .join(format!("idecraft-folder-arg-{}", std::process::id()))
            .join("repo");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("queries")).unwrap();
        let file = dir.join("queries/file.sql");
        std::fs::write(&file, "select 1;\n").unwrap();
        let refused = folder_argument(&file).expect_err("a file is refused");
        assert!(refused.contains("is a file"), "{refused}");
        assert!(
            refused.contains(&dir.canonicalize().unwrap().display().to_string()),
            "{refused}"
        );
        assert!(folder_argument(&dir.join("queries")).is_ok());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}

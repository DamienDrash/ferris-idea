//! Command-line entry point. Until the UI exists (M3), `ferris-idea` opens a
//! workspace headlessly (`open`) or serves it to LLM agents over MCP on
//! stdin/stdout (`mcp`, ADR-0008).

// The self-test chains its steps of every feature into one iterator, deeper than the default.
#![recursion_limit = "256"]

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod app_icon;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod claude_link;
mod drive;
#[cfg(all(test, target_os = "macos"))]
mod f083_driver_tests;
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
mod splash_chrome;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod termination;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod title_bar;
mod traffic_lights;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod ui_driver;
#[cfg(all(test, target_os = "macos"))]
mod ui_driver_window_tests;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod workbench;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use ferris_idea_core::{Layer, LayeredConfig, Workspace};
use ferris_idea_languages::LanguageId;

const DEFAULT_SETTINGS: &str = "[editor]\ntab_size = 4\ninsert_spaces = true\n";

/// Each command's forms, in the order the usage lists them (`SPEC-0321` v6 behaviour 1a).
const USAGES: &[(&str, &str)] = &[
    ("open", "ferris-idea open <directory>"),
    ("open", "ferris-idea open [--print] <file.idc…>"),
    (
        "mcp",
        "ferris-idea mcp <directory> [--trust] [--tools all|<group>,…]",
    ),
    (
        "serve",
        "ferris-idea serve [<directory>] --stdio [--state-dir <dir>] [--run-dir <dir>] [--reattach-minutes <n>]",
    ),
    (
        "serve",
        "ferris-idea serve [<directory>] --pair [--listen <addr>:<port>] [--allow-network] [--new-device] [--grant <rights>] [--key-file]",
    ),
    ("serve", "ferris-idea serve --devices"),
    ("serve", "ferris-idea serve --revoke <fingerprint>"),
    (
        "serve",
        "ferris-idea serve --attach <session> [--run-dir <dir>]",
    ),
    ("trust", "ferris-idea trust status|add|list|remove …"),
    (
        "device",
        "ferris-idea device pair|hosts|forget|add-path|call …",
    ),
    (
        "quit",
        "ferris-idea quit [--save-all | --discard | --force] [--pid <pid>]",
    ),
    (
        "call",
        "ferris-idea call <directory> <tool> [<json arguments>]",
    ),
    (
        "agent-bench",
        "ferris-idea agent-bench [<tasks folder>] [--out <file>]",
    ),
    (
        "drive",
        "ferris-idea drive [--project <folder>] [--json] <verb> …",
    ),
    ("launch", "ferris-idea launch [folder]"),
    (
        "container-exec",
        "ferris-idea container-exec <engine> <container>",
    ),
    ("spike-scroll", "ferris-idea spike-scroll [file]"),
    (
        "ui",
        "ferris-idea ui [folder [--trust] | --selftest [--list-groups]]",
    ),
    ("edit", "ferris-idea edit <file>"),
    ("spike-input", "ferris-idea spike-input"),
    ("spike-editor", "ferris-idea spike-editor [file]"),
    (
        "spike-layout",
        "ferris-idea spike-layout [--interactive] [directory]",
    ),
    ("--version", "ferris-idea --version"),
    ("--help", "ferris-idea [<command>] --help"),
];

/// The usage on one line, as a wrong command line gets it on standard error.
fn usage_line() -> String {
    let forms: Vec<&str> = USAGES.iter().map(|(_, form)| *form).collect();
    format!("usage: {}", forms.join(" | "))
}

/// The help of `command` (all commands for `None`), one form per line; `None` for an unknown
/// command.
fn help_text(command: Option<&str>) -> Option<String> {
    let forms: Vec<&str> = USAGES
        .iter()
        .filter(|(name, _)| command.is_none_or(|c| c == *name))
        .map(|(_, form)| *form)
        .collect();
    if forms.is_empty() {
        return None;
    }
    Some(format!("usage: {}\n", forms.join("\n       ")))
}

fn main() -> ExitCode {
    let mut args = launch::without_launch_arguments(std::env::args().skip(1).collect());
    // Started from the app bundle (Finder, Dock) there is no command line (SPEC-0282 behaviour 7).
    if args.is_empty() && launch::started_from_bundle() {
        args.push("launch".into());
    }
    adopt_login_shell_environment(&args);
    // The settings folder of the product's former name moves once, before anything reads or
    // writes settings (`SPEC-0356` behaviour 1).
    if let Some(line) = ferris_idea_core::takeover::take_over_user_config().message() {
        eprintln!("{line}");
    }
    // Ferris IDEA's own SSH hosts beside `~/.ssh/config` (SPEC-0121 v5 behaviour 1b): the product
    // uses the user's settings folder; tests set their own files or none.
    ferris_idea_remote::set_host_files(ferris_idea_remote::HostFiles::for_user());
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
        let folder = match ferris_idea_core::project_root::open_target(Path::new(folder)) {
            Ok(target) if args[0] == "ui" => target.root,
            _ => Path::new(folder).to_path_buf(),
        };
        ferris_idea_core::trust::trust_for_process(&folder);
    }
    match args.as_slice() {
        // `--help` and `-h` (`SPEC-0321` v6 behaviour 1a); `serve`, `device`, `drive`, `trust`
        // and `quit` answer it themselves.
        [flag] if ferris_idea_serve::is_help(flag) => {
            print!("{}", help_text(None).unwrap_or_default());
            ExitCode::SUCCESS
        }
        [cmd, flag]
            if ferris_idea_serve::is_help(flag)
                && !matches!(
                    cmd.as_str(),
                    "serve" | "device" | "drive" | "trust" | "quit"
                )
                && help_text(Some(cmd)).is_some() =>
        {
            print!("{}", help_text(Some(cmd)).unwrap_or_default());
            ExitCode::SUCCESS
        }
        [cmd, flag, file] if cmd == "open" && flag == "--print" => print_preview(Path::new(file)),
        [cmd, path]
            if cmd == "open" && ferris_idea_core::format::is_exchange_file(Path::new(path)) =>
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
        [cmd, path] if cmd == "mcp" => {
            match mcp(Path::new(path), ferris_idea_mcp::groups::Offer::default()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        // The tools offered at first (SPEC-0006 v58 behaviour 18).
        [cmd, path, flag, groups] if cmd == "mcp" && flag == "--tools" => {
            match ferris_idea_mcp::groups::Offer::parse(groups)
                .map_err(Into::into)
                .and_then(|offer| mcp(Path::new(path), offer))
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        [cmd, rest @ ..] if cmd == "trust" => trust_cli(rest),
        [cmd, rest @ ..] if cmd == "serve" => ferris_idea_serve::serve_cli(rest),
        [cmd, rest @ ..] if cmd == "device" => ferris_idea_serve::device::run(rest),
        [cmd, rest @ ..] if cmd == "quit" => quit_cli(rest),
        // The agent benchmark (SPEC-0345 behaviour 9).
        [cmd, rest @ ..] if cmd == "agent-bench" => agent_bench_cli(rest),
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
            println!("{}", ferris_idea_core::release::version_line());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{}", usage_line());
            ExitCode::from(2)
        }
    }
}

/// Gives a window that was not started from a shell the environment of the user's login shell,
/// so tools are found as in a terminal (`SPEC-0328`). Must stay the first thing `main` does after
/// reading its arguments.
fn adopt_login_shell_environment(args: &[String]) {
    use ferris_idea_core::shell_env::{self, Request, Source};
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
        if let Some(dir) = ferris_idea_core::logs::log_dir() {
            let line = shell_env::log_line(&report, entries, std::time::SystemTime::now());
            let _ =
                ferris_idea_core::logs::append_line(&dir.join(shell_env::ENVIRONMENT_LOG), &line);
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
    let Some(host) = ferris_idea_containers::DockerHost::parse(engine, ssh_extra.to_vec()) else {
        eprintln!("error: not a Docker engine address: {engine}");
        return ExitCode::from(2);
    };
    #[cfg(unix)]
    {
        let code = ferris_idea_containers::exec::run_in_terminal(&host, container);
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
        return Err(ferris_idea_core::project_root::file_given_for_folder(path));
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

/// `ferris-idea open --print <file>`: the import preview of an exchange file as text, against the
/// current folder and the user's settings (`SPEC-0303` behaviour 17). Exit 0 without problems, 1
/// with problems, 2 when the file is not an exchange file.
fn print_preview(file: &Path) -> ExitCode {
    if !ferris_idea_core::format::is_exchange_file(file) {
        eprintln!(
            "error: {} is not a Ferris IDEA exchange file (.idcrun, .idckeymap, .idctheme, .idcsources, .idcsettings, …)",
            file.display()
        );
        return ExitCode::from(2);
    }
    let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let preview =
        ferris_idea_formats::preview_file(file, &ferris_idea_formats::Targets::user(&root));
    for line in preview.lines() {
        println!("{line}");
    }
    if preview.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `ferris-idea open <file>`: an exchange file's preview in the running window, else in a window this
/// starts (`SPEC-0303` behaviour 17). Other files are refused as before (`open` takes folders).
fn open_file(file: &Path) -> ExitCode {
    let Ok(path) = file.canonicalize() else {
        eprintln!("error: {}: cannot be read", file.display());
        return ExitCode::FAILURE;
    };
    let windows = ferris_idea_core::user_config_dir()
        .map(|c| ferris_idea_mcp::client::running_windows(&c))
        .unwrap_or_default();
    if let Some(window) = windows.last() {
        let shown = ferris_idea_mcp::client::Client::connect(window, "ferris-idea-cli").and_then(
            |mut c| {
                c.call(
                    "import_file",
                    serde_json::json!({ "path": path.display().to_string(), "show": true }),
                )
            },
        );
        match shown {
            Ok(reply) => {
                println!(
                    "shown in Ferris IDEA (pid {}, {}); confirm the import there",
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

fn mcp(
    path: &Path,
    offer: ferris_idea_mcp::groups::Offer,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = folder_argument(path)?;
    // Untrusted projects are served in safe mode (SPEC-0278 behaviour 11).
    ferris_idea_core::trust::enforce();
    let runtime = tokio::runtime::Runtime::new()?;
    // Agents asking about code reuse running language servers (SPEC-0090).
    ferris_idea_languages::pool::enable();
    let served = runtime.block_on(ferris_idea_mcp::serve_stdio_offering(workspace, offer));
    ferris_idea_languages::pool::shutdown();
    served.map_err(|e| e.to_string().into())
}

/// `ferris-idea call <folder> <tool> [<json>]`: one MCP tool call from a shell, with the rights and
/// safe mode of `ferris-idea mcp` (SPEC-0281 behaviour 5). Exit 0 with the result on standard output,
/// 1 when the tool refuses or fails, 2 for wrong arguments or a folder that cannot be opened.
fn call_cli(folder: &str, tool: &str, arguments: Option<&str>) -> ExitCode {
    use ferris_idea_mcp::call::{Answer, call_once};
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
    ferris_idea_core::trust::enforce();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    ferris_idea_languages::pool::enable();
    let server = ferris_idea_mcp::IdeServer::new(workspace);
    let answer = runtime.block_on(call_once(server, tool, arguments));
    ferris_idea_languages::pool::shutdown();
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

/// `ferris-idea quit [--save-all | --discard | --force] [--pid <pid>]`: quits the running window through
/// its MCP server over HTTP (SPEC-0261 v2 behaviour 11). Exit status 0 when it quit, 1 when it did not
/// (no window, refused, cancelled), 2 for pending work without a terminal to ask on.
/// `ferris-idea agent-bench [<tasks folder>] [--out <file>]`: every recorded run of every task, the
/// suite report as JSON; exit 0 only when every run passes and every control is caught
/// (SPEC-0345 behaviour 9).
fn agent_bench_cli(args: &[String]) -> ExitCode {
    let mut folder = None;
    let mut out = None;
    let mut rest = args.iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--out" => match rest.next() {
                Some(f) => out = Some(f.clone()),
                None => {
                    eprintln!("error: --out needs a file");
                    return ExitCode::from(2);
                }
            },
            f if folder.is_none() && !f.starts_with("--") => folder = Some(f.to_owned()),
            other => {
                eprintln!(
                    "usage: ferris-idea agent-bench [<tasks folder>] [--out <file>] ({other}?)"
                );
                return ExitCode::from(2);
            }
        }
    }
    let folder = folder.unwrap_or_else(|| ferris_idea_mcp::agent_bench::TASKS_DIR.to_owned());
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    ferris_idea_languages::pool::enable();
    let report = runtime.block_on(ferris_idea_mcp::agent_bench::run_suite(Path::new(&folder)));
    ferris_idea_languages::pool::shutdown();
    let report = match report {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    match out {
        Some(file) => {
            if let Err(e) = std::fs::write(&file, text + "\n") {
                eprintln!("error: {file}: {e}");
                return ExitCode::FAILURE;
            }
        }
        None => println!("{text}"),
    }
    for r in report.runs.iter().filter(|r| r.control == r.success) {
        eprintln!(
            "{}/{}: {}",
            r.task,
            r.script,
            if r.control {
                "the control was not caught"
            } else {
                "failed"
            }
        );
    }
    if report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn quit_cli(args: &[String]) -> ExitCode {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    let terminal = stdin.is_terminal();
    let (mut input, mut out, mut err) = (stdin.lock(), std::io::stdout(), std::io::stderr());
    ExitCode::from(quit_with(
        args,
        ferris_idea_core::user_config_dir().as_deref(),
        &mut Console {
            terminal,
            input: &mut input,
            out: &mut out,
            err: &mut err,
        },
    ))
}

/// The terminal `ferris-idea quit` talks to: the process's own streams, or a scripted one in the
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

/// How long `ferris-idea quit` waits to reach the window (`SPEC-0261` v10 behaviour 11).
const QUIT_CONNECT: std::time::Duration = std::time::Duration::from_secs(2);

/// How long `ferris-idea quit` waits for each answer of the window; with [`QUIT_CONNECT`] the command
/// ends within 10 s (`SPEC-0261` v10 behaviour 11).
const QUIT_ANSWER: std::time::Duration = std::time::Duration::from_secs(8);

/// What `ferris-idea quit` says when the window at `pid` did not answer in time.
fn no_answer(pid: u32) -> String {
    format!(
        "Ferris IDEA (pid {pid}) did not answer within {} s, so nothing was done (the call was withdrawn). \
The window may be asking for the right to quit: answer it there and run the command again. If the \
window does not respond, end it with `kill {pid}`: it quits within 5 s without saving.",
        QUIT_ANSWER.as_secs()
    )
}

/// [`quit_cli`] with the settings folder `config` (where the windows' discovery files are) and
/// `console`; returns the exit status.
pub(crate) fn quit_with(args: &[String], config: Option<&Path>, console: &mut Console) -> u8 {
    const USAGE: &str = "usage: ferris-idea quit [--save-all | --discard | --force] [--pid <pid>]";
    let usage = |console: &mut Console| {
        let _ = writeln!(console.err, "{USAGE}");
        2
    };
    if args.iter().any(|a| ferris_idea_serve::is_help(a)) {
        let _ = writeln!(console.out, "{USAGE}");
        return 0;
    }
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
    let windows = ferris_idea_mcp::client::running_windows(config);
    let window = match (pid, windows.as_slice()) {
        (Some(p), _) => match windows.iter().find(|w| w.pid == p) {
            Some(w) => w,
            None => {
                return fail(
                    format!("no running Ferris IDEA window with pid {p}"),
                    console.err,
                );
            }
        },
        (None, [one]) => one,
        (None, []) => {
            return fail(
                "no running Ferris IDEA window serves MCP over HTTP (Settings › AI Assistant › \"Serve MCP to local agents over HTTP\")".into(),
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
                    "several Ferris IDEA windows run; pass --pid with one of: {}",
                    list.join("; ")
                ),
                console.err,
            );
        }
    };
    // An answer or an error within 10 s, never a silent wait (SPEC-0261 v10 behaviour 11).
    let mut client = match ferris_idea_mcp::client::Client::connect_within(
        window,
        "ferris-idea-cli",
        QUIT_CONNECT,
    ) {
        Ok(c) => c,
        Err(e) => return fail(e, console.err),
    };
    loop {
        let reply = match client.call_within(
            "quit_ide",
            serde_json::json!({ "pending": pending }),
            QUIT_ANSWER,
        ) {
            Ok(r) => r,
            Err(ferris_idea_mcp::client::CallError::TimedOut) => {
                return fail(no_answer(window.pid), console.err);
            }
            Err(ferris_idea_mcp::client::CallError::Failed(e)) => return fail(e, console.err),
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
            let _ = writeln!(out, "Ferris IDEA (pid {}) quit", window.pid);
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
            return fail("Ferris IDEA did not quit".into(), console.err);
        }
        if !console.terminal {
            let _ = writeln!(
                console.err,
                "Ferris IDEA did not quit: pass --save-all to save and quit, or --discard to quit without saving"
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
                let _ = writeln!(console.out, "Cancelled; Ferris IDEA keeps running");
                return 1;
            }
        };
    }
}

/// `ferris-idea trust …`: reads and changes the remembered trusted locations (SPEC-0278 behaviour 10).
fn trust_cli(args: &[String]) -> ExitCode {
    use ferris_idea_core::trust::{self, TrustKind, TrustSettings};
    const USAGE: &str = "usage: ferris-idea trust status <folder> | ferris-idea trust add <folder> [--folder] | ferris-idea trust list | ferris-idea trust remove <path>";
    if let [flag] = args
        && ferris_idea_serve::is_help(flag)
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let Some(file) = trust::trust_file() else {
        eprintln!("error: no settings folder (set HOME or FERRIS_IDEA_CONFIG_DIR)");
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
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in window that never answers a question: it keeps the reply, as a window whose
    /// question nobody answers or whose thread does not respond.
    #[derive(Default)]
    struct SilentWindow {
        questions: std::sync::Mutex<
            Vec<tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PermissionAnswer>>,
        >,
        quits: std::sync::atomic::AtomicUsize,
    }

    impl ferris_idea_mcp::window::Window for SilentWindow {
        fn ask(
            &self,
            _request: ferris_idea_mcp::window::PermissionRequest,
            reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PermissionAnswer>,
        ) {
            self.questions.lock().unwrap().push(reply);
        }
        fn pending_work(
            &self,
            reply: tokio::sync::oneshot::Sender<ferris_idea_mcp::window::PendingWork>,
        ) {
            let _ = reply.send(Default::default());
        }
        fn quit(
            &self,
            _mode: ferris_idea_mcp::window::QuitMode,
            reply: tokio::sync::oneshot::Sender<Result<(), String>>,
        ) {
            self.quits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = reply.send(Ok(()));
        }
    }

    /// SPEC-0261 AK-14 (v10, behaviour 11): `ferris-idea quit --discard --pid` against a window that
    /// never answers the question for `quit` exits with status 1 within 10 s, names the right, the
    /// withdrawal and `kill <pid>`; the question is withdrawn and the window never quits.
    #[test]
    fn quit_reports_a_window_that_does_not_answer() {
        let base =
            std::env::temp_dir().join(format!("ferris-idea-quit-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, config) = (base.join("project"), base.join("config"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        let window = std::sync::Arc::new(SilentWindow::default());
        let server = ferris_idea_mcp::http::HttpServer::start(Some(&config)).unwrap();
        let w: std::sync::Arc<dyn ferris_idea_mcp::window::Window> = window.clone();
        let ide = ferris_idea_mcp::IdeServer::with_permissions(
            ferris_idea_core::Workspace::open(&root).unwrap(),
            Default::default(),
        )
        .with_window(w);
        server.add_workspace(ide).unwrap();
        let pid = std::process::id().to_string();
        let args: Vec<String> = ["--discard", "--pid", pid.as_str()]
            .map(str::to_owned)
            .to_vec();
        let (mut input, mut out, mut err) = (std::io::empty(), Vec::new(), Vec::new());
        let start = std::time::Instant::now();
        let code = quit_with(
            &args,
            Some(&config),
            &mut Console {
                terminal: false,
                input: &mut input,
                out: &mut out,
                err: &mut err,
            },
        );
        let took = start.elapsed();
        let err = String::from_utf8_lossy(&err).into_owned();
        assert_eq!(code, 1, "{err}");
        assert!(took < std::time::Duration::from_secs(10), "took {took:?}");
        assert!(took >= QUIT_ANSWER, "gave up before its limit: {took:?}");
        assert!(err.contains("did not answer within 8 s"), "{err}");
        assert!(err.contains("right to quit"), "{err}");
        assert!(err.contains("withdrawn"), "{err}");
        assert!(err.contains(&format!("kill {pid}")), "{err}");
        // The question lost its waiting side, so the window withdraws it (SPEC-0053 v12).
        let asked = std::time::Instant::now();
        loop {
            let closed = {
                let questions = window.questions.lock().unwrap();
                !questions.is_empty() && questions.iter().all(|q| q.is_closed())
            };
            if closed {
                break;
            }
            assert!(
                asked.elapsed() < std::time::Duration::from_secs(10),
                "the question still waits"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let reply = window.questions.lock().unwrap().remove(0);
        assert!(
            reply
                .send(ferris_idea_mcp::window::PermissionAnswer::Always)
                .is_err()
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(window.quits.load(std::sync::atomic::Ordering::SeqCst), 0);
        server.stop();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// SPEC-0094 v7 AK-17: `mcp`, `call` and `open` refuse a file and name its project; a folder
    /// still opens.
    #[test]
    fn a_file_is_refused_with_its_project() {
        let dir = std::env::temp_dir()
            .join(format!("ferris-idea-folder-arg-{}", std::process::id()))
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

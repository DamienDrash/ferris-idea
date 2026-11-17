//! What debugging a run configuration's command means (`SPEC-0091` v2 behaviour 2, `SPEC-0097`
//! behaviour 6, `SPEC-0168`), on the machine this code runs on: the window's for a local project,
//! the host's for a remote one (`SPEC-0322` behaviour 6).

use std::path::{Path, PathBuf};

use idecraft_debugger::Kind;

/// The arguments of a package manager command that runs a `package.json` script: `run <script>
/// …` of npm, pnpm, yarn and bun, and npm's `start`/`test` shorthands (as `run start`).
fn package_script(manager: &str, rest: &[String]) -> Option<Vec<String>> {
    let manager = manager.trim_end_matches(".cmd");
    if !["npm", "pnpm", "yarn", "bun"].contains(&manager) {
        return None;
    }
    match rest.first().map(String::as_str) {
        Some("run" | "run-script") if rest.len() > 1 => {
            let mut args = vec!["run".to_owned()];
            args.extend(rest[1..].iter().cloned());
            Some(args)
        }
        Some(short @ ("start" | "test")) if manager == "npm" => {
            let mut args = vec!["run".to_owned(), short.to_owned()];
            args.extend(rest[1..].iter().cloned());
            Some(args)
        }
        _ => None,
    }
}

/// The program to debug for a run configuration's command: its first word, relative to `cwd` when
/// it is a path (`SPEC-0091` behaviour 2). `None` for an empty command.
pub fn program_of(command: &[String], cwd: &Path) -> Option<(PathBuf, Vec<String>)> {
    let (first, rest) = command.split_first()?;
    let path = PathBuf::from(first);
    let program = if path.is_absolute() || !first.contains('/') {
        if path.is_absolute() {
            path
        } else {
            which(first).unwrap_or_else(|| cwd.join(first))
        }
    } else {
        cwd.join(path)
    };
    Some((program, rest.to_vec()))
}

/// What debugging a run configuration's command means: a Python script when the command starts with
/// a `python…` interpreter or a `.py` file, else a native program (behaviour 2). Returns the kind, the
/// interpreter for a script, the program and its arguments.
pub fn plan(command: &[String], cwd: &Path) -> Option<DebugPlan> {
    let first = command.first()?;
    let name = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.starts_with("python") {
        let (interpreter, _) = program_of(&command[..1], cwd)?;
        let (script, args) = program_of(&command[1..], cwd).map(|(s, a)| {
            // A bare script name is a file in the working directory, not a program on the PATH.
            if command[1].contains('/') || Path::new(&command[1]).is_absolute() {
                (s, a)
            } else {
                (cwd.join(&command[1]), a)
            }
        })?;
        return Some((Kind::Python, Some(interpreter), script, args));
    }
    // `node script.js …` (SPEC-0091 v4 behaviour 10), also TypeScript (SPEC-0168 behaviour 3).
    if name == "node" || name == "node.exe" {
        let script = command.get(1).filter(|s| {
            [".js", ".mjs", ".cjs", ".ts", ".mts", ".cts"]
                .iter()
                .any(|e| s.ends_with(e))
        })?;
        let path = Path::new(script);
        let program = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        return Some((Kind::Node, None, program, command[2..].to_vec()));
    }
    // `go run …` and `go test …` run under Delve (SPEC-0039 v7 behaviour 11). The words after
    // `go` are kept: the launch reads them and says what Delve cannot debug (`go build`, several
    // packages) instead of debugging the go tool itself.
    if name == "go" || name == "go.exe" {
        let words = command[1..].to_vec();
        let target = idecraft_debugger::go_command(&words)
            .map(|c| c.target)
            .unwrap_or_default();
        return Some((Kind::Go, None, cwd.join(target), words));
    }
    // `npm run dev`, `pnpm run build -- --watch`, `npm start` (SPEC-0168 behaviour 1).
    if let Some(args) = package_script(&name, &command[1..]) {
        let runner = which(&name).unwrap_or_else(|| PathBuf::from(first));
        return Some((Kind::PackageScript, None, runner, args));
    }
    // `npx jest …` of a Jest test configuration or ▶ marker (SPEC-0045 v6 behaviour 12): npx
    // under js-debug like a package script, the tests in Jest's own process (`--runInBand`), where
    // the debugger is attached.
    if is_npx_jest(command) {
        let manager = name.trim_end_matches(".cmd").to_owned();
        let runner = which(&manager).unwrap_or_else(|| PathBuf::from(first));
        let mut args = command[1..].to_vec();
        if !args.iter().any(|a| a == "--runInBand" || a == "-i") {
            args.push("--runInBand".to_owned());
        }
        return Some((Kind::PackageScript, None, runner, args));
    }
    // `sh -c "<build> && kotlin -cp <classes> <MainKt>"` of a Kotlin module (SPEC-0132).
    if name == "sh"
        && command.get(1).map(String::as_str) == Some("-c")
        && let Some(script) = command.get(2)
        && let Some((build, run)) = script.rsplit_once(" && ")
        && let Some(class) = run
            .strip_prefix("kotlin -cp ")
            .and_then(|r| r.split_whitespace().nth(1))
    {
        return Some((
            Kind::Kotlin,
            None,
            PathBuf::from(class),
            vec![build.to_owned()],
        ));
    }
    if let Some(class) = java_main(command, cwd) {
        let args = if name == "java" {
            command.iter().skip(2).cloned().collect()
        } else {
            Vec::new()
        };
        return Some((Kind::Java, None, PathBuf::from(class), args));
    }
    if name.ends_with(".py") {
        let (script, args) = program_of(command, cwd)?;
        let script = if first.contains('/') {
            script
        } else {
            cwd.join(first)
        };
        return Some((Kind::Python, None, script, args));
    }
    let (program, args) = program_of(command, cwd)?;
    Some((Kind::Native, None, program, args))
}

/// Whether `command` runs Jest through npx (`npx jest …`), which can be debugged with js-debug
/// (`SPEC-0045` v6 behaviour 12).
pub fn is_npx_jest(command: &[String]) -> bool {
    let Some(first) = command.first() else {
        return false;
    };
    let name = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.trim_end_matches(".cmd") == "npx" && command.get(1).is_some_and(|w| w == "jest")
}

/// What [`plan`] returns: the kind, the interpreter for a script, the program and its arguments.
pub type DebugPlan = (Kind, Option<PathBuf>, PathBuf, Vec<String>);

/// [`plan`] for run configuration `config` of workspace `root`, from the command it starts:
/// variables replaced and a bare `python`/`python3` replaced by the workspace's selected
/// interpreter (`SPEC-0050` v2 behaviour 8). Err: why it cannot start; `Ok(None)`: it cannot be
/// debugged.
pub fn plan_run_config(
    root: &Path,
    config: &idecraft_core::RunConfig,
    cwd: &Path,
) -> Result<Option<DebugPlan>, String> {
    let command = idecraft_core::launch_command(root, config)?;
    Ok(plan(&command, cwd))
}

/// The main class a Java command runs: `java File.java` (package and file name) or a build tool
/// command with `-Dexec.mainClass=…` (`SPEC-0097` behaviour 6).
fn java_main(command: &[String], cwd: &Path) -> Option<String> {
    if let Some(class) = command
        .iter()
        .find_map(|a| a.strip_prefix("-Dexec.mainClass="))
    {
        return Some(class.to_owned());
    }
    let name = Path::new(command.first()?).file_name()?.to_string_lossy();
    let file = command.get(1).filter(|f| f.ends_with(".java"))?;
    if name != "java" {
        return None;
    }
    let path = cwd.join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    crate::java_debug::main_class(&path, &text)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    /// SPEC-0045 v6 AK-15 (behaviour 12): Jest through npx is debugged with js-debug, in one
    /// process; Vitest and other npx commands are not planned for js-debug.
    #[test]
    fn jest_through_npx_runs_under_js_debug() {
        let cwd = Path::new("/work/app");
        let command =
            words("npx jest --json --outputFile=.idecraft/jest.json sum.test.js -t ^sum$");
        assert!(is_npx_jest(&command));
        let (kind, interpreter, program, args) = plan(&command, cwd).unwrap();
        assert_eq!(kind, Kind::PackageScript);
        assert_eq!(kind.adapter_name(), "js-debug");
        assert_eq!(interpreter, None);
        assert!(program.ends_with("npx"), "{program:?}");
        assert_eq!(
            args,
            words("jest --json --outputFile=.idecraft/jest.json sum.test.js -t ^sum$ --runInBand")
        );
        // Already in one process: nothing added.
        let (_, _, _, args) = plan(&words("npx jest -i a.test.js"), cwd).unwrap();
        assert_eq!(args, words("jest -i a.test.js"));
        for other in ["npx vitest run --reporter=json", "npx eslint ."] {
            assert!(!is_npx_jest(&words(other)), "{other}");
            assert_ne!(
                plan(&words(other), cwd).unwrap().0,
                Kind::PackageScript,
                "{other}"
            );
        }
    }

    /// The commands of a Go file, of its tests and of one test function (the ▶ markers) are
    /// debugged with Delve, never as the native program `go` (`SPEC-0039` v7 behaviour 11).
    #[test]
    fn go_commands_are_planned_for_delve() {
        let cwd = Path::new("/work/shop");
        let (kind, interpreter, program, args) = plan(&words("go run main.go -v"), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        assert_eq!(kind.adapter_name(), "dlv");
        assert_eq!(interpreter, None);
        assert_eq!(program, cwd.join("main.go"));
        assert_eq!(args, words("run main.go -v"));
        let (kind, _, program, args) =
            plan(&words("go test -json -run ^TestTotal$ ./cart"), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        assert_eq!(program, cwd.join("./cart"));
        let launch = idecraft_debugger::go_launch(&args, cwd, &Default::default()).unwrap();
        assert_eq!(launch["mode"], "test");
        assert_eq!(
            launch["args"],
            serde_json::json!(["-test.v", "-test.run", "^TestTotal$"])
        );
        // What Delve cannot debug is still planned for it, so the start says why.
        let (kind, _, _, args) = plan(&words("go test -json ./..."), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        assert!(idecraft_debugger::go_command(&args).is_err());
        let (kind, ..) = plan(&words("go build ./..."), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        // Other programs are untouched.
        let (kind, ..) = plan(&words("./gopher run"), cwd).unwrap();
        assert_eq!(kind, Kind::Native);
    }
}

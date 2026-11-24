//! `ferris-idea-sandbox-launch <policy as JSON> (--probe | -- <program> <args>…)`: the Linux
//! launcher of `SPEC-0347` behaviour 8. It enters the sandbox and then executes the program, or
//! with `--probe` exits 0. Every failure ends it with exit code 125 and a line starting with
//! `ferris-idea-sandbox: refused:`; it never executes the program after a failed step.

use std::process::ExitCode;

use ferris_idea_sandbox::{Policy, REFUSED_EXIT, REFUSED_PREFIX};

fn refuse(reason: &str) -> ExitCode {
    eprintln!("{REFUSED_PREFIX} {reason}");
    ExitCode::from(REFUSED_EXIT as u8)
}

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let Some(json) = args.first().and_then(|a| a.to_str()) else {
        return refuse(
            "usage: ferris-idea-sandbox-launch <policy as JSON> (--probe | -- <program> <args>…)",
        );
    };
    let policy: Policy = match serde_json::from_str(json) {
        Ok(p) => p,
        Err(e) => return refuse(&format!("the policy is not valid JSON: {e}")),
    };
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let valid = match policy.validate(home.as_deref()) {
        Ok(v) => v,
        Err(problems) => {
            let text: Vec<String> = problems.iter().map(ToString::to_string).collect();
            return refuse(&format!("invalid policy: {}", text.join("; ")));
        }
    };
    let probe = match args.get(1).and_then(|a| a.to_str()) {
        Some("--probe") if args.len() == 2 => true,
        Some("--") if args.len() > 2 => false,
        _ => return refuse("expected `--probe` or `-- <program> <args>…` after the policy"),
    };
    if let Err(e) = enter(&valid) {
        return refuse(&e);
    }
    if probe {
        return ExitCode::SUCCESS;
    }
    exec(&args[2], &args[3..])
}

#[cfg(target_os = "linux")]
fn enter(policy: &ferris_idea_sandbox::ValidPolicy) -> Result<(), String> {
    ferris_idea_sandbox::linux::enter(policy)
}

#[cfg(not(target_os = "linux"))]
fn enter(_: &ferris_idea_sandbox::ValidPolicy) -> Result<(), String> {
    Err("this launcher sandboxes on Linux only".into())
}

#[cfg(unix)]
fn exec(program: &std::ffi::OsStr, args: &[std::ffi::OsString]) -> ExitCode {
    use std::os::unix::process::CommandExt;
    // Only returns on failure; the sandbox is already in force.
    let e = std::process::Command::new(program).args(args).exec();
    eprintln!(
        "ferris-idea-sandbox: cannot execute {}: {e}",
        program.to_string_lossy()
    );
    ExitCode::from(127)
}

#[cfg(not(unix))]
fn exec(_: &std::ffi::OsStr, _: &[std::ffi::OsString]) -> ExitCode {
    refuse("not supported on this system")
}

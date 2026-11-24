//! `ferris-idea-serve`: the headless backend for hosts (`SPEC-0321` behaviour 1).

use std::process::ExitCode;

const USAGE: &str = "usage: ferris-idea-serve serve [<folder>] --stdio | --attach <session> | --pair | --wss | --devices | --revoke <fingerprint> | --rights <fingerprint> <list> | --device-log | --access-off; ferris-idea-serve device …; ferris-idea-serve mcp <folder>; ferris-idea-serve --version; ferris-idea-serve <command> --help";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The settings folder of the product's former name moves once (`SPEC-0356` behaviour 1).
    if let Some(line) = ferris_idea_core::takeover::take_over_user_config().message() {
        eprintln!("{line}");
    }
    match args.as_slice() {
        [flag] if flag == "--version" => {
            println!("{}", ferris_idea_core::release::version_line());
            ExitCode::SUCCESS
        }
        // `SPEC-0321` v6 behaviour 1a.
        [flag] if ferris_idea_serve::is_help(flag) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [cmd, flag] if cmd == "mcp" && ferris_idea_serve::is_help(flag) => {
            println!("usage: ferris-idea-serve mcp <folder>");
            ExitCode::SUCCESS
        }
        [cmd, rest @ ..] if cmd == "serve" => ferris_idea_serve::serve_cli(rest),
        [cmd, rest @ ..] if cmd == "device" => ferris_idea_serve::device::run(rest),
        #[cfg(feature = "mcp")]
        [cmd, folder] if cmd == "mcp" => match ferris_idea_serve::mcp_stdio(folder.as_ref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

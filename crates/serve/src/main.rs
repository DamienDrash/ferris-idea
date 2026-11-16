//! `idecraft-serve`: the headless backend for hosts (`SPEC-0321` behaviour 1).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" => {
            println!("{}", idecraft_core::release::version_line());
            ExitCode::SUCCESS
        }
        [cmd, rest @ ..] if cmd == "serve" => idecraft_serve::serve_cli(rest),
        [cmd, rest @ ..] if cmd == "device" => idecraft_serve::device::run(rest),
        #[cfg(feature = "mcp")]
        [cmd, folder] if cmd == "mcp" => match idecraft_serve::mcp_stdio(folder.as_ref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!(
                "usage: idecraft-serve serve [<folder>] --stdio | --attach <session> | --pair | --devices | --revoke <fingerprint> | --rights <fingerprint> <list> | --device-log | --access-off; idecraft-serve device …; idecraft-serve mcp <folder>; idecraft-serve --version"
            );
            ExitCode::from(2)
        }
    }
}

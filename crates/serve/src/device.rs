//! `idecraft device` (`SPEC-0324` behaviour 15): the command line of a paired device. Pair with a
//! machine running `idecraft serve --pair`, list and forget hosts, add a path, and call one method.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use idecraft_pairing::parse_link;
use idecraft_remote::paired;
use idecraft_remote::{HostSpec, PairedProject};
use serde_json::{Value, json};

const USAGE: &str = "usage: idecraft device pair <address> --code <code> | pair <link> | hosts | \
                     forget <address> | add-path <address> <new path> | \
                     call <address> <folder> <method> [<json params>]";

/// The address of a paired host string, as the key store keeps it (`alias:port@ssh`, `host:port`).
fn address(text: &str) -> Result<String, String> {
    let spec = HostSpec::parse(&format!("paired://{text}")).map_err(|e| e.to_string())?;
    let label = spec.label();
    Ok(label.trim_start_matches("paired://").to_owned())
}

/// Runs `idecraft device …`.
pub fn run(args: &[String]) -> ExitCode {
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["pair", link] if parse_link(link).is_some() => pair_link(link),
        ["pair", address_text, "--code", code] => pair_code(address_text, code),
        ["hosts"] => hosts(),
        ["forget", address_text] => forget(address_text),
        ["add-path", address_text, path] => add_path(address_text, path),
        ["call", address_text, folder, method] => call(address_text, folder, method, "{}"),
        ["call", address_text, folder, method, params] => {
            call(address_text, folder, method, params)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Asks the user whether the digits match. Only a terminal can answer (`SPEC-0324` behaviour 4).
fn confirm(digits: &str, host: &str) -> Result<bool, String> {
    eprintln!("Host key {host}. Digits: {digits}");
    if !std::io::stdin().is_terminal() {
        return Err(
            "standard input is not a terminal: only a person at a terminal can confirm the \
             digits"
                .to_owned(),
        );
    }
    eprint!("Do these digits match the ones on the host? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn finish(pending: idecraft_pairing::PendingPairing) -> Result<(), String> {
    match confirm(pending.digits(), &pending.host_fingerprint()) {
        Ok(true) => {
            let fingerprint = paired::finish_pairing(pending).map_err(|e| e.to_string())?;
            println!("paired, host key {fingerprint}");
            Ok(())
        }
        Ok(false) => {
            pending.decline();
            Err("declined; nothing was stored".to_owned())
        }
        Err(e) => {
            pending.decline();
            Err(e)
        }
    }
}

fn pair_code(address_text: &str, code: &str) -> Result<(), String> {
    let address = address(address_text)?;
    let pending = paired::begin_pairing(&address, code).map_err(|e| e.to_string())?;
    finish(pending)
}

fn pair_link(link: &str) -> Result<(), String> {
    let link = parse_link(link).ok_or("not a pairing link")?;
    let pending = paired::begin_pairing_link(&link).map_err(|e| e.to_string())?;
    finish(pending)
}

fn hosts() -> Result<(), String> {
    let list = paired::paired_hosts().map_err(|e| e.to_string())?;
    println!("{}", json!({ "hosts": list }));
    Ok(())
}

fn forget(address_text: &str) -> Result<(), String> {
    let address = address(address_text)?;
    let removed = paired::forget_paired(&address).map_err(|e| e.to_string())?;
    println!("{}", json!({ "removed": removed }));
    Ok(())
}

fn add_path(address_text: &str, path: &str) -> Result<(), String> {
    let address = address(address_text)?;
    let path = self::address(path)?;
    if !paired::add_path(&address, &path).map_err(|e| e.to_string())? {
        return Err(format!("{address} is not paired"));
    }
    println!("{}", json!({ "added": path }));
    Ok(())
}

fn call(address_text: &str, folder: &str, method: &str, params: &str) -> Result<(), String> {
    let params: Value = serde_json::from_str(params).map_err(|e| format!("params: {e}"))?;
    let address = address(address_text)?;
    let mut project = PairedProject::open(&address, folder, None).map_err(|e| e.to_string())?;
    let result = project
        .session
        .call(method, params)
        .map_err(|e| format!("{}: {}", e.code.as_str(), e.message));
    let _ = project.close(idecraft_remote::Unsaved::Discard);
    println!("{}", result?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_strings_become_store_keys() {
        assert_eq!(address("nas:9000").unwrap(), "nas:9000");
        assert_eq!(address("nas:9000@ssh").unwrap(), "nas:9000@ssh");
        assert_eq!(address("nas@ssh").unwrap(), "nas:47100@ssh");
        assert!(address("-oBad=1").is_err());
    }
}

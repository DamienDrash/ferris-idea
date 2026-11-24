//! `ferris-idea serve --pair`, `--devices` and `--revoke` (`SPEC-0321` behaviours 13, 17–20).

use std::io::{BufRead, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ferris_idea_pairing::{
    Console, EXIT_REFUSED, Limits, ListenError, ListenOptions, Side, Store, Timing, listen,
};
use ferris_idea_session::{Backend, Right};

/// The address `--pair` binds without `--listen`.
pub fn default_address() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], ferris_idea_pairing::DEFAULT_PORT))
}

/// Rights named on the command line; `view` is always among them.
pub fn parse_rights(text: &str) -> Result<Vec<Right>, String> {
    let mut out = vec![Right::View];
    for name in text.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        let right = Right::ALL
            .iter()
            .copied()
            .find(|r| r.name() == name)
            .ok_or_else(|| {
                format!(
                    "unknown right {name}; the rights are {}",
                    Right::ALL.map(Right::name).join(", ")
                )
            })?;
        if !out.contains(&right) {
            out.push(right);
        }
    }
    Ok(out)
}

/// The console of `serve --pair`: standard error for text, standard input for answers.
struct Terminal {
    grant: Option<Vec<Right>>,
}

fn ask(question: &str) -> String {
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_owned()
}

impl Console for Terminal {
    fn show(&self, line: &str) {
        eprintln!("{line}");
    }

    fn confirm(&self, device: &str, fingerprint: &str, digits: &str) -> bool {
        eprintln!("Device {device} ({fingerprint}) wants to pair. Digits: {digits}");
        if !std::io::stdin().is_terminal() {
            eprintln!("standard input is not a terminal; the pairing is declined (SPEC-0324)");
            return false;
        }
        matches!(
            ask("Do these digits match the ones in the window? [y/N] ")
                .to_lowercase()
                .as_str(),
            "y" | "yes"
        )
    }

    fn pairing_link(&self, link: &str) {
        eprintln!("{}", crate::pair::qr_text(link));
        eprintln!("Pairing link: {link}");
    }

    fn rights(&self, device: &str, offered: &[Right]) -> Vec<Right> {
        if let Some(g) = &self.grant {
            return g.clone();
        }
        let names: Vec<&str> = offered.iter().map(|r| r.name()).collect();
        let answer = ask(&format!(
            "Rights for {device} (view is always granted; `agents` includes what the project's agent tools may do; more, comma separated, from {}): ",
            names.join(", ")
        ));
        parse_rights(&answer).unwrap_or_else(|e| {
            eprintln!("{e}; granting view only");
            vec![Right::View]
        })
    }
}

fn refused(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(EXIT_REFUSED)
}

/// The store folder below the state folder.
pub fn store_dir(state: &Path) -> PathBuf {
    state.join("remote")
}

/// `--devices`: the paired devices, one per line.
pub fn list_devices(state: &Path, allow_file: bool) -> ExitCode {
    let store = match Store::open(Side::Serve, &store_dir(state), allow_file) {
        Ok(s) => s,
        Err(e) => return refused(&e.0),
    };
    match store.devices() {
        Ok(list) => {
            for d in list {
                println!("{}  {}  {}", d.fingerprint, d.name, d.rights.join(","));
            }
            ExitCode::SUCCESS
        }
        Err(e) => refused(&e.0),
    }
}

/// `--revoke <fingerprint>`.
pub fn revoke(state: &Path, fingerprint: &str, allow_file: bool) -> ExitCode {
    let store = match Store::open(Side::Serve, &store_dir(state), allow_file) {
        Ok(s) => s,
        Err(e) => return refused(&e.0),
    };
    match store.revoke(fingerprint) {
        Ok(true) => {
            eprintln!(
                "revoked {fingerprint}; a running listener closes its sessions within a second"
            );
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("{fingerprint} was not paired; it is on the revocation list now");
            ExitCode::SUCCESS
        }
        Err(e) => refused(&e.0),
    }
}

/// What `--pair` was given.
pub struct PairOptions {
    /// Address to bind.
    pub addr: SocketAddr,
    /// `--allow-network`.
    pub allow_network: bool,
    /// `--new-device`.
    pub new_device: bool,
    /// `--grant`.
    pub grant: Option<Vec<Right>>,
    /// `--key-file`.
    pub key_file: bool,
    /// `--qr`.
    pub qr: bool,
    /// A `<folder>` was given.
    pub has_folder: bool,
}

/// `link` as a QR code in block characters (`ADR-0025` §2); the link is printed beside it.
pub fn qr_text(link: &str) -> String {
    match qrcode::QrCode::new(link.as_bytes()) {
        Ok(code) => code
            .render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build(),
        Err(e) => format!("(no QR code: {e})"),
    }
}

/// `--rights <fingerprint> <list>`.
pub fn set_rights(state: &Path, fingerprint: &str, rights: &[Right], allow_file: bool) -> ExitCode {
    let store = match Store::open(Side::Serve, &store_dir(state), allow_file) {
        Ok(s) => s,
        Err(e) => return refused(&e.0),
    };
    let names: Vec<String> = rights.iter().map(|r| r.name().to_owned()).collect();
    match store.set_rights(fingerprint, &names) {
        Ok(true) => {
            eprintln!(
                "{fingerprint} now has {}; a running listener applies this within a second",
                names.join(",")
            );
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("error: {fingerprint} is not a paired device");
            ExitCode::from(2)
        }
        Err(e) => refused(&e.0),
    }
}

/// `--device-log`: the newest lines of the log, one JSON object per line.
pub fn show_log(state: &Path, fingerprint: Option<&str>) -> ExitCode {
    for line in ferris_idea_pairing::audit::read_log(state, fingerprint, 200) {
        println!("{line}");
    }
    ExitCode::SUCCESS
}

/// `--access-off`: a running listener closes everything within a second.
pub fn switch_off(state: &Path, allow_file: bool) -> ExitCode {
    let store = match Store::open(Side::Serve, &store_dir(state), allow_file) {
        Ok(s) => s,
        Err(e) => return refused(&e.0),
    };
    match store.set_access_off(true) {
        Ok(()) => {
            eprintln!("remote access is off; a running listener closes within a second");
            ExitCode::SUCCESS
        }
        Err(e) => refused(&e.0),
    }
}

/// Runs the listener until the process ends.
pub fn run(backend: Backend, state: &Path, o: PairOptions) -> ExitCode {
    let store = match Store::open(Side::Serve, &store_dir(state), o.key_file) {
        Ok(s) => s,
        Err(e) => return refused(&e.0),
    };
    // A key that cannot be read refuses the start before anything listens.
    if let Err(e) = store.identity() {
        return refused(&e.0);
    }
    // A device is bound to one project (`SPEC-0324` behaviour 2).
    if !o.has_folder {
        eprintln!(
            "error: --pair needs a folder: a paired device is bound to one project \
             (ferris-idea serve --pair <folder>)"
        );
        return ExitCode::from(2);
    }
    let options = ListenOptions {
        addr: o.addr,
        allow_network: o.allow_network,
        new_device: o.new_device,
        qr: o.qr,
        timing: Timing::default(),
        limits: Limits::default(),
    };
    let console = Arc::new(Terminal { grant: o.grant });
    match listen(options, store, backend, console) {
        Ok(running) => {
            while !running.stopped() {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            ExitCode::SUCCESS
        }
        Err(ListenError::Keys(e)) => refused(&e.0),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_always_hold_view_and_refuse_unknown_names() {
        assert_eq!(parse_rights("").unwrap(), [Right::View]);
        assert_eq!(
            parse_rights("edit, run").unwrap(),
            [Right::View, Right::Edit, Right::Run]
        );
        assert!(parse_rights("root").unwrap_err().contains("unknown right"));
    }

    #[test]
    fn the_default_address_is_loopback() {
        assert!(default_address().ip().is_loopback());
    }
}

//! The shared options of every Ferris IDEA call of the system `ssh`, with the required hybrid
//! post-quantum key exchange (ADR-0025 v3 section 1a, PO-STATUS question 29).
//!
//! Every SSH path — remote host commands and files, the backend and `ferris-idea serve`, the SSH
//! terminal, Docker over SSH, database tunnels and Git over SSH — builds its `ssh` command line
//! from [`ssh_options`] (Git through `crate::git_ssh_command`). The options come first on the command
//! line, so nothing in `~/.ssh/config` or in extra arguments can weaken them (OpenSSH keeps the
//! first value it obtains). The key exchange is limited to hybrid post-quantum methods; a host
//! that offers none of them is refused with [`RemoteError::NoPqKex`], and a local client that
//! knows none of them with [`RemoteError::ClientWithoutPq`] — there is no fallback to a classical
//! exchange. The one exception, Git to Azure DevOps, lives in `pq_git` and never reaches the other
//! paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::ssh::RemoteError;

/// Hybrid post-quantum key exchanges Ferris IDEA allows, in order of preference.
///
/// - `mlkem768x25519-sha256`: ML-KEM-768 (FIPS 203) with X25519, OpenSSH 9.9 and later, the
///   default from 10.0.
/// - `sntrup761x25519-sha512` and its older name `sntrup761x25519-sha512@openssh.com`: Streamlined
///   NTRU Prime with X25519, the default of OpenSSH 9.0–9.8, which have no ML-KEM.
///
/// Both are hybrid: the session key needs the classical X25519 secret *and* the post-quantum one,
/// so breaking either alone reveals nothing.
pub const PQ_KEX: [&str; 3] = [
    "mlkem768x25519-sha256",
    "sntrup761x25519-sha512",
    "sntrup761x25519-sha512@openssh.com",
];

/// The stricter set of [`KexPolicy::MlKemOnly`]: the standardised KEM only.
pub const PQ_KEX_ML_KEM: [&str; 1] = ["mlkem768x25519-sha256"];

/// Environment variable that selects the stricter policy (`mlkem`); it can only narrow the set.
pub const KEX_POLICY_ENV: &str = "FERRIS_IDEA_SSH_KEX";

/// Which post-quantum key exchanges are allowed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KexPolicy {
    /// [`PQ_KEX`]: ML-KEM preferred, sntrup761 for OpenSSH 9.0–9.8 (default).
    #[default]
    Hybrid,
    /// [`PQ_KEX_ML_KEM`]: ML-KEM only; hosts and clients before OpenSSH 9.9 are refused.
    MlKemOnly,
}

impl KexPolicy {
    /// The policy of this process: [`KexPolicy::MlKemOnly`] when `FERRIS_IDEA_SSH_KEX=mlkem`, else
    /// the default. Any other value keeps the default; nothing weakens it.
    pub fn current() -> Self {
        Self::parse(
            ferris_idea_core::product_var(KEX_POLICY_ENV)
                .ok()
                .as_deref(),
        )
    }

    /// The policy named by `value` (`mlkem`, `ml-kem` or `mlkem-only`, any case).
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("mlkem" | "ml-kem" | "mlkem-only") => Self::MlKemOnly,
            _ => Self::Hybrid,
        }
    }

    /// The allowed methods, in order of preference.
    pub fn algorithms(self) -> &'static [&'static str] {
        match self {
            Self::Hybrid => &PQ_KEX,
            Self::MlKemOnly => &PQ_KEX_ML_KEM,
        }
    }

    /// The oldest OpenSSH that offers one of them.
    pub fn needs(self) -> &'static str {
        match self {
            Self::Hybrid => "OpenSSH 9.0 or later; ML-KEM from 9.9",
            Self::MlKemOnly => "OpenSSH 9.9 or later (ML-KEM)",
        }
    }
}

/// What `ssh -Q kex` printed per program, asked once per process.
pub(crate) fn known_kex(program: &str) -> Result<Vec<String>, RemoteError> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(list) = cache.lock().ok().and_then(|c| c.get(program).cloned()) {
        return Ok(list);
    }
    let out = crate::launch::output(program, &["-Q".to_owned(), "kex".to_owned()])
        .map_err(|e| RemoteError::NoSsh(format!("{program}: {e}")))?;
    // An `ssh` too old for `-Q kex` (before 6.3) prints nothing useful: no post-quantum method.
    let list: Vec<String> = if out.status.success() {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        Vec::new()
    };
    if let Ok(mut c) = cache.lock() {
        c.insert(program.to_owned(), list.clone());
    }
    Ok(list)
}

/// The allowed methods of `policy` that a client knowing `known` supports, in preference order.
pub fn usable_kex(policy: KexPolicy, known: &[String]) -> Vec<&'static str> {
    policy
        .algorithms()
        .iter()
        .copied()
        .filter(|a| known.iter().any(|k| k == a))
        .collect()
}

/// Checks once per process that the local `program` (`ssh`) offers a post-quantum key exchange of
/// the current policy; returns the methods to pass.
pub(crate) fn check_client(program: &str) -> Result<Vec<&'static str>, RemoteError> {
    let policy = KexPolicy::current();
    let usable = usable_kex(policy, &known_kex(program)?);
    if usable.is_empty() {
        return Err(RemoteError::ClientWithoutPq(policy.needs().to_owned()));
    }
    Ok(usable)
}

/// The `KexAlgorithms=` value for `program`: the allowed methods it knows, or — when it knows none
/// or cannot be asked — all allowed ones, so that `ssh` itself refuses; never a classical method.
pub(crate) fn kex_option(program: &str) -> String {
    let list = check_client(program)
        .unwrap_or_else(|_| KexPolicy::current().algorithms().to_vec())
        .join(",");
    format!("KexAlgorithms={list}")
}

/// The shared `ssh` options of every SSH path, for the client `program`: batch mode (never a
/// password or passphrase prompt), strict host keys, a 10 s connect timeout and the post-quantum
/// key exchange. Callers put them before any other argument.
pub(crate) fn ssh_options(program: &str) -> Vec<String> {
    options_with_kex(kex_option(program))
}

/// The shared options with the `KexAlgorithms=` option `kex`.
pub(crate) fn options_with_kex(kex: String) -> Vec<String> {
    let mut args = Vec::with_capacity(8);
    for o in [
        "BatchMode=yes".to_owned(),
        "StrictHostKeyChecking=yes".to_owned(),
        "ConnectTimeout=10".to_owned(),
        kex,
    ] {
        args.push("-o".to_owned());
        args.push(o);
    }
    args
}

/// `GIT_SSH_COMMAND` without any exception, for when no stand-in can be written: the system
/// `ssh` with [`ssh_options`] and the direct route pinned (`ProxyCommand=none`; a jump host would
/// otherwise be opened by OpenSSH without the shared options).
pub(crate) fn strict_git_ssh_command() -> String {
    std::iter::once("ssh".to_owned())
        .chain(ssh_options("ssh"))
        .chain(["-o".to_owned(), "ProxyCommand=none".to_owned()])
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first `name` on `PATH` that is a file, leaving out the folder `skip`.
fn on_path(name: &str, skip: &Path) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|d| d != skip)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The `ssh` stand-in for programs that start `ssh` from `PATH` themselves (the Docker Compose CLI
/// with an `ssh://` engine): `<folder>/ssh` runs the system `ssh` with [`ssh_options`] and the
/// route over jump hosts (`crate::jump`) before its own arguments, and `-F <Ferris IDEA's file>` for
/// Ferris IDEA's own aliases (`SPEC-0121` v6 behaviour 1b). Returns the folder to put first
/// on `PATH`. The folder and the script are private to the user (0700); the script is written anew
/// each time.
pub(crate) fn ssh_shim_dir() -> Result<PathBuf, RemoteError> {
    let base = crate::ssh::control_dir();
    let dir = base.join("bin");
    if !crate::ssh::prepare_control_dir(&base) || !crate::ssh::prepare_control_dir(&dir) {
        return Err(RemoteError::NoSsh(format!(
            "{}: cannot make a private folder",
            dir.display()
        )));
    }
    let real = on_path("ssh", &dir)
        .ok_or_else(|| RemoteError::NoSsh("no ssh on PATH".into()))?
        .display()
        .to_string();
    check_client(&real)?;
    let options = crate::jump::without_shared_connection(ssh_options(&real))
        .iter()
        .map(|o| crate::ssh::quote(o))
        .collect::<Vec<_>>()
        .join(" ");
    let hop = crate::jump::hop_path(&real)?;
    let lib = crate::jump::route_lib(
        &crate::ssh::quote(&hop.display().to_string()),
        crate::jump::own_proxy_command_setting(),
    );
    let real = crate::ssh::quote(&real);
    let own = crate::own_hosts::stand_in_step(
        crate::own_hosts::stand_in_file().as_deref(),
        &real,
        &options,
    );
    let script = format!(
        "#!/bin/sh\n# Written by Ferris IDEA: the system ssh with the post-quantum key exchange on every hop (ADR-0025).\n\
         {lib}\
         idc_config \"$@\"\n\
         {own}\
         conf=$({real} {options} -G \"$@\" </dev/null 2>/dev/null)\n\
         host=$(printf '%s\\n' \"$conf\" | sed -n 's/^hostname //p' | head -n 1)\n\
         idc_route \"$conf\" \"$idc_cfg\" \"$host\" || exit 255\n\
         if [ -n \"$idc_proxy\" ]; then set -- -o \"$idc_proxy\" \"$@\"; fi\n\
         exec {real} {options} \"$@\"\n"
    );
    let path = dir.join("ssh");
    let part = dir.join(format!("ssh.{}.part", std::process::id()));
    let fail = |e: std::io::Error| RemoteError::NoSsh(format!("{}: {e}", path.display()));
    std::fs::write(&part, script).map_err(fail)?;
    crate::ssh::set_mode(&part, 0o700).map_err(fail)?;
    std::fs::rename(&part, &path).map_err(fail)?;
    Ok(dir)
}

/// Whether `ssh`'s messages say the key exchange failed because one side has no allowed method:
/// the host offered none (`no matching key exchange method found`) or the local client does not
/// know the names (`Bad SSH2 KexAlgorithms`, `Unsupported KEX algorithm`).
pub fn kex_refusal(host: &str, stderr: &str) -> Option<RemoteError> {
    let lower = stderr.to_lowercase();
    if lower.contains("no matching key exchange method") {
        Some(RemoteError::NoPqKex(host.to_owned()))
    } else if lower.contains("bad ssh2 kexalgorithms")
        || lower.contains("unsupported kex algorithm")
        || lower.contains("bad key exchange")
    {
        Some(RemoteError::ClientWithoutPq(
            KexPolicy::current().needs().to_owned(),
        ))
    } else {
        None
    }
}

/// The peer `ssh` names in "Unable to negotiate with <peer> port <n>", for messages where the
/// host is not otherwise known (Git).
pub fn negotiating_peer(stderr: &str) -> Option<String> {
    let line = stderr
        .lines()
        .find(|l| l.contains("Unable to negotiate with "))?;
    let rest = line.split("Unable to negotiate with ").nth(1)?;
    let peer = rest.split(" port ").next()?.trim();
    (!peer.is_empty()).then(|| peer.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hybrid_post_quantum_methods_are_allowed() {
        assert_eq!(
            KexPolicy::default().algorithms()[0],
            "mlkem768x25519-sha256"
        );
        for a in PQ_KEX {
            assert!(a.contains("x25519"), "{a} must be hybrid");
            assert!(a.starts_with("mlkem") || a.starts_with("sntrup"), "{a}");
        }
        assert_eq!(KexPolicy::parse(Some("MLKEM")), KexPolicy::MlKemOnly);
        assert_eq!(KexPolicy::parse(Some("mlkem-only")), KexPolicy::MlKemOnly);
        // Nothing weakens the default.
        for v in [
            None,
            Some(""),
            Some("classic"),
            Some("curve25519-sha256"),
            Some("off"),
        ] {
            assert_eq!(KexPolicy::parse(v), KexPolicy::Hybrid);
        }
        assert_eq!(KexPolicy::MlKemOnly.algorithms(), ["mlkem768x25519-sha256"]);
    }

    #[test]
    fn the_client_list_narrows_and_never_widens() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        // OpenSSH 10: all three, ML-KEM first.
        let new = s(&[
            "curve25519-sha256",
            "sntrup761x25519-sha512",
            "sntrup761x25519-sha512@openssh.com",
            "mlkem768x25519-sha256",
        ]);
        assert_eq!(usable_kex(KexPolicy::Hybrid, &new), PQ_KEX);
        // OpenSSH 9.0–9.8: sntrup761 under its old name only.
        let old = s(&["curve25519-sha256", "sntrup761x25519-sha512@openssh.com"]);
        assert_eq!(
            usable_kex(KexPolicy::Hybrid, &old),
            ["sntrup761x25519-sha512@openssh.com"]
        );
        assert!(usable_kex(KexPolicy::MlKemOnly, &old).is_empty());
        // OpenSSH 8: nothing usable.
        let classic = s(&["curve25519-sha256", "ecdh-sha2-nistp256"]);
        assert!(usable_kex(KexPolicy::Hybrid, &classic).is_empty());
    }

    #[test]
    fn options_are_batch_strict_and_post_quantum() {
        let o = ssh_options("ssh");
        assert!(o.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(
            o.windows(2)
                .any(|w| w == ["-o", "StrictHostKeyChecking=yes"])
        );
        let kex = o
            .iter()
            .find_map(|a| a.strip_prefix("KexAlgorithms="))
            .expect("KexAlgorithms is always set");
        assert!(!kex.is_empty());
        for a in kex.split(',') {
            assert!(PQ_KEX.contains(&a), "{a} is not an allowed method");
        }
        // A missing client still gets only post-quantum names (so ssh refuses, no fallback).
        assert_eq!(
            kex_option("/no/such/ssh"),
            format!(
                "KexAlgorithms={}",
                KexPolicy::current().algorithms().join(",")
            )
        );
        assert!(matches!(
            check_client("/no/such/ssh"),
            Err(RemoteError::NoSsh(_))
        ));
        let git = strict_git_ssh_command();
        assert!(git.starts_with("ssh -o BatchMode=yes "), "{git}");
        assert!(git.contains("-o KexAlgorithms="), "{git}");
    }

    #[test]
    fn the_stand_in_puts_the_shared_options_first() {
        use std::os::unix::fs::PermissionsExt;
        let Ok(dir) = ssh_shim_dir() else {
            eprintln!("skipped: no ssh on PATH");
            return;
        };
        let script = std::fs::read_to_string(dir.join("ssh")).unwrap();
        let exec = script.lines().find(|l| l.starts_with("exec ")).unwrap();
        assert!(exec.ends_with("\"$@\""), "{exec}");
        let kex = exec.find("KexAlgorithms=").unwrap();
        assert!(kex < exec.find("\"$@\"").unwrap());
        assert!(
            !exec.contains(&dir.display().to_string()),
            "must not call itself"
        );
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("ssh")), 0o700);
    }

    #[test]
    fn refusals_of_the_key_exchange_are_named() {
        let host = "Unable to negotiate with 127.0.0.1 port 2222: no matching key exchange method \
                    found. Their offer: curve25519-sha256,ecdh-sha2-nistp256\n";
        assert_eq!(
            kex_refusal("box", host),
            Some(RemoteError::NoPqKex("box".into()))
        );
        assert_eq!(negotiating_peer(host).as_deref(), Some("127.0.0.1"));
        assert!(matches!(
            kex_refusal(
                "box",
                "command-line line 0: Bad SSH2 KexAlgorithms 'mlkem768x25519-sha256'."
            ),
            Some(RemoteError::ClientWithoutPq(_))
        ));
        assert_eq!(kex_refusal("box", "Permission denied (publickey)."), None);
        let text = RemoteError::NoPqKex("box".into()).to_string();
        assert!(text.contains("box"), "{text}");
        assert!(text.contains("post-quantum"), "{text}");
        assert!(text.contains("ADR-0025"), "{text}");
        assert!(!text.contains("Their offer"), "{text}");
    }
}

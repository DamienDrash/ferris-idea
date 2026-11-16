//! Git over SSH and its one exception to the post-quantum rule: Azure DevOps (ADR-0025 v4 section
//! 1a, PO-STATUS question 30).
//!
//! Git gets the shared options of [`crate::ssh_options`] through `GIT_SSH_COMMAND`. Azure DevOps
//! offers only classical Diffie-Hellman over SSH, so the Project Owner allowed an exception, as
//! narrow as possible:
//!
//! - **Git only.** Only `GIT_SSH_COMMAND` ([`git_ssh_command`]) can carry it; every other SSH path
//!   builds its command line from [`crate::ssh_options`], which never contains a classical method.
//! - **Azure DevOps only, by the resolved host name.** A private stand-in script asks `ssh -G` with
//!   Git's own arguments for the host name `ssh` will really connect to (after `~/.ssh/config`
//!   aliases and `Hostname`) and compares it exactly with [`AZURE_DEVOPS_HOSTS`]. Anything else
//!   gets the strict options.
//! - **The strongest classical methods Azure offers, after the post-quantum ones.** Azure offered
//!   `diffie-hellman-group14-sha256`, `-group16-sha512`, `-group-exchange-sha256` and
//!   `-group18-sha512` on 2026-11-07; only the two largest fixed groups are allowed
//!   ([`AZURE_CLASSIC_KEX`]), behind [`crate::PQ_KEX`], so a post-quantum method is chosen as soon
//!   as Azure offers one. Host keys stay strict.
//! - **Visible.** For an Azure connection the stand-in reports the negotiated method on a marker
//!   line ([`KEX_MARKER`]) that the Git layer turns into `pq: false` and a notice.
//! - **Switchable.** The setting [`AZURE_CLASSIC_SETTING`] (default on) and `IDECRAFT_SSH_KEX=mlkem`
//!   each turn it off; the strictest wins.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::pq::{KexPolicy, PQ_KEX, check_client, known_kex, options_with_kex, ssh_options};

/// The Azure DevOps SSH hosts the exception applies to, compared exactly with the host name that
/// `ssh -G` resolves (lower case, no trailing dot, no patterns).
pub const AZURE_DEVOPS_HOSTS: [&str; 2] = ["ssh.dev.azure.com", "vs-ssh.visualstudio.com"];

/// The classical key exchanges allowed for [`AZURE_DEVOPS_HOSTS`] after [`PQ_KEX`], strongest
/// first: finite-field Diffie-Hellman with the 8192-bit and 4096-bit groups of RFC 3526 and SHA-512
/// (RFC 8268). Not allowed although Azure offers them: `diffie-hellman-group14-sha256` (2048 bit)
/// and `diffie-hellman-group-exchange-sha256` (the server picks the group, 2048 bit possible).
/// Never SHA-1, never `group1`.
pub const AZURE_CLASSIC_KEX: [&str; 2] = [
    "diffie-hellman-group18-sha512",
    "diffie-hellman-group16-sha512",
];

/// The setting that allows [`AZURE_CLASSIC_KEX`] for Git to Azure DevOps (default on).
pub const AZURE_CLASSIC_SETTING: &str = "git.ssh.allowClassicKexForAzureDevOps";

/// Start of the line the stand-in writes to standard error after an Azure DevOps connection:
/// `idecraft-git-ssh: host=<resolved host> kex=<negotiated method>`.
pub const KEX_MARKER: &str = "idecraft-git-ssh:";

/// The setting of this process, given by the window or the MCP server from the IDE settings.
static AZURE_CLASSIC_ALLOWED: AtomicBool = AtomicBool::new(true);

/// Applies the setting [`AZURE_CLASSIC_SETTING`] to the Git operations of this process.
pub fn set_azure_classic_kex(allowed: bool) {
    AZURE_CLASSIC_ALLOWED.store(allowed, Ordering::SeqCst);
}

/// The setting [`AZURE_CLASSIC_SETTING`] as last applied (default on).
pub fn azure_classic_kex_setting() -> bool {
    AZURE_CLASSIC_ALLOWED.load(Ordering::SeqCst)
}

/// Whether `resolved` (the `hostname` of `ssh -G`) is an Azure DevOps SSH host: exact comparison.
pub fn is_azure_devops_host(resolved: &str) -> bool {
    AZURE_DEVOPS_HOSTS.contains(&resolved)
}

/// Whether the exception is in force: the setting is on and the policy is not the stricter
/// ML-KEM-only one (the strictest choice wins).
pub fn azure_exception_active(setting: bool, policy: KexPolicy) -> bool {
    setting && policy == KexPolicy::Hybrid
}

/// The key exchanges Git may use to the host `resolved`, for a client knowing `known`: the
/// policy's post-quantum methods, and only for Azure DevOps with the exception in force the
/// allowed classical ones behind them. Empty when the client knows no post-quantum method (it is
/// refused, also for Azure).
pub fn git_kex(
    policy: KexPolicy,
    setting: bool,
    resolved: &str,
    known: &[String],
) -> Vec<&'static str> {
    let mut list = crate::pq::usable_kex(policy, known);
    if list.is_empty() {
        return list;
    }
    if azure_exception_active(setting, policy) && is_azure_devops_host(resolved) {
        list.extend(
            AZURE_CLASSIC_KEX
                .iter()
                .copied()
                .filter(|a| known.iter().any(|k| k == a)),
        );
    }
    list
}

/// The stand-in script for `GIT_SSH_COMMAND`: resolves the host with `ssh -G` and routes jump
/// hosts through the hop stand-in `hop` (a shell word; every hop with the strict options, a
/// `ProxyCommand` of the user's own refused unless `allow_own`, `crate::jump`). It runs `real_ssh`
/// with `strict` for every host but Azure DevOps; with `azure` (`Some` while the exception is in
/// force) for those, plus a debug log in `tmp_dir`, read for the negotiated method and removed,
/// then writes `ssh`'s own messages (without the debug and `-v` progress lines) and the
/// [`KEX_MARKER`] line.
pub fn git_shim_script(
    real_ssh: &str,
    strict: &[String],
    azure: Option<&[String]>,
    tmp_dir: &Path,
    hop: &str,
    allow_own: bool,
) -> String {
    let q = |v: &[String]| {
        v.iter()
            .map(|o| crate::ssh::quote(o))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let strict = q(strict);
    let ssh = crate::ssh::quote(real_ssh);
    let lib = crate::jump::route_lib(hop, allow_own);
    let head = format!(
        "#!/bin/sh\n\
         # Written by IDECraft: Git over SSH with the post-quantum key exchange on every hop; Azure\n\
         # DevOps alone also allows the strongest classical methods it offers (ADR-0025 section 1a).\n\
         {lib}\
         idc_config \"$@\"\n\
         conf=$({ssh} -G {strict} \"$@\" </dev/null 2>/dev/null)\n\
         host=$(printf '%s\\n' \"$conf\" | sed -n 's/^hostname //p' | head -n 1)\n\
         idc_route \"$conf\" \"$idc_cfg\" \"$host\" || exit 255\n\
         if [ -n \"$idc_proxy\" ]; then set -- -o \"$idc_proxy\" \"$@\"; fi\n"
    );
    let Some(azure) = azure else {
        return format!("{head}exec {ssh} {strict} \"$@\"\n");
    };
    let azure = q(azure);
    let hosts = AZURE_DEVOPS_HOSTS.join("|");
    let tmp = crate::ssh::quote(&tmp_dir.join("kex.XXXXXX").display().to_string());
    format!(
        "{head}\
         case \"$host\" in\n\
         {hosts}) ;;\n\
         *) exec {ssh} {strict} \"$@\" ;;\n\
         esac\n\
         log=$(mktemp {tmp}) || exec {ssh} {strict} \"$@\"\n\
         trap 'rm -f \"$log\"; exit 143' HUP INT TERM\n\
         {ssh} {azure} -v -E \"$log\" \"$@\"\n\
         status=$?\n\
         grep -v -E '^(debug[0-9]+: |Authenticated to |Transferred: |Bytes per second: )' \"$log\" >&2\n\
         kex=$(sed -n 's/^debug1: kex: algorithm: //p' \"$log\" | head -n 1)\n\
         rm -f \"$log\"\n\
         printf '{KEX_MARKER} host=%s kex=%s\\n' \"$host\" \"$kex\" >&2\n\
         exit \"$status\"\n"
    )
}

/// The options for Azure DevOps: [`ssh_options`] with the classical methods of the exception
/// behind the post-quantum ones the client knows.
fn azure_options(program: &str) -> Option<Vec<String>> {
    let known = known_kex(program).ok()?;
    let list = git_kex(KexPolicy::Hybrid, true, AZURE_DEVOPS_HOSTS[0], &known);
    (!list.is_empty()).then(|| {
        crate::jump::without_shared_connection(options_with_kex(format!(
            "KexAlgorithms={}",
            list.join(",")
        )))
    })
}

/// Writes the stand-in (once per process and variant, again when it was removed) into a private
/// folder and returns its path; `azure` selects the variant with the exception. Its name carries a
/// hash of its text, so processes of other versions never replace it with a different one.
fn git_shim(azure: bool) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    static SHIM: OnceLock<Mutex<HashMap<(bool, bool), PathBuf>>> = OnceLock::new();
    let allow_own = crate::jump::own_proxy_command_setting();
    let cell = SHIM.get_or_init(|| Mutex::new(HashMap::new()));
    let mut done = cell.lock().ok()?;
    if let Some(p) = done.get(&(azure, allow_own)).filter(|p| p.is_file()) {
        return Some(p.clone());
    }
    let base = crate::ssh::control_dir();
    let dir = base.join("git");
    if !crate::ssh::prepare_control_dir(&base) || !crate::ssh::prepare_control_dir(&dir) {
        return None;
    }
    check_client("ssh").ok()?;
    let hop = crate::jump::hop_path("ssh").ok()?;
    let azure_opts = if azure {
        Some(azure_options("ssh")?)
    } else {
        None
    };
    let script = git_shim_script(
        "ssh",
        &crate::jump::without_shared_connection(ssh_options("ssh")),
        azure_opts.as_deref(),
        &dir,
        &crate::ssh::quote(&hop.display().to_string()),
        allow_own,
    );
    let hash = Sha256::digest(script.as_bytes());
    let name: String = hash[..6].iter().map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("git-ssh-{name}"));
    let part = dir.join(format!("git-ssh.{}.part", std::process::id()));
    std::fs::write(&part, script).ok()?;
    crate::ssh::set_mode(&part, 0o700).ok()?;
    std::fs::rename(&part, &path).ok()?;
    done.insert((azure, allow_own), path.clone());
    Some(path)
}

/// `GIT_SSH_COMMAND` for the given setting and policy: the stand-in (with the Azure DevOps
/// exception while it is in force), or — when no stand-in can be written — the system `ssh` with
/// the strict [`ssh_options`] and the direct route pinned.
pub(crate) fn git_ssh_command_with(setting: bool, policy: KexPolicy) -> String {
    match git_shim(azure_exception_active(setting, policy)) {
        Some(shim) => crate::ssh::quote(&shim.display().to_string()),
        None => crate::pq::strict_git_ssh_command(),
    }
}

/// `GIT_SSH_COMMAND` for Git over SSH in this process ([`git_ssh_command_with`] with the applied
/// setting and the current policy). It replaces a `GIT_SSH_COMMAND` or `core.sshCommand` of the
/// user, so Git cannot connect without the post-quantum exchange (Azure DevOps excepted, see the
/// module); `~/.ssh/config` still applies.
pub(crate) fn git_ssh_command() -> String {
    git_ssh_command_with(azure_classic_kex_setting(), KexPolicy::current())
}

/// One SSH connection the stand-in reported: the resolved host and the negotiated method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KexReport {
    /// The host `ssh` connected to.
    pub host: String,
    /// The negotiated key exchange; empty when the connection ended before it.
    pub kex: String,
}

impl KexReport {
    /// Whether the connection used a post-quantum key exchange; `None` when none was negotiated.
    pub fn pq(&self) -> Option<bool> {
        (!self.kex.is_empty()).then(|| PQ_KEX.contains(&self.kex.as_str()))
    }
}

/// The [`KEX_MARKER`] lines of Git's messages, and the messages without them.
pub fn take_kex_reports(stderr: &str) -> (Vec<KexReport>, String) {
    let mut reports = Vec::new();
    let mut rest = Vec::new();
    for line in stderr.lines() {
        let Some(fields) = line.trim().strip_prefix(KEX_MARKER) else {
            rest.push(line);
            continue;
        };
        let mut host = String::new();
        let mut kex = String::new();
        for f in fields.split_whitespace() {
            if let Some(v) = f.strip_prefix("host=") {
                host = v.to_owned();
            } else if let Some(v) = f.strip_prefix("kex=") {
                kex = v.to_owned();
            }
        }
        reports.push(KexReport { host, kex });
    }
    (reports, rest.join("\n"))
}

/// The notice for Git operations that ran without a post-quantum key exchange to `hosts`.
pub fn classic_kex_notice(hosts: &[String], kex: &[String]) -> String {
    format!(
        "Git over SSH to {} used the classical key exchange {}, not a post-quantum one: Azure DevOps offers none (ADR-0025 exception, setting {AZURE_CLASSIC_SETTING}). Recorded traffic could be decrypted by a future quantum computer; use HTTPS with a token, or SSH once Azure DevOps offers a post-quantum key exchange",
        hosts.join(", "),
        kex.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    /// What OpenSSH 10 knows.
    fn openssh10() -> Vec<String> {
        s(&[
            "diffie-hellman-group1-sha1",
            "diffie-hellman-group14-sha1",
            "diffie-hellman-group14-sha256",
            "diffie-hellman-group16-sha512",
            "diffie-hellman-group18-sha512",
            "diffie-hellman-group-exchange-sha1",
            "diffie-hellman-group-exchange-sha256",
            "ecdh-sha2-nistp256",
            "curve25519-sha256",
            "sntrup761x25519-sha512",
            "sntrup761x25519-sha512@openssh.com",
            "mlkem768x25519-sha256",
        ])
    }

    #[test]
    fn hosts_match_exactly() {
        assert!(is_azure_devops_host("ssh.dev.azure.com"));
        assert!(is_azure_devops_host("vs-ssh.visualstudio.com"));
        for other in [
            "SSH.DEV.AZURE.COM",
            "ssh.dev.azure.com.",
            "x.ssh.dev.azure.com",
            "ssh.dev.azure.com.evil.example",
            "dev.azure.com",
            "evil-ssh.dev.azure.com",
            "*.azure.com",
            "github.com",
            "",
        ] {
            assert!(!is_azure_devops_host(other), "{other}");
        }
    }

    #[test]
    fn only_azure_gets_classical_methods_behind_the_post_quantum_ones() {
        let known = openssh10();
        let azure = git_kex(KexPolicy::Hybrid, true, "ssh.dev.azure.com", &known);
        assert_eq!(&azure[..3], PQ_KEX, "post-quantum first");
        assert_eq!(&azure[3..], AZURE_CLASSIC_KEX);
        for host in [
            "github.com",
            "my-azure-alias",
            "ssh.dev.azure.com.",
            "10.0.0.1",
        ] {
            assert_eq!(
                git_kex(KexPolicy::Hybrid, true, host, &known),
                PQ_KEX,
                "{host}"
            );
        }
        // Setting off, or the stricter policy: the full refusal again.
        assert_eq!(
            git_kex(KexPolicy::Hybrid, false, "ssh.dev.azure.com", &known),
            PQ_KEX
        );
        assert_eq!(
            git_kex(KexPolicy::MlKemOnly, true, "ssh.dev.azure.com", &known),
            ["mlkem768x25519-sha256"]
        );
        assert!(!azure_exception_active(true, KexPolicy::MlKemOnly));
        assert!(!azure_exception_active(false, KexPolicy::Hybrid));
        assert!(azure_exception_active(true, KexPolicy::Hybrid));
        // A client without a post-quantum method is refused for Azure too.
        let classic_client = s(&["curve25519-sha256", "diffie-hellman-group16-sha512"]);
        assert!(
            git_kex(
                KexPolicy::Hybrid,
                true,
                "ssh.dev.azure.com",
                &classic_client
            )
            .is_empty()
        );
    }

    #[test]
    fn the_classical_list_is_strong() {
        for a in AZURE_CLASSIC_KEX {
            assert!(!a.contains("sha1"), "{a}: SHA-1");
            assert!(
                !a.contains("group1-") && !a.contains("group14"),
                "{a}: small group"
            );
            assert!(
                !a.contains("group-exchange"),
                "{a}: the server picks the group"
            );
            assert!(a.ends_with("sha512"), "{a}");
        }
        // Nothing classical leaks into the shared options of the other paths.
        assert!(!PQ_KEX.iter().any(|a| AZURE_CLASSIC_KEX.contains(a)));
    }

    #[test]
    fn the_stand_in_resolves_the_host_and_keeps_others_strict() {
        let strict = s(&[
            "-o",
            "BatchMode=yes",
            "-o",
            "KexAlgorithms=mlkem768x25519-sha256",
        ]);
        let azure = s(&[
            "-o",
            "BatchMode=yes",
            "-o",
            "KexAlgorithms=mlkem768x25519-sha256,diffie-hellman-group18-sha512",
        ]);
        let script = git_shim_script(
            "/usr/bin/ssh",
            &strict,
            Some(&azure),
            Path::new("/tmp/x y"),
            "/tmp/hop",
            false,
        );
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(
            script.contains("/usr/bin/ssh -G -o BatchMode=yes"),
            "{script}"
        );
        assert!(script.contains("ssh.dev.azure.com|vs-ssh.visualstudio.com) ;;"));
        assert!(script.contains(
            "*) exec /usr/bin/ssh -o BatchMode=yes -o KexAlgorithms=mlkem768x25519-sha256 \"$@\" ;;"
        ));
        assert!(script.contains("mktemp '/tmp/x y/kex.XXXXXX'"), "{script}");
        assert!(script.contains("diffie-hellman-group18-sha512 -v -E \"$log\" \"$@\""));
        assert!(script.contains(KEX_MARKER));
        // Jump hosts go through the hop stand-in; without one the direct route is pinned.
        assert!(script.contains("idc_route \"$conf\""), "{script}");
        assert!(
            script.contains("ProxyCommand=$(idc_q /tmp/hop)"),
            "{script}"
        );
        assert!(script.contains("refused %s: ProxyCommand"), "{script}");
        let plain = git_shim_script("/usr/bin/ssh", &strict, None, Path::new("/t"), "/h", true);
        assert!(!plain.contains("diffie-hellman"), "{plain}");
        assert!(!plain.contains("refused %s: ProxyCommand"), "{plain}");
        assert!(plain.ends_with(
            "exec /usr/bin/ssh -o BatchMode=yes -o KexAlgorithms=mlkem768x25519-sha256 \"$@\"\n"
        ));
    }

    #[test]
    fn reports_are_taken_from_the_messages() {
        let stderr = "remote: counting\nidecraft-git-ssh: host=ssh.dev.azure.com kex=diffie-hellman-group18-sha512\nfatal: x";
        let (reports, rest) = take_kex_reports(stderr);
        assert_eq!(rest, "remote: counting\nfatal: x");
        assert_eq!(
            reports,
            [KexReport {
                host: "ssh.dev.azure.com".into(),
                kex: "diffie-hellman-group18-sha512".into()
            }]
        );
        assert_eq!(reports[0].pq(), Some(false));
        let pq = KexReport {
            host: "ssh.dev.azure.com".into(),
            kex: "mlkem768x25519-sha256".into(),
        };
        assert_eq!(pq.pq(), Some(true));
        let none = take_kex_reports("idecraft-git-ssh: host=ssh.dev.azure.com kex=\n").0;
        assert_eq!(none[0].pq(), None);
        let text = classic_kex_notice(
            &["ssh.dev.azure.com".into()],
            &["diffie-hellman-group18-sha512".into()],
        );
        assert!(text.contains("HTTPS with a token"), "{text}");
        assert!(text.contains(AZURE_CLASSIC_SETTING), "{text}");
    }

    #[test]
    fn the_command_is_a_stand_in_or_strict_and_pinned() {
        let strict = crate::pq::strict_git_ssh_command();
        assert!(strict.starts_with("ssh -o BatchMode=yes "), "{strict}");
        assert!(strict.ends_with(" -o ProxyCommand=none"), "{strict}");
        let off = git_ssh_command_with(false, KexPolicy::Hybrid);
        let on = git_ssh_command_with(true, KexPolicy::Hybrid);
        for c in [&off, &on] {
            if *c != strict {
                assert!(c.contains("/git/git-ssh-"), "{c}");
            }
        }
        // The exception and the strict variant are different scripts.
        if off != strict && on != strict {
            assert_ne!(off, on);
            let text = std::fs::read_to_string(&off).unwrap();
            assert!(!text.contains("diffie-hellman"), "{text}");
        }
        assert_eq!(git_ssh_command_with(true, KexPolicy::MlKemOnly), off);
    }
}

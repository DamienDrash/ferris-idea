//! Jump hosts (`ProxyJump`, `-J`, `ProxyCommand`) with the post-quantum key exchange on every hop
//! (ADR-0025 v5 section 1a).
//!
//! OpenSSH opens the hops of a `ProxyJump` with its own `ssh`, without IDECraft's options, so the
//! outer hop could negotiate a classical key exchange. IDECraft therefore never lets OpenSSH build
//! a hop itself:
//!
//! - Before a connection, `ssh -G` resolves the target's configuration. A `ProxyJump` (from
//!   `~/.ssh/config` or `-J`) becomes IDECraft's own `ProxyCommand`: the hop stand-in
//!   ([`hop_script`]), which runs the same `ssh` with the same shared options and `-W` to the next
//!   host, and resolves that hop's own configuration in turn — every hop of a multi-step jump gets
//!   the post-quantum key exchange. Without a jump, `ProxyCommand=none` pins the direct
//!   connection, so nothing in the configuration can add a hop afterwards (OpenSSH keeps the first
//!   value it obtains).
//! - A `ProxyCommand` of the user's own is a program IDECraft cannot hold to the post-quantum rule:
//!   it is refused ([`RemoteError::ProxyCommandRefused`]) unless the setting
//!   [`OWN_PROXY_COMMAND_SETTING`] (default off, shown with a warning) allows it.
//! - Git and the Docker Compose CLI start `ssh` through stand-in scripts; they use the same rules
//!   through the shell functions of [`route_lib`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::ssh::{RemoteError, quote};

/// The setting that allows a `ProxyCommand` of the user's own on SSH paths (default off).
pub const OWN_PROXY_COMMAND_SETTING: &str = "ssh.allowOwnProxyCommand";

/// Start of the lines the hop stand-in writes to standard error: `idecraft-ssh-hop: <hop> exit
/// <status>` when a hop failed, `idecraft-ssh-hop: refused <host>: ProxyCommand` when a hop's own
/// `ProxyCommand` was refused.
pub const HOP_MARKER: &str = "idecraft-ssh-hop:";

/// The most hops one connection may pass (also ends a loop in the configuration).
pub const MAX_HOPS: u32 = 8;

/// File name of the log of connections that used a `ProxyCommand` of the user's own, in the
/// IDECraft log folder.
pub const OWN_PROXY_COMMAND_LOG: &str = "ssh-proxy-command.log";

static OWN_PROXY_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Another folder for [`OWN_PROXY_COMMAND_LOG`] (tests, `crate::testing::redirect_logs`).
pub(crate) static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Applies the setting [`OWN_PROXY_COMMAND_SETTING`] to the SSH paths of this process.
pub fn set_own_proxy_command(allowed: bool) {
    OWN_PROXY_ALLOWED.store(allowed, Ordering::SeqCst);
}

/// The setting [`OWN_PROXY_COMMAND_SETTING`] as last applied (default off).
pub fn own_proxy_command_setting() -> bool {
    OWN_PROXY_ALLOWED.load(Ordering::SeqCst)
}

/// How `ssh` would reach a host according to its resolved configuration (`ssh -G`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// No proxy: a direct connection.
    Direct,
    /// `ProxyJump`: the hops, comma-separated, first hop first.
    Jump(String),
    /// A `ProxyCommand` of the user's own.
    OwnCommand(String),
}

impl Route {
    /// The route `ssh -G` printed in `dump` (`proxyjump` before `proxycommand`, `none` is none).
    pub fn from_config(dump: &str) -> Self {
        let field = |key: &str| {
            dump.lines()
                .find_map(|l| l.strip_prefix(key))
                .map(str::trim)
                .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none"))
                .map(str::to_owned)
        };
        if let Some(j) = field("proxyjump ") {
            Self::Jump(j)
        } else if let Some(c) = field("proxycommand ") {
            Self::OwnCommand(c)
        } else {
            Self::Direct
        }
    }
}

/// The `-F <file>` of `extra` arguments, which the hops use too (as OpenSSH's own `-J` does).
pub(crate) fn config_file(extra: &[String]) -> Option<String> {
    let mut it = extra.iter();
    while let Some(a) = it.next() {
        if a == "-F" {
            return it.next().cloned();
        }
        if let Some(f) = a.strip_prefix("-F").filter(|f| !f.is_empty()) {
            return Some(f.to_owned());
        }
    }
    None
}

/// `extra` without `-J <hops>`: the jump is resolved with `ssh -G` and replaced by IDECraft's own
/// `ProxyCommand` (`ssh` refuses `-J` together with a `ProxyCommand`).
pub(crate) fn without_jump(extra: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(extra.len());
    let mut it = extra.iter();
    while let Some(a) = it.next() {
        if a == "-J" {
            it.next();
        } else if !(a.starts_with("-J") && a.len() > 2) {
            out.push(a.clone());
        }
    }
    out
}

/// A literal for a `ProxyCommand`: quoted for the shell `ssh` runs it with, `%` doubled so that
/// `ssh`'s token expansion keeps it.
fn literal(s: &str) -> String {
    let q = quote(s);
    // `quote` leaves safe words bare; a bare word may still contain `%`.
    q.replace('%', "%%")
}

/// The `ProxyCommand` that reaches a host through `jump` (`hop1,hop2,…`): the hop stand-in `hop`
/// connects to the last hop with `-W` to the host (`%h`, `%p`), the hops before it in turn.
pub fn jump_proxy_command(hop: &str, config: Option<&str>, jump: &str) -> String {
    let (rest, last) = match jump.rsplit_once(',') {
        Some((rest, last)) => (Some(rest), last),
        None => (None, jump),
    };
    let mut s = literal(hop);
    if let Some(c) = config {
        s.push_str(" -F ");
        s.push_str(&literal(c));
    }
    if let Some(r) = rest {
        s.push_str(" --jump ");
        s.push_str(&literal(r));
    }
    s.push_str(" -W '[%h]:%p' ");
    s.push_str(&literal(last));
    s
}

/// The shell functions every stand-in uses to route a connection (POSIX `sh`): `idc_route DUMP
/// CONFIG NAME` sets `idc_proxy` to the `ProxyCommand=…` option for a host whose `ssh -G` output
/// is `DUMP` — the hop stand-in for a `ProxyJump`, `none` for a direct connection — or refuses a
/// `ProxyCommand` of the user's own (returns 1) unless `allow_own`. `hop` is the hop stand-in as
/// a shell word.
pub fn route_lib(hop: &str, allow_own: bool) -> String {
    let own = if allow_own {
        "idc_proxy="
    } else {
        "printf '%s refused %s: ProxyCommand\\n' '@MARKER@' \"$3\" >&2; \
         if [ -n \"$IDECRAFT_SSH_HOP_LOG\" ]; then printf '%s refused %s: ProxyCommand\\n' '@MARKER@' \"$3\" >> \"$IDECRAFT_SSH_HOP_LOG\"; fi; return 1"
    };
    r#"idc_q() { printf "'%s'" "$(printf '%s' "$1" | sed -e "s/'/'\\\\''/g" -e 's/%/%%/g')"; }
idc_jump_to() {
  idc_last=${1##*,}; idc_rest=${1%,*}
  [ "$idc_rest" = "$1" ] && idc_rest=
  idc_proxy="ProxyCommand=$(idc_q @HOP@)"
  [ -n "$2" ] && idc_proxy="$idc_proxy -F $(idc_q "$2")"
  [ -n "$idc_rest" ] && idc_proxy="$idc_proxy --jump $(idc_q "$idc_rest")"
  idc_proxy="$idc_proxy -W '[%h]:%p' $(idc_q "$idc_last")"
}
idc_route() {
  idc_j=$(printf '%s\n' "$1" | sed -n 's/^proxyjump //p' | head -n 1)
  idc_c=$(printf '%s\n' "$1" | sed -n 's/^proxycommand //p' | head -n 1)
  [ "$idc_j" = none ] && idc_j=
  [ "$idc_c" = none ] && idc_c=
  if [ -n "$idc_j" ]; then idc_jump_to "$idc_j" "$2"
  elif [ -n "$idc_c" ]; then @OWN@
  else idc_proxy=ProxyCommand=none; fi
  return 0
}
idc_config() {
  idc_cfg=; idc_prev=
  for idc_a in "$@"; do [ "$idc_prev" = -F ] && idc_cfg=$idc_a; idc_prev=$idc_a; done
}
"#
    .replace("@OWN@", own)
    .replace("@HOP@", hop)
    .replace("@MARKER@", HOP_MARKER)
}

/// The hop stand-in for the `ssh` program `real_ssh` with the options `options`:
/// `<script> [-F config] [--jump hops] -W host:port destination` connects to `destination` (a
/// `ProxyJump` entry, `[user@]host[:port]` or `ssh://…`) and forwards to `host:port`. Without
/// `--jump` it resolves the destination's own configuration, so its `ProxyJump` gets the same
/// treatment and its own `ProxyCommand` the same refusal. At most [`MAX_HOPS`] deep.
pub fn hop_script(real_ssh: &str, options: &[String], allow_own: bool) -> String {
    let opts = options
        .iter()
        .map(|o| quote(o))
        .collect::<Vec<_>>()
        .join(" ");
    let ssh = quote(real_ssh);
    format!(
        "#!/bin/sh\n\
         # Written by IDECraft: one jump host of an SSH connection, with the same post-quantum key\n\
         # exchange as every other hop (ADR-0025 section 1a).\n\
         {lib}\
         d=${{IDECRAFT_SSH_HOP_DEPTH:-0}}\n\
         if [ \"$d\" -ge {MAX_HOPS} ]; then printf '%s more than {MAX_HOPS} jump hosts\\n' '{HOP_MARKER}' >&2; exit 255; fi\n\
         IDECRAFT_SSH_HOP_DEPTH=$((d + 1)); export IDECRAFT_SSH_HOP_DEPTH\n\
         cfg=; jump=; given=\n\
         if [ \"$1\" = -F ]; then cfg=$2; shift 2; fi\n\
         if [ \"$1\" = --jump ]; then jump=$2; given=1; shift 2; fi\n\
         if [ \"$#\" -ne 3 ] || [ \"$1\" != -W ]; then printf '%s usage: [-F config] [--jump hops] -W host:port destination\\n' '{HOP_MARKER}' >&2; exit 255; fi\n\
         fwd=$2; spec=$3\n\
         user=; port=; s=${{spec#ssh://}}; s=${{s%/}}\n\
         case $s in *@*) user=${{s%@*}}; s=${{s##*@}} ;; esac\n\
         case $s in\n\
         \\[*\\]:*) port=${{s##*]:}}; host=${{s#\\[}}; host=${{host%%]*}} ;;\n\
         \\[*\\]) host=${{s#\\[}}; host=${{host%]}} ;;\n\
         *:*:*) host=$s ;;\n\
         *:*) host=${{s%:*}}; port=${{s##*:}} ;;\n\
         *) host=$s ;;\n\
         esac\n\
         set --\n\
         if [ -n \"$cfg\" ]; then set -- \"$@\" -F \"$cfg\"; fi\n\
         if [ -n \"$user\" ]; then set -- \"$@\" -l \"$user\"; fi\n\
         if [ -n \"$port\" ]; then set -- \"$@\" -p \"$port\"; fi\n\
         if [ -n \"$given\" ]; then\n\
         \x20 idc_jump_to \"$jump\" \"$cfg\"\n\
         else\n\
         \x20 conf=$({ssh} {opts} \"$@\" -G -- \"$host\" </dev/null 2>/dev/null)\n\
         \x20 idc_route \"$conf\" \"$cfg\" \"$spec\" || exit 255\n\
         fi\n\
         if [ -n \"$idc_proxy\" ]; then set -- \"$@\" -o \"$idc_proxy\"; fi\n\
         log=$(mktemp \"${{0%/*}}/msg.XXXXXX\" 2>/dev/null) || log=\n\
         if [ -n \"$log\" ]; then set -- \"$@\" -E \"$log\"; trap 'rm -f \"$log\"; exit 143' HUP INT TERM; fi\n\
         {ssh} {opts} \"$@\" -W \"$fwd\" -- \"$host\"\n\
         st=$?\n\
         if [ -n \"$log\" ]; then\n\
         \x20 cat \"$log\" >&2\n\
         \x20 if [ \"$st\" -ne 0 ] && [ -n \"$IDECRAFT_SSH_HOP_LOG\" ]; then cat \"$log\" >> \"$IDECRAFT_SSH_HOP_LOG\"; fi\n\
         \x20 rm -f \"$log\"\n\
         fi\n\
         if [ \"$st\" -ne 0 ]; then\n\
         \x20 printf '%s %s exit %s\\n' '{HOP_MARKER}' \"$spec\" \"$st\" >&2\n\
         \x20 if [ -n \"$IDECRAFT_SSH_HOP_LOG\" ]; then printf '%s %s exit %s\\n' '{HOP_MARKER}' \"$spec\" \"$st\" >> \"$IDECRAFT_SSH_HOP_LOG\"; fi\n\
         fi\n\
         exit \"$st\"\n",
        lib = route_lib("\"$0\"", allow_own),
    )
}

/// The options of every hop: the shared ones of `program` and no shared connection.
fn hop_options(program: &str) -> Vec<String> {
    without_shared_connection(crate::pq::ssh_options(program))
}

/// `options` with no shared connection: a `ControlMaster` a user opened by hand (possibly without
/// the post-quantum exchange) through a `ControlPath` of `~/.ssh/config` is never reused. For the
/// stand-ins of Git and the Compose CLI and for every hop; IDECraft's own connections use their
/// own `ControlPath`.
pub(crate) fn without_shared_connection(mut o: Vec<String>) -> Vec<String> {
    o.extend(
        ["-o", "ControlMaster=no", "-o", "ControlPath=none"]
            .iter()
            .map(|s| (*s).to_owned()),
    );
    o
}

/// Writes the hop stand-in for `program` (once per process and setting, again when it was
/// removed) into a private folder and returns its path. Its name carries a hash of its text, so
/// processes of other versions or settings never replace it with a different one.
pub(crate) fn hop_path(program: &str) -> Result<PathBuf, RemoteError> {
    use sha2::{Digest, Sha256};
    static PATHS: OnceLock<Mutex<HashMap<(String, bool), PathBuf>>> = OnceLock::new();
    let allow = own_proxy_command_setting();
    let key = (program.to_owned(), allow);
    let cell = PATHS.get_or_init(|| Mutex::new(HashMap::new()));
    let fail = |what: String| RemoteError::NoSsh(format!("jump host stand-in: {what}"));
    let mut paths = cell.lock().map_err(|_| fail("locked".into()))?;
    if let Some(p) = paths.get(&key).filter(|p| p.is_file()) {
        return Ok(p.clone());
    }
    let base = crate::ssh::control_dir();
    let dir = base.join("hop");
    if !crate::ssh::prepare_control_dir(&base) || !crate::ssh::prepare_control_dir(&dir) {
        return Err(fail(format!(
            "{}: cannot make a private folder",
            dir.display()
        )));
    }
    let script = hop_script(program, &hop_options(program), allow);
    let hash = Sha256::digest(script.as_bytes());
    let name: String = hash[..6].iter().map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("ssh-hop-{name}"));
    let part = dir.join(format!("ssh-hop.{}.part", std::process::id()));
    let io = |e: std::io::Error| fail(format!("{}: {e}", path.display()));
    std::fs::write(&part, script).map_err(io)?;
    crate::ssh::set_mode(&part, 0o700).map_err(io)?;
    std::fs::rename(&part, &path).map_err(io)?;
    paths.insert(key, path.clone());
    Ok(path)
}

/// How long a resolved route is reused before `ssh -G` is asked again.
const ROUTE_TTL: Duration = Duration::from_secs(30);

/// The route of `destination` for `program` with the configuration arguments `conf` (`-p`, `-l`,
/// the extra arguments), from `ssh -G` with the shared options `shared`; cached briefly, since
/// every remote call asks. When `ssh -G` fails, the connection itself fails the same way, so the
/// route is direct (and pinned).
pub(crate) fn route(program: &str, shared: &[String], conf: &[String], destination: &str) -> Route {
    type Cache = HashMap<String, (Instant, Route)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let key = format!("{program}\0{}\0{destination}", conf.join("\0"));
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((at, r)) = cache.lock().ok().and_then(|c| c.get(&key).cloned())
        && at.elapsed() < ROUTE_TTL
    {
        return r;
    }
    let mut args = shared.to_vec();
    args.extend(conf.iter().cloned());
    args.extend(["-G".to_owned(), "--".to_owned(), destination.to_owned()]);
    let route = crate::launch::output(program, &args)
        .ok()
        .filter(|o| o.status.success())
        .map(|o| Route::from_config(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(Route::Direct);
    if let Ok(mut c) = cache.lock() {
        c.insert(key, (Instant::now(), route.clone()));
    }
    route
}

/// The options that put `route` into effect for a connection of `program` to `host`:
/// `ProxyCommand=none` for a direct one, the hop stand-in for a jump, nothing for a `ProxyCommand`
/// of the user's own when the setting allows it (logged) — otherwise its refusal.
pub(crate) fn route_options(
    route: &Route,
    program: &str,
    config: Option<&str>,
    host: &str,
) -> Result<Vec<String>, RemoteError> {
    match route {
        Route::Direct => Ok(vec!["-o".into(), "ProxyCommand=none".into()]),
        Route::Jump(jump) => {
            let hop = hop_path(program)?;
            Ok(vec![
                "-o".into(),
                format!(
                    "ProxyCommand={}",
                    jump_proxy_command(&hop.display().to_string(), config, jump)
                ),
            ])
        }
        Route::OwnCommand(_) if own_proxy_command_setting() => {
            log_own_proxy_command(host);
            Ok(Vec::new())
        }
        Route::OwnCommand(_) => Err(RemoteError::ProxyCommandRefused(host.to_owned())),
    }
}

/// One line per connection through a `ProxyCommand` of the user's own: time and host, nothing
/// of the command itself.
fn log_own_proxy_command(host: &str) {
    let dir = LOG_DIR.get().cloned().or_else(idecraft_core::logs::log_dir);
    if let Some(dir) = dir {
        let line = format!(
            "{} host={host} own ProxyCommand: its hops are not held to the post-quantum key exchange ({OWN_PROXY_COMMAND_SETTING}, ADR-0025 section 1a)",
            idecraft_core::logs::utc_stamp(std::time::SystemTime::now())
        );
        let _ = idecraft_core::logs::append_line(&dir.join(OWN_PROXY_COMMAND_LOG), &line);
    }
}

/// Environment variable naming a private file the hop stand-ins append their messages to when a
/// hop fails: `ssh` sends a `ProxyCommand`'s messages to `/dev/null` when it keeps a shared
/// connection (`ControlPersist`), so the refusal would otherwise be lost.
pub const HOP_LOG_ENV: &str = "IDECRAFT_SSH_HOP_LOG";

/// A new file name for [`HOP_LOG_ENV`] in the private folder of the hop stand-ins.
pub(crate) fn hop_log_path() -> PathBuf {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    crate::ssh::control_dir().join("hop").join(format!(
        "log-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The hop the stand-in named in `ssh`'s messages: the first `<hop> exit <n>` line (the innermost
/// failing hop) or the host of a refused `ProxyCommand`.
pub(crate) fn failed_hop(stderr: &str) -> Option<(String, bool)> {
    stderr.lines().find_map(|l| {
        let rest = l.trim().strip_prefix(HOP_MARKER)?.trim();
        if let Some(host) = rest
            .strip_prefix("refused ")
            .and_then(|r| r.strip_suffix(": ProxyCommand"))
        {
            return Some((host.to_owned(), true));
        }
        let (hop, _) = rest.rsplit_once(" exit ")?;
        Some((hop.to_owned(), false))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn the_route_comes_from_the_resolved_configuration() {
        let jump = "user me\nhostname 10.0.0.5\nproxyjump u@hop1:2200,hop2\n";
        assert_eq!(
            Route::from_config(jump),
            Route::Jump("u@hop1:2200,hop2".into())
        );
        assert_eq!(
            Route::from_config("hostname x\nproxycommand nc %h %p\n"),
            Route::OwnCommand("nc %h %p".into())
        );
        assert_eq!(
            Route::from_config("hostname x\nproxyjump none\n"),
            Route::Direct
        );
        assert_eq!(
            Route::from_config("hostname x\nproxyusefdpass no\n"),
            Route::Direct
        );
        assert_eq!(Route::from_config(""), Route::Direct);
    }

    #[test]
    fn jump_arguments_are_taken_out_and_the_config_file_kept() {
        let extra = s(&["-F", "/c f", "-J", "a,b", "-Jc", "-v"]);
        assert_eq!(config_file(&extra).as_deref(), Some("/c f"));
        assert_eq!(config_file(&s(&["-F/x"])).as_deref(), Some("/x"));
        assert_eq!(config_file(&s(&["-v"])), None);
        assert_eq!(without_jump(&extra), s(&["-F", "/c f", "-v"]));
    }

    #[test]
    fn the_proxy_command_chains_the_hops_through_the_stand_in() {
        let one = jump_proxy_command("/tmp/h/ssh-hop-1", None, "bastion");
        assert_eq!(one, "/tmp/h/ssh-hop-1 -W '[%h]:%p' bastion");
        let two = jump_proxy_command("/tmp/h/ssh-hop-1", Some("/my cfg"), "u@a:2200,b");
        assert_eq!(
            two,
            "/tmp/h/ssh-hop-1 -F '/my cfg' --jump u@a:2200 -W '[%h]:%p' b"
        );
        // A literal `%` survives ssh's token expansion.
        let pct = jump_proxy_command("/h", Some("/a%b"), "x%y");
        assert!(pct.contains("/a%%b") && pct.contains("x%%y"), "{pct}");
    }

    #[test]
    fn own_proxy_commands_are_refused_by_default() {
        assert!(!own_proxy_command_setting());
        let own = Route::OwnCommand("nc %h %p".into());
        assert_eq!(
            route_options(&own, "ssh", None, "box"),
            Err(RemoteError::ProxyCommandRefused("box".into()))
        );
        assert_eq!(
            route_options(&Route::Direct, "ssh", None, "box").unwrap(),
            ["-o", "ProxyCommand=none"]
        );
        let text = RemoteError::ProxyCommandRefused("box".into()).to_string();
        assert!(text.contains("ProxyJump"), "{text}");
        assert!(text.contains(OWN_PROXY_COMMAND_SETTING), "{text}");
    }

    #[test]
    fn the_hop_stand_in_keeps_every_hop_strict() {
        let opts = s(&[
            "-o",
            "KexAlgorithms=mlkem768x25519-sha256",
            "-o",
            "ControlPath=none",
        ]);
        let deny = hop_script("/usr/bin/ssh", &opts, false);
        assert!(deny.starts_with("#!/bin/sh\n"));
        assert!(deny.contains("refused %s: ProxyCommand"), "{deny}");
        assert!(deny.contains(
            "/usr/bin/ssh -o KexAlgorithms=mlkem768x25519-sha256 -o ControlPath=none \"$@\" -W \"$fwd\" -- \"$host\""
        ));
        assert!(deny.contains("idc_proxy=ProxyCommand=none"));
        assert!(
            deny.contains("$(idc_q \"$0\")"),
            "calls itself for the next hop"
        );
        let allow = hop_script("/usr/bin/ssh", &opts, true);
        assert!(!allow.contains("refused %s: ProxyCommand"));
        assert_ne!(deny, allow);
    }

    #[test]
    fn failing_hops_are_found_in_the_messages() {
        let err = "Unable to negotiate with 127.0.0.1 port 2201: no matching key exchange method found.\n\
                   idecraft-ssh-hop: inner exit 255\nidecraft-ssh-hop: outer exit 255\n\
                   Connection closed by UNKNOWN port 65535\n";
        assert_eq!(failed_hop(err), Some(("inner".into(), false)));
        assert_eq!(
            failed_hop("idecraft-ssh-hop: refused u@b:22: ProxyCommand\n"),
            Some(("u@b:22".into(), true))
        );
        assert_eq!(failed_hop("Permission denied"), None);
    }
}

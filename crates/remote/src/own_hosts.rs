//! Ferris IDEA's own SSH hosts (`SPEC-0121` v5 behaviour 1b): a host list next to the user's
//! `~/.ssh/config`, so a host can be added without editing the user's configuration.
//!
//! The list is one OpenSSH configuration file in the settings folder (`<settings>/ssh/config`,
//! 0600 in a 0700 folder) that `ssh -F` uses as it is: one `Host` block per host, then
//! `Match all` and `Include` lines for the user's `~/.ssh/config` and the system configuration,
//! so the user's own defaults still apply after the host's own lines. The start point
//! ([`crate::SshCommand::build`]) passes `-F <file>` for an alias of this list and for nothing
//! else. Every value is checked before anything is written ([`check_host`]), so a typed alias,
//! host name or user can never add an option or a line; no password or passphrase is kept, a key
//! is referred to by path only. The user's `~/.ssh` is never written.
//!
//! Which files are used is set once per process by the product ([`set_host_files`] with
//! [`HostFiles::for_user`]); tests set files of their own in the temporary folder, and a process
//! that sets nothing has no own hosts, so a test never reads the user's settings folder.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// The first line of Ferris IDEA's own SSH configuration.
pub const OWN_HOSTS_MARKER: &str = "# Ferris IDEA's own SSH hosts (SPEC-0121 v5).";

/// The line after the hosts' blocks; what follows is Ferris IDEA's `Include` of the user's files.
const END_OF_HOSTS: &str = "Match all";

/// The system-wide OpenSSH client configuration, which `ssh -F` would otherwise skip.
const SYSTEM_CONFIG: &str = "/etc/ssh/ssh_config";

/// One of Ferris IDEA's own hosts, as added in the window or with `remote_host_add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OwnHost {
    /// The alias (`Host`).
    pub alias: String,
    /// The real host name or address (`HostName`).
    pub hostname: String,
    /// The port, when not 22.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The login user, when not the local one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// The private key file, by absolute path (never copied or read).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
}

/// The two configuration files of the host list: Ferris IDEA's own and the user's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFiles {
    /// Ferris IDEA's own SSH configuration (`<settings>/ssh/config`).
    pub own: PathBuf,
    /// The user's `~/.ssh/config`, read only; Ferris IDEA's file includes it.
    pub user: PathBuf,
}

impl HostFiles {
    /// The files of the user running Ferris IDEA: `ssh/config` in the settings folder
    /// (`ferris_idea_core::user_config_dir`) and `$HOME/.ssh/config`; `None` without either folder.
    pub fn for_user() -> Option<Self> {
        let config = ferris_idea_core::user_config_dir()?;
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        Some(Self::in_folders(&config, &home))
    }

    /// The files for the settings folder `config` and the home folder `home`.
    pub fn in_folders(config: &Path, home: &Path) -> Self {
        Self {
            own: config.join("ssh").join("config"),
            user: home.join(".ssh").join("config"),
        }
    }
}

static FILES: RwLock<Option<HostFiles>> = RwLock::new(None);

/// Uses `files` for Ferris IDEA's own hosts in this process: the product sets
/// [`HostFiles::for_user`] at start; tests set files of their own; `None` turns own hosts off.
pub fn set_host_files(files: Option<HostFiles>) {
    *FILES.write().unwrap_or_else(|e| e.into_inner()) = files;
}

/// The files set with [`set_host_files`], if any.
pub fn host_files() -> Option<HostFiles> {
    FILES.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Why Ferris IDEA's host list was not changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostFileError {
    /// A value is not allowed; the text names the field and what is allowed.
    #[error("{0}")]
    Invalid(String),
    /// The alias is a host already.
    #[error("{0} is already a host in {1}; choose another alias")]
    Exists(String, String),
    /// The alias is a host of the user's own configuration, which Ferris IDEA never changes.
    #[error(
        "{0} comes from ~/.ssh/config, which Ferris IDEA never changes; edit that file yourself"
    )]
    UserHost(String),
    /// No host with this alias.
    #[error("{0} is not one of Ferris IDEA's hosts")]
    NotFound(String),
    /// No settings folder to keep the list in.
    #[error(
        "Ferris IDEA's host list is not available: no settings folder (set HOME or FERRIS_IDEA_CONFIG_DIR)"
    )]
    NoFiles,
    /// The file could not be written.
    #[error("Ferris IDEA's host list could not be written: {0}")]
    Io(String),
}

fn word(s: &str, max: usize, first: impl Fn(char) -> bool, rest: impl Fn(char) -> bool) -> bool {
    let mut chars = s.chars();
    s.chars().count() <= max && chars.next().is_some_and(first) && chars.all(rest)
}

fn plain(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// Checks and normalises `host` before it is written (`SPEC-0121` v5 behaviour 1b): the alias,
/// host name and user from a small set of characters, so none of them can carry an option, a
/// second word or a line of its own; the identity file an existing absolute path (`~/` is
/// written out with `home`) that is not a public key. Nothing is written here.
pub fn check_host(host: &OwnHost, home: &Path) -> Result<OwnHost, HostFileError> {
    let invalid = |s: &str| Err(HostFileError::Invalid(s.to_owned()));
    let alias = host.alias.trim();
    if !word(alias, 64, plain, |c| {
        plain(c) || matches!(c, '.' | '_' | '-')
    }) {
        return invalid(
            "Alias: 1–64 letters, digits, '.', '_' or '-', starting with a letter or digit",
        );
    }
    let hostname = host.hostname.trim();
    if !word(
        hostname,
        253,
        |c| plain(c) || c == ':',
        |c| plain(c) || matches!(c, '.' | '-' | ':'),
    ) {
        return invalid(
            "Host name: a name or address of 1–253 letters, digits, '.', '-' or ':', starting with a letter, digit or ':'",
        );
    }
    let user = host
        .user
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if let Some(u) = user
        && !word(
            u,
            64,
            |c| plain(c) || c == '_',
            |c| plain(c) || matches!(c, '.' | '_' | '-'),
        )
    {
        return invalid("User: 1–64 letters, digits, '.', '_' or '-', not starting with '-'");
    }
    if host.port == Some(0) {
        return invalid("Port: a number from 1 to 65535");
    }
    let identity = match host
        .identity_file
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        None => None,
        Some(f) => Some(check_identity(f, home)?),
    };
    Ok(OwnHost {
        alias: alias.to_owned(),
        hostname: hostname.to_owned(),
        port: host.port.filter(|p| *p != 22),
        user: user.map(str::to_owned),
        identity_file: identity,
    })
}

fn check_identity(file: &str, home: &Path) -> Result<String, HostFileError> {
    let invalid = |s: &str| Err(HostFileError::Invalid(s.to_owned()));
    if file
        .chars()
        .any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\' | '%'))
    {
        return invalid(
            "Identity file: no control characters, quotes, backslashes or '%' in the path",
        );
    }
    let path = match file.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(file),
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return invalid("Identity file: an absolute path or one starting with ~/");
    }
    if path.extension().is_some_and(|e| e == "pub") {
        return invalid("Identity file: the private key, not its .pub file");
    }
    if !path.is_file() {
        return Err(HostFileError::Invalid(format!(
            "Identity file: {} is not a file",
            path.display()
        )));
    }
    let text = path.display().to_string();
    if text.chars().any(|c| c.is_control() || c == '"') {
        return invalid("Identity file: the path cannot be written to the configuration");
    }
    Ok(text)
}

/// A path for an `ssh_config` line: in double quotes when it holds white space.
fn path_arg(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        format!("\"{path}\"")
    } else {
        path.to_owned()
    }
}

/// The text of Ferris IDEA's configuration for `hosts` that includes the user's `user` file.
pub fn render(hosts: &[OwnHost], user: &Path) -> String {
    let mut s = format!(
        "{OWN_HOSTS_MARKER} Ferris IDEA writes this file: change it in the Remote\n\
         # Host window (Add Host…, Remove Host) or with the MCP tools remote_host_add and\n\
         # remote_host_remove; other changes are lost with the next change there. Ferris IDEA uses it\n\
         # with `ssh -F` for these hosts only (`ssh -F <this file> <alias>` in a terminal too).\n\
         # Your own configuration follows through the Include lines at the end; Ferris IDEA never\n\
         # writes it. No password or passphrase is kept here.\n"
    );
    for h in hosts {
        s.push_str(&format!("\nHost {}\n  HostName {}\n", h.alias, h.hostname));
        if let Some(p) = h.port {
            s.push_str(&format!("  Port {p}\n"));
        }
        if let Some(u) = &h.user {
            s.push_str(&format!("  User {u}\n"));
        }
        if let Some(f) = &h.identity_file {
            s.push_str(&format!(
                "  IdentityFile {}\n  IdentitiesOnly yes\n",
                path_arg(f)
            ));
        }
    }
    s.push_str(&format!(
        "\n{END_OF_HOSTS}\nInclude {}\nInclude {SYSTEM_CONFIG}\n",
        path_arg(&user.display().to_string())
    ));
    s
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    v.strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(v)
        .to_owned()
}

/// The hosts of Ferris IDEA's configuration text `text` (its blocks before `Match all`).
pub fn parse(text: &str) -> Vec<OwnHost> {
    let mut out: Vec<OwnHost> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == END_OF_HOSTS {
            break;
        }
        let Some((key, value)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else {
            continue;
        };
        let value = value.trim_start_matches([' ', '\t', '=']).trim();
        match key.to_ascii_lowercase().as_str() {
            "host" => out.push(OwnHost {
                alias: value.to_owned(),
                ..OwnHost::default()
            }),
            "hostname" => {
                if let Some(h) = out.last_mut() {
                    h.hostname = value.to_owned();
                }
            }
            "port" => {
                if let Some(h) = out.last_mut() {
                    h.port = value.parse().ok();
                }
            }
            "user" => {
                if let Some(h) = out.last_mut() {
                    h.user = Some(value.to_owned());
                }
            }
            "identityfile" => {
                if let Some(h) = out.last_mut() {
                    h.identity_file = Some(unquote(value));
                }
            }
            _ => {}
        }
    }
    out
}

/// Ferris IDEA's own hosts in `files` (none when the file is missing).
pub fn own_hosts(files: &HostFiles) -> Vec<OwnHost> {
    crate::testing::check_config_read(&files.own);
    std::fs::read_to_string(&files.own)
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// Writes `hosts` to Ferris IDEA's file of `files`: the folder made 0700, the text into a temporary
/// file of mode 0600 in the same folder, renamed into place.
fn write(files: &HostFiles, hosts: &[OwnHost]) -> Result<(), HostFileError> {
    let io = |e: std::io::Error| HostFileError::Io(e.to_string());
    let dir = files.own.parent().ok_or(HostFileError::NoFiles)?;
    std::fs::create_dir_all(dir).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    let tmp = dir.join(format!(".config.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut f = options.open(&tmp)?;
        f.write_all(render(hosts, &files.user).as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &files.own)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(io)
}

/// Checks `host` ([`check_host`]) and adds it to Ferris IDEA's list in `files`, unless its alias is a
/// host of either list already; the checked entry as written. `dry_run` checks only.
pub fn add_host(
    files: &HostFiles,
    host: &OwnHost,
    dry_run: bool,
) -> Result<OwnHost, HostFileError> {
    let home = user_home(files);
    let host = check_host(host, home)?;
    let mut hosts = own_hosts(files);
    if hosts.iter().any(|h| h.alias == host.alias) {
        return Err(HostFileError::Exists(
            host.alias,
            "Ferris IDEA's hosts".into(),
        ));
    }
    if crate::config_hosts_in(&files.user, home).contains(&host.alias) {
        return Err(HostFileError::Exists(host.alias, "~/.ssh/config".into()));
    }
    if !dry_run {
        hosts.push(host.clone());
        write(files, &hosts)?;
    }
    Ok(host)
}

/// The home folder of the user's `~/.ssh/config` in `files`.
fn user_home(files: &HostFiles) -> &Path {
    files
        .user
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("/"))
}

/// Removes the host `alias` from Ferris IDEA's list in `files`; a host of the user's `~/.ssh/config`
/// is refused, that file is never written. `dry_run` checks only.
pub fn remove_host(
    files: &HostFiles,
    alias: &str,
    dry_run: bool,
) -> Result<OwnHost, HostFileError> {
    let mut hosts = own_hosts(files);
    let Some(at) = hosts.iter().position(|h| h.alias == alias) else {
        if crate::config_hosts_in(&files.user, user_home(files))
            .iter()
            .any(|a| a == alias)
        {
            return Err(HostFileError::UserHost(alias.to_owned()));
        }
        return Err(HostFileError::NotFound(alias.to_owned()));
    };
    let removed = hosts.remove(at);
    if !dry_run {
        write(files, &hosts)?;
    }
    Ok(removed)
}

/// The host part of an `ssh` destination: `alias` of `user@alias`.
fn host_part(destination: &str) -> &str {
    destination.rsplit_once('@').map_or(destination, |(_, h)| h)
}

/// Ferris IDEA's configuration file when `destination` (`alias` or `user@alias`) is one of its own
/// hosts and `extra` names no `-F` configuration of its own: what the start point passes as
/// `-F` (`SPEC-0121` v5 behaviour 1b).
pub(crate) fn own_config_for(destination: &str, extra: &[String]) -> Option<String> {
    if crate::jump::config_file(extra).is_some() {
        return None;
    }
    let files = host_files()?;
    let alias = host_part(destination);
    if alias.is_empty() || !files.own.is_file() {
        return None;
    }
    own_hosts(&files)
        .iter()
        .any(|h| h.alias == alias)
        .then(|| files.own.display().to_string())
}

/// `path` in single quotes for a POSIX shell, a quote inside written `'\''`.
fn single_quoted(path: &str) -> String {
    format!("'{}'", path.replace('\'', r"'\''"))
}

/// The terminal command that connects to `destination` as Ferris IDEA does, for the user to check
/// and accept an unknown host key (`SPEC-0121` v6 behaviour 2): `ssh -F '<Ferris IDEA's file>'
/// <destination>` for one of Ferris IDEA's own hosts (`user@alias` too, the rule of
/// [`own_config_for`]), `ssh <destination>` for every other host. The file is always quoted, the
/// destination when it needs it, so the command can be copied into a POSIX shell as it is.
pub fn terminal_command(destination: &str) -> String {
    let dest = crate::ssh::quote(destination);
    match own_config_for(destination, &[]) {
        Some(file) => format!("ssh -F {} {dest}", single_quoted(&file)),
        None => format!("ssh {dest}"),
    }
}

/// The step of the stand-ins of Git and the Compose CLI that applies [`own_config_for`]'s rule at
/// run time (`SPEC-0121` v6 behaviour 1b), as POSIX shell text, or nothing for a process without
/// host files. It runs after `idc_config "$@"` (which sets `idc_cfg` to a given `-F`): when no
/// `-F` was given and Ferris IDEA's file `own` exists, it asks `ssh -G` (the program and options as
/// shell words) for the destination of the arguments (`host`, as given, without `user@`) and puts
/// `-F <own>` in front of the arguments when that is the alias of one of the file's `Host` blocks
/// (exact comparison), then runs `idc_config` again. The file is read at every connection, so a
/// host added later is seen without writing the stand-in again.
pub(crate) fn stand_in_step(own: Option<&Path>, ssh: &str, options: &str) -> String {
    let Some(own) = own else {
        return String::new();
    };
    let own = single_quoted(&own.display().to_string());
    format!(
        "idc_own={own}\n\
         if [ -z \"$idc_cfg\" ] && [ -f \"$idc_own\" ]; then\n\
         \x20 idc_dest=$({ssh} {options} -F /dev/null -G \"$@\" </dev/null 2>/dev/null | sed -n 's/^host //p' | head -n 1)\n\
         \x20 if [ -n \"$idc_dest\" ] && sed -n '/^{END_OF_HOSTS}$/q; s/^Host //p' \"$idc_own\" | grep -Fqx -e \"$idc_dest\"; then\n\
         \x20   set -- -F \"$idc_own\" \"$@\"\n\
         \x20   idc_config \"$@\"\n\
         \x20 fi\n\
         fi\n"
    )
}

/// Serialises the unit tests that change the host files of the process ([`set_host_files`]).
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ferris IDEA's own file for the stand-ins of this process: the file of [`host_files`], or `None`
/// for a process without host files.
pub(crate) fn stand_in_file() -> Option<PathBuf> {
    host_files().map(|f| f.own)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ferris-idea-own-hosts-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn host(alias: &str, hostname: &str) -> OwnHost {
        OwnHost {
            alias: alias.into(),
            hostname: hostname.into(),
            ..OwnHost::default()
        }
    }

    #[test]
    fn values_that_could_carry_an_option_or_a_line_are_refused() {
        let home = Path::new("/home/u");
        let bad_aliases = [
            "",
            " ",
            "two words",
            "a\nHost x",
            "-oProxyCommand=x",
            "a=b",
            "*",
            "a*",
            "!a",
            "a\"b",
            "a\tb",
            "a#b",
            "a%h",
            "-F",
        ];
        for a in bad_aliases {
            let e = check_host(&host(a, "example.org"), home).unwrap_err();
            assert!(e.to_string().starts_with("Alias:"), "{a:?}: {e}");
        }
        let bad_names = [
            "",
            "-oProxyCommand=sh",
            "host name",
            "h\nProxyCommand sh",
            "h\rx",
            "%h",
            "a/b",
            "a=b",
            "\"x\"",
            "a;b",
        ];
        for n in bad_names {
            let e = check_host(&host("ok", n), home).unwrap_err();
            assert!(e.to_string().starts_with("Host name:"), "{n:?}: {e}");
        }
        for u in ["-l", "a b", "a\nb", "root=x", "a@b", "a%u", "\"a\""] {
            let mut h = host("ok", "example.org");
            h.user = Some(u.into());
            let e = check_host(&h, home).unwrap_err();
            assert!(e.to_string().starts_with("User:"), "{u:?}: {e}");
        }
        let mut h = host("ok", "example.org");
        h.port = Some(0);
        assert!(
            check_host(&h, home)
                .unwrap_err()
                .to_string()
                .starts_with("Port:")
        );
        let dir = folder("identity");
        std::fs::write(dir.join("id"), "k").unwrap();
        std::fs::write(dir.join("id.pub"), "k").unwrap();
        let bad_files = [
            "relative/id".to_owned(),
            dir.join("missing").display().to_string(),
            dir.join("id.pub").display().to_string(),
            format!("{}/%d/id", dir.display()),
            format!("{}\n  ProxyCommand sh", dir.join("id").display()),
            format!("{}\"", dir.join("id").display()),
            format!(
                "{}/../{}/id",
                dir.display(),
                dir.file_name().unwrap().to_string_lossy()
            ),
        ];
        for f in &bad_files {
            let mut h = host("ok", "example.org");
            h.identity_file = Some(f.clone());
            let e = check_host(&h, home).unwrap_err();
            assert!(e.to_string().starts_with("Identity file:"), "{f:?}: {e}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn good_values_are_kept_and_normalised() {
        let dir = folder("good");
        std::fs::create_dir_all(dir.join(".ssh")).unwrap();
        std::fs::write(dir.join(".ssh/id_test"), "k").unwrap();
        let h = OwnHost {
            alias: " test-vm.1 ".into(),
            hostname: "192.168.64.5".into(),
            port: Some(22),
            user: Some(" dev_user ".into()),
            identity_file: Some("~/.ssh/id_test".into()),
        };
        let c = check_host(&h, &dir).unwrap();
        assert_eq!(c.alias, "test-vm.1");
        assert_eq!(c.port, None, "22 is the default and not written");
        assert_eq!(c.user.as_deref(), Some("dev_user"));
        assert_eq!(
            c.identity_file.as_deref(),
            Some(dir.join(".ssh/id_test").display().to_string().as_str())
        );
        assert!(check_host(&host("v6", "::1"), &dir).is_ok());
        assert!(check_host(&host("v6", "fe80::1"), &dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_is_private_parses_back_and_includes_the_users_config_last() {
        let dir = folder("file");
        let files = HostFiles::in_folders(&dir.join("settings"), &dir.join("home"));
        std::fs::create_dir_all(dir.join("home/.ssh")).unwrap();
        std::fs::write(dir.join("home/.ssh/config"), "Host theirs\n  HostName t\n").unwrap();
        let before = std::fs::read(dir.join("home/.ssh/config")).unwrap();
        let key = dir.join("my key");
        std::fs::write(&key, "k").unwrap();
        let mut a = host("vm", "10.0.0.5");
        a.port = Some(2222);
        a.user = Some("dev".into());
        a.identity_file = Some(key.display().to_string());
        let dry = add_host(&files, &a, true).unwrap();
        assert!(!files.own.exists(), "a dry run writes nothing");
        assert_eq!(add_host(&files, &a, false).unwrap(), dry);
        add_host(&files, &host("box", "box.example"), false).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&files.own), 0o600);
            assert_eq!(mode(files.own.parent().unwrap()), 0o700);
        }
        let text = std::fs::read_to_string(&files.own).unwrap();
        assert!(text.starts_with(OWN_HOSTS_MARKER));
        assert!(text.contains(&format!(
            "IdentityFile \"{}\"\n  IdentitiesOnly yes",
            key.display()
        )));
        let include = text.find("\nMatch all\nInclude").unwrap();
        assert!(
            text.find("Host vm").unwrap() < include && text.find("Host box").unwrap() < include
        );
        assert!(text.trim_end().ends_with(&format!(
            "Match all\nInclude {}\nInclude /etc/ssh/ssh_config",
            files.user.display()
        )));
        for weak in [
            "StrictHostKeyChecking",
            "UserKnownHostsFile",
            "Password",
            "ProxyCommand",
        ] {
            assert!(!text.contains(weak), "{weak}");
        }
        assert_eq!(
            own_hosts(&files),
            vec![dry.clone(), host("box", "box.example")]
        );
        assert_eq!(
            std::fs::read_dir(files.own.parent().unwrap())
                .unwrap()
                .count(),
            1,
            "no temporary file is left"
        );

        // Aliases of either list are refused; the user's file is never written.
        assert_eq!(
            add_host(&files, &host("vm", "x"), false).unwrap_err(),
            HostFileError::Exists("vm".into(), "Ferris IDEA's hosts".into())
        );
        assert_eq!(
            add_host(&files, &host("theirs", "x"), false).unwrap_err(),
            HostFileError::Exists("theirs".into(), "~/.ssh/config".into())
        );
        assert_eq!(
            remove_host(&files, "theirs", false).unwrap_err(),
            HostFileError::UserHost("theirs".into())
        );
        assert_eq!(
            remove_host(&files, "nope", false).unwrap_err(),
            HostFileError::NotFound("nope".into())
        );
        assert_eq!(remove_host(&files, "vm", true).unwrap().alias, "vm");
        assert_eq!(own_hosts(&files).len(), 2, "a dry run removes nothing");
        remove_host(&files, "vm", false).unwrap();
        assert_eq!(own_hosts(&files), vec![host("box", "box.example")]);
        assert_eq!(std::fs::read(dir.join("home/.ssh/config")).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ssh -G -F <file>` reads the file as written: the host's own lines, the quoted key path,
    /// and the user's `Host *` default after them.
    #[test]
    fn ssh_reads_the_file_as_written() {
        let dir = folder("ssh-g");
        let files = HostFiles::in_folders(&dir.join("settings"), &dir.join("home"));
        std::fs::create_dir_all(dir.join("home/.ssh")).unwrap();
        std::fs::write(
            dir.join("home/.ssh/config"),
            "Host *\n  User fromdefaults\n  ServerAliveInterval 17\n",
        )
        .unwrap();
        let key = dir.join("a key");
        std::fs::write(&key, "k").unwrap();
        let mut a = host("vm", "10.0.0.5");
        a.port = Some(2222);
        a.user = Some("dev".into());
        a.identity_file = Some(key.display().to_string());
        add_host(&files, &a, false).unwrap();
        add_host(&files, &host("plain", "plain.example"), false).unwrap();
        // The one start point, as `describe` runs it.
        let own = files.own.display().to_string();
        let g = |alias: &str| {
            crate::SshCommand::new(crate::SshPurpose::Describe)
                .args(["-G"])
                .extra(&["-F".to_owned(), own.clone()])
                .build(alias)
                .unwrap()
                .command()
                .output()
        };
        let Ok(out) = g("vm") else {
            eprintln!("skipped: no ssh");
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        for line in [
            "hostname 10.0.0.5",
            "port 2222",
            "user dev",
            "identitiesonly yes",
            "serveraliveinterval 17",
        ] {
            assert!(text.lines().any(|l| l == line), "{line}");
        }
        assert!(
            text.lines()
                .any(|l| l == format!("identityfile {}", key.display()))
        );
        let plain = String::from_utf8_lossy(&g("plain").unwrap().stdout).to_string();
        assert!(plain.lines().any(|l| l == "user fromdefaults"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_own_aliases_get_the_file() {
        assert_eq!(host_part("me@vm"), "vm");
        assert_eq!(host_part("vm"), "vm");
        assert!(own_config_for("vm", &["-F".into(), "/t/c".into()]).is_none());
    }
}

//! The Welcome window's section Remote Development › SSH, toolkit-free (`SPEC-0355` v3 behaviour
//! 12): its rows — recent SSH projects, then the hosts of the SSH configuration — the search, what
//! opening a row does and the checks of the New Project form. Connecting and opening use the one
//! SSH stack of `ferris_idea_remote` (`SPEC-0121`, `SPEC-0317`); nothing here connects.

use ferris_idea_remote::{HostInfo, HostSpec, RecentRemote};

/// The section's heading (behaviour 12a).
pub const HEADING: &str = "Recent SSH Projects";

/// What the section says without any row (behaviour 12a).
pub const EMPTY: &str =
    "SSH projects appear here after the first connection. New Project connects to a host.";

/// The fixed line of the form on authentication and host keys (behaviours 12d, 12g, 12h).
pub const AUTH_NOTE: &str = "Authentication: your SSH agent or the keys your SSH configuration names (IdentityFile); Ferris IDEA never asks for a password. Host keys must already be known: an unknown key is refused, never accepted here.";

/// One row of the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshRow {
    /// A recent remote project on an SSH host.
    Recent(RecentRemote),
    /// A host of the SSH configuration.
    Host(HostInfo),
}

impl SshRow {
    /// The row's name: the folder's name, or the host's alias.
    pub fn name(&self) -> String {
        match self {
            SshRow::Recent(r) => folder_name(&r.folder),
            SshRow::Host(h) => h.alias.clone(),
        }
    }

    /// The line under the name: `host:folder`, or what `ssh -G` resolves for the host.
    pub fn detail(&self) -> String {
        match self {
            SshRow::Recent(r) => format!("{}:{}", r.host, r.folder),
            SshRow::Host(h) => resolved(h),
        }
    }

    /// The initials in the square (`SPEC-0061` rule).
    pub fn initials(&self) -> String {
        crate::chrome::project_initials(&self.name())
    }

    /// The row as tests and agents name it: `name — detail`, a host marked "(host)".
    pub fn text(&self) -> String {
        match self {
            SshRow::Recent(_) => format!("{} — {}", self.name(), self.detail()),
            SshRow::Host(_) => format!("{} (host) — {}", self.name(), self.detail()),
        }
    }
}

fn folder_name(folder: &str) -> String {
    folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(folder)
        .to_owned()
}

/// `user@hostname`, `:port` when not 22; the alias when nothing was resolved.
fn resolved(h: &HostInfo) -> String {
    let mut s = String::new();
    if let Some(user) = &h.user {
        s.push_str(user);
        s.push('@');
    }
    s.push_str(h.hostname.as_deref().unwrap_or(&h.alias));
    if let Some(port) = h.port.filter(|p| *p != 22) {
        s.push_str(&format!(":{port}"));
    }
    s
}

/// Whether `host` of a recent remote project is an SSH host (an alias or `[user@]host[:port]`),
/// not a container, WSL or a paired host (behaviour 12a).
pub fn is_ssh(host: &str) -> bool {
    matches!(HostSpec::parse(host), Ok(HostSpec::Ssh(_)))
}

/// The rows: the recent SSH projects in their order, then the configured hosts (behaviour 12a).
pub fn ssh_rows(recent: &[RecentRemote], hosts: &[HostInfo]) -> Vec<SshRow> {
    recent
        .iter()
        .filter(|r| is_ssh(&r.host))
        .cloned()
        .map(SshRow::Recent)
        .chain(hosts.iter().cloned().map(SshRow::Host))
        .collect()
}

/// The indexes of the rows the search `query` leaves: name or detail contain it, ignoring case;
/// every row for an empty query (behaviour 12b).
pub fn filter(rows: &[SshRow], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, r)| {
            q.is_empty()
                || r.name().to_lowercase().contains(&q)
                || r.detail().to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

/// What opening a row does (behaviours 12c and 12d).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenSsh {
    /// Open this folder of this host as a remote project.
    Project {
        /// The host as the list keeps it.
        host: String,
        /// The absolute folder on the host.
        folder: String,
    },
    /// Show the New Project form with this host filled in.
    Form(String),
}

/// What opening `row` does.
pub fn open_row(row: &SshRow) -> OpenSsh {
    match row {
        SshRow::Recent(r) => OpenSsh::Project {
            host: r.host.clone(),
            folder: r.folder.clone(),
        },
        SshRow::Host(h) => OpenSsh::Form(h.alias.clone()),
    }
}

/// A name of the form: letters, digits, `.`, `_` and `-`, not starting with `-` (so it is never
/// taken for an option of `ssh`).
fn plain_name(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The host to connect to from the form's Host, User and Port, as `[user@]host[:port]`, or why
/// not (behaviour 12d). Surrounding spaces are ignored.
pub fn connect_spec(host: &str, user: &str, port: &str) -> Result<String, String> {
    let (host, user, port) = (host.trim(), user.trim(), port.trim());
    if host.is_empty() {
        return Err(
            "Type a host: an alias of your SSH configuration, a host name or an address.".into(),
        );
    }
    if !plain_name(host) {
        return Err(format!(
            "Host {host:?} is not valid: it may hold letters, digits, `.`, `_` and `-` and cannot start with `-`."
        ));
    }
    if !user.is_empty() && !plain_name(user) {
        return Err(format!(
            "User {user:?} is not valid: it may hold letters, digits, `.`, `_` and `-` and cannot start with `-`."
        ));
    }
    let port = match port {
        "" => None,
        p => match p.parse::<u16>() {
            Ok(n) if n > 0 => Some(n),
            _ => {
                return Err(format!(
                    "Port {p:?} is not valid: a number from 1 to 65535."
                ));
            }
        },
    };
    let mut spec = String::new();
    if !user.is_empty() {
        spec.push_str(user);
        spec.push('@');
    }
    spec.push_str(host);
    if let Some(port) = port {
        spec.push_str(&format!(":{port}"));
    }
    Ok(spec)
}

/// The rows as `welcome` `list_ssh` answers them (behaviour 22): the recent SSH projects and the
/// configured hosts.
pub fn report(recent: &[RecentRemote], hosts: &[HostInfo]) -> serde_json::Value {
    let recent: Vec<serde_json::Value> = recent
        .iter()
        .filter(|r| is_ssh(&r.host))
        .map(|r| {
            serde_json::json!({ "host": r.host, "folder": r.folder, "name": folder_name(&r.folder) })
        })
        .collect();
    serde_json::json!({ "recent": recent, "hosts": hosts })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recent(host: &str, folder: &str) -> RecentRemote {
        RecentRemote {
            host: host.into(),
            folder: folder.into(),
        }
    }

    fn host(alias: &str, user: Option<&str>, name: Option<&str>, port: Option<u16>) -> HostInfo {
        HostInfo {
            alias: alias.into(),
            user: user.map(str::to_owned),
            hostname: name.map(str::to_owned),
            port,
            ..HostInfo::default()
        }
    }

    fn sample() -> Vec<SshRow> {
        ssh_rows(
            &[
                recent("build", "/srv/shop-front/"),
                recent("docker://web", "/app"),
                recent("me@box:2222", "/home/me/site"),
                recent("paired://10.0.0.7:7000", "/w"),
            ],
            &[
                host("staging", Some("deploy"), Some("10.0.0.9"), Some(22)),
                host("lab", None, Some("lab.example.org"), Some(2200)),
            ],
        )
    }

    /// SPEC-0355 v3 AK-8: recent SSH projects first in their order (containers and paired hosts
    /// left out), then the configured hosts with what `ssh -G` resolved.
    #[test]
    fn rows_are_recent_ssh_projects_then_configured_hosts() {
        let rows = sample();
        assert_eq!(
            rows.iter().map(SshRow::text).collect::<Vec<_>>(),
            [
                "shop-front — build:/srv/shop-front/",
                "site — me@box:2222:/home/me/site",
                "staging (host) — deploy@10.0.0.9",
                "lab (host) — lab.example.org:2200",
            ]
        );
        assert_eq!(rows[0].initials(), "SF");
        assert_eq!(rows[2].initials(), "ST");
        assert_eq!(
            SshRow::Host(host("bare", None, None, None)).detail(),
            "bare",
            "nothing resolved: the alias"
        );
        let report = report(
            &[recent("build", "/srv/app"), recent("docker://web", "/app")],
            &[host("staging", Some("deploy"), Some("10.0.0.9"), Some(22))],
        );
        assert_eq!(report["recent"].as_array().unwrap().len(), 1);
        assert_eq!(report["recent"][0]["name"], "app");
        assert_eq!(report["hosts"][0]["alias"], "staging");
        assert_eq!(report["hosts"][0]["user"], "deploy");
    }

    /// SPEC-0355 v3 AK-8: the search matches name, host and folder, ignoring case.
    #[test]
    fn the_search_matches_name_host_and_folder() {
        let rows = sample();
        assert_eq!(filter(&rows, ""), [0, 1, 2, 3]);
        assert_eq!(filter(&rows, "SHOP"), [0]);
        assert_eq!(filter(&rows, "box"), [1]);
        assert_eq!(filter(&rows, "deploy@"), [2]);
        assert_eq!(filter(&rows, "/srv"), [0]);
        assert!(filter(&rows, "zzz").is_empty());
    }

    /// SPEC-0355 v3 AK-8: a recent row opens its project, a host row fills the form.
    #[test]
    fn a_recent_row_opens_and_a_host_row_fills_the_form() {
        let rows = sample();
        assert_eq!(
            open_row(&rows[1]),
            OpenSsh::Project {
                host: "me@box:2222".into(),
                folder: "/home/me/site".into()
            }
        );
        assert_eq!(open_row(&rows[2]), OpenSsh::Form("staging".into()));
    }

    /// SPEC-0355 v3 AK-8: the form composes `[user@]host[:port]` and refuses what could be taken
    /// for an option or is not a host, user or port.
    #[test]
    fn the_form_composes_the_host_and_refuses_bad_values() {
        assert_eq!(connect_spec("staging", "", "").unwrap(), "staging");
        assert_eq!(
            connect_spec(" box.example.org ", "me", "2222").unwrap(),
            "me@box.example.org:2222"
        );
        assert_eq!(connect_spec("10.0.0.5", "", "22").unwrap(), "10.0.0.5:22");
        for (h, u, p, word) in [
            ("", "", "", "Type a host"),
            ("-oProxyCommand=x", "", "", "Host"),
            ("box;rm", "", "", "Host"),
            ("a b", "", "", "Host"),
            ("me@box", "", "", "Host"),
            ("box", "-l", "", "User"),
            ("box", "a@b", "", "User"),
            ("box", "", "70000", "Port"),
            ("box", "", "0", "Port"),
            ("box", "", "22x", "Port"),
        ] {
            let e = connect_spec(h, u, p).unwrap_err();
            assert!(e.starts_with(word), "{h:?} {u:?} {p:?}: {e}");
        }
        assert!(is_ssh("me@box:2222"));
        assert!(!is_ssh("docker://web"));
    }
}

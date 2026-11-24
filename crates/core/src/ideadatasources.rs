//! Data sources of the reference IDE's shared `.idea/dataSources.xml` (`SPEC-0049` v7 behaviour
//! 18): read only, their URLs cleaned of user info and password-like parameters, and offered —
//! never connected and never added as a connection here. The personal `dataSources.local.xml` and
//! the folder `dataSources/` are never opened.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::ideaproject::{Element, IDEA_DIR, read_xml};

/// The shared file of the database tool, project-relative.
const FILE: &str = ".idea/dataSources.xml";

/// Parts of a parameter name that mark it as a secret (behaviour 18, "Clean URL").
const SECRET_PARTS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "pass",
    "token",
    "secret",
    "key",
    "credential",
    "auth",
];

/// One data source of `.idea/dataSources.xml`, cleaned (`SPEC-0049` v7 behaviour 18).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct IdeaDataSource {
    /// The data source's name.
    pub name: String,
    /// The driver's ref as written (`sqlite.xerial`, `postgresql`, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// Ferris IDEA's kind: `sqlite`, `postgresql`, `mysql`, `mariadb` or `sqlserver`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The URL with `$PROJECT_DIR$` resolved and without user info or password-like parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// SQLite: the database file, project-relative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// A server's host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A server's port, when the URL names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// A server's database, when the URL names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// What cleaning took out of the URL: `user info` and parameter names, never a value.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    /// Why the data source is not offered; none when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl IdeaDataSource {
    /// Whether the data source can be added as a connection.
    pub fn offered(&self) -> bool {
        self.reason.is_none()
    }
}

/// The data sources of the project at `root` (folder format only), for the window: problems with
/// the file are left out here; `idea_project` reports them.
pub fn read_idea_data_sources(root: &Path) -> Vec<IdeaDataSource> {
    if !root.join(IDEA_DIR).is_dir() {
        return Vec::new();
    }
    read_data_sources(root, &mut Vec::new())
}

/// Reads `.idea/dataSources.xml` of `root`; nothing else of the database tool is opened.
pub(crate) fn read_data_sources(root: &Path, problems: &mut Vec<String>) -> Vec<IdeaDataSource> {
    match std::fs::symlink_metadata(root.join(FILE)) {
        Err(_) => return Vec::new(),
        Ok(m) if m.file_type().is_symlink() => {
            problems.push(format!("{FILE}: a symbolic link, not followed"));
            return Vec::new();
        }
        Ok(_) => {}
    }
    let Some(doc) = read_xml(root, FILE, problems) else {
        return Vec::new();
    };
    let component = if doc.name == "component" {
        (doc.attr("name") == Some("DataSourceManagerImpl")).then_some(&doc)
    } else {
        doc.component("DataSourceManagerImpl")
    };
    let Some(component) = component else {
        return Vec::new();
    };
    component
        .all("data-source")
        .map(|e| data_source(root, e))
        .collect()
}

/// A key of a `data-source`: its attribute, or the text of its child element (assumption A2).
fn value(e: &Element, key: &str) -> Option<String> {
    e.attr(key)
        .map(str::to_owned)
        .or_else(|| e.first(key).map(|c| c.text.clone()))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn data_source(root: &Path, e: &Element) -> IdeaDataSource {
    let mut source = IdeaDataSource {
        name: value(e, "name").unwrap_or_default(),
        driver: value(e, "driver-ref"),
        ..IdeaDataSource::default()
    };
    let raw = value(e, "jdbc-url");
    let cleaned = raw.as_deref().map(|u| clean_url(u, root));
    if let Some((url, removed)) = &cleaned {
        source.url = Some(url.clone());
        source.removed = removed.clone();
    }
    let url = cleaned.map(|(u, _)| u);
    let url_kind = url.as_deref().and_then(scheme).map(|s| kind_of(&s));
    let kind = match &source.driver {
        Some(d) => kind_of(d.split('.').next().unwrap_or_default()),
        None => url_kind.clone().flatten(),
    };
    source.kind = kind.clone();
    let reason = (|| -> Result<(), String> {
        if source.name.is_empty() {
            return Err("the data source has no name".into());
        }
        let Some(kind) = kind else {
            return Err(match &source.driver {
                Some(d) => format!("driver `{d}` is not supported"),
                None => "the URL names no supported database".into(),
            });
        };
        let Some(url) = url.as_deref() else {
            return Err("the data source has no URL".into());
        };
        let fits = match url_kind.clone().flatten() {
            Some(k) => k == kind || (kind == "mariadb" && k == "mysql"),
            None => false,
        };
        if !fits {
            return Err(format!("the URL is not a {kind} URL"));
        }
        if kind == "sqlite" {
            source.file = Some(sqlite_file(url, root)?);
        } else {
            let (host, port, database) = server(url, &kind)?;
            source.host = Some(host);
            source.port = port;
            source.database = database;
        }
        Ok(())
    })();
    source.reason = reason.err();
    source
}

/// Ferris IDEA's kind for the first part of a driver ref or a URL scheme (behaviour 18, "Kind").
fn kind_of(name: &str) -> Option<String> {
    let kind = match name.to_ascii_lowercase().as_str() {
        "sqlite" => "sqlite",
        "postgresql" | "postgres" => "postgresql",
        "mysql" => "mysql",
        "mariadb" => "mariadb",
        "sqlserver" | "mssql" => "sqlserver",
        _ => return None,
    };
    Some(kind.to_owned())
}

/// The scheme of `jdbc:<scheme>:...`.
fn scheme(url: &str) -> Option<String> {
    let rest = url.get(..5).filter(|p| p.eq_ignore_ascii_case("jdbc:"))?;
    let rest = &url[rest.len()..];
    rest.split_once(':').map(|(s, _)| s.to_owned())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(s.get(i + 1..i + 3).unwrap_or("zz"), 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a parameter name looks like it holds a secret.
fn secret_name(name: &str) -> bool {
    let plain: String = percent_decode(name)
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | '.'))
        .collect();
    SECRET_PARTS.iter().any(|p| plain.contains(p))
}

/// The URL with `$PROJECT_DIR$` resolved, the user info removed and the password-like parameters
/// removed, and what was removed (names only).
pub(crate) fn clean_url(raw: &str, root: &Path) -> (String, Vec<String>) {
    let project = root.to_string_lossy().replace('\\', "/");
    let url = raw.trim().replace("$PROJECT_DIR$", &project);
    let mut removed = Vec::new();
    // User info: after `//`, up to the last `@` before the path, or up to the last `@` at all when
    // no `=` comes before it (a password with `/`, `?` or `;` in it); parameters hold `=`.
    let url = match url.find("//") {
        Some(start) => {
            let r = &url[start + 2..];
            let before_path = r[..r.find('/').unwrap_or(r.len())].rfind('@');
            let anywhere = r.rfind('@').filter(|at| !r[..*at].contains('='));
            match before_path.max(anywhere) {
                Some(at) => {
                    removed.push("user info".to_owned());
                    format!("{}{}", &url[..start + 2], &r[at + 1..])
                }
                None => url,
            }
        }
        None => url,
    };
    // Where the parameters start: the first `?` or `;`.
    let (head, params) = match url.find(['?', ';']) {
        Some(i) => (url[..i].to_owned(), &url[i..]),
        None => (url.clone(), ""),
    };
    let mut out = head;
    let mut query_started = false;
    let mut rest = params;
    while let Some(sep) = rest.chars().next() {
        let body = &rest[1..];
        let end = body.find(['?', '&', ';']).unwrap_or(body.len());
        let segment = &body[..end];
        rest = &body[end..];
        if segment.is_empty() {
            continue;
        }
        let key = segment.split('=').next().unwrap_or_default();
        if secret_name(key) {
            removed.push(percent_decode(key));
            continue;
        }
        if sep == ';' {
            out.push(';');
        } else {
            out.push(if query_started { '&' } else { '?' });
            query_started = true;
        }
        out.push_str(segment);
    }
    (out, removed)
}

/// Lexical normalisation of an absolute path (`..` and `.` resolved).
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The SQLite file of a clean URL, project-relative; outside the project, with a macro, empty or in
/// memory it has a reason instead.
fn sqlite_file(url: &str, root: &Path) -> Result<String, String> {
    let rest = &url["jdbc:sqlite:".len().min(url.len())..];
    let path = rest.split(['?', ';']).next().unwrap_or_default().trim();
    if path.is_empty() || path.starts_with(":memory:") || path.contains("mode=memory") {
        return Err("an in-memory or unnamed database".into());
    }
    if path.contains('$') {
        return Err("the path holds a macro other than $PROJECT_DIR$".into());
    }
    let path = path.strip_prefix("file:").unwrap_or(path);
    let file = normalise(&root.join(path));
    let roots = [Some(root.to_path_buf()), root.canonicalize().ok()];
    for r in roots.iter().flatten() {
        let r = normalise(r);
        if let Ok(rel) = file.strip_prefix(&r)
            && !rel.as_os_str().is_empty()
        {
            return Ok(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Err("the file lies outside the project".into())
}

/// Host, port and database of a clean server URL (behaviour 18, "Target").
fn server(url: &str, kind: &str) -> Result<(String, Option<u16>, Option<String>), String> {
    let not_understood = || "the URL's form is not understood".to_owned();
    let after = url.split_once(':').and_then(|(_, r)| r.split_once(':'));
    let rest = after.map(|(_, r)| r).ok_or_else(not_understood)?;
    let rest = rest.strip_prefix("//").ok_or_else(not_understood)?;
    let auth_end = rest.find(['/', '?', ';']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let tail = &rest[auth_end..];
    if authority.contains(',') {
        return Err("several hosts are not supported".into());
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, p) = v6.split_once(']').ok_or_else(not_understood)?;
        (h.to_owned(), p.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (authority.to_owned(), None),
        }
    };
    if host.is_empty() {
        return Err("the URL names no host".into());
    }
    if host.contains('\\') {
        return Err("named SQL Server instances are not supported".into());
    }
    let port = match port.filter(|p| !p.is_empty()) {
        Some(p) => Some(
            p.parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| format!("port `{p}` is not a number from 1 to 65535"))?,
        ),
        None => None,
    };
    let database = if kind == "sqlserver" {
        tail.split(';').skip(1).find_map(|prop| {
            let (k, v) = prop.split_once('=')?;
            (k.eq_ignore_ascii_case("databaseName") || k.eq_ignore_ascii_case("database"))
                .then(|| v.trim().to_owned())
        })
    } else {
        tail.strip_prefix('/')
            .map(|d| d.split(['?', ';']).next().unwrap_or_default())
            .map(percent_decode)
    }
    .filter(|d| !d.is_empty());
    Ok((host, port, database))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(raw: &str) -> (String, Vec<String>) {
        clean_url(raw, Path::new("/work/shop"))
    }

    #[test]
    fn the_project_macro_is_resolved() {
        let (url, removed) = clean("jdbc:sqlite:$PROJECT_DIR$/inventory.sqlite");
        assert_eq!(url, "jdbc:sqlite:/work/shop/inventory.sqlite");
        assert!(removed.is_empty());
        assert_eq!(
            sqlite_file(&url, Path::new("/work/shop")),
            Ok("inventory.sqlite".into())
        );
        assert_eq!(
            sqlite_file("jdbc:sqlite:/work/shop/db/../a.db", Path::new("/work/shop")),
            Ok("a.db".into())
        );
        assert_eq!(
            sqlite_file("jdbc:sqlite:data/a.db", Path::new("/work/shop")),
            Ok("data/a.db".into())
        );
        for outside in [
            "jdbc:sqlite:/work/shop/../other.db",
            "jdbc:sqlite:/etc/x.db",
            "jdbc:sqlite:/work/shop",
        ] {
            assert!(
                sqlite_file(outside, Path::new("/work/shop")).is_err(),
                "{outside}"
            );
        }
        assert!(sqlite_file("jdbc:sqlite:$USER_HOME$/a.db", Path::new("/w")).is_err());
        assert!(sqlite_file("jdbc:sqlite::memory:", Path::new("/w")).is_err());
        assert!(sqlite_file("jdbc:sqlite:", Path::new("/w")).is_err());
    }

    #[test]
    fn user_info_and_secrets_are_removed() {
        let (url, removed) = clean(
            "jdbc:postgresql://bob:s3cr3t@db.example:5433/shop?password=hunter2&sslmode=require&access_token=t0k",
        );
        assert_eq!(
            url,
            "jdbc:postgresql://db.example:5433/shop?sslmode=require"
        );
        assert_eq!(removed, ["user info", "password", "access_token"]);
        let (url, removed) = clean(
            "jdbc:sqlserver://srv:1433;databaseName=shop;user=sa;Password=x;encrypt=true;trustStorePassword=y",
        );
        assert_eq!(
            url,
            "jdbc:sqlserver://srv:1433;databaseName=shop;user=sa;encrypt=true"
        );
        assert_eq!(removed, ["Password", "trustStorePassword"]);
        let (url, removed) =
            clean("jdbc:mysql://h/db?PWD=1&api-key=2&secret.value=3&credentials=4&auth=5&x=1");
        assert_eq!(url, "jdbc:mysql://h/db?x=1");
        assert_eq!(removed.len(), 5);
        // A percent-encoded name is still recognised.
        let (url, removed) = clean("jdbc:mysql://h/db?pass%77ord=1");
        assert_eq!(url, "jdbc:mysql://h/db");
        assert_eq!(removed, ["password"]);
        // Passwords with characters of a path or of parameters still go with the user info, and
        // a parameter value with `@` stays.
        for (raw, kept) in [
            ("jdbc:postgresql://u:hun/ter2@h/d", "jdbc:postgresql://h/d"),
            ("jdbc:postgresql://u:hun?ter2@h/d", "jdbc:postgresql://h/d"),
            ("jdbc:postgresql://u:hun=ter2@h/d", "jdbc:postgresql://h/d"),
            (
                "jdbc:postgresql://h/d?user=bob@corp",
                "jdbc:postgresql://h/d?user=bob@corp",
            ),
        ] {
            assert_eq!(clean(raw).0, kept, "{raw}");
        }
    }

    #[test]
    fn driver_refs_and_schemes_map_to_kinds() {
        for (name, kind) in [
            ("sqlite", Some("sqlite")),
            ("SQLite", Some("sqlite")),
            ("postgresql", Some("postgresql")),
            ("postgres", Some("postgresql")),
            ("mysql", Some("mysql")),
            ("mariadb", Some("mariadb")),
            ("sqlserver", Some("sqlserver")),
            ("mssql", Some("sqlserver")),
            ("oracle", None),
            ("h2", None),
        ] {
            assert_eq!(kind_of(name).as_deref(), kind, "{name}");
        }
        assert_eq!(
            scheme("jdbc:postgresql://h/d").as_deref(),
            Some("postgresql")
        );
        assert_eq!(scheme("JDBC:sqlite:a.db").as_deref(), Some("sqlite"));
        assert_eq!(scheme("postgresql://h/d"), None);
    }

    #[test]
    fn servers_give_host_port_and_database() {
        assert_eq!(
            server(
                "jdbc:postgresql://db:5433/shop?sslmode=require",
                "postgresql"
            ),
            Ok(("db".into(), Some(5433), Some("shop".into())))
        );
        assert_eq!(
            server("jdbc:mysql://localhost/inv", "mysql"),
            Ok(("localhost".into(), None, Some("inv".into())))
        );
        assert_eq!(
            server("jdbc:mariadb://[::1]:3307", "mariadb"),
            Ok(("::1".into(), Some(3307), None))
        );
        assert_eq!(
            server("jdbc:sqlserver://srv:1433;databaseName=shop", "sqlserver"),
            Ok(("srv".into(), Some(1433), Some("shop".into())))
        );
        assert!(server("jdbc:sqlserver://srv\\SQLEXPRESS;database=x", "sqlserver").is_err());
        assert!(server("jdbc:postgresql://a,b/d", "postgresql").is_err());
        assert!(server("jdbc:postgresql:shop", "postgresql").is_err());
        assert!(server("jdbc:postgresql://h:x/d", "postgresql").is_err());
    }
}

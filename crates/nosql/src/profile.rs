//! Store profiles and `.ferris-idea/stores.toml` (`SPEC-0310` behaviours 1–4).
//!
//! A store profile is plain data: system, URL without password, read-only flag, keychain entry and
//! agent rights. The file carries the ADR-0021 envelope.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use ferris_idea_database::{Permission, SecretRef, SecretStore, default_permissions};
use serde::{Deserialize, Serialize};

use crate::StoreError;

/// Workspace-relative location of the store file.
pub const STORE_FILE: &str = ".ferris-idea/stores.toml";
/// Envelope kind of the store file (ADR-0021).
pub const STORE_KIND: &str = "stores";
/// Format version this Ferris IDEA reads and writes.
pub const STORE_FORMAT: u32 = 1;

/// Non-relational system of a store profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    /// MongoDB (document store family).
    Mongodb,
    /// Redis (key-value store family).
    Redis,
}

impl System {
    /// Name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            System::Mongodb => "MongoDB",
            System::Redis => "Redis",
        }
    }

    /// Store family (ADR-0022).
    pub fn family(self) -> &'static str {
        match self {
            System::Mongodb => "document",
            System::Redis => "key-value",
        }
    }

    /// Default server port.
    pub fn default_port(self) -> u16 {
        match self {
            System::Mongodb => 27017,
            System::Redis => 6379,
        }
    }
}

/// Transport security of a store URL (behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls {
    /// Plain TCP.
    Off,
    /// TLS with certificate and host name checked.
    Verified,
    /// TLS without checking the certificate (asked for in the URL).
    Unverified,
}

/// What a store URL says, parsed (never holds a password).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// System selected by the scheme.
    pub system: System,
    /// `mongodb+srv` seed list.
    pub srv: bool,
    /// User name, if any.
    pub user: Option<String>,
    /// Hosts with their ports (default port applied; none for `mongodb+srv`).
    pub hosts: Vec<(String, Option<u16>)>,
    /// MongoDB: database in the path; Redis: database number as text.
    pub database: Option<String>,
    /// Redis database number (0–15), 0 for MongoDB.
    pub db_index: u8,
    /// Transport security.
    pub tls: Tls,
    /// Query options in their order (decoded).
    pub options: Vec<(String, String)>,
}

impl Endpoint {
    /// `host:port[,host:port]` as shown in the tree.
    pub fn address(&self) -> String {
        self.hosts
            .iter()
            .map(|(h, p)| match p {
                Some(p) => format!("{h}:{p}"),
                None => h.clone(),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Value of a query option, case-insensitive key.
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// Parses a store URL (behaviour 2).
    pub fn parse(url: &str) -> Result<Self, StoreError> {
        let bad = |m: String| StoreError::Url(m);
        let (scheme, rest) = url.split_once("://").ok_or_else(|| {
            bad("missing scheme (mongodb://, mongodb+srv://, redis://, rediss://)".into())
        })?;
        let scheme = scheme.to_ascii_lowercase();
        let (system, srv, tls_scheme) = match scheme.as_str() {
            "mongodb" => (System::Mongodb, false, false),
            "mongodb+srv" => (System::Mongodb, true, false),
            "redis" => (System::Redis, false, false),
            "rediss" => (System::Redis, false, true),
            other => return Err(bad(format!("unsupported scheme `{other}`"))),
        };
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let hosts_part = match authority.rsplit_once('@') {
            Some((userinfo, hosts)) => {
                if userinfo.contains(':') {
                    return Err(StoreError::PasswordInUrl);
                }
                Some((decode(userinfo)?, hosts))
            }
            None => None,
        };
        let (user, hosts_text) = match hosts_part {
            Some((u, h)) => (Some(u).filter(|u| !u.is_empty()), h),
            None => (None, authority),
        };
        let mut hosts = Vec::new();
        for part in hosts_text.split(',') {
            if part.is_empty() {
                return Err(bad("missing host".into()));
            }
            let (host, port) = split_host_port(part)?;
            if srv && port.is_some() {
                return Err(bad("mongodb+srv URLs take no port".into()));
            }
            let port = if srv {
                None
            } else {
                Some(port.unwrap_or(system.default_port()))
            };
            hosts.push((host, port));
        }
        if srv && hosts.len() != 1 {
            return Err(bad("mongodb+srv URLs name exactly one host".into()));
        }
        if system == System::Redis && hosts.len() != 1 {
            return Err(bad("Redis URLs name exactly one host".into()));
        }
        let mut options = Vec::new();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let (k, v) = (decode(k)?, decode(v)?);
            if k.eq_ignore_ascii_case("password") {
                return Err(StoreError::PasswordInUrl);
            }
            options.push((k, v));
        }
        let path = decode(path)?;
        let (database, db_index) = match system {
            System::Mongodb => (Some(path).filter(|p| !p.is_empty()), 0),
            System::Redis => {
                if path.is_empty() {
                    (None, 0)
                } else {
                    match path.parse::<u8>() {
                        Ok(n) if n <= 15 => (Some(path), n),
                        _ => {
                            return Err(bad(format!(
                                "Redis database `{path}` is not a number from 0 to 15"
                            )));
                        }
                    }
                }
            }
        };
        let mut endpoint = Self {
            system,
            srv,
            user,
            hosts,
            database,
            db_index,
            tls: Tls::Off,
            options,
        };
        let truthy = |v: Option<&str>| v.is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let falsy = |v: Option<&str>| v.is_some_and(|v| v.eq_ignore_ascii_case("false"));
        endpoint.tls = match system {
            System::Mongodb => {
                let tls = endpoint.option("tls").or(endpoint.option("ssl"));
                let on = if srv { !falsy(tls) } else { truthy(tls) };
                let insecure = truthy(endpoint.option("tlsAllowInvalidCertificates"))
                    || truthy(endpoint.option("tlsInsecure"));
                match (on, insecure) {
                    (false, _) => Tls::Off,
                    (true, false) => Tls::Verified,
                    (true, true) => Tls::Unverified,
                }
            }
            System::Redis => match (tls_scheme, truthy(endpoint.option("insecure"))) {
                (false, _) => Tls::Off,
                (true, false) => Tls::Verified,
                (true, true) => Tls::Unverified,
            },
        };
        Ok(endpoint)
    }
}

fn split_host_port(part: &str) -> Result<(String, Option<u16>), StoreError> {
    let (host, port) = if let Some(rest) = part.strip_prefix('[') {
        let (h, after) = rest
            .split_once(']')
            .ok_or_else(|| StoreError::Url("unclosed `[` in host".into()))?;
        (h.to_owned(), after.strip_prefix(':'))
    } else {
        match part.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (part.to_owned(), None),
        }
    };
    let port = match port {
        Some(p) => Some(
            p.parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| StoreError::Url(format!("invalid port `{p}`")))?,
        ),
        None => None,
    };
    if host.is_empty() {
        return Err(StoreError::Url("missing host".into()));
    }
    Ok((decode(&host)?, port))
}

fn decode(s: &str) -> Result<String, StoreError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| StoreError::Url(format!("invalid escape at {i}")))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| StoreError::Url("URL is not UTF-8".into()))
}

/// One non-relational connection (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreProfile {
    /// Stable identifier, unique across `connections.toml` and `stores.toml`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// System.
    pub system: System,
    /// Connection URL without password.
    pub url: String,
    /// Refuse every command that is not a read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// Keychain entry holding the password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<SecretRef>,
    /// What agents may do (SPEC-0012); reading only by default.
    #[serde(
        default = "default_permissions",
        skip_serializing_if = "is_default_permissions"
    )]
    pub ai: BTreeSet<Permission>,
}

fn is_default_permissions(set: &BTreeSet<Permission>) -> bool {
    *set == default_permissions()
}

impl StoreProfile {
    /// A profile called `name` from `url`; the id is derived from the name.
    pub fn from_url(name: &str, url: &str) -> Result<Self, StoreError> {
        let endpoint = Endpoint::parse(url)?;
        Ok(Self {
            id: slug(name),
            name: name.to_owned(),
            system: endpoint.system,
            url: url.to_owned(),
            read_only: false,
            secret: None,
            ai: default_permissions(),
        })
    }

    /// The parsed URL.
    pub fn endpoint(&self) -> Result<Endpoint, StoreError> {
        let e = Endpoint::parse(&self.url)?;
        if e.system != self.system {
            return Err(StoreError::Invalid {
                name: self.name.clone(),
                reason: format!("URL is not a {} URL", self.system.label()),
            });
        }
        Ok(e)
    }

    /// What the tree row says: "MongoDB · host:27017" or "Redis · host:6379/0", plus TLS notes.
    pub fn summary(&self) -> String {
        match self.endpoint() {
            Ok(e) => {
                let mut s = format!("{} · {}", self.system.label(), e.address());
                if self.system == System::Redis {
                    s.push_str(&format!("/{}", e.db_index));
                } else if let Some(db) = &e.database {
                    s.push_str(&format!("/{db}"));
                }
                match e.tls {
                    Tls::Off => {}
                    Tls::Verified => s.push_str(" · TLS"),
                    Tls::Unverified => s.push_str(" · TLS, certificate not checked"),
                }
                s
            }
            Err(err) => format!("{} · {err}", self.system.label()),
        }
    }

    /// Checks the profile on its own.
    pub fn validate(&self) -> Result<(), StoreError> {
        let invalid = |reason: &str| StoreError::Invalid {
            name: self.name.clone(),
            reason: reason.to_owned(),
        };
        if self.name.trim().is_empty() {
            return Err(invalid("name is empty"));
        }
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(invalid("id must be lower-case letters, digits, `-` or `_`"));
        }
        self.endpoint().map(|_| ())
    }

    /// Password from the keychain without waiting for input (behaviour 4); `None` when the profile
    /// names no entry.
    pub fn password(
        &self,
        store: &dyn SecretStore,
    ) -> Result<Option<ferris_idea_database::Secret>, StoreError> {
        let Some(reference) = &self.secret else {
            return Ok(None);
        };
        match store.get(reference) {
            Ok(Some(secret)) => Ok(Some(secret)),
            Ok(None) => Err(StoreError::MissingCredentials {
                profile: self.name.clone(),
                service: reference.service.clone(),
                account: reference.account.clone(),
            }),
            Err(e) => Err(StoreError::Keychain(e.to_string())),
        }
    }
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_owned();
    if s.is_empty() { "store".into() } else { s }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Envelope {
    kind: String,
    format: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    #[serde(
        default,
        rename = "ferris-idea",
        skip_serializing_if = "Option::is_none"
    )]
    ferris_idea: Option<Envelope>,
    #[serde(default)]
    store: Vec<StoreProfile>,
}

/// Checks a list of profiles together: each valid, ids and names unique, no id taken by an SQL
/// connection of `sql_ids`.
pub fn validate_stores(profiles: &[StoreProfile], sql_ids: &[String]) -> Result<(), StoreError> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for p in profiles {
        p.validate()?;
        if !ids.insert(p.id.as_str()) {
            return Err(StoreError::Invalid {
                name: p.name.clone(),
                reason: format!("id `{}` is used twice", p.id),
            });
        }
        if !names.insert(p.name.as_str()) {
            return Err(StoreError::Invalid {
                name: p.name.clone(),
                reason: "name is used twice".into(),
            });
        }
        if sql_ids.iter().any(|s| s == &p.id) {
            return Err(StoreError::Invalid {
                name: p.name.clone(),
                reason: format!(
                    "id `{}` is already an SQL connection in connections.toml",
                    p.id
                ),
            });
        }
    }
    Ok(())
}

fn sql_ids(root: &Path) -> Vec<String> {
    ferris_idea_database::load_profiles(root)
        .map(|ps| ps.into_iter().map(|p| p.id).collect())
        .unwrap_or_default()
}

/// Parses the text of a store file (envelope rules of ADR-0021).
pub fn parse_stores(text: &str) -> Result<Vec<StoreProfile>, StoreError> {
    // An envelope written under the product's former name is read too (`SPEC-0356` behaviour 10).
    let file: StoreFile = ferris_idea_core::format::from_str_adopting(text)
        .map_err(|e| StoreError::File(e.message().to_owned()))?;
    if let Some(env) = &file.ferris_idea {
        if env.kind != STORE_KIND {
            return Err(StoreError::File(format!(
                "kind `{}` is not a store file (expected `{STORE_KIND}`)",
                env.kind
            )));
        }
        if env.format > STORE_FORMAT {
            return Err(StoreError::File(format!(
                "written by a newer Ferris IDEA, format {}; this version reads up to {STORE_FORMAT}",
                env.format
            )));
        }
    }
    Ok(file.store)
}

/// Loads `.ferris-idea/stores.toml` under `root`; no file means no stores.
pub fn load_stores(root: &Path) -> Result<Vec<StoreProfile>, StoreError> {
    let path = root.join(STORE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(StoreError::File(format!("{}: {e}", path.display()))),
    };
    let stores =
        parse_stores(&text).map_err(|e| StoreError::File(format!("{}: {e}", path.display())))?;
    validate_stores(&stores, &sql_ids(root))?;
    Ok(stores)
}

/// Text of a store file with the envelope.
pub fn render_stores(profiles: &[StoreProfile]) -> Result<String, StoreError> {
    let file = StoreFile {
        ferris_idea: Some(Envelope {
            kind: STORE_KIND.into(),
            format: STORE_FORMAT,
        }),
        store: profiles.to_vec(),
    };
    toml::to_string(&file).map_err(|e| StoreError::File(e.to_string()))
}

/// Validates and writes `.ferris-idea/stores.toml` under `root`.
pub fn save_stores(root: &Path, profiles: &[StoreProfile]) -> Result<(), StoreError> {
    validate_stores(profiles, &sql_ids(root))?;
    let path = root.join(STORE_FILE);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| StoreError::File(format!("{}: {e}", dir.display())))?;
    }
    fs::write(&path, render_stores(profiles)?)
        .map_err(|e| StoreError::File(format!("{}: {e}", path.display())))
}

/// A free id for a new store called `name`, unique across both files.
pub fn unique_store_id(name: &str, stores: &[StoreProfile], sql: &[String]) -> String {
    let base = slug(name);
    let taken = |id: &str| stores.iter().any(|s| s.id == id) || sql.iter().any(|s| s == id);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .unwrap_or(base)
}

/// The SQL connection ids of the workspace at `root` (for [`unique_store_id`]).
pub fn sql_connection_ids(root: &Path) -> Vec<String> {
    sql_ids(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_database::MemoryStore;

    #[test]
    fn urls_select_system_ports_and_tls() {
        let e = Endpoint::parse("mongodb://app@db1:27019,db2/shop?authSource=admin").unwrap();
        assert_eq!(e.system, System::Mongodb);
        assert_eq!(e.user.as_deref(), Some("app"));
        assert_eq!(
            e.hosts,
            vec![("db1".into(), Some(27019)), ("db2".into(), Some(27017))]
        );
        assert_eq!(e.database.as_deref(), Some("shop"));
        assert_eq!(e.option("authsource"), Some("admin"));
        assert_eq!(e.tls, Tls::Off);

        let e = Endpoint::parse("MONGODB+SRV://app@cluster0.example.net/").unwrap();
        assert!(e.srv);
        assert_eq!(e.hosts, vec![("cluster0.example.net".into(), None)]);
        assert_eq!(e.tls, Tls::Verified);
        assert_eq!(
            Endpoint::parse("mongodb+srv://c.example.net/?tls=false")
                .unwrap()
                .tls,
            Tls::Off
        );
        assert!(Endpoint::parse("mongodb+srv://c.example.net:27017/").is_err());
        assert_eq!(
            Endpoint::parse("mongodb://h/?tls=true&tlsAllowInvalidCertificates=true")
                .unwrap()
                .tls,
            Tls::Unverified
        );
        assert_eq!(
            Endpoint::parse("mongodb://h/?ssl=true").unwrap().tls,
            Tls::Verified
        );

        let e = Endpoint::parse("redis://default@cache:6380/3").unwrap();
        assert_eq!(e.system, System::Redis);
        assert_eq!(e.db_index, 3);
        assert_eq!(e.address(), "cache:6380");
        assert_eq!(
            Endpoint::parse("redis://cache").unwrap().hosts[0].1,
            Some(6379)
        );
        assert_eq!(
            Endpoint::parse("rediss://cache/0").unwrap().tls,
            Tls::Verified
        );
        assert_eq!(
            Endpoint::parse("rediss://cache/0?insecure=true")
                .unwrap()
                .tls,
            Tls::Unverified
        );
    }

    #[test]
    fn passwords_and_bad_paths_are_refused() {
        for url in [
            "mongodb://app:secret@h/",
            "redis://:secret@h/0",
            "mongodb://h/?password=x",
            "redis://h/0?password=x",
        ] {
            assert!(
                matches!(Endpoint::parse(url), Err(StoreError::PasswordInUrl)),
                "{url}"
            );
        }
        for url in [
            "redis://h/x",
            "redis://h/16",
            "ftp://h",
            "redis://h:0/",
            "mongodb://",
        ] {
            assert!(Endpoint::parse(url).is_err(), "{url}");
        }
        let err = Endpoint::parse("redis://h/16").unwrap_err().to_string();
        assert!(err.contains("`16`"), "{err}");
    }

    #[test]
    fn profiles_round_trip_through_the_file_with_the_envelope() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-stores-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut mongo =
            StoreProfile::from_url("Shop Mongo", "mongodb://app@localhost/shop").unwrap();
        mongo.secret = Some(SecretRef {
            service: "ferris-idea".into(),
            account: "shop-mongo".into(),
        });
        let mut redis = StoreProfile::from_url("Cache", "redis://localhost/2").unwrap();
        redis.read_only = true;
        save_stores(&dir, &[mongo.clone(), redis.clone()]).unwrap();
        let text = fs::read_to_string(dir.join(STORE_FILE)).unwrap();
        assert!(
            text.starts_with("[ferris-idea]\nkind = \"stores\"\nformat = 1"),
            "{text}"
        );
        assert!(!text.contains("ai ="), "{text}");
        assert_eq!(load_stores(&dir).unwrap(), vec![mongo.clone(), redis]);
        assert_eq!(mongo.id, "shop-mongo");
        assert_eq!(mongo.summary(), "MongoDB · localhost:27017/shop");

        // SQL connection with the same id.
        fs::write(
            dir.join(".ferris-idea/connections.toml"),
            "[[connection]]\nid = \"cache\"\nname = \"C\"\ndialect = \"sqlite\"\npath = \"a.db\"\n",
        )
        .unwrap();
        let err = load_stores(&dir).unwrap_err().to_string();
        assert!(err.contains("already an SQL connection"), "{err}");
        assert_eq!(
            unique_store_id("Cache", &[], &sql_connection_ids(&dir)),
            "cache-2"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_refuses_unknown_keys_duplicates_wrong_kind_and_newer_formats() {
        let entry =
            "[[store]]\nid = \"a\"\nname = \"A\"\nsystem = \"redis\"\nurl = \"redis://h\"\n";
        assert_eq!(
            parse_stores(entry).unwrap().len(),
            1,
            "no envelope reads as format 1"
        );
        assert!(parse_stores(&format!("{entry}colour = \"red\"\n")).is_err());
        let err = parse_stores(&format!(
            "[ferris-idea]\nkind = \"keymap\"\nformat = 1\n{entry}"
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("kind `keymap`"), "{err}");
        let err = parse_stores(&format!(
            "[ferris-idea]\nkind = \"stores\"\nformat = 2\n{entry}"
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("newer Ferris IDEA, format 2"), "{err}");
        let twice = parse_stores(&format!("{entry}{}", entry.replace("\"A\"", "\"B\""))).unwrap();
        assert!(
            validate_stores(&twice, &[])
                .unwrap_err()
                .to_string()
                .contains("used twice")
        );
    }

    #[test]
    fn a_missing_keychain_entry_fails_before_any_network_access() {
        let mut p = StoreProfile::from_url("Far", "redis://10.255.255.1:6379/0").unwrap();
        p.secret = Some(SecretRef {
            service: "ferris-idea-test".into(),
            account: "far".into(),
        });
        let started = std::time::Instant::now();
        let err = p.password(&MemoryStore::default()).unwrap_err().to_string();
        assert!(
            err.contains("password missing in keychain entry ferris-idea-test/far"),
            "{err}"
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert_eq!(
            StoreProfile::from_url("x", "redis://h")
                .unwrap()
                .password(&MemoryStore::default())
                .unwrap(),
            None
        );
    }
}

//! Importing, exporting and removing connector definitions (`SPEC-0302`). A definition is shown
//! and validated before anything is stored; stored files keep the received text unchanged, and
//! `imported.json` records where each imported one came from. Exports use the exchange layout of
//! ADR-0021 (`[ferris-idea]` envelope, `.idcconnector`).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::connector::{
    CONNECTOR_KEYS, Catalogue, Connector, EXTENSION, FORMAT, IMPORTED_FILE, Origin, Problem,
    parse_definition,
};
use crate::profile::ConnectionProfile;

/// What the user sees before saving (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    /// The definition, if it validates.
    pub connector: Option<Connector>,
    /// Every problem with its key path; saving is refused while there are any.
    pub problems: Vec<Problem>,
    /// Every SQL text the definition contains, with its key ("Queries this system will run").
    pub queries: Vec<(String, String)>,
    /// Name of the user or imported system an import would replace.
    pub replaces: Option<String>,
    /// The text as received.
    pub text: String,
}

impl Review {
    /// Whether the definition may be saved (no problems).
    pub fn ok(&self) -> bool {
        self.problems.is_empty() && self.connector.is_some()
    }
}

/// Validates `text` against `catalogue` for an import (behaviour 2): format problems, a built-in
/// id (refused) and taken schemes; an id of a user or imported file is reported in `replaces`.
pub fn review(text: &str, catalogue: &Catalogue) -> Review {
    let parsed = parse_definition(text, Origin::Imported(PathBuf::new()));
    let (connector, mut problems) = match parsed {
        Ok(c) => (Some(c), Vec::new()),
        Err(p) => (None, p),
    };
    let mut replaces = None;
    if let Some(c) = &connector {
        // Checked against the catalogue without the file it would replace.
        let mut others = catalogue.clone();
        if let Some(existing) = others.get(&c.id) {
            if existing.origin == Origin::BuiltIn {
                problems.push(Problem {
                    path: "id".into(),
                    message: format!("`{}` is a built-in system and cannot be replaced", c.id),
                });
            } else {
                replaces = Some(existing.name.clone());
                others.connectors.retain(|k| k.id != c.id);
            }
        }
        if let Err(p) = others.check_new(c) {
            problems.extend(p.into_iter().filter(|p| p.path != "id"));
        }
    }
    let queries = connector.as_ref().map(queries_of).unwrap_or_default();
    Review {
        connector: connector.filter(|_| problems.is_empty()),
        problems,
        queries,
        replaces,
        text: text.to_owned(),
    }
}

/// The SQL texts of `c` with their keys.
pub fn queries_of(c: &Connector) -> Vec<(String, String)> {
    let i = &c.introspection;
    let s = &c.sql;
    [
        ("introspection.schemas", &i.schemas),
        ("introspection.columns", &i.columns),
        ("introspection.keys", &i.keys),
        ("introspection.indexes", &i.indexes),
        ("introspection.views", &i.views),
        ("sql.test_query", &s.test_query),
        ("sql.version_query", &s.version_query),
        ("sql.current_schema", &s.current_schema),
        ("sql.explain", &s.explain),
        ("sql.explain_analyze", &s.explain_analyze),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.clone().map(|v| (k.to_owned(), v)))
    .collect()
}

/// Why an import was not stored.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The definition does not validate.
    #[error("the definition has problems: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<Problem>),
    /// It would replace a user or imported system and `replace` was not given.
    #[error("a system called {0} already exists; confirm replacing it")]
    Exists(String),
    /// Writing failed.
    #[error("{0}")]
    Io(String),
}

/// Stores a reviewed definition in `dir` as `<id>.idcconnector` (text unchanged, written
/// atomically, an older `<id>.toml` removed) and records `source` in `imported.json`
/// (behaviour 3). Returns the stored path.
pub fn import(
    dir: &Path,
    text: &str,
    source: &str,
    replace: bool,
    catalogue: &Catalogue,
) -> Result<PathBuf, ImportError> {
    let review = review(text, catalogue);
    let Some(connector) = review.connector else {
        return Err(ImportError::Invalid(review.problems));
    };
    if let Some(name) = review.replaces
        && !replace
    {
        return Err(ImportError::Exists(name));
    }
    let io = |e: std::io::Error| ImportError::Io(e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let path = dir.join(format!("{}.{EXTENSION}", connector.id));
    write_atomic(&path, text).map_err(io)?;
    // The same system as a file of the plain layout, or under another name, goes away.
    if let Some(Origin::User(old) | Origin::Imported(old)) =
        catalogue.get(&connector.id).map(|c| c.origin.clone())
        && old != path
    {
        let _ = std::fs::remove_file(old);
    }
    let record = dir.join(IMPORTED_FILE);
    let mut map = read_record(&record);
    map.insert(
        connector.id.clone(),
        serde_json::json!({
            "source": source,
            "imported": now_utc(),
        }),
    );
    write_atomic(
        &record,
        &serde_json::to_string_pretty(&map).unwrap_or_default(),
    )
    .map_err(io)?;
    Ok(path)
}

/// Removes a user or imported system's file and its import record (behaviour 6).
pub fn remove(dir: &Path, id: &str, catalogue: &Catalogue) -> Result<String, String> {
    let c = catalogue
        .get(id)
        .ok_or_else(|| format!("no database system `{id}`"))?;
    let path = match &c.origin {
        Origin::BuiltIn => return Err(format!("{} is built in and cannot be removed", c.name)),
        Origin::User(p) | Origin::Imported(p) => p.clone(),
    };
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let record = dir.join(IMPORTED_FILE);
    let mut map = read_record(&record);
    if map.remove(id).is_some() {
        let text = serde_json::to_string_pretty(&map).unwrap_or_default();
        write_atomic(&record, &text).map_err(|e| e.to_string())?;
    }
    Ok(c.name.clone())
}

/// The current time as `YYYY-MM-DDTHH:MM:SSZ`.
fn now_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    chrono::DateTime::from_timestamp(i64::try_from(secs).unwrap_or(0), 0)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

fn read_record(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The text to share for `c` (behaviour 4) in the exchange layout: a file already in it unchanged;
/// otherwise the envelope first, then `[connector]` without `status` and `tested` (a shared copy
/// never claims Supported) and the tables of the definition.
pub fn export(c: &Connector) -> String {
    let mut table: toml::Table = toml::from_str(&c.source).unwrap_or_default();
    ferris_idea_core::format::adopt_former_envelope(&mut table);
    if c.origin != Origin::BuiltIn && table.contains_key("ferris-idea") {
        return c.source.clone();
    }
    // Written by hand so the envelope keys keep the order SPEC-0303 shows.
    let quoted = |v: &str| toml::Value::String(v.to_owned()).to_string();
    let mut envelope = format!(
        "kind = \"connector\"\nformat = {FORMAT}\nid = {}\nname = {}\n",
        quoted(&c.id),
        quoted(&c.name)
    );
    if let Some(h) = &c.homepage {
        envelope.push_str(&format!("homepage = {}\n", quoted(h)));
    }
    let mut body = toml::Table::new();
    let mut rest = toml::Table::new();
    for (key, value) in table {
        match key.as_str() {
            "status" | "tested" | "format" | "id" | "name" | "homepage" => {}
            "connection" | "sql" | "introspection" => {
                rest.insert(key, value);
            }
            k if CONNECTOR_KEYS.contains(&k) => {
                body.insert(key, value);
            }
            _ => {}
        }
    }
    let part = |t: &toml::Table| toml::to_string(t).unwrap_or_default();
    format!(
        "# Ferris IDEA database connector (SPEC-0300); import it with \"Import database system…\".\n\
         [ferris-idea]\n{envelope}\n[connector]\n{}\n{}",
        part(&body),
        toml::to_string_pretty(&rest).unwrap_or_default()
    )
}

/// Where "Share…" opens a prefilled issue (GitHub Issues, the project owner 2026-11-05; behaviour 4).
pub const SHARE_URL: &str = "https://github.com/DamienDrash/ferris-idea/issues/new?template=connector-definition.md&title={title}&body={body}";

/// The prefilled issue for sharing `c` and whether its body was cut (behaviour 4).
pub fn share_url(c: &Connector) -> (String, bool) {
    let text = export(c);
    let report = crate::report::Report {
        title: format!("[connector] {}", c.name),
        body: format!(
            "Definition of **{}** (`{}`, family {}, {}). Attach the file `{}` if the text below is cut.\n\n```toml\n{}```\n",
            c.name,
            c.id,
            c.family.name(),
            c.category.label(),
            export_file_name(c),
            text
        ),
    };
    crate::report::issue_url(SHARE_URL, &report).unwrap_or_default()
}

/// File name an export proposes: `<id>.idcconnector`.
pub fn export_file_name(c: &Connector) -> String {
    format!("{}.{EXTENSION}", c.id)
}

/// A connection to share (behaviour 5) as a `.idcsources` file: the envelope of ADR-0021 and one
/// `[[connection]]` without keychain reference, agent permissions and, unless `include_user`, the
/// user name; an absolute database file path is left out too (SPEC-0303 behaviour 5). Returns the
/// text and what was left out.
pub fn export_profile(profile: &ConnectionProfile, include_user: bool) -> (String, Vec<String>) {
    #[derive(Serialize)]
    struct File<'a> {
        #[serde(rename = "ferris-idea")]
        ferris_idea: Envelope,
        connection: [&'a ConnectionProfile; 1],
    }
    #[derive(Serialize)]
    struct Envelope {
        kind: &'static str,
        format: i64,
    }
    let mut left_out = Vec::new();
    let mut p = profile.clone();
    if p.secret.take().is_some() {
        left_out.push("secret (the password stays in your keychain)".to_owned());
    }
    if p.ai != crate::permission::default_permissions() {
        left_out.push("ai (agent rights are set by each user)".to_owned());
    }
    p.ai = crate::permission::default_permissions();
    if !include_user && p.user.take().is_some() {
        left_out.push("user".to_owned());
    }
    if p.path.as_ref().is_some_and(|path| path.is_absolute()) {
        p.path = None;
        left_out.push("path (absolute paths are machine-bound)".to_owned());
    }
    let text = toml::to_string_pretty(&File {
        ferris_idea: Envelope {
            kind: "sources",
            format: 1,
        },
        connection: [&p],
    })
    .unwrap_or_default();
    (text, left_out)
}

/// File name a connection export proposes: its id with `.idcsources`.
pub fn export_profile_file_name(profile: &ConnectionProfile) -> String {
    format!("{}.idcsources", profile.id)
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: &str, scheme: &str) -> String {
        format!(
            "format = 1\nid = \"{id}\"\nname = \"{id} DB\"\nfamily = \"postgresql\"\ncategory = \"other\"\n\
             [connection]\nschemes = [\"{scheme}\"]\ndefault_port = 7777\n\
             [introspection]\ncolumns = \"select 1 from t where s = {{schema}}\"\n"
        )
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn import_stores_records_and_replaces_only_when_confirmed() {
        let dir = temp("import");
        let catalogue = Catalogue::load(Some(&dir));
        let r = review(&def("acme", "acme"), &catalogue);
        assert!(r.ok(), "{:?}", r.problems);
        assert_eq!(r.queries[0].0, "introspection.columns");
        // A plain-layout file the user put there by hand is replaced by the imported one.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("acme.toml"), def("acme", "acme")).unwrap();
        let catalogue = Catalogue::load(Some(&dir));
        assert_eq!(
            review(&def("acme", "acme"), &catalogue).replaces.as_deref(),
            Some("acme DB")
        );
        let path = import(
            &dir,
            &def("acme", "acme"),
            "/tmp/acme.toml",
            true,
            &catalogue,
        )
        .unwrap();
        assert!(path.ends_with("acme.idcconnector") && !dir.join("acme.toml").exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), def("acme", "acme"));
        let catalogue = Catalogue::load(Some(&dir));
        let acme = catalogue.get("acme").unwrap();
        assert_eq!(acme.status, crate::connector::Status::Imported);
        let record = std::fs::read_to_string(dir.join(IMPORTED_FILE)).unwrap();
        assert!(
            record.contains("\"source\": \"/tmp/acme.toml\""),
            "{record}"
        );
        // Same id again: asks, then replaces; same scheme under the same id is not a clash.
        let again = def("acme", "acme");
        assert_eq!(
            review(&again, &catalogue).replaces.as_deref(),
            Some("acme DB")
        );
        assert!(matches!(
            import(&dir, &again, "x", false, &catalogue),
            Err(ImportError::Exists(_))
        ));
        import(&dir, &again, "x", true, &catalogue).unwrap();
        // Built-ins are refused; invalid files list their problems; a taken scheme is named.
        let tidb = def("tidb", "tidb2");
        assert!(
            review(&tidb, &catalogue).problems[0]
                .message
                .contains("built-in")
        );
        let clash = review(&def("other", "acme"), &Catalogue::load(Some(&dir)));
        assert_eq!(clash.problems[0].path, "connection.schemes[0]");
        assert!(matches!(
            import(&dir, "format = 2", "x", false, &catalogue),
            Err(ImportError::Invalid(_))
        ));
        // Removing deletes the file and its record.
        let catalogue = Catalogue::load(Some(&dir));
        assert_eq!(remove(&dir, "acme", &catalogue).unwrap(), "acme DB");
        assert!(!dir.join("acme.idcconnector").exists());
        assert!(
            !std::fs::read_to_string(dir.join(IMPORTED_FILE))
                .unwrap()
                .contains("acme")
        );
        assert!(remove(&dir, "tidb", &catalogue).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exports_never_claim_supported_and_round_trip() {
        let catalogue = Catalogue::built_in();
        let pg = export(catalogue.get("postgresql").unwrap());
        assert!(
            pg.contains("[ferris-idea]\nkind = \"connector\"\nformat = 1\nid = \"postgresql\""),
            "{pg}"
        );
        assert!(!pg.contains("status") && !pg.contains("tested"), "{pg}");
        let parsed = parse_definition(&pg, Origin::User(PathBuf::from("x"))).unwrap();
        assert_eq!(parsed.status, crate::connector::Status::User);
        assert_eq!(
            export_file_name(catalogue.get("tidb").unwrap()),
            "tidb.idcconnector"
        );
        let tidb = catalogue.get("tidb").unwrap();
        let shared = export(tidb);
        let dir = temp("roundtrip");
        // A built-in id cannot be imported back; under a new id the same system comes back.
        assert!(
            shared.contains("[sql]") && shared.contains("explain_analyze"),
            "{shared}"
        );
        let renamed = shared
            .replace("id = \"tidb\"", "id = \"tidb-copy\"")
            .replace("schemes = [\"tidb\"]", "schemes = [\"tidbc\"]");
        import(
            &dir,
            &renamed,
            "export",
            false,
            &Catalogue::load(Some(&dir)),
        )
        .unwrap();
        let back = Catalogue::load(Some(&dir));
        let copy = back.get("tidb-copy").unwrap();
        // Exporting an exchange-layout file gives its text back unchanged.
        assert_eq!(export(copy), renamed);
        assert_eq!(copy.connection.default_port, tidb.connection.default_port);
        assert_eq!(copy.sql, tidb.sql);
        assert_eq!(copy.family, tidb.family);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn share_opens_the_connector_template() {
        let catalogue = Catalogue::built_in();
        let (url, cut) = share_url(catalogue.get("tidb").unwrap());
        assert!(!cut);
        assert!(url.starts_with(
            "https://github.com/DamienDrash/ferris-idea/issues/new?template=connector-definition.md&title=%5Bconnector%5D%20TiDB&body="
        ));
        assert!(
            url.contains("%5Bferris-idea%5D"),
            "the definition is in the body"
        );
    }

    #[test]
    fn profile_export_has_no_secrets() {
        let mut p = ConnectionProfile::from_url("Shop", "tidb://alice@h:4000/shop").unwrap();
        p.secret = Some(crate::SecretRef {
            service: "ferris-idea".into(),
            account: "connection:shop".into(),
        });
        p.ai = [crate::Permission::ModifyData].into_iter().collect();
        let (text, left_out) = export_profile(&p, false);
        assert!(
            text.starts_with("[ferris-idea]\nkind = \"sources\"\nformat = 1\n"),
            "{text}"
        );
        assert!(
            text.contains("[[connection]]") && text.contains("system = \"tidb\""),
            "{text}"
        );
        assert!(
            !text.contains("secret") && !text.contains("alice") && !text.contains("ai ="),
            "{text}"
        );
        assert_eq!(left_out.len(), 3, "{left_out:?}");
        assert!(export_profile(&p, true).0.contains("user = \"alice\""));
        let file = ConnectionProfile::from_url("Local", "duckdb:/Users/x/data.duckdb");
        if let Ok(file) = file {
            let (text, left_out) = export_profile(&file, false);
            assert!(!text.contains("/Users/x") && left_out.iter().any(|l| l.starts_with("path")));
        }
    }
}

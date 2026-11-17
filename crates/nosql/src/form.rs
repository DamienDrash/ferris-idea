//! Store connections from the connection dialog and MCP (`SPEC-0310` behaviours 1–4, 6, 8): the
//! dialog's host/port/database/user/SSL fields become a store URL; the password goes to the
//! keychain, never into the file.

use std::path::Path;

use idecraft_database::{ConnectionForm, Secret, SecretStore, SslMode, secret_ref};

use crate::{
    MongoSession, RedisSession, StoreError, StoreProfile, System, load_stores, save_stores,
    sql_connection_ids, unique_store_id,
};

/// The store system of connector `id` when it is a non-relational store (SPEC-0300 catalogue).
pub fn store_system(id: &str) -> Option<System> {
    let c = idecraft_database::catalogue().get(id)?.clone();
    if !c.is_store() {
        return None;
    }
    match c.family.name() {
        "mongodb" => Some(System::Mongodb),
        "redis" => Some(System::Redis),
        _ => None,
    }
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The store URL the dialog's fields describe, if its system is a store.
pub fn form_url(form: &ConnectionForm) -> Option<Result<(System, String), StoreError>> {
    let system = store_system(form.system.as_deref()?)?;
    let host = form.host.trim();
    if host.is_empty() {
        return Some(Err(StoreError::Url("host is required".into())));
    }
    let port = match form.port.trim() {
        "" => None,
        p => match p.parse::<u16>() {
            Ok(p) if p > 0 => Some(p),
            _ => return Some(Err(StoreError::Url(format!("invalid port `{p}`")))),
        },
    };
    // `prefer` off the loopback is TLS without certificate checks, as `require` (SPEC-0310 v4
    // behaviour 3, SPEC-0016 v3); plain text only for a store on the loopback.
    let ssl = match form.ssl {
        SslMode::Prefer if !idecraft_database::is_loopback(Some(host)) => SslMode::Require,
        mode => mode,
    };
    let tls = matches!(
        ssl,
        SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull
    );
    let scheme = match (system, tls) {
        (System::Mongodb, _) => "mongodb",
        (System::Redis, false) => "redis",
        (System::Redis, true) => "rediss",
    };
    let user = form.user.trim();
    let mut url = format!("{scheme}://");
    if !user.is_empty() {
        url.push_str(&encode(user));
        url.push('@');
    }
    url.push_str(host);
    if let Some(p) = port {
        url.push_str(&format!(":{p}"));
    }
    url.push('/');
    url.push_str(&encode(form.database.trim()));
    let mut query: Vec<&str> = Vec::new();
    match (system, ssl) {
        (System::Mongodb, SslMode::VerifyFull | SslMode::VerifyCa) => query.push("tls=true"),
        (System::Mongodb, SslMode::Require) => {
            query.extend(["tls=true", "tlsAllowInvalidCertificates=true"]);
        }
        (System::Redis, SslMode::Require) => query.push("insecure=true"),
        _ => {}
    }
    if system == System::Mongodb && !user.is_empty() {
        query.push("authSource=admin");
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    Some(Ok((system, url)))
}

/// The profile a dialog form describes (id given), or `None` for an SQL system.
pub fn form_profile(form: &ConnectionForm, id: &str) -> Option<Result<StoreProfile, StoreError>> {
    Some(form_url(form)?.and_then(|(_, url)| {
        let mut p = StoreProfile::from_url(&form.name, &url)?;
        p.id = id.to_owned();
        p.read_only = form.read_only;
        p.validate()?;
        Ok(p)
    }))
}

/// Adds or changes a store from `profile`; a typed `password` goes to the keychain first. Keeps
/// the agent rights and keychain entry of the store it replaces. Returns the name.
pub fn save_store(
    root: &Path,
    mut profile: StoreProfile,
    replace: Option<&str>,
    password: Option<&str>,
    secrets: &dyn SecretStore,
) -> Result<String, StoreError> {
    let mut stores = load_stores(root)?;
    let sql = sql_connection_ids(root);
    let base = replace.and_then(|id| stores.iter().position(|s| s.id == id));
    match base {
        Some(i) => {
            profile.id = stores[i].id.clone();
            profile.ai = stores[i].ai.clone();
            profile.secret = stores[i].secret.clone();
        }
        None => profile.id = unique_store_id(&profile.name, &stores, &sql),
    }
    profile.validate()?;
    if let Some(pw) = password {
        let reference = profile
            .secret
            .clone()
            .unwrap_or_else(|| secret_ref(&profile.id));
        secrets
            .set(&reference, &Secret::new(pw))
            .map_err(|e| StoreError::Keychain(e.to_string()))?;
        profile.secret = Some(reference);
    }
    let name = profile.name.clone();
    match base {
        Some(i) => stores[i] = profile,
        None => stores.push(profile),
    }
    save_stores(root, &stores)?;
    Ok(name)
}

/// Removes store `id` and its keychain entry; returns its name.
pub fn remove_store(
    root: &Path,
    id: &str,
    secrets: &dyn SecretStore,
) -> Result<String, StoreError> {
    let mut stores = load_stores(root)?;
    let i = stores
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| StoreError::File(format!("no store `{id}`")))?;
    let removed = stores.remove(i);
    if let Some(reference) = &removed.secret {
        secrets
            .delete(reference)
            .map_err(|e| StoreError::Keychain(e.to_string()))?;
    }
    save_stores(root, &stores)?;
    Ok(removed.name)
}

/// Connects and answers "Connected: MongoDB 8.2.12" (Test Connection, SPEC-0310 behaviour 4).
pub async fn test_store(
    profile: &StoreProfile,
    password: Option<Secret>,
) -> Result<String, StoreError> {
    let version = match profile.system {
        System::Mongodb => {
            MongoSession::connect(profile, password)
                .await?
                .server_version()
                .await?
        }
        System::Redis => {
            RedisSession::connect(profile, password)
                .await?
                .server_version()
                .await?
        }
    };
    Ok(format!("Connected: {} {version}", profile.system.label()))
}

/// Test Connection from the dialog: the typed password, else the keychain's; blocking.
pub fn test_store_form(
    root: &Path,
    id: Option<&str>,
    profile: &StoreProfile,
    password: Option<&str>,
    secrets: &dyn SecretStore,
) -> Result<String, String> {
    let mut profile = profile.clone();
    if let Some(base) = id.and_then(|id| load_stores(root).ok()?.into_iter().find(|s| s.id == id)) {
        profile.secret = base.secret;
    }
    let password = match password {
        Some(pw) => Some(Secret::new(pw)),
        None => profile.password(secrets).map_err(|e| match e {
            StoreError::MissingCredentials {
                service, account, ..
            } => format!(
                "no password in the keychain for {service}/{account}; type it into the dialog"
            ),
            other => other.to_string(),
        })?,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(test_store(&profile, password))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_database::MemoryStore;

    fn form(system: &str, ssl: SslMode) -> ConnectionForm {
        ConnectionForm {
            name: "Shop".into(),
            dialect: idecraft_database::Dialect::PostgreSql,
            system: Some(system.into()),
            host: "db.local".into(),
            port: "27018".into(),
            database: "shop".into(),
            user: "app user".into(),
            path: String::new(),
            ssl,
            read_only: true,
            extras: Default::default(),
        }
    }

    #[test]
    fn dialog_fields_become_store_urls() {
        let url = |f: &ConnectionForm| form_url(f).unwrap().unwrap().1;
        assert_eq!(
            url(&form("mongodb", SslMode::Disable)),
            "mongodb://app%20user@db.local:27018/shop?authSource=admin"
        );
        assert_eq!(
            url(&form("documentdb", SslMode::Require)),
            "mongodb://app%20user@db.local:27018/shop?tls=true&tlsAllowInvalidCertificates=true&authSource=admin"
        );
        let mut r = form("redis", SslMode::VerifyFull);
        r.database = "2".into();
        r.port.clear();
        r.user.clear();
        assert_eq!(url(&r), "rediss://db.local/2");
        r.ssl = SslMode::Require;
        assert_eq!(url(&r), "rediss://db.local/2?insecure=true");
        assert!(
            form_url(&form("tidb", SslMode::Prefer)).is_none(),
            "SQL systems stay SQL"
        );
        assert!(
            form_url(&form("cassandra", SslMode::Prefer)).is_none(),
            "planned systems"
        );
        let mut v = form("valkey", SslMode::Disable);
        v.database = "1".into();
        let p = form_profile(&v, "x").unwrap().unwrap();
        assert_eq!(
            (p.system, p.read_only, p.id.as_str()),
            (System::Redis, true, "x")
        );
    }

    /// SPEC-0310 v4 AK-10: `prefer` off the loopback is the TLS URL of `require`; plain text only
    /// for a store on the loopback.
    #[test]
    fn prefer_needs_tls_off_the_loopback() {
        let url = |f: &ConnectionForm| form_url(f).unwrap().unwrap().1;
        let at = |system: &str, host: &str| {
            let mut f = form(system, SslMode::Prefer);
            f.host = host.into();
            f.user.clear();
            f.port.clear();
            url(&f)
        };
        assert_eq!(
            at("mongodb", "db.local"),
            "mongodb://db.local/shop?tls=true&tlsAllowInvalidCertificates=true"
        );
        assert_eq!(
            at("redis", "db.local"),
            "rediss://db.local/shop?insecure=true"
        );
        assert_eq!(at("mongodb", "localhost"), "mongodb://localhost/shop");
        assert_eq!(at("redis", "127.0.0.1"), "redis://127.0.0.1/shop");
    }

    #[test]
    fn saving_keeps_the_password_in_the_keychain_and_removing_deletes_it() {
        let dir = std::env::temp_dir().join(format!("idecraft-store-form-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let secrets = MemoryStore::default();
        let p = StoreProfile::from_url("Cache", "redis://default@localhost/0").unwrap();
        save_store(&dir, p.clone(), None, Some("s3cret"), &secrets).unwrap();
        let text = std::fs::read_to_string(dir.join(crate::STORE_FILE)).unwrap();
        assert!(!text.contains("s3cret"), "{text}");
        let saved = load_stores(&dir).unwrap();
        assert_eq!(
            saved[0].password(&secrets).unwrap().unwrap().expose(),
            "s3cret"
        );
        // A second store whose name gives the same id gets its own id.
        let mut q = p;
        q.name = "cache".into();
        save_store(&dir, q, None, None, &secrets).unwrap();
        assert_eq!(load_stores(&dir).unwrap()[1].id, "cache-2");
        assert_eq!(remove_store(&dir, "cache", &secrets).unwrap(), "Cache");
        assert!(secrets.get(&secret_ref("cache")).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redis_from_the_dialog_list_is_saved_as_a_store() {
        // The form the connection dialog builds when Redis is picked from the system list.
        let redis = idecraft_database::catalogue().get("redis").unwrap().clone();
        let mut f = ConnectionForm::for_connector(&redis);
        f.name = "cache".into();
        f.host = "127.0.0.1".into();
        f.port = "1".into();
        f.database = "2".into();
        let p = form_profile(&f, "new").expect("Redis is a store").unwrap();
        let dir =
            std::env::temp_dir().join(format!("idecraft-store-dialog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let name = save_store(&dir, p, None, None, &MemoryStore::default()).unwrap();
        assert_eq!(name, "cache");
        let text = std::fs::read_to_string(dir.join(crate::STORE_FILE)).unwrap();
        assert!(text.contains("[idecraft]"), "ADR-0021 envelope: {text}");
        assert!(text.contains("kind = \"stores\""), "{text}");
        // New connections use TLS with certificate checks by default (SPEC-0016 v2, IMP-0370).
        assert!(text.contains("url = \"rediss://127.0.0.1:1/2\""), "{text}");
        assert!(
            !dir.join(".idecraft/connections.toml").exists(),
            "not an SQL connection"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! A connection from a data source of the reference IDE's `.idea/dataSources.xml` (`SPEC-0008` v9
//! behaviour 19): the form the window's dialog and MCP `add_idea_data_source` start from. Nothing
//! here connects or writes; saving goes through the usual connection paths.

use std::path::{Path, PathBuf};

use ferris_idea_core::IdeaDataSource;

use crate::{ConnectionForm, ConnectionProfile, Dialect};

/// The dialect of a data source's kind (`SPEC-0049` v7 behaviour 18).
fn dialect_of(kind: &str) -> Option<Dialect> {
    Some(match kind {
        "sqlite" => Dialect::Sqlite,
        "postgresql" => Dialect::PostgreSql,
        "mysql" => Dialect::MySql,
        "mariadb" => Dialect::MariaDb,
        "sqlserver" => Dialect::SqlServer,
        _ => return None,
    })
}

/// The connection form for an offered data source: its name, dialect and target, no user and no
/// password, and the SSL mode of every new connection whatever the URL says (IMP-0530). A data
/// source that is not offered gives its reason.
pub fn idea_source_form(source: &IdeaDataSource) -> Result<ConnectionForm, String> {
    if let Some(reason) = &source.reason {
        return Err(format!("data source `{}`: {reason}", source.name));
    }
    let dialect = source
        .kind
        .as_deref()
        .and_then(dialect_of)
        .ok_or_else(|| format!("data source `{}` has no supported kind", source.name))?;
    // `new` gives the secure default: `verify-full` for servers (IMP-0370).
    let mut form = ConnectionForm::new(dialect);
    form.name = source.name.clone();
    if dialect.file_based() {
        form.path = source.file.clone().unwrap_or_default();
    } else {
        form.host = source.host.clone().unwrap_or_default();
        form.port = source.port.map(|p| p.to_string()).unwrap_or_default();
        form.database = source.database.clone().unwrap_or_default();
    }
    Ok(form)
}

/// The note over a connection filled from a data source, in the window's dialog and as MCP
/// `add_idea_data_source`'s `note` (`SPEC-0049` v10 behaviour 19): it asks for the user only where
/// the database system logs in; a file such as SQLite has no user.
pub fn idea_source_note(form: &ConnectionForm) -> &'static str {
    if form.dialect.file_based() {
        "From the project's .idea: check the fields, then Save"
    } else {
        "From the project's .idea: check the fields, add the user, then Save"
    }
}

/// Lexical normalisation, so `a/../b.db` and `b.db` are the same file.
fn normal(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Whether a profile of the workspace at `root` already has the data source: the same name, or the
/// same dialect and the same file or the same host, port and database (`SPEC-0008` v9 behaviour 19).
pub fn is_added(form: &ConnectionForm, root: &Path, profiles: &[ConnectionProfile]) -> bool {
    let Ok(wanted) = form.to_profile("idea-source", None, None) else {
        return profiles.iter().any(|p| p.name == form.name);
    };
    profiles.iter().any(|p| {
        if p.name == form.name {
            return true;
        }
        if p.dialect != wanted.dialect {
            return false;
        }
        if wanted.dialect.file_based() {
            let file = |p: &ConnectionProfile| p.path.as_deref().map(|f| normal(&root.join(f)));
            file(p).is_some() && file(p) == file(&wanted)
        } else {
            p.host
                .as_deref()
                .zip(wanted.host.as_deref())
                .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
                && p.effective_port() == wanted.effective_port()
                && p.database == wanted.database
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SslMode;

    fn source(name: &str, kind: &str) -> IdeaDataSource {
        IdeaDataSource {
            name: name.into(),
            kind: Some(kind.into()),
            ..IdeaDataSource::default()
        }
    }

    /// SPEC-0049 AK-19 (v10, behaviour 19, W1 of OBS-0112): the note asks for the user of a
    /// server's data source, not of a SQLite file's.
    #[test]
    fn the_note_asks_for_a_user_only_where_one_logs_in() {
        let mut sqlite = source("inventory", "sqlite");
        sqlite.file = Some("inventory.sqlite".into());
        let note = idea_source_note(&idea_source_form(&sqlite).unwrap());
        assert!(!note.contains("user"), "{note}");
        assert!(note.contains("check the fields"), "{note}");
        for kind in ["postgresql", "mysql", "mariadb", "sqlserver"] {
            let mut server = source("shop", kind);
            server.host = Some("db".into());
            let note = idea_source_note(&idea_source_form(&server).unwrap());
            assert!(note.contains("add the user"), "{kind}: {note}");
        }
    }

    /// SPEC-0008 AK-21 (v9, behaviour 19).
    #[test]
    fn data_sources_become_forms_without_user_password_or_weak_ssl() {
        let mut sqlite = source("inventory", "sqlite");
        sqlite.file = Some("data/inventory.sqlite".into());
        let form = idea_source_form(&sqlite).unwrap();
        assert_eq!(form.dialect, Dialect::Sqlite);
        assert_eq!(form.name, "inventory");
        assert_eq!(form.path, "data/inventory.sqlite");
        assert!(form.user.is_empty());

        let mut pg = source("shop", "postgresql");
        pg.url = Some("jdbc:postgresql://db:5433/shop?sslmode=disable".into());
        pg.host = Some("db".into());
        pg.port = Some(5433);
        pg.database = Some("shop".into());
        let form = idea_source_form(&pg).unwrap();
        assert_eq!(form.dialect, Dialect::PostgreSql);
        assert_eq!(
            (
                form.host.as_str(),
                form.port.as_str(),
                form.database.as_str()
            ),
            ("db", "5433", "shop")
        );
        assert_eq!(
            form.ssl,
            SslMode::VerifyFull,
            "the URL's mode is not taken over"
        );
        assert!(form.user.is_empty());
        let profile = form.to_profile("x", None, None).unwrap();
        assert!(profile.secret.is_none() && profile.user.is_none());

        for (kind, dialect) in [
            ("mysql", Dialect::MySql),
            ("mariadb", Dialect::MariaDb),
            ("sqlserver", Dialect::SqlServer),
        ] {
            let mut s = source("s", kind);
            s.host = Some("h".into());
            let form = idea_source_form(&s).unwrap();
            assert_eq!(form.dialect, dialect);
            assert_eq!(form.ssl, SslMode::VerifyFull);
        }

        let mut refused = source("ora", "oracle");
        refused.reason = Some("driver `oracle.19` is not supported".into());
        let e = idea_source_form(&refused).unwrap_err();
        assert!(e.contains("not supported"), "{e}");
    }

    #[test]
    fn a_profile_with_the_name_or_the_target_has_the_data_source() {
        let root = Path::new("/work/shop");
        let mut sqlite = source("inventory", "sqlite");
        sqlite.file = Some("inventory.sqlite".into());
        let form = idea_source_form(&sqlite).unwrap();
        assert!(!is_added(&form, root, &[]));
        let by_name = ConnectionForm {
            path: "other.sqlite".into(),
            ..form.clone()
        }
        .to_profile("a", None, None)
        .unwrap();
        assert!(is_added(&form, root, &[by_name]));
        let by_file = ConnectionForm {
            name: "inv".into(),
            path: "/work/shop/./inventory.sqlite".into(),
            ..form.clone()
        }
        .to_profile("b", None, None)
        .unwrap();
        assert!(is_added(&form, root, &[by_file]));
        let elsewhere = ConnectionForm {
            name: "inv".into(),
            path: "x.sqlite".into(),
            ..form.clone()
        }
        .to_profile("c", None, None)
        .unwrap();
        assert!(!is_added(&form, root, &[elsewhere]));

        let mut pg = source("shop", "postgresql");
        pg.host = Some("DB".into());
        pg.database = Some("shop".into());
        let form = idea_source_form(&pg).unwrap();
        let same = ConnectionForm {
            name: "prod".into(),
            host: "db".into(),
            port: "5432".into(),
            ..form.clone()
        }
        .to_profile("d", None, None)
        .unwrap();
        assert!(is_added(&form, root, &[same]));
        let other_db = ConnectionForm {
            name: "prod".into(),
            database: "other".into(),
            ..form.clone()
        }
        .to_profile("e", None, None)
        .unwrap();
        assert!(!is_added(&form, root, &[other_db]));
    }
}

//! Which data source a `.sql` file belongs to, and where new consoles go (`SPEC-0192` behaviours
//! 1–2).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::ConnectionProfile;

/// Bindings of `.sql` files to connections, relative to the workspace.
pub const SQL_FILES: &str = ".ferris-idea/sql-files.toml";
/// Folder of the consoles, one subfolder per connection id.
pub const CONSOLES_DIR: &str = ".ferris-idea/consoles";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Bindings {
    #[serde(default)]
    files: BTreeMap<String, String>,
}

fn relative(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn read(root: &Path) -> Bindings {
    // With or without the envelope (`SPEC-0303` behaviour 16); another kind or a newer format
    // reads as none and is not overwritten.
    std::fs::read_to_string(root.join(SQL_FILES))
        .ok()
        .filter(|t| ferris_idea_core::format::project_format(t, "sql-files").is_ok())
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

/// The id of the connection `file` is bound to: its entry in `sql-files.toml`, else the console
/// folder it lies in; only ids of `profiles` count.
pub fn bound_connection(
    root: &Path,
    file: &Path,
    profiles: &[ConnectionProfile],
) -> Option<String> {
    let rel = relative(root, file)?;
    let exists = |id: &str| profiles.iter().any(|p| p.id == id);
    if let Some(id) = read(root).files.get(&rel).filter(|id| exists(id)) {
        return Some(id.clone());
    }
    let id = rel
        .strip_prefix(CONSOLES_DIR)?
        .strip_prefix('/')?
        .split_once('/')?
        .0;
    exists(id).then(|| id.to_owned())
}

/// Binds `file` to connection `id`, or removes its binding with `None`.
pub fn bind_file(root: &Path, file: &Path, id: Option<&str>) -> io::Result<()> {
    let rel = relative(root, file).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is outside the workspace", file.display()),
        )
    })?;
    let mut bindings = read(root);
    match id {
        Some(id) => {
            bindings.files.insert(rel, id.to_owned());
        }
        None => {
            bindings.files.remove(&rel);
        }
    }
    let path = root.join(SQL_FILES);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    ferris_idea_core::format::prepare_write(&path, "sql-files")?;
    let text = ferris_idea_core::format::write(
        &ferris_idea_core::format::Envelope::current("sql-files"),
        &bindings,
    )
    .map_err(io::Error::other)?;
    std::fs::write(path, text)
}

/// The next free console file of connection `id`: `console.sql`, then `console_2.sql`, …
pub fn next_console(root: &Path, id: &str) -> PathBuf {
    let dir = root.join(CONSOLES_DIR).join(id);
    (1..)
        .map(|n| {
            dir.join(if n == 1 {
                "console.sql".to_owned()
            } else {
                format!("console_{n}.sql")
            })
        })
        .find(|p| !p.exists())
        .expect("a free name")
}

/// Creates the next console of `id` (empty) and returns its path.
pub fn create_console(root: &Path, id: &str) -> io::Result<PathBuf> {
    let path = next_console(root, id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> ConnectionProfile {
        ConnectionProfile::from_url(id, "sqlite:/tmp/x.db").unwrap()
    }

    /// SPEC-0303 AK-5: an old file is read, saved with the envelope, kept once as `.bak`; a newer
    /// one is not overwritten.
    #[test]
    fn old_binding_files_gain_the_envelope() {
        let root = std::env::temp_dir().join(format!("ferris-idea-sqlmig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
        let profiles = [profile("shop"), profile("audit")];
        std::fs::write(root.join(SQL_FILES), "[files]\n\"a.sql\" = \"shop\"\n").unwrap();
        let a = root.join("a.sql");
        assert_eq!(
            bound_connection(&root, &a, &profiles).as_deref(),
            Some("shop")
        );
        bind_file(&root, &root.join("b.sql"), Some("audit")).unwrap();
        let text = std::fs::read_to_string(root.join(SQL_FILES)).unwrap();
        assert!(
            text.starts_with("[ferris-idea]\nkind = \"sql-files\"\nformat = 1\n"),
            "{text}"
        );
        assert!(root.join(".ferris-idea/sql-files.toml.bak").exists());
        assert_eq!(
            bound_connection(&root, &a, &profiles).as_deref(),
            Some("shop")
        );
        std::fs::write(
            root.join(SQL_FILES),
            text.replace("format = 1", "format = 7"),
        )
        .unwrap();
        assert_eq!(bound_connection(&root, &a, &profiles), None);
        let e = bind_file(&root, &a, None).unwrap_err().to_string();
        assert!(e.contains("newer Ferris IDEA (format 7)"), "{e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_binding_and_file_winning() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-sqlfiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let profiles = [profile("shop"), profile("audit")];
        let console = create_console(&root, "shop").unwrap();
        assert!(console.ends_with(".ferris-idea/consoles/shop/console.sql"));
        assert!(next_console(&root, "shop").ends_with("console_2.sql"));
        assert_eq!(
            bound_connection(&root, &console, &profiles).as_deref(),
            Some("shop")
        );
        let script = root.join("db/report.sql");
        assert_eq!(bound_connection(&root, &script, &profiles), None);
        bind_file(&root, &script, Some("audit")).unwrap();
        bind_file(&root, &console, Some("audit")).unwrap();
        assert_eq!(
            bound_connection(&root, &script, &profiles).as_deref(),
            Some("audit")
        );
        assert_eq!(
            bound_connection(&root, &console, &profiles).as_deref(),
            Some("audit")
        );
        // A binding to a connection that no longer exists is ignored.
        assert_eq!(bound_connection(&root, &script, &profiles[..1]), None);
        bind_file(&root, &console, None).unwrap();
        assert_eq!(
            bound_connection(&root, &console, &profiles).as_deref(),
            Some("shop")
        );
        assert!(bind_file(&root, Path::new("/elsewhere/x.sql"), Some("shop")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}

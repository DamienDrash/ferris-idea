//! Which icon a Java or Kotlin file shows on its tab and in the project tree (`SPEC-0131` v3
//! behaviour 7): the kind of its primary declaration, remembered per text so a frame does not
//! scan or read a file again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use idecraft_languages::declaration_kind::{primary_declaration, primary_declaration_of};

/// How long a file on disk keeps its icon before its modification time is looked at again.
const RECHECK: Duration = Duration::from_secs(1);

/// What is known about a file on disk.
#[derive(Debug, Clone, Copy)]
struct OnDisk {
    checked: Instant,
    modified: Option<SystemTime>,
    len: u64,
    icon: &'static str,
}

/// The icons of the files seen so far.
#[derive(Debug, Default)]
pub struct DeclIcons {
    /// Open files: the editor's revision and the icon of that text.
    open: HashMap<PathBuf, (u64, &'static str)>,
    /// Files on disk.
    disk: HashMap<PathBuf, OnDisk>,
}

/// Whether `path` is a file whose icon depends on its content.
pub fn has_kinds(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("java") || e.eq_ignore_ascii_case("kt"))
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The icon of `path` whose content is `text`: its declaration's kind, else the file type's.
pub fn icon_of_text(path: &Path, text: &str) -> &'static str {
    primary_declaration(path, text).map_or_else(
        || idecraft_ui::icons::file_icon(&name_of(path)),
        |kind| kind.icon(),
    )
}

impl DeclIcons {
    /// The icon of the open file `path` at `revision`; `text` is asked for only when the revision
    /// is new.
    pub fn of_open(
        &mut self,
        path: &Path,
        revision: u64,
        text: impl FnOnce() -> String,
    ) -> &'static str {
        if !has_kinds(path) {
            return idecraft_ui::icons::file_icon(&name_of(path));
        }
        if let Some((seen, icon)) = self.open.get(path)
            && *seen == revision
        {
            return icon;
        }
        let icon = icon_of_text(path, &text());
        self.open.insert(path.to_path_buf(), (revision, icon));
        icon
    }

    /// The icon of the file `path` on disk at the time `now`; the file is read again only when
    /// its modification time or length changed, looked at once a second at most.
    pub fn of_file(&mut self, path: &Path, now: Instant) -> &'static str {
        let plain = idecraft_ui::icons::file_icon(&name_of(path));
        if !has_kinds(path) {
            return plain;
        }
        if let Some(known) = self.disk.get(path)
            && now.saturating_duration_since(known.checked) < RECHECK
        {
            return known.icon;
        }
        let meta = std::fs::metadata(path).ok();
        let (modified, len) = (
            meta.as_ref().and_then(|m| m.modified().ok()),
            meta.as_ref().map_or(0, |m| m.len()),
        );
        let icon = match self.disk.get(path) {
            Some(known) if known.modified == modified && known.len == len => known.icon,
            _ => primary_declaration_of(path).map_or(plain, |kind| kind.icon()),
        };
        // The tree of a large project shows many files over time; start over rather than grow.
        if self.disk.len() >= 4096 {
            self.disk.clear();
        }
        self.disk.insert(
            path.to_path_buf(),
            OnDisk {
                checked: now,
                modified,
                len,
                icon,
            },
        );
        icon
    }

    /// Forgets the open text of `path` (its tab was closed).
    pub fn closed(&mut self, path: &Path) {
        self.open.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0131 AK-6 (v3): the icon follows the text of an open file by revision and a file on
    /// disk by its modification; other files keep their type's icon.
    #[test]
    fn icons_follow_the_text_and_the_file() {
        let mut icons = DeclIcons::default();
        let pet = Path::new("/p/src/Pet.java");
        assert_eq!(
            icons.of_open(pet, 1, || "public interface Pet {}".into()),
            "nodes/interface"
        );
        // The same revision is not scanned again.
        assert_eq!(
            icons.of_open(pet, 1, || panic!("asked for the text again")),
            "nodes/interface"
        );
        assert_eq!(
            icons.of_open(pet, 2, || "public record Pet(String name) {}".into()),
            "idecraft/record"
        );
        assert_eq!(
            icons.of_open(pet, 3, || "// nothing yet".into()),
            "fileTypes/java"
        );
        assert_eq!(
            icons.of_open(Path::new("/p/Main.kt"), 1, || "fun main() {}".into()),
            "fileTypes/kotlin"
        );
        assert_eq!(
            icons.of_open(Path::new("/p/Registry.kt"), 1, || "object Registry".into()),
            "idecraft/object"
        );
        assert_eq!(
            icons.of_open(Path::new("/p/a.rs"), 1, || panic!("not a JVM file")),
            "idecraft/rust"
        );
        icons.closed(pet);
        assert_eq!(
            icons.of_open(pet, 3, || "enum Pet { CAT }".into()),
            "nodes/enum"
        );

        let dir = std::env::temp_dir().join(format!("idecraft-declicon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Shape.java");
        std::fs::write(&file, "public class Shape {}\n").unwrap();
        let start = Instant::now();
        assert_eq!(icons.of_file(&file, start), "nodes/class");
        std::fs::write(&file, "public sealed interface Shape permits Circle {}\n").unwrap();
        // Within the second the remembered icon stays; after it the changed file is read.
        assert_eq!(
            icons.of_file(&file, start + Duration::from_millis(200)),
            "nodes/class"
        );
        assert_eq!(
            icons.of_file(&file, start + Duration::from_secs(2)),
            "nodes/interface"
        );
        assert_eq!(
            icons.of_file(&dir.join("Missing.java"), start),
            "fileTypes/java"
        );
        assert_eq!(
            icons.of_file(&dir.join("notes.md"), start),
            "fileTypes/markdown"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Which icon a file shows by the language its project decided (`SPEC-0004` v6 behaviour 15): an
//! override, a file name pattern or the hashbang line give the language's icon, a file decided by
//! its name keeps the icon of its name; an overridden file carries a mark on its icon.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_languages::LanguageId;
use ferris_idea_languages::file_types::{Choice, Detected, Source};
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, px};

/// The icon of plain text and of a language without an icon of its own.
pub const TEXT: &str = "fileTypes/text";
/// The icon of a file that Ferris IDEA does not open (a binary file), and of a name without one.
pub const UNKNOWN: &str = "fileTypes/unknown";
/// The mark of an overridden file.
pub const OVERRIDE_MARK: &str = "ferris-idea/overrideMark";

/// How long a file keeps its decision before it is asked again.
const RECHECK: Duration = Duration::from_secs(1);

/// The icon of `language`; the text icon for one without its own, never the unknown icon.
pub fn language_icon(language: LanguageId) -> &'static str {
    use LanguageId as L;
    match language {
        L::Python => "fileTypes/python",
        L::Sql => "fileTypes/sql",
        L::JavaScript => "fileTypes/javaScript",
        L::TypeScript => "fileTypes/typeScript",
        L::Java => "fileTypes/java",
        L::Kotlin => "fileTypes/kotlin",
        L::Php => "ferris-idea/php",
        L::Html => "fileTypes/html",
        L::Css | L::Scss | L::Less => "fileTypes/css",
        L::Json => "fileTypes/json",
        L::Go => "ferris-idea/go",
        L::Rust => "ferris-idea/rust",
        L::Ruby => "ferris-idea/ruby",
        L::C => "fileTypes/c",
        L::Cpp => "fileTypes/cpp",
        L::Xml => "fileTypes/xml",
        L::Yaml => "fileTypes/yaml",
        L::Markdown => "fileTypes/markdown",
        L::Toml => "fileTypes/toml",
        L::Shell => "fileTypes/shell",
        L::Dockerfile => "fileTypes/docker",
        L::Groovy => "fileTypes/groovy",
        L::Swift => "fileTypes/swiftLang",
        L::Vue => "fileTypes/vue",
        L::Svelte => "ferris-idea/svelte",
        L::Properties => "fileTypes/properties",
        L::Scala | L::Lua | L::R => TEXT,
    }
}

/// The icon of a row of the list of file types.
pub fn choice_icon(choice: Choice) -> &'static str {
    choice.language().map_or(TEXT, language_icon)
}

/// The icon of a file whose language was decided as `detected`; `None` when its name decides
/// the icon.
pub fn decided_icon(detected: &Detected) -> Option<&'static str> {
    match detected.source {
        Source::Override
        | Source::Pattern
        | Source::Hashbang
        | Source::Modeline
        | Source::Prolog => Some(detected.language.map_or(TEXT, language_icon)),
        Source::Name | Source::None => None,
    }
}

/// What the icons need to know of a file's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTypeIcon {
    /// The icon the decision gives, `None` when the name decides.
    pub icon: Option<&'static str>,
    /// The file's language.
    pub language: Option<LanguageId>,
    /// The file has an override.
    pub overridden: bool,
    /// The file opens as text: looked at only when its name gives no icon (`SPEC-0004` v12
    /// behaviour 15), false otherwise.
    pub opens_as_text: bool,
}

impl FileTypeIcon {
    fn of(detected: &Detected) -> Self {
        Self {
            icon: decided_icon(detected),
            language: detected.language,
            overridden: detected.source == Source::Override,
            opens_as_text: false,
        }
    }
}

/// How many bytes of a file decide whether it opens as text.
const SNIFF_BYTES: u64 = 8192;

/// Whether the regular file `path` opens in an editor as text (`SPEC-0004` v12 behaviour 15,
/// `SPEC-0336` behaviour 3): not larger than an editor takes, and its first bytes are text.
pub fn opens_as_text(path: &Path) -> bool {
    use std::io::Read;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() > ferris_idea_core::file_properties::MAX_BYTES {
        return false;
    }
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(SNIFF_BYTES).read_to_end(&mut head));
    if read.is_err() {
        return false;
    }
    !matches!(
        ferris_idea_core::file_properties::detect(&head, None),
        Err(ferris_idea_core::file_properties::Refusal::Binary)
    )
}

/// The icon of the file `path` by its name, where a name without an icon of its own gives the
/// text icon when the file opens as text (`opens_as_text`) and the unknown icon only otherwise.
pub fn name_icon(path: &Path, opens_as_text: bool) -> &'static str {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match ferris_idea_ui::icons::file_icon(&name) {
        UNKNOWN if opens_as_text => TEXT,
        icon => icon,
    }
}

/// The decisions of the files seen so far, each kept for a second at most.
#[derive(Debug, Default)]
pub struct FileTypeIcons {
    known: HashMap<PathBuf, (Instant, FileTypeIcon)>,
}

impl FileTypeIcons {
    /// What the icons of `path` show at the time `now`.
    pub fn of(&mut self, path: &Path, now: Instant) -> FileTypeIcon {
        if let Some((checked, known)) = self.known.get(path)
            && now.saturating_duration_since(*checked) < RECHECK
        {
            return *known;
        }
        let mut known = FileTypeIcon::of(&ferris_idea_languages::file_types::detect(path));
        // Only a file whose name gives no icon is read (SPEC-0004 v12 behaviour 15).
        if known.icon.is_none() && name_icon(path, false) == UNKNOWN {
            known.opens_as_text = opens_as_text(path);
        }
        if self.known.len() >= 4096 {
            self.known.clear();
        }
        self.known.insert(path.to_path_buf(), (now, known));
        known
    }

    /// Forgets every decision (an action changed the project's file types).
    pub fn forget(&mut self) {
        self.known.clear();
    }
}

/// `icon` with the override mark on its lower right when `overridden`.
pub fn with_override_mark(icon: AnyElement, overridden: bool) -> AnyElement {
    if !overridden {
        return icon;
    }
    div()
        .relative()
        .flex_none()
        .size(px(crate::icon_images::SIZE))
        .child(icon)
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .child(crate::icon_images::icon(OVERRIDE_MARK)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0004 v6 AK-9: every language has a bundled icon, never the unknown one.
    #[test]
    fn every_language_has_an_icon() {
        for l in LanguageId::ALL {
            let key = language_icon(l);
            assert_ne!(key, "fileTypes/unknown", "{l:?}");
            assert!(
                ferris_idea_ui::icons::svg(key, false).is_some(),
                "{l:?}: {key}"
            );
            assert!(
                ferris_idea_ui::icons::svg(key, true).is_some(),
                "{l:?}: {key}"
            );
        }
        assert_eq!(choice_icon(Choice::PlainText), TEXT);
        assert_eq!(
            choice_icon(Choice::Language(LanguageId::Html)),
            "fileTypes/html"
        );
        assert!(ferris_idea_ui::icons::svg(OVERRIDE_MARK, true).is_some());
        assert!(ferris_idea_ui::icons::svg(OVERRIDE_MARK, false).is_some());
    }

    /// SPEC-0004 v6 AK-9: override, pattern and hashbang give the language's icon (plain text
    /// the text icon); a file decided by its name keeps its name's icon.
    #[test]
    fn the_icon_follows_what_decided_the_language() {
        let d = |language, source| Detected {
            language,
            source,
            pattern: None,
        };
        let html = Some(LanguageId::Html);
        assert_eq!(
            decided_icon(&d(html, Source::Override)),
            Some("fileTypes/html")
        );
        assert_eq!(
            decided_icon(&d(html, Source::Pattern)),
            Some("fileTypes/html")
        );
        assert_eq!(
            decided_icon(&d(Some(LanguageId::Python), Source::Hashbang)),
            Some("fileTypes/python")
        );
        assert_eq!(decided_icon(&d(None, Source::Override)), Some(TEXT));
        assert_eq!(decided_icon(&d(html, Source::Name)), None);
        assert_eq!(decided_icon(&d(None, Source::None)), None);
        assert!(FileTypeIcon::of(&d(html, Source::Override)).overridden);
        assert!(!FileTypeIcon::of(&d(html, Source::Pattern)).overridden);
    }

    /// SPEC-0004 v12 AK-19 (OBS-0090 L1): a file that opens as plain text shows the text icon,
    /// whatever its name; only a file that does not open (binary) shows the unknown icon.
    #[test]
    fn plain_text_files_show_the_text_icon() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-fti-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files: [(&str, &[u8]); 7] = [
            ("data", b"{\"a\": 1}\n"),
            ("Makefile", b"all:\n\techo hi\n"),
            ("notes.data", b"plain notes\n"),
            ("tools.cmake", b"set(X 1)\n"),
            ("empty", b""),
            ("blob", &[0x7f, b'E', b'L', b'F', 0, 1, 2, 3]),
            ("notes.txt", b"text\n"),
        ];
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let mut icons = FileTypeIcons::default();
        let now = Instant::now();
        let shown = |icons: &mut FileTypeIcons, name: &str| {
            let path = dir.join(name);
            let t = icons.of(&path, now);
            t.icon.unwrap_or_else(|| name_icon(&path, t.opens_as_text))
        };
        for name in [
            "data",
            "Makefile",
            "notes.data",
            "tools.cmake",
            "empty",
            "notes.txt",
        ] {
            assert_eq!(shown(&mut icons, name), TEXT, "{name}");
        }
        assert_eq!(shown(&mut icons, "blob"), UNKNOWN);
        assert_eq!(shown(&mut icons, "missing"), UNKNOWN);
        assert!(!opens_as_text(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

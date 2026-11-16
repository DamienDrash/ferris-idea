//! Which icon a file shows by the language its project decided (`SPEC-0004` v6 behaviour 15): an
//! override, a file name pattern or the hashbang line give the language's icon, a file decided by
//! its name keeps the icon of its name; an overridden file carries a mark on its icon.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, px};
use idecraft_languages::LanguageId;
use idecraft_languages::file_types::{Choice, Detected, Source};

/// The icon of plain text and of a language without an icon of its own.
pub const TEXT: &str = "fileTypes/text";
/// The mark of an overridden file.
pub const OVERRIDE_MARK: &str = "idecraft/overrideMark";

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
        L::Php => "idecraft/php",
        L::Html => "fileTypes/html",
        L::Css | L::Scss | L::Less => "fileTypes/css",
        L::Json => "fileTypes/json",
        L::Go => "idecraft/go",
        L::Rust => "idecraft/rust",
        L::Ruby => "idecraft/ruby",
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
        L::Svelte => "idecraft/svelte",
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
        Source::Override | Source::Pattern | Source::Hashbang => {
            Some(detected.language.map_or(TEXT, language_icon))
        }
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
}

impl FileTypeIcon {
    fn of(detected: &Detected) -> Self {
        Self {
            icon: decided_icon(detected),
            language: detected.language,
            overridden: detected.source == Source::Override,
        }
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
        let known = FileTypeIcon::of(&idecraft_languages::file_types::detect(path));
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
                idecraft_ui::icons::svg(key, false).is_some(),
                "{l:?}: {key}"
            );
            assert!(idecraft_ui::icons::svg(key, true).is_some(), "{l:?}: {key}");
        }
        assert_eq!(choice_icon(Choice::PlainText), TEXT);
        assert_eq!(
            choice_icon(Choice::Language(LanguageId::Html)),
            "fileTypes/html"
        );
        assert!(idecraft_ui::icons::svg(OVERRIDE_MARK, true).is_some());
        assert!(idecraft_ui::icons::svg(OVERRIDE_MARK, false).is_some());
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
}

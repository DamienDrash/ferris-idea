//! The project's own code scheme between `.editorconfig` and Code Style (`SPEC-0150` v6
//! behaviour 8): which level gives a file its indentation and right margin.
//!
//! The scheme is what `SPEC-0049` v4 behaviour 17 reads from `.idea/codeStyles`; it applies only
//! when `codeStyleConfig.xml` switches the project's own scheme on.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::editorconfig::EditorConfig;
use crate::{CodeStyleSettings, IDEA_DIR, IdeaCodeStyle, IdeaLanguageStyle, IndentStyle};

/// The language id a `codeStyleSettings` element names for the Code Style row `language` (one of
/// [`crate::CODE_STYLE_LANGUAGES`]); none for "Default" and unknown rows (behaviour 8).
pub fn idea_language_id(language: &str) -> Option<&'static str> {
    Some(match language {
        "Java" => "JAVA",
        "Kotlin" => "kotlin",
        "Python" => "Python",
        "JavaScript" => "JavaScript",
        "TypeScript" => "TypeScript",
        "JSON" => "JSON",
        "HTML" => "HTML",
        "CSS" => "CSS",
        "SCSS" => "SCSS",
        "Less" => "LESS",
        "YAML" => "yaml",
        "Markdown" => "Markdown",
        "SQL" => "SQL",
        "Go" => "go",
        "Ruby" => "ruby",
        "Rust" => "Rust",
        "C/C++" => "ObjectiveC",
        _ => return None,
    })
}

/// The level that gave a value (behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StyleSource {
    /// The file's `.editorconfig`.
    Editorconfig,
    /// The project's own code scheme in `.idea/codeStyles`.
    Project,
    /// Ferris IDEA's Code Style settings (the user's, else the built-in table).
    Ide,
    /// An argument of the call (`format_file`'s `tab_size`/`insert_spaces`).
    Argument,
}

impl IdeaCodeStyle {
    /// Whether the scheme applies: switched on in `codeStyleConfig.xml` and read from `Project.xml`.
    pub fn applies(&self) -> bool {
        self.per_project && self.preferred_scheme.is_none() && self.file.is_some()
    }

    /// The first `codeStyleSettings` element for the Code Style row `language`, the id compared
    /// without regard to case.
    pub fn language(&self, language: &str) -> Option<&IdeaLanguageStyle> {
        let id = idea_language_id(language)?;
        self.languages
            .iter()
            .find(|l| l.language.eq_ignore_ascii_case(id))
    }
}

/// The nearest folder at or above `file`'s folder that holds a `.idea` folder; the search stops
/// after `stop` when one is given (a workspace root).
pub fn project_dir(file: &Path, stop: Option<&Path>) -> Option<PathBuf> {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d.join(IDEA_DIR).is_dir() {
            return Some(d.to_path_buf());
        }
        if stop.is_some_and(|s| s == d) {
            return None;
        }
        dir = d.parent();
    }
    None
}

/// The code scheme of `file`'s project as `SPEC-0049` reads it, whether or not it applies.
pub fn project_scheme(file: &Path, stop: Option<&Path>) -> Option<IdeaCodeStyle> {
    crate::ideaproject::idea_code_style(&project_dir(file, stop)?)
}

/// What the scheme files of `file`'s project hold now (a hash per file, `None` where there is none),
/// so an open editor knows when to read them again (behaviour 8, as `SPEC-0137` v2 behaviour 2b).
pub fn change_stamp(file: &Path) -> Vec<Option<u64>> {
    use std::hash::{Hash, Hasher};
    let Some(dir) = project_dir(file, None) else {
        return Vec::new();
    };
    ["codeStyleConfig.xml", "Project.xml"]
        .iter()
        .map(|name| {
            std::fs::read(dir.join(IDEA_DIR).join("codeStyles").join(name))
                .ok()
                .map(|bytes| {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut h);
                    h.finish()
                })
        })
        .collect()
}

/// What the project's scheme sets for one language, empty when the scheme does not apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectStyle {
    /// `INDENT_SIZE`.
    pub indent: Option<u32>,
    /// `TAB_SIZE`.
    pub tab: Option<u32>,
    /// `USE_TAB_CHARACTER`.
    pub use_tabs: Option<bool>,
    /// The language's `RIGHT_MARGIN`, else the scheme's.
    pub right_margin: Option<u32>,
}

impl ProjectStyle {
    /// What `scheme` sets for the Code Style row `language`.
    pub fn of(scheme: Option<&IdeaCodeStyle>, language: &str) -> Self {
        let Some(scheme) = scheme.filter(|s| s.applies()) else {
            return Self::default();
        };
        let own = scheme.language(language);
        Self {
            indent: own.and_then(|l| l.indent).filter(|n| *n > 0),
            tab: own.and_then(|l| l.tab_size).filter(|n| *n > 0),
            use_tabs: own.and_then(|l| l.use_tabs),
            right_margin: own
                .and_then(|l| l.right_margin)
                .or(scheme.right_margin)
                .filter(|n| *n > 0),
        }
    }

    /// What the project of `file` sets for the Code Style row `language`.
    pub fn for_file(file: &Path, stop: Option<&Path>, language: &str) -> Self {
        Self::of(project_scheme(file, stop).as_ref(), language)
    }

    /// Whether the scheme sets any part of the indentation.
    pub fn sets_indent(&self) -> bool {
        self.indent.is_some() || self.tab.is_some() || self.use_tabs.is_some()
    }

    /// `ide` with what the scheme sets put over it, value by value.
    pub fn over(&self, ide: IndentStyle) -> IndentStyle {
        IndentStyle {
            indent: self.indent.unwrap_or(ide.indent),
            tab: self.tab.unwrap_or(ide.tab),
            use_tabs: self.use_tabs.unwrap_or(ide.use_tabs),
        }
    }

    /// The scheme's indentation over `ide`, if it sets any.
    pub fn indent_over(&self, ide: IndentStyle) -> Option<IndentStyle> {
        self.sets_indent().then(|| self.over(ide))
    }
}

/// The indentation and right margin a file gets, with the level each came from (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedStyle {
    /// Columns of a tab when indenting with tabs, else spaces per level (LSP `tabSize`).
    pub tab_size: u32,
    /// Spaces instead of tabs (LSP `insertSpaces`).
    pub insert_spaces: bool,
    /// Where the indentation came from.
    pub indent_source: StyleSource,
    /// The right margin's column; none when `.editorconfig` turns it off.
    pub right_margin: Option<u32>,
    /// Where the right margin came from.
    pub right_margin_source: StyleSource,
}

/// The order of behaviour 8: `.editorconfig`, else the project's scheme, else Code Style, per
/// value, for a file of the Code Style row `language`.
pub fn resolve(
    config: &EditorConfig,
    project: &ProjectStyle,
    ide: &CodeStyleSettings,
    language: &str,
) -> ResolvedStyle {
    let ide_indent = ide.indent_for(language);
    let (base, base_source) = match project.indent_over(ide_indent) {
        Some(p) => (p, StyleSource::Project),
        None => (ide_indent, StyleSource::Ide),
    };
    let (unit, indent_source) = match config.indent_unit() {
        Some(u) => (u, StyleSource::Editorconfig),
        None => (base.unit(), base_source),
    };
    let tabs = unit == "\t";
    let tab_size = if tabs {
        config.tab_width().map_or(base.tab, |w| w as u32)
    } else {
        unit.chars().count().max(1) as u32
    };
    let (right_margin, right_margin_source) = match config.right_margin() {
        Some(column) => (column.map(|c| c as u32), StyleSource::Editorconfig),
        None => match project.right_margin {
            Some(m) => (Some(m), StyleSource::Project),
            None => (Some(ide.right_margin), StyleSource::Ide),
        },
    };
    ResolvedStyle {
        tab_size,
        insert_spaces: !tabs,
        indent_source,
        right_margin,
        right_margin_source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editorconfig::Setting;

    fn config(pairs: &[(&str, &str)]) -> EditorConfig {
        let mut c = EditorConfig::default();
        for (k, v) in pairs {
            c.properties.insert(
                (*k).into(),
                Setting {
                    value: (*v).into(),
                    file: PathBuf::from(".editorconfig"),
                    line: 1,
                },
            );
        }
        c
    }

    fn lang(id: &str, indent: Option<u32>, margin: Option<u32>) -> IdeaLanguageStyle {
        IdeaLanguageStyle {
            language: id.into(),
            right_margin: margin,
            indent,
            ..Default::default()
        }
    }

    fn scheme(languages: Vec<IdeaLanguageStyle>) -> IdeaCodeStyle {
        IdeaCodeStyle {
            per_project: true,
            applied: true,
            preferred_scheme: None,
            file: Some(".idea/codeStyles/Project.xml".into()),
            right_margin: Some(100),
            languages,
        }
    }

    #[test]
    fn editorconfig_over_the_scheme_over_code_style() {
        let ide = CodeStyleSettings::default();
        let s = scheme(vec![lang("JAVA", Some(2), None)]);
        let p = ProjectStyle::of(Some(&s), "Java");
        // The scheme over Code Style, its margin over "Hard wrap at".
        let r = resolve(&EditorConfig::default(), &p, &ide, "Java");
        assert_eq!((r.tab_size, r.insert_spaces), (2, true));
        assert_eq!(r.indent_source, StyleSource::Project);
        assert_eq!(
            (r.right_margin, r.right_margin_source),
            (Some(100), StyleSource::Project)
        );
        // `.editorconfig` over both.
        let c = config(&[("indent_size", "3"), ("max_line_length", "80")]);
        let r = resolve(&c, &p, &ide, "Java");
        assert_eq!(
            (r.tab_size, r.indent_source),
            (3, StyleSource::Editorconfig)
        );
        assert_eq!(
            (r.right_margin, r.right_margin_source),
            (Some(80), StyleSource::Editorconfig)
        );
        let off = resolve(&config(&[("max_line_length", "off")]), &p, &ide, "Java");
        assert_eq!(off.right_margin, None);
        // A language the scheme says nothing about keeps Code Style's indentation but takes the
        // scheme's margin.
        let py = resolve(
            &EditorConfig::default(),
            &ProjectStyle::of(Some(&s), "Python"),
            &ide,
            "Python",
        );
        assert_eq!((py.tab_size, py.indent_source), (4, StyleSource::Ide));
        assert_eq!(py.right_margin_source, StyleSource::Project);
        // Without a scheme: Code Style.
        let none = resolve(
            &EditorConfig::default(),
            &ProjectStyle::default(),
            &ide,
            "Go",
        );
        assert_eq!(
            (none.tab_size, none.insert_spaces, none.indent_source),
            (4, false, StyleSource::Ide)
        );
        assert_eq!(
            (none.right_margin, none.right_margin_source),
            (Some(120), StyleSource::Ide)
        );
    }

    #[test]
    fn values_the_scheme_leaves_out_come_from_code_style() {
        let mut ide = CodeStyleSettings::default();
        ide.indents.insert(
            "Java".into(),
            IndentStyle {
                indent: 3,
                tab: 6,
                use_tabs: false,
            },
        );
        // Only tabs switched on: the tab width is Code Style's.
        let s = scheme(vec![IdeaLanguageStyle {
            language: "JAVA".into(),
            use_tabs: Some(true),
            ..Default::default()
        }]);
        let r = resolve(
            &EditorConfig::default(),
            &ProjectStyle::of(Some(&s), "Java"),
            &ide,
            "Java",
        );
        assert_eq!((r.tab_size, r.insert_spaces), (6, false));
        assert_eq!(r.indent_source, StyleSource::Project);
        // A tab width of the `.editorconfig` goes with the scheme's tabs.
        let r = resolve(
            &config(&[("tab_width", "8")]),
            &ProjectStyle::of(Some(&s), "Java"),
            &ide,
            "Java",
        );
        assert_eq!((r.tab_size, r.insert_spaces), (8, false));
        // The language's margin before the scheme's.
        let s = scheme(vec![lang("JAVA", None, Some(90))]);
        let p = ProjectStyle::of(Some(&s), "Java");
        assert_eq!(p.right_margin, Some(90));
        assert!(!p.sets_indent());
        let r = resolve(&EditorConfig::default(), &p, &ide, "Java");
        assert_eq!((r.tab_size, r.indent_source), (3, StyleSource::Ide));
    }

    #[test]
    fn the_scheme_applies_only_when_switched_on() {
        let mut s = scheme(vec![lang("JAVA", Some(2), None)]);
        assert!(ProjectStyle::of(Some(&s), "Java").sets_indent());
        s.per_project = false;
        assert_eq!(ProjectStyle::of(Some(&s), "Java"), ProjectStyle::default());
        s.per_project = true;
        s.preferred_scheme = Some("Team".into());
        assert_eq!(ProjectStyle::of(Some(&s), "Java"), ProjectStyle::default());
        s.preferred_scheme = None;
        s.file = None;
        assert!(!s.applies());
        assert_eq!(ProjectStyle::of(None, "Java"), ProjectStyle::default());
    }

    #[test]
    fn language_ids_map_to_code_style_rows() {
        for row in crate::CODE_STYLE_LANGUAGES {
            assert_eq!(idea_language_id(row).is_none(), row == "Default", "{row}");
        }
        let s = scheme(vec![
            lang("XML", Some(1), None),
            lang("Kotlin", Some(2), None),
            lang("kotlin", Some(8), None),
            lang("Klingon", Some(5), None),
            lang("yaml", Some(4), None),
            lang("less", Some(3), None),
        ]);
        // Case ignored; the first of two elements counts.
        assert_eq!(ProjectStyle::of(Some(&s), "Kotlin").indent, Some(2));
        assert_eq!(ProjectStyle::of(Some(&s), "YAML").indent, Some(4));
        assert_eq!(ProjectStyle::of(Some(&s), "Less").indent, Some(3));
        // XML has no Code Style row, an unknown id none; "Default" has no id.
        for row in crate::CODE_STYLE_LANGUAGES {
            if !["Kotlin", "YAML", "Less"].contains(&row) {
                assert_eq!(ProjectStyle::of(Some(&s), row).indent, None, "{row}");
            }
        }
        assert_eq!(idea_language_id("Java"), Some("JAVA"));
        assert_eq!(idea_language_id("C/C++"), Some("ObjectiveC"));
    }

    #[test]
    fn the_project_of_a_file_is_the_nearest_folder_with_idea() {
        let root = std::env::temp_dir().join(format!("ferris-idea-pcs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".idea/codeStyles")).unwrap();
        std::fs::create_dir_all(root.join("src/main")).unwrap();
        std::fs::write(
            root.join(".idea/codeStyles/codeStyleConfig.xml"),
            r#"<component name="ProjectCodeStyleConfiguration"><state><option name="USE_PER_PROJECT_SETTINGS" value="true" /></state></component>"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".idea/codeStyles/Project.xml"),
            r#"<component name="ProjectCodeStyleConfiguration"><code_scheme name="Project" version="173"><option name="RIGHT_MARGIN" value="100" /><codeStyleSettings language="JAVA"><indentOptions><option name="INDENT_SIZE" value="2" /></indentOptions></codeStyleSettings></code_scheme></component>"#,
        )
        .unwrap();
        let file = root.join("src/main/A.java");
        assert_eq!(project_dir(&file, None).as_deref(), Some(root.as_path()));
        let p = ProjectStyle::for_file(&file, None, "Java");
        assert_eq!((p.indent, p.right_margin), (Some(2), Some(100)));
        // The stamp changes with a scheme file.
        let before = change_stamp(&file);
        assert_eq!(before.len(), 2);
        std::fs::write(root.join(".idea/codeStyles/Project.xml"), "<component/>").unwrap();
        assert_ne!(change_stamp(&file), before);
        // The search stops at the given root.
        assert_eq!(project_dir(&file, Some(&root.join("src"))), None);
        // A nested project with `.idea` but no scheme hides the outer one.
        std::fs::create_dir_all(root.join("src/.idea")).unwrap();
        assert_eq!(
            ProjectStyle::for_file(&file, None, "Java"),
            ProjectStyle::default()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

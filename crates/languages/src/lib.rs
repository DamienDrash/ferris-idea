//! Language identification and language-service integration.
//!
//! Implements `SPEC-0004` (language identification by file name) and
//! `SPEC-0023` (Language Server Protocol client), `SPEC-0024` (code insight over LSP)
//! and `SPEC-0027` (syntax highlighting).

use std::path::Path;

pub mod actions;
pub mod config_completion;
pub mod context;
pub mod copyright;
pub mod debug_plan;
pub mod declaration_kind;
pub mod dictionaries;
pub mod endpoints;
pub mod everywhere;
pub mod file_types;
pub mod find_usages;
pub mod folding;
pub mod format;
pub mod grammar_ignore;
pub mod grazie;
pub mod highlighting;
pub mod html_css;
pub mod inlay;
pub mod insight;
mod inspection_catalogue;
pub mod inspection_profile;
pub mod inspections;
pub mod java_debug;
pub mod java_generate;
pub mod java_inspections;
pub mod java_lambdas;
pub mod java_marks;
pub mod java_quick_fixes;
pub mod java_reassigned;
pub mod java_refactor;
pub mod java_roots;
pub mod java_structure;
pub mod java_types;
pub mod javadoc;
pub mod js_imports;
pub mod js_tests;
pub mod js_tools;
pub mod jvm;
pub mod kotlin_inspections;
pub mod kotlin_structure;
pub mod lsp;
pub mod markdown_format;
pub mod move_file;
pub mod names;
pub mod occurrences;
pub mod pool;
pub mod project_dictionary;
pub mod properties;
pub mod python_completion;
pub mod python_constant;
pub mod python_extract;
pub mod python_inline;
pub mod python_inspections;
pub mod python_origin;
pub mod python_quick_fixes;
pub mod python_server;
pub mod python_structure;
pub mod python_tools;
pub mod quick_definition;
pub mod reference_index;
pub mod reference_name;
pub mod related;
pub mod rename;
pub mod rename_text;
pub mod safe_delete;
pub mod script_completion;
pub mod script_structure;
pub mod script_usages;
pub mod semantic;
pub mod servers;
pub mod shown;
pub mod signature;
pub mod smart;
pub mod spring;
pub mod spring_nav;
pub mod structure;
pub mod syntax;
pub mod test_markers;
pub mod todo;
pub mod ts_extract;
pub mod ts_refactor;
pub mod ts_structure;
pub mod undo_group;
pub mod usages;
pub mod watched_files;

/// Languages the project plans to support, in priority order of the roadmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    /// Python
    Python,
    /// SQL (dialect is resolved separately by the database crate)
    Sql,
    /// JavaScript
    JavaScript,
    /// TypeScript
    TypeScript,
    /// Java
    Java,
    /// Kotlin
    Kotlin,
    /// PHP
    Php,
    /// HTML
    Html,
    /// CSS
    Css,
    /// JSON
    Json,
    /// Go
    Go,
    /// Rust
    Rust,
    /// Ruby
    Ruby,
    /// C
    C,
    /// C++
    Cpp,
    /// XML (also SVG, XSD, XSL)
    Xml,
    /// YAML
    Yaml,
    /// Markdown
    Markdown,
    /// TOML
    Toml,
    /// Shell scripts (sh, bash, zsh)
    Shell,
    /// Dockerfile
    Dockerfile,
    /// Groovy (also Gradle build scripts)
    Groovy,
    /// Scala
    Scala,
    /// SCSS
    Scss,
    /// Less
    Less,
    /// Swift
    Swift,
    /// Lua
    Lua,
    /// R
    R,
    /// Vue single-file components (`SPEC-0141`)
    Vue,
    /// Svelte components (`SPEC-0141`)
    Svelte,
}

impl LanguageId {
    /// Every language, sorted by id.
    pub const ALL: [Self; 30] = [
        Self::C,
        Self::Cpp,
        Self::Css,
        Self::Dockerfile,
        Self::Go,
        Self::Groovy,
        Self::Html,
        Self::Java,
        Self::JavaScript,
        Self::Json,
        Self::Kotlin,
        Self::Less,
        Self::Lua,
        Self::Markdown,
        Self::Php,
        Self::Python,
        Self::R,
        Self::Ruby,
        Self::Rust,
        Self::Scala,
        Self::Scss,
        Self::Shell,
        Self::Sql,
        Self::Svelte,
        Self::Swift,
        Self::Toml,
        Self::TypeScript,
        Self::Vue,
        Self::Xml,
        Self::Yaml,
    ];

    /// Line comment token (`SPEC-0056`); `None` for languages with block comments only.
    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Self::Python
            | Self::Ruby
            | Self::Yaml
            | Self::Toml
            | Self::Shell
            | Self::Dockerfile
            | Self::R => Some("#"),
            Self::Sql | Self::Lua => Some("--"),
            Self::JavaScript
            | Self::TypeScript
            | Self::Java
            | Self::Kotlin
            | Self::Php
            | Self::Go
            | Self::Rust
            | Self::C
            | Self::Cpp
            | Self::Groovy
            | Self::Scala
            | Self::Scss
            | Self::Less
            | Self::Swift => Some("//"),
            Self::Html
            | Self::Css
            | Self::Json
            | Self::Xml
            | Self::Markdown
            | Self::Vue
            | Self::Svelte => None,
        }
    }

    /// Block comment tokens (`SPEC-0056` v3 behaviour 16); `None` for languages without block
    /// comments.
    pub fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::JavaScript
            | Self::TypeScript
            | Self::Java
            | Self::Kotlin
            | Self::Php
            | Self::Go
            | Self::Rust
            | Self::C
            | Self::Cpp
            | Self::Groovy
            | Self::Scala
            | Self::Scss
            | Self::Less
            | Self::Swift
            | Self::Css
            | Self::Sql => Some(("/*", "*/")),
            Self::Html | Self::Xml | Self::Markdown | Self::Vue | Self::Svelte => {
                Some(("<!--", "-->"))
            }
            Self::Lua => Some(("--[[", "]]")),
            Self::Python
            | Self::Ruby
            | Self::Yaml
            | Self::Toml
            | Self::Shell
            | Self::Dockerfile
            | Self::R
            | Self::Json => None,
        }
    }

    /// The LSP language id for a document named `name` (a path or a `file:` URI): `.tsx` and `.jsx`
    /// are `typescriptreact` and `javascriptreact`, so servers parse their JSX (window run of hand
    /// test 60, 2026-11-10: `App.tsx` showed 362 parse errors as plain TypeScript).
    pub fn lsp_id_for(self, name: &str) -> &'static str {
        let lower = name.to_ascii_lowercase();
        match self {
            Self::TypeScript if lower.ends_with(".tsx") => "typescriptreact",
            Self::JavaScript if lower.ends_with(".jsx") => "javascriptreact",
            _ => self.lsp_id(),
        }
    }

    /// Identifier used for this language in the Language Server Protocol
    /// (`TextDocumentItem.languageId`).
    pub fn lsp_id(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Sql => "sql",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Php => "php",
            Self::Html => "html",
            Self::Css => "css",
            Self::Json => "json",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::Ruby => "ruby",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Xml => "xml",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
            Self::Toml => "toml",
            Self::Shell => "shellscript",
            Self::Dockerfile => "dockerfile",
            Self::Groovy => "groovy",
            Self::Scala => "scala",
            Self::Scss => "scss",
            Self::Less => "less",
            Self::Swift => "swift",
            Self::Lua => "lua",
            Self::R => "r",
            Self::Vue => "vue",
            Self::Svelte => "svelte",
        }
    }

    /// The language of the file at `path`: its project's override or file name pattern, else its
    /// name, else its hashbang line (`SPEC-0004` v5 behaviours 9–12, [`file_types`]).
    pub fn detect(path: &Path) -> Option<Self> {
        file_types::detect(path).language
    }

    /// The name people know the language by, as lists show it (`SPEC-0004` v5 behaviour 13).
    pub fn title(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::Sql => "SQL",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::Php => "PHP",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Json => "JSON",
            Self::Go => "Go",
            Self::Rust => "Rust",
            Self::Ruby => "Ruby",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Xml => "XML",
            Self::Yaml => "YAML",
            Self::Markdown => "Markdown",
            Self::Toml => "TOML",
            Self::Shell => "Shell Script",
            Self::Dockerfile => "Dockerfile",
            Self::Groovy => "Groovy",
            Self::Scala => "Scala",
            Self::Scss => "SCSS",
            Self::Less => "Less",
            Self::Swift => "Swift",
            Self::Lua => "Lua",
            Self::R => "R",
            Self::Vue => "Vue",
            Self::Svelte => "Svelte",
        }
    }

    /// The language whose LSP id is `id`.
    pub fn from_lsp_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.lsp_id() == id)
    }

    /// Detects the language from a file name alone (behaviours 1, 2, 8). Extensions are matched
    /// case-insensitively; well-known extensionless names are matched exactly.
    pub fn detect_by_name(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        match name {
            "Gemfile" | "Rakefile" => return Some(Self::Ruby),
            "Dockerfile" | "Containerfile" => return Some(Self::Dockerfile),
            "Jenkinsfile" => return Some(Self::Groovy),
            "Cargo.lock" | "Pipfile" => return Some(Self::Toml),
            ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile" | ".profile" => {
                return Some(Self::Shell);
            }
            _ if name.starts_with("Dockerfile.") => return Some(Self::Dockerfile),
            _ => {}
        }
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "py" | "pyi" | "pyw" => Self::Python,
            "sql" | "ddl" => Self::Sql,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" | "tsx" => Self::TypeScript,
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "php" | "phtml" => Self::Php,
            "html" | "htm" => Self::Html,
            "vue" => Self::Vue,
            "svelte" => Self::Svelte,
            "css" => Self::Css,
            "json" | "jsonc" => Self::Json,
            "go" => Self::Go,
            "rs" => Self::Rust,
            "rb" | "rake" | "gemspec" => Self::Ruby,
            "c" => Self::C,
            // `.h` is ambiguous between C and C++; C++ is the safer default
            // because a C++ language service also understands C headers.
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "h" => Self::Cpp,
            "xml" | "xsd" | "xsl" | "xslt" | "svg" | "fxml" | "iml" => Self::Xml,
            "yaml" | "yml" => Self::Yaml,
            "md" | "markdown" => Self::Markdown,
            "toml" => Self::Toml,
            "sh" | "bash" | "zsh" => Self::Shell,
            "dockerfile" => Self::Dockerfile,
            "groovy" | "gradle" | "gvy" => Self::Groovy,
            "scala" | "sc" | "sbt" => Self::Scala,
            "scss" => Self::Scss,
            "less" => Self::Less,
            "swift" => Self::Swift,
            "lua" => Self::Lua,
            "r" => Self::R,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(name: &str) -> Option<LanguageId> {
        LanguageId::detect(Path::new(name))
    }

    #[test]
    fn detects_priority_languages() {
        assert_eq!(detect("src/app.py"), Some(LanguageId::Python));
        assert_eq!(detect("schema.SQL"), Some(LanguageId::Sql));
        assert_eq!(detect("index.tsx"), Some(LanguageId::TypeScript));
        assert_eq!(detect("main.mjs"), Some(LanguageId::JavaScript));
        assert_eq!(detect("Main.java"), Some(LanguageId::Java));
    }

    #[test]
    fn special_names_and_unknowns() {
        assert_eq!(detect("Gemfile"), Some(LanguageId::Ruby));
        assert_eq!(detect("util.h"), Some(LanguageId::Cpp));
        assert_eq!(detect("README"), None);
        assert_eq!(detect("notes.txt"), None);
        assert_eq!(detect("package.json"), Some(LanguageId::Json));
    }

    #[test]
    fn added_languages() {
        assert_eq!(detect("pom.xml"), Some(LanguageId::Xml));
        assert_eq!(detect("icon.svg"), Some(LanguageId::Xml));
        assert_eq!(detect("ci.yml"), Some(LanguageId::Yaml));
        assert_eq!(detect("README.md"), Some(LanguageId::Markdown));
        assert_eq!(detect("Cargo.toml"), Some(LanguageId::Toml));
        assert_eq!(detect("Cargo.lock"), Some(LanguageId::Toml));
        assert_eq!(detect("build.sh"), Some(LanguageId::Shell));
        assert_eq!(detect(".zshrc"), Some(LanguageId::Shell));
        assert_eq!(detect("Dockerfile"), Some(LanguageId::Dockerfile));
        assert_eq!(detect("Dockerfile.dev"), Some(LanguageId::Dockerfile));
        assert_eq!(detect("build.gradle"), Some(LanguageId::Groovy));
        assert_eq!(detect("Jenkinsfile"), Some(LanguageId::Groovy));
        assert_eq!(detect("Main.scala"), Some(LanguageId::Scala));
        assert_eq!(detect("build.sbt"), Some(LanguageId::Scala));
        assert_eq!(detect("site.scss"), Some(LanguageId::Scss));
        assert_eq!(detect("site.less"), Some(LanguageId::Less));
        assert_eq!(detect("App.swift"), Some(LanguageId::Swift));
        assert_eq!(detect("init.lua"), Some(LanguageId::Lua));
        assert_eq!(detect("App.vue"), Some(LanguageId::Vue));
        assert_eq!(detect("Page.svelte"), Some(LanguageId::Svelte));
        assert_eq!(detect("analysis.R"), Some(LanguageId::R));
        // `build.gradle.kts` stays Kotlin.
        assert_eq!(detect("build.gradle.kts"), Some(LanguageId::Kotlin));
        let mut ids: Vec<&str> = LanguageId::ALL.iter().map(|l| l.lsp_id()).collect();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn lsp_ids_are_lowercase() {
        assert_eq!(LanguageId::Cpp.lsp_id(), "cpp");
        assert_eq!(LanguageId::TypeScript.lsp_id(), "typescript");
    }

    #[test]
    fn jsx_files_open_as_react_documents() {
        assert_eq!(
            LanguageId::TypeScript.lsp_id_for("file:///w/src/App.tsx"),
            "typescriptreact"
        );
        assert_eq!(
            LanguageId::TypeScript.lsp_id_for("/w/a.TSX"),
            "typescriptreact"
        );
        assert_eq!(LanguageId::TypeScript.lsp_id_for("/w/a.ts"), "typescript");
        assert_eq!(
            LanguageId::JavaScript.lsp_id_for("/w/a.jsx"),
            "javascriptreact"
        );
        assert_eq!(LanguageId::JavaScript.lsp_id_for("/w/a.js"), "javascript");
        assert_eq!(LanguageId::Cpp.lsp_id_for("/w/a.cpp"), "cpp");
    }
}

//! Bundled New UI icons (`SPEC-0060` behaviour 8): which icon a file or folder gets, and the SVG bytes.
//!
//! The SVG files are unchanged upstream assets (`icons/README.md`); this module is own code.

/// One icon: key (upstream path without `.svg`), light and dark SVG.
pub struct Icon {
    /// Upstream path without extension, e.g. `fileTypes/json`.
    pub key: &'static str,
    /// Light variant.
    pub light: &'static [u8],
    /// Dark variant.
    pub dark: &'static [u8],
}

macro_rules! icon {
    ($path:literal) => {
        Icon {
            key: $path,
            light: include_bytes!(concat!("../icons/", $path, ".svg")),
            dark: include_bytes!(concat!("../icons/", $path, "_dark.svg")),
        }
    };
}

/// Every bundled icon.
pub const ICONS: &[Icon] = &[
    icon!("nodes/folder"),
    icon!("general/chevronRight"),
    icon!("general/chevronDown"),
    icon!("general/closeSmall"),
    icon!("general/closeSmallHovered"),
    icon!("general/search"),
    icon!("run/run"),
    icon!("run/stop"),
    icon!("nodes/dataSchema"),
    icon!("nodes/dataTables"),
    icon!("nodes/dataColumn"),
    icon!("idecraft/dbKeyPrimary"),
    icon!("idecraft/dbKeyForeign"),
    icon!("idecraft/dbIndex"),
    // The Debug window's toolbar (SPEC-0194 v3 behaviour 10).
    icon!("idecraft/debugPause"),
    icon!("idecraft/debugRunToCursor"),
    icon!("idecraft/debugRerun"),
    icon!("idecraft/debugMuteBreakpoints"),
    icon!("idecraft/debugViewBreakpoints"),
    // Own tool window icons (SPEC-0061 v7 behaviours 41–43).
    icon!("idecraft/moreHorizontal"),
    icon!("idecraft/structure"),
    icon!("idecraft/spring"),
    icon!("idecraft/chat"),
    icon!("idecraft/profiler"),
    icon!("idecraft/remote"),
    // Own Git icons: the branch popup's actions and tags (SPEC-0068 v5 behaviour 15) and the
    // Commit window's toolbar (SPEC-0063 v7 behaviour 19).
    icon!("idecraft/vcsUpdate"),
    icon!("idecraft/vcsCommit"),
    icon!("idecraft/vcsPush"),
    icon!("idecraft/tag"),
    icon!("idecraft/rollback"),
    icon!("idecraft/diff"),
    icon!("idecraft/groupBy"),
    icon!("idecraft/filter"),
    icon!("idecraft/preview"),
    icon!("idecraft/openInFind"),
    icon!("idecraft/newLine"),
    icon!("idecraft/pin"),
    // The mark of a file whose type is overridden (SPEC-0004 v6 behaviour 15).
    icon!("idecraft/overrideMark"),
    icon!("idecraft/history"),
    icon!("idecraft/preserveCase"),
    // Tag labels of the Git log (SPEC-0086 v7 behaviour 14).
    icon!("idecraft/gitTag"),
    // Favourite branches: the star filled and as an outline (SPEC-0068 v8, SPEC-0086 v10).
    icon!("idecraft/star"),
    icon!("idecraft/starOutline"),
    // The build tools window's toolbar, the folder roles of Maven and Gradle modules and the POM
    // file icon (SPEC-0100 v2, SPEC-0048 v3).
    icon!("idecraft/execute"),
    icon!("idecraft/offline"),
    icon!("idecraft/skipTests"),
    icon!("idecraft/sourceRoot"),
    icon!("idecraft/testRoot"),
    icon!("idecraft/resourcesRoot"),
    icon!("idecraft/testResourcesRoot"),
    icon!("idecraft/excludedRoot"),
    icon!("idecraft/maven"),
    // Sources, analyzer, diagram and unlink of the build tools window (SPEC-0100 v3).
    icon!("idecraft/download"),
    icon!("idecraft/dependencyAnalyzer"),
    icon!("idecraft/diagram"),
    icon!("idecraft/remove"),
    icon!("status/warning"),
    icon!("run/testPassed"),
    icon!("run/testFailed"),
    icon!("run/testError"),
    icon!("run/testIgnored"),
    icon!("toolwindows/run"),
    icon!("toolwindows/services"),
    icon!("breakpoints/breakpoint"),
    icon!("breakpoints/breakpointValid"),
    icon!("breakpoints/breakpointInvalid"),
    icon!("run/debug"),
    icon!("run/resume"),
    icon!("run/stepOver"),
    icon!("run/stepInto"),
    icon!("run/stepOut"),
    icon!("toolwindows/debug"),
    icon!("toolwindows/problems"),
    icon!("status/error"),
    icon!("status/info"),
    icon!("debugger/frame"),
    icon!("debugger/dbPrimitive"),
    icon!("debugger/dbObject"),
    icon!("general/refresh"),
    icon!("general/vcs"),
    icon!("toolwindows/commit"),
    icon!("toolwindows/project"),
    icon!("toolwindows/terminal"),
    icon!("fileTypes/text"),
    icon!("fileTypes/unknown"),
    icon!("fileTypes/json"),
    icon!("fileTypes/xml"),
    icon!("fileTypes/html"),
    icon!("fileTypes/css"),
    icon!("fileTypes/markdown"),
    icon!("fileTypes/yaml"),
    icon!("fileTypes/toml"),
    icon!("fileTypes/properties"),
    icon!("fileTypes/shell"),
    icon!("fileTypes/sql"),
    icon!("fileTypes/java"),
    icon!("fileTypes/javaScript"),
    icon!("fileTypes/typeScript"),
    icon!("fileTypes/c"),
    icon!("fileTypes/cpp"),
    icon!("fileTypes/h"),
    icon!("fileTypes/image"),
    icon!("fileTypes/archive"),
    icon!("fileTypes/csv"),
    icon!("fileTypes/config"),
    icon!("fileTypes/editorConfig"),
    icon!("fileTypes/gradle"),
    icon!("fileTypes/groovy"),
    icon!("fileTypes/patch"),
    icon!("fileTypes/python"),
    icon!("fileTypes/kotlin"),
    icon!("fileTypes/swiftLang"),
    icon!("fileTypes/docker"),
    icon!("fileTypes/gitignore"),
    icon!("fileTypes/vue"),
    icon!("fileTypes/graphql"),
    icon!("fileTypes/http"),
    icon!("fileTypes/jupyter"),
    icon!("fileTypes/terraform"),
    icon!("fileTypes/perl"),
    icon!("idecraft/rust"),
    icon!("idecraft/go"),
    icon!("idecraft/ruby"),
    icon!("idecraft/php"),
    icon!("idecraft/svelte"),
    icon!("idecraft/astro"),
    // Kinds of a Java or Kotlin file's primary declaration that have no bundled node icon
    // (SPEC-0131 v3 behaviour 7).
    icon!("idecraft/record"),
    icon!("idecraft/annotation"),
    icon!("idecraft/object"),
    icon!("codeInsight/intentionBulb"),
    icon!("codeInsight/quickfixBulb"),
    icon!("nodes/class"),
    icon!("nodes/enum"),
    icon!("nodes/field"),
    icon!("nodes/function"),
    icon!("nodes/interface"),
    icon!("nodes/method"),
    icon!("nodes/variable"),
    icon!("nodes/constant"),
    icon!("nodes/property"),
    icon!("nodes/module"),
    icon!("nodes/package"),
    icon!("nodes/parameter"),
    icon!("nodes/template"),
    // Window structure (SPEC-0061 v4): header actions, toolbar, tree nodes, 20 px stripe icons.
    icon!("general/moreVertical"),
    icon!("general/hide"),
    icon!("general/locate"),
    icon!("general/expandAll"),
    icon!("general/collapseAll"),
    icon!("general/up"),
    icon!("general/down"),
    icon!("general/softWrap"),
    icon!("general/scrollDown"),
    icon!("general/delete"),
    icon!("general/add"),
    icon!("general/settings"),
    icon!("general/search@20x20"),
    icon!("general/settings@20x20"),
    icon!("nodes/library"),
    icon!("fileTypes/scratches"),
    icon!("toolwindows/project@20x20"),
    icon!("toolwindows/commit@20x20"),
    icon!("toolwindows/problems@20x20"),
    icon!("toolwindows/run@20x20"),
    icon!("toolwindows/debug@20x20"),
    icon!("toolwindows/services@20x20"),
    icon!("toolwindows/vcs@20x20"),
    icon!("toolwindows/build@20x20"),
    icon!("toolwindows/terminal@20x20"),
    // The TODO tool window (SPEC-0334 behaviour 13).
    icon!("toolwindows/todo"),
    icon!("toolwindows/todo@20x20"),
    // The Structure and Notifications tool windows and the menu marker of header buttons
    // (SPEC-0061 v17 behaviours 63, 64, 67).
    icon!("toolwindows/structure"),
    icon!("toolwindows/structure@20x20"),
    icon!("toolwindows/notifications"),
    icon!("toolwindows/notifications@20x20"),
    icon!("general/dropdown"),
];

/// The icon of a language server symbol kind (`SPEC-0079`), e.g. `function` → `nodes/function`.
pub fn symbol_icon(kind: &str) -> &'static str {
    match kind {
        "class" | "struct" | "object" => "nodes/class",
        "enum" => "nodes/enum",
        "interface" => "nodes/interface",
        "method" | "constructor" | "operator" => "nodes/method",
        "function" | "event" => "nodes/function",
        "field" | "enum-member" => "nodes/field",
        "constant" | "number" | "string" | "boolean" | "null" => "nodes/constant",
        "property" | "key" => "nodes/property",
        "module" | "namespace" => "nodes/module",
        "package" => "nodes/package",
        "type-parameter" => "nodes/parameter",
        _ => "nodes/variable",
    }
}

/// The SVG of `key` for the dark or light theme.
pub fn svg(key: &str, dark: bool) -> Option<&'static [u8]> {
    ICONS
        .iter()
        .find(|i| i.key == key)
        .map(|i| if dark { i.dark } else { i.light })
}

/// Icon key for a file name, by extension or well-known name; `fileTypes/unknown` otherwise.
/// Languages whose reference icons are not open (Rust, Go, Ruby, PHP) get IDECraft's own icons
/// (`SPEC-0131` behaviour 4).
pub fn file_icon(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        ".editorconfig" => return "fileTypes/editorConfig",
        ".gitignore" | ".dockerignore" | ".ignore" => return "fileTypes/gitignore",
        "dockerfile" | "containerfile" => return "fileTypes/docker",
        "build.gradle" | "settings.gradle" | "build.gradle.kts" | "settings.gradle.kts" => {
            return "fileTypes/gradle";
        }
        // Maven's build file and the build tools' wrapper scripts (SPEC-0048 v3 behaviour 10).
        "pom.xml" => return "idecraft/maven",
        "gradlew" | "mvnw" => return "fileTypes/shell",
        _ => {}
    }
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "json" | "jsonc" | "json5" => "fileTypes/json",
        "xml" | "xsd" | "xsl" | "iml" | "svg" | "plist" => "fileTypes/xml",
        "html" | "htm" | "xhtml" => "fileTypes/html",
        "css" | "scss" | "sass" | "less" => "fileTypes/css",
        "md" | "markdown" => "fileTypes/markdown",
        "yml" | "yaml" => "fileTypes/yaml",
        "toml" => "fileTypes/toml",
        "properties" | "ini" | "env" => "fileTypes/properties",
        "sh" | "bash" | "zsh" | "fish" => "fileTypes/shell",
        "sql" | "ddl" => "fileTypes/sql",
        "java" => "fileTypes/java",
        "js" | "mjs" | "cjs" | "jsx" => "fileTypes/javaScript",
        "ts" | "tsx" | "mts" | "cts" => "fileTypes/typeScript",
        "c" => "fileTypes/c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "fileTypes/cpp",
        "h" => "fileTypes/h",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => "fileTypes/image",
        "zip" | "jar" | "tar" | "gz" | "tgz" | "xz" | "7z" => "fileTypes/archive",
        "csv" | "tsv" => "fileTypes/csv",
        "conf" | "cfg" | "lock" => "fileTypes/config",
        "gradle" | "kts" => "fileTypes/gradle",
        "groovy" => "fileTypes/groovy",
        "patch" | "diff" => "fileTypes/patch",
        "py" | "pyi" | "pyw" => "fileTypes/python",
        "kt" => "fileTypes/kotlin",
        "swift" => "fileTypes/swiftLang",
        "vue" => "fileTypes/vue",
        // Web component and template languages (SPEC-0131 v2 behaviour 4): own icons where the
        // reference's are not open, HTML templates the HTML icon, MDX the Markdown icon.
        "svelte" => "idecraft/svelte",
        "astro" => "idecraft/astro",
        "hbs" | "handlebars" | "mustache" | "ejs" | "njk" | "twig" | "liquid" | "jinja"
        | "jinja2" | "j2" | "pug" | "jade" | "erb" => "fileTypes/html",
        "mdx" => "fileTypes/markdown",
        "graphql" | "gql" => "fileTypes/graphql",
        "http" | "rest" => "fileTypes/http",
        "ipynb" => "fileTypes/jupyter",
        "tf" | "tfvars" => "fileTypes/terraform",
        "pl" | "pm" => "fileTypes/perl",
        "rs" => "idecraft/rust",
        "go" => "idecraft/go",
        "rb" | "rake" | "gemspec" => "idecraft/ruby",
        "php" | "phtml" => "idecraft/php",
        "txt" | "log" => "fileTypes/text",
        _ => "fileTypes/unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_both_variants_and_files_map_to_bundled_icons() {
        assert_eq!(ICONS.len(), 176);
        // SPEC-0068 v8 AK-15: the favourite stars, light and dark.
        assert!(
            svg("idecraft/star", true).is_some() && svg("idecraft/starOutline", false).is_some()
        );
        assert!(svg("idecraft/gitTag", false).is_some());
        // SPEC-0061 v7 AK-26: every tool window's stripe icon is bundled.
        for w in crate::tool_windows::ToolWindow::ALL {
            assert!(svg(w.icon(), true).is_some(), "{}", w.icon());
        }
        for i in ICONS {
            for bytes in [i.light, i.dark] {
                assert!(String::from_utf8_lossy(bytes).contains("<svg"), "{}", i.key);
            }
        }
        for name in [
            "a.json",
            "Cargo.toml",
            "main.rs",
            "x.unknownext",
            "build.gradle.kts",
            "README.MD",
        ] {
            assert!(svg(file_icon(name), true).is_some(), "{name}");
        }
        assert_eq!(file_icon("README.MD"), "fileTypes/markdown");
        assert_eq!(file_icon("main.rs"), "idecraft/rust");
        assert_eq!(file_icon("app.py"), "fileTypes/python");
        assert_eq!(file_icon("Dockerfile"), "fileTypes/docker");
        assert_eq!(file_icon(".gitignore"), "fileTypes/gitignore");
        assert_eq!(file_icon("notes.txt"), "fileTypes/text");
        assert_eq!(file_icon("noext"), "fileTypes/unknown");
        // SPEC-0131 v2 AK-1: web component and template files never show the unknown icon.
        assert_eq!(file_icon("App.svelte"), "idecraft/svelte");
        assert_eq!(file_icon("index.astro"), "idecraft/astro");
        assert_eq!(file_icon("App.vue"), "fileTypes/vue");
        for name in [
            "a.hbs", "a.ejs", "a.njk", "a.twig", "a.liquid", "a.pug", "a.j2", "a.mdx",
        ] {
            assert_ne!(file_icon(name), "fileTypes/unknown", "{name}");
            assert!(svg(file_icon(name), false).is_some(), "{name}");
        }
        assert!(svg("nodes/folder", false).is_some());
        assert!(svg("missing", true).is_none());
    }
}

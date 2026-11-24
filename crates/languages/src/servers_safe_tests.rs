//! Language servers in safe mode (`SPEC-0278` AK-5).

use super::*;
use std::os::unix::fs::PermissionsExt;

fn program(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// SPEC-0291 AK-2e (v7): typescript-language-server is asked for the types of fields and the values
/// of enum members, for both languages, and for no kind beyond those of behaviours 1 and 2b.
#[test]
fn typescript_hint_settings() {
    let base = std::env::temp_dir().join(format!("ferris-idea-ts-hints-{}", std::process::id()));
    let bin = base.join("bin");
    let root = base.join("project");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let path = bin.clone().into_os_string();
    program(&bin, "typescript-language-server");
    let ts = resolve_for_mode(&path, LanguageId::TypeScript, &root, false).unwrap();
    let settings = ts.settings(&root);
    for language in ["typescript", "javascript"] {
        let hints = settings[language]["inlayHints"].as_object().unwrap();
        assert_eq!(hints["includeInlayPropertyDeclarationTypeHints"], true);
        assert_eq!(hints["includeInlayEnumMemberValueHints"], true);
        let mut keys: Vec<&str> = hints.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "includeInlayEnumMemberValueHints",
                "includeInlayFunctionLikeReturnTypeHints",
                "includeInlayParameterNameHints",
                "includeInlayParameterNameHintsWhenArgumentMatchesName",
                "includeInlayPropertyDeclarationTypeHints",
                "includeInlayVariableTypeHints",
                "includeInlayVariableTypeHintsWhenTypeMatchesName",
            ]
        );
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn servers_in_safe_mode() {
    let base = std::env::temp_dir().join(format!("ferris-idea-safe-srv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (bin, root) = (base.join("bin"), base.join("project"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let path = std::env::join_paths([&bin]).unwrap();

    // rust-analyzer: no build scripts, proc macros or check, also when it asks for settings.
    program(&bin, "rust-analyzer");
    let full = resolve_for_mode(&path, LanguageId::Rust, &root, false).unwrap();
    assert!(!full.safe);
    // SPEC-0292 v8 behaviour 11: reference lenses in both modes, nothing of safe mode in full.
    let o = full.initialization_options(std::path::Path::new("/nonexistent"));
    assert_eq!(o["lens"]["references"]["adt"]["enable"], true);
    assert!(o["procMacro"].is_null() && o["cargo"].is_null(), "{o}");
    assert_eq!(full.settings(&root)["rust-analyzer"], o);
    let safe = resolve_for_mode(&path, LanguageId::Rust, &root, true).unwrap();
    assert!(safe.safe);
    let o = safe.initialization_options(std::path::Path::new("/nonexistent"));
    assert_eq!(o["cargo"]["buildScripts"]["enable"], false);
    assert_eq!(o["procMacro"]["enable"], false);
    assert_eq!(o["checkOnSave"], false);
    assert_eq!(o["lens"]["references"]["method"]["enable"], true);
    assert_eq!(safe.settings(&root)["rust-analyzer"], o);
    assert_eq!(serde_json::to_value(&safe).unwrap()["safe_mode"], true);

    // jdtls: no Gradle or Maven import, signature help kept.
    program(&bin, "jdtls");
    let j = resolve_for_mode(&path, LanguageId::Java, &root, true).unwrap();
    let java = &j.initialization_options(std::path::Path::new("/nonexistent"))["settings"]["java"];
    assert_eq!(java["import"]["gradle"]["enabled"], false);
    assert_eq!(java["import"]["maven"]["enabled"], false);
    assert_eq!(java["signatureHelp"]["enabled"], true);
    assert!(java["project"].is_null(), "no roots without Java files");
    // Inferred source roots, in safe mode and without it (`SPEC-0042` v8 behaviour 17).
    std::fs::create_dir_all(root.join("src/main/java")).unwrap();
    let full = resolve_for_mode(&path, LanguageId::Java, &root, false).unwrap();
    for r in [&j, &full] {
        // SPEC-0048 v5 behaviour 13: no Eclipse files in the project.
        assert_eq!(
            r.initialization_options(&root)["settings"]["java"]["import"]["generatesMetadataFilesAtProjectRoot"],
            false
        );
        assert_eq!(
            r.initialization_options(&root)["settings"]["java"]["project"]["sourcePaths"],
            serde_json::json!(["src/main/java"])
        );
    }

    // Servers that would run project code are skipped with the reason.
    program(&bin, "kotlin-language-server");
    let err = resolve_for_mode(&path, LanguageId::Kotlin, &root, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("not started in safe mode"), "{err}");
    assert!(err.contains("Gradle or Maven"), "{err}");
    assert!(resolve_for_mode(&path, LanguageId::Kotlin, &root, false).is_ok());

    // typescript-language-server without its own TypeScript: not in safe mode; vtsls instead.
    program(&bin, "typescript-language-server");
    let err = resolve_for_mode(&path, LanguageId::TypeScript, &root, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("no TypeScript is installed with it"), "{err}");
    program(&bin, "vtsls");
    let ts = resolve_for_mode(&path, LanguageId::TypeScript, &root, true).unwrap();
    assert_eq!(ts.candidate.program, "vtsls");

    // Angular: probes only the server's own installation.
    let modules = base.join("node_modules");
    let ng_bin = modules.join(".bin");
    std::fs::create_dir_all(&ng_bin).unwrap();
    program(&ng_bin, "ngserver");
    std::fs::write(root.join("angular.json"), "{}").unwrap();
    let ng_path = std::env::join_paths([&ng_bin]).unwrap();
    let ng = resolve_for_mode(&ng_path, LanguageId::Html, &root, true).unwrap();
    let probes = &ng.extra_args[1];
    assert!(!probes.split(',').any(|p| Path::new(p) == root), "{probes}");
    let ng = resolve_for_mode(&ng_path, LanguageId::Html, &root, false).unwrap();
    assert!(ng.extra_args[1].starts_with(&root.display().to_string()));

    // Python: no project interpreter in safe mode.
    program(&bin, "pyright-langserver");
    let py = resolve_for_mode(&path, LanguageId::Python, &root, true).unwrap();
    assert_eq!(py.settings(&root), serde_json::json!({"python": {}}));

    // kotlin-language-server: parameter names asked for (SPEC-0291 v5 AK-2d); not started in
    // safe mode.
    program(&bin, "kotlin-language-server");
    let kt = resolve_for_mode(&path, LanguageId::Kotlin, &root, false).unwrap();
    let hints = &kt.settings(&root)["kotlin"]["inlayHints"];
    assert_eq!(hints["parameterHints"], true);
    assert_eq!(hints["typeHints"], false);
    assert_eq!(hints["chainedHints"], false);
    assert!(resolve_for_mode(&path, LanguageId::Kotlin, &root, true).is_err());

    // Ruby: never through Bundler, ruby-lsp skipped.
    let rb = base.join("rb");
    std::fs::create_dir_all(&rb).unwrap();
    program(&rb, "bundle");
    program(&rb, "ruby-lsp");
    let solargraph = program(&rb, "solargraph");
    std::fs::write(root.join("Gemfile"), "gem 'ruby-lsp'\n").unwrap();
    std::fs::write(
        root.join("Gemfile.lock"),
        "GEM\n  remote: https://rubygems.org/\n  specs:\n    ruby-lsp (0.20.0)\n\nBUNDLED WITH\n   2.5.22\n",
    )
    .unwrap();
    let search = ferris_idea_core::RubySearch {
        root: root.clone(),
        path_var: Some(std::env::join_paths([&rb]).unwrap()),
        ..Default::default()
    };
    assert_eq!(resolve_ruby(&search).unwrap().args()[0], "exec");
    let r = resolve_ruby_mode(&search, true).unwrap();
    assert_eq!(
        (r.command, r.prefix_args.len(), r.safe),
        (solargraph, 0, true)
    );

    // The overview says what each server does in safe mode.
    let rust = overview()
        .into_iter()
        .find(|l| l.language == "rust")
        .unwrap();
    assert_eq!(rust.candidates[0].safe_mode, SafeMode::Restricted);
    assert_eq!(safe_mode("gopls").0, SafeMode::Unchanged);
    let _ = std::fs::remove_dir_all(&base);
}

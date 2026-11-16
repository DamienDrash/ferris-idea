//! SPEC-0210 AK-1, SPEC-0211 AK-1 and SPEC-0276 AK-1: the sample plugins under `tests/plugins/` (one per
//! extension point), reviewing and updating plugins.

use std::path::{Path, PathBuf};

use idecraft_plugins::{
    Access, ActionKind, CompleteContext, EditorContext, MAX_STATUS, PanelContext, Permission,
    Plugin, ReviewKind, TokenKind, action_list, compare_versions, contributions, file_type_for,
    granted, highlight, install, review, set_granted,
};

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/plugins")
}

fn sample(name: &str) -> Plugin {
    let wasm = wat::parse_file(samples().join(format!("{name}.wat"))).unwrap();
    Plugin::from_bytes(Path::new(&format!("{name}.wasm")), &wasm).unwrap()
}

/// The built modules next to the sources are what the sources compile to (with `IDECRAFT_BLESS=1`
/// they are written anew), so they can be installed by hand and by the window self-test.
#[test]
fn sample_modules_match_their_sources() {
    for name in ["complete", "filetype", "status", "panel", "runconfig"] {
        let built = wat::parse_file(samples().join(format!("{name}.wat"))).unwrap();
        let file = samples().join(format!("{name}.wasm"));
        if std::env::var_os("IDECRAFT_BLESS").is_some() {
            std::fs::write(&file, &built).unwrap();
        }
        assert_eq!(
            std::fs::read(&file).unwrap_or_default(),
            built,
            "{name}.wasm is stale: run with IDECRAFT_BLESS=1"
        );
        assert!(Plugin::load(&file).is_ok());
    }
}

#[test]
fn completion_items() {
    let p = sample("complete");
    assert!(
        action_list(std::slice::from_ref(&p)).is_empty(),
        "no command"
    );
    let action = &p.manifest.actions[0];
    assert_eq!(action.kind, ActionKind::Complete);
    assert!(action.applies_to(Path::new("a/notes.conf")));
    assert!(!action.applies_to(Path::new("main.rs")));
    let context = CompleteContext {
        path: "notes.txt".into(),
        language: None,
        text: "plu".into(),
        offset: 3,
        prefix: "plu".into(),
    };
    let items = p
        .run_complete("words", &context, Access::default())
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label, "plugin_hello");
    assert_eq!(
        items[0].insert.as_deref(),
        Some("plugin_hello"),
        "label by default"
    );
    assert_eq!(items[0].detail.as_deref(), Some("greeting"));
    assert_eq!(items[1].insert.as_deref(), Some("plugin_world()"));
    // Another kind's runner refuses the action.
    assert!(
        p.run_panel(
            "words",
            &PanelContext {
                project: "p".into(),
                active: None
            },
            Access::default()
        )
        .is_err()
    );
}

#[test]
fn status_text() {
    let p = sample("status");
    let context = |text: &str| EditorContext {
        path: "a.txt".into(),
        language: None,
        text: text.into(),
        selection_start: 0,
        selection_end: 0,
        line: 0,
        column: 0,
    };
    let status = p
        .run_status("lines", &context("one\ntwo\nthree"), Access::default())
        .unwrap();
    assert_eq!(status.text, "Lines: 3");
    assert!(status.tooltip.is_some_and(|t| t.contains("status sample")));
    // A backslash followed by `n` in the text is no line break.
    let status = p
        .run_status("lines", &context("a\\nb"), Access::default())
        .unwrap();
    assert_eq!(status.text, "Lines: 1");
    let long = "x\n".repeat(12_345);
    let status = p
        .run_status("lines", &context(&long), Access::default())
        .unwrap();
    assert_eq!(status.text, "Lines: 12346");
    assert!(status.text.chars().count() <= MAX_STATUS);
}

#[test]
fn panel_rows() {
    let p = sample("panel");
    let rows = p
        .run_panel(
            "rows",
            &PanelContext {
                project: "demo".into(),
                active: Some("notes.conf".into()),
            },
            Access::default(),
        )
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].path.as_deref(), Some("notes.conf"));
    assert_eq!(rows[0].line, Some(1));
    assert_eq!(rows[1].path, None);
}

#[test]
fn file_type_highlighting() {
    let p = sample("filetype");
    let plugins = [p];
    let (_, t) = file_type_for(&plugins, Path::new("site/nginx.conf"), false).unwrap();
    assert_eq!(t.name, "Conf");
    assert_eq!(t.line_comment.as_deref(), Some("#"));
    assert!(
        file_type_for(&plugins, Path::new("a.conf"), true).is_none(),
        "built-in wins"
    );
    assert!(file_type_for(&plugins, Path::new("a.txt"), false).is_none());
    let text = "# note\nSERVER \"a # b\" 8080 1.5\n/* x\ny */ listen 'c' plain\n";
    let tokens: Vec<(String, TokenKind)> = highlight(t, text)
        .into_iter()
        .map(|(r, k)| (text.chars().skip(r.start).take(r.len()).collect(), k))
        .collect();
    assert_eq!(
        tokens,
        [
            ("# note".to_owned(), TokenKind::Comment),
            ("SERVER".to_owned(), TokenKind::Keyword),
            ("\"a # b\"".to_owned(), TokenKind::String),
            ("8080".to_owned(), TokenKind::Number),
            ("1.5".to_owned(), TokenKind::Number),
            ("/* x\ny */".to_owned(), TokenKind::Comment),
            ("listen".to_owned(), TokenKind::Keyword),
            ("'c'".to_owned(), TokenKind::String),
        ]
    );
    // Unterminated comment and string end at the text's end and the line's end.
    let tokens = highlight(t, "'open\n/* never");
    assert_eq!(tokens[0], (0..5, TokenKind::String));
    assert_eq!(tokens[1], (6..14, TokenKind::Comment));
    assert_eq!(
        contributions(&plugins[0].manifest),
        ["file types: Conf (.conf)"]
    );
}

#[test]
fn manifests_with_bad_file_types_are_refused() {
    let source = std::fs::read_to_string(samples().join("filetype.wat")).unwrap();
    let broken = source.replace(r##"\"line_comment\":\"#\""##, r#"\"line_comment\":\"\""#);
    assert_ne!(broken, source);
    let err = Plugin::from_bytes(Path::new("x.wasm"), &wat::parse_str(&broken).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("empty comment marker"), "{err}");
}

/// SPEC-0211 AK-1.
#[test]
fn review_and_update() {
    use std::cmp::Ordering::*;
    assert_eq!(compare_versions("1.10", "1.9"), Greater);
    assert_eq!(compare_versions("1.0", "1"), Equal);
    assert_eq!(compare_versions("v2.0", "1.5.3"), Greater);
    assert_eq!(compare_versions("1.0-beta", "1.0-alpha"), Greater);
    assert_eq!(compare_versions("0.9", "0.10"), Less);

    let base = std::env::temp_dir().join(format!("idecraft-plugin-review-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (download, dir) = (base.join("download"), base.join("plugins"));
    std::fs::create_dir_all(&download).unwrap();
    let source = std::fs::read_to_string(samples().join("panel.wat")).unwrap();
    let version = |v: &str, read: bool| {
        let mut text = source.replace(r#"\"version\":\"1.0\""#, &format!(r#"\"version\":\"{v}\""#));
        if read {
            text = text.replace(
                r#"\"actions\":"#,
                r#"\"permissions\":[\"project-read\"],\"actions\":"#,
            );
        }
        let file = download.join(v).join("panel.wasm");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, wat::parse_str(&text).unwrap()).unwrap();
        file
    };
    let (v1, v2, v3) = (
        version("1.0", true),
        version("1.1", true),
        version("1.2", false),
    );
    std::fs::write(download.join("broken.wasm"), b"no").unwrap();

    assert!(review(&dir, &download.join("broken.wasm")).is_err());
    let r = review(&dir, &v1).unwrap();
    assert_eq!(r.kind, ReviewKind::Install);
    assert_eq!(r.headline(), "Install Sample Panel 1.0");
    assert_eq!(r.contributions, ["tool window"]);
    assert_eq!(r.permissions, [(Permission::ProjectRead, false)]);
    assert!(!dir.exists(), "reviewing installs nothing");

    install(&dir, &v1).unwrap();
    set_granted(&dir, "panel", Permission::ProjectRead, true).unwrap();
    assert_eq!(review(&dir, &v1).unwrap().kind, ReviewKind::Reinstall);
    let r = review(&dir, &v2).unwrap();
    assert_eq!(r.headline(), "Update Sample Panel 1.0 → 1.1");
    assert_eq!(r.permissions, [(Permission::ProjectRead, true)]);
    install(&dir, &v2).unwrap();
    assert!(
        granted(&dir, "panel", Permission::ProjectRead),
        "kept: still asked"
    );
    install(&dir, &v3).unwrap();
    assert!(
        !granted(&dir, "panel", Permission::ProjectRead),
        "dropped: no longer asked"
    );
    // Asking again after an update does not bring the old grant back.
    let r = review(&dir, &v2).unwrap();
    assert_eq!(r.kind, ReviewKind::Downgrade { from: "1.2".into() });
    assert_eq!(r.permissions, [(Permission::ProjectRead, false)]);
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0276 AK-1: the run configuration sample suggests commands; the host drops the shell and
/// the folder outside, runs nothing without the grant, and the grant goes with an update that no
/// longer asks for it and with removal.
#[test]
fn run_config_suggestions() {
    use idecraft_plugins::{RunConfigContext, run_types, uninstall};
    let p = sample("runconfig");
    assert_eq!(p.manifest.permissions, [Permission::SuggestCommands]);
    assert_eq!(p.manifest.actions[0].kind, ActionKind::RunConfig);
    assert!(
        action_list(std::slice::from_ref(&p)).is_empty(),
        "no command"
    );
    assert_eq!(contributions(&p.manifest), ["run configurations"]);
    let context = RunConfigContext {
        project: "demo".into(),
        active: None,
    };
    let (commands, problems) = p
        .run_config_suggestions("tasks", &context, Access::default())
        .unwrap();
    let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Say hello", "Print greeting"]);
    assert_eq!(commands[0].program, "echo");
    assert_eq!(commands[0].args, ["hello from", "${workspace}"]);
    assert_eq!(commands[0].cwd, ".");
    assert_eq!(commands[1].cwd, "src");
    assert_eq!(commands[1].env["GREETING"], "hello from a plugin");
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].contains("Hidden shell") && problems[0].contains("interpreter"));
    assert!(problems[1].contains("Escape") && problems[1].contains("folder"));

    // Through the plugins folder: listed, but run only with the grant.
    let base = std::env::temp_dir().join(format!("idecraft-plugin-runcfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (dir, root) = (base.join("plugins"), base.join("demo"));
    std::fs::create_dir_all(&root).unwrap();
    install(&dir, &samples().join("runconfig.wasm")).unwrap();
    let plugins = idecraft_plugins::load_dir(&dir).0;
    let types = run_types(&plugins, Some(&dir), &root, None);
    assert_eq!(types.len(), 1);
    assert_eq!(
        (types[0].plugin.as_str(), types[0].kind.as_str()),
        ("runconfig", "tasks")
    );
    assert!(!types[0].granted && types[0].commands.is_empty());
    set_granted(&dir, "runconfig", Permission::SuggestCommands, true).unwrap();
    let types = run_types(&plugins, Some(&dir), &root, None);
    assert!(types[0].granted);
    assert_eq!(types[0].commands.len(), 2);
    assert_eq!(
        types[0].config_name(&types[0].commands[0]),
        "Sample Tasks: Say hello"
    );
    let config = types[0].run_config(&types[0].commands[1]);
    assert_eq!(config.command, ["printenv", "GREETING"]);
    assert_eq!(config.cwd.as_deref(), Some("src"));
    assert!(config.verbatim);
    assert_eq!(types[0].allowance(&types[0].commands[1]).cwd, "src");

    // SPEC-0276 AK-9 (v3): a suggestion whose folder is reached through a symbolic link is
    // dropped and named, whether the link leads out of the project or stays inside.
    #[cfg(unix)]
    {
        std::fs::create_dir_all(base.join("elsewhere")).unwrap();
        std::fs::create_dir_all(root.join("real")).unwrap();
        for target in [base.join("elsewhere"), root.join("real")] {
            std::os::unix::fs::symlink(&target, root.join("src")).unwrap();
            let types = run_types(&plugins, Some(&dir), &root, None);
            let names: Vec<&str> = types[0].commands.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, ["Say hello"]);
            assert!(
                types[0]
                    .problems
                    .iter()
                    .any(|p| p
                        == "Print greeting: the folder `src` is reached through a symbolic link"),
                "{:?}",
                types[0].problems
            );
            std::fs::remove_file(root.join("src")).unwrap();
        }
        // A real folder of the project is taken.
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert_eq!(
            run_types(&plugins, Some(&dir), &root, None)[0]
                .commands
                .len(),
            2
        );
    }

    // An update that no longer asks for the permission drops the grant; removal too.
    let source = std::fs::read_to_string(samples().join("runconfig.wat")).unwrap();
    let without = source
        .replace(r#"\"version\":\"1.0\""#, r#"\"version\":\"1.1\""#)
        .replace(r#"\"permissions\":[\"suggest-commands\"],"#, "")
        .replace(
            r#",\"actions\":[{\"id\":\"tasks\",\"title\":\"Tasks\",\"kind\":\"run-config\"}]"#,
            r#",\"actions\":[]"#,
        );
    let file = base.join("v1.1/runconfig.wasm");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, wat::parse_str(&without).unwrap()).unwrap();
    install(&dir, &file).unwrap();
    assert!(!granted(&dir, "runconfig", Permission::SuggestCommands));
    install(&dir, &samples().join("runconfig.wasm")).unwrap();
    assert!(
        !granted(&dir, "runconfig", Permission::SuggestCommands),
        "never granted by updating"
    );
    set_granted(&dir, "runconfig", Permission::SuggestCommands, true).unwrap();
    uninstall(&dir, "runconfig").unwrap();
    assert!(!granted(&dir, "runconfig", Permission::SuggestCommands));
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0276 AK-1: a `run-config` action without `suggest-commands` is refused on loading; another
/// kind's runner refuses the action.
#[test]
fn run_config_needs_the_permission() {
    let source = std::fs::read_to_string(samples().join("runconfig.wat")).unwrap();
    let without = source.replace(r#"\"permissions\":[\"suggest-commands\"],"#, "");
    let wasm = wat::parse_str(&without).unwrap();
    let e = Plugin::from_bytes(Path::new("runconfig.wasm"), &wasm).unwrap_err();
    assert!(e.to_string().contains("suggest-commands"), "{e}");
    let p = sample("runconfig");
    assert!(
        p.run_panel(
            "tasks",
            &PanelContext {
                project: "p".into(),
                active: None
            },
            Access::default()
        )
        .is_err()
    );
    assert!(
        sample("panel")
            .run_config_suggestions(
                "rows",
                &PanelContext {
                    project: "p".into(),
                    active: None
                },
                Access::default()
            )
            .is_err()
    );
}

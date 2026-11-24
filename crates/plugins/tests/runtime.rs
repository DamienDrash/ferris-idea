//! SPEC-0104 AK-1: loading plugins, running actions, and the sandbox's limits.

use std::path::Path;

use ferris_idea_plugins::{
    Access, ActionKind, EditorContext, FILE_AUDIT, FILE_NOT_READABLE, FILE_SECRET, FILE_TOO_LARGE,
    MAX_LOGGED, Permission, Plugin, PluginError, action_id, file_refusals, granted, install,
    installed, load_dir, openable, set_enabled, set_granted, uninstall,
};

/// A plugin written in the WebAssembly text format: `upper` turns ASCII letters into capitals,
/// `count` reports the length, `spin` never ends.
const UPPER: &str = r#"
(module
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 4096))
  (data (i32.const 16) "{\"name\":\"Upper\",\"version\":\"1.0\",\"actions\":[{\"id\":\"upper\",\"title\":\"Upper Case\"},{\"id\":\"count\",\"title\":\"Count\",\"kind\":\"message\"},{\"id\":\"spin\",\"title\":\"Spin\"}]}")
  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const 157)))
  (func (export "ferris_idea_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    (local $i i32) (local $c i32)
    ;; the action's first letter: u(pper), c(ount), s(pin)
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 115))
      (then (loop $forever (br $forever))))
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 99))
      (then
        ;; "n" plus the length as one digit character is enough for the test
        (i32.store8 (i32.const 8) (i32.add (i32.const 48) (local.get $l)))
        (return (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 1)))))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $l)))
        (local.set $c (i32.load8_u (i32.add (local.get $p) (local.get $i))))
        (if (i32.and (i32.ge_u (local.get $c) (i32.const 97)) (i32.le_u (local.get $c) (i32.const 122)))
          (then (local.set $c (i32.sub (local.get $c) (i32.const 32)))))
        (i32.store8 (i32.add (local.get $p) (local.get $i)) (local.get $c))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (i64.or (i64.shl (i64.extend_i32_u (local.get $p)) (i64.const 32)) (i64.extend_i32_u (local.get $l))))
)
"#;

fn upper() -> Plugin {
    let wasm = wat::parse_str(UPPER).unwrap();
    Plugin::from_bytes(Path::new("upper.wasm"), &wasm).unwrap()
}

#[test]
fn manifest_and_actions() {
    let p = upper();
    assert_eq!(p.manifest.name, "Upper");
    assert_eq!(p.manifest.actions.len(), 3);
    assert_eq!(p.manifest.actions[1].kind, ActionKind::Message);
    assert_eq!(action_id(&p, "upper"), "Plugin.upper.upper");
    assert_eq!(p.run("upper", "hello, wörld").unwrap(), "HELLO, WöRLD");
    assert_eq!(p.run("count", "abc").unwrap(), "3");
    assert!(matches!(
        p.run("missing", ""),
        Err(PluginError::UnknownAction(..))
    ));
}

#[test]
fn the_sandbox_stops_endless_and_refuses_imports() {
    let p = upper();
    let err = p
        .run_with_fuel("spin", "", 1_000_000)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ran too long"), "{err}");
    // A module that wants a host function does not load: there are none.
    let importing =
        wat::parse_str(r#"(module (import "env" "open" (func)) (memory (export "memory") 1))"#)
            .unwrap();
    let err = Plugin::from_bytes(Path::new("bad.wasm"), &importing).unwrap_err();
    assert!(matches!(err, PluginError::Invalid(..)), "{err}");
}

#[test]
fn a_folder_of_plugins_with_a_broken_one() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-plugins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("upper.wasm"), wat::parse_str(UPPER).unwrap()).unwrap();
    std::fs::write(dir.join("broken.wasm"), b"not wasm").unwrap();
    std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
    let (plugins, problems) = load_dir(&dir);
    assert_eq!(plugins.len(), 1);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].starts_with("broken.wasm"), "{problems:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0126 AK-1: install from a file, disable, enable, replace, remove.
#[test]
fn managing_installed_plugins() {
    let base =
        std::env::temp_dir().join(format!("ferris-idea-plugin-admin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (source, dir) = (base.join("download"), base.join("plugins"));
    std::fs::create_dir_all(&source).unwrap();
    let module = source.join("upper.wasm");
    std::fs::write(&module, wat::parse_str(UPPER).unwrap()).unwrap();
    std::fs::write(source.join("junk.wasm"), b"not wasm").unwrap();
    std::fs::write(source.join("upper.txt"), wat::parse_str(UPPER).unwrap()).unwrap();

    assert!(install(&dir, &source.join("junk.wasm")).is_err());
    assert!(install(&dir, &source.join("upper.txt")).is_err());
    assert!(
        !dir.exists() || installed(&dir).is_empty(),
        "nothing broken lands"
    );
    assert_eq!(install(&dir, &module).unwrap().name, "Upper");
    let list = installed(&dir);
    assert_eq!(list.len(), 1);
    assert!(list[0].enabled && list[0].problem.is_none());
    assert_eq!(list[0].file, "upper");

    set_enabled(&dir, "upper", false).unwrap();
    assert!(
        load_dir(&dir).0.is_empty(),
        "a disabled plugin does not load"
    );
    assert!(!installed(&dir)[0].enabled);
    // Installing again replaces the disabled copy with an enabled one.
    install(&dir, &module).unwrap();
    assert_eq!(installed(&dir).len(), 1);
    assert!(installed(&dir)[0].enabled);
    set_enabled(&dir, "upper", true).unwrap();
    assert_eq!(load_dir(&dir).0.len(), 1);

    std::fs::write(dir.join("broken.wasm"), b"x").unwrap();
    let list = installed(&dir);
    assert!(list[0].problem.is_some() && list[0].manifest.is_none());

    uninstall(&dir, "upper").unwrap();
    uninstall(&dir, "broken").unwrap();
    assert!(installed(&dir).is_empty());
    assert!(uninstall(&dir, "upper").is_err());
    assert!(set_enabled(&dir, "upper", true).is_err());
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0156: a plugin with an `edit` action and actions that read the project through the host.
const PROJECT: &str = r#"
(module
  (import "ferris-idea" "list_files" (func $list (result i64)))
  (import "ferris-idea" "read_file" (func $read (param i32 i32) (result i64)))
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 8192))
  (data (i32.const 16) "{\"name\":\"Project\",\"version\":\"1.0\",\"permissions\":[\"project-read\"],\"actions\":[{\"id\":\"wrap\",\"title\":\"Comment\",\"kind\":\"edit\"},{\"id\":\"readme\",\"title\":\"Readme\",\"kind\":\"message\"},{\"id\":\"list\",\"title\":\"List\",\"kind\":\"message\"},{\"id\":\"escape\",\"title\":\"Escape\",\"kind\":\"message\"}]}")
  (data (i32.const 1024) "{\"edits\":[{\"start\":0,\"end\":0,\"text\":\"// \"}],\"select\":[0,3],\"message\":\"commented\"}")
  (data (i32.const 2048) "README.md")
  (data (i32.const 2064) "../outside.txt")
  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const 269)))
  (func (export "ferris_idea_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    ;; w(rap), r(eadme), l(ist), e(scape)
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 119))
      (then (return (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 81)))))
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 114))
      (then (return (call $read (i32.const 2048) (i32.const 9)))))
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 108))
      (then (return (call $list))))
    (call $read (i32.const 2064) (i32.const 14)))
)
"#;

fn project_plugin() -> Plugin {
    let wasm = wat::parse_str(PROJECT).unwrap();
    Plugin::from_bytes(Path::new("project.wasm"), &wasm).unwrap()
}

#[test]
fn edit_actions_get_the_context_and_answer_with_edits() {
    let p = project_plugin();
    assert_eq!(p.manifest.permissions, [Permission::ProjectRead]);
    assert_eq!(p.manifest.actions[0].kind, ActionKind::Edit);
    let context = EditorContext {
        path: "a.rs".into(),
        language: Some("rust".into()),
        text: "fn a() {}".into(),
        selection_start: 0,
        selection_end: 0,
        line: 0,
        column: 0,
    };
    let result = p.run_edit("wrap", &context, Access::default()).unwrap();
    assert_eq!(result.edits[0].text, "// ");
    assert_eq!(result.select, Some([0, 3]));
    assert_eq!(result.message.as_deref(), Some("commented"));
}

#[test]
fn project_access_needs_the_grant_and_stays_in_the_project() {
    let p = project_plugin();
    let base = std::env::temp_dir().join(format!("ferris-idea-plugin-api-{}", std::process::id()));
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("README.md"), "# Hello").unwrap();
    std::fs::write(base.join("outside.txt"), "secret").unwrap();
    let fuel = ferris_idea_plugins::FUEL;
    let refused = Access {
        root: Some(root.clone()),
        project_read: false,
        network: None,
        log: None,
    };
    let denied = p
        .run_in("readme", "", fuel, refused)
        .unwrap_err()
        .to_string();
    assert!(denied.contains("error -1"), "{denied}");
    let access = Access {
        root: Some(root.clone()),
        project_read: true,
        network: None,
        log: None,
    };
    assert_eq!(
        p.run_in("readme", "", fuel, access.clone()).unwrap(),
        "# Hello"
    );
    let listed = p.run_in("list", "", fuel, access.clone()).unwrap();
    assert_eq!(listed, "[\"README.md\"]");
    // SPEC-0156/AK-5 (v2): only the project's own sources are listed, not an environment,
    // installed dependencies or a file an ignore rule matches.
    for (rel, text) in [
        (".venv/lib/python3.12/site-packages/x.py", "x = 1\n"),
        ("node_modules/y.js", "y\n"),
        (".gitignore", "local.env\n"),
        ("local.env", "KEY=value\n"),
        ("src/main.py", "print(1)\n"),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let listed = p.run_in("list", "", fuel, access.clone()).unwrap();
    assert_eq!(listed, "[\".gitignore\",\"README.md\",\"src/main.py\"]");
    let escape = p
        .run_in("escape", "", fuel, access)
        .unwrap_err()
        .to_string();
    assert!(escape.contains("error -2"), "{escape}");
    // Grants are kept per plugin file in the plugins folder.
    let dir = base.join("plugins");
    assert!(!granted(&dir, "project", Permission::ProjectRead));
    set_granted(&dir, "project", Permission::ProjectRead, true).unwrap();
    assert!(granted(&dir, "project", Permission::ProjectRead));
    set_granted(&dir, "project", Permission::ProjectRead, false).unwrap();
    assert!(!granted(&dir, "project", Permission::ProjectRead));
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0156 v3: a plugin whose actions hand their input to `read_file` as the path: `read` once,
/// `many` thirty times before.
fn reader_plugin() -> Plugin {
    let manifest = r#"{"name":"Reader","permissions":["project-read"],"actions":[{"id":"read","title":"Read","kind":"message"},{"id":"many","title":"Many","kind":"message"}]}"#;
    let wat = format!(
        r#"
(module
  (import "ferris-idea" "read_file" (func $read (param i32 i32) (result i64)))
  (memory (export "memory") 200)
  (global $next (mut i32) (i32.const 8192))
  (data (i32.const 16) "{data}")
  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const {len})))
  (func (export "ferris_idea_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    (local $i i32)
    ;; m(any)
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 109))
      (then
        (loop $again
          (drop (call $read (local.get $p) (local.get $l)))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br_if $again (i32.lt_u (local.get $i) (i32.const 30))))))
    (call $read (local.get $p) (local.get $l)))
)
"#,
        data = manifest.replace('"', "\\\""),
        len = manifest.len(),
    );
    let wasm = wat::parse_str(&wat).unwrap();
    Plugin::from_bytes(Path::new("reader.wasm"), &wasm).unwrap()
}

/// A project with own sources and everything a plugin must not get, in a fresh folder: the
/// folder above the project and the project.
fn guarded_project(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let base =
        std::env::temp_dir().join(format!("ferris-idea-plugin-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("project");
    for (rel, text) in [
        ("project/src/main.rs", "fn main() {}\n"),
        ("project/.gitignore", "local.txt\nbuild/\n"),
        ("project/local.txt", "CONTENT-IGNORED\n"),
        ("project/.env", "TOKEN=CONTENT-ENV\n"),
        ("project/config/prod.pem", "CONTENT-PEM\n"),
        (
            "project/.git/config",
            "[remote \"origin\"]\n\turl = https://user:CONTENT-GIT@example.com/x.git\n",
        ),
        (
            "project/.ferris-idea/state.json",
            "{\"x\":\"CONTENT-IDE\"}\n",
        ),
        ("project/node_modules/x.js", "CONTENT-DEP\n"),
        ("outside.txt", "CONTENT-OUTSIDE\n"),
        ("elsewhere/o.txt", "CONTENT-ELSEWHERE\n"),
    ] {
        let path = base.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    (base, root)
}

/// SPEC-0156/AK-6 (v3): `read_file` gives an own source and nothing else; what it refuses is
/// logged without the file's content, and a plugin cannot tell "not yours" from "not there".
#[test]
fn read_file_takes_only_own_sources_and_logs_refusals() {
    let p = reader_plugin();
    let (base, root) = guarded_project("read");
    let dir = base.join("plugins");
    let fuel = ferris_idea_plugins::FUEL;
    let access = Access {
        root: Some(root.clone()),
        project_read: true,
        network: None,
        log: Some(dir.join(FILE_AUDIT)),
    };
    let read = |path: &str| p.run_in("read", path, fuel, access.clone());
    let code = |path: &str| {
        let error = read(path).unwrap_err().to_string();
        let at = error.rfind("error ").expect(&error) + "error ".len();
        error[at..].parse::<i64>().expect(&error)
    };
    assert_eq!(read("src/main.rs").unwrap(), "fn main() {}\n");

    let outside = base.join("outside.txt");
    let mut refused: Vec<(String, i64, &str)> = vec![
        (".env".into(), FILE_SECRET, "environment file (deny list)"),
        (
            "config/prod.pem".into(),
            FILE_SECRET,
            "private key or key store (deny list)",
        ),
        (
            "local.txt".into(),
            FILE_NOT_READABLE,
            "ignored by .gitignore",
        ),
        (
            ".git/config".into(),
            FILE_NOT_READABLE,
            "version control or IDE data",
        ),
        (
            ".ferris-idea/state.json".into(),
            FILE_NOT_READABLE,
            "version control or IDE data",
        ),
        (
            "node_modules/x.js".into(),
            FILE_NOT_READABLE,
            "in installed dependencies",
        ),
        (
            "../outside.txt".into(),
            FILE_NOT_READABLE,
            "not a relative path of plain names",
        ),
        (
            outside.to_string_lossy().into_owned(),
            FILE_NOT_READABLE,
            "not a relative path of plain names",
        ),
        ("src".into(), FILE_NOT_READABLE, "not a regular file"),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(base.join("elsewhere"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("src/main.rs"), root.join("alias.rs")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("out.txt")).unwrap();
        for path in ["link/o.txt", "alias.rs", "out.txt"] {
            refused.push((path.into(), FILE_NOT_READABLE, "behind a symbolic link"));
        }
    }
    // A disk that ignores case reaches the file under another spelling; the rules do not.
    if root.join("SRC/MAIN.RS").exists() {
        refused.push((
            "SRC/MAIN.RS".into(),
            FILE_NOT_READABLE,
            "another spelling of the file's name",
        ));
        // Since SPEC-0148 v5 the folder is judged by its name on disk.
        refused.push((
            "Node_Modules/x.js".into(),
            FILE_NOT_READABLE,
            "in installed dependencies",
        ));
    }
    for (path, answer, _) in &refused {
        assert_eq!(code(path), *answer, "{path}");
    }
    // Not there: the same answer as "not yours", and nothing to log.
    assert_eq!(code("src/missing.rs"), FILE_NOT_READABLE);
    // The same answers where the file does not exist at all.
    assert_eq!(code("other/.env"), FILE_SECRET);
    assert_eq!(code(".git/nothing"), FILE_NOT_READABLE);
    assert_eq!(code("build/nothing.txt"), FILE_NOT_READABLE);
    refused.push((
        "other/.env".into(),
        FILE_SECRET,
        "environment file (deny list)",
    ));
    refused.push((
        ".git/nothing".into(),
        FILE_NOT_READABLE,
        "version control or IDE data",
    ));
    refused.push((
        "build/nothing.txt".into(),
        FILE_NOT_READABLE,
        "build output",
    ));

    let logged = file_refusals(&dir, Some("reader"), 1000);
    let lines: Vec<(&str, &str, &str)> = logged
        .iter()
        .map(|r| (r.function.as_str(), r.path.as_str(), r.reason.as_str()))
        .collect();
    let expected: Vec<(&str, &str, &str)> = refused
        .iter()
        .map(|(path, _, reason)| ("read_file", path.as_str(), *reason))
        .collect();
    assert_eq!(lines, expected);
    assert!(logged.iter().all(|r| !r.time.is_empty()));
    let raw = std::fs::read_to_string(dir.join(FILE_AUDIT)).unwrap();
    assert!(!raw.contains("CONTENT-"), "{raw}");

    // Too large: only for a file the plugin may read.
    std::fs::write(
        root.join("src/big.txt"),
        vec![b'x'; ferris_idea_plugins::MAX_TEXT + 1],
    )
    .unwrap();
    assert_eq!(code("src/big.txt"), FILE_TOO_LARGE);

    // One call logs at most MAX_LOGGED refusals and one closing line.
    let before = file_refusals(&dir, None, 1000).len();
    let many = p
        .run_in("many", ".env", fuel, access.clone())
        .unwrap_err()
        .to_string();
    assert!(many.contains("error -5"), "{many}");
    let after = file_refusals(&dir, None, 1000);
    assert_eq!(after.len(), before + MAX_LOGGED + 1);
    let last = after.last().unwrap();
    assert!(last.reason.contains("not logged"), "{last:?}");
    assert_eq!(last.path, "");

    // Without a log nothing is written.
    let _ = std::fs::remove_dir_all(&dir);
    let silent = Access {
        log: None,
        ..access.clone()
    };
    assert!(p.run_in("read", ".env", fuel, silent).is_err());
    assert!(!dir.exists());
    // Without the grant nothing is looked at, and the attempt is logged.
    let no_grant = Access {
        project_read: false,
        ..access
    };
    let denied = p
        .run_in("read", "src/main.rs", fuel, no_grant)
        .unwrap_err()
        .to_string();
    assert!(denied.contains("error -1"), "{denied}");
    let logged = file_refusals(&dir, Some("reader"), 10);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].path, "src/main.rs");
    assert_eq!(logged[0].reason, "reading the project is not granted");
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0156/AK-8 (v3): a file a plugin asks to open is one it could read.
#[test]
fn open_requests_follow_the_read_rule() {
    let (base, root) = guarded_project("open");
    let dir = base.join("plugins");
    let log = dir.join(FILE_AUDIT);
    assert_eq!(
        openable(&root, "src/main.rs", Some(&log), "reader"),
        Some(root.join("src/main.rs"))
    );
    for path in ["../outside.txt", ".env", "local.txt", ".git/config"] {
        assert_eq!(openable(&root, path, Some(&log), "reader"), None, "{path}");
    }
    assert_eq!(
        openable(&root, "src/missing.rs", Some(&log), "reader"),
        None
    );
    let logged: Vec<(String, String)> = file_refusals(&dir, Some("reader"), 100)
        .into_iter()
        .map(|r| (r.function, r.path))
        .collect();
    let expected: Vec<(String, String)> = ["../outside.txt", ".env", "local.txt", ".git/config"]
        .iter()
        .map(|p| ("open".to_owned(), (*p).to_owned()))
        .collect();
    assert_eq!(logged, expected);
    let _ = std::fs::remove_dir_all(&base);
}

/// SPEC-0166: an `inspect` action: a fixed finding with a fix on `.txt` files, and one whose
/// finding lies outside the text.
const TODO: &str = r#"
(module
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 8192))
  (data (i32.const 16) "{\"name\":\"Todo\",\"version\":\"1.0\",\"actions\":[{\"id\":\"todo\",\"title\":\"TODO check\",\"kind\":\"inspect\",\"files\":[\"txt\"]},{\"id\":\"bad\",\"title\":\"Bad\",\"kind\":\"inspect\"}]}")
  (data (i32.const 1024) "{\"findings\":[{\"start\":0,\"end\":4,\"message\":\"TODO left\",\"fix\":{\"title\":\"Remove TODO\",\"edits\":[{\"start\":0,\"end\":5,\"text\":\"\"}]}}]}")
  (data (i32.const 2048) "{\"findings\":[{\"start\":0,\"end\":99999,\"message\":\"out\"}]}")
  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const 155)))
  (func (export "ferris_idea_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    ;; t(odo) or b(ad)
    (if (i32.eq (i32.load8_u (local.get $a)) (i32.const 98))
      (then (return (i64.or (i64.shl (i64.const 2048) (i64.const 32)) (i64.const 54)))))
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 126)))
)
"#;

#[test]
fn inspect_actions_report_checked_findings() {
    use ferris_idea_plugins::{InspectContext, action_list};
    let plugin =
        Plugin::from_bytes(Path::new("todo.wasm"), &wat::parse_str(TODO).unwrap()).unwrap();
    let todo = &plugin.manifest.actions[0];
    assert_eq!(todo.kind, ActionKind::Inspect);
    assert!(todo.inspects(Path::new("notes.txt")));
    assert!(!todo.inspects(Path::new("main.rs")));
    assert!(
        plugin.manifest.actions[1].inspects(Path::new("main.rs")),
        "no files: all"
    );
    assert!(
        action_list(std::slice::from_ref(&plugin)).is_empty(),
        "inspections are no commands"
    );
    let ctx = InspectContext {
        path: "notes.txt".into(),
        language: None,
        text: "TODO write\n".into(),
    };
    let found = plugin.run_inspect("todo", &ctx, Access::default()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].start, found[0].end), (0, 4));
    assert_eq!(found[0].level, ferris_idea_plugins::FindingLevel::Warning);
    assert_eq!(found[0].fix.as_ref().unwrap().title, "Remove TODO");
    let err = plugin
        .run_inspect("bad", &ctx, Access::default())
        .unwrap_err();
    assert!(err.to_string().contains("outside the text"), "{err}");
    // A fix edit beyond a short text is refused too.
    let short = InspectContext {
        text: "TODO".into(),
        ..ctx
    };
    assert!(
        plugin
            .run_inspect("todo", &short, Access::default())
            .is_err()
    );
}

/// The module above as bytes, for the window self-test (`cargo test -p ferris-idea-plugins --test
/// runtime -- --ignored --nocapture print_todo_bytes`).
#[test]
#[ignore]
fn print_todo_bytes() {
    let bytes = wat::parse_str(TODO).unwrap();
    for chunk in bytes.chunks(16) {
        let line: Vec<String> = chunk.iter().map(|b| format!("0x{b:02x}")).collect();
        println!("    {},", line.join(", "));
    }
}

/// SPEC-0156/AK-9 (v4): the files whose text the host does not hand to a plugin by itself are the
/// files `read_file` refuses; handing one over on purpose leaves a line in the log.
#[test]
fn guarded_files_are_what_read_file_refuses() {
    use ferris_idea_core::NotOwn;
    use ferris_idea_plugins::{Refusal, guarded, note_passed_on};
    let (base, root) = guarded_project("guard");
    let dir = base.join("plugins");
    let log = dir.join(FILE_AUDIT);
    for path in ["src/main.rs", "src/not-saved-yet.rs", ".gitignore"] {
        assert_eq!(guarded(&root, Path::new(path)), None, "{path}");
        assert_eq!(guarded(&root, &root.join(path)), None, "absolute {path}");
    }
    for (path, why) in [
        (".env", Refusal::Secret("environment file")),
        (
            "deploy/.ENV.production",
            Refusal::Secret("environment file"),
        ),
        (
            "config/prod.pem",
            Refusal::Secret("private key or key store"),
        ),
        ("local.txt", Refusal::NotOwn(NotOwn::Ignored)),
        (".git/config", Refusal::Data),
        (".ferris-idea/state.json", Refusal::Data),
        ("node_modules/x.js", Refusal::NotOwn(NotOwn::Dependencies)),
        ("src", Refusal::NotAFile),
    ] {
        assert_eq!(guarded(&root, Path::new(path)), Some(why), "{path}");
        assert_eq!(
            guarded(&root, &root.join(path)),
            Some(why),
            "absolute {path}"
        );
    }
    assert_eq!(
        guarded(&root, &base.join("outside.txt")),
        Some(Refusal::NotOwn(NotOwn::OutsideProject))
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(base.join("outside.txt"), root.join("out.txt")).unwrap();
        assert_eq!(
            guarded(&root, Path::new("out.txt")),
            Some(Refusal::NotOwn(NotOwn::SymbolicLink))
        );
    }
    // Asking leaves nothing in the log; handing over on purpose does.
    assert!(file_refusals(&dir, None, 10).is_empty());
    note_passed_on(
        Some(&log),
        "marker",
        "run_plugin_action",
        ".env",
        Refusal::Secret("environment file"),
        "an agent asked with allow_secret_file",
    );
    let logged = file_refusals(&dir, Some("marker"), 10);
    assert_eq!(logged.len(), 1);
    assert_eq!(
        (logged[0].function.as_str(), logged[0].path.as_str()),
        ("run_plugin_action", ".env")
    );
    assert_eq!(
        logged[0].reason,
        "environment file (deny list); handed over all the same: an agent asked with allow_secret_file"
    );
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(!text.contains("CONTENT"), "{text}");
    let _ = std::fs::remove_dir_all(&base);
}

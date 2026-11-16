//! SPEC-0114 AK-2: moving a C header and a TypeScript module in a real folder; the references and
//! the moved files' own relative references follow. Runs without language servers (clangd offers no
//! file-move edits; without a server the own rewriting still applies).

use std::time::Duration;

use idecraft_languages::move_file::{apply_move, plan_move};

#[tokio::test]
async fn moving_files_keeps_includes_and_imports_working() {
    let root = std::env::temp_dir().join(format!("idecraft-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    let write = |rel: &str, text: &str| std::fs::write(root.join(rel), text).unwrap();
    write("src/types.h", "typedef int num;\n");
    write("src/util.h", "#include \"types.h\"\nnum twice(num x);\n");
    write(
        "src/main.c",
        "#include \"util.h\"\nint main(void) { return twice(1); }\n",
    );
    write(
        "src/math.ts",
        "import { k } from './consts';\nexport const add = (a: number) => a + k;\n",
    );
    write("src/consts.ts", "export const k = 1;\n");
    write(
        "src/app.ts",
        "import { add } from './math';\nconsole.log(add(1));\n",
    );
    let wait = Duration::from_secs(5);

    let plan = plan_move(&root, &root.join("src/util.h"), "include", wait)
        .await
        .unwrap();
    assert_eq!(plan.to, root.join("include/util.h"));
    apply_move(&plan).unwrap();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap();
    assert!(!root.join("src/util.h").exists());
    assert_eq!(
        read("src/main.c").lines().next(),
        Some("#include \"../include/util.h\"")
    );
    assert_eq!(
        read("include/util.h").lines().next(),
        Some("#include \"../src/types.h\"")
    );

    let plan = plan_move(&root, &root.join("src/math.ts"), "src/lib", wait)
        .await
        .unwrap();
    apply_move(&plan).unwrap();
    assert_eq!(
        read("src/app.ts").lines().next(),
        Some("import { add } from './lib/math';")
    );
    assert_eq!(
        read("src/lib/math.ts").lines().next(),
        Some("import { k } from '../consts';")
    );

    assert!(
        plan_move(&root, &root.join("src/app.ts"), "../out", wait)
            .await
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0114 v2 AK-5 (hand test 41, D27): a Rust module moved into another folder still builds,
/// and moved back the `#[path]` goes again. Skipped without `cargo`.
#[tokio::test]
async fn moving_a_rust_module_keeps_the_crate_building() {
    let root = std::env::temp_dir().join(format!("idecraft-move-rs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    let write = |rel: &str, text: &str| std::fs::write(root.join(rel), text).unwrap();
    write(
        "Cargo.toml",
        "[package]\nname = \"geldtest\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    write(
        "src/main.rs",
        "mod geld;\n\nfn main() {\n    println!(\"{}\", geld::runde(2.5));\n}\n",
    );
    write(
        "src/geld.rs",
        "pub fn runde(x: f64) -> f64 {\n    x.round()\n}\n",
    );
    let check = || {
        std::process::Command::new("cargo")
            .args(["check", "--quiet", "--offline"])
            .current_dir(&root)
            .env_remove("RUSTUP_TOOLCHAIN")
            .output()
    };
    let Ok(first) = check() else {
        eprintln!("skipped: cargo not installed");
        return;
    };
    if !first.status.success() {
        eprintln!(
            "skipped: cargo check fails here: {}",
            String::from_utf8_lossy(&first.stderr)
        );
        return;
    }
    let wait = Duration::from_secs(5);
    let plan = plan_move(&root, &root.join("src/geld.rs"), "src/finanz", wait)
        .await
        .unwrap();
    apply_move(&plan).unwrap();
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(
        main.starts_with("#[path = \"finanz/geld.rs\"]\nmod geld;\n"),
        "{main}"
    );
    let built = check().unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    // And back: the attribute goes.
    let plan = plan_move(&root, &root.join("src/finanz/geld.rs"), "src", wait)
        .await
        .unwrap();
    apply_move(&plan).unwrap();
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(main.starts_with("mod geld;\n"), "{main}");
    assert!(check().unwrap().status.success());
    let _ = std::fs::remove_dir_all(&root);
}

fn git(dir: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// SPEC-0114/AK-7 (v3): a move changes only the project's own files; references in a virtual
/// environment, in an ignored file and in a folder the workspace listing skips anyway are left as
/// they are, the first two listed apart with the reason. A file Git tracks is the project's own
/// although an ignore rule matches it. Entries asked for by index are changed too.
#[tokio::test]
async fn a_move_changes_only_the_projects_own_files() {
    use idecraft_core::NotOwn;
    use idecraft_languages::move_file::include_outside;
    let root = std::env::temp_dir().join(format!("idecraft-move-own-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let write = |rel: &str, text: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap();
    let has_git = git(&root, &["init", "-q"]);
    write(".gitignore", "*.gen.c\n");
    write("src/util.h", "int twice(int x);\n");
    write("src/main.c", "#include \"util.h\"\n");
    write("src/local.gen.c", "#include \"util.h\"\n");
    write("src/kept.gen.c", "#include \"util.h\"\n");
    let venv = ".venv/lib/python3.12/site-packages/x.c";
    write(venv, "#include \"../../../../src/util.h\"\n");
    write("node_modules/y.c", "#include \"../src/util.h\"\n");
    let tracked = has_git && git(&root, &["add", "-f", "src/kept.gen.c"]);
    let wait = Duration::from_secs(5);
    let rel = |p: &std::path::Path| {
        p.strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    };

    let mut plan = plan_move(&root, &root.join("src/util.h"), "include", wait)
        .await
        .unwrap();
    let planned: Vec<String> = plan.files.iter().map(|f| rel(&f.path)).collect();
    let outside: Vec<(String, NotOwn)> =
        plan.outside.iter().map(|f| (rel(&f.path), f.why)).collect();
    let ignored = |name: &str| (name.to_owned(), NotOwn::Ignored);
    let environment = (venv.to_owned(), NotOwn::VirtualEnvironment);
    if tracked {
        assert_eq!(planned, ["src/kept.gen.c", "src/main.c"]);
        assert_eq!(outside, [environment, ignored("src/local.gen.c")]);
    } else {
        assert_eq!(planned, ["src/main.c"]);
        assert_eq!(
            outside,
            [
                environment,
                ignored("src/kept.gen.c"),
                ignored("src/local.gen.c")
            ]
        );
    }
    assert_eq!(plan.outside[0].reason, "in a virtual environment");
    // An unknown entry is refused; the ignored file is taken in on purpose.
    assert!(include_outside(&mut plan, &[7]).is_err());
    let last = plan.outside.len() - 1;
    include_outside(&mut plan, &[last]).unwrap();
    apply_move(&plan).unwrap();
    let moved = "#include \"../include/util.h\"\n";
    assert_eq!(read("src/main.c"), moved);
    assert_eq!(read("src/local.gen.c"), moved);
    assert_eq!(
        read("src/kept.gen.c"),
        if tracked {
            moved
        } else {
            "#include \"util.h\"\n"
        }
    );
    assert_eq!(read(venv), "#include \"../../../../src/util.h\"\n");
    assert_eq!(read("node_modules/y.c"), "#include \"../src/util.h\"\n");

    // The moved file is taken as it is, also out of an environment and into a named folder that
    // is not the project's own.
    write(".venv/w.c", "#include \"../src/main.c\"\n");
    let plan = plan_move(&root, &root.join(".venv/w.c"), "node_modules/z", wait)
        .await
        .unwrap();
    let planned: Vec<String> = plan.files.iter().map(|f| rel(&f.path)).collect();
    assert_eq!(planned, [".venv/w.c"], "{plan:?}");
    apply_move(&plan).unwrap();
    assert_eq!(
        read("node_modules/z/w.c"),
        "#include \"../../src/main.c\"\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}

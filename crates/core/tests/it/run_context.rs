//! SPEC-0097 AK-1: the configuration `RunClass` runs for the file in the editor.

use ferris_idea_core::{
    ProjectModel, RunKind, Workspace, context_config, read_project, test_method_config,
};

fn model(files: &[(&str, &str)]) -> (std::path::PathBuf, ProjectModel) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("ferris-idea-context-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (p, text) in files {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    let list = Workspace::open(&root).unwrap().files().unwrap();
    let m = read_project(&root, &list);
    (root, m)
}

#[test]
fn files_of_each_language() {
    let (_, empty) = model(&[("x.txt", "")]);
    let main = "public class Main { public static void main(String[] a) {} }";
    let c = context_config(&empty, "Main.java", main).unwrap();
    assert_eq!(
        (c.name.as_str(), c.command.clone()),
        ("Main", vec!["java".into(), "Main.java".into()])
    );
    assert!(
        context_config(&empty, "Util.java", "class Util {}").is_none(),
        "no main"
    );
    let kt = context_config(&empty, "app.kt", "fun main() {}").unwrap();
    assert!(kt.command[2].contains("kotlinc 'app.kt'"));
    // In a Gradle or Maven module the build tool compiles and `kotlin` runs `<File>Kt`
    // (SPEC-0132).
    let (_, gradle) = model(&[
        ("build.gradle.kts", "plugins { application }\n"),
        (
            "src/main/kotlin/demo/Main.kt",
            "package demo\n\nfun main() {}\n",
        ),
    ]);
    let g = context_config(
        &gradle,
        "src/main/kotlin/demo/Main.kt",
        "package demo\n\nfun main() {}\n",
    )
    .unwrap();
    assert!(
        g.command[2].ends_with("-q classes && kotlin -cp build/classes/kotlin/main demo.MainKt"),
        "{:?}",
        g.command
    );
    let (_, maven) = model(&[
        ("pom.xml", "<project><artifactId>a</artifactId></project>"),
        ("src/main/kotlin/App.kt", "fun main() {}\n"),
    ]);
    let m = context_config(&maven, "src/main/kotlin/App.kt", "fun main() {}\n").unwrap();
    assert_eq!(
        m.command[2],
        "mvn -q compile && kotlin -cp target/classes AppKt"
    );
    let py = context_config(&empty, "tools/run.py", "print(1)").unwrap();
    assert_eq!(py.command, ["python3", "tools/run.py"]);
    let pt = context_config(&empty, "test_calc.py", "def test_x(): pass").unwrap();
    assert_eq!(pt.kind, RunKind::Test);
    assert_eq!(pt.report_file.as_deref(), Some(".ferris-idea/pytest.xml"));
    assert_eq!(
        context_config(&empty, "a.js", "").unwrap().command,
        ["node", "a.js"]
    );
    assert!(context_config(&empty, "lib.go", "package lib").is_none());
    assert_eq!(
        context_config(&empty, "main.go", "package main")
            .unwrap()
            .command,
        ["go", "run", "main.go"]
    );
    assert!(context_config(&empty, "README.md", "").is_none());
}

#[test]
fn build_tool_modules() {
    let pom = "<project><modelVersion>4.0.0</modelVersion><groupId>g</groupId><artifactId>calc</artifactId><version>1</version></project>";
    let (_, maven) = model(&[("svc/pom.xml", pom)]);
    let main = "package calc;\npublic class App { public static void main(String[] a) {} }";
    let c = context_config(&maven, "svc/src/main/java/calc/App.java", main).unwrap();
    assert_eq!(c.cwd.as_deref(), Some("svc"));
    assert_eq!(c.command.last().unwrap(), "-Dexec.mainClass=calc.App");
    let test = "package calc;\nclass AppTest { @Test void t() {} }";
    let t = context_config(&maven, "svc/src/test/java/calc/AppTest.java", test).unwrap();
    assert_eq!(t.command, ["mvn", "test", "-Dtest=AppTest"]);
    assert_eq!(
        t.report_file.as_deref(),
        Some("svc/target/surefire-reports")
    );
    let (_, gradle) = model(&[
        ("settings.gradle.kts", "rootProject.name = \"calc\""),
        ("build.gradle.kts", "plugins { java }"),
        ("gradlew", "#!/bin/sh"),
    ]);
    let g = context_config(&gradle, "src/test/java/calc/AppTest.java", test).unwrap();
    assert_eq!(g.command, ["./gradlew", "test", "--tests", "calc.AppTest"]);
    // SPEC-0122 behaviour 1: one test method.
    let m = test_method_config(&maven, "svc/src/test/java/calc/AppTest.java", test, "t").unwrap();
    assert_eq!(
        (m.name.as_str(), m.command.clone()),
        (
            "AppTest.t",
            vec!["mvn".into(), "test".into(), "-Dtest=AppTest#t".into()]
        )
    );
    assert_eq!(m.kind, RunKind::Test);
    let gm = test_method_config(&gradle, "src/test/java/calc/AppTest.java", test, "t").unwrap();
    assert_eq!(
        gm.command,
        ["./gradlew", "test", "--tests", "calc.AppTest.t"]
    );
    assert!(test_method_config(&maven, "svc/src/main/java/calc/App.java", main, "main").is_none());
    let (_, cargo) = model(&[(
        "Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\n",
    )]);
    let r = context_config(&cargo, "src/main.rs", "fn main() {}").unwrap();
    assert_eq!(
        (r.name.as_str(), r.command.clone()),
        (
            "tool",
            vec!["cargo".into(), "run".into(), "--bin".into(), "tool".into()]
        )
    );
    let b = context_config(&cargo, "src/bin/extra.rs", "fn main() {}").unwrap();
    assert_eq!(b.command, ["cargo", "run", "--bin", "extra"]);
    // SPEC-0097 v3 AK-5 (behaviour 8): a binary in its own folder and the examples.
    let d = context_config(&cargo, "src/bin/serve/main.rs", "fn main() {}").unwrap();
    assert_eq!(
        (d.name.as_str(), d.command.clone()),
        (
            "serve",
            vec!["cargo".into(), "run".into(), "--bin".into(), "serve".into()]
        )
    );
    let e = context_config(&cargo, "examples/demo.rs", "fn main() {}").unwrap();
    assert_eq!(e.command, ["cargo", "run", "--example", "demo"]);
    let f = context_config(&cargo, "examples/big/main.rs", "fn main() {}").unwrap();
    assert_eq!(f.command, ["cargo", "run", "--example", "big"]);
    assert!(context_config(&cargo, "src/shapes.rs", "fn main() {}").is_none());
}

/// SPEC-0045 v3 AK-9: test files of JavaScript/TypeScript and Go run with a result tree.
#[test]
fn test_files_of_js_and_go() {
    let (_, web) = model(&[(
        "web/package.json",
        r#"{"name": "web", "devDependencies": {"vitest": "^2"}}"#,
    )]);
    let v = context_config(&web, "web/src/math.test.ts", "").unwrap();
    assert_eq!(v.name, "vitest math.test.ts");
    assert_eq!(v.cwd.as_deref(), Some("web"));
    assert_eq!(v.kind, RunKind::Test);
    assert_eq!(v.report.as_deref(), Some("jest"));
    assert_eq!(
        v.report_file.as_deref(),
        Some("web/.ferris-idea/vitest.json")
    );
    assert_eq!(v.command.last().unwrap(), "src/math.test.ts");
    // SPEC-0168 behaviour 3: TypeScript runs on Node's type stripping, JSX does not run.
    let ts = context_config(&web, "web/src/math.ts", "").unwrap();
    assert_eq!(ts.command, ["node", "src/math.ts"]);
    assert_eq!(ts.kind, RunKind::Run);
    assert!(context_config(&web, "web/src/App.tsx", "").is_none());
    assert!(context_config(&web, "web/src/env.d.ts", "").is_none());
    let (_, jest) = model(&[(
        "package.json",
        r#"{"name": "j", "devDependencies": {"jest": "^29"}}"#,
    )]);
    let j = context_config(&jest, "sum.spec.js", "").unwrap();
    assert_eq!(
        j.command,
        [
            "npx",
            "jest",
            "--json",
            "--outputFile=.ferris-idea/jest.json",
            "sum.spec.js"
        ]
    );
    let (_, empty) = model(&[("x.txt", "")]);
    let g = context_config(&empty, "pkg/calc_test.go", "package pkg").unwrap();
    assert_eq!(g.command, ["go", "test", "-json", "./pkg"]);
    assert_eq!(g.report.as_deref(), Some("gotest"));
}

/// SPEC-0045 v4 AK-13: the name filter escapes the name, keeps `.each` placeholders open and
/// separates a test's end from a suite.
#[test]
fn js_test_name_filters() {
    use ferris_idea_core::js_test_pattern;
    assert_eq!(js_test_pattern("math adds", false), "math adds$");
    assert_eq!(js_test_pattern("math", true), "math ");
    assert_eq!(js_test_pattern("a (b) [c]?", false), r"a \(b\) \[c\]\?$");
    assert_eq!(
        js_test_pattern("each %i of $name.x", false),
        "each .* of .*$"
    );
    assert_eq!(js_test_pattern("100%", false), "100%$");
}

/// SPEC-0045 v5 behaviour 13 (OBS-0034 R1): one test of pytest, Go and Rust, named after the test;
/// a Rust file's tests as a whole outside a test.
#[test]
fn single_tests_of_pytest_go_and_rust() {
    use ferris_idea_core::single_test_config;
    let (_, empty) = model(&[("x.txt", "")]);
    let py = "def test_total():\n    pass\n";
    let one =
        single_test_config(&empty, "tests/test_prices.py", py, "", Some("test_total")).unwrap();
    assert_eq!(
        one.command,
        [
            "python3",
            "-m",
            "pytest",
            "tests/test_prices.py::test_total",
            "--junitxml=.ferris-idea/pytest.xml"
        ]
    );
    assert_eq!(one.name, "test_prices.py::test_total");
    assert_eq!(one.kind, RunKind::Test);
    let class = single_test_config(&empty, "tests/test_prices.py", py, "TestCart", None).unwrap();
    assert_eq!(class.command[3], "tests/test_prices.py::TestCart");
    let method = single_test_config(
        &empty,
        "tests/test_prices.py",
        py,
        "TestCart",
        Some("test_empty"),
    )
    .unwrap();
    assert_eq!(
        method.command[3],
        "tests/test_prices.py::TestCart::test_empty"
    );
    assert!(single_test_config(&empty, "tools/run.py", py, "", Some("test_total")).is_none());

    let go = single_test_config(
        &empty,
        "shop/total_test.go",
        "package shop\n",
        "",
        Some("TestTotal"),
    )
    .unwrap();
    assert_eq!(
        go.command,
        ["go", "test", "-json", "-run", "^TestTotal$", "./shop"]
    );
    assert_eq!(go.name, "TestTotal");

    let (_, cargo) = model(&[(
        "Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )]);
    let lib = "pub fn a() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {}\n}\n";
    let rs = single_test_config(&cargo, "src/shop/cart.rs", lib, "tests", Some("adds")).unwrap();
    assert_eq!(
        rs.command,
        ["cargo", "test", "shop::cart::tests::adds", "--", "--exact"]
    );
    assert_eq!(rs.name, "shop::cart::tests::adds");
    assert_eq!(rs.report.as_deref(), Some("libtest"));
    let it = single_test_config(&cargo, "tests/it/cart.rs", lib, "", Some("adds")).unwrap();
    assert_eq!(
        it.command,
        [
            "cargo",
            "test",
            "--test",
            "it",
            "cart::adds",
            "--",
            "--exact"
        ]
    );
    let alone = single_test_config(&cargo, "tests/smoke.rs", lib, "", Some("starts")).unwrap();
    assert_eq!(
        alone.command,
        [
            "cargo", "test", "--test", "smoke", "starts", "--", "--exact"
        ]
    );
    // Outside a test: the file's tests.
    let file = context_config(&cargo, "src/shop/cart.rs", lib).unwrap();
    assert_eq!(file.command, ["cargo", "test", "shop::cart"]);
    assert_eq!(file.kind, RunKind::Test);
    let whole = context_config(&cargo, "tests/it/main.rs", "mod cart;\n").unwrap();
    assert_eq!(whole.command, ["cargo", "test", "--test", "it"]);
    assert_eq!(whole.name, "cargo test it");
    // Without a Cargo module nothing runs.
    assert!(single_test_config(&empty, "src/lib.rs", lib, "tests", Some("adds")).is_none());
}

const REPORT: &str = "package lab.stock;\n\npublic class Report {\n    public static void main(String[] args) {\n        System.out.println(\"Total: \" + Inventory.totalValue());\n    }\n}\n";
const INVENTORY: &str = "package lab.stock;\n\nclass Inventory {\n    static int totalValue() {\n        return 4 * 10;\n    }\n}\n";
const MODULES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"ProjectModuleManager\">\n    <modules>\n      <module fileurl=\"file://$PROJECT_DIR$/lab.iml\" filepath=\"$PROJECT_DIR$/lab.iml\" />\n    </modules>\n  </component>\n</project>\n";
const LAB_IML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<module type=\"JAVA_MODULE\" version=\"4\">\n  <component name=\"NewModuleRootManager\" inherit-compiler-output=\"true\">\n    <exclude-output />\n    <content url=\"file://$MODULE_DIR$\">\n      <sourceFolder url=\"file://$MODULE_DIR$/src/main/java\" isTestSource=\"false\" />\n    </content>\n    <orderEntry type=\"inheritedJdk\" />\n    <orderEntry type=\"sourceFolder\" forTests=\"false\" />\n    <orderEntry type=\"module-library\">\n      <library>\n        <CLASSES>\n          <root url=\"jar://$MODULE_DIR$/lib/util.jar!/\" />\n        </CLASSES>\n      </library>\n    </orderEntry>\n  </component>\n</module>\n";

/// The script of a `sh -c` configuration.
fn script(c: &ferris_idea_core::RunConfig) -> &str {
    assert_eq!(c.command[..2], ["sh", "-c"], "{:?}", c.command);
    &c.command[2]
}

/// SPEC-0097 v4 AK-7 (behaviour 10; window check 2026-11-18 gap RJ1): a class in a source folder
/// of a module without a build tool runs with the whole module compiled into
/// `out/production/<module>`; a file outside a source folder still runs alone.
#[test]
fn a_module_without_a_build_tool_is_compiled_before_the_run() {
    // Only an .iml file beside the sources, no .idea folder.
    let (root, m) = model(&[
        ("lab.iml", LAB_IML),
        ("src/main/java/lab/stock/Report.java", REPORT),
        ("src/main/java/lab/stock/Inventory.java", INVENTORY),
        (
            "scripts/Tool.java",
            "class Tool { public static void main(String[] a) {} }\n",
        ),
    ]);
    let c = ferris_idea_core::context_config_in(
        &root,
        &m,
        "src/main/java/lab/stock/Report.java",
        REPORT,
    )
    .unwrap();
    let s = script(&c);
    assert!(s.contains("find 'src/main/java' -name '*.java'"), "{s}");
    assert!(s.contains("javac -d 'out/production/lab'"), "{s}");
    assert!(
        s.ends_with("java -cp 'out/production/lab' 'lab.stock.Report'"),
        "{s}"
    );
    assert_eq!(c.kind, RunKind::Run);
    assert_eq!(c.cwd, None, "runs in the project folder");
    assert_eq!(c.name, "Report");
    // Outside a source folder: the JDK's source launcher, as before.
    let tool = ferris_idea_core::context_config_in(
        &root,
        &m,
        "scripts/Tool.java",
        "class Tool { public static void main(String[] a) {} }\n",
    )
    .unwrap();
    assert_eq!(tool.command, ["java", "scripts/Tool.java"]);
    // Without the project folder the path alone says it: the module is named after its folder,
    // here the project folder, which is unknown.
    let alone = context_config(&m, "src/main/java/lab/stock/Report.java", REPORT).unwrap();
    assert!(script(&alone).contains("javac -d 'out/production/main'"));
    // With .idea: the module's name, source folders and libraries come from there.
    std::fs::create_dir_all(root.join(".idea")).unwrap();
    std::fs::write(root.join(".idea/modules.xml"), MODULES).unwrap();
    let module =
        ferris_idea_core::java_module(Some(&root), "src/main/java/lab/stock/Report.java", REPORT)
            .unwrap();
    assert_eq!(module.name, "lab");
    assert_eq!(module.sources, ["src/main/java"]);
    assert_eq!(module.output, "out/production/lab");
    assert_eq!(module.classpath, ["lib/util.jar"]);
    let s = ferris_idea_core::java_module_command(&module, "lab.stock.Report");
    assert!(
        s[2].contains("javac -d 'out/production/lab' -cp 'lib/util.jar'"),
        "{}",
        s[2]
    );
    assert!(s[2].ends_with("java -cp 'out/production/lab:lib/util.jar' 'lab.stock.Report'"));
    assert!(ferris_idea_core::java_module(Some(&root), "scripts/Tool.java", "").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0097 v4 AK-7: the command really compiles and runs the two classes, and a compile error
/// stops it before the run with javac's message naming the file and line. Needs a JDK; without
/// `javac` on the path the test says so and passes.
#[test]
fn the_compiled_module_runs_and_a_compile_error_stops_it() {
    let has_javac = std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_javac {
        eprintln!("skipped: no javac on the path");
        return;
    }
    let (root, m) = model(&[
        (
            "lab.iml",
            LAB_IML.replace("lib/util.jar", "lib/none.jar").as_str(),
        ),
        ("src/main/java/lab/stock/Report.java", REPORT),
        ("src/main/java/lab/stock/Inventory.java", INVENTORY),
    ]);
    let run = |c: &ferris_idea_core::RunConfig| {
        std::process::Command::new(&c.command[0])
            .args(&c.command[1..])
            .current_dir(&root)
            .output()
            .unwrap()
    };
    let c = ferris_idea_core::context_config_in(
        &root,
        &m,
        "src/main/java/lab/stock/Report.java",
        REPORT,
    )
    .unwrap();
    let out = run(&c);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout.trim(), "Total: 40");
    assert!(
        root.join("out/production/lab/lab/stock/Inventory.class")
            .is_file()
    );
    // A compile error: no run, exit code not 0, javac names the file and line.
    std::fs::write(
        root.join("src/main/java/lab/stock/Inventory.java"),
        INVENTORY.replace("4 * 10", "4 * missing"),
    )
    .unwrap();
    let out = run(&c);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Total"));
    assert!(
        stderr.contains("src/main/java/lab/stock/Inventory.java:5: error"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0097 v4 behaviour 10: every word of the module command is quoted, and a source folder
/// that starts with `-` reaches `find` as a path, never as its expression.
#[test]
fn the_module_command_quotes_every_word() {
    let module = ferris_idea_core::JavaModule {
        name: "lab".into(),
        sources: vec!["-delete".into(), "src".into()],
        output: "out/production/lab".into(),
        classpath: vec![],
    };
    let cmd = ferris_idea_core::java_module_command(&module, "lab.stock.Report");
    let script = &cmd[2];
    assert!(script.contains("find './-delete' 'src' -name"), "{script}");
    assert!(
        script.ends_with("java -cp 'out/production/lab' 'lab.stock.Report'"),
        "{script}"
    );
}

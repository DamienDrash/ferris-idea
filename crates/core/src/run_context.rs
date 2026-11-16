//! The run configuration of the file in the editor (`SPEC-0097` behaviours 3–5): what `RunClass`
//! (⌃⇧R) and `DebugClass` (⌃⇧D) run for a Java, Kotlin, Python, JavaScript, Go or Rust file.

use std::path::Path;

use crate::project::{Module, ModuleKind, ProjectModel};
use crate::run::{RunConfig, RunKind};

/// The module whose folder contains `rel` most deeply (workspace-relative paths).
fn module_of<'a>(model: &'a ProjectModel, rel: &str) -> Option<&'a Module> {
    model
        .modules
        .iter()
        .filter(|m| m.dir == "." || rel.starts_with(&format!("{}/", m.dir)))
        .max_by_key(|m| if m.dir == "." { 0 } else { m.dir.len() })
}

/// The package declared at the top of a Java or Kotlin source, if any.
fn package(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|l| {
        let rest = l.strip_prefix("package ")?;
        Some(rest.trim_end_matches(';').trim().to_owned())
    })
}

fn has_main(text: &str, java: bool) -> bool {
    if java {
        text.contains("static void main(")
    } else {
        text.lines()
            .any(|l| l.trim_start().starts_with("fun main("))
    }
}

fn is_test_source(rel: &str, text: &str) -> bool {
    rel.contains("/src/test/") || rel.starts_with("src/test/") || text.contains("@Test")
}

fn config(name: String, command: Vec<String>, cwd: &str, kind: RunKind) -> RunConfig {
    RunConfig {
        name,
        command,
        cwd: (cwd != ".").then(|| cwd.to_owned()),
        env: Default::default(),
        kind,
        report: None,
        report_file: None,
        before: Vec::new(),
        timeout: None,
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: false,
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The configuration that runs only test method `method` of the test source `rel`, named
/// `Class.method` (`SPEC-0122` behaviour 1): Maven `-Dtest=Class#method`, Gradle
/// `--tests pkg.Class.method`. `None` when the file's tests cannot be run.
pub fn test_method_config(
    model: &ProjectModel,
    rel: &str,
    text: &str,
    method: &str,
) -> Option<RunConfig> {
    let mut c = context_config(model, rel, text)?;
    if c.kind != RunKind::Test {
        return None;
    }
    let class = c.name.clone();
    let mut changed = false;
    for i in 0..c.command.len() {
        if let Some(name) = c.command[i].strip_prefix("-Dtest=") {
            c.command[i] = format!("-Dtest={name}#{method}");
            changed = true;
        } else if c.command[i] == "--tests" && i + 1 < c.command.len() {
            c.command[i + 1] = format!("{}.{method}", c.command[i + 1]);
            changed = true;
            break;
        }
    }
    if !changed {
        return None;
    }
    c.name = format!("{class}.{method}");
    Some(c)
}

/// The extensions of JavaScript and TypeScript sources.
const JS_EXTENSIONS: [&str; 8] = ["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx"];

/// Whether `rel` is named like a test file of Vitest or Jest (`*.test.*`, `*.spec.*`; SPEC-0045 v3).
pub fn is_js_test_file(rel: &str) -> bool {
    let path = Path::new(rel);
    path.extension()
        .is_some_and(|e| JS_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
        && path.file_stem().is_some_and(|s| {
            let stem = s.to_string_lossy();
            stem.ends_with(".test") || stem.ends_with(".spec")
        })
}

/// The name filter (`-t`) of a test or suite (SPEC-0045 v4 behaviour 12): the runners match it as
/// a regular expression against `suite … test`. Placeholders of `.each` names match anything; a
/// test's name ends at the filter's end, a suite's is followed by its tests' names.
pub fn js_test_pattern(full_name: &str, suite: bool) -> String {
    let mut out = String::with_capacity(full_name.len() + 8);
    let mut chars = full_name.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('%', Some('s' | 'd' | 'i' | 'f' | 'j' | 'o' | 'O' | '#')) => {
                chars.next();
                out.push_str(".*");
            }
            ('$', Some(n)) if n.is_ascii_alphabetic() || n == '_' => {
                while chars
                    .peek()
                    .is_some_and(|n| n.is_ascii_alphanumeric() || matches!(n, '_' | '.'))
                {
                    chars.next();
                }
                out.push_str(".*");
            }
            _ => {
                if "\\^$.|?*+()[]{}".contains(c) {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    out.push_str(if suite { " " } else { "$" });
    out
}

/// The configuration that runs only the test or suite `full_name` of test file `rel`: the file's
/// own configuration with the runner's name filter, named after the test. `None` when `rel` does
/// not run with Vitest or Jest.
pub fn js_test_config(
    model: &ProjectModel,
    rel: &str,
    text: &str,
    full_name: &str,
    suite: bool,
) -> Option<RunConfig> {
    if !is_js_test_file(rel) {
        return None;
    }
    let mut c = context_config(model, rel, text)?;
    if c.kind != RunKind::Test {
        return None;
    }
    c.command.push("-t".to_owned());
    c.command.push(js_test_pattern(full_name, suite));
    c.name = format!(
        "{}: {full_name}",
        Path::new(rel).file_name()?.to_string_lossy()
    );
    Some(c)
}

/// Where a Rust source's tests are: the test target (`--test name`) for a file below the crate's
/// `tests/` folder, and the module path of the file in its crate (`shop::cart` for
/// `src/shop/cart.rs`, `cart` for `tests/it/cart.rs`). `rel` is relative to the crate folder.
fn rust_target(local: &str) -> (Option<String>, Vec<String>) {
    let parts: Vec<&str> = local.split('/').collect();
    let strip = |name: &str| name.trim_end_matches(".rs").to_owned();
    if parts.first() == Some(&"tests") && parts.len() >= 2 {
        if parts.len() == 2 {
            return (Some(strip(parts[1])), Vec::new());
        }
        let mut path: Vec<String> = parts[2..].iter().map(|p| strip(p)).collect();
        if path.last().is_some_and(|l| l == "main" || l == "mod") {
            path.pop();
        }
        return (Some(parts[1].to_owned()), path);
    }
    let below = parts
        .iter()
        .position(|p| *p == "src")
        .map_or(&parts[..], |i| &parts[i + 1..]);
    let mut path: Vec<String> = below.iter().map(|p| strip(p)).collect();
    if path
        .last()
        .is_some_and(|l| l == "lib" || l == "main" || l == "mod")
    {
        path.pop();
    }
    (None, path)
}

/// `cargo test` for the tests below `path` of a Rust source, or exactly the test `path` with
/// `exact` (`SPEC-0045` v5 behaviour 13).
fn cargo_test(local: &str, dir: &str, inner: &[&str], exact: bool, name: String) -> RunConfig {
    let (target, mut path) = rust_target(local);
    path.extend(
        inner
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string()),
    );
    let mut command = words(&["cargo", "test"]);
    if let Some(t) = target {
        command.extend(words(&["--test", &t]));
    }
    let filter = path.join("::");
    if !filter.is_empty() {
        command.push(filter);
    }
    if exact {
        command.extend(words(&["--", "--exact"]));
    }
    let mut c = config(name, command, dir, RunKind::Test);
    c.report = Some("libtest".into());
    c
}

/// The configuration that runs only the test `method` (or the pytest class `class` when there is
/// no method) of the pytest, Go or Rust test source `rel`, named after the test (`SPEC-0045` v5
/// behaviour 13, OBS-0034 R1): pytest `file::Class::name`, Go `-run '^Name$'`, Rust
/// `cargo test path::name -- --exact`. `class` is the pytest class or the Rust module path inside
/// the file, empty when there is none. `None` when the file's tests cannot be run this way.
pub fn single_test_config(
    model: &ProjectModel,
    rel: &str,
    text: &str,
    class: &str,
    method: Option<&str>,
) -> Option<RunConfig> {
    let path = Path::new(rel);
    let ext = path.extension()?.to_str()?;
    let module = module_of(model, rel);
    let dir = module.map_or(".", |m| m.dir.as_str());
    let local = if dir == "." {
        rel.to_owned()
    } else {
        rel.strip_prefix(&format!("{dir}/"))?.to_owned()
    };
    let file = path.file_name()?.to_string_lossy().into_owned();
    match ext {
        "py" => {
            let mut c = context_config(model, rel, text)?;
            if c.kind != RunKind::Test {
                return None;
            }
            let mut id: Vec<&str> = Vec::new();
            if !class.is_empty() {
                id.push(class);
            }
            id.extend(method);
            if id.is_empty() {
                return None;
            }
            let node = format!("{local}::{}", id.join("::"));
            let at = c.command.iter().position(|w| *w == local)?;
            c.command[at] = node;
            c.name = format!("{file}::{}", id.join("::"));
            Some(c)
        }
        "go" => {
            let method = method?;
            let mut c = context_config(model, rel, text)?;
            if c.kind != RunKind::Test {
                return None;
            }
            let at = c.command.iter().position(|w| w == "-json")?;
            c.command.insert(at + 1, "-run".into());
            c.command.insert(at + 2, format!("^{method}$"));
            c.name = method.to_owned();
            Some(c)
        }
        "rs" => {
            module.filter(|m| m.kind == ModuleKind::Cargo)?;
            let method = method?;
            let mut inner: Vec<&str> = class.split("::").collect();
            inner.push(method);
            let (_, mut full) = rust_target(&local);
            full.extend(
                inner
                    .iter()
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string()),
            );
            Some(cargo_test(&local, dir, &inner, true, full.join("::")))
        }
        _ => None,
    }
}

/// The configuration that runs `rel` (workspace-relative) with content `text`, or `None` when the
/// file cannot be run (behaviour 3). Test sources run their tests with a result tree.
pub fn context_config(model: &ProjectModel, rel: &str, text: &str) -> Option<RunConfig> {
    let path = Path::new(rel);
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let module = module_of(model, rel);
    let dir = module.map_or(".", |m| m.dir.as_str());
    let in_module = |m: &str| -> String {
        if dir == "." {
            m.to_owned()
        } else {
            format!("{dir}/{m}")
        }
    };
    // Paths in commands are relative to the module folder the command runs in.
    let local = if dir == "." {
        rel.to_owned()
    } else {
        rel.strip_prefix(&format!("{dir}/"))
            .unwrap_or(rel)
            .to_owned()
    };
    match ext.as_str() {
        "java" | "kt" => {
            let java = ext == "java";
            let class = match package(text) {
                Some(p) => format!("{p}.{stem}"),
                None => stem.clone(),
            };
            // Kotlin's top-level `main` lives in the class `<File>Kt`.
            let kotlin_main = format!("{class}Kt");
            let tool = module.map(|m| m.kind);
            if is_test_source(rel, text) {
                let mut c = match tool {
                    Some(ModuleKind::Maven) => {
                        let mut c = config(
                            stem.clone(),
                            words(&["mvn", "test", &format!("-Dtest={stem}")]),
                            dir,
                            RunKind::Test,
                        );
                        c.report_file = Some(in_module("target/surefire-reports"));
                        c
                    }
                    Some(ModuleKind::Gradle) => {
                        let runner = module
                            .and_then(|m| m.tasks.get("test"))
                            .and_then(|t| t.split_whitespace().next())
                            .unwrap_or("gradle")
                            .to_owned();
                        let mut c = config(
                            stem.clone(),
                            words(&[&runner, "test", "--tests", &class]),
                            dir,
                            RunKind::Test,
                        );
                        c.report_file = Some(in_module("build/test-results/test"));
                        c
                    }
                    _ => return None,
                };
                c.report = Some("junit".into());
                return Some(c);
            }
            if !has_main(text, java) {
                return None;
            }
            let command = match (tool, java) {
                (Some(ModuleKind::Maven), true) => words(&[
                    "mvn",
                    "-q",
                    "compile",
                    "exec:java",
                    &format!("-Dexec.mainClass={class}"),
                ]),
                // A single source file: the JDK's source launcher compiles and runs it.
                (_, true) => words(&["java", &local]),
                // Kotlin in a Gradle or Maven module: the build tool compiles, `kotlin` runs the
                // class; the debugger recognises this command (`SPEC-0132` behaviour 2).
                (Some(ModuleKind::Gradle), false) => {
                    let runner = module
                        .and_then(|m| m.tasks.get("build"))
                        .and_then(|t| t.split_whitespace().next())
                        .unwrap_or("gradle")
                        .to_owned();
                    words(&[
                        "sh",
                        "-c",
                        &format!(
                            "{runner} -q classes && kotlin -cp build/classes/kotlin/main {kotlin_main}"
                        ),
                    ])
                }
                (Some(ModuleKind::Maven), false) => words(&[
                    "sh",
                    "-c",
                    &format!("mvn -q compile && kotlin -cp target/classes {kotlin_main}"),
                ]),
                (_, false) => {
                    let jar = format!(".idecraft/run/{stem}.jar");
                    words(&[
                        "sh",
                        "-c",
                        &format!(
                            "kotlinc '{local}' -include-runtime -d '{jar}' 2>&1 && java -jar '{jar}'"
                        ),
                    ])
                }
            };
            Some(config(stem, command, dir, RunKind::Run))
        }
        "py" => {
            let name = path.file_name()?.to_string_lossy().into_owned();
            if stem.starts_with("test_") || stem.ends_with("_test") {
                let mut c = config(
                    name,
                    words(&[
                        "python3",
                        "-m",
                        "pytest",
                        &local,
                        "--junitxml=.idecraft/pytest.xml",
                    ]),
                    dir,
                    RunKind::Test,
                );
                c.report = Some("junit".into());
                c.report_file = Some(in_module(".idecraft/pytest.xml"));
                return Some(c);
            }
            Some(config(name, words(&["python3", &local]), dir, RunKind::Run))
        }
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" => {
            let name = path.file_name()?.to_string_lossy().into_owned();
            // A test file runs with the module's test runner and a result tree (SPEC-0045 v3).
            let test_file = is_js_test_file(rel);
            let uses = |n: &str| module.is_some_and(|m| m.dependencies.iter().any(|d| d.name == n));
            if test_file && (uses("vitest") || uses("jest")) {
                let (runner, report, mut command) = if uses("vitest") {
                    (
                        "vitest",
                        ".idecraft/vitest.json",
                        words(&["npx", "vitest", "run", "--reporter=json"]),
                    )
                } else {
                    (
                        "jest",
                        ".idecraft/jest.json",
                        words(&["npx", "jest", "--json"]),
                    )
                };
                command.push(format!("--outputFile={report}"));
                command.push(local.clone());
                let mut c = config(format!("{runner} {name}"), command, dir, RunKind::Test);
                c.report = Some("jest".into());
                c.report_file = Some(in_module(report));
                return Some(c);
            }
            // TypeScript on Node's type stripping (SPEC-0168 behaviour 3); JSX needs a build.
            (matches!(ext.as_str(), "js" | "mjs" | "cjs" | "ts" | "mts" | "cts")
                && !stem.ends_with(".d"))
            .then(|| config(name, words(&["node", &local]), dir, RunKind::Run))
        }
        "go" => {
            let name = path.file_name()?.to_string_lossy().into_owned();
            if stem.ends_with("_test") {
                // The package's tests, as a result tree (SPEC-0045 v3).
                let package = Path::new(&local)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| !p.is_empty())
                    .map_or_else(|| ".".to_owned(), |p| format!("./{p}"));
                let mut c = config(
                    format!("go test {package}"),
                    words(&["go", "test", "-json", &package]),
                    dir,
                    RunKind::Test,
                );
                c.report = Some("gotest".into());
                return Some(c);
            }
            text.contains("package main")
                .then(|| config(name, words(&["go", "run", &local]), dir, RunKind::Run))
        }
        "rs" => {
            let m = module.filter(|m| m.kind == ModuleKind::Cargo)?;
            // A source with tests and no `main`: its tests (SPEC-0045 v5 behaviour 13).
            if !text.contains("fn main(") && text.contains("#[test]") || local.starts_with("tests/")
            {
                let (_, path) = rust_target(&local);
                let name = if path.is_empty() {
                    format!("cargo test {}", rust_target(&local).0.unwrap_or_default())
                        .trim_end()
                        .to_owned()
                } else {
                    path.join("::")
                };
                return Some(cargo_test(&local, dir, &[], false, name));
            }
            let bin = if stem == "main" {
                None
            } else if rel.contains("/bin/") {
                Some(stem.clone())
            } else {
                return None;
            };
            let mut command = words(&["cargo", "run"]);
            if let Some(b) = &bin {
                command.extend(words(&["--bin", b]));
            }
            text.contains("fn main(").then(|| {
                config(
                    bin.unwrap_or_else(|| m.name.clone()),
                    command,
                    dir,
                    RunKind::Run,
                )
            })
        }
        _ => None,
    }
}

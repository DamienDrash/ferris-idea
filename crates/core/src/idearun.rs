//! Run configurations of the reference IDE's project files as IDECraft run configurations
//! (`SPEC-0049` v3 behaviour 13), read only: nothing here starts a process.
//!
//! Sources, in order: `.idea/runConfigurations/*.xml` and `ProjectRunConfigurationManager` of an
//! `.ipr` (shared), then the `RunManager` component of `workspace.xml` or the `.iws` (personal).
//! Format knowledge of the open run configuration types (Application, JUnit, Gradle, Maven,
//! Python, Shell Script) of the open platform `JetBrains/intellij-community`; no code.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::ideaproject::{Element, IdeaModule, IdeaProject, SourceKind, resolve};
use crate::run::{RunConfig, RunKind};

/// One configuration of the reference IDE and what IDECraft made of it (behaviours 13, 16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaRun {
    /// Name.
    pub name: String,
    /// The reference's type id, e.g. `Application`, `JUnit`, `GradleRunConfiguration`.
    #[serde(rename = "type")]
    pub type_id: String,
    /// The file it was read from, project-relative.
    pub source: String,
    /// Its folder in the run list, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Run or test, when translated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<RunKind>,
    /// The command, when translated.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// Working folder, project-relative, when translated and not the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Names of its environment variables; the values are never listed (behaviour 16).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    /// Why it was not translated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The translated configuration, environment values included.
    #[serde(skip)]
    pub config: Option<RunConfig>,
}

/// Reads the run configurations of `sources` (project-relative file, its root element, and
/// whether it is personal state of which only `RunManager` counts); problems name the file and
/// the configuration that was not translated.
pub(crate) fn read_runs(
    root: &Path,
    project: &IdeaProject,
    sources: &[(String, Element)],
    problems: &mut Vec<String>,
) -> Vec<IdeaRun> {
    let mut out: Vec<IdeaRun> = Vec::new();
    for (file, doc) in sources {
        for c in configurations(doc) {
            if c.attr("default") == Some("true") {
                continue;
            }
            let Some(name) = c.attr("name").filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            if out.iter().any(|r| r.name == name) {
                continue;
            }
            let type_id = c.attr("type").unwrap_or_default().to_owned();
            let folder = c
                .attr("folderName")
                .filter(|f| !f.is_empty())
                .map(str::to_owned);
            let mut run = IdeaRun {
                name: name.to_owned(),
                type_id: type_id.clone(),
                source: file.clone(),
                folder: folder.clone(),
                kind: None,
                command: Vec::new(),
                cwd: None,
                env: Vec::new(),
                reason: None,
                config: None,
            };
            match translate(root, project, c, &type_id) {
                Ok(mut t) => {
                    let mut config = RunConfig {
                        name: name.to_owned(),
                        command: t.command,
                        cwd: t.cwd.filter(|d| d != "."),
                        env: envs(c),
                        kind: t.kind,
                        report: t.report.map(str::to_owned),
                        report_file: t.report_file.take(),
                        before: Vec::new(),
                        timeout: None,
                        url: None,
                        folder,
                        allow_multiple: false,
                        before_launch: Vec::new(),
                        // Texts are used as written; only Python needs the interpreter IDECraft
                        // resolves (SPEC-0050), which a verbatim configuration skips.
                        verbatim: type_id != "PythonConfigurationType",
                    };
                    config.env.extend(t.env);
                    run.kind = Some(config.kind);
                    run.command = config.command.clone();
                    run.cwd = config.cwd.clone();
                    run.env = config.env.keys().cloned().collect();
                    run.config = Some(config);
                }
                Err(reason) => {
                    problems.push(format!("{file}: run configuration {name}: {reason}"));
                    run.reason = Some(reason);
                }
            }
            out.push(run);
        }
    }
    out
}

/// The `configuration` elements of a shared file (a `ProjectRunConfigurationManager` component,
/// in its own file or in an `.ipr`) or of the `RunManager` component of personal state; any other
/// component of personal state is not looked at (behaviour 7).
fn configurations(doc: &Element) -> Vec<&Element> {
    let manager = if doc.name == "component" {
        Some(doc)
    } else {
        doc.component("ProjectRunConfigurationManager")
            .or_else(|| doc.component("RunManager"))
    };
    manager
        .map(|m| m.all("configuration").collect())
        .unwrap_or_default()
}

/// The value of the `option` named `name`, searched below `e`.
fn option<'a>(e: &'a Element, name: &str) -> Option<&'a str> {
    find_option(e, name).and_then(|o| o.attr("value"))
}

fn find_option<'a>(e: &'a Element, name: &str) -> Option<&'a Element> {
    for c in &e.children {
        if c.name == "option" && c.attr("name") == Some(name) {
            return Some(c);
        }
        if let Some(found) = find_option(c, name) {
            return Some(found);
        }
    }
    None
}

/// The values of a list option (`<option name=…><list><option value=…/></list></option>`).
fn list_option(e: &Element, name: &str) -> Vec<String> {
    find_option(e, name)
        .and_then(|o| o.first("list"))
        .map(|l| {
            l.all("option")
                .filter_map(|o| o.attr("value"))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of a map option (`<option name=…><map><entry key=… value=…/></map></option>`).
fn map_option(e: &Element, name: &str) -> Vec<(String, String)> {
    find_option(e, name)
        .and_then(|o| o.first("map"))
        .map(|m| {
            m.all("entry")
                .filter_map(|x| Some((x.attr("key")?.to_owned(), x.attr("value")?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// The `<envs><env name value/></envs>` of a configuration.
fn envs(c: &Element) -> BTreeMap<String, String> {
    c.first("envs")
        .map(|e| {
            e.all("env")
                .filter_map(|v| Some((v.attr("name")?.to_owned(), v.attr("value")?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// What a type gives: command, working folder (project-relative), kind and report.
struct Translated {
    command: Vec<String>,
    cwd: Option<String>,
    kind: RunKind,
    report: Option<&'static str>,
    report_file: Option<String>,
    env: BTreeMap<String, String>,
}

impl Translated {
    fn run(command: Vec<String>, cwd: String) -> Self {
        Self {
            command,
            cwd: Some(cwd),
            kind: RunKind::Run,
            report: None,
            report_file: None,
            env: BTreeMap::new(),
        }
    }
}

/// Splits program parameters as the reference writes them: blanks separate, double or single
/// quotes keep blanks inside, a backslash keeps the next character.
pub(crate) fn split_params(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                }
                started = true;
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// `word` quoted for `sh`.
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// A project-relative path below the project: no `..` at its start, no macro left.
fn in_project(rel: &str) -> bool {
    !rel.contains('$') && rel != ".." && !rel.starts_with("../") && !Path::new(rel).is_absolute()
}

/// `target` relative to the folder `base`, both project-relative and normalised.
fn relative(base: &str, target: &str) -> String {
    let parts = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .map(str::to_owned)
            .collect()
    };
    let (b, t) = (parts(base), parts(target));
    let common = b.iter().zip(&t).take_while(|(x, y)| x == y).count();
    let mut out: Vec<String> = vec!["..".to_owned(); b.len() - common];
    out.extend(t[common..].iter().cloned());
    if out.is_empty() {
        ".".to_owned()
    } else {
        out.join("/")
    }
}

/// The build a module's folder has (behaviour 13).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Build {
    Maven,
    Gradle,
    None,
}

/// The folder of a module (its first content root inside the project) and its build tool.
fn module_build(root: &Path, module: Option<&IdeaModule>) -> (String, Build) {
    let dir = module
        .and_then(|m| {
            m.content_roots
                .iter()
                .map(|c| c.dir.clone())
                .find(|d| in_project(d))
        })
        .unwrap_or_else(|| ".".to_owned());
    let at = root.join(&dir);
    let build = if at.join("pom.xml").is_file() {
        Build::Maven
    } else if [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|f| at.join(f).is_file())
    {
        Build::Gradle
    } else {
        Build::None
    };
    (dir, build)
}

fn gradle_runner(root: &Path, dir: &str) -> String {
    if root.join(dir).join("gradlew").is_file() {
        "./gradlew".to_owned()
    } else {
        "gradle".to_owned()
    }
}

/// The working folder of a configuration, project-relative: the option `key` resolved against
/// the module's folder, the module's folder when it names the module's working folder, `default`
/// when empty; `Err` when it lies outside the project.
fn working_dir(c: &Element, key: &str, module_dir: &str, default: &str) -> Result<String, String> {
    let raw = option(c, key).unwrap_or_default();
    let dir = if raw.is_empty() {
        default.to_owned()
    } else if raw.starts_with("$MODULE_WORKING_DIR$") || raw.starts_with("$MODULE_DIR$") {
        let rest = raw
            .trim_start_matches("$MODULE_WORKING_DIR$")
            .trim_start_matches("$MODULE_DIR$");
        resolve(&format!("$MODULE_DIR${rest}"), module_dir)
    } else {
        resolve(raw, ".")
    };
    if in_project(&dir) {
        Ok(dir)
    } else {
        Err(format!("its working folder {raw} lies outside the project"))
    }
}

fn translate(
    root: &Path,
    project: &IdeaProject,
    c: &Element,
    type_id: &str,
) -> Result<Translated, String> {
    let module = c
        .first("module")
        .and_then(|m| m.attr("name"))
        .and_then(|n| project.modules.iter().find(|m| m.name == n));
    let (module_dir, build) = module_build(root, module);
    match type_id {
        "Application" => application(project, c, module, &module_dir, build),
        "JUnit" => junit(root, c, module, &module_dir, build),
        "GradleRunConfiguration" => {
            let dir = option(c, "externalProjectPath")
                .map(|p| resolve(p, "."))
                .unwrap_or_else(|| ".".to_owned());
            if !in_project(&dir) {
                return Err("its Gradle project lies outside the project".into());
            }
            let tasks = list_option(c, "taskNames");
            if tasks.is_empty() {
                return Err("it names no Gradle task".into());
            }
            let mut command = vec![gradle_runner(root, &dir)];
            command.extend(tasks.iter().cloned());
            command.extend(split_params(option(c, "scriptParameters").unwrap_or_default()));
            let mut t = Translated::run(command, dir.clone());
            t.env = map_option(c, "env").into_iter().collect();
            if tasks.iter().any(|t| t == "test" || t.ends_with(":test")) {
                t.kind = RunKind::Test;
                t.report = Some("junit");
                t.report_file = Some(join(&dir, "build/test-results/test"));
            }
            Ok(t)
        }
        "MavenRunConfiguration" => {
            let goals = list_option(c, "goals");
            if goals.is_empty() {
                return Err("it names no Maven goal".into());
            }
            let dir = option(c, "workingDirPath")
                .map(|p| resolve(p, "."))
                .unwrap_or_else(|| module_dir.clone());
            if !in_project(&dir) {
                return Err("its working folder lies outside the project".into());
            }
            let mut command = vec!["mvn".to_owned()];
            command.extend(goals.iter().cloned());
            let profiles: Vec<String> = map_option(c, "profilesMap")
                .into_iter()
                .map(|(p, on)| if on == "true" { p } else { format!("!{p}") })
                .collect();
            if !profiles.is_empty() {
                command.push("-P".into());
                command.push(profiles.join(","));
            }
            command.extend(split_params(option(c, "cmdOptions").unwrap_or_default()));
            let mut t = Translated::run(command, dir.clone());
            if goals.iter().any(|g| g == "test" || g == "verify" || g == "install") {
                t.kind = RunKind::Test;
                t.report = Some("junit");
                t.report_file = Some(join(&dir, "target/surefire-reports"));
            }
            Ok(t)
        }
        "PythonConfigurationType" => {
            let script = option(c, "SCRIPT_NAME").unwrap_or_default();
            if script.is_empty() {
                return Err("it names no script or module".into());
            }
            let dir = working_dir(c, "WORKING_DIRECTORY", &module_dir, ".")?;
            let mut command = vec!["python3".to_owned()];
            command.extend(split_params(option(c, "INTERPRETER_OPTIONS").unwrap_or_default()));
            if option(c, "MODULE_MODE") == Some("true") {
                command.push("-m".into());
                command.push(script.to_owned());
            } else {
                let file = resolve(script, &module_dir);
                if !in_project(&file) {
                    return Err(format!("its script {script} lies outside the project"));
                }
                command.push(relative(&dir, &file));
            }
            command.extend(split_params(option(c, "PARAMETERS").unwrap_or_default()));
            Ok(Translated::run(command, dir))
        }
        "ShConfigurationType" => {
            let dir = working_dir(c, "SCRIPT_WORKING_DIRECTORY", &module_dir, ".")?;
            let path = option(c, "SCRIPT_PATH").unwrap_or_default();
            let text = option(c, "SCRIPT_TEXT").unwrap_or_default();
            let file_mode = option(c, "EXECUTE_SCRIPT_FILE") != Some("false") && !path.is_empty();
            if !file_mode {
                if text.trim().is_empty() {
                    return Err("it holds neither a script file nor a script text".into());
                }
                return Ok(Translated::run(
                    vec!["sh".into(), "-c".into(), text.to_owned()],
                    dir,
                ));
            }
            let file = resolve(path, ".");
            if !in_project(&file) {
                return Err(format!("its script {path} lies outside the project"));
            }
            let interpreter = option(c, "INTERPRETER_PATH")
                .filter(|i| !i.trim().is_empty())
                .unwrap_or("sh");
            let mut command = vec![interpreter.to_owned()];
            command.extend(split_params(option(c, "INTERPRETER_OPTIONS").unwrap_or_default()));
            command.push(relative(&dir, &file));
            command.extend(split_params(option(c, "SCRIPT_OPTIONS").unwrap_or_default()));
            Ok(Translated::run(command, dir))
        }
        "CompoundRunConfigurationType" => {
            Err("a compound configuration; IDECraft runs one configuration after the other with `before`".into())
        }
        "js.build_tools.npm" => Err(
            "an npm configuration; its format is not part of the open sources, so it is not translated (use the npm scripts IDECraft detects from package.json)".into(),
        ),
        other => Err(format!("the type {other} has no IDECraft counterpart")),
    }
}

fn join(dir: &str, rel: &str) -> String {
    if dir == "." {
        rel.to_owned()
    } else {
        format!("{dir}/{rel}")
    }
}

/// The output folder of a module's production classes (inherited: the project's
/// `production/<module>`).
fn output_of(project: &IdeaProject, m: &IdeaModule) -> Option<String> {
    if m.inherit_output {
        project
            .output
            .as_ref()
            .map(|o| join(o, &format!("production/{}", m.name)))
    } else {
        m.output.clone()
    }
}

fn application(
    project: &IdeaProject,
    c: &Element,
    module: Option<&IdeaModule>,
    module_dir: &str,
    build: Build,
) -> Result<Translated, String> {
    let main = option(c, "MAIN_CLASS_NAME").unwrap_or_default();
    if main.is_empty() {
        return Err("it names no main class".into());
    }
    let args = split_params(option(c, "PROGRAM_PARAMETERS").unwrap_or_default());
    let vm = split_params(option(c, "VM_PARAMETERS").unwrap_or_default());
    match build {
        Build::Maven => {
            let mut command = vec![
                "mvn".to_owned(),
                "-q".into(),
                "compile".into(),
                "exec:java".into(),
                format!("-Dexec.mainClass={main}"),
            ];
            if !args.is_empty() {
                command.push(format!(
                    "-Dexec.args={}",
                    args.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" ")
                ));
            }
            let mut t = Translated::run(command, module_dir.to_owned());
            if !vm.is_empty() {
                t.env.insert("MAVEN_OPTS".into(), vm.join(" "));
            }
            Ok(t)
        }
        Build::Gradle => Err(
            "an Application in a Gradle module; run it through a Gradle configuration (the application plugin's run task)"
                .into(),
        ),
        Build::None => {
            let module = module.ok_or("it names no module of the project")?;
            let dir = working_dir(c, "WORKING_DIRECTORY", module_dir, ".")?;
            let out = output_of(project, module)
                .filter(|o| in_project(o))
                .ok_or("its module has no output folder in the project")?;
            let sources: Vec<String> = module
                .content_roots
                .iter()
                .flat_map(|r| &r.sources)
                .filter(|s| s.kind == SourceKind::Source && in_project(&s.dir))
                .map(|s| relative(&dir, &s.dir))
                .collect();
            if sources.is_empty() {
                return Err("its module has no source folder in the project".into());
            }
            let classpath = classpath(project, module)
                .iter()
                .map(|p| relative(&dir, p))
                .collect::<Vec<_>>()
                .join(":");
            let out_rel = relative(&dir, &out);
            let find = sources.iter().map(|s| sh_quote(s)).collect::<Vec<_>>().join(" ");
            let mut run = vec!["java".to_owned()];
            run.extend(vm);
            run.extend(["-cp".to_owned(), classpath.clone(), main.to_owned()]);
            run.extend(args);
            let script = format!(
                "mkdir -p {out} && javac -d {out} -cp {cp} $(find {find} -name '*.java') && {run}",
                out = sh_quote(&out_rel),
                cp = sh_quote(&classpath),
                run = run.iter().map(|w| sh_quote(w)).collect::<Vec<_>>().join(" "),
            );
            Ok(Translated::run(vec!["sh".into(), "-c".into(), script], dir))
        }
    }
}

/// The class path of a module of `.idea` alone: its output, the outputs of the modules it
/// depends on (and theirs), the class roots of its module libraries and project libraries that
/// lie in the project.
fn classpath(project: &IdeaProject, module: &IdeaModule) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut todo = vec![module];
    let mut seen: Vec<&str> = Vec::new();
    while let Some(m) = todo.pop() {
        if seen.contains(&m.name.as_str()) {
            continue;
        }
        seen.push(&m.name);
        if let Some(o) = output_of(project, m).filter(|o| in_project(o)) {
            out.push(o);
        }
        for d in &m.dependencies {
            match d.kind.as_str() {
                "module" => {
                    if let Some(dep) = project.modules.iter().find(|x| x.name == d.name) {
                        todo.insert(0, dep);
                    }
                }
                "module-library" => out.extend(d.roots.iter().filter(|r| in_project(r)).cloned()),
                "library" if d.level.as_deref() == Some("project") => {
                    if let Some(lib) = project.libraries.iter().find(|l| l.name == d.name) {
                        out.extend(lib.classes.iter().filter(|r| in_project(r)).cloned());
                    }
                }
                _ => {}
            }
        }
    }
    let mut unique = Vec::new();
    for p in out {
        if !unique.contains(&p) {
            unique.push(p);
        }
    }
    unique
}

fn junit(
    root: &Path,
    c: &Element,
    module: Option<&IdeaModule>,
    module_dir: &str,
    build: Build,
) -> Result<Translated, String> {
    let class = option(c, "MAIN_CLASS_NAME").unwrap_or_default();
    let method = option(c, "METHOD_NAME").unwrap_or_default();
    let package = option(c, "PACKAGE_NAME").unwrap_or_default();
    let object = option(c, "TEST_OBJECT").unwrap_or("class");
    let target = match object {
        "class" if !class.is_empty() => (class.to_owned(), None),
        "method" if !class.is_empty() && !method.is_empty() => {
            (class.to_owned(), Some(method.to_owned()))
        }
        "package" => (package.to_owned(), None),
        "class" | "method" => return Err("it names no test class".into()),
        other => return Err(format!("JUnit tests by {other} are not translated")),
    };
    if module.is_none() && build == Build::None {
        return Err("it names no module of the project".into());
    }
    let (command, report_file) = match (build, object) {
        (Build::Maven, "package") => (
            vec![
                "mvn".to_owned(),
                "test".into(),
                format!("-Dtest={}.**", target.0),
            ],
            "target/surefire-reports",
        ),
        (Build::Maven, _) => (
            vec![
                "mvn".to_owned(),
                "test".into(),
                match &target.1 {
                    Some(m) => format!("-Dtest={}#{m}", target.0),
                    None => format!("-Dtest={}", target.0),
                },
            ],
            "target/surefire-reports",
        ),
        (Build::Gradle, _) => (
            vec![
                gradle_runner(root, module_dir),
                "test".into(),
                "--tests".into(),
                match (&target.1, object) {
                    (Some(m), _) => format!("{}.{m}", target.0),
                    (None, "package") => format!("{}.*", target.0),
                    (None, _) => target.0.clone(),
                },
            ],
            "build/test-results/test",
        ),
        (Build::None, _) => {
            return Err(
                "JUnit in a module without a Maven or Gradle build; IDECraft runs tests through the build tool"
                    .into(),
            );
        }
    };
    Ok(Translated {
        command,
        cwd: Some(module_dir.to_owned()),
        kind: RunKind::Test,
        report: Some("junit"),
        report_file: Some(join(module_dir, report_file)),
        env: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_split_like_the_reference_writes_them() {
        assert_eq!(
            split_params(r#"-v "two words" 'x y' a\ b  """#),
            ["-v", "two words", "x y", "a b", ""]
        );
        assert!(split_params("   ").is_empty());
    }

    #[test]
    fn relative_paths_between_project_folders() {
        assert_eq!(relative(".", "src/a.py"), "src/a.py");
        assert_eq!(relative("app", "app/run.sh"), "run.sh");
        assert_eq!(relative("app/bin", "lib/x.jar"), "../../lib/x.jar");
        assert_eq!(relative("app", "app"), ".");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }
}

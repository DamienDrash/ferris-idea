//! SPEC-0047 v3 AK-6, AK-7, AK-9 and AK-10: folders, instances and the before-launch tasks
//! "Build", "Run external tool" and "Open browser", with real processes. The browser is a
//! recorder: nothing is opened on the machine that runs the tests.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use ferris_idea_core::{
    BeforeTask, RunEvent, SecondStart, entries_of, parse_run_configs, rewrite_run_toml, run_config,
    run_plan, running_instances, second_start, start_host_run, start_run, stop_instances,
};

static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Replaces the system's opener for this test program, once.
fn record_browser() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        ferris_idea_core::set_browser_opener(Some(Arc::new(|url: &str| {
            OPENED.lock().unwrap().push(url.to_owned());
            Ok(())
        })));
    });
}

fn opened(marker: &str) -> Vec<String> {
    OPENED
        .lock()
        .unwrap()
        .iter()
        .filter(|u| u.contains(marker))
        .cloned()
        .collect()
}

struct Root(PathBuf);

impl Root {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "ferris-idea-run-tasks-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join(".ferris-idea")).unwrap();
        std::fs::create_dir_all(p.join("scripts")).unwrap();
        Self(p.canonicalize().unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const OLD: &str = r#"[ferris-idea]
format = 1
kind = "run-configurations"

# The server, started first.
[[run]]
name = "server"
command = ["node", "server.js"]   # keep this comment
env = { PORT = "8080" }

[[run]]
name = "tests"
kind = "test"
report = "tap"
command = ["npm", "test"]
before = ["server"]
timeout = 30
"#;

/// The sample of the specification: a folder, the instance switch and the three kinds of tasks.
const SAMPLE: &str = r#"[ferris-idea]
format = 1
kind = "run-configurations"

# Started by hand while working on the API.
[[run]]
name = "api"
folder = "Backend"
command = ["cargo", "run", "-p", "api"]

[[run]]
name = "web"
folder = "Frontend"
command = ["npm", "run", "dev"]
allow_multiple = true
before = ["api"]
before_launch = [
    { build = "project" },
    { tool = ["sh", "gen.sh", "--fast"], cwd = "scripts" },
    { browser = "http://localhost:3000" },
]
"#;

/// AK-6: a file written before v3 means what it meant and is not rewritten.
#[test]
fn old_files_load_and_write_back_unchanged() {
    let loaded = parse_run_configs(OLD);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    for c in &loaded.configs {
        assert_eq!(c.folder, None);
        assert!(!c.allow_multiple);
        assert!(c.before_launch.is_empty());
        assert_eq!(second_start(c, 1), SecondStart::Ask);
    }
    assert_eq!(loaded.configs[1].before, ["server"]);
    let entries = entries_of(OLD).unwrap();
    assert_eq!(rewrite_run_toml(OLD, &entries).unwrap(), OLD);
    // Changing one configuration leaves the other and the new keys out.
    let mut changed = entries.clone();
    changed[1].config.timeout = Some(60);
    let out = rewrite_run_toml(OLD, &changed).unwrap();
    assert!(out.contains("command = [\"node\", \"server.js\"]   # keep this comment"));
    for key in ["folder", "allow_multiple", "before_launch"] {
        assert!(!out.contains(key), "{key} in {out}");
    }
    // As JSON (the MCP tools) an old configuration has none of the new keys either.
    let json = serde_json::to_value(&loaded.configs[0]).unwrap();
    for key in ["folder", "allow_multiple", "before_launch"] {
        assert!(json.get(key).is_none(), "{key}");
    }
}

/// AK-6: the new keys load, are written by the writer of SPEC-0327 and read back the same.
#[test]
fn folder_instances_and_tasks_round_trip() {
    let loaded = parse_run_configs(SAMPLE);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let web = &loaded.configs[1];
    assert_eq!(web.folder.as_deref(), Some("Frontend"));
    assert!(web.allow_multiple);
    assert_eq!(
        web.before_launch,
        [
            BeforeTask::build(),
            BeforeTask::tool(
                vec!["sh".into(), "gen.sh".into(), "--fast".into()],
                Some("scripts".into())
            ),
            BeforeTask::browser("http://localhost:3000"),
        ]
    );
    // Unchanged entries leave the file as it is, comment and layout included.
    let entries = entries_of(SAMPLE).unwrap();
    assert_eq!(rewrite_run_toml(SAMPLE, &entries).unwrap(), SAMPLE);
    // Written from nothing, the same configurations give the same keys in this layout.
    let mut fresh = entries.clone();
    for e in &mut fresh {
        e.original = None;
    }
    let written = rewrite_run_toml("", &fresh).unwrap();
    assert!(
        written.contains(
            "before_launch = [\n    { build = \"project\" },\n    { tool = [\"sh\", \"gen.sh\", \"--fast\"], cwd = \"scripts\" },\n    { browser = \"http://localhost:3000\" },\n]\n"
        ),
        "{written}"
    );
    assert!(written.contains("folder = \"Frontend\"\n"));
    assert!(written.contains("allow_multiple = true\n"));
    assert_eq!(parse_run_configs(&written).configs, loaded.configs);
    // Taking the keys away again removes them from the file.
    let mut plain = entries.clone();
    plain[1].config.folder = None;
    plain[1].config.allow_multiple = false;
    plain[1].config.before_launch.clear();
    let out = rewrite_run_toml(SAMPLE, &plain).unwrap();
    assert!(out.contains("# Started by hand while working on the API."));
    assert!(out.contains("folder = \"Backend\""));
    for gone in ["Frontend", "allow_multiple", "before_launch"] {
        assert!(!out.contains(gone), "{gone} in {out}");
    }
    // Changing one task rewrites the list and nothing else.
    let mut other = entries.clone();
    other[1].config.before_launch[2] = BeforeTask::browser("https://example.org/app");
    let out = rewrite_run_toml(SAMPLE, &other).unwrap();
    assert!(out.contains("{ browser = \"https://example.org/app\" },\n"));
    assert!(out.contains("# Started by hand while working on the API."));
    // Tasks written as tables (the plain serializer) read the same.
    let tables = "[[run]]\nname = \"web\"\ncommand = [\"x\"]\n\n[[run.before_launch]]\nbuild = \"project\"\n\n[[run.before_launch]]\nbrowser = \"http://localhost:3000\"\n";
    let loaded = parse_run_configs(tables);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    assert_eq!(loaded.configs[0].before_launch.len(), 2);
}

/// AK-6: every malformed task and folder is a problem naming the configuration; a refused URL
/// is not written.
#[test]
fn malformed_tasks_are_problems() {
    let with = |task: &str| {
        parse_run_configs(&format!(
            "[[run]]\nname = \"app\"\ncommand = [\"x\"]\nbefore_launch = [{task}]\n"
        ))
        .problems
    };
    for (task, part) in [
        ("{}", "needs one of build, tool or browser"),
        (
            "{ build = \"project\", browser = \"http://localhost\" }",
            "only one of",
        ),
        ("{ build = \"module\" }", "build must be \"project\""),
        ("{ tool = [] }", "has no command"),
        (
            "{ tool = [\"make\"], cwd = \"../out\" }",
            "leaves the workspace",
        ),
        (
            "{ tool = [\"make\"], cwd = \"/tmp\" }",
            "leaves the workspace",
        ),
        ("{ build = \"project\", cwd = \"x\" }", "cwd belongs to"),
        (
            "{ browser = \"vscode://file/x\" }",
            "only http, https and file",
        ),
        (
            "{ browser = \"javascript:alert(1)\" }",
            "only http, https and file",
        ),
        (
            "{ browser = \"file:///Applications/Calculator.app\" }",
            "file://${workspace}/",
        ),
        (
            "{ browser = \"file://${workspace}/run.command\" }",
            "only .html and .htm",
        ),
        (
            "{ browser = \"https://me@example.org/\" }",
            "without a user name",
        ),
    ] {
        let problems = with(task);
        assert_eq!(problems.len(), 1, "{task}: {problems:?}");
        assert!(
            problems[0].starts_with("app: before-launch task 1: ") && problems[0].contains(part),
            "{task}: {problems:?}"
        );
    }
    // A key the format does not know is refused as before.
    assert_eq!(with("{ shell = \"rm -rf /\" }").len(), 1);
    assert!(with("{ build = \"project\" }, { tool = [\"make\"] }").is_empty());
    let folder = parse_run_configs("[[run]]\nname = \"app\"\ncommand = [\"x\"]\nfolder = \" \"\n");
    assert_eq!(folder.problems.len(), 1);
    assert!(folder.problems[0].starts_with("app: the folder name"));
    // The writer refuses the same and leaves the text alone.
    let mut entries = entries_of(SAMPLE).unwrap();
    entries[0].config.before_launch = vec![BeforeTask::browser("ssh://host/")];
    let problems = rewrite_run_toml(SAMPLE, &entries).unwrap_err();
    assert!(
        problems[0].contains("only http, https and file"),
        "{problems:?}"
    );
}

/// AK-7: `before` configurations, then the tool steps in the order written, then the
/// configuration; a failing tool stops the chain.
#[test]
fn tool_steps_run_in_order_and_a_failing_one_stops_the_chain() {
    let root = Root::new("tools");
    let configs = parse_run_configs(
        r#"[[run]]
name = "prep"
command = ["sh", "-c", "echo prep >> log.txt"]
before_launch = [{ tool = ["sh", "-c", "echo prep-tool >> log.txt"] }]

[[run]]
name = "app"
command = ["sh", "-c", "echo app >> log.txt"]
before = ["prep"]
before_launch = [
    { tool = ["sh", "-c", "pwd > where.txt; echo one >> ../log.txt"], cwd = "scripts" },
    { tool = ["sh", "-c", "echo ${workspace} > ws.txt; echo two >> log.txt"] },
]

[[run]]
name = "broken"
command = ["sh", "-c", "echo never >> log.txt"]
before_launch = [{ tool = ["sh", "-c", "exit 3"] }, { tool = ["sh", "-c", "echo never >> log.txt"] }]
"#,
    );
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    let plan = run_plan(root.path(), &configs, "app").unwrap();
    assert_eq!(
        plan.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["prep · sh", "prep", "app · sh", "app · sh", "app"]
    );
    assert_eq!(plan[2].cwd, "scripts");
    let runs = run_config(root.path(), &configs, "app").unwrap();
    assert_eq!(runs.len(), 5);
    assert!(runs.iter().all(|r| r.success()), "{runs:?}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("log.txt")).unwrap(),
        "prep-tool\nprep\none\ntwo\napp\n"
    );
    let where_ = std::fs::read_to_string(root.path().join("scripts/where.txt")).unwrap();
    assert!(where_.trim().ends_with("/scripts"), "{where_}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("ws.txt"))
            .unwrap()
            .trim(),
        root.path().to_string_lossy()
    );
    // The failing tool is the last step that ran; streamed, the chain says where it stopped.
    let runs = run_config(root.path(), &configs, "broken").unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].exit_code, Some(3));
    let handle = start_run(root.path(), &configs, "broken").unwrap();
    let events: Vec<RunEvent> = handle.events.iter().collect();
    assert!(
        matches!(events.last(), Some(RunEvent::ChainStopped { failed }) if failed == "broken · sh")
    );
    assert!(
        !std::fs::read_to_string(root.path().join("log.txt"))
            .unwrap()
            .contains("never")
    );
}

/// AK-7: "Build" is what the project model knows; without a build the configuration cannot start.
#[test]
fn build_runs_what_the_project_model_knows() {
    let run_toml = "[[run]]\nname = \"gen\"\ncommand = [\"true\"]\nbefore_launch = [{ build = \"project\" }]\n\n[[run]]\nname = \"app\"\ncommand = [\"true\"]\nbefore = [\"gen\"]\nbefore_launch = [{ build = \"project\" }]\n";
    let configs = parse_run_configs(run_toml);
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    // Nothing to build: a reason instead of a start.
    let empty = Root::new("no-build");
    let why = run_plan(empty.path(), &configs, "app").unwrap_err();
    assert!(
        why.starts_with("gen: before-launch task 1: the project has no build"),
        "{why}"
    );
    assert!(run_config(empty.path(), &configs, "app").is_err());
    assert!(start_run(empty.path(), &configs, "app").is_err());
    // Cargo and a package with a script named build; the run builds once.
    let root = Root::new("build");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join("web")).unwrap();
    std::fs::write(
        root.path().join("web/package.json"),
        "{ \"name\": \"web\", \"scripts\": { \"build\": \"tsc\", \"dev\": \"vite\" } }",
    )
    .unwrap();
    let plan = run_plan(root.path(), &configs, "app").unwrap();
    let steps: Vec<(String, String, &str)> = plan
        .iter()
        .map(|p| (p.name.clone(), p.command.join(" "), p.cwd.as_str()))
        .collect();
    assert_eq!(steps.len(), 4, "{steps:?}");
    assert_eq!(steps[0].1, "cargo build");
    assert_eq!(steps[0].2, ".");
    assert_eq!(
        steps[1],
        (
            "npm run build (web)".to_owned(),
            "npm run build".to_owned(),
            "web"
        )
    );
    assert_eq!(steps[2].0, "gen");
    assert_eq!(steps[3].0, "app");
}

/// AK-9: after the start the configuration's own URLs reach the opener, variables replaced.
#[test]
fn the_browser_opens_after_the_start_with_the_configurations_own_urls() {
    record_browser();
    let root = Root::new("browser");
    std::fs::create_dir_all(root.path().join("docs")).unwrap();
    let configs = parse_run_configs(
        r#"[[run]]
name = "api"
command = ["sh", "-c", "echo api"]
before_launch = [{ browser = "http://localhost:1/ferris-idea-test-not-mine" }]

[[run]]
name = "web"
command = ["sh", "-c", "echo web"]
before = ["api"]
before_launch = [
    { tool = ["sh", "-c", "echo tool"] },
    { browser = "http://localhost:1/ferris-idea-test-own-a" },
    { browser = "file://${workspace}/docs/ferris-idea-test-own-b.html" },
]

[[run]]
name = "fails"
command = ["ferris-idea-no-such-program-xyz"]
before_launch = [{ browser = "http://localhost:1/ferris-idea-test-never-started" }]
"#,
    );
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    let file_url = format!(
        "file://{}/docs/ferris-idea-test-own-b.html",
        root.path().to_string_lossy()
    );
    // The plan names what would be opened, on the last step only.
    let plan = run_plan(root.path(), &configs, "web").unwrap();
    assert!(plan[..plan.len() - 1].iter().all(|p| p.opens.is_empty()));
    assert_eq!(
        plan.last().unwrap().opens,
        [
            "http://localhost:1/ferris-idea-test-own-a".to_owned(),
            file_url.clone()
        ]
    );
    assert!(
        opened("ferris-idea-test-own").is_empty(),
        "a plan opens nothing"
    );
    // Streamed: the lines of the output say what was opened.
    let handle = start_run(root.path(), &configs, "web").unwrap();
    let events: Vec<RunEvent> = handle.events.iter().collect();
    let lines: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Output { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        lines.contains(&"Opened http://localhost:1/ferris-idea-test-own-a in the browser"),
        "{lines:?}"
    );
    assert_eq!(
        opened("ferris-idea-test-own"),
        [
            "http://localhost:1/ferris-idea-test-own-a".to_owned(),
            file_url.clone()
        ]
    );
    // The URL of the configuration run before it was not opened.
    assert!(opened("ferris-idea-test-not-mine").is_empty());
    // Blocking (agents): the outcome of the last step says it.
    let runs = run_config(root.path(), &configs, "web").unwrap();
    assert!(runs[..runs.len() - 1].iter().all(|r| r.browser.is_empty()));
    assert_eq!(
        runs.last().unwrap().browser,
        [
            "Opened http://localhost:1/ferris-idea-test-own-a in the browser".to_owned(),
            format!("Opened {file_url} in the browser"),
        ]
    );
    assert_eq!(opened("ferris-idea-test-own").len(), 4);
    // A program that does not start opens nothing.
    let runs = run_config(root.path(), &configs, "fails").unwrap();
    assert!(runs[0].start_error.is_some());
    assert!(opened("ferris-idea-test-never-started").is_empty());
    // A URL that passes the file's check but not the check after its variables are replaced is
    // not opened; the run goes on and says why.
    let unset = parse_run_configs(
        "[[run]]\nname = \"web\"\ncommand = [\"true\"]\nbefore_launch = [{ browser = \"http://${env:FERRIS_IDEA_TEST_NO_SUCH_HOST}/ferris-idea-test-unset\" }]\n",
    );
    assert!(unset.problems.is_empty(), "{:?}", unset.problems);
    assert!(run_plan(root.path(), &unset, "web").is_err());
    let runs = run_config(root.path(), &unset, "web").unwrap();
    assert!(runs[0].success());
    assert!(
        runs[0].browser[0].starts_with("Open browser: the URL"),
        "{runs:?}"
    );
    assert!(opened("ferris-idea-test-unset").is_empty());
}

/// AK-9: the host of a remote project opens nothing and says so.
#[test]
fn a_host_run_opens_nothing() {
    record_browser();
    let root = Root::new("host");
    let configs = parse_run_configs(
        "[[run]]\nname = \"web\"\ncommand = [\"true\"]\nbefore_launch = [{ browser = \"http://localhost:1/ferris-idea-test-host\" }]\n",
    );
    let handle = start_host_run(root.path(), &configs, "web").unwrap();
    let events: Vec<RunEvent> = handle.events.iter().collect();
    assert!(events.iter().any(|e| matches!(
        e,
        RunEvent::Output { text, .. }
            if text == "Open browser: http://localhost:1/ferris-idea-test-host is not opened on the host"
    )));
    assert!(opened("ferris-idea-test-host").is_empty());
}

/// AK-10: runs of this process are counted per configuration and workspace, and stopped.
#[test]
fn running_instances_are_counted_and_stopped() {
    let root = Root::new("instances");
    let configs = parse_run_configs(
        "[[run]]\nname = \"server\"\ncommand = [\"sleep\", \"30\"]\n\n[[run]]\nname = \"many\"\ncommand = [\"sleep\", \"30\"]\nallow_multiple = true\n\n[[run]]\nname = \"quick\"\ncommand = [\"true\"]\n",
    );
    let config = |name: &str| configs.configs.iter().find(|c| c.name == name).unwrap();
    assert_eq!(running_instances(root.path(), "server"), 0);
    assert_eq!(second_start(config("server"), 0), SecondStart::Start);
    let first = start_run(root.path(), &configs, "server").unwrap();
    assert!(matches!(
        first.events.recv_timeout(Duration::from_secs(10)).unwrap(),
        RunEvent::Started { .. }
    ));
    assert_eq!(running_instances(root.path(), "server"), 1);
    assert_eq!(second_start(config("server"), 1), SecondStart::Ask);
    // Another configuration and another workspace do not count.
    assert_eq!(running_instances(root.path(), "many"), 0);
    assert_eq!(running_instances(Path::new("/"), "server"), 0);
    // Stop and Rerun: the running one ends, then nothing of it runs.
    assert_eq!(
        stop_instances(root.path(), "server", Duration::from_secs(10)).unwrap(),
        1
    );
    assert_eq!(running_instances(root.path(), "server"), 0);
    let finished = first
        .events
        .iter()
        .find_map(|e| match e {
            RunEvent::Finished(o) => Some(o),
            _ => None,
        })
        .unwrap();
    assert!(finished.stopped);
    // Several instances side by side.
    let a = start_run(root.path(), &configs, "many").unwrap();
    let b = start_run(root.path(), &configs, "many").unwrap();
    assert_eq!(running_instances(root.path(), "many"), 2);
    assert_eq!(second_start(config("many"), 2), SecondStart::NewInstance);
    assert_eq!(
        stop_instances(root.path(), "many", Duration::from_secs(10)).unwrap(),
        2
    );
    drop((a, b));
    // A run that ended is no instance any more.
    let quick = start_run(root.path(), &configs, "quick").unwrap();
    let _: Vec<RunEvent> = quick.events.iter().collect();
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while running_instances(root.path(), "quick") > 0 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(running_instances(root.path(), "quick"), 0);
    assert_eq!(
        stop_instances(root.path(), "quick", Duration::from_secs(1)).unwrap(),
        0
    );
}

/// SPEC-0047 v4 AK-13: "Open browser" with `wait_port` opens the page once the port accepts
/// connections, and not at all when the program ends first.
#[test]
fn the_browser_waits_for_its_port() {
    record_browser();
    let root = Root::new("wait-port");
    // A port nobody listens on: bound once to learn a free number, then released.
    let free = || {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let (port, never) = (free(), free());
    let text = format!(
        r#"[[run]]
name = "server"
command = ["sleep", "30"]
before_launch = [{{ browser = "http://localhost:{port}/ferris-idea-test-waited", wait_port = {port} }}]

[[run]]
name = "gone"
command = ["true"]
before_launch = [{{ browser = "http://localhost:{never}/ferris-idea-test-never-up", wait_port = {never} }}]
"#
    );
    let configs = parse_run_configs(&text);
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    assert_eq!(
        configs.configs[0].before_launch,
        [BeforeTask::browser_after_port(
            &format!("http://localhost:{port}/ferris-idea-test-waited"),
            port
        )]
    );
    // The key round-trips through the writer.
    let entries = entries_of(&text).unwrap();
    assert!(
        rewrite_run_toml(&text, &entries)
            .unwrap()
            .ends_with(text.as_str())
    );
    let plan = run_plan(root.path(), &configs, "server").unwrap();
    assert_eq!(
        plan[0].opens_after_port,
        [(
            format!("http://localhost:{port}/ferris-idea-test-waited"),
            port
        )]
    );
    let handle = start_run(root.path(), &configs, "server").unwrap();
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(10)).unwrap(),
        RunEvent::Started { .. }
    ));
    // The program runs, the port is closed: nothing is opened yet.
    std::thread::sleep(Duration::from_millis(400));
    assert!(opened("ferris-idea-test-waited").is_empty());
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    let line = handle
        .events
        .iter()
        .find_map(|e| match e {
            RunEvent::Output { text, .. } if text.contains("ferris-idea-test-waited") => Some(text),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        line,
        format!("Opened http://localhost:{port}/ferris-idea-test-waited in the browser")
    );
    assert_eq!(opened("ferris-idea-test-waited").len(), 1);
    drop(listener);
    stop_instances(root.path(), "server", Duration::from_secs(10)).unwrap();
    // A program that ends before its port opens: the page is not opened, and the output says so.
    let outcomes = run_config(root.path(), &configs, "gone").unwrap();
    assert_eq!(
        outcomes[0].browser,
        [format!(
            "Open browser: http://localhost:{never}/ferris-idea-test-never-up is not opened: the program ended before port {never} opened"
        )]
    );
    assert!(opened("ferris-idea-test-never-up").is_empty());
}

//! SPEC-0047 AK-1 – AK-4 and SPEC-0087 AK-1 with real processes.
#![cfg(unix)]

use std::path::{Path, PathBuf};

use idecraft_core::{
    RunEvent, TestStatus, expand_vars, load_run_configs, parse_run_configs, run_config, start_run,
};

struct Root(PathBuf);

impl Root {
    fn new(tag: &str, run_toml: &str) -> Self {
        let p = std::env::temp_dir().join(format!("idecraft-run-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join(".idecraft")).unwrap();
        std::fs::create_dir_all(p.join("sub")).unwrap();
        std::fs::write(p.join(".idecraft/run.toml"), run_toml).unwrap();
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

#[test]
fn loading_and_validation() {
    let ok = parse_run_configs(
        "[[run]]\nname = \"app\"\ncommand = [\"echo\", \"hi\"]\n\n[[run]]\nname = \"build\"\ncommand = [\"true\"]\n",
    );
    assert!(ok.problems.is_empty(), "{:?}", ok.problems);
    assert_eq!(ok.configs.len(), 2);

    let bad = parse_run_configs(
        r#"
[[run]]
name = "a"
command = []
before = ["b", "ghost"]
cwd = "../outside"

[[run]]
name = "b"
command = ["true"]
before = ["a"]
report = "nunit"

[[run]]
name = "b"
command = ["true"]
"#,
    );
    let all = bad.problems.join("\n");
    for expected in [
        "a: the command is empty",
        "a: unknown before-launch configuration ghost",
        "a: ../outside leaves the workspace",
        "b: the name is used twice",
        "b: unknown report format nunit",
        "before-launch cycle",
    ] {
        assert!(all.contains(expected), "missing {expected:?} in\n{all}");
    }
    let syntax = parse_run_configs("[[run]]\nname = 1\n");
    assert_eq!(syntax.problems.len(), 1);

    let root = Path::new("/ws");
    let home = std::env::var("HOME").unwrap_or_default();
    assert_eq!(
        expand_vars("${workspace}/x ${env:HOME} ${other}", root),
        format!("/ws/x {home} ${{other}}")
    );
}

#[test]
fn running_processes_and_timeout() {
    let r = Root::new(
        "proc",
        r#"
[[run]]
name = "show"
command = ["/bin/sh", "-c", "pwd; echo \"$GREETING\"; echo oops >&2; exit 3"]
cwd = "sub"
env = { GREETING = "hello from ${workspace}" }

[[run]]
name = "slow"
# `sleep` runs as a child of the shell; the timeout must end both.
command = ["/bin/sh", "-c", "sleep 30; echo late"]
timeout = 1
"#,
    );
    let configs = load_run_configs(r.path());
    let out = run_config(r.path(), &configs, "show").unwrap();
    let o = &out[0];
    assert_eq!(o.exit_code, Some(3));
    assert!(!o.success());
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], r.path().join("sub").to_string_lossy());
    assert_eq!(lines[1], format!("hello from {}", r.path().display()));
    assert_eq!(o.stderr, "oops\n");

    let slow = &run_config(r.path(), &configs, "slow").unwrap()[0];
    assert!(slow.timed_out && slow.exit_code.is_none());
    assert!(slow.millis < 10_000);
    assert!(run_config(r.path(), &configs, "missing").is_err());
}

#[test]
fn before_launch_chain() {
    let r = Root::new(
        "chain",
        r#"
[[run]]
name = "gen"
command = ["/bin/sh", "-c", "echo gen >> log.txt"]

[[run]]
name = "build"
command = ["/bin/sh", "-c", "echo build >> log.txt"]
before = ["gen"]

[[run]]
name = "app"
command = ["/bin/sh", "-c", "echo app >> log.txt"]
before = ["build", "gen"]

[[run]]
name = "broken"
command = ["/bin/sh", "-c", "exit 1"]

[[run]]
name = "guarded"
command = ["/bin/sh", "-c", "echo never >> log.txt"]
before = ["broken"]
"#,
    );
    let configs = load_run_configs(r.path());
    let out = run_config(r.path(), &configs, "app").unwrap();
    let names: Vec<&str> = out.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["gen", "build", "app"]);
    assert_eq!(
        std::fs::read_to_string(r.path().join("log.txt")).unwrap(),
        "gen\nbuild\napp\n"
    );
    let out = run_config(r.path(), &configs, "guarded").unwrap();
    assert_eq!(out.len(), 1, "the chain stops at the failure");
    assert!(
        !std::fs::read_to_string(r.path().join("log.txt"))
            .unwrap()
            .contains("never")
    );
}

#[test]
fn test_configurations_attach_reports() {
    let r = Root::new(
        "tests",
        r#"
[[run]]
name = "tap"
kind = "test"
report = "tap"
command = ["/bin/sh", "-c", "echo 1..2; echo 'ok 1 - a'; echo 'not ok 2 - b'; exit 1"]

[[run]]
name = "junit"
kind = "test"
report_file = "out/report.xml"
command = ["/bin/sh", "-c", "mkdir -p out && printf '<testsuite name=\"s\"><testcase name=\"x\"/></testsuite>' > out/report.xml"]
"#,
    );
    let configs = load_run_configs(r.path());
    let tap = &run_config(r.path(), &configs, "tap").unwrap()[0];
    let report = tap.report.as_ref().unwrap();
    assert_eq!((report.totals.passed, report.totals.failed), (1, 1));
    assert_eq!(report.suites[0].tests[1].status, TestStatus::Failed);
    let junit = &run_config(r.path(), &configs, "junit").unwrap()[0];
    assert!(junit.success());
    assert_eq!(junit.report.as_ref().unwrap().totals.passed, 1);
}

/// Events of a run until it ends.
fn events(handle: idecraft_core::RunHandle) -> Vec<RunEvent> {
    handle.events.iter().collect()
}

/// SPEC-0087 AK-1: streaming runs.
#[test]
fn streaming_runs_report_lines_exit_stop_and_chain() {
    let r = Root::new(
        "stream",
        r#"
[[run]]
name = "talk"
command = ["/bin/sh", "-c", "echo one; echo oops >&2; sleep 0.2; echo two; exit 3"]

[[run]]
name = "broken"
command = ["/bin/sh", "-c", "exit 1"]

[[run]]
name = "guarded"
command = ["/bin/sh", "-c", "echo never"]
before = ["broken"]

[[run]]
name = "forever"
command = ["/bin/sh", "-c", "echo started; sleep 30 & wait"]
"#,
    );
    let configs = load_run_configs(r.path());

    let got = events(start_run(r.path(), &configs, "talk").unwrap());
    assert!(
        matches!(&got[0], RunEvent::Started { name, command } if name == "talk" && command[0] == "/bin/sh")
    );
    let lines: Vec<(bool, &str)> = got
        .iter()
        .filter_map(|e| match e {
            RunEvent::Output { stderr, text } => Some((*stderr, text.as_str())),
            _ => None,
        })
        .collect();
    let stdout: Vec<&str> = lines.iter().filter(|l| !l.0).map(|l| l.1).collect();
    assert_eq!(stdout, ["one", "two"]);
    assert!(lines.contains(&(true, "oops")));
    match got.last().unwrap() {
        RunEvent::Finished(o) => {
            assert_eq!(o.exit_code, Some(3));
            assert_eq!(o.stdout, "one\ntwo\n");
        }
        other => panic!("{other:?}"),
    }
    // The blocking run gives the same outcome.
    let blocking = run_config(r.path(), &configs, "talk").unwrap();
    assert_eq!(blocking[0].exit_code, Some(3));
    assert_eq!(blocking[0].stderr, "oops\n");

    let got = events(start_run(r.path(), &configs, "guarded").unwrap());
    assert!(matches!(got.last(), Some(RunEvent::ChainStopped { failed }) if failed == "broken"));
    assert!(
        !got.iter()
            .any(|e| matches!(e, RunEvent::Output { text, .. } if text == "never"))
    );

    let handle = start_run(r.path(), &configs, "forever").unwrap();
    let first = handle.events.recv().unwrap();
    assert!(matches!(first, RunEvent::Started { .. }));
    let line = handle.events.recv().unwrap();
    assert_eq!(
        line,
        RunEvent::Output {
            stderr: false,
            text: "started".into()
        }
    );
    let at = std::time::Instant::now();
    handle.stop();
    let rest: Vec<RunEvent> = handle.events.iter().collect();
    assert!(
        at.elapsed() < std::time::Duration::from_secs(5),
        "the sleeping child does not keep it alive"
    );
    match rest.last().unwrap() {
        RunEvent::Finished(o) => assert!(o.stopped && !o.success()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn detected_configurations_come_after_and_do_not_replace() {
    use idecraft_core::{RunConfig, RunKind, add_detected, parse_run_configs};
    let mut configs =
        parse_run_configs("[[run]]\nname = \"npm run build\"\ncommand = [\"make\"]\n");
    let detected = |name: &str| RunConfig {
        name: name.into(),
        command: vec!["npm".into(), "run".into()],
        cwd: None,
        env: Default::default(),
        kind: RunKind::Run,
        report: None,
        report_file: None,
        before: Vec::new(),
        timeout: None,
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: false,
    };
    let added = add_detected(
        &mut configs,
        vec![detected("npm run build"), detected("npm run lint")],
    );
    assert_eq!(added, ["npm run lint"]);
    assert_eq!(configs.configs.len(), 2);
    assert_eq!(
        configs.configs[0].command,
        ["make"],
        "run.toml wins on a name clash"
    );
}

/// SPEC-0276 AK-3: a verbatim configuration (a command suggested by a plugin) runs its argument
/// list as given — `${…}` is not replaced — in its folder with its environment.
#[test]
fn verbatim_configuration_is_not_expanded() {
    use idecraft_core::{RunConfig, RunConfigs, RunKind, run_plan};
    let root = Root::new("verbatim", "");
    let config = RunConfig {
        name: "plugin".into(),
        command: vec![
            "printf".into(),
            "%s|%s|%s\\n".into(),
            "${workspace}".into(),
            "a b; echo no".into(),
            "${env:HOME}".into(),
        ],
        cwd: Some("sub".into()),
        env: [("GREETING".to_owned(), "${workspace}".to_owned())].into(),
        kind: RunKind::Run,
        report: None,
        report_file: None,
        before: Vec::new(),
        timeout: Some(20),
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: true,
    };
    let configs = RunConfigs {
        configs: vec![config.clone()],
        problems: Vec::new(),
    };
    // The plan shows the command exactly as it will be passed.
    let plan = run_plan(root.path(), &configs, "plugin").unwrap();
    assert_eq!(plan[0].command[2], "${workspace}");
    assert_eq!(plan[0].cwd, "sub");
    let out = run_config(root.path(), &configs, "plugin").unwrap();
    assert_eq!(out[0].stdout, "${workspace}|a b; echo no|${env:HOME}\n");
    // Folder and environment apply; the variable in the value stays as given.
    let configs = RunConfigs {
        configs: vec![RunConfig {
            command: vec![
                "sh".into(),
                "-c".into(),
                "pwd; printf %s \"$GREETING\"".into(),
            ],
            ..config
        }],
        problems: Vec::new(),
    };
    let out = run_config(root.path(), &configs, "plugin").unwrap();
    assert_eq!(
        out[0].stdout,
        format!("{}\n${{workspace}}", root.path().join("sub").display())
    );
}

/// SPEC-0276 AK-9 (v3): a command a plugin suggested does not start in a folder that a symbolic
/// link leads to, inside or outside the project, also when the link was put there after the
/// suggestion was made; a configuration of the user's own `run.toml` is not affected.
#[test]
fn verbatim_configuration_does_not_start_through_a_link() {
    use idecraft_core::{RunConfig, RunConfigs, RunKind};
    let root = Root::new("verbatim-link", "");
    let outside = Root::new("verbatim-link-outside", "");
    std::os::unix::fs::symlink(outside.path(), root.path().join("out")).unwrap();
    std::os::unix::fs::symlink(root.path().join("sub"), root.path().join("inside")).unwrap();
    let config = |cwd: &str, verbatim: bool| RunConfigs {
        configs: vec![RunConfig {
            name: "plugin".into(),
            command: vec!["pwd".into()],
            cwd: Some(cwd.into()),
            env: Default::default(),
            kind: RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: Some(20),
            url: None,
            verbatim,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
        }],
        problems: Vec::new(),
    };
    for folder in ["out", "out/sub", "inside", "missing"] {
        let out = run_config(root.path(), &config(folder, true), "plugin").unwrap();
        assert_eq!(out[0].exit_code, None, "{folder}");
        assert_eq!(out[0].stdout, "", "{folder}");
        let why = out[0].start_error.clone().unwrap_or_default();
        assert!(why.contains(&format!("the folder `{folder}`")), "{why}");
    }
    let why = run_config(root.path(), &config("out", true), "plugin").unwrap()[0]
        .start_error
        .clone()
        .unwrap();
    assert_eq!(
        why,
        "plugin: the folder `out` is reached through a symbolic link"
    );
    let out = run_config(root.path(), &config("sub", true), "plugin").unwrap();
    assert_eq!(
        out[0].stdout,
        format!("{}\n", root.path().join("sub").display())
    );
    // The user's own configuration may name a link: it is the user's file.
    let out = run_config(root.path(), &config("inside", false), "plugin").unwrap();
    assert_eq!(out[0].exit_code, Some(0));
}

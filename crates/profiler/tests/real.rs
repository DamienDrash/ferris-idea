//! Real profiling runs: a small C program under `sample` (macOS), a Python script under
//! `cProfile` and a Java program under JDK Flight Recorder.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use idecraft_profiler::{Engine, ProfileTarget, Unit, hot_spots, last_profile, profile};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-profiler-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn python() -> Option<String> {
    ["python3", "python"]
        .into_iter()
        .map(str::to_owned)
        .find(|p| {
            Command::new(p)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
}

fn has_cc() -> bool {
    Command::new("cc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A JDK (17 or newer) with `javac`: `JAVA_HOME`, else the macOS default.
fn jdk() -> Option<PathBuf> {
    let home = std::env::var("JAVA_HOME").ok().or_else(|| {
        let out = Command::new("/usr/libexec/java_home").output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    })?;
    let bin = Path::new(&home).join("bin");
    bin.join("javac").is_file().then_some(bin)
}

#[test]
fn java_program_under_flight_recorder() {
    let Some(bin) = jdk() else {
        eprintln!("skipped: no JDK");
        return;
    };
    let dir = scratch("java");
    let src = dir.join("src/main/java/demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("Busy.java"),
        "package demo;\n\npublic class Busy {\n    static long inner(long n) {\n        long s = 0;\n        for (long i = 0; i < n; i++) s += i % 7;\n        return s;\n    }\n    static long outer() {\n        long t = 0;\n        for (int k = 0; k < 400; k++) t += inner(1_000_000);\n        return t;\n    }\n    public static void main(String[] args) {\n        System.out.println(\"done \" + outer());\n    }\n}\n",
    )
    .unwrap();
    let javac = Command::new(bin.join("javac"))
        .args(["-d", "out", "src/main/java/demo/Busy.java"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(javac.success());
    let target = ProfileTarget {
        command: vec![
            bin.join("java").display().to_string(),
            "-cp".into(),
            "out".into(),
            "demo.Busy".into(),
        ],
        cwd: dir.clone(),
        env: Default::default(),
    };
    let out = dir.join("profiles");
    let run = profile(&target, &out, Duration::from_secs(60)).unwrap();
    assert_eq!(run.plan.engine, Engine::Jfr);
    assert_eq!(run.exit_code, Some(0));
    assert!(run.output.contains("done"), "{}", run.output);
    assert!(!run.output.contains("Started recording"), "{}", run.output);
    assert_eq!(run.profile.unit, Unit::Samples);
    assert!(run.profile.total > 50.0, "{}", run.profile.total);
    let hot = hot_spots(&run.profile);
    assert_eq!(hot[0].name, "Busy.inner", "{hot:?}");
    assert_eq!(hot[0].location, Some(("demo/Busy.java".into(), 6)));
    assert!(run.profile.roots[0].name.starts_with("Thread main"));
    // The reported package path is found under the source root.
    assert_eq!(
        idecraft_profiler::find_source(&dir, "demo/Busy.java"),
        Some(dir.join("src/main/java/demo/Busy.java"))
    );
    assert_eq!(last_profile(&out).unwrap(), run);
}

#[test]
fn python_script_under_cprofile() {
    let Some(py) = python() else {
        eprintln!("skipped: no python");
        return;
    };
    let dir = scratch("py");
    std::fs::write(
        dir.join("app.py"),
        "def inner(n):\n    return sum(i * i for i in range(n))\n\n\
def outer():\n    return [inner(20000) for _ in range(40)]\n\n\
outer()\nprint('done')\n",
    )
    .unwrap();
    let target = ProfileTarget {
        command: vec![py, "app.py".into()],
        cwd: dir.clone(),
        env: Default::default(),
    };
    let out = dir.join("profiles");
    let run = profile(&target, &out, Duration::from_secs(60)).unwrap();
    assert_eq!(run.plan.engine, Engine::CProfile);
    assert_eq!(run.exit_code, Some(0));
    assert!(run.output.contains("done"));
    assert_eq!(run.profile.unit, Unit::Seconds);
    let hot = hot_spots(&run.profile);
    assert!(hot.iter().any(|h| h.name == "inner"), "{hot:?}");
    let outer = hot.iter().find(|h| h.name == "outer").unwrap();
    assert_eq!(outer.location.as_ref().map(|l| l.1), Some(4));
    assert!(outer.location.as_ref().unwrap().0.ends_with("app.py"));
    assert_eq!(last_profile(&out).unwrap(), run);
}

#[test]
fn native_program_under_sample() {
    if !cfg!(target_os = "macos") || !Path::new(idecraft_profiler::SAMPLE).exists() || !has_cc() {
        eprintln!("skipped: needs macOS sample and cc");
        return;
    }
    let dir = scratch("c");
    std::fs::write(
        dir.join("busy.c"),
        "#include <stdio.h>\n\
volatile double sink;\n\
__attribute__((noinline)) static void inner(void) { for (long i = 0; i < 2000000; i++) sink += i * 0.5; }\n\
__attribute__((noinline)) static void outer(void) { for (int i = 0; i < 400; i++) inner(); }\n\
int main(void) { outer(); printf(\"done\\n\"); return 0; }\n",
    )
    .unwrap();
    let built = Command::new("cc")
        .args(["-g", "-O0", "-o", "busy", "busy.c"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(built.success());
    let target = ProfileTarget {
        command: vec!["./busy".into()],
        cwd: dir.clone(),
        env: Default::default(),
    };
    let run = profile(&target, &dir.join("profiles"), Duration::from_secs(60)).unwrap();
    assert_eq!(run.plan.engine, Engine::Sample);
    // A launchd service (the CI runner) has no Developer Tools permission; the profile says so.
    if run.profile.notes == [idecraft_profiler::SAMPLE_REFUSED] {
        eprintln!("skipped: {}", idecraft_profiler::SAMPLE_REFUSED);
        return;
    }
    assert_eq!(run.exit_code, Some(0));
    assert!(
        run.profile.total > 50.0,
        "{:?} {}ms; sample said: {}",
        run.profile.notes,
        run.millis,
        std::fs::read_to_string(dir.join("profiles/sample.log")).unwrap_or_default()
    );
    let hot = hot_spots(&run.profile);
    assert_eq!(hot[0].name, "inner", "{hot:?}");
    assert_eq!(
        hot[0].location.as_ref().map(|l| l.0.as_str()),
        Some("busy.c")
    );
}

#[test]
fn time_limit_ends_the_program() {
    let Some(py) = python() else {
        return;
    };
    let dir = scratch("limit");
    std::fs::write(dir.join("spin.py"), "while True:\n    pass\n").unwrap();
    let target = ProfileTarget {
        command: vec![py, "spin.py".into()],
        cwd: dir.clone(),
        env: Default::default(),
    };
    let err = profile(&target, &dir.join("profiles"), Duration::from_millis(500)).unwrap_err();
    assert!(err.contains("time limit"), "{err}");
}

#[test]
fn run_configuration_with_before_step() {
    let Some(py) = python() else {
        return;
    };
    let dir = scratch("config");
    std::fs::create_dir_all(dir.join(".idecraft")).unwrap();
    std::fs::write(
        dir.join("gen.py"),
        "open('data.txt', 'w').write('x' * 1000)\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.py"),
        "import os\ndef work():\n    return len(open('data.txt').read()) * len(os.environ['MODE'])\nprint(work())\n",
    )
    .unwrap();
    std::fs::write(
        dir.join(".idecraft/run.toml"),
        format!(
            "[[run]]\nname = \"gen\"\ncommand = [\"{py}\", \"gen.py\"]\n\n\
[[run]]\nname = \"main\"\ncommand = [\"{py}\", \"main.py\"]\nbefore = [\"gen\"]\nenv = {{ MODE = \"ab\" }}\n"
        ),
    )
    .unwrap();
    let configs = idecraft_core::load_run_configs(&dir);
    let run = idecraft_profiler::profile_config(&dir, &configs, "main").unwrap();
    assert_eq!(run.output.trim(), "2000");
    assert!(
        hot_spots(&run.profile).iter().any(|h| h.name == "work")
            || run
                .profile
                .roots
                .iter()
                .any(|r| format!("{r:?}").contains("work"))
    );
    assert!(
        dir.join(idecraft_profiler::PROFILES_DIR)
            .join("last.json")
            .exists()
    );
    assert!(idecraft_profiler::profile_config(&dir, &configs, "missing").is_err());
}

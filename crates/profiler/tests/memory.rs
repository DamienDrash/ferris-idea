//! Real runs for memory profiles (`SPEC-0250`), attaching (`SPEC-0251`), Gradle `run` and Linux
//! `perf` (`SPEC-0252`). Each test skips when its tool is missing.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use idecraft_profiler::{
    Engine, Kind, ProfileTarget, Unit, attach, hot_spots, last_profile, plan, profile_kind,
};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-profmem-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn works(program: &str, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Kills the child when dropped, so a failed assertion does not leave it running.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn target(command: &[&str], cwd: &Path) -> ProfileTarget {
    ProfileTarget {
        command: command.iter().map(|s| s.to_string()).collect(),
        cwd: cwd.to_path_buf(),
        env: Default::default(),
    }
}

fn compile(dir: &Path, name: &str, source: &str) -> bool {
    std::fs::write(dir.join(format!("{name}.c")), source).unwrap();
    Command::new("cc")
        .args(["-g", "-O0", "-o", name, &format!("{name}.c")])
        .current_dir(dir)
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn python_memory_under_tracemalloc() {
    if !works("python3", "--version") {
        eprintln!("skipped: no python3");
        return;
    }
    let dir = scratch("py");
    std::fs::write(
        dir.join("app.py"),
        "import sys\nkeep = []\n\ndef make(n):\n    return [str(i) * 20 for i in range(n)]\n\ndef main():\n    for _ in range(5):\n        keep.append(make(20000))\n    tmp = make(100000)\n    del tmp\n\nmain()\nprint('done', sys.argv[1:])\n",
    )
    .unwrap();
    let out = dir.join("profiles");
    let run = profile_kind(
        &target(&["python3", "app.py", "a"], &dir),
        &out,
        Duration::from_secs(60),
        Kind::Memory,
    )
    .unwrap();
    assert_eq!(run.plan.engine, Engine::Tracemalloc);
    assert_eq!(run.kind, Kind::Memory);
    assert_eq!(run.exit_code, Some(0));
    assert!(run.output.contains("done ['a']"), "{}", run.output);
    let p = &run.profile;
    assert_eq!(p.unit, Unit::Bytes);
    assert!(
        p.total > 5_000_000.0,
        "five lists of 20000 strings: {}",
        p.total
    );
    let hot = hot_spots(p);
    assert_eq!(hot[0].name, "make", "{hot:?}");
    assert_eq!(hot[0].location.as_ref().unwrap().1, 5);
    assert!(p.notes[0].contains("peak"), "{:?}", p.notes);
    assert_eq!(last_profile(&out).unwrap().kind, Kind::Memory);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_memory_on_macos_or_linux() {
    if !works("cc", "--version") {
        eprintln!("skipped: no C compiler");
        return;
    }
    if cfg!(target_os = "linux") && !works("valgrind", "--version") {
        eprintln!("skipped: no valgrind");
        return;
    }
    let dir = scratch("native");
    assert!(compile(
        &dir,
        "alloc",
        "#include <stdlib.h>\n#include <string.h>\nstatic void *keep[30000];\nvoid *grow(int n) { char *p = malloc(n); memset(p, 1, n); return p; }\nvoid build(void) { for (int i = 0; i < 20000; i++) keep[i] = grow(1000); }\nvoid churn(void) { for (int i = 0; i < 20000; i++) free(grow(64)); }\nint main(void) { build(); churn(); return 0; }\n",
    ));
    let run = profile_kind(
        &target(&["./alloc"], &dir),
        &dir.join("profiles"),
        Duration::from_secs(120),
        Kind::Memory,
    );
    let run = match run {
        Ok(r) => r,
        Err(e) if e.contains("Developer Tools") => {
            eprintln!("skipped: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let hot = hot_spots(&run.profile);
    assert_eq!(hot[0].name, "grow", "{hot:?}");
    assert!(hot[0].self_cost >= 19_000_000.0, "{}", hot[0].self_cost);
    let text = serde_json::to_string(&run.profile).unwrap();
    assert!(text.contains("\"build\""));
    if cfg!(target_os = "macos") {
        assert_eq!(run.plan.engine, Engine::Leaks);
        assert!(run.exit_unknown);
        // Hand test 180: the program's own functions have file and line through `atos`.
        if works("atos", "-h") || std::path::Path::new("/usr/bin/atos").exists() {
            let (file, line) = hot[0].location.clone().expect("grow has file:line");
            assert_eq!((file.as_str(), line), ("alloc.c", 4), "{hot:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whether perf may record another process of this user: `kernel.perf_event_paranoid` at most 2,
/// or root.
fn perf_may_record() -> bool {
    let level = std::fs::read_to_string("/proc/sys/kernel/perf_event_paranoid")
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(4);
    let root = Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0");
    level <= 2 || root
}

#[test]
fn attaches_to_a_running_program() {
    if !(cfg!(target_os = "macos") || cfg!(target_os = "linux")) || !works("cc", "--version") {
        eprintln!("skipped: no C compiler or platform");
        return;
    }
    if cfg!(target_os = "linux") && !works("perf", "--version") {
        eprintln!("skipped: no perf");
        return;
    }
    let dir = scratch("attach");
    assert!(compile(
        &dir,
        "spin",
        "#include <stdlib.h>\n#include <unistd.h>\nvolatile long x;\nvoid spin(void) { for (long i = 0; i < 400000000L; i++) x += i; }\nint main(void) { alarm(120); void *p[3000]; for (int i = 0; i < 3000; i++) p[i] = malloc(4096); write(1, \"r\\n\", 2); for (;;) spin(); }\n",
    ));
    // `alarm` ends the program even if the test process itself is killed.
    let mut running = Running(
        Command::new(dir.join("spin"))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let child = &mut running.0;
    // Attach only once the program is in its loop: under load a fixed wait can still find it in
    // the dynamic loader, and then the hottest function is not `spin`.
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(child.stdout.take().unwrap()),
        &mut ready,
    )
    .unwrap();
    assert_eq!(ready, "r\n");
    let out = dir.join("profiles");
    // A Linux kernel that keeps perf from recording other processes (`perf_event_paranoid` above
    // 2, the default of several distributions) is the machine's choice and is not lowered here:
    // the attach is then refused with a message that names the setting, and the program runs on.
    if cfg!(target_os = "linux") && !perf_may_record() {
        let refused = attach(child.id(), 2, Kind::Cpu, &out).unwrap_err();
        assert!(refused.contains("perf_event_paranoid"), "{refused}");
        assert!(
            child.try_wait().unwrap().is_none(),
            "the program keeps running"
        );
        eprintln!("perf may not record on this kernel: only the refusal was checked");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let cpu = attach(child.id(), 2, Kind::Cpu, &out).unwrap();
    assert_eq!(cpu.attached, Some(child.id()));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the program keeps running"
    );
    let refused = cpu
        .profile
        .notes
        .iter()
        .any(|n| n.contains("Developer Tools") || n.contains("perf_event_paranoid"));
    eprintln!("attach notes: {:?}", cpu.profile.notes);
    if !refused {
        assert_eq!(hot_spots(&cpu.profile)[0].name, "spin");
    }
    if cfg!(target_os = "macos") && !refused {
        let mem = attach(child.id(), 1, Kind::Memory, &out).unwrap();
        assert_eq!(mem.plan.engine, Engine::Heap);
        let plain = mem
            .profile
            .roots
            .iter()
            .find(|r| r.name == "non-object")
            .unwrap();
        assert!(plain.total >= 3000.0 * 4096.0, "{}", plain.total);
    }
    assert!(attach(9_999_999, 1, Kind::Cpu, &out).is_err());
    let pid = child.id();
    drop(running);
    let signalled = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    assert!(!signalled, "spin is still running");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gradle_run_under_flight_recorder() {
    if !works("gradle", "--version") || !works("java", "-version") {
        eprintln!("skipped: no Gradle or java");
        return;
    }
    let dir = scratch("gradle");
    let src = dir.join("src/main/java/demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(dir.join("settings.gradle"), "rootProject.name = 'busy'\n").unwrap();
    std::fs::write(
        dir.join("build.gradle"),
        "plugins { id 'application' }\napplication { mainClass = 'demo.Busy' }\n",
    )
    .unwrap();
    std::fs::write(
        src.join("Busy.java"),
        "package demo;\n\npublic class Busy {\n    static long inner(long n) {\n        long s = 0;\n        for (long i = 0; i < n; i++) s += i % 7;\n        return s;\n    }\n    public static void main(String[] args) {\n        long t = 0;\n        for (int k = 0; k < 600; k++) t += inner(1_000_000);\n        System.out.println(\"done \" + t);\n    }\n}\n",
    )
    .unwrap();
    let t = target(&["gradle", "-q", "--offline", "run"], &dir);
    assert_eq!(plan(&t).unwrap().engine, Engine::Jfr);
    let run = match profile_kind(
        &t,
        &dir.join("profiles"),
        Duration::from_secs(300),
        Kind::Cpu,
    ) {
        Ok(r) => r,
        Err(e) if e.contains("JDK 17") => {
            eprintln!("skipped: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    assert_eq!(run.exit_code, Some(0), "{}", run.output);
    let hot = hot_spots(&run.profile);
    assert_eq!(hot[0].name, "Busy.inner", "{hot:?}");
    assert_eq!(
        hot[0].location.as_ref().map(|l| l.0.as_str()),
        Some("demo/Busy.java")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_program_under_perf_on_linux() {
    if !cfg!(target_os = "linux") || !works("perf", "--version") || !works("cc", "--version") {
        eprintln!("skipped: needs Linux with perf");
        return;
    }
    let dir = scratch("perf");
    assert!(compile(
        &dir,
        "busy",
        "volatile long x;\nvoid inner(void) { for (long i = 0; i < 300000000L; i++) x += i; }\nint main(void) { inner(); return 0; }\n",
    ));
    match profile_kind(
        &target(&["./busy"], &dir),
        &dir.join("profiles"),
        Duration::from_secs(60),
        Kind::Cpu,
    ) {
        Ok(run) => assert_eq!(hot_spots(&run.profile)[0].name, "inner"),
        Err(e) => assert!(e.contains("perf_event_paranoid"), "{e}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

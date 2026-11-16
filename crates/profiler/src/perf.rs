//! Linux `perf` (`SPEC-0252` behaviour 1) and Gradle's `run` task under JDK Flight Recorder
//! (behaviour 3).

use std::path::Path;

use crate::jfr::RECORDING;
use crate::model::{Profile, StackFrame, StackTree, Unit};

/// `perf record` options: 1000 samples per second of CPU time, user space only, call graphs.
pub const PERF_RECORD: [&str; 6] = ["record", "-F", "1000", "-g", "-e", "cpu-clock:u"];

/// `perf script` fields: one header per sample (`name tid`), then the frames.
pub const PERF_SCRIPT: [&str; 2] = ["script", "-F"];

/// Field list for [`PERF_SCRIPT`].
pub const PERF_FIELDS: &str = "comm,tid,ip,sym,dso";

/// Note when the kernel refuses `perf`.
pub const PERF_REFUSED: &str = "the kernel did not allow perf to record (kernel.perf_event_paranoid \
is above 2); allow it with `sudo sysctl kernel.perf_event_paranoid=2` or ask the machine's owner";

/// Packages that bring `perf`.
pub const PERF_MISSING: &str = "native programs are profiled with `perf` on Linux, which is not \
installed: install `linux-perf` (Debian), `linux-tools-common` (Ubuntu) or `perf` (Fedora, Arch)";

/// One frame line: `55d1c2a0b139 inner+0x19 (/tmp/busy)`.
fn perf_frame(line: &str) -> Option<StackFrame> {
    let (addr, rest) = line.trim().split_once(' ')?;
    if !addr.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let rest = rest.trim();
    let (symbol, object) = match rest.rsplit_once(" (") {
        Some((s, o)) => (s.trim(), Some(o.trim_end_matches(')').to_owned())),
        None => (rest, None),
    };
    let name = match symbol.rsplit_once("+0x") {
        Some((n, off)) if off.chars().all(|c| c.is_ascii_hexdigit()) => n,
        _ => symbol,
    };
    let module = object.map(|o| {
        Path::new(&o)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .unwrap_or(o)
    });
    let name = if name == "[unknown]" {
        format!("[unknown] ({})", module.as_deref().unwrap_or("?"))
    } else {
        name.to_owned()
    };
    Some(StackFrame {
        name,
        module,
        location: None,
    })
}

/// Builds the profile from `perf script -F comm,tid,ip,sym,dso`: one root per thread, every
/// sample one millisecond.
pub fn parse_perf_script(text: &str) -> Profile {
    let mut tree = StackTree::default();
    let mut thread: Option<String> = None;
    let mut frames: Vec<StackFrame> = Vec::new();
    let mut finish = |thread: &mut Option<String>, frames: &mut Vec<StackFrame>| {
        if let Some(t) = thread.take()
            && !frames.is_empty()
        {
            // Frames come innermost first.
            frames.reverse();
            tree.add(&t, frames, 1.0);
        }
        frames.clear();
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            finish(&mut thread, &mut frames);
            continue;
        }
        // Frames are indented with a tab; the header pads the thread name with spaces.
        if line.starts_with('\t')
            && thread.is_some()
            && let Some(f) = perf_frame(line)
        {
            frames.push(f);
            continue;
        }
        finish(&mut thread, &mut frames);
        let header = line.trim().trim_end_matches(':');
        thread = Some(match header.rsplit_once(char::is_whitespace) {
            Some((name, tid)) if tid.parse::<u64>().is_ok() => format!("{} {tid}", name.trim()),
            _ => header.to_owned(),
        });
    }
    finish(&mut thread, &mut frames);
    let mut profile = tree.profile("perf", Unit::Samples);
    if profile.roots.is_empty() {
        profile
            .notes
            .push("perf recorded no samples (the program ended at once?)".into());
    }
    profile
}

/// Whether `command` runs Gradle's `run` task (`gradle run`, `./gradlew :app:run`).
pub fn is_gradle_run(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    matches!(base, "gradle" | "gradlew" | "gradlew.bat")
        && command[1..]
            .iter()
            .any(|a| a == "run" || (a.ends_with(":run") && !a.starts_with('-')))
}

/// Text of the Gradle init script that records every `JavaExec` task into `out`.
pub fn gradle_init_script(out: &Path) -> String {
    let escape = |s: &str| s.replace('\\', "\\\\").replace('\'', "\\'");
    let args: Vec<String> = RECORDING
        .iter()
        .map(|a| {
            format!(
                "'{}'",
                escape(&a.replace("{out}", &out.display().to_string()))
            )
        })
        .collect();
    format!(
        "// Written by IDECraft's profiler (SPEC-0252): records the application's JVM.\nallprojects {{\n    tasks.withType(JavaExec).configureEach {{\n        jvmArgs({})\n    }}\n}}\n",
        args.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "            busy 4321
\t    5581d1a0b139 inner+0x19 (/tmp/busy)
\t    5581d1a0b170 outer+0x20 (/tmp/busy)
\t    5581d1a0b1a5 main+0x15 (/tmp/busy)
\t    7f3a1c029d90 __libc_start_call_main+0x80 (/usr/lib/x86_64-linux-gnu/libc.so.6)

busy 4321
\t    5581d1a0b139 inner+0x19 (/tmp/busy)
\t    5581d1a0b170 outer+0x20 (/tmp/busy)
\t    5581d1a0b1a5 main+0x15 (/tmp/busy)
\t    7f3a1c029d90 __libc_start_call_main+0x80 (/usr/lib/x86_64-linux-gnu/libc.so.6)

worker thread 4322
\t    7f3a1c0aa000 [unknown] (/usr/lib/libfoo.so)
\t    5581d1a0b1a5 main+0x15 (/tmp/busy)
";

    #[test]
    fn perf_samples_become_threads_from_the_outermost_frame() {
        let p = parse_perf_script(SCRIPT);
        assert_eq!((p.engine.as_str(), p.total), ("perf", 3.0));
        assert_eq!(p.roots[0].name, "busy 4321");
        let start = &p.roots[0].children[0];
        assert_eq!(start.name, "__libc_start_call_main");
        assert_eq!(start.module.as_deref(), Some("libc.so.6"));
        let inner = &start.children[0].children[0].children[0];
        assert_eq!((inner.name.as_str(), inner.self_cost), ("inner", 2.0));
        assert_eq!(p.roots[1].name, "worker thread 4322");
        assert_eq!(
            p.roots[1].children[0].children[0].name,
            "[unknown] (libfoo.so)"
        );
        assert!(parse_perf_script("").notes[0].contains("no samples"));
    }

    #[test]
    fn gradle_run_is_recognised_and_recorded() {
        let cmd = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert!(is_gradle_run(&cmd(&["gradle", "-q", "run"])));
        assert!(is_gradle_run(&cmd(&["./gradlew", ":app:run", "--args=x"])));
        assert!(!is_gradle_run(&cmd(&["gradle", "build"])));
        assert!(!is_gradle_run(&cmd(&["make", "run"])));
        let text = gradle_init_script(Path::new("/tmp/it's/last.jfr"));
        assert!(text.contains("tasks.withType(JavaExec)"));
        assert!(text.contains("'-XX:StartFlightRecording=filename=/tmp/it\\'s/last.jfr,"));
    }
}

//! Java programs under JDK Flight Recorder (`SPEC-0136`): the JVM records one execution sample per
//! millisecond, and the JDK's own `jfr print` turns the recording into text, which becomes one call
//! tree per thread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{LineCosts, Profile, ProfileNode, Unit};

/// JVM option that records execution samples every millisecond into `{out}` (JDK 17 or newer);
/// the recorder's start-up messages are kept out of the program's output.
pub const RECORDING: [&str; 2] = [
    "-Xlog:jfr+startup=error",
    "-XX:StartFlightRecording=filename={out},settings=profile,jdk.ExecutionSample#period=1ms",
];

/// Arguments of `jfr` that print the samples with their whole stacks.
pub const PRINT: [&str; 5] = [
    "print",
    "--events",
    "jdk.ExecutionSample",
    "--stack-depth",
    "64",
];

/// One frame as `jfr print` writes it: `demo.Busy.inner(long) line: 6`.
#[derive(Debug, Clone)]
struct Frame {
    /// `Busy.inner`.
    name: String,
    /// `demo`, if the class has a package.
    package: Option<String>,
    /// `demo/Busy.java` and the line, if the frame has one.
    location: Option<(String, u32)>,
}

fn frame(line: &str) -> Option<Frame> {
    let line = line.trim();
    let (method, rest) = line.split_once('(')?;
    let number = rest
        .rsplit_once("line:")
        .and_then(|(_, n)| n.trim().parse::<u32>().ok());
    let (class, method) = method.rsplit_once('.')?;
    let (package, simple) = match class.rsplit_once('.') {
        Some((p, s)) => (Some(p.to_owned()), s),
        None => (None, class),
    };
    // The source file is named after the outermost class: `Busy$Inner` lives in `Busy.java`.
    let outer = simple.split('$').next().unwrap_or(simple);
    let file = match &package {
        Some(p) => format!("{}/{outer}.java", p.replace('.', "/")),
        None => format!("{outer}.java"),
    };
    Some(Frame {
        name: format!("{simple}.{method}"),
        package,
        location: number.map(|n| (file, n)),
    })
}

/// A method in a calling context: name, package and source file (lines differ between samples).
type Key = (String, Option<String>, Option<String>);

/// A tree of methods with sample counts while reading.
#[derive(Default)]
struct Tree {
    count: u64,
    /// Samples per line; the busiest line is where the method's row jumps to.
    lines: BTreeMap<u32, u64>,
    children: BTreeMap<Key, Tree>,
}

impl Tree {
    fn node(self, (name, package, file): Key) -> ProfileNode {
        let mut node = ProfileNode::leaf(&name, self.count as f64);
        node.module = package;
        let line = self
            .lines
            .iter()
            .max_by_key(|(line, n)| (**n, std::cmp::Reverse(**line)))
            .map(|(line, _)| *line);
        node.location = file.zip(line);
        node.children = self.children.into_iter().map(|(k, t)| t.node(k)).collect();
        node
    }
}

/// Builds the profile from the text of `jfr print` ([`PRINT`]). Every sample counts one
/// millisecond of its thread; samples of the recorder itself (`jdk.jfr.` frames, while it starts)
/// are left out.
pub fn parse_jfr(text: &str) -> Profile {
    let mut threads: BTreeMap<String, Tree> = BTreeMap::new();
    let mut thread = String::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut in_stack = false;
    let mut own = false;
    let mut total = 0u64;
    // Every line of a sample's stack, once per sample (SPEC-0136 v3 behaviour 7).
    let mut lines = LineCosts::default();
    for line in text.lines() {
        let t = line.trim();
        if t == "jdk.ExecutionSample {" {
            thread.clear();
            stack.clear();
            own = false;
        } else if let Some(rest) = t.strip_prefix("sampledThread = \"") {
            thread = rest.split('"').next().unwrap_or("").to_owned();
        } else if t == "stackTrace = [" {
            in_stack = true;
        } else if in_stack && t == "]" {
            in_stack = false;
        } else if in_stack {
            if t.starts_with("jdk.jfr.") {
                own = true;
            }
            if let Some(f) = frame(t) {
                stack.push(f);
            }
        } else if t == "}" && !stack.is_empty() && !own {
            total += 1;
            lines.add(
                stack
                    .iter()
                    .rev()
                    .map(|f| (f.name.as_str(), f.location.as_ref())),
                1.0,
            );
            let mut node = threads.entry(thread.clone()).or_default();
            node.count += 1;
            // Frames come innermost first.
            for f in stack.iter().rev() {
                let file = f.location.as_ref().map(|(file, _)| file.clone());
                node = node
                    .children
                    .entry((f.name.clone(), f.package.clone(), file))
                    .or_default();
                node.count += 1;
                if let Some((_, line)) = &f.location {
                    *node.lines.entry(*line).or_default() += 1;
                }
            }
            stack.clear();
        }
    }
    let mut roots: Vec<ProfileNode> = threads
        .into_iter()
        .map(|(name, t)| {
            let mut n = t.node((format!("Thread {name}"), None, None));
            n.finish();
            n
        })
        .collect();
    roots.sort_by(|a, b| {
        b.total
            .partial_cmp(&a.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Profile {
        engine: "JFR".into(),
        unit: Unit::Samples,
        total: total as f64,
        roots,
        notes: Vec::new(),
        lines: lines.finish(),
    }
}

/// Whether `program` starts a JVM directly.
pub fn is_java(program: &str) -> bool {
    Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "java" || n == "java.exe")
}

/// The `jfr` tool of the JDK that runs `java`: next to it (following links), else in `JAVA_HOME`
/// (the configuration's, then the environment's), else the macOS default JDK, else on `PATH`.
pub fn jfr_tool(java: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let java = Path::new(java);
    if java.components().count() > 1 {
        let real = java.canonicalize().unwrap_or_else(|_| java.to_path_buf());
        if let Some(dir) = real.parent() {
            candidates.push(dir.join("jfr"));
        }
    }
    let home = env
        .get("JAVA_HOME")
        .cloned()
        .or_else(|| std::env::var("JAVA_HOME").ok());
    if let Some(home) = home {
        candidates.push(Path::new(&home).join("bin/jfr"));
    }
    if cfg!(target_os = "macos")
        && let Ok(out) = std::process::Command::new("/usr/libexec/java_home").output()
        && out.status.success()
    {
        let home = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        candidates.push(Path::new(&home).join("bin/jfr"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join("jfr")));
    }
    candidates.into_iter().find(|c| c.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINTED: &str = "jdk.ExecutionSample {
  startTime = 15:17:40.299
  sampledThread = \"main\" (javaThreadId = 1)
  state = \"STATE_RUNNABLE\"
  stackTrace = [
    jdk.jfr.internal.Utils.isSettingVisible(Control, boolean) line: 617
    jdk.jfr.Recording.start() line: 184
  ]
}

jdk.ExecutionSample {
  startTime = 15:17:40.304
  sampledThread = \"main\" (javaThreadId = 1)
  state = \"STATE_RUNNABLE\"
  stackTrace = [
    demo.Busy.inner(long) line: 6
    demo.Busy.outer() line: 11
    demo.Busy.main(String[]) line: 15
  ]
}

jdk.ExecutionSample {
  sampledThread = \"main\" (javaThreadId = 1)
  stackTrace = [
    demo.Busy.inner(long) line: 6
    demo.Busy.outer() line: 11
    demo.Busy.main(String[]) line: 15
  ]
}

jdk.ExecutionSample {
  sampledThread = \"main\" (javaThreadId = 1)
  stackTrace = [
    demo.Busy$Cache.get(Object) line: 30
    demo.Busy.main(String[]) line: 16
  ]
}

jdk.ExecutionSample {
  sampledThread = \"pool-1-thread-1\" (javaThreadId = 21)
  stackTrace = [
    java.lang.Thread.sleep(long)
    Plain.run() line: 3
  ]
}
";

    #[test]
    fn samples_become_a_tree_per_thread_without_the_recorder() {
        let p = parse_jfr(PRINTED);
        assert_eq!(p.total, 4.0);
        assert_eq!(p.roots[0].name, "Thread main");
        let main = &p.roots[0].children[0];
        assert_eq!(main.name, "Busy.main");
        assert_eq!(main.module.as_deref(), Some("demo"));
        assert_eq!(main.location, Some(("demo/Busy.java".into(), 15)));
        assert_eq!(main.total, 3.0);
        let outer = &main.children[0];
        assert_eq!((outer.name.as_str(), outer.total), ("Busy.outer", 2.0));
        assert_eq!(outer.children[0].self_cost, 2.0);
        // Nested classes live in the outer class's file; a native frame has no line.
        let cache = &main.children[1];
        assert_eq!(cache.location, Some(("demo/Busy.java".into(), 30)));
        let worker = &p.roots[1];
        assert_eq!(worker.name, "Thread pool-1-thread-1");
        assert_eq!(worker.children[0].location, Some(("Plain.java".into(), 3)));
        assert_eq!(worker.children[0].children[0].location, None);
        let hot = crate::hot_spots(&p);
        assert_eq!(hot[0].name, "Busy.inner");
        assert_eq!(hot[0].self_cost, 2.0);
    }

    #[test]
    fn java_programs_are_recognised() {
        assert!(is_java("java"));
        assert!(is_java("/opt/jdk/bin/java"));
        assert!(!is_java("javac"));
        assert!(!is_java("kotlin"));
    }

    // SPEC-0136 v3 behaviour 7: every line of a sample once, self at the innermost line; the JDK's
    // frames are outside the project.
    #[test]
    fn lines_count_once_per_sample_and_jdk_frames_are_outside() {
        let text = "jdk.ExecutionSample {
  sampledThread = \"main\" (javaThreadId = 1)
  stackTrace = [
    java.util.Objects.equals(Object, Object) line: 64
    demo.Tree.walk(Node) line: 9
    demo.Tree.walk(Node) line: 9
    demo.Tree.main(String[]) line: 20
  ]
}

jdk.ExecutionSample {
  sampledThread = \"main\" (javaThreadId = 1)
  stackTrace = [
    demo.Tree.walk(Node) line: 7
    demo.Tree.main(String[]) line: 20
  ]
}
";
        let mut p = parse_jfr(text);
        let lines = p.line_costs();
        let get = |f: &str, l: u32| {
            lines
                .iter()
                .find(|c| c.file == f && c.line == l)
                .map(|c| (c.total, c.self_cost))
        };
        assert_eq!(get("demo/Tree.java", 9), Some((1.0, 0.0)), "recursion once");
        assert_eq!(get("demo/Tree.java", 7), Some((1.0, 1.0)));
        assert_eq!(get("demo/Tree.java", 20), Some((2.0, 0.0)));
        assert_eq!(get("java/util/Objects.java", 64), Some((1.0, 1.0)));
        // The method's row keeps its busiest line; the lines keep all of them.
        let root = crate::project::tests::workspace("jfr-lines", &["src/main/java/demo/Tree.java"]);
        let mut index = crate::SourceIndex::new(&root);
        p.mark_project(&mut |f| index.in_project(f));
        let hot = crate::hot_spots(&p);
        let equals = hot.iter().find(|h| h.name == "Objects.equals").unwrap();
        let walk = hot.iter().find(|h| h.name == "Tree.walk").unwrap();
        assert!(!equals.in_project && walk.in_project);
        assert!(p.roots[0].is_thread());
        let _ = std::fs::remove_dir_all(&root);
    }
}

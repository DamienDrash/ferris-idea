//! Attaching to a running process (`SPEC-0251`): the user's processes, the rule that JetBrains
//! IDEs are never attached to, sampling from outside and heap snapshots.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::model::{Profile, ProfileNode, Unit};
use crate::perf::{PERF_FIELDS, PERF_MISSING, PERF_RECORD, PERF_REFUSED, PERF_SCRIPT};
use crate::run::{Engine, Kind, ProfilePlan, ProfileRun, SAMPLE, SAMPLE_REFUSED};
use crate::{parse_perf_script, parse_sample};

/// The macOS heap reader.
pub const HEAP: &str = "/usr/bin/heap";

/// Longest recording of a running process.
pub const MAX_ATTACH_SECONDS: u64 = 120;

/// One of the user's processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// Process id.
    pub pid: u32,
    /// Command line.
    pub command: String,
    /// Why it cannot be attached to, if it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// Why `command` must not be attached to: JetBrains IDEs are the reference and off limits
/// (`RESEARCH_BOUNDARIES.md`, behaviour 2).
pub fn reference_refusal(command: &str) -> Option<String> {
    let lower = command.to_lowercase();
    let program = command.split_whitespace().next().unwrap_or("");
    let base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    let marked = ["jetbrains", "intellij", "idea.vmoptions", "com.intellij"]
        .iter()
        .any(|m| lower.contains(m))
        || matches!(base, "idea" | "idea.sh" | "idea64" | "idea64.exe");
    marked.then(|| {
        "a JetBrains IDE (the reference) is never profiled or attached to (RESEARCH_BOUNDARIES)"
            .to_owned()
    })
}

/// Parses `ps -o pid=,args=` lines.
pub fn parse_ps(text: &str, own: u32) -> Vec<ProcessInfo> {
    let mut out: Vec<ProcessInfo> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, command) = l.split_once(char::is_whitespace)?;
            let pid: u32 = pid.parse().ok()?;
            let command = command.trim().to_owned();
            (pid != own && !command.is_empty()).then(|| ProcessInfo {
                pid,
                refused: reference_refusal(&command),
                command,
            })
        })
        .collect();
    // Newest first: process ids grow over time (until they wrap).
    out.sort_by_key(|a| std::cmp::Reverse(a.pid));
    out
}

fn uid() -> Option<String> {
    let out = Command::new("id").arg("-u").output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|s| !s.is_empty())
}

/// The user's own processes, newest first, without this one (behaviour 1).
pub fn processes() -> Result<Vec<ProcessInfo>, String> {
    let uid = uid().ok_or("cannot read the user id")?;
    let out = Command::new("ps")
        .args(["-U", &uid, "-o", "pid=,args="])
        .output()
        .map_err(|e| format!("ps: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(parse_ps(
        &String::from_utf8_lossy(&out.stdout),
        std::process::id(),
    ))
}

/// The process `pid` if it is the user's and may be attached to.
pub fn attachable(pid: u32) -> Result<ProcessInfo, String> {
    let found = processes()?
        .into_iter()
        .find(|p| p.pid == pid)
        .ok_or_else(|| format!("no process {pid} of this user"))?;
    match &found.refused {
        Some(why) => Err(format!("process {pid}: {why}")),
        None => Ok(found),
    }
}

/// Parses the class table of `heap <pid>` (behaviour 4): one root per class with its bytes.
pub fn parse_heap(text: &str) -> Profile {
    let mut roots = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("COUNT") && t.contains("CLASS_NAME") {
            in_table = true;
            continue;
        }
        if !in_table || t.starts_with('=') {
            continue;
        }
        if t.is_empty() {
            if !roots.is_empty() {
                break;
            }
            continue;
        }
        let mut parts = t.split_whitespace();
        let (Some(count), Some(bytes), Some(_avg)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (Ok(count), Ok(bytes)) = (count.parse::<u64>(), bytes.parse::<f64>()) else {
            continue;
        };
        // The rest is `CLASS_NAME  TYPE  BINARY`, separated by two or more spaces.
        let mut rest = t;
        for _ in 0..3 {
            rest = rest.trim_start();
            rest = &rest[rest.find(char::is_whitespace).unwrap_or(rest.len())..];
        }
        let fields: Vec<&str> = rest
            .split("  ")
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .collect();
        let mut node = ProfileNode::leaf(fields.first().copied().unwrap_or("?"), bytes);
        node.calls = Some(count);
        node.module = fields.get(2).or(fields.get(1)).map(|m| (*m).to_owned());
        roots.push(node);
    }
    roots.sort_by(|a, b| {
        b.total
            .partial_cmp(&a.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = roots.iter().map(|r| r.total).sum();
    let mut notes = Vec::new();
    if roots.is_empty() {
        notes.push("heap reported no allocations".into());
    }
    Profile {
        engine: "heap".into(),
        unit: Unit::Bytes,
        total,
        roots,
        notes,
        lines: Vec::new(),
    }
}

/// The plan of attaching to `pid` for `seconds` (behaviours 3–4), without running it.
pub fn attach_plan(pid: u32, seconds: u64, kind: Kind) -> Result<ProfilePlan, String> {
    let secs = seconds.clamp(1, MAX_ATTACH_SECONDS).to_string();
    let pid = pid.to_string();
    let (engine, command): (Engine, Vec<String>) = match kind {
        Kind::Cpu if cfg!(target_os = "macos") => (
            Engine::Sample,
            [SAMPLE, &pid, &secs, "1", "-mayDie", "-f", "{out}"]
                .map(str::to_owned)
                .to_vec(),
        ),
        Kind::Cpu if cfg!(target_os = "linux") => {
            if !crate::run::perf_runs() {
                return Err(PERF_MISSING.into());
            }
            let mut c: Vec<String> = vec!["perf".into()];
            c.extend(PERF_RECORD.map(str::to_owned));
            c.extend(["-p", &pid, "-o", "{out}", "--", "sleep", &secs].map(str::to_owned));
            (Engine::Perf, c)
        }
        Kind::Memory if cfg!(target_os = "macos") => {
            (Engine::Heap, [HEAP, "-s", &pid].map(str::to_owned).to_vec())
        }
        Kind::Memory if cfg!(target_os = "linux") => {
            return Err("Linux has no tool that reads another process's heap without rights; profile memory by starting the program (Massif)".into());
        }
        _ => return Err("attaching is supported on macOS and Linux".into()),
    };
    Ok(ProfilePlan {
        engine,
        command,
        env: Default::default(),
    })
}

/// Attaches to `pid` for `seconds` and keeps the result as `dir/last.json` (behaviours 3–5).
pub fn attach(pid: u32, seconds: u64, kind: Kind, dir: &Path) -> Result<ProfileRun, String> {
    let process = attachable(pid)?;
    let mut plan = attach_plan(pid, seconds, kind)?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let raw = dir.join(match plan.engine {
        Engine::Perf => "last.perf.data",
        Engine::Heap => "last.heap.txt",
        _ => "last.sample.txt",
    });
    let _ = fs::remove_file(&raw);
    for part in &mut plan.command {
        *part = part.replace("{out}", &raw.display().to_string());
    }
    let start = Instant::now();
    let out = Command::new(&plan.command[0])
        .args(&plan.command[1..])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", plan.command[0]))?;
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    let mut profile = match plan.engine {
        Engine::Heap => {
            if !out.status.success() {
                return Err(
                    if said.contains("cannot examine") || said.contains("permission") {
                        SAMPLE_REFUSED.replace("/usr/bin/sample", HEAP)
                    } else {
                        format!("heap: {}", said.trim())
                    },
                );
            }
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let _ = fs::write(&raw, &text);
            parse_heap(&text)
        }
        Engine::Perf => {
            if said.contains("perf_event_paranoid") {
                return Err(PERF_REFUSED.into());
            }
            let printed = Command::new("perf")
                .args(PERF_SCRIPT)
                .arg(PERF_FIELDS)
                .arg("-i")
                .arg(&raw)
                .output()
                .map_err(|e| format!("perf: {e}"))?;
            parse_perf_script(&String::from_utf8_lossy(&printed.stdout))
        }
        _ => {
            let mut p = parse_sample(&fs::read_to_string(&raw).unwrap_or_default());
            if p.roots.is_empty() && said.contains("cannot examine") {
                p.notes = vec![SAMPLE_REFUSED.into()];
            }
            p
        }
    };
    profile.notes.push(format!("attached to process {pid}"));
    let mut run = ProfileRun {
        plan: ProfilePlan {
            command: vec![format!("pid {pid}: {}", process.command)],
            ..plan
        },
        exit_code: None,
        timed_out: false,
        millis: start.elapsed().as_millis() as u64,
        output: String::new(),
        profile,
        kind,
        exit_unknown: true,
        attached: Some(pid),
        sources: Default::default(),
        marked: false,
    };
    crate::run::store(dir, &mut run)?;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_ide_is_recognised() {
        for refused in [
            "/Applications/IntelliJ IDEA.app/Contents/MacOS/idea",
            "/Users/me/Applications/IntelliJ IDEA Ultimate.app/Contents/MacOS/idea",
            "/opt/idea-IU-243/bin/idea.sh",
            "/opt/idea/jbr/bin/java -Djb.vmOptionsFile=/opt/idea/bin/idea64.vmoptions -cp x com.intellij.idea.Main",
            "/home/me/.local/share/JetBrains/Toolbox/apps/intellij-idea-ultimate/bin/fsnotifier",
            "idea64.exe",
        ] {
            assert!(reference_refusal(refused).is_some(), "{refused}");
        }
        for allowed in [
            "java -cp out demo.Busy",
            "python3 server.py --idea 3",
            "/usr/bin/ideaview",
            "./busy",
        ] {
            assert!(reference_refusal(allowed).is_none(), "{allowed}");
        }
    }

    #[test]
    fn ps_lines_are_listed_newest_first_without_ourselves() {
        let list = parse_ps(
            "  101 /bin/zsh -l\n  205 ./busy 7\n   99 /Applications/IntelliJ IDEA.app/Contents/MacOS/idea\n  300 ferris-idea ui\n",
            300,
        );
        let pids: Vec<u32> = list.iter().map(|p| p.pid).collect();
        assert_eq!(pids, [205, 101, 99]);
        assert_eq!(list[0].command, "./busy 7");
        assert!(list[2].refused.is_some());
    }

    const HEAP_TEXT: &str = "Process 73625: 2 zones

All zones: 1198 nodes (2596400 bytes)

   COUNT      BYTES       AVG   CLASS_NAME                                        TYPE    BINARY
   =====      =====       ===   ==========                                        ====    ======
     492    2547936    5178.7   non-object
     199       6368      32.0   Metaclass.data (metaclass_rw_v2_t)                C       libobjc.A.dylib
       5        240      48.0   xpc_string_t                                      ObjC    libxpc.dylib

";

    #[test]
    fn heap_classes_become_roots() {
        let p = parse_heap(HEAP_TEXT);
        assert_eq!((p.engine.as_str(), p.unit), ("heap", Unit::Bytes));
        assert_eq!(p.total, 2547936.0 + 6368.0 + 240.0);
        assert_eq!(p.roots[0].name, "non-object");
        assert_eq!(p.roots[0].calls, Some(492));
        assert_eq!(p.roots[1].name, "Metaclass.data (metaclass_rw_v2_t)");
        assert_eq!(p.roots[1].module.as_deref(), Some("libobjc.A.dylib"));
        assert!(parse_heap("nothing").notes[0].contains("no allocations"));
    }

    #[test]
    fn attach_plans_per_platform() {
        let cpu = attach_plan(42, 500, Kind::Cpu);
        let mem = attach_plan(42, 5, Kind::Memory);
        if cfg!(target_os = "macos") {
            let cpu = cpu.unwrap();
            assert_eq!(cpu.engine, Engine::Sample);
            assert_eq!(&cpu.command[1..3], ["42", "120"], "at most 120 s");
            assert_eq!(mem.unwrap().command, [HEAP, "-s", "42"]);
        } else if cfg!(target_os = "linux") {
            assert!(mem.unwrap_err().contains("Massif"));
            if let Ok(cpu) = cpu {
                assert!(cpu.command.contains(&"-p".to_owned()));
            }
        }
    }
}

//! Memory profiles (`SPEC-0250`): Python's `tracemalloc` through a small runner, macOS
//! `malloc_history` call trees of a `leaks` memory graph, and Valgrind Massif's peak snapshot on
//! Linux. Costs are bytes.

use serde::Deserialize;

use crate::model::{Profile, StackFrame, StackTree, Unit};

/// Root of every memory call tree.
pub const ALL_ALLOCATIONS: &str = "all allocations";

/// Runs a Python program under `tracemalloc` (behaviour 2). Arguments: output file, `path` or
/// `-m`, the script or module, then the program's own arguments. Writes the allocation sites
/// still held when the program ends as JSON: `{current, peak, stats: [{size, count, frames:
/// [[file, line, function]]}]}`, frames from the outermost call down.
pub const TRACEMALLOC_RUNNER: &str = r#"import ast, json, os, runpy, sys, tracemalloc
out, mode = sys.argv[1], sys.argv[2]
del sys.argv[:3]
if mode == "-m":
    sys.path[0] = os.getcwd()
else:
    sys.path[0] = os.path.dirname(os.path.abspath(sys.argv[0]))
names = {}
def function(file, line):
    if file not in names:
        spans = []
        try:
            with open(file, "rb") as f:
                tree = ast.parse(f.read())
            for n in ast.walk(tree):
                if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                    spans.append((n.lineno, getattr(n, "end_lineno", n.lineno), n.name))
        except Exception:
            pass
        names[file] = spans
    best = None
    for a, b, name in names[file]:
        if a <= line <= b and (best is None or a >= best[0]):
            best = (a, name)
    return best[1] if best else "<module>"
own = (os.path.abspath(__file__), runpy.__file__)
tracemalloc.start(64)
held = None
try:
    if mode == "-m":
        held = runpy.run_module(sys.argv[0], run_name="__main__", alter_sys=True)
    else:
        held = runpy.run_path(sys.argv[0], run_name="__main__")
except SystemExit:
    pass
finally:
    snapshot = tracemalloc.take_snapshot()
    current, peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    stats = []
    for s in snapshot.statistics("traceback"):
        frames = [f for f in s.traceback]
        while frames and (os.path.abspath(frames[0].filename) in own or frames[0].filename.startswith("<frozen")):
            frames.pop(0)
        if not frames:
            continue
        stats.append({"size": s.size, "count": s.count,
                      "frames": [[f.filename, f.lineno, function(f.filename, f.lineno)] for f in frames]})
    with open(out, "w") as f:
        json.dump({"current": current, "peak": peak, "stats": stats}, f)
"#;

#[derive(Deserialize)]
struct Traced {
    current: u64,
    peak: u64,
    stats: Vec<TracedStat>,
}

#[derive(Deserialize)]
struct TracedStat {
    size: u64,
    frames: Vec<(String, u32, String)>,
}

/// Bytes as a short size: `512 B`, `12.3 KB`, `4.0 MB` (powers of 1024).
pub fn size_label(bytes: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut at = 0;
    while value >= 1024.0 && at + 1 < units.len() {
        value /= 1024.0;
        at += 1;
    }
    if at == 0 {
        format!("{value:.0} B")
    } else {
        format!("{value:.1} {}", units[at])
    }
}

/// Builds the profile from the runner's JSON ([`TRACEMALLOC_RUNNER`]).
pub fn parse_tracemalloc(json: &str) -> Result<Profile, String> {
    let traced: Traced =
        serde_json::from_str(json).map_err(|e| format!("tracemalloc output: {e}"))?;
    let mut tree = StackTree::default();
    for s in &traced.stats {
        let frames: Vec<StackFrame> = s
            .frames
            .iter()
            .map(|(file, line, name)| StackFrame {
                name: name.clone(),
                module: None,
                location: Some((file.clone(), *line)),
            })
            .collect();
        tree.add(ALL_ALLOCATIONS, &frames, s.size as f64);
    }
    let mut profile = tree.profile("tracemalloc", Unit::Bytes);
    profile.notes.push(format!(
        "{} still traced when the program ended; peak {}",
        size_label(traced.current as f64),
        size_label(traced.peak as f64)
    ));
    Ok(profile)
}

/// `112 bytes`, `793K`, `19.5M`, `1.2G` as bytes.
fn parse_size(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Some(n) = t.strip_suffix("bytes").or_else(|| t.strip_suffix("byte")) {
        return n.trim().parse().ok();
    }
    let (number, factor) = match t.chars().last()? {
        'K' => (&t[..t.len() - 1], 1024.0),
        'M' => (&t[..t.len() - 1], 1024.0 * 1024.0),
        'G' => (&t[..t.len() - 1], 1024.0 * 1024.0 * 1024.0),
        _ => (t, 1.0),
    };
    number.trim().parse::<f64>().ok().map(|n| n * factor)
}

/// One line of `malloc_history -callTree`: `  + ! 20000 (19.5M) grow  (in alloc) + 24  [0x…]`.
fn history_line(line: &str) -> Option<(usize, f64, StackFrame)> {
    let at = line.find(|c: char| c.is_ascii_digit())?;
    if !line[..at].chars().all(|c| " +!:|".contains(c)) {
        return None;
    }
    let rest = &line[at..];
    let (_count, rest) = rest.split_once(" (")?;
    let (size, rest) = rest.split_once(") ")?;
    let bytes = parse_size(size)?;
    let rest = rest.trim();
    let (name, tail) = match rest.find("  (in ") {
        Some(i) => (&rest[..i], &rest[i + 2..]),
        None => (rest, ""),
    };
    let module = tail
        .strip_prefix("(in ")
        .and_then(|t| t.split_once(')'))
        .map(|(m, _)| m.to_owned());
    let location = tail.rsplit_once("]  ").and_then(|(_, loc)| {
        let (file, line) = loc.trim().rsplit_once(':')?;
        Some((file.to_owned(), line.trim().parse::<u32>().ok()?))
    });
    Some((
        at,
        bytes,
        StackFrame {
            name: name.trim().to_owned(),
            module,
            location,
        },
    ))
}

/// Builds the profile from `malloc_history <memgraph> -callTree` (behaviour 3): bytes still
/// allocated at the end, by call path; the malloc library's own frames are left out.
pub fn parse_malloc_history(text: &str) -> Profile {
    // (column, bytes, frame, bytes of the children seen so far)
    let mut stack: Vec<(usize, f64, StackFrame, f64)> = Vec::new();
    let mut tree = StackTree::default();
    let mut notes = Vec::new();
    let flush = |stack: &mut Vec<(usize, f64, StackFrame, f64)>, tree: &mut StackTree| {
        if let Some((_, bytes, _, below)) = stack.last().cloned() {
            let own = bytes - below;
            if own > 0.0 {
                let frames: Vec<StackFrame> = stack
                    .iter()
                    .map(|(_, _, f, _)| f.clone())
                    .filter(|f| {
                        f.name != "<< TOTAL >>"
                            && f.module.as_deref() != Some("libsystem_malloc.dylib")
                    })
                    .collect();
                tree.add(ALL_ALLOCATIONS, &frames, own);
            }
            stack.pop();
            if let Some(parent) = stack.last_mut() {
                parent.3 += bytes;
            }
        }
    };
    let mut in_graph = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(peak) = t.strip_prefix("Physical footprint (peak):") {
            notes.push(format!("peak physical footprint {}", peak.trim()));
        } else if t.starts_with("Malloc blocks with no stack:") {
            notes.push(t.split_whitespace().collect::<Vec<_>>().join(" "));
        }
        if t.starts_with("Call graph:") {
            in_graph = true;
            continue;
        }
        if !in_graph {
            continue;
        }
        if t.is_empty() || t.starts_with("Total number in stack") {
            if !stack.is_empty() {
                break;
            }
            continue;
        }
        let Some((column, bytes, frame)) = history_line(line) else {
            continue;
        };
        while stack.last().is_some_and(|(c, ..)| *c >= column) {
            flush(&mut stack, &mut tree);
        }
        stack.push((column, bytes, frame, 0.0));
    }
    while !stack.is_empty() {
        flush(&mut stack, &mut tree);
    }
    let mut profile = tree.profile("leaks", Unit::Bytes);
    if profile.roots.is_empty() {
        notes.insert(0, "nothing was allocated at the end of the program".into());
    }
    profile.notes = notes;
    profile
}

/// The load address of the program and the address of each of its functions in a
/// `malloc_history -callTree` report, for `atos` (hand test 180: C functions had no `file:line`
/// although built with `-g`). Only frames `(in <binary>)` without a location count; the first
/// address of a function is kept.
pub fn history_addresses(text: &str, binary: &str) -> Option<(String, Vec<(String, String)>)> {
    let load = text.lines().find_map(|l| {
        l.trim()
            .strip_prefix("Load Address:")
            .map(|a| a.trim().to_owned())
    })?;
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some((_, _, frame)) = history_line(line) else {
            continue;
        };
        if frame.module.as_deref() != Some(binary) || frame.location.is_some() {
            continue;
        }
        let Some(address) = line
            .rsplit_once('[')
            .and_then(|(_, a)| a.split_once(']'))
            .map(|(a, _)| a.trim().to_owned())
            .filter(|a| a.starts_with("0x"))
        else {
            continue;
        };
        if !out.iter().any(|(n, _)| *n == frame.name) {
            out.push((frame.name, address));
        }
    }
    Some((load, out))
}

/// `file:line` per function from `atos` lines such as `lege_an (in leak) (leak.c:5)`.
pub fn atos_locations(text: &str) -> Vec<(String, (String, u32))> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(" (in ")?;
            let (_, at) = rest.rsplit_once(" (")?;
            let (file, line) = at.trim_end_matches(')').rsplit_once(':')?;
            Some((
                name.trim().to_owned(),
                (file.to_owned(), line.parse().ok()?),
            ))
        })
        .collect()
}

/// Gives the nodes named in `found` their `file:line` where they have none.
pub fn set_locations(nodes: &mut [crate::model::ProfileNode], found: &[(String, (String, u32))]) {
    for n in nodes {
        if n.location.is_none()
            && let Some((_, at)) = found.iter().find(|(name, _)| *name == n.name)
        {
            n.location = Some(at.clone());
        }
        set_locations(&mut n.children, found);
    }
}

/// A Massif tree line's function: `0x109189: grow (alloc.c:4)`, `0x4E3B: f (in /lib/x.so)`.
fn massif_frame(text: &str) -> StackFrame {
    let rest = match text.split_once(": ") {
        Some((addr, rest)) if addr.starts_with("0x") => rest,
        _ => text,
    };
    match rest.rsplit_once(" (") {
        Some((name, tail)) => {
            let tail = tail.trim_end_matches(')');
            if let Some(module) = tail.strip_prefix("in ") {
                StackFrame {
                    name: name.trim().to_owned(),
                    module: Some(module.to_owned()),
                    location: None,
                }
            } else {
                let location = tail
                    .rsplit_once(':')
                    .and_then(|(f, l)| Some((f.to_owned(), l.parse::<u32>().ok()?)));
                StackFrame {
                    name: name.trim().to_owned(),
                    module: None,
                    location,
                }
            }
        }
        None => StackFrame::named(rest.trim()),
    }
}

/// Builds the profile from a `massif.out` file (behaviour 4): the peak snapshot's tree (allocator
/// on top, callers below) turned into a call tree from the outermost caller.
pub fn parse_massif(text: &str) -> Profile {
    // Snapshot blocks: (heap bytes, peak, tree lines).
    let mut snapshots: Vec<(f64, bool, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("snapshot=") {
            snapshots.push((0.0, false, Vec::new()));
        } else if let Some(s) = snapshots.last_mut() {
            if let Some(b) = line.strip_prefix("mem_heap_B=") {
                s.0 = b.trim().parse().unwrap_or(0.0);
            } else if line.trim() == "heap_tree=peak" {
                s.1 = true;
            } else if line.trim_start().starts_with('n') && line.contains(": ") {
                s.2.push(line);
            }
        }
    }
    let chosen = snapshots
        .iter()
        .find(|s| s.1 && !s.2.is_empty())
        .or_else(|| {
            snapshots
                .iter()
                .filter(|s| !s.2.is_empty())
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        });
    let mut tree = StackTree::default();
    let mut notes = Vec::new();
    if let Some((heap, peak, lines)) = chosen {
        notes.push(format!(
            "{} snapshot: {} on the heap",
            if *peak { "peak" } else { "largest" },
            size_label(*heap)
        ));
        // (depth, bytes, frame, children bytes); depth 0 is the allocator line.
        let mut stack: Vec<(usize, f64, StackFrame, f64, bool)> = Vec::new();
        let flush = |stack: &mut Vec<(usize, f64, StackFrame, f64, bool)>, tree: &mut StackTree| {
            if let Some((_, bytes, _, below, small)) = stack.last().cloned() {
                let own = bytes - below;
                if own > 0.0 && stack.len() > 1 {
                    // Callers come below the allocator: the outermost caller is the deepest.
                    let mut frames: Vec<StackFrame> =
                        stack[1..].iter().rev().map(|e| e.2.clone()).collect();
                    if small {
                        frames[0] = StackFrame::named("(smaller call paths)");
                    }
                    tree.add(ALL_ALLOCATIONS, &frames, own);
                }
                stack.pop();
                if let Some(parent) = stack.last_mut() {
                    parent.3 += bytes;
                }
            }
        };
        for line in lines {
            let depth = line.len() - line.trim_start().len();
            let Some((_, rest)) = line.trim_start().split_once(": ") else {
                continue;
            };
            let Some((bytes, what)) = rest.split_once(' ') else {
                continue;
            };
            let Ok(bytes) = bytes.parse::<f64>() else {
                continue;
            };
            while stack.last().is_some_and(|(d, ..)| *d >= depth) {
                flush(&mut stack, &mut tree);
            }
            let small = what.contains("below massif's threshold");
            stack.push((depth, bytes, massif_frame(what), 0.0, small));
        }
        while !stack.is_empty() {
            flush(&mut stack, &mut tree);
        }
    } else {
        notes.push("Massif recorded no heap tree".into());
    }
    let mut profile = tree.profile("massif", Unit::Bytes);
    profile.notes = notes;
    profile
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hot_spots;

    // Hand test 180: the program's functions get file and line from `atos`.
    #[test]
    fn program_addresses_and_atos_lines() {
        let report = "Load Address:    0x1042d0000\n\nCall graph:\n    202 (15.6M) << TOTAL >>\n      200 (15.6M) start  (in dyld) + 6688  [0x1901b7e80]\n      + 200 (15.6M) main  (in leak) + 48  [0x1042d0528]\n      +   200 (15.6M) lege_an  (in leak) + 28  [0x1042d04c4]\n      +     200 (15.6M) _malloc_zone_malloc_instrumented_or_legacy  (in libsystem_malloc.dylib) + 152  [0x1903b4630]\n";
        let (load, functions) = history_addresses(report, "leak").unwrap();
        assert_eq!(load, "0x1042d0000");
        assert_eq!(
            functions,
            [
                ("main".to_owned(), "0x1042d0528".to_owned()),
                ("lege_an".to_owned(), "0x1042d04c4".to_owned())
            ]
        );
        let found = atos_locations(
            "lege_an (in leak) (leak.c:5)\nmain (in leak) (leak.c:9)\n0x1 (in leak)\n",
        );
        assert_eq!(found[0], ("lege_an".to_owned(), ("leak.c".to_owned(), 5)));
        let mut profile = parse_malloc_history(report);
        set_locations(&mut profile.roots, &found);
        let hot = hot_spots(&profile);
        assert_eq!(hot[0].name, "lege_an");
        assert_eq!(hot[0].location, Some(("leak.c".to_owned(), 5)));
    }

    #[test]
    fn tracemalloc_sites_become_a_call_tree() {
        let json = r#"{"current": 2000, "peak": 9000, "stats": [
            {"size": 1500, "count": 3, "frames": [["app.py", 10, "<module>"], ["app.py", 7, "main"], ["app.py", 4, "make"]]},
            {"size": 300, "count": 1, "frames": [["app.py", 10, "<module>"], ["app.py", 7, "main"], ["app.py", 3, "make"]]},
            {"size": 200, "count": 1, "frames": [["lib/x.py", 2, "<module>"]]}
        ]}"#;
        let p = parse_tracemalloc(json).unwrap();
        assert_eq!(
            (p.engine.as_str(), p.unit, p.total),
            ("tracemalloc", Unit::Bytes, 2000.0)
        );
        let root = &p.roots[0];
        assert_eq!(root.name, ALL_ALLOCATIONS);
        let module = &root.children[0];
        assert_eq!(module.location, Some(("app.py".into(), 10)));
        let make = &module.children[0].children[0];
        assert_eq!((make.name.as_str(), make.self_cost), ("make", 1800.0));
        assert_eq!(
            make.location,
            Some(("app.py".into(), 4)),
            "the costliest line"
        );
        assert_eq!(hot_spots(&p)[0].name, "make");
        assert!(p.notes[0].contains("peak 8.8 KB"));
        assert!(parse_tracemalloc("{").is_err());
    }

    const HISTORY: &str = "malloc_history Report Version:  2.0
Process:         alloc [73019]
Physical footprint (peak):  22.0M

Malloc blocks with no stack:    207   total size: 793K

Call graph:
    20002 (19.5M) << TOTAL >>
      20000 (19.5M) start  (in dyld) + 6688  [0x1901b7e80]
      + 20000 (19.5M) main  (in alloc) + 28  [0x10228c5b8]
      +   20000 (19.5M) build  (in alloc) + 48  [0x10228c518]
      +     20000 (19.5M) grow  (in alloc) + 24  [0x10228c4c0]
      +       20000 (19.5M) _malloc_zone_malloc_instrumented_or_legacy  (in libsystem_malloc.dylib) + 152  [0x1903b4630]
      2 (112 bytes) start  (in dyld) + 6596  [0x1901b7e24]
        1 (80 bytes) _owned_ptr_alloc  (in libsystem_c.dylib) + 72  [0x19041a474]
        ! 1 (80 bytes) _malloc_type_malloc_outlined  (in libsystem_malloc.dylib) + 96  [0x190393dd0]
        1 (32 bytes) unsetenv  (in libsystem_c.dylib) + 40  [0x19041a454]

Total number in stack -- this line is here
";

    #[test]
    fn malloc_history_tree_without_malloc_frames() {
        let p = parse_malloc_history(HISTORY);
        assert_eq!(p.engine, "leaks");
        assert_eq!(p.unit, Unit::Bytes);
        assert_eq!(p.total, 19.5 * 1024.0 * 1024.0 + 112.0);
        let hot = hot_spots(&p);
        assert_eq!(
            hot[0].name, "grow",
            "the malloc library's frames are left out"
        );
        assert_eq!(hot[0].self_cost, 19.5 * 1024.0 * 1024.0);
        let start = &p.roots[0].children[0];
        assert_eq!(
            (start.name.as_str(), start.module.as_deref()),
            ("start", Some("dyld"))
        );
        assert!(
            hot.iter()
                .any(|h| h.name == "_owned_ptr_alloc" && h.self_cost == 80.0)
        );
        assert!(p.notes.iter().any(|n| n.contains("22.0M")));
        assert!(p.notes.iter().any(|n| n.contains("no stack: 207")));
        assert_eq!(parse_size("793K"), Some(793.0 * 1024.0));
        assert_eq!(parse_size("1 byte"), Some(1.0));
    }

    const MASSIF: &str = "desc: --time-unit=B
cmd: ./alloc
time_unit: B
#-----------
snapshot=0
#-----------
time=0
mem_heap_B=0
mem_heap_extra_B=0
mem_stacks_B=0
heap_tree=empty
#-----------
snapshot=1
#-----------
time=21000000
mem_heap_B=20004000
mem_heap_extra_B=160000
mem_stacks_B=0
heap_tree=peak
n2: 20004000 (heap allocation functions) malloc/new/new[], --alloc-fns, etc.
 n2: 20000000 0x109189: grow (alloc.c:4)
  n1: 19000000 0x1091B9: build (alloc.c:5)
   n0: 19000000 0x1091F5: main (alloc.c:7)
  n0: 1000000 in 2 places, below massif's threshold (1.00%)
 n0: 4000 in 3 places, all below massif's threshold (1.00%)
#-----------
snapshot=2
#-----------
time=30000000
mem_heap_B=0
heap_tree=empty
";

    #[test]
    fn massif_peak_tree_is_turned_around() {
        let p = parse_massif(MASSIF);
        assert_eq!((p.engine.as_str(), p.total), ("massif", 20004000.0));
        let main = &p.roots[0]
            .children
            .iter()
            .find(|c| c.name == "main")
            .unwrap();
        assert_eq!(main.location, Some(("alloc.c".into(), 7)));
        assert_eq!(main.children[0].name, "build");
        assert_eq!(main.children[0].children[0].name, "grow");
        assert_eq!(main.children[0].children[0].self_cost, 19000000.0);
        let hot = hot_spots(&p);
        assert_eq!(hot[0].name, "grow");
        assert_eq!(hot[0].self_cost, 20000000.0, "grow also from smaller paths");
        assert!(
            p.roots[0]
                .children
                .iter()
                .any(|c| c.name == "(smaller call paths)")
        );
        assert!(p.notes[0].starts_with("peak snapshot"));
        assert_eq!(size_label(512.0), "512 B");
        assert_eq!(size_label(20004000.0), "19.1 MB");
    }
}

//! The costs of a profile by project file, as the editor's gutter shows them (`SPEC-0120` v5
//! behaviour 10): per line a label (time as share of the run's wall time for sampled CPU
//! profiles, seconds for cProfile, a size for memory profiles) and a description with total and
//! self share, time and function; per file the SHA-256 digest of the text that was profiled.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferris_idea_profiler::{LineCost, ProfileRun, SourceIndex};

use crate::profiler_view::{percent, spot_time};

/// The cost of one line in the gutter.
#[derive(Debug, Clone, PartialEq)]
pub struct LineMark {
    /// 0-based line.
    pub line: usize,
    /// What the gutter shows: the line's total cost.
    pub label: String,
    /// Tooltip and accessible name: shares, times and function.
    pub detail: String,
    /// The function the line belongs to.
    pub function: String,
    /// The source file as the profiler reported it.
    pub reported: String,
}

/// The costs of one file and the digest of the text they were measured on.
#[derive(Debug, Clone, PartialEq)]
pub struct FileCosts {
    /// SHA-256 of the file's text when the profile was stored.
    pub digest: String,
    /// The costs by line, in line order.
    pub lines: Vec<LineMark>,
}

/// One line's mark from its cost in `run`.
fn mark(run: &ProfileRun, cost: &LineCost) -> LineMark {
    let p = &run.profile;
    let time = |c: f64| spot_time(p.unit, c, p.total, run.millis);
    LineMark {
        line: cost.line.saturating_sub(1) as usize,
        label: time(cost.total),
        detail: format!(
            "Line {}: {} of the run ({}), self {} ({}) in {}",
            cost.line,
            percent(cost.total, p.total),
            time(cost.total),
            percent(cost.self_cost, p.total),
            time(cost.self_cost),
            cost.function
        ),
        function: cost.function.clone(),
        reported: cost.file.clone(),
    }
}

/// The costs of `run` by absolute path of the project files below `root` that have a digest
/// (profiles stored before `SPEC-0120` v5 have none and give nothing). Lists the workspace once.
pub fn file_costs(run: &ProfileRun, root: &Path) -> HashMap<PathBuf, Arc<FileCosts>> {
    let mut index = SourceIndex::new(root);
    let mut by_file: HashMap<PathBuf, FileCosts> = HashMap::new();
    for cost in run.profile.line_costs() {
        if cost.total <= 0.0 {
            continue;
        }
        let Some(digest) = run.sources.get(&cost.file) else {
            continue;
        };
        let Some(relative) = index.project_file(&cost.file) else {
            continue;
        };
        let entry = by_file
            .entry(root.join(relative))
            .or_insert_with(|| FileCosts {
                digest: digest.clone(),
                lines: Vec::new(),
            });
        if entry.digest == *digest {
            entry.lines.push(mark(run, &cost));
        }
    }
    by_file
        .into_iter()
        .map(|(path, mut costs)| {
            costs.lines.sort_by_key(|l| l.line);
            costs.lines.dedup_by_key(|l| l.line);
            (path, Arc::new(costs))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferris_idea_profiler::{
        Engine, Kind, Profile, ProfilePlan, StackFrame, StackTree, Unit, parse_jfr,
    };

    use super::*;

    fn run_of(profile: Profile, millis: u64, kind: Kind) -> ProfileRun {
        ProfileRun {
            plan: ProfilePlan {
                engine: Engine::Jfr,
                command: vec![],
                env: BTreeMap::new(),
            },
            exit_code: Some(0),
            timed_out: false,
            millis,
            output: String::new(),
            profile,
            kind,
            exit_unknown: false,
            attached: None,
            sources: BTreeMap::new(),
            marked: false,
        }
    }

    fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-lines-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (f, text) in files {
            let path = root.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root.canonicalize().unwrap()
    }

    // Behaviour 10: time as share of the run's wall time, the detail, the file's digest.
    #[test]
    fn costs_by_file_with_labels_and_digest() {
        let busy = "class Busy {}\n";
        let root = workspace("jfr", &[("src/main/java/demo/Busy.java", busy)]);
        let text = "jdk.ExecutionSample {\n  sampledThread = \"main\" (javaThreadId = 1)\n  stackTrace = [\n    demo.Busy.inner(long) line: 6\n    demo.Busy.main(String[]) line: 11\n    java.lang.Thread.run() line: 840\n  ]\n}\n\njdk.ExecutionSample {\n  sampledThread = \"main\" (javaThreadId = 1)\n  stackTrace = [\n    demo.Busy.main(String[]) line: 12\n  ]\n}\n";
        let mut run = run_of(parse_jfr(text), 4000, Kind::Cpu);
        ferris_idea_profiler::prepare(&mut run, &root);
        let costs = file_costs(&run, &root);
        assert_eq!(costs.len(), 1, "the JDK's file is not the project's");
        let file = &costs[&root.join("src/main/java/demo/Busy.java")];
        assert_eq!(file.digest, ferris_idea_core::sha256_hex(busy.as_bytes()));
        let lines: Vec<(usize, &str)> = file
            .lines
            .iter()
            .map(|l| (l.line, l.label.as_str()))
            .collect();
        assert_eq!(lines, vec![(5, "2.00 s"), (10, "2.00 s"), (11, "2.00 s")]);
        assert_eq!(
            file.lines[0].detail,
            "Line 6: 50.0 % of the run (2.00 s), self 50.0 % (2.00 s) in Busy.inner"
        );
        assert_eq!(file.lines[1].function, "Busy.main");
        assert_eq!(file.lines[1].reported, "demo/Busy.java");
        // A profile stored before v5 has no digests: nothing in the gutter.
        run.sources.clear();
        assert!(file_costs(&run, &root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // Memory profiles read in bytes; cProfile's seconds as time.
    #[test]
    fn labels_per_unit() {
        let root = workspace("units", &[("mem.py", "x = 1\n")]);
        let mut tree = StackTree::default();
        let frame = StackFrame {
            name: "fill".into(),
            module: None,
            location: Some(("mem.py".into(), 1)),
        };
        tree.add("all allocations", &[frame], 3.0 * 1024.0 * 1024.0);
        let mut run = run_of(tree.profile("tracemalloc", Unit::Bytes), 900, Kind::Memory);
        ferris_idea_profiler::prepare(&mut run, &root);
        let costs = file_costs(&run, &root);
        assert_eq!(costs[&root.join("mem.py")].lines[0].label, "3.0 MB");
        let mut node = ferris_idea_profiler::ProfileNode::leaf("busy", 1.5);
        node.location = Some((root.join("mem.py").display().to_string(), 1));
        let mut seconds = run_of(
            Profile {
                engine: "cProfile".into(),
                unit: Unit::Seconds,
                total: 3.0,
                roots: vec![node],
                notes: vec![],
                lines: vec![],
            },
            3000,
            Kind::Cpu,
        );
        ferris_idea_profiler::prepare(&mut seconds, &root);
        let costs = file_costs(&seconds, &root);
        assert_eq!(costs[&root.join("mem.py")].lines[0].label, "1.50 s");
        let _ = std::fs::remove_dir_all(&root);
    }
}

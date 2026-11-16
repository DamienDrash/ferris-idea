//! Python's `cProfile` (behaviour 2): the statistics file is turned into JSON by the program's
//! own interpreter (`pstats`), and the caller edges into a call tree.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::model::{Profile, ProfileNode, Unit};

/// Reads a `cProfile` file (argument 1) with `pstats` and prints functions with their callers as
/// JSON. Runs with the interpreter that ran the program, so the file format always matches.
pub const DUMP_SCRIPT: &str = r#"import json, pstats, sys
s = pstats.Stats(sys.argv[1]).stats
out = []
for (f, l, n), (cc, nc, tt, ct, callers) in s.items():
    out.append({'file': f, 'line': l, 'name': n, 'calls': nc, 'self': tt, 'total': ct,
        'callers': [{'file': a, 'line': b, 'name': c, 'calls': v[1], 'total': v[3]}
                    for (a, b, c), v in callers.items()]})
print(json.dumps(out))
"#;

#[derive(Deserialize)]
struct Caller {
    file: String,
    line: u32,
    name: String,
    calls: u64,
    total: f64,
}

#[derive(Deserialize)]
struct Function {
    file: String,
    line: u32,
    name: String,
    calls: u64,
    #[serde(rename = "self")]
    self_time: f64,
    total: f64,
    callers: Vec<Caller>,
}

type Key = (String, u32, String);

/// Deepest call tree built; deeper calls are folded into their caller.
const MAX_DEPTH: usize = 40;

fn label(name: &str) -> String {
    // `<built-in method builtins.sum>` → `builtins.sum`; `<module>` stays.
    name.strip_prefix("<built-in method ")
        .or_else(|| name.strip_prefix("<method "))
        .map(|n| {
            n.trim_end_matches('>')
                .trim_end_matches(" objects")
                .to_owned()
        })
        .unwrap_or_else(|| name.to_owned())
}

/// Builds the profile from the JSON of [`DUMP_SCRIPT`]; times are seconds. The entry points are
/// the functions nobody in the program called (the profiler's own frames are left out).
pub fn parse_cprofile(json: &str) -> Result<Profile, String> {
    let functions: Vec<Function> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let by_key: BTreeMap<Key, &Function> = functions
        .iter()
        .map(|f| ((f.file.clone(), f.line, f.name.clone()), f))
        .collect();
    // callees[caller] = [(callee, calls, total time in that edge)]
    let mut callees: BTreeMap<Key, Vec<(Key, u64, f64)>> = BTreeMap::new();
    for f in &functions {
        for c in &f.callers {
            callees
                .entry((c.file.clone(), c.line, c.name.clone()))
                .or_default()
                .push(((f.file.clone(), f.line, f.name.clone()), c.calls, c.total));
        }
    }
    let profiler_own = |k: &Key| k.2.contains("_lsprof.Profiler") || k.2.contains("'disable'");
    fn build(
        key: &Key,
        total: f64,
        calls: u64,
        by_key: &BTreeMap<Key, &Function>,
        callees: &BTreeMap<Key, Vec<(Key, u64, f64)>>,
        path: &mut Vec<Key>,
    ) -> ProfileNode {
        let f = by_key.get(key);
        let mut node = ProfileNode::leaf(&label(&key.2), total);
        node.calls = Some(calls);
        node.location = (key.0 != "~").then(|| (key.0.clone(), key.1));
        node.module = (key.0 == "~").then(|| "built-in".to_owned());
        // Share of the function's own time that belongs to this edge.
        let share = f.map_or(1.0, |f| if f.total > 0.0 { total / f.total } else { 0.0 });
        node.self_cost = f.map_or(total, |f| f.self_time * share);
        if path.len() < MAX_DEPTH && !path.contains(key) {
            path.push(key.clone());
            for (callee, c, t) in callees.get(key).map(Vec::as_slice).unwrap_or(&[]) {
                // A recursive edge would repeat the caller's cost; it stays in the caller.
                if path.contains(callee) {
                    continue;
                }
                let child = build(callee, (*t).min(total), *c, by_key, callees, path);
                node.children.push(child);
            }
            path.pop();
        }
        node.children.sort_by(|a, b| {
            b.total
                .partial_cmp(&a.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        node
    }
    let mut roots: Vec<ProfileNode> = functions
        .iter()
        .filter(|f| f.callers.is_empty())
        .map(|f| (f.file.clone(), f.line, f.name.clone()))
        .filter(|k| !profiler_own(k))
        .map(|k| {
            let f = by_key[&k];
            build(&k, f.total, f.calls, &by_key, &callees, &mut Vec::new())
        })
        .filter(|n| n.total > 0.0)
        .collect();
    roots.sort_by(|a, b| {
        b.total
            .partial_cmp(&a.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = roots.iter().map(|r| r.total).fold(0.0, f64::max);
    Ok(Profile {
        engine: "cProfile".into(),
        unit: Unit::Seconds,
        total,
        roots,
        notes: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"[
      {"file": "~", "line": 0, "name": "<built-in method builtins.exec>", "calls": 1, "self": 0.0, "total": 1.0, "callers": []},
      {"file": "~", "line": 0, "name": "<method 'disable' of '_lsprof.Profiler' objects>", "calls": 1, "self": 0.0, "total": 0.0, "callers": []},
      {"file": "app.py", "line": 1, "name": "<module>", "calls": 1, "self": 0.1, "total": 1.0,
       "callers": [{"file": "~", "line": 0, "name": "<built-in method builtins.exec>", "calls": 1, "total": 1.0}]},
      {"file": "app.py", "line": 5, "name": "outer", "calls": 5, "self": 0.1, "total": 0.9,
       "callers": [{"file": "app.py", "line": 1, "name": "<module>", "calls": 5, "total": 0.9}]},
      {"file": "app.py", "line": 2, "name": "inner", "calls": 250, "self": 0.8, "total": 0.8,
       "callers": [{"file": "app.py", "line": 5, "name": "outer", "calls": 250, "total": 0.8}]},
      {"file": "app.py", "line": 9, "name": "fact", "calls": 3, "self": 0.0, "total": 0.0,
       "callers": [{"file": "app.py", "line": 9, "name": "fact", "calls": 2, "total": 0.0},
                   {"file": "app.py", "line": 1, "name": "<module>", "calls": 1, "total": 0.0}]}
    ]"#;

    #[test]
    fn caller_edges_become_a_tree() {
        let p = parse_cprofile(JSON).unwrap();
        assert_eq!(p.unit, Unit::Seconds);
        assert_eq!(p.roots.len(), 1, "the profiler's own frame is left out");
        let exec = &p.roots[0];
        assert_eq!(exec.name, "builtins.exec");
        assert_eq!(exec.module.as_deref(), Some("built-in"));
        let module = &exec.children[0];
        assert_eq!(module.location, Some(("app.py".into(), 1)));
        let outer = &module.children[0];
        assert_eq!((outer.name.as_str(), outer.calls), ("outer", Some(5)));
        let inner = &outer.children[0];
        assert_eq!(
            (inner.name.as_str(), inner.calls, inner.total),
            ("inner", Some(250), 0.8)
        );
        assert!((inner.self_cost - 0.8).abs() < 1e-9);
        // The recursive edge of `fact` does not repeat it below itself.
        let fact = module.children.iter().find(|c| c.name == "fact").unwrap();
        assert!(fact.children.is_empty());
        assert_eq!(p.total, 1.0);
        assert!(parse_cprofile("not json").is_err());
    }
}

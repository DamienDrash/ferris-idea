//! The project cockpit (`SPEC-0351`, ROADMAP M20.4, ADR-0034 decision 8), headless: tiles of KPIs
//! computed on the machine from data that is already there — the test history, coverage reports,
//! Ferris IDEA's inspections, a local copy of OSV advisories against the lockfiles, and Git. Every
//! value carries its definition, its sources and their time; a tile without enough data says so.
//! Nothing is sent and nothing is written (ADR-0007). The tool `project_cockpit` has its own
//! router and row of the effects table ([`TOOL_EFFECTS`]): `build.rs` finds both (ADR-0035).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::effects::{Dry, Reach, ToolEffect, tool};
use crate::{IdeServer, json_result, unix_time};

/// The row of `project_cockpit` in the effects table (`SPEC-0053` v8 behaviour 13): it changes
/// nothing, and reading may start a process.
#[rustfmt::skip]
pub(crate) const TOOL_EFFECTS: &[ToolEffect] = &[
    tool("project_cockpit", &[], Dry::None, &[Reach::Process], &[]),
];

/// The tiles of v1 in their order (behaviour 3).
pub const TILES: [&str; 7] = [
    "tests",
    "build",
    "coverage",
    "inspections",
    "dependencies",
    "git",
    "delivery",
];
/// Bounds and default of `period_days` (behaviour 1).
pub const PERIOD_DAYS: (u32, u32, u32) = (1, 365, 30);
/// Runs the green share looks at (behaviour 5).
pub const GREEN_RUNS: usize = 20;
/// Most commits one walk reads (behaviours 10 and 11).
pub const MAX_COMMITS: usize = 20_000;
/// Most unmerged branches named (behaviour 10).
pub const MAX_BRANCHES: usize = 50;
/// Most advisory files read, and the size of one (behaviour 9).
pub const MAX_ADVISORIES: (usize, u64) = (100_000, 1 << 20);
/// The coverage reports looked for (behaviour 7).
pub const COVERAGE_FILES: [&str; 7] = [
    "lcov.info",
    "coverage/lcov.info",
    "coverage.lcov",
    "target/coverage/lcov.info",
    "coverage.xml",
    "cobertura.xml",
    "coverage/cobertura-coverage.xml",
];
const DAY: u64 = 86_400;
const WEEK: u64 = 7 * DAY;

/// Where a value comes from (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// What kind of source.
    pub kind: &'static str,
    /// A path or a name.
    pub path: String,
    /// The time of the data, seconds since the Unix epoch.
    pub time: Option<u64>,
}

/// One tile (behaviour 2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tile {
    /// Its id.
    pub id: &'static str,
    /// Its title.
    pub title: &'static str,
    /// `ok` or `not_enough_data`.
    pub status: &'static str,
    /// Why there is not enough data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The values.
    pub values: Value,
    /// What each value means.
    pub definition: BTreeMap<&'static str, &'static str>,
    /// What was read.
    pub sources: Vec<Source>,
}

impl Tile {
    fn new(
        id: &'static str,
        title: &'static str,
        definition: &[(&'static str, &'static str)],
    ) -> Self {
        Tile {
            id,
            title,
            status: "ok",
            reason: None,
            values: json!({}),
            definition: definition.iter().copied().collect(),
            sources: Vec::new(),
        }
    }

    fn lacking(mut self, why: impl Into<String>) -> Self {
        self.status = "not_enough_data";
        self.reason = Some(why.into());
        self
    }
}

/// What the cockpit is computed from (behaviour 1).
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    /// The project's root.
    pub root: &'a Path,
    /// The test history folder in the user's settings folder.
    pub test_history: Option<&'a Path>,
    /// The local OSV copy.
    pub advisories: Option<&'a Path>,
    /// The time computed at.
    pub as_of: u64,
    /// The period's length in days.
    pub period_days: u32,
    /// The tag pattern of deployments.
    pub deploy_tags: &'a str,
}

impl Inputs<'_> {
    fn start(&self) -> u64 {
        self.as_of.saturating_sub(u64::from(self.period_days) * DAY)
    }

    fn in_period(&self, t: u64) -> bool {
        t > self.start() && t <= self.as_of
    }
}

fn median(mut v: Vec<u64>) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    })
}

fn mtime(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------------------------------
// Tests (behaviour 5) and build (behaviour 6)
// ---------------------------------------------------------------------------------------------

/// The `tests` tile (behaviour 5).
pub fn tests_tile(i: &Inputs<'_>) -> Tile {
    let tile = Tile::new(
        "tests",
        "Test health",
        &[
            (
                "last_runs",
                "per configuration, the last run in the period: time and the counts passed, failed, errors, skipped",
            ),
            (
                "runs_in_period",
                "test runs that finished in the period, all configurations",
            ),
            (
                "green_share",
                "share of the last 20 runs up to as_of without a failure or error, all configurations",
            ),
            (
                "since_last_green_seconds",
                "seconds from the last green run to as_of",
            ),
        ],
    );
    let Some(dir) = i.test_history else {
        return tile.lacking("no settings folder is known, so there is no test history");
    };
    let mut runs: Vec<(u64, String, ferris_idea_core::Totals)> = Vec::new();
    let mut sources = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(entries) = serde_json::from_str::<Vec<ferris_idea_core::HistoryEntry>>(&text) else {
            continue;
        };
        let Some(config) = entries.first().map(|e| e.config.clone()) else {
            continue;
        };
        // Only this project's histories: the file name is the hash of root and configuration.
        if ferris_idea_core::history_file(dir, i.root, &config) != file {
            continue;
        }
        let mut newest = None;
        for e in entries
            .iter()
            .filter(|e| e.config == config && e.time <= i.as_of)
        {
            runs.push((e.time, e.config.clone(), e.report.totals));
            newest = newest.max(Some(e.time));
        }
        sources.push(Source {
            kind: "test history",
            path: file.display().to_string(),
            time: newest,
        });
    }
    let mut tile = tile;
    tile.sources = sources;
    runs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let in_period: Vec<&(u64, String, _)> = runs.iter().filter(|r| i.in_period(r.0)).collect();
    if in_period.is_empty() {
        return tile.lacking("no test run of this project finished in the period");
    }
    let green = |t: &ferris_idea_core::Totals| t.failed == 0 && t.errors == 0;
    let mut last: BTreeMap<String, Value> = BTreeMap::new();
    for (time, config, t) in &in_period {
        last.entry(config.clone()).or_insert_with(|| {
            json!({"time": time, "passed": t.passed, "failed": t.failed, "errors": t.errors, "skipped": t.skipped, "tests": t.tests})
        });
    }
    let recent: Vec<_> = runs.iter().take(GREEN_RUNS).collect();
    let greens = recent.iter().filter(|r| green(&r.2)).count();
    let last_green = runs.iter().find(|r| green(&r.2)).map(|r| r.0);
    tile.values = json!({
        "last_runs": last,
        "runs_in_period": in_period.len(),
        "green_share": round2(greens as f64 / recent.len() as f64),
        "runs_counted": recent.len(),
        "since_last_green_seconds": last_green.map(|t| i.as_of - t),
        "flaky": "not counted: the test history does not keep the commit of a run",
    });
    tile
}

/// The `build` tile (behaviour 6).
pub fn build_tile() -> Tile {
    Tile::new(
        "build",
        "Build health",
        &[("green_share", "share of green builds of the main branch over the last 20 runs")],
    )
    .lacking("Ferris IDEA keeps no record of builds yet; the test runs (tile tests) and a configured CI are the later sources")
}

// ---------------------------------------------------------------------------------------------
// Coverage (behaviour 7)
// ---------------------------------------------------------------------------------------------

fn lcov(text: &str) -> Option<(u64, u64)> {
    let (mut found, mut hit) = (0u64, 0u64);
    let mut any = false;
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("LF:") {
            found += n.trim().parse::<u64>().ok()?;
            any = true;
        } else if let Some(n) = line.strip_prefix("LH:") {
            hit += n.trim().parse::<u64>().ok()?;
        }
    }
    (any && found > 0).then_some((hit, found))
}

fn cobertura(text: &str) -> Option<f64> {
    let start = text.find("<coverage")?;
    let tag = &text[start..start + text[start..].find('>')?];
    let at = tag.find("line-rate=\"")? + "line-rate=\"".len();
    let value = &tag[at..at + tag[at..].find('"')?];
    value
        .parse::<f64>()
        .ok()
        .filter(|r| (0.0..=1.0).contains(r))
}

/// The `coverage` tile (behaviour 7).
pub fn coverage_tile(i: &Inputs<'_>) -> Tile {
    let mut tile = Tile::new(
        "coverage",
        "Coverage",
        &[
            (
                "line_coverage_percent",
                "covered lines divided by instrumented lines of the newest report, in per cent",
            ),
            ("report", "the report read, relative to the project"),
        ],
    );
    let mut reports: Vec<(u64, &str)> = COVERAGE_FILES
        .iter()
        .filter_map(|rel| {
            let t = mtime(&i.root.join(rel))?;
            (t <= i.as_of).then_some((t, *rel))
        })
        .collect();
    reports.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    for (time, rel) in reports {
        let Ok(text) = std::fs::read_to_string(i.root.join(rel)) else {
            continue;
        };
        let (percent, lines) = if rel.ends_with(".xml") {
            match cobertura(&text) {
                Some(r) => (r * 100.0, None),
                None => continue,
            }
        } else {
            match lcov(&text) {
                Some((hit, found)) => (hit as f64 * 100.0 / found as f64, Some((hit, found))),
                None => continue,
            }
        };
        tile.values = json!({
            "line_coverage_percent": round2(percent),
            "report": rel,
            "lines_hit": lines.map(|l| l.0),
            "lines_found": lines.map(|l| l.1),
        });
        tile.sources.push(Source {
            kind: if rel.ends_with(".xml") {
                "cobertura report"
            } else {
                "lcov report"
            },
            path: rel.to_owned(),
            time: Some(time),
        });
        return tile;
    }
    tile.lacking(format!(
        "no readable coverage report: looked for {}",
        COVERAGE_FILES.join(", ")
    ))
}

// ---------------------------------------------------------------------------------------------
// Inspections (behaviour 8)
// ---------------------------------------------------------------------------------------------

/// The `inspections` tile (behaviour 8).
pub fn inspections_tile(i: &Inputs<'_>) -> Tile {
    use ferris_idea_languages::inspections::{inspect_project, inspection_files};
    let mut tile = Tile::new(
        "inspections",
        "Inspections",
        &[
            (
                "by_level",
                "problems of Ferris IDEA's own inspections over the project's sources, by level",
            ),
            (
                "top",
                "the ten inspections with the most problems (ties by id)",
            ),
            (
                "checked_at",
                "when they ran: the files as they are now, not as of an earlier time",
            ),
        ],
    );
    let scope = inspection_files(i.root, None);
    let found = inspect_project(i.root, &scope.files);
    let mut by_level: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &found {
        let level = serde_json::to_value(f.level)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        *by_level.entry(level).or_default() += 1;
        *by_id.entry(f.id).or_default() += 1;
    }
    let mut top: Vec<(&str, usize)> = by_id.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    top.truncate(10);
    tile.values = json!({
        "total": found.len(),
        "by_level": by_level,
        "top": top.iter().map(|(id, n)| json!({"id": id, "problems": n})).collect::<Vec<_>>(),
        "files": scope.files.len(),
        "checked_at": i.as_of,
        "not_inspected": scope.not_inspected,
    });
    tile.sources.push(Source {
        kind: "inspections",
        path: "Ferris IDEA inspections (SPEC-0098), run at the call".into(),
        time: Some(i.as_of),
    });
    tile
}

// ---------------------------------------------------------------------------------------------
// Dependencies (behaviour 9)
// ---------------------------------------------------------------------------------------------

/// A version split for comparison: numeric parts, then a pre-release (which sorts first).
fn version_key(v: &str) -> (Vec<u64>, Option<String>) {
    let v = v.trim().trim_start_matches('v');
    let v = v.split('+').next().unwrap_or(v);
    let (main, pre) = match v.split_once('-') {
        Some((m, p)) => (m, Some(p.to_owned())),
        None => (v, None),
    };
    let nums = main
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect();
    (nums, pre)
}

/// Compares two versions (semantic versioning order; missing parts are 0).
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let (an, ap) = version_key(a);
    let (bn, bp) = version_key(b);
    let len = an.len().max(bn.len());
    for k in 0..len {
        let (x, y) = (
            an.get(k).copied().unwrap_or(0),
            bn.get(k).copied().unwrap_or(0),
        );
        if x != y {
            return x.cmp(&y);
        }
    }
    match (ap, bp) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    }
}

#[derive(Debug, Deserialize)]
struct OsvRecord {
    id: String,
    modified: Option<String>,
    #[serde(default)]
    affected: Vec<OsvAffected>,
    database_specific: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OsvAffected {
    package: Option<OsvPackage>,
    #[serde(default)]
    versions: Vec<String>,
    #[serde(default)]
    ranges: Vec<OsvRange>,
}

#[derive(Debug, Deserialize)]
struct OsvPackage {
    ecosystem: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct OsvRange {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    events: Vec<BTreeMap<String, String>>,
}

/// Whether `version` is inside the range's events (behaviour 9).
fn in_range(version: &str, events: &[BTreeMap<String, String>]) -> bool {
    let mut evs: Vec<(&str, &str)> = events
        .iter()
        .flat_map(|e| e.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .filter(|(k, _)| matches!(*k, "introduced" | "fixed" | "last_affected"))
        .collect();
    evs.sort_by(|a, b| compare_versions(a.1, b.1));
    let mut affected = false;
    for (kind, at) in evs {
        let cmp = compare_versions(version, at);
        match kind {
            "introduced" if cmp != std::cmp::Ordering::Less => affected = true,
            "fixed" if cmp != std::cmp::Ordering::Less => affected = false,
            "last_affected" if cmp == std::cmp::Ordering::Greater => affected = false,
            _ => {}
        }
    }
    affected
}

/// The first `fixed` event of the record's ranges that is above `version`.
fn fixed_above(version: &str, a: &OsvAffected) -> Option<String> {
    let mut fixes: Vec<&String> = a
        .ranges
        .iter()
        .flat_map(|r| r.events.iter())
        .filter_map(|e| e.get("fixed"))
        .filter(|f| compare_versions(f, version) == std::cmp::Ordering::Greater)
        .collect();
    fixes.sort_by(|x, y| compare_versions(x, y));
    fixes.first().map(|f| (*f).clone())
}

/// The packages of the project's lockfiles: (ecosystem, name, version), sorted, and the files read.
pub fn locked_packages(root: &Path) -> (BTreeSet<(String, String, String)>, Vec<Source>) {
    let mut out = BTreeSet::new();
    let mut sources = Vec::new();
    let cargo = root.join("Cargo.lock");
    if let Ok(text) = std::fs::read_to_string(&cargo) {
        // `[[package]]` blocks with `name`, `version` and `source` lines; registry packages only.
        let mut block: BTreeMap<&str, String> = BTreeMap::new();
        let mut flush = |b: &mut BTreeMap<&str, String>| {
            let source = b.get("source").map_or("", String::as_str);
            if (source.starts_with("registry+") || source.starts_with("sparse+"))
                && let (Some(n), Some(v)) = (b.get("name"), b.get("version"))
            {
                out.insert(("crates.io".to_owned(), n.clone(), v.clone()));
            }
            b.clear();
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                flush(&mut block);
                continue;
            }
            if let Some((k, v)) = line.split_once(" = ")
                && matches!(k, "name" | "version" | "source")
            {
                block.insert(
                    match k {
                        "name" => "name",
                        "version" => "version",
                        _ => "source",
                    },
                    v.trim_matches('"').to_owned(),
                );
            }
        }
        flush(&mut block);
        sources.push(Source {
            kind: "lockfile",
            path: "Cargo.lock".into(),
            time: mtime(&cargo),
        });
    }
    let npm = root.join("package-lock.json");
    if let Ok(text) = std::fs::read_to_string(&npm)
        && let Ok(doc) = serde_json::from_str::<Value>(&text)
    {
        if let Some(packages) = doc["packages"].as_object() {
            for (key, p) in packages {
                let Some(at) = key.rfind("node_modules/") else {
                    continue;
                };
                if p["link"] == json!(true) {
                    continue;
                }
                let name = &key[at + "node_modules/".len()..];
                if let Some(v) = p["version"].as_str() {
                    out.insert(("npm".to_owned(), name.to_owned(), v.to_owned()));
                }
            }
        }
        sources.push(Source {
            kind: "lockfile",
            path: "package-lock.json".into(),
            time: mtime(&npm),
        });
    }
    (out, sources)
}

fn osv_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let p = e.path();
            if kind.is_dir() {
                todo.push(p);
            } else if kind.is_file() && p.extension().is_some_and(|x| x == "json") {
                out.push(p);
                if out.len() >= MAX_ADVISORIES.0 {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

fn rfc3339_seconds(s: &str) -> Option<u64> {
    // `YYYY-MM-DDTHH:MM:SS` with any fraction and zone `Z`; enough for OSV's `modified`.
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Days from the civil date (H. Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// What matched one package version: advisory ids, the first fixed version, the severities.
type Hit = (BTreeSet<String>, Option<String>, BTreeSet<String>);

/// The `dependencies` tile (behaviour 9).
pub fn dependencies_tile(i: &Inputs<'_>) -> Tile {
    let mut tile = Tile::new(
        "dependencies",
        "Dependency vulnerabilities",
        &[
            (
                "affected",
                "packages of the lockfiles with a known advisory of the local OSV copy: name, version, advisory ids, first fixed version",
            ),
            (
                "by_severity",
                "affected package versions by the advisory's stated severity (unknown when it states none)",
            ),
            (
                "packages_checked",
                "registry packages read from the lockfiles",
            ),
            ("advisories_read", "OSV records read from the local copy"),
        ],
    );
    let (packages, mut sources) = locked_packages(i.root);
    if packages.is_empty() {
        tile.sources = sources;
        return tile.lacking(
            "no Cargo.lock or package-lock.json with registry packages at the project's root",
        );
    }
    let Some(dir) = i.advisories.filter(|d| d.is_dir()) else {
        tile.sources = sources;
        return tile.lacking(format!(
            "no local copy of advisories{}; Ferris IDEA does not download one in this version (put OSV JSON records there)",
            i.advisories.map(|d| format!(" in {}", d.display())).unwrap_or_default()
        ));
    };
    let by_name: HashMap<(&str, &str), Vec<&str>> =
        packages.iter().fold(HashMap::new(), |mut m, (e, n, v)| {
            m.entry((e.as_str(), n.as_str()))
                .or_default()
                .push(v.as_str());
            m
        });
    let files = osv_files(dir);
    let mut newest: Option<u64> = None;
    let mut read = 0usize;
    // (ecosystem, name, version) → (advisory ids, first fixed, severities)
    let mut hits: BTreeMap<(String, String, String), Hit> = BTreeMap::new();
    for f in &files {
        if std::fs::metadata(f).map_or(true, |m| m.len() > MAX_ADVISORIES.1) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<OsvRecord>(&text) else {
            continue;
        };
        read += 1;
        if let Some(t) = record.modified.as_deref().and_then(rfc3339_seconds) {
            newest = newest.max(Some(t));
        }
        let severity = record
            .database_specific
            .as_ref()
            .and_then(|d| d["severity"].as_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| "unknown".into());
        for a in &record.affected {
            let Some(pkg) = &a.package else { continue };
            let Some(versions) = by_name.get(&(pkg.ecosystem.as_str(), pkg.name.as_str())) else {
                continue;
            };
            for v in versions {
                let listed = a.versions.iter().any(|x| x == v);
                let ranged = a
                    .ranges
                    .iter()
                    .filter(|r| matches!(r.kind.as_str(), "SEMVER" | "ECOSYSTEM"))
                    .any(|r| in_range(v, &r.events));
                if !(listed || ranged) {
                    continue;
                }
                let entry = hits
                    .entry((pkg.ecosystem.clone(), pkg.name.clone(), (*v).to_owned()))
                    .or_default();
                entry.0.insert(record.id.clone());
                if let Some(fix) = fixed_above(v, a) {
                    entry.1 = match entry.1.take() {
                        Some(old) if compare_versions(&old, &fix) != std::cmp::Ordering::Less => {
                            Some(fix)
                        }
                        Some(old) => Some(old),
                        None => Some(fix),
                    };
                }
                entry.2.insert(severity.clone());
            }
        }
    }
    let mut by_severity: BTreeMap<String, usize> = BTreeMap::new();
    let affected: Vec<Value> = hits
        .into_iter()
        .map(|((eco, name, version), (ids, fixed, sev))| {
            // The most severe the advisories state, in a fixed order.
            let worst = ["critical", "high", "moderate", "medium", "low"]
                .iter()
                .find(|s| sev.contains(**s))
                .map_or_else(|| sev.iter().next().cloned().unwrap_or_default(), |s| (*s).to_owned());
            *by_severity.entry(worst.clone()).or_default() += 1;
            json!({"ecosystem": eco, "name": name, "version": version, "advisories": ids, "fixed": fixed, "severity": worst})
        })
        .collect();
    sources.push(Source {
        kind: "osv copy",
        path: dir.display().to_string(),
        time: newest,
    });
    tile.sources = sources;
    tile.values = json!({
        "affected": affected,
        "by_severity": by_severity,
        "packages_checked": packages.len(),
        "advisories_read": read,
        "copy_date": newest,
    });
    tile
}

// ---------------------------------------------------------------------------------------------
// Git (behaviour 10) and delivery (behaviour 11)
// ---------------------------------------------------------------------------------------------

/// Whether `name` matches `pattern` (`*` any characters).
pub fn glob(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (k, part) in parts.iter().enumerate() {
        if k == 0 {
            let Some(r) = rest.strip_prefix(part) else {
                return false;
            };
            rest = r;
        } else if k == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(at) = rest.find(part) else {
                return false;
            };
            rest = &rest[at + part.len()..];
        }
    }
    true
}

fn main_branch(repo: &ferris_idea_vcs::Repo) -> Option<String> {
    let branches = repo.branches().ok()?;
    ["main", "master"]
        .iter()
        .find(|n| branches.branches.iter().any(|b| !b.remote && b.name == **n))
        .map(|n| (*n).to_owned())
        .or_else(|| Some("HEAD".to_owned()))
}

/// The `git` tile (behaviour 10).
pub fn git_tile(i: &Inputs<'_>) -> Tile {
    let mut tile = Tile::new(
        "git",
        "Git",
        &[
            ("main_branch", "main, else master, else HEAD"),
            (
                "commits_in_period",
                "commits reachable from the main branch with a commit time in the period",
            ),
            (
                "authors_in_period",
                "distinct author e-mail addresses of those commits (counted, not named)",
            ),
            (
                "last_commit",
                "the newest commit time of the main branch up to as_of",
            ),
            (
                "unmerged_branches",
                "local branches whose tip is not reachable from the main branch, oldest tip first (at most 50 named)",
            ),
            (
                "unmerged_remote_branches",
                "remote-tracking branches whose tip is not reachable from the main branch",
            ),
        ],
    );
    let Ok(repo) = ferris_idea_vcs::Repo::discover(i.root) else {
        return tile.lacking("the project is not in a Git repository");
    };
    let Some(main) = main_branch(&repo) else {
        return tile.lacking("the repository has no branches");
    };
    let log = repo.log_from(&main, MAX_COMMITS).unwrap_or_default();
    if log.is_empty() {
        return tile.lacking("the main branch has no commits");
    }
    let reachable: BTreeSet<&str> = log.iter().map(|c| c.id.as_str()).collect();
    let as_of = i.as_of as i64;
    let in_period: Vec<_> = log
        .iter()
        .filter(|c| c.time >= 0 && i.in_period(c.time as u64))
        .collect();
    let authors: BTreeSet<&str> = in_period.iter().map(|c| c.email.as_str()).collect();
    let last = log.iter().filter(|c| c.time <= as_of).map(|c| c.time).max();
    let branches = repo.branches().map(|b| b.branches).unwrap_or_default();
    let mut local: Vec<(i64, String)> = Vec::new();
    let mut remote = 0usize;
    for b in branches
        .iter()
        .filter(|b| b.name != main && !b.name.ends_with("/HEAD"))
    {
        if reachable.iter().any(|id| id.starts_with(&b.target)) {
            continue;
        }
        let Some(tip) = repo
            .log_from(&b.name, 1)
            .ok()
            .and_then(|l| l.into_iter().next())
        else {
            continue;
        };
        if tip.time > as_of {
            continue;
        }
        if b.remote {
            remote += 1;
        } else {
            local.push((tip.time, b.name.clone()));
        }
    }
    local.sort();
    tile.values = json!({
        "main_branch": main,
        "commits_in_period": in_period.len(),
        "authors_in_period": authors.len(),
        "last_commit": last,
        "unmerged_branches": local.iter().take(MAX_BRANCHES).map(|(t, n)| json!({"name": n, "tip_time": t})).collect::<Vec<_>>(),
        "unmerged_branch_count": local.len(),
        "unmerged_remote_branches": remote,
        "walk_cut": log.len() >= MAX_COMMITS,
    });
    tile.sources.push(Source {
        kind: "git",
        path: repo.workdir().unwrap_or(i.root).display().to_string(),
        time: last.and_then(|t| u64::try_from(t).ok()),
    });
    tile
}

/// The `delivery` tile: the DORA keys from Git (behaviour 11).
pub fn delivery_tile(i: &Inputs<'_>) -> Tile {
    let mut tile = Tile::new(
        "delivery",
        "Delivery (DORA)",
        &[
            (
                "deployments",
                "tags matching deploy_tags; a deployment's time is the tag's creation time (tagger date, or the commit time of a lightweight tag)",
            ),
            (
                "deployment_frequency_per_week",
                "deployments in the period per week",
            ),
            (
                "weeks_with_deployment_share",
                "share of the period's weeks with at least one deployment",
            ),
            (
                "lead_time_median_seconds",
                "median of deployment time minus commit time over the commits first contained in a deployment of the period",
            ),
            (
                "change_failure_rate",
                "failed deployments of the period divided by its deployments; failed: followed by a rollback (a deployment of an ancestor of its commit)",
            ),
            (
                "recovery_time_median_seconds",
                "median time from a failed deployment of the period to the next deployment",
            ),
        ],
    );
    let Ok(repo) = ferris_idea_vcs::Repo::discover(i.root) else {
        return tile.lacking("the project is not in a Git repository");
    };
    let tags = match repo.tags_with_time() {
        Ok(t) => t,
        Err(e) => return tile.lacking(format!("the tags could not be read: {e}")),
    };
    let deployments: Vec<(String, String, u64)> = tags
        .into_iter()
        .filter(|(name, _, time)| {
            glob(i.deploy_tags, name) && *time >= 0 && (*time as u64) <= i.as_of
        })
        .map(|(n, c, t)| (n, c, t as u64))
        .collect();
    tile.sources.push(Source {
        kind: "git tags",
        path: format!("refs/tags matching {}", i.deploy_tags),
        time: deployments.last().map(|d| d.2),
    });
    if deployments.len() < 2 {
        return tile.lacking(format!(
            "fewer than two deployments (tags matching {}) up to as_of",
            i.deploy_tags
        ));
    }
    // Commits reachable from each deployment's commit, with their times.
    let mut reach: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for (_, commit, _) in &deployments {
        if !reach.contains_key(commit) {
            let set = repo
                .log_from(commit, MAX_COMMITS)
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.id, c.time))
                .collect();
            reach.insert(commit.clone(), set);
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut leads = Vec::new();
    let mut failed: Vec<usize> = Vec::new();
    for (k, (_, commit, time)) in deployments.iter().enumerate() {
        let commits = &reach[commit];
        if i.in_period(*time) {
            for (id, t) in commits {
                if !seen.contains(id) && *t >= 0 {
                    leads.push(time.saturating_sub(*t as u64));
                }
            }
        }
        seen.extend(commits.keys().cloned());
        if k > 0 {
            let before = &deployments[k - 1].1;
            if before != commit && reach[before].contains_key(commit) {
                failed.push(k - 1);
            }
        }
    }
    let in_period: Vec<usize> = (0..deployments.len())
        .filter(|k| i.in_period(deployments[*k].2))
        .collect();
    let weeks = u64::from(i.period_days).div_ceil(7).max(1);
    let weeks_with: BTreeSet<u64> = in_period
        .iter()
        .map(|k| (i.as_of - deployments[*k].2) / WEEK)
        .filter(|w| *w < weeks)
        .collect();
    let failed_in: Vec<usize> = failed
        .iter()
        .copied()
        .filter(|k| in_period.contains(k))
        .collect();
    let recoveries: Vec<u64> = failed_in
        .iter()
        .map(|k| deployments[k + 1].2.saturating_sub(deployments[*k].2))
        .collect();
    tile.values = json!({
        "deployments": in_period.iter().map(|k| json!({"tag": deployments[*k].0, "time": deployments[*k].2})).collect::<Vec<_>>(),
        "deployment_frequency_per_week": round2(in_period.len() as f64 * 7.0 / f64::from(i.period_days)),
        "weeks_with_deployment_share": round2(weeks_with.len() as f64 / weeks as f64),
        "lead_time_median_seconds": median(leads.clone()),
        "lead_time_commits": leads.len(),
        "change_failure_rate": if in_period.is_empty() { Value::Null } else { json!(round2(failed_in.len() as f64 / in_period.len() as f64)) },
        "failed_deployments": failed_in.iter().map(|k| deployments[*k].0.clone()).collect::<Vec<_>>(),
        "recovery_time_median_seconds": median(recoveries),
        "failures_counted": "rollbacks only: incidents come with the tickets of ROADMAP M20.2",
    });
    tile
}

/// The KPIs of ADR-0034 v1 does not compute, with the reason (behaviour 3).
pub fn not_available() -> Value {
    json!([
        {"kpis": "flow: lead time, cycle time, throughput, WIP, blocked, flow efficiency; incidents for the DORA keys", "reason": "waits for the tickets of ROADMAP M20.1/M20.2"},
        {"kpis": "open pull requests and their age", "reason": "needs a connected hosting service, off by default (ADR-0034 question 5)"},
        {"kpis": "agents: sessions, success rate, gate failures, cost and budget, waiting approvals", "reason": "waits for the session records of ROADMAP M19.10"},
        {"kpis": "databases: connections in use, slow queries", "reason": "not in this version"},
    ])
}

/// The whole cockpit (behaviours 1 to 3).
pub fn cockpit(i: &Inputs<'_>, tiles: &[&str]) -> Value {
    let chosen: Vec<Tile> = TILES
        .iter()
        .filter(|t| tiles.is_empty() || tiles.contains(t))
        .map(|t| match *t {
            "tests" => tests_tile(i),
            "build" => build_tile(),
            "coverage" => coverage_tile(i),
            "inspections" => inspections_tile(i),
            "dependencies" => dependencies_tile(i),
            "git" => git_tile(i),
            _ => delivery_tile(i),
        })
        .collect();
    json!({
        "as_of": i.as_of,
        "period_days": i.period_days,
        "period_start": i.start(),
        "deploy_tags": i.deploy_tags,
        "tiles": chosen,
        "not_available": not_available(),
        "sent": "nothing: every value is computed on this machine (ADR-0007)",
    })
}

/// Arguments of `project_cockpit`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectCockpitArgs {
    /// The time to compute at, seconds since the Unix epoch (default now); later records are left
    /// out, so the same data and as_of give the same answer.
    pub as_of: Option<u64>,
    /// The period's length in days, 1 to 365 (default 30).
    pub period_days: Option<u32>,
    /// The tiles: tests, build, coverage, inspections, dependencies, git, delivery (default all).
    #[serde(default)]
    pub tiles: Vec<String>,
    /// The tag pattern of deployments, `*` for any characters (default `v*`).
    pub deploy_tags: Option<String>,
}

#[tool_router(router = cockpit_router, vis = "pub(crate)")]
impl IdeServer {
    #[tool(
        description = "Read-only project cockpit, computed locally (nothing sent). Tiles: `tests`, `build`, `coverage`, `inspections`, `dependencies` (lock files against a local OSV copy, never downloaded), `git` and `delivery` (DORA metrics from tags matching `deploy_tags`). Each tile has its definition and sources, or `not_enough_data` with a reason instead of a guess; `not_available` names KPIs awaiting other data. `as_of` and `period_days` (1 to 365) make it deterministic. Needs no right."
    )]
    async fn project_cockpit(
        &self,
        Parameters(args): Parameters<ProjectCockpitArgs>,
    ) -> Result<CallToolResult, McpError> {
        let period_days = args.period_days.unwrap_or(PERIOD_DAYS.2);
        if !(PERIOD_DAYS.0..=PERIOD_DAYS.1).contains(&period_days) {
            return Err(McpError::invalid_params(
                format!("period_days is {} to {}", PERIOD_DAYS.0, PERIOD_DAYS.1),
                None,
            ));
        }
        for t in &args.tiles {
            if !TILES.contains(&t.as_str()) {
                return Err(McpError::invalid_params(
                    format!("unknown tile `{t}`: one of {}", TILES.join(", ")),
                    None,
                ));
            }
        }
        let deploy_tags = args.deploy_tags.unwrap_or_else(|| "v*".into());
        if deploy_tags.is_empty()
            || deploy_tags.len() > 100
            || deploy_tags.starts_with('-')
            || deploy_tags
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(McpError::invalid_params(
                "deploy_tags is a tag pattern of 1 to 100 characters without spaces, `*` for any characters",
                None,
            ));
        }
        let root = self.workspace.root().to_path_buf();
        let history = self.test_history.clone();
        let advisories = ferris_idea_core::user_cache_dir().map(|d| d.join("osv"));
        let as_of = args.as_of.unwrap_or_else(unix_time);
        let tiles = args.tiles.clone();
        let answer = tokio::task::spawn_blocking(move || {
            let i = Inputs {
                root: &root,
                test_history: history.as_deref(),
                advisories: advisories.as_deref(),
                as_of,
                period_days,
                deploy_tags: &deploy_tags,
            };
            let names: Vec<&str> = tiles.iter().map(String::as_str).collect();
            cockpit(&i, &names)
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        json_result(answer)
    }
}

/// The groups of this module's tools (`SPEC-0006` v58 behaviour 17; joined by `build.rs`).
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)] = &[
    ("project_cockpit", crate::groups::Group::Ide),
];

/// Where the untrusted text of this module's tools comes from (`SPEC-0006` v60 behaviour 25).
pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])] = &[(
    "project_cockpit",
    &[
        crate::untrusted::Source::Files,
        crate::untrusted::Source::Git,
        crate::untrusted::Source::Agents,
        crate::untrusted::Source::Records,
    ],
)];

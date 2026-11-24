//! Node.js runtimes, package manager and installed packages (`SPEC-0051`), read statically.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::run::{RunConfig, RunKind};

/// One Node.js runtime (behaviour 1–2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeRuntime {
    /// Path of the `node` executable.
    pub path: String,
    /// Version as far as it is known without starting it.
    pub version: Option<String>,
    /// How it was found: `configured`, `nvm`, `fnm`, `PATH`.
    pub found_by: String,
}

/// Package manager of the project (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageManager {
    /// `npm`, `yarn`, `pnpm` or `bun`.
    pub name: String,
    /// Version from `packageManager`, if given.
    pub version: Option<String>,
    /// Where it was taken from: `packageManager`, a lockfile name, or `default`.
    pub source: String,
}

/// A declared dependency the installed packages do not satisfy (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeUnsatisfied {
    /// Package name.
    pub name: String,
    /// Declared range.
    pub range: String,
    /// Kind: `dependencies`, `devDependencies`, `optionalDependencies`, `peerDependencies`.
    pub kind: String,
    /// Installed version, if any.
    pub installed: Option<String>,
}

/// Inputs of the search; [`NodeSearch::from_env`] fills them from the process environment.
#[derive(Debug, Clone, Default)]
pub struct NodeSearch {
    /// Workspace root.
    pub root: PathBuf,
    /// `PATH`.
    pub path_var: Option<OsString>,
    /// nvm folder (`NVM_DIR` or `~/.nvm`).
    pub nvm_dir: Option<PathBuf>,
    /// fnm folder (`FNM_DIR` or `~/.local/share/fnm`).
    pub fnm_dir: Option<PathBuf>,
    /// Runtime chosen in the settings (executable or installation folder).
    pub configured: Option<PathBuf>,
}

impl NodeSearch {
    /// Search inputs from the environment of this process.
    pub fn from_env(root: &Path) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            root: root.to_path_buf(),
            path_var: std::env::var_os("PATH"),
            nvm_dir: std::env::var_os("NVM_DIR")
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(".nvm"))),
            fnm_dir: std::env::var_os("FNM_DIR")
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(".local/share/fnm"))),
            configured: None,
        }
    }
}

/// Result of [`node_environment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeEnvironment {
    /// Runtimes found, without duplicates.
    pub runtimes: Vec<NodeRuntime>,
    /// Index of the selected runtime.
    pub selected: Option<usize>,
    /// Why it was selected.
    pub reason: Option<String>,
    /// Version asked for by `.nvmrc` or `.node-version`.
    pub version_file: Option<String>,
    /// `engines.node` of `package.json`.
    pub engines_node: Option<String>,
    /// Whether the selected version satisfies `engines.node` (`None`: unknown).
    pub compatible: Option<bool>,
    /// Package manager.
    pub package_manager: Option<PackageManager>,
    /// Lockfiles present.
    pub lockfiles: Vec<String>,
    /// Whether `node_modules` exists.
    pub installed: bool,
    /// Declared dependencies that are missing or outside their range, per kind sorted by name.
    pub unsatisfied: Vec<NodeUnsatisfied>,
    /// Declared dependencies that are not version ranges (tags, git, files, workspaces, aliases).
    pub not_checked: Vec<String>,
    /// Test framework: `vitest`, `jest`, `mocha` or `node:test`.
    pub test_framework: Option<String>,
    /// Suggested test run configuration (SPEC-0047).
    pub test_config: Option<RunConfig>,
    /// Web frameworks used, e.g. `React`, `Next.js`, `Vue`, `Angular` (`SPEC-0133` behaviour 4).
    pub frameworks: Vec<String>,
    /// Suggested configuration that starts the development server (`dev` or `start` script).
    pub dev_config: Option<RunConfig>,
    /// Suggested configuration that opens the development server's page, to debug it in Chrome.
    pub browser_config: Option<RunConfig>,
    /// Problems: conflicting lockfiles, `packageManager` against the lockfile (IMP-0026), bad JSON.
    pub problems: Vec<String>,
    /// Remarks.
    pub notes: Vec<String>,
}

// ---------- Semantic versions and npm ranges ----------

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemVer {
    release: [u64; 3],
    pre: Vec<String>,
}

fn parse_semver(text: &str) -> Option<SemVer> {
    let t = text.trim().trim_start_matches(['v', '=']).trim();
    let t = t.split('+').next()?;
    let (core, pre) = match t.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
        None => (t, Vec::new()),
    };
    let mut release = [0u64; 3];
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    for (i, p) in parts.iter().enumerate() {
        release[i] = p.parse().ok()?;
    }
    Some(SemVer { release, pre })
}

fn cmp_semver(a: &SemVer, b: &SemVer) -> Ordering {
    a.release
        .cmp(&b.release)
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (x, y) in a.pre.iter().zip(&b.pre) {
                    let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => x.cmp(y),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                a.pre.len().cmp(&b.pre.len())
            }
        })
}

/// A partial version: missing or `x`/`*` parts are `None`.
fn parse_partial(text: &str) -> Option<(Vec<Option<u64>>, Vec<String>)> {
    let t = text.trim().trim_start_matches(['v', '=']).trim();
    if t.is_empty() {
        return Some((Vec::new(), Vec::new()));
    }
    let t = t.split('+').next()?;
    let (core, pre) = match t.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
        None => (t, Vec::new()),
    };
    let mut parts = Vec::new();
    for p in core.split('.') {
        if matches!(p, "x" | "X" | "*") {
            parts.push(None);
        } else {
            parts.push(Some(p.parse().ok()?));
        }
    }
    if parts.len() > 3 {
        return None;
    }
    // After a wildcard, everything is a wildcard.
    if let Some(i) = parts.iter().position(Option::is_none) {
        parts.truncate(i);
    }
    Some((parts, pre))
}

/// One comparator after desugaring.
#[derive(Debug, Clone)]
struct Comparator {
    op: Ordering,
    inclusive: bool,
    version: SemVer,
}

impl Comparator {
    fn test(&self, v: &SemVer) -> bool {
        let o = cmp_semver(v, &self.version);
        o == self.op || (self.inclusive && o == Ordering::Equal)
    }
}

fn ver(release: [u64; 3], pre: Vec<String>) -> SemVer {
    SemVer { release, pre }
}

/// `-0`: the lowest pre-release, so an exclusive upper bound excludes pre-releases of that version.
fn floor_pre() -> Vec<String> {
    vec!["0".into()]
}

fn ge(v: SemVer) -> Comparator {
    Comparator {
        op: Ordering::Greater,
        inclusive: true,
        version: v,
    }
}

fn lt(v: SemVer) -> Comparator {
    Comparator {
        op: Ordering::Less,
        inclusive: false,
        version: v,
    }
}

/// Lower and upper bound of a partial version (`1.2` → `>=1.2.0 <1.3.0-0`).
fn x_range(parts: &[Option<u64>], pre: Vec<String>) -> Vec<Comparator> {
    let p: Vec<u64> = parts.iter().map(|x| x.unwrap_or(0)).collect();
    match p.len() {
        0 => Vec::new(),
        1 => vec![
            ge(ver([p[0], 0, 0], Vec::new())),
            lt(ver([p[0] + 1, 0, 0], floor_pre())),
        ],
        2 => vec![
            ge(ver([p[0], p[1], 0], Vec::new())),
            lt(ver([p[0], p[1] + 1, 0], floor_pre())),
        ],
        _ => vec![Comparator {
            op: Ordering::Equal,
            inclusive: true,
            version: ver([p[0], p[1], p[2]], pre),
        }],
    }
}

fn desugar(op: &str, text: &str) -> Option<Vec<Comparator>> {
    let (parts, pre) = parse_partial(text)?;
    let full = |pre: Vec<String>| {
        let p: Vec<u64> = parts.iter().map(|x| x.unwrap_or(0)).collect();
        ver(
            [
                p.first().copied().unwrap_or(0),
                p.get(1).copied().unwrap_or(0),
                p.get(2).copied().unwrap_or(0),
            ],
            pre,
        )
    };
    Some(match op {
        "" | "=" => x_range(&parts, pre),
        "^" => {
            if parts.is_empty() {
                return Some(Vec::new());
            }
            let p: Vec<u64> = parts.iter().map(|x| x.unwrap_or(0)).collect();
            let upper = if p[0] > 0 || parts.len() == 1 {
                [p[0] + 1, 0, 0]
            } else if parts.len() == 2 || p[1] > 0 {
                [0, p[1] + 1, 0]
            } else {
                [0, 0, p[2] + 1]
            };
            vec![ge(full(pre)), lt(ver(upper, floor_pre()))]
        }
        "~" => {
            if parts.is_empty() {
                return Some(Vec::new());
            }
            let p: Vec<u64> = parts.iter().map(|x| x.unwrap_or(0)).collect();
            let upper = if parts.len() == 1 {
                [p[0] + 1, 0, 0]
            } else {
                [p[0], p[1] + 1, 0]
            };
            vec![ge(full(pre)), lt(ver(upper, floor_pre()))]
        }
        ">=" => {
            if parts.is_empty() {
                Vec::new()
            } else {
                vec![ge(full(pre))]
            }
        }
        "<" => {
            if parts.is_empty() {
                vec![lt(ver([0, 0, 0], floor_pre()))]
            } else {
                vec![lt(full(if parts.len() < 3 { floor_pre() } else { pre }))]
            }
        }
        ">" => {
            if parts.is_empty() {
                vec![lt(ver([0, 0, 0], floor_pre()))]
            } else if parts.len() < 3 {
                // `>1.2` means `>=1.3.0`.
                let mut b = x_range(&parts, Vec::new());
                let upper = b.pop()?.version;
                vec![ge(ver(upper.release, Vec::new()))]
            } else {
                vec![Comparator {
                    op: Ordering::Greater,
                    inclusive: false,
                    version: full(pre),
                }]
            }
        }
        "<=" => {
            if parts.is_empty() {
                Vec::new()
            } else if parts.len() < 3 {
                // `<=1.2` means `<1.3.0-0`.
                let mut b = x_range(&parts, Vec::new());
                vec![b.pop()?]
            } else {
                vec![Comparator {
                    op: Ordering::Less,
                    inclusive: true,
                    version: full(pre),
                }]
            }
        }
        _ => return None,
    })
}

fn comparator_set(text: &str) -> Option<Vec<Comparator>> {
    let text = text.trim();
    if let Some((a, b)) = text.split_once(" - ") {
        let mut out = desugar(">=", a)?;
        let (hi, _) = parse_partial(b)?;
        out.extend(if hi.len() < 3 {
            x_range(&hi, Vec::new()).pop().into_iter().collect()
        } else {
            desugar("<=", b)?
        });
        return Some(out);
    }
    let mut out = Vec::new();
    // Join operators separated from their version by spaces (`>= 1.2`).
    let mut tokens: Vec<String> = Vec::new();
    for t in text.split_whitespace() {
        match tokens.last_mut() {
            Some(last) if last.chars().all(|c| "<>=^~".contains(c)) => last.push_str(t),
            _ => tokens.push(t.to_owned()),
        }
    }
    for t in tokens {
        let op_len = t.find(|c: char| !"<>=^~".contains(c)).unwrap_or(t.len());
        let (op, v) = t.split_at(op_len);
        let op = match op {
            "~>" => "~",
            o => o,
        };
        out.extend(desugar(op, v)?);
    }
    Some(out)
}

/// Whether `version` satisfies an npm range; `None` when the range is not a version range.
pub fn satisfies_range(version: &str, range: &str) -> Option<bool> {
    let v = parse_semver(version)?;
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return Some(true);
    }
    let mut any_understood = false;
    for set in range.split("||") {
        let comps = comparator_set(set)?;
        any_understood = true;
        let ok = comps.iter().all(|c| c.test(&v));
        // A pre-release only matches a set that names a pre-release of the same version.
        let pre_ok = v.pre.is_empty()
            || comps
                .iter()
                .any(|c| !c.version.pre.is_empty() && c.version.release == v.release);
        if ok && pre_ok {
            return Some(true);
        }
    }
    any_understood.then_some(false)
}

/// Whether a dependency value is a version range (not a tag, URL, path, git, workspace or alias).
fn is_range(spec: &str) -> bool {
    let s = spec.trim();
    !(s.contains(':') || s.contains('/') || s.starts_with('.'))
        && (s.is_empty()
            || s.chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || "<>=^~*xXv".contains(c)))
}

// ---------- Runtimes ----------

fn node_exe(prefix: &Path) -> Option<PathBuf> {
    ["bin/node", "node.exe", "node"]
        .iter()
        .map(|p| prefix.join(p))
        .find(|p| p.is_file())
}

/// Version from `include/node/node_version.h` of the installation prefix.
fn header_version(prefix: &Path) -> Option<String> {
    let text = std::fs::read_to_string(prefix.join("include/node/node_version.h")).ok()?;
    let get = |name: &str| {
        text.lines().find_map(|l| {
            let rest = l.trim().strip_prefix("#define ")?.strip_prefix(name)?;
            rest.trim().parse::<u64>().ok()
        })
    };
    Some(format!(
        "{}.{}.{}",
        get("NODE_MAJOR_VERSION")?,
        get("NODE_MINOR_VERSION")?,
        get("NODE_PATCH_VERSION")?
    ))
}

fn runtime(exe: PathBuf, found_by: &str, version: Option<String>) -> NodeRuntime {
    let version = version.or_else(|| {
        let real = std::fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());
        let prefix = real.parent().and_then(Path::parent)?;
        header_version(prefix)
    });
    // A version manager's folder name (`…/v20.11.1/…`) as last resort.
    let version = version.or_else(|| {
        exe.ancestors().filter_map(|a| a.file_name()).find_map(|n| {
            let n = n.to_string_lossy();
            parse_semver(&n).map(|_| n.trim_start_matches('v').to_owned())
        })
    });
    NodeRuntime {
        path: exe.to_string_lossy().into_owned(),
        version,
        found_by: found_by.to_owned(),
    }
}

/// Managed versions under `dir`, each folder `vX.Y.Z` holding an installation (fnm: `installation/`).
fn managed(dir: &Path, sub: &str, found_by: &str) -> Vec<NodeRuntime> {
    let mut out: Vec<(SemVer, NodeRuntime)> = std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let v = parse_semver(&name)?;
                    let exe = node_exe(&e.path().join(sub))?;
                    let version = name.trim_start_matches('v').to_owned();
                    Some((v, runtime(exe, found_by, Some(version))))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| cmp_semver(&b.0, &a.0));
    out.into_iter().map(|(_, r)| r).collect()
}

/// Finds Node.js runtimes (behaviour 1–2).
pub fn find_node_runtimes(search: &NodeSearch) -> Vec<NodeRuntime> {
    let mut out = Vec::new();
    if let Some(c) = &search.configured {
        let c = if c.is_absolute() {
            c.clone()
        } else {
            search.root.join(c)
        };
        if c.is_dir() {
            out.extend(node_exe(&c).map(|e| runtime(e, "configured", None)));
        } else if c.is_file() {
            out.push(runtime(c, "configured", None));
        }
    }
    if let Some(nvm) = &search.nvm_dir {
        out.extend(managed(&nvm.join("versions/node"), "", "nvm"));
    }
    if let Some(fnm) = &search.fnm_dir {
        out.extend(managed(&fnm.join("node-versions"), "installation", "fnm"));
    }
    if let Some(path) = &search.path_var {
        for dir in std::env::split_paths(path) {
            for name in ["node", "node.exe"] {
                let p = dir.join(name);
                if p.is_file() {
                    out.push(runtime(p, "PATH", None));
                }
            }
        }
    }
    let mut seen = Vec::new();
    out.retain(|r| {
        let id = std::fs::canonicalize(&r.path).unwrap_or_else(|_| PathBuf::from(&r.path));
        if seen.contains(&id) {
            false
        } else {
            seen.push(id);
            true
        }
    });
    out
}

fn version_file(root: &Path) -> Option<String> {
    [".nvmrc", ".node-version"].iter().find_map(|f| {
        let text = std::fs::read_to_string(root.join(f)).ok()?;
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))?
            .to_owned();
        Some(line)
    })
}

fn select(runtimes: &[NodeRuntime], wanted: Option<&str>) -> (Option<usize>, Option<String>) {
    if let Some(i) = runtimes.iter().position(|r| r.found_by == "configured") {
        return (Some(i), Some("chosen in the settings".into()));
    }
    if let Some(w) = wanted {
        let range = w.trim_start_matches('v');
        if let Some(i) = runtimes.iter().position(|r| {
            r.found_by != "PATH"
                && r.version
                    .as_deref()
                    .and_then(|v| satisfies_range(v, range))
                    .unwrap_or(false)
        }) {
            return (Some(i), Some(format!("version file asks for {w}")));
        }
    }
    if let Some(i) = runtimes.iter().position(|r| r.found_by == "PATH") {
        return (Some(i), Some("first node on PATH".into()));
    }
    if runtimes.is_empty() {
        (None, None)
    } else {
        (Some(0), Some("newest managed version".into()))
    }
}

// ---------- Project ----------

const LOCKFILES: &[(&str, &str)] = &[
    ("package-lock.json", "npm"),
    ("npm-shrinkwrap.json", "npm"),
    ("yarn.lock", "yarn"),
    ("pnpm-lock.yaml", "pnpm"),
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
];

/// A script of `package.json` (`SPEC-0168` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageScript {
    /// 0-based line of its name.
    pub line: usize,
    /// Its name (`dev`).
    pub name: String,
    /// What it runs (`vite`).
    pub command: String,
}

/// The scripts of a `package.json` text in their order, with the line of each name; empty when
/// the text has no `scripts` object at the top level.
pub fn package_scripts(text: &str) -> Vec<PackageScript> {
    // Strings (with their line) and the punctuation between them; numbers and words are skipped.
    enum Token {
        Str(String, usize),
        Punct(char),
    }
    let mut tokens = Vec::new();
    let mut line = 0;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            '"' => {
                let mut value = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            if let Some(e) = chars.next() {
                                value.push(match e {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                            }
                        }
                        '\n' => line += 1,
                        c => value.push(c),
                    }
                }
                tokens.push(Token::Str(value, line));
            }
            '{' | '}' | '[' | ']' | ':' | ',' => tokens.push(Token::Punct(c)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut in_scripts = false;
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Punct('{' | '[') => depth += 1,
            Token::Punct('}' | ']') => {
                depth = depth.saturating_sub(1);
                if depth < 2 {
                    in_scripts = false;
                }
            }
            Token::Str(key, line) if matches!(tokens.get(i + 1), Some(Token::Punct(':'))) => {
                if depth == 1
                    && key == "scripts"
                    && matches!(tokens.get(i + 2), Some(Token::Punct('{')))
                {
                    in_scripts = true;
                } else if depth == 2
                    && in_scripts
                    && let Some(Token::Str(command, _)) = tokens.get(i + 2)
                {
                    out.push(PackageScript {
                        line: *line,
                        name: key.clone(),
                        command: command.clone(),
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The package manager of the package in `dir` with manifest `text`: its `packageManager`, else
/// the one of its lockfile, else npm (`SPEC-0168` behaviour 4).
pub fn package_manager_of(dir: &Path, text: &str) -> String {
    let manifest: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    if let Some(declared) = manifest["packageManager"].as_str() {
        return declared.split('@').next().unwrap_or("npm").to_owned();
    }
    LOCKFILES
        .iter()
        .find(|(f, _)| dir.join(f).is_file())
        .map_or("npm", |(_, m)| *m)
        .to_owned()
}

/// The run configuration of script `script` of the `package.json` in workspace folder `rel_dir`
/// (`.` for the root): `<package manager> run <script>` in that folder, named like the command.
pub fn package_script_config(dir: &Path, rel_dir: &str, text: &str, script: &str) -> RunConfig {
    let pm = package_manager_of(dir, text);
    RunConfig {
        name: format!("{pm} run {script}"),
        command: vec![pm, "run".into(), script.into()],
        cwd: (rel_dir != "." && !rel_dir.is_empty()).then(|| rel_dir.to_owned()),
        env: BTreeMap::new(),
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
        http: None,
    }
}

fn installed_version(root: &Path, name: &str) -> Option<String> {
    let text =
        std::fs::read_to_string(root.join("node_modules").join(name).join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["version"].as_str().map(str::to_owned)
}

/// Runtimes, selection, package manager, installed packages and checks against `package.json`
/// (behaviour 1–7).
pub fn node_environment(search: &NodeSearch) -> NodeEnvironment {
    let root = &search.root;
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    let manifest: serde_json::Value = match std::fs::read_to_string(root.join("package.json")) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            problems.push(format!("package.json: {e}"));
            serde_json::Value::Null
        }),
        Err(_) => serde_json::Value::Null,
    };
    let runtimes = find_node_runtimes(search);
    let wanted = version_file(root);
    let (selected, reason) = select(&runtimes, wanted.as_deref());
    let chosen = selected.map(|i| &runtimes[i]);
    let engines_node = manifest["engines"]["node"].as_str().map(str::to_owned);
    let compatible = match (chosen.and_then(|r| r.version.as_deref()), &engines_node) {
        (Some(v), Some(range)) => satisfies_range(v, range),
        _ => None,
    };
    if compatible == Some(false) {
        notes.push(format!(
            "the selected node ({}) does not satisfy engines.node {}",
            chosen.and_then(|r| r.version.clone()).unwrap_or_default(),
            engines_node.clone().unwrap_or_default()
        ));
    }
    if let (Some(w), Some(v)) = (&wanted, chosen.and_then(|r| r.version.as_deref()))
        && satisfies_range(v, w.trim_start_matches('v')) == Some(false)
    {
        notes.push(format!(
            "no installed node matches the version file ({w}); using {v}"
        ));
    }

    let lockfiles: Vec<String> = LOCKFILES
        .iter()
        .filter(|(f, _)| root.join(f).is_file())
        .map(|(f, _)| (*f).to_owned())
        .collect();
    let lock_managers: Vec<&str> = {
        let mut m: Vec<&str> = LOCKFILES
            .iter()
            .filter(|(f, _)| lockfiles.iter().any(|l| l == f))
            .map(|(_, m)| *m)
            .collect();
        m.dedup();
        m
    };
    if lock_managers.len() > 1 {
        problems.push(format!(
            "lockfiles of several package managers: {}",
            lockfiles.join(", ")
        ));
    }
    let declared_pm = manifest["packageManager"].as_str().map(|s| {
        let (name, version) = s.split_once('@').unwrap_or((s, ""));
        let version = version.split('+').next().unwrap_or("").to_owned();
        PackageManager {
            name: name.to_owned(),
            version: (!version.is_empty()).then_some(version),
            source: "packageManager".into(),
        }
    });
    if let Some(pm) = &declared_pm
        && !lock_managers.is_empty()
        && !lock_managers.contains(&pm.name.as_str())
    {
        problems.push(format!(
            "packageManager is {} but the lockfile is {}",
            pm.name,
            lockfiles.join(", ")
        ));
    }
    let package_manager = declared_pm.or_else(|| {
        if manifest.is_null() {
            return None;
        }
        Some(match lock_managers.first() {
            Some(m) => PackageManager {
                name: (*m).to_owned(),
                version: None,
                source: lockfiles[0].clone(),
            },
            None => PackageManager {
                name: "npm".into(),
                version: None,
                source: "default".into(),
            },
        })
    });

    let installed = root.join("node_modules").is_dir();
    let mut unsatisfied = Vec::new();
    let mut not_checked = Vec::new();
    let mut names = Vec::new();
    for kind in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(map) = manifest[kind].as_object() else {
            continue;
        };
        // Sorted by name: JSON object order depends on a serde_json feature other crates may enable.
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by_key(|(name, _)| name.as_str());
        for (name, spec) in entries {
            names.push(name.clone());
            let range = spec.as_str().unwrap_or_default();
            if !is_range(range) {
                not_checked.push(format!("{name} {range}"));
                continue;
            }
            let have = installed_version(root, name);
            let ok = have
                .as_deref()
                .is_some_and(|v| satisfies_range(v, range).unwrap_or(true));
            // Optional and peer dependencies may be absent.
            let may_miss = matches!(kind, "optionalDependencies" | "peerDependencies");
            if !ok && !(may_miss && have.is_none()) {
                unsatisfied.push(NodeUnsatisfied {
                    name: name.clone(),
                    range: range.to_owned(),
                    kind: kind.to_owned(),
                    installed: have,
                });
            }
        }
    }
    if !installed && !names.is_empty() {
        notes.push("dependencies are not installed (no node_modules)".into());
    }

    let has = |n: &str| names.iter().any(|x| x == n);
    let script_test = manifest["scripts"]["test"].as_str().unwrap_or_default();
    let bin = chosen.and_then(|r| Path::new(&r.path).parent().map(Path::to_path_buf));
    let npx = bin
        .as_ref()
        .map(|b| b.join("npx"))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "npx".into());
    let config = |name: &str,
                  command: Vec<String>,
                  kind: RunKind,
                  report: Option<&str>,
                  file: Option<&str>| RunConfig {
        name: name.into(),
        command,
        cwd: None,
        env: BTreeMap::new(),
        kind,
        report: report.map(str::to_owned),
        report_file: file.map(str::to_owned),
        before: Vec::new(),
        timeout: None,
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: false,
        http: None,
    };
    let (test_framework, test_config) = if has("vitest") {
        (
            Some("vitest"),
            Some(config(
                "vitest",
                vec![
                    npx,
                    "vitest".into(),
                    "run".into(),
                    "--reporter=json".into(),
                    "--outputFile=.ferris-idea/vitest.json".into(),
                ],
                RunKind::Test,
                Some("jest"),
                Some(".ferris-idea/vitest.json"),
            )),
        )
    } else if has("jest") {
        (
            Some("jest"),
            Some(config(
                "jest",
                vec![
                    npx,
                    "jest".into(),
                    "--ci".into(),
                    "--json".into(),
                    "--outputFile=.ferris-idea/jest.json".into(),
                ],
                RunKind::Test,
                Some("jest"),
                Some(".ferris-idea/jest.json"),
            )),
        )
    } else if has("mocha") {
        (
            Some("mocha"),
            Some(config(
                "mocha",
                vec![npx, "mocha".into(), "--reporter".into(), "tap".into()],
                RunKind::Test,
                Some("tap"),
                None,
            )),
        )
    } else if script_test.contains("node --test") || (!manifest.is_null() && chosen.is_some()) {
        let node = chosen
            .map(|r| r.path.clone())
            .unwrap_or_else(|| "node".into());
        (
            Some("node:test"),
            Some(config(
                "node --test",
                vec![node, "--test".into(), "--test-reporter=tap".into()],
                RunKind::Test,
                Some("tap"),
                None,
            )),
        )
    } else {
        (None, None)
    };

    // Web frameworks and where their development servers listen by default (SPEC-0133).
    let mut frameworks = Vec::new();
    for (package, name) in [
        ("next", "Next.js"),
        ("nuxt", "Nuxt"),
        ("@angular/core", "Angular"),
        ("@sveltejs/kit", "SvelteKit"),
        ("svelte", "Svelte"),
        ("vue", "Vue"),
        ("react", "React"),
        ("solid-js", "Solid"),
    ] {
        if has(package) {
            frameworks.push(name.to_owned());
        }
    }
    let port = if has("next") || has("nuxt") || has("react-scripts") {
        Some(3000)
    } else if has("@angular/core") {
        Some(4200)
    } else if has("vite") || has("@sveltejs/kit") {
        Some(5173)
    } else if has("@vue/cli-service") {
        Some(8080)
    } else {
        None
    };
    let pm = package_manager
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "npm".into());
    let dev_script = ["dev", "start", "serve"]
        .into_iter()
        .find(|s| manifest["scripts"][*s].is_string());
    let dev_config = dev_script.filter(|_| !frameworks.is_empty()).map(|script| {
        config(
            &format!("{pm} run {script}"),
            vec![pm.clone(), "run".into(), script.into()],
            RunKind::Run,
            None,
            None,
        )
    });
    let browser_config = port.filter(|_| !frameworks.is_empty()).map(|p| RunConfig {
        url: Some(format!("http://localhost:{p}")),
        ..config("Debug in Browser", Vec::new(), RunKind::Run, None, None)
    });

    NodeEnvironment {
        frameworks,
        dev_config,
        browser_config,
        runtimes,
        selected,
        reason,
        version_file: wanted,
        engines_node,
        compatible,
        package_manager,
        lockfiles,
        installed,
        unsatisfied,
        not_checked,
        test_framework: test_framework.map(str::to_owned),
        test_config,
        problems,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0168 AK-4: scripts with their lines, nested `scripts` keys ignored, package manager.
    #[test]
    fn package_scripts_with_lines() {
        let text = "{\n  \"name\": \"web\",\n  \"config\": { \"scripts\": { \"no\": \"x\" } },\n  \"scripts\": {\n    \"dev\": \"vite --port 5173\",\n    \"build\": \"tsc && vite build\",\n    \"q\\\"x\": \"echo \\\"hi\\\"\"\n  },\n  \"version\": \"1.0.0\"\n}\n";
        let scripts = package_scripts(text);
        let got: Vec<(usize, &str, &str)> = scripts
            .iter()
            .map(|s| (s.line, s.name.as_str(), s.command.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (4, "dev", "vite --port 5173"),
                (5, "build", "tsc && vite build"),
                (6, "q\"x", "echo \"hi\""),
            ]
        );
        assert!(package_scripts("{}").is_empty());
        assert!(package_scripts("not json").is_empty());

        let dir = std::env::temp_dir().join(format!("ferris-idea-pm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(package_manager_of(&dir, "{}"), "npm");
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(package_manager_of(&dir, "{}"), "pnpm");
        assert_eq!(
            package_manager_of(&dir, r#"{"packageManager": "yarn@4.1.0+sha"}"#),
            "yarn"
        );
        let c = package_script_config(&dir, "apps/web", "{}", "dev");
        assert_eq!(c.name, "pnpm run dev");
        assert_eq!(c.command, ["pnpm", "run", "dev"]);
        assert_eq!(c.cwd.as_deref(), Some("apps/web"));
        assert_eq!(package_script_config(&dir, ".", "{}", "dev").cwd, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn npm_ranges() {
        let ok = |v: &str, r: &str| satisfies_range(v, r).unwrap();
        // Caret.
        assert!(ok("19.1.0", "^19.0.0"));
        assert!(!ok("20.0.0", "^19.0.0"));
        assert!(ok("0.2.9", "^0.2.3"));
        assert!(!ok("0.3.0", "^0.2.3"));
        assert!(ok("0.0.3", "^0.0.3"));
        assert!(!ok("0.0.4", "^0.0.3"));
        assert!(ok("1.9.0", "^1.x"));
        assert!(ok("0.9.0", "^0.x"));
        // Tilde.
        assert!(ok("1.2.9", "~1.2.3"));
        assert!(!ok("1.3.0", "~1.2.3"));
        assert!(ok("1.9.0", "~1"));
        // X-ranges and partials.
        assert!(ok("1.2.7", "1.2.x"));
        assert!(!ok("1.3.0", "1.2"));
        assert!(ok("3.0.0", "*"));
        assert!(ok("3.0.0", ""));
        // Comparators, hyphen, or.
        assert!(ok("20.11.1", ">=18"));
        assert!(!ok("16.20.2", ">=18"));
        assert!(ok("1.3.0", ">1.2"));
        assert!(!ok("1.2.9", ">1.2"));
        assert!(ok("1.2.9", "<=1.2"));
        assert!(!ok("1.3.0", "<=1.2"));
        assert!(ok("2.3.0", "1.2.3 - 2.3"));
        assert!(!ok("2.4.0", "1.2.3 - 2.3"));
        assert!(ok("16.10.1", "^14.17.0 || ^16.10.0 || >=17.0.0"));
        assert!(!ok("16.0.0", "^14.17.0 || ^16.10.0 || >=17.0.0"));
        assert!(ok("18.1.0", ">= 18.0.0 < 19"));
        assert!(!ok("19.0.0", ">= 18.0.0 < 19"));
        // Pre-releases only match when named.
        assert!(!ok("2.0.0-rc.1", "^1.0.0 || >=2.0.0-0 <1.0.0"));
        assert!(!ok("2.0.0-beta.1", ">=1.0.0"));
        assert!(ok("2.0.0-beta.2", ">=2.0.0-beta.1"));
        assert!(!ok("2.0.0-alpha", ">=2.0.0-beta.1"));
        assert_eq!(satisfies_range("1.0.0", "foo"), None);
    }

    #[test]
    fn range_detection() {
        assert!(is_range("^1.2.3"));
        assert!(is_range(">=1 <2"));
        assert!(is_range("*"));
        assert!(!is_range("latest"));
        assert!(!is_range("workspace:*"));
        assert!(!is_range("file:../lib"));
        assert!(!is_range("github:user/repo"));
        assert!(!is_range("npm:react@^18"));
    }
}

//! Ruby runtimes, requested version and Bundler (`SPEC-0185`), read statically.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One Ruby runtime (behaviour 1–2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RubyRuntime {
    /// Path of the `ruby` executable.
    pub path: String,
    /// Version as far as it is known without starting it.
    pub version: Option<String>,
    /// How it was found: `configured`, `rbenv`, `rvm`, `chruby`, `asdf`, `mise`, `PATH`.
    pub found_by: String,
}

/// One source of the requested version (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RubyRequest {
    /// File it comes from: `.ruby-version`, `.tool-versions`, `mise.toml`, `.mise.toml`, `Gemfile`.
    pub file: String,
    /// The version or requirement as written (a leading `ruby-` dropped).
    pub version: String,
}

/// Bundler state of the project (behaviour 6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Bundle {
    /// Whether `Gemfile` exists.
    pub gemfile: bool,
    /// Whether `Gemfile.lock` exists.
    pub lockfile: bool,
    /// Locked gems with their versions.
    pub gems: BTreeMap<String, String>,
    /// `RUBY VERSION` of the lockfile.
    pub ruby: Option<String>,
    /// `BUNDLED WITH` of the lockfile.
    pub bundler: Option<String>,
}

/// Inputs of the search; [`RubySearch::from_env`] fills them from the process environment.
#[derive(Debug, Clone, Default)]
pub struct RubySearch {
    /// Workspace root.
    pub root: PathBuf,
    /// `PATH`.
    pub path_var: Option<OsString>,
    /// Home folder (chruby's `~/.rubies`, `~/.gem`).
    pub home: Option<PathBuf>,
    /// rbenv folder (`RBENV_ROOT` or `~/.rbenv`).
    pub rbenv_root: Option<PathBuf>,
    /// rvm folder (`rvm_path` or `~/.rvm`).
    pub rvm_path: Option<PathBuf>,
    /// asdf data folder (`ASDF_DATA_DIR` or `~/.asdf`).
    pub asdf_dir: Option<PathBuf>,
    /// mise data folder (`MISE_DATA_DIR` or `~/.local/share/mise`).
    pub mise_dir: Option<PathBuf>,
    /// Extra chruby folders (`/opt/rubies`).
    pub rubies_dirs: Vec<PathBuf>,
    /// `GEM_HOME`.
    pub gem_home: Option<PathBuf>,
    /// Ruby chosen in the settings (executable or installation folder).
    pub configured: Option<PathBuf>,
}

impl RubySearch {
    /// Search inputs from the environment of this process.
    pub fn from_env(root: &Path) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let env_or = |var: &str, rel: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(rel)))
        };
        Self {
            root: root.to_path_buf(),
            path_var: std::env::var_os("PATH"),
            rbenv_root: env_or("RBENV_ROOT", ".rbenv"),
            rvm_path: env_or("rvm_path", ".rvm"),
            asdf_dir: env_or("ASDF_DATA_DIR", ".asdf"),
            mise_dir: env_or("MISE_DATA_DIR", ".local/share/mise"),
            rubies_dirs: vec![PathBuf::from("/opt/rubies")],
            gem_home: std::env::var_os("GEM_HOME").map(PathBuf::from),
            home,
            configured: None,
        }
    }
}

/// Result of [`ruby_environment`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RubyEnvironment {
    /// Runtimes found, without duplicates.
    pub runtimes: Vec<RubyRuntime>,
    /// Index of the selected runtime.
    pub selected: Option<usize>,
    /// Why it was selected.
    pub reason: Option<String>,
    /// Every source of a requested version, in precedence order.
    pub requested: Vec<RubyRequest>,
    /// Bundler state.
    pub bundle: Bundle,
    /// Folders the language server and `bundle` are looked up in, before `PATH` (behaviour 7).
    pub server_path: Vec<String>,
    /// Problems: contradicting version files, a request no runtime matches.
    pub problems: Vec<String>,
    /// Remarks.
    pub notes: Vec<String>,
}

impl RubyEnvironment {
    /// The selected runtime.
    pub fn selected_runtime(&self) -> Option<&RubyRuntime> {
        self.selected.and_then(|i| self.runtimes.get(i))
    }
}

// ---------- Versions and requirements ----------

fn segments(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().split(['-', '+', 'p']).next()?;
    core.split('.').map(|s| s.parse().ok()).collect()
}

fn compare(a: &[u64], b: &[u64]) -> Ordering {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| {
            a.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&b.get(i).copied().unwrap_or(0))
        })
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// Whether `version` matches `wanted` (behaviour 4): a plain version as segment prefix, else
/// comma-separated RubyGems requirements. `None` when either side cannot be read.
pub fn ruby_version_matches(version: &str, wanted: &str) -> Option<bool> {
    let have = segments(version)?;
    let wanted = wanted.trim().trim_start_matches("ruby-");
    if wanted.starts_with(|c: char| c.is_ascii_digit()) && !wanted.contains(',') {
        let want = segments(wanted)?;
        return Some(have.len() >= want.len() && have[..want.len()] == want[..]);
    }
    let mut all = true;
    for part in wanted.split(',') {
        let part = part.trim();
        let op_len = part
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(part.len());
        let (op, v) = (part[..op_len].trim(), &part[op_len..]);
        let want = segments(v)?;
        let ord = compare(&have, &want);
        let ok = match op {
            "" | "=" => ord.is_eq(),
            "!=" => ord.is_ne(),
            ">" => ord.is_gt(),
            "<" => ord.is_lt(),
            ">=" => ord.is_ge(),
            "<=" => ord.is_le(),
            "~>" => {
                let mut upper = want[..(want.len() - 1).max(1)].to_vec();
                if let Some(last) = upper.last_mut() {
                    *last += 1;
                }
                ord.is_ge() && compare(&have, &upper).is_lt()
            }
            _ => return None,
        };
        all &= ok;
    }
    Some(all)
}

// ---------- Runtimes ----------

fn runtime(path: PathBuf, found_by: &str, version: Option<String>) -> RubyRuntime {
    let version = version.or_else(|| version_in_path(&path));
    RubyRuntime {
        path: path.display().to_string(),
        version,
        found_by: found_by.into(),
    }
}

/// A version-like folder name in the resolved path of `exe` (behaviour 2).
fn version_in_path(exe: &Path) -> Option<String> {
    let real = std::fs::canonicalize(exe).ok()?;
    real.ancestors()
        .filter_map(|a| a.file_name()?.to_str())
        .map(|n| n.trim_start_matches("ruby-"))
        .find(|n| n.contains('.') && segments(n).is_some())
        .map(str::to_owned)
}

/// `<dir>/<version>/bin/ruby` for every installation in `dir`, newest first.
fn managed(dir: &Path, found_by: &str) -> Vec<RubyRuntime> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(Vec<u64>, RubyRuntime)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let exe = e.path().join("bin/ruby");
            if !exe.is_file() {
                return None;
            }
            let version = name.trim_start_matches("ruby-").to_owned();
            let key = segments(&version).unwrap_or_default();
            let version = segments(&version).map(|_| version);
            Some((key, runtime(exe, found_by, version)))
        })
        .collect();
    found.sort_by(|a, b| compare(&b.0, &a.0));
    found.into_iter().map(|(_, r)| r).collect()
}

/// All runtimes (behaviour 1).
pub fn find_ruby_runtimes(search: &RubySearch) -> Vec<RubyRuntime> {
    let mut out = Vec::new();
    if let Some(c) = &search.configured {
        let c = if c.is_absolute() {
            c.clone()
        } else {
            search.root.join(c)
        };
        if c.is_dir() {
            let exe = c.join("bin/ruby");
            if exe.is_file() {
                out.push(runtime(exe, "configured", None));
            }
        } else if c.is_file() {
            out.push(runtime(c, "configured", None));
        }
    }
    if let Some(d) = &search.rbenv_root {
        out.extend(managed(&d.join("versions"), "rbenv"));
    }
    if let Some(d) = &search.rvm_path {
        out.extend(managed(&d.join("rubies"), "rvm"));
    }
    let chruby = search
        .home
        .iter()
        .map(|h| h.join(".rubies"))
        .chain(search.rubies_dirs.iter().cloned());
    for d in chruby {
        out.extend(managed(&d, "chruby"));
    }
    if let Some(d) = &search.asdf_dir {
        out.extend(managed(&d.join("installs/ruby"), "asdf"));
    }
    if let Some(d) = &search.mise_dir {
        out.extend(managed(&d.join("installs/ruby"), "mise"));
    }
    if let Some(path) = &search.path_var {
        for dir in std::env::split_paths(path) {
            let p = dir.join("ruby");
            if p.is_file() {
                out.push(runtime(p, "PATH", None));
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

// ---------- Project ----------

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches(|c| c == '"' || c == '\'')
}

/// Requested versions in precedence order (behaviour 3).
fn requests(root: &Path) -> Vec<RubyRequest> {
    let read = |f: &str| std::fs::read_to_string(root.join(f)).ok();
    let mut out = Vec::new();
    let mut push = |file: &str, version: String| {
        out.push(RubyRequest {
            file: file.into(),
            version: version.trim_start_matches("ruby-").to_owned(),
        })
    };
    if let Some(v) = read(".ruby-version").as_deref().and_then(first_line) {
        push(".ruby-version", v);
    }
    if let Some(text) = read(".tool-versions") {
        let v = text.lines().find_map(|l| {
            let mut words = l.split_whitespace();
            (words.next() == Some("ruby")).then(|| words.next().map(str::to_owned))?
        });
        if let Some(v) = v {
            push(".tool-versions", v);
        }
    }
    for file in ["mise.toml", ".mise.toml"] {
        let Some(text) = read(file) else { continue };
        let mut in_tools = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_tools = line == "[tools]";
            } else if in_tools
                && let Some((k, v)) = line.split_once('=')
                && k.trim() == "ruby"
            {
                push(file, unquote(v).to_owned());
            }
        }
    }
    if let Some(text) = read("Gemfile") {
        let v = text.lines().map(str::trim).find_map(|l| {
            let rest = l
                .strip_prefix("ruby ")
                .or_else(|| l.strip_prefix("ruby("))?;
            let rest = rest.trim().trim_end_matches(')');
            // `ruby file: ".ruby-version"` points at a file already read.
            if rest.starts_with("file:") || rest.starts_with(":file") {
                return None;
            }
            let reqs: Vec<&str> = rest
                .split(',')
                .map(unquote)
                .filter(|r| r.starts_with(|c: char| c.is_ascii_digit() || "<>=!~".contains(c)))
                .collect();
            (!reqs.is_empty()).then(|| reqs.join(", "))
        });
        if let Some(v) = v {
            push("Gemfile", v);
        }
    }
    out
}

fn exact(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_digit()) && !version.contains(',')
}

/// Contradictions between the requests (behaviour 3).
fn contradictions(requests: &[RubyRequest]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in requests.iter().enumerate() {
        for b in &requests[i + 1..] {
            let clash = match (exact(&a.version), exact(&b.version)) {
                (true, true) => {
                    ruby_version_matches(&a.version, &b.version) == Some(false)
                        && ruby_version_matches(&b.version, &a.version) == Some(false)
                }
                (true, false) => ruby_version_matches(&a.version, &b.version) == Some(false),
                (false, true) => ruby_version_matches(&b.version, &a.version) == Some(false),
                (false, false) => false,
            };
            if clash {
                out.push(format!(
                    "{} asks for {} but {} for {}",
                    a.file, a.version, b.file, b.version
                ));
            }
        }
    }
    out
}

/// Reads `Gemfile` and `Gemfile.lock` (behaviour 6).
pub fn read_bundle(root: &Path) -> Bundle {
    let mut bundle = Bundle {
        gemfile: root.join("Gemfile").is_file(),
        ..Bundle::default()
    };
    let Ok(text) = std::fs::read_to_string(root.join("Gemfile.lock")) else {
        return bundle;
    };
    bundle.lockfile = true;
    let mut section = "";
    let mut in_specs = false;
    for line in text.lines() {
        if !line.starts_with(' ') {
            section = line.trim();
            in_specs = false;
            continue;
        }
        match section {
            "GEM" | "PATH" | "GIT" => {
                if line.trim() == "specs:" {
                    in_specs = true;
                } else if in_specs
                    && line.starts_with("    ")
                    && !line.starts_with("     ")
                    && let Some((name, rest)) = line.trim().split_once(" (")
                {
                    let version = rest.trim_end_matches(')');
                    // Platform gems: `nokogiri (1.16.0-arm64-darwin)`.
                    bundle.gems.insert(name.to_owned(), version.to_owned());
                }
            }
            "RUBY VERSION" => {
                bundle.ruby = line.trim().strip_prefix("ruby ").map(str::to_owned);
            }
            "BUNDLED WITH" => bundle.bundler = Some(line.trim().to_owned()),
            _ => {}
        }
    }
    bundle
}

fn select(
    runtimes: &[RubyRuntime],
    wanted: Option<&RubyRequest>,
) -> (Option<usize>, Option<String>) {
    if let Some(i) = runtimes.iter().position(|r| r.found_by == "configured") {
        return (Some(i), Some("chosen in the settings".into()));
    }
    if let Some(w) = wanted {
        // Newest wins; among equal versions the first found (reversed, as `max_by` keeps the last).
        let best = runtimes
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, r)| {
                r.found_by != "PATH"
                    && r.version
                        .as_deref()
                        .and_then(|v| ruby_version_matches(v, &w.version))
                        .unwrap_or(false)
            })
            .max_by(|(_, a), (_, b)| {
                let (a, b) = (a.version.as_deref(), b.version.as_deref());
                compare(
                    &a.and_then(segments).unwrap_or_default(),
                    &b.and_then(segments).unwrap_or_default(),
                )
            });
        if let Some((i, _)) = best {
            return (Some(i), Some(format!("{} asks for {}", w.file, w.version)));
        }
    }
    if let Some(i) = runtimes.iter().position(|r| r.found_by == "PATH") {
        return (Some(i), Some("first ruby on PATH".into()));
    }
    if runtimes.is_empty() {
        (None, None)
    } else {
        (Some(0), Some("newest managed version".into()))
    }
}

/// Folders searched before `PATH` for the server and `bundle` (behaviour 7).
fn server_path(search: &RubySearch, selected: Option<&RubyRuntime>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(r) = selected
        && let Some(bin) = Path::new(&r.path).parent()
    {
        out.push(bin.to_path_buf());
    }
    if let Some(g) = &search.gem_home {
        out.push(g.join("bin"));
    }
    if let (Some(home), Some(v)) = (
        &search.home,
        selected
            .and_then(|r| r.version.as_deref())
            .and_then(segments),
    ) && v.len() >= 2
    {
        out.push(home.join(format!(".gem/ruby/{}.{}.0/bin", v[0], v[1])));
    }
    out
}

/// Runtimes, selection, requested versions and Bundler state (behaviour 1–7).
pub fn ruby_environment(search: &RubySearch) -> RubyEnvironment {
    let runtimes = find_ruby_runtimes(search);
    let requested = requests(&search.root);
    let mut problems = contradictions(&requested);
    let (selected, reason) = select(&runtimes, requested.first());
    if let Some(w) = requested.first() {
        let any = runtimes.iter().any(|r| {
            r.version
                .as_deref()
                .and_then(|v| ruby_version_matches(v, &w.version))
                .unwrap_or(false)
        });
        if !any {
            problems.push(format!(
                "{} asks for Ruby {} but no installed Ruby matches",
                w.file, w.version
            ));
        }
    }
    let bundle = read_bundle(&search.root);
    let mut notes = Vec::new();
    if bundle.gemfile && !bundle.lockfile {
        notes.push("Gemfile without Gemfile.lock: run bundle install".into());
    }
    if runtimes.is_empty() {
        notes.push("no Ruby found".into());
    }
    let server_path = server_path(search, selected.map(|i| &runtimes[i]))
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    RubyEnvironment {
        runtimes,
        selected,
        reason,
        requested,
        bundle,
        server_path,
        problems,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements() {
        let m = |v, w| ruby_version_matches(v, w).unwrap();
        assert!(m("3.3.6", "3.3"));
        assert!(!m("3.30.0", "3.3"));
        assert!(m("3.2.9", "~> 3.2"));
        assert!(m("3.9.0", "~> 3.2"));
        assert!(!m("4.0.0", "~> 3.2"));
        assert!(m("3.2.4", "~> 3.2.1"));
        assert!(!m("3.3.0", "~> 3.2.1"));
        assert!(!m("3.2.0", "~> 3.2.1"));
        assert!(m("3.3.1", ">= 3.1, < 3.4"));
        assert!(!m("3.4.0", ">= 3.1, < 3.4"));
        assert!(!m("3.3.0", "!= 3.3.0"));
        assert!(m("3.3.0p0", "3.3.0"));
        assert_eq!(ruby_version_matches("jruby", "3.3"), None);
    }

    #[test]
    fn contradicting_requests() {
        let r = |file: &str, v: &str| RubyRequest {
            file: file.into(),
            version: v.into(),
        };
        let found = contradictions(&[r(".ruby-version", "3.3.0"), r("Gemfile", "~> 3.2.0")]);
        assert_eq!(
            found,
            ["`.ruby-version` asks for 3.3.0 but Gemfile for ~> 3.2.0".replace('`', "")]
        );
        assert!(
            contradictions(&[r(".ruby-version", "3.3"), r(".tool-versions", "3.3.6")]).is_empty()
        );
    }
}

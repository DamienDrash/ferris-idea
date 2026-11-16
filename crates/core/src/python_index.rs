//! Searching the Python package index and installing from it (`SPEC-0171`).
//!
//! The index offers no search API, so the names come from its simple API (PEP 691, JSON) and are
//! kept in the user cache; details come from the JSON API of one project. Network access goes
//! through a [`Fetch`] function the caller provides, so tests can stand in for the index.

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::python::{Interpreter, InterpreterKind, environment_tools, normalize_name};
use crate::python_manage::pip_install_command;

/// The only index contacted (behaviour 1).
pub const PYPI: &str = "https://pypi.org";

/// Most matches returned by [`search_names`] (behaviour 2).
pub const SEARCH_LIMIT: usize = 50;

/// Age after which the name list is downloaded again (behaviour 1).
pub const NAMES_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// A response of the index: status, body, and whether the body is gzip-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// HTTP status.
    pub status: u16,
    /// Body bytes as received.
    pub body: Vec<u8>,
    /// `Content-Encoding: gzip`.
    pub gzip: bool,
}

/// Fetches `url` with the given `Accept` header and `Accept-Encoding: gzip`.
pub type Fetch<'a> = &'a dyn Fn(&str, &str) -> Result<Fetched, String>;

fn body(f: &Fetched) -> Result<Vec<u8>, String> {
    if !f.gzip {
        return Ok(f.body.clone());
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&f.body[..])
        .read_to_end(&mut out)
        .map_err(|e| format!("cannot unpack the index answer: {e}"))?;
    Ok(out)
}

/// Project names from a simple API JSON page (`projects[].name`).
pub fn parse_simple_names(json: &[u8]) -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct Project {
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct Page {
        projects: Vec<Project>,
    }
    let page: Page =
        serde_json::from_slice(json).map_err(|e| format!("unexpected index answer: {e}"))?;
    Ok(page.projects.into_iter().map(|p| p.name).collect())
}

/// IDECraft's user cache folder: `$XDG_CACHE_HOME/idecraft`, else `~/Library/Caches/idecraft` on
/// macOS, else `~/.cache/idecraft`; `None` without `HOME`.
pub fn user_cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(d).join("idecraft"));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Caches/idecraft")
    } else {
        home.join(".cache/idecraft")
    })
}

/// The file holding the cached names: `pypi-names.txt` in [`user_cache_dir`].
pub fn names_cache_file() -> Option<PathBuf> {
    Some(user_cache_dir()?.join("pypi-names.txt"))
}

/// The cached names in `file` when it is younger than `max_age` (behaviour 1).
pub fn cached_names(file: &Path, max_age: Duration) -> Option<Vec<String>> {
    let modified = std::fs::metadata(file).ok()?.modified().ok()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    if age > max_age {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    Some(text.lines().map(str::to_owned).collect())
}

/// Whether `file` holds names fresh enough to be used without downloading (file date only).
pub fn names_cache_fresh(file: &Path) -> bool {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age <= NAMES_MAX_AGE)
}

/// Downloads the name list from `index` and writes it to `file` (one name per line).
pub fn download_names(fetch: Fetch, index: &str, file: &Path) -> Result<Vec<String>, String> {
    let url = format!("{}/simple/", index.trim_end_matches('/'));
    let got = fetch(&url, "application/vnd.pypi.simple.v1+json")?;
    if got.status != 200 {
        return Err(format!("{url}: HTTP {}", got.status));
    }
    let names = parse_simple_names(&body(&got)?)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("part");
    std::fs::write(&tmp, names.join("\n")).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    Ok(names)
}

/// The names: cached when fresh, else downloaded (behaviour 1).
pub fn names(fetch: Fetch, index: &str, file: &Path) -> Result<Vec<String>, String> {
    match cached_names(file, NAMES_MAX_AGE) {
        Some(n) => Ok(n),
        None => download_names(fetch, index, file),
    }
}

/// Names matching `query` (behaviour 2): exact, then starting with it, then containing it, each
/// group shorter first (then by name), at most `limit`. Compared normalised (PEP 503).
pub fn search_names(names: &[String], query: &str, limit: usize) -> Vec<String> {
    let q = normalize_name(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u8, usize, String, &String)> = names
        .iter()
        .filter_map(|n| {
            let norm = normalize_name(n);
            let rank = if norm == q {
                0
            } else if norm.starts_with(&q) {
                1
            } else if norm.contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank, norm.len(), norm, n))
        })
        .collect();
    hits.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    hits.into_iter().take(limit).map(|h| h.3.clone()).collect()
}

/// One release of a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    /// Version as published.
    pub version: String,
    /// Alpha, beta, release candidate or development release.
    pub prerelease: bool,
}

/// What the index says about one package (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageInfo {
    /// Name as published.
    pub name: String,
    /// One-line summary.
    pub summary: String,
    /// Latest version.
    pub latest: String,
    /// `requires_python`, if any.
    pub requires_python: Option<String>,
    /// Home page or project URL.
    pub home_page: Option<String>,
    /// Releases newest first, without fully yanked ones.
    pub releases: Vec<Release>,
}

// PEP 440 ordering: release, then dev < a < b < rc < final < post.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    epoch: u64,
    release: Vec<u64>,
    /// (phase, number): dev-only = (0, n), a = (1, n), b = (2, n), rc = (3, n), final = (4, 0).
    pre: (u8, u64),
    post: Option<u64>,
    /// Dev releases sort before everything else of the same version (None last).
    dev: (u8, u64),
}

fn number_after(s: &str, tag: &str) -> Option<u64> {
    let i = s.find(tag)?;
    let digits: String = s[i + tag.len()..]
        .trim_start_matches(['.', '-', '_'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some(digits.parse().unwrap_or(0))
}

fn key(version: &str) -> Key {
    let v = version.trim().to_lowercase();
    let v = v.trim_start_matches('v');
    let v = v.split('+').next().unwrap_or(v);
    let (epoch, v) = match v.split_once('!') {
        Some((e, rest)) => (e.parse().unwrap_or(0), rest),
        None => (0, v),
    };
    let release_end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(v.len());
    let release: Vec<u64> = v[..release_end]
        .split('.')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let mut release = release;
    while release.len() > 1 && release.last() == Some(&0) {
        release.pop();
    }
    let rest = &v[release_end..];
    let rest_pre = rest.split("post").next().unwrap_or(rest);
    let rest_pre = rest_pre.split("dev").next().unwrap_or(rest_pre);
    let pre = if let Some(n) = number_after(rest_pre, "rc").or(number_after(rest_pre, "c")) {
        (3, n)
    } else if let Some(n) = number_after(rest_pre, "b").or(number_after(rest_pre, "beta")) {
        (2, n)
    } else if let Some(n) = number_after(rest_pre, "a").or(number_after(rest_pre, "alpha")) {
        (1, n)
    } else {
        (4, 0)
    };
    let post = number_after(rest, "post");
    let dev = number_after(rest, "dev");
    let pre = if dev.is_some() && pre.0 == 4 && post.is_none() {
        (0, 0)
    } else {
        pre
    };
    Key {
        epoch,
        release,
        pre,
        post,
        dev: dev.map_or((1, 0), |n| (0, n)),
    }
}

/// PEP 440 order of two versions.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    key(a).cmp(&key(b))
}

/// Whether `version` is a pre-release or development release.
pub fn is_prerelease(version: &str) -> bool {
    let k = key(version);
    k.pre.0 < 4 || k.dev.0 == 0
}

/// Parses the JSON API answer for one project (behaviour 3).
pub fn parse_package_info(json: &[u8]) -> Result<PackageInfo, String> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| format!("unexpected index answer: {e}"))?;
    let info = v.get("info").ok_or("unexpected index answer: no info")?;
    let text = |k: &str| {
        info.get(k)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
    };
    let mut releases: Vec<Release> = v
        .get("releases")
        .and_then(serde_json::Value::as_object)
        .map(|r| {
            r.iter()
                .filter(|(_, files)| {
                    // A release whose every file is yanked is left out (PEP 592); one without
                    // files is kept.
                    let files = files.as_array().map(Vec::as_slice).unwrap_or_default();
                    files.is_empty()
                        || files.iter().any(|f| {
                            !f.get("yanked")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false)
                        })
                })
                .map(|(version, _)| Release {
                    version: version.clone(),
                    prerelease: is_prerelease(version),
                })
                .collect()
        })
        .unwrap_or_default();
    releases.sort_by(|a, b| compare_versions(&b.version, &a.version));
    let home_page = text("home_page").or_else(|| {
        info.get("project_urls")
            .and_then(serde_json::Value::as_object)
            .and_then(|u| u.values().find_map(serde_json::Value::as_str))
            .map(str::to_owned)
    });
    Ok(PackageInfo {
        name: text("name").unwrap_or_default(),
        summary: text("summary").unwrap_or_default(),
        latest: text("version").unwrap_or_default(),
        requires_python: text("requires_python"),
        home_page,
        releases,
    })
}

/// Details of `name` from `index`; `Ok(None)` when the index does not know it (behaviour 3).
pub fn package_info(fetch: Fetch, index: &str, name: &str) -> Result<Option<PackageInfo>, String> {
    let norm = normalize_name(name.trim());
    if norm.is_empty()
        || !norm
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
    {
        return Ok(None);
    }
    let url = format!("{}/pypi/{norm}/json", index.trim_end_matches('/'));
    let got = fetch(&url, "application/json")?;
    match got.status {
        200 => parse_package_info(&body(&got)?).map(Some),
        404 => Ok(None),
        s => Err(format!("{url}: HTTP {s}")),
    }
}

/// The tool that installs into the project's environment (behaviour 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Installer {
    /// `uv pip install --python …`.
    Uv,
    /// `poetry add …`.
    Poetry,
    /// `python -m pip install …`.
    Pip,
}

/// The installer the project's files ask for, before checking `PATH`.
pub fn project_installer(root: &Path) -> Installer {
    let pyproject = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    let table: toml::Table = toml::from_str(&pyproject).unwrap_or_default();
    let tool = |name: &str| table.get("tool").and_then(|t| t.get(name)).is_some();
    if root.join("uv.lock").is_file() || tool("uv") {
        Installer::Uv
    } else if root.join("poetry.lock").is_file() || tool("poetry") {
        Installer::Poetry
    } else {
        Installer::Pip
    }
}

/// How a package would be installed: the tool, the command and notes (behaviour 4, 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallPlan {
    /// The tool used.
    pub installer: Installer,
    /// The command (no shell); empty when refused.
    pub command: Vec<String>,
    /// Why the command is what it is (fallbacks, Poetry's own environment).
    pub notes: Vec<String>,
    /// Why nothing may be installed, if so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// `name==version`, or the bare name without a version.
pub fn requirement(name: &str, version: Option<&str>) -> String {
    match version.filter(|v| !v.trim().is_empty()) {
        Some(v) => format!("{name}=={v}"),
        None => name.to_owned(),
    }
}

/// The plan for installing `requirement` from the index in `root` with `interpreter`; `on_path`
/// finds a program (behaviour 4, 5). A Poetry project adds with Poetry.
pub fn install_plan(
    root: &Path,
    interpreter: Option<&Interpreter>,
    requirement: &str,
    on_path: &dyn Fn(&str) -> Option<PathBuf>,
) -> InstallPlan {
    plan(root, interpreter, &[requirement.to_owned()], on_path, true)
}

/// The plan for installing `specs` into the selected environment (`SPEC-0117` behaviour 5: the
/// field, unmet declarations, the import quick fix, MCP `python_install_packages`): the same
/// choice between uv and pip as [`install_plan`], but never Poetry, so that installing what is
/// declared does not rewrite the declaration (`SPEC-0171` v2 behaviour 4).
pub fn environment_install_plan(
    root: &Path,
    interpreter: Option<&Interpreter>,
    specs: &[String],
    on_path: &dyn Fn(&str) -> Option<PathBuf>,
) -> InstallPlan {
    plan(root, interpreter, specs, on_path, false)
}

fn plan(
    root: &Path,
    interpreter: Option<&Interpreter>,
    specs: &[String],
    on_path: &dyn Fn(&str) -> Option<PathBuf>,
    poetry_allowed: bool,
) -> InstallPlan {
    let wanted = project_installer(root);
    let mut notes = Vec::new();
    if wanted == Installer::Poetry && poetry_allowed {
        if let Some(poetry) = on_path("poetry") {
            notes.push(
                "Poetry records the dependency in pyproject.toml and installs into the environment it manages for the project".into(),
            );
            let mut command = vec![poetry.to_string_lossy().into_owned(), "add".into()];
            command.extend(specs.iter().cloned());
            return InstallPlan {
                installer: Installer::Poetry,
                command,
                notes,
                refused: None,
            };
        }
        notes.push(
            "the project uses Poetry, but poetry is not on PATH; the environment's installer is used"
                .into(),
        );
    }
    let refuse = |installer: Installer, notes: Vec<String>, why: String| InstallPlan {
        installer,
        command: Vec::new(),
        notes,
        refused: Some(why),
    };
    let from_project = if wanted == Installer::Uv {
        Installer::Uv
    } else {
        Installer::Pip
    };
    let Some(python) = interpreter else {
        return refuse(
            from_project,
            notes,
            "no Python interpreter: create an environment first".into(),
        );
    };
    if matches!(
        python.kind,
        InterpreterKind::System | InterpreterKind::Pyenv
    ) {
        return refuse(
            from_project,
            notes,
            format!(
                "{} is a shared interpreter; create or choose a virtual environment first",
                python.path
            ),
        );
    }
    let path = Path::new(&python.path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    // The environment's own files decide next: made by uv, or without pip (read, not started).
    let tools = environment_tools(&path);
    let why_uv = if wanted == Installer::Uv {
        Some("the project uses uv")
    } else if tools.made_by_uv {
        Some("the environment was made by uv")
    } else if tools.lacks_pip {
        Some("the environment has no pip")
    } else {
        None
    };
    let uv = why_uv.and_then(|_| on_path("uv"));
    match (why_uv, uv) {
        (Some(why), Some(uv)) => {
            if wanted != Installer::Uv {
                notes.push(format!("{why}; uv is used"));
            }
            let mut command = vec![
                uv.to_string_lossy().into_owned(),
                "pip".into(),
                "install".into(),
                "--python".into(),
                path.to_string_lossy().into_owned(),
            ];
            command.extend(specs.iter().cloned());
            InstallPlan {
                installer: Installer::Uv,
                command,
                notes,
                refused: None,
            }
        }
        (Some(_), None) if tools.lacks_pip => refuse(
            Installer::Uv,
            notes,
            format!(
                "the environment of {python} has no pip and uv was not found on PATH: run `{python} -m ensurepip` in that environment, or install uv",
                python = path.display()
            ),
        ),
        (why, _) => {
            if let Some(why) = why {
                notes.push(format!("{why}, but uv is not on PATH; pip is used"));
            }
            InstallPlan {
                installer: Installer::Pip,
                command: pip_install_command(&path, specs),
                notes,
                refused: None,
            }
        }
    }
}

/// [`Fetch`] over HTTPS with IDECraft's HTTP client (60 s per request, redirects followed).
pub fn http_fetch(url: &str, accept: &str) -> Result<Fetched, String> {
    let request = idecraft_http::ResolvedRequest {
        name: None,
        line: 0,
        method: "GET".into(),
        url: url.to_owned(),
        headers: vec![
            ("Accept".into(), accept.to_owned()),
            ("Accept-Encoding".into(), "gzip".into()),
            ("User-Agent".into(), "IDECraft".into()),
        ],
        body: String::new(),
        unresolved: Vec::new(),
    };
    let response =
        idecraft_http::send(&request, Duration::from_secs(60)).map_err(|e| e.to_string())?;
    let gzip = response
        .header("content-encoding")
        .is_some_and(|e| e.eq_ignore_ascii_case("gzip"));
    Ok(Fetched {
        status: response.status,
        body: response.body,
        gzip,
    })
}

/// `program` on the process's `PATH`.
pub fn on_path(program: &str) -> Option<PathBuf> {
    on_path_in(&std::env::var_os("PATH")?, program)
}

/// `program` in the folders of `paths` (a `PATH` value).
pub fn on_path_in(paths: &std::ffi::OsStr, program: &str) -> Option<PathBuf> {
    std::env::split_paths(paths)
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// `program` in `paths` when given, else on the process's `PATH`: the lookup the Packages
/// window, the quick fix and the MCP tools share, with the folders replaceable in tests.
pub fn find_tool(paths: Option<&std::ffi::OsStr>, program: &str) -> Option<PathBuf> {
    match paths {
        Some(paths) => on_path_in(paths, program),
        None => on_path(program),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn temp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("idecraft-pyindex-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn names_from_the_simple_api_cached_and_matched() {
        let page = br#"{"meta":{"api-version":"1.4"},"projects":[{"name":"requests"},{"name":"Requests-OAuthlib"},{"name":"types-requests"},{"name":"rich"},{"name":"requests_toolbelt"}]}"#;
        let dir = temp("names");
        let file = dir.join("cache/pypi-names.txt");
        let calls = std::cell::Cell::new(0);
        let fetch = |url: &str, accept: &str| {
            calls.set(calls.get() + 1);
            assert_eq!(url, "https://pypi.org/simple/");
            assert_eq!(accept, "application/vnd.pypi.simple.v1+json");
            Ok(Fetched {
                status: 200,
                body: gzip(page),
                gzip: true,
            })
        };
        let list = names(&fetch, PYPI, &file).unwrap();
        assert_eq!(list.len(), 5);
        // Fresh cache: no second download; an old one is not used.
        assert_eq!(names(&fetch, PYPI, &file).unwrap(), list);
        assert_eq!(calls.get(), 1);
        assert_eq!(cached_names(&file, Duration::ZERO), None);
        assert_eq!(
            search_names(&list, "Requests", SEARCH_LIMIT),
            [
                "requests",
                "Requests-OAuthlib",
                "requests_toolbelt",
                "types-requests"
            ]
        );
        assert_eq!(search_names(&list, "requests", 2).len(), 2);
        assert_eq!(search_names(&list, "  ", 10), Vec::<String>::new());
        assert_eq!(
            search_names(&list, "requests.toolbelt", 10),
            ["requests_toolbelt"]
        );
        let failing = |_: &str, _: &str| {
            Ok(Fetched {
                status: 503,
                body: Vec::new(),
                gzip: false,
            })
        };
        assert!(download_names(&failing, PYPI, &file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_in_pep440_order() {
        let mut v = vec![
            "1.0",
            "1.0rc1",
            "1.0.post1",
            "1.0b2",
            "1.0a1",
            "1.0.dev1",
            "0.9",
            "1.10",
            "1.2",
            "2!0.1",
            "1.0b10",
        ];
        v.sort_by(|a, b| compare_versions(a, b));
        assert_eq!(
            v,
            [
                "0.9",
                "1.0.dev1",
                "1.0a1",
                "1.0b2",
                "1.0b10",
                "1.0rc1",
                "1.0",
                "1.0.post1",
                "1.2",
                "1.10",
                "2!0.1"
            ]
        );
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert!(is_prerelease("2.0rc1") && is_prerelease("2.0.dev3"));
        assert!(!is_prerelease("2.0") && !is_prerelease("2.0.post1"));
    }

    #[test]
    fn package_details_without_yanked_releases() {
        let json = br#"{"info":{"name":"demo","summary":"A demo","version":"2.0.0","requires_python":">=3.9","home_page":"","project_urls":{"Homepage":"https://example.org"}},
            "releases":{"1.0.0":[{"yanked":false}],"1.5.0":[{"yanked":true}],"2.0.0":[{"yanked":false},{"yanked":true}],"2.1.0b1":[{"yanked":false}],"0.1":[]}}"#;
        let info = parse_package_info(json).unwrap();
        assert_eq!(info.summary, "A demo");
        assert_eq!(info.latest, "2.0.0");
        assert_eq!(info.requires_python.as_deref(), Some(">=3.9"));
        assert_eq!(info.home_page.as_deref(), Some("https://example.org"));
        let versions: Vec<_> = info.releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["2.1.0b1", "2.0.0", "1.0.0", "0.1"]);
        assert!(info.releases[0].prerelease);
        let fetch = |url: &str, _: &str| {
            Ok(Fetched {
                status: if url.ends_with("/pypi/demo/json") {
                    200
                } else {
                    404
                },
                body: json.to_vec(),
                gzip: false,
            })
        };
        assert!(package_info(&fetch, PYPI, "Demo").unwrap().is_some());
        assert_eq!(package_info(&fetch, PYPI, "nothing-here").unwrap(), None);
        assert_eq!(package_info(&fetch, PYPI, "../x").unwrap(), None);
    }

    #[test]
    fn installer_and_commands() {
        let root = temp("installer");
        let venv = Interpreter {
            path: ".venv/bin/python".into(),
            kind: InterpreterKind::Venv,
            version: Some("3.12".into()),
            base: None,
            in_project: true,
            found_by: "project".into(),
        };
        let everything = |p: &str| Some(PathBuf::from(format!("/bin/{p}")));
        let nothing = |_: &str| None;
        let req = requirement("rich", Some("13.7.0"));
        assert_eq!(req, "rich==13.7.0");
        assert_eq!(requirement("rich", None), "rich");

        let pip = install_plan(&root, Some(&venv), &req, &everything);
        assert_eq!(pip.installer, Installer::Pip);
        let python = root.join(".venv/bin/python").display().to_string();
        assert_eq!(
            pip.command,
            [
                python.as_str(),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "rich==13.7.0"
            ]
        );

        std::fs::write(
            root.join("pyproject.toml"),
            "[project]\nname='x'\n[tool.uv]\n",
        )
        .unwrap();
        let uv = install_plan(&root, Some(&venv), &req, &everything);
        assert_eq!(
            uv.command,
            [
                "/bin/uv",
                "pip",
                "install",
                "--python",
                python.as_str(),
                "rich==13.7.0"
            ]
        );
        let fallback = install_plan(&root, Some(&venv), &req, &nothing);
        assert_eq!(fallback.installer, Installer::Pip);
        assert!(fallback.notes[0].contains("uv is not on PATH"));

        std::fs::write(root.join("pyproject.toml"), "[tool.poetry]\nname='x'\n").unwrap();
        let poetry = install_plan(&root, None, &req, &everything);
        assert_eq!(poetry.command, ["/bin/poetry", "add", "rich==13.7.0"]);
        assert!(poetry.refused.is_none());

        // Poetry missing: the environment's installer, with the reason.
        let no_poetry = install_plan(&root, Some(&venv), &req, &nothing);
        assert_eq!(no_poetry.installer, Installer::Pip);
        assert!(no_poetry.notes[0].contains("poetry is not on PATH"));
        // Installing into the environment never goes through Poetry.
        let direct = environment_install_plan(
            &root,
            Some(&venv),
            &["flask>=3".into(), "rich".into()],
            &everything,
        );
        assert_eq!(direct.installer, Installer::Pip);
        assert_eq!(direct.command[5..], ["flask>=3", "rich"]);

        std::fs::remove_file(root.join("pyproject.toml")).unwrap();
        let system = Interpreter {
            kind: InterpreterKind::System,
            path: "/usr/bin/python3".into(),
            ..venv.clone()
        };
        let refused = install_plan(&root, Some(&system), &req, &everything);
        assert!(refused.command.is_empty());
        assert!(refused.refused.unwrap().contains("shared interpreter"));
        assert!(
            install_plan(&root, None, &req, &everything)
                .refused
                .is_some()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A fixture environment `name` under `root`: `pyvenv.cfg` (with `cfg` appended), an empty
    /// `bin/python`, a site-packages folder, and the `pip` package when asked.
    fn fixture_env(root: &Path, name: &str, cfg: &str, pip: bool) -> Interpreter {
        let env = root.join(name);
        let site = env.join("lib/python3.12/site-packages");
        std::fs::create_dir_all(env.join("bin")).unwrap();
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(env.join("pyvenv.cfg"), format!("version = 3.12.4\n{cfg}")).unwrap();
        std::fs::write(env.join("bin/python"), "").unwrap();
        if pip {
            std::fs::create_dir_all(site.join("pip")).unwrap();
        }
        Interpreter {
            path: format!("{name}/bin/python"),
            kind: InterpreterKind::Venv,
            version: Some("3.12.4".into()),
            base: None,
            in_project: true,
            found_by: "project".into(),
        }
    }

    /// SPEC-0171 v2 behaviour 4: the environment's files choose the installer, and `uv` is looked
    /// up in the folders given (an injected `PATH`).
    #[test]
    fn the_environment_chooses_the_installer() {
        let root = temp("env-installer");
        let with_pip = fixture_env(&root, "with-pip", "", true);
        let without_pip = fixture_env(&root, "without-pip", "", false);
        let by_uv = fixture_env(&root, "by-uv", "uv = 0.5.1\n", true);
        assert_eq!(
            environment_tools(&root.join("with-pip/bin/python")),
            crate::python::EnvironmentTools::default()
        );
        assert!(environment_tools(&root.join("without-pip/bin/python")).lacks_pip);
        let tools = environment_tools(&root.join("by-uv/bin/python"));
        assert!(tools.made_by_uv && !tools.lacks_pip);
        // Without a site-packages folder nothing is known about pip.
        std::fs::create_dir_all(root.join("bare/bin")).unwrap();
        assert!(!environment_tools(&root.join("bare/bin/python")).lacks_pip);

        // An injected PATH: one folder with a `uv` file, one without.
        let bin = root.join("tools");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("uv"), "").unwrap();
        let empty = root.join("no-tools");
        std::fs::create_dir_all(&empty).unwrap();
        let path_with = std::env::join_paths([&empty, &bin]).unwrap();
        let path_without = std::env::join_paths([&empty]).unwrap();
        let found = |p: &str| find_tool(Some(&path_with), p);
        let missing = |p: &str| find_tool(Some(&path_without), p);
        assert_eq!(found("uv"), Some(bin.join("uv")));
        assert_eq!(missing("uv"), None);
        let specs = ["requests>=2.31".to_owned(), "rich".to_owned()];
        let uv = bin.join("uv").display().to_string();

        // pip in the environment, nothing said by uv: pip, whether uv is there or not.
        for lookup in [&found as &dyn Fn(&str) -> Option<PathBuf>, &missing] {
            let plan = environment_install_plan(&root, Some(&with_pip), &specs, lookup);
            assert_eq!(plan.installer, Installer::Pip);
            assert_eq!(
                plan.command[1..5],
                ["-m", "pip", "install", "--disable-pip-version-check"]
            );
            assert!(plan.notes.is_empty() && plan.refused.is_none());
        }

        // No pip: uv when found, with every requirement and the interpreter's absolute path.
        let python = root.join("without-pip/bin/python").display().to_string();
        let plan = environment_install_plan(&root, Some(&without_pip), &specs, &found);
        assert_eq!(plan.installer, Installer::Uv);
        assert_eq!(
            plan.command,
            [
                uv.as_str(),
                "pip",
                "install",
                "--python",
                python.as_str(),
                "requests>=2.31",
                "rich"
            ]
        );
        assert_eq!(plan.notes, ["the environment has no pip; uv is used"]);
        // The index install makes the same choice.
        let index = install_plan(&root, Some(&without_pip), "rich==13.7.0", &found);
        assert_eq!(index.installer, Installer::Uv);
        assert_eq!(index.command[5], "rich==13.7.0");

        // No pip and no uv: nothing would be started, and the message names both remedies.
        let plan = environment_install_plan(&root, Some(&without_pip), &specs, &missing);
        assert!(plan.command.is_empty());
        let why = plan.refused.unwrap();
        assert!(why.contains("has no pip and uv was not found"), "{why}");
        assert!(why.contains(&format!("`{python} -m ensurepip`")), "{why}");
        assert!(why.contains("install uv"), "{why}");

        // Made by uv (pip added later): uv when found, else pip with the reason.
        let plan = environment_install_plan(&root, Some(&by_uv), &specs, &found);
        assert_eq!(plan.installer, Installer::Uv);
        assert_eq!(plan.notes, ["the environment was made by uv; uv is used"]);
        let plan = environment_install_plan(&root, Some(&by_uv), &specs, &missing);
        assert_eq!(plan.installer, Installer::Pip);
        assert_eq!(
            plan.notes,
            ["the environment was made by uv, but uv is not on PATH; pip is used"]
        );

        // The shared-interpreter refusal stays, whatever is installed (IMP-0042).
        let shared = Interpreter {
            kind: InterpreterKind::Pyenv,
            path: "/home/u/.pyenv/versions/3.12.4/bin/python".into(),
            ..with_pip.clone()
        };
        let plan = environment_install_plan(&root, Some(&shared), &specs, &found);
        assert!(plan.refused.unwrap().contains("shared interpreter"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

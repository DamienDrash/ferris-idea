//! Project trust and safe mode (`SPEC-0278`): which folders the user trusts to run their code, why
//! a project would run code, and the check every action that runs project code makes.
//!
//! Trusted locations live in the user's settings folder, never in a project (IMP-0028). The check
//! is switched on by the program ([`enforce`]): the window and `ferris-idea mcp` enforce it, the
//! library used by tests does not, so tests see trusted projects unless they ask otherwise.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Name of the trust file inside the user's Ferris IDEA settings folder (behaviour 6).
pub const TRUST_FILE: &str = "trusted-projects.toml";
/// Environment variable that trusts projects without asking: `all`, or paths separated as in
/// `PATH` (behaviour 10).
pub const TRUST_ENV: &str = "FERRIS_IDEA_TRUST_PROJECTS";

static ENFORCED: AtomicBool = AtomicBool::new(false);
static PROCESS_ALL: AtomicBool = AtomicBool::new(false);
static PROCESS_PATHS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

/// Switches safe mode on for this process: untrusted projects run nothing (behaviour 11).
pub fn enforce() {
    ENFORCED.store(true, Ordering::SeqCst);
}

/// Whether safe mode is switched on for this process.
pub fn enforced() -> bool {
    ENFORCED.load(Ordering::SeqCst)
}

/// Trusts `path` and everything below it for this process, without remembering it (`--trust`,
/// the self-test).
pub fn trust_for_process(path: &Path) {
    let mut paths = PROCESS_PATHS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    paths.push(canonical(path));
}

/// Trusts every project for this process (the window self-test).
pub fn trust_all_for_process() {
    PROCESS_ALL.store(true, Ordering::SeqCst);
}

/// Ends [`trust_all_for_process`]: only the remembered and the process's own locations are
/// trusted again (the window self-test's steps of the safe-mode banner). Never widens trust.
pub fn stop_trusting_all_for_process() {
    PROCESS_ALL.store(false, Ordering::SeqCst);
}

/// Whether a location was trusted as a project or as a folder of projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustKind {
    /// This project (and anything below it).
    Project,
    /// Every project below this folder (*Trust Projects in …*).
    Folder,
}

/// One remembered trusted location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedLocation {
    /// Absolute path, links resolved.
    pub path: PathBuf,
    /// Project or folder.
    pub kind: TrustKind,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustFile {
    #[serde(default)]
    location: Vec<TrustedLocation>,
}

/// Why a path is trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "via", content = "detail")]
pub enum TrustSource {
    /// Safe mode is not switched on in this process (library, tests).
    NotEnforced,
    /// `FERRIS_IDEA_TRUST_PROJECTS`, with the value or path that matched.
    Environment(String),
    /// Trusted for this process only (`--trust`, self-test).
    Process(PathBuf),
    /// A remembered entry.
    Location(TrustedLocation),
}

impl TrustSource {
    /// Short description for people.
    pub fn describe(&self) -> String {
        match self {
            TrustSource::NotEnforced => "safe mode is off in this process".into(),
            TrustSource::Environment(v) => format!("{TRUST_ENV}={v}"),
            TrustSource::Process(p) => format!("trusted for this session: {}", p.display()),
            TrustSource::Location(l) => match l.kind {
                TrustKind::Project => format!("trusted project {}", l.path.display()),
                TrustKind::Folder => format!("trusted folder {}", l.path.display()),
            },
        }
    }
}

/// Whether a path is trusted, and through what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustStatus {
    /// The path checked (links resolved).
    pub path: PathBuf,
    /// Whether its code may run.
    pub trusted: bool,
    /// Why it is trusted; `None` when it is not.
    pub source: Option<TrustSource>,
    /// Problem reading the trust file, if any; nothing from the file counts then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Where trust is read from: the trust file, whether safe mode is on, and the process overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustSettings {
    /// The trust file (`None`: no settings folder known).
    pub file: Option<PathBuf>,
    /// Whether untrusted projects are in safe mode; when false everything is trusted.
    pub enforced: bool,
    /// Value of `FERRIS_IDEA_TRUST_PROJECTS`, if set.
    pub env: Option<String>,
    /// Everything is trusted for the process.
    pub process_all: bool,
    /// Paths trusted for the process.
    pub process_paths: Vec<PathBuf>,
}

/// The trust file in the user's settings folder.
pub fn trust_file() -> Option<PathBuf> {
    crate::user_config_dir().map(|d| d.join(TRUST_FILE))
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

impl TrustSettings {
    /// The settings of this process: the user's trust file, [`enforce`], the environment and the
    /// process overrides.
    pub fn process() -> Self {
        Self {
            file: trust_file(),
            enforced: enforced(),
            env: crate::product_var(TRUST_ENV)
                .ok()
                .filter(|v| !v.trim().is_empty()),
            process_all: PROCESS_ALL.load(Ordering::SeqCst),
            process_paths: PROCESS_PATHS
                .get()
                .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()).clone())
                .unwrap_or_default(),
        }
    }

    /// Settings that enforce safe mode with only `file` (tests).
    pub fn with_file(file: &Path) -> Self {
        Self {
            file: Some(file.to_path_buf()),
            enforced: true,
            ..Self::default()
        }
    }

    /// Whether `path` is trusted (behaviour 6, 10).
    pub fn status(&self, path: &Path) -> TrustStatus {
        let path = canonical(path);
        let mut status = TrustStatus {
            path: path.clone(),
            trusted: true,
            source: None,
            problem: None,
        };
        let below = |p: &Path| path.starts_with(canonical(p));
        status.source = if !self.enforced {
            Some(TrustSource::NotEnforced)
        } else if let Some(env) = &self.env {
            if env.trim().eq_ignore_ascii_case("all") || env.trim() == "1" {
                Some(TrustSource::Environment(env.trim().to_owned()))
            } else {
                std::env::split_paths(env)
                    .find(|p| p.is_absolute() && below(p))
                    .map(|p| TrustSource::Environment(p.display().to_string()))
            }
        } else {
            None
        };
        if status.source.is_none() && self.process_all {
            status.source = Some(TrustSource::Process(PathBuf::from("/")));
        }
        if status.source.is_none() {
            status.source = self
                .process_paths
                .iter()
                .find(|p| below(p))
                .map(|p| TrustSource::Process(p.clone()));
        }
        if status.source.is_none()
            && let Some(file) = &self.file
        {
            if canonical(file).starts_with(&path) {
                status.problem =
                    Some("the trust file lies inside the project and is ignored".into());
            } else {
                match load_locations(file) {
                    Ok(list) => {
                        // The most specific entry names the reason.
                        status.source = list
                            .into_iter()
                            .filter(|l| below(&l.path))
                            .max_by_key(|l| l.path.components().count())
                            .map(TrustSource::Location);
                    }
                    Err(e) => status.problem = Some(e),
                }
            }
        }
        status.trusted = status.source.is_some();
        status
    }

    /// The remembered entries (empty without a file).
    pub fn locations(&self) -> Result<Vec<TrustedLocation>, String> {
        self.file.as_deref().map_or(Ok(Vec::new()), load_locations)
    }
}

/// The entries of the trust file `file`; a missing file has none.
pub fn load_locations(file: &Path) -> Result<Vec<TrustedLocation>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => toml::from_str::<TrustFile>(&text)
            .map(|f| f.location)
            .map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

fn save_locations(file: &Path, list: &[TrustedLocation]) -> Result<(), String> {
    let text = toml::to_string(&TrustFile {
        location: list.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    let text = format!(
        "# Projects and folders whose code Ferris IDEA may run (SPEC-0278). Remove an entry to put its\n# projects back into safe mode.\n{text}"
    );
    crate::write_atomic(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// Remembers `path` as trusted in `file` (behaviour 6); returns the entry. An entry for the same
/// path is replaced.
pub fn add_location(file: &Path, path: &Path, kind: TrustKind) -> Result<TrustedLocation, String> {
    let path = canonical(path);
    if !path.is_absolute() {
        return Err(format!("not an absolute path: {}", path.display()));
    }
    let mut list = load_locations(file)?;
    list.retain(|l| l.path != path);
    let entry = TrustedLocation { path, kind };
    list.push(entry.clone());
    list.sort_by(|a, b| a.path.cmp(&b.path));
    save_locations(file, &list)?;
    Ok(entry)
}

/// Removes the entry for `path` from `file`; returns whether there was one (behaviour 8, 14).
pub fn remove_location(file: &Path, path: &Path) -> Result<bool, String> {
    let path = canonical(path);
    let mut list = load_locations(file)?;
    let before = list.len();
    list.retain(|l| l.path != path && canonical(&l.path) != path);
    let removed = list.len() != before;
    if removed {
        save_locations(file, &list)?;
    }
    Ok(removed)
}

/// Whether the code of the project at (or around) `path` may run in this process (behaviour 5).
pub fn allows_project_code(path: &Path) -> bool {
    !enforced() || TrustSettings::process().status(path).trusted
}

/// The message for an action refused in safe mode (behaviour 5).
pub fn refusal(root: &Path) -> String {
    format!(
        "Safe mode: {} is not trusted, so Ferris IDEA runs none of its code (build scripts, build-tool sync, run configurations, tests, tasks). Trust it with \"Trust Project…\" in the banner, `ferris-idea trust add <folder>`, or the MCP tool trust_project.",
        root.display()
    )
}

/// `Err` with [`refusal`] when `root`'s code may not run.
pub fn require_trusted(root: &Path) -> Result<(), String> {
    if allows_project_code(root) {
        Ok(())
    } else {
        Err(refusal(root))
    }
}

/// Something in a project that would run its code (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The file or folder, relative to the project.
    pub file: String,
    /// What would run.
    pub runs: &'static str,
}

/// Files and folders that make a project run code, with what each runs (behaviour 3).
const MARKERS: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "Cargo build scripts and procedural macros of the package and its dependencies",
    ),
    ("build.rs", "a Cargo build script"),
    (
        "build.gradle",
        "Gradle build logic when the project is synced",
    ),
    (
        "build.gradle.kts",
        "Gradle build logic when the project is synced",
    ),
    ("settings.gradle", "Gradle settings logic"),
    ("settings.gradle.kts", "Gradle settings logic"),
    (
        "gradlew",
        "the Gradle wrapper, which downloads and runs Gradle",
    ),
    ("pom.xml", "Maven plugins when dependencies are resolved"),
    ("mvnw", "the Maven wrapper, which downloads and runs Maven"),
    ("build.sbt", "sbt build logic"),
    (
        "Package.swift",
        "the Swift package manifest, compiled and run",
    ),
    ("Gemfile", "Ruby code run by Bundler"),
    (".Rprofile", "R code run when R starts in the project"),
    ("package.json", "npm scripts"),
    (
        "node_modules",
        "ESLint, Prettier and TypeScript of the project",
    ),
    (".venv", "the project's Python interpreter"),
    ("venv", "the project's Python interpreter"),
    ("setup.py", "Python packaging code"),
    (crate::run::RUN_FILE, "run configurations"),
    (crate::check_gates::GATES_FILE, "check gates"),
    (".run", "run configurations"),
    (".idea/runConfigurations", "run configurations"),
    (".luarc.json", "Lua language server plugins"),
];

/// What in `root` would run code (behaviour 3), in a fixed order.
pub fn findings(root: &Path) -> Vec<Finding> {
    MARKERS
        .iter()
        .filter(|(f, _)| root.join(f).exists())
        .map(|(file, runs)| Finding {
            file: (*file).to_owned(),
            runs,
        })
        .collect()
}

/// The question and its explanation for opening `root` (behaviour 1–3).
pub fn question(root: &Path) -> (String, String) {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    let found = findings(root);
    let mut detail = if found.is_empty() {
        "No build files or run configurations were found. Trusting the project lets Ferris IDEA run its configurations, tests and tasks.".to_owned()
    } else {
        let mut d = "Trusting it lets Ferris IDEA run code from the project:\n".to_owned();
        for f in &found {
            d.push_str(&format!("• {} — {}\n", f.file, f.runs));
        }
        d
    };
    detail.push_str(
        "\nIn safe mode you can read and edit the files; nothing of the project runs until you trust it.",
    );
    (format!("Trust and open project \"{name}\"?"), detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-trust-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        canonical(&dir)
    }

    #[test]
    fn projects_folders_and_removal() {
        let dir = scratch("store");
        let file = dir.join("config").join(TRUST_FILE);
        let (a, b, parent) = (dir.join("p/a"), dir.join("p/b"), dir.join("p"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d.join("src")).unwrap();
        }
        let s = TrustSettings::with_file(&file);
        assert!(!s.status(&a).trusted);
        assert_eq!(s.status(&a).problem, None, "a missing file is no problem");
        add_location(&file, &a, TrustKind::Project).unwrap();
        assert!(s.status(&a).trusted);
        assert!(s.status(&a.join("src")).trusted, "below a project");
        assert!(!s.status(&b).trusted, "a sibling is not trusted");
        add_location(&file, &parent, TrustKind::Folder).unwrap();
        assert!(s.status(&b).trusted, "below a trusted folder");
        // The most specific entry is named.
        assert_eq!(
            s.status(&a).source,
            Some(TrustSource::Location(TrustedLocation {
                path: a.clone(),
                kind: TrustKind::Project
            }))
        );
        assert_eq!(s.locations().unwrap().len(), 2);
        // Adding again replaces.
        add_location(&file, &a, TrustKind::Project).unwrap();
        assert_eq!(s.locations().unwrap().len(), 2);
        assert!(remove_location(&file, &parent).unwrap());
        assert!(!remove_location(&file, &parent).unwrap());
        assert!(!s.status(&b).trusted);
        assert!(s.status(&a).trusted);
        assert!(remove_location(&file, &a).unwrap());
        assert!(!s.status(&a).trusted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_resolved_and_a_file_in_the_project_is_ignored() {
        let dir = scratch("links");
        let real = dir.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let file = dir.join("trust.toml");
        add_location(&file, &link, TrustKind::Project).unwrap();
        let s = TrustSettings::with_file(&file);
        assert!(s.status(&real).trusted, "the entry holds the resolved path");
        assert!(
            s.status(&link.join("..").join("real")).trusted,
            "`..` resolved"
        );
        // A trust file inside the project cannot trust it (IMP-0028).
        let inside = real.join(".ferris-idea").join(TRUST_FILE);
        add_location(&inside, &real, TrustKind::Project).unwrap();
        let st = TrustSettings::with_file(&inside).status(&real);
        assert!(!st.trusted);
        assert!(st.problem.unwrap().contains("inside the project"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overrides_and_enforcement() {
        let dir = scratch("override");
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let off = TrustSettings::default();
        assert_eq!(off.status(&a).source, Some(TrustSource::NotEnforced));
        let all = TrustSettings {
            enforced: true,
            env: Some("all".into()),
            ..TrustSettings::default()
        };
        assert!(all.status(&a).trusted);
        let list = std::env::join_paths([dir.join("nowhere"), a.clone()])
            .unwrap()
            .into_string()
            .unwrap();
        let paths = TrustSettings {
            enforced: true,
            env: Some(list),
            ..TrustSettings::default()
        };
        assert!(paths.status(&a.join("x")).trusted);
        assert!(!paths.status(&b).trusted);
        let process = TrustSettings {
            enforced: true,
            process_paths: vec![b.clone()],
            ..TrustSettings::default()
        };
        assert!(process.status(&b).trusted);
        assert!(!process.status(&a).trusted);
        assert_eq!(
            process.status(&b).source,
            Some(TrustSource::Process(b.clone()))
        );
        let everything = TrustSettings {
            enforced: true,
            process_all: true,
            ..TrustSettings::default()
        };
        assert!(everything.status(&a).trusted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn findings_and_question() {
        let dir = scratch("findings");
        assert!(findings(&dir).is_empty());
        let (q, detail) = question(&dir);
        assert!(q.contains("Trust and open project"));
        assert!(detail.contains("No build files"), "{detail}");
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(dir.join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(dir.join(".ferris-idea")).unwrap();
        std::fs::write(dir.join(crate::run::RUN_FILE), "").unwrap();
        std::fs::write(dir.join(crate::check_gates::GATES_FILE), "").unwrap();
        let f: Vec<String> = findings(&dir).into_iter().map(|f| f.file).collect();
        assert_eq!(
            f,
            [
                "Cargo.toml",
                "build.rs",
                ".ferris-idea/run.toml",
                ".ferris-idea/check-gates.toml"
            ]
        );
        let (_, detail) = question(&dir);
        assert!(
            detail.contains("build.rs — a Cargo build script"),
            "{detail}"
        );
        assert!(detail.contains("safe mode"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

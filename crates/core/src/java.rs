//! Java development kits and the project's Java requirement (`SPEC-0052`), read statically.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One installed JDK or JRE (behaviour 1–2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Jdk {
    /// Java home folder.
    pub home: String,
    /// Full version (`JAVA_VERSION` of the `release` file).
    pub version: Option<String>,
    /// Feature release (8, 11, 17, 21, …).
    pub feature: Option<u32>,
    /// Vendor (`IMPLEMENTOR`).
    pub vendor: Option<String>,
    /// Whether a compiler (`javac`) is present.
    pub has_compiler: bool,
    /// CPU architecture (`OS_ARCH`).
    pub arch: Option<String>,
    /// How it was found: `configured`, `JAVA_HOME`, `system`, `user`, `sdkman`, `gradle`, `PATH`.
    pub found_by: String,
}

/// Where the project states its Java version (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JavaRequirement {
    /// Required feature release.
    pub feature: u32,
    /// File that states it.
    pub source: String,
    /// What it says, e.g. `maven.compiler.release 17`.
    pub detail: String,
}

/// Inputs of the search; [`JavaSearch::from_env`] fills them from the process environment.
#[derive(Debug, Clone, Default)]
pub struct JavaSearch {
    /// Workspace root.
    pub root: PathBuf,
    /// `PATH`.
    pub path_var: Option<OsString>,
    /// `JAVA_HOME`.
    pub java_home: Option<PathBuf>,
    /// Folders whose sub-folders are JDKs, with the label used as `found_by`.
    pub jdk_dirs: Vec<(PathBuf, String)>,
    /// JDK chosen in the settings.
    pub configured: Option<PathBuf>,
}

impl JavaSearch {
    /// Search inputs from the environment of this process and the usual install locations.
    pub fn from_env(root: &Path) -> Self {
        let mut jdk_dirs = vec![
            (
                PathBuf::from("/Library/Java/JavaVirtualMachines"),
                "system".to_owned(),
            ),
            (PathBuf::from("/usr/lib/jvm"), "system".to_owned()),
        ];
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            jdk_dirs.push((home.join("Library/Java/JavaVirtualMachines"), "user".into()));
            jdk_dirs.push((home.join(".jdks"), "user".into()));
            jdk_dirs.push((home.join(".sdkman/candidates/java"), "sdkman".into()));
            jdk_dirs.push((home.join(".gradle/jdks"), "gradle".into()));
        }
        Self {
            root: root.to_path_buf(),
            path_var: std::env::var_os("PATH"),
            java_home: std::env::var_os("JAVA_HOME").map(PathBuf::from),
            jdk_dirs,
            configured: None,
        }
    }
}

/// Result of [`java_environment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JavaEnvironment {
    /// JDKs found, without duplicates.
    pub jdks: Vec<Jdk>,
    /// Index of the selected JDK.
    pub selected: Option<usize>,
    /// Why it was selected.
    pub reason: Option<String>,
    /// Requirements stated by the project, strictest first.
    pub requirements: Vec<JavaRequirement>,
    /// Whether the selected JDK can build the project (`None`: unknown).
    pub compatible: Option<bool>,
    /// Problems: contradicting requirements, no suitable JDK.
    pub problems: Vec<String>,
    /// Remarks.
    pub notes: Vec<String>,
}

/// Feature release of a Java version string (`1.8.0_392` → 8, `17.0.2` → 17, `21` → 21).
pub fn java_feature(version: &str) -> Option<u32> {
    let v = version.trim().trim_matches('"');
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn read_release(home: &Path) -> Option<Vec<(String, String)>> {
    let text = std::fs::read_to_string(home.join("release")).ok()?;
    Some(
        text.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().trim_matches('"').to_owned()))
            .collect(),
    )
}

/// The Java home of a folder: itself, or `Contents/Home` of a macOS bundle.
fn java_home_of(dir: &Path) -> Option<PathBuf> {
    [dir.to_path_buf(), dir.join("Contents/Home")]
        .into_iter()
        .find(|h| h.join("bin/java").is_file() || h.join("bin/java.exe").is_file())
}

fn jdk(home: PathBuf, found_by: &str) -> Jdk {
    let release = read_release(&home).unwrap_or_default();
    let get = |k: &str| {
        release
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    let version = get("JAVA_VERSION");
    Jdk {
        home: home.to_string_lossy().into_owned(),
        feature: version.as_deref().and_then(java_feature),
        version,
        vendor: get("IMPLEMENTOR"),
        has_compiler: home.join("bin/javac").is_file() || home.join("bin/javac.exe").is_file(),
        arch: get("OS_ARCH"),
        found_by: found_by.to_owned(),
    }
}

/// Finds JDKs (behaviour 1–2); duplicates (same resolved home) are dropped.
pub fn find_jdks(search: &JavaSearch) -> Vec<Jdk> {
    let mut out = Vec::new();
    let abs = |p: &PathBuf| {
        if p.is_absolute() {
            p.clone()
        } else {
            search.root.join(p)
        }
    };
    if let Some(c) = &search.configured
        && let Some(home) = java_home_of(&abs(c))
    {
        out.push(jdk(home, "configured"));
    }
    if let Some(h) = &search.java_home
        && let Some(home) = java_home_of(h)
    {
        out.push(jdk(home, "JAVA_HOME"));
    }
    for (dir, label) in &search.jdk_dirs {
        let mut homes: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .filter(|e| e.file_name() != "current")
                    .filter_map(|e| java_home_of(&e.path()))
                    .collect()
            })
            .unwrap_or_default();
        homes.sort();
        out.extend(homes.into_iter().map(|h| jdk(h, label)));
    }
    if let Some(path) = &search.path_var {
        for dir in std::env::split_paths(path) {
            let java = dir.join("java");
            if !java.is_file() {
                continue;
            }
            // The home is two levels above the resolved executable; launcher stubs without a
            // `release` file (e.g. macOS `/usr/bin/java`) are skipped.
            let real = std::fs::canonicalize(&java).unwrap_or(java);
            if let Some(home) = real.parent().and_then(Path::parent)
                && home.join("release").is_file()
            {
                out.push(jdk(home.to_path_buf(), "PATH"));
            }
        }
    }
    let mut seen = Vec::new();
    out.retain(|j| {
        let id = std::fs::canonicalize(&j.home).unwrap_or_else(|_| PathBuf::from(&j.home));
        if seen.contains(&id) {
            false
        } else {
            seen.push(id);
            true
        }
    });
    out
}

fn tag(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{name}>"))? + start;
    Some(text[start..end].trim().to_owned())
}

/// Requirements stated in the project's files (behaviour 3).
pub fn java_requirements(root: &Path) -> Vec<JavaRequirement> {
    let mut out = Vec::new();
    let mut push = |text: Option<String>, source: &str, what: &str| {
        if let Some(t) = text
            && let Some(f) = java_feature(&t)
        {
            out.push(JavaRequirement {
                feature: f,
                source: source.to_owned(),
                detail: format!("{what} {t}"),
            });
        }
    };
    if let Ok(pom) = std::fs::read_to_string(root.join("pom.xml")) {
        for key in [
            "maven.compiler.release",
            "maven.compiler.source",
            "maven.compiler.target",
            "java.version",
        ] {
            push(tag(&pom, key), "pom.xml", key);
        }
        // Compiler plugin configuration.
        if let Some(i) = pom.find("maven-compiler-plugin") {
            let rest = &pom[i..];
            let end = rest.find("</plugin>").unwrap_or(rest.len());
            let block = &rest[..end];
            for key in ["release", "source"] {
                push(
                    tag(block, key),
                    "pom.xml",
                    &format!("maven-compiler-plugin {key}"),
                );
            }
        }
    }
    for file in ["build.gradle.kts", "build.gradle"] {
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            if let Some(i) = text.find("JavaLanguageVersion.of(") {
                let rest = &text[i + "JavaLanguageVersion.of(".len()..];
                push(
                    rest.split(')').next().map(|s| s.trim().to_owned()),
                    file,
                    "toolchain languageVersion",
                );
            }
            for key in ["sourceCompatibility", "targetCompatibility"] {
                if let Some(i) = text.find(key) {
                    let rest = text[i + key.len()..].trim_start();
                    let rest = rest.trim_start_matches('=').trim_start();
                    let value: String = rest
                        .trim_start_matches("JavaVersion.VERSION_")
                        .trim_start_matches(['\'', '"'])
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_')
                        .collect();
                    push(Some(value.replace('_', ".")), file, key);
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(root.join(".java-version")) {
        push(
            text.lines().next().map(str::to_owned),
            ".java-version",
            "version",
        );
    }
    if let Ok(text) = std::fs::read_to_string(root.join(".sdkmanrc")) {
        let v = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("java="))
            .map(str::to_owned);
        push(v, ".sdkmanrc", "java");
    }
    if let Ok(text) = std::fs::read_to_string(root.join(".idea/misc.xml"))
        && let Some(i) = text.find("languageLevel=\"")
    {
        let rest = &text[i + "languageLevel=\"".len()..];
        let level = rest.split('"').next().unwrap_or_default();
        // `JDK_17`, `JDK_1_8`, `JDK_21_PREVIEW`.
        let v = level
            .trim_start_matches("JDK_")
            .trim_end_matches("_PREVIEW")
            .replace('_', ".");
        push(Some(v), ".idea/misc.xml", "languageLevel");
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.feature));
    out
}

/// JDKs, requirement, selection and compatibility (behaviour 1–5).
pub fn java_environment(search: &JavaSearch) -> JavaEnvironment {
    let jdks = find_jdks(search);
    let requirements = java_requirements(&search.root);
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    let mut features: Vec<u32> = requirements.iter().map(|r| r.feature).collect();
    features.dedup();
    if features.len() > 1 {
        problems.push(format!(
            "the project states different Java versions: {}",
            requirements
                .iter()
                .map(|r| format!("{} ({})", r.detail, r.source))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let required = requirements.first().map(|r| r.feature);
    let usable =
        |j: &Jdk| j.has_compiler && j.feature.is_some_and(|f| required.is_none_or(|r| f >= r));
    let (selected, reason) = if let Some(i) = jdks.iter().position(|j| j.found_by == "configured") {
        (Some(i), Some("chosen in the settings".to_owned()))
    } else if let Some(r) = required {
        // Same feature release first, else the closest newer one.
        let exact = jdks
            .iter()
            .position(|j| j.has_compiler && j.feature == Some(r));
        let newer = jdks
            .iter()
            .enumerate()
            .filter(|(_, j)| usable(j))
            .min_by_key(|(_, j)| j.feature)
            .map(|(i, _)| i);
        match exact.or(newer) {
            Some(i) => (
                Some(i),
                Some(format!(
                    "{} the project's Java {r}",
                    if exact.is_some() {
                        "matches"
                    } else {
                        "is the closest newer JDK for"
                    }
                )),
            ),
            None => {
                problems.push(format!("no installed JDK can build Java {r}"));
                fallback(&jdks)
            }
        }
    } else {
        fallback(&jdks)
    };
    let chosen = selected.map(|i| &jdks[i]);
    let compatible = match (chosen.and_then(|j| j.feature), required) {
        (Some(f), Some(r)) => Some(f >= r && chosen.is_some_and(|j| j.has_compiler)),
        _ => None,
    };
    if let Some(j) = chosen {
        if !j.has_compiler {
            notes.push(format!("{} is a runtime without javac", j.home));
        }
        if j.version.is_none() {
            notes.push(format!(
                "{} has no release file; its version is unknown",
                j.home
            ));
        }
        let host = std::env::consts::ARCH;
        if let Some(a) = &j.arch {
            let a = if a == "amd64" { "x86_64" } else { a.as_str() };
            if a != host {
                notes.push(format!(
                    "{} is built for {a}, this machine is {host} (runs emulated or not at all)",
                    j.home
                ));
            }
        }
    }
    JavaEnvironment {
        jdks,
        selected,
        reason,
        requirements,
        compatible,
        problems,
        notes,
    }
}

fn fallback(jdks: &[Jdk]) -> (Option<usize>, Option<String>) {
    if let Some(i) = jdks.iter().position(|j| j.found_by == "JAVA_HOME") {
        return (Some(i), Some("JAVA_HOME".into()));
    }
    if let Some(i) = jdks.iter().position(|j| j.found_by == "PATH") {
        return (Some(i), Some("java on PATH".into()));
    }
    jdks.iter()
        .enumerate()
        .filter(|(_, j)| j.has_compiler)
        .max_by_key(|(_, j)| j.feature)
        .map_or((None, None), |(i, _)| {
            (Some(i), Some("newest installed JDK".into()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features() {
        assert_eq!(java_feature("1.8.0_392"), Some(8));
        assert_eq!(java_feature("17.0.20.1"), Some(17));
        assert_eq!(java_feature("21"), Some(21));
        assert_eq!(java_feature("21.0.2-tem"), Some(21));
        assert_eq!(java_feature("1.8"), Some(8));
        assert_eq!(java_feature("abc"), None);
    }
}

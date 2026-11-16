//! Holds a Kotlin program before its main class until kotlin-debug-adapter is ready for it
//! (`SPEC-0132` v6 behaviour 4c).
//!
//! The adapter starts the program as soon as it is connected, then registers the breakpoints and
//! only then passes on what the program reports. A program that is at its first breakpoint or at
//! its end within that time runs past the breakpoint, or its stop or its end is never reported.
//! IDECraft therefore starts the program with a small Java agent of its own
//! (`kotlin_gate/IdecraftGate.java`): it waits, where the JVM is about to define the main class,
//! until a file is gone, and IDECraft removes the file when the adapter has answered `launch`.
//! The agent is built once per user with the JDK's `javac` and `jar` and kept in the user's cache
//! folder; without them the program starts as before.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

/// The agent's source.
const SOURCE: &str = include_str!("kotlin_gate/IdecraftGate.java");
const CLASS: &str = "IdecraftGate";

/// The file the program waits for; removed when dropped.
#[derive(Debug)]
pub(crate) struct Flag(PathBuf);

impl Drop for Flag {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A folder only the user can enter.
fn private_dir(dir: &Path) -> Option<()> {
    std::fs::create_dir_all(dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    Some(())
}

/// A tool of the JDK: in `JAVA_HOME/bin`, else on the PATH.
fn jdk_tool(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(name))
        .filter(|tool| tool.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn run(tool: &str, args: &[&std::ffi::OsStr], dir: &Path) -> Option<()> {
    Command::new(jdk_tool(tool))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .filter(|status| status.success())
        .map(|_| ())
}

/// The agent's jar below `cache`, built when it is not there yet. The folder is named after the
/// source, so another source never meets an old jar.
fn jar_in(cache: &Path) -> Option<PathBuf> {
    let digest: String = Sha256::digest(SOURCE.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    let home = cache.join("kotlin-gate");
    private_dir(&home)?;
    let dir = home.join(&digest);
    let jar = dir.join("gate.jar");
    if jar.is_file() {
        return Some(jar);
    }
    // Built aside and moved into place, so a half-written jar is never used.
    let build = home.join(format!("{digest}.build-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&build);
    private_dir(&build)?;
    let built = (|| {
        std::fs::write(build.join(format!("{CLASS}.java")), SOURCE).ok()?;
        std::fs::write(
            build.join("manifest.txt"),
            format!("Premain-Class: {CLASS}\n"),
        )
        .ok()?;
        let arg = std::ffi::OsStr::new;
        // For an old class file version, so that the program's JVM may be older than this
        // `javac`; a JDK that no longer writes version 8 writes 11.
        let compile = |release: &str| {
            run(
                "javac",
                &[
                    arg("--release"),
                    arg(release),
                    arg("-d"),
                    arg("classes"),
                    arg("IdecraftGate.java"),
                ],
                &build,
            )
        };
        compile("8").or_else(|| compile("11"))?;
        run(
            "jar",
            &[
                arg("cfm"),
                arg("gate.jar"),
                arg("manifest.txt"),
                arg("-C"),
                arg("classes"),
                arg("."),
            ],
            &build,
        )?;
        // Another session may have been faster: its folder is as good.
        if std::fs::rename(&build, &dir).is_err() && !jar.is_file() {
            return None;
        }
        Some(())
    })();
    let _ = std::fs::remove_dir_all(&build);
    built.map(|()| jar)
}

/// The JVM argument that makes a program with the main class `main_class` wait before that class,
/// and the file it waits for. `None` when the agent cannot be built (no JDK tools, no cache
/// folder) or a name cannot be passed on.
pub(crate) fn prepare(main_class: &str) -> Option<(String, Flag)> {
    prepare_in(&idecraft_core::user_cache_dir()?, main_class)
}

fn prepare_in(cache: &Path, main_class: &str) -> Option<(String, Flag)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // A class of a named module (`module/Class`) is launched another way.
    let plain = !main_class.is_empty()
        && main_class
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '$'));
    if !plain {
        return None;
    }
    let jar = jar_in(cache)?;
    let flags = cache.join("kotlin-gate").join("flags");
    private_dir(&flags)?;
    let flag = flags.join(format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let argument = format!(
        "-javaagent:{}={main_class}={}",
        jar.display(),
        flag.display()
    );
    // The adapter's launcher splits its options at spaces outside double quotes.
    if argument.contains(['"', '\\']) {
        return None;
    }
    std::fs::write(&flag, b"").ok()?;
    Some((format!("\"{argument}\""), Flag(flag)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_is_built_once_and_the_flag_goes_with_its_guard() {
        let cache = std::env::temp_dir().join(format!("idecraft-ktgate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        // Names that cannot be passed on need no JDK to be refused.
        assert!(prepare_in(&cache, "app/zoo.MainKt").is_none());
        assert!(prepare_in(&cache, "").is_none());
        let Some((argument, flag)) = prepare_in(&cache, "zoo.MainKt") else {
            eprintln!("skipped: javac or jar is not available");
            let _ = std::fs::remove_dir_all(&cache);
            return;
        };
        let path = flag.0.clone();
        assert!(path.is_file());
        assert!(argument.starts_with("\"-javaagent:"), "{argument}");
        assert!(
            argument.ends_with(&format!("gate.jar=zoo.MainKt={}\"", path.display())),
            "{argument}"
        );
        let jar = jar_in(&cache).unwrap();
        let built = std::fs::metadata(&jar).unwrap().modified().unwrap();
        // A second session: the same jar, another flag.
        let (_, second) = prepare_in(&cache, "zoo.MainKt").unwrap();
        assert_ne!(second.0, path);
        assert_eq!(std::fs::metadata(&jar).unwrap().modified().unwrap(), built);
        drop(flag);
        assert!(!path.exists());
        assert!(second.0.is_file());
        drop(second);
        let _ = std::fs::remove_dir_all(&cache);
    }
}

//! Where `idecraft launch` starts, and when a binary counts as started from its app bundle
//! (`SPEC-0282` behaviours 6–7).

use std::path::{Path, PathBuf};

/// Whether `exe` is the executable of an app bundle (`….app/Contents/MacOS/<name>`).
pub fn in_app_bundle(exe: &Path) -> bool {
    let Some(macos) = exe.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    macos.file_name().is_some_and(|n| n == "MacOS")
        && contents.file_name().is_some_and(|n| n == "Contents")
        && contents
            .parent()
            .and_then(Path::extension)
            .is_some_and(|e| e == "app")
}

/// Whether this process was started from an app bundle.
pub fn started_from_bundle() -> bool {
    std::env::current_exe().is_ok_and(|exe| in_app_bundle(&exe))
}

/// Drops the process serial number argument macOS may add when it starts an application.
pub fn without_launch_arguments(args: Vec<String>) -> Vec<String> {
    args.into_iter()
        .filter(|a| !a.starts_with("-psn_"))
        .collect()
}

/// Whether the command line opens the IDE window (`SPEC-0328` behaviour 2): `launch`, `ui` without
/// its self-test, and `open` with an exchange file, which starts a window when none runs.
pub fn opens_window(args: &[String]) -> bool {
    match args {
        [cmd, ..] if cmd == "launch" => true,
        [cmd, rest @ ..] if cmd == "ui" => !rest.iter().any(|a| a == "--selftest"),
        [cmd, path] if cmd == "open" => idecraft_core::format::is_exchange_file(Path::new(path)),
        _ => false,
    }
}

/// The folder to open: the given one (for a file its project, `SPEC-0094` v7 behaviour 18), else,
/// when `reopen` (System Settings › Reopen the last project at start-up, `SPEC-0337` behaviour 2),
/// the most recent project that is still a folder; `None` means asking the user.
pub fn start_folder(given: Option<&Path>, recent: &[PathBuf], reopen: bool) -> Option<PathBuf> {
    given
        .map(|p| match idecraft_core::project_root::open_target(p) {
            Ok(target) => target.root,
            Err(_) => p.to_path_buf(),
        })
        .or_else(|| {
            recent
                .iter()
                .filter(|_| reopen)
                .find(|p| p.is_dir())
                .cloned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_executables_are_recognised() {
        assert!(in_app_bundle(Path::new(
            "/Applications/IDECraft.app/Contents/MacOS/idecraft"
        )));
        assert!(in_app_bundle(Path::new(
            "target/bundle/macos/IDECraft.app/Contents/MacOS/idecraft"
        )));
        assert!(!in_app_bundle(Path::new("/usr/local/bin/idecraft")));
        assert!(!in_app_bundle(Path::new("target/release/idecraft")));
        assert!(!in_app_bundle(Path::new("/x/Contents/MacOS/idecraft")));
        assert!(!in_app_bundle(Path::new("idecraft")));
    }

    #[test]
    fn window_commands_are_told_from_the_others() {
        let args = |line: &str| -> Vec<String> { line.split(' ').map(str::to_owned).collect() };
        for line in ["launch", "launch /p", "ui", "ui /p", "open a.idcrun"] {
            assert!(opens_window(&args(line)), "{line}");
        }
        for line in [
            "ui --selftest",
            "ui --selftest --list-groups",
            "open /p",
            "open --print a.idcrun",
            "call /p python_environments",
            "mcp /p",
            "serve --stdio",
            "--version",
        ] {
            assert!(!opens_window(&args(line)), "{line}");
        }
        assert!(!opens_window(&[]));
    }

    #[test]
    fn launch_arguments_from_macos_are_dropped() {
        let args = vec!["-psn_0_12345".to_owned(), "ui".to_owned()];
        assert_eq!(without_launch_arguments(args), ["ui"]);
    }

    #[test]
    fn the_given_folder_wins_then_the_most_recent_existing_project() {
        let dir = std::env::temp_dir().join(format!("idecraft-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gone = dir.join("gone");
        let given = Path::new("/somewhere");
        assert_eq!(
            start_folder(Some(given), std::slice::from_ref(&dir), true),
            Some(given.to_path_buf())
        );
        assert_eq!(
            start_folder(None, &[gone.clone(), dir.clone()], true),
            Some(dir.clone())
        );
        std::fs::write(dir.join("a.txt"), "").unwrap();
        assert_eq!(
            start_folder(Some(&dir.join("a.txt")), &[], true),
            Some(dir.canonicalize().unwrap())
        );
        assert_eq!(start_folder(None, &[gone], true), None);
        assert_eq!(start_folder(None, &[], true), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// SPEC-0094 v7 AK-16: `idecraft launch <file>` starts in the file's project, here the Git
    /// repository above the file's folder, never in the file.
    #[test]
    fn a_file_starts_in_its_project() {
        let dir = std::env::temp_dir()
            .join(format!("idecraft-launch-file-{}", std::process::id()))
            .join("repo");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("queries")).unwrap();
        let file = dir.join("queries/file.sql");
        std::fs::write(&file, "select 1;\n").unwrap();
        assert_eq!(
            start_folder(Some(&file), &[], true),
            Some(dir.canonicalize().unwrap())
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    /// SPEC-0337 AK-3: with "Reopen the last project at start-up" off a start without a folder
    /// asks, and a given folder still opens.
    #[test]
    fn start_folder_follows_reopen_last_project() {
        let dir =
            std::env::temp_dir().join(format!("idecraft-launch-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let recent = std::slice::from_ref(&dir);
        assert_eq!(start_folder(None, recent, true), Some(dir.clone()));
        assert_eq!(
            start_folder(None, recent, false),
            None,
            "the folder chooser"
        );
        assert_eq!(
            start_folder(Some(&dir), &[], false),
            Some(dir.canonicalize().unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

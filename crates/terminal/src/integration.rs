//! Shell integration (`SPEC-0206` behaviour 5): start-up files that load the user's own and then
//! add hooks writing OSC 133 prompt marks and OSC 7, without touching the user's files.

use std::path::{Path, PathBuf};

const ZSHENV: &str = include_str!("shell/zshenv");
const ZPROFILE: &str = include_str!("shell/zprofile");
const ZSHRC: &str = include_str!("shell/zshrc");
const BASHRC: &str = include_str!("shell/bashrc");

/// Arguments and environment that turn on the integration for a shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Integration {
    /// Arguments after the program.
    pub args: Vec<String>,
    /// Variables to set.
    pub env: Vec<(String, String)>,
}

/// The integration for `program` (zsh or bash, by file name), with its files written under
/// `dir`; `None` for other programs, when the files cannot be written, or when
/// `IDECRAFT_SHELL_INTEGRATION=0`.
pub fn prepare(program: &str, dir: &Path) -> Option<Integration> {
    if std::env::var("IDECRAFT_SHELL_INTEGRATION").is_ok_and(|v| v == "0") {
        return None;
    }
    let name = Path::new(program).file_name()?.to_str()?;
    match name.trim_start_matches('-') {
        "zsh" => {
            let zdot = dir.join("zsh");
            write_all(
                &zdot,
                &[
                    (".zshenv", ZSHENV),
                    (".zprofile", ZPROFILE),
                    (".zshrc", ZSHRC),
                ],
            )?;
            let user = std::env::var("ZDOTDIR")
                .ok()
                .filter(|d| !d.is_empty())
                .or_else(|| std::env::var("HOME").ok())
                .unwrap_or_default();
            Some(Integration {
                args: Vec::new(),
                env: vec![
                    ("ZDOTDIR".into(), zdot.to_string_lossy().into_owned()),
                    ("IDECRAFT_USER_ZDOTDIR".into(), user),
                ],
            })
        }
        "bash" => {
            let bash = dir.join("bash");
            write_all(&bash, &[("bashrc", BASHRC)])?;
            Some(Integration {
                args: vec![
                    "--rcfile".into(),
                    bash.join("bashrc").to_string_lossy().into_owned(),
                ],
                env: Vec::new(),
            })
        }
        _ => None,
    }
}

/// Default folder for the generated files: per user under the system's temporary folder.
pub fn default_dir() -> PathBuf {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    std::env::temp_dir().join(format!("idecraft-shell-integration-{user}"))
}

fn write_all(dir: &Path, files: &[(&str, &str)]) -> Option<()> {
    std::fs::create_dir_all(dir).ok()?;
    for (name, text) in files {
        let path = dir.join(name);
        // Rewritten only when different, so running shells never read a half-written file.
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            let tmp = dir.join(format!("{name}.{}", std::process::id()));
            std::fs::write(&tmp, text).ok()?;
            std::fs::rename(&tmp, &path).ok()?;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_and_bash_only() {
        let dir = std::env::temp_dir().join(format!("idecraft-integration-{}", std::process::id()));
        let zsh = prepare("/bin/zsh", &dir).unwrap();
        assert!(zsh.args.is_empty());
        assert_eq!(zsh.env[0].1, dir.join("zsh").to_string_lossy());
        assert!(dir.join("zsh/.zshrc").is_file());
        let bash = prepare("/usr/local/bin/bash", &dir).unwrap();
        assert_eq!(bash.args[0], "--rcfile");
        assert!(Path::new(&bash.args[1]).is_file());
        assert!(prepare("fish", &dir).is_none());
        assert!(prepare("/usr/bin/ssh", &dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

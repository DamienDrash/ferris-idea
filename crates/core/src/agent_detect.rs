//! ACP programs installed on this computer and adding one to the agent host's configuration
//! (`SPEC-0357` behaviours 3 and 12, OBS-0121 gap A11).
//!
//! Only programs already installed are offered, found by name in the folders of `PATH` and a few
//! usual install folders. Adding writes one `[[agent]]` table into `agent-host.toml` in the user's
//! settings folder and happens only on the person's click in the window: no tool call can add a
//! program (`SPEC-0348` behaviour 11).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agent_host::{AgentDef, HostConfig};

/// A known ACP program: its file name, its id, its name and the argument that starts ACP.
pub const KNOWN: [(&str, &str, &str, &[&str]); 4] = [
    ("gemini", "gemini", "Gemini CLI", &["--acp"]),
    ("agy", "antigravity", "Antigravity", &["acp"]),
    (
        "claude-code-acp",
        "claude",
        "Claude Code (ACP adapter)",
        &[],
    ),
    ("codex-acp", "codex", "Codex (ACP adapter)", &[]),
];

/// An installed ACP program (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    /// The id it gets when added.
    pub id: String,
    /// Its name for people.
    pub name: String,
    /// The program, an absolute path.
    pub command: PathBuf,
    /// The arguments that start ACP.
    pub args: Vec<String>,
    /// `agent-host.toml` has an agent with this program already.
    pub configured: bool,
}

fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The folders searched: those of `path` (the value of `PATH`), then `~/.local/bin`,
/// `/opt/homebrew/bin` and `/usr/local/bin`, each once.
pub fn search_folders(path: Option<&str>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let extra = [
        home.map(|h| h.join(".local/bin")),
        Some(PathBuf::from("/opt/homebrew/bin")),
        Some(PathBuf::from("/usr/local/bin")),
    ];
    for d in path
        .unwrap_or("")
        .split(':')
        .filter(|s| s.starts_with('/'))
        .map(PathBuf::from)
        .chain(extra.into_iter().flatten())
    {
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// The known ACP programs found in `folders`, the first match per program, marked when `config`
/// has an agent with the same program already.
pub fn detect(folders: &[PathBuf], config: &HostConfig) -> Vec<Found> {
    KNOWN
        .iter()
        .filter_map(|(file, id, name, args)| {
            let command = folders
                .iter()
                .map(|d| d.join(file))
                .find(|p| executable(p))?;
            let canon = std::fs::canonicalize(&command).unwrap_or_else(|_| command.clone());
            let configured = config.agent.iter().any(|a| {
                a.command == command || std::fs::canonicalize(&a.command).is_ok_and(|c| c == canon)
            });
            Some(Found {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                command,
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                configured,
            })
        })
        .collect()
}

/// The programs found with this process's `PATH` and home folder.
pub fn detect_here(config: &HostConfig) -> Vec<Found> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = std::env::var("PATH").ok();
    detect(&search_folders(path.as_deref(), home.as_deref()), config)
}

/// Adds `found` as one `[[agent]]` table to the configuration file `file` (behaviour 3): keeps
/// everything else in the file, gives a taken id a number, creates the file with mode 0600 and
/// checks the result like the host does. Answers the id it got.
pub fn add(file: &Path, found: &Found) -> Result<String, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let current: HostConfig = toml::from_str(&text).map_err(|e| {
        format!(
            "{} cannot be read, so nothing was added: {e}",
            file.display()
        )
    })?;
    let mut id = found.id.clone();
    let mut n = 2;
    while current.agent.iter().any(|a| a.id == id) {
        id = format!("{}-{n}", found.id);
        n += 1;
    }
    let def = AgentDef {
        id: id.clone(),
        name: Some(found.name.clone()),
        command: found.command.clone(),
        args: found.args.clone(),
        env: Vec::new(),
    };
    let mut checked = current.clone();
    checked.agent.push(def.clone());
    checked.check()?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let mut table = toml_edit::Table::new();
    table["id"] = toml_edit::value(def.id.clone());
    table["name"] = toml_edit::value(found.name.clone());
    table["command"] = toml_edit::value(found.command.to_string_lossy().into_owned());
    let mut args = toml_edit::Array::new();
    for a in &found.args {
        args.push(a.as_str());
    }
    table["args"] = toml_edit::value(args);
    match doc.get_mut("agent") {
        Some(toml_edit::Item::ArrayOfTables(a)) => a.push(table),
        Some(_) => {
            return Err(format!(
                "{}: `agent` is not a list of tables",
                file.display()
            ));
        }
        None => {
            let mut a = toml_edit::ArrayOfTables::new();
            a.push(table);
            doc["agent"] = toml_edit::Item::ArrayOfTables(a);
        }
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_private(file, doc.to_string().as_bytes())
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(id)
}

/// Writes `bytes` to `file` through a new file of mode 0600 renamed over it.
fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = file.with_extension(format!("tmp-{}", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ferris-idea-detect-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn program(d: &Path, name: &str, mode: u32) {
        let p = d.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn installed_programs_are_found_and_added_once_per_click() {
        let bin = dir("bin");
        program(&bin, "gemini", 0o755);
        program(&bin, "agy", 0o755);
        program(&bin, "codex-acp", 0o644);
        let folders = vec![bin.clone()];
        let found = detect(&folders, &HostConfig::default());
        let names: Vec<(&str, &str, Vec<String>)> = found
            .iter()
            .map(|f| (f.id.as_str(), f.name.as_str(), f.args.clone()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("gemini", "Gemini CLI", vec!["--acp".to_owned()]),
                ("antigravity", "Antigravity", vec!["acp".to_owned()]),
            ]
        );
        let settings = dir("settings");
        let file = settings.join(crate::agent_host::HOST_FILE);
        std::fs::write(&file, "# mine\nmax_agents = 3\n").unwrap();
        assert_eq!(add(&file, &found[0]).unwrap(), "gemini");
        assert_eq!(add(&file, &found[0]).unwrap(), "gemini-2");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# mine\nmax_agents = 3\n"), "{text}");
        let loaded: HostConfig = toml::from_str(&text).unwrap();
        assert_eq!(loaded.agent.len(), 2);
        assert_eq!(loaded.agent[0].command, bin.join("gemini"));
        assert_eq!(loaded.agent[0].args, vec!["--acp"]);
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let again = detect(&folders, &loaded);
        assert!(again[0].configured && !again[1].configured);
        std::fs::write(&file, "nonsense = [").unwrap();
        assert!(
            add(&file, &found[1])
                .unwrap_err()
                .contains("nothing was added")
        );
    }

    #[test]
    fn folders_come_from_path_then_the_usual_places_once() {
        let f = search_folders(
            Some("/usr/bin:relative:/usr/bin:/opt/homebrew/bin"),
            Some(Path::new("/h")),
        );
        assert_eq!(
            f,
            vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/h/.local/bin"),
                PathBuf::from("/usr/local/bin"),
            ]
        );
    }
}

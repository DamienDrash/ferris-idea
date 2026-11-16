//! pyright and basedpyright with the workspace's selected interpreter (`SPEC-0170`).
//!
//! The servers ask for the `python` section of their configuration; the answer carries
//! `pythonPath`, so imports resolve against the environment the project runs with. A new choice is
//! pushed to running servers with `workspace/didChangeConfiguration`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use idecraft_core::{PythonSearch, selected_interpreter};

/// Programs of the Python language servers that read `python.pythonPath`.
pub const PYTHON_SERVERS: [&str; 2] = ["basedpyright-langserver", "pyright-langserver"];

/// Whether `program` is pyright or basedpyright.
pub fn is_python_server(program: &str) -> bool {
    PYTHON_SERVERS.contains(&program)
}

/// The selected interpreter of `root` as an absolute path, and why it was selected.
pub fn interpreter(root: &Path) -> Option<(PathBuf, String)> {
    let (i, reason) = selected_interpreter(&PythonSearch::for_workspace(root))?;
    let path = PathBuf::from(&i.path);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    Some((path, reason))
}

/// Settings for `python_path` (behaviour 2): `{"python": {"pythonPath": …}}`, an empty `python`
/// section without one.
pub fn settings_for(python_path: Option<&Path>) -> Value {
    match python_path {
        Some(p) => json!({"python": {"pythonPath": p.to_string_lossy()}}),
        None => json!({"python": {}}),
    }
}

/// Settings for the workspace `root` with its selected interpreter.
pub fn settings(root: &Path) -> Value {
    settings_for(interpreter(root).map(|(p, _)| p).as_deref())
}

/// The project file that names an environment for pyright (`venv` in `pyrightconfig.json` or in
/// `[tool.pyright]` of `pyproject.toml`), with the environment's name (behaviour 5).
pub fn project_environment(root: &Path) -> Option<(String, String)> {
    if let Ok(text) = std::fs::read_to_string(root.join("pyrightconfig.json"))
        && let Some(venv) = json_venv(&text)
    {
        return Some(("pyrightconfig.json".into(), venv));
    }
    let text = std::fs::read_to_string(root.join("pyproject.toml")).ok()?;
    let doc: toml::Table = toml::from_str(&text).ok()?;
    let venv = doc
        .get("tool")?
        .get("pyright")?
        .get("venv")?
        .as_str()?
        .to_owned();
    Some(("pyproject.toml [tool.pyright]".into(), venv))
}

/// `venv` of a `pyrightconfig.json` (JSON with comments and trailing commas tolerated as far as
/// a line-based strip allows).
fn json_venv(text: &str) -> Option<String> {
    let stripped: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let value: Value = serde_json::from_str(&stripped).ok().or_else(|| {
        // Trailing commas: drop them before `}` and `]`.
        let re = strip_trailing_commas(&stripped);
        serde_json::from_str(&re).ok()
    })?;
    value.get("venv")?.as_str().map(str::to_owned)
}

fn strip_trailing_commas(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if *ch == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(*ch);
    }
    out
}

/// Pushes the current settings of `root` to its running pyright/basedpyright servers (behaviour
/// 4); returns how many servers got them.
pub fn interpreter_changed(root: &Path) -> usize {
    let settings = settings(root);
    PYTHON_SERVERS
        .iter()
        .map(|program| crate::pool::change_settings(root, program, settings.clone()))
        .sum()
}

/// What the MCP tool `python_language_server` reports (behaviour 6).
#[derive(Debug, Clone, Serialize)]
pub struct PythonServerReport {
    /// The server program, or `None`.
    pub server: Option<String>,
    /// Its full path.
    pub command: Option<String>,
    /// Why no server is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// The interpreter passed to it.
    pub interpreter: Option<String>,
    /// How that interpreter was selected.
    pub reason: Option<String>,
    /// The settings answered to `workspace/configuration`.
    pub settings: Value,
    /// A project file that names an environment and decides instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_environment: Option<String>,
    /// Number of running servers that got the settings (`apply`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<usize>,
}

/// The report for `root`; with `apply` the settings are pushed to running servers.
pub fn report(root: &Path, apply: bool) -> PythonServerReport {
    let resolved = crate::servers::resolve(crate::LanguageId::Python);
    let chosen = interpreter(root);
    let settings = settings_for(chosen.as_ref().map(|(p, _)| p.as_path()));
    PythonServerReport {
        server: resolved
            .as_ref()
            .ok()
            .map(|r| r.candidate.program.to_owned()),
        command: resolved
            .as_ref()
            .ok()
            .map(|r| r.command.display().to_string()),
        problem: resolved.as_ref().err().map(ToString::to_string),
        interpreter: chosen.as_ref().map(|(p, _)| p.display().to_string()),
        reason: chosen.map(|(_, r)| r),
        settings,
        project_environment: project_environment(root).map(|(file, venv)| {
            format!("{file} names the environment `{venv}`; pyright follows the project file")
        }),
        applied: apply.then(|| interpreter_changed(root)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::configuration_section;

    #[test]
    fn settings_and_sections() {
        let s = settings_for(Some(Path::new("/p/.venv/bin/python")));
        assert_eq!(
            configuration_section(&s, Some("python")),
            json!({"pythonPath": "/p/.venv/bin/python"})
        );
        assert_eq!(
            configuration_section(&s, Some("python.pythonPath")),
            json!("/p/.venv/bin/python")
        );
        assert_eq!(
            configuration_section(&s, Some("python.analysis")),
            Value::Null
        );
        assert_eq!(configuration_section(&s, None), s);
        assert_eq!(configuration_section(&s, Some("")), s);
        assert_eq!(settings_for(None), json!({"python": {}}));
        assert!(is_python_server("pyright-langserver"));
        assert!(is_python_server("basedpyright-langserver"));
        assert!(!is_python_server("clangd"));
    }

    #[test]
    fn project_files_that_name_an_environment() {
        let root = std::env::temp_dir().join(format!("idecraft-pyserver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(project_environment(&root), None);
        std::fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"x\"\n\n[tool.pyright]\nvenvPath = \".\"\nvenv = \"env\"\n",
        )
        .unwrap();
        assert_eq!(
            project_environment(&root),
            Some(("pyproject.toml [tool.pyright]".into(), "env".into()))
        );
        std::fs::write(
            root.join("pyrightconfig.json"),
            "{\n  // comment\n  \"venvPath\": \".\",\n  \"venv\": \".venv\",\n}\n",
        )
        .unwrap();
        assert_eq!(
            project_environment(&root),
            Some(("pyrightconfig.json".into(), ".venv".into()))
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

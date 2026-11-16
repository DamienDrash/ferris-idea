//! Formatting and import clean-up of Python files with tools that are already installed
//! (`SPEC-0329`): `ruff` or `black` from the selected environment or from `PATH`, run on the
//! editor's text through standard input in the project root, never downloaded or installed.
//! Without `ruff`, unused imports are removed from what the language server reports.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tree_sitter::Node;

use crate::LanguageId;
use crate::format::{FormatPlan, whitespace_only};
use crate::insight::{Opened, Range};
use crate::js_tools::{minimal_edit, run_command};
use crate::lsp::LspError;
use crate::rename::{Edit, FileEdits};
use crate::syntax::SyntaxDocument;

/// A tool IDECraft can run on Python text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    /// `ruff` (`ruff format`, `ruff check --fix-only`).
    Ruff,
    /// `black`.
    Black,
}

impl ToolKind {
    /// The program's name.
    pub fn program(self) -> &'static str {
        match self {
            Self::Ruff => "ruff",
            Self::Black => "black",
        }
    }
}

/// A tool found for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PythonTool {
    /// Which tool.
    pub kind: ToolKind,
    /// Its executable.
    pub bin: PathBuf,
    /// The folder it runs in: the project root, so the project's configuration applies.
    pub dir: PathBuf,
    /// Whether it comes from the selected environment (else from `PATH`).
    pub from_environment: bool,
}

/// Where tools are looked for (behaviour 1).
#[derive(Debug, Clone, Default)]
pub struct ToolSearch {
    /// Project root.
    pub root: PathBuf,
    /// The folder of the selected interpreter (`.venv/bin`).
    pub environment: Option<PathBuf>,
    /// `PATH`.
    pub path_var: Option<OsString>,
    /// Whether the project is trusted; in safe mode only tools outside the project and outside
    /// its environment are used (behaviour 2).
    pub trusted: bool,
}

impl ToolSearch {
    /// The search of the workspace `root`: its selected interpreter's folder (`SPEC-0050`
    /// behaviour 3), the process's `PATH` and the project's trust.
    pub fn for_workspace(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            environment: idecraft_core::workspace_python(root, None)
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            path_var: std::env::var_os("PATH"),
            trusted: idecraft_core::trust::allows_project_code(root),
        }
    }
}

fn executable(dir: &Path, program: &str) -> Option<PathBuf> {
    let bin = dir.join(format!("{program}{}", std::env::consts::EXE_SUFFIX));
    bin.is_file().then_some(bin)
}

/// Whether `path` lies in `root` (links resolved where the paths exist).
fn inside(root: &Path, path: &Path) -> bool {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    real(path).starts_with(real(root)) || path.starts_with(root)
}

fn in_environment(search: &ToolSearch, kind: ToolKind) -> Option<PathBuf> {
    executable(search.environment.as_deref()?, kind.program())
}

fn on_path(search: &ToolSearch, kind: ToolKind) -> Option<PathBuf> {
    std::env::split_paths(search.path_var.as_ref()?)
        // A relative entry would be looked up in the project root; an entry inside the project
        // is project content.
        .filter(|dir| dir.is_absolute() && (search.trusted || !inside(&search.root, dir)))
        .find_map(|dir| executable(&dir, kind.program()))
}

/// The first of `kinds` that is installed: for each kind the selected environment's (only in a
/// trusted project), then the one on `PATH` (behaviours 1–2).
pub fn find(search: &ToolSearch, kinds: &[ToolKind]) -> Option<PythonTool> {
    kinds.iter().find_map(|&kind| {
        let environment = search
            .trusted
            .then(|| in_environment(search, kind))
            .flatten();
        let from_environment = environment.is_some();
        let bin = environment.or_else(|| on_path(search, kind))?;
        Some(PythonTool {
            kind,
            bin,
            dir: search.root.clone(),
            from_environment,
        })
    })
}

/// The formatter of the project: `ruff`, else `black`.
pub fn find_formatter(search: &ToolSearch) -> Option<PythonTool> {
    find(search, &[ToolKind::Ruff, ToolKind::Black])
}

/// `ruff`, for the import clean-up.
pub fn find_ruff(search: &ToolSearch) -> Option<PythonTool> {
    find(search, &[ToolKind::Ruff])
}

/// Whether the import clean-up runs without `ruff` only because the project is not trusted: the
/// selected environment has one, which safe mode keeps from running, and none is usable on
/// `PATH` (`SPEC-0329` v2 behaviour 6).
pub fn ruff_waits_for_trust(search: &ToolSearch) -> bool {
    !search.trusted
        && in_environment(search, ToolKind::Ruff).is_some()
        && find_ruff(search).is_none()
}

/// What to say when none of `kinds` was found (behaviour 3): what is looked for, that nothing is
/// installed by IDECraft, and where to install it; in safe mode, that the environment's tool
/// waits for trust.
pub fn missing(search: &ToolSearch, kinds: &[ToolKind]) -> String {
    let names: Vec<&str> = kinds.iter().map(|k| k.program()).collect();
    let names = names.join(" or ");
    let waiting = !search.trusted && kinds.iter().any(|&k| in_environment(search, k).is_some());
    if waiting {
        return format!(
            "Safe mode: {names} of the project's environment is not run until the project is trusted, and none is on PATH. Trust the project, or install {names} outside it."
        );
    }
    let place = match &search.environment {
        Some(dir) => format!("the selected environment ({})", dir.display()),
        None => "the selected environment".to_owned(),
    };
    format!(
        "No {names} found in {place} or on PATH. IDECraft runs a tool that is already installed and installs nothing itself: install {} into the project's virtual environment (Python Packages tool window; MCP: python_install_packages) or on PATH, then try again.",
        kinds.first().map_or("ruff", |k| k.program())
    )
}

fn relative(tool: &PythonTool, file: &Path) -> String {
    file.strip_prefix(&tool.dir)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn first_error(tool: &PythonTool, err: &str) -> String {
    let line = err
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("failed");
    let line = line.strip_prefix("error: ").unwrap_or(line);
    format!("{}: {line}", tool.kind.program())
}

/// The arguments that format `name` from standard input, the whole text or `range` of it.
pub fn format_arguments(kind: ToolKind, name: &str, range: Option<&Range>) -> Vec<String> {
    let mut args: Vec<String> = match kind {
        ToolKind::Ruff => vec!["format".into(), "--stdin-filename".into(), name.into()],
        ToolKind::Black => vec!["-q".into(), "--stdin-filename".into(), name.into()],
    };
    if let Some(r) = range {
        match kind {
            // 1-based lines and code points, the end exclusive.
            ToolKind::Ruff => args.push(format!(
                "--range={}:{}-{}:{}",
                r.start_line + 1,
                r.start_character + 1,
                r.end_line + 1,
                r.end_character + 1
            )),
            // 1-based lines, both inclusive; a range that ends at the start of a line does not
            // take that line.
            ToolKind::Black => {
                let last = if r.end_character == 0 && r.end_line > r.start_line {
                    r.end_line
                } else {
                    r.end_line + 1
                };
                args.push(format!("--line-ranges={}-{last}", r.start_line + 1));
            }
        }
    }
    args.push("-".into());
    args
}

/// The arguments that remove unused imports and sort the imports of `name` from standard input.
pub fn import_arguments(name: &str) -> Vec<String> {
    [
        "check",
        "--select",
        "F401,I",
        "--fix-only",
        "--no-cache",
        "--stdin-filename",
        name,
        "-",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn run(tool: &PythonTool, args: &[String], text: &str) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (out, err, code) = run_command(&tool.bin, &tool.dir, &args, text)?;
    if code != Some(0) {
        return Err(first_error(tool, &err));
    }
    Ok(out)
}

/// `text` (the content of `file`, saved or not) formatted by `tool`, the whole text or `range`.
pub fn format(
    tool: &PythonTool,
    file: &Path,
    text: &str,
    range: Option<&Range>,
) -> Result<String, String> {
    let name = relative(tool, file);
    run(tool, &format_arguments(tool.kind, &name, range), text)
}

/// 0-based line and character (Unicode scalar values) of byte `at` of `text`.
fn position(text: &str, at: usize) -> (u64, u64) {
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (
        before.matches('\n').count() as u64,
        before[line_start..].chars().count() as u64,
    )
}

/// The change from `old` to `new` of `file` as a plan (`SPEC-0043` behaviour 3): one edit over
/// the part that differs, none when the texts are equal.
pub fn plan(file: &Path, old: &str, new: &str) -> FormatPlan {
    let edits = minimal_edit(old, new)
        .map(|e| {
            let (start_line, start_character) = position(old, e.range.start);
            let (end_line, end_character) = position(old, e.range.end);
            Edit {
                range: Range {
                    start_line,
                    start_character,
                    end_line,
                    end_character,
                },
                old_text: old[e.range].to_owned(),
                new_text: e.text,
            }
        })
        .into_iter()
        .collect();
    FormatPlan {
        edits: FileEdits {
            path: file.to_path_buf(),
            edits,
        },
        formatted: new.to_owned(),
        whitespace_only: whitespace_only(old, new),
    }
}

/// What an import clean-up made of a text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportCleanup {
    /// The text afterwards.
    pub text: String,
    /// The names of the removed imports, in document order.
    pub removed: Vec<String>,
    /// Unused imports that were left, with the reason.
    pub left: Vec<String>,
    /// `ruff` or `language server`.
    pub by: &'static str,
    /// Whether the imports were also sorted.
    pub sorted: bool,
}

/// One line that says what a clean-up did (`changed`: whether the text differs), for the status
/// bar. `ruff_waits`: [`ruff_waits_for_trust`] of the search the clean-up ran with; the hint
/// after a clean-up that did not sort then names trust instead of an installation.
pub fn summary(done: &ImportCleanup, changed: bool, ruff_waits: bool) -> String {
    let count = |n: usize| {
        if n == 1 {
            "1 unused import".to_owned()
        } else {
            format!("{n} unused imports")
        }
    };
    let mut line = match (done.removed.len(), changed, done.sorted) {
        (0, false, _) => "Imports are already optimized".to_owned(),
        (0, true, _) => format!("Imports sorted with {}", done.by),
        (n, _, true) => format!(
            "Removed {} and sorted the imports with {}",
            count(n),
            done.by
        ),
        (n, _, false) => format!("Removed {}", count(n)),
    };
    if !done.left.is_empty() {
        line.push_str(&format!("; left: {}", done.left.join(", ")));
    }
    if !done.sorted {
        line.push_str(if ruff_waits {
            " (trust the project to also sort the imports with its ruff)"
        } else {
            " (install ruff to also sort the imports)"
        });
    }
    line
}

fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

/// The import statements below `n`, with whether each is a statement of the module itself.
fn import_statements<'t>(n: Node<'t>, top: bool, out: &mut Vec<(Node<'t>, bool)>) {
    for c in children(n) {
        match c.kind() {
            "import_statement" | "import_from_statement" => out.push((c, top)),
            _ => import_statements(c, false, out),
        }
    }
}

/// The imported names of a statement: each with the node that carries the bound name (the alias
/// of `a as b`).
fn imported<'t>(statement: Node<'t>) -> Vec<(Node<'t>, Node<'t>)> {
    let mut cursor = statement.walk();
    statement
        .children_by_field_name("name", &mut cursor)
        .map(|name| {
            let bound = if name.kind() == "aliased_import" {
                name.child_by_field_name("alias").unwrap_or(name)
            } else {
                name
            };
            (name, bound)
        })
        .collect()
}

/// The names the import statements of `text` bind, as written (`os`, `a.b`, the alias of `a as b`).
pub fn import_names(text: &str) -> Vec<String> {
    let Ok(doc) = SyntaxDocument::new(LanguageId::Python, false, text) else {
        return Vec::new();
    };
    let mut statements = Vec::new();
    import_statements(doc.tree().root_node(), true, &mut statements);
    statements
        .into_iter()
        .flat_map(|(s, _)| imported(s))
        .map(|(_, bound)| word(bound, text).to_owned())
        .collect()
}

/// The names of `before` that `after` no longer imports, in the order of `before`.
fn names_removed(before: &str, after: &str) -> Vec<String> {
    let mut kept = import_names(after);
    import_names(before)
        .into_iter()
        .filter(|name| match kept.iter().position(|k| k == name) {
            Some(i) => {
                kept.swap_remove(i);
                false
            }
            None => true,
        })
        .collect()
}

/// `text` (the content of `file`) with unused imports removed and the imports sorted by `ruff`
/// with the project's configuration (behaviour 5).
pub fn ruff_imports(tool: &PythonTool, file: &Path, text: &str) -> Result<ImportCleanup, String> {
    let name = relative(tool, file);
    let new = run(tool, &import_arguments(&name), text)?;
    Ok(ImportCleanup {
        removed: names_removed(text, &new),
        left: Vec::new(),
        by: "ruff",
        sorted: true,
        text: new,
    })
}

/// Whether the imports of `file` are left alone without `ruff`: in a package's `__init__.py` an
/// import that nothing in the file uses is usually what the package offers.
pub fn is_package_init(file: &Path) -> bool {
    file.file_name().is_some_and(|n| n == "__init__.py")
}

/// `text` without the imports whose bound names start at one of the 0-based `unused` positions
/// (line, character) — the places the language server marks as not accessed (behaviour 6). Only
/// import statements of the module itself that stand alone on their lines and hold no comment
/// are changed; a statement loses its unused names and goes when none is left.
pub fn remove_unused_imports(text: &str, unused: &[(u64, u64)]) -> ImportCleanup {
    let mut result = ImportCleanup {
        text: text.to_owned(),
        removed: Vec::new(),
        left: Vec::new(),
        by: "language server",
        sorted: false,
    };
    let Ok(doc) = SyntaxDocument::new(LanguageId::Python, false, text) else {
        return result;
    };
    let unused: HashSet<(u64, u64)> = unused.iter().copied().collect();
    let mut statements = Vec::new();
    import_statements(doc.tree().root_node(), true, &mut statements);
    // (byte range, replacement), in document order.
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (statement, top) in statements {
        let names = imported(statement);
        let (gone, kept): (Vec<_>, Vec<_>) = names
            .iter()
            .partition(|(_, bound)| unused.contains(&position(text, bound.start_byte())));
        if gone.is_empty() {
            continue;
        }
        let listed = || gone.iter().map(|(_, b)| word(*b, text).to_owned());
        let line_start = text[..statement.start_byte()]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let line_end = text[statement.end_byte()..]
            .find('\n')
            .map_or(text.len(), |i| statement.end_byte() + i);
        let alone = text[line_start..statement.start_byte()].trim().is_empty()
            && text[statement.end_byte()..line_end].trim().is_empty();
        let commented = {
            fn has_comment(n: Node) -> bool {
                n.kind() == "comment" || children(n).into_iter().any(has_comment)
            }
            has_comment(statement)
        };
        let reason = if !top {
            Some("inside a block")
        } else if !alone {
            Some("shares its line with other code or a comment")
        } else if commented {
            Some("holds a comment")
        } else if statement.has_error() {
            Some("not parsed")
        } else {
            None
        };
        if let Some(reason) = reason {
            result
                .left
                .extend(listed().map(|n| format!("{n} ({reason})")));
            continue;
        }
        result.removed.extend(listed());
        if kept.is_empty() {
            // The whole statement with its line break.
            let end = (line_end + 1).min(text.len());
            edits.push((line_start..end, String::new()));
            continue;
        }
        let first = names[0].0.start_byte();
        let last = names[names.len() - 1].0.end_byte();
        let separator = if text[first..last].contains('\n') {
            // One name per line, indented like the first.
            let indent_start = text[..first].rfind('\n').map_or(0, |i| i + 1);
            let indent: String = text[indent_start..first]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            format!(",\n{indent}")
        } else {
            ", ".to_owned()
        };
        let kept: Vec<&str> = kept.iter().map(|(name, _)| word(*name, text)).collect();
        edits.push((first..last, kept.join(&separator)));
    }
    for (range, replacement) in edits.into_iter().rev() {
        result.text.replace_range(range, &replacement);
    }
    result
}

/// The 0-based positions (line, character) where the language server of `file` marks something
/// as unnecessary in `text` (the saved file without one): for Python the names that are not
/// accessed, unused imports among them.
pub async fn unnecessary_positions_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<(u64, u64)>, LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let found = o
        .diagnostics(wait)
        .await
        .iter()
        .map(crate::lsp::diagnostic)
        .filter(|d| d.unnecessary)
        .map(|d| (d.start_line, d.start_character))
        .collect();
    o.close().await;
    Ok(found)
}

/// The import clean-up of `file` with content `text` without `ruff` (behaviour 6): removes the
/// imports the language server reports as unused; nothing is sorted.
pub async fn language_server_imports_in(
    root: &Path,
    file: &Path,
    text: &str,
    wait: Duration,
) -> Result<ImportCleanup, String> {
    if is_package_init(file) {
        return Err(
            "Without ruff the imports of an __init__.py are left alone: an import nothing in the file uses is usually what the package offers. Install ruff to clean them by the project's rules."
                .to_owned(),
        );
    }
    let unused = unnecessary_positions_in(root, file, Some(text), wait)
        .await
        .map_err(|e| {
            format!(
                "Without ruff the unused imports come from the Python language server, which gave none: {e}"
            )
        })?;
    Ok(remove_unused_imports(text, &unused))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-pytools-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    /// SPEC-0329/AK-1: ruff before black, the environment before `PATH`; in safe mode never the
    /// environment's tool and no `PATH` entry inside the project.
    #[test]
    fn lookup_order_and_safe_mode() {
        let base = tree("lookup");
        let root = base.join("project");
        let env = root.join(".venv/bin");
        let outside = base.join("tools/bin");
        let inside_path = root.join("scripts");
        std::fs::create_dir_all(&root).unwrap();
        let search = |trusted: bool, dirs: &[&Path]| ToolSearch {
            root: root.clone(),
            environment: Some(env.clone()),
            path_var: Some(std::env::join_paths(dirs).unwrap()),
            trusted,
        };
        let found = |s: &ToolSearch| find_formatter(s).map(|t| (t.kind, t.bin, t.from_environment));
        assert_eq!(found(&search(true, &[&outside])), None);

        touch(&outside.join("black"));
        assert_eq!(
            found(&search(true, &[&outside])),
            Some((ToolKind::Black, outside.join("black"), false))
        );
        touch(&env.join("black"));
        assert_eq!(
            found(&search(true, &[&outside])),
            Some((ToolKind::Black, env.join("black"), true))
        );
        // ruff on PATH wins over black in the environment.
        touch(&outside.join("ruff"));
        assert_eq!(
            found(&search(true, &[&outside])),
            Some((ToolKind::Ruff, outside.join("ruff"), false))
        );
        touch(&env.join("ruff"));
        let trusted = search(true, &[&outside]);
        assert_eq!(
            found(&trusted),
            Some((ToolKind::Ruff, env.join("ruff"), true))
        );
        assert_eq!(find_formatter(&trusted).unwrap().dir, root);
        assert_eq!(find_ruff(&trusted).unwrap().bin, env.join("ruff"));

        // Safe mode: the environment's tools are passed over.
        assert_eq!(
            found(&search(false, &[&outside])),
            Some((ToolKind::Ruff, outside.join("ruff"), false))
        );
        // A PATH entry inside the project and a relative one are passed over too.
        touch(&inside_path.join("ruff"));
        let safe = search(false, &[&inside_path, Path::new("scripts"), &env]);
        assert_eq!(found(&safe), None);
        let message = missing(&safe, &[ToolKind::Ruff, ToolKind::Black]);
        assert!(message.starts_with("Safe mode: ruff or black"), "{message}");
        assert_eq!(
            found(&search(true, &[&inside_path])),
            Some((ToolKind::Ruff, env.join("ruff"), true))
        );

        // Nothing anywhere: the message names the place and the way to install.
        let none = ToolSearch {
            root: root.clone(),
            environment: Some(base.join("empty/bin")),
            path_var: None,
            trusted: true,
        };
        assert_eq!(find_formatter(&none), None);
        let message = missing(&none, &[ToolKind::Ruff, ToolKind::Black]);
        assert!(message.starts_with("No ruff or black found"), "{message}");
        assert!(message.contains("python_install_packages"), "{message}");
        assert!(message.contains("installs nothing itself"), "{message}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// SPEC-0329/AK-2: the command lines.
    #[test]
    fn command_lines() {
        let range = Range {
            start_line: 2,
            start_character: 0,
            end_line: 5,
            end_character: 0,
        };
        assert_eq!(
            format_arguments(ToolKind::Ruff, "app/a.py", None),
            ["format", "--stdin-filename", "app/a.py", "-"]
        );
        assert_eq!(
            format_arguments(ToolKind::Ruff, "a.py", Some(&range)),
            ["format", "--stdin-filename", "a.py", "--range=3:1-6:1", "-"]
        );
        assert_eq!(
            format_arguments(ToolKind::Black, "a.py", None),
            ["-q", "--stdin-filename", "a.py", "-"]
        );
        assert_eq!(
            format_arguments(ToolKind::Black, "a.py", Some(&range)),
            ["-q", "--stdin-filename", "a.py", "--line-ranges=3-5", "-"]
        );
        let within = Range {
            end_character: 4,
            ..range
        };
        assert_eq!(
            format_arguments(ToolKind::Black, "a.py", Some(&within))[3],
            "--line-ranges=3-6"
        );
        assert_eq!(
            import_arguments("a.py"),
            [
                "check",
                "--select",
                "F401,I",
                "--fix-only",
                "--no-cache",
                "--stdin-filename",
                "a.py",
                "-"
            ]
        );
    }

    /// SPEC-0329/AK-3: the plan of a formatted text.
    #[test]
    fn plan_of_a_change() {
        let old = "ä = 1\nx=1\ny = 2\n";
        let new = "ä = 1\nx = 1\ny = 2\n";
        let p = plan(Path::new("/p/a.py"), old, new);
        assert!(p.whitespace_only);
        assert_eq!(p.formatted, new);
        assert_eq!(p.edits.edits.len(), 1);
        let e = &p.edits.edits[0];
        assert_eq!(
            (
                e.range.start_line,
                e.range.start_character,
                e.range.end_line,
                e.range.end_character
            ),
            (1, 1, 1, 2)
        );
        assert_eq!((e.old_text.as_str(), e.new_text.as_str()), ("=", " = "));
        assert_eq!(
            crate::rename::apply_edits(old, &p.edits.edits).unwrap(),
            new
        );
        let same = plan(Path::new("/p/a.py"), old, old);
        assert!(same.edits.edits.is_empty() && same.whitespace_only);
        assert!(
            !plan(
                Path::new("/p/a.py"),
                "import b\nimport a\n",
                "import a\nimport b\n"
            )
            .whitespace_only
        );
    }

    /// The position of each of `names` in `text`: its first occurrence as a whole word after the
    /// first `import`.
    fn at(text: &str, names: &[&str]) -> Vec<(u64, u64)> {
        let part = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        let from = text.find("import").unwrap();
        names
            .iter()
            .map(|n| {
                let (i, _) = text[from..]
                    .match_indices(n)
                    .find(|(i, _)| {
                        !part(text[..from + i].chars().next_back())
                            && !part(text[from + i + n.len()..].chars().next())
                    })
                    .unwrap();
                position(text, from + i)
            })
            .collect()
    }

    /// SPEC-0329/AK-5: whole statements go, others lose their unused names.
    #[test]
    fn unused_imports_are_removed() {
        let text = "\
import os, sys
from app.prices import Item
import json, re, typing as t
from collections import (
    OrderedDict,
    defaultdict,
    namedtuple,
)
from pathlib import Path, PurePath as PP

print(re, defaultdict, Path)
";
        let unused = at(
            text,
            &[
                "os",
                "sys",
                "Item",
                "json",
                "t",
                "OrderedDict",
                "namedtuple",
                "PP",
            ],
        );
        let done = remove_unused_imports(text, &unused);
        assert_eq!(
            done.text,
            "\
import re
from collections import (
    defaultdict,
)
from pathlib import Path

print(re, defaultdict, Path)
"
        );
        assert_eq!(
            done.removed,
            [
                "os",
                "sys",
                "Item",
                "json",
                "t",
                "OrderedDict",
                "namedtuple",
                "PP"
            ]
        );
        assert!(done.left.is_empty());
        assert_eq!((done.by, done.sorted), ("language server", false));
        // Nothing unused: nothing changes; other unnecessary places do not matter.
        let quiet = remove_unused_imports(text, &[(10, 0), (40, 2)]);
        assert_eq!(quiet.text, text);
        assert!(quiet.removed.is_empty());
    }

    /// SPEC-0329/AK-5: what is left alone, with the reason.
    #[test]
    fn unused_imports_that_are_left() {
        let text = "\
from __future__ import annotations
import os  # noqa: F401
import sys; import re
from a import (
    b,  # the b
    c,
)
try:
    import yaml
except ImportError:
    yaml = None
if True:
    import json
def f():
    import csv
from m import *
import last
";
        let unused = at(
            text,
            &["os", "sys", "re", "b", "yaml", "json", "csv", "last"],
        );
        let done = remove_unused_imports(text, &unused);
        assert_eq!(done.text, text.replace("import last\n", ""));
        assert_eq!(done.removed, ["last"]);
        assert_eq!(
            done.left,
            [
                "os (shares its line with other code or a comment)",
                "sys (shares its line with other code or a comment)",
                "re (shares its line with other code or a comment)",
                "b (holds a comment)",
                "yaml (inside a block)",
                "json (inside a block)",
                "csv (inside a block)",
            ]
        );
        // The last line without a line break.
        let done = remove_unused_imports("x = 1\nimport os", &[(1, 7)]);
        assert_eq!(done.text, "x = 1\n");
        assert!(is_package_init(Path::new("/p/app/__init__.py")));
        assert!(!is_package_init(Path::new("/p/app/init.py")));
    }

    /// SPEC-0329/AK-4: the names an import clean-up removed.
    #[test]
    fn removed_names_between_two_texts() {
        let before = "import os, sys\nfrom a import b as c, d\nimport x.y\nimport os\n";
        let after = "import os\nfrom a import d\n";
        assert_eq!(names_removed(before, after), ["sys", "c", "x.y", "os"]);
        assert_eq!(import_names(after), ["os", "d"]);
        let done = |removed: &[&str], left: &[&str], ruff: bool| ImportCleanup {
            text: String::new(),
            removed: removed.iter().map(|s| s.to_string()).collect(),
            left: left.iter().map(|s| s.to_string()).collect(),
            by: if ruff { "ruff" } else { "language server" },
            sorted: ruff,
        };
        assert_eq!(
            summary(&done(&[], &[], true), false, false),
            "Imports are already optimized"
        );
        assert_eq!(
            summary(&done(&[], &[], true), true, false),
            "Imports sorted with ruff"
        );
        assert_eq!(
            summary(&done(&["a", "b"], &[], true), true, false),
            "Removed 2 unused imports and sorted the imports with ruff"
        );
        assert_eq!(
            summary(&done(&["a"], &["b (inside a block)"], false), true, false),
            "Removed 1 unused import; left: b (inside a block) (install ruff to also sort the imports)"
        );
        assert_eq!(
            summary(&done(&[], &[], false), false, false),
            "Imports are already optimized (install ruff to also sort the imports)"
        );
    }

    /// SPEC-0329/AK-8: the hint after a clean-up that did not sort.
    #[test]
    fn summaries() {
        let done = |removed: &[&str], sorted: bool| ImportCleanup {
            text: String::new(),
            removed: removed.iter().map(|s| s.to_string()).collect(),
            left: Vec::new(),
            by: if sorted { "ruff" } else { "language server" },
            sorted,
        };
        assert_eq!(
            summary(&done(&["a"], false), true, false),
            "Removed 1 unused import (install ruff to also sort the imports)"
        );
        assert_eq!(
            summary(&done(&["a"], false), true, true),
            "Removed 1 unused import (trust the project to also sort the imports with its ruff)"
        );
        assert_eq!(
            summary(&done(&[], false), false, true),
            "Imports are already optimized (trust the project to also sort the imports with its ruff)"
        );
        // A clean-up that sorted has no hint, whatever the search says.
        assert_eq!(
            summary(&done(&["a"], true), true, true),
            "Removed 1 unused import and sorted the imports with ruff"
        );
    }

    /// SPEC-0329/AK-8: ruff waits for trust only when the environment of an untrusted project
    /// has the only usable one.
    #[test]
    fn ruff_that_waits_for_trust() {
        let base = tree("waits");
        let root = base.join("project");
        let env = root.join(".venv/bin");
        let outside = base.join("tools/bin");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let search = |trusted: bool, dirs: &[&Path]| ToolSearch {
            root: root.clone(),
            environment: Some(env.clone()),
            path_var: Some(std::env::join_paths(dirs).unwrap()),
            trusted,
        };
        // No ruff at all: nothing waits.
        assert!(!ruff_waits_for_trust(&search(false, &[&outside])));
        // black in the environment is not ruff.
        touch(&env.join("black"));
        assert!(!ruff_waits_for_trust(&search(false, &[&outside])));
        touch(&env.join("ruff"));
        assert!(ruff_waits_for_trust(&search(false, &[&outside])));
        // A PATH entry inside the project does not count in safe mode.
        assert!(ruff_waits_for_trust(&search(false, &[&env])));
        assert!(!ruff_waits_for_trust(&search(true, &[&outside])));
        // ruff on PATH outside the project runs in safe mode: nothing waits.
        touch(&outside.join("ruff"));
        assert!(!ruff_waits_for_trust(&search(false, &[&outside])));
        // Without a selected environment nothing waits.
        let none = ToolSearch {
            root: root.clone(),
            environment: None,
            path_var: None,
            trusted: false,
        };
        assert!(!ruff_waits_for_trust(&none));
        let _ = std::fs::remove_dir_all(&base);
    }
}

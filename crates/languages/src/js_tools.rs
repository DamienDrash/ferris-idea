//! ESLint and Prettier of the project (`SPEC-0123`): found in the project's own `node_modules`,
//! run on the editor's text through standard input, never downloaded or installed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::jvm::TextEdit;

/// Time limit of one run.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// A tool found for a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsTool {
    /// `node_modules/.bin/eslint` or `…/prettier`.
    pub bin: PathBuf,
    /// The package folder the tool runs in (where `node_modules` is).
    pub dir: PathBuf,
}

const ESLINT_FILES: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue"];
const PRETTIER_FILES: &[&str] = &[
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue", "css", "scss", "less", "json",
    "md", "yaml", "yml", "html", "graphql",
];
const ESLINT_CONFIGS: &[&str] = &[
    "eslint.config.js",
    "eslint.config.mjs",
    "eslint.config.cjs",
    "eslint.config.ts",
    "eslint.config.mts",
    "eslint.config.cts",
    ".eslintrc",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".eslintrc.json",
    ".eslintrc.yaml",
    ".eslintrc.yml",
];

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn package_has(dir: &Path, key: &str) -> bool {
    std::fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v.get(key).is_some())
}

/// Folders from the file's own up to `root` (inclusive), nearest first.
fn folders<'a>(root: &'a Path, file: &'a Path) -> impl Iterator<Item = &'a Path> {
    file.ancestors()
        .skip(1)
        .take_while(move |d| d.starts_with(root))
}

/// The project's ESLint for `file`: the nearest folder (up to `root`) with
/// `node_modules/.bin/eslint`, provided an ESLint configuration exists in it or above it within
/// `root` (`eslint.config.*`, `.eslintrc*` or `eslintConfig` in `package.json`).
pub fn find_eslint(root: &Path, file: &Path) -> Option<JsTool> {
    if !ESLINT_FILES.contains(&extension(file).as_str()) {
        return None;
    }
    let dir = folders(root, file).find(|d| d.join("node_modules/.bin/eslint").exists())?;
    let configured = folders(root, file).any(|d| {
        ESLINT_CONFIGS.iter().any(|c| d.join(c).exists()) || package_has(d, "eslintConfig")
    });
    configured.then(|| JsTool {
        bin: dir.join("node_modules/.bin/eslint"),
        dir: dir.to_path_buf(),
    })
}

/// Whether Prettier formats files of `file`'s type (behaviour 1), whether or not the project has it.
pub fn prettier_formats(file: &Path) -> bool {
    PRETTIER_FILES.contains(&extension(file).as_str())
}

/// The project's Prettier for `file`: the nearest folder (up to `root`) with
/// `node_modules/.bin/prettier` (Prettier's defaults apply without a configuration).
pub fn find_prettier(root: &Path, file: &Path) -> Option<JsTool> {
    if !PRETTIER_FILES.contains(&extension(file).as_str()) {
        return None;
    }
    let dir = folders(root, file).find(|d| d.join("node_modules/.bin/prettier").exists())?;
    Some(JsTool {
        bin: dir.join("node_modules/.bin/prettier"),
        dir: dir.to_path_buf(),
    })
}

/// Runs `tool` with `args` in its folder, `input` on standard input; output, error text, exit code.
fn run(tool: &JsTool, args: &[&str], input: &str) -> Result<(String, String, Option<i32>), String> {
    // The project's own tools are project code: never in safe mode (`SPEC-0278` behaviour 5).
    ferris_idea_core::trust::require_trusted(&tool.dir)?;
    run_command(&tool.bin, &tool.dir, args, input)
}

/// Runs `bin` with `args` in `dir`, `input` on standard input, for at most [`TIMEOUT`]; output,
/// error text, exit code. Whether the program may run is the caller's decision.
pub(crate) fn run_command(
    bin: &Path,
    dir: &Path,
    args: &[&str],
    input: &str,
) -> Result<(String, String, Option<i32>), String> {
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    let mut stdin = child.stdin.take();
    let text = input.to_owned();
    let writer = std::thread::spawn(move || {
        if let Some(s) = stdin.as_mut() {
            let _ = s.write_all(text.as_bytes());
        }
    });
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no answer within {} s", TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let _ = writer.join();
    Ok((
        reader.join().unwrap_or_default(),
        err_reader.join().unwrap_or_default(),
        status.code(),
    ))
}

fn relative(tool: &JsTool, file: &Path) -> String {
    file.strip_prefix(&tool.dir)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Severity of an ESLint finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    /// ESLint severity 2.
    Error,
    /// ESLint severity 1.
    Warning,
}

/// One ESLint finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintProblem {
    /// Byte range in the text that was checked.
    pub range: std::ops::Range<usize>,
    /// 1-based line.
    pub line: usize,
    /// Error or warning.
    pub severity: LintSeverity,
    /// Rule, e.g. `no-unused-vars` (none for a parse error).
    pub rule: Option<String>,
    /// ESLint's message.
    pub message: String,
    /// Whether `--fix` can fix it.
    pub fixable: bool,
    /// ESLint's own fix: byte range of the checked text and its replacement (SPEC-0123 v2).
    #[serde(skip)]
    pub fix: Option<(std::ops::Range<usize>, String)>,
}

#[derive(Deserialize)]
struct EslintMessage {
    #[serde(rename = "ruleId")]
    rule_id: Option<String>,
    severity: u8,
    message: String,
    line: Option<usize>,
    column: Option<usize>,
    #[serde(rename = "endLine")]
    end_line: Option<usize>,
    #[serde(rename = "endColumn")]
    end_column: Option<usize>,
    fix: Option<EslintFix>,
    #[serde(default)]
    fatal: bool,
}

impl EslintMessage {
    /// ESLint's note that it does not check the file at all — no configuration covers it, or an
    /// ignore pattern matches it: no finding about the code (`SPEC-0123` v6 behaviour 2a, OBS-0028
    /// T6). ESLint 9 sends it as a warning without a rule at line 1.
    fn file_ignored(&self) -> bool {
        self.rule_id.is_none() && !self.fatal && self.message.starts_with("File ignored")
    }
}

#[derive(Deserialize)]
struct EslintFix {
    /// Start and end in UTF-16 units of the whole text.
    range: (usize, usize),
    text: String,
}

/// Byte offset of `units` UTF-16 units into `text`.
fn byte_of_units(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.len()
}

#[derive(Deserialize)]
struct EslintResult {
    messages: Vec<EslintMessage>,
    output: Option<String>,
}

/// Byte offset of 1-based `line` and 1-based `column` counted in UTF-16 units, as ESLint reports.
fn byte_at(text: &str, line: usize, column: usize) -> usize {
    let start = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>()
        .min(text.len());
    let mut units = 0;
    for (i, c) in text[start..].char_indices() {
        if units >= column.saturating_sub(1) || c == '\n' {
            return start + i;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// Parses ESLint's JSON report for `text`.
pub fn parse_eslint(text: &str, json: &str) -> Result<Vec<LintProblem>, String> {
    let results: Vec<EslintResult> =
        serde_json::from_str(json).map_err(|e| format!("unexpected ESLint output: {e}"))?;
    Ok(results
        .into_iter()
        .flat_map(|r| r.messages)
        .filter(|m| !m.file_ignored())
        .map(|m| {
            let line = m.line.unwrap_or(1);
            let start = byte_at(text, line, m.column.unwrap_or(1));
            let mut end = match (m.end_line, m.end_column) {
                (Some(l), Some(c)) => byte_at(text, l, c),
                _ => start,
            };
            if end <= start {
                // A point finding marks the rest of its word (or one character).
                end = text[start..]
                    .char_indices()
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map_or(text.len(), |(i, _)| start + i.max(1));
                end = end.min(text.len()).max((start + 1).min(text.len()));
            }
            LintProblem {
                range: start..end,
                line,
                severity: if m.severity >= 2 {
                    LintSeverity::Error
                } else {
                    LintSeverity::Warning
                },
                rule: m.rule_id,
                message: m.message,
                fixable: m.fix.is_some(),
                fix: m.fix.map(|f| {
                    (
                        byte_of_units(text, f.range.0)..byte_of_units(text, f.range.1),
                        f.text,
                    )
                }),
            }
        })
        .collect())
}

/// The ⌥↩ fixes at byte `at` of the checked text (SPEC-0123 v2 behaviour 3a): one per fixable
/// finding under the caret, and, if there is one, a fix of the whole file. Overlapping fixes of
/// the whole file are left to a later run, as ESLint does.
pub fn fixes_at(problems: &[LintProblem], at: usize) -> Vec<crate::inspections::Fix> {
    use crate::inspections::Fix;
    use crate::jvm::TextEdit;
    let mut out = Vec::new();
    let here = |p: &&LintProblem| p.range.start <= at && at <= p.range.end;
    if !problems.iter().any(|p| here(&p)) {
        return out;
    }
    for p in problems.iter().filter(here) {
        if let (Some((range, text)), Some(rule)) = (&p.fix, &p.rule) {
            out.push(Fix {
                title: format!("ESLint: Fix '{rule}'"),
                edits: vec![TextEdit {
                    range: range.clone(),
                    text: text.clone(),
                }],
                learn: None,
                keep: None,
                bundle_key: None,
                action: None,
            });
        }
    }
    let mut fixes: Vec<_> = problems.iter().filter_map(|p| p.fix.as_ref()).collect();
    fixes.sort_by_key(|(r, _)| r.start);
    let mut edits: Vec<TextEdit> = Vec::new();
    for (range, text) in fixes {
        if edits.last().is_none_or(|e| e.range.end <= range.start) {
            edits.push(TextEdit {
                range: range.clone(),
                text: text.clone(),
            });
        }
    }
    if !edits.is_empty() {
        out.push(Fix {
            title: "ESLint: Fix current file".into(),
            edits,
            learn: None,
            keep: None,
            bundle_key: None,
            action: None,
        });
    }
    out
}

/// Checks `text` (the editor's content of `file`, saved or not) with the project's ESLint.
pub fn eslint_check(tool: &JsTool, file: &Path, text: &str) -> Result<Vec<LintProblem>, String> {
    let name = relative(tool, file);
    let (out, err, code) = run(
        tool,
        &["--format", "json", "--stdin", "--stdin-filename", &name],
        text,
    )?;
    // 0: clean, 1: findings, 2: configuration or crash.
    if !matches!(code, Some(0) | Some(1)) || out.trim().is_empty() {
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ESLint failed");
        return Err(format!("ESLint: {}", line.trim()));
    }
    parse_eslint(text, &out)
}

/// What `eslint --fix` makes of `text`: the fixed text, or `None` when nothing is fixable.
pub fn eslint_fix(tool: &JsTool, file: &Path, text: &str) -> Result<Option<String>, String> {
    let name = relative(tool, file);
    let (out, err, code) = run(
        tool,
        &[
            "--fix-dry-run",
            "--format",
            "json",
            "--stdin",
            "--stdin-filename",
            &name,
        ],
        text,
    )?;
    if !matches!(code, Some(0) | Some(1)) || out.trim().is_empty() {
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ESLint failed");
        return Err(format!("ESLint: {}", line.trim()));
    }
    let results: Vec<EslintResult> =
        serde_json::from_str(&out).map_err(|e| format!("unexpected ESLint output: {e}"))?;
    Ok(results
        .into_iter()
        .find_map(|r| r.output)
        .filter(|o| o != text))
}

/// `text` formatted by the project's Prettier with the file's configuration.
pub fn prettier_format(tool: &JsTool, file: &Path, text: &str) -> Result<String, String> {
    let name = relative(tool, file);
    let (out, err, code) = run(tool, &["--stdin-filepath", &name], text)?;
    if code != Some(0) {
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Prettier failed");
        return Err(format!("Prettier: {}", line.trim()));
    }
    Ok(out)
}

/// The smallest single edit that turns `old` into `new` (common start and end kept), so the
/// caret, folds and undo stay where nothing changed.
pub fn minimal_edit(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    Some(TextEdit {
        range: prefix..old.len() - suffix,
        text: new[prefix..new.len() - suffix].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0123 v6 behaviour 2a, AK-14: ESLint 9's notes on files it does not check are no
    /// findings, in the form ESLint 9.17 prints them for a `.ts` file its configuration does not
    /// cover; a fatal parse error without a rule still is one.
    #[test]
    fn notes_on_ignored_files_are_no_findings() {
        let unconfigured = r#"[{"filePath":"/p/src/fmt.ts","messages":[{"ruleId":null,"fatal":false,"severity":1,"message":"File ignored because no matching configuration was supplied.","nodeType":null}],"errorCount":0,"warningCount":1}]"#;
        assert_eq!(parse_eslint("const a = 1\n", unconfigured).unwrap(), []);
        let ignored = r#"[{"filePath":"/p/dist/a.js","messages":[{"ruleId":null,"fatal":false,"severity":1,"message":"File ignored because of a matching ignore pattern. Use \"--no-ignore\" to override."}]}]"#;
        assert_eq!(parse_eslint("x\n", ignored).unwrap(), []);
        let parse_error = r#"[{"filePath":"/p/a.js","messages":[{"ruleId":null,"fatal":true,"severity":2,"message":"Parsing error: Unexpected token","line":1,"column":3}]}]"#;
        assert_eq!(parse_eslint("a b\n", parse_error).unwrap().len(), 1);
    }

    /// SPEC-0123 AK-7: fixes under the caret and for the whole file, in byte ranges.
    #[test]
    fn eslint_fixes_at_the_caret() {
        let text = "var a = '😀'; var b;\n";
        let json = r#"[{"messages":[
            {"ruleId":"no-var","severity":2,"message":"m","line":1,"column":1,"endLine":1,"endColumn":4,"fix":{"range":[0,3],"text":"let"}},
            {"ruleId":"no-var","severity":2,"message":"m","line":1,"column":15,"endLine":1,"endColumn":18,"fix":{"range":[14,17],"text":"let"}},
            {"ruleId":"no-unused","severity":2,"message":"n","line":1,"column":19,"endLine":1,"endColumn":20}
        ]}]"#;
        let p = parse_eslint(text, json).unwrap();
        let second = text.rfind("var").unwrap();
        let f = fixes_at(&p, second);
        assert_eq!(f.len(), 2, "{f:?}");
        assert_eq!(f[0].title, "ESLint: Fix 'no-var'");
        assert_eq!(f[0].edits[0].range, second..second + 3);
        assert_eq!(f[1].edits.len(), 2);
        assert!(fixes_at(&p, 6).is_empty(), "no finding under the caret");
        let no_fix = text.rfind('b').unwrap();
        assert_eq!(fixes_at(&p, no_fix).len(), 1, "whole file only");
    }

    #[test]
    fn eslint_positions_in_utf16_columns() {
        let text = "let a = '😀'; let b\nx == y\n";
        let json = r#"[{"messages":[
            {"ruleId":"prefer-const","severity":2,"message":"m","line":1,"column":19,"endLine":1,"endColumn":20,"fix":{"range":[0,1],"text":""}},
            {"ruleId":"eqeqeq","severity":1,"message":"e","line":2,"column":3,"endLine":2,"endColumn":5},
            {"ruleId":null,"severity":2,"message":"Parsing error","line":2,"column":1}
        ]}]"#;
        let p = parse_eslint(text, json).unwrap();
        assert_eq!(&text[p[0].range.clone()], "b");
        assert!(p[0].fixable && p[0].severity == LintSeverity::Error);
        assert_eq!(&text[p[1].range.clone()], "==");
        assert_eq!(p[1].severity, LintSeverity::Warning);
        assert_eq!(
            &text[p[2].range.clone()],
            "x",
            "a point finding marks its word"
        );
        assert_eq!(p[2].rule, None);
        assert!(parse_eslint(text, "nope").is_err());
    }

    #[test]
    fn smallest_edit() {
        let e = minimal_edit("let a = 1\nlet b=2\n", "let a = 1\nlet b = 2;\n").unwrap();
        assert_eq!((e.range.clone(), e.text.as_str()), (15..17, " = 2;"));
        assert_eq!(minimal_edit("x", "x"), None);
        let e = minimal_edit("äb", "äc").unwrap();
        assert_eq!((e.range, e.text.as_str()), (2..3, "c"));
        let e = minimal_edit("aaa", "aa").unwrap();
        assert_eq!(apply_one("aaa", &e), "aa");
    }

    fn apply_one(text: &str, e: &TextEdit) -> String {
        crate::jvm::apply(text, std::slice::from_ref(e))
    }

    #[test]
    fn tools_come_from_the_project() {
        let root = std::env::temp_dir().join(format!("ferris-idea-jstools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = root.join("packages/app");
        std::fs::create_dir_all(app.join("node_modules/.bin")).unwrap();
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::write(app.join("node_modules/.bin/eslint"), "").unwrap();
        std::fs::write(app.join("node_modules/.bin/prettier"), "").unwrap();
        let file = app.join("src/a.ts");
        assert!(find_eslint(&root, &file).is_none(), "no configuration");
        std::fs::write(root.join("eslint.config.js"), "export default []").unwrap();
        assert_eq!(find_eslint(&root, &file).unwrap().dir, app);
        assert_eq!(
            find_prettier(&root, &file).unwrap().bin,
            app.join("node_modules/.bin/prettier")
        );
        assert!(find_eslint(&root, &app.join("README.md")).is_none());
        assert!(find_prettier(&root, &app.join("main.rs")).is_none());
        assert!(
            find_prettier(&app, &root.join("x.js")).is_none(),
            "outside the root"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

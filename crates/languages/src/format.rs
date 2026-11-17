//! Code formatting through the language server (`SPEC-0043`).

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::LanguageId;
use crate::insight::{Opened, Range};
use crate::lsp::LspError;
use crate::rename::{FileEdits, apply_edits, file_edits};

/// Formatting options (behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    /// Width of an indentation level.
    pub tab_size: u32,
    /// Spaces instead of tabs.
    pub insert_spaces: bool,
    /// Remove whitespace at line ends.
    pub trim_trailing_whitespace: bool,
    /// End the file with a newline.
    pub insert_final_newline: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: true,
            insert_final_newline: true,
        }
    }
}

/// The Code Style language (`SPEC-0150` behaviour 2, `idecraft_core::CODE_STYLE_LANGUAGES`) whose
/// indentation applies to `path`; "Default" for a language without a row of its own.
pub fn code_style_language(path: &Path) -> &'static str {
    use LanguageId as L;
    match L::detect(path) {
        Some(L::Cpp) => "C/C++",
        Some(L::Css) => "CSS",
        Some(L::Scss) => "SCSS",
        Some(L::Less) => "Less",
        Some(L::Go) => "Go",
        Some(L::Html | L::Vue | L::Svelte) => "HTML",
        Some(L::Java) => "Java",
        Some(L::JavaScript) => "JavaScript",
        Some(L::Json) => "JSON",
        Some(L::Kotlin) => "Kotlin",
        Some(L::Markdown) => "Markdown",
        Some(L::Python) => "Python",
        Some(L::Ruby) => "Ruby",
        Some(L::Rust) => "Rust",
        Some(L::Sql) => "SQL",
        Some(L::TypeScript) => "TypeScript",
        Some(L::Yaml) => "YAML",
        _ => "Default",
    }
}

impl FormatOptions {
    /// The options `file` is formatted with when nobody chose others (behaviour 1, v5): the
    /// indentation of its `.editorconfig` (`SPEC-0137`), else of Code Style for its language
    /// (`style`: what the user set, else the built-in table
    /// `idecraft_core::CodeStyleSettings::default_indent` — two spaces for JSON, YAML, SCSS and
    /// Less, four for CSS and most others).
    pub fn for_file(file: &Path, style: &idecraft_core::CodeStyleSettings) -> Self {
        let config = idecraft_core::editorconfig::editorconfig_for(file);
        let style = style.indent_for(code_style_language(file));
        let unit = config.indent_unit().unwrap_or_else(|| style.unit());
        let tabs = unit == "\t";
        Self {
            tab_size: if tabs {
                config.tab_width().map_or(style.tab, |w| w as u32)
            } else {
                unit.chars().count().max(1) as u32
            },
            insert_spaces: !tabs,
            ..Self::default()
        }
        .with_editorconfig(&config)
    }

    /// These options with trim trailing whitespace and insert final newline as `config` sets them
    /// (behaviour 1, v8); what it leaves unset stays.
    pub fn with_editorconfig(self, config: &idecraft_core::editorconfig::EditorConfig) -> Self {
        Self {
            trim_trailing_whitespace: config
                .trim_trailing_whitespace()
                .unwrap_or(self.trim_trailing_whitespace),
            insert_final_newline: config
                .insert_final_newline()
                .unwrap_or(self.insert_final_newline),
            ..self
        }
    }

    fn json(&self) -> serde_json::Value {
        json!({
            "tabSize": self.tab_size,
            "insertSpaces": self.insert_spaces,
            "trimTrailingWhitespace": self.trim_trailing_whitespace,
            "insertFinalNewline": self.insert_final_newline,
        })
    }
}

/// A language IDECraft formats with a formatter of its own (behaviours 11 and 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnFormatter {
    /// `idecraft_database::sql_format`.
    Sql,
    /// [`crate::markdown_format`].
    Markdown,
}

/// What `format_file` names as formatter when IDECraft formatted itself.
pub const OWN_FORMATTER: &str = "idecraft";

impl OwnFormatter {
    /// The formatter of `file`'s language, if IDECraft has one of its own.
    pub fn of(file: &Path) -> Option<Self> {
        match LanguageId::detect(file)? {
            LanguageId::Sql => Some(Self::Sql),
            LanguageId::Markdown => Some(Self::Markdown),
            _ => None,
        }
    }

    /// "SQL", "Markdown".
    pub fn language(self) -> &'static str {
        match self {
            Self::Sql => "SQL",
            Self::Markdown => "Markdown",
        }
    }

    /// What to say when a selection was to be formatted: these formatters lay out whole files.
    pub fn whole_files_only(self) -> String {
        format!(
            "IDECraft's {} formatter formats whole files only, not a selection: reformat without a selection (MCP: without `range`)",
            self.language()
        )
    }

    /// The plan for `file` from its text and what the formatter made of it; a refusal of the
    /// formatter says that nothing was changed.
    pub fn plan(
        self,
        file: &Path,
        old: &str,
        formatted: Result<String, String>,
    ) -> Result<FormatPlan, LspError> {
        match formatted {
            Ok(new) => Ok(crate::python_tools::plan(file, old, &new)),
            Err(why) => Err(LspError::Refused(format!(
                "IDECraft's {} formatter changed nothing: {why}",
                self.language()
            ))),
        }
    }
}

/// What formatting changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormatPlan {
    /// Edits of the file (empty when already formatted).
    pub edits: FileEdits,
    /// The file after formatting.
    pub formatted: String,
    /// Whether only whitespace changes.
    pub whitespace_only: bool,
}

/// Whether `a` and `b` differ only in whitespace (behaviour 3).
pub fn whitespace_only(a: &str, b: &str) -> bool {
    a.chars()
        .filter(|c| !c.is_whitespace())
        .eq(b.chars().filter(|c| !c.is_whitespace()))
}

/// The lines `first..=last` (0-based) of `old` laid out as in `whole`, a formatter's result for
/// all of `old`, and the rest of `old` left as it is (behaviour 15). The visible characters of
/// `old` and `whole` are matched where both sides agree from the start and from the end; inside
/// the lines, the text between the first and the last visible character is `whole`'s, the
/// indentation before it and the line break after it too, unless the formatter joined these lines
/// with the line before or after them, in which case `old`'s stay. Returns the edit (range in
/// `old`, replacement), `None` when the lines hold only white space, and `Err` when the formatter
/// changed visible characters among them.
fn lines_from_whole(
    old: &str,
    whole: &str,
    first: usize,
    last: usize,
) -> Result<Option<(std::ops::Range<usize>, String)>, ()> {
    let o: Vec<char> = old.chars().collect();
    let n: Vec<char> = whole.chars().collect();
    // Char offsets of the lines in `old`: from the start of `first` to after the line break of
    // `last` (or the end of the text).
    let mut line = 0;
    let (mut start, mut end) = (None, o.len());
    if first == 0 {
        start = Some(0);
    }
    for (i, c) in o.iter().enumerate() {
        if *c == '\n' {
            line += 1;
            if line == first {
                start = Some(i + 1);
            }
            if line == last + 1 {
                end = i + 1;
                break;
            }
        }
    }
    let Some(start) = start else {
        return Ok(None);
    };
    let vo: Vec<usize> = (0..o.len()).filter(|&i| !o[i].is_whitespace()).collect();
    let vn: Vec<usize> = (0..n.len()).filter(|&i| !n[i].is_whitespace()).collect();
    let k1 = vo.partition_point(|&i| i < start);
    let k2 = vo.partition_point(|&i| i < end);
    if k1 == k2 {
        return Ok(None);
    }
    // Visible characters that agree from the start and from the end.
    let prefix = vo
        .iter()
        .zip(&vn)
        .take_while(|(a, b)| o[**a] == n[**b])
        .count();
    let suffix = vo
        .iter()
        .rev()
        .zip(vn.iter().rev())
        .take(vo.len().min(vn.len()) - prefix)
        .take_while(|(a, b)| o[**a] == n[**b])
        .count();
    let map = |k: usize| -> Option<usize> {
        if k < prefix {
            Some(k)
        } else if k >= vo.len() - suffix {
            Some(k + vn.len() - vo.len())
        } else {
            None
        }
    };
    // Every visible character of the lines, and the ones next to them, must be matched.
    let (m1, m2) = (map(k1).ok_or(())?, map(k2 - 1).ok_or(())?);
    if m2 - m1 != k2 - 1 - k1 {
        return Err(());
    }
    let before = if k1 == 0 {
        Some(0)
    } else {
        map(k1 - 1).map(|m| vn[m] + 1)
    };
    let after = if k2 == vo.len() {
        Some(n.len())
    } else {
        map(k2).map(|m| vn[m])
    };
    let (Some(before), Some(after)) = (before, after) else {
        return Err(());
    };
    let text = |v: &[char]| v.iter().collect::<String>();
    let breaks = |v: &[char]| v.iter().filter(|c| **c == '\n').count();
    // The index in `v` after its `count`-th line break (0 for none).
    let after_breaks = |v: &[char], count: usize| match count {
        0 => 0,
        _ => v
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .nth(count - 1)
            .map_or(0, |(i, _)| i + 1),
    };
    // Indentation: the formatter's white space before the first visible character, after as many
    // line breaks as stay outside the lines.
    let outside = breaks(&o[if k1 == 0 { 0 } else { vo[k1 - 1] + 1 }..start]);
    let ws = &n[before..vn[m1]];
    let lead = if breaks(ws) >= outside {
        text(&ws[after_breaks(ws, outside)..])
    } else {
        text(&o[start..vo[k1]])
    };
    // The line break: the formatter's white space after the last visible character, up to as many
    // line breaks as the lines held.
    let inside = breaks(&o[vo[k2 - 1] + 1..end]);
    let ws = &n[vn[m2] + 1..after];
    let trail = if end == o.len() {
        text(ws)
    } else if breaks(ws) >= inside {
        text(&ws[..after_breaks(ws, inside)])
    } else {
        text(&o[vo[k2 - 1] + 1..end])
    };
    Ok(Some((
        start..end,
        format!("{lead}{}{trail}", text(&n[vn[m1]..=vn[m2]])),
    )))
}

/// What a language's files are called in a message: "Markdown files", "shell scripts".
fn files_of(language: LanguageId) -> String {
    match language {
        LanguageId::Shell => "shell scripts".to_owned(),
        LanguageId::Dockerfile => "Dockerfiles".to_owned(),
        other => format!("{} files", crate::servers::language_name(other)),
    }
}

/// The sentence that says the project's Prettier would format `file`, when Prettier formats such
/// files (`SPEC-0123` behaviour 1); empty otherwise.
fn prettier_hint(file: &Path) -> &'static str {
    if crate::js_tools::prettier_formats(file) {
        " The project's Prettier formats such files: add prettier to the package's devDependencies (whole files, trusted projects)."
    } else {
        ""
    }
}

/// Why `file` has no formatter at all, before any server is looked for (behaviour 8): its type is
/// unknown, or IDECraft serves the language itself and has no formatter for it. `None` when a
/// language server or IDECraft's own formatter ([`OwnFormatter`]) is the one to ask.
pub fn no_formatter(file: &Path) -> Option<String> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(language) = LanguageId::detect(file) else {
        return Some(format!(
            "No formatter for {name}: IDECraft knows no language for this file type.{}",
            prettier_hint(file)
        ));
    };
    (crate::servers::candidates(language).is_empty() && OwnFormatter::of(file).is_none()).then(|| {
        format!(
            "No formatter for {}: IDECraft serves them itself, without a language server, and has no formatter of its own for them.{}",
            files_of(language),
            prettier_hint(file)
        )
    })
}

/// What the server `program` needs besides itself to format, and what to do when it is missing
/// (behaviour 8): bash-language-server formats with `shfmt` from the search path and answers
/// without edits when there is none; metals formats with scalafmt and asks what to do when the
/// project has no `.scalafmt.conf`. `None` when nothing is missing.
pub fn missing_tool(program: &str, root: &Path, path_var: &OsStr) -> Option<String> {
    match program {
        "bash-language-server"
            if !matches!(
                crate::servers::find_in(path_var, "shfmt"),
                crate::servers::Availability::Ready(_)
            ) =>
        {
            Some(
                "bash-language-server formats shell scripts with shfmt, and shfmt was not found on PATH (install: brew install shfmt, or go install mvdan.cc/sh/v3/cmd/shfmt@latest); nothing was changed"
                    .to_owned(),
            )
        }
        "metals" if !root.join(".scalafmt.conf").is_file() => Some(
            "metals formats Scala files with scalafmt and needs a .scalafmt.conf in the project folder, which is missing: create one that sets `version` (the scalafmt release) and `runner.dialect` (scala213 or scala3); nothing was changed"
                .to_owned(),
        ),
        _ => None,
    }
}

/// What to say when the server `program` has no formatter (behaviours 2 and 8). A server that
/// formats whole files but no ranges formats a selection through the whole file (behaviour 15).
pub fn not_offered(program: &str, file: &Path) -> String {
    let files = LanguageId::detect(file).map_or_else(|| "such files".to_owned(), files_of);
    format!(
        "{program} has no formatter, so {files} are not reformatted.{}",
        prettier_hint(file)
    )
}

/// Servers whose range formatting is not used although they offer it, because it formats the
/// selected text as if it stood alone (behaviour 15): kotlin-language-server's ktfmt splits a
/// name at the end of a selection inside a line, fails on a selection that ends inside a block,
/// and drops the indentation of a selected member.
pub fn ranges_through_whole_file(program: &str) -> bool {
    program == "kotlin-language-server"
}

/// The lines of the selection `r` laid out as in `whole`, the server's result for the whole of
/// `old` (behaviour 15): the edits and the text after them.
fn selection_of(
    program: &str,
    old: &str,
    whole: &str,
    r: &Range,
) -> Result<(Vec<crate::rename::Edit>, String), LspError> {
    let first = r.start_line as usize;
    // A selection that ends at the start of a line does not take that line.
    let last = if r.end_character == 0 && r.end_line > r.start_line {
        r.end_line as usize - 1
    } else {
        r.end_line as usize
    };
    let Some((range, replacement)) = lines_from_whole(old, whole, first, last).map_err(|()| {
        LspError::Refused(format!(
            "{program} formats a selection only as part of the whole file, and there it changes more than white space in the selected lines or next to them, so they were left as they are: reformat without a selection (MCP: without `range`)"
        ))
    })?
    else {
        return Ok((Vec::new(), old.to_owned()));
    };
    let chars: Vec<char> = old.chars().collect();
    let was: String = chars[range.clone()].iter().collect();
    if was == replacement {
        return Ok((Vec::new(), old.to_owned()));
    }
    // Line and character of a char offset of `old`.
    let place = |at: usize| {
        let line = chars[..at].iter().filter(|c| **c == '\n').count() as u64;
        let column = chars[..at].iter().rev().take_while(|c| **c != '\n').count() as u64;
        (line, column)
    };
    let ((start_line, start_character), (end_line, end_character)) =
        (place(range.start), place(range.end));
    let mut formatted: String = chars[..range.start].iter().collect();
    formatted.push_str(&replacement);
    formatted.extend(&chars[range.end..]);
    Ok((
        vec![crate::rename::Edit {
            range: Range {
                start_line,
                start_character,
                end_line,
                end_character,
            },
            old_text: was,
            new_text: replacement,
        }],
        formatted,
    ))
}

/// What to say when the server answered without edits but showed an error or asked a question
/// meanwhile (behaviour 9); `None` when it said nothing, which means already formatted.
pub fn said_instead(program: &str, messages: &[String]) -> Option<String> {
    let last = messages.last()?;
    Some(format!(
        "{program} changed nothing and says: {last} (IDECraft chooses none of a server's answers for you)"
    ))
}

/// Whether a server capability such as `documentFormattingProvider` is offered: `true` or an
/// options object.
fn offered(capability: &serde_json::Value) -> bool {
    !(capability.is_null() || *capability == false)
}

/// Plans formatting `file`, or only `range` of it (behaviours 1–3).
pub async fn plan_format(
    root: &Path,
    file: &Path,
    range: Option<&Range>,
    options: FormatOptions,
    wait: Duration,
) -> Result<FormatPlan, LspError> {
    plan_format_in(root, file, None, range, options, wait).await
}

/// Like [`plan_format`], on `text` instead of the saved file when given (unsaved editor text); the
/// edits and the formatted text refer to `text` (SPEC-0075).
pub async fn plan_format_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: Option<&Range>,
    options: FormatOptions,
    wait: Duration,
) -> Result<FormatPlan, LspError> {
    if let Some(why) = no_formatter(file) {
        return Err(LspError::Refused(why));
    }
    match OwnFormatter::of(file) {
        Some(own) if range.is_some() => return Err(LspError::Refused(own.whole_files_only())),
        // No server is asked: marksman has no formatter (behaviour 12).
        Some(own @ OwnFormatter::Markdown) => {
            let old = match text {
                Some(t) => t.to_owned(),
                None => std::fs::read_to_string(file).map_err(|e| LspError::Io(e.to_string()))?,
            };
            let new = crate::markdown_format::format_markdown(&old);
            return own.plan(file, &old, new);
        }
        // The SQL formatter lives with the SQL lexer in `idecraft_database`, which this crate
        // cannot call; `format_file` and Reformat Code call it themselves (behaviour 11).
        Some(OwnFormatter::Sql) => {
            return Err(LspError::Refused(
                "SQL files are formatted by IDECraft's own SQL formatter, not by a language server"
                    .to_owned(),
            ));
        }
        None => {}
    }
    let language =
        LanguageId::detect(file).ok_or_else(|| LspError::NoServer(file.display().to_string()))?;
    let server = crate::servers::resolve_for(language, root)?;
    let program = server.candidate.program;
    let path_var = server
        .path
        .clone()
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    if let Some(why) = missing_tool(program, root, &path_var) {
        return Err(LspError::Refused(why));
    }
    // A server that asks for the file's `formattingOptions` hears the ones of this request, from
    // before the file is opened to after the answer (behaviour 13).
    let _asked = crate::lsp::FormatRequest::new(file, options.json());
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let opts = options.json();
        let whole_files = offered(&o.capabilities["documentFormattingProvider"]);
        // A selection through the whole file's result: the server formats no ranges, or not
        // reliably (behaviour 15).
        let through_whole = range.is_some()
            && whole_files
            && (!offered(&o.capabilities["documentRangeFormattingProvider"])
                || ranges_through_whole_file(program));
        let (method, capability, params) = match range {
            Some(r) if !through_whole => {
                let start = o.position(r.start_line, r.start_character)["position"].clone();
                let end = o.position(r.end_line, r.end_character)["position"].clone();
                (
                    "textDocument/rangeFormatting",
                    "documentRangeFormattingProvider",
                    json!({"textDocument": {"uri": o.uri},
                           "range": {"start": start, "end": end}, "options": opts}),
                )
            }
            _ => (
                "textDocument/formatting",
                "documentFormattingProvider",
                json!({"textDocument": {"uri": o.uri}, "options": opts}),
            ),
        };
        if !offered(&o.capabilities[capability]) {
            return Err(LspError::Refused(not_offered(program, file)));
        }
        // What the server said before belongs to other questions.
        o.lsp().take_messages();
        let answer = o.ask(method, params).await?;
        let uri = o.uri.clone();
        let edits = answer.as_array().cloned().unwrap_or_default();
        let mut files = file_edits(&mut o, &[json!({"changes": {uri: edits}})])?;
        let mut edits = files.pop().unwrap_or(FileEdits {
            path: file.to_path_buf(),
            edits: Vec::new(),
        });
        let mut formatted = apply_edits(&o.text, &edits.edits).map_err(LspError::Protocol)?;
        if let Some(r) = range.filter(|_| through_whole) {
            (edits.edits, formatted) = selection_of(program, &o.text, &formatted, r)?;
        }
        // A server that answers with the whole text also when nothing changes
        // (vscode-css-language-server, vscode-html-language-server) has no edits to make
        // (behaviour 4).
        if formatted == o.text {
            edits.edits.clear();
        }
        // No edits and an error shown or a question asked: that is the answer, not "already
        // formatted" (behaviour 9).
        if edits.edits.is_empty()
            && let Some(why) = said_instead(program, &o.lsp().take_messages())
        {
            return Err(LspError::Refused(why));
        }
        Ok(FormatPlan {
            whitespace_only: whitespace_only(&o.text, &formatted),
            edits,
            formatted,
        })
    }
    .await;
    o.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_detection() {
        assert!(whitespace_only("int a;", "int a;"));
        assert!(whitespace_only(
            "int  main(){\nreturn 0;}",
            "int main() {\n    return 0;\n}\n"
        ));
        assert!(!whitespace_only(
            "#include <b.h>\n#include <a.h>\n",
            "#include <a.h>\n#include <b.h>\n"
        ));
        assert!(!whitespace_only("int a;", "int a ;x"));
    }

    /// AK-5: a file without any formatter says which kind of file it is and why; a file a
    /// language server may format says nothing here.
    #[test]
    fn files_without_a_formatter() {
        let text = no_formatter(Path::new("notes/todo.txt")).unwrap();
        assert_eq!(
            text,
            "No formatter for todo.txt: IDECraft knows no language for this file type."
        );
        // Prettier formats GraphQL, which has no language in IDECraft.
        let graphql = no_formatter(Path::new("schema.graphql")).unwrap();
        assert!(
            graphql.starts_with("No formatter for schema.graphql: "),
            "{graphql}"
        );
        assert!(
            graphql.contains("add prettier to the package's devDependencies"),
            "{graphql}"
        );
        // v5: SQL and Markdown have IDECraft's own formatter.
        for served in [
            "main.c",
            "a.json",
            "README.md",
            "build.sh",
            "Dockerfile",
            "db/query.sql",
        ] {
            assert_eq!(no_formatter(Path::new(served)), None, "{served}");
        }
    }

    /// AK-8a (v5, behaviour 1): the indentation nobody chose comes from one table, Code Style,
    /// and an `.editorconfig` wins over it.
    #[test]
    fn default_options_per_language() {
        let dir =
            std::env::temp_dir().join(format!("idecraft-fmt-defaults-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("configured")).unwrap();
        // Nothing above the folder decides.
        std::fs::write(dir.join(".editorconfig"), "root = true\n").unwrap();
        let mut style = idecraft_core::CodeStyleSettings::default();
        let options = |name: &str, style: &idecraft_core::CodeStyleSettings| {
            let o = FormatOptions::for_file(&dir.join(name), style);
            assert!(o.trim_trailing_whitespace && o.insert_final_newline);
            (o.tab_size, o.insert_spaces)
        };
        for (name, size) in [
            ("data.json", 2),
            ("config.yaml", 2),
            ("theme.scss", 2),
            ("theme.less", 2),
            ("theme.css", 4),
            ("app.ts", 4),
            ("app.js", 4),
            ("page.html", 4),
            ("query.sql", 4),
            ("notes.txt", 4),
        ] {
            assert_eq!(options(name, &style), (size, true), "{name}");
        }
        assert_eq!(options("main.go", &style), (4, false));
        // What the user set in Code Style wins over the table.
        style = serde_json::from_value(json!({
            "indents": {"SCSS": {"indent": 3, "tab": 3, "use_tabs": false}}
        }))
        .unwrap();
        assert_eq!(options("theme.scss", &style), (3, true));
        assert_eq!(options("theme.less", &style), (2, true));
        // An `.editorconfig` wins over both.
        std::fs::write(
            dir.join("configured/.editorconfig"),
            "[*.scss]\nindent_style = space\nindent_size = 8\n[*.json]\nindent_style = tab\ntab_width = 6\n",
        )
        .unwrap();
        assert_eq!(options("configured/theme.scss", &style), (8, true));
        assert_eq!(options("configured/data.json", &style), (6, false));
        assert_eq!(options("configured/config.yaml", &style), (2, true));
        // v8: trim trailing whitespace and the final newline as the `.editorconfig` sets them.
        std::fs::write(
            dir.join("configured/.editorconfig"),
            "[*.txt]\ntrim_trailing_whitespace = false\n[*.md]\ninsert_final_newline = false\n",
        )
        .unwrap();
        let txt = FormatOptions::for_file(&dir.join("configured/notes.txt"), &style);
        assert!(!txt.trim_trailing_whitespace && txt.insert_final_newline);
        let md = FormatOptions::for_file(&dir.join("configured/notes.md"), &style);
        assert!(md.trim_trailing_whitespace && !md.insert_final_newline);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AK-9 and AK-11: SQL and Markdown have a formatter of IDECraft's own; a selection is
    /// refused, a refusal of the formatter says that nothing changed, and a Markdown file is
    /// planned without any server.
    #[tokio::test]
    async fn own_formatters() {
        assert_eq!(
            OwnFormatter::of(Path::new("a/query.sql")),
            Some(OwnFormatter::Sql)
        );
        assert_eq!(
            OwnFormatter::of(Path::new("README.md")),
            Some(OwnFormatter::Markdown)
        );
        assert_eq!(OwnFormatter::of(Path::new("a.json")), None);
        assert_eq!(OwnFormatter::of(Path::new("a.unknown")), None);
        assert_eq!(
            OwnFormatter::Sql.whole_files_only(),
            "IDECraft's SQL formatter formats whole files only, not a selection: reformat without a selection (MCP: without `range`)"
        );
        let refused = OwnFormatter::Sql
            .plan(
                Path::new("q.sql"),
                "select 'a",
                Err("line 1: the quoted text that starts here never ends".into()),
            )
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("IDECraft's SQL formatter changed nothing: line 1: the quoted text"),
            "{refused}"
        );
        let plan = OwnFormatter::Sql
            .plan(Path::new("q.sql"), "select  1;\n", Ok("select 1;\n".into()))
            .unwrap();
        assert!(plan.whitespace_only);
        assert_eq!(plan.formatted, "select 1;\n");
        assert_eq!(plan.edits.edits.len(), 1);

        let dir = std::env::temp_dir().join(format!("idecraft-fmt-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.md");
        std::fs::write(&file, "# Notes\ntext\n|a|b|\n|-|-|\n|long|1|\n").unwrap();
        let wait = Duration::from_secs(5);
        let plan = plan_format(&dir, &file, None, FormatOptions::default(), wait)
            .await
            .unwrap();
        assert_eq!(
            plan.formatted,
            "# Notes\n\ntext\n\n| a    | b |\n|------|---|\n| long | 1 |\n"
        );
        assert!(!plan.edits.edits.is_empty());
        // The editor's unsaved text is what is formatted; formatted text has no edits.
        let again = plan_format_in(
            &dir,
            &file,
            Some(&plan.formatted),
            None,
            FormatOptions::default(),
            wait,
        )
        .await
        .unwrap();
        assert!(again.edits.edits.is_empty());
        let first_line = Range {
            start_line: 0,
            start_character: 0,
            end_line: 1,
            end_character: 0,
        };
        let why = plan_format(
            &dir,
            &file,
            Some(&first_line),
            FormatOptions::default(),
            wait,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            why.contains("IDECraft's Markdown formatter formats whole files only"),
            "{why}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AK-5: what a server that does not offer the formatting is said to lack.
    #[test]
    fn formatting_a_server_does_not_offer() {
        assert_eq!(
            not_offered("groovy-language-server", Path::new("a.groovy")),
            "groovy-language-server has no formatter, so Groovy files are not reformatted."
        );
        // A selection in a file whose server formats nothing: the same as for the file.
        let markdown = not_offered("marksman", Path::new("README.md"));
        assert!(
            markdown.starts_with("marksman has no formatter, so Markdown files are not reformatted. The project's Prettier formats such files"),
            "{markdown}"
        );
        assert!(offered(&json!(true)));
        assert!(offered(&json!({"rangesSupport": true})));
        assert!(!offered(&json!(false)));
        assert!(!offered(&serde_json::Value::Null));
        assert!(ranges_through_whole_file("kotlin-language-server"));
        assert!(!ranges_through_whole_file("clangd"));
    }

    /// SPEC-0043 v8 AK-15: the selected lines laid out as in the whole file's result, the rest
    /// as it was.
    #[test]
    fn a_selection_through_the_whole_file() {
        let take = |old: &str, whole: &str, first: usize, last: usize| {
            lines_from_whole(old, whole, first, last).map(|edit| {
                edit.map(|(range, text)| {
                    let chars: Vec<char> = old.chars().collect();
                    let mut out: String = chars[..range.start].iter().collect();
                    out.push_str(&text);
                    out.extend(&chars[range.end..]);
                    out
                })
            })
        };
        let old = "func a() {\nx:=1\n      if x>1{return}\nreturn\n}\nfunc b(){y:=2}\n";
        let whole = "func a() {\n\tx := 1\n\tif x > 1 {\n\t\treturn\n\t}\n\treturn\n}\nfunc b() { y := 2 }\n";
        // The two middle lines only; the line after them and `b` stay as they were.
        assert_eq!(
            take(old, whole, 1, 2).unwrap().unwrap(),
            "func a() {\n\tx := 1\n\tif x > 1 {\n\t\treturn\n\t}\nreturn\n}\nfunc b(){y:=2}\n"
        );
        // The last line, without a line break at the end of the file.
        let old_end = "a {\nb  c}";
        assert_eq!(
            take(old_end, "a {\n    b c\n}\n", 1, 1).unwrap().unwrap(),
            "a {\n    b c\n}\n"
        );
        // The first line of the file.
        assert_eq!(
            take("  x=1\ny=2\n", "x = 1\ny = 2\n", 0, 0)
                .unwrap()
                .unwrap(),
            "x = 1\ny=2\n"
        );
        // Blank lines the formatter removes among the selected lines go; those before them stay.
        assert_eq!(
            take("a\n\n\n\nb\n\n\nc\n", "a\n\nb\n\nc\n", 2, 4)
                .unwrap()
                .unwrap(),
            "a\n\nb\n\n\nc\n"
        );
        // Lines of white space only: nothing to do.
        assert_eq!(take("a\n   \nb\n", "a\n\nb\n", 1, 1), Ok(None));
        // A formatter that joins the selected line with the next one: the line break stays.
        assert_eq!(
            take("if x\n{\n  y }\n", "if x {\n    y\n}\n", 0, 0)
                .unwrap()
                .unwrap(),
            "if x\n{\n  y }\n"
        );
        // Visible characters changed outside the lines (sorted imports) do not matter; among them
        // they refuse.
        let old_imports = "use b;\nuse a;\nfn f(){1}\n";
        let sorted = "use a;\nuse b;\nfn f() {\n    1\n}\n";
        assert_eq!(
            take(old_imports, sorted, 2, 2).unwrap().unwrap(),
            "use b;\nuse a;\nfn f() {\n    1\n}\n"
        );
        assert_eq!(take(old_imports, sorted, 0, 0), Err(()));
        // A selection past the end.
        assert_eq!(take("a\n", "a\n", 5, 6), Ok(None));
        // Whatever is taken changes only white space, and a second run changes nothing.
        let once = take(old, whole, 1, 2).unwrap().unwrap();
        assert!(whitespace_only(old, &once));
        let again = take(&once, whole, 1, 2).unwrap();
        assert!(again.is_none_or(|t| t == once));
    }

    /// SPEC-0043 v8 AK-15: the edit and text of a selection through the whole file; a result
    /// that differs in visible characters is refused with a sentence.
    #[test]
    fn the_edit_of_a_selection_through_the_whole_file() {
        let r = |sl, sc, el, ec| Range {
            start_line: sl,
            start_character: sc,
            end_line: el,
            end_character: ec,
        };
        let old = "fn a(){\nlet x=1;\n}\n";
        let whole = "fn a() {\n    let x = 1;\n}\n";
        // Up to the start of line 2: line 1 only.
        let (edits, text) = selection_of("rust-analyzer", old, whole, &r(1, 3, 2, 0)).unwrap();
        assert_eq!(text, "fn a(){\n    let x = 1;\n}\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, r(1, 0, 2, 0));
        assert_eq!(edits[0].old_text, "let x=1;\n");
        assert_eq!(
            crate::rename::apply_edits(old, &edits).unwrap(),
            text,
            "the edit gives the text"
        );
        // Already laid out: no edits.
        let (edits, text) = selection_of("rust-analyzer", whole, whole, &r(1, 0, 1, 4)).unwrap();
        assert!(edits.is_empty());
        assert_eq!(text, whole);
        let why = selection_of("rustfmt", "b\na\n", "a\nb\n", &r(0, 0, 0, 1)).unwrap_err();
        assert!(
            why.to_string()
                .contains("rustfmt formats a selection only as part of the whole file"),
            "{why}"
        );
    }

    /// AK-5: the tool a server formats with is looked for before the server is asked.
    #[cfg(unix)]
    #[test]
    fn tools_a_server_formats_with() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("idecraft-fmt-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (bin, root) = (base.join("bin"), base.join("project"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let path = bin.clone().into_os_string();

        let shell = missing_tool("bash-language-server", &root, &path).unwrap();
        assert!(
            shell.contains("shfmt was not found on PATH") && shell.contains("brew install shfmt"),
            "{shell}"
        );
        let shfmt = bin.join("shfmt");
        std::fs::write(&shfmt, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&shfmt, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(missing_tool("bash-language-server", &root, &path), None);

        let scala = missing_tool("metals", &root, &path).unwrap();
        assert!(
            scala.contains(".scalafmt.conf in the project folder"),
            "{scala}"
        );
        std::fs::write(root.join(".scalafmt.conf"), "version = \"3.8.3\"\n").unwrap();
        assert_eq!(missing_tool("metals", &root, &path), None);

        assert_eq!(missing_tool("clangd", &root, &path), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// AK-6: no edits and something said is no "already formatted".
    #[test]
    fn what_the_server_said_instead_of_formatting() {
        assert_eq!(said_instead("metals", &[]), None);
        assert_eq!(
            said_instead(
                "metals",
                &[
                    "old".into(),
                    "No configuration. Proceed? [Create | Not now]".into()
                ]
            )
            .unwrap(),
            "metals changed nothing and says: No configuration. Proceed? [Create | Not now] (IDECraft chooses none of a server's answers for you)"
        );
    }
}

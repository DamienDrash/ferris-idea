//! EditorConfig (`SPEC-0137`): the `.editorconfig` files that apply to a file and the properties
//! they give it, after the EditorConfig specification (editorconfig.org). Files are read from the
//! file's folder upwards until one says `root = true`; sections whose glob matches the file's path
//! set properties, nearer files and later sections winning; `unset` removes a property.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// File name of EditorConfig files.
pub const FILE_NAME: &str = ".editorconfig";

/// One property and where it was set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Setting {
    /// Value, lower-cased for the standard properties.
    pub value: String,
    /// The `.editorconfig` file.
    pub file: PathBuf,
    /// 1-based line.
    pub line: usize,
}

/// The properties of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EditorConfig {
    /// Every property by lower-cased name.
    pub properties: BTreeMap<String, Setting>,
    /// The `.editorconfig` files read, nearest last.
    pub files: Vec<PathBuf>,
    /// Problems: unreadable files, values that are not understood.
    pub problems: Vec<String>,
}

/// `indent_style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IndentStyle {
    /// Tabs.
    Tab,
    /// Spaces.
    Space,
}

impl EditorConfig {
    fn get(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(|s| s.value.as_str())
    }

    fn number(&self, key: &str) -> Option<usize> {
        self.get(key)
            .and_then(|v| v.parse().ok())
            .filter(|n| *n > 0)
    }

    fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    /// Whether any file set anything.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// `indent_style`.
    pub fn indent_style(&self) -> Option<IndentStyle> {
        match self.get("indent_style")? {
            "tab" => Some(IndentStyle::Tab),
            "space" => Some(IndentStyle::Space),
            _ => None,
        }
    }

    /// `tab_width`, else a numeric `indent_size` (the specification's default).
    pub fn tab_width(&self) -> Option<usize> {
        self.number("tab_width")
            .or_else(|| self.number("indent_size"))
    }

    /// `indent_size`; `tab` (or `indent_style = tab` without a size) means the tab width.
    pub fn indent_size(&self) -> Option<usize> {
        match self.get("indent_size") {
            Some("tab") => self.tab_width(),
            Some(_) => self.number("indent_size"),
            None if self.indent_style() == Some(IndentStyle::Tab) => self.tab_width(),
            None => None,
        }
    }

    /// The text one indentation step inserts: a tab, or `indent_size` spaces.
    pub fn indent_unit(&self) -> Option<String> {
        match self.indent_style() {
            Some(IndentStyle::Tab) => Some("\t".into()),
            Some(IndentStyle::Space) => Some(" ".repeat(self.indent_size().unwrap_or(4))),
            None => self.indent_size().map(|n| " ".repeat(n)),
        }
    }

    /// `end_of_line` as the line break to write: `"\n"`, `"\r\n"` or `"\r"`.
    pub fn line_break(&self) -> Option<&'static str> {
        match self.get("end_of_line")? {
            "lf" => Some("\n"),
            "crlf" => Some("\r\n"),
            "cr" => Some("\r"),
            _ => None,
        }
    }

    /// `trim_trailing_whitespace`.
    pub fn trim_trailing_whitespace(&self) -> Option<bool> {
        self.flag("trim_trailing_whitespace")
    }

    /// `insert_final_newline`: `true` adds a final line break, `false` removes it.
    pub fn insert_final_newline(&self) -> Option<bool> {
        self.flag("insert_final_newline")
    }

    /// `max_line_length` (`off` gives none).
    pub fn max_line_length(&self) -> Option<usize> {
        self.number("max_line_length")
    }

    /// The right margin `max_line_length` asks for (`SPEC-0137` v2 behaviour 2a): `Some(Some(n))`
    /// draws it after column `n`, `Some(None)` (`off`) draws none, `None` leaves it to Code Style
    /// (also for a value that is not understood, which is listed as a problem).
    pub fn right_margin(&self) -> Option<Option<usize>> {
        match self.get("max_line_length")? {
            "off" => Some(None),
            _ => self.max_line_length().map(Some),
        }
    }

    /// `charset`.
    pub fn charset(&self) -> Option<&str> {
        self.get("charset")
    }
}

/// Standard properties whose values are case-insensitive (the specification lower-cases them).
const STANDARD: &[&str] = &[
    "indent_style",
    "indent_size",
    "tab_width",
    "end_of_line",
    "charset",
    "trim_trailing_whitespace",
    "insert_final_newline",
    "max_line_length",
    "root",
];

/// A section: its glob and its properties (key, value, 1-based line).
type Section = (String, Vec<(String, String, usize)>);

/// One parsed file: whether it is the root, and its sections.
struct Parsed {
    root: bool,
    sections: Vec<Section>,
}

fn parse(text: &str) -> Parsed {
    let mut root = false;
    let mut sections: Vec<Section> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_owned(), Vec::new()));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let mut value = value.trim().to_owned();
        if STANDARD.contains(&key.as_str()) {
            value = value.to_ascii_lowercase();
        }
        match sections.last_mut() {
            Some((_, props)) => props.push((key, value, i + 1)),
            None if key == "root" => root = value == "true",
            None => {}
        }
    }
    Parsed { root, sections }
}

/// The properties of `file` (an absolute path) from the `.editorconfig` files above it.
pub fn editorconfig_for(file: &Path) -> EditorConfig {
    let mut chain = Vec::new();
    let mut out = EditorConfig::default();
    let mut dir = file.parent();
    while let Some(d) = dir {
        let candidate = d.join(FILE_NAME);
        if candidate.is_file() {
            match std::fs::read_to_string(&candidate) {
                Ok(text) => {
                    let parsed = parse(&text);
                    let root = parsed.root;
                    chain.push((d.to_path_buf(), candidate, parsed));
                    if root {
                        break;
                    }
                }
                Err(e) => out.problems.push(format!("{}: {e}", candidate.display())),
            }
        }
        dir = d.parent();
    }
    // The file nearest the root first, so nearer files win.
    for (dir, path, parsed) in chain.into_iter().rev() {
        let Ok(rel) = file.strip_prefix(&dir) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        out.files.push(path.clone());
        for (glob, props) in &parsed.sections {
            if !section_matches(glob, &rel) {
                continue;
            }
            for (key, value, line) in props {
                if value == "unset" {
                    out.properties.remove(key);
                } else {
                    out.properties.insert(
                        key.clone(),
                        Setting {
                            value: value.clone(),
                            file: path.clone(),
                            line: *line,
                        },
                    );
                }
            }
        }
    }
    for key in [
        "indent_style",
        "end_of_line",
        "trim_trailing_whitespace",
        "insert_final_newline",
        "max_line_length",
    ] {
        let known = match key {
            "indent_style" => out.indent_style().is_some(),
            "max_line_length" => out.right_margin().is_some(),
            "end_of_line" => out.line_break().is_some(),
            _ => out.flag(key).is_some(),
        };
        if let Some(s) = out.properties.get(key).filter(|_| !known) {
            out.problems.push(format!(
                "{}:{}: `{key} = {}` is not understood",
                s.file.display(),
                s.line,
                s.value
            ));
        }
    }
    out
}

/// What the `.editorconfig` files that could apply to `file` hold now: one entry per folder from the
/// file's own up to the file system root, with a hash of the file's bytes or `None` where there is
/// none (or it cannot be read). Two stamps differ when one of them was created, changed or deleted,
/// so an open editor knows when to read its properties again (`SPEC-0137` v2 behaviour 2b). The
/// whole way up is taken, because a change to `root = true` moves where the lookup stops.
pub fn change_stamp(file: &Path) -> Vec<Option<u64>> {
    use std::hash::{Hash, Hasher};
    let mut stamp = Vec::new();
    let mut dir = file.parent();
    while let Some(d) = dir {
        stamp.push(std::fs::read(d.join(FILE_NAME)).ok().map(|bytes| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut h);
            h.finish()
        }));
        dir = d.parent();
    }
    stamp
}

/// Whether section `glob` of an `.editorconfig` file matches `rel`, the file's path relative to
/// that file's folder with `/` separators. A glob without `/` matches the file name in any folder;
/// one with `/` matches from the folder (a leading `/` is dropped).
pub fn section_matches(glob: &str, rel: &str) -> bool {
    let has_slash = glob.contains('/');
    let pattern = glob.strip_prefix('/').unwrap_or(glob);
    let tokens = tokens(pattern);
    if has_slash {
        matches(&tokens, rel)
    } else {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        matches(&tokens, name)
    }
}

#[derive(Debug, Clone)]
enum Tok {
    Lit(char),
    /// `*`: anything but `/`.
    Star,
    /// `**`: anything.
    StarStar,
    /// `?`: one character but `/`.
    Any,
    /// `[…]` or `[!…]`: one character of (or not of) the ranges.
    Class(bool, Vec<(char, char)>),
    /// `{a,b}`: one of the alternatives.
    Alt(Vec<Vec<Tok>>),
    /// `{1..3}`: an integer in the range.
    Range(i64, i64),
}

/// The index of the `}` closing the `{` at `open`, respecting nesting and `\`.
fn closing_brace(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn tokens(pattern: &str) -> Vec<Tok> {
    let chars: Vec<char> = pattern.chars().collect();
    parse_tokens(&chars)
}

fn parse_tokens(chars: &[char]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.push(Tok::Lit(chars[i + 1]));
                i += 2;
                continue;
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push(Tok::StarStar);
                i += 2;
                continue;
            }
            '*' => out.push(Tok::Star),
            '?' => out.push(Tok::Any),
            '[' => {
                if let Some(end) = (i + 1..chars.len()).find(|&j| chars[j] == ']' && j > i + 1) {
                    let body = &chars[i + 1..end];
                    if !body.contains(&'/') {
                        let (neg, body) = match body.first() {
                            Some('!') => (true, &body[1..]),
                            _ => (false, body),
                        };
                        let mut ranges = Vec::new();
                        let mut k = 0;
                        while k < body.len() {
                            if k + 2 < body.len() && body[k + 1] == '-' {
                                ranges.push((body[k], body[k + 2]));
                                k += 3;
                            } else {
                                ranges.push((body[k], body[k]));
                                k += 1;
                            }
                        }
                        out.push(Tok::Class(neg, ranges));
                        i = end + 1;
                        continue;
                    }
                }
                out.push(Tok::Lit('['));
            }
            '{' => {
                if let Some(end) = closing_brace(chars, i) {
                    let body = &chars[i + 1..end];
                    let text: String = body.iter().collect();
                    if let Some(range) = numeric_range(&text) {
                        out.push(range);
                        i = end + 1;
                        continue;
                    }
                    // Split at top-level commas.
                    let mut parts = Vec::new();
                    let (mut depth, mut start, mut k) = (0, 0, 0);
                    while k < body.len() {
                        match body[k] {
                            '\\' => k += 1,
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            ',' if depth == 0 => {
                                parts.push(&body[start..k]);
                                start = k + 1;
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    if !parts.is_empty() {
                        parts.push(&body[start..]);
                        out.push(Tok::Alt(parts.into_iter().map(parse_tokens).collect()));
                        i = end + 1;
                        continue;
                    }
                }
                out.push(Tok::Lit('{'));
            }
            c => out.push(Tok::Lit(c)),
        }
        i += 1;
    }
    out
}

fn numeric_range(text: &str) -> Option<Tok> {
    let (a, b) = text.split_once("..")?;
    let num = |s: &str| {
        let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
        (!digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<i64>().ok())
            .flatten()
    };
    let (a, b) = (num(a)?, num(b)?);
    Some(Tok::Range(a.min(b), a.max(b)))
}

fn matches(toks: &[Tok], s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    match_at(toks, &chars)
}

fn match_at(toks: &[Tok], s: &[char]) -> bool {
    let Some((first, rest)) = toks.split_first() else {
        return s.is_empty();
    };
    match first {
        Tok::Lit(c) => s.first() == Some(c) && match_at(rest, &s[1..]),
        Tok::Any => s.first().is_some_and(|c| *c != '/') && match_at(rest, &s[1..]),
        Tok::Class(neg, ranges) => {
            s.first()
                .is_some_and(|c| *c != '/' && ranges.iter().any(|(a, b)| a <= c && c <= b) != *neg)
                && match_at(rest, &s[1..])
        }
        Tok::Star => (0..=s.len())
            .take_while(|&n| n == 0 || s[n - 1] != '/')
            .any(|n| match_at(rest, &s[n..])),
        Tok::StarStar => (0..=s.len()).any(|n| match_at(rest, &s[n..])),
        Tok::Alt(alternatives) => alternatives.iter().any(|alt| {
            let mut joined = alt.clone();
            joined.extend_from_slice(rest);
            match_at(&joined, s)
        }),
        Tok::Range(lo, hi) => {
            let sign = usize::from(s.first() == Some(&'-'));
            let digits = s[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
            (1..=digits).any(|n| {
                let text: String = s[..sign + n].iter().collect();
                text.parse::<i64>().is_ok_and(|v| *lo <= v && v <= *hi)
                    && match_at(rest, &s[sign + n..])
            })
        }
    }
}

/// Edits that saving makes under `config` and the IDE's own rules (`SPEC-0137` behaviour 3), as
/// byte ranges of `text` with their replacements, in order: trailing spaces removed (except on
/// `keep_line`), line breaks converted to `end_of_line`, and the final line break added or removed.
/// `strip`/`final_break` are the IDE settings EditorConfig overrides.
pub fn save_edits(
    text: &str,
    config: &EditorConfig,
    strip: bool,
    final_break: bool,
    keep_line: Option<usize>,
) -> Vec<(std::ops::Range<usize>, String)> {
    let strip = config.trim_trailing_whitespace().unwrap_or(strip);
    let final_newline = config
        .insert_final_newline()
        .or(final_break.then_some(true));
    let target = config.line_break();
    let mut edits = Vec::new();
    let mut at = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let ends = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);
        let crlf = ends && body.ends_with('\r');
        let body = if crlf { &body[..body.len() - 1] } else { body };
        let kept = body.trim_end_matches([' ', '\t']).len();
        if strip && Some(i) != keep_line && kept < body.len() {
            edits.push((at + kept..at + body.len(), String::new()));
        }
        if let Some(target) = target.filter(|_| ends) {
            let current = if crlf { "\r\n" } else { "\n" };
            if current != target {
                edits.push((at + body.len()..at + line.len(), target.to_owned()));
            }
        }
        at += line.len();
    }
    match final_newline {
        Some(true) if !text.is_empty() && !text.ends_with('\n') && !text.ends_with('\r') => {
            let brk = target.unwrap_or(if text.contains("\r\n") { "\r\n" } else { "\n" });
            edits.push((text.len()..text.len(), brk.to_owned()));
        }
        Some(false) => {
            let cut = text.trim_end_matches(['\r', '\n']).len();
            if cut < text.len() {
                edits.retain(|(r, _)| r.end <= cut);
                edits.push((cut..text.len(), String::new()));
            }
        }
        _ => {}
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_follow_the_specification() {
        let m = section_matches;
        assert!(m("*", "src/a.rs"));
        assert!(m("*.md", "docs/deep/readme.md"));
        assert!(!m("*.md", "readme.mdx"));
        assert!(m("*.{js,ts}", "web/app.ts"));
        assert!(!m("*.{js,ts}", "web/app.tsx"));
        assert!(m("{package.json,.travis.yml}", "package.json"));
        assert!(m("lib/**.js", "lib/a/b/c.js"));
        assert!(!m("lib/*.js", "lib/a/c.js"));
        assert!(m("lib/*.js", "lib/c.js"));
        assert!(m("/Makefile", "Makefile"));
        assert!(!m("/Makefile", "sub/Makefile"));
        assert!(m("Makefile", "sub/Makefile"));
        assert!(m("file?.txt", "file1.txt"));
        assert!(m("[abc].c", "b.c") && !m("[!abc].c", "b.c") && m("[a-c]x", "bx"));
        assert!(m("v{1..3}.txt", "v2.txt") && !m("v{1..3}.txt", "v4.txt"));
        assert!(m("v{-2..2}", "v-1"));
        assert!(m("a{b,{c,d}}e", "ade"));
        assert!(m("{single}", "{single}"), "no comma: literal braces");
        assert!(m("a\\*b", "a*b") && !m("a\\*b", "axb"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("idecraft-ec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proj/web")).unwrap();
        dir
    }

    #[test]
    fn max_line_length_sets_the_right_margin() {
        let dir = scratch("margin");
        let file = dir.join("proj/web/a.txt");
        let margin = |text: &str| {
            std::fs::write(dir.join("proj").join(FILE_NAME), text).unwrap();
            let config = editorconfig_for(&file);
            (config.right_margin(), config.problems.len())
        };
        assert_eq!(
            margin("root = true\n[*]\nmax_line_length = 20\n"),
            (Some(Some(20)), 0)
        );
        assert_eq!(
            margin("root = true\n[*]\nmax_line_length = off\n"),
            (Some(None), 0)
        );
        assert_eq!(
            margin("root = true\n[*]\nmax_line_length = OFF\n"),
            (Some(None), 0)
        );
        assert_eq!(margin("root = true\n[*]\nindent_size = 2\n"), (None, 0));
        assert_eq!(
            margin("root = true\n[*]\nmax_line_length = 20\n[*.txt]\nmax_line_length = unset\n"),
            (None, 0)
        );
        let (none, problems) = margin("root = true\n[*]\nmax_line_length = wide\n");
        assert_eq!(
            (none, problems),
            (None, 1),
            "not understood: a problem, Code Style decides"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_change_stamp_follows_created_changed_and_deleted_files() {
        let dir = scratch("stamp");
        let file = dir.join("proj/web/a.txt");
        let before = change_stamp(&file);
        assert_eq!(
            before,
            change_stamp(&file),
            "nothing changed: the same stamp"
        );
        let web = dir.join("proj/web").join(FILE_NAME);
        std::fs::write(&web, "[*]\nindent_size = 2\n").unwrap();
        let created = change_stamp(&file);
        assert_ne!(before, created, "created");
        // Same length, other bytes: still seen.
        std::fs::write(&web, "[*]\nindent_size = 6\n").unwrap();
        let changed = change_stamp(&file);
        assert_ne!(created, changed, "changed");
        std::fs::write(dir.join(FILE_NAME), "[*]\ntab_width = 3\n").unwrap();
        assert_ne!(changed, change_stamp(&file), "a file further up counts too");
        std::fs::remove_file(&web).unwrap();
        std::fs::remove_file(dir.join(FILE_NAME)).unwrap();
        assert_eq!(before, change_stamp(&file), "deleted again: as before");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nearer_files_and_later_sections_win_until_root() {
        let dir = scratch("chain");
        // Above the root file: never read.
        std::fs::write(
            dir.join(FILE_NAME),
            "[*]\nindent_size = 8\ncharset = latin1\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("proj").join(FILE_NAME),
            "root = true\n\n[*]\nindent_style = space\nindent_size = 4\nend_of_line = LF\n\
             trim_trailing_whitespace = true\n\n[*.ts]\nindent_size = 2\n\n[Makefile]\nindent_style = tab\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("proj/web").join(FILE_NAME),
            "# web\n[*.ts]\nindent_style = tab\ntab_width = 3\ntrim_trailing_whitespace = unset\n",
        )
        .unwrap();
        let ts = editorconfig_for(&dir.join("proj/web/app.ts"));
        assert_eq!(ts.files.len(), 2);
        assert_eq!(ts.indent_style(), Some(IndentStyle::Tab));
        assert_eq!(ts.indent_unit().as_deref(), Some("\t"));
        assert_eq!(ts.indent_size(), Some(2));
        assert_eq!(ts.tab_width(), Some(3));
        assert_eq!(ts.line_break(), Some("\n"));
        assert_eq!(ts.trim_trailing_whitespace(), None, "unset");
        assert_eq!(ts.charset(), None, "above the root");
        let s = &ts.properties["indent_style"];
        assert_eq!((s.file.ends_with("web/.editorconfig"), s.line), (true, 3));
        let rs = editorconfig_for(&dir.join("proj/src/main.rs"));
        assert_eq!(rs.indent_unit().as_deref(), Some("    "));
        assert_eq!(rs.tab_width(), Some(4), "indent_size gives the tab width");
        let mk = editorconfig_for(&dir.join("proj/Makefile"));
        assert_eq!(mk.indent_unit().as_deref(), Some("\t"));
        assert!(editorconfig_for(&dir.join("elsewhere/x")).properties.len() <= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_values_are_reported() {
        let dir = scratch("bad");
        std::fs::write(
            dir.join("proj").join(FILE_NAME),
            "root=true\n[*]\nindent_style = tabs\ninsert_final_newline = yes\n",
        )
        .unwrap();
        let c = editorconfig_for(&dir.join("proj/a.txt"));
        assert_eq!(c.indent_style(), None);
        assert_eq!(c.problems.len(), 2, "{:?}", c.problems);
        assert!(c.problems[0].contains(":3: `indent_style = tabs` is not understood"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn apply(text: &str, edits: &[(std::ops::Range<usize>, String)]) -> String {
        let mut out = text.to_owned();
        for (r, t) in edits.iter().rev() {
            out.replace_range(r.clone(), t);
        }
        out
    }

    fn config(pairs: &[(&str, &str)]) -> EditorConfig {
        let mut c = EditorConfig::default();
        for (k, v) in pairs {
            c.properties.insert(
                (*k).into(),
                Setting {
                    value: (*v).into(),
                    file: PathBuf::new(),
                    line: 1,
                },
            );
        }
        c
    }

    #[test]
    fn saving_trims_converts_and_handles_the_final_break() {
        let crlf = config(&[
            ("end_of_line", "crlf"),
            ("trim_trailing_whitespace", "true"),
            ("insert_final_newline", "true"),
        ]);
        let text = "a  \nb\r\nc\t";
        assert_eq!(
            apply(text, &save_edits(text, &crlf, false, false, None)),
            "a\r\nb\r\nc\r\n"
        );
        // The caret line keeps its spaces.
        assert_eq!(
            apply(text, &save_edits(text, &crlf, false, false, Some(0))),
            "a  \r\nb\r\nc\r\n"
        );
        let lf = config(&[("end_of_line", "lf"), ("insert_final_newline", "false")]);
        let text = "a\r\nb\r\n\r\n";
        assert_eq!(
            apply(text, &save_edits(text, &lf, false, false, None)),
            "a\nb"
        );
        // EditorConfig overrides the IDE settings, which apply where it says nothing.
        let off = config(&[("trim_trailing_whitespace", "false")]);
        assert_eq!(
            apply("a \n", &save_edits("a \n", &off, true, true, None)),
            "a \n"
        );
        let none = EditorConfig::default();
        assert_eq!(
            apply("a \nb", &save_edits("a \nb", &none, true, true, None)),
            "a\nb\n"
        );
        assert!(save_edits("a \nb", &none, false, false, None).is_empty());
    }
}

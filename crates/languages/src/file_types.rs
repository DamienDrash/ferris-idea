//! What language a file has when its name does not say it or the user says otherwise
//! (`SPEC-0004` v5 behaviours 9–12): the hashbang line of a script, the file name patterns and
//! per-file overrides a project keeps in `.ferris-idea/file-types.toml`.
//!
//! The projects whose window or MCP server is open register here, so every place that asks
//! [`crate::LanguageId::detect`] for a file's language follows them. A project's file is read again
//! when it changed on disk, at most once a second (behaviour 12).

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::LanguageId;

/// Where a project keeps its associations, relative to the project folder (behaviour 10).
pub const FILE: &str = ".ferris-idea/file-types.toml";

/// How often a registered project's file is looked at for changes on disk (behaviour 12).
const RECHECK: Duration = Duration::from_secs(1);

/// The most of a file's first line looked at for its hashbang (behaviour 9).
const HASHBANG_BYTES: usize = 256;

/// The most of a file's start and of its end read for an editor modeline (v9 behaviour 18).
const MODELINE_BYTES: u64 = 1024;

/// How many lines at a file's start and at its end can hold a Vim modeline (v9 behaviour 18).
const MODELINE_LINES: usize = 5;

/// A file type a user can choose: a language or plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// The language.
    Language(LanguageId),
    /// Plain text: no language.
    PlainText,
}

impl Choice {
    /// The id in the file and in MCP: the language's LSP id, or `text`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Language(l) => l.lsp_id(),
            Self::PlainText => "text",
        }
    }

    /// The choice with [`Self::id`] `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        if id == "text" {
            return Some(Self::PlainText);
        }
        LanguageId::from_lsp_id(id).map(Self::Language)
    }

    /// The name lists show.
    pub fn title(self) -> &'static str {
        match self {
            Self::Language(l) => l.title(),
            Self::PlainText => "Plain Text",
        }
    }

    /// The language, `None` for plain text.
    pub fn language(self) -> Option<LanguageId> {
        match self {
            Self::Language(l) => Some(l),
            Self::PlainText => None,
        }
    }

    /// Plain Text, then every language by its name (behaviour 13).
    pub fn all() -> Vec<Self> {
        let mut languages: Vec<LanguageId> = LanguageId::ALL.to_vec();
        languages.sort_by_key(|l| l.title().to_lowercase());
        std::iter::once(Self::PlainText)
            .chain(languages.into_iter().map(Self::Language))
            .collect()
    }

    /// Every id, for messages that list them.
    pub fn ids() -> Vec<&'static str> {
        Self::all().into_iter().map(Self::id).collect()
    }
}

/// What decided a file's language (behaviour 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The project's override of the file.
    Override,
    /// A file name pattern of the project.
    Pattern,
    /// The file's name or extension.
    Name,
    /// The hashbang line.
    Hashbang,
    /// An editor modeline (v9 behaviour 18).
    Modeline,
    /// The markup a file starts with: an XML declaration or an HTML doctype (v10 behaviour 21).
    Prolog,
    /// Nothing: plain text.
    None,
}

impl Source {
    /// The name in MCP.
    pub fn name(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::Pattern => "pattern",
            Self::Name => "name",
            Self::Hashbang => "hashbang",
            Self::Modeline => "modeline",
            Self::Prolog => "prolog",
            Self::None => "none",
        }
    }
}

/// A file's language and what decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// The language, `None` for plain text.
    pub language: Option<LanguageId>,
    /// What decided it.
    pub source: Source,
    /// The pattern that matched, with [`Source::Pattern`].
    pub pattern: Option<String>,
}

/// The associations of one project (behaviours 10, 11).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Associations {
    /// File name patterns and their file types.
    pub patterns: BTreeMap<String, Choice>,
    /// Files by their path relative to the project folder (`/` between parts) and their file
    /// types.
    pub overrides: BTreeMap<String, Choice>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Form {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    patterns: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    overrides: BTreeMap<String, String>,
}

impl Associations {
    /// The associations of the project at `root`; none when it has no file. Entries whose file
    /// type is not known are left out.
    pub fn read(root: &Path) -> Result<Self, String> {
        let at = root.join(FILE);
        let text = match std::fs::read_to_string(&at) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{FILE}: {e}")),
        };
        let form: Form = toml::from_str(&text).map_err(|e| format!("{FILE}: {e}"))?;
        let known = |m: BTreeMap<String, String>| {
            m.into_iter()
                .filter_map(|(k, v)| Choice::from_id(&v).map(|c| (k, c)))
                .collect()
        };
        Ok(Self {
            patterns: known(form.patterns),
            overrides: known(form.overrides),
        })
    }

    /// Writes the associations of the project at `root`, through a temporary file and a rename;
    /// with none the file is removed.
    pub fn write(&self, root: &Path) -> io::Result<()> {
        let at = root.join(FILE);
        if self.patterns.is_empty() && self.overrides.is_empty() {
            return match std::fs::remove_file(&at) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        let ids = |m: &BTreeMap<String, Choice>| {
            m.iter()
                .map(|(k, c)| (k.clone(), c.id().to_owned()))
                .collect()
        };
        let form = Form {
            patterns: ids(&self.patterns),
            overrides: ids(&self.overrides),
        };
        let body = toml::to_string(&form).map_err(io::Error::other)?;
        let text = format!(
            "# File types of this project (Ferris IDEA, SPEC-0004): file name patterns and single files.\n{body}"
        );
        if let Some(dir) = at.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = at.with_extension(format!("toml.{}.tmp", std::process::id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, &at)
    }

    /// The pattern that decides a file named `name` and its file type (behaviour 10): one without
    /// wildcards over one with them, then the longer pattern.
    pub fn pattern_for(&self, name: &str) -> Option<(&str, Choice)> {
        self.patterns
            .iter()
            .filter(|(p, _)| glob_matches(p, name))
            .max_by_key(|(p, _)| (!p.contains(['*', '?']), p.chars().count()))
            .map(|(p, c)| (p.as_str(), *c))
    }

    /// What the associations say of the file `relative` (its path in the project) named `name`.
    fn decide(
        &self,
        relative: Option<&str>,
        name: &str,
    ) -> Option<(Choice, Source, Option<String>)> {
        if let Some(c) = relative.and_then(|r| self.overrides.get(r)) {
            return Some((*c, Source::Override, None));
        }
        self.pattern_for(name)
            .map(|(p, c)| (c, Source::Pattern, Some(p.to_owned())))
    }
}

/// Whether the file name `name` matches `pattern`: `*` any run of characters, `?` one, ignoring
/// case (behaviour 10).
pub fn glob_matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    // Classic wildcard matching with one backtrack point for the last `*`.
    let (mut i, mut j) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = star {
            i = si + 1;
            j = sj + 1;
            star = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    p[i..].iter().all(|&c| c == '*')
}

/// The language of the interpreter a script's first line names (behaviour 9).
pub fn hashbang_language(first_line: &str) -> Option<LanguageId> {
    let rest = first_line.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_owned();
    let mut program = base(words.next()?);
    if program == "env" {
        program = words
            .find(|w| !w.starts_with('-') && !w.contains('='))
            .map(base)?;
    }
    let name = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    use LanguageId as L;
    Some(match name {
        "python" | "pypy" => L::Python,
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" => L::Shell,
        "node" | "nodejs" => L::JavaScript,
        "ts-node" | "tsx" => L::TypeScript,
        "ruby" => L::Ruby,
        "php" => L::Php,
        "lua" | "luajit" => L::Lua,
        "Rscript" => L::R,
        "groovy" => L::Groovy,
        "scala" => L::Scala,
        "swift" => L::Swift,
        _ => return None,
    })
}

/// `path` relative to `root` with `/` between its parts, when it lies inside.
pub fn relative_key(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rest
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let parts = parts?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

// --------------------------------------------------------------------------- the registry

struct Entry {
    /// The project folder as registered and canonical.
    roots: Vec<PathBuf>,
    associations: Associations,
    stamp: Option<SystemTime>,
    checked: Instant,
}

static PROJECTS: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

fn stamp(root: &Path) -> Option<SystemTime> {
    std::fs::metadata(root.join(FILE))
        .and_then(|m| m.modified())
        .ok()
}

fn roots_of(root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![root.to_path_buf()];
    if let Ok(c) = root.canonicalize()
        && c != root
    {
        roots.push(c);
    }
    roots
}

/// Registers the project at `root`, so detection follows its associations (behaviour 12); a
/// project registered before is read again. A file that cannot be read counts as none.
pub fn register(root: &Path) {
    let roots = roots_of(root);
    let associations = Associations::read(root).unwrap_or_default();
    let entry = Entry {
        stamp: stamp(root),
        roots: roots.clone(),
        associations,
        checked: Instant::now(),
    };
    let mut projects = PROJECTS.write().unwrap_or_else(|e| e.into_inner());
    projects.retain(|e| !e.roots.iter().any(|r| roots.contains(r)));
    projects.push(entry);
}

/// The associations of the project at `root` as detection uses them now, registering it first.
pub fn associations(root: &Path) -> Associations {
    let roots = roots_of(root);
    {
        let projects = PROJECTS.read().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = projects
            .iter()
            .find(|e| e.roots.iter().any(|r| roots.contains(r)))
            && e.checked.elapsed() < RECHECK
        {
            return e.associations.clone();
        }
    }
    register(root);
    let projects = PROJECTS.read().unwrap_or_else(|e| e.into_inner());
    projects
        .iter()
        .find(|e| e.roots.iter().any(|r| roots.contains(r)))
        .map(|e| e.associations.clone())
        .unwrap_or_default()
}

/// Changes the associations of the project at `root` with `change`, starting from what its file
/// says now, writes them and registers the result. Returns the associations before and after.
pub fn update(
    root: &Path,
    change: impl FnOnce(&mut Associations),
) -> io::Result<(Associations, Associations)> {
    let before = Associations::read(root).map_err(io::Error::other)?;
    let mut after = before.clone();
    change(&mut after);
    if after != before {
        after.write(root)?;
    }
    register(root);
    Ok((before, after))
}

/// What the registered project holding `path` says of it.
fn project_decision(path: &Path, name: &str) -> Option<(Choice, Source, Option<String>)> {
    let stale = {
        let projects = PROJECTS.read().unwrap_or_else(|e| e.into_inner());
        let mut stale = None;
        for e in projects.iter() {
            let Some(root) = e.roots.iter().find(|r| path.starts_with(r)) else {
                continue;
            };
            if e.checked.elapsed() >= RECHECK {
                stale = Some(root.clone());
                break;
            }
            let relative = relative_key(root, path);
            return e.associations.decide(relative.as_deref(), name);
        }
        stale?
    };
    // Looked at more than a second ago: read again when the file changed on disk.
    {
        let now = stamp(&stale);
        let mut projects = PROJECTS.write().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = projects.iter_mut().find(|e| e.roots.contains(&stale)) {
            if now != e.stamp {
                e.associations = Associations::read(&stale).unwrap_or_default();
                e.stamp = now;
            }
            e.checked = Instant::now();
        }
    }
    let projects = PROJECTS.read().unwrap_or_else(|e| e.into_inner());
    let e = projects.iter().find(|e| e.roots.contains(&stale))?;
    e.associations
        .decide(relative_key(&stale, path).as_deref(), name)
}

/// The language an editor modeline names (v9 behaviour 18): Emacs' `-*- mode: python -*-` or
/// `-*- python -*-`, Vim's `vim: set ft=python :`, `vi:ft=sh` or `ex: syntax=yaml` (also
/// `filetype=` and `syn=`). Modes are compared ignoring case and an Emacs `-mode` suffix.
pub fn modeline_language(line: &str) -> Option<LanguageId> {
    emacs_mode(line)
        .or_else(|| vim_filetype(line))
        .and_then(|m| mode_language(&m))
}

/// The mode between Emacs' two `-*-` marks.
fn emacs_mode(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("-*-")?;
    let (inside, _) = rest.split_once("-*-")?;
    let inside = inside.trim();
    if !inside.contains(':') {
        return (!inside.is_empty()).then(|| inside.to_owned());
    }
    inside.split(';').find_map(|part| {
        let (key, value) = part.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case("mode")
            .then(|| value.trim().to_owned())
    })
}

/// The file type a Vim modeline sets: `vi:`, `vim:` or `ex:` at the line's start or after a
/// blank, then options separated by blanks or `:` (after `set`, ended by `:`).
fn vim_filetype(line: &str) -> Option<String> {
    let at = ["vim:", "vi:", "ex:"].iter().find_map(|marker| {
        line.match_indices(marker)
            .find(|(i, _)| *i == 0 || line[..*i].ends_with(char::is_whitespace))
            .map(|(i, _)| i + marker.len())
    })?;
    let rest = line[at..].trim_start();
    let rest = rest
        .strip_prefix("set ")
        .or_else(|| rest.strip_prefix("se "))
        .unwrap_or(rest);
    rest.split(|c: char| c.is_whitespace() || c == ':')
        .find_map(|option| {
            let (key, value) = option.split_once('=')?;
            matches!(key, "ft" | "filetype" | "syn" | "syntax").then(|| value.trim().to_owned())
        })
        .filter(|v| !v.is_empty())
}

/// The language of an editor's mode or file type name.
fn mode_language(mode: &str) -> Option<LanguageId> {
    let lower = mode.to_ascii_lowercase();
    let mode = lower.strip_suffix("-mode").unwrap_or(&lower);
    use LanguageId as L;
    let alias = match mode {
        "py" | "python3" => Some(L::Python),
        "sh" | "bash" | "zsh" | "shell" | "shell-script" => Some(L::Shell),
        "js" | "js2" => Some(L::JavaScript),
        "ts" => Some(L::TypeScript),
        "c++" => Some(L::Cpp),
        "nxml" => Some(L::Xml),
        "md" | "gfm" => Some(L::Markdown),
        "conf-toml" => Some(L::Toml),
        "less-css" => Some(L::Less),
        "ess-r" => Some(L::R),
        "kt" => Some(L::Kotlin),
        "rs" => Some(L::Rust),
        "yml" => Some(L::Yaml),
        _ => None,
    };
    alias.or_else(|| {
        L::ALL
            .into_iter()
            .find(|l| l.lsp_id() == mode || l.title().eq_ignore_ascii_case(mode))
    })
}

/// The language a file's text says it has (v9 behaviour 18, behaviour 9): an Emacs modeline on
/// its first line (its second after a hashbang line), a Vim modeline on one of its first or last
/// five lines, else its hashbang line. `head` is the start of the file, `tail` its end (empty when
/// the start holds the whole file).
pub fn content_language(head: &str, tail: &str) -> Option<(LanguageId, Source)> {
    let first: Vec<&str> = head.lines().take(MODELINE_LINES).collect();
    let emacs_lines = if first.first().is_some_and(|l| l.starts_with("#!")) {
        2
    } else {
        1
    };
    let emacs = first
        .iter()
        .take(emacs_lines)
        .find_map(|l| emacs_mode(l).and_then(|m| mode_language(&m)));
    let ends = if tail.is_empty() { head } else { tail };
    let last: Vec<&str> = ends.lines().rev().take(MODELINE_LINES).collect();
    let vim = || {
        first
            .iter()
            .chain(last.iter())
            .find_map(|l| vim_filetype(l).and_then(|m| mode_language(&m)))
    };
    if let Some(l) = emacs.or_else(vim) {
        return Some((l, Source::Modeline));
    }
    let line = first.first()?;
    let end = (0..=HASHBANG_BYTES.min(line.len()))
        .rev()
        .find(|i| line.is_char_boundary(*i))
        .unwrap_or(0);
    let line = &line[..end];
    hashbang_language(line)
        .map(|l| (l, Source::Hashbang))
        .or_else(|| prolog_language(head).map(|l| (l, Source::Prolog)))
}

/// The language of markup a file starts with (v10 behaviour 21): its first line that is not empty,
/// after a byte order mark and leading white space, begins with an XML declaration `<?xml` (XML)
/// or, in any case, with `<!DOCTYPE html` or `<html` (HTML). Nothing else counts: a file that
/// merely contains markup further down, or starts with another element, stays plain text.
pub fn prolog_language(head: &str) -> Option<LanguageId> {
    let first = head
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim_start)
        .find(|l| !l.is_empty())?;
    if first.starts_with("<?xml") && first[5..].starts_with(|c: char| c.is_whitespace() || c == '?')
    {
        return Some(LanguageId::Xml);
    }
    let lower: String = first
        .chars()
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    let html =
        |rest: &str| rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == '>');
    if lower.strip_prefix("<!doctype html").is_some_and(html)
        || lower.strip_prefix("<html").is_some_and(html)
    {
        return Some(LanguageId::Html);
    }
    None
}

type ContentCache = HashMap<PathBuf, (Option<SystemTime>, Option<(LanguageId, Source)>)>;
static CONTENTS: Mutex<Option<ContentCache>> = Mutex::new(None);

/// The language the text of the regular file at the absolute `path` names (behaviours 9, 18),
/// kept until its modification time changes.
fn content_of(path: &Path) -> Option<(LanguageId, Source)> {
    if !path.is_absolute() {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta.modified().ok();
    let mut cache = CONTENTS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some((at, found)) = cache.get(path)
        && *at == modified
    {
        return *found;
    }
    let found = read_ends(path, meta.len()).and_then(|(head, tail)| content_language(&head, &tail));
    if cache.len() > 4096 {
        cache.clear();
    }
    cache.insert(path.to_path_buf(), (modified, found));
    found
}

/// The first and the last [`MODELINE_BYTES`] of a file of `len` bytes; the last are empty when
/// the first hold the whole file. A file whose start holds a NUL byte is not text: `None`.
fn read_ends(path: &Path, len: u64) -> Option<(String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    (&mut file)
        .take(MODELINE_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    if head.contains(&0) {
        return None;
    }
    let mut tail = Vec::new();
    if len > MODELINE_BYTES {
        let from = len.saturating_sub(MODELINE_BYTES).max(MODELINE_BYTES);
        file.seek(SeekFrom::Start(from)).ok()?;
        file.take(MODELINE_BYTES).read_to_end(&mut tail).ok()?;
    }
    Some((
        String::from_utf8_lossy(&head).into_owned(),
        String::from_utf8_lossy(&tail).into_owned(),
    ))
}

/// The language of the file at `path` and what decided it (behaviours 9–12, 18, 21): the override
/// of its registered project, a pattern of it, the name, an editor modeline, the hashbang line, the
/// markup a file without extension starts with.
pub fn detect(path: &Path) -> Detected {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if path.is_absolute()
        && let Some((choice, source, pattern)) = project_decision(path, name)
    {
        return Detected {
            language: choice.language(),
            source,
            pattern,
        };
    }
    if let Some(language) = LanguageId::detect_by_name(path) {
        return Detected {
            language: Some(language),
            source: Source::Name,
            pattern: None,
        };
    }
    // The markup a file starts with counts only for a name without extension (v10 behaviour 21):
    // `notes.txt` holding an HTML example stays text.
    let found = content_of(path)
        .filter(|(_, source)| *source != Source::Prolog || Path::new(name).extension().is_none());
    match found {
        Some((language, source)) => Detected {
            language: Some(language),
            source,
            pattern: None,
        },
        None => Detected {
            language: None,
            source: Source::None,
            pattern: None,
        },
    }
}

/// The pattern Associate with File Type saves for a file named `name` (behaviour 13): `*.ext` of
/// its extension, the whole name when it has none.
pub fn pattern_of_name(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => format!("*.{ext}"),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LanguageId as L;

    fn project(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ferris-idea-file-types-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    /// SPEC-0004 AK-5: the interpreters of behaviour 9.
    #[test]
    fn interpreters_of_the_hashbang_line() {
        for (line, language) in [
            ("#!/usr/bin/env python3", Some(L::Python)),
            ("#!/usr/bin/python3.12 -u", Some(L::Python)),
            (
                "#!/usr/bin/env -S PYTHONUNBUFFERED=1 python3 -u",
                Some(L::Python),
            ),
            ("#! /bin/bash -e", Some(L::Shell)),
            ("#!/bin/sh", Some(L::Shell)),
            ("#!/usr/bin/env zsh", Some(L::Shell)),
            ("#!/usr/bin/env node", Some(L::JavaScript)),
            ("#!/usr/bin/env ts-node", Some(L::TypeScript)),
            ("#!/usr/bin/env ruby", Some(L::Ruby)),
            ("#!/usr/bin/php", Some(L::Php)),
            ("#!/usr/bin/env lua5.4", Some(L::Lua)),
            ("#!/usr/bin/env Rscript", Some(L::R)),
            ("#!/usr/bin/env groovy", Some(L::Groovy)),
            ("#!/usr/bin/env perl", None),
            ("#!/usr/bin/env", None),
            ("# not a hashbang", None),
        ] {
            assert_eq!(hashbang_language(line), language, "{line}");
        }
    }

    /// SPEC-0004 AK-5: a script without extension is read; one whose name says its language is
    /// not; a relative path and a folder are never read.
    #[test]
    fn scripts_get_the_language_of_their_hashbang() {
        let root = project("hashbang");
        std::fs::write(root.join("deploy"), "#!/usr/bin/env python3\nprint(1)\n").unwrap();
        std::fs::write(root.join("run.cgi"), "#!/bin/bash\necho\n").unwrap();
        std::fs::write(root.join("tool.rb"), "#!/usr/bin/env python3\n").unwrap();
        std::fs::write(root.join("README"), "Read me\n").unwrap();
        let d = detect(&root.join("deploy"));
        assert_eq!((d.language, d.source), (Some(L::Python), Source::Hashbang));
        assert_eq!(detect(&root.join("run.cgi")).language, Some(L::Shell));
        let d = detect(&root.join("tool.rb"));
        assert_eq!((d.language, d.source), (Some(L::Ruby), Source::Name));
        assert_eq!(detect(&root.join("README")).source, Source::None);
        assert_eq!(detect(Path::new("deploy")).language, None);
        assert_eq!(detect(&root).language, None);
        // A changed first line is seen (the modification time changes).
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(root.join("deploy"), "#!/bin/sh\necho changed\n").unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        let f = std::fs::File::options()
            .write(true)
            .open(root.join("deploy"))
            .unwrap();
        f.set_modified(later).unwrap();
        assert_eq!(detect(&root.join("deploy")).language, Some(L::Shell));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0004 v10 AK-17: the markup a file starts with (behaviour 21), after the hashbang and
    /// modelines, only for files whose name says nothing.
    #[test]
    fn the_prolog_names_xml_and_html() {
        for (head, language) in [
            ("<?xml version=\"1.0\"?>\n<a/>\n", Some(L::Xml)),
            (
                "\u{feff}\n  <?xml version='1.0' encoding='UTF-8'?>\n",
                Some(L::Xml),
            ),
            ("<?xml?>", Some(L::Xml)),
            ("<!DOCTYPE html>\n<html></html>\n", Some(L::Html)),
            (
                "<!doctype HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\">",
                Some(L::Html),
            ),
            ("<html lang=\"en\">", Some(L::Html)),
            ("<HTML>", Some(L::Html)),
            ("<?xml-stylesheet href=\"a.xsl\"?>", None),
            ("<htmlx>", None),
            ("<svg xmlns=\"http://www.w3.org/2000/svg\"/>", None),
            ("text\n<?xml version=\"1.0\"?>", None),
            ("", None),
        ] {
            assert_eq!(prolog_language(head), language, "{head:?}");
        }
        assert_eq!(
            content_language("<?xml version=\"1.0\"?>\n", ""),
            Some((L::Xml, Source::Prolog))
        );
        // A modeline names the language before the markup does.
        assert_eq!(
            content_language("<!DOCTYPE html>\n<!-- vim: set ft=xml: -->\n", ""),
            Some((L::Xml, Source::Modeline))
        );
        let root = project("prolog");
        std::fs::write(root.join("config"), "<?xml version=\"1.0\"?>\n<a/>\n").unwrap();
        std::fs::write(root.join("page"), "<!DOCTYPE html>\n<html></html>\n").unwrap();
        std::fs::write(root.join("page.txt"), "<!DOCTYPE html>\n").unwrap();
        let d = detect(&root.join("config"));
        assert_eq!((d.language, d.source), (Some(L::Xml), Source::Prolog));
        assert_eq!(Source::Prolog.name(), "prolog");
        let d = detect(&root.join("page"));
        assert_eq!((d.language, d.source), (Some(L::Html), Source::Prolog));
        // The name decides first: `.txt` is plain text whatever it holds.
        assert_ne!(detect(&root.join("page.txt")).source, Source::Prolog);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0004 v9 AK-14: the modelines of behaviour 18.
    #[test]
    fn modelines_name_the_language() {
        for (line, language) in [
            ("# -*- mode: python; coding: utf-8 -*-", Some(L::Python)),
            ("/* -*- C++ -*- */", Some(L::Cpp)),
            (";; -*- Mode: Shell-Script -*-", Some(L::Shell)),
            ("-*- js2-mode -*-", Some(L::JavaScript)),
            ("# vim: set ft=yaml ts=2 :", Some(L::Yaml)),
            ("// vim:ft=rust", Some(L::Rust)),
            ("# vi: filetype=sh", Some(L::Shell)),
            ("<!-- vim: syntax=html -->", Some(L::Html)),
            ("# ex: syn=toml", Some(L::Toml)),
            ("# vim: ts=4 sw=4", None),
            ("# vim: set ft=perl :", None),
            ("# envim: ft=python", None),
            ("-*- coding: utf-8 -*-", None),
            ("plain text", None),
        ] {
            assert_eq!(modeline_language(line), language, "{line}");
        }
        // Emacs: the first line, or the second after a hashbang line; it wins over the hashbang.
        let d = |head: &str, tail: &str| content_language(head, tail);
        assert_eq!(
            d("#!/bin/sh\n# -*- mode: python -*-\n", ""),
            Some((L::Python, Source::Modeline))
        );
        assert_eq!(d("one\n# -*- mode: python -*-\n", ""), None);
        // Vim: one of the first or the last five lines.
        let middle = format!("{}# vim: ft=lua\n{}", "x\n".repeat(5), "y\n".repeat(5));
        assert_eq!(d(&middle, ""), None);
        let last = format!("{}# vim: ft=lua\n", "x\n".repeat(20));
        assert_eq!(d(&last, ""), Some((L::Lua, Source::Modeline)));
        assert_eq!(
            d("x\n", "z\n-- vim: ft=lua\n"),
            Some((L::Lua, Source::Modeline))
        );
        // Without a modeline the hashbang line decides.
        assert_eq!(
            d("#!/usr/bin/env node\n", ""),
            Some((L::JavaScript, Source::Hashbang))
        );
    }

    /// SPEC-0004 v9 AK-14: a file without a known name is read at its start and its end; a name
    /// that says its language is not overruled; a file with a NUL byte is not text.
    #[test]
    fn files_with_a_modeline() {
        let root = project("modeline");
        std::fs::write(root.join("settings.conf"), "a: 1\n# vim: set ft=yaml :\n").unwrap();
        let long = format!(
            "{}\n{}\n# vim: ft=python\n",
            "#!/bin/sh",
            "x = 1\n".repeat(2000)
        );
        std::fs::write(root.join("job"), long).unwrap();
        std::fs::write(root.join("x.rb"), "# vim: ft=python\n").unwrap();
        std::fs::write(root.join("blob"), b"\0\0# vim: ft=python\n").unwrap();
        let d = detect(&root.join("settings.conf"));
        assert_eq!((d.language, d.source), (Some(L::Yaml), Source::Modeline));
        assert_eq!(Source::Modeline.name(), "modeline");
        let d = detect(&root.join("job"));
        assert_eq!((d.language, d.source), (Some(L::Python), Source::Modeline));
        assert_eq!(detect(&root.join("x.rb")).language, Some(L::Ruby));
        assert_eq!(detect(&root.join("blob")).source, Source::None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0004 AK-5: file name patterns.
    #[test]
    fn patterns_match_file_names() {
        assert!(glob_matches("*.tpl", "page.TPL"));
        assert!(glob_matches("Brewfile", "brewfile"));
        assert!(glob_matches("*.conf.?", "app.conf.1"));
        assert!(glob_matches("a*b*c", "aXXbYYc"));
        assert!(!glob_matches("*.tpl", "page.tpl.bak"));
        assert!(!glob_matches("a*b", "ac"));
        let mut a = Associations::default();
        a.patterns
            .insert("*.conf".into(), Choice::Language(L::Toml));
        a.patterns
            .insert("*.d.conf".into(), Choice::Language(L::Yaml));
        a.patterns.insert("app.conf".into(), Choice::PlainText);
        assert_eq!(
            a.pattern_for("app.conf"),
            Some(("app.conf", Choice::PlainText))
        );
        assert_eq!(
            a.pattern_for("x.d.conf"),
            Some(("*.d.conf", Choice::Language(L::Yaml)))
        );
        assert_eq!(a.pattern_for("x.conf").map(|p| p.0), Some("*.conf"));
        assert_eq!(a.pattern_for("x.txt"), None);
        assert_eq!(pattern_of_name("page.tpl"), "*.tpl");
        assert_eq!(pattern_of_name("Brewfile"), "Brewfile");
        assert_eq!(pattern_of_name(".envrc"), ".envrc");
    }

    /// SPEC-0004 AK-5: override over pattern over name over hashbang; `text`; the file written and
    /// read; a change on disk seen after a second; a file outside every project untouched.
    #[test]
    fn overrides_and_patterns_of_a_registered_project() {
        let root = project("order");
        let other = project("order-other");
        for dir in [&root, &other] {
            std::fs::create_dir_all(dir.join("conf")).unwrap();
            std::fs::write(dir.join("conf/app.cfg"), "a: 1\n").unwrap();
            std::fs::write(dir.join("main.py"), "print(1)\n").unwrap();
            std::fs::write(dir.join("page.tpl"), "<p>\n").unwrap();
            std::fs::write(dir.join("tool"), "#!/bin/sh\n").unwrap();
        }
        register(&root);
        assert_eq!(detect(&root.join("conf/app.cfg")).source, Source::None);
        let (before, after) = update(&root, |a| {
            a.overrides
                .insert("conf/app.cfg".into(), Choice::Language(L::Yaml));
            a.overrides.insert("main.py".into(), Choice::PlainText);
            a.patterns.insert("*.tpl".into(), Choice::Language(L::Html));
            a.patterns.insert("tool".into(), Choice::Language(L::Ruby));
            a.patterns.insert("*.py".into(), Choice::Language(L::Ruby));
        })
        .unwrap();
        assert_eq!(before, Associations::default());
        assert_eq!(after.overrides.len(), 2);
        let d = detect(&root.join("conf/app.cfg"));
        assert_eq!((d.language, d.source), (Some(L::Yaml), Source::Override));
        let d = detect(&root.join("main.py"));
        assert_eq!((d.language, d.source), (None, Source::Override));
        let d = detect(&root.join("page.tpl"));
        assert_eq!(
            (d.language, d.source, d.pattern.as_deref()),
            (Some(L::Html), Source::Pattern, Some("*.tpl"))
        );
        assert_eq!(detect(&root.join("tool")).language, Some(L::Ruby));
        // The other project knows none of it.
        assert_eq!(detect(&other.join("conf/app.cfg")).language, None);
        assert_eq!(detect(&other.join("main.py")).language, Some(L::Python));
        assert_eq!(detect(&other.join("tool")).language, Some(L::Shell));
        // The file reads back the same.
        let text = std::fs::read_to_string(root.join(FILE)).unwrap();
        assert!(text.contains("\"conf/app.cfg\" = \"yaml\""), "{text}");
        assert!(text.contains("\"main.py\" = \"text\""), "{text}");
        assert_eq!(Associations::read(&root).unwrap(), after);
        // Changed on disk by someone else: seen within a second or so.
        std::fs::write(root.join(FILE), "[overrides]\n\"main.py\" = \"python\"\n").unwrap();
        let f = std::fs::File::options()
            .write(true)
            .open(root.join(FILE))
            .unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while detect(&root.join("page.tpl")).source == Source::Pattern {
            assert!(Instant::now() < end, "the change on disk is seen");
            std::thread::sleep(Duration::from_millis(100));
        }
        let d = detect(&root.join("main.py"));
        assert_eq!((d.language, d.source), (Some(L::Python), Source::Override));
        // Taking everything away removes the file.
        update(&root, |a| {
            a.overrides.clear();
            a.patterns.clear();
        })
        .unwrap();
        assert!(!root.join(FILE).exists());
        assert_eq!(detect(&root.join("main.py")).source, Source::Name);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    /// SPEC-0004 AK-5: ids, titles and the order of the list.
    #[test]
    fn choices_have_ids_and_titles() {
        let all = Choice::all();
        assert_eq!(all[0], Choice::PlainText);
        assert_eq!(all.len(), LanguageId::ALL.len() + 1);
        for c in &all {
            assert_eq!(Choice::from_id(c.id()), Some(*c));
            assert!(!c.title().is_empty());
        }
        assert_eq!(Choice::from_id("cobol"), None);
        assert_eq!(Choice::Language(L::Shell).title(), "Shell Script");
        let titles: Vec<String> = all[1..].iter().map(|c| c.title().to_lowercase()).collect();
        let mut sorted = titles.clone();
        sorted.sort();
        assert_eq!(titles, sorted);
        assert_eq!(
            relative_key(Path::new("/p"), Path::new("/p/a/b.txt")).as_deref(),
            Some("a/b.txt")
        );
        assert_eq!(relative_key(Path::new("/p"), Path::new("/q/a")), None);
    }
}

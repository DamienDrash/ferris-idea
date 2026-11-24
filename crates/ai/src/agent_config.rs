//! Shared agent configuration (`SPEC-0346`, ROADMAP M19.8): the rule, skill and hook files that
//! coding agents read from a project, discovered at a fixed list of paths, read without following
//! anything they say, merged into one view with the files and lines of every item, and rendered
//! for any agent's format. Everything read is untrusted project data: nothing here acts on a rule,
//! runs a hook or copies a section with a secret-looking value. Free of the UI toolkit and of the
//! MCP server; the tools `agent_config` and `agent_config_export` answer from it.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::context::mask_secrets;

/// Largest file that is read (behaviour 4).
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
/// Most bytes read in all (behaviour 4).
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024;
/// Most files read (behaviour 4).
pub const MAX_FILES: usize = 200;
/// Most entries of one folder looked at (behaviour 4).
pub const MAX_DIR_ENTRIES: usize = 500;
/// The marker of a file written by an export (behaviour 6).
pub const GENERATED_MARKER: &str = "ferris-idea:agent-config generated";
/// Default and largest page of the view (behaviour 13).
pub const LIMIT_DEFAULT: usize = 100;
/// Largest page of the view.
pub const LIMIT_MAX: usize = 500;

/// An agent whose files Ferris IDEA knows (behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    /// The tool-neutral `AGENTS.md` and `.agents/skills`.
    AgentsMd,
    /// Claude Code.
    Claude,
    /// Gemini CLI.
    Gemini,
    /// GitHub Copilot.
    Copilot,
    /// Cursor.
    Cursor,
    /// Windsurf.
    Windsurf,
    /// Cline.
    Cline,
    /// Junie.
    Junie,
}

impl Agent {
    /// Every agent, in the order of behaviour 2.
    pub const ALL: [Agent; 8] = [
        Agent::AgentsMd,
        Agent::Claude,
        Agent::Gemini,
        Agent::Copilot,
        Agent::Cursor,
        Agent::Windsurf,
        Agent::Cline,
        Agent::Junie,
    ];

    /// The name in tool arguments and answers.
    pub fn name(self) -> &'static str {
        match self {
            Agent::AgentsMd => "agents_md",
            Agent::Claude => "claude",
            Agent::Gemini => "gemini",
            Agent::Copilot => "copilot",
            Agent::Cursor => "cursor",
            Agent::Windsurf => "windsurf",
            Agent::Cline => "cline",
            Agent::Junie => "junie",
        }
    }

    /// The agent of `name`.
    pub fn from_name(name: &str) -> Option<Agent> {
        Agent::ALL.into_iter().find(|a| a.name() == name)
    }

    /// The file an export for this agent writes, relative to the workspace (behaviour 15).
    pub fn target_file(self) -> &'static str {
        match self {
            Agent::AgentsMd => "AGENTS.md",
            Agent::Claude => "CLAUDE.md",
            Agent::Gemini => "GEMINI.md",
            Agent::Copilot => ".github/copilot-instructions.md",
            Agent::Cursor => ".cursor/rules/ferris-idea-shared.mdc",
            Agent::Windsurf => ".windsurf/rules/ferris-idea-shared.md",
            Agent::Cline => ".clinerules/ferris-idea-shared.md",
            Agent::Junie => ".junie/guidelines.md",
        }
    }
}

/// What an item or a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A project rule (instructions in Markdown).
    Rule,
    /// A skill (`SKILL.md`).
    Skill,
    /// A lifecycle hook (a command in a JSON settings file); never executed.
    Hook,
}

impl Kind {
    /// The name in tool arguments and answers.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Skill => "skill",
            Kind::Hook => "hook",
        }
    }

    /// The kind of `name`.
    pub fn from_name(name: &str) -> Option<Kind> {
        [Kind::Rule, Kind::Skill, Kind::Hook]
            .into_iter()
            .find(|k| k.name() == name)
    }
}

/// How a known path is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// One Markdown file of rules.
    RuleFile,
    /// A folder of rule files with this suffix, one level deep.
    RuleDir(&'static str),
    /// One rule file, or a folder of `.md` rule files when it is a folder (Cline).
    RuleFileOrDir,
    /// A folder of skill folders, each with a `SKILL.md`.
    SkillDir,
    /// A JSON file whose `hooks` member is in Claude Code's form.
    ClaudeHooks,
    /// A JSON file whose `hooks` member is in Cursor's form.
    CursorHooks,
}

/// One known path (behaviour 2).
#[derive(Debug, Clone, Copy)]
struct Known {
    agent: Agent,
    path: &'static str,
    shape: Shape,
}

const fn known(agent: Agent, path: &'static str, shape: Shape) -> Known {
    Known { agent, path, shape }
}

/// The fixed list of behaviour 2; a project cannot extend it.
const KNOWN: &[Known] = &[
    known(Agent::AgentsMd, "AGENTS.md", Shape::RuleFile),
    known(Agent::AgentsMd, ".agents/skills", Shape::SkillDir),
    known(Agent::Claude, "CLAUDE.md", Shape::RuleFile),
    known(Agent::Claude, ".claude/CLAUDE.md", Shape::RuleFile),
    known(Agent::Claude, "CLAUDE.local.md", Shape::RuleFile),
    known(Agent::Claude, ".claude/rules", Shape::RuleDir(".md")),
    known(Agent::Claude, ".claude/skills", Shape::SkillDir),
    known(Agent::Claude, ".claude/settings.json", Shape::ClaudeHooks),
    known(
        Agent::Claude,
        ".claude/settings.local.json",
        Shape::ClaudeHooks,
    ),
    known(Agent::Gemini, "GEMINI.md", Shape::RuleFile),
    known(
        Agent::Copilot,
        ".github/copilot-instructions.md",
        Shape::RuleFile,
    ),
    known(
        Agent::Copilot,
        ".github/instructions",
        Shape::RuleDir(".instructions.md"),
    ),
    known(Agent::Cursor, ".cursorrules", Shape::RuleFile),
    known(Agent::Cursor, ".cursor/rules", Shape::RuleDir(".mdc")),
    known(Agent::Cursor, ".cursor/hooks.json", Shape::CursorHooks),
    known(Agent::Windsurf, ".windsurfrules", Shape::RuleFile),
    known(Agent::Windsurf, ".windsurf/rules", Shape::RuleDir(".md")),
    known(Agent::Cline, ".clinerules", Shape::RuleFileOrDir),
    known(Agent::Junie, ".junie/guidelines.md", Shape::RuleFile),
];

/// The known paths of every agent, for the answer of `agent_config`: (agent, path, kind).
pub fn known_paths() -> Vec<(&'static str, &'static str, &'static str)> {
    KNOWN
        .iter()
        .map(|k| {
            let kind = match k.shape {
                Shape::RuleFile | Shape::RuleDir(_) | Shape::RuleFileOrDir => Kind::Rule,
                Shape::SkillDir => Kind::Skill,
                Shape::ClaudeHooks | Shape::CursorHooks => Kind::Hook,
            };
            (k.agent.name(), k.path, kind.name())
        })
        .collect()
}

/// One file looked at (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// Workspace-relative path with `/`.
    pub path: String,
    /// The agent whose file it is.
    pub agent: Agent,
    /// What it holds.
    pub kind: Kind,
    /// Its size in bytes, when known.
    pub bytes: Option<u64>,
    /// The first 16 hex digits of the SHA-256 of its bytes, when it was read.
    pub digest: Option<String>,
    /// Why it was not read, or not fully.
    pub problem: Option<String>,
    /// Whether it was written by an export (behaviour 6); it contributes no items.
    pub generated: bool,
    /// `@path` imports it names; never followed (behaviour 8).
    pub imports: Vec<String>,
}

/// Where an item was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Origin {
    /// Workspace-relative path.
    pub path: String,
    /// The agent whose file it is.
    pub agent: Agent,
    /// First and last line in the file (1-based).
    pub lines: [usize; 2],
}

/// One item of the merged view (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// First 12 hex digits of SHA-256 over kind and normalised text.
    pub id: String,
    /// What it is.
    pub kind: Kind,
    /// The heading, the skill's name, or the hook's event and matcher.
    pub title: String,
    /// The text; empty when the item is rejected.
    pub text: String,
    /// The scope from `globs` or `applyTo`.
    pub applies_to: Option<String>,
    /// A skill's or rule file's `description`.
    pub description: Option<String>,
    /// A hook's event.
    pub event: Option<String>,
    /// A hook's matcher.
    pub matcher: Option<String>,
    /// Every file and lines it was found in.
    pub sources: Vec<Origin>,
    /// Why its text is withheld (behaviour 11).
    pub rejected: Option<String>,
}

/// The merged view of a workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct View {
    /// Every file looked at, in the order of behaviour 2.
    pub sources: Vec<Source>,
    /// The items, merged.
    pub items: Vec<Item>,
}

impl View {
    /// The problems of the sources, as `path: problem`.
    pub fn problems(&self) -> Vec<String> {
        self.sources
            .iter()
            .filter_map(|s| s.problem.as_ref().map(|p| format!("{}: {p}", s.path)))
            .collect()
    }
}

/// An item before merging.
struct Raw {
    kind: Kind,
    title: String,
    text: String,
    applies_to: Option<String>,
    description: Option<String>,
    event: Option<String>,
    matcher: Option<String>,
    lines: [usize; 2],
}

/// The budget of one discovery (behaviour 4).
struct Budget {
    files: usize,
    bytes: u64,
}

/// Discovers and reads the known files of the workspace `root` (behaviours 1 to 12); with
/// `from`, only that agent's files.
pub fn discover(root: &Path, from: Option<Agent>) -> View {
    let Ok(canonical) = root.canonicalize() else {
        return View::default();
    };
    let mut budget = Budget { files: 0, bytes: 0 };
    let mut sources = Vec::new();
    let mut found: Vec<(Origin, Raw)> = Vec::new();
    for k in KNOWN {
        if from.is_some_and(|a| a != k.agent) {
            continue;
        }
        let path = root.join(k.path);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let Some(resolved) = resolve(&canonical, &path, k, k.path, &mut sources) else {
            continue;
        };
        match k.shape {
            Shape::RuleFile => read_one(
                k,
                k.path,
                &resolved,
                Kind::Rule,
                &mut budget,
                &mut sources,
                &mut found,
            ),
            Shape::ClaudeHooks | Shape::CursorHooks => read_one(
                k,
                k.path,
                &resolved,
                Kind::Hook,
                &mut budget,
                &mut sources,
                &mut found,
            ),
            Shape::RuleFileOrDir if resolved.is_file() => read_one(
                k,
                k.path,
                &resolved,
                Kind::Rule,
                &mut budget,
                &mut sources,
                &mut found,
            ),
            Shape::RuleDir(suffix) => read_dir(
                &canonical,
                k,
                &resolved,
                suffix,
                &mut budget,
                &mut sources,
                &mut found,
            ),
            Shape::RuleFileOrDir => read_dir(
                &canonical,
                k,
                &resolved,
                ".md",
                &mut budget,
                &mut sources,
                &mut found,
            ),
            Shape::SkillDir => read_skills(
                &canonical,
                k,
                &resolved,
                &mut budget,
                &mut sources,
                &mut found,
            ),
        }
    }
    View {
        sources,
        items: merge(found),
    }
}

/// `path` resolved, when it stays inside `canonical` and has the expected type; otherwise a
/// source with the problem is added (behaviour 3).
fn resolve(
    canonical: &Path,
    path: &Path,
    k: &Known,
    rel: &str,
    sources: &mut Vec<Source>,
) -> Option<PathBuf> {
    let kind = match k.shape {
        Shape::SkillDir => Kind::Skill,
        Shape::ClaudeHooks | Shape::CursorHooks => Kind::Hook,
        _ => Kind::Rule,
    };
    let problem = |p: &str, sources: &mut Vec<Source>| {
        sources.push(Source {
            path: rel.to_owned(),
            agent: k.agent,
            kind,
            bytes: None,
            digest: None,
            problem: Some(p.to_owned()),
            generated: false,
            imports: Vec::new(),
        });
    };
    let Ok(resolved) = path.canonicalize() else {
        problem("cannot be resolved (a broken link?); not read", sources);
        return None;
    };
    if !resolved.starts_with(canonical) {
        problem("link leads outside the project; not read", sources);
        return None;
    }
    let wants_dir = matches!(k.shape, Shape::RuleDir(_) | Shape::SkillDir);
    let ok = if matches!(k.shape, Shape::RuleFileOrDir) {
        resolved.is_file() || resolved.is_dir()
    } else if wants_dir {
        resolved.is_dir()
    } else {
        resolved.is_file()
    };
    if !ok {
        let want = if wants_dir {
            "a folder"
        } else {
            "a regular file"
        };
        problem(&format!("is not {want}; not read"), sources);
        return None;
    }
    Some(resolved)
}

/// The sorted entries of a folder, at most [`MAX_DIR_ENTRIES`].
fn entries(dir: &Path) -> (Vec<(String, PathBuf)>, bool) {
    let mut out: Vec<(String, PathBuf)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .take(MAX_DIR_ENTRIES + 1)
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
            .collect(),
        Err(_) => Vec::new(),
    };
    let more = out.len() > MAX_DIR_ENTRIES;
    out.truncate(MAX_DIR_ENTRIES);
    out.sort();
    (out, more)
}

fn read_dir(
    canonical: &Path,
    k: &Known,
    dir: &Path,
    suffix: &str,
    budget: &mut Budget,
    sources: &mut Vec<Source>,
    found: &mut Vec<(Origin, Raw)>,
) {
    let (list, more) = entries(dir);
    if more {
        sources.push(problem_source(
            k.path,
            k.agent,
            Kind::Rule,
            &format!("more than {MAX_DIR_ENTRIES} entries; only the first were looked at"),
        ));
    }
    for (name, path) in list {
        if !name.ends_with(suffix) {
            continue;
        }
        let rel = format!("{}/{name}", k.path);
        let Some(resolved) = resolve(canonical, &path, &file_shape(k), &rel, sources) else {
            continue;
        };
        read_one(k, &rel, &resolved, Kind::Rule, budget, sources, found);
    }
}

fn read_skills(
    canonical: &Path,
    k: &Known,
    dir: &Path,
    budget: &mut Budget,
    sources: &mut Vec<Source>,
    found: &mut Vec<(Origin, Raw)>,
) {
    let (list, more) = entries(dir);
    if more {
        sources.push(problem_source(
            k.path,
            k.agent,
            Kind::Skill,
            &format!("more than {MAX_DIR_ENTRIES} entries; only the first were looked at"),
        ));
    }
    for (name, path) in list {
        let skill = path.join("SKILL.md");
        if std::fs::symlink_metadata(&skill).is_err() {
            continue;
        }
        let rel = format!("{}/{name}/SKILL.md", k.path);
        let shape = Known {
            agent: k.agent,
            path: k.path,
            shape: Shape::RuleFile,
        };
        let before = sources.len();
        let Some(resolved) = resolve(canonical, &skill, &shape, &rel, sources) else {
            if let Some(s) = sources.get_mut(before) {
                s.kind = Kind::Skill;
            }
            continue;
        };
        read_one(k, &rel, &resolved, Kind::Skill, budget, sources, found);
        if let Some((_, raw)) = found.last_mut()
            && raw.kind == Kind::Skill
            && raw.title.is_empty()
        {
            raw.title = name;
        }
    }
}

/// The shape of one file inside a folder of `k`.
fn file_shape(k: &Known) -> Known {
    Known {
        agent: k.agent,
        path: k.path,
        shape: Shape::RuleFile,
    }
}

fn problem_source(path: &str, agent: Agent, kind: Kind, problem: &str) -> Source {
    Source {
        path: path.to_owned(),
        agent,
        kind,
        bytes: None,
        digest: None,
        problem: Some(problem.to_owned()),
        generated: false,
        imports: Vec::new(),
    }
}

/// Reads one resolved file within the budget and parses it as `kind` (behaviours 4 to 10).
fn read_one(
    k: &Known,
    rel: &str,
    resolved: &Path,
    kind: Kind,
    budget: &mut Budget,
    sources: &mut Vec<Source>,
    found: &mut Vec<(Origin, Raw)>,
) {
    let mut source = problem_source(rel, k.agent, kind, "");
    source.problem = None;
    let size = std::fs::metadata(resolved).map(|m| m.len()).unwrap_or(0);
    source.bytes = Some(size);
    if budget.files >= MAX_FILES || budget.bytes + size.min(MAX_FILE_BYTES) > MAX_TOTAL_BYTES {
        source.problem = Some(format!(
            "the budget of {MAX_FILES} files and {} KiB is used up; not read",
            MAX_TOTAL_BYTES / 1024
        ));
        sources.push(source);
        return;
    }
    if size > MAX_FILE_BYTES {
        source.problem = Some(format!(
            "{size} bytes, more than {} KiB; not read",
            MAX_FILE_BYTES / 1024
        ));
        sources.push(source);
        return;
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(resolved)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes));
    if let Err(e) = read {
        source.problem = Some(format!("cannot be read: {e}"));
        sources.push(source);
        return;
    }
    if bytes.len() as u64 > MAX_FILE_BYTES {
        source.problem = Some(format!(
            "grew past {} KiB while it was read; not read",
            MAX_FILE_BYTES / 1024
        ));
        sources.push(source);
        return;
    }
    budget.files += 1;
    budget.bytes += bytes.len() as u64;
    source.bytes = Some(bytes.len() as u64);
    source.digest = Some(hex(&Sha256::digest(&bytes))[..16].to_owned());
    let Ok(text) = String::from_utf8(bytes) else {
        source.problem = Some("is not valid UTF-8; not read".into());
        source.digest = None;
        sources.push(source);
        return;
    };
    if is_generated(&text) {
        source.generated = true;
        sources.push(source);
        return;
    }
    let origin = |lines: [usize; 2]| Origin {
        path: rel.to_owned(),
        agent: k.agent,
        lines,
    };
    match kind {
        Kind::Rule => {
            let (raws, imports) = parse_rules(&text, file_name(rel));
            source.imports = imports;
            found.extend(raws.into_iter().map(|r| (origin(r.lines), r)));
        }
        Kind::Skill => {
            if let Some(r) = parse_skill(&text) {
                found.push((origin(r.lines), r));
            }
        }
        Kind::Hook => match parse_hooks(&text, k.shape) {
            Ok(raws) => found.extend(raws.into_iter().map(|r| (origin(r.lines), r))),
            Err(p) => source.problem = Some(p),
        },
    }
    sources.push(source);
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Whether `text` holds the marker of behaviour 6 in its first ten lines.
pub fn is_generated(text: &str) -> bool {
    text.lines().take(10).any(|l| l.contains(GENERATED_MARKER))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Front matter: the `key: value` lines between a first `---` and the next, and the line where
/// the body starts (0-based).
fn front_matter(lines: &[&str]) -> (Vec<(String, String)>, usize) {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return (Vec::new(), 0);
    }
    let Some(end) = lines.iter().skip(1).position(|l| l.trim_end() == "---") else {
        return (Vec::new(), 0);
    };
    let fields = lines[1..=end]
        .iter()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
            Some((k.trim().to_owned(), v.to_owned()))
        })
        .collect();
    (fields, end + 2)
}

fn field(fields: &[(String, String)], name: &str) -> Option<String> {
    fields
        .iter()
        .find(|(k, v)| k == name && !v.is_empty())
        .map(|(_, v)| v.clone())
}

/// The level and title of an ATX heading.
fn heading(line: &str) -> Option<String> {
    let t = line.trim_start();
    if line.len() - t.len() > 3 {
        return None;
    }
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &t[hashes..];
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim().to_owned())
}

/// Whether `line` opens or closes a fenced code block.
fn fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// An `@path` import line (behaviour 8).
fn import(line: &str) -> Option<String> {
    let t = line.trim();
    let rest = t.strip_prefix('@')?;
    (!rest.is_empty() && !rest.contains(char::is_whitespace) && rest.contains(['.', '/']))
        .then(|| rest.to_owned())
}

/// Sections of a rule file (behaviour 7) and its imports (behaviour 8).
fn parse_rules(text: &str, name: &str) -> (Vec<Raw>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let (fields, body) = front_matter(&lines);
    let applies_to = field(&fields, "globs").or_else(|| field(&fields, "applyTo"));
    let description = field(&fields, "description");
    let mut out = Vec::new();
    let mut imports = Vec::new();
    let mut title = name.to_owned();
    let mut start = body;
    let mut text_lines: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let mut flush = |title: &str, start: usize, end: usize, text_lines: &mut Vec<&str>| {
        let t = text_lines.join("\n").trim().to_owned();
        text_lines.clear();
        if !t.is_empty() {
            out.push(Raw {
                kind: Kind::Rule,
                title: title.to_owned(),
                text: t,
                applies_to: applies_to.clone(),
                description: description.clone(),
                event: None,
                matcher: None,
                lines: [start + 1, end.max(start) + 1],
            });
        }
    };
    for (i, line) in lines.iter().enumerate().skip(body) {
        if fence(line) {
            in_fence = !in_fence;
        }
        if !in_fence && let Some(h) = heading(line) {
            let end = i.saturating_sub(1);
            flush(&title, start, end, &mut text_lines);
            title = if h.is_empty() { name.to_owned() } else { h };
            start = i;
            continue;
        }
        if !in_fence && let Some(path) = import(line) {
            imports.push(path);
            continue;
        }
        text_lines.push(line);
    }
    flush(
        &title,
        start,
        lines.len().saturating_sub(1),
        &mut text_lines,
    );
    (out, imports)
}

/// A skill (behaviour 9); its title is empty when the front matter names none.
fn parse_skill(text: &str) -> Option<Raw> {
    let lines: Vec<&str> = text.lines().collect();
    let (fields, body) = front_matter(&lines);
    let t = lines.get(body..).unwrap_or_default().join("\n");
    let t = t.trim();
    let description = field(&fields, "description");
    if t.is_empty() && description.is_none() {
        return None;
    }
    Some(Raw {
        kind: Kind::Skill,
        title: field(&fields, "name").unwrap_or_default(),
        text: t.to_owned(),
        applies_to: None,
        description,
        event: None,
        matcher: None,
        lines: [1, lines.len().max(1)],
    })
}

/// The hooks of a JSON settings file (behaviour 10); never executed.
fn parse_hooks(text: &str, shape: Shape) -> Result<Vec<Raw>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("is not valid JSON: {e}"))?;
    let Some(hooks) = v.get("hooks") else {
        return Ok(Vec::new());
    };
    let bad = || "its `hooks` member has an unknown shape; nothing of it was read".to_owned();
    let events = hooks.as_object().ok_or_else(bad)?;
    let line = |needle: &str| {
        text.find(needle)
            .map(|at| text[..at].matches('\n').count() + 1)
            .unwrap_or(1)
    };
    let mut out = Vec::new();
    for (event, list) in events {
        let list = list.as_array().ok_or_else(bad)?;
        for entry in list {
            let (matcher, commands): (Option<String>, Vec<&Value>) = match shape {
                Shape::ClaudeHooks => {
                    let matcher = entry
                        .get("matcher")
                        .and_then(Value::as_str)
                        .filter(|m| !m.is_empty())
                        .map(str::to_owned);
                    let inner = entry
                        .get("hooks")
                        .and_then(Value::as_array)
                        .ok_or_else(bad)?;
                    (matcher, inner.iter().collect())
                }
                _ => (None, vec![entry]),
            };
            for c in commands {
                let command = c.get("command").and_then(Value::as_str).ok_or_else(bad)?;
                let title = match &matcher {
                    Some(m) => format!("{event} {m}"),
                    None => event.clone(),
                };
                let at = line(&serde_json::to_string(command).unwrap_or_default());
                out.push(Raw {
                    kind: Kind::Hook,
                    title,
                    text: command.to_owned(),
                    applies_to: None,
                    description: None,
                    event: Some(event.clone()),
                    matcher: matcher.clone(),
                    lines: [at, at],
                });
            }
        }
    }
    Ok(out)
}

/// The text used to find the same item in two files: lines trimmed, empty lines dropped.
fn normalised(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Merges equal items and rejects those with secret-looking values (behaviours 11 and 12).
fn merge(found: Vec<(Origin, Raw)>) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for (origin, raw) in found {
        let norm = normalised(&raw.text);
        let id = hex(&Sha256::digest(format!("{}\n{norm}", raw.kind.name())))[..12].to_owned();
        if let Some(existing) = items.iter_mut().find(|i| i.id == id) {
            existing.sources.push(origin);
            continue;
        }
        let hits = mask_secrets(&raw.title).1
            + mask_secrets(&raw.text).1
            + raw.description.as_deref().map_or(0, |d| mask_secrets(d).1);
        let rejected = (hits > 0).then(|| {
            format!(
                "{hits} secret-looking value{}; the text is withheld and never exported",
                if hits == 1 { "" } else { "s" }
            )
        });
        let (title, description) = if hits > 0 {
            (mask_secrets(&raw.title).0, None)
        } else {
            (raw.title, raw.description)
        };
        items.push(Item {
            id,
            kind: raw.kind,
            title,
            text: if hits > 0 { String::new() } else { raw.text },
            applies_to: raw.applies_to,
            description,
            event: raw.event,
            matcher: raw.matcher,
            sources: vec![origin],
            rejected,
        });
    }
    items
}

/// What an export renders (behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rendered {
    /// The target agent.
    pub target: Agent,
    /// The file it writes, relative to the workspace.
    pub file: &'static str,
    /// The whole content.
    pub content: String,
    /// Ids of the rules rendered.
    pub items: Vec<String>,
    /// Ids of the skills listed.
    pub skills: Vec<String>,
    /// Ids and reasons of the rejected items left out.
    pub rejected: Vec<(String, String)>,
    /// Ids of the hooks left out (behaviour 15).
    pub hooks_not_exported: Vec<String>,
}

/// Why hooks are never rendered.
pub const HOOKS_NOT_EXPORTED: &str = "hooks are never exported: writing a hook into another agent's settings makes that agent run it (SPEC-0346, out of scope)";

/// Renders `view` for `target` (behaviour 15); deterministic.
pub fn render(view: &View, target: Agent) -> Rendered {
    let mut out = String::new();
    if target == Agent::Cursor {
        out.push_str(
            "---\ndescription: Project rules shared by Ferris IDEA\nalwaysApply: true\n---\n",
        );
    }
    out.push_str(&format!(
        "<!-- {GENERATED_MARKER} (SPEC-0346): edit the sources listed below and export again; changes here are replaced -->\n\n"
    ));
    out.push_str("# Project rules\n\n");
    let mut paths: Vec<&str> = Vec::new();
    for i in view
        .items
        .iter()
        .filter(|i| i.rejected.is_none() && i.kind != Kind::Hook)
    {
        for s in &i.sources {
            if !paths.contains(&s.path.as_str()) {
                paths.push(&s.path);
            }
        }
    }
    if paths.is_empty() {
        out.push_str("No project rules were found.\n");
    } else {
        out.push_str(&format!(
            "Merged from: {}.\n",
            paths
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut r = Rendered {
        target,
        file: target.target_file(),
        content: String::new(),
        items: Vec::new(),
        skills: Vec::new(),
        rejected: Vec::new(),
        hooks_not_exported: Vec::new(),
    };
    for i in &view.items {
        if let Some(why) = &i.rejected {
            r.rejected.push((i.id.clone(), why.clone()));
            continue;
        }
        match i.kind {
            Kind::Hook => r.hooks_not_exported.push(i.id.clone()),
            Kind::Skill => r.skills.push(i.id.clone()),
            Kind::Rule => {
                r.items.push(i.id.clone());
                out.push_str(&format!("\n## {}\n\n", i.title));
                if let Some(scope) = &i.applies_to {
                    out.push_str(&format!("Applies to: `{scope}`\n\n"));
                }
                out.push_str(&i.text);
                out.push_str(&format!("\n\n_Source: {}_\n", origins(&i.sources)));
            }
        }
    }
    let skills: Vec<&Item> = view
        .items
        .iter()
        .filter(|i| i.kind == Kind::Skill && i.rejected.is_none())
        .collect();
    if !skills.is_empty() {
        out.push_str("\n## Skills\n\n");
        for s in skills {
            let path = s.sources.first().map(|o| o.path.as_str()).unwrap_or("");
            match &s.description {
                Some(d) => out.push_str(&format!("- **{}**: {d} (`{path}`)\n", s.title)),
                None => out.push_str(&format!("- **{}** (`{path}`)\n", s.title)),
            }
        }
    }
    r.content = out;
    r
}

fn origins(sources: &[Origin]) -> String {
    sources
        .iter()
        .map(|o| format!("`{}` lines {}–{}", o.path, o.lines[0], o.lines[1]))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `target` (workspace-relative) stays inside `root` after resolving the links of the
/// file and of every folder above it that exists (behaviour 17).
pub fn target_inside(root: &Path, target: &str) -> Result<PathBuf, String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("the workspace cannot be resolved: {e}"))?;
    let path = root.join(target);
    let mut probe = path.clone();
    loop {
        if std::fs::symlink_metadata(&probe).is_ok() {
            let resolved = probe.canonicalize().map_err(|_| {
                format!("{target}: a link on the way cannot be resolved; nothing was written")
            })?;
            if !resolved.starts_with(&canonical) {
                return Err(format!(
                    "{target}: a link leads outside the project; nothing was written"
                ));
            }
            break;
        }
        if !probe.pop() {
            break;
        }
    }
    if let Ok(m) = std::fs::symlink_metadata(&path) {
        let real = path.canonicalize().map_err(|e| e.to_string())?;
        if !(m.is_file() || (m.file_type().is_symlink() && real.is_file())) {
            return Err(format!(
                "{target} exists and is not a regular file; nothing was written"
            ));
        }
        return Ok(real);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test, removed first.
    fn workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-agent-config-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn paths(view: &View) -> Vec<&str> {
        view.sources.iter().map(|s| s.path.as_str()).collect()
    }

    /// SPEC-0346 AK-1: the known paths, folders one level deep in name order, nothing else,
    /// generated files without items.
    #[test]
    fn discovers_the_known_files_of_every_agent_and_nothing_else() {
        let root = workspace("discover");
        put(&root, "AGENTS.md", "# Build\nRun the tests.\n");
        put(&root, "CLAUDE.md", "# Style\nShort lines.\n");
        put(&root, ".claude/rules/b.md", "Rule b.\n");
        put(&root, ".claude/rules/a.md", "Rule a.\n");
        put(&root, ".claude/rules/notes.txt", "not a rule\n");
        put(&root, ".claude/rules/deep/c.md", "too deep\n");
        put(
            &root,
            ".cursor/rules/x.mdc",
            "---\nglobs: src/**\n---\nUse tabs.\n",
        );
        put(&root, ".clinerules", "Cline rule.\n");
        put(&root, "docs/AGENTS.md", "not at a known path\n");
        put(&root, "README.md", "not a rule file\n");
        put(
            &root,
            "GEMINI.md",
            &format!("<!-- {GENERATED_MARKER} -->\n# Old\nGenerated before.\n"),
        );
        let view = discover(&root, None);
        assert_eq!(
            paths(&view),
            [
                "AGENTS.md",
                "CLAUDE.md",
                ".claude/rules/a.md",
                ".claude/rules/b.md",
                "GEMINI.md",
                ".cursor/rules/x.mdc",
                ".clinerules",
            ]
        );
        let agents = view.sources.iter().find(|s| s.path == "AGENTS.md").unwrap();
        assert_eq!(agents.agent, Agent::AgentsMd);
        assert_eq!(agents.kind, Kind::Rule);
        assert_eq!(agents.bytes, Some(23));
        assert_eq!(agents.digest.as_ref().unwrap().len(), 16);
        let gemini = view.sources.iter().find(|s| s.path == "GEMINI.md").unwrap();
        assert!(gemini.generated);
        assert!(view.items.iter().all(|i| i.title != "Old"));
        let titles: Vec<&str> = view.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Build", "Style", "a.md", "b.md", "x.mdc", ".clinerules"]
        );
        let cursor = view.items.iter().find(|i| i.title == "x.mdc").unwrap();
        assert_eq!(cursor.applies_to.as_deref(), Some("src/**"));
        // `from` reads one agent's files only.
        let claude = discover(&root, Some(Agent::Claude));
        assert_eq!(
            paths(&claude),
            ["CLAUDE.md", ".claude/rules/a.md", ".claude/rules/b.md"]
        );
        // A Cline folder is read as a folder of `.md` files.
        std::fs::remove_file(root.join(".clinerules")).unwrap();
        put(&root, ".clinerules/one.md", "One.\n");
        let cline = discover(&root, Some(Agent::Cline));
        assert_eq!(paths(&cline), [".clinerules/one.md"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0346 AK-2: links to outside, large files, non-UTF-8 and the file budget.
    #[cfg(unix)]
    #[test]
    fn links_outside_large_files_and_the_budget_are_not_read() {
        let root = workspace("limits");
        let outside = workspace("limits-outside");
        put(&outside, "secret.md", "# Outside\nNever read.\n");
        put(&outside, "rules/w.md", "# Outside rule\nNever read.\n");
        std::os::unix::fs::symlink(outside.join("secret.md"), root.join("AGENTS.md")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".windsurf")).unwrap();
        put(&root, "inside.md", "# Inside\nRead through a link.\n");
        std::os::unix::fs::symlink(root.join("inside.md"), root.join("GEMINI.md")).unwrap();
        put(&root, "CLAUDE.md", &"x".repeat(MAX_FILE_BYTES as usize + 1));
        std::fs::write(root.join(".cursorrules"), [0xff, 0xfe, 0x00]).unwrap();
        let view = discover(&root, None);
        let problem = |p: &str| {
            view.sources
                .iter()
                .find(|s| s.path == p)
                .and_then(|s| s.problem.clone())
                .unwrap_or_default()
        };
        assert!(
            problem("AGENTS.md").contains("outside the project"),
            "{view:?}"
        );
        assert!(
            problem(".windsurf/rules").contains("outside the project"),
            "{view:?}"
        );
        assert!(
            problem("CLAUDE.md").contains("more than 256 KiB"),
            "{view:?}"
        );
        assert!(problem(".cursorrules").contains("UTF-8"), "{view:?}");
        assert!(problem("GEMINI.md").is_empty(), "{view:?}");
        let titles: Vec<&str> = view.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Inside"]);
        // More files than the budget: the rest are listed, not read.
        let many = workspace("limits-many");
        for i in 0..(MAX_FILES + 3) {
            put(
                &many,
                &format!(".claude/rules/r{i:03}.md"),
                &format!("Rule {i}.\n"),
            );
        }
        let view = discover(&many, None);
        let unread: Vec<&Source> = view
            .sources
            .iter()
            .filter(|s| s.problem.as_deref().is_some_and(|p| p.contains("budget")))
            .collect();
        assert_eq!(unread.len(), 3);
        assert_eq!(view.items.len(), MAX_FILES);
        for d in [root, outside, many] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// SPEC-0346 AK-3: sections, fences, imports, skills and both hook forms; no hook runs.
    #[test]
    fn parses_sections_imports_skills_and_hooks_without_running_any() {
        let root = workspace("parse");
        put(
            &root,
            "CLAUDE.md",
            "Intro line.\n@AGENTS.md\n\n# Build\nRun `cargo test`.\n```sh\n# not a heading\nmake\n```\n\n## Empty\n## Style\nShort lines.\n",
        );
        put(
            &root,
            ".claude/skills/review/SKILL.md",
            "---\nname: review\ndescription: Reviews a diff\n---\nRead the diff, then comment.\n",
        );
        put(
            &root,
            ".agents/skills/plain/SKILL.md",
            "Do the plain thing.\n",
        );
        let marker = root.join("hook-ran");
        let command = format!("touch {}", marker.display());
        put(
            &root,
            ".claude/settings.json",
            &serde_json::to_string_pretty(&serde_json::json!({
                "permissions": {"allow": ["Bash"]},
                "hooks": {"PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": command}]}]}
            }))
            .unwrap(),
        );
        put(
            &root,
            ".cursor/hooks.json",
            r#"{"version": 1, "hooks": {"afterFileEdit": [{"command": "fmt.sh"}]}}"#,
        );
        put(&root, ".claude/settings.local.json", "{ not json");
        let view = discover(&root, None);
        let claude = view.sources.iter().find(|s| s.path == "CLAUDE.md").unwrap();
        assert_eq!(claude.imports, ["AGENTS.md"]);
        let rules: Vec<(&str, [usize; 2])> = view
            .items
            .iter()
            .filter(|i| i.kind == Kind::Rule)
            .map(|i| (i.title.as_str(), i.sources[0].lines))
            .collect();
        assert_eq!(
            rules,
            [
                ("CLAUDE.md", [1, 3]),
                ("Build", [4, 10]),
                ("Style", [12, 13])
            ]
        );
        let build = view.items.iter().find(|i| i.title == "Build").unwrap();
        assert!(build.text.contains("# not a heading"), "{build:?}");
        let skills: Vec<(&str, Option<&str>)> = view
            .items
            .iter()
            .filter(|i| i.kind == Kind::Skill)
            .map(|i| (i.title.as_str(), i.description.as_deref()))
            .collect();
        assert_eq!(
            skills,
            [("plain", None), ("review", Some("Reviews a diff"))]
        );
        let hooks: Vec<(&str, Option<&str>, &str)> = view
            .items
            .iter()
            .filter(|i| i.kind == Kind::Hook)
            .map(|i| {
                (
                    i.event.as_deref().unwrap(),
                    i.matcher.as_deref(),
                    i.text.as_str(),
                )
            })
            .collect();
        assert_eq!(
            hooks,
            [
                ("PostToolUse", Some("Edit"), command.as_str()),
                ("afterFileEdit", None, "fmt.sh"),
            ]
        );
        let local = view
            .sources
            .iter()
            .find(|s| s.path == ".claude/settings.local.json")
            .unwrap();
        assert!(local.problem.as_deref().unwrap().contains("not valid JSON"));
        assert!(!marker.exists(), "a hook ran");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0346 AK-4: a section with a secret is withheld, its neighbours stay, the export
    /// leaves it out.
    #[test]
    fn sections_with_secrets_are_rejected_and_never_exported() {
        let root = workspace("secrets");
        let token = format!("ghp_{}", "a".repeat(36));
        put(
            &root,
            "AGENTS.md",
            &format!("# Deploy\nUse the token {token} to push.\n\n# Tests\nRun them all.\n"),
        );
        let view = discover(&root, None);
        let deploy = view.items.iter().find(|i| i.title == "Deploy").unwrap();
        assert!(deploy.text.is_empty());
        assert!(
            deploy
                .rejected
                .as_deref()
                .unwrap()
                .starts_with("1 secret-looking value")
        );
        let tests = view.items.iter().find(|i| i.title == "Tests").unwrap();
        assert!(tests.rejected.is_none());
        let r = render(&view, Agent::Claude);
        assert!(!r.content.contains(&token));
        assert!(!r.content.contains("## Deploy"));
        assert!(r.content.contains("## Tests"));
        assert_eq!(r.rejected.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0346 AK-5: one item with two sources and a stable id; the view is deterministic.
    #[test]
    fn equal_sections_merge_with_every_source_and_the_view_is_stable() {
        let root = workspace("merge");
        put(
            &root,
            "AGENTS.md",
            "# Tests\nRun   them all.\n\n# Only here\nA.\n",
        );
        put(&root, "CLAUDE.md", "# Tests\n\n  Run   them all.\n");
        let view = discover(&root, None);
        let tests: Vec<&Item> = view.items.iter().filter(|i| i.title == "Tests").collect();
        assert_eq!(tests.len(), 1);
        let from: Vec<&str> = tests[0].sources.iter().map(|o| o.path.as_str()).collect();
        assert_eq!(from, ["AGENTS.md", "CLAUDE.md"]);
        assert_eq!(tests[0].id.len(), 12);
        assert_eq!(discover(&root, None), view);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0346 AK-6: every target's file and format, hooks not rendered, deterministic.
    #[test]
    fn renders_every_target_deterministically_without_hooks() {
        let root = workspace("render");
        put(
            &root,
            "AGENTS.md",
            "# Tests\nRun them.\n### Detail\nMore.\n",
        );
        put(
            &root,
            ".cursor/rules/r.mdc",
            "---\nglobs: *.rs\n---\nNo unwrap.\n",
        );
        put(
            &root,
            ".claude/skills/review/SKILL.md",
            "---\nname: review\ndescription: Reviews a diff\n---\nBody.\n",
        );
        put(
            &root,
            ".cursor/hooks.json",
            r#"{"hooks": {"stop": [{"command": "notify.sh"}]}}"#,
        );
        let view = discover(&root, None);
        for target in Agent::ALL {
            let r = render(&view, target);
            assert_eq!(r.file, target.target_file());
            assert!(is_generated(&r.content), "{target:?}");
            assert_eq!(render(&view, target), r);
            assert!(!r.content.contains("notify.sh"));
            assert_eq!(r.hooks_not_exported.len(), 1);
            assert!(
                r.content.contains("## Tests\n\nRun them.\n"),
                "{}",
                r.content
            );
            assert!(r.content.contains("## Detail\n\nMore.\n"), "{}", r.content);
            assert!(r.content.contains("Applies to: `*.rs`"));
            assert!(
                r.content
                    .contains("- **review**: Reviews a diff (`.claude/skills/review/SKILL.md`)")
            );
            assert!(
                r.content.contains("_Source: `AGENTS.md` lines 1–2_"),
                "{}",
                r.content
            );
            assert_eq!(
                r.content.starts_with("---\ndescription:"),
                target == Agent::Cursor
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0346 behaviour 17: the target stays inside the workspace.
    #[cfg(unix)]
    #[test]
    fn a_target_behind_a_link_to_outside_is_refused() {
        let root = workspace("target");
        let outside = workspace("target-outside");
        std::os::unix::fs::symlink(&outside, root.join(".cursor")).unwrap();
        assert!(
            target_inside(&root, ".cursor/rules/ferris-idea-shared.mdc")
                .unwrap_err()
                .contains("outside the project")
        );
        std::fs::create_dir_all(root.join("CLAUDE.md")).unwrap();
        assert!(
            target_inside(&root, "CLAUDE.md")
                .unwrap_err()
                .contains("not a regular file")
        );
        assert!(target_inside(&root, "AGENTS.md").is_ok());
        for d in [root, outside] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

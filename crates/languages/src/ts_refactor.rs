//! Extract Interface, Extract Superclass and Pull Members Up for JavaScript and TypeScript
//! (`SPEC-0320` behaviours 12–16): edits planned from the syntax tree, the server asked only for
//! which declaration a name means and for types that are not written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::insight::Range;
use crate::lsp::LspError;
use crate::rename::{Edit, FileEdits};
use crate::ts_structure::{
    Member, MemberKind, Parsed, Session, TypeDecl, hover_signature, is_typescript, parse_file,
    place_of,
};

/// One member a refactoring can choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassMember {
    /// The name.
    pub name: String,
    /// What it is.
    pub kind: MemberKind,
    /// The text of its first line, to show.
    pub label: String,
    /// Whether an interface can declare it.
    pub for_interface: bool,
    /// Whether it can move.
    pub movable: bool,
}

/// What can be chosen in the type at a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassMembers {
    /// The type's name.
    pub class: String,
    /// `interface` rather than `class`.
    pub is_interface: bool,
    /// Its members.
    pub members: Vec<ClassMember>,
}

/// The type at a place of a script file and what can be chosen in it (`class_members`).
pub fn class_members(
    path: &Path,
    text: &str,
    line: u64,
    character: u64,
) -> Result<ClassMembers, String> {
    let parsed = parse(path, text)?;
    let t = type_at(&parsed, line, character)?;
    Ok(ClassMembers {
        class: t.name.clone(),
        is_interface: t.is_interface,
        members: t
            .members
            .iter()
            .map(|m| ClassMember {
                name: m.name.clone(),
                kind: m.kind,
                label: first_line(&text[m.start..m.end]),
                for_interface: m.public_instance(),
                movable: m.movable(),
            })
            .collect(),
    })
}

/// The first code line of a member, without its doc comment.
fn first_line(member: &str) -> String {
    member
        .lines()
        .map(str::trim)
        .find(|l| {
            !l.is_empty()
                && !l.starts_with("/*")
                && !l.starts_with('*')
                && !l.starts_with("//")
                && !l.starts_with('@')
        })
        .unwrap_or_default()
        .trim_end_matches('{')
        .trim()
        .to_owned()
}

fn parse(path: &Path, text: &str) -> Result<Parsed, String> {
    parse_file(path, text).ok_or_else(|| format!("{} is no script file", path.display()))
}

fn type_at(parsed: &Parsed, line: u64, character: u64) -> Result<&TypeDecl, String> {
    parsed
        .type_at(line, character)
        .map(|i| &parsed.types[i])
        .ok_or_else(|| "no class or interface at this place".to_owned())
}

fn line_start(text: &str, byte: usize) -> usize {
    text[..byte].rfind('\n').map_or(0, |n| n + 1)
}

/// The white space of the line of `byte`, or `None` when text stands before it.
fn indent(text: &str, byte: usize) -> Option<&str> {
    let before = &text[line_start(text, byte)..byte];
    before.trim().is_empty().then_some(before)
}

fn range_of(text: &str, start: usize, end: usize) -> Range {
    let (start_line, start_character) = place_of(text, start);
    let (end_line, end_character) = place_of(text, end);
    Range {
        start_line,
        start_character,
        end_line,
        end_character,
    }
}

fn edit(text: &str, start: usize, end: usize, new_text: String) -> Edit {
    Edit {
        range: range_of(text, start, end),
        old_text: text[start..end].to_owned(),
        new_text,
    }
}

fn is_identifier(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "class",
        "interface",
        "function",
        "const",
        "let",
        "var",
        "new",
        "this",
        "super",
        "extends",
        "implements",
        "return",
        "if",
        "else",
        "for",
        "while",
        "import",
        "export",
        "enum",
        "type",
        "default",
        "delete",
        "typeof",
        "void",
    ];
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !RESERVED.contains(&name)
}

fn check_name(parsed: &Parsed, name: &str, reasons: &mut Vec<String>) {
    if !is_identifier(name) {
        reasons.push(format!("`{name}` is not a valid name"));
    } else if parsed.top_level.iter().any(|n| n == name) {
        reasons.push(format!("`{name}` is already declared in this file"));
    }
}

fn chosen<'a>(t: &'a TypeDecl, names: &[String], reasons: &mut Vec<String>) -> Vec<&'a Member> {
    if names.is_empty() {
        reasons.push("no member chosen".to_owned());
    }
    let mut out = Vec::new();
    for name in names {
        let mut found = false;
        for m in t.members.iter().filter(|m| &m.name == name) {
            found = true;
            out.push(m);
        }
        if !found {
            reasons.push(format!("`{}` has no member `{name}`", t.name));
        }
    }
    out.sort_by_key(|m| m.start);
    out
}

fn refuse(reasons: Vec<String>) -> Result<(), String> {
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons.join("; "))
    }
}

fn member_indent(t: &TypeDecl, text: &str, decl: &str) -> String {
    t.members
        .first()
        .and_then(|m| indent(text, m.start))
        .map_or_else(|| format!("{decl}  "), str::to_owned)
}

/// The interface signature of `m`, from its written types or the server's hover (behaviour 12).
fn signature(m: &Member, hovers: &HashMap<String, String>, reasons: &mut Vec<String>) -> String {
    if let Some(s) = &m.signature {
        return s.clone();
    }
    match hovers.get(&m.name) {
        Some(h) if m.readonly => format!("readonly {h}"),
        Some(h) => h.clone(),
        None => {
            reasons.push(format!(
                "the server gave no type for `{}`; add one and try again",
                m.name
            ));
            String::new()
        }
    }
}

/// The signatures of `members`, a getter and setter of one name becoming one writable property.
fn signatures(
    members: &[&Member],
    hovers: &HashMap<String, String>,
    reasons: &mut Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for m in members {
        if seen.contains(&m.name.as_str()) {
            continue;
        }
        seen.push(&m.name);
        let both = m.kind == MemberKind::Getter
            && members
                .iter()
                .any(|o| o.name == m.name && o.kind == MemberKind::Setter);
        let s = signature(m, hovers, reasons);
        out.push(if both {
            s.trim_start_matches("readonly ").to_owned()
        } else {
            s
        });
    }
    out
}

/// Where a new declaration goes before `t`, and its indentation.
fn before_type<'a>(text: &'a str, t: &TypeDecl) -> (usize, &'a str) {
    match indent(text, t.start) {
        Some(i) => (line_start(text, t.start), i),
        None => (t.start, ""),
    }
}

/// `SPEC-0320` behaviour 12.
pub fn plan_extract_interface(
    path: &Path,
    parsed: &Parsed,
    index: usize,
    name: &str,
    names: &[String],
    hovers: &HashMap<String, String>,
) -> Result<Vec<FileEdits>, String> {
    let t = &parsed.types[index];
    let text = &parsed.text;
    let mut reasons = Vec::new();
    if !is_typescript(path) {
        reasons.push("JavaScript has no interfaces".to_owned());
    }
    check_name(parsed, name, &mut reasons);
    let members = chosen(t, names, &mut reasons);
    for m in &members {
        if !m.public_instance() {
            reasons.push(format!("`{}` is not public", m.name));
        }
    }
    uses_type_parameters(t, &members, &mut reasons);
    let sigs = signatures(&members, hovers, &mut reasons);
    refuse(reasons)?;
    let (at, decl) = before_type(text, t);
    let inner = member_indent(t, text, decl);
    let body: String = sigs.iter().map(|s| format!("{inner}{s};\n")).collect();
    let block = format!(
        "{decl}{}interface {name} {{\n{body}{decl}}}\n\n",
        if t.exported { "export " } else { "" }
    );
    let mut edits = vec![edit(text, at, at, block)];
    let (where_, new) = if t.is_interface {
        match t.extends_clause {
            Some((_, end)) => (end, format!(", {name}")),
            None => (t.after_name, format!(" extends {name}")),
        }
    } else {
        match (t.implements_end, t.extends_clause) {
            (Some(end), _) => (end, format!(", {name}")),
            (None, Some((_, end))) => (end, format!(" implements {name}")),
            (None, None) => (t.after_name, format!(" implements {name}")),
        }
    };
    edits.push(edit(text, where_, where_, new));
    Ok(vec![finish(path, edits)])
}

fn finish(path: &Path, mut edits: Vec<Edit>) -> FileEdits {
    edits.sort_by_key(|e| {
        (
            e.range.start_line,
            e.range.start_character,
            e.range.end_line,
            e.range.end_character,
        )
    });
    FileEdits {
        path: path.to_path_buf(),
        edits,
    }
}

fn uses_type_parameters(t: &TypeDecl, members: &[&Member], reasons: &mut Vec<String>) {
    for m in members {
        for p in &t.type_parameters {
            if m.identifiers.iter().any(|i| i == p) {
                reasons.push(format!(
                    "`{}` uses type parameter `{p}` of `{}`",
                    m.name, t.name
                ));
            }
        }
    }
}

/// Reasons a chosen member cannot leave `t`: it uses what stays (behaviour 16).
fn leaves(t: &TypeDecl, members: &[&Member], target: Option<&TypeDecl>, reasons: &mut Vec<String>) {
    for m in members {
        if !m.movable() {
            reasons.push(format!("`{}` cannot move", m.name));
        }
        for used in &m.this_uses {
            let moves = members.iter().any(|x| &x.name == used);
            let stays = t.members.iter().any(|x| &x.name == used)
                || t.members
                    .iter()
                    .any(|x| x.parameter_properties.iter().any(|p| p == used));
            let above = target.is_some_and(|x| x.member(used).is_some());
            if stays && !moves && !above {
                reasons.push(format!("`{}` uses `this.{used}`, which stays", m.name));
            }
        }
    }
    uses_type_parameters(t, members, reasons);
}

/// The text of a moved member, `private` widened when what stays uses it (behaviour 14).
fn moved_text(t: &TypeDecl, text: &str, m: &Member, members: &[&Member]) -> String {
    let body = &text[m.start..m.end];
    let used_by_rest = t
        .members
        .iter()
        .filter(|x| !members.iter().any(|y| y.start == x.start))
        .any(|x| x.this_uses.contains(&m.name));
    match m.private_at {
        Some(at) if used_by_rest => {
            let at = at - m.start;
            format!("{}protected{}", &body[..at], &body[at + "private".len()..])
        }
        _ => body.to_owned(),
    }
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn line_at(text: &str, start: usize) -> &str {
    text[start..].split('\n').next().unwrap_or_default()
}

/// The bytes that go when `m` leaves its class, with its line and one blank line.
fn removal(text: &str, m: &Member) -> (usize, usize) {
    let mut start = if indent(text, m.start).is_some() {
        line_start(text, m.start)
    } else {
        m.start
    };
    let rest = line_at(text, m.end);
    let mut end = if blank(rest) {
        (m.end + rest.len() + 1).min(text.len())
    } else {
        m.end
    };
    let previous = (start > 0).then(|| line_start(text, start - 1));
    let previous_blank = previous.is_some_and(|p| blank(line_at(text, p)));
    let next = line_at(text, end);
    let previous_opens = previous.is_some_and(|p| line_at(text, p).trim_end().ends_with('{'));
    if (previous_blank || previous_opens) && blank(next) && end < text.len() {
        end = (end + next.len() + 1).min(text.len());
    } else if previous_blank && next.trim_start().starts_with('}') {
        start = previous.unwrap_or(start);
    }
    (start, end)
}

/// One edit per run of removed members.
fn removals(text: &str, members: &[&Member]) -> Vec<Edit> {
    let mut ranges: Vec<(usize, usize)> = members.iter().map(|m| removal(text, m)).collect();
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
        .into_iter()
        .map(|(s, e)| edit(text, s, e, String::new()))
        .collect()
}

/// Members one after another; a blank line around anything but a field.
fn join_members(parts: &[(MemberKind, String)]) -> String {
    let mut out = String::new();
    for (i, (kind, text)) in parts.iter().enumerate() {
        if i > 0 {
            let spaced = *kind != MemberKind::Field || parts[i - 1].0 != MemberKind::Field;
            out.push_str(if spaced { "\n\n" } else { "\n" });
        }
        out.push_str(text);
    }
    out
}

/// `SPEC-0320` behaviours 13, 14 and 16.
pub fn plan_extract_superclass(
    path: &Path,
    parsed: &Parsed,
    index: usize,
    name: &str,
    names: &[String],
) -> Result<Vec<FileEdits>, String> {
    let t = &parsed.types[index];
    let text = &parsed.text;
    let mut reasons = Vec::new();
    if t.is_interface {
        reasons.push("an interface cannot have a superclass".to_owned());
    }
    check_name(parsed, name, &mut reasons);
    let members = chosen(t, names, &mut reasons);
    leaves(t, &members, None, &mut reasons);
    if let Some((s, e)) = t.extends_clause {
        for p in &t.type_parameters {
            if crate::actions::has_word(&text[s..e], p) {
                reasons.push(format!("the `extends` clause uses type parameter `{p}`"));
            }
        }
    }
    refuse(reasons)?;
    let (at, decl) = before_type(text, t);
    let inner = member_indent(t, text, decl);
    let parts: Vec<(MemberKind, String)> = members
        .iter()
        .map(|m| {
            (
                m.kind,
                format!("{inner}{}", moved_text(t, text, m, &members)),
            )
        })
        .collect();
    let inherited = t
        .extends_clause
        .map_or(String::new(), |(s, e)| format!(" {}", &text[s..e]));
    let block = format!(
        "{decl}{}{}class {name}{inherited} {{\n{}\n{decl}}}\n\n",
        if t.exported { "export " } else { "" },
        if t.is_abstract { "abstract " } else { "" },
        join_members(&parts)
    );
    let mut edits = vec![edit(text, at, at, block)];
    match t.extends_clause {
        Some((s, e)) => edits.push(edit(text, s, e, format!("extends {name}"))),
        None => {
            edits.push(edit(
                text,
                t.after_name,
                t.after_name,
                format!(" extends {name}"),
            ));
            if let Some(open) = t
                .members
                .iter()
                .find(|m| m.kind == MemberKind::Constructor)
                .and_then(|m| m.body_open)
            {
                edits.push(edit(text, open, open, format!("\n{inner}  super();")));
            }
        }
    }
    edits.extend(removals(text, &members));
    Ok(vec![finish(path, edits)])
}

/// A member's text moved from `from` to `to` indentation; the first line starts unindented.
fn reindent(member: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    for (i, line) in member.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(to);
            out.push_str(line.strip_prefix(from).unwrap_or(line));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The edit that puts `block` at the end of `t`'s body.
fn append_to_body(text: &str, t: &TypeDecl, block: &str, spaced: bool) -> Edit {
    let decl = indent(text, t.start).unwrap_or("");
    match indent(text, t.body_end) {
        Some(_) => {
            let at = line_start(text, t.body_end);
            let gap = if spaced && !t.members.is_empty() {
                "\n"
            } else {
                ""
            };
            edit(text, at, at, format!("{gap}{block}\n"))
        }
        None => edit(text, t.body_end, t.body_end, format!("\n{block}\n{decl}")),
    }
}

/// What the plan of a pull needs of the target.
pub struct PullTarget<'a> {
    /// Its file.
    pub path: &'a Path,
    /// Its file parsed.
    pub parsed: &'a Parsed,
    /// Its index there.
    pub index: usize,
}

/// `SPEC-0320` behaviours 15 and 16.
pub fn plan_pull_up(
    path: &Path,
    parsed: &Parsed,
    index: usize,
    target: &PullTarget,
    names: &[String],
    make_abstract: bool,
    hovers: &HashMap<String, String>,
) -> Result<Vec<FileEdits>, String> {
    let t = &parsed.types[index];
    let text = &parsed.text;
    let above = &target.parsed.types[target.index];
    let mut reasons = Vec::new();
    let members = chosen(t, names, &mut reasons);
    let same_file = target.path == path;
    for m in &members {
        if above.member(&m.name).is_some() {
            reasons.push(format!("`{}` already declares `{}`", above.name, m.name));
        }
        if m.uses_super {
            reasons.push(format!("`{}` uses `super`", m.name));
        }
    }
    if make_abstract && (above.is_interface || !above.is_abstract) {
        reasons.push(format!("`{}` is not an abstract class", above.name));
    }
    let signature_only = above.is_interface || make_abstract;
    if signature_only {
        uses_type_parameters(t, &members, &mut reasons);
        for m in &members {
            if m.is_static {
                reasons.push(format!("`{}` is static", m.name));
            }
            if above.is_interface && !m.public_instance() {
                reasons.push(format!("`{}` is not public", m.name));
            }
            if m.visibility.as_deref() == Some("private") || m.name.starts_with('#') {
                reasons.push(format!("`{}` is private", m.name));
            }
        }
    } else {
        leaves(t, &members, Some(above), &mut reasons);
    }
    if !same_file {
        for m in &members {
            for name in &m.identifiers {
                let here = parsed.top_level.contains(name) && *name != t.name;
                let there = target.parsed.top_level.contains(name);
                if here
                    && !there
                    && !reasons
                        .iter()
                        .any(|r| r.contains(&format!("`{name}` of this file")))
                {
                    reasons.push(format!(
                        "`{}` uses `{name}` of this file, which the target's file does not have",
                        m.name
                    ));
                }
            }
        }
    }
    let sigs = if signature_only {
        signatures(&members, hovers, &mut reasons)
    } else {
        Vec::new()
    };
    refuse(reasons)?;
    let decl = indent(&target.parsed.text, above.start).unwrap_or("");
    let inner = member_indent(above, &target.parsed.text, decl);
    let source_indent = member_indent(t, text, indent(text, t.start).unwrap_or(""));
    let block = if signature_only {
        let visible = |name: &str| {
            members
                .iter()
                .find(|m| m.name == name)
                .and_then(|m| m.visibility.as_deref().filter(|v| *v == "protected"))
        };
        sigs.iter()
            .zip(unique_names(&members))
            .map(|(s, name)| {
                if make_abstract {
                    let prefix = visible(name).map_or(String::new(), |v| format!("{v} "));
                    format!("{inner}{prefix}abstract {s};")
                } else {
                    format!("{inner}{s};")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let parts: Vec<(MemberKind, String)> = members
            .iter()
            .map(|m| {
                let moved = moved_text(t, text, m, &members);
                (
                    m.kind,
                    format!("{inner}{}", reindent(&moved, &source_indent, &inner)),
                )
            })
            .collect();
        join_members(&parts)
    };
    let insert = append_to_body(&target.parsed.text, above, &block, !above.is_interface);
    let removed = if signature_only {
        Vec::new()
    } else {
        removals(text, &members)
    };
    if same_file {
        let mut edits = removed;
        edits.push(insert);
        return Ok(vec![finish(path, edits)]);
    }
    let mut files = vec![finish(target.path, vec![insert])];
    if !removed.is_empty() {
        files.push(finish(path, removed));
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn unique_names<'a>(members: &[&'a Member]) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for m in members {
        if !out.contains(&m.name.as_str()) {
            out.push(&m.name);
        }
    }
    out
}

/// Asks the server for the members whose types are not written.
async fn hovers_of(
    s: &mut Session,
    t: &TypeDecl,
    names: &[String],
) -> Result<HashMap<String, String>, LspError> {
    let mut out = HashMap::new();
    for m in t
        .members
        .iter()
        .filter(|m| names.contains(&m.name) && m.signature.is_none())
    {
        let left = s.left();
        let o = s.server()?;
        let at = o.position(m.line, m.character);
        let v = o.ask_bounded("textDocument/hover", at, left, None).await?;
        let uri = o.uri.clone();
        if let Some(h) = crate::insight::parse_hover(&v, &uri, &mut o.files)
            && let Some(sig) = hover_signature(&h.text, &m.name)
        {
            out.insert(m.name.clone(), sig);
        }
    }
    Ok(out)
}

fn to_message(e: LspError) -> String {
    e.to_string()
}

/// Extract Interface at a place (behaviour 12).
#[allow(clippy::too_many_arguments)]
pub async fn extract_interface_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    name: &str,
    names: &[String],
    wait: Duration,
) -> Result<Vec<FileEdits>, String> {
    let mut s = Session::open(root, file, text, wait)
        .await
        .map_err(to_message)?;
    let result = async {
        let parsed = s.main().ok_or("no script file")?;
        let index = parsed
            .type_at(line, character)
            .ok_or("no class or interface at this place")?;
        let hovers = hovers_of(&mut s, &parsed.types[index], names)
            .await
            .map_err(to_message)?;
        plan_extract_interface(s.file(), &parsed, index, name, names, &hovers)
    }
    .await;
    s.close().await;
    result
}

/// Extract Superclass at a place (behaviour 13); needs no server.
pub fn extract_superclass(
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
    name: &str,
    names: &[String],
) -> Result<Vec<FileEdits>, String> {
    let parsed = parse(file, text)?;
    let index = parsed
        .type_at(line, character)
        .ok_or("no class or interface at this place")?;
    plan_extract_superclass(file, &parsed, index, name, names)
}

/// The direct supertypes in the project of the type at a place (behaviour 15).
pub async fn pull_targets_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Vec<String>, String> {
    let mut s = Session::open(root, file, text, wait)
        .await
        .map_err(to_message)?;
    let result = async {
        let parsed = s.main().ok_or("no script file")?;
        let index = parsed
            .type_at(line, character)
            .ok_or("no class or interface at this place")?;
        let supers = s.direct_project_supers(index).await.map_err(to_message)?;
        let mut out = Vec::new();
        for (p, i) in supers {
            if let Some(f) = s.parsed(&p) {
                out.push(f.types[i].name.clone());
            }
        }
        Ok::<_, String>(out)
    }
    .await;
    s.close().await;
    result
}

/// Pull Members Up at a place (behaviours 15 and 16). `target` names a direct supertype; it can be
/// left out when there is only one.
#[allow(clippy::too_many_arguments)]
pub async fn pull_members_up_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    target: Option<&str>,
    names: &[String],
    make_abstract: bool,
    wait: Duration,
) -> Result<Vec<FileEdits>, String> {
    let mut s = Session::open(root, file, text, wait)
        .await
        .map_err(to_message)?;
    let result = async {
        let parsed = s.main().ok_or("no script file")?;
        let index = parsed
            .type_at(line, character)
            .ok_or("no class or interface at this place")?;
        let supers = s.direct_project_supers(index).await.map_err(to_message)?;
        let mut resolved: Vec<(PathBuf, usize, String)> = Vec::new();
        for (p, i) in supers {
            if let Some(f) = s.parsed(&p) {
                resolved.push((p, i, f.types[i].name.clone()));
            }
        }
        if resolved.is_empty() {
            return Err(format!(
                "`{}` has no supertype in the project",
                parsed.types[index].name
            ));
        }
        let picked = match target {
            Some(wanted) => resolved
                .iter()
                .find(|(_, _, n)| n == wanted)
                .ok_or_else(|| format!("`{wanted}` is not a supertype in the project"))?,
            None if resolved.len() == 1 => &resolved[0],
            None => {
                let names: Vec<_> = resolved.iter().map(|(_, _, n)| n.as_str()).collect();
                return Err(format!("choose a target: {}", names.join(", ")));
            }
        };
        let target_parsed = s.parsed(&picked.0).ok_or("the target cannot be read")?;
        let hovers = hovers_of(&mut s, &parsed.types[index], names)
            .await
            .map_err(to_message)?;
        plan_pull_up(
            s.file(),
            &parsed,
            index,
            &PullTarget {
                path: &picked.0,
                parsed: &target_parsed,
                index: picked.1,
            },
            names,
            make_abstract,
            &hovers,
        )
    }
    .await;
    s.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rename::apply_edits;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    fn applied(text: &str, files: &[FileEdits]) -> String {
        assert_eq!(files.len(), 1);
        apply_edits(text, &files[0].edits).unwrap()
    }

    fn interface(text: &str, name: &str, chosen: &[&str]) -> Result<String, String> {
        let p = Path::new("a.ts");
        let parsed = parse_file(p, text).unwrap();
        plan_extract_interface(p, &parsed, 0, name, &names(chosen), &HashMap::new())
            .map(|f| applied(text, &f))
    }

    /// SPEC-0320 AK-5.
    #[test]
    fn extract_interface_in_all_heritage_shapes() {
        let a = "export class A {\n  m(x: number = 1): void {}\n}\n";
        assert_eq!(
            interface(a, "I", &["m"]).unwrap(),
            "export interface I {\n  m(x?: number): void;\n}\n\nexport class A implements I {\n  m(x: number = 1): void {}\n}\n"
        );
        let b = "class A extends B {\n  m(): void {}\n}\n";
        assert!(
            interface(b, "I", &["m"])
                .unwrap()
                .contains("class A extends B implements I {")
        );
        let c = "class A extends B implements J<K>, L {\n  m(): void {}\n}\n";
        assert!(
            interface(c, "I", &["m"])
                .unwrap()
                .contains("implements J<K>, L, I {")
        );
        let d = "interface A extends B {\n  m(): void;\n}\ninterface C {\n  n: string;\n}\n";
        assert!(
            interface(d, "I", &["m"])
                .unwrap()
                .contains("interface A extends B, I {")
        );
        let e = "interface A {\n  m(): void;\n}\n";
        assert!(
            interface(e, "I", &["m"])
                .unwrap()
                .contains("interface A extends I {")
        );
    }

    /// SPEC-0320 behaviour 12: getter and setter become one writable property.
    #[test]
    fn getter_and_setter_become_one_property() {
        let a = "class A {\n  get v(): string { return ''; }\n  set v(x: string) {}\n  get r(): number { return 1; }\n}\n";
        let out = interface(a, "I", &["v", "r"]).unwrap();
        assert!(
            out.contains("  v: string;\n  readonly r: number;\n}"),
            "{out}"
        );
    }

    /// SPEC-0320 AK-8.
    #[test]
    fn extract_interface_refuses() {
        let a = "class A<T> {\n  private p: number = 1;\n  g(x: T): T { return x; }\n  static s(): void {}\n  m(): void {}\n}\nconst Used = 1;\n";
        let refused = |name, chosen: &[&str], reason: &str| {
            let e = interface(a, name, chosen).unwrap_err();
            assert!(e.contains(reason), "{e} lacks {reason}");
        };
        refused("1x", &["m"], "not a valid name");
        refused("Used", &["m"], "already declared");
        refused("I", &[], "no member");
        refused("I", &["p"], "not public");
        refused("I", &["s"], "not public");
        refused("I", &["g"], "type parameter");
        refused("I", &["zz"], "no member `zz`");
        let p = Path::new("a.js");
        let text = "class A {\n  m() {}\n}\n";
        let parsed = parse_file(p, text).unwrap();
        let e = plan_extract_interface(p, &parsed, 0, "I", &names(&["m"]), &HashMap::new())
            .unwrap_err();
        assert!(e.contains("JavaScript"), "{e}");
    }

    /// SPEC-0320 behaviour 12: types the server supplied, with `readonly`.
    #[test]
    fn extract_interface_uses_hovers() {
        let text = "class A {\n  readonly r = 1;\n  n(a) { return a; }\n}\n";
        let p = Path::new("a.ts");
        let parsed = parse_file(p, text).unwrap();
        let hovers = HashMap::from([
            ("r".to_owned(), "r: 1".to_owned()),
            ("n".to_owned(), "n(a: any): any".to_owned()),
        ]);
        let f = plan_extract_interface(p, &parsed, 0, "I", &names(&["r", "n"]), &hovers).unwrap();
        let out = applied(text, &f);
        assert!(
            out.contains("  readonly r: 1;\n  n(a: any): any;\n"),
            "{out}"
        );
        let f = plan_extract_interface(p, &parsed, 0, "I", &names(&["r"]), &HashMap::new());
        assert!(f.unwrap_err().contains("no type for `r`"));
    }

    fn superclass(text: &str, name: &str, chosen: &[&str]) -> Result<String, String> {
        let p = Path::new("a.ts");
        let parsed = parse_file(p, text).unwrap();
        plan_extract_superclass(p, &parsed, 0, name, &names(chosen)).map(|f| applied(text, &f))
    }

    /// SPEC-0320 AK-6.
    #[test]
    fn extract_superclass_moves_members() {
        let a = "export class A {\n  x = 1;\n\n  constructor() {\n    this.y();\n  }\n\n  /** doc */\n  private z(): void {}\n\n  y(): void {\n    this.z();\n  }\n}\n";
        assert_eq!(
            superclass(a, "S", &["z"]).unwrap(),
            "export class S {\n  /** doc */\n  protected z(): void {}\n}\n\nexport class A extends S {\n  x = 1;\n\n  constructor() {\n    super();\n    this.y();\n  }\n\n  y(): void {\n    this.z();\n  }\n}\n"
        );
    }

    /// SPEC-0320 AK-6: the new class takes the old superclass; `private` stays when unused.
    #[test]
    fn extract_superclass_with_existing_extends() {
        let a = "class A extends B implements I {\n  private p = 1;\n  m(): void {}\n  constructor() { super(); }\n}\n";
        let out = superclass(a, "S", &["p"]).unwrap();
        assert!(out.starts_with("class S extends B {\n  private p = 1;\n}\n\nclass A extends S implements I {\n  m(): void {}\n"), "{out}");
        let abstract_ = "abstract class A {\n  abstract m(): void;\n}\n";
        let out = superclass(abstract_, "S", &["m"]).unwrap();
        assert!(
            out.starts_with("abstract class S {\n  abstract m(): void;\n}\n"),
            "{out}"
        );
    }

    /// SPEC-0320 AK-8.
    #[test]
    fn extract_superclass_refuses() {
        let a = "class A<T> extends B<T> {\n  x = 1;\n  m(): number { return this.x; }\n  g(v: T): void {}\n  constructor(private c: number) { super(); }\n  n(): number { return this.c; }\n}\n";
        let e = superclass(a, "S", &["m"]).unwrap_err();
        assert!(
            e.contains("`this.x`") && e.contains("`extends` clause"),
            "{e}"
        );
        let e = superclass(a, "S", &["n"]).unwrap_err();
        assert!(e.contains("`this.c`"), "{e}");
        let e = superclass(a, "S", &["g"]).unwrap_err();
        assert!(e.contains("type parameter"), "{e}");
        let e = superclass(a, "S", &["constructor"]).unwrap_err();
        assert!(e.contains("cannot move"), "{e}");
        let b = "interface A {\n  m(): void;\n}\n";
        assert!(
            superclass(b, "S", &["m"])
                .unwrap_err()
                .contains("interface")
        );
    }

    fn pull(
        text: &str,
        sub: usize,
        up: usize,
        chosen: &[&str],
        make_abstract: bool,
    ) -> Result<String, String> {
        let p = Path::new("a.ts");
        let parsed = parse_file(p, text).unwrap();
        let target = PullTarget {
            path: p,
            parsed: &parsed,
            index: up,
        };
        plan_pull_up(
            p,
            &parsed,
            sub,
            &target,
            &names(chosen),
            make_abstract,
            &HashMap::new(),
        )
        .map(|f| applied(text, &f))
    }

    const BASE: &str = "abstract class B {\n  a = 1;\n}\ninterface I {\n}\nclass C extends B implements I {\n  m(): number {\n    return this.a;\n  }\n\n  p: string = '';\n}\n";

    /// SPEC-0320 AK-7 within one file.
    #[test]
    fn pulls_members_up() {
        let out = pull(BASE, 2, 0, &["m"], false).unwrap();
        assert!(
            out.contains(
                "abstract class B {\n  a = 1;\n\n  m(): number {\n    return this.a;\n  }\n}"
            ),
            "{out}"
        );
        assert!(
            out.contains("class C extends B implements I {\n  p: string = '';\n}"),
            "{out}"
        );
        let out = pull(BASE, 2, 1, &["m"], false).unwrap();
        assert!(out.contains("interface I {\n  m(): number;\n}"), "{out}");
        assert!(
            out.contains("return this.a;\n  }\n\n  p: string"),
            "member stays: {out}"
        );
        let out = pull(BASE, 2, 0, &["m"], true).unwrap();
        assert!(
            out.contains("a = 1;\n\n  abstract m(): number;\n}"),
            "{out}"
        );
        assert!(
            out.contains("return this.a;\n  }\n\n  p: string"),
            "implementation stays: {out}"
        );
    }

    /// SPEC-0320 AK-7: re-indented across files; the source loses the member.
    #[test]
    fn pulls_across_files_and_reindents() {
        let up_text = "export abstract class B {\n    x = 1;\n}\n";
        let sub_text = "import { B } from './b';\nexport class C extends B {\n  m(): number {\n    return 1;\n  }\n}\n";
        let (up_p, sub_p) = (Path::new("b.ts"), Path::new("c.ts"));
        let up = parse_file(up_p, up_text).unwrap();
        let sub = parse_file(sub_p, sub_text).unwrap();
        let target = PullTarget {
            path: up_p,
            parsed: &up,
            index: 0,
        };
        let files = plan_pull_up(
            sub_p,
            &sub,
            0,
            &target,
            &names(&["m"]),
            false,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(files.len(), 2);
        let out = apply_edits(up_text, &files[0].edits).unwrap();
        assert_eq!(
            out,
            "export abstract class B {\n    x = 1;\n\n    m(): number {\n      return 1;\n    }\n}\n"
        );
        let out = apply_edits(sub_text, &files[1].edits).unwrap();
        assert_eq!(
            out,
            "import { B } from './b';\nexport class C extends B {\n}\n"
        );
    }

    /// SPEC-0320 AK-8.
    #[test]
    fn pull_refuses() {
        let text = "class B {\n  dup = 1;\n}\nabstract class D {}\nclass C extends B {\n  dup = 2;\n  s(): void { super.toString(); }\n  n(): number { return this.k; }\n  k = 1;\n  private h = 1;\n}\n";
        let e = pull(text, 2, 0, &["dup"], false).unwrap_err();
        assert!(e.contains("already declares"), "{e}");
        let e = pull(text, 2, 0, &["s"], false).unwrap_err();
        assert!(e.contains("`super`"), "{e}");
        let e = pull(text, 2, 0, &["n"], false).unwrap_err();
        assert!(e.contains("`this.k`"), "{e}");
        let e = pull(text, 2, 0, &["k"], true).unwrap_err();
        assert!(e.contains("not an abstract class"), "{e}");
        let e = pull(text, 2, 1, &["h"], true).unwrap_err();
        assert!(e.contains("private"), "{e}");
    }

    /// SPEC-0320 AK-8: a member that needs what only its own file has stays.
    #[test]
    fn pull_refuses_what_the_target_file_lacks() {
        let up_text = "export class B {\n}\n";
        let sub_text = "import { dep } from './dep';\nconst local = 1;\nexport class C extends B {\n  m(): number { return dep + local; }\n}\n";
        let (up_p, sub_p) = (Path::new("b.ts"), Path::new("c.ts"));
        let up = parse_file(up_p, up_text).unwrap();
        let sub = parse_file(sub_p, sub_text).unwrap();
        let target = PullTarget {
            path: up_p,
            parsed: &up,
            index: 0,
        };
        let e = plan_pull_up(
            sub_p,
            &sub,
            0,
            &target,
            &names(&["m"]),
            false,
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(e.contains("`dep`") && e.contains("`local`"), "{e}");
    }

    /// SPEC-0320 `class_members`.
    #[test]
    fn lists_members() {
        let text = "class A {\n  /** d */\n  private x = 1;\n  m(): void {\n  }\n}\n";
        let m = class_members(Path::new("a.ts"), text, 0, 7).unwrap();
        assert_eq!(m.class, "A");
        assert_eq!(m.members[0].label, "private x = 1;");
        assert!(!m.members[0].for_interface);
        assert_eq!(m.members[1].label, "m(): void");
        assert!(class_members(Path::new("a.ts"), "const x = 1;\n", 0, 0).is_err());
    }

    /// SPEC-0320 behaviour 16: a one-line body takes members too.
    #[test]
    fn appends_to_a_one_line_body() {
        let text = "class B {}\nclass C extends B {\n  m(): void {}\n}\n";
        let out = pull(text, 1, 0, &["m"], false).unwrap();
        assert!(out.starts_with("class B {\n  m(): void {}\n}\n"), "{out}");
    }
}

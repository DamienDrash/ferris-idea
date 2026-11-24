//! The project's inspection profile (`SPEC-0315` behaviours 1–3, data of behaviour 6): which of
//! Ferris IDEA's inspections are on and how serious they are, kept where the reference keeps its
//! project profile, `.idea/inspectionProfiles/Project_Default.xml` (the directory-based project
//! format of `intellij-community`), so both IDEs share it.
//!
//! An `inspection_tool` whose `class` is an inspection id or a short name that stands for exactly
//! that inspection is Ferris IDEA's and is rewritten; every other child of the profile is kept
//! verbatim and in its order. A short name standing for several inspections (`unused`) or one of
//! two names of an inspection (`EmptyFinallyBlock`) is kept verbatim too, but still applies to
//! its inspections when no entry of their own exists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::inspections::{Finding, Level, default_level, ids_of_short_name, short_names};

/// The profile file relative to the project root.
pub const PROFILE_FILE: &str = ".idea/inspectionProfiles/Project_Default.xml";

/// The name the profile is written with.
pub const PROFILE_NAME: &str = "Project Default";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    /// One of Ferris IDEA's inspections with its state; `inner` is the element's content as read.
    Tool {
        id: &'static str,
        enabled: bool,
        level: Level,
        inner: String,
    },
    /// Anything else, written back as read; `applies` are the inspections a shared short name
    /// stands for, with the state it sets.
    Raw {
        xml: String,
        applies: Vec<&'static str>,
        enabled: Option<bool>,
        level: Option<Level>,
    },
}

/// The project's inspection profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    items: Vec<Item>,
}

/// The class Ferris IDEA writes for `id`: its short name when that names exactly this inspection
/// and the inspection has no other, else the id.
fn class_of(id: &'static str) -> &'static str {
    let mut names = short_names(id);
    match (names.next(), names.next()) {
        (Some(n), None) if ids_of_short_name(n).len() == 1 => n,
        _ => id,
    }
}

/// The inspection a `class` names exactly.
fn exact_id(class: &str) -> Option<&'static str> {
    crate::inspections::known_id(class)
        .or_else(|| match ids_of_short_name(class).as_slice() {
            [id] if class_of(id) == class => Some(*id),
            _ => None,
        })
        .filter(|id| crate::inspections::description(id).is_some())
}

fn level_of(s: &str) -> Level {
    match s {
        "ERROR" => Level::Error,
        "WARNING" => Level::Warning,
        // "No highlighting, only fix" (SPEC-0315 v2 behaviour 7).
        "INFORMATION" => Level::Info,
        _ => Level::Weak,
    }
}

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Error => "ERROR",
        Level::Warning => "WARNING",
        Level::Weak => "WEAK WARNING",
        Level::Info => "INFORMATION",
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `class`, `enabled` and `level` of an `inspection_tool` element.
type Attrs = (Option<String>, Option<bool>, Option<Level>);

/// `class`, `enabled` and `level` of an element.
fn attributes(e: &BytesStart) -> Result<Attrs, String> {
    let (mut class, mut enabled, mut level) = (None, None, None);
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| e.to_string())?;
        match a.key.as_ref() {
            b"class" => class = Some(value.into_owned()),
            b"enabled" => enabled = Some(value.trim() != "false"),
            b"level" => level = Some(level_of(value.trim())),
            _ => {}
        }
    }
    Ok((class, enabled, level))
}

fn name_of(e: &BytesStart) -> String {
    String::from_utf8_lossy(e.name().as_ref()).into_owned()
}

/// A child of the profile element being read.
struct Open {
    start: usize,
    inner_start: usize,
    tool: bool,
    attrs: Attrs,
}

impl Profile {
    /// Reads a profile file's text (behaviour 1); an error when it is not well-formed XML or not
    /// a profile component.
    pub fn parse(xml: &str) -> Result<Profile, String> {
        let mut reader = Reader::from_str(xml);
        let mut path: Vec<String> = Vec::new();
        let mut open: Option<Open> = None;
        let mut items = Vec::new();
        let mut root_seen = false;
        loop {
            let before = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
            let event = reader.read_event().map_err(|e| {
                format!(
                    "not readable as XML at byte {}: {e}",
                    reader.error_position()
                )
            })?;
            let after = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
            let in_profile = path.len() == 2 && path[1] == "profile";
            match event {
                Event::Start(e) | Event::Empty(e) if path.is_empty() => {
                    if root_seen || name_of(&e) != "component" {
                        return Err("not an inspection profile: the root is not <component>".into());
                    }
                    root_seen = true;
                    if xml.get(before..after).is_some_and(|t| !t.ends_with("/>")) {
                        path.push(name_of(&e));
                    }
                }
                Event::Start(e) => {
                    if in_profile {
                        let tool = name_of(&e) == "inspection_tool";
                        open = Some(Open {
                            start: before,
                            inner_start: after,
                            tool,
                            attrs: if tool {
                                attributes(&e)?
                            } else {
                                (None, None, None)
                            },
                        });
                    }
                    path.push(name_of(&e));
                }
                Event::Empty(e) if in_profile => {
                    let raw = xml.get(before..after).unwrap_or_default().to_owned();
                    let name = name_of(&e);
                    if name == "option" && is_my_name(&e) {
                        continue;
                    }
                    let attrs = if name == "inspection_tool" {
                        attributes(&e)?
                    } else {
                        (None, None, None)
                    };
                    items.push(item(raw, String::new(), name == "inspection_tool", attrs));
                }
                Event::End(_) => {
                    path.pop();
                    if path.len() == 2
                        && path[1] == "profile"
                        && let Some(o) = open.take()
                    {
                        let raw = xml.get(o.start..after).unwrap_or_default().to_owned();
                        let inner = xml
                            .get(o.inner_start..before)
                            .unwrap_or_default()
                            .to_owned();
                        items.push(item(raw, inner, o.tool, o.attrs));
                    }
                }
                Event::Comment(_) if in_profile => items.push(Item::Raw {
                    xml: xml.get(before..after).unwrap_or_default().to_owned(),
                    applies: Vec::new(),
                    enabled: None,
                    level: None,
                }),
                Event::Eof => break,
                _ => {}
            }
        }
        if !path.is_empty() {
            return Err(format!(
                "not readable as XML: <{}> is not closed",
                path.join("> <")
            ));
        }
        if !root_seen {
            return Err("not an inspection profile: no <component>".into());
        }
        // Several entries for one inspection: the last wins.
        let mut seen = std::collections::BTreeSet::new();
        let mut kept: Vec<Item> = Vec::new();
        for i in items.into_iter().rev() {
            if let Item::Tool { id, .. } = &i
                && !seen.insert(*id)
            {
                continue;
            }
            kept.push(i);
        }
        kept.reverse();
        Ok(Profile { items: kept })
    }

    /// The profile file's text (behaviour 2); `None` when nothing is left to write.
    pub fn render(&self) -> Option<String> {
        // An entry of Ferris IDEA's that says what the default says is no deviation.
        let written: Vec<&Item> = self
            .items
            .iter()
            .filter(|i| match i {
                Item::Tool {
                    id,
                    enabled,
                    level,
                    inner,
                } => {
                    (*enabled, *level) != (true, default_level(id))
                        || self.shared(id).is_some()
                        || !inner.trim().is_empty()
                }
                Item::Raw { .. } => true,
            })
            .collect();
        if written.is_empty() {
            return None;
        }
        let mut s = String::from(
            "<component name=\"InspectionProjectProfileManager\">\n  <profile version=\"1.0\">\n",
        );
        s.push_str(&format!(
            "    <option name=\"myName\" value=\"{}\" />\n",
            escape(PROFILE_NAME)
        ));
        for i in written {
            s.push_str("    ");
            match i {
                Item::Raw { xml, .. } => s.push_str(xml),
                Item::Tool {
                    id,
                    enabled,
                    level,
                    inner,
                } => {
                    let head = format!(
                        "<inspection_tool class=\"{}\" enabled=\"{enabled}\" level=\"{}\" enabled_by_default=\"{enabled}\"",
                        escape(class_of(id)),
                        level_name(*level)
                    );
                    s.push_str(&head);
                    if inner.trim().is_empty() {
                        s.push_str(" />");
                    } else {
                        s.push('>');
                        s.push_str(inner);
                        s.push_str("</inspection_tool>");
                    }
                }
            }
            s.push('\n');
        }
        s.push_str("  </profile>\n</component>\n");
        Some(s)
    }

    /// Whether `id` is on and its level (behaviour 1): the entry of the inspection, else a
    /// shared entry covering it, else the default.
    pub fn state(&self, id: &str) -> (bool, Level) {
        let default = (true, default_level(id));
        if let Some((enabled, level)) = self.items.iter().find_map(|i| match i {
            Item::Tool {
                id: t,
                enabled,
                level,
                ..
            } if *t == id => Some((*enabled, *level)),
            _ => None,
        }) {
            return (enabled, level);
        }
        self.shared(id).map_or(default, |(e, l)| {
            (e.unwrap_or(default.0), l.unwrap_or(default.1))
        })
    }

    /// The state the last shared entry covering `id` sets.
    fn shared(&self, id: &str) -> Option<(Option<bool>, Option<Level>)> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Raw {
                applies,
                enabled,
                level,
                ..
            } if applies.contains(&id) => Some((*enabled, *level)),
            _ => None,
        })
    }

    /// Turns `id` on or off and/or sets its level; a state equal to the default (with no shared
    /// entry covering it) leaves no entry. Unknown ids are ignored.
    pub fn set(&mut self, id: &str, enabled: Option<bool>, level: Option<Level>) {
        let Some(id) = crate::inspections::known_id(id) else {
            return;
        };
        let (cur_enabled, cur_level) = self.state(id);
        let (enabled, level) = (enabled.unwrap_or(cur_enabled), level.unwrap_or(cur_level));
        let default = (true, default_level(id));
        if (enabled, level) == default && self.shared(id).is_none() {
            self.reset(id);
            return;
        }
        for i in &mut self.items {
            if let Item::Tool {
                id: t,
                enabled: e,
                level: l,
                ..
            } = i
                && *t == id
            {
                *e = enabled;
                *l = level;
                return;
            }
        }
        // A new entry goes before the first entry with a larger class, so a sorted file stays
        // sorted and the other entries keep their order.
        let class = class_of(id);
        let at = self
            .items
            .iter()
            .position(|i| item_class(i).is_some_and(|c| c.as_str() > class))
            .unwrap_or(self.items.len());
        self.items.insert(
            at,
            Item::Tool {
                id,
                enabled,
                level,
                inner: String::new(),
            },
        );
    }

    /// Returns `id` to the default by removing its own entry (a shared entry stays).
    pub fn reset(&mut self, id: &str) {
        self.items
            .retain(|i| !matches!(i, Item::Tool { id: t, .. } if *t == id));
    }

    /// Whether `id` differs from its default.
    pub fn is_changed(&self, id: &str) -> bool {
        self.state(id) != (true, default_level(id))
    }

    /// Drops the findings of turned-off inspections and sets the level of the others (behaviour 3).
    pub fn apply(&self, findings: Vec<Finding>) -> Vec<Finding> {
        if self.items.is_empty() {
            return findings;
        }
        findings
            .into_iter()
            .filter_map(|mut f| {
                let own = self.items.iter().any(|i| match i {
                    Item::Tool { id, .. } => *id == f.id,
                    Item::Raw { applies, .. } => applies.contains(&f.id),
                });
                if !own {
                    return Some(f);
                }
                let (enabled, level) = self.state(f.id);
                enabled.then(|| {
                    f.level = level;
                    f
                })
            })
            .collect()
    }
}

fn item_class(i: &Item) -> Option<String> {
    match i {
        Item::Tool { id, .. } => Some(class_of(id).to_owned()),
        Item::Raw { xml, .. } => {
            let rest = xml.strip_prefix("<inspection_tool")?;
            let at = rest.find("class=\"")? + 7;
            let end = rest[at..].find('"')?;
            Some(rest[at..at + end].to_owned())
        }
    }
}

fn is_my_name(e: &BytesStart) -> bool {
    e.attributes()
        .flatten()
        .any(|a| a.key.as_ref() == b"name" && a.value.as_ref() == b"myName")
}

fn item(raw: String, inner: String, tool: bool, (class, enabled, level): Attrs) -> Item {
    if tool && let Some(class) = class.as_deref() {
        if let Some(id) = exact_id(class) {
            return Item::Tool {
                id,
                enabled: enabled.unwrap_or(true),
                level: level.unwrap_or_else(|| default_level(id)),
                inner,
            };
        }
        return Item::Raw {
            xml: raw,
            applies: ids_of_short_name(class),
            enabled,
            level,
        };
    }
    Item::Raw {
        xml: raw,
        applies: Vec::new(),
        enabled: None,
        level: None,
    }
}

/// The project folder of `path`: the nearest folder (the path itself or an ancestor) holding
/// `.idea`.
pub fn root_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|d| d.join(".idea").is_dir())
        .map(Path::to_path_buf)
}

/// The profile of the project at `root`; a missing file is the default profile.
pub fn load(root: &Path) -> Result<Profile, String> {
    match std::fs::read_to_string(root.join(PROFILE_FILE)) {
        Ok(xml) => Profile::parse(&xml),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Profile::default()),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes `profile` for the project at `root` (behaviour 2): atomically through a temporary file
/// and a rename, creating `.idea/inspectionProfiles`; removes the file when nothing is left.
pub fn save(root: &Path, profile: &Profile) -> std::io::Result<()> {
    let file = root.join(PROFILE_FILE);
    let Some(xml) = profile.render() else {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    let dir = file
        .parent()
        .ok_or_else(|| std::io::Error::other("profile file without a folder"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".Project_Default.xml.{}.tmp", std::process::id()));
    std::fs::write(&tmp, xml)?;
    std::fs::rename(&tmp, &file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

type Cache = HashMap<PathBuf, (Option<(SystemTime, u64)>, Profile)>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The profile of the project of `path`, read again when its file changes; the default profile
/// when there is no project or the file is not readable.
pub fn profile_for(path: &Path) -> Profile {
    let Some(root) = root_of(path) else {
        return Profile::default();
    };
    let file = root.join(PROFILE_FILE);
    let stamp = std::fs::metadata(&file)
        .ok()
        .and_then(|m| Some((m.modified().ok()?, m.len())));
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some((s, p)) = cache.get(&file)
        && *s == stamp
    {
        return p.clone();
    }
    let profile = load(&root).unwrap_or_default();
    cache.insert(file, (stamp, profile.clone()));
    profile
}

/// Drops the findings of `path` whose inspection the project's profile turns off and sets the
/// level of the others (behaviour 3). `path` is absolute.
pub fn filter(path: &Path, findings: Vec<Finding>) -> Vec<Finding> {
    if findings.is_empty() {
        return findings;
    }
    profile_for(path).apply(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: &str = r#"<component name="InspectionProjectProfileManager">
  <profile version="1.0">
    <option name="myName" value="Project Default" />
    <inspection_tool class="CatchMayIgnoreException" enabled="false" level="WARNING" enabled_by_default="false" />
    <inspection_tool class="SpellCheckingInspection" enabled="false" level="TYPO" enabled_by_default="false">
      <option name="processCode" value="true" />
    </inspection_tool>
    <inspection_tool class="StringEquality" enabled="true" level="ERROR" enabled_by_default="true" />
    <inspection_tool class="java.print-stack-trace" enabled="true" level="WARNING" enabled_by_default="true" />
    <inspection_tool class="SizeReplaceableByIsEmpty" enabled="true" level="INFORMATION" enabled_by_default="true" />
    <inspection_tool class="unused" enabled="false" level="WARNING" enabled_by_default="false" />
    <inspection_tool class="AlphaUnsorted" enabled="true" level="TYPO" enabled_by_default="true" />
  </profile>
</component>
"#;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-iprof-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".idea")).unwrap();
        dir
    }

    /// SPEC-0315/AK-1: the reference's format, known and unknown classes, all levels.
    #[test]
    fn reads_the_reference_format() {
        let p = Profile::parse(REFERENCE).unwrap();
        assert_eq!(p.state("java.empty-catch"), (false, Level::Warning));
        assert_eq!(p.state("java.string-equality"), (true, Level::Error));
        assert_eq!(p.state("java.print-stack-trace"), (true, Level::Warning));
        assert!(p.is_changed("java.print-stack-trace"), "weak by default");
        // INFORMATION is "no highlighting, only fix" (SPEC-0315 v2 behaviour 7, AK-6).
        assert_eq!(p.state("java.size-zero"), (true, Level::Info));
        assert!(p.is_changed("java.size-zero"));
        let mut q = Profile::default();
        q.set("java.size-zero", None, Some(Level::Info));
        assert!(
            q.render().unwrap().contains(
                "class=\"SizeReplaceableByIsEmpty\" enabled=\"true\" level=\"INFORMATION\""
            )
        );
        // `unused` covers the unused rules but not the import rule's own name.
        assert_eq!(p.state("java.unused-local"), (false, Level::Warning));
        assert_eq!(p.state("java.unused-import"), (true, Level::Weak));
        assert_eq!(p.state("java.empty-if"), (true, Level::Warning));
        assert!(!p.is_changed("java.empty-if"));
        assert!(Profile::parse("<component><profile>").is_err());
        assert!(Profile::parse("<settings/>").is_err());
        assert!(Profile::parse("not xml").is_err());
        assert_eq!(
            Profile::parse("<component name=\"InspectionProjectProfileManager\"/>").unwrap(),
            Profile::default()
        );
    }

    /// SPEC-0315/AK-1: only deviations are written, unknown entries keep their text and order.
    #[test]
    fn writes_deviations_and_keeps_unknown_entries() {
        let mut p = Profile::parse(REFERENCE).unwrap();
        p.set("java.size-zero", None, Some(Level::Weak));
        p.set("java.empty-catch", Some(true), None);
        p.set("java.empty-if", Some(false), None);
        p.set("java.null-dereference", None, Some(Level::Error));
        p.set("not.an-inspection", Some(false), None);
        let xml = p.render().unwrap();
        let lines: Vec<&str> = xml.lines().map(str::trim).collect();
        assert_eq!(
            lines,
            [
                "<component name=\"InspectionProjectProfileManager\">",
                "<profile version=\"1.0\">",
                "<option name=\"myName\" value=\"Project Default\" />",
                "<inspection_tool class=\"DataFlowIssue\" enabled=\"true\" level=\"ERROR\" enabled_by_default=\"true\" />",
                "<inspection_tool class=\"EmptyStatementBody\" enabled=\"false\" level=\"WARNING\" enabled_by_default=\"false\" />",
                "<inspection_tool class=\"SpellCheckingInspection\" enabled=\"false\" level=\"TYPO\" enabled_by_default=\"false\">",
                "<option name=\"processCode\" value=\"true\" />",
                "</inspection_tool>",
                "<inspection_tool class=\"StringEquality\" enabled=\"true\" level=\"ERROR\" enabled_by_default=\"true\" />",
                "<inspection_tool class=\"ThrowablePrintStackTrace\" enabled=\"true\" level=\"WARNING\" enabled_by_default=\"true\" />",
                "<inspection_tool class=\"unused\" enabled=\"false\" level=\"WARNING\" enabled_by_default=\"false\" />",
                "<inspection_tool class=\"AlphaUnsorted\" enabled=\"true\" level=\"TYPO\" enabled_by_default=\"true\" />",
                "</profile>",
                "</component>",
            ]
        );
        assert!(
            xml.contains("enabled_by_default=\"false\">\n      <option name=\"processCode\""),
            "unknown entries are kept verbatim: {xml}"
        );
        // Reading the written file gives the same profile.
        assert_eq!(Profile::parse(&xml).unwrap(), p);
        p.reset("java.empty-if");
        assert!(!p.is_changed("java.empty-if"));
        // An unused rule turned on explicitly against the shared `unused` entry gets its own.
        p.set("java.unused-local", Some(true), None);
        assert_eq!(p.state("java.unused-local"), (true, Level::Warning));
        assert!(p.render().unwrap().contains("class=\"java.unused-local\""));
    }

    /// SPEC-0315/AK-1: the file is created with its folder, written atomically and removed when
    /// no entry is left; a missing file is the default.
    #[test]
    fn saves_atomically_and_removes_an_empty_profile() {
        let root = temp("save");
        assert_eq!(load(&root).unwrap(), Profile::default());
        let mut p = Profile::default();
        p.set("java.string-equality", Some(false), None);
        save(&root, &p).unwrap();
        let file = root.join(PROFILE_FILE);
        let xml = std::fs::read_to_string(&file).unwrap();
        assert!(
            xml.contains("class=\"StringEquality\" enabled=\"false\""),
            "{xml}"
        );
        let leftovers: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers, [std::ffi::OsString::from("Project_Default.xml")]);
        assert_eq!(load(&root).unwrap(), p);
        p.reset("java.string-equality");
        save(&root, &p).unwrap();
        assert!(!file.exists());
        save(&root, &p).unwrap();
        // An unreadable file is reported, and `filter` then keeps the defaults.
        std::fs::write(&file, "<component><profile>").unwrap();
        assert!(load(&root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0315 behaviour 3: the filter finds the project above an absolute path.
    #[test]
    fn filter_finds_the_project() {
        let root = temp("filter");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/A.java");
        assert_eq!(root_of(&file).as_deref(), Some(root.as_path()));
        let f = |id: &'static str| Finding {
            id,
            start: 0,
            end: 1,
            level: Level::Warning,
            message: String::new(),
            fix: None,
        };
        let mut p = Profile::default();
        p.set("java.empty-catch", Some(false), None);
        p.set("java.empty-if", None, Some(Level::Error));
        save(&root, &p).unwrap();
        let kept = filter(
            &file,
            vec![f("java.empty-catch"), f("java.empty-if"), f("x.y")],
        );
        let kept: Vec<_> = kept.iter().map(|f| (f.id, f.level)).collect();
        assert_eq!(
            kept,
            [("java.empty-if", Level::Error), ("x.y", Level::Warning)]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

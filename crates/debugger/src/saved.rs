//! Breakpoints of a workspace as they are kept between runs (`SPEC-0091` v3 behaviour 9,
//! `SPEC-0127`): lines per file and what each breakpoint does besides stopping. Shared by the
//! window and the MCP tools.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{BreakpointRules, SourceBreakpoint};

/// Where the breakpoints of a workspace and what they do are kept (`SPEC-0303` behaviour 14).
pub const BREAKPOINTS_FILE: &str = ".idecraft/breakpoints.toml";

/// Where older versions kept the breakpoint lines (`SPEC-0091` v3 behaviour 9); read while
/// [`BREAKPOINTS_FILE`] does not exist.
pub const LEGACY_BREAKPOINTS_FILE: &str = ".idecraft/breakpoints";

/// Breakpoints by file (0-based lines) from the text of the breakpoints file: one file per line,
/// `path:line,line` with 1-based lines and paths relative to `root`. Malformed lines are skipped.
pub fn parse_breakpoints(root: &Path, text: &str) -> BTreeMap<PathBuf, Vec<usize>> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some((path, lines)) = line.rsplit_once(':') else {
            continue;
        };
        let mut lines: Vec<usize> = lines
            .split(',')
            .filter_map(|n| n.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
            .map(|n| n - 1)
            .collect();
        lines.sort_unstable();
        lines.dedup();
        if path.is_empty() || lines.is_empty() {
            continue;
        }
        out.insert(root.join(path), lines);
    }
    out
}

/// The text of the breakpoints file for `breakpoints` (the inverse of [`parse_breakpoints`]).
pub fn format_breakpoints(root: &Path, breakpoints: &BTreeMap<PathBuf, Vec<usize>>) -> String {
    breakpoints
        .iter()
        .filter(|(_, lines)| !lines.is_empty())
        .map(|(path, lines)| {
            let path = path.strip_prefix(root).unwrap_or(path);
            let lines: Vec<String> = lines.iter().map(|l| (l + 1).to_string()).collect();
            format!("{}:{}\n", path.display(), lines.join(","))
        })
        .collect()
}

/// The saved breakpoints of the workspace at `root`; none when the file is missing.
pub fn load_breakpoints(root: &Path) -> BTreeMap<PathBuf, Vec<usize>> {
    load_all(root).0
}

/// Saves `breakpoints` for the workspace at `root`, keeping the saved properties of lines that
/// still have a breakpoint; removes the file when there are none.
pub fn save_breakpoints(root: &Path, breakpoints: &BTreeMap<PathBuf, Vec<usize>>) {
    let (_, props) = load_all(root);
    save_all(root, breakpoints, &props);
}

/// One `[[breakpoint]]` of `breakpoints.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file: String,
    line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hit_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    log_message: Option<String>,
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    enabled: bool,
    /// The further options (`SPEC-0127` v3), `depends_on` relative to the workspace.
    #[serde(default, skip_serializing_if = "BreakpointRules::is_empty")]
    rules: BreakpointRules,
}

/// `rules` with the path of `depends_on` made absolute under `root` (`into`) or relative to it.
fn place_rules(root: &Path, mut rules: BreakpointRules, into: bool) -> BreakpointRules {
    if let Some(text) = &rules.depends_on
        && let Some((path, line)) = crate::breakpoint_place(text)
    {
        let path = if into {
            root.join(path)
        } else {
            path.strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or(path)
        };
        rules.depends_on = Some(format!(
            "{}:{line}",
            path.to_string_lossy().replace('\\', "/")
        ));
    }
    rules
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct File {
    #[serde(default)]
    breakpoint: Vec<Entry>,
}

/// Lines and properties from `breakpoints.toml`, else from the two files of older versions
/// (`SPEC-0303` behaviour 14). An unreadable or newer file gives none.
pub fn load_all(root: &Path) -> (BTreeMap<PathBuf, Vec<usize>>, BreakpointPropsMap) {
    let text = match std::fs::read_to_string(root.join(BREAKPOINTS_FILE)) {
        Ok(t) => t,
        Err(_) => {
            let lines = std::fs::read_to_string(root.join(LEGACY_BREAKPOINTS_FILE))
                .map(|t| parse_breakpoints(root, &t))
                .unwrap_or_default();
            return (lines, load_legacy_props(root));
        }
    };
    let Ok(file) = idecraft_core::format::read(
        &text,
        Some("breakpoints"),
        idecraft_core::format::Use::Project,
    )
    .and_then(|d| d.body_as::<File>()) else {
        return Default::default();
    };
    let mut lines: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    let mut props = BreakpointPropsMap::new();
    for e in file.breakpoint.into_iter().filter(|e| e.line > 0) {
        let path = root.join(&e.file);
        let line = e.line - 1;
        lines.entry(path.clone()).or_default().push(line);
        let p = BreakpointProps {
            condition: e.condition,
            hit_condition: e.hit_condition,
            log_message: e.log_message,
            disabled: !e.enabled,
            rules: place_rules(root, e.rules, true),
        };
        if !p.is_plain() {
            props.entry(path).or_default().insert(line, p);
        }
    }
    for l in lines.values_mut() {
        l.sort_unstable();
        l.dedup();
    }
    (lines, props)
}

/// Saves the breakpoint `lines` with their `props` as `breakpoints.toml` (properties of lines
/// without a breakpoint are dropped); removes it when there are none. The files of older versions
/// are kept once as `.bak` (`SPEC-0303` behaviours 14–15).
pub fn save_all(root: &Path, lines: &BTreeMap<PathBuf, Vec<usize>>, props: &BreakpointPropsMap) {
    let mut entries = Vec::new();
    for (path, ls) in lines {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        for &l in ls {
            let p = props
                .get(path)
                .and_then(|m| m.get(&l))
                .cloned()
                .unwrap_or_default();
            entries.push(Entry {
                file: rel.clone(),
                line: l + 1,
                condition: p.condition,
                hit_condition: p.hit_condition,
                log_message: p.log_message,
                enabled: !p.disabled,
                rules: place_rules(root, p.rules, false),
            });
        }
    }
    let file = root.join(BREAKPOINTS_FILE);
    if idecraft_core::format::prepare_write(&file, "breakpoints").is_err() {
        return;
    }
    if entries.is_empty() {
        let _ = std::fs::remove_file(&file);
    } else if let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Ok(text) = idecraft_core::format::write(
            &idecraft_core::format::Envelope::current("breakpoints"),
            &File {
                breakpoint: entries,
            },
        )
    {
        let _ = std::fs::write(&file, text);
    }
    for old in [LEGACY_BREAKPOINTS_FILE, BREAKPOINT_PROPS_FILE] {
        let _ = idecraft_core::format::retire(&root.join(old));
    }
}

/// Where the exception and method breakpoints of a workspace are kept (`SPEC-0146`).
pub const SPECIAL_BREAKPOINTS_FILE: &str = ".idecraft/special-breakpoints.json";

/// The saved exception and method breakpoints of the workspace at `root`; none when the file is
/// missing or unreadable.
pub fn load_special_breakpoints(root: &Path) -> crate::SpecialBreakpoints {
    std::fs::read_to_string(root.join(SPECIAL_BREAKPOINTS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Saves `special` for the workspace at `root`; removes the file when nothing is set.
pub fn save_special_breakpoints(root: &Path, special: &crate::SpecialBreakpoints) {
    let file = root.join(SPECIAL_BREAKPOINTS_FILE);
    if special.is_empty() && special.exception_condition.is_empty() {
        let _ = std::fs::remove_file(file);
    } else if let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Ok(text) = serde_json::to_string_pretty(special)
    {
        let _ = std::fs::write(file, text + "\n");
    }
}

/// Where the renderers of a workspace are kept (`SPEC-0091` v12 behaviour 33).
pub const RENDERERS_FILE: &str = ".idecraft/debug-renderers.json";

/// The saved renderers of the workspace at `root`; none when the file is missing or unreadable.
pub fn load_renderers(root: &Path) -> Vec<crate::extras::Renderer> {
    std::fs::read_to_string(root.join(RENDERERS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Saves `renderers` for the workspace at `root`; removes the file when there are none.
pub fn save_renderers(root: &Path, renderers: &[crate::extras::Renderer]) {
    let file = root.join(RENDERERS_FILE);
    if renderers.is_empty() {
        let _ = std::fs::remove_file(file);
    } else if let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Ok(text) = serde_json::to_string_pretty(renderers)
    {
        let _ = std::fs::write(file, text + "\n");
    }
}

/// What a breakpoint does besides stopping (`SPEC-0127`): all empty for a plain breakpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BreakpointProps {
    /// Expression that must be true for the program to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Pass count: stop only on this hit (adapter syntax, usually a number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Log this message instead of stopping; `{expr}` parts are evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
    /// Kept but not sent to the debugger.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Which threads stop, messages, removal, dependency and filters (`SPEC-0127` v3).
    #[serde(default, skip_serializing_if = "BreakpointRules::is_empty")]
    pub rules: BreakpointRules,
}

impl BreakpointProps {
    /// Whether this is a plain, enabled breakpoint (nothing to keep).
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    /// A temporary line breakpoint: its only property is "Remove once hit", so it stops the
    /// program once and is gone (`SPEC-0127` v8 behaviour 18).
    pub fn temporary() -> Self {
        Self {
            rules: BreakpointRules {
                remove_once_hit: true,
                ..BreakpointRules::default()
            },
            ..Self::default()
        }
    }

    /// How the gutter shows it.
    pub fn look(&self) -> BreakpointLook {
        if self.disabled {
            BreakpointLook::Disabled
        } else if self.log_message.is_some() || self.rules.suspend == Some(crate::Suspend::None) {
            BreakpointLook::Log
        } else if self.condition.is_some() || self.hit_condition.is_some() || !self.rules.is_empty()
        {
            BreakpointLook::Conditional
        } else {
            BreakpointLook::Plain
        }
    }
}

/// How a breakpoint looks in the gutter (`SPEC-0127` behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BreakpointLook {
    /// A red dot.
    #[default]
    Plain,
    /// A red dot with a light centre: it has a condition or pass count.
    Conditional,
    /// An amber dot: it logs and does not stop.
    Log,
    /// A red ring: it is switched off.
    Disabled,
    /// A red square: a method breakpoint on its header line (`SPEC-0146` v6 behaviour 12).
    Method,
    /// A red outline square: a switched-off method breakpoint.
    MethodDisabled,
}

impl BreakpointLook {
    /// Whether the breakpoint's line gets the breakpoint background: not when it is switched off
    /// (hand test 54: a disabled breakpoint kept the red line).
    pub fn tints_line(self) -> bool {
        !matches!(
            self,
            BreakpointLook::Disabled | BreakpointLook::MethodDisabled
        )
    }
}

/// Toggle Temporary Line Breakpoint on `line` of a file with the breakpoints `lines` (`SPEC-0127`
/// v8 behaviour 18): the properties of the breakpoint the toggle sets, or `None` when the line has
/// a breakpoint, temporary or not, which the toggle removes with its properties.
pub fn toggle_temporary(lines: &[usize], line: usize) -> Option<BreakpointProps> {
    (!lines.contains(&line)).then(BreakpointProps::temporary)
}

/// Properties of breakpoints per file and 0-based line.
pub type BreakpointPropsMap = BTreeMap<PathBuf, BTreeMap<usize, BreakpointProps>>;

/// Where older versions kept the breakpoint properties, beside [`LEGACY_BREAKPOINTS_FILE`]; read
/// while [`BREAKPOINTS_FILE`] does not exist.
pub const BREAKPOINT_PROPS_FILE: &str = ".idecraft/breakpoint-properties.json";

/// The breakpoints of one file as the debugger takes them: 1-based lines with their properties;
/// disabled ones are left out.
pub fn source_breakpoints(
    lines: &[usize],
    props: Option<&BTreeMap<usize, BreakpointProps>>,
) -> Vec<SourceBreakpoint> {
    lines
        .iter()
        .filter_map(|&l| {
            let p = props.and_then(|m| m.get(&l)).cloned().unwrap_or_default();
            (!p.disabled).then(|| SourceBreakpoint {
                line: l as u64 + 1,
                condition: p.condition,
                hit_condition: p.hit_condition,
                log_message: p.log_message,
                rules: p.rules,
            })
        })
        .collect()
}

/// The properties after the breakpoint lines of a file went from `before` to `after`: with as many
/// breakpoints as before they moved together (an edit shifted them), so the properties follow by
/// position; otherwise properties stay on lines that still have a breakpoint.
pub fn carry_props(
    before: &[usize],
    after: &[usize],
    props: &BTreeMap<usize, BreakpointProps>,
) -> BTreeMap<usize, BreakpointProps> {
    if before.len() == after.len() {
        before
            .iter()
            .zip(after)
            .filter_map(|(b, a)| props.get(b).map(|p| (*a, p.clone())))
            .collect()
    } else {
        props
            .iter()
            .filter(|(l, _)| after.contains(l))
            .map(|(l, p)| (*l, p.clone()))
            .collect()
    }
}

/// Saved breakpoint properties of the workspace at `root`; none when the file is missing or
/// unreadable.
pub fn load_breakpoint_props(root: &Path) -> BreakpointPropsMap {
    load_all(root).1
}

/// Saves `props`, keeping the saved breakpoint lines; properties of lines without a breakpoint
/// are dropped. Use [`save_all`] when both changed.
pub fn save_breakpoint_props(root: &Path, props: &BreakpointPropsMap) {
    let (lines, _) = load_all(root);
    save_all(root, &lines, props);
}

/// The properties file of older versions (paths relative to `root`, 1-based lines).
fn load_legacy_props(root: &Path) -> BreakpointPropsMap {
    let Ok(text) = std::fs::read_to_string(root.join(BREAKPOINT_PROPS_FILE)) else {
        return BTreeMap::new();
    };
    let raw: BTreeMap<String, BTreeMap<usize, BreakpointProps>> =
        serde_json::from_str(&text).unwrap_or_default();
    raw.into_iter()
        .map(|(path, lines)| {
            (
                root.join(path),
                lines
                    .into_iter()
                    .filter(|(l, p)| *l > 0 && !p.is_plain())
                    .map(|(l, p)| (l - 1, p))
                    .collect(),
            )
        })
        .filter(|(_, m): &(PathBuf, BTreeMap<usize, BreakpointProps>)| !m.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_breakpoints_tint_their_line() {
        assert!(BreakpointLook::Plain.tints_line());
        assert!(BreakpointLook::Log.tints_line());
        assert!(!BreakpointLook::Disabled.tints_line());
    }

    /// SPEC-0127 AK-1: properties follow moved breakpoints, reach the debugger, and are saved.
    #[test]
    fn special_breakpoints_round_trip() {
        let root = std::env::temp_dir().join(format!("idecraft-special-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            load_special_breakpoints(&root),
            crate::SpecialBreakpoints::default()
        );
        let special = crate::SpecialBreakpoints {
            uncaught: true,
            functions: vec![
                crate::FunctionBreakpoint {
                    name: "Shapes::area".into(),
                    condition: Some("w > 2".into()),
                    ..crate::FunctionBreakpoint::default()
                },
                // SPEC-0146 v6 behaviours 12–14: from ⌘F8 on a header, watching the exit only,
                // switched off.
                crate::FunctionBreakpoint {
                    name: "shop.Cart.total".into(),
                    entry: false,
                    exit: true,
                    disabled: true,
                    file: Some(root.join("Cart.java")),
                    line: Some(4),
                    ..crate::FunctionBreakpoint::default()
                },
            ],
            languages: [(
                "python".to_owned(),
                crate::LanguageExceptions {
                    caught: true,
                    ..crate::LanguageExceptions::default()
                },
            )]
            .into(),
            ..crate::SpecialBreakpoints::default()
        };
        save_special_breakpoints(&root, &special);
        assert_eq!(load_special_breakpoints(&root), special);
        let text = std::fs::read_to_string(root.join(SPECIAL_BREAKPOINTS_FILE)).unwrap();
        // `entry` is written only when off; the first breakpoint has neither `entry` nor `exit`.
        assert_eq!(text.matches("\"entry\"").count(), 1, "{text}");
        assert!(text.contains("\"languages\""), "{text}");
        // A file written before v6 reads with entry on.
        std::fs::write(
            root.join(SPECIAL_BREAKPOINTS_FILE),
            r#"{"functions": [{"name": "main"}]}"#,
        )
        .unwrap();
        let old = load_special_breakpoints(&root);
        assert!(old.functions[0].entry && !old.functions[0].exit && !old.functions[0].disabled);
        save_special_breakpoints(&root, &crate::SpecialBreakpoints::default());
        assert!(!root.join(SPECIAL_BREAKPOINTS_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn breakpoint_properties() {
        let cond = BreakpointProps {
            condition: Some("i == 3".into()),
            ..Default::default()
        };
        let off = BreakpointProps {
            disabled: true,
            ..Default::default()
        };
        let log = BreakpointProps {
            log_message: Some("i={i}".into()),
            ..Default::default()
        };
        assert_eq!(BreakpointProps::default().look(), BreakpointLook::Plain);
        assert_eq!(cond.look(), BreakpointLook::Conditional);
        assert_eq!(off.look(), BreakpointLook::Disabled);
        assert_eq!(log.look(), BreakpointLook::Log);
        let props: BTreeMap<usize, BreakpointProps> =
            [(4, cond.clone()), (9, off.clone()), (12, log.clone())].into();
        // An edit above shifted every breakpoint by two lines.
        let moved = carry_props(&[4, 9, 12], &[6, 11, 14], &props);
        assert_eq!(moved.keys().copied().collect::<Vec<_>>(), [6, 11, 14]);
        // One breakpoint removed: the others keep theirs.
        let fewer = carry_props(&[4, 9, 12], &[4, 12], &props);
        assert_eq!(fewer.keys().copied().collect::<Vec<_>>(), [4, 12]);
        let sent = source_breakpoints(&[2, 4, 9, 12], Some(&props));
        assert_eq!(
            sent.iter().map(|b| b.line).collect::<Vec<_>>(),
            [3, 5, 13],
            "the disabled one is not sent"
        );
        assert_eq!(sent[1].condition.as_deref(), Some("i == 3"));
        assert_eq!(sent[2].log_message.as_deref(), Some("i={i}"));

        let root = std::env::temp_dir().join(format!("idecraft-bp-props-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut all = BreakpointPropsMap::new();
        all.insert(root.join("src/main.c"), props.clone());
        all.insert(
            root.join("plain.c"),
            [(1, BreakpointProps::default())].into(),
        );
        let lines: BTreeMap<PathBuf, Vec<usize>> = [
            (root.join("src/main.c"), vec![4, 9, 12]),
            (root.join("plain.c"), vec![1]),
        ]
        .into();
        save_all(&root, &lines, &all);
        let text = std::fs::read_to_string(root.join(BREAKPOINTS_FILE)).unwrap();
        assert!(
            text.contains("file = \"src/main.c\"\nline = 5\ncondition = \"i == 3\""),
            "{text}"
        );
        assert!(text.contains("line = 10\nenabled = false"), "{text}");
        let back = load_breakpoint_props(&root);
        assert_eq!(back.len(), 1, "plain breakpoints have no properties");
        assert_eq!(back[&root.join("src/main.c")], props);
        assert_eq!(load_breakpoints(&root), lines);
        // Lines saved alone keep the properties of lines that stay.
        let fewer: BTreeMap<PathBuf, Vec<usize>> = [(root.join("src/main.c"), vec![4, 12])].into();
        save_breakpoints(&root, &fewer);
        assert_eq!(
            load_breakpoint_props(&root)[&root.join("src/main.c")]
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [4, 12]
        );
        save_breakpoint_props(&root, &BreakpointPropsMap::new());
        assert!(load_breakpoint_props(&root).is_empty());
        assert_eq!(load_breakpoints(&root), fewer);
        save_breakpoints(&root, &BTreeMap::new());
        assert!(!root.join(BREAKPOINTS_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0127 v3: the further options are saved with the breakpoint, the dependency's path
    /// relative to the workspace, and read back with it absolute; they draw the light centre.
    #[test]
    fn breakpoint_rules_round_trip() {
        let root = std::env::temp_dir().join(format!("idecraft-bp-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("src/Main.java");
        let rules = BreakpointProps {
            rules: BreakpointRules {
                suspend: Some(crate::Suspend::All),
                log_hit: true,
                remove_once_hit: true,
                depends_on: Some(format!("{}:3", file.display())),
                class_filter: Some("Main*".into()),
                ..BreakpointRules::default()
            },
            ..BreakpointProps::default()
        };
        assert_eq!(rules.look(), BreakpointLook::Conditional);
        let quiet = BreakpointProps {
            rules: BreakpointRules {
                suspend: Some(crate::Suspend::None),
                ..BreakpointRules::default()
            },
            ..BreakpointProps::default()
        };
        assert_eq!(quiet.look(), BreakpointLook::Log, "no suspend logs");
        let lines: BTreeMap<PathBuf, Vec<usize>> = [(file.clone(), vec![2, 6])].into();
        let props: BreakpointPropsMap = [(file.clone(), [(6, rules.clone())].into())].into();
        save_all(&root, &lines, &props);
        let text = std::fs::read_to_string(root.join(BREAKPOINTS_FILE)).unwrap();
        assert!(text.contains("depends_on = \"src/Main.java:3\""), "{text}");
        assert_eq!(load_all(&root), (lines, props));
        let sent = source_breakpoints(&[2, 6], load_all(&root).1.get(&file));
        assert_eq!(sent[1].rules.suspend, Some(crate::Suspend::All));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0127 AK-13 (v8): a temporary breakpoint has "Remove once hit" and nothing else, is
    /// saved and sent like any breakpoint with an option, and the toggle sets it on a free line
    /// and removes whatever breakpoint a line has, the other lines keeping their properties.
    #[test]
    fn a_temporary_breakpoint_is_removed_once_hit_and_toggles() {
        let temporary = BreakpointProps::temporary();
        assert!(temporary.rules.remove_once_hit);
        assert_eq!(
            BreakpointProps {
                rules: BreakpointRules::default(),
                ..temporary.clone()
            },
            BreakpointProps::default(),
            "nothing but the removal"
        );
        assert!(!temporary.is_plain());
        assert_eq!(temporary.look(), BreakpointLook::Conditional);

        // A free line gets one; a line with a breakpoint, temporary or plain, loses it.
        assert_eq!(toggle_temporary(&[2], 6), Some(temporary.clone()));
        assert_eq!(toggle_temporary(&[], 0), Some(temporary.clone()));
        assert_eq!(toggle_temporary(&[2, 6], 6), None);
        assert_eq!(toggle_temporary(&[2, 6], 2), None);

        // Setting it on line 6 keeps the condition of line 2; removing it again drops only its
        // own properties.
        let condition = BreakpointProps {
            condition: Some("n > 1".into()),
            ..BreakpointProps::default()
        };
        let before: BTreeMap<usize, BreakpointProps> = [(2, condition.clone())].into();
        let mut set = carry_props(&[2], &[2, 6], &before);
        set.insert(6, toggle_temporary(&[2], 6).unwrap());
        assert_eq!(set, [(2, condition.clone()), (6, temporary.clone())].into());
        assert_eq!(carry_props(&[2, 6], &[2], &set), before);

        // Saved and read back, and sent to the debugger with the rule.
        let root = std::env::temp_dir().join(format!("idecraft-bp-temp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("src/main.py");
        let lines: BTreeMap<PathBuf, Vec<usize>> = [(file.clone(), vec![2, 6])].into();
        let props: BreakpointPropsMap = [(file.clone(), set)].into();
        save_all(&root, &lines, &props);
        assert_eq!(load_all(&root), (lines, props.clone()));
        let sent = source_breakpoints(&[2, 6], props.get(&file));
        assert!(sent[1].rules.remove_once_hit && !sent[0].rules.remove_once_hit);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0303 AK-5: the two files of older versions are read and replaced by
    /// `breakpoints.toml` at the next save, each kept once as `.bak`.
    #[test]
    fn old_breakpoint_files_migrate() {
        let root = std::env::temp_dir().join(format!("idecraft-bp-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".idecraft")).unwrap();
        std::fs::write(root.join(LEGACY_BREAKPOINTS_FILE), "src/a.c:3,7\n").unwrap();
        std::fs::write(
            root.join(BREAKPOINT_PROPS_FILE),
            "{\"src/a.c\": {\"7\": {\"condition\": \"n > 1\"}}}\n",
        )
        .unwrap();
        let (lines, props) = load_all(&root);
        assert_eq!(lines[&root.join("src/a.c")], [2, 6]);
        assert_eq!(
            props[&root.join("src/a.c")][&6].condition.as_deref(),
            Some("n > 1")
        );
        save_all(&root, &lines, &props);
        assert!(root.join(BREAKPOINTS_FILE).exists());
        assert!(!root.join(LEGACY_BREAKPOINTS_FILE).exists());
        assert!(root.join(".idecraft/breakpoints.bak").exists());
        assert!(
            root.join(".idecraft/breakpoint-properties.json.bak")
                .exists()
        );
        assert_eq!(load_all(&root), (lines, props));
        let text = std::fs::read_to_string(root.join(BREAKPOINTS_FILE)).unwrap();
        assert!(text.starts_with("[idecraft]\nkind = \"breakpoints\"\nformat = 1\n"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

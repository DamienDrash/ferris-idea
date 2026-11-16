//! Grammar findings a project no longer reports (`SPEC-0155` v3 behaviour 10): "Ignore '<text>'"
//! in ⌥↩ keeps the rule and the text of a finding in the project's `.idecraft/grammar-ignore.toml`,
//! and the same rule's finding on the same text is then left out of the project's files.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::inspections::Finding;

/// The file, relative to the project folder.
pub const FILE: &str = ".idecraft/grammar-ignore.toml";

/// The rules whose findings can be ignored by their text: a repeated word and an article.
pub const RULES: [&str; 2] = ["grazie.repeated-word", "grazie.article"];

/// One ignored finding: its rule and its text, compared as [`normalize`] gives it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ignored {
    /// Rule id, e.g. `grazie.article`.
    pub rule: String,
    /// Text, e.g. `an user`.
    pub text: String,
}

impl Ignored {
    /// The entry of `rule` and `text`, the text normalized.
    pub fn new(rule: &str, text: &str) -> Self {
        Self {
            rule: rule.to_owned(),
            text: normalize(text),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Form {
    #[serde(default)]
    ignore: Vec<Ignored>,
}

/// `text` as it is compared: lower case, runs of white space as one space.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The ignore file that applies to `path`: the nearest one in `path` or a folder above it.
pub fn file_of(path: &Path) -> Option<PathBuf> {
    path.ancestors().map(|d| d.join(FILE)).find(|f| f.is_file())
}

/// The entries of a file's text; unknown fields are refused, as for the project's other files.
pub fn parse(text: &str) -> Result<BTreeSet<Ignored>, String> {
    let form: Form = toml::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
    Ok(form
        .ignore
        .into_iter()
        .map(|i| Ignored::new(&i.rule, &i.text))
        .filter(|i| !i.text.is_empty())
        .collect())
}

/// The text of a file with `entries`.
pub fn render(entries: &BTreeSet<Ignored>) -> String {
    let form = Form {
        ignore: entries.iter().cloned().collect(),
    };
    let body = toml::to_string(&form).unwrap_or_default();
    format!(
        "# Grammar findings IDECraft does not report in this project (\"Ignore\" in Alt+Enter).\n\n{body}"
    )
}

/// The entries of `file`; none when it is missing or cannot be read.
pub fn load(file: &Path) -> BTreeSet<Ignored> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| parse(&t).ok())
        .unwrap_or_default()
}

fn save(file: &Path, entries: &BTreeSet<Ignored>) -> Result<(), String> {
    let dir = file.parent().ok_or("the ignore file has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".grammar-ignore.toml.{}.tmp", std::process::id()));
    std::fs::write(&tmp, render(entries)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Adds the finding of `rule` on `text` to `file`, creating it; whether it was new. A file that
/// cannot be read as TOML is not overwritten.
pub fn add(file: &Path, rule: &str, text: &str) -> Result<bool, String> {
    let mut all = match std::fs::read_to_string(file) {
        Ok(t) => parse(&t)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(e) => return Err(e.to_string()),
    };
    let entry = Ignored::new(rule, text);
    if entry.text.is_empty() || !all.insert(entry) {
        return Ok(false);
    }
    save(file, &all).map(|()| true)
}

type Cache = HashMap<PathBuf, (Option<(SystemTime, u64)>, BTreeSet<Ignored>)>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The entries of `file`, read again when it changes.
fn cached(file: &Path) -> BTreeSet<Ignored> {
    let stamp = std::fs::metadata(file)
        .ok()
        .and_then(|m| Some((m.modified().ok()?, m.len())));
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some((s, entries)) = cache.get(file)
        && *s == stamp
    {
        return entries.clone();
    }
    let entries = load(file);
    cache.insert(file.to_path_buf(), (stamp, entries.clone()));
    entries
}

/// Whether the finding of `rule` at `range` of `text` is one of `entries`.
pub fn is_ignored(
    entries: &BTreeSet<Ignored>,
    text: &str,
    rule: &str,
    range: std::ops::Range<usize>,
) -> bool {
    RULES.contains(&rule)
        && crate::grazie::phrase(text, rule, range)
            .is_some_and(|p| entries.contains(&Ignored::new(rule, p)))
}

/// Drops the grammar findings of `text` in `path` whose text the project ignores.
pub fn filter(path: &Path, text: &str, findings: Vec<Finding>) -> Vec<Finding> {
    if !findings.iter().any(|f| RULES.contains(&f.id)) {
        return findings;
    }
    let Some(file) = file_of(path) else {
        return findings;
    };
    let entries = cached(&file);
    if entries.is_empty() {
        return findings;
    }
    findings
        .into_iter()
        .filter(|f| !is_ignored(&entries, text, f.id, f.start..f.end))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0155/AK-7 (v3): reading, writing, matching without case and with spaces folded, and
    /// the filter of a project's files.
    #[test]
    fn keeps_and_filters_ignored_texts() {
        let dir = std::env::temp_dir().join(format!("idecraft-gignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        let note = dir.join("docs/notes.txt");
        assert_eq!(file_of(&note), None);
        let file = dir.join(FILE);
        assert_eq!(add(&file, "grazie.article", "An  user"), Ok(true));
        assert_eq!(
            add(&file, "grazie.article", "an user"),
            Ok(false),
            "already kept"
        );
        assert_eq!(file_of(&note), Some(file.clone()));
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.starts_with("# Grammar findings"), "{written}");
        assert!(
            written.contains("rule = \"grazie.article\"") && written.contains("text = \"an user\""),
            "{written}"
        );
        assert_eq!(
            load(&file),
            BTreeSet::from([Ignored::new("grazie.article", "an user")])
        );
        let text = "Send an user a apple. Give an user the the file.";
        let finding = |id: &'static str, at: &str, len: usize| {
            let start = text.find(at).unwrap();
            Finding {
                id,
                start,
                end: start + len,
                level: crate::inspections::Level::Weak,
                message: String::new(),
                fix: None,
            }
        };
        let found = vec![
            finding("grazie.article", "an user", 2),
            finding("grazie.article", "a apple", 1),
            finding("grazie.repeated-word", " the file", 4),
        ];
        let kept: Vec<&str> = filter(&note, text, found)
            .iter()
            .map(|f| &text[f.start..f.end])
            .collect();
        assert_eq!(kept, ["a", " the"], "only `an user` is ignored");
        assert!(is_ignored(&load(&file), "AN USER", "grazie.article", 0..2));
        assert!(!is_ignored(&load(&file), "an user", "grazie.typo", 0..2));
        // A file that is not TOML is not overwritten.
        std::fs::write(&file, "not toml [").unwrap();
        assert!(add(&file, "grazie.article", "an hour").is_err());
        assert!(
            parse("[[ignore]]\nrule = \"grazie.article\"\ntext = \"an user\"\nextra = 1\n")
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

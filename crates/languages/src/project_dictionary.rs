//! The project's own words for the typo check (`SPEC-0163`): kept where the reference keeps its
//! project dictionary, `.idea/dictionaries/project.xml` of the project folder (format of the open
//! spell checker of `intellij-community`), so both IDEs share it.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::inspections::{Finding, Fix};

/// The project folder of `path`: the nearest folder holding `.idea`, else `.git`.
pub fn project_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let find = |marker: &str| {
        start
            .ancestors()
            .find(|d| d.join(marker).exists())
            .map(Path::to_path_buf)
    };
    find(".idea").or_else(|| find(".git"))
}

/// The dictionary file of the project of `path`.
pub fn dictionary_file(path: &Path) -> Option<PathBuf> {
    project_root(path).map(|r| r.join(".idea/dictionaries/project.xml"))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The words of a dictionary file's text (`<w>word</w>` elements).
pub fn parse(xml: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<w>") {
        rest = &rest[i + 3..];
        let Some(j) = rest.find("</w>") else { break };
        let w = unescape(rest[..j].trim());
        if !w.is_empty() {
            out.insert(w);
        }
        rest = &rest[j + 4..];
    }
    out
}

/// A dictionary file with `words`, as the reference writes it.
pub fn render(words: &BTreeSet<String>) -> String {
    let mut s = String::from(
        "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"project\">\n    <words>\n",
    );
    for w in words {
        s.push_str(&format!("      <w>{}</w>\n", escape(w)));
    }
    s.push_str("    </words>\n  </dictionary>\n</component>\n");
    s
}

type Cache = HashMap<PathBuf, (Option<SystemTime>, BTreeSet<String>)>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The project's words for `path` (lower case), read again when the file changes.
pub fn words(path: &Path) -> BTreeSet<String> {
    let Some(file) = dictionary_file(path) else {
        return BTreeSet::new();
    };
    let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some((m, w)) = cache.get(&file)
        && *m == modified
    {
        return w.clone();
    }
    let w: BTreeSet<String> = std::fs::read_to_string(&file)
        .map(|t| parse(&t).into_iter().map(|w| w.to_lowercase()).collect())
        .unwrap_or_default();
    cache.insert(file, (modified, w.clone()));
    w
}

/// Adds `word` to the project dictionary of `path` (behaviour 1); the file it was written to.
pub fn learn(path: &Path, word: &str) -> Result<PathBuf, String> {
    let file = dictionary_file(path).ok_or("no project folder (.idea or .git) above the file")?;
    learn_at(&file, word)?;
    Ok(file)
}

/// Adds `word` to the dictionary file `file`, creating it.
pub fn learn_at(file: &Path, word: &str) -> Result<(), String> {
    let file = file.to_path_buf();
    let mut all = std::fs::read_to_string(&file)
        .map(|t| parse(&t))
        .unwrap_or_default();
    all.insert(word.trim().to_lowercase());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, render(&all)).map_err(|e| e.to_string())
}

/// Drops typo findings of `text` whose word is in the project dictionary of `path` (behaviour 2),
/// and the variant findings of such a word (`SPEC-0155` v7 behaviour 7).
pub fn filter(path: &Path, text: &str, findings: Vec<Finding>) -> Vec<Finding> {
    let spelling = |id: &str| id == "grazie.typo" || crate::spelling_variants::RULES.contains(&id);
    if !findings.iter().any(|f| spelling(f.id)) {
        return findings;
    }
    let known = words(path);
    findings
        .into_iter()
        .filter(|f| {
            !spelling(f.id)
                || !text
                    .get(f.start..f.end)
                    .is_some_and(|w| known.contains(&w.to_lowercase()))
        })
        .collect()
}

/// The quick fix that saves the word of a typo finding (behaviour 1).
pub fn learn_fix(word: &str) -> Fix {
    Fix {
        title: format!("Save '{word}' to project dictionary"),
        edits: Vec::new(),
        learn: Some(word.to_owned()),
        keep: None,
        bundle_key: None,
        action: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_writes_and_filters() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-pdict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let file = dir.join("src/notes.md");
        assert_eq!(project_root(&file).as_deref(), Some(dir.as_path()));
        assert!(words(&file).is_empty());
        let written = learn(&file, "Frobnicate").unwrap();
        assert_eq!(written, dir.join(".idea/dictionaries/project.xml"));
        let xml = std::fs::read_to_string(&written).unwrap();
        assert!(xml.contains("<w>frobnicate</w>") && xml.contains("ProjectDictionaryState"));
        learn(&file, "a&b").unwrap();
        assert_eq!(
            parse(&std::fs::read_to_string(&written).unwrap()),
            BTreeSet::from(["a&b".to_owned(), "frobnicate".to_owned()])
        );
        let text = "we frobnicate and teh";
        let f = |s: usize, e: usize| Finding {
            id: "grazie.typo",
            start: s,
            end: e,
            level: crate::inspections::Level::Weak,
            message: String::new(),
            fix: None,
        };
        let kept = filter(&file, text, vec![f(3, 13), f(18, 21)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(&text[kept[0].start..kept[0].end], "teh");
        assert_eq!(learn_fix("teh").learn.as_deref(), Some("teh"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! The window's side of usage counts (`SPEC-0292` v8 behaviours 12–15): the counts kept per file
//! and text, the counts of the two questions shown together, and the declaration a click on a count
//! means.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use ferris_idea_languages::usages::UsageCount;

/// Files whose counts are kept at most (behaviour 13).
pub const KEPT_FILES: usize = 64;

/// A short fingerprint of a text: counts are kept for the text they were counted for.
fn fingerprint(text: &str) -> (u64, usize) {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    (h.finish(), text.len())
}

/// The last counts of each file with the text they were counted for, the longest unused file
/// dropped first (behaviour 13). Kept in memory only.
#[derive(Debug, Default)]
pub struct UsageCache {
    /// Least recently used first.
    entries: VecDeque<(PathBuf, (u64, usize), Vec<UsageCount>)>,
}

impl UsageCache {
    /// The counts of `path` when they were counted for exactly `text`.
    pub fn get(&mut self, path: &Path, text: &str) -> Option<Vec<UsageCount>> {
        let at = self.entries.iter().position(|(p, _, _)| p == path)?;
        let entry = self.entries.remove(at)?;
        let hit = (entry.1 == fingerprint(text)).then(|| entry.2.clone());
        self.entries.push_back(entry);
        hit
    }

    /// Keeps `counts` of `path` for `text`, in place of what was kept for the file.
    pub fn put(&mut self, path: &Path, text: &str, counts: Vec<UsageCount>) {
        self.entries.retain(|(p, _, _)| p != path);
        self.entries
            .push_back((path.to_path_buf(), fingerprint(text), counts));
        while self.entries.len() > KEPT_FILES {
            self.entries.pop_front();
        }
    }

    /// The last counts of `path`, whatever text they were counted for.
    pub fn latest(&self, path: &Path) -> Option<&[UsageCount]> {
        self.entries
            .iter()
            .find(|(p, _, _)| p == path)
            .map(|(_, _, c)| c.as_slice())
    }

    /// Forgets every file (the setting was switched off).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Files kept.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The counts of the first question (the visible declarations) and of the second (the rest)
/// together, in document order, each declaration once (behaviour 12).
pub fn merged(first: &[UsageCount], rest: &[UsageCount]) -> Vec<UsageCount> {
    let mut out: Vec<UsageCount> = rest.to_vec();
    for c in first {
        if !out
            .iter()
            .any(|o| o.line == c.line && o.character == c.character)
        {
            out.push(c.clone());
        }
    }
    out.sort_by_key(|c| (c.line, c.character));
    out
}

/// The declarations already counted, for the second question to leave out.
pub fn counted_places(counts: &[UsageCount]) -> Vec<(u64, u64)> {
    counts.iter().map(|c| (c.line, c.character)).collect()
}

/// Where the caret goes for a click on the count of `line` (behaviour 15): the char column of the
/// name of the declaration counted there, found by its name in `line_text`. Among the counts of
/// the file the one on that line comes first (the editor draws the first count of a line); after
/// edits moved the lines, the nearest one whose name stands in the line.
pub fn declaration_at(counts: &[UsageCount], line: usize, line_text: &str) -> Option<usize> {
    let mut candidates: Vec<&UsageCount> = counts
        .iter()
        .filter(|c| word_column(line_text, &c.name).is_some())
        .collect();
    candidates.sort_by_key(|c| ((c.line as i64 - line as i64).unsigned_abs(), c.character));
    candidates
        .first()
        .and_then(|c| word_column(line_text, &c.name))
}

/// The char column of the first whole-word `name` in `text`.
fn word_column(text: &str, name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut from = 0;
    while let Some(i) = text[from..].find(name).map(|i| i + from) {
        let before = text[..i].chars().next_back();
        let after = text[i + name.len()..].chars().next();
        if !before.is_some_and(word) && !after.is_some_and(word) {
            return Some(text[..i].chars().count());
        }
        from = i + name.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(name: &str, line: u64, character: u64, n: usize) -> UsageCount {
        let mut c = UsageCount::new(name, "method", line, character);
        c.count = n;
        c
    }

    /// SPEC-0292 AK-17 (v8): kept per text, the longest unused file dropped first.
    #[test]
    fn counts_are_kept_for_their_text() {
        let mut cache = UsageCache::default();
        let a = Path::new("/p/A.java");
        cache.put(a, "class A {}", vec![count("A", 0, 6, 2)]);
        assert_eq!(cache.get(a, "class A {}").unwrap()[0].count, 2);
        assert!(cache.get(a, "class A { }").is_none());
        assert_eq!(cache.latest(a).unwrap().len(), 1);
        cache.put(a, "class A { }", vec![]);
        assert!(cache.get(a, "class A {}").is_none());
        assert_eq!(cache.len(), 1);
        // 65 files: the first one, never used again, is dropped; a used one stays.
        let b = Path::new("/p/B.java");
        cache.put(b, "b", vec![count("B", 0, 0, 1)]);
        for i in 0..KEPT_FILES - 1 {
            cache.put(&PathBuf::from(format!("/p/F{i}.java")), "f", vec![]);
            if i == 10 {
                assert!(cache.get(a, "class A { }").is_some());
            }
        }
        assert_eq!(cache.len(), KEPT_FILES);
        assert!(cache.latest(b).is_none(), "B was the longest unused");
        assert!(cache.latest(a).is_some());
        cache.clear();
        assert!(cache.is_empty());
    }

    /// SPEC-0292 AK-17 (v8): both questions shown together, each declaration once, in order.
    #[test]
    fn both_questions_are_shown_together() {
        let first = [count("b", 40, 4, 1), count("c", 41, 4, 2)];
        let rest = [
            count("a", 3, 4, 5),
            count("c", 41, 4, 3),
            count("d", 90, 4, 1),
        ];
        let all = merged(&first, &rest);
        let shown: Vec<(&str, usize)> = all.iter().map(|c| (c.name.as_str(), c.count)).collect();
        assert_eq!(shown, [("a", 5), ("b", 1), ("c", 3), ("d", 1)]);
        assert_eq!(counted_places(&first), [(40, 4), (41, 4)]);
        assert_eq!(merged(&first, &[]).len(), 2);
    }

    /// SPEC-0292 AK-17 (v8): a click on a count finds its declaration's name in the line, also
    /// after the lines moved.
    #[test]
    fn a_click_finds_the_declaration() {
        let counts = [
            count("Item", 5, 18, 3),
            count("sku", 5, 30, 0),
            count("totalCents", 6, 12, 1),
        ];
        let record = "    public record Item(String sku, String name) {";
        assert_eq!(declaration_at(&counts, 5, record), Some(18));
        // Two lines inserted above: the method now stands on line 8.
        assert_eq!(
            declaration_at(&counts, 8, "        int totalCents() {"),
            Some(12)
        );
        // Whole words only.
        assert_eq!(declaration_at(&counts, 6, "  int totalCentsX() {"), None);
        assert_eq!(declaration_at(&[], 5, record), None);
    }
}

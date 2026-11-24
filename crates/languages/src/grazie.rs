//! Natural-language checks of comments, Markdown and plain text (`SPEC-0155` behaviours 7–9), as
//! the reference's bundled open Grazie plugin does: repeated words, `a`/`an` before a word and
//! misspelt words. The rules are Ferris IDEA's own. Typos are looked for with an English Hunspell word
//! list — the built-in en_US or en_GB, or one downloaded in Settings, or several of them
//! (`SPEC-0159` v4) —, plus programming
//! words, inflections, the user's learned words (`~/Library/Spelling/LocalDictionary`) and the words
//! of the file's code. Text that reads as German is left to the reader.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// One problem in prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Byte range in the checked text.
    pub range: Range<usize>,
    /// `grazie.repeated-word`, `grazie.article`, `grazie.typo`, `grazie.other-variant` or
    /// `grazie.mixed-spelling` (`SPEC-0155` v7).
    pub rule: &'static str,
    /// Message for the user.
    pub message: String,
    /// Text that fixes it (empty deletes the range), when there is one.
    pub replacement: Option<String>,
    /// For a typo: up to five replacements, the first being `replacement` (`SPEC-0155` v5
    /// behaviour 7).
    pub suggestions: Vec<String>,
}

/// The dictionary codes chosen in the settings (empty: the first installed).
static CHOSEN: RwLock<String> = RwLock::new(String::new());

/// Counts the changes of [`CHOSEN`] (`SPEC-0159` v5 behaviour 3).
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Chooses the dictionaries for the typo check (`SPEC-0159` v4 behaviour 3): one code or several
/// separated by commas; empty for the first installed one. A different choice than before counts
/// as a change of [`dictionary_generation`].
pub fn choose_dictionary(code: &str) {
    if let Ok(mut c) = CHOSEN.write()
        && *c != code
    {
        code.clone_into(&mut c);
        GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The dictionary codes chosen now, as [`choose_dictionary`] got them.
pub fn chosen_dictionary() -> String {
    CHOSEN.read().map(|c| c.clone()).unwrap_or_default()
}

/// How often the choice of dictionaries has changed in this process: open editors are checked
/// again when it moves (`SPEC-0159` v5 behaviour 3).
pub fn dictionary_generation() -> u64 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Which variant findings the chosen dictionaries give (`SPEC-0159` v5 behaviour 8).
pub fn variant_mode() -> crate::spelling_variants::Mode {
    variant_mode_for(&chosen_dictionary())
}

/// Which variant findings the dictionary codes `chosen` (as in the setting) give.
pub fn variant_mode_for(chosen: &str) -> crate::spelling_variants::Mode {
    if !TYPOS.load(std::sync::atomic::Ordering::Relaxed) {
        return crate::spelling_variants::Mode::Off;
    }
    let dir = crate::dictionaries::dictionaries_dir();
    crate::spelling_variants::Mode::of_codes(&crate::dictionaries::active_all(
        dir.as_deref(),
        chosen,
    ))
}

/// The scope of the checks chosen in Settings › Editor › Natural Languages › Grammar and Style
/// (`SPEC-0155` v8 behaviour 14); `None` until the window or a tool sets it: the defaults.
static SCOPE: RwLock<Option<ferris_idea_core::GrammarSettings>> = RwLock::new(None);

/// Sets the scope of the grammar and spelling checks for this process; a different scope than
/// before counts as a change of [`dictionary_generation`], so open editors are checked again.
pub fn set_grammar_settings(settings: &ferris_idea_core::GrammarSettings) {
    if let Ok(mut s) = SCOPE.write()
        && s.as_ref()
            .unwrap_or(&ferris_idea_core::GrammarSettings::default())
            != settings
    {
        *s = Some(settings.clone());
        GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The scope in effect.
pub fn grammar_settings() -> ferris_idea_core::GrammarSettings {
    SCOPE
        .read()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

/// The file types the scope lists (`SPEC-0155` v8 behaviour 14): every language by the name the
/// status bar shows, then `Plain text`, sorted.
pub fn grammar_file_types() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = crate::LanguageId::ALL.iter().map(|l| l.title()).collect();
    out.push(PLAIN_TEXT);
    out.sort_unstable();
    out.dedup();
    out
}

/// The file type of `.txt` files and files without an extension in the scope.
pub const PLAIN_TEXT: &str = "Plain text";

/// Typos are looked for (tests switch them off to be independent of the machine's dictionaries).
static TYPOS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Switches the typo check on or off for this process.
pub fn set_typos(on: bool) {
    TYPOS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The word lists the typo check reads (`SPEC-0159` v4 behaviour 3): the chosen dictionaries that
/// are downloaded or built in, else the first downloaded one, else the built-in en_US; `None` only
/// while the check is switched off.
pub fn word_list() -> Option<Vec<crate::dictionaries::WordList>> {
    let chosen = CHOSEN.read().map(|c| c.clone()).unwrap_or_default();
    word_lists_for(&chosen)
}

/// The word lists of the dictionary codes `chosen` (as in the setting); `None` while the check is
/// switched off.
fn word_lists_for(chosen: &str) -> Option<Vec<crate::dictionaries::WordList>> {
    if !TYPOS.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let dir = crate::dictionaries::dictionaries_dir();
    Some(crate::dictionaries::word_lists(dir.as_deref(), chosen))
}

/// Words that may follow themselves in a correct sentence (`SPEC-0155` v8 behaviour 7): "I said
/// that that was fine", "she had had enough". Every other repetition is reported, `is is` too.
pub const MAY_REPEAT: [&str; 2] = ["that", "had"];

/// Words of programming prose the dictionary lacks.
const PROGRAMMING_WORDS: &[&str] = &[
    "api",
    "apis",
    "app",
    "apps",
    "args",
    "arg",
    "async",
    "auth",
    "backend",
    "boolean",
    "bool",
    "browser",
    "builtin",
    "bytecode",
    "callback",
    "checkbox",
    "classpath",
    "cli",
    "codebase",
    "config",
    "configs",
    "const",
    "cpu",
    "css",
    "csv",
    "dataset",
    "debug",
    "debugger",
    "decrement",
    "deps",
    "deserialize",
    "dev",
    "dict",
    "dir",
    "dirs",
    "dns",
    "docs",
    "dropdown",
    "email",
    "emails",
    "enum",
    "enums",
    "env",
    "filename",
    "filesystem",
    "frontend",
    "func",
    "getter",
    "github",
    "gpu",
    "hashmap",
    "html",
    "http",
    "https",
    "init",
    "inline",
    "int",
    "io",
    "iterator",
    "javascript",
    "json",
    "keybinding",
    "keyword",
    "lambda",
    "linter",
    "localhost",
    "login",
    "logout",
    "lookup",
    "macos",
    "metadata",
    "middleware",
    "mutex",
    "namespace",
    "nullable",
    "offline",
    "online",
    "param",
    "params",
    "parser",
    "pathname",
    "plugin",
    "plugins",
    "popup",
    "prepend",
    "println",
    "pre",
    "repo",
    "repos",
    "runtime",
    "serialize",
    "setter",
    "sql",
    "src",
    "stderr",
    "stdin",
    "stdout",
    "struct",
    "structs",
    "subclass",
    "subfolder",
    "submodule",
    "syntax",
    "tab",
    "tooltip",
    "toolbar",
    "tuple",
    "typedef",
    "ui",
    "unicode",
    "unix",
    "url",
    "urls",
    "username",
    "utf",
    "uuid",
    "validator",
    "vec",
    "website",
    "whitespace",
    "wildcard",
    "workflow",
    "workspace",
    "xml",
    "yaml",
    "todo",
    "fixme",
    "ok",
    "okay",
    "etc",
    "keymap",
    "keymaps",
    "keychain",
    "keystroke",
    "lockfile",
    "lockfiles",
    "toolchain",
    "worktree",
    "worktrees",
    "stdio",
    "semver",
    "standalone",
    "initializer",
    "analyzer",
    "fixup",
    "scrollback",
    "scrollbar",
    "viewport",
    "symlink",
    "symlinks",
    "varargs",
    "monospace",
    "monospaced",
    "lifecycle",
    "accessor",
    "accessors",
    "comparator",
    "nullability",
    "breakpoint",
    "breakpoints",
    "logpoint",
    "logpoints",
    "callee",
    "callees",
    "inode",
    "backticks",
    "roadmap",
    "subcommand",
    "codegen",
    "refactoring",
    "refactorings",
    "dialog",
    "dialogs",
];

/// A word list for the typo check.
pub struct Dictionary {
    words: HashSet<String>,
    /// Suggestions already looked for, by word and limit: the background inspection of the
    /// editor and ⌥↩ ask for the same typo, and the search two edits away is the slow part
    /// (`SPEC-0076` v12 behaviour 18). The words never change after the list is read.
    memo: Mutex<HashMap<(String, usize), Vec<String>>>,
    /// The words by length with their letters, built at the first search or by [`prepare`]
    /// (`SPEC-0155` v6 behaviour 7).
    index: OnceLock<SuggestionIndex>,
}

/// How many words keep their suggestions; the memo starts over when it is full.
const MEMO_WORDS: usize = 1024;

impl Dictionary {
    /// A dictionary of `words` (lowercase) and the programming words.
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(words: I) -> Self {
        let mut set: HashSet<String> = words
            .into_iter()
            .map(|w| w.as_ref().trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        set.extend(PROGRAMMING_WORDS.iter().map(|w| (*w).to_owned()));
        Self {
            words: set,
            memo: Mutex::new(HashMap::new()),
            index: OnceLock::new(),
        }
    }

    /// Words of a Hunspell `.dic` file: the first line is the count, then `word/FLAGS`.
    pub fn from_hunspell(dic: &str) -> Self {
        Self::from_hunspell_with(dic, "")
    }

    /// Words of a Hunspell `.dic` file with the prefixes and suffixes of its `.aff` file applied
    /// (`PFX`/`SFX` rules with their conditions; one-character flags), so `put/I` also gives
    /// `input` (`SPEC-0159` behaviour 4).
    pub fn from_hunspell_with(dic: &str, aff: &str) -> Self {
        let rules = Affixes::parse(aff);
        let mut words = Vec::new();
        for line in dic.lines().skip(1) {
            let line = line.split(['\t', ' ']).next().unwrap_or_default();
            let (word, flags) = line.split_once('/').unwrap_or((line, ""));
            if word.is_empty() {
                continue;
            }
            words.push(word.to_owned());
            let flags: Vec<char> = flags.chars().collect();
            let mut suffixed = Vec::new();
            for f in &flags {
                for r in rules.suffixes.iter().filter(|r| r.flag == *f) {
                    if let Some(w) = r.apply_suffix(word) {
                        suffixed.push((w, r.cross));
                    }
                }
            }
            for f in &flags {
                for r in rules.prefixes.iter().filter(|r| r.flag == *f) {
                    if let Some(w) = r.apply_prefix(word) {
                        words.push(w);
                    }
                    if r.cross {
                        for (sw, cross) in &suffixed {
                            if *cross && let Some(w) = r.apply_prefix(sw) {
                                words.push(w);
                            }
                        }
                    }
                }
            }
            words.extend(suffixed.into_iter().map(|(w, _)| w));
        }
        Self::new(words)
    }

    /// The word list the typo check uses (`SPEC-0159` v4 behaviour 3): the dictionaries chosen in
    /// Settings › Editor › Natural Languages or the first one downloaded there, else the built-in
    /// en_US; read once per choice and kept. `None` while the check is off or when no list can be
    /// read (then no typos are reported).
    pub fn current() -> Option<Arc<Dictionary>> {
        Self::cached(word_list()?)
    }

    /// The word list of the dictionary codes `codes` (one or several separated by commas, as in
    /// the setting) instead of the chosen ones (`SPEC-0159` v4 behaviour 5, `check_grammar`).
    pub fn for_codes(codes: &str) -> Option<Arc<Dictionary>> {
        Self::cached(word_lists_for(codes)?)
    }

    /// The words of `lists` together (a word is known when one list knows it), read once and kept
    /// for the last few choices.
    fn cached(lists: Vec<crate::dictionaries::WordList>) -> Option<Arc<Dictionary>> {
        use crate::dictionaries::WordList;
        type Cache = Vec<(Vec<WordList>, Arc<Dictionary>)>;
        static CACHE: Mutex<Cache> = Mutex::new(Vec::new());
        /// How many choices stay read: the window's and one other (`check_grammar`).
        const KEPT: usize = 2;
        let mut cache = CACHE.lock().ok()?;
        if let Some(at) = cache.iter().position(|(l, _)| *l == lists) {
            let hit = cache.remove(at);
            let dict = hit.1.clone();
            cache.insert(0, hit);
            return Some(dict);
        }
        let mut dict: Option<Dictionary> = None;
        for list in &lists {
            let read = match list {
                WordList::File(file) => {
                    let Ok(text) = std::fs::read_to_string(file) else {
                        continue;
                    };
                    let aff =
                        std::fs::read_to_string(file.with_extension("aff")).unwrap_or_default();
                    Dictionary::from_hunspell_with(&text, &aff)
                }
                WordList::BuiltIn(code) => {
                    let Some((dic, aff)) = crate::dictionaries::built_in(code) else {
                        continue;
                    };
                    Dictionary::from_hunspell_with(dic, aff)
                }
            };
            match dict.as_mut() {
                Some(d) => d.words.extend(read.words),
                None => dict = Some(read),
            }
        }
        let mut dict = dict?;
        if let Some(learned) = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Spelling/LocalDictionary"))
            .and_then(|p| std::fs::read_to_string(p).ok())
        {
            dict.words
                .extend(learned.lines().map(|w| w.trim().to_lowercase()));
        }
        let dict = Arc::new(dict);
        cache.insert(0, (lists, dict.clone()));
        cache.truncate(KEPT);
        Some(dict)
    }

    fn has(&self, w: &str) -> bool {
        self.words.contains(w)
    }

    /// The words of the list, lowercase (`SPEC-0159` v5 behaviour 8 compares two lists).
    pub(crate) fn word_set(&self) -> &HashSet<String> {
        &self.words
    }

    /// Whether `word` (lowercase) is a word: listed, an inflection of a listed word, or a listed
    /// word with a common prefix.
    pub fn knows(&self, word: &str) -> bool {
        if self.has(word) {
            return true;
        }
        let w = word.strip_suffix("'s").unwrap_or(word);
        if w != word && self.knows(w) {
            return true;
        }
        let stems = |suffix: &str, adds: &[&str]| -> bool {
            let Some(base) = w.strip_suffix(suffix) else {
                return false;
            };
            if base.len() < 2 {
                return false;
            }
            if adds.iter().any(|a| self.has(&format!("{base}{a}"))) {
                return true;
            }
            // stopped → stop, running → run
            let b = base.as_bytes();
            b.len() >= 3 && b[b.len() - 1] == b[b.len() - 2] && self.has(&base[..base.len() - 1])
        };
        let inflected = stems("s", &[""])
            || stems("es", &[""])
            || stems("ies", &["y"])
            || stems("ed", &["", "e"])
            || stems("ied", &["y"])
            || stems("ing", &["", "e"])
            || stems("ly", &["", "le"])
            || stems("ily", &["y"])
            || stems("er", &["", "e"])
            || stems("ers", &["", "e"])
            || stems("est", &["", "e"])
            || stems("ier", &["y"])
            || stems("ness", &[""])
            || stems("ment", &[""])
            || stems("ments", &[""])
            || stems("able", &["", "e"])
            || stems("ful", &[""])
            || stems("less", &[""])
            || stems("ize", &["", "e"])
            || stems("ized", &["", "e"])
            || stems("izes", &["", "e"])
            || stems("ization", &["", "e", "ize"])
            || stems("ation", &["e", "", "ate"])
            || stems("ations", &["e", "", "ate"]);
        if inflected {
            return true;
        }
        [
            "un", "re", "pre", "non", "sub", "over", "multi", "de", "dis", "mis", "auto",
        ]
        .iter()
        .any(|p| {
            w.strip_prefix(p)
                .is_some_and(|rest| rest.len() >= 3 && self.knows(rest))
        })
    }

    /// The best suggestion for `word`: the first of [`Self::suggestions`].
    pub fn suggest(&self, word: &str) -> Option<String> {
        self.suggestions(word, 1).into_iter().next()
    }

    /// Up to `limit` listed words for the typo `word` (`SPEC-0155` v5 behaviour 7): those one edit
    /// away (a swap of neighbours, a deletion, a replacement, an insertion), then — while fewer
    /// than `limit` — those two edits away; within a distance the more common word first by the
    /// frequency classes of `SPEC-0159` v3, words of no class last, then alphabetically.
    pub fn suggestions(&self, word: &str, limit: usize) -> Vec<String> {
        let key = (word.to_owned(), limit);
        if let Some(known) = self.memo.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return known;
        }
        let found = self.search(word, limit);
        if let Ok(mut memo) = self.memo.lock() {
            if memo.len() >= MEMO_WORDS {
                memo.clear();
            }
            memo.insert(key, found.clone());
        }
        found
    }

    /// Builds the index the suggestions are looked up in, unless it is built already; [`prepare`]
    /// calls it off the UI thread so the first ⌥↩ does not wait for it (`SPEC-0155` v6
    /// behaviour 7).
    pub fn prepare_suggestions(&self) {
        let _ = self.index();
    }

    fn index(&self) -> &SuggestionIndex {
        self.index.get_or_init(|| SuggestionIndex::new(&self.words))
    }

    /// The listed words one and two edits away from `word` (`SPEC-0155` v6 behaviour 7): only the
    /// words whose length differs by at most two and whose letters differ in at most four are
    /// compared, each with an edit distance. It finds the same words as trying every string one
    /// and two edits away, in milliseconds instead of a few hundred.
    fn search(&self, word: &str, limit: usize) -> Vec<String> {
        let typo: Vec<char> = word.chars().collect();
        let rank = |mut found: Vec<String>| {
            found.sort_by(|a, b| {
                (frequency_class(a).unwrap_or(u8::MAX), a)
                    .cmp(&(frequency_class(b).unwrap_or(u8::MAX), b))
            });
            found
        };
        let mut out = rank(self.within(&typo, word, 1));
        // Two edits away only while fewer than `limit` are found, and for words of up to twelve
        // letters, as in v5.
        if out.len() < limit && typo.len() <= 12 {
            out.extend(rank(self.within(&typo, word, 2)));
        }
        out.truncate(limit);
        out
    }

    /// The listed words exactly `distance` (1 or 2) steps from `typo` (`word` as characters).
    fn within(&self, typo: &[char], word: &str, distance: u8) -> Vec<String> {
        let reach = distance as usize;
        let (mask, counts) = (letters_mask(typo), letter_counts(typo));
        // The strings one step from the typo, made only for a word with a character no step adds.
        let mut steps: Option<Vec<Vec<char>>> = None;
        let mut found = Vec::new();
        let mut chars = Vec::new();
        for len in typo.len().saturating_sub(reach)..=typo.len() + reach {
            let Some(bucket) = self.index().by_len.get(len) else {
                continue;
            };
            for e in bucket {
                if (e.mask ^ mask).count_ones() as usize > 2 * reach
                    || counts_apart(&e.counts, &counts) > 2 * reach
                    || e.word.len() < 2
                    || e.word == word
                {
                    continue;
                }
                chars.clear();
                chars.extend(e.word.chars());
                if steps_apart(typo, &chars, reach, &mut steps) == distance {
                    found.push(e.word.clone());
                }
            }
        }
        found
    }

    /// The search of `SPEC-0155` v5: every string one and two edits away looked up in the list.
    /// Kept to show that [`Self::search`] finds the same words.
    #[cfg(test)]
    fn search_every_edit(&self, word: &str, limit: usize) -> Vec<String> {
        let near = edits(word);
        let known = |w: &&String| w.len() >= 2 && w.as_str() != word && self.has(w);
        let rank = |mut found: Vec<String>| {
            found.sort_by(|a, b| {
                (frequency_class(a).unwrap_or(u8::MAX), a)
                    .cmp(&(frequency_class(b).unwrap_or(u8::MAX), b))
            });
            found
        };
        let mut out = rank(near.iter().filter(known).cloned().collect());
        if out.len() < limit && word.chars().count() <= 12 {
            let mut far: HashSet<String> = HashSet::new();
            for n in &near {
                for w in edits(n) {
                    if known(&&w) && !near.contains(&w) {
                        far.insert(w);
                    }
                }
            }
            out.extend(rank(far.into_iter().collect()));
        }
        out.truncate(limit);
        out
    }
}

/// Reads the word list of the typo check, builds its suggestion index and the frequency classes
/// (`SPEC-0155` v6 behaviour 7). The window calls it on a background thread at start and when the
/// dictionary changes, so neither the editor's first inspection nor ⌥↩ waits for it.
pub fn prepare() {
    if let Some(d) = Dictionary::current() {
        d.prepare_suggestions();
    }
    let _ = frequency_class("");
}

/// How many suggestions ⌥↩ lists for a typo (`SPEC-0155` v5 behaviour 7).
pub const MAX_SUGGESTIONS: usize = 5;

/// The built-in frequency classes of English words (`SPEC-0159` v3 behaviour 7), from SCOWL.
const FREQUENCY: &str = include_str!("../dictionaries/en_frequency.txt");

/// The frequency class of `word` (`SPEC-0159` v3 behaviour 7): 1 for the most common English words,
/// then 2 and 3; `None` for a word of no class (rarer).
pub fn frequency_class(word: &str) -> Option<u8> {
    static CLASSES: OnceLock<HashMap<&'static str, u8>> = OnceLock::new();
    let classes = CLASSES.get_or_init(|| {
        let mut map = HashMap::new();
        let mut class = 0u8;
        for line in FREQUENCY.lines() {
            if let Some(n) = line.strip_prefix("# class ") {
                class = n.trim().parse().unwrap_or(class);
            } else if !line.starts_with('#') && !line.is_empty() && class > 0 {
                map.entry(line).or_insert(class);
            }
        }
        map
    });
    match classes.get(word) {
        Some(c) => Some(*c),
        None => classes.get(word.to_lowercase().as_str()).copied(),
    }
}

/// One word of a [`SuggestionIndex`].
struct Entry {
    word: String,
    /// [`letters_mask`] of the word.
    mask: u32,
    /// [`letter_counts`] of the word.
    counts: [u8; 27],
}

/// The words of a dictionary by their length in characters.
struct SuggestionIndex {
    by_len: Vec<Vec<Entry>>,
}

impl SuggestionIndex {
    fn new(words: &HashSet<String>) -> Self {
        let mut by_len: Vec<Vec<Entry>> = Vec::new();
        let mut chars = Vec::new();
        for w in words {
            chars.clear();
            chars.extend(w.chars());
            if by_len.len() <= chars.len() {
                by_len.resize_with(chars.len() + 1, Vec::new);
            }
            by_len[chars.len()].push(Entry {
                word: w.clone(),
                mask: letters_mask(&chars),
                counts: letter_counts(&chars),
            });
        }
        Self { by_len }
    }
}

/// The slot of `c` in [`letters_mask`] and [`letter_counts`]: 0–25 for `a`–`z`, 26 for any other.
fn slot(c: char) -> usize {
    if c.is_ascii_lowercase() {
        c as usize - 'a' as usize
    } else {
        26
    }
}

/// Which of `a`–`z` occur in `word`, one bit each, and bit 26 for any other character. One edit
/// changes at most two bits, two edits at most four.
fn letters_mask(word: &[char]) -> u32 {
    word.iter().fold(0, |m, c| m | 1 << slot(*c))
}

/// How often each of `a`–`z` occurs in `word`, and any other character in the last place.
fn letter_counts(word: &[char]) -> [u8; 27] {
    let mut counts = [0u8; 27];
    for c in word {
        let s = slot(*c);
        counts[s] = counts[s].saturating_add(1);
    }
    counts
}

/// The characters added and taken away between two [`letter_counts`]: a swap changes none, a
/// left-out or added character one, a replaced one two.
fn counts_apart(a: &[u8; 27], b: &[u8; 27]) -> usize {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as usize).sum()
}

/// The number of edits that turn `typo` into `word`, capped at 3: neighbours swapped, a character
/// left out, replaced or added — only `a`–`z` are added or put in unless `any` — (the
/// Damerau–Levenshtein distance, where a swap with edits between the swapped characters counts
/// them too). With only `a`–`z` added it can exceed the steps for a character outside them moved
/// by two swaps; it never falls below them.
fn edit_distance(typo: &[char], word: &[char], any: bool) -> u8 {
    const CAP: u8 = 3;
    const NONE: u8 = 100;
    let (n, m) = (typo.len(), word.len());
    if n.abs_diff(m) >= CAP as usize {
        return CAP;
    }
    let put = |c: char| {
        if any || c.is_ascii_lowercase() {
            1u8
        } else {
            NONE
        }
    };
    // d[i + 1][j + 1]: the distance of typo[..i] to word[..j]; row and column 0 hold NONE.
    let width = m + 2;
    let mut d = vec![NONE; (n + 2) * width];
    let at = |i: usize, j: usize| i * width + j;
    d[at(1, 1)] = 0;
    for i in 1..=n {
        d[at(i + 1, 1)] = i.min(NONE as usize) as u8;
    }
    for j in 1..=m {
        d[at(1, j + 1)] = d[at(1, j)].saturating_add(put(word[j - 1])).min(NONE);
    }
    // The last row of `typo` holding each character met so far.
    let mut last_row: Vec<(char, usize)> = Vec::new();
    for i in 1..=n {
        let mut last_col = 0;
        for j in 1..=m {
            let k = last_row
                .iter()
                .find(|(c, _)| *c == word[j - 1])
                .map_or(0, |(_, r)| *r);
            let l = last_col;
            let same = typo[i - 1] == word[j - 1];
            if same {
                last_col = j;
            }
            let replace = if same { 0 } else { put(word[j - 1]) };
            let mut best = d[at(i, j)]
                .saturating_add(replace)
                .min(d[at(i + 1, j)].saturating_add(put(word[j - 1])))
                .min(d[at(i, j + 1)].saturating_add(1));
            if k > 0 && l > 0 {
                let between = word[l..j - 1]
                    .iter()
                    .fold(0u8, |s, c| s.saturating_add(put(*c)));
                let swap = d[at(k, l)]
                    .saturating_add((i - k - 1).min(NONE as usize) as u8)
                    .saturating_add(1)
                    .saturating_add(between);
                best = best.min(swap);
            }
            d[at(i + 1, j + 1)] = best.min(NONE);
        }
        match last_row.iter_mut().find(|(c, _)| *c == typo[i - 1]) {
            Some(e) => e.1 = i,
            None => last_row.push((typo[i - 1], i)),
        }
    }
    d[at(n + 1, m + 1)].min(CAP)
}

/// The number of steps of `SPEC-0155` v5 behaviour 7 from `typo` to `word`, capped at 3; with
/// `reach` 1 only one step is told apart exactly. `steps` keeps the strings one step from the typo once they are needed: a
/// character outside `a`–`z` moved by two swaps is two steps, which [`edit_distance`] does not
/// see.
fn steps_apart(
    typo: &[char],
    word: &[char],
    reach: usize,
    steps: &mut Option<Vec<Vec<char>>>,
) -> u8 {
    let distance = edit_distance(typo, word, false);
    if distance > 2
        && reach == 2
        && word.iter().any(|c| !c.is_ascii_lowercase())
        && edit_distance(typo, word, true) <= 2
    {
        let steps =
            steps.get_or_insert_with(|| one_step(typo).into_iter().filter(|s| s != typo).collect());
        if steps.iter().any(|s| edit_distance(s, word, false) == 1) {
            return 2;
        }
    }
    distance
}

/// The strings one edit away from `word` as characters (see [`edits`]).
fn one_step(word: &[char]) -> Vec<Vec<char>> {
    let mut found: Vec<Vec<char>> = Vec::new();
    for i in 0..word.len().saturating_sub(1) {
        let mut c = word.to_vec();
        c.swap(i, i + 1);
        found.push(c);
    }
    for i in 0..word.len() {
        let mut c = word.to_vec();
        c.remove(i);
        found.push(c);
    }
    for i in 0..word.len() {
        for l in 'a'..='z' {
            if l != word[i] {
                let mut c = word.to_vec();
                c[i] = l;
                found.push(c);
            }
        }
    }
    for i in 0..=word.len() {
        for l in 'a'..='z' {
            let mut c = word.to_vec();
            c.insert(i, l);
            found.push(c);
        }
    }
    found
}

/// The strings one edit away from `word`: neighbours swapped, a letter left out, replaced or
/// added (`a`–`z`), without duplicates.
#[cfg(test)]
fn edits(word: &str) -> HashSet<String> {
    let chars: Vec<char> = word.chars().collect();
    one_step(&chars)
        .into_iter()
        .map(|c| c.into_iter().collect())
        .collect()
}

/// One prefix or suffix rule of a Hunspell `.aff` file.
#[derive(Debug, Clone)]
struct AffixRule {
    flag: char,
    cross: bool,
    strip: String,
    add: String,
    /// Condition elements: a set of characters and whether it is negated (`.` is an empty negated
    /// set).
    condition: Vec<(Vec<char>, bool)>,
}

fn parse_condition(c: &str) -> Vec<(Vec<char>, bool)> {
    let mut out = Vec::new();
    let mut chars = c.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '.' => out.push((Vec::new(), true)),
            '[' => {
                let negated = chars.peek() == Some(&'^');
                if negated {
                    chars.next();
                }
                let mut set = Vec::new();
                for x in chars.by_ref() {
                    if x == ']' {
                        break;
                    }
                    set.push(x);
                }
                out.push((set, negated));
            }
            other => out.push((vec![other], false)),
        }
    }
    out
}

fn condition_matches(cond: &[(Vec<char>, bool)], part: &[char]) -> bool {
    cond.len() == part.len()
        && cond
            .iter()
            .zip(part)
            .all(|((set, negated), c)| set.contains(c) != *negated)
}

impl AffixRule {
    fn apply_suffix(&self, word: &str) -> Option<String> {
        let chars: Vec<char> = word.chars().collect();
        let n = self.condition.len();
        if chars.len() < n || !condition_matches(&self.condition, &chars[chars.len() - n..]) {
            return None;
        }
        let base = word.strip_suffix(self.strip.as_str())?;
        Some(format!("{base}{}", self.add))
    }

    fn apply_prefix(&self, word: &str) -> Option<String> {
        let chars: Vec<char> = word.chars().collect();
        let n = self.condition.len();
        if chars.len() < n || !condition_matches(&self.condition, &chars[..n]) {
            return None;
        }
        let rest = word.strip_prefix(self.strip.as_str())?;
        Some(format!("{}{rest}", self.add))
    }
}

#[derive(Debug, Default)]
struct Affixes {
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
}

impl Affixes {
    fn parse(aff: &str) -> Self {
        let mut out = Affixes::default();
        let mut cross = std::collections::HashMap::new();
        for line in aff.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let (Some(kind), Some(flag)) = (parts.first(), parts.get(1)) else {
                continue;
            };
            if !matches!(*kind, "PFX" | "SFX") {
                continue;
            }
            let mut fc = flag.chars();
            let (Some(flag), None) = (fc.next(), fc.next()) else {
                continue;
            };
            // Header: `PFX A Y 1`.
            if parts.len() == 4
                && matches!(parts[2], "Y" | "N")
                && parts[3].parse::<usize>().is_ok()
            {
                cross.insert((*kind, flag), parts[2] == "Y");
                continue;
            }
            if parts.len() < 5 {
                continue;
            }
            let none = |s: &str| {
                if s == "0" {
                    String::new()
                } else {
                    s.to_owned()
                }
            };
            let add = parts[3].split('/').next().unwrap_or_default();
            let rule = AffixRule {
                flag,
                cross: cross.get(&(*kind, flag)).copied().unwrap_or(false),
                strip: none(parts[2]),
                add: none(add),
                condition: parse_condition(parts[4]),
            };
            if *kind == "PFX" {
                out.prefixes.push(rule);
            } else {
                out.suffixes.push(rule);
            }
        }
        out
    }
}

/// A word of prose: byte range and text.
struct Word<'t> {
    range: Range<usize>,
    text: &'t str,
    /// Part of a chunk that looks like code (`snake_case`, `a.b`, `x()`, digits, camelCase).
    code: bool,
}

fn looks_like_code(chunk: &str) -> bool {
    let inner = chunk.trim_matches(|c: char| ".,;:!?()\"'«»“”‘’".contains(c));
    inner
        .chars()
        .any(|c| c.is_ascii_digit() || "_./\\:()[]{}<>=#@$`*|&%^~+".contains(c))
        || inner.char_indices().skip(1).any(|(_, c)| c.is_uppercase())
}

fn words(text: &str, range: Range<usize>) -> Vec<Word<'_>> {
    let mut out = Vec::new();
    let part = &text[range.clone()];
    let mut offset = 0;
    for chunk in part.split_inclusive(char::is_whitespace) {
        let chunk_start = range.start + offset;
        offset += chunk.len();
        let trimmed = chunk.trim_end();
        let code = looks_like_code(trimmed);
        let mut i = 0;
        let bytes: Vec<(usize, char)> = trimmed.char_indices().collect();
        while i < bytes.len() {
            if !bytes[i].1.is_alphabetic() {
                i += 1;
                continue;
            }
            let start = bytes[i].0;
            let mut j = i;
            while j < bytes.len()
                && (bytes[j].1.is_alphabetic()
                    || (bytes[j].1 == '\''
                        && bytes.get(j + 1).is_some_and(|(_, c)| c.is_alphabetic())))
            {
                j += 1;
            }
            let end = bytes.get(j).map_or(trimmed.len(), |(b, _)| *b);
            out.push(Word {
                range: chunk_start + start..chunk_start + end,
                text: &trimmed[start..end],
                code,
            });
            i = j;
        }
    }
    out
}

/// Whether `a` (not `an`) goes before `word`; `None` when it cannot be told from the letters
/// (a word without vowels is spelt out: `an npm`, `a ssh` both occur).
fn takes_a(word: &str) -> Option<bool> {
    let w = word.to_lowercase();
    if !w.chars().any(|c| "aeiouy".contains(c)) {
        return None;
    }
    Some(takes_a_letters(&w))
}

fn takes_a_letters(w: &str) -> bool {
    let first = w.chars().next().unwrap_or('x');
    if "aeio".contains(first) {
        return ["one", "once", "eu"].iter().any(|p| w.starts_with(p));
    }
    if first == 'u' {
        return (w.starts_with("us") && !w.starts_with("ush"))
            || ["uni", "uti", "ura", "ure", "uro", "ubi", "uku"]
                .iter()
                .any(|p| w.starts_with(p));
    }
    if first == 'h' {
        return !["hour", "honest", "honor", "honour", "heir"]
            .iter()
            .any(|p| w.starts_with(p));
    }
    true
}

const ENGLISH: [&str; 12] = [
    "the", "and", "is", "of", "to", "with", "for", "this", "that", "are", "when", "from",
];
const GERMAN: [&str; 16] = [
    "der", "die", "das", "und", "ist", "nicht", "mit", "für", "ein", "eine", "auf", "zu", "den",
    "dem", "im", "wird",
];

/// English and German stop words in `ranges`.
fn stopwords(text: &str, ranges: impl Iterator<Item = Range<usize>>) -> (usize, usize) {
    let (mut en, mut de) = (0, 0);
    for r in ranges {
        for w in text[r].split(|c: char| !c.is_alphabetic()) {
            let w = w.to_lowercase();
            en += usize::from(ENGLISH.contains(&w.as_str()));
            de += usize::from(GERMAN.contains(&w.as_str()));
        }
    }
    (en, de)
}

/// Whether the prose reads as English rather than German (ties and no evidence: English).
fn reads_english(text: &str, ranges: impl Iterator<Item = Range<usize>>) -> bool {
    let (en, de) = stopwords(text, ranges);
    en >= de
}

/// The problems in the prose `ranges` (byte ranges) of `text`. `dictionary` enables the typo check;
/// `known` are further words to accept (the identifiers of the file, lowercase). No variant
/// findings: [`check_with`] gives them.
pub fn check(
    text: &str,
    ranges: &[Range<usize>],
    dictionary: Option<&Dictionary>,
    known: &HashSet<String>,
) -> Vec<Issue> {
    check_with(
        text,
        ranges,
        dictionary,
        known,
        crate::spelling_variants::Mode::Off,
    )
}

/// [`check`] with the variant findings of `variants` (`SPEC-0155` v7 behaviour 7), which belongs
/// to the dictionaries of `dictionary`: [`variant_mode`] for [`Dictionary::current`],
/// [`variant_mode_for`] for [`Dictionary::for_codes`].
pub fn check_with(
    text: &str,
    ranges: &[Range<usize>],
    dictionary: Option<&Dictionary>,
    known: &HashSet<String>,
    variants: crate::spelling_variants::Mode,
) -> Vec<Issue> {
    use crate::spelling_variants::{Candidate, Mode};
    let mut out = Vec::new();
    let mut spellings: Vec<Candidate> = Vec::new();
    let document_english = reads_english(text, ranges.iter().cloned());
    for range in ranges {
        let ws = words(text, range.clone());
        let english = match stopwords(text, std::iter::once(range.clone())) {
            (en, de) if en + de >= 3 => en >= de,
            _ => document_english,
        };
        for (k, w) in ws.iter().enumerate() {
            let next = ws.get(k + 1);
            let between = next.map(|n| &text[w.range.end..n.range.start]);
            let spaced = between.is_some_and(|b| !b.is_empty() && b.chars().all(|c| c == ' '));
            // Repeated word.
            if let Some(n) = next
                && spaced
                && !w.code
                && !n.code
                && w.text.eq_ignore_ascii_case(n.text)
                && !MAY_REPEAT.contains(&w.text.to_lowercase().as_str())
            {
                out.push(Issue {
                    range: w.range.end..n.range.end,
                    rule: "grazie.repeated-word",
                    message: format!("Repeated word \"{}\"", n.text),
                    replacement: Some(String::new()),
                    suggestions: Vec::new(),
                });
            }
            // a / an.
            if let Some(n) = next
                && spaced
                && !n.code
                && n.text.chars().count() > 1
                && !n.text.chars().all(|c| c.is_uppercase())
            {
                let lower = w.text.to_lowercase();
                let wrong = match (lower.as_str(), takes_a(n.text)) {
                    ("a", Some(a)) => !a && english,
                    ("an", Some(a)) => a && english,
                    _ => false,
                };
                if wrong {
                    let fixed = match (lower.as_str(), w.text.starts_with('A')) {
                        ("a", true) => "An",
                        ("a", false) => "an",
                        (_, true) => "A",
                        (_, false) => "a",
                    };
                    out.push(Issue {
                        range: w.range.clone(),
                        rule: "grazie.article",
                        message: format!("Use \"{fixed}\" before \"{}\"", n.text),
                        replacement: Some(fixed.to_owned()),
                        suggestions: Vec::new(),
                    });
                }
            }
            // American and British spellings (SPEC-0159 v5 behaviour 8): lowercase, or with a
            // capital first letter.
            if let Some(dict) = dictionary
                && variants != Mode::Off
                && english
                && !w.code
                && w.text.chars().count() >= 4
                && w.text
                    .chars()
                    .skip(1)
                    .all(|c| c.is_ascii_lowercase() || c == '\'')
                && w.text
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
            {
                let lower = w.text.to_lowercase();
                if !known.contains(&lower)
                    && let Some(form) = crate::spelling_variants::form_of(&lower)
                    && (matches!(variants, Mode::One(_)) || dict.knows(&lower))
                {
                    spellings.push(Candidate {
                        range: w.range.clone(),
                        text: w.text.to_owned(),
                        form,
                    });
                    continue;
                }
            }
            // Typos: words of four letters or more outside code, lowercase or with a capital
            // first letter (SPEC-0155 v8 behaviour 7: `Quixlet`, a sentence's first word); a
            // word in capitals or with a capital inside (code) stays alone. The lowercase form is
            // looked up, so a dictionary word at the start of a sentence is no typo.
            let capital = w
                .text
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
                && w.text
                    .chars()
                    .skip(1)
                    .all(|c| c.is_ascii_lowercase() || c == '\'');
            let lower = w.text.to_ascii_lowercase();
            if let Some(dict) = dictionary
                && english
                && !w.code
                && w.text.chars().count() >= 4
                && (capital || w.text.chars().all(|c| c.is_ascii_lowercase() || c == '\''))
                && !known.contains(&lower)
                && !dict.knows(&lower)
            {
                let mut suggestions = dict.suggestions(&lower, MAX_SUGGESTIONS);
                if capital {
                    // The replacement keeps the capital, as the variant fixes do.
                    for s in &mut suggestions {
                        *s = capitalised(s);
                    }
                }
                let suggestion = suggestions.first().cloned();
                out.push(Issue {
                    range: w.range.clone(),
                    rule: "grazie.typo",
                    message: match &suggestion {
                        Some(s) => format!("Typo: \"{}\" (did you mean \"{s}\"?)", w.text),
                        None => format!("Typo: \"{}\"", w.text),
                    },
                    replacement: suggestion,
                    suggestions,
                });
            }
        }
    }
    let marked = crate::spelling_variants::findings(&spellings, variants);
    if !marked.is_empty() {
        out.extend(
            marked
                .into_iter()
                .map(|(range, rule, message, replacement)| Issue {
                    range,
                    rule,
                    message,
                    suggestions: vec![replacement.clone()],
                    replacement: Some(replacement),
                }),
        );
        out.sort_by_key(|i| i.range.start);
    }
    out
}

/// `word` with its first letter in capitals (`quixlet` → `Quixlet`).
pub fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().chain(chars).collect::<String>()
    })
}

/// The text that "Ignore" keeps for the finding of `rule` at `range` of `text` (`SPEC-0155` v3
/// behaviour 10): the article with the word after it (`an user`), both repeated words
/// (`the the`); `None` for other rules or a range that does not fit.
pub fn phrase<'t>(text: &'t str, rule: &str, range: Range<usize>) -> Option<&'t str> {
    let in_word = |c: char| c.is_alphabetic() || c == '\'';
    match rule {
        "grazie.article" => {
            let rest = text.get(range.end..)?;
            let gap = rest.len() - rest.trim_start_matches(' ').len();
            let word = &rest[gap..];
            let len = word.find(|c: char| !in_word(c)).unwrap_or(word.len());
            (len > 0 && !range.is_empty()).then(|| &text[range.start..range.end + gap + len])
        }
        "grazie.repeated-word" => {
            let before = text.get(..range.start)?;
            let start = before
                .char_indices()
                .rev()
                .find(|(_, c)| !in_word(*c))
                .map_or(0, |(i, c)| i + c.len_utf8());
            (start < range.start && range.end <= text.len()).then(|| &text[start..range.end])
        }
        _ => None,
    }
}

/// Byte ranges of Markdown prose: everything but fenced code blocks, inline code, link targets and
/// HTML tags.
pub fn markdown_prose(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        // Split the line around `code`, (targets) after ], and <tags>.
        let bytes = line.as_bytes();
        let mut from = 0;
        let mut i = 0;
        while i < bytes.len() {
            let skip_to = match bytes[i] {
                b'`' => line[i + 1..].find('`').map(|e| i + 1 + e + 1),
                b'(' if i > 0 && bytes[i - 1] == b']' => line[i..].find(')').map(|e| i + e + 1),
                b'<' => line[i..].find('>').map(|e| i + e + 1),
                _ => None,
            };
            match skip_to {
                Some(end) => {
                    if from < i {
                        out.push(start + from..start + i);
                    }
                    from = end;
                    i = end;
                }
                None => i += 1,
            }
        }
        if from < line.len() {
            out.push(start + from..start + line.len());
        }
    }
    out
}

/// Lowercase words of `text` outside `prose` (identifiers of code), split at `_` and case changes,
/// so that names used in the code are not typos in its comments.
pub fn code_words(text: &str, prose: &[Range<usize>]) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut add = |part: &str| {
        let mut word = String::new();
        let mut prev_lower = false;
        for c in part.chars() {
            if c.is_alphabetic() {
                if c.is_uppercase() && prev_lower && !word.is_empty() {
                    out.insert(std::mem::take(&mut word).to_lowercase());
                }
                prev_lower = c.is_lowercase();
                word.push(c);
            } else {
                if !word.is_empty() {
                    out.insert(std::mem::take(&mut word).to_lowercase());
                }
                prev_lower = false;
            }
        }
        if !word.is_empty() {
            out.insert(word.to_lowercase());
        }
    };
    let mut at = 0;
    for r in prose {
        if at < r.start {
            add(&text[at..r.start]);
        }
        at = at.max(r.end);
    }
    if at < text.len() {
        add(&text[at..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dictionary {
        Dictionary::new([
            "the", "file", "open", "stop", "run", "is", "this", "that", "a", "an", "apple", "hour",
            "user", "test", "receive", "check", "word", "happy", "list", "usage", "module",
        ])
    }

    #[test]
    fn applies_hunspell_affixes() {
        let aff = "SET UTF-8\nPFX I Y 1\nPFX I   0     in         .\nSFX S Y 2\nSFX S   y     ies        [^aeiou]y\nSFX S   0     s          [^y]\nPFX U Y 1\nPFX U   0     un         .\n";
        let d = Dictionary::from_hunspell_with("3\nput/IS\nit/U\ncity/S\n", aff);
        for w in ["put", "input", "puts", "inputs", "unit", "city", "cities"] {
            assert!(d.has(w), "{w}");
        }
        assert!(!d.has("citys"));
    }

    /// SPEC-0155/AK-11 (v5): one edit away first, the common word before the rare one, then two
    /// edits away; at most the limit.
    #[test]
    fn suggestions_ranked_by_distance_then_frequency() {
        let d = Dictionary::new(["menage", "message", "massage", "messages", "mesa"]);
        assert_eq!(
            d.suggestions("mesage", 5),
            ["message", "menage", "messages", "massage", "mesa"]
        );
        assert_eq!(d.suggestions("mesage", 2), ["message", "menage"]);
        assert_eq!(d.suggest("mesage").as_deref(), Some("message"));
        // With the built-in en_US.
        let (dic, aff) = crate::dictionaries::built_in("en_US").expect("built-in en_US");
        let en = Dictionary::from_hunspell_with(dic, aff);
        let s = en.suggestions("mesage", MAX_SUGGESTIONS);
        assert_eq!(s.first().map(String::as_str), Some("message"), "{s:?}");
        assert!(
            s.len() <= MAX_SUGGESTIONS && s.iter().any(|w| w == "menage"),
            "{s:?}"
        );
        let text = "The mesage arrived.";
        let found = check(
            text,
            std::slice::from_ref(&(0..text.len())),
            Some(&en),
            &HashSet::new(),
        );
        assert_eq!(found[0].replacement.as_deref(), Some("message"));
        assert_eq!(found[0].suggestions, s);
        assert!(found[0].message.contains("did you mean \"message\""));
    }

    /// SPEC-0076 v12 behaviour 18: a second look for the same typo gives the same list.
    #[test]
    fn suggestions_are_kept_per_word() {
        let d = dict();
        let first = d.suggestions("mesage", MAX_SUGGESTIONS);
        assert_eq!(d.suggestions("mesage", MAX_SUGGESTIONS), first);
        assert_eq!(d.memo.lock().unwrap().len(), 1);
        assert_eq!(d.suggestions("mesage", 1), first[..first.len().min(1)]);
    }

    /// SPEC-0155/AK-13 (v6): the distance of the index search is that of the steps of v5 — every
    /// string one step from the typo is 1 away, every further string a second step reaches is 2
    /// away —, with letters, an apostrophe and a letter outside `a`–`z`.
    #[test]
    fn edit_distance_counts_the_steps() {
        for typo in ["mesage", "can't", "teh", "café"] {
            let t: Vec<char> = typo.chars().collect();
            let mut steps = None;
            let mut apart = |w: &str| {
                let c: Vec<char> = w.chars().collect();
                steps_apart(&t, &c, 2, &mut steps)
            };
            let near = edits(typo);
            for w in near.iter().filter(|w| w.as_str() != typo) {
                assert_eq!(apart(w), 1, "{typo} → {w}");
            }
            let mut seen = HashSet::new();
            for n in &near {
                for w in edits(n) {
                    if w != typo && !near.contains(&w) && seen.insert(w.clone()) {
                        assert_eq!(apart(&w), 2, "{typo} → {w}");
                    }
                }
            }
        }
        let d = |a: &str, b: &str| {
            let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
            steps_apart(&a, &b, 2, &mut None)
        };
        // Three steps, or a character no step can add.
        assert_eq!(d("mesage", "massager"), 3);
        assert_eq!(d("cant", "can't"), 3);
        assert_eq!(d("cafe", "café"), 3);
        assert_eq!(d("ca", "abc"), 2);
        // The apostrophe moved by two swaps, and by three.
        assert_eq!(d("can't", "c'ant"), 2);
        assert_eq!(d("can't", "'cant"), 3);
    }

    /// SPEC-0155/AK-13 (v6): the index finds the same words in the same order as trying every
    /// string one and two edits away, on the built-in en_US.
    #[test]
    fn index_search_finds_what_every_edit_finds() {
        let (dic, aff) = crate::dictionaries::built_in("en_US").expect("built-in en_US");
        let en = Dictionary::from_hunspell_with(dic, aff);
        for typo in [
            "mesage",
            "recieve",
            "definately",
            "seperately",
            "teh",
            "wich",
            "adress",
            "begining",
            "doesnt",
            "occured",
            "untill",
            "thier",
            "cant",
            "xyzzyq",
            "acommodationsx",
        ] {
            assert_eq!(
                en.search(typo, MAX_SUGGESTIONS),
                en.search_every_edit(typo, MAX_SUGGESTIONS),
                "{typo}"
            );
        }
        // More than five, so the second distance is compared in full too.
        for typo in ["mesage", "thier"] {
            assert_eq!(
                en.search(typo, 40),
                en.search_every_edit(typo, 40),
                "{typo}"
            );
        }
    }

    /// SPEC-0155/AK-13 (v6): with the index built, each typo's replacements are found within
    /// 50 ms on the built-in en_US — in a test build without optimisation, so the window, which
    /// adds the list and its drawing, stays well inside 200 ms.
    #[test]
    fn suggestions_found_within_the_bound() {
        let (dic, aff) = crate::dictionaries::built_in("en_US").expect("built-in en_US");
        let en = Dictionary::from_hunspell_with(dic, aff);
        en.prepare_suggestions();
        let _ = frequency_class("");
        for typo in [
            "mesage",
            "recieve",
            "definately",
            "seperately",
            "accomodation",
        ] {
            // The fastest of five runs is compared, so a test machine under load (other tests,
            // other builds) does not fail the bound by a scheduling pause.
            let mut took = std::time::Duration::MAX;
            for _ in 0..5 {
                let started = std::time::Instant::now();
                let found = en.suggestions(typo, MAX_SUGGESTIONS);
                took = took.min(started.elapsed());
                assert!(!found.is_empty(), "{typo}");
            }
            assert!(
                took < std::time::Duration::from_millis(50),
                "{typo} took {took:?}"
            );
        }
        assert_eq!(
            en.suggestions("mesage", MAX_SUGGESTIONS),
            ["message", "menage", "manage", "messages", "usage"]
        );
    }

    /// SPEC-0155/AK-13 (v6): `prepare` leaves the current dictionary with its index built.
    #[test]
    fn prepare_builds_the_index() {
        prepare();
        if let Some(d) = Dictionary::current() {
            assert!(d.index.get().is_some());
        }
    }

    /// SPEC-0159/AK-7 (v3): the three classes of SCOWL, `message` common, `menage` in none; the
    /// licence file lies beside the list.
    #[test]
    fn frequency_classes_of_scowl() {
        assert_eq!(frequency_class("message"), Some(1));
        assert_eq!(frequency_class("Message"), Some(1));
        assert_eq!(frequency_class("menage"), None);
        let classes: Vec<&str> = FREQUENCY
            .lines()
            .filter(|l| l.starts_with("# class "))
            .collect();
        assert_eq!(classes, ["# class 1", "# class 2", "# class 3"]);
        assert!((1..=3).contains(&frequency_class("abandon").unwrap_or(0)));
        let licence = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("dictionaries/SCOWL-Copyright.txt"),
        )
        .unwrap();
        assert!(licence.contains("Kevin Atkinson"));
    }

    /// SPEC-0159/AK-6 (v2): the built-in en_US finds `recieve` and suggests `receive`, and knows
    /// the common words and forms of the sentence.
    #[test]
    fn built_in_word_list_finds_typos() {
        let (dic, aff) = crate::dictionaries::built_in("en_US").expect("built-in en_US");
        let d = Dictionary::from_hunspell_with(dic, aff);
        let text = "We recieve the files and receive the answers, then reopen the cities' parks.";
        let found = check(
            text,
            std::slice::from_ref(&(0..text.len())),
            Some(&d),
            &HashSet::new(),
        );
        let typos: Vec<(&str, Option<&str>)> = found
            .iter()
            .map(|i| (&text[i.range.clone()], i.replacement.as_deref()))
            .collect();
        assert_eq!(typos, [("recieve", Some("receive"))]);
        assert_eq!(found[0].rule, "grazie.typo");
        // British spellings are typos for en_US (SPEC-0159 AK-4).
        assert!(!d.knows("colour") && d.knows("color"));
    }

    /// The typo `word` in `d`: its suggestions, or `None` when `d` knows it.
    fn typo(d: &Dictionary, word: &str) -> Option<Vec<String>> {
        let text = format!("We {word} it.");
        let found = check(
            &text,
            std::slice::from_ref(&(0..text.len())),
            Some(d),
            &HashSet::new(),
        );
        found
            .into_iter()
            .find(|i| i.rule == "grazie.typo")
            .map(|i| i.suggestions)
    }

    /// SPEC-0159/AK-8 (v4): with the built-in en_GB the British spellings are words and the
    /// American ones typos with British suggestions; with nothing chosen en_US is used.
    #[test]
    fn british_word_list() {
        let (dic, aff) = crate::dictionaries::built_in("en_GB").expect("built-in en_GB");
        let gb = Dictionary::from_hunspell_with(dic, aff);
        for w in [
            "colour",
            "organise",
            "behaviour",
            "centre",
            "colours",
            "organised",
        ] {
            assert!(gb.knows(w), "en_GB knows {w}");
        }
        for (w, first) in [
            ("color", "colour"),
            ("behavior", "behaviour"),
            ("favor", "favour"),
            ("recieve", "receive"),
        ] {
            let found = typo(&gb, w).unwrap_or_else(|| panic!("{w} is a typo with en_GB"));
            assert_eq!(
                found.first().map(String::as_str),
                Some(first),
                "{w}: {found:?}"
            );
        }
        // Nothing chosen: en_US, where the American forms are words and the British ones typos.
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-grazie-gb-{}", std::process::id()));
        let lists = crate::dictionaries::word_lists(Some(&dir), "");
        assert_eq!(lists, [crate::dictionaries::WordList::BuiltIn("en_US")]);
        let (dic, aff) = crate::dictionaries::built_in("en_US").expect("built-in en_US");
        let us = Dictionary::from_hunspell_with(dic, aff);
        for w in ["color", "behavior", "favor"] {
            assert!(us.knows(w), "en_US knows {w}");
        }
        assert_eq!(
            typo(&us, "colour")
                .and_then(|s| s.into_iter().next())
                .as_deref(),
            Some("color")
        );
        assert!(
            typo(&us, "organise").is_some(),
            "organise is a typo with en_US"
        );
        assert_eq!(
            typo(&us, "recieve")
                .and_then(|s| s.into_iter().next())
                .as_deref(),
            Some("receive")
        );
    }

    /// The spelling findings of `text` with the dictionaries `codes`: (rule, word, replacement).
    fn spelling(codes: &str, text: &str) -> Vec<(&'static str, String, Option<String>)> {
        set_typos(true);
        let dict = Dictionary::for_codes(codes).expect("the lists are read");
        check_with(
            text,
            std::slice::from_ref(&(0..text.len())),
            Some(&dict),
            &HashSet::new(),
            variant_mode_for(codes),
        )
        .into_iter()
        .filter(|i| crate::spelling_variants::kind_of(i.rule).is_some())
        .map(|i| (i.rule, text[i.range].to_owned(), i.replacement))
        .collect()
    }

    /// SPEC-0155/AK-14 (v7): with en_GB the American spellings are the other variant with the
    /// British one, not typos, and the reverse with en_US; words of both lists are none; with
    /// both dictionaries no word is the other variant.
    #[test]
    fn other_variant_both_ways() {
        use crate::spelling_variants::OTHER_VARIANT;
        let text = "We pick the color and center, the gray tone, organize the behavior. \
                    The program will recieve it.";
        let gb = spelling("en_GB", text);
        let expected = |pairs: &[(&str, &str)]| -> Vec<(&'static str, String, Option<String>)> {
            pairs
                .iter()
                .map(|(w, r)| {
                    if *r == "?" {
                        ("grazie.typo", (*w).to_owned(), Some("receive".to_owned()))
                    } else {
                        (OTHER_VARIANT, (*w).to_owned(), Some((*r).to_owned()))
                    }
                })
                .collect()
        };
        assert_eq!(
            gb,
            expected(&[
                ("color", "colour"),
                ("center", "centre"),
                ("gray", "grey"),
                ("organize", "organise"),
                ("behavior", "behaviour"),
                ("recieve", "?"),
            ])
        );
        let text = "We pick the colour and centre, the grey tone, organise the behaviour. \
                    The programme will recieve it.";
        let us = spelling("en_US", text);
        let mut want = expected(&[
            ("colour", "color"),
            ("centre", "center"),
            ("grey", "gray"),
            ("organise", "organize"),
            ("behaviour", "behavior"),
        ]);
        // `programme` has no American counterpart of its own (`program` is British too): it
        // stays a typo, as `recieve`.
        assert_eq!(us[..5], want[..], "{us:?}");
        assert!(
            us[5..]
                .iter()
                .all(|(rule, w, _)| *rule == "grazie.typo" && (w == "programme" || w == "recieve")),
            "{us:?}"
        );
        // A capital first letter is kept.
        assert_eq!(
            spelling("en_US", "Colour matters here."),
            [(OTHER_VARIANT, "Colour".to_owned(), Some("Color".to_owned()))]
        );
        // Several dictionaries: both spellings are the user's choice.
        want.clear();
        assert_eq!(
            spelling("en_GB,en_US", "We pick the colour and center."),
            want
        );
    }

    /// SPEC-0155/AK-14 (v7): mixed spelling on the less used spelling of one word; on a tie the
    /// one that occurs later; with one variant chosen never the chosen spelling.
    #[test]
    fn mixed_spelling_on_the_minority() {
        use crate::spelling_variants::{MIXED_SPELLING, OTHER_VARIANT};
        let mixed = |w: &str, r: &str| (MIXED_SPELLING, w.to_owned(), Some(r.to_owned()));
        let other = |w: &str, r: &str| (OTHER_VARIANT, w.to_owned(), Some(r.to_owned()));
        // Both dictionaries: the minority, then the later one on a tie.
        assert_eq!(
            spelling(
                "en_GB,en_US",
                "We organize files, organize tests and then organise the rest."
            ),
            [mixed("organise", "organize")]
        );
        assert_eq!(
            spelling(
                "en_GB,en_US",
                "The colour of the text and the color of the frame."
            ),
            [mixed("color", "colour")]
        );
        // en_US: the British spelling beside the American one is mixed spelling while the
        // American one is used at least as often; else the other variant. The American spelling
        // is never marked.
        assert_eq!(
            spelling("en_US", "We organize the files and organise the tests."),
            [mixed("organise", "organize")]
        );
        assert_eq!(
            spelling(
                "en_US",
                "We organise files, organise tests and then organize the rest."
            ),
            [other("organise", "organize"), other("organise", "organize")]
        );
        // Different words are counted apart.
        assert_eq!(
            spelling(
                "en_GB",
                "The colour of the centre and the color of the center."
            ),
            [mixed("color", "colour"), mixed("center", "centre")]
        );
    }

    /// SPEC-0159/AK-9 (v4): en_GB and en_US together know both spellings and suggest from both.
    #[test]
    fn two_dictionaries_know_both_spellings() {
        set_typos(true);
        let both = Dictionary::for_codes("en_GB,en_US").expect("both lists are read");
        for w in ["colour", "color", "behaviour", "behavior", "organise"] {
            assert!(both.knows(w), "both know {w}");
        }
        let found = typo(&both, "colur").expect("colur is a typo");
        assert!(
            found.iter().any(|s| s == "colour") && found.iter().any(|s| s == "color"),
            "{found:?}"
        );
        let gb = Dictionary::for_codes("en_GB").expect("en_GB is read");
        assert!(!gb.knows("color") && gb.knows("colour"));
        // Unknown codes are left out; nothing usable falls back to en_US.
        let us = Dictionary::for_codes("xx_XX").expect("the default list");
        assert!(us.knows("color") && !us.knows("colour"));
    }

    /// SPEC-0159/AK-11 (v4): the British list is about as large as the American one (within 5 %
    /// of the words), so reading it costs about the same.
    #[test]
    fn british_list_is_as_large_as_the_american() {
        let words = |code: &str| {
            let (dic, aff) = crate::dictionaries::built_in(code).expect("built in");
            let started = std::time::Instant::now();
            let d = Dictionary::from_hunspell_with(dic, aff);
            let took = started.elapsed();
            let started = std::time::Instant::now();
            let s = d.suggestions("recieve", MAX_SUGGESTIONS);
            eprintln!(
                "{code}: {} bytes, {} words, read in {took:?}, suggestions {s:?} in {:?}",
                dic.len() + aff.len(),
                d.words.len(),
                started.elapsed()
            );
            d.words.len()
        };
        let (us, gb) = (words("en_US"), words("en_GB"));
        assert!(gb.abs_diff(us) * 20 <= us, "en_GB {gb} words, en_US {us}");
    }

    /// SPEC-0155/AK-7 (v3): the text "Ignore" keeps for an article and a repeated word.
    #[test]
    fn phrase_of_a_finding() {
        let text = "Open an user the the file.";
        let found = rules(text);
        let phrases: Vec<Option<&str>> = check(
            text,
            std::slice::from_ref(&(0..text.len())),
            Some(&dict()),
            &HashSet::new(),
        )
        .into_iter()
        .map(|i| phrase(text, i.rule, i.range))
        .collect();
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(phrases, [Some("an user"), Some("the the")]);
        assert_eq!(
            phrase("A   apple", "grazie.article", 0..1),
            Some("A   apple")
        );
        assert_eq!(phrase("recieve", "grazie.typo", 0..7), None);
        assert_eq!(
            phrase("an", "grazie.article", 0..2),
            None,
            "no word after it"
        );
        assert_eq!(phrase("x", "grazie.article", 5..6), None);
    }

    #[test]
    fn reads_hunspell_word_lists() {
        let d = Dictionary::from_hunspell("3\ndatabase/SM\ndialog/MS\nbyte\n");
        assert!(d.knows("databases") && d.knows("dialog") && d.knows("bytes"));
        assert!(!d.knows("dialgo"));
    }

    fn rules(text: &str) -> Vec<(&'static str, String, Option<String>)> {
        check(
            text,
            std::slice::from_ref(&(0..text.len())),
            Some(&dict()),
            &HashSet::new(),
        )
        .into_iter()
        .map(|i| (i.rule, text[i.range].to_owned(), i.replacement))
        .collect()
    }

    #[test]
    fn finds_repeated_words_and_wrong_articles() {
        assert_eq!(
            rules("open the the file"),
            vec![("grazie.repeated-word", " the".into(), Some(String::new()))]
        );
        assert_eq!(rules("that that is"), vec![]);
        assert_eq!(
            rules("a apple, an user, an hour, a user, A apple"),
            vec![
                ("grazie.article", "a".into(), Some("an".into())),
                ("grazie.article", "an".into(), Some("a".into())),
                ("grazie.article", "A".into(), Some("An".into())),
            ]
        );
        assert_eq!(rules("a URL and a `x`, a usage, an npm module"), vec![]);
        assert_eq!(
            rules("Marker an geänderter Zeile und der Rest ist für die Datei"),
            vec![]
        );
    }

    /// SPEC-0155 v8 behaviour 7 (OBS-0117 G1): `is is` is a repeated word; only the pairs of
    /// [`MAY_REPEAT`] may stand twice.
    #[test]
    fn repeats_every_word_but_that_and_had() {
        assert_eq!(MAY_REPEAT, ["that", "had"]);
        for (text, twice) in [
            ("this is is a test", " is"),
            ("Is is it", " is"),
            ("do do it", " do"),
            ("no no", " no"),
        ] {
            assert_eq!(
                rules(text),
                vec![("grazie.repeated-word", twice.into(), Some(String::new()))],
                "{text}"
            );
        }
        assert_eq!(rules("this had had a test"), vec![]);
        assert_eq!(rules("check that that is a test"), vec![]);
    }

    /// SPEC-0155 v8 behaviour 7 (OBS-0117 G2): a word with a capital first letter is checked by
    /// its lowercase form; capitals, CamelCase and words of the code or the list stay quiet.
    #[test]
    fn checks_capitalised_words() {
        assert_eq!(
            rules("Quixlet checks the file"),
            vec![("grazie.typo", "Quixlet".into(), None)]
        );
        // The suggestion keeps the capital.
        assert_eq!(
            rules("Recieve the file"),
            vec![("grazie.typo", "Recieve".into(), Some("Receive".into()))]
        );
        // A list word at the start of a sentence, capitals, CamelCase: no typo.
        assert_eq!(rules("Check the file. Happy usage"), vec![]);
        assert_eq!(rules("the QUIXLET and QuixLet check"), vec![]);
        // A word of the code counts in any case.
        let text = "// Quixlet checks\nlet quixlet = 1;";
        let comment = 0..17;
        let prose = std::slice::from_ref(&comment);
        let known = code_words(text, prose);
        assert!(check(text, prose, Some(&dict()), &known).is_empty());
        assert_eq!(capitalised("quixlet"), "Quixlet");
        assert_eq!(capitalised(""), "");
    }

    #[test]
    fn finds_typos_but_not_inflections_code_or_known_words() {
        assert_eq!(
            rules("this tset checks"),
            vec![("grazie.typo", "tset".into(), Some("test".into()))]
        );
        assert_eq!(
            rules("files opened, stopped running happily; the lists"),
            vec![]
        );
        assert_eq!(rules("see file_name, fooBar() and v2 or JSON"), vec![]);
        let known: HashSet<String> = ["recieve".to_owned()].into();
        assert!(
            check(
                "recieve",
                std::slice::from_ref(&(0..7)),
                Some(&dict()),
                &known
            )
            .is_empty()
        );
        assert_eq!(
            check(
                "recieve",
                std::slice::from_ref(&(0..7)),
                Some(&dict()),
                &HashSet::new()
            )[0]
            .replacement,
            Some("receive".into())
        );
        assert!(
            check(
                "zzzz qqqq",
                std::slice::from_ref(&(0..9)),
                None,
                &HashSet::new()
            )
            .is_empty()
        );
    }

    #[test]
    fn markdown_prose_leaves_out_code_links_and_tags() {
        let md = "Run `the the` now\n```\nthe the\n```\n[link](the/the) <b>x</b>\n";
        let prose: Vec<&str> = markdown_prose(md).into_iter().map(|r| &md[r]).collect();
        assert_eq!(prose, ["Run ", " now\n", "[link]", " ", "x", "\n"]);
        let words = code_words(
            "fn openFile(user_name) // check\n",
            std::slice::from_ref(&(22..31)),
        );
        assert!(words.contains("open") && words.contains("file") && words.contains("user"));
        assert!(!words.contains("check"));
    }
}

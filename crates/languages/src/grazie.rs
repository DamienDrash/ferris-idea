//! Natural-language checks of comments, Markdown and plain text (`SPEC-0155` behaviours 7–9), as
//! the reference's bundled open Grazie plugin does: repeated words, `a`/`an` before a word and
//! misspelt words. The rules are IDECraft's own. Typos are looked for with an English Hunspell word
//! list — the built-in en_US or one downloaded in Settings (`SPEC-0159` v2) —, plus programming
//! words, inflections, the user's learned words (`~/Library/Spelling/LocalDictionary`) and the words
//! of the file's code. Text that reads as German is left to the reader.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

/// One problem in prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Byte range in the checked text.
    pub range: Range<usize>,
    /// `grazie.repeated-word`, `grazie.article` or `grazie.typo`.
    pub rule: &'static str,
    /// Message for the user.
    pub message: String,
    /// Text that fixes it (empty deletes the range), when there is one.
    pub replacement: Option<String>,
}

/// The dictionary code chosen in the settings (empty: the first installed).
static CHOSEN: RwLock<String> = RwLock::new(String::new());

/// Chooses the downloaded dictionary `code` for the typo check (`SPEC-0159`); empty for the first
/// installed one.
pub fn choose_dictionary(code: &str) {
    if let Ok(mut c) = CHOSEN.write() {
        code.clone_into(&mut c);
    }
}

/// Typos are looked for (tests switch them off to be independent of the machine's dictionaries).
static TYPOS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Switches the typo check on or off for this process.
pub fn set_typos(on: bool) {
    TYPOS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The word list the typo check reads (`SPEC-0159` v2 behaviour 3): the chosen dictionary when it
/// is downloaded or built in, else the first downloaded one, else the built-in en_US; `None` only
/// while the check is switched off.
pub fn word_list() -> Option<crate::dictionaries::WordList> {
    if !TYPOS.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let chosen = CHOSEN.read().map(|c| c.clone()).unwrap_or_default();
    let dir = crate::dictionaries::dictionaries_dir();
    Some(crate::dictionaries::word_list(dir.as_deref(), &chosen))
}

/// Words that may follow themselves (`that that`, `had had`).
const MAY_REPEAT: [&str; 6] = ["that", "had", "is", "do", "bye", "no"];

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
}

impl Dictionary {
    /// A dictionary of `words` (lowercase) and the programming words.
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(words: I) -> Self {
        let mut set: HashSet<String> = words
            .into_iter()
            .map(|w| w.as_ref().trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        set.extend(PROGRAMMING_WORDS.iter().map(|w| (*w).to_owned()));
        Self { words: set }
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

    /// The word list the typo check uses (`SPEC-0159` v2 behaviour 3): the dictionary chosen in
    /// Settings › Editor › Natural Languages or the first one downloaded there, else the built-in
    /// en_US; read once per list and kept. `None` while the check is off or when the list cannot
    /// be read (then no typos are reported).
    pub fn current() -> Option<Arc<Dictionary>> {
        use crate::dictionaries::WordList;
        static CACHE: Mutex<Option<(WordList, Arc<Dictionary>)>> = Mutex::new(None);
        let list = word_list()?;
        let mut cache = CACHE.lock().ok()?;
        if let Some((l, d)) = cache.as_ref()
            && *l == list
        {
            return Some(d.clone());
        }
        let mut dict = match &list {
            WordList::File(file) => {
                let text = std::fs::read_to_string(file).ok()?;
                let aff = std::fs::read_to_string(file.with_extension("aff")).unwrap_or_default();
                Dictionary::from_hunspell_with(&text, &aff)
            }
            WordList::BuiltIn => {
                let (dic, aff) = crate::dictionaries::built_in()?;
                Dictionary::from_hunspell_with(dic, aff)
            }
        };
        if let Some(learned) = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Spelling/LocalDictionary"))
            .and_then(|p| std::fs::read_to_string(p).ok())
        {
            dict.words
                .extend(learned.lines().map(|w| w.trim().to_lowercase()));
        }
        let dict = Arc::new(dict);
        *cache = Some((list, dict.clone()));
        Some(dict)
    }

    fn has(&self, w: &str) -> bool {
        self.words.contains(w)
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

    /// The closest listed word one edit away (swap, deletion, replacement, insertion), if any.
    pub fn suggest(&self, word: &str) -> Option<String> {
        let chars: Vec<char> = word.chars().collect();
        let letters = 'a'..='z';
        let mut found: Vec<String> = Vec::new();
        for i in 0..chars.len().saturating_sub(1) {
            let mut c = chars.clone();
            c.swap(i, i + 1);
            found.push(c.into_iter().collect());
        }
        for i in 0..chars.len() {
            let mut c = chars.clone();
            c.remove(i);
            found.push(c.into_iter().collect());
        }
        for i in 0..chars.len() {
            for l in letters.clone() {
                if l != chars[i] {
                    let mut c = chars.clone();
                    c[i] = l;
                    found.push(c.into_iter().collect());
                }
            }
        }
        for i in 0..=chars.len() {
            for l in letters.clone() {
                let mut c = chars.clone();
                c.insert(i, l);
                found.push(c.into_iter().collect());
            }
        }
        found.into_iter().find(|w| w.len() >= 2 && self.has(w))
    }
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
/// `known` are further words to accept (the identifiers of the file, lowercase).
pub fn check(
    text: &str,
    ranges: &[Range<usize>],
    dictionary: Option<&Dictionary>,
    known: &HashSet<String>,
) -> Vec<Issue> {
    let mut out = Vec::new();
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
                    });
                }
            }
            // Typos: lowercase words of four letters or more outside code.
            if let Some(dict) = dictionary
                && english
                && !w.code
                && w.text.chars().count() >= 4
                && w.text.chars().all(|c| c.is_ascii_lowercase() || c == '\'')
                && !known.contains(w.text)
                && !dict.knows(w.text)
            {
                let suggestion = dict.suggest(w.text);
                out.push(Issue {
                    range: w.range.clone(),
                    rule: "grazie.typo",
                    message: match &suggestion {
                        Some(s) => format!("Typo: \"{}\" (did you mean \"{s}\"?)", w.text),
                        None => format!("Typo: \"{}\"", w.text),
                    },
                    replacement: suggestion,
                });
            }
        }
    }
    out
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

    /// SPEC-0159/AK-6 (v2): the built-in en_US finds `recieve` and suggests `receive`, and knows
    /// the common words and forms of the sentence.
    #[test]
    fn built_in_word_list_finds_typos() {
        let (dic, aff) = crate::dictionaries::built_in().expect("built-in en_US");
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

    /// SPEC-0155/AK-7 (v3): the text "Ignore" keeps for an article and a repeated word.
    #[test]
    fn phrase_of_a_finding() {
        let text = "Send an user the the file.";
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

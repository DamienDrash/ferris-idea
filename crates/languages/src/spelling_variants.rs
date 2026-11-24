//! American and British spellings of the same word (`SPEC-0159` v5 behaviour 8, `SPEC-0155` v7
//! behaviour 7): which words only one of the two built-in SCOWL lists holds, which word of the
//! other list is their counterpart, and when a spelling is reported as the other variant or as
//! mixed spelling. Nothing here is a hand-made word list: both sides come from the built-in
//! `en_US` and `en_GB-ise` dictionaries with their affixes applied.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

/// Rule id of a spelling of the other English variant.
pub const OTHER_VARIANT: &str = "grazie.other-variant";
/// Rule id of the less used of two spellings of one word in a file.
pub const MIXED_SPELLING: &str = "grazie.mixed-spelling";
/// The rules of this module; their marks are drawn calmer than a typo's (`SPEC-0155` v7
/// behaviour 7).
pub const RULES: [&str; 2] = [OTHER_VARIANT, MIXED_SPELLING];

/// The kind of a spelling finding as tool output names it (`SPEC-0155` v7 behaviour 9):
/// `typo`, `other_variant` or `mixed_spelling`; `None` for other rules.
pub fn kind_of(rule: &str) -> Option<&'static str> {
    match rule {
        "grazie.typo" => Some("typo"),
        OTHER_VARIANT => Some("other_variant"),
        MIXED_SPELLING => Some("mixed_spelling"),
        _ => None,
    }
}

/// One of the two English variants Ferris IDEA carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// English (United States), `en_US`.
    American,
    /// English (United Kingdom), `en_GB`.
    British,
}

impl Variant {
    /// The variant of the dictionary code `code`.
    pub fn of_code(code: &str) -> Option<Variant> {
        match code {
            "en_US" => Some(Variant::American),
            "en_GB" => Some(Variant::British),
            _ => None,
        }
    }

    /// The adjective used in messages.
    pub fn name(self) -> &'static str {
        match self {
            Variant::American => "American",
            Variant::British => "British",
        }
    }
}

/// Which variant checks apply, from the dictionaries in use (`SPEC-0159` v5 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Only the built-in dictionary of this variant is in use: spellings of the other variant are
    /// reported, and mixed spelling.
    One(Variant),
    /// Several dictionaries, or one English other than these two: only mixed spelling.
    MixedOnly,
    /// No spelling check (typos switched off).
    Off,
}

impl Mode {
    /// The mode for the dictionary codes in use, in order.
    pub fn of_codes(in_use: &[&str]) -> Mode {
        match in_use {
            [] => Mode::Off,
            [one] => Variant::of_code(one).map_or(Mode::MixedOnly, Mode::One),
            _ => Mode::MixedOnly,
        }
    }
}

/// The words only one of the two lists holds.
struct Table {
    /// Words of `en_US` that `en_GB-ise` lacks, by length in characters.
    american: Vec<Vec<String>>,
    /// Words of `en_GB-ise` that `en_US` lacks, by length in characters.
    british: Vec<Vec<String>>,
    /// The counterparts already looked for.
    memo: Mutex<HashMap<String, Option<String>>>,
}

impl Table {
    fn build() -> Option<Table> {
        // Common words only: an entry with a capital first letter is a name (`Gray`, `Kokomo`),
        // which says nothing about how a word is spelt.
        let read = |code: &str| {
            let (dic, aff) = crate::dictionaries::built_in(code)?;
            let common: String = dic
                .lines()
                .enumerate()
                .filter(|(i, l)| *i == 0 || l.starts_with(|c: char| c.is_lowercase()))
                .map(|(_, l)| format!("{l}\n"))
                .collect();
            Some(crate::grazie::Dictionary::from_hunspell_with(&common, aff))
        };
        let (us, gb) = (read("en_US")?, read("en_GB")?);
        let only = |a: &HashSet<String>, b: &HashSet<String>| {
            let mut by_len: Vec<Vec<String>> = Vec::new();
            for w in a.difference(b) {
                let n = w.chars().count();
                if by_len.len() <= n {
                    by_len.resize_with(n + 1, Vec::new);
                }
                by_len[n].push(w.clone());
            }
            for bucket in &mut by_len {
                bucket.sort();
            }
            by_len
        };
        Some(Table {
            american: only(us.word_set(), gb.word_set()),
            british: only(gb.word_set(), us.word_set()),
            memo: Mutex::new(HashMap::new()),
        })
    }

    fn side(&self, v: Variant) -> &[Vec<String>] {
        match v {
            Variant::American => &self.american,
            Variant::British => &self.british,
        }
    }

    fn holds(&self, v: Variant, word: &str) -> bool {
        self.side(v)
            .get(word.chars().count())
            .is_some_and(|b| b.binary_search_by(|w| w.as_str().cmp(word)).is_ok())
    }

    /// The counterpart of `word` (a word of `v` only) among the words of the other variant only:
    /// the nearest within two edits that starts with the same letter; ties go to the more common
    /// word, then alphabetical order.
    fn counterpart(&self, v: Variant, word: &str) -> Option<String> {
        if let Some(known) = self.memo.lock().ok().and_then(|m| m.get(word).cloned()) {
            return known;
        }
        let other = match v {
            Variant::American => Variant::British,
            Variant::British => Variant::American,
        };
        let chars: Vec<char> = word.chars().collect();
        let mut best: Option<(u8, u8, &String)> = None;
        let mut cand = Vec::new();
        for len in chars.len().saturating_sub(2)..=chars.len() + 2 {
            let Some(bucket) = self.side(other).get(len) else {
                continue;
            };
            for w in bucket {
                if w.chars().next() != chars.first().copied() {
                    continue;
                }
                cand.clear();
                cand.extend(w.chars());
                let d = distance(&chars, &cand);
                if d > 2 {
                    continue;
                }
                let class = crate::grazie::frequency_class(w).unwrap_or(u8::MAX);
                if best.is_none_or(|(bd, bc, bw)| (d, class, w) < (bd, bc, bw)) {
                    best = Some((d, class, w));
                }
            }
        }
        let found = best.map(|(_, _, w)| w.clone());
        if let Ok(mut memo) = self.memo.lock() {
            memo.insert(word.to_owned(), found.clone());
        }
        found
    }
}

fn table() -> Option<&'static Table> {
    static TABLE: OnceLock<Option<Table>> = OnceLock::new();
    TABLE.get_or_init(Table::build).as_ref()
}

/// Reads both built-in lists and keeps the words only one of them holds; the window calls it
/// off the UI thread so the first check does not wait for it.
pub fn prepare() {
    let _ = table();
}

/// Letters that American and British spellings of one word exchange (`realize`/`realise`,
/// `defense`/`defence`, `skeptic`/`sceptic`, `gray`/`grey`, `pajamas`/`pyjamas`).
fn near(a: char, b: char) -> bool {
    const PAIRS: [(char, char); 6] = [
        ('z', 's'),
        ('s', 'c'),
        ('c', 'k'),
        ('a', 'e'),
        ('i', 'y'),
        ('a', 'y'),
    ];
    PAIRS
        .iter()
        .any(|&(x, y)| (a, b) == (x, y) || (a, b) == (y, x))
}

/// The edits between `a` and `b` — a letter left out or added, two neighbours swapped, a letter
/// replaced by one it is [`near`] (any other replacement counts two) —, capped at 3.
fn distance(a: &[char], b: &[char]) -> u8 {
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > 2 {
        return 3;
    }
    let width = m + 1;
    let mut d = vec![0usize; (n + 1) * width];
    for i in 0..=n {
        d[i * width] = i;
    }
    for (j, cell) in d.iter_mut().enumerate().take(m + 1) {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] {
                0
            } else if near(a[i - 1], b[j - 1]) {
                1
            } else {
                2
            };
            let mut v = (d[(i - 1) * width + j] + 1)
                .min(d[i * width + j - 1] + 1)
                .min(d[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * width + j - 2] + 1);
            }
            d[i * width + j] = v;
        }
    }
    d[n * width + m].min(3) as u8
}

/// A word of one variant only that has a counterpart in the other: its family is the pair
/// (American form, British form).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// The variant the word belongs to.
    pub variant: Variant,
    /// The same word in the other variant.
    pub counterpart: String,
    /// (American form, British form), both lowercase.
    pub family: (String, String),
}

/// The form of `word` (lowercase): `None` when both lists or neither hold it, or it has no
/// counterpart.
pub fn form_of(word: &str) -> Option<Form> {
    let t = table()?;
    let variant = if t.holds(Variant::American, word) {
        Variant::American
    } else if t.holds(Variant::British, word) {
        Variant::British
    } else {
        return None;
    };
    let counterpart = t.counterpart(variant, word)?;
    let family = match variant {
        Variant::American => (word.to_owned(), counterpart.clone()),
        Variant::British => (counterpart.clone(), word.to_owned()),
    };
    Some(Form {
        variant,
        counterpart,
        family,
    })
}

/// A word of a checked text that is a [`Form`].
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Byte range in the text.
    pub range: Range<usize>,
    /// The word as written.
    pub text: String,
    /// Its form.
    pub form: Form,
}

/// The spelling of `lower` written like `like` (a capital first letter kept).
fn cased(lower: &str, like: &str) -> String {
    if like.chars().next().is_some_and(char::is_uppercase) {
        let mut c = lower.chars();
        c.next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default()
    } else {
        lower.to_owned()
    }
}

/// The findings of the words `found` of one text under `mode` (`SPEC-0155` v7 behaviour 7), as
/// (range, rule, message, replacement):
/// - with one variant chosen, a word of the other variant is reported as `other-variant` with
///   the chosen spelling — or as `mixed-spelling` when the chosen spelling of the same word occurs
///   in the text at least as often;
/// - with several dictionaries (or another English), where both spellings of one word occur, each
///   place of the less used one is reported as `mixed-spelling` with the other; on a tie, the
///   spelling that first occurs later.
pub fn findings(
    found: &[Candidate],
    mode: Mode,
) -> Vec<(Range<usize>, &'static str, String, String)> {
    // Per family: places of the American and the British form, and where each first occurs.
    let mut count: HashMap<&(String, String), [(usize, usize); 2]> = HashMap::new();
    for c in found {
        let slot = &mut count.entry(&c.form.family).or_insert([(0, usize::MAX); 2])
            [usize::from(c.form.variant == Variant::British)];
        slot.0 += 1;
        slot.1 = slot.1.min(c.range.start);
    }
    let mut out = Vec::new();
    for c in found {
        let [us, gb] = count[&c.form.family];
        let (mine, theirs) = match c.form.variant {
            Variant::American => (us, gb),
            Variant::British => (gb, us),
        };
        let replacement = cased(&c.form.counterpart, &c.text);
        let mixed_msg = || {
            format!(
                "Mixed spelling: \"{}\" here, \"{}\" elsewhere in this text",
                c.text, replacement
            )
        };
        match mode {
            Mode::Off => {}
            Mode::One(chosen) if c.form.variant != chosen => {
                if theirs.0 > 0 && mine.0 <= theirs.0 {
                    out.push((c.range.clone(), MIXED_SPELLING, mixed_msg(), replacement));
                } else {
                    out.push((
                        c.range.clone(),
                        OTHER_VARIANT,
                        format!(
                            "{} spelling \"{}\" ({} English: \"{}\")",
                            c.form.variant.name(),
                            c.text,
                            chosen.name(),
                            replacement
                        ),
                        replacement,
                    ));
                }
            }
            Mode::One(_) => {}
            Mode::MixedOnly => {
                let minority = theirs.0 > 0
                    && (mine.0 < theirs.0 || (mine.0 == theirs.0 && mine.1 > theirs.1));
                if minority {
                    out.push((c.range.clone(), MIXED_SPELLING, mixed_msg(), replacement));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0159/AK-13 (v5): the American and British words and their counterparts come from the
    /// two built-in lists; words of both lists and words without a counterpart are no variants.
    #[test]
    fn american_and_british_words_from_the_lists() {
        for (us, gb) in [
            ("color", "colour"),
            ("center", "centre"),
            ("gray", "grey"),
            ("organize", "organise"),
            ("behavior", "behaviour"),
            ("analyze", "analyse"),
            ("judgment", "judgement"),
            ("colorizes", "colourises"),
            ("pediatrician", "paediatrician"),
        ] {
            let american = form_of(us).unwrap_or_else(|| panic!("{us} is American"));
            assert_eq!(american.variant, Variant::American, "{us}");
            assert_eq!(american.counterpart, gb, "{us}");
            let british = form_of(gb).unwrap_or_else(|| panic!("{gb} is British"));
            assert_eq!(british.variant, Variant::British, "{gb}");
            assert_eq!(british.counterpart, us, "{gb}");
            assert_eq!(american.family, british.family);
        }
        // In both lists, or in one without a counterpart.
        for w in [
            "program",
            "license",
            "practice",
            "meter",
            "dialog",
            "check",
            "tire",
            "programme",
            "plow",
            "kokomo",
            "recieve",
        ] {
            assert_eq!(form_of(w), None, "{w}");
        }
        assert_eq!(kind_of("grazie.typo"), Some("typo"));
        assert_eq!(kind_of(OTHER_VARIANT), Some("other_variant"));
        assert_eq!(kind_of(MIXED_SPELLING), Some("mixed_spelling"));
        assert_eq!(kind_of("grazie.article"), None);
    }

    /// SPEC-0159/AK-13 (v5): one built-in dictionary alone gives its variant; several, or another
    /// English, only mixed spelling.
    #[test]
    fn modes_of_the_dictionaries_in_use() {
        assert_eq!(Mode::of_codes(&["en_US"]), Mode::One(Variant::American));
        assert_eq!(Mode::of_codes(&["en_GB"]), Mode::One(Variant::British));
        assert_eq!(Mode::of_codes(&["en_GB", "en_US"]), Mode::MixedOnly);
        assert_eq!(Mode::of_codes(&["en_AU"]), Mode::MixedOnly);
        assert_eq!(Mode::of_codes(&[]), Mode::Off);
        // The distance: a swap, an added letter and a near letter count one each.
        let d = |a: &str, b: &str| {
            distance(
                &a.chars().collect::<Vec<_>>(),
                &b.chars().collect::<Vec<_>>(),
            )
        };
        assert_eq!(d("center", "centre"), 1);
        assert_eq!(d("color", "colour"), 1);
        assert_eq!(d("gray", "grey"), 1);
        assert_eq!(d("colorizes", "colonises"), 3);
        assert_eq!(d("plow", "plough"), 3);
    }
}

//! Fuzzy matching for "search everywhere" (`SPEC-0031` behaviours 1–2).

use std::cmp::Ordering;

/// A successful match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// Higher is better.
    pub score: i64,
    /// Char positions in the candidate that matched the query.
    pub positions: Vec<usize>,
}

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ':' | ' ')
}

/// Whether char `i` of `chars` starts a word.
fn word_start(chars: &[char], i: usize) -> bool {
    i == 0
        || is_separator(chars[i - 1])
        || (chars[i].is_uppercase() && chars[i - 1].is_lowercase())
        || (chars[i].is_alphanumeric() && !chars[i - 1].is_alphanumeric())
}

const START: i64 = 30;
const WORD_START: i64 = 20;
const CONSECUTIVE: i64 = 15;
const EXACT_CASE: i64 = 2;
const MATCH: i64 = 10;
const GAP: i64 = 2;

/// Matches `query` against `candidate`; `None` when not every query char appears in order.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
    let c: Vec<char> = candidate.chars().collect();
    if q.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }
    if q.len() > c.len() {
        return None;
    }
    let eq = |a: char, b: char| a.to_lowercase().eq(b.to_lowercase());
    // Quick rejection: plain subsequence test.
    let mut k = 0;
    for &ch in &c {
        if k < q.len() && eq(q[k], ch) {
            k += 1;
        }
    }
    if k < q.len() {
        return None;
    }
    // best[i][j]: best score for q[..=i] with q[i] matched at c[j]; from[i][j]: previous position.
    let n = c.len();
    let mut best = vec![vec![i64::MIN; n]; q.len()];
    let mut from = vec![vec![usize::MAX; n]; q.len()];
    for i in 0..q.len() {
        for j in i..n {
            if !eq(q[i], c[j]) {
                continue;
            }
            let mut here = MATCH;
            if j == 0 {
                here += START;
            }
            if word_start(&c, j) {
                here += WORD_START;
            }
            if q[i] == c[j] {
                here += EXACT_CASE;
            }
            if i == 0 {
                best[i][j] = here - GAP * j as i64;
                continue;
            }
            for p in (i - 1)..j {
                if best[i - 1][p] == i64::MIN {
                    continue;
                }
                let gap = j - p - 1;
                let link = if gap == 0 {
                    CONSECUTIVE
                } else {
                    -GAP * gap.min(10) as i64
                };
                let total = best[i - 1][p] + here + link;
                if total > best[i][j] {
                    best[i][j] = total;
                    from[i][j] = p;
                }
            }
        }
    }
    let last = q.len() - 1;
    let (mut j, score) = (last..n)
        .filter(|&j| best[last][j] != i64::MIN)
        .map(|j| (j, best[last][j]))
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))?;
    let mut positions = vec![0; q.len()];
    for i in (0..q.len()).rev() {
        positions[i] = j;
        j = from[i][j];
    }
    Some(FuzzyMatch {
        score: score - (n as i64 - q.len() as i64) / 4,
        positions,
    })
}

/// Like [`fuzzy_match`], but a query of several words also matches when its words appear in another
/// order, each on its own: "attach profiler to process" finds "Profiler Attach To Process" (D50).
/// A match in the typed order scores higher than one only found word by word.
pub fn words_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    if let Some(m) = fuzzy_match(query, candidate) {
        return Some(m);
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let mut score = 0;
    let mut positions = Vec::new();
    for w in &words {
        let m = fuzzy_match(w, candidate)?;
        score += m.score;
        positions.extend(m.positions);
    }
    positions.sort_unstable();
    positions.dedup();
    Some(FuzzyMatch {
        score: score - 10 * words.len() as i64,
        positions,
    })
}

/// Matches `query` against a name the way Search Everywhere does (`SPEC-0031` v4 behaviour 1):
/// the query is cut into fragments, each made of consecutive characters of the candidate, and
/// every fragment starts at a word start (the first character, after `/ _ - . :` or a space, an
/// upper-case letter after a lower-case one, a letter after a digit). `order` finds `OrderService`,
/// `createOrder` and `describe_order` but not `ProductService`; `OrSe` finds `OrderService`;
/// `calculatetotal` finds `calculate_total` and `calculateOrderTotal`. A `*` lets the next fragment
/// start anywhere; a space or a separator in the query (`.`, `/`, `_`, `-`, `:`) must be matched by
/// the same separator later in the candidate, so `model.Ord` finds `lab.shop.model.Order`. Case
/// is ignored, an exact case scores higher.
pub fn hump_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    // Query characters with whether a `*` or a space came before them.
    let mut q: Vec<(char, bool, bool)> = Vec::new();
    let (mut star, mut space) = (false, false);
    for ch in query.trim().chars() {
        match ch {
            '*' => star = true,
            ' ' => space = true,
            _ => {
                q.push((ch, star, space));
                star = false;
                space = false;
            }
        }
    }
    let c: Vec<char> = candidate.chars().collect();
    if q.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }
    if q.len() > c.len() {
        return None;
    }
    let eq = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    // Quick rejection: plain subsequence test.
    let mut k = 0;
    for &ch in &c {
        if k < q.len() && eq(q[k].0, ch) {
            k += 1;
        }
    }
    if k < q.len() {
        return None;
    }
    let n = c.len();
    let starts: Vec<bool> = (0..n)
        .map(|i| word_start(&c, i) || (i > 0 && c[i].is_alphabetic() && c[i - 1].is_ascii_digit()))
        .collect();
    // best[i][j]: best score for q[..=i] with q[i] matched at c[j].
    let mut best = vec![vec![i64::MIN; n]; q.len()];
    let mut from = vec![vec![usize::MAX; n]; q.len()];
    for i in 0..q.len() {
        let (qc, anywhere, new_word) = q[i];
        let sep = is_separator(qc);
        for j in i..n {
            if !eq(qc, c[j]) {
                continue;
            }
            let mut here = MATCH;
            if qc == c[j] {
                here += EXACT_CASE;
            }
            // May a fragment start at `j`?
            let may_start = sep || anywhere || starts[j];
            if i == 0 {
                if !may_start {
                    continue;
                }
                here += if j == 0 {
                    START + WORD_START
                } else {
                    WORD_START
                };
                best[i][j] = here - GAP * j.min(10) as i64;
                continue;
            }
            for p in (i - 1)..j {
                if best[i - 1][p] == i64::MIN {
                    continue;
                }
                let consecutive = p + 1 == j;
                // A space in the query asks for a new word; a separator needs its own match.
                // A run that goes on into the next hump (`applyDisc` in `applyDiscount`) beats a
                // jump over a separator (`apply_discount`).
                let link = if consecutive && !new_word {
                    CONSECUTIVE + if starts[j] { WORD_START / 2 } else { 0 }
                } else if may_start {
                    WORD_START - GAP * (j - p - 1).min(10) as i64
                } else {
                    continue;
                };
                let total = best[i - 1][p] + here + link;
                if total > best[i][j] {
                    best[i][j] = total;
                    from[i][j] = p;
                }
            }
        }
    }
    let last = q.len() - 1;
    let (mut j, score) = (last..n)
        .filter(|&j| best[last][j] != i64::MIN)
        .map(|j| (j, best[last][j]))
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))?;
    let mut positions = vec![0; q.len()];
    for i in (0..q.len()).rev() {
        positions[i] = j;
        j = from[i][j];
    }
    // The whole candidate typed exactly ranks first.
    let exact = if q.len() == n { 100 } else { 0 };
    Some(FuzzyMatch {
        score: score + exact - (n as i64 - q.len() as i64) / 4,
        positions,
    })
}

/// [`hump_match`] for several words in any order, as [`words_match`] does for [`fuzzy_match`].
pub fn hump_words_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    if let Some(m) = hump_match(query, candidate) {
        return Some(m);
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let mut score = 0;
    let mut positions = Vec::new();
    for w in &words {
        let m = hump_match(w, candidate)?;
        score += m.score;
        positions.extend(m.positions);
    }
    positions.sort_unstable();
    positions.dedup();
    Some(FuzzyMatch {
        score: score - 10 * words.len() as i64,
        positions,
    })
}

/// Orders results: higher score, then shorter text, then alphabetical (`SPEC-0031` behaviour 2).
pub fn rank(a: (i64, &str), b: (i64, &str)) -> Ordering {
    b.0.cmp(&a.0)
        .then(a.1.chars().count().cmp(&b.1.chars().count()))
        .then(a.1.cmp(b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(q: &str, c: &str) -> i64 {
        fuzzy_match(q, c).map_or(i64::MIN, |m| m.score)
    }

    /// D50: Find Action does not depend on the order of the typed words.
    #[test]
    fn words_in_any_order() {
        let title = "Profiler Attach To Process";
        assert!(fuzzy_match("attach profiler to process", title).is_none());
        let any = words_match("attach profiler to process", title).unwrap();
        let typed = words_match("profiler attach", title).unwrap();
        assert!(typed.score > words_match("attach profiler", title).unwrap().score);
        assert!(any.positions.contains(&0) && any.positions.contains(&9));
        assert!(
            words_match("attach xyz", title).is_none(),
            "every word must match"
        );
        assert!(
            words_match("dg", "GotoDeclaration").is_none(),
            "one word keeps its order"
        );
    }

    #[test]
    fn subsequence_camel_humps_and_positions() {
        assert!(fuzzy_match("gtdcl", "GotoDeclaration").is_some());
        assert!(fuzzy_match("xyz", "GotoDeclaration").is_none());
        assert!(
            fuzzy_match("dg", "GotoDeclaration").is_none(),
            "order matters"
        );
        let m = fuzzy_match("GoDe", "GotoDeclaration").unwrap();
        assert_eq!(m.positions, [0, 1, 4, 5]);
        assert!(
            score("GoDe", "GotoDeclaration") > score("GoDe", "GotodaEcho"),
            "humps beat inner letters"
        );
        assert!(
            score("main", "main.rs") > score("main", "src/domain.rs"),
            "start beats inner"
        );
        assert!(
            score("dom", "xdomx") > score("dom", "xdxoxmx"),
            "consecutive"
        );
        assert!(score("Foo", "Foo") > score("Foo", "foo"), "exact case");
        assert!(
            score("se", "SearchEverywhere") > score("se", "Close"),
            "word starts"
        );
        assert_eq!(fuzzy_match("", "x").unwrap().score, 0);
        assert!(
            fuzzy_match("go de", "GotoDeclaration").is_some(),
            "spaces ignored"
        );
    }

    /// SPEC-0031 v4 behaviour 1 (OBS-0023 L2, L5): fragments start at word starts.
    #[test]
    fn humps_need_word_starts() {
        let hit = |q: &str, c: &str| hump_match(q, c).is_some();
        assert!(hit("order", "OrderService"));
        assert!(hit("order", "createOrder"));
        assert!(hit("order", "describe_order"));
        assert!(hit("order", "calculateOrderTotal"));
        assert!(
            !hit("order", "ProductService"),
            "inner letters do not count"
        );
        assert!(!hit("order", "ProductRepository.java"));
        assert!(!hit("order", "Editor Decrease Font Size Global"));
        assert!(hit("OrSe", "OrderService"));
        assert!(!hit("OrSe", "CustomerService"));
        assert!(!hit("OrSe", "warehouse.py"));
        assert!(hit("calculatetotal", "calculate_total"));
        assert!(hit("calculatetotal", "calculateTotal"));
        assert!(
            hit("calculatetotal", "calculateOrderTotal"),
            "words may be skipped"
        );
        assert!(hit("model.Ord", "src.main.java.lab.shop.model.Order"));
        assert!(!hit("model.Ord", "lab.shop.repo.OrderRepository"));
        assert!(hit("Order.java", "OrderRepository.java"));
        assert!(
            hit("*vice", "OrderService"),
            "a star lets a fragment start anywhere"
        );
        assert!(!hit("vice", "OrderService"));
        assert!(hit("bulk item", "bulk item 7"));
        assert!(hit("compute1199", "compute1199"));
        let score = |q: &str, c: &str| hump_match(q, c).map(|m| m.score).unwrap_or(i64::MIN);
        assert!(
            score("applyDisc", "applyDiscount") > score("applyDisc", "apply_discount"),
            "the typed humps rank before a match across a separator"
        );
        assert!(!hit("total", "TableModelListener"));
        let m = hump_match("OrSe", "OrderService").unwrap();
        assert_eq!(m.positions, [0, 1, 5, 6]);
        let score = |q: &str, c: &str| hump_match(q, c).map_or(i64::MIN, |m| m.score);
        assert!(score("order", "order.py") > score("order", "createOrder"));
        assert!(
            score("Order", "Order") > score("Order", "OrderStore"),
            "exact wins"
        );
        assert!(hump_words_match("total order", "calculateOrderTotal").is_some());
    }

    #[test]
    fn ranking_ties() {
        let mut v = vec![(5, "bbb"), (5, "aa"), (9, "zzzz"), (5, "ab")];
        v.sort_by(|a, b| rank(*a, *b));
        assert_eq!(v, [(9, "zzzz"), (5, "aa"), (5, "ab"), (5, "bbb")]);
    }
}

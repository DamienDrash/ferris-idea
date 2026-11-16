//! Text operations and their transformation (`ADR-0026` §5, `SPEC-0317` v2 detail 4): an edit is
//! a list of `{at, delete, insert}` ranges of its base text in Unicode scalar offsets; inside, it
//! is a sequence of retain, insert and delete steps, which can be transformed against a concurrent
//! edit (server-sequenced, as in the Jupiter system: on equal offsets the edit sequenced earlier
//! goes first) and composed with a following one.

use ropey::Rope;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::rpc::{ErrorCode, RpcError};

/// One range of an edit in the protocol: at `at` (in the base text), delete `delete` scalars and
/// insert `insert`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    /// Offset in Unicode scalars of the base text.
    pub at: usize,
    /// Scalars removed from `at`.
    #[serde(default)]
    pub delete: usize,
    /// Text inserted at `at`.
    #[serde(default)]
    pub insert: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Retain(usize),
    Insert(String),
    Delete(usize),
}

impl Step {
    fn len(&self) -> usize {
        match self {
            Self::Retain(n) | Self::Delete(n) => *n,
            Self::Insert(s) => s.chars().count(),
        }
    }

    /// Splits off the first `n` units, leaving the rest in `self` (`None` when nothing is left).
    fn take(self, n: usize) -> (Step, Option<Step>) {
        let len = self.len();
        if n >= len {
            return (self, None);
        }
        match self {
            Self::Retain(m) => (Self::Retain(n), Some(Self::Retain(m - n))),
            Self::Delete(m) => (Self::Delete(n), Some(Self::Delete(m - n))),
            Self::Insert(s) => {
                let cut = s.char_indices().nth(n).map_or(s.len(), |(i, _)| i);
                (
                    Self::Insert(s[..cut].to_owned()),
                    Some(Self::Insert(s[cut..].to_owned())),
                )
            }
        }
    }
}

/// An edit from a text of `base_len` scalars to one of `target_len`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextOp {
    steps: Vec<Step>,
    base_len: usize,
    target_len: usize,
}

fn invalid(m: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Invalid, m)
}

impl TextOp {
    /// The edit that changes nothing in a text of `len` scalars.
    pub fn identity(len: usize) -> Self {
        let mut op = Self::default();
        op.retain(len);
        op
    }

    /// Length of the text the edit applies to.
    pub fn base_len(&self) -> usize {
        self.base_len
    }

    /// Length of the text after the edit.
    pub fn target_len(&self) -> usize {
        self.target_len
    }

    /// Whether the edit changes nothing.
    pub fn is_noop(&self) -> bool {
        self.steps.iter().all(|s| matches!(s, Step::Retain(_)))
    }

    fn retain(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.base_len += n;
        self.target_len += n;
        if let Some(Step::Retain(m)) = self.steps.last_mut() {
            *m += n;
        } else {
            self.steps.push(Step::Retain(n));
        }
    }

    fn insert(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.target_len += s.chars().count();
        let n = self.steps.len();
        match self.steps.last_mut() {
            Some(Step::Insert(t)) => t.push_str(s),
            // Inserts go before an adjacent delete, so equal edits have one form.
            Some(Step::Delete(_)) => {
                if n >= 2
                    && let Step::Insert(t) = &mut self.steps[n - 2]
                {
                    t.push_str(s);
                } else {
                    self.steps.insert(n - 1, Step::Insert(s.to_owned()));
                }
            }
            _ => self.steps.push(Step::Insert(s.to_owned())),
        }
    }

    fn delete(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.base_len += n;
        if let Some(Step::Delete(m)) = self.steps.last_mut() {
            *m += n;
        } else {
            self.steps.push(Step::Delete(n));
        }
    }

    /// The edit of protocol `ops` on a text of `base_len` scalars; ranges must be sorted, must
    /// not overlap and must lie inside the text.
    pub fn from_ops(base_len: usize, ops: &[Op]) -> Result<Self, RpcError> {
        let mut out = Self::default();
        let mut pos = 0;
        for op in ops {
            if op.at < pos {
                return Err(invalid(
                    "the ranges of an edit must be sorted and must not overlap",
                ));
            }
            let end = op
                .at
                .checked_add(op.delete)
                .filter(|e| *e <= base_len)
                .ok_or_else(|| {
                    invalid(format!(
                        "range {}+{} is outside the text of {base_len} characters",
                        op.at, op.delete
                    ))
                })?;
            out.retain(op.at - pos);
            out.insert(&op.insert);
            out.delete(op.delete);
            pos = end;
        }
        if pos > base_len {
            return Err(invalid("an edit range is outside the text"));
        }
        out.retain(base_len - pos);
        Ok(out)
    }

    /// The protocol form: sorted, non-overlapping ranges of the base text.
    pub fn to_ops(&self) -> Vec<Op> {
        let mut out = Vec::new();
        let mut cur: Option<Op> = None;
        let mut pos = 0;
        for step in &self.steps {
            match step {
                Step::Retain(n) => {
                    out.extend(cur.take());
                    pos += n;
                }
                Step::Insert(s) => cur
                    .get_or_insert(Op {
                        at: pos,
                        delete: 0,
                        insert: String::new(),
                    })
                    .insert
                    .push_str(s),
                Step::Delete(n) => {
                    cur.get_or_insert(Op {
                        at: pos,
                        delete: 0,
                        insert: String::new(),
                    })
                    .delete += n;
                    pos += n;
                }
            }
        }
        out.extend(cur);
        out
    }

    /// Applies the edit to `text`.
    pub fn apply(&self, text: &mut Rope) -> Result<(), RpcError> {
        if text.len_chars() != self.base_len {
            return Err(invalid(format!(
                "the edit is for a text of {} characters, the text has {}",
                self.base_len,
                text.len_chars()
            )));
        }
        let mut at = 0;
        for step in &self.steps {
            match step {
                Step::Retain(n) => at += n,
                Step::Insert(s) => {
                    text.insert(at, s);
                    at += s.chars().count();
                }
                Step::Delete(n) => text.remove(at..at + n),
            }
        }
        Ok(())
    }

    /// Applies the edit to a string (tests and small texts).
    pub fn apply_str(&self, text: &str) -> Result<String, RpcError> {
        let mut rope = Rope::from_str(text);
        self.apply(&mut rope)?;
        Ok(rope.to_string())
    }

    /// `(a', b')` for two edits of the same text, so that `a` then `b'` equals `b` then `a'`.
    /// `a` was sequenced first: on equal offsets its insert comes first.
    pub fn transform(a: &TextOp, b: &TextOp) -> Result<(TextOp, TextOp), RpcError> {
        if a.base_len != b.base_len {
            return Err(invalid("concurrent edits of texts of different lengths"));
        }
        let (mut a2, mut b2) = (TextOp::default(), TextOp::default());
        let mut ia = a.steps.iter().cloned();
        let mut ib = b.steps.iter().cloned();
        let (mut x, mut y) = (ia.next(), ib.next());
        loop {
            match (x.take(), y.take()) {
                (None, None) => break,
                (Some(Step::Insert(s)), other) => {
                    b2.retain(s.chars().count());
                    a2.insert(&s);
                    x = ia.next();
                    y = other;
                }
                (other, Some(Step::Insert(s))) => {
                    a2.retain(s.chars().count());
                    b2.insert(&s);
                    x = other;
                    y = ib.next();
                }
                (Some(p), Some(q)) => {
                    let m = p.len().min(q.len());
                    let (p1, prest) = p.take(m);
                    let (q1, qrest) = q.take(m);
                    match (p1, q1) {
                        (Step::Retain(n), Step::Retain(_)) => {
                            a2.retain(n);
                            b2.retain(n);
                        }
                        (Step::Delete(_), Step::Delete(_)) => {}
                        (Step::Delete(n), Step::Retain(_)) => a2.delete(n),
                        (Step::Retain(_), Step::Delete(n)) => b2.delete(n),
                        _ => return Err(invalid("edits do not fit together")),
                    }
                    x = prest.or_else(|| ia.next());
                    y = qrest.or_else(|| ib.next());
                }
                _ => return Err(invalid("concurrent edits of texts of different lengths")),
            }
        }
        Ok((a2, b2))
    }

    /// The one edit that does `self` and then `next`.
    pub fn compose(&self, next: &TextOp) -> Result<TextOp, RpcError> {
        if self.target_len != next.base_len {
            return Err(invalid("the second edit does not follow the first"));
        }
        let mut out = TextOp::default();
        let mut ia = self.steps.iter().cloned();
        let mut ib = next.steps.iter().cloned();
        let (mut x, mut y) = (ia.next(), ib.next());
        loop {
            match (x.take(), y.take()) {
                (None, None) => break,
                (Some(Step::Delete(n)), other) => {
                    out.delete(n);
                    x = ia.next();
                    y = other;
                }
                (other, Some(Step::Insert(s))) => {
                    out.insert(&s);
                    x = other;
                    y = ib.next();
                }
                (Some(p), Some(q)) => {
                    let m = p.len().min(q.len());
                    let (p1, prest) = p.take(m);
                    let (q1, qrest) = q.take(m);
                    match (p1, q1) {
                        (Step::Retain(n), Step::Retain(_)) => out.retain(n),
                        (Step::Insert(_), Step::Delete(_)) => {}
                        (Step::Insert(s), Step::Retain(_)) => out.insert(&s),
                        (Step::Retain(_), Step::Delete(n)) => out.delete(n),
                        _ => return Err(invalid("edits do not compose")),
                    }
                    x = prest.or_else(|| ia.next());
                    y = qrest.or_else(|| ib.next());
                }
                _ => return Err(invalid("the second edit does not follow the first")),
            }
        }
        Ok(out)
    }

    /// The edit that undoes `self` on its base `text`.
    pub fn invert(&self, text: &Rope) -> TextOp {
        let mut out = TextOp::default();
        let mut at = 0;
        for step in &self.steps {
            match step {
                Step::Retain(n) => {
                    out.retain(*n);
                    at += n;
                }
                Step::Insert(s) => out.delete(s.chars().count()),
                Step::Delete(n) => {
                    out.insert(&text.slice(at..at + n).to_string());
                    at += n;
                }
            }
        }
        out
    }

    /// One edit that turns `old` into `new` (common start and end kept), for reloads from disk.
    pub fn diff(old: &str, new: &str) -> TextOp {
        let a: Vec<char> = old.chars().collect();
        let b: Vec<char> = new.chars().collect();
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        let mut out = TextOp::default();
        out.retain(prefix);
        out.insert(&b[prefix..b.len() - suffix].iter().collect::<String>());
        out.delete(a.len() - suffix - prefix);
        out.retain(suffix);
        out
    }

    #[cfg(test)]
    fn steps_for_test(&self) -> usize {
        self.steps.len()
    }
}

/// The text's check value: the first 64 bits of SHA-256 over its UTF-8, as 16 hex digits.
pub fn text_hash(text: &Rope) -> String {
    let mut h = Sha256::new();
    for chunk in text.chunks() {
        h.update(chunk.as_bytes());
    }
    crate::hello::hex(&h.finalize()[..8])
}

/// [`text_hash`] of a string.
pub fn str_hash(text: &str) -> String {
    text_hash(&Rope::from_str(text))
}

impl TextOp {
    /// A random edit of a text of `len` scalars (property tests).
    pub fn random(len: usize, rng: &mut crate::testing::Rng) -> Self {
        let mut out = TextOp::default();
        let mut left = len;
        while left > 0 {
            match rng.below(4) {
                0 => {
                    let n = 1 + rng.below(4);
                    out.insert(&rng.text(n));
                }
                1 => {
                    let n = 1 + rng.below(left.min(3));
                    out.delete(n);
                    left -= n;
                }
                _ => {
                    let n = 1 + rng.below(left);
                    out.retain(n);
                    left -= n;
                }
            }
        }
        if rng.below(3) == 0 {
            let n = 1 + rng.below(3);
            out.insert(&rng.text(n));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Rng;

    fn op(at: usize, delete: usize, insert: &str) -> Op {
        Op {
            at,
            delete,
            insert: insert.into(),
        }
    }

    #[test]
    fn ops_round_trip_and_bad_ranges_are_invalid() {
        let e = TextOp::from_ops(10, &[op(1, 2, "ab"), op(5, 0, "x"), op(9, 1, "")]).unwrap();
        assert_eq!(e.to_ops(), [op(1, 2, "ab"), op(5, 0, "x"), op(9, 1, "")]);
        assert_eq!(e.apply_str("0123456789").unwrap(), "0ab34x5678");
        assert_eq!(e.target_len(), 10);
        // Overlapping, unsorted and out-of-range edits are refused.
        for bad in [
            vec![op(3, 2, ""), op(4, 0, "x")],
            vec![op(5, 0, ""), op(2, 0, "")],
            vec![op(9, 2, "")],
            vec![op(11, 0, "x")],
        ] {
            assert_eq!(
                TextOp::from_ops(10, &bad).unwrap_err().code,
                ErrorCode::Invalid
            );
        }
        assert!(TextOp::identity(4).is_noop());
        assert!(e.apply(&mut Rope::from_str("short")).is_err());
        // Inserts before a delete share one form.
        let a = TextOp::from_ops(3, &[op(0, 1, "x")]).unwrap();
        assert_eq!(a.steps_for_test(), 3);
    }

    #[test]
    fn concurrent_edits_converge_with_the_earlier_first() {
        let base = "hello";
        let check = |a: TextOp, b: TextOp, expect: &str| {
            let (a2, b2) = TextOp::transform(&a, &b).unwrap();
            let left = b2.apply_str(&a.apply_str(base).unwrap()).unwrap();
            let right = a2.apply_str(&b.apply_str(base).unwrap()).unwrap();
            assert_eq!(left, right);
            assert_eq!(left, expect);
        };
        let ins = |at, s| TextOp::from_ops(5, &[op(at, 0, s)]).unwrap();
        let del = |at, n| TextOp::from_ops(5, &[op(at, n, "")]).unwrap();
        // Same offset: the earlier edit's text goes first.
        check(ins(2, "A"), ins(2, "B"), "heABllo");
        check(ins(2, "B"), ins(2, "A"), "heBAllo");
        // Overlapping deletes remove the union once.
        check(del(1, 3), del(2, 3), "h");
        // An insert inside a deleted range survives.
        check(del(1, 3), ins(2, "X"), "hXo");
        // Unicode scalars beyond the BMP count as one.
        let emoji = TextOp::from_ops(5, &[op(0, 0, "😀𝄞")]).unwrap();
        check(emoji, del(0, 1), "😀𝄞ello");
        let base = "a😀b";
        let x = TextOp::from_ops(3, &[op(1, 1, "")]).unwrap();
        assert_eq!(x.apply_str(base).unwrap(), "ab");
        assert_eq!(
            x.invert(&Rope::from_str(base)).apply_str("ab").unwrap(),
            base
        );
    }

    #[test]
    fn random_pairs_converge_compose_and_invert() {
        let mut rng = Rng::new(7);
        for _ in 0..2000 {
            let n = rng.below(12);
            let base = rng.text(n);
            let len = base.chars().count();
            let a = TextOp::random(len, &mut rng);
            let b = TextOp::random(len, &mut rng);
            assert_eq!((a.base_len(), b.base_len()), (len, len));
            let (a2, b2) = TextOp::transform(&a, &b).unwrap();
            let after_a = a.apply_str(&base).unwrap();
            let after_b = b.apply_str(&base).unwrap();
            assert_eq!(
                b2.apply_str(&after_a).unwrap(),
                a2.apply_str(&after_b).unwrap(),
                "{base:?} {a:?} {b:?}"
            );
            // Ops survive the protocol form.
            assert_eq!(TextOp::from_ops(len, &a.to_ops()).unwrap(), a);
            // Composition equals applying one after the other.
            let c = TextOp::random(a.target_len(), &mut rng);
            let ac = a.compose(&c).unwrap();
            assert_eq!(ac.apply_str(&base).unwrap(), c.apply_str(&after_a).unwrap());
            // Inversion undoes.
            let inv = a.invert(&Rope::from_str(&base));
            assert_eq!(inv.apply_str(&after_a).unwrap(), base);
        }
    }

    #[test]
    fn diffs_and_hashes() {
        let d = TextOp::diff("fn main() {}\n", "fn main() { run(); }\n");
        assert_eq!(d.to_ops(), [op(11, 0, " run(); ")]);
        assert_eq!(
            d.apply_str("fn main() {}\n").unwrap(),
            "fn main() { run(); }\n"
        );
        assert!(TextOp::diff("same", "same").is_noop());
        assert_eq!(TextOp::diff("aXa", "aa").apply_str("aXa").unwrap(), "aa");
        let h = str_hash("hello");
        assert_eq!(h.len(), 16);
        // SHA-256("hello") starts with 2cf24dba5fb0a30e.
        assert_eq!(h, "2cf24dba5fb0a30e");
        assert_ne!(h, str_hash("hello!"));
    }
}

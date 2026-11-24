//! Redaction of values the vault put in place or stored (`SPEC-0353` behaviours 17 and 18): each
//! value is registered with its common encodings for the life of the process, and [`redact`]
//! replaces every registered form by `«vault:<name>»`. A value a program transformed on purpose is
//! not found; that is why the more contained injection forms come first (ADR-0033 D3).

use std::sync::Mutex;

use data_encoding::{BASE64, BASE64_NOPAD, BASE64URL_NOPAD, HEXLOWER, HEXUPPER};
use zeroize::Zeroizing;

/// One registered form: the text to find and the entry's name.
struct Form {
    text: Zeroizing<String>,
    name: String,
}

static FORMS: Mutex<Vec<Form>> = Mutex::new(Vec::new());

/// The most forms kept; older ones give way (a process injects far fewer).
const MAX_FORMS: usize = 16_384;

/// The forms `value` is searched in: raw, JSON-escaped, percent-encoded, base64 (standard with and
/// without padding, URL-safe without padding) and hex (lower and upper case).
pub fn forms(value: &str) -> Vec<Zeroizing<String>> {
    let bytes = value.as_bytes();
    let json = serde_json::to_string(value).unwrap_or_default();
    let mut out: Vec<Zeroizing<String>> = vec![
        Zeroizing::new(value.to_owned()),
        Zeroizing::new(
            json.get(1..json.len().saturating_sub(1))
                .unwrap_or_default()
                .to_owned(),
        ),
        Zeroizing::new(percent(bytes)),
        Zeroizing::new(BASE64.encode(bytes)),
        Zeroizing::new(BASE64_NOPAD.encode(bytes)),
        Zeroizing::new(BASE64URL_NOPAD.encode(bytes)),
        Zeroizing::new(HEXLOWER.encode(bytes)),
        Zeroizing::new(HEXUPPER.encode(bytes)),
    ];
    out.retain(|f| !f.is_empty());
    out.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    out.dedup();
    out
}

fn percent(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Registers `value` of the entry `name` for redaction. Values shorter than 8 bytes are never
/// stored by the vault (behaviour 2) and are not registered.
pub fn register(name: &str, value: &str) {
    if value.len() < crate::VALUE_BYTES.0 {
        return;
    }
    let mut all = FORMS.lock().unwrap_or_else(|e| e.into_inner());
    for text in forms(value) {
        if all.iter().any(|f| f.text == text) {
            continue;
        }
        all.push(Form {
            text,
            name: name.to_owned(),
        });
    }
    if all.len() > MAX_FORMS {
        let extra = all.len() - MAX_FORMS;
        all.drain(..extra);
    }
    all.sort_by_key(|f| std::cmp::Reverse(f.text.len()));
}

/// Whether any value is registered.
pub fn active() -> bool {
    !FORMS.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// `text` with every registered form replaced by `«vault:<name>»`, and how many were replaced.
pub fn redact(text: &str) -> (String, usize) {
    let all = FORMS.lock().unwrap_or_else(|e| e.into_inner());
    if all.is_empty() {
        return (text.to_owned(), 0);
    }
    let mut out = text.to_owned();
    let mut n = 0;
    for f in all.iter() {
        if out.contains(f.text.as_str()) {
            n += out.matches(f.text.as_str()).count();
            out = out.replace(f.text.as_str(), &format!("«vault:{}»", f.name));
        }
    }
    (out, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0353 AK-4: the raw value and each encoding are replaced; other text is kept.
    #[test]
    fn registered_values_are_redacted_in_every_encoding() {
        let value = "dummy/redact \"q\"+x9";
        register("redact-test", value);
        assert!(active());
        for form in forms(value) {
            let text = format!("before {} after", form.as_str());
            let (out, n) = redact(&text);
            assert_eq!(out, "before «vault:redact-test» after", "{}", form.as_str());
            assert_eq!(n, 1);
        }
        assert_eq!(redact("nothing here"), ("nothing here".to_owned(), 0));
        // Too short to register: never redacted.
        register("short", "abc");
        assert_eq!(redact("abc").1, 0);
    }
}

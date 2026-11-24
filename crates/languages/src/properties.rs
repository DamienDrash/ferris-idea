//! `.properties` files (`SPEC-0155` behaviours 4–6), as the reference's open Properties plugin
//! reads them: comments with `#` or `!`, keys ended by `=`, `:` or white space, values continued
//! with a backslash at the line end, and `\` escapes (format of `java.util.Properties.load`).

use std::ops::Range;

use serde::Serialize;

/// One property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Property {
    /// Key with escapes resolved.
    pub key: String,
    /// Value with escapes resolved and continuation lines joined.
    pub value: String,
    /// 0-based line of the key.
    pub line: usize,
    /// Byte range of the key as written.
    pub key_range: Range<usize>,
    /// Byte range of the value as written (to the end of its last line, without the line break).
    pub value_range: Range<usize>,
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) => out.push(ch),
                    None => out.push_str(&hex),
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Whether the line ends in an odd number of backslashes (continues on the next line).
fn continues(line: &str) -> bool {
    line.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1
}

/// The properties of `text` in file order.
pub fn parse(text: &str) -> Vec<Property> {
    let mut out = Vec::new();
    let lines: Vec<(usize, &str)> = {
        let mut at = 0;
        text.split_inclusive('\n')
            .map(|l| {
                let start = at;
                at += l.len();
                (start, l.trim_end_matches(['\n', '\r']))
            })
            .collect()
    };
    let mut n = 0;
    while n < lines.len() {
        let (start, line) = lines[n];
        let first = n;
        n += 1;
        let indent = line.len() - line.trim_start().len();
        let body = &line[indent..];
        if body.is_empty() || body.starts_with('#') || body.starts_with('!') {
            continue;
        }
        // Key: up to an unescaped `=`, `:` or white space.
        let bytes = body.as_bytes();
        let mut k = 0;
        while k < bytes.len() {
            match bytes[k] {
                b'\\' => k += 2,
                b'=' | b':' | b' ' | b'\t' | b'\x0c' => break,
                _ => k += 1,
            }
        }
        let k = k.min(body.len());
        let key_raw = &body[..k];
        // Separator: white space, then at most one `=` or `:`, then white space.
        let mut v = k;
        while v < bytes.len() && matches!(bytes[v], b' ' | b'\t' | b'\x0c') {
            v += 1;
        }
        if v < bytes.len() && matches!(bytes[v], b'=' | b':') {
            v += 1;
            while v < bytes.len() && matches!(bytes[v], b' ' | b'\t' | b'\x0c') {
                v += 1;
            }
        }
        let mut raw = body[v..].to_owned();
        let mut end = start + line.len();
        while continues(&raw) && n < lines.len() {
            raw.pop();
            let (s, next) = lines[n];
            raw.push_str(next.trim_start());
            end = s + next.len();
            n += 1;
        }
        let key_start = start + indent;
        out.push(Property {
            key: unescape(key_raw),
            value: unescape(&raw),
            line: first,
            key_range: key_start..key_start + k,
            value_range: (start + indent + v).min(end)..end,
        });
    }
    out
}

/// A problem of a properties file: byte range, inspection id, message and the edit that fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyIssue {
    /// Byte range.
    pub range: Range<usize>,
    /// `properties.duplicate-key` or `properties.trailing-spaces`.
    pub id: &'static str,
    /// Message.
    pub message: String,
    /// Title, range and replacement of the fix, if safe.
    pub fix: Option<(&'static str, Range<usize>, String)>,
    /// For `properties.missing-in-bundle`: the key and the locales that lack it, as
    /// [`locale_label`] names them (`SPEC-0163` v3 behaviour 3).
    pub missing: Option<(String, Vec<String>)>,
}

/// The bytes of property `p` in `text`: from the start of its first line to the end of its last
/// line with the line break, so removing them leaves no empty line (`SPEC-0155` v2 behaviour 5).
pub fn property_lines(text: &str, p: &Property) -> Range<usize> {
    let start = text[..p.key_range.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[p.value_range.end..]
        .find('\n')
        .map_or(text.len(), |i| p.value_range.end + i + 1);
    start..end
}

/// Duplicate keys and values ending in spaces that the reader keeps (`SPEC-0155` behaviour 5).
/// v2: every place of a key that occurs more than once, naming the other lines, with "Remove
/// property" as its fix.
pub fn check(text: &str) -> Vec<PropertyIssue> {
    let props = parse(text);
    let mut places: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for p in &props {
        places.entry(p.key.as_str()).or_default().push(p.line + 1);
    }
    let mut out = Vec::new();
    for p in &props {
        let others: Vec<String> = places[p.key.as_str()]
            .iter()
            .filter(|l| **l != p.line + 1)
            .map(usize::to_string)
            .collect();
        if !others.is_empty() {
            let lines = match others.as_slice() {
                [one] => format!("line {one}"),
                more => format!("lines {}", more.join(", ")),
            };
            out.push(PropertyIssue {
                range: p.key_range.clone(),
                id: "properties.duplicate-key",
                message: format!("Duplicate property key `{}` (also on {lines})", p.key),
                fix: Some(("Remove property", property_lines(text, p), String::new())),
                missing: None,
            });
        }
        let written = &text[p.value_range.clone()];
        let trimmed = written.trim_end_matches([' ', '\t']);
        // An escaped last space (`\ `) is meant.
        if trimmed.len() < written.len() && !trimmed.ends_with('\\') {
            let at = p.value_range.start + trimmed.len();
            out.push(PropertyIssue {
                range: at..p.value_range.end,
                id: "properties.trailing-spaces",
                message: format!("Trailing spaces in the value of `{}`", p.key),
                fix: Some((
                    "Remove trailing spaces",
                    at..p.value_range.end,
                    String::new(),
                )),
                missing: None,
            });
        }
    }
    out
}

/// The resource bundle a properties file belongs to (`SPEC-0163` behaviour 3): its base name and
/// locale, as `Messages_de_AT.properties` is `("Messages", "de_AT")` and `Messages.properties`
/// `("Messages", "")`. The locale is read from the end of the name: a two- or three-letter
/// lower-case language, then an optional country (two capitals or three digits) and variant.
pub fn bundle_name(path: &std::path::Path) -> Option<(String, String)> {
    let stem = path.file_stem()?.to_str()?;
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("properties"))
    {
        return None;
    }
    let parts: Vec<&str> = stem.split('_').collect();
    let lang = |p: &str| (2..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_lowercase());
    let country = |p: &str| {
        (p.len() == 2 && p.chars().all(|c| c.is_ascii_uppercase()))
            || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
    };
    // The locale is read from the end, as Java names bundle files: language, then an optional
    // country, then an optional variant (`base_de`, `base_de_AT`, `base_de_AT_POSIX`).
    let locale_len = (1..=3.min(parts.len().saturating_sub(1))).find(|&k| {
        let tail = &parts[parts.len() - k..];
        lang(tail[0]) && (tail.len() < 2 || country(tail[1]))
    });
    Some(match locale_len {
        Some(k) => (
            parts[..parts.len() - k].join("_"),
            parts[parts.len() - k..].join("_"),
        ),
        None => (stem.to_owned(), String::new()),
    })
}

/// The files of the bundle of `path` in its folder: locale and path, the base file first.
pub fn bundle_files(path: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let (Some((base, _)), Some(dir)) = (bundle_name(path), path.parent()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| match bundle_name(&p) {
            Some((b, locale)) if b == base => Some((locale, p)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// Keys of `text` (the bundle file `path`) missing in other files of its bundle, with those
/// locales (`SPEC-0163` behaviour 3). Nothing when the bundle has one file.
pub fn missing_in_bundle(path: &std::path::Path, text: &str) -> Vec<PropertyIssue> {
    let files = bundle_files(path);
    if files.len() < 2 {
        return Vec::new();
    }
    let others: Vec<(String, std::collections::HashSet<String>)> = files
        .iter()
        .filter(|(_, p)| p.file_name() != path.file_name())
        .map(|(locale, p)| {
            let keys = std::fs::read_to_string(p)
                .map(|t| parse(&t).into_iter().map(|p| p.key).collect())
                .unwrap_or_default();
            (locale.clone(), keys)
        })
        .collect();
    parse(text)
        .into_iter()
        .filter_map(|p| {
            let missing: Vec<String> = others
                .iter()
                .filter(|(_, keys)| !keys.contains(&p.key))
                .map(|(l, _)| locale_label(l).to_owned())
                .collect();
            (!missing.is_empty()).then(|| PropertyIssue {
                range: p.key_range.clone(),
                id: "properties.missing-in-bundle",
                message: format!(
                    "`{}` is missing in the bundle's {}",
                    p.key,
                    missing.join(", ")
                ),
                fix: None,
                missing: Some((p.key.clone(), missing)),
            })
        })
        .collect()
}

/// How a locale is named to the user: `default` for the base file, else its code (`SPEC-0163` v3
/// behaviour 3, IMP-0534: never an empty name).
pub fn locale_label(locale: &str) -> &str {
    if locale.is_empty() { "default" } else { locale }
}

/// Whether the values of the bundle file `path` are spell-checked with the English word lists
/// (`SPEC-0159` v6 behaviour 9, IMP-0535): only the files of a resource bundle with at least two
/// files hold translations; of those the base file and the English locales (`en`, `en_GB`) are
/// English. Another language's values would need that language's dictionary, which the catalog
/// does not have, so they are not checked. A properties file outside a bundle holds settings, not
/// prose.
pub fn values_in_english(path: &std::path::Path) -> bool {
    let Some((_, locale)) = bundle_name(path) else {
        return false;
    };
    let language = locale.split('_').next().unwrap_or_default();
    (language.is_empty() || language == "en") && bundle_files(path).len() >= 2
}

/// The new texts that add the property `key` with an empty value to every other file of the
/// bundle of `path` that lacks it (`SPEC-0163` v3 behaviour 3, the fix "Add missing translation to …"):
/// file, text before, text after; `text_of` gives a file's text (an open editor's, else the file
/// on disk). An empty value, so no placeholder text reaches a user and the bundle editor shows the
/// gap as empty. Refused with the reason when nothing can be added (IMP-0536): no other file in the
/// bundle, the key already in every file, or a file that cannot be read.
pub fn add_missing_key(
    path: &std::path::Path,
    key: &str,
    text_of: impl Fn(&std::path::Path) -> Option<String>,
) -> Result<Vec<(std::path::PathBuf, String, String)>, String> {
    let files = bundle_files(path);
    let others: Vec<std::path::PathBuf> = files
        .into_iter()
        .map(|(_, p)| p)
        .filter(|p| p.file_name() != path.file_name())
        .collect();
    if others.is_empty() {
        return Err(format!(
            "{} has no other file in its resource bundle",
            path.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        ));
    }
    let mut out = Vec::new();
    for file in others {
        let name = file
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let before = text_of(&file).ok_or_else(|| format!("{name} cannot be read"))?;
        if parse(&before).iter().any(|p| p.key == key) {
            continue;
        }
        let after = set_property(&before, key, "");
        out.push((file, before, after));
    }
    if out.is_empty() {
        return Err(format!("`{key}` is in every file of the bundle already"));
    }
    Ok(out)
}

/// One key of a resource bundle with its value in each locale (`SPEC-0212` behaviour 2–3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleKey {
    /// The key.
    pub key: String,
    /// Its value per locale of [`Bundle::locales`]; `None` where the key is missing.
    pub values: Vec<Option<String>>,
}

impl BundleKey {
    /// Indexes of the locales that lack the key.
    pub fn missing(&self) -> Vec<usize> {
        (0..self.values.len())
            .filter(|&i| self.values[i].is_none())
            .collect()
    }
}

/// A resource bundle as the bundle editor shows it (`SPEC-0212`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bundle {
    /// Base name.
    pub base: String,
    /// Locales, the default (`""`) first.
    pub locales: Vec<String>,
    /// The file of each locale.
    pub files: Vec<std::path::PathBuf>,
    /// Keys in the order of the first file, then those only later files have.
    pub keys: Vec<BundleKey>,
}

impl Bundle {
    /// The bundle of `path` from the files' texts: `text_of` gives a file's text (an open
    /// editor's, else the file on disk).
    pub fn read(
        path: &std::path::Path,
        text_of: impl Fn(&std::path::Path) -> Option<String>,
    ) -> Option<Self> {
        let (base, _) = bundle_name(path)?;
        let files = bundle_files(path);
        if files.is_empty() {
            return None;
        }
        let parsed: Vec<Vec<Property>> = files
            .iter()
            .map(|(_, p)| parse(&text_of(p).unwrap_or_default()))
            .collect();
        let mut keys: Vec<BundleKey> = Vec::new();
        let mut index = std::collections::HashMap::new();
        for (n, props) in parsed.iter().enumerate() {
            for p in props {
                let at = *index.entry(p.key.clone()).or_insert_with(|| {
                    keys.push(BundleKey {
                        key: p.key.clone(),
                        values: vec![None; files.len()],
                    });
                    keys.len() - 1
                });
                // The first of duplicate keys counts, as `missing_in_bundle` reads them.
                keys[at].values[n].get_or_insert_with(|| p.value.clone());
            }
        }
        Some(Self {
            base,
            locales: files.iter().map(|(l, _)| l.clone()).collect(),
            files: files.into_iter().map(|(_, p)| p).collect(),
            keys,
        })
    }

    /// Locale index of `locale` (`""` or `default` for the default file).
    pub fn locale_index(&self, locale: &str) -> Option<usize> {
        let locale = if locale == "default" { "" } else { locale };
        self.locales.iter().position(|l| l == locale)
    }
}

fn escape(text: &str, key: bool, ascii: bool) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{c}' => out.push_str("\\f"),
            ' ' if key || i == 0 => out.push_str("\\ "),
            '=' | ':' if key => {
                out.push('\\');
                out.push(c);
            }
            '#' | '!' if i == 0 => {
                out.push('\\');
                out.push(c);
            }
            c if ascii && !c.is_ascii() => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04X}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// `value` escaped for a value in `text` (`SPEC-0212` behaviour 4): what `Properties.load` reads
/// back as `value`; non-ASCII characters as `\uXXXX` only when `text` is ASCII so far.
pub fn escape_value(text: &str, value: &str) -> String {
    escape(value, false, text.is_ascii())
}

/// `text` with `key` set to `value` (`SPEC-0212` behaviour 4): an existing property's value
/// changes on its own lines only; a new one is appended as `key=value`.
pub fn set_property(text: &str, key: &str, value: &str) -> String {
    let escaped = escape_value(text, value);
    if let Some(p) = parse(text).into_iter().find(|p| p.key == key) {
        let (start, end) = (p.key_range.end, p.value_range.end);
        // Keep the key and its separator; only the value changes.
        let between = &text[start..p.value_range.start.max(start)];
        let separator = if between.is_empty() { "=" } else { between };
        return format!("{}{separator}{escaped}{}", &text[..start], &text[end..]);
    }
    let mut out = text.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&escape(key, true, text.is_ascii()));
    out.push('=');
    out.push_str(&escaped);
    out.push('\n');
    out
}

/// `text` without the property `key` (all of its lines, every occurrence).
pub fn remove_property(text: &str, key: &str) -> String {
    let mut out = text.to_owned();
    let mut found: Vec<Property> = parse(text).into_iter().filter(|p| p.key == key).collect();
    found.reverse();
    for p in found {
        out.replace_range(property_lines(text, &p), "");
    }
    out
}

/// The colours of a properties file (`SPEC-0155` v5 behaviour 12), from Ferris IDEA's own lexer of
/// the format [`parse`] reads: comment lines as `comment`, keys as `keyword`, the `=` or `:`
/// separator as `operator`, values as `string`, and escapes in keys and values (also the backslash
/// that continues a value) as `string.escape`. Spans are char offsets in file order.
pub fn highlight(text: &str) -> Vec<crate::syntax::Span> {
    let mut found: Vec<(Range<usize>, &'static str)> = Vec::new();
    let mut continued = false;
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let start = at;
        at += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let indent = line.len() - line.trim_start().len();
        let body_start = start + indent;
        let body = &line[indent..];
        if continued {
            continued = continues(body);
            escaped_runs(body, body_start, "string", &mut found);
            continue;
        }
        if body.is_empty() {
            continue;
        }
        if body.starts_with('#') || body.starts_with('!') {
            found.push((body_start..body_start + body.len(), "comment"));
            continue;
        }
        let bytes = body.as_bytes();
        let mut k = 0;
        while k < bytes.len() {
            match bytes[k] {
                b'\\' => k += 2,
                b'=' | b':' | b' ' | b'\t' | b'\x0c' => break,
                _ => k += 1,
            }
        }
        let k = k.min(body.len());
        escaped_runs(&body[..k], body_start, "keyword", &mut found);
        let mut v = k;
        while v < bytes.len() && matches!(bytes[v], b' ' | b'\t' | b'\x0c') {
            v += 1;
        }
        if v < bytes.len() && matches!(bytes[v], b'=' | b':') {
            found.push((body_start + v..body_start + v + 1, "operator"));
            v += 1;
            while v < bytes.len() && matches!(bytes[v], b' ' | b'\t' | b'\x0c') {
                v += 1;
            }
        }
        escaped_runs(&body[v..], body_start + v, "string", &mut found);
        continued = continues(body);
    }
    // Byte ranges to char offsets, in one pass (the ranges are ascending).
    let mut chars = 0;
    let mut byte = 0;
    let mut char_at = |b: usize| {
        chars += text[byte..b].chars().count();
        byte = b;
        chars
    };
    found
        .into_iter()
        .map(|(r, kind)| {
            let start = char_at(r.start);
            let end = char_at(r.end);
            crate::syntax::Span {
                start,
                end,
                kind: kind.to_owned(),
            }
        })
        .collect()
}

/// Runs of `part` (starting at byte `from` of the file) as `kind`, with its escapes split off as
/// `string.escape`: `\uXXXX`, a backslash and the character after it, a backslash at the end.
fn escaped_runs(
    part: &str,
    from: usize,
    kind: &'static str,
    out: &mut Vec<(Range<usize>, &'static str)>,
) {
    let bytes = part.as_bytes();
    let mut plain = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        let len = match bytes.get(i + 1) {
            None => 1,
            Some(b'u') => {
                2 + bytes[i + 2..]
                    .iter()
                    .take(4)
                    .take_while(|b| b.is_ascii_hexdigit())
                    .count()
            }
            // The escaped character may be any (non-ASCII too).
            Some(_) => 1 + part[i + 1..].chars().next().map_or(0, char::len_utf8),
        };
        if i > plain {
            out.push((from + plain..from + i, kind));
        }
        out.push((from + i..from + i + len, "string.escape"));
        i += len;
        plain = i;
    }
    if bytes.len() > plain {
        out.push((from + plain..from + bytes.len(), kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0155/AK-12 (v5): key, separator, value, comment and escapes; a continued value; a key
    /// ended by white space; char offsets after non-ASCII text.
    #[test]
    fn highlights_keys_separators_values_comments_escapes() {
        let text = "# Grüße\nkey = a\\tb\nmulti: one \\\n  two\nname value\nü\\u00FC=x\n";
        let spans = highlight(text);
        let spans: Vec<(String, &str)> = spans
            .iter()
            .map(|s| {
                let t: String = text.chars().skip(s.start).take(s.end - s.start).collect();
                (t, s.kind.as_str())
            })
            .collect();
        let want: Vec<(String, &str)> = [
            ("# Grüße", "comment"),
            ("key", "keyword"),
            ("=", "operator"),
            ("a", "string"),
            ("\\t", "string.escape"),
            ("b", "string"),
            ("multi", "keyword"),
            (":", "operator"),
            ("one ", "string"),
            ("\\", "string.escape"),
            ("two", "string"),
            ("name", "keyword"),
            ("value", "string"),
            ("ü", "keyword"),
            ("\\u00FC", "string.escape"),
            ("=", "operator"),
            ("x", "string"),
        ]
        .into_iter()
        .map(|(t, k)| (t.to_owned(), k))
        .collect();
        assert_eq!(spans, want);
    }

    #[test]
    fn parses_keys_values_continuations_and_escapes() {
        let text = "# comment\n! other\napp.name = IDE\\u0043raft\nkey\\ with\\:colon:value\n\
                    multi = one, \\\n        two\nempty\n  spaced   value  \n";
        let p = parse(text);
        let kv: Vec<(&str, &str, usize)> = p
            .iter()
            .map(|p| (p.key.as_str(), p.value.as_str(), p.line))
            .collect();
        assert_eq!(
            kv,
            [
                ("app.name", "IDECraft", 2), // the escaped text above spells it; rebrand:keep
                ("key with:colon", "value", 3),
                ("multi", "one, two", 4),
                ("empty", "", 6),
                ("spaced", "value  ", 7),
            ]
        );
        assert_eq!(&text[p[0].key_range.clone()], "app.name");
        assert_eq!(&text[p[2].value_range.clone()], "one, \\\n        two");
    }

    #[test]
    fn bundles_and_their_missing_keys() {
        use std::path::Path;
        assert_eq!(
            bundle_name(Path::new("a/Messages_de_AT.properties")),
            Some(("Messages".into(), "de_AT".into()))
        );
        assert_eq!(
            bundle_name(Path::new("my_app_messages_fr.properties")),
            Some(("my_app_messages".into(), "fr".into()))
        );
        assert_eq!(
            bundle_name(Path::new("Messages.properties")),
            Some(("Messages".into(), String::new()))
        );
        assert_eq!(bundle_name(Path::new("Messages.txt")), None);
        let dir = std::env::temp_dir().join(format!("ferris-idea-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("Messages.properties");
        std::fs::write(&base, "hello=Hello\nbye=Bye\n").unwrap();
        std::fs::write(dir.join("Messages_de.properties"), "hello=Hallo\n").unwrap();
        std::fs::write(
            dir.join("Messages_fr.properties"),
            "hello=Salut\nbye=Salut\n",
        )
        .unwrap();
        std::fs::write(dir.join("Other.properties"), "x=1\n").unwrap();
        assert_eq!(bundle_files(&base).len(), 3);
        let text = std::fs::read_to_string(&base).unwrap();
        let issues = missing_in_bundle(&base, &text);
        assert_eq!(issues.len(), 1);
        assert_eq!(&text[issues[0].range.clone()], "bye");
        assert!(issues[0].message.contains("de"), "{}", issues[0].message);
        let de = dir.join("Messages_de.properties");
        assert!(
            missing_in_bundle(&de, "hello=Hallo\nextra=1\n")[0]
                .message
                .contains("default, fr")
        );
        assert!(missing_in_bundle(&dir.join("Other.properties"), "x=1\n").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0163 v3 behaviour 3 (OBS-0117 G3, IMP-0534, IMP-0536): a key only one locale has
    /// names the base file `default`; the fix adds the key with an empty value to each file that
    /// lacks it, or says why it cannot.
    #[test]
    fn missing_key_names_default_and_is_added_or_refused() {
        use std::path::Path;
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-bundle-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("messages.properties");
        let de = dir.join("messages_de.properties");
        let fr = dir.join("messages_fr.properties");
        std::fs::write(&base, "hello=Hello\n").unwrap();
        std::fs::write(&de, "hello=Hallo").unwrap();
        std::fs::write(&fr, "hello=Salut\nextra.only.fr=Seulement\n").unwrap();
        let text = std::fs::read_to_string(&fr).unwrap();
        let issues = missing_in_bundle(&fr, &text);
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].message,
            "`extra.only.fr` is missing in the bundle's default, de"
        );
        assert_eq!(
            issues[0].missing,
            Some((
                "extra.only.fr".to_owned(),
                vec!["default".to_owned(), "de".to_owned()]
            ))
        );
        let read = |p: &Path| std::fs::read_to_string(p).ok();
        let changes = add_missing_key(&fr, "extra.only.fr", read).unwrap();
        let names: Vec<String> = changes
            .iter()
            .map(|(p, _, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["messages.properties", "messages_de.properties"]);
        assert_eq!(changes[0].2, "hello=Hello\nextra.only.fr=\n");
        // A file without a final line break gets one before the new key.
        assert_eq!(changes[1].2, "hello=Hallo\nextra.only.fr=\n");
        for (p, _, after) in &changes {
            std::fs::write(p, after).unwrap();
        }
        assert_eq!(
            add_missing_key(&fr, "extra.only.fr", read).unwrap_err(),
            "`extra.only.fr` is in every file of the bundle already"
        );
        assert_eq!(
            add_missing_key(&fr, "hello", |p: &Path| (p != de).then(String::new)).unwrap_err(),
            "messages_de.properties cannot be read"
        );
        let lone = dir.join("settings.properties");
        assert!(
            add_missing_key(&lone, "x", read)
                .unwrap_err()
                .contains("no other file")
        );
        // IMP-0535: only translations in English are spell-checked.
        assert!(values_in_english(&base));
        assert!(!values_in_english(&de) && !values_in_english(&fr));
        assert!(!values_in_english(&lone), "a lone file holds settings");
        std::fs::write(dir.join("messages_en_GB.properties"), "hello=Hello\n").unwrap();
        assert!(values_in_english(&dir.join("messages_en_GB.properties")));
        assert_eq!(locale_label(""), "default");
        assert_eq!(locale_label("de_AT"), "de_AT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0155/AK-6 (v2): every place of a duplicate key names the others; "Remove property"
    /// takes the property's lines, continuation lines included.
    #[test]
    fn reports_duplicates_and_trailing_spaces() {
        let text = "a=1\nb=2  \na=3\nc=x\\ \n";
        let issues = check(text);
        let found: Vec<(&str, &str)> = issues
            .iter()
            .map(|i| (i.id, &text[i.range.clone()]))
            .collect();
        assert_eq!(
            found,
            [
                ("properties.duplicate-key", "a"),
                ("properties.trailing-spaces", "  "),
                ("properties.duplicate-key", "a")
            ]
        );
        assert_eq!(
            issues[0].message,
            "Duplicate property key `a` (also on line 3)"
        );
        assert_eq!(
            issues[2].message,
            "Duplicate property key `a` (also on line 1)"
        );
        let remove = |i: &PropertyIssue| {
            let (title, range, with) = i.fix.clone().unwrap();
            assert_eq!(title, "Remove property");
            format!("{}{with}{}", &text[..range.start], &text[range.end..])
        };
        assert_eq!(remove(&issues[0]), "b=2  \na=3\nc=x\\ \n");
        assert_eq!(remove(&issues[2]), "a=1\nb=2  \nc=x\\ \n");
        assert_eq!(issues[1].fix.as_ref().unwrap().0, "Remove trailing spaces");
        // A continued value goes with its key; three places name the two others.
        let text = "greeting = Hello, \\\n    world\ngreeting=Hi\n# end\ngreeting=Hey";
        let issues = check(text);
        assert_eq!(issues.len(), 3);
        assert_eq!(
            issues[0].message,
            "Duplicate property key `greeting` (also on lines 3, 5)"
        );
        let (_, range, _) = issues[0].fix.clone().unwrap();
        assert_eq!(&text[range], "greeting = Hello, \\\n    world\n");
        let (_, range, _) = issues[2].fix.clone().unwrap();
        assert_eq!(
            &text[range], "greeting=Hey",
            "the last line without a line break"
        );
        assert!(check("a=1\nb=2\n").is_empty());
    }

    /// SPEC-0212 AK-1.
    #[test]
    fn bundle_editor_model() {
        use std::path::Path;
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-bundle-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("messages.properties");
        std::fs::write(&base, "hello=Hello\nbye=Bye\n").unwrap();
        std::fs::write(dir.join("messages_de.properties"), "hello=Hallo\nonly=x\n").unwrap();
        let read = |p: &Path| std::fs::read_to_string(p).ok();
        let b = Bundle::read(&dir.join("messages_de.properties"), read).unwrap();
        assert_eq!(b.base, "messages");
        assert_eq!(b.locales, ["", "de"]);
        let keys: Vec<(&str, Vec<usize>)> = b
            .keys
            .iter()
            .map(|k| (k.key.as_str(), k.missing()))
            .collect();
        assert_eq!(
            keys,
            [("hello", vec![]), ("bye", vec![1]), ("only", vec![0])]
        );
        assert_eq!(b.keys[0].values[1].as_deref(), Some("Hallo"));
        assert_eq!(b.locale_index("default"), Some(0));
        let _ = std::fs::remove_dir_all(&dir);

        // Set in place: key, separator, comments and other lines stay.
        let text = "# greeting\nhello : Hello\nmulti = one, \\\n    two\nlast=1";
        assert_eq!(
            set_property(text, "hello", "Hi there"),
            "# greeting\nhello : Hi there\nmulti = one, \\\n    two\nlast=1"
        );
        assert_eq!(
            set_property(text, "multi", "single"),
            "# greeting\nhello : Hello\nmulti = single\nlast=1"
        );
        assert_eq!(set_property("empty\n", "empty", "v"), "empty=v\n");
        // New keys are appended, with a line break before them if needed.
        assert_eq!(
            set_property(text, "new key", "x"),
            format!("{text}\nnew\\ key=x\n")
        );
        // Escapes: line breaks, a leading space, backslashes; non-ASCII only in ASCII files.
        assert_eq!(escape_value("a=1\n", " a\\b\nc"), "\\ a\\\\b\\nc");
        assert_eq!(escape_value("a=1\n", "Grüße"), "Gr\\u00FC\\u00DFe");
        assert_eq!(escape_value("a=Grüß\n", "Grüße"), "Grüße");
        let set = set_property("a=1\n", "a", "Grüße");
        assert_eq!(parse(&set)[0].value, "Grüße", "read back as written");
        // Removing takes all lines of every occurrence.
        assert_eq!(
            remove_property("a=1\nmulti=x\\\n  y\nb=2\nmulti=z", "multi"),
            "a=1\nb=2\n"
        );
    }
}

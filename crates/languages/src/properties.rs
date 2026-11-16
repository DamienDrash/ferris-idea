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
                .map(|(l, _)| {
                    if l.is_empty() {
                        "default".to_owned()
                    } else {
                        l.clone()
                    }
                })
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
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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
                ("app.name", "IDECraft", 2),
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
        let dir = std::env::temp_dir().join(format!("idecraft-bundle-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("idecraft-bundle-edit-{}", std::process::id()));
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

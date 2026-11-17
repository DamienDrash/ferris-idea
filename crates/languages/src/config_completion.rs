//! Completion of configuration keys (`SPEC-0115` v7 behaviour 19, `SPEC-0134` v6 behaviour 8):
//! in the Java code, the keys the configuration files define, inside a `${…}` placeholder and in
//! the `name` of `@ConfigProperty` and `@Property`; in a configuration file, the keys the code
//! reads and the other files define that this file does not define yet. Everything comes from the
//! bean model ([`SpringModel`]); no language server is asked.

use std::collections::BTreeSet;
use std::path::Path;

use crate::insight::{CompletionItem, Range};
use crate::spring::{SpringModel, canonical, is_config_file, is_external_key, without_profile};

/// Whether `file` is a configuration file of the bean model: `application*` or `bootstrap*`
/// ending in `.properties`, `.yml` or `.yaml`.
pub fn is_config_path(file: &Path) -> bool {
    is_config_file(file).is_some()
}

/// The characters a key is written with.
fn is_key_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '[' | ']' | '%')
}

/// Where a key is typed on a line: its first character and the end of the key text around the
/// caret (columns in characters), and the text typed before the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Typed {
    start: usize,
    end: usize,
    typed: String,
}

fn typed_at(chars: &[char], start: usize, column: usize) -> Typed {
    let mut end = column;
    while end < chars.len() && is_key_char(chars[end]) {
        end += 1;
    }
    Typed {
        start,
        end,
        typed: chars[start..column].iter().collect(),
    }
}

/// The key typed at `column` of a Java line: inside a `${…}` placeholder of a string, before
/// its `:` and `}`, or inside the string after `name =` of `@ConfigProperty` or `@Property`.
fn java_site(line: &str, column: usize) -> Option<Typed> {
    let chars: Vec<char> = line.chars().collect();
    let column = column.min(chars.len());
    let before: String = chars[..column].iter().collect();
    // The caret must be inside a string: an odd number of unescaped quotes before it.
    let mut quotes = Vec::new();
    let mut escaped = false;
    for (i, c) in chars[..column].iter().enumerate() {
        match c {
            '\\' if !escaped => {
                escaped = true;
                continue;
            }
            '"' if !escaped => quotes.push(i),
            _ => {}
        }
        escaped = false;
    }
    if quotes.len() % 2 == 0 {
        return None;
    }
    let open = *quotes.last()?;
    let inside: String = chars[open + 1..column].iter().collect();
    if let Some(at) = inside.rfind("${") {
        let after = &inside[at + 2..];
        if after.contains(['}', ':']) {
            return None;
        }
        let start = open + 1 + inside[..at + 2].chars().count();
        return Some(typed_at(&chars, start, column));
    }
    // `@ConfigProperty(name = "…")`, `@Property(name = "…")`: the string is the key.
    let head = before[..before
        .char_indices()
        .nth(open)
        .map_or(before.len(), |(b, _)| b)]
        .trim_end();
    let head = head.strip_suffix('=')?.trim_end().strip_suffix("name")?;
    let annotation = head.trim_end().strip_suffix('(')?.trim_end();
    let name = annotation.rsplit('@').next()?;
    if !(annotation.contains('@')
        && matches!(name.rsplit('.').next(), Some("ConfigProperty" | "Property")))
    {
        return None;
    }
    if !inside.chars().all(is_key_char) {
        return None;
    }
    Some(typed_at(&chars, open + 1, column))
}

/// The key typed at `column` of a `.properties` line: the key part, before any separator.
fn properties_site(line: &str, column: usize) -> Option<Typed> {
    let chars: Vec<char> = line.chars().collect();
    let column = column.min(chars.len());
    let start = chars.iter().take_while(|c| c.is_whitespace()).count();
    if start > column || matches!(chars.get(start), Some('#' | '!')) {
        return None;
    }
    if !chars[start..column].iter().all(|c| is_key_char(*c)) {
        return None;
    }
    Some(typed_at(&chars, start, column))
}

/// The key typed at `column` of line `n` of a YAML text and the keys of the maps it stands in
/// (`shop.limits` for `max` under `shop:` and `limits:`); none in a list, a block scalar or
/// after the `:` of the line.
fn yaml_site(text: &str, n: usize, column: usize) -> Option<(Typed, String)> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let line = lines.get(n)?;
    let chars: Vec<char> = line.chars().collect();
    let column = column.min(chars.len());
    let indent = chars.iter().take_while(|c| **c == ' ').count();
    if indent > column || matches!(chars.get(indent), Some('#' | '-')) {
        return None;
    }
    if !chars[indent..column].iter().all(|c| is_key_char(*c)) {
        return None;
    }
    let mut parents = Vec::new();
    let mut want = indent;
    for above in lines[..n].iter().rev() {
        if want == 0 {
            break;
        }
        let trimmed = above.trim_start_matches(' ');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("---") && above.len() == trimmed.len() {
            return None;
        }
        let at = above.len() - trimmed.len();
        if at >= want {
            continue;
        }
        // A parent is a map key without a value.
        let (key, rest) = trimmed.split_once(':')?;
        let rest = rest.trim();
        if !(rest.is_empty() || rest.starts_with('#')) || key.starts_with('-') {
            return None;
        }
        parents.push(key.trim().trim_matches(['"', '\'']).to_owned());
        want = at;
    }
    if want != 0 {
        return None;
    }
    parents.reverse();
    Some((typed_at(&chars, indent, column), parents.join(".")))
}

/// One candidate key with what the list shows beside it.
struct Candidate {
    key: String,
    detail: String,
}

/// Whether `key` fits what is typed: it starts with it, without regard to case.
fn fits(key: &str, typed: &str) -> bool {
    key.to_lowercase().starts_with(&typed.to_lowercase())
}

/// The keys the configuration files define, once each, with the value the application gets
/// (masked for a key that looks like a secret) or the profiles that define it.
fn defined_keys(model: &SpringModel, except_file: Option<&Path>) -> Vec<Candidate> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for p in &model.properties {
        if except_file.is_some_and(|f| p.file == f) {
            continue;
        }
        let key = without_profile(&p.key).to_owned();
        if !seen.insert(canonical(&key)) {
            continue;
        }
        let info = model.key_info(&key);
        let detail = match (&info.value, &info.value_profile) {
            // `key_info` masks the value of a key that looks like a secret.
            (Some(v), None) => format!("= {v}"),
            (Some(v), Some(profile)) => format!("= {v} [{profile}]"),
            (None, _) => {
                let profiles: BTreeSet<&str> = model
                    .properties
                    .iter()
                    .filter(|o| without_profile(&o.key) == key)
                    .filter_map(|o| o.profile.as_deref())
                    .collect();
                if profiles.is_empty() {
                    "defined".to_owned()
                } else {
                    format!(
                        "only in {}",
                        profiles.into_iter().collect::<Vec<_>>().join(", ")
                    )
                }
            }
        };
        let file = p
            .file
            .file_name()
            .map_or_else(String::new, |f| format!(" · {}", f.to_string_lossy()));
        out.push(Candidate {
            key,
            detail: format!("{detail}{file}"),
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The keys the code reads that `file` does not define, first those nothing defines.
fn read_keys(model: &SpringModel, file: &Path) -> Vec<Candidate> {
    let here: BTreeSet<String> = model
        .properties
        .iter()
        .filter(|p| p.file == file)
        .map(|p| canonical(without_profile(&p.key)))
        .collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for v in &model.values {
        if v.prefix || is_external_key(&v.key) || here.contains(&canonical(&v.key)) {
            continue;
        }
        if !seen.insert(canonical(&v.key)) {
            continue;
        }
        let defined = model
            .properties
            .iter()
            .any(|p| crate::spring::defines(&p.key, v));
        let at = format!(
            "{}:{}",
            v.file
                .file_name()
                .map_or_else(String::new, |f| f.to_string_lossy().into_owned()),
            v.line + 1
        );
        out.push((
            defined,
            Candidate {
                key: v.key.clone(),
                detail: if defined {
                    format!("read in {at}")
                } else {
                    format!("read in {at}, defined nowhere")
                },
            },
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.key.cmp(&b.1.key)));
    out.into_iter().map(|(_, c)| c).collect()
}

fn item(
    label: String,
    insert: String,
    detail: String,
    line: u64,
    t: &Typed,
    rank: usize,
) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some("property".into()),
        detail: Some(detail),
        insert,
        sort_text: Some(format!("{rank:05}")),
        edit_range: Some(Range {
            start_line: line,
            start_character: t.start as u64,
            end_line: line,
            end_character: t.end as u64,
        }),
        ..Default::default()
    }
}

/// Whether 0-based `line` and `column` (characters) of `file` is a place where a key is typed
/// ([`key_completions`] answers there): cheap, so the bean model is read only for such a place.
pub fn is_key_site(file: &Path, text: &str, line: u64, column: u64) -> bool {
    let line_text = crate::insight::line_of(text, line);
    let column = column as usize;
    match is_config_file(file) {
        None => {
            file.extension().is_some_and(|e| e == "java") && java_site(line_text, column).is_some()
        }
        Some(false) => properties_site(line_text, column).is_some(),
        Some(true) => yaml_site(text, line as usize, column).is_some(),
    }
}

/// The key completions at 0-based `line` and `column` (characters) of `file`
/// (workspace-relative) whose text is `text`, or `None` when the place is not a key: then the
/// language server answers as usual.
pub fn key_completions(
    model: &SpringModel,
    file: &Path,
    text: &str,
    line: u64,
    column: u64,
    limit: usize,
) -> Option<Vec<CompletionItem>> {
    let line_text = crate::insight::line_of(text, line);
    let column = column as usize;
    let mut items = Vec::new();
    match is_config_file(file) {
        None if file.extension().is_some_and(|e| e == "java") => {
            let t = java_site(line_text, column)?;
            for (rank, c) in defined_keys(model, None)
                .into_iter()
                .filter(|c| fits(&c.key, &t.typed))
                .enumerate()
            {
                items.push(item(c.key.clone(), c.key, c.detail, line, &t, rank));
            }
        }
        None => return None,
        Some(false) => {
            let t = properties_site(line_text, column)?;
            // `%dev.` typed before the key keeps its profile.
            let (profile, typed) = match t.typed.strip_prefix('%') {
                Some(rest) => match rest.split_once('.') {
                    Some((p, k)) => (format!("%{p}."), k.to_owned()),
                    None => return Some(Vec::new()),
                },
                None => (String::new(), t.typed.clone()),
            };
            let rest_of_line: String = line_text.chars().skip(t.end).collect();
            let separator = if rest_of_line.trim().is_empty() {
                "="
            } else {
                ""
            };
            let mut seen = BTreeSet::new();
            let candidates = read_keys(model, file)
                .into_iter()
                .chain(defined_keys(model, Some(file)));
            for c in candidates.filter(|c| fits(&c.key, &typed)) {
                if !seen.insert(canonical(&c.key)) {
                    continue;
                }
                let rank = items.len();
                items.push(item(
                    c.key.clone(),
                    format!("{profile}{}{separator}", c.key),
                    c.detail,
                    line,
                    &t,
                    rank,
                ));
            }
        }
        Some(true) => {
            let (t, parent) = yaml_site(text, line as usize, column)?;
            let here: BTreeSet<String> = model
                .properties
                .iter()
                .filter(|p| p.file == file)
                .map(|p| canonical(without_profile(&p.key)))
                .collect();
            let mut seen = BTreeSet::new();
            let candidates = read_keys(model, file)
                .into_iter()
                .chain(defined_keys(model, Some(file)));
            for c in candidates {
                if here.contains(&canonical(&c.key)) {
                    continue;
                }
                let rest = if parent.is_empty() {
                    c.key.as_str()
                } else {
                    match c
                        .key
                        .strip_prefix(&parent)
                        .and_then(|r| r.strip_prefix('.'))
                    {
                        Some(r) => r,
                        None => continue,
                    }
                };
                if rest.is_empty() || !fits(rest, &t.typed) || !seen.insert(canonical(rest)) {
                    continue;
                }
                let rank = items.len();
                let rest_of_line: String = line_text.chars().skip(t.end).collect();
                let colon = if rest_of_line.trim().is_empty() {
                    ": "
                } else {
                    ""
                };
                items.push(item(
                    rest.to_owned(),
                    format!("{rest}{colon}"),
                    c.detail,
                    line,
                    &t,
                    rank,
                ));
            }
        }
    }
    items.truncate(limit);
    Some(items)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::spring::model_of;

    fn sample() -> SpringModel {
        model_of(&[
            (
                PathBuf::from("src/main/java/a/S.java"),
                "package a;\nimport org.springframework.stereotype.Service;\n\
                 import org.springframework.beans.factory.annotation.Value;\n\
                 @Service\nclass S {\n  @Value(\"${shop.name}\") String n;\n  \
                 @Value(\"${shop.max-items}\") int m;\n  @Value(\"${shop.unset}\") int u;\n  \
                 @Value(\"${user.home}\") String h;\n  @Value(\"${db.password}\") String p;\n}\n"
                    .into(),
            ),
            (
                PathBuf::from("src/main/resources/application.properties"),
                "shop.name=Corner\nshop.maxItems=5\ndb.password=hunter2\n".into(),
            ),
            (
                PathBuf::from("src/main/resources/application-dev.yml"),
                "shop:\n  colour: red\n".into(),
            ),
        ])
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    /// SPEC-0115 v7 behaviour 19 (AK-21): in a `${…}` placeholder the defined keys with their
    /// values, a secret masked, the typed part narrowing them and replaced as a whole.
    #[test]
    fn keys_in_a_placeholder() {
        let m = sample();
        let java = Path::new("src/main/java/a/S.java");
        let text = "  @Value(\"${shop.}\") String x;\n";
        let items = key_completions(&m, java, text, 0, 17, 50).unwrap();
        assert_eq!(
            labels(&items),
            ["shop.colour", "shop.maxItems", "shop.name"]
        );
        assert_eq!(
            items[2].detail.as_deref(),
            Some("= Corner · application.properties")
        );
        assert_eq!(
            items[0].detail.as_deref(),
            Some("only in dev · application-dev.yml")
        );
        let r = items[0].edit_range.as_ref().unwrap();
        assert_eq!((r.start_character, r.end_character), (12, 17));
        let all = key_completions(&m, java, "@Value(\"${\")", 0, 10, 50).unwrap();
        let secret = all.iter().find(|i| i.label == "db.password").unwrap();
        assert_eq!(
            secret.detail.as_deref(),
            Some("= ••• · application.properties")
        );
        assert!(!format!("{all:?}").contains("hunter2"));
        // After the default's colon, outside a string, in a plain string: not a key.
        assert!(key_completions(&m, java, "@Value(\"${shop.name:x\")", 0, 22, 50).is_none());
        assert!(key_completions(&m, java, "int shop = 1;", 0, 8, 50).is_none());
        assert!(key_completions(&m, java, "log(\"shop.\")", 0, 10, 50).is_none());
    }

    /// SPEC-0134 v6 behaviour 8 (AK-14): the `name` of `@ConfigProperty` and `@Property`.
    #[test]
    fn keys_in_config_property_names() {
        let m = sample();
        let java = Path::new("src/main/java/a/R.java");
        let line = "    @ConfigProperty(name = \"shop.n\")";
        let items = key_completions(&m, java, line, 0, 34, 50).unwrap();
        assert_eq!(labels(&items), ["shop.name"]);
        let line = "    @Property(name = \"sh\") String o;";
        assert_eq!(key_completions(&m, java, line, 0, 24, 50).unwrap().len(), 3);
        let line = "    @Named(name = \"sh\")";
        assert!(key_completions(&m, java, line, 0, 21, 50).is_none());
    }

    /// SPEC-0115 v7 behaviour 19 (AK-21): in a `.properties` file the keys the code reads that
    /// it lacks (those defined nowhere first), then the keys of other files; `=` added at a line
    /// end, a `%dev.` profile kept; nothing in a value or a comment.
    #[test]
    fn keys_in_a_properties_file() {
        let m = sample();
        let props = Path::new("src/main/resources/application.properties");
        let text = "shop.name=Corner\nshop.maxItems=5\ndb.password=hunter2\nsh\n";
        let items = key_completions(&m, props, text, 3, 2, 50).unwrap();
        assert_eq!(labels(&items), ["shop.unset", "shop.colour"]);
        assert_eq!(items[0].insert, "shop.unset=");
        assert_eq!(
            items[0].detail.as_deref(),
            Some("read in S.java:8, defined nowhere")
        );
        let items = key_completions(&m, props, "%dev.shop.u", 0, 11, 50).unwrap();
        assert_eq!(items[0].insert, "%dev.shop.unset=");
        assert!(key_completions(&m, props, "shop.name=Cor", 0, 13, 50).is_none());
        assert!(key_completions(&m, props, "# sh", 0, 4, 50).is_none());
    }

    /// SPEC-0115 v7 behaviour 19 (AK-21): in a YAML file the rest of the key below the maps the
    /// line stands in.
    #[test]
    fn keys_in_a_yaml_file() {
        let m = sample();
        let yml = Path::new("src/main/resources/application-dev.yml");
        let text = "shop:\n  colour: red\n  \n";
        let items = key_completions(&m, yml, text, 2, 2, 50).unwrap();
        assert_eq!(labels(&items), ["unset", "max-items", "name"]);
        assert_eq!(items[0].insert, "unset: ");
        let top = key_completions(&m, yml, "shop:\n  colour: red\nd\n", 2, 1, 50).unwrap();
        assert_eq!(labels(&top), ["db.password"]);
        assert!(key_completions(&m, yml, "shop:\n  - a\n  \n", 2, 2, 50).is_some());
        assert!(key_completions(&m, yml, "list:\n  - a\n  - b", 2, 3, 50).is_none());
    }
}

//! Copyright notices (`SPEC-0155` behaviours 1–3), as the reference's open Copyright plugin keeps
//! them: a notice template with variables, written as a comment at the top of a file and replaced
//! where a comment there already holds the keyword.

use std::path::Path;

use crate::LanguageId;

/// The notice template and the word that marks an existing notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Template text; `$today.year`, `$file.fileName`, `$project.name` and `$username` are
    /// replaced.
    pub notice: String,
    /// A comment at the top holding this word (case-insensitive) is the notice to update.
    pub keyword: String,
}

/// The current year (UTC).
pub fn current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    year_of_day((secs / 86_400) as i64)
}

/// Year of a day counted from 1970-01-01 (proleptic Gregorian calendar).
fn year_of_day(days: i64) -> i32 {
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2)) as i32
}

/// What the template's variables stand for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values {
    /// The current year.
    pub year: i32,
    /// Name of the file.
    pub file_name: String,
    /// Name of the project (its folder).
    pub project: String,
    /// The user's login name.
    pub user: String,
}

impl Profile {
    /// The notice with its variables replaced.
    pub fn render(&self, v: &Values) -> String {
        self.notice
            .replace("$today.year", &v.year.to_string())
            .replace("${today.year}", &v.year.to_string())
            .replace("$file.fileName", &v.file_name)
            .replace("$project.name", &v.project)
            .replace("$username", &v.user)
            .trim_end()
            .to_owned()
    }
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let Some(end) = rest[i..].find(';') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let entity = &rest[i + 1..i + end];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&rest[i..=i + end]),
        }
        rest = &rest[i + end + 1..];
    }
    out.push_str(rest);
    out
}

/// `<option name="NAME" value="…"/>` of an XML text.
fn option(xml: &str, name: &str) -> Option<String> {
    let at = xml.find(&format!("name=\"{name}\""))?;
    let tag_start = xml[..at].rfind('<')?;
    let tag_end = at + xml[at..].find('>')?;
    let tag = &xml[tag_start..tag_end];
    let v = tag.find("value=\"")? + 7;
    let end = tag[v..].find('"')?;
    Some(xml_unescape(&tag[v..v + end]))
}

/// The project's copyright profile: `.idecraft/copyright.txt` (the template; an optional first line
/// `keyword: …` sets the keyword), else the default profile of the reference's
/// `.idea/copyright/` folder (`profiles_settings.xml` names it; with one profile, that one).
pub fn load_profile(root: &Path) -> Option<Profile> {
    if let Ok(text) = std::fs::read_to_string(root.join(".idecraft/copyright.txt")) {
        let (keyword, notice) = match text.split_once('\n') {
            Some((first, rest)) if first.starts_with("keyword:") => {
                (first["keyword:".len()..].trim().to_owned(), rest.to_owned())
            }
            _ => ("Copyright".to_owned(), text),
        };
        return Some(Profile { notice, keyword });
    }
    let dir = root.join(".idea/copyright");
    let default = std::fs::read_to_string(dir.join("profiles_settings.xml"))
        .ok()
        .and_then(|x| {
            let at = x.find("default=\"")? + 9;
            let end = x[at..].find('"')?;
            Some(xml_unescape(&x[at..at + end]))
        });
    let mut profiles: Vec<(String, String)> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.ends_with(".xml") && n != "profiles_settings.xml"
        })
        .filter_map(|e| {
            Some((
                e.path().display().to_string(),
                std::fs::read_to_string(e.path()).ok()?,
            ))
        })
        .collect();
    profiles.sort();
    let chosen = match default {
        Some(name) => profiles
            .iter()
            .find(|(_, x)| option(x, "myName").as_deref() == Some(name.as_str())),
        None if profiles.len() == 1 => profiles.first(),
        None => None,
    }?;
    Some(Profile {
        notice: option(&chosen.1, "notice")?,
        keyword: option(&chosen.1, "keyword").unwrap_or_else(|| "Copyright".into()),
    })
}

/// How a language writes the notice: block delimiters with a line prefix, or a line comment.
enum Style {
    Block(&'static str, &'static str, &'static str),
    Line(&'static str),
}

fn style(language: LanguageId) -> Style {
    use LanguageId as L;
    match language {
        L::Java
        | L::Kotlin
        | L::JavaScript
        | L::TypeScript
        | L::C
        | L::Cpp
        | L::Groovy
        | L::Scala
        | L::Swift
        | L::Css
        | L::Scss
        | L::Less
        | L::Php
        | L::Go => Style::Block("/*", " * ", " */"),
        L::Html | L::Xml | L::Markdown | L::Vue | L::Svelte => Style::Block("<!--", "  ", "-->"),
        L::Sql | L::Lua => Style::Line("--"),
        L::Rust => Style::Line("//"),
        L::Json => Style::Line(""),
        _ => Style::Line("#"),
    }
}

/// The notice as a comment of `language`.
pub fn comment(language: LanguageId, notice: &str) -> Option<String> {
    let lines: Vec<&str> = notice.lines().collect();
    Some(match style(language) {
        Style::Line("") => return None,
        Style::Line(token) => lines
            .iter()
            .map(|l| {
                if l.is_empty() {
                    token.to_owned()
                } else {
                    format!("{token} {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Style::Block(open, prefix, close) => {
            let mut out = vec![open.to_owned()];
            out.extend(
                lines
                    .iter()
                    .map(|l| format!("{prefix}{l}").trim_end().to_owned()),
            );
            out.push(close.to_owned());
            out.join("\n")
        }
    })
}

/// Byte length of the lines that must stay first: a shebang, an XML declaration, `<?php`, a
/// Python encoding line.
fn preamble(text: &str) -> usize {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_end();
        let keep = (at == 0 && (t.starts_with("#!") || t.starts_with("<?xml") || t == "<?php"))
            || (t.starts_with('#') && t.contains("coding") && (t.contains(':') || t.contains('=')));
        if !keep {
            break;
        }
        at += line.len();
    }
    at
}

/// The comment right after the preamble (block or run of line comments), as a byte range with its
/// line break.
fn leading_comment(
    text: &str,
    from: usize,
    language: LanguageId,
) -> Option<std::ops::Range<usize>> {
    let rest = &text[from..];
    let skipped = rest.len() - rest.trim_start_matches(['\n', '\r']).len();
    let start = from + skipped;
    let body = &text[start..];
    match style(language) {
        Style::Block(open, _, close) if body.starts_with(open) => {
            let end = body.find(close)? + close.len();
            let end = start + end + usize::from(body[end..].starts_with('\n'));
            Some(start..end)
        }
        Style::Block(..) => {
            // Also a run of `//` comments in C-like languages.
            line_run(text, start, "//")
        }
        Style::Line(token) if !token.is_empty() => line_run(text, start, token),
        _ => None,
    }
}

fn line_run(text: &str, start: usize, token: &str) -> Option<std::ops::Range<usize>> {
    let mut end = start;
    for line in text[start..].split_inclusive('\n') {
        if !line.trim_start().starts_with(token) || line.trim_start().starts_with("#!") {
            break;
        }
        end += line.len();
    }
    (end > start).then_some(start..end)
}

/// The text of `path` with the notice inserted at the top, or the existing notice (a leading
/// comment holding the keyword) replaced; `None` when the file already has exactly this notice or
/// its language has no comments (`SPEC-0155` behaviour 2).
pub fn update(path: &Path, text: &str, profile: &Profile, values: &Values) -> Option<String> {
    let language = LanguageId::detect(path)?;
    let notice = comment(language, &profile.render(values))?;
    let keep = preamble(text);
    let keyword = profile.keyword.to_lowercase();
    let existing = leading_comment(text, keep, language)
        .filter(|r| text[r.clone()].to_lowercase().contains(&keyword));
    let out = match existing {
        Some(r) => {
            if text[r.clone()].trim_end() == notice {
                return None;
            }
            format!("{}{notice}\n{}", &text[..r.start], &text[r.end..])
        }
        None => {
            let gap = if text[keep..].trim().is_empty() {
                ""
            } else {
                "\n"
            };
            format!("{}{notice}\n{gap}{}", &text[..keep], &text[keep..])
        }
    };
    (out != text).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile {
        Profile {
            notice: "Copyright (c) $today.year $project.name.\nAll rights reserved.".into(),
            keyword: "Copyright".into(),
        }
    }

    fn values(year: i32) -> Values {
        Values {
            year,
            file_name: "A.java".into(),
            project: "demo".into(),
            user: "me".into(),
        }
    }

    #[test]
    fn inserts_and_updates_the_notice_per_language() {
        let java = "package a;\n\nclass A {}\n";
        let once = update(Path::new("A.java"), java, &profile(), &values(2025)).unwrap();
        assert_eq!(
            once,
            "/*\n * Copyright (c) 2025 demo.\n * All rights reserved.\n */\n\npackage a;\n\nclass A {}\n"
        );
        assert_eq!(
            update(Path::new("A.java"), &once, &profile(), &values(2025)),
            None
        );
        let next = update(Path::new("A.java"), &once, &profile(), &values(2026)).unwrap();
        assert!(next.starts_with("/*\n * Copyright (c) 2026 demo."));
        assert!(next.ends_with("*/\n\npackage a;\n\nclass A {}\n"));
        let py = "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\nprint(1)\n";
        assert_eq!(
            update(Path::new("x.py"), py, &profile(), &values(2025)).unwrap(),
            "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\n# Copyright (c) 2025 demo.\n# All rights reserved.\n\nprint(1)\n"
        );
        let xml = "<?xml version=\"1.0\"?>\n<a/>\n";
        assert!(
            update(Path::new("a.xml"), xml, &profile(), &values(2025))
                .unwrap()
                .starts_with("<?xml version=\"1.0\"?>\n<!--\n  Copyright (c) 2025 demo.\n")
        );
        assert_eq!(
            update(Path::new("a.json"), "{}", &profile(), &values(2025)),
            None
        );
        // A leading comment without the keyword stays; the notice goes above it.
        let rs = "// Helpers.\nfn a() {}\n";
        assert!(
            update(Path::new("a.rs"), rs, &profile(), &values(2025))
                .unwrap()
                .ends_with("reserved.\n\n// Helpers.\nfn a() {}\n")
        );
    }

    #[test]
    fn years_of_days() {
        assert_eq!(year_of_day(0), 1970);
        assert_eq!(year_of_day(20_759), 2026);
        assert_eq!(year_of_day(20_453), 2025);
        assert!(current_year() >= 2026);
    }

    #[test]
    fn reads_the_reference_profile_format() {
        let dir = std::env::temp_dir().join(format!("idecraft-copyright-{}", std::process::id()));
        let c = dir.join(".idea/copyright");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(
            c.join("profiles_settings.xml"),
            "<component name=\"CopyrightManager\">\n  <settings default=\"Acme\" />\n</component>\n",
        )
        .unwrap();
        std::fs::write(
            c.join("Acme.xml"),
            "<component name=\"CopyrightManager\">\n  <copyright>\n    <option name=\"notice\" value=\"&#169; $today.year Acme &amp; Co.&#10;MIT\" />\n    <option name=\"myName\" value=\"Acme\" />\n  </copyright>\n</component>\n",
        )
        .unwrap();
        let p = load_profile(&dir).unwrap();
        assert_eq!(p.notice, "© $today.year Acme & Co.\nMIT");
        assert_eq!(p.keyword, "Copyright");
        std::fs::create_dir_all(dir.join(".idecraft")).unwrap();
        std::fs::write(
            dir.join(".idecraft/copyright.txt"),
            "keyword: ©\n© $today.year Own\n",
        )
        .unwrap();
        let own = load_profile(&dir).unwrap();
        assert_eq!(
            (own.keyword.as_str(), own.notice.as_str()),
            ("©", "© $today.year Own\n")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

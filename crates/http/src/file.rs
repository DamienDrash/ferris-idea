//! Request files (`SPEC-0046` behaviours 1–3, 9 and 10).

use std::collections::BTreeMap;

use serde::Serialize;

/// One request as written in the file (before variables are resolved).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawRequest {
    /// Name from `### name` or `# @name name`.
    pub name: Option<String>,
    /// 1-based line of the request line.
    pub line: usize,
    /// Method.
    pub method: String,
    /// URL.
    pub url: String,
    /// Headers in order.
    pub headers: Vec<(String, String)>,
    /// Body (empty when none).
    pub body: String,
    /// Settings written before the request line (behaviour 10).
    pub settings: RequestSettings,
    /// What the file asks for that is not done, one sentence each: a response handler or a response
    /// target that was left out (behaviour 9), a setting that could not be read, a line in the
    /// headers that is not a header.
    pub notes: Vec<String>,
}

/// Settings of one request (behaviour 10).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RequestSettings {
    /// `# @no-redirect`: the first response is the result, also a 3xx.
    pub no_redirect: bool,
    /// `# @timeout N`: seconds for connecting and for each read, 1 to 600.
    pub timeout_secs: Option<u64>,
}

/// A parsed request file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestFile {
    /// Variables defined with `@name = value`.
    pub variables: BTreeMap<String, String>,
    /// Requests in file order.
    pub requests: Vec<RawRequest>,
}

const METHODS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    (t.starts_with('#') && !t.starts_with("###")) || t.starts_with("//")
}

fn named(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix('#')
        .or_else(|| t.strip_prefix("//"))?
        .trim_start();
    rest.strip_prefix("@name")
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
}

/// Reads a setting line (`# @no-redirect`, `# @timeout N`) into `settings`; a note for a value
/// that cannot be used. False for any other line.
fn setting(line: &str, settings: &mut RequestSettings, notes: &mut Vec<String>) -> bool {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix('#').or_else(|| t.strip_prefix("//")) else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix('@') else {
        return false;
    };
    let (word, value) = match rest.split_once(char::is_whitespace) {
        Some((w, v)) => (w, v.trim()),
        None => (rest.trim_end(), ""),
    };
    match word {
        "no-redirect" => settings.no_redirect = true,
        "timeout" => match value.parse::<u64>() {
            Ok(n) if (1..=600).contains(&n) => settings.timeout_secs = Some(n),
            _ => notes.push(format!(
                "The setting @timeout {value} was not used: it takes 1 to 600 seconds."
            )),
        },
        _ => return false,
    }
    true
}

/// Whether a line starts a part that follows a body and is never sent (behaviour 9).
fn starts_unsupported(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("> {%") || t.starts_with(">> ") || t.starts_with(">>! ") || t.starts_with("<> ")
}

/// Takes one response handler, response target or stored-response reference off the end of
/// `body`; the note about it. `None` when the body ends with none (behaviour 9).
fn take_unsupported(body: &mut Vec<&str>) -> Option<String> {
    let last = body.last()?.trim();
    if let Some(path) = last
        .strip_prefix(">>! ")
        .or_else(|| last.strip_prefix(">> "))
    {
        let note = format!(
            "The response was not saved to {}: saving responses is not supported.",
            path.trim()
        );
        body.pop();
        return Some(note);
    }
    if let Some(path) = last.strip_prefix("<> ") {
        let note = format!(
            "The stored response {} was not used: comparing responses is not supported.",
            path.trim()
        );
        body.pop();
        return Some(note);
    }
    if last.ends_with("%}")
        && let Some(start) = body
            .iter()
            .rposition(|l| l.trim_start().starts_with("> {%"))
    {
        body.truncate(start);
        return Some(
            "The response handler script was not run: scripts in request files are not supported."
                .to_owned(),
        );
    }
    if let Some(path) = last.strip_prefix("> ")
        && !path.trim().contains(char::is_whitespace)
        && (path.trim().ends_with(".js") || path.trim().ends_with(".mjs"))
    {
        let note = format!(
            "The response handler {} was not run: scripts are not supported.",
            path.trim()
        );
        body.pop();
        return Some(note);
    }
    None
}

/// The pieces of a request line (`METHOD URL [VERSION]`) as byte ranges of `line`: split at white
/// space outside `{{…}}`, so a placeholder with spaces such as `{{$random.integer(1, 5)}}` stays in
/// one piece; a `{{` that is not closed keeps the rest of the line in its piece (`SPEC-0046` v6
/// behaviour 2).
pub fn request_line_words(line: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut open = false;
    let mut i = 0;
    while let Some(c) = line[i..].chars().next() {
        if !open && line[i..].starts_with("{{") {
            open = true;
            start.get_or_insert(i);
            i += 2;
            continue;
        }
        if open && line[i..].starts_with("}}") {
            open = false;
            i += 2;
            continue;
        }
        if !open && c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push(s..i);
            }
        } else {
            start.get_or_insert(i);
        }
        i += c.len_utf8();
    }
    if let Some(s) = start {
        out.push(s..line.len());
    }
    out
}

/// Method and URL of the request line `t` (trimmed): the URL runs from the piece after the method
/// to the HTTP version at the end, or to the end, so white space left in it is not dropped but
/// refused when the request is checked (`SPEC-0046` v6 behaviour 2).
fn request_line(t: &str) -> (String, String) {
    let words = request_line_words(t);
    let Some(first) = words.first() else {
        return ("GET".to_owned(), String::new());
    };
    let (method, rest) = if METHODS.contains(&t[first.clone()].to_ascii_uppercase().as_str()) {
        (t[first.clone()].to_ascii_uppercase(), &words[1..])
    } else {
        ("GET".to_owned(), &words[..])
    };
    let rest = match rest.split_last() {
        Some((last, before)) if !before.is_empty() && t[last.clone()].starts_with("HTTP/") => {
            before
        }
        _ => rest,
    };
    let url = match (rest.first(), rest.last()) {
        (Some(a), Some(b)) => t[a.start..b.end].to_owned(),
        _ => String::new(),
    };
    (method, url)
}

fn variable(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix('@')?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-'))
    .then(|| (name.to_owned(), value.trim().to_owned()))
}

/// Parses a request file (behaviours 1, 2).
pub fn parse(text: &str) -> RequestFile {
    let mut variables = BTreeMap::new();
    let mut requests = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut name: Option<String> = None;
    let mut settings = RequestSettings::default();
    let mut notes: Vec<String> = Vec::new();
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("###") {
            let n = rest.trim();
            name = (!n.is_empty()).then(|| n.to_owned());
            settings = RequestSettings::default();
            notes.clear();
            i += 1;
            continue;
        }
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some(n) = named(line) {
            name = Some(n);
            i += 1;
            continue;
        }
        if setting(line, &mut settings, &mut notes) || is_comment(line) {
            i += 1;
            continue;
        }
        if let Some((k, v)) = variable(line) {
            variables.insert(k, v);
            i += 1;
            continue;
        }
        // Request line.
        let start = i + 1;
        let (method, mut url) = request_line(t);
        i += 1;
        while i < lines.len() {
            let c = lines[i].trim();
            if c.starts_with('?') || c.starts_with('&') {
                url.push_str(c);
                i += 1;
            } else {
                break;
            }
        }
        let mut headers: Vec<(String, String)> = Vec::new();
        // The indentation of the last header line; a deeper line continues its value.
        let mut header_indent = 0;
        while i < lines.len()
            && !lines[i].trim().is_empty()
            && !lines[i].trim().starts_with("###")
            && !starts_unsupported(lines[i])
        {
            let l = lines[i];
            let indent = l.len() - l.trim_start().len();
            if is_comment(l) {
            } else if let Some(last) = headers.last_mut().filter(|_| indent > header_indent) {
                if !last.1.is_empty() {
                    last.1.push(' ');
                }
                last.1.push_str(l.trim());
            } else if let Some((k, v)) = l.split_once(':') {
                headers.push((k.trim().to_owned(), v.trim().to_owned()));
                header_indent = indent;
            } else {
                notes.push(format!(
                    "Line {} is not a header and was left out: {}",
                    i + 1,
                    l.trim()
                ));
            }
            i += 1;
        }
        let mut body_lines = Vec::new();
        if i < lines.len() && (lines[i].trim().is_empty() || starts_unsupported(lines[i])) {
            if lines[i].trim().is_empty() {
                i += 1;
            }
            while i < lines.len() && !lines[i].trim_start().starts_with("###") {
                body_lines.push(lines[i]);
                i += 1;
            }
        }
        // Trailing blank and comment lines belong to the next separator; response handlers and
        // response targets after the body are never sent (behaviour 9).
        let mut left_out = Vec::new();
        loop {
            while body_lines
                .last()
                .is_some_and(|l| l.trim().is_empty() || is_comment(l))
            {
                body_lines.pop();
            }
            match take_unsupported(&mut body_lines) {
                Some(note) => left_out.push(note),
                None => break,
            }
        }
        notes.extend(left_out.into_iter().rev());
        requests.push(RawRequest {
            name: name.take(),
            line: start,
            method,
            url,
            headers,
            body: body_lines.join("\n"),
            settings: std::mem::take(&mut settings),
            notes: std::mem::take(&mut notes),
        });
    }
    RequestFile {
        variables,
        requests,
    }
}

/// Replaces `{{name}}` in `text`; returns the text and the unresolved names (behaviour 3).
pub fn substitute(
    text: &str,
    file: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> (String, Vec<String>) {
    let mut missing = Vec::new();
    let out = expand(text, file, env, &mut missing, 0);
    missing.sort();
    missing.dedup();
    (out, missing)
}

fn expand(
    text: &str,
    file: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    missing: &mut Vec<String>,
    depth: usize,
) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            // A `{{` that is never closed is unresolved too, named with the rest of its line:
            // nothing goes out with half a placeholder (SPEC-0046 v6 behaviour 3).
            let line = after.lines().next().unwrap_or_default().trim_end();
            missing.push(format!("{{{{{line}"));
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        match file.get(name).or_else(|| env.get(name)) {
            // Variables may refer to others; the depth limit stops cycles.
            Some(value) if depth < 10 => {
                out.push_str(&expand(value, file, env, missing, depth + 1))
            }
            Some(_) => {
                missing.push(name.to_owned());
                out.push_str(&rest[start..start + 2 + end + 2]);
            }
            None => match dynamic(name) {
                Some(value) => out.push_str(&value),
                None => {
                    missing.push(name.to_owned());
                    out.push_str(&rest[start..start + 2 + end + 2]);
                }
            },
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// A random number. Not for secrets: it comes from the hasher seeds of the standard library.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

/// A random UUID of version 4 (RFC 9562 §5.4).
fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&random_u64().to_be_bytes());
    bytes[8..].copy_from_slice(&random_u64().to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// Seconds since 1970-01-01 UTC as `2026-11-13T09:30:00Z`.
fn iso_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days to a civil date (proleptic Gregorian calendar), counted in 400-year eras from 0000-03-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The longest text `$random.alphabetic(n)` and its kin generate (`SPEC-0046` v6 behaviour 3), so
/// a slip of the finger cannot fill the memory.
pub const MAX_RANDOM_LENGTH: usize = 10_000;

const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const LETTERS_DIGITS_UNDERSCORE: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_";
const HEX_DIGITS: &[u8] = b"0123456789abcdef";
const LOWER_DIGITS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// `n` characters drawn from `alphabet`.
fn random_text(alphabet: &[u8], n: usize) -> String {
    (0..n)
        .map(|_| char::from(alphabet[(random_u64() % alphabet.len() as u64) as usize]))
        .collect()
}

/// A dynamic name split into its function and arguments: `$random.integer(1, 5)` is
/// `("$random.integer", ["1", "5"])`; `$random.integer` and `$random.integer()` have none. `None`
/// when the parentheses are not closed at the end.
fn call(name: &str) -> Option<(&str, Vec<&str>)> {
    let Some(open) = name.find('(') else {
        return Some((name, Vec::new()));
    };
    let inner = name[open + 1..].strip_suffix(')')?;
    let args = if inner.trim().is_empty() {
        Vec::new()
    } else {
        inner.split(',').map(str::trim).collect()
    };
    Some((name[..open].trim_end(), args))
}

/// A whole number from `from` up to but not including `to`.
fn random_integer(from: i64, to: i64) -> Option<String> {
    (from < to).then(|| {
        let span = (i128::from(to) - i128::from(from)) as u128;
        (i128::from(from) + (u128::from(random_u64()) % span) as i128).to_string()
    })
}

/// A decimal number from `from` up to but not including `to`.
fn random_float(from: f64, to: f64) -> Option<String> {
    if !(from.is_finite() && to.is_finite() && from < to) {
        return None;
    }
    // 53 random bits: a fraction from 0 up to but not including 1.
    let fraction = (random_u64() >> 11) as f64 / (1u64 << 53) as f64;
    let value = from + (to - from) * fraction;
    // Rounding may reach `to`, which is never given.
    Some(if value < to { value } else { from }.to_string())
}

/// A value generated for each use of a `$` name (behaviour 3, v6 with arguments); `None` for a
/// name that is not one or arguments that do not fit, which leaves it unresolved.
fn dynamic(name: &str) -> Option<String> {
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    };
    let length = |args: &[&str]| match args {
        [n] => n
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=MAX_RANDOM_LENGTH).contains(n)),
        _ => None,
    };
    let (function, args) = call(name)?;
    match (function, args.as_slice()) {
        ("$uuid" | "$random.uuid", []) => Some(uuid_v4()),
        ("$timestamp", []) => Some(now().to_string()),
        ("$isoTimestamp", []) => Some(iso_utc(now())),
        ("$randomInt", []) => Some((random_u64() % 1_001).to_string()),
        ("$random.integer", []) => random_integer(0, 1_000),
        ("$random.integer", [from, to]) => random_integer(from.parse().ok()?, to.parse().ok()?),
        ("$random.float", []) => random_float(0., 1_000.),
        ("$random.float", [from, to]) => random_float(from.parse().ok()?, to.parse().ok()?),
        ("$random.alphabetic", args) => Some(random_text(LETTERS, length(args)?)),
        ("$random.alphanumeric", args) => {
            Some(random_text(LETTERS_DIGITS_UNDERSCORE, length(args)?))
        }
        ("$random.hexadecimal", args) => Some(random_text(HEX_DIGITS, length(args)?)),
        // A domain reserved for examples (RFC 2606): the address never belongs to a person.
        ("$random.email", []) => Some(format!("{}@example.com", random_text(LOWER_DIGITS, 10))),
        _ => None,
    }
}

/// A request with variables resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedRequest {
    /// Name, if any.
    pub name: Option<String>,
    /// 1-based line in the file.
    pub line: usize,
    /// Method.
    pub method: String,
    /// URL.
    pub url: String,
    /// Headers.
    pub headers: Vec<(String, String)>,
    /// Body.
    pub body: String,
    /// Names that could not be resolved; the request must not be sent while this is not empty.
    pub unresolved: Vec<String>,
}

/// `url` with `http://` in front when it names no scheme, such as `localhost:8080/api` or
/// `127.0.0.1:63344/api` (`SPEC-0046` v5 behaviour 4); a URL with a scheme stays as it is, so
/// `ftp://…` is still refused and `https` is never dropped.
pub fn with_default_scheme(url: &str) -> String {
    if url.contains("://") || url.is_empty() {
        url.to_owned()
    } else {
        format!("http://{url}")
    }
}

/// Resolves the variables of every request in `file` with `env` as fallback.
pub fn resolve(file: &RequestFile, env: &BTreeMap<String, String>) -> Vec<ResolvedRequest> {
    file.requests
        .iter()
        .map(|r| {
            let mut unresolved = Vec::new();
            let mut sub = |s: &str| {
                let (v, m) = substitute(s, &file.variables, env);
                unresolved.extend(m);
                v
            };
            let url = sub(&r.url);
            let headers = r.headers.iter().map(|(k, v)| (sub(k), sub(v))).collect();
            let body = sub(&r.body);
            unresolved.sort();
            unresolved.dedup();
            // A URL without a scheme is sent over http once its variables are resolved
            // (behaviour 4).
            let url = if unresolved.is_empty() {
                with_default_scheme(&url)
            } else {
                url
            };
            ResolvedRequest {
                name: r.name.clone(),
                line: r.line,
                method: r.method.clone(),
                url,
                headers,
                body,
                unresolved,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "@host = localhost:8080
@token = secret-{{user}}

### List users
GET http://{{host}}/users
    ?page=2
    &size=10
Accept: application/json
# a comment inside headers is ignored

###
# @name create
POST http://{{host}}/users HTTP/1.1
Content-Type: application/json
Authorization: Bearer {{token}}

{\"name\": \"Ada\"}


// separate request with a bare URL
###
http://{{host}}/health
";

    /// SPEC-0046 v5 AK-11: a URL without a scheme is sent over http, also after its host came
    /// from a variable; a URL with a scheme keeps it, one with unresolved names stays as written.
    #[test]
    fn a_url_without_a_scheme_is_http() {
        let file = parse(
            "@host = localhost:8080\n\nPOST 127.0.0.1:63344/api/update\nContent-Type: application/json\n\n{}\n\n###\n\nGET {{host}}/users\n\n###\nGET https://example.com/\n\n###\nGET {{missing}}/x\n",
        );
        let urls: Vec<String> = resolve(&file, &BTreeMap::new())
            .into_iter()
            .map(|r| r.url)
            .collect();
        assert_eq!(
            urls,
            [
                "http://127.0.0.1:63344/api/update",
                "http://localhost:8080/users",
                "https://example.com/",
                "{{missing}}/x",
            ]
        );
        assert_eq!(with_default_scheme("ftp://x/y"), "ftp://x/y");
        assert_eq!(with_default_scheme(""), "");
    }

    #[test]
    fn parsing_requests() {
        let f = parse(FILE);
        assert_eq!(f.variables["host"], "localhost:8080");
        assert_eq!(f.requests.len(), 3);
        let a = &f.requests[0];
        assert_eq!(a.name.as_deref(), Some("List users"));
        assert_eq!((a.method.as_str(), a.line), ("GET", 5));
        assert_eq!(a.url, "http://{{host}}/users?page=2&size=10");
        assert_eq!(a.headers, [("Accept".into(), "application/json".into())]);
        assert_eq!(a.body, "");
        let b = &f.requests[1];
        assert_eq!(b.name.as_deref(), Some("create"));
        assert_eq!(b.method, "POST");
        assert_eq!(b.headers.len(), 2);
        assert_eq!(b.body, "{\"name\": \"Ada\"}");
        let c = &f.requests[2];
        assert_eq!(
            (c.method.as_str(), c.url.as_str()),
            ("GET", "http://{{host}}/health")
        );
    }

    #[test]
    fn variables_and_environment() {
        let f = parse(FILE);
        let env = BTreeMap::from([("user".to_owned(), "ada".to_owned())]);
        let r = resolve(&f, &env);
        assert_eq!(r[0].url, "http://localhost:8080/users?page=2&size=10");
        assert_eq!(r[1].headers[1].1, "Bearer secret-ada");
        assert!(r.iter().all(|x| x.unresolved.is_empty()));
        // File variables win over the environment.
        let env = BTreeMap::from([
            ("user".to_owned(), "x".to_owned()),
            ("host".to_owned(), "other".to_owned()),
        ]);
        assert!(resolve(&f, &env)[2].url.contains("localhost:8080"));
        // Missing names are listed and kept literally.
        let r = resolve(&f, &BTreeMap::new());
        assert_eq!(r[1].unresolved, ["user"]);
        assert!(r[1].headers[1].1.contains("{{user}}"));
        let (cyc, missing) = substitute(
            "{{a}}",
            &BTreeMap::from([("a".into(), "{{a}}".into())]),
            &BTreeMap::new(),
        );
        assert!(cyc.contains("{{a}}") && missing == ["a"]);
    }

    #[test]
    fn dynamic_variables() {
        let none = BTreeMap::new();
        let (a, missing) = substitute("{{$uuid}} {{ $random.uuid }}", &none, &none);
        assert!(missing.is_empty());
        let ids: Vec<&str> = a.split(' ').collect();
        assert_ne!(ids[0], ids[1], "generated for each use");
        for id in ids {
            let groups: Vec<usize> = id.split('-').map(str::len).collect();
            assert_eq!(groups, [8, 4, 4, 4, 12], "{id}");
            assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
            assert_eq!(&id[14..15], "4", "version 4: {id}");
            assert!(
                matches!(&id[19..20], "8" | "9" | "a" | "b"),
                "variant: {id}"
            );
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let (t, _) = substitute("{{$timestamp}}", &none, &none);
        assert!(t.parse::<u64>().unwrap().abs_diff(now) <= 2);
        let (iso, _) = substitute("{{$isoTimestamp}}", &none, &none);
        assert_eq!(iso.len(), 20, "{iso}");
        assert!(iso.ends_with('Z') && &iso[10..11] == "T");
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951_782_399), "2000-02-28T23:59:59Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_794_562_200), "2026-11-13T09:30:00Z");
        assert_eq!(iso_utc(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(iso_utc(4_107_542_400), "2100-03-01T00:00:00Z");
        for _ in 0..50 {
            let (n, _) = substitute("{{$randomInt}}", &none, &none);
            assert!(n.parse::<u32>().unwrap() <= 1000);
        }
        // A name the file or the environment defines wins; an unknown `$` name is unresolved.
        let env = BTreeMap::from([("$uuid".to_owned(), "fixed".to_owned())]);
        assert_eq!(substitute("{{$uuid}}", &none, &env).0, "fixed");
        let (kept, missing) = substitute("{{$nope}}", &none, &none);
        assert_eq!(
            (kept.as_str(), missing),
            ("{{$nope}}", vec!["$nope".to_owned()])
        );
    }

    /// SPEC-0046 v6 AK-13: placeholders are found before the request line is split.
    #[test]
    fn placeholders_with_spaces_stay_whole() {
        let f = parse(
            "GET http://{{host}}/d?u={{$uuid}}&r={{$random.integer(1, 5)}} HTTP/1.1\n\n\
             ###\nPOST {{ base }}/x?id={{ $uuid }}\n\n\
             ###\n{{base}}/bare?f={{$random.float(0.5, 1)}}\n\n\
             ###\nGET http://h/a b HTTP/1.1\n\n\
             ###\nGET http://h/x?v={{$random.integer(1, 5) HTTP/1.1\n",
        );
        let urls: Vec<(&str, &str)> = f
            .requests
            .iter()
            .map(|r| (r.method.as_str(), r.url.as_str()))
            .collect();
        assert_eq!(
            urls,
            [
                (
                    "GET",
                    "http://{{host}}/d?u={{$uuid}}&r={{$random.integer(1, 5)}}"
                ),
                ("POST", "{{ base }}/x?id={{ $uuid }}"),
                ("GET", "{{base}}/bare?f={{$random.float(0.5, 1)}}"),
                // White space outside a placeholder stays in the URL, where the request check
                // refuses it, instead of cutting the URL short.
                ("GET", "http://h/a b"),
                // Not closed: the rest of the line, the version too.
                ("GET", "http://h/x?v={{$random.integer(1, 5) HTTP/1.1"),
            ]
        );
        let line = "GET  {{a b}}c  HTTP/1.1 ";
        let words: Vec<&str> = request_line_words(line)
            .into_iter()
            .map(|r| &line[r])
            .collect();
        assert_eq!(words, ["GET", "{{a b}}c", "HTTP/1.1"]);
        assert!(request_line_words("   ").is_empty());

        let env = BTreeMap::from([("host".to_owned(), "127.0.0.1:9".to_owned())]);
        let r = resolve(&f, &env);
        assert!(r[0].unresolved.is_empty(), "{:?}", r[0].unresolved);
        let n: i64 = r[0].url.rsplit('=').next().unwrap().parse().unwrap();
        assert!((1..5).contains(&n), "{}", r[0].url);
        assert_eq!(r[1].unresolved, ["base"]);
        assert_eq!(
            r[4].unresolved,
            ["{{$random.integer(1, 5) HTTP/1.1"],
            "a placeholder that is not closed is unresolved"
        );
    }

    /// SPEC-0046 v6 AK-14: the dynamic values with arguments, their ranges and what does not fit.
    #[test]
    fn random_values_with_arguments() {
        let none = BTreeMap::new();
        let value = |text: &str| {
            let (v, missing) = substitute(text, &none, &none);
            assert!(missing.is_empty(), "{text}: {missing:?}");
            v
        };
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..400 {
            let n: i64 = value("{{$random.integer(1, 5)}}").parse().unwrap();
            assert!((1..5).contains(&n), "{n}: `to` is never reached");
            seen.insert(n);
            let n: i64 = value("{{ $random.integer( -3 ,-1 ) }}").parse().unwrap();
            assert!((-3..-1).contains(&n), "{n}");
            let n: i64 = value("{{$random.integer}}").parse().unwrap();
            assert!((0..1000).contains(&n));
            let n: i64 = value("{{$random.integer()}}").parse().unwrap();
            assert!((0..1000).contains(&n));
            let x: f64 = value("{{$random.float(10.5, 20.3)}}").parse().unwrap();
            assert!((10.5..20.3).contains(&x), "{x}");
            let x: f64 = value("{{$random.float}}").parse().unwrap();
            assert!((0.0..1000.0).contains(&x), "{x}");
        }
        assert_eq!(seen.len(), 4, "every value from 1 to 4 comes: {seen:?}");
        assert_eq!(value("{{$random.integer(7, 8)}}"), "7");

        let alpha = value("{{$random.alphabetic(40)}}");
        assert_eq!(alpha.len(), 40);
        assert!(alpha.chars().all(|c| c.is_ascii_alphabetic()), "{alpha}");
        let alnum = value("{{$random.alphanumeric(300)}}");
        assert_eq!(alnum.len(), 300);
        assert!(
            alnum.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{alnum}"
        );
        let hex = value("{{$random.hexadecimal(64)}}");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{hex}"
        );
        assert_eq!(
            value(&format!("{{{{$random.hexadecimal({MAX_RANDOM_LENGTH})}}}}")).len(),
            MAX_RANDOM_LENGTH
        );
        let mail = value("{{$random.email}}");
        let (local, domain) = mail.split_once('@').unwrap();
        assert_eq!(domain, "example.com");
        assert_eq!(local.len(), 10);
        assert!(
            local
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
        assert_ne!(value("{{$random.email}}"), mail, "generated for each use");
        let id = value("{{$random.uuid}}");
        assert_eq!(id.len(), 36);

        // What does not fit stays unresolved, named as written.
        for bad in [
            "$random.integer(5, 1)",
            "$random.integer(1, 1)",
            "$random.integer(1)",
            "$random.integer(1, 2, 3)",
            "$random.integer(a, b)",
            "$random.integer(1.5, 3)",
            "$random.integer(1, 5",
            "$random.float(2, 1)",
            "$random.float(0, inf)",
            "$random.alphabetic",
            "$random.alphabetic(0)",
            "$random.alphanumeric(-1)",
            "$random.hexadecimal(10001)",
            "$random.email(3)",
            "$random.nothing(1)",
            "$foo bar",
        ] {
            let text = format!("x={{{{{bad}}}}}");
            let (kept, missing) = substitute(&text, &none, &none);
            assert_eq!(
                (kept.as_str(), missing),
                (text.as_str(), vec![bad.to_owned()])
            );
        }
        let (_, missing) = substitute("a {{$uuid}} b {{$random.integer(1,\nnext", &none, &none);
        assert_eq!(missing, ["{{$random.integer(1,"]);
    }

    #[test]
    fn settings_notes_and_parts_never_sent() {
        let f = parse(
            "### plain\nGET http://h/a\n\n### settings\n# @no-redirect\n// @timeout 5\n# @name set\nGET http://h/b\n\n\
             ### bad timeout\n# @timeout soon\n# @timeout 0\n# @timeout 601\n# @unknown x\nGET http://h/c\n\n\
             ### handler\nGET http://h/d\n\n> {% client.test(\"x\", function() {}); %}\n\n\
             ### long handler\nPOST http://h/e\nContent-Type: text/plain\n\nkept body\n> {%\n  client.log(response.body);\n%}\n\n>>! out.json\n\n\
             ### handler without blank line\nGET http://h/f\nAccept: */*\n> {% x %}\n\n\
             ### target and reference\nGET http://h/g\n\n>> ./saved.json\n<> ./previous.200.json\n\n\
             ### handler file\nGET http://h/h\n\n> ./check.js\n\n\
             ### a quote stays\nPOST http://h/i\n\n> quoted text\n> more.js and words\n",
        );
        let by = |name: &str| {
            f.requests
                .iter()
                .find(|r| r.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(by("plain").settings, RequestSettings::default());
        assert!(by("plain").notes.is_empty());
        let set = by("set");
        assert_eq!(
            set.settings,
            RequestSettings {
                no_redirect: true,
                timeout_secs: Some(5)
            }
        );
        let bad = by("bad timeout");
        assert_eq!(bad.settings.timeout_secs, None);
        assert_eq!(bad.notes.len(), 3, "{:?}", bad.notes);
        assert!(bad.notes[0].contains("@timeout soon"));

        let handler = by("handler");
        assert_eq!(handler.body, "", "the script is never sent");
        assert_eq!(handler.notes.len(), 1);
        assert!(handler.notes[0].contains("not run"));
        let long = by("long handler");
        assert_eq!(long.body, "kept body");
        assert_eq!(long.notes.len(), 2);
        assert!(long.notes[0].contains("not run") && long.notes[1].contains("out.json"));
        let close = by("handler without blank line");
        assert_eq!(close.headers, [("Accept".into(), "*/*".into())]);
        assert_eq!((close.body.as_str(), close.notes.len()), ("", 1));
        let target = by("target and reference");
        assert_eq!(target.body, "");
        assert!(target.notes[0].contains("./saved.json"));
        assert!(target.notes[1].contains("./previous.200.json"));
        assert_eq!(by("handler file").body, "");
        assert!(by("handler file").notes[0].contains("./check.js"));
        // Text that only looks similar is a body.
        assert_eq!(
            by("a quote stays").body,
            "> quoted text\n> more.js and words"
        );
        assert!(by("a quote stays").notes.is_empty());
        // Settings and notes belong to one request only.
        assert!(!by("handler").settings.no_redirect);
    }

    #[test]
    fn continued_headers() {
        let f = parse(
            "GET http://h/a\nX-Long: first part,\n  second part\n\tthird\nX-Next: yes\nnot a header\n  Indented: after a stray line\n\n###\nGET http://h/b\n  Accept: */*\n  X-Same: indent\n",
        );
        assert_eq!(
            f.requests[0].headers,
            [
                (
                    "X-Long".to_owned(),
                    "first part, second part third".to_owned()
                ),
                (
                    "X-Next".to_owned(),
                    "yes Indented: after a stray line".to_owned()
                ),
            ]
        );
        assert_eq!(f.requests[0].notes.len(), 1);
        assert!(
            f.requests[0].notes[0].contains("Line 6")
                && f.requests[0].notes[0].contains("not a header")
        );
        // Headers that are all indented alike stay separate headers.
        assert_eq!(
            f.requests[1].headers,
            [
                ("Accept".to_owned(), "*/*".to_owned()),
                ("X-Same".to_owned(), "indent".to_owned())
            ]
        );
    }
}

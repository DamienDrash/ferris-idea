//! "Report a problem with this database" (`SPEC-0301`): a report built on the user's machine,
//! without credentials, data or (unless the user keeps them) host and database names. Ferris IDEA never
//! sends it (ADR-0007); the user copies it or opens a prefilled issue in the browser.

use serde::Serialize;

use crate::ConnectionProfile;
use crate::connector::{Connector, Origin};

/// Where reports go unless the setting or the definition says otherwise: a prefilled issue from the
/// template in Ferris IDEA's repository (GitHub Issues, the project owner 2026-11-05; usable by others after
/// GO-4).
pub const DEFAULT_REPORT_URL: &str = "https://github.com/DamienDrash/ferris-idea/issues/new?template=database-system-problem.md&title={title}&body={body}";

/// Most characters of the body put into an issue URL (behaviour 4).
pub const URL_BODY_LIMIT: usize = 6000;
/// Most characters of a report title.
pub const TITLE_LIMIT: usize = 120;

/// The step that failed (behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReportStep {
    /// Opening the session.
    Connect,
    /// The connection test's query.
    TestQuery,
    /// Listing schemas.
    ListSchemas,
    /// Reading tables, keys and indexes.
    Introspect,
    /// Running a statement.
    RunStatement,
    /// Explaining a statement.
    Explain,
    /// Editing data.
    Edit,
    /// Anything else.
    #[default]
    Other,
}

impl ReportStep {
    /// Words used in the title and body.
    pub fn label(self) -> &'static str {
        match self {
            ReportStep::Connect => "connect",
            ReportStep::TestQuery => "test query",
            ReportStep::ListSchemas => "list schemas",
            ReportStep::Introspect => "introspect",
            ReportStep::RunStatement => "run statement",
            ReportStep::Explain => "explain",
            ReportStep::Edit => "edit",
            ReportStep::Other => "other",
        }
    }
}

/// What a report is built from.
#[derive(Debug, Clone)]
pub struct ReportInput<'a> {
    /// The database system.
    pub connector: &'a Connector,
    /// The connection, if the problem happened on one.
    pub profile: Option<&'a ConnectionProfile>,
    /// Version text the server reported, when a session was open.
    pub server_version: Option<String>,
    /// The failing step.
    pub step: ReportStep,
    /// The error message as shown.
    pub error: String,
    /// The failing statement; included only with `include_statement`.
    pub statement: Option<String>,
    /// Keep host, port, database and schema names (behaviour 3 checkbox).
    pub include_hosts: bool,
    /// Include the statement as written.
    pub include_statement: bool,
}

/// A report the user sees and edits before copying or opening it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Issue title.
    pub title: String,
    /// Markdown body.
    pub body: String,
}

impl Report {
    /// Title and body as copied to the clipboard.
    pub fn text(&self) -> String {
        format!("{}\n\n{}", self.title, self.body)
    }
}

/// Ferris IDEA's version, stage and platform, e.g. `0.8.0 (alpha, macos-aarch64)`.
pub fn ide_version() -> String {
    format!(
        "{} ({}, {}-{})",
        ferris_idea_core::release::VERSION,
        ferris_idea_core::release::STAGE,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Builds the report (behaviours 2–4).
pub fn build_report(input: &ReportInput<'_>) -> Report {
    let c = input.connector;
    let redact = |text: &str| redact(text, input.profile, input.include_hosts);
    let error = redact(input.error.trim());
    let first = error.lines().next().unwrap_or("").trim();
    let mut title = format!("[{}] {} fails: {first}", c.name, input.step.label());
    if title.chars().count() > TITLE_LIMIT {
        title = title.chars().take(TITLE_LIMIT - 1).collect::<String>() + "…";
    }
    let origin = match &c.origin {
        Origin::BuiltIn => "built-in".to_owned(),
        Origin::User(_) => "user definition".to_owned(),
        Origin::Imported(_) => "imported definition".to_owned(),
    };
    let mut body = String::new();
    body.push_str("## Environment\n\n");
    body.push_str(&format!("- Ferris IDEA: {}\n", ide_version()));
    body.push_str(&format!(
        "- Database system: {} (`{}`, {}, status {}, family {})\n",
        c.name,
        c.id,
        origin,
        serde_json::to_value(c.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
        c.family.name()
    ));
    if c.origin != Origin::BuiltIn {
        body.push_str(&format!(
            "- Definition: format {}, SHA-256 {}\n",
            crate::connector::FORMAT,
            sha256_hex(c.source.as_bytes())
        ));
    }
    if let Some(v) = &input.server_version {
        body.push_str(&format!("- Server version: {}\n", redact(v.trim())));
    }
    if let Some(p) = input.profile {
        body.push_str(&format!(
            "- SSL mode: {}, read-only: {}\n",
            crate::session::mode_name(p.ssl),
            p.read_only
        ));
    }
    body.push_str(&format!(
        "\n## What failed\n\nStep: {}\n\n```text\n{}\n```\n",
        input.step.label(),
        error
    ));
    if input.include_statement
        && let Some(statement) = &input.statement
    {
        body.push_str(&format!(
            "\n## Statement\n\n```sql\n{}\n```\n",
            statement.trim()
        ));
    }
    body.push_str("\n## What did you expect?\n\n\n");
    Report { title, body }
}

/// Replaces names that identify the user's systems by placeholders (behaviour 3): the profile's
/// host, port, database, user, name and file, any IPv4 or IPv6 address and `password=…` fragments.
/// With `include_hosts` host, port and database stay; user, profile name, path and passwords never.
pub fn redact(text: &str, profile: Option<&ConnectionProfile>, include_hosts: bool) -> String {
    let mut out = text.to_owned();
    out = redact_secrets(&out);
    if let Some(p) = profile {
        let mut names: Vec<(String, &str)> = Vec::new();
        if let Some(path) = &p.path {
            names.push((path.display().to_string(), "<path>"));
        }
        if !include_hosts {
            if let Some(h) = &p.host {
                names.push((h.clone(), "<host>"));
            }
            if let Some(d) = &p.database {
                names.push((d.clone(), "<database>"));
            }
        }
        if let Some(u) = &p.user {
            names.push((u.clone(), "<user>"));
        }
        names.push((p.name.clone(), "<connection>"));
        // Longer names first, so a host containing the database name is replaced whole.
        names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        for (name, placeholder) in names {
            if name.chars().count() >= 2 {
                out = replace_word(&out, &name, placeholder);
            }
        }
        if !include_hosts && let Some(port) = p.effective_port() {
            out = out.replace(&format!(":{port}"), ":<port>");
            out = replace_word(&out, &format!("port {port}"), "port <port>");
        }
    }
    if !include_hosts {
        out = redact_addresses(&out);
    }
    out
}

/// `text` with `name` replaced where it is not part of a longer word.
fn replace_word(text: &str, name: &str, placeholder: &str) -> String {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].chars().next();
        out.push_str(&rest[..at]);
        if word(before) || word(after) {
            out.push_str(name);
        } else {
            out.push_str(placeholder);
        }
        rest = &rest[at + name.len()..];
    }
    out.push_str(rest);
    out
}

/// `password=…`, `pwd=…` and `secret=…` values become `<secret>`.
fn redact_secrets(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let key = ["password=", "pwd=", "secret=", "password: "]
            .iter()
            .find(|k| lower[i..].starts_with(**k));
        if let Some(k) = key {
            out.push_str(&text[i..i + k.len()]);
            out.push_str("<secret>");
            i += k.len();
            while i < text.len() {
                let c = text[i..].chars().next().unwrap_or(' ');
                if c.is_whitespace() || matches!(c, '&' | ';' | ',' | '"' | '\'' | ')') {
                    break;
                }
                i += c.len_utf8();
            }
        } else {
            let c = text[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// IPv4 addresses (four dot-separated numbers ≤ 255) and IPv6 addresses (at least two `:` among
/// hex digits, with at least one digit group) become `<address>`.
fn redact_addresses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '.');
        if boundary {
            let end = token_end(&chars, i, |c| c.is_ascii_hexdigit() || c == '.' || c == ':');
            let token: String = chars[i..end].iter().collect();
            let token = token.trim_end_matches([':', '.']);
            if !token.is_empty() && (is_ipv4(token) || is_ipv6(token)) {
                out.push_str("<address>");
                i += token.chars().count();
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn token_end(chars: &[char], start: usize, f: impl Fn(char) -> bool) -> usize {
    let mut end = start;
    while end < chars.len() && f(chars[end]) {
        end += 1;
    }
    end
}

fn is_ipv4(s: &str) -> bool {
    let s = s.split(':').next().unwrap_or("");
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

fn is_ipv6(s: &str) -> bool {
    s.matches(':').count() >= 2
        && !s.contains('.')
        && s.split(':').all(|g| g.len() <= 4)
        && s.split(':').any(|g| !g.is_empty())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The URL template a report about `connector` goes to (behaviour 5): the definition's own
/// `report_url`; else the setting — empty for [`DEFAULT_REPORT_URL`], `off` for none (copy only).
/// Only `https://` templates count.
pub fn destination(setting: &str, connector: &Connector) -> Option<String> {
    let setting = setting.trim();
    let chosen = match (connector.report_url.as_deref(), setting) {
        (Some(own), _) => own,
        (None, "off") => return None,
        (None, "") => DEFAULT_REPORT_URL,
        (None, s) => s,
    };
    let chosen = chosen.trim();
    chosen.starts_with("https://").then(|| chosen.to_owned())
}

/// Checks a URL template typed into the settings (behaviour 5).
pub fn check_template(template: &str) -> Result<(), String> {
    // The same check as Settings › Advanced Settings (SPEC-0337 behaviour 5).
    ferris_idea_core::advanced_settings::check_report_url(template)
}

/// The issue URL for `report` (behaviour 4): `{title}` and `{body}` filled in URL-encoded, the body
/// cut to [`URL_BODY_LIMIT`] characters with a note. Returns the URL and whether it was cut.
pub fn issue_url(template: &str, report: &Report) -> Result<(String, bool), String> {
    check_template(template)?;
    let mut body = report.body.clone();
    let cut = body.chars().count() > URL_BODY_LIMIT;
    if cut {
        body = body.chars().take(URL_BODY_LIMIT).collect::<String>()
            + "\n\n(shortened — paste the full report from the clipboard)";
    }
    Ok((
        template
            .trim()
            .replace("{title}", &encode(&report.title))
            .replace("{body}", &encode(&body)),
        cut,
    ))
}

/// Percent-encoding of everything except unreserved characters (RFC 3986).
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{Catalogue, parse_definition};

    fn profile() -> ConnectionProfile {
        let mut p =
            ConnectionProfile::from_url("Prod orders", "tidb://alice@db7.corp.example:4000/orders")
                .unwrap();
        p.name = "Prod orders".into();
        p
    }

    fn input<'a>(c: &'a Connector, p: &'a ConnectionProfile, error: &str) -> ReportInput<'a> {
        ReportInput {
            connector: c,
            profile: Some(p),
            server_version: Some("8.0.11-TiDB-v8.5.3".into()),
            step: ReportStep::Introspect,
            error: error.into(),
            statement: Some("select secret_column from orders".into()),
            include_hosts: false,
            include_statement: false,
        }
    }

    #[test]
    fn redaction_removes_names_addresses_and_secrets() {
        let catalogue = Catalogue::built_in();
        let c = catalogue.get("tidb").unwrap();
        let p = profile();
        let error = "connection `Prod orders` (db7.corp.example:4000): user alice denied on \
                     database orders from 10.1.2.3 and fe80::1:2; dsn password=hunter2&x=1; \
                     see /Users/alice/x.db";
        let report = build_report(&input(c, &p, error));
        for secret in [
            "Prod orders",
            "db7.corp.example",
            "4000",
            "alice",
            "orders",
            "10.1.2.3",
            "fe80::1:2",
            "hunter2",
        ] {
            assert!(
                !report.text().contains(secret),
                "{secret} left in {}",
                report.text()
            );
        }
        assert!(report.body.contains("<host>:<port>"), "{}", report.body);
        assert!(report.body.contains("<address>"));
        assert!(report.body.contains("password=<secret>"));
        assert!(
            !report.body.contains("secret_column"),
            "statement only when asked"
        );
        // Names inside longer words stay.
        assert_eq!(redact("orders_archive", Some(&p), false), "orders_archive");
        let kept = build_report(&ReportInput {
            include_hosts: true,
            include_statement: true,
            ..input(c, &p, error)
        });
        assert!(
            kept.body.contains("db7.corp.example:4000") && kept.body.contains("database orders")
        );
        assert!(!kept.body.contains("alice") && !kept.body.contains("hunter2"));
        assert!(kept.body.contains("secret_column"));
    }

    #[test]
    fn title_body_and_definition_hash() {
        let catalogue = Catalogue::built_in();
        let c = catalogue.get("tidb").unwrap();
        let p = profile();
        let report = build_report(&input(
            c,
            &p,
            &format!("Unknown column {}\nsecond line", "x".repeat(200)),
        ));
        assert!(
            report
                .title
                .starts_with("[TiDB] introspect fails: Unknown column xxx")
        );
        assert_eq!(report.title.chars().count(), TITLE_LIMIT);
        for section in [
            "## Environment",
            "## What failed",
            "## What did you expect?",
            "Server version: 8.0.11-TiDB-v8.5.3",
            "status supported, family mysql",
        ] {
            assert!(
                report.body.contains(section),
                "{section} in {}",
                report.body
            );
        }
        assert!(!report.body.contains("SHA-256"), "built-ins carry no hash");
        let user = parse_definition(
            "format = 1\nid = \"acme\"\nname = \"Acme\"\nfamily = \"postgresql\"\n[connection]\nschemes = [\"acme\"]\n",
            crate::connector::Origin::Imported("acme.toml".into()),
        )
        .unwrap();
        let report = build_report(&ReportInput {
            server_version: None,
            profile: None,
            ..input(&user, &p, "boom")
        });
        assert!(report.body.contains("imported definition, status imported"));
        assert!(report.body.contains("SHA-256 ") && !report.body.contains("Server version"));
    }

    #[test]
    fn destination_and_issue_url() {
        let catalogue = Catalogue::built_in();
        let c = catalogue.get("tidb").unwrap();
        assert_eq!(destination("", c).as_deref(), Some(DEFAULT_REPORT_URL));
        assert_eq!(destination("off", c), None);
        assert_eq!(destination("http://x/{title}{body}", c), None);
        let t = "https://example.org/new?title={title}&body={body}";
        assert_eq!(destination(t, c).as_deref(), Some(t));
        let mut own = c.clone();
        own.report_url = Some("https://vendor.example/issues?t={title}&b={body}".into());
        assert!(destination(t, &own).unwrap().starts_with("https://vendor"));
        assert!(check_template("off").is_ok() && check_template(DEFAULT_REPORT_URL).is_ok());
        assert!(check_template("https://x/?q={title}").is_err());
        assert!(check_template("ftp://x/{title}{body}").is_err());
        let report = Report {
            title: "[TiDB] a & b".into(),
            body: "é".repeat(7000),
        };
        let (url, cut) = issue_url(t, &report).unwrap();
        assert!(cut);
        assert!(
            url.starts_with("https://example.org/new?title=%5BTiDB%5D%20a%20%26%20b&body=%C3%A9")
        );
        assert!(url.ends_with(&encode(
            "(shortened — paste the full report from the clipboard)"
        )));
        let (_, cut) = issue_url(
            t,
            &Report {
                title: "t".into(),
                body: "short".into(),
            },
        )
        .unwrap();
        assert!(!cut);
    }
}

//! Masking of secret-looking values in container details (`SPEC-0119` v5 behaviour 11, IMP-0384):
//! a value whose name suggests a secret, the password of a URL, and the value of a secret-looking
//! command-line option are replaced before anything is shown, returned over MCP or logged. There
//! is no way to reveal them in Ferris IDEA; `docker inspect` on the command line still shows them.

/// What a masked value reads as; the same for every value, so its length is not given away.
pub const MASK: &str = "••••••";

/// Parts of a name that mark it as secret wherever they stand (`DBPASSWORD`, `x-api-token`).
const ANYWHERE: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "credential",
    "apikey",
    "privatekey",
    "accesskey",
    "secretkey",
    "signingkey",
    "encryptionkey",
    "masterkey",
    "sessionkey",
    "authorization",
];

/// Whole words that mark a name as secret (`DB_PWD`, `BASIC_AUTH`, `SENTRY_DSN`).
const WORDS: &[&str] = &[
    "pwd", "pass", "auth", "dsn", "jwt", "cookie", "bearer", "pat", "otp", "salt", "pepper",
];

/// Word pairs that mark a name as secret (`API_KEY`, `PRIVATE_KEY`, `CLIENT_SECRET`).
const PAIRS: &[(&str, &str)] = &[
    ("api", "key"),
    ("private", "key"),
    ("access", "key"),
    ("secret", "key"),
    ("signing", "key"),
    ("encryption", "key"),
    ("master", "key"),
    ("session", "key"),
    ("license", "key"),
];

/// The words of a name: split at anything but letters and digits and at lower-to-upper changes.
fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether a variable, label or option name suggests that its value is secret.
pub fn is_secret_name(name: &str) -> bool {
    let w = words(name);
    let joined: String = w.concat();
    ANYWHERE.iter().any(|p| joined.contains(p))
        || w.iter().any(|x| WORDS.contains(&x.as_str()))
        || w.windows(2)
            .any(|p| PAIRS.contains(&(p[0].as_str(), p[1].as_str())))
}

/// `value` with the password of every `scheme://user:password@` in it masked, and whether one was.
fn mask_url_passwords(value: &str) -> (String, bool) {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    let mut masked = false;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        // The authority ends at the first `/`, `?` or `#` (or whitespace) after the scheme.
        let end = tail
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rfind('@') {
            Some(at_sign) => {
                let userinfo = &authority[..at_sign];
                match userinfo.find(':') {
                    Some(colon) => {
                        out.push_str(&userinfo[..=colon]);
                        out.push_str(MASK);
                        masked = true;
                    }
                    None => out.push_str(userinfo),
                }
                out.push_str(&authority[at_sign..]);
            }
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    (out, masked)
}

/// The value as it may be shown for `name`, and whether something was masked.
pub fn mask_value(name: &str, value: &str) -> (String, bool) {
    if is_secret_name(name) && !value.is_empty() {
        return (MASK.to_owned(), true);
    }
    mask_url_passwords(value)
}

/// A `NAME=value` entry (an environment entry) with its value masked when needed.
pub fn mask_assignment(entry: &str) -> (String, bool) {
    match entry.split_once('=') {
        Some((name, value)) => {
            let (v, m) = mask_value(name, value);
            (format!("{name}={v}"), m)
        }
        None => mask_url_passwords(entry),
    }
}

/// Command-line arguments with secret values masked: `--db-password=x`, `--token x`,
/// `PASSWORD=x` and URLs with a password.
pub fn mask_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for a in args {
        if hide_next {
            hide_next = false;
            out.push(MASK.to_owned());
            continue;
        }
        if let Some(opt) = a.strip_prefix('-') {
            let opt = opt.trim_start_matches('-');
            match opt.split_once('=') {
                Some((name, value)) => {
                    let (v, _) = mask_value(name, value);
                    let dashes = &a[..a.len() - opt.len()];
                    out.push(format!("{dashes}{name}={v}"));
                }
                None => {
                    hide_next = is_secret_name(opt);
                    out.push(a.clone());
                }
            }
            continue;
        }
        out.push(mask_assignment(a).0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_looking_names() {
        for n in [
            "DB_PASSWORD",
            "POSTGRES_PASSWORD",
            "dbPassword",
            "MYSQL_ROOT_PASSWD",
            "API_KEY",
            "apiKey",
            "GITHUB_TOKEN",
            "x-auth-token",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_ACCESS_KEY_ID",
            "CLIENT_SECRET",
            "SENTRY_DSN",
            "BASIC_AUTH",
            "DB_PWD",
            "SESSION_COOKIE",
            "JWT",
            "PRIVATE_KEY",
            "GPG_PASSPHRASE",
            "Authorization",
        ] {
            assert!(is_secret_name(n), "{n}");
        }
        for n in [
            "PATH",
            "HOME",
            "GREETING",
            "NGINX_VERSION",
            "OAUTH_CALLBACK_URL",
            "KEYBOARD",
            "PASSAGE",
            "com.docker.compose.project",
            "PRIMARY",
        ] {
            assert!(!is_secret_name(n), "{n}");
        }
    }

    #[test]
    fn values_entries_and_arguments() {
        assert_eq!(
            mask_assignment("DB_PASSWORD=hunter2"),
            (format!("DB_PASSWORD={MASK}"), true)
        );
        assert_eq!(
            mask_assignment("GREETING=hello"),
            ("GREETING=hello".to_owned(), false)
        );
        assert_eq!(
            mask_assignment("DB_PASSWORD="),
            ("DB_PASSWORD=".to_owned(), false),
            "an empty value has nothing to hide"
        );
        assert_eq!(
            mask_assignment("DATABASE_URL=postgres://app:s3cret@db:5432/shop?x=1"),
            (
                format!("DATABASE_URL=postgres://app:{MASK}@db:5432/shop?x=1"),
                true
            )
        );
        assert_eq!(
            mask_value("U", "see http://user@host/ and https://a:b@c"),
            (
                format!("see http://user@host/ and https://a:{MASK}@c"),
                true
            )
        );
        assert_eq!(mask_assignment("NOEQ"), ("NOEQ".to_owned(), false));
        let args: Vec<String> = [
            "server",
            "--db-password=x1",
            "--token",
            "abc",
            "-v",
            "--name=web",
            "API_KEY=k",
            "redis://:pw@cache:6379",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            mask_args(&args),
            [
                "server".to_owned(),
                format!("--db-password={MASK}"),
                "--token".into(),
                MASK.into(),
                "-v".into(),
                "--name=web".into(),
                format!("API_KEY={MASK}"),
                format!("redis://:{MASK}@cache:6379"),
            ]
        );
    }
}

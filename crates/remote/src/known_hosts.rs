//! The host keys the user already trusts (`SPEC-0323` v7 behaviour 49, `SPEC-0121`, IMP-0044):
//! reading OpenSSH's `known_hosts` files to compare a room link's fingerprint with them before
//! anything connects. Ferris IDEA only reads these files; it never writes them and never accepts a
//! key that is not in them.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// One host key `known_hosts` holds for a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownKey {
    /// The key type as written in the file (`ssh-ed25519`, `ecdsa-sha2-nistp256`, `ssh-rsa`, …).
    pub key_type: String,
    /// Its fingerprint in OpenSSH's form (`SHA256:` and unpadded Base64).
    pub fingerprint: String,
    /// The line carries the `@revoked` marker: this key must never be accepted.
    pub revoked: bool,
}

/// What the user's `known_hosts` says about a link's fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyCheck {
    /// A trusted key of the host has the fingerprint; the connection is narrowed to its type.
    Matches {
        /// Its key type.
        key_type: String,
    },
    /// Keys of the host are known but none has the fingerprint, or the fingerprint's key is
    /// revoked: refused as a possible attack.
    Different {
        /// The fingerprints the user trusts for the host.
        trusted: Vec<String>,
    },
    /// No key of the host is known; Ferris IDEA does not add one.
    Unknown,
}

impl HostKeyCheck {
    /// The name agents read (`matches`, `different`, `unknown`).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Matches { .. } => "matches",
            Self::Different { .. } => "different",
            Self::Unknown => "unknown",
        }
    }
}

/// The SHA-256 fingerprint of a public key blob, in OpenSSH's form.
pub fn fingerprint_of_blob(blob: &[u8]) -> String {
    format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(blob)))
}

/// `text` as a fingerprint in OpenSSH's form, or `None`: `SHA256:` and 43 characters of
/// unpadded Base64 that decode to 32 bytes.
pub fn parse_fingerprint(text: &str) -> Option<String> {
    let b64 = text.strip_prefix("SHA256:")?;
    if b64.len() != 43 {
        return None;
    }
    let bytes = STANDARD_NO_PAD.decode(b64).ok()?;
    (bytes.len() == 32).then(|| text.to_owned())
}

/// The name a host has in `known_hosts`: `host` for port 22, else `[host]:port`.
pub fn lookup_name(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Whether one host pattern of a line names `name` (plain or hashed `|1|salt|hash`). Wildcard
/// and negated patterns are not matched: a link names one host exactly.
fn pattern_names(pattern: &str, name: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix("|1|") {
        let Some((salt, hash)) = rest.split_once('|') else {
            return false;
        };
        let (Ok(salt), Ok(hash)) = (STANDARD.decode(salt), STANDARD.decode(hash)) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
            return false;
        };
        mac.update(name.as_bytes());
        return mac.verify_slice(&hash).is_ok();
    }
    if pattern.contains(['*', '?', '!']) {
        return false;
    }
    pattern.eq_ignore_ascii_case(name)
}

/// The keys `text` (a `known_hosts` file) holds for `name` (see [`lookup_name`]).
/// `@cert-authority` lines are skipped: a link names a host key, not an authority.
pub fn keys_for(text: &str, name: &str) -> Vec<KnownKey> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let mut first = fields.next().unwrap_or("");
        let mut revoked = false;
        if first.starts_with('@') {
            match first {
                "@revoked" => revoked = true,
                _ => continue,
            }
            first = fields.next().unwrap_or("");
        }
        let (Some(key_type), Some(blob)) = (fields.next(), fields.next()) else {
            continue;
        };
        if !first.split(',').any(|p| pattern_names(p, name)) {
            continue;
        }
        let Ok(blob) = STANDARD.decode(blob) else {
            continue;
        };
        out.push(KnownKey {
            key_type: key_type.to_owned(),
            fingerprint: fingerprint_of_blob(&blob),
            revoked,
        });
    }
    out
}

/// Compares the link's `fingerprint` with the keys `known` for the host (behaviour 49).
pub fn check(known: &[KnownKey], fingerprint: &str) -> HostKeyCheck {
    if known
        .iter()
        .any(|k| k.revoked && k.fingerprint == fingerprint)
    {
        return HostKeyCheck::Different {
            trusted: trusted(known),
        };
    }
    if let Some(k) = known
        .iter()
        .find(|k| !k.revoked && k.fingerprint == fingerprint)
    {
        return HostKeyCheck::Matches {
            key_type: k.key_type.clone(),
        };
    }
    if known.iter().any(|k| !k.revoked) {
        HostKeyCheck::Different {
            trusted: trusted(known),
        }
    } else {
        HostKeyCheck::Unknown
    }
}

fn trusted(known: &[KnownKey]) -> Vec<String> {
    let mut list: Vec<String> = known
        .iter()
        .filter(|k| !k.revoked)
        .map(|k| k.fingerprint.clone())
        .collect();
    list.dedup();
    list
}

/// The value of `HostKeyAlgorithms` that makes `ssh` verify exactly a key of `key_type`
/// (behaviour 50); RSA keys sign with SHA-2 only.
pub fn host_key_algorithms(key_type: &str) -> String {
    match key_type {
        "ssh-rsa" => "rsa-sha2-512,rsa-sha2-256".to_owned(),
        other => other.to_owned(),
    }
}

/// The preferred key of a host for a link (behaviour 44): Ed25519, then ECDSA, then RSA, then
/// any other; never a revoked one.
pub fn preferred(known: &[KnownKey]) -> Option<&KnownKey> {
    let rank = |t: &str| {
        if t == "ssh-ed25519" {
            0
        } else if t.starts_with("ecdsa-") {
            1
        } else if t == "ssh-rsa" {
            2
        } else {
            3
        }
    };
    known
        .iter()
        .filter(|k| !k.revoked)
        .min_by_key(|k| rank(&k.key_type))
}

/// The exact steps that add an unknown host key after checking it (behaviour 49), the way
/// `SPEC-0121` asks for an unknown key; Ferris IDEA never runs them itself.
pub fn keyscan_steps(host: &str, port: u16, fingerprint: &str) -> Vec<String> {
    vec![
        format!(
            "In a terminal: ssh -p {port} {host} — answer yes only if the fingerprint ssh shows is exactly {fingerprint}; otherwise answer no and tell the room's admin."
        ),
        format!(
            "To see the host's fingerprints first without logging in: ssh-keyscan -p {port} {host} | ssh-keygen -lf -"
        ),
        "Then Check Again.".to_owned(),
    ]
}

/// Where the host's keys are looked up: the name in `known_hosts` and the files `ssh` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostsLookup {
    /// The name the host has in the files (`HostKeyAlias`, else `HostName`, with the port).
    pub name: String,
    /// The files (user's first, then the system's).
    pub files: Vec<PathBuf>,
    /// The host `ssh` really connects to (`HostName`).
    pub hostname: String,
    /// Its port.
    pub port: u16,
}

/// Reads the lines of `ssh -G` into a lookup for `host` and `port`; `home` expands `~`.
pub fn lookup_from_config(config: &str, host: &str, port: u16, home: &Path) -> KnownHostsLookup {
    let mut hostname = host.to_owned();
    let mut real_port = port;
    let mut alias = None;
    let mut files = Vec::new();
    let mut global = Vec::new();
    for line in config.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim();
        match key {
            "hostname" => hostname = value.to_owned(),
            "port" => real_port = value.parse().unwrap_or(port),
            "hostkeyalias" if !value.is_empty() && value != "none" => {
                alias = Some(value.to_owned())
            }
            "userknownhostsfile" => files.extend(paths(value, home)),
            "globalknownhostsfile" => global.extend(paths(value, home)),
            _ => {}
        }
    }
    if files.is_empty() {
        files.push(home.join(".ssh/known_hosts"));
    }
    files.extend(global);
    let name = match alias {
        Some(a) => a,
        None => lookup_name(&hostname, real_port),
    };
    KnownHostsLookup {
        name,
        files,
        hostname,
        port: real_port,
    }
}

/// The paths of an `ssh -G` file list; `~` expanded, `%` tokens and `none` dropped.
fn paths(value: &str, home: &Path) -> Vec<PathBuf> {
    value
        .split_whitespace()
        .filter(|p| *p != "none" && !p.contains('%'))
        .map(|p| match p.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(p),
        })
        .collect()
}

/// What `ssh -G` makes of `host` and `port` (the configuration's when `None`) with `extra` (no
/// connection is made), as a lookup.
pub fn system_lookup(host: &str, port: Option<u16>, extra: &[String]) -> KnownHostsLookup {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let out = crate::SshCommand::new(crate::SshPurpose::Describe)
        .args(["-G"])
        .port(port)
        .extra(extra)
        .build(host)
        .ok()
        .and_then(|line| {
            line.command()
                .stdin(std::process::Stdio::null())
                .output()
                .ok()
        })
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut lookup = lookup_from_config(&out, host, port.unwrap_or(22), &home);
    if out.trim().is_empty() {
        // Without an answer of `ssh` nothing is read: no guessed file, so no key is "known".
        lookup.files.clear();
    }
    lookup
}

/// A lookup in the given `files` for `host` and `port`, without asking `ssh` (tests, which never
/// read `~/.ssh`).
pub fn files_lookup(host: &str, port: u16, files: &[PathBuf]) -> KnownHostsLookup {
    KnownHostsLookup {
        name: lookup_name(host, port),
        files: files.to_vec(),
        hostname: host.to_owned(),
        port,
    }
}

/// The keys the files of `lookup` hold for its host; missing files hold none.
pub fn known_keys(lookup: &KnownHostsLookup) -> Vec<KnownKey> {
    lookup
        .files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|text| keys_for(&text, &lookup.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // An Ed25519 public key blob (type string and 32 key bytes) made up for the tests.
    fn blob(seed: u8) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&11u32.to_be_bytes());
        b.extend_from_slice(b"ssh-ed25519");
        b.extend_from_slice(&32u32.to_be_bytes());
        b.extend(std::iter::repeat_n(seed, 32));
        b
    }

    fn line(hosts: &str, seed: u8) -> String {
        format!("{hosts} ssh-ed25519 {}", STANDARD.encode(blob(seed)))
    }

    #[test]
    fn fingerprints_have_the_openssh_form() {
        let fp = fingerprint_of_blob(&blob(1));
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(fp.len(), 7 + 43);
        assert_eq!(parse_fingerprint(&fp), Some(fp.clone()));
        // A known value: SHA-256 of the empty input.
        assert_eq!(
            fingerprint_of_blob(b""),
            "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU"
        );
        for bad in [
            "",
            "SHA256:",
            "MD5:aa:bb",
            "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
            "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuF",
            "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSu!U",
        ] {
            assert_eq!(parse_fingerprint(bad), None, "{bad}");
        }
    }

    #[test]
    fn keys_are_found_by_plain_bracketed_and_hashed_names() {
        // A hashed entry for "dev.example.com": salt and HMAC-SHA1 of the name.
        let salt = [7u8; 20];
        let mut mac = Hmac::<Sha1>::new_from_slice(&salt).unwrap();
        mac.update(b"dev.example.com");
        let hashed = format!(
            "|1|{}|{}",
            STANDARD.encode(salt),
            STANDARD.encode(mac.finalize().into_bytes())
        );
        let text = [
            "# comment".to_owned(),
            line("other.example.com,10.0.0.9", 1),
            line("[dev.example.com]:2222", 2),
            line(&hashed, 3),
            line("*.example.com", 4),
            format!("@cert-authority dev.example.com {}", line("", 5).trim()),
            format!("@revoked {}", line("dev.example.com", 6)),
            "dev.example.com ssh-ed25519 not-base64!".to_owned(),
        ]
        .join("\n");
        let on_22 = keys_for(&text, &lookup_name("dev.example.com", 22));
        assert_eq!(on_22.len(), 2, "{on_22:?}");
        assert_eq!(on_22[0].fingerprint, fingerprint_of_blob(&blob(3)));
        assert!(!on_22[0].revoked);
        assert!(on_22[1].revoked);
        let on_2222 = keys_for(&text, &lookup_name("dev.example.com", 2222));
        assert_eq!(on_2222.len(), 1);
        assert_eq!(on_2222[0].fingerprint, fingerprint_of_blob(&blob(2)));
        assert_eq!(keys_for(&text, "10.0.0.9").len(), 1);
        assert!(keys_for(&text, "nobody.example.com").is_empty());
    }

    #[test]
    fn the_check_matches_refuses_or_says_unknown() {
        let good = fingerprint_of_blob(&blob(1));
        let other = fingerprint_of_blob(&blob(2));
        let known = keys_for(&line("h", 1), "h");
        assert_eq!(
            check(&known, &good),
            HostKeyCheck::Matches {
                key_type: "ssh-ed25519".into()
            }
        );
        assert_eq!(
            check(&known, &other),
            HostKeyCheck::Different {
                trusted: vec![good.clone()]
            }
        );
        assert_eq!(check(&[], &good), HostKeyCheck::Unknown);
        let revoked = keys_for(&format!("@revoked {}", line("h", 2)), "h");
        assert_eq!(
            check(&revoked, &other),
            HostKeyCheck::Different { trusted: vec![] },
            "a revoked key is refused, never unknown"
        );
        assert_eq!(check(&known, &good).name(), "matches");
        assert_eq!(check(&known, &other).name(), "different");
        assert_eq!(check(&[], &good).name(), "unknown");
    }

    #[test]
    fn algorithms_preference_and_steps() {
        assert_eq!(host_key_algorithms("ssh-ed25519"), "ssh-ed25519");
        assert_eq!(host_key_algorithms("ssh-rsa"), "rsa-sha2-512,rsa-sha2-256");
        let k = |t: &str| KnownKey {
            key_type: t.into(),
            fingerprint: t.into(),
            revoked: false,
        };
        let keys = [k("ssh-rsa"), k("ecdsa-sha2-nistp256"), k("ssh-ed25519")];
        assert_eq!(preferred(&keys).unwrap().key_type, "ssh-ed25519");
        assert_eq!(
            preferred(&keys[..2]).unwrap().key_type,
            "ecdsa-sha2-nistp256"
        );
        let steps = keyscan_steps("dev.example.com", 2222, "SHA256:abc");
        assert!(steps[0].contains("ssh -p 2222 dev.example.com"));
        assert!(steps[0].contains("exactly SHA256:abc"));
        assert!(steps[1].contains("ssh-keyscan -p 2222 dev.example.com | ssh-keygen -lf -"));
    }

    #[test]
    fn the_lookup_follows_ssh_g() {
        let home = Path::new("/home/me");
        let config = "hostname 10.1.2.3\nport 2200\nuserknownhostsfile ~/.ssh/known_hosts /x/%h\nglobalknownhostsfile /etc/ssh/ssh_known_hosts\nhostkeyalias none\n";
        let l = lookup_from_config(config, "dev", 22, home);
        assert_eq!(l.name, "[10.1.2.3]:2200");
        assert_eq!(
            l.files,
            vec![
                PathBuf::from("/home/me/.ssh/known_hosts"),
                PathBuf::from("/etc/ssh/ssh_known_hosts")
            ]
        );
        let l = lookup_from_config("hostkeyalias box\n", "dev", 22, home);
        assert_eq!(l.name, "box");
        assert_eq!(l.files, vec![PathBuf::from("/home/me/.ssh/known_hosts")]);
        let l = lookup_from_config("", "dev", 22, home);
        assert_eq!(l.name, "dev");
    }
}

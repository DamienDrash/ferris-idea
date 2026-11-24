//! The Seatbelt profile of a policy for `sandbox-exec -p` on macOS (`SPEC-0347` behaviours 5–7).
//! Pure text, built and tested on every system.

use std::path::{Path, PathBuf};

use crate::policy::ValidPolicy;

/// The system locations every sandboxed process may read and execute below on macOS
/// (behaviour 5).
pub const SYSTEM_READ: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/System",
    "/Library/Apple",
    "/private/etc",
    "/private/var/select",
];

/// Devices that may be read; `/dev/null` may also be written.
pub const DEVICES: &[&str] = &["/dev/null", "/dev/zero", "/dev/random", "/dev/urandom"];

/// The only Mach services a sandboxed process may look up (behaviour 6).
pub const MACH_SERVICES: &[&str] = &[
    "com.apple.system.opendirectoryd.libinfo",
    "com.apple.system.notification_center",
    "com.apple.logd",
];

/// The profile. `proxy_port` is the allowlist proxy's port, `None` when the policy has no hosts
/// or the port is not chosen yet (then the profile says so in a comment and allows no network).
pub fn profile(policy: &ValidPolicy, proxy_port: Option<u16>) -> String {
    let mut p = String::from(
        "; Ferris IDEA agent sandbox (SPEC-0347)\n(version 1)\n(deny default)\n(allow process-fork)\n(allow signal (target same-sandbox))\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow file-read-data (literal \"/\"))\n",
    );
    let readable: Vec<&Path> = SYSTEM_READ
        .iter()
        .map(Path::new)
        .chain(policy.read().iter().map(PathBuf::as_path))
        .chain(policy.write().iter().map(PathBuf::as_path))
        .collect();
    p.push_str("(allow process-exec");
    for r in &readable {
        p.push_str(&format!(" (subpath {})", quoted(r)));
    }
    p.push_str(")\n(allow file-read*");
    for r in &readable {
        p.push_str(&format!(" (subpath {})", quoted(r)));
    }
    for d in DEVICES {
        p.push_str(&format!(" (literal {})", quoted(Path::new(d))));
    }
    p.push_str(")\n(allow file-write* (literal \"/dev/null\")");
    for w in policy.write() {
        p.push_str(&format!(" (subpath {})", quoted(w)));
    }
    p.push_str(")\n(allow file-ioctl");
    for d in DEVICES {
        p.push_str(&format!(" (literal {})", quoted(Path::new(d))));
    }
    p.push_str(")\n(allow mach-lookup");
    for s in MACH_SERVICES {
        p.push_str(&format!(" (global-name \"{s}\")"));
    }
    p.push_str(")\n");
    match (policy.hosts().is_empty(), proxy_port) {
        (true, _) => p.push_str("; network: none\n"),
        (false, Some(port)) => p.push_str(&format!(
            "; network: only the allowlist proxy\n(allow network-outbound (remote tcp \"localhost:{port}\"))\n"
        )),
        (false, None) => {
            p.push_str("; network: only the allowlist proxy, its port is chosen at launch\n")
        }
    }
    p
}

/// A path as an SBPL string literal. Validation keeps `"`, `\` and control characters out of
/// roots, so quoting is enough; the escapes are kept as a second line of defence.
fn quoted(path: &Path) -> String {
    let mut s = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            c if c.is_control() => {}
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Policy;

    /// SPEC-0347 AK-2.
    #[test]
    fn profile_denies_by_default_and_allows_only_the_policy() {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let (r, w) = (base.join("r"), base.join("w"));
        std::fs::create_dir_all(&r).unwrap();
        std::fs::create_dir_all(&w).unwrap();
        let none = Policy {
            read: vec![r.clone()],
            write: vec![w.clone()],
            hosts: vec![],
        }
        .validate(None)
        .unwrap();
        let text = profile(&none, Some(1234));
        assert!(text.contains("(version 1)\n(deny default)\n"), "{text}");
        let write_line = text
            .lines()
            .find(|l| l.starts_with("(allow file-write*"))
            .unwrap();
        assert_eq!(
            write_line,
            format!(
                "(allow file-write* (literal \"/dev/null\") (subpath \"{}\"))",
                w.display()
            )
        );
        assert_eq!(text.matches("(allow file-write*").count(), 1);
        assert!(text.contains(&format!("(subpath \"{}\")", r.display())));
        let mach = text
            .lines()
            .find(|l| l.starts_with("(allow mach-lookup"))
            .unwrap();
        assert_eq!(mach.matches("global-name").count(), 3);
        assert!(!text.contains("network-outbound"), "{text}");
        assert!(!text.contains("(allow network"), "{text}");

        let hosts = Policy {
            write: vec![w.clone()],
            hosts: vec!["api.example.com".into()],
            ..Policy::default()
        }
        .validate(None)
        .unwrap();
        let text = profile(&hosts, Some(4321));
        let net: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("(allow network"))
            .collect();
        assert_eq!(
            net,
            ["(allow network-outbound (remote tcp \"localhost:4321\"))"]
        );
        assert!(!profile(&hosts, None).contains("(allow network"));
        assert_eq!(quoted(Path::new("/a\"b\\c")), "\"/a\\\"b\\\\c\"");
    }
}

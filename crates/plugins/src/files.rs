//! Which project files a plugin may name (`SPEC-0156` v3 behaviours 7 to 11): only the project's
//! own sources, never a file whose name commonly holds secrets, and every refusal in a local log
//! without file contents. Since v4 the host asks the same question before it hands a file's text to
//! a plugin by itself (behaviours 12 and 13), and a file is read from the handle that was checked
//! (behaviour 14).

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use ferris_idea_core::{NotOwn, OwnSources};
use serde::{Deserialize, Serialize};

use crate::{MAX_TEXT, now_rfc3339};

/// File in the plugins folder with the log of refused file requests (behaviour 10).
pub const FILE_AUDIT: &str = "file-audit.jsonl";
/// Size at which the log is moved aside and started again.
pub const MAX_FILE_AUDIT: u64 = 1 << 20;
/// Most refusals of one call that are logged; one closing line follows.
pub const MAX_LOGGED: usize = 20;
/// Longest path a plugin may name, in bytes.
pub const MAX_PATH: usize = 4096;

/// Answer of a file host function: the plugin has no grant.
pub const FILE_NO_ACCESS: i64 = -1;
/// Answer: not a file of the project's own sources that a plugin may read, or no such file.
pub const FILE_NOT_READABLE: i64 = -2;
/// Answer: the file is larger than [`MAX_TEXT`].
pub const FILE_TOO_LARGE: i64 = -3;
/// Answer: the plugin's memory could not be written.
pub const FILE_NO_MEMORY: i64 = -4;
/// Answer: the name is on the list of files that commonly hold secrets.
pub const FILE_SECRET: i64 = -5;

/// What an answer of `read_file` means, for hosts and plugin authors (behaviour 9).
pub fn file_answer_meaning(code: i64) -> &'static str {
    match code {
        FILE_NO_ACCESS => "the user has not granted reading the project",
        FILE_NOT_READABLE => {
            "not a file of the project's own sources that a plugin may read, or no such file"
        }
        FILE_TOO_LARGE => "the file is larger than 4 MiB",
        FILE_NO_MEMORY => "the plugin's memory could not be written",
        FILE_SECRET => "the name is on the list of files that commonly hold secrets",
        _ => "unknown answer",
    }
}

/// Names of version control and IDE data, refused as any part of a path in any case (behaviour 7
/// check 2).
const DATA_NAMES: &[&str] = &[".git", ".hg", ".svn", ".jj", "cvs", ".ferris-idea", ".idea"];

/// One family of the deny list (behaviour 8); every name in lower case.
struct Family {
    name: &'static str,
    exact: &'static [&'static str],
    prefixes: &'static [&'static str],
    suffixes: &'static [&'static str],
    contains: &'static [&'static str],
    folders: &'static [&'static str],
}

const DENY: &[Family] = &[
    Family {
        name: "environment file",
        exact: &[".env", ".envrc"],
        prefixes: &[".env."],
        suffixes: &[".env"],
        contains: &[],
        folders: &[],
    },
    Family {
        name: "private key or key store",
        exact: &[],
        prefixes: &[],
        suffixes: &[
            ".pem",
            ".key",
            ".p8",
            ".p12",
            ".pfx",
            ".jks",
            ".keystore",
            ".ppk",
            ".kdbx",
        ],
        contains: &[],
        folders: &[],
    },
    Family {
        name: "SSH or GnuPG identity",
        exact: &[],
        prefixes: &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"],
        suffixes: &[],
        contains: &[],
        folders: &[".ssh", ".gnupg"],
    },
    Family {
        name: "credential file of a tool",
        exact: &[
            ".netrc",
            "_netrc",
            ".npmrc",
            ".yarnrc",
            ".yarnrc.yml",
            ".pypirc",
            ".git-credentials",
            ".pgpass",
            ".my.cnf",
            ".htpasswd",
            ".dockercfg",
            ".s3cfg",
            ".boto",
            ".vault-token",
            "auth.json",
            "kubeconfig",
            "http-client.private.env.json",
        ],
        prefixes: &[],
        suffixes: &[".kubeconfig"],
        contains: &[],
        folders: &[".aws", ".azure", ".docker", ".kube"],
    },
    Family {
        name: "infrastructure state or variables",
        exact: &[],
        prefixes: &[],
        suffixes: &[".tfvars", ".tfvars.json", ".tfstate"],
        contains: &[".tfstate."],
        folders: &[],
    },
    Family {
        name: "name that says it holds secrets",
        exact: &["secret"],
        prefixes: &[
            "credentials",
            ".credentials",
            "secrets",
            ".secrets",
            "secret.",
            "client_secret",
        ],
        suffixes: &[".secret", ".secrets"],
        contains: &[],
        folders: &["credentials", ".credentials", "secrets", ".secrets"],
    },
];

/// The family of the deny list that the path `rel` (relative, `/` between the parts) belongs to
/// by its names alone, in any case; the disk is not looked at (`SPEC-0156` v3 behaviour 8).
pub fn secret_family(rel: &str) -> Option<&'static str> {
    let lower = rel.to_lowercase();
    let parts: Vec<&str> = lower
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let file = parts.last()?;
    DENY.iter()
        .find(|f| {
            parts.iter().any(|p| f.folders.contains(p))
                || f.exact.contains(file)
                || f.prefixes.iter().any(|p| file.starts_with(p))
                || f.suffixes.iter().any(|s| file.ends_with(s))
                || f.contains.iter().any(|c| file.contains(c))
        })
        .map(|f| f.name)
}

/// Why a plugin does not get a file (`SPEC-0156` v3 behaviours 7 to 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The user has not granted reading the project.
    NoGrant,
    /// Not a relative path of plain names.
    Form,
    /// A part names data of version control or of an IDE.
    Data,
    /// On the deny list: the family.
    Secret(&'static str),
    /// Not one of the project's own sources.
    NotOwn(NotOwn),
    /// On disk under another spelling of the name (case, Unicode form).
    Spelling,
    /// Not a regular file.
    NotAFile,
    /// Passes every rule and does not exist: no refusal, and not logged.
    Missing,
    /// Larger than [`MAX_TEXT`].
    TooLarge,
    /// Another file than the one checked was at the path when it was opened (v4 behaviour 14).
    Changed,
}

impl Refusal {
    /// What the plugin is told: only [`FILE_SECRET`] and [`FILE_TOO_LARGE`] differ from "not
    /// readable or no such file", and neither tells whether a file the plugin may not read exists.
    pub fn code(self) -> i64 {
        match self {
            Self::NoGrant => FILE_NO_ACCESS,
            Self::Secret(_) => FILE_SECRET,
            Self::TooLarge => FILE_TOO_LARGE,
            _ => FILE_NOT_READABLE,
        }
    }

    /// The reason as the user's log names it; none for a file that is simply not there.
    pub fn reason(self) -> Option<String> {
        Some(match self {
            Self::NoGrant => "reading the project is not granted".into(),
            Self::Form => "not a relative path of plain names".into(),
            Self::Data => "version control or IDE data".into(),
            Self::Secret(family) => format!("{family} (deny list)"),
            Self::NotOwn(why) => why.reason().to_owned(),
            Self::Spelling => "another spelling of the file's name".into(),
            Self::NotAFile => "not a regular file".into(),
            Self::Missing => return None,
            Self::TooLarge => "larger than 4 MiB".into(),
            Self::Changed => "changed between the check and the read".into(),
        })
    }
}

/// Whether `rel` has the form of behaviour 7 check 1.
fn well_formed(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= MAX_PATH
        && !rel.chars().any(|c| c.is_control() || c == '\\' || c == ':')
        && !Path::new(rel).is_absolute()
        && rel.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' '])
        })
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// The checks of behaviour 7 that need no look at the disk (1 to 3).
fn by_name(rel: &str) -> Result<(), Refusal> {
    if !well_formed(rel) {
        return Err(Refusal::Form);
    }
    if rel
        .split('/')
        .any(|part| DATA_NAMES.contains(&part.to_lowercase().as_str()))
    {
        return Err(Refusal::Data);
    }
    match secret_family(rel) {
        Some(family) => Err(Refusal::Secret(family)),
        None => Ok(()),
    }
}

/// A file that passed checks 1 to 5 of behaviour 7, with what the disk said about it then.
#[derive(Debug)]
pub(crate) struct Checked {
    /// Its path below the canonical project folder.
    pub(crate) file: PathBuf,
    /// The path relative to the project folder, as asked and as it is on disk.
    relative: PathBuf,
    /// What was at the path when it was checked; a link is not followed.
    meta: std::fs::Metadata,
}

/// The file `rel` of the project `own` when a plugin may read it (`SPEC-0156` v3 behaviour 7
/// checks 1 to 5): its path below the canonical project folder.
pub(crate) fn own_file(own: &OwnSources, rel: &str) -> Result<Checked, Refusal> {
    by_name(rel)?;
    let relative = own.readable(Path::new(rel)).map_err(Refusal::NotOwn)?;
    if let Some(why) = own.not_own(&relative) {
        return Err(Refusal::NotOwn(why));
    }
    let file = own.root().join(rel);
    let Ok(meta) = std::fs::symlink_metadata(&file) else {
        return Err(Refusal::Missing);
    };
    if !meta.is_file() {
        return Err(Refusal::NotAFile);
    }
    // Only the exact name reaches a file: the deny list looked at the name as it was written.
    if relative != Path::new(rel) || file.canonicalize().ok().as_deref() != Some(file.as_path()) {
        return Err(Refusal::Spelling);
    }
    Ok(Checked {
        file,
        relative,
        meta,
    })
}

/// Opens a checked file for reading (`SPEC-0156` v4 behaviour 14): without following a link at its
/// name, and only when the handle is the file that was checked (same device and inode) and no
/// link stands on the way now that it is open. What is read afterwards comes from this handle.
fn open_checked(own: &OwnSources, checked: &Checked) -> Result<std::fs::File, Refusal> {
    let (handle, meta) = match ferris_idea_core::open_regular(&checked.file) {
        Ok(opened) => opened,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Missing),
        Err(_) => return Err(Refusal::Changed),
    };
    if !ferris_idea_core::same_file(&checked.meta, &meta)
        || own.readable(&checked.relative).as_deref() != Ok(checked.relative.as_path())
        || checked.file.canonicalize().ok().as_deref() != Some(checked.file.as_path())
    {
        return Err(Refusal::Changed);
    }
    Ok(handle)
}

/// The file `rel` of the project at `root` when a plugin may read it (`SPEC-0156` v3 behaviour 7
/// checks 1 to 5, and it can be opened as behaviour 14 says): its path below the canonical project
/// folder, or why not.
pub fn project_file(root: &Path, rel: &str) -> Result<PathBuf, Refusal> {
    by_name(rel)?;
    let own = OwnSources::new(root);
    let checked = own_file(&own, rel)?;
    open_checked(&own, &checked)?;
    Ok(checked.file)
}

/// The bytes of a checked file, read from the handle [`open_checked`] verified; the size is
/// checked on what is read.
pub(crate) fn read(own: &OwnSources, checked: &Checked) -> Result<Vec<u8>, Refusal> {
    let handle = open_checked(own, checked)?;
    let mut bytes = Vec::new();
    handle
        .take(MAX_TEXT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::NotAFile)?;
    if bytes.len() > MAX_TEXT {
        return Err(Refusal::TooLarge);
    }
    Ok(bytes)
}

/// Why the host does not hand the file at `path` (absolute, or relative to the project folder
/// `root`) to a plugin unless the user asks for exactly that (`SPEC-0156` v4 behaviour 12): what
/// `read_file` would refuse — a name on the deny list, data of version control or of an IDE,
/// anything that is not the project's own sources, a link, a file outside the project. `None` for
/// a file a plugin may read and for a new file that is not on disk yet. The name is looked at
/// before the disk.
pub fn guarded(root: &Path, path: &Path) -> Option<Refusal> {
    let relative = if path.is_absolute() {
        let below = path
            .strip_prefix(root)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| {
                let root = root.canonicalize().ok()?;
                let path = path.canonicalize().ok()?;
                path.strip_prefix(root).ok().map(Path::to_path_buf)
            });
        match below {
            Some(below) => below,
            None => return Some(Refusal::NotOwn(NotOwn::OutsideProject)),
        }
    } else {
        path.to_path_buf()
    };
    let Some(rel) = relative.to_str() else {
        return Some(Refusal::Form);
    };
    let rel = if std::path::MAIN_SEPARATOR == '/' {
        rel.to_owned()
    } else {
        rel.replace(std::path::MAIN_SEPARATOR, "/")
    };
    if let Err(refusal) = by_name(&rel) {
        return Some(refusal);
    }
    match own_file(&OwnSources::new(root), &rel) {
        Ok(_) | Err(Refusal::Missing) => None,
        Err(refusal) => Some(refusal),
    }
}

/// The question before a plugin action the user picked runs on a guarded file (`SPEC-0156` v4
/// behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardQuestion {
    /// The question; names the file and the plugin.
    pub message: String,
    /// Why Ferris IDEA asks, and what the plugin gets.
    pub detail: String,
    /// The answers; the first is the default and changes nothing.
    pub buttons: [&'static str; 2],
}

/// The question for action `action` of plugin `plugin` on the file `path` that is guarded for
/// `refusal`.
pub fn guard_question(plugin: &str, action: &str, path: &str, refusal: Refusal) -> GuardQuestion {
    let reason = refusal.reason().unwrap_or_default();
    GuardQuestion {
        message: format!("Give {path} to the plugin {plugin}?"),
        detail: format!(
            "\"{action}\" would get the text of this file. Ferris IDEA does not hand it to plugins by itself: {reason}. The plugin runs in its sandbox without network access; if you go on, this is noted in {FILE_AUDIT}."
        ),
        buttons: ["Cancel", "Run on This File"],
    }
}

/// What an agent is told when a plugin tool is asked for a guarded file without saying that this
/// is meant (`SPEC-0156` v4 behaviour 13).
pub fn guard_refusal(path: &str, refusal: Refusal) -> String {
    format!(
        "`{path}` is not handed to a plugin: {}. Files whose names commonly hold secrets and files that are not the project's own sources reach a plugin only on purpose. If the user asked for the plugin to run on exactly this file, repeat the call with `allow_secret_file: true`; it is then noted in {FILE_AUDIT}.",
        refusal.reason().unwrap_or_default()
    )
}

/// Notes in the log `log` that the guarded file `path` was handed to `plugin` by `function` all
/// the same, and on whose word (`by`: `confirmed by the user`, `an agent asked with
/// allow_secret_file`): `SPEC-0156` v4 behaviours 12 and 13. Never file contents.
pub fn note_passed_on(
    log: Option<&Path>,
    plugin: &str,
    function: &str,
    path: &str,
    refusal: Refusal,
    by: &str,
) {
    let (Some(log), Some(reason)) = (log, refusal.reason()) else {
        return;
    };
    let mut path = path.to_owned();
    if path.len() > MAX_PATH {
        let mut end = MAX_PATH;
        while !path.is_char_boundary(end) {
            end -= 1;
        }
        path.truncate(end);
    }
    append(
        log,
        &FileRefusal {
            time: now_rfc3339(),
            plugin: plugin.to_owned(),
            function: function.to_owned(),
            path,
            reason: format!("{reason}; handed over all the same: {by}"),
        },
    );
}

/// One line of the log of refused file requests (`SPEC-0156` v3 behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRefusal {
    /// RFC 3339 UTC.
    pub time: String,
    /// Plugin file stem.
    pub plugin: String,
    /// `read_file` or `open`; for a guarded file handed over on purpose (v4 behaviours 12 and 13)
    /// `action`, `run_plugin_action` or `plugin_inspections`.
    pub function: String,
    /// The path as the plugin gave it.
    pub path: String,
    /// Why it was refused, in words.
    pub reason: String,
}

/// The last `limit` refusals logged in `dir`, oldest first; only plugin `plugin`'s when given.
pub fn file_refusals(dir: &Path, plugin: Option<&str>, limit: usize) -> Vec<FileRefusal> {
    let text = std::fs::read_to_string(dir.join(FILE_AUDIT)).unwrap_or_default();
    let all: Vec<FileRefusal> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<FileRefusal>(l).ok())
        .filter(|r| plugin.is_none_or(|p| r.plugin == p))
        .collect();
    let skip = all.len().saturating_sub(limit);
    all.into_iter().skip(skip).collect()
}

fn append(file: &Path, record: &FileRefusal) {
    let Ok(line) = serde_json::to_string(record) else {
        return;
    };
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(file).is_ok_and(|m| m.len() >= MAX_FILE_AUDIT) {
        let mut old = file.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::rename(file, PathBuf::from(old));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// The refusals of one call on their way to the log: at most [`MAX_LOGGED`], then one closing
/// line.
#[derive(Debug, Default)]
pub(crate) struct Refusals {
    log: Option<PathBuf>,
    plugin: String,
    logged: usize,
}

impl Refusals {
    pub(crate) fn new(log: Option<PathBuf>, plugin: &str) -> Self {
        Self {
            log,
            plugin: plugin.to_owned(),
            logged: 0,
        }
    }

    /// Logs that `function` refused `path` (none: the bytes were no UTF-8) for `refusal`.
    pub(crate) fn note(&mut self, function: &str, path: Option<&str>, refusal: Refusal) {
        let (Some(log), Some(reason)) = (&self.log, refusal.reason()) else {
            return;
        };
        if self.logged > MAX_LOGGED {
            return;
        }
        self.logged += 1;
        let closing = self.logged > MAX_LOGGED;
        let mut path = path.unwrap_or("(not UTF-8)").to_owned();
        if path.len() > MAX_PATH {
            let mut end = MAX_PATH;
            while !path.is_char_boundary(end) {
                end -= 1;
            }
            path.truncate(end);
        }
        append(
            log,
            &FileRefusal {
                time: now_rfc3339(),
                plugin: self.plugin.clone(),
                function: function.to_owned(),
                path: if closing { String::new() } else { path },
                reason: if closing {
                    format!("further refusals of this call are not logged (more than {MAX_LOGGED})")
                } else {
                    reason
                },
            },
        );
    }
}

/// The project file a plugin asked to open (`SPEC-0156` v3 behaviour 11), below `root` as given:
/// only a file `read_file` would read and could open just now (v4 behaviour 14). A refusal is
/// logged for `plugin` in `log`, if any.
pub fn openable(root: &Path, rel: &str, log: Option<&Path>, plugin: &str) -> Option<PathBuf> {
    match project_file(root, rel) {
        Ok(_) => Some(root.join(rel)),
        Err(refusal) => {
            Refusals::new(log.map(Path::to_path_buf), plugin).note("open", Some(rel), refusal);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0156/AK-7.
    #[test]
    fn the_deny_list_goes_by_name() {
        for (path, family) in [
            (".env", "environment file"),
            ("apps/web/.env.production", "environment file"),
            (".ENV", "environment file"),
            ("deploy/prod.env", "environment file"),
            (".envrc", "environment file"),
            ("certs/server.pem", "private key or key store"),
            ("certs/SERVER.KEY", "private key or key store"),
            ("ios/AuthKey_ABC.p8", "private key or key store"),
            ("a.p12", "private key or key store"),
            ("a.pfx", "private key or key store"),
            ("android/release.jks", "private key or key store"),
            ("android/release.keystore", "private key or key store"),
            ("putty.ppk", "private key or key store"),
            ("vault.kdbx", "private key or key store"),
            ("id_rsa", "SSH or GnuPG identity"),
            ("keys/id_ed25519.pub", "SSH or GnuPG identity"),
            ("id_ecdsa_deploy", "SSH or GnuPG identity"),
            ("home/.ssh/config", "SSH or GnuPG identity"),
            (".gnupg/pubring.kbx", "SSH or GnuPG identity"),
            (".netrc", "credential file of a tool"),
            (".npmrc", "credential file of a tool"),
            // SPEC-0156 v5: the private values of the HTTP client.
            (
                "api/http-client.private.env.json",
                "credential file of a tool",
            ),
            ("packages/a/.NPMRC", "credential file of a tool"),
            (".yarnrc.yml", "credential file of a tool"),
            (".pypirc", "credential file of a tool"),
            (".git-credentials", "credential file of a tool"),
            (".pgpass", "credential file of a tool"),
            (".htpasswd", "credential file of a tool"),
            (".aws/config", "credential file of a tool"),
            ("ops/.kube/config", "credential file of a tool"),
            (".docker/config.json", "credential file of a tool"),
            ("staging.kubeconfig", "credential file of a tool"),
            ("auth.json", "credential file of a tool"),
            ("infra/prod.tfvars", "infrastructure state or variables"),
            (
                "infra/prod.tfvars.json",
                "infrastructure state or variables",
            ),
            (
                "infra/terraform.tfstate",
                "infrastructure state or variables",
            ),
            (
                "infra/terraform.tfstate.backup",
                "infrastructure state or variables",
            ),
            ("credentials", "name that says it holds secrets"),
            ("credentials.json", "name that says it holds secrets"),
            ("config/Secrets.yaml", "name that says it holds secrets"),
            ("src/secrets.rs", "name that says it holds secrets"),
            ("secret", "name that says it holds secrets"),
            ("secret.txt", "name that says it holds secrets"),
            ("client_secret_123.json", "name that says it holds secrets"),
            ("db.secret", "name that says it holds secrets"),
            ("secrets/readme.md", "name that says it holds secrets"),
            ("a/b/.secrets/x/y.txt", "name that says it holds secrets"),
            // The other separator is taken as one too.
            ("secrets\\readme.md", "name that says it holds secrets"),
        ] {
            assert_eq!(secret_family(path), Some(family), "{path}");
        }
        for path in [
            "src/main.rs",
            "src/environment.ts",
            "src/keys.rs",
            "README.md",
            "src/main/resources/application.properties",
            "docs/secretary.md",
            "src/env.rs",
            "src/envelope.env.rs",
            "src/keystore.rs",
            "src/id_rsa_test/mod.rs",
            "tfstate.md",
            "api/http-client.env.json",
        ] {
            assert_eq!(secret_family(path), None, "{path}");
        }
    }

    // SPEC-0156/AK-7: check 1 and check 2 of behaviour 7.
    #[test]
    fn the_form_of_a_path() {
        for path in ["README.md", "src/a b/c.rs", "ä/ö.txt", "a/.hidden"] {
            assert_eq!(by_name(path), Ok(()), "{path}");
        }
        for path in [
            "",
            "/etc/passwd",
            "../outside.txt",
            "a/../b",
            "./a",
            "a//b",
            "a/",
            "a\\b",
            "C:\\x",
            "a.txt::$DATA",
            "a.txt.",
            "a.txt ",
            "dir./a",
            "a\nb",
            "a\0",
        ] {
            assert_eq!(by_name(path), Err(Refusal::Form), "{path:?}");
        }
        let long = "a/".repeat(MAX_PATH / 2) + "b";
        assert_eq!(by_name(&long), Err(Refusal::Form));
        for path in [
            ".git/config",
            ".GIT/config",
            "sub/.git",
            ".ferris-idea/state.json",
            ".Idea/workspace.xml",
            "a/.hg/hgrc",
            "CVS/Root",
        ] {
            assert_eq!(by_name(path), Err(Refusal::Data), "{path}");
        }
        assert_eq!(Refusal::Data.code(), FILE_NOT_READABLE);
        assert_eq!(Refusal::Missing.code(), FILE_NOT_READABLE);
        assert_eq!(Refusal::Secret("x").code(), FILE_SECRET);
        assert_eq!(Refusal::Missing.reason(), None);
        assert!(file_answer_meaning(FILE_SECRET).contains("secrets"));
    }

    // SPEC-0156/AK-7: behaviour 10.
    #[test]
    fn the_log_starts_again_at_its_limit() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-file-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = dir.join(FILE_AUDIT);
        let record = |path: &str| FileRefusal {
            time: now_rfc3339(),
            plugin: "p".into(),
            function: "read_file".into(),
            path: path.into(),
            reason: "environment file (deny list)".into(),
        };
        append(&log, &record("one"));
        assert_eq!(file_refusals(&dir, Some("p"), 10).len(), 1);
        assert!(file_refusals(&dir, Some("other"), 10).is_empty());
        std::fs::write(&log, vec![b'x'; MAX_FILE_AUDIT as usize]).unwrap();
        append(&log, &record("two"));
        let now = file_refusals(&dir, None, 10);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].path, "two");
        assert_eq!(
            std::fs::metadata(dir.join(format!("{FILE_AUDIT}.1")))
                .unwrap()
                .len(),
            MAX_FILE_AUDIT
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0156/AK-12 (v4): the file that is read is the file that was checked.
    #[cfg(unix)]
    #[test]
    fn a_file_swapped_after_the_check_is_not_read() {
        use std::os::unix::fs::symlink;
        let base =
            std::env::temp_dir().join(format!("ferris-idea-file-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(base.join("elsewhere")).unwrap();
        std::fs::write(root.join("src/a.rs"), "OWN").unwrap();
        std::fs::write(base.join("secret.txt"), "SECRET").unwrap();
        std::fs::write(base.join("elsewhere/a.rs"), "SECRET").unwrap();
        let own = OwnSources::new(&root);
        let file = own.root().join("src/a.rs");

        // Unchanged: read from the handle.
        let checked = own_file(&own, "src/a.rs").unwrap();
        assert_eq!(read(&own, &checked), Ok(b"OWN".to_vec()));

        // A link in the file's place.
        let checked = own_file(&own, "src/a.rs").unwrap();
        std::fs::remove_file(&file).unwrap();
        symlink(base.join("secret.txt"), &file).unwrap();
        assert_eq!(read(&own, &checked), Err(Refusal::Changed));
        assert_eq!(project_file(&root, "src/a.rs").map(|_| ()), {
            Err(Refusal::NotOwn(NotOwn::SymbolicLink))
        });

        // Another regular file in its place.
        std::fs::remove_file(&file).unwrap();
        std::fs::write(&file, "OWN").unwrap();
        let checked = own_file(&own, "src/a.rs").unwrap();
        std::fs::rename(base.join("secret.txt"), &file).unwrap();
        assert_eq!(read(&own, &checked), Err(Refusal::Changed));

        // The folder above replaced by a link to a folder with a file of the same name.
        let checked = own_file(&own, "src/a.rs").unwrap();
        std::fs::rename(root.join("src"), base.join("src-moved")).unwrap();
        symlink(base.join("elsewhere"), root.join("src")).unwrap();
        assert_eq!(read(&own, &checked), Err(Refusal::Changed));

        // Gone: not there, as before the check.
        std::fs::remove_file(root.join("src")).unwrap();
        assert_eq!(read(&own, &checked), Err(Refusal::Missing));
        assert_eq!(Refusal::Changed.code(), FILE_NOT_READABLE);
        let _ = std::fs::remove_dir_all(&base);
    }

    // SPEC-0156/AK-9 (v4): the question before an action the user picked runs on a guarded file.
    #[test]
    fn the_question_names_the_file_and_the_plugin() {
        let q = guard_question(
            "Marker",
            "Mark Line",
            ".env",
            Refusal::Secret("environment file"),
        );
        assert_eq!(q.message, "Give .env to the plugin Marker?");
        assert!(q.detail.contains("\"Mark Line\""), "{}", q.detail);
        assert!(q.detail.contains("environment file (deny list)"));
        assert_eq!(q.buttons[0], "Cancel", "the default changes nothing");
        let told = guard_refusal("node_modules/x.js", Refusal::NotOwn(NotOwn::Dependencies));
        assert!(told.contains("`node_modules/x.js`") && told.contains("in installed dependencies"));
        assert!(told.contains("allow_secret_file: true"));
    }
}

//! Files on the host (behaviour 3): listing a folder, reading and writing a file with a check
//! that nobody changed it in between. Only POSIX tools run on the host (`sh`, `cat`, `cksum`,
//! `cp`, `mv`), so any Unix host works without installing anything.

use std::time::Duration;

use serde::Serialize;

use crate::ssh::{Remote, RemoteError, TIMEOUT};

/// Largest file opened from a host.
pub const MAX_FILE: u64 = 10 * 1024 * 1024;

/// What a folder entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A folder (or a link to one).
    Dir,
    /// A file (or a link to one).
    File,
}

/// One entry of a remote folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteEntry {
    /// Name inside the folder.
    pub name: String,
    /// Folder or file.
    pub kind: EntryKind,
    /// Whether it is a symbolic link.
    pub link: bool,
}

/// A file read from a host: its bytes and the `cksum` the host computes for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    /// Content.
    pub bytes: Vec<u8>,
    /// POSIX `cksum` as `"<crc> <size>"`, used to notice changes on the host.
    pub stamp: String,
}

/// POSIX `cksum` of `data` as `"<crc> <size>"` (CRC-32, polynomial 0x04C11DB7, length appended).
pub fn cksum(data: &[u8]) -> String {
    fn step(mut crc: u32, byte: u8) -> u32 {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
        crc
    }
    let mut crc = data.iter().fold(0u32, |c, b| step(c, *b));
    let mut len = data.len() as u64;
    while len > 0 {
        crc = step(crc, (len & 0xff) as u8);
        len >>= 8;
    }
    format!("{} {}", !crc, data.len())
}

/// The order of a folder's entries everywhere they are shown (`SPEC-0253` v3 behaviour 9):
/// folders first, then files; each group by name without regard to case (`alpha`, `beta`,
/// `Zeta`), names equal but for case in byte order.
pub fn entry_order(a: &RemoteEntry, b: &RemoteEntry) -> std::cmp::Ordering {
    a.kind
        .cmp(&b.kind)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

/// Sorts `entries` by [`entry_order`].
pub fn sort_entries(entries: &mut [RemoteEntry]) {
    entries.sort_by(entry_order);
}

const LIST: &str = r#"cd -- "$1" || exit 2
for f in * .*; do
  case "$f" in .|..) continue;; esac
  if [ -L "$f" ]; then l=1; else l=0; fi
  if [ -d "$f" ]; then t=d; elif [ -e "$f" ]; then t=f; elif [ "$l" = 1 ]; then t=f; else continue; fi
  printf '%s%s %s\0' "$t" "$l" "$f"
done"#;

const READ: &str = r#"[ -f "$1" ] || { echo "$1 is not a file" >&2; exit 2; }
s=$(wc -c < "$1") || exit 2
[ "$s" -le "$2" ] || { echo "$1 is too large to open ($s bytes)" >&2; exit 3; }
exec cat -- "$1""#;

const WRITE: &str = r#"f=$1
if [ -e "$f" ]; then have=$(cksum < "$f" | awk '{print $1" "$2}') || exit 4; else have=absent; fi
if [ "$2" != any ] && [ "$have" != "$2" ]; then echo "$have" >&2; exit 3; fi
t="$f.ferris-idea-$$"
if [ -e "$f" ]; then cp -p -- "$f" "$t" || exit 4; fi
if cat > "$t" && mv -f -- "$t" "$f"; then exit 0; fi
rm -f -- "$t"; exit 4"#;

const CREATE: &str = r#"if [ -e "$1" ] || [ -L "$1" ]; then echo "$1 already exists" >&2; exit 3; fi
if [ "$2" = dir ]; then mkdir -- "$1" || exit 4; else set -C; : > "$1" || exit 4; fi"#;

const RENAME: &str = r#"[ -e "$1" ] || [ -L "$1" ] || { echo "$1 does not exist" >&2; exit 2; }
if [ -e "$2" ] || [ -L "$2" ]; then echo "$2 already exists" >&2; exit 3; fi
d=$(dirname -- "$2")
[ -d "$d" ] || { echo "$d is not a folder" >&2; exit 2; }
mv -- "$1" "$2" || exit 4"#;

const COUNT: &str = r#"if [ -L "$1" ] || [ ! -d "$1" ]; then echo 0; exit 0; fi
n=$(find "$1" | wc -l) || exit 4
echo $((n - 1))"#;

const DELETE: &str = r#"p=$1
if [ -L "$p" ] || [ -f "$p" ]; then rm -f -- "$p" || exit 4; exit 0; fi
[ -e "$p" ] || { echo "$p does not exist" >&2; exit 2; }
[ -d "$p" ] || { rm -f -- "$p" || exit 4; exit 0; }
r=$(cd -- "$p" && pwd -P) || exit 4
h=$(cd && pwd -P)
case "$h/" in "$r"/*|"$r"/) echo "refusing to delete $r: it holds the home folder" >&2; exit 5;; esac
if [ "$r" = / ]; then echo "refusing to delete /" >&2; exit 5; fi
if [ "$2" = recursive ]; then rm -rf -- "$p" || exit 4
else rmdir -- "$p" 2>/dev/null || { echo "$p is not empty" >&2; exit 3; }; fi"#;

/// What [`Remote::create`] makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NewEntry {
    /// An empty file.
    File,
    /// A folder.
    Dir,
}

/// Whether deleting `path` would take `home` with it (`path` is `home` or a folder above it),
/// compared after removing `.`/`..` and trailing slashes.
pub fn protects_home(home: &str, path: &str) -> bool {
    fn norm(p: &str) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for part in p.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    }
    let (h, p) = (norm(home), norm(path));
    h.starts_with(&p)
}

/// Refuses paths that are not absolute or name `/`, `.` or `..` (SPEC-0253 behaviour 3).
pub fn check_path(path: &str) -> Result<(), RemoteError> {
    if !path.starts_with('/') {
        return Err(RemoteError::Failed(format!(
            "{path}: the path must be absolute"
        )));
    }
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if path.trim_end_matches('/').is_empty() || last == "." || last == ".." {
        return Err(RemoteError::Failed(format!("{path}: refused")));
    }
    Ok(())
}

fn failed(out: &crate::ssh::RemoteOutput) -> RemoteError {
    let msg = out.stderr.trim();
    RemoteError::Failed(if msg.is_empty() {
        format!("exit code {:?}", out.exit_code)
    } else {
        msg.to_owned()
    })
}

impl Remote {
    /// The user's home folder on the host.
    pub fn home(&self) -> Result<String, RemoteError> {
        let out = self.sh("cd && pwd -P", &[], None, TIMEOUT)?;
        if !out.success() {
            return Err(failed(&out));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// `path` with links and `..` resolved; refuses what is not a folder.
    pub fn resolve_dir(&self, path: &str) -> Result<String, RemoteError> {
        let out = self.sh(r#"cd -- "$1" && pwd -P"#, &[path], None, TIMEOUT)?;
        if !out.success() {
            return Err(RemoteError::Failed(format!(
                "{path} is not a folder on the host"
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// The entries of folder `path`: folders first, then files, each by name; `.` and `..`
    /// are left out, hidden entries are listed.
    pub fn list(&self, path: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        let out = self.sh(LIST, &[path], None, TIMEOUT)?;
        if out.exit_code == Some(2) {
            return Err(RemoteError::Failed(format!(
                "{path} is not a folder on the host"
            )));
        }
        if !out.success() {
            return Err(failed(&out));
        }
        let mut entries: Vec<RemoteEntry> = out
            .stdout
            .split(|b| *b == 0)
            .filter(|r| r.len() > 3)
            .map(|r| RemoteEntry {
                kind: if r[0] == b'd' {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                },
                link: r[1] == b'1',
                name: String::from_utf8_lossy(&r[3..]).into_owned(),
            })
            .collect();
        entries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
        Ok(entries)
    }

    /// Reads file `path` (at most [`MAX_FILE`] bytes).
    pub fn read(&self, path: &str) -> Result<RemoteFile, RemoteError> {
        let limit = MAX_FILE.to_string();
        let out = self.sh(READ, &[path, &limit], None, Duration::from_secs(120))?;
        if !out.success() {
            return Err(failed(&out));
        }
        let stamp = cksum(&out.stdout);
        Ok(RemoteFile {
            bytes: out.stdout,
            stamp,
        })
    }

    /// Writes `bytes` to `path` through a temporary file in the same folder that keeps the mode of
    /// the old file, then renames it into place. With `expected` (the stamp from [`read`], or
    /// `None` for a new file that must not exist yet), a file that changed meanwhile is left alone
    /// and [`RemoteError::Changed`] is returned. Returns the new stamp.
    ///
    /// [`read`]: Remote::read
    pub fn write(
        &self,
        path: &str,
        bytes: &[u8],
        expected: Option<&str>,
    ) -> Result<String, RemoteError> {
        let want = expected.unwrap_or("absent");
        self.write_checked(path, bytes, want)
    }

    /// Writes `bytes` to `path` whatever is there now (after the user confirmed).
    pub fn overwrite(&self, path: &str, bytes: &[u8]) -> Result<String, RemoteError> {
        self.write_checked(path, bytes, "any")
    }

    /// Creates an empty file or a folder at `path`; refused when something of that name exists
    /// (`SPEC-0253` behaviour 1).
    pub fn create(&self, path: &str, kind: NewEntry) -> Result<(), RemoteError> {
        check_path(path)?;
        let kind = match kind {
            NewEntry::File => "file",
            NewEntry::Dir => "dir",
        };
        let out = self.sh(CREATE, &[path, kind], None, TIMEOUT)?;
        if out.success() {
            Ok(())
        } else {
            Err(failed(&out))
        }
    }

    /// Renames or moves `from` to `to`; refused when `to` exists or its folder does not
    /// (behaviour 2).
    pub fn rename(&self, from: &str, to: &str) -> Result<(), RemoteError> {
        check_path(from)?;
        check_path(to)?;
        let out = self.sh(RENAME, &[from, to], None, TIMEOUT)?;
        if out.success() {
            Ok(())
        } else {
            Err(failed(&out))
        }
    }

    /// Number of entries below folder `path` (0 for a file or a link).
    pub fn count_entries(&self, path: &str) -> Result<u64, RemoteError> {
        let out = self.sh(COUNT, &[path], None, Duration::from_secs(60))?;
        if !out.success() {
            return Err(failed(&out));
        }
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .map_err(|_| RemoteError::Failed("cannot count the entries".into()))
    }

    /// Deletes a file, a link (never followed) or an empty folder; a non-empty folder only with
    /// `recursive`. `/`, the home folder and folders that hold it are refused (behaviour 3).
    pub fn delete(&self, path: &str, recursive: bool) -> Result<(), RemoteError> {
        check_path(path)?;
        // Checked here as well as on the host, where links are resolved too.
        let home = self.home()?;
        if protects_home(&home, path) {
            return Err(RemoteError::Failed(format!(
                "refusing to delete {path}: it holds the home folder"
            )));
        }
        let mode = if recursive { "recursive" } else { "single" };
        let out = self.sh(DELETE, &[path, mode], None, Duration::from_secs(120))?;
        if out.success() {
            Ok(())
        } else {
            Err(failed(&out))
        }
    }

    fn write_checked(&self, path: &str, bytes: &[u8], want: &str) -> Result<String, RemoteError> {
        let out = self.sh(WRITE, &[path, want], Some(bytes), Duration::from_secs(120))?;
        match out.exit_code {
            Some(0) => Ok(cksum(bytes)),
            Some(3) => Err(RemoteError::Changed {
                path: path.to_owned(),
                expected: want.to_owned(),
                // The write script prints the host's stamp as its last line on a mismatch.
                host: out
                    .stderr
                    .lines()
                    .map(str::trim)
                    .rfind(|l| !l.is_empty())
                    .unwrap_or("unknown")
                    .to_owned(),
            }),
            _ => Err(failed(&out)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_must_be_absolute_and_name_an_entry() {
        assert!(check_path("/srv/app/a.txt").is_ok());
        assert!(check_path("/srv/app/").is_ok());
        for bad in ["a.txt", "/", "//", "/srv/.", "/srv/..", ""] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
        assert!(protects_home("/home/me", "/home/me/"));
        assert!(protects_home("/home/me", "/home"));
        assert!(protects_home("/home/me", "/home/me/src/../"));
        assert!(!protects_home("/home/me", "/home/me/src"));
        assert!(!protects_home("/home/me", "/home/meow"));
    }

    /// SPEC-0253 v3 AK-7: folders first, then files, by name without regard to case.
    #[test]
    fn entries_sort_folders_first_without_regard_to_case() {
        let e = |name: &str, kind| RemoteEntry {
            name: name.into(),
            kind,
            link: false,
        };
        let mut entries = vec![
            e("Zeta.txt", EntryKind::File),
            e("beta", EntryKind::Dir),
            e("alpha.txt", EntryKind::File),
            e("Alpha", EntryKind::Dir),
            e("readonly", EntryKind::File),
            e("alpha", EntryKind::Dir),
            e("_x", EntryKind::File),
        ];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Alpha",
                "alpha",
                "beta",
                "_x",
                "alpha.txt",
                "readonly",
                "Zeta.txt"
            ]
        );
    }

    #[test]
    fn cksum_matches_the_posix_tool() {
        // Values of `printf … | cksum` on macOS and GNU coreutils.
        assert_eq!(cksum(b""), "4294967295 0");
        assert_eq!(cksum(b"hello\n"), "3015617425 6");
        let big = vec![b'x'; 70000];
        let tool = std::process::Command::new("cksum")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.take().unwrap().write_all(&big)?;
                c.wait_with_output()
            });
        if let Ok(out) = tool {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut parts = text.split_whitespace();
            let line = format!("{} {}", parts.next().unwrap(), parts.next().unwrap());
            assert_eq!(cksum(&big), line);
        }
    }
}

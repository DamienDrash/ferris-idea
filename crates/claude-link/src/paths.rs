//! Paths Claude Code names (`SPEC-0358` behaviour 20): absolute and inside the workspace after
//! links are resolved.

use std::path::{Component, Path, PathBuf};

/// The largest file text the link reads or takes (behaviour 9).
pub const MAX_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// A path or `file://` URI as Claude Code sends it, as a path.
pub fn from_wire(raw: &str) -> Result<PathBuf, String> {
    if raw.contains('\0') || raw.contains('\n') || raw.contains('\r') {
        return Err("the path contains a control character".into());
    }
    let text = match raw.strip_prefix("file://") {
        Some(rest) => percent_decode(rest)?,
        None => raw.to_owned(),
    };
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err("the path is not absolute".into());
    }
    Ok(path)
}

fn percent_decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or("the URI has a broken escape")?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).map_err(|_| "the URI is not UTF-8".to_owned())?;
    if decoded.contains('\0') {
        return Err("the path contains a control character".into());
    }
    Ok(decoded)
}

/// A `file://` URI of an absolute path.
pub fn to_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `path` if it lies inside `workspace` (canonical): no `..`, and the deepest existing folder on
/// the way resolves into the workspace, so a link cannot lead out. The part below it does not
/// exist yet and is kept as written.
pub fn inside(workspace: &Path, path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("the path is not absolute".into());
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("the path contains `.` or `..`".into());
    }
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(canonical) => {
                let mut full = canonical;
                for part in rest.iter().rev() {
                    full.push(part);
                }
                return if full.starts_with(workspace) {
                    Ok(full)
                } else {
                    Err("the path lies outside the project".into())
                };
            }
            Err(_) => {
                let Some(name) = existing.file_name().map(|n| n.to_owned()) else {
                    return Err("the path lies outside the project".into());
                };
                rest.push(name);
                if !existing.pop() {
                    return Err("the path lies outside the project".into());
                }
            }
        }
    }
}

/// Reads a text file of at most [`MAX_TEXT_BYTES`]; a missing file reads as `None`.
pub fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) if !m.is_file() => return Err("the path is not a file".into()),
        Ok(m) if m.len() > MAX_TEXT_BYTES as u64 => return Err("the file is too large".into()),
        Ok(_) => {}
    }
    std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|b| String::from_utf8(b).map_err(|_| "the file is not UTF-8 text".into()))
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_paths() {
        assert_eq!(
            from_wire("file:///tmp/a%20b.rs").unwrap(),
            PathBuf::from("/tmp/a b.rs")
        );
        assert_eq!(from_wire("/tmp/x").unwrap(), PathBuf::from("/tmp/x"));
        assert!(from_wire("relative/x").is_err());
        assert!(from_wire("/tmp/a\nb").is_err());
        assert!(from_wire("file:///tmp/%00").is_err());
        assert_eq!(to_uri(Path::new("/tmp/a b.rs")), "file:///tmp/a%20b.rs");
    }

    /// SPEC-0358 AK-9: paths outside the project are refused, also through a link.
    #[test]
    fn only_paths_inside_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        let ws = ws.canonicalize().unwrap();
        assert_eq!(
            inside(&ws, &ws.join("src/new/file.rs")).unwrap(),
            ws.join("src/new/file.rs")
        );
        assert!(inside(&ws, &ws.join("../other.rs")).is_err());
        assert!(inside(&ws, &dir.path().join("other.rs")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), ws.join("out")).unwrap();
            assert!(inside(&ws, &ws.join("out/escape.rs")).is_err());
        }
    }
}

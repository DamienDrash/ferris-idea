//! Paths stay in the project (`SPEC-0317` behaviour 18, `ADR-0025` T5, SEC-14): every path on the
//! wire is relative to the project folder; `..`, absolute paths and links that leave the folder
//! are refused in both directions.

use std::path::{Component, Path, PathBuf};

use crate::rpc::{ErrorCode, RpcError};

/// Checks the form of a path from the other side: relative, no `..`, no empty or `.` tricks that
/// leave the folder. Returns it in `/`-separated form.
pub fn check_relative(path: &str) -> Result<String, RpcError> {
    let refuse = |why: &str| {
        RpcError::new(ErrorCode::Invalid, format!("path {path:?} {why}"))
            .hint("paths are relative to the project folder and stay inside it")
    };
    if path.is_empty() || path.contains('\0') {
        return Err(refuse("is empty or has a NUL byte"));
    }
    let p = Path::new(path);
    if p.is_absolute() || path.starts_with('/') || path.starts_with('\\') {
        return Err(refuse("is absolute"));
    }
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => return Err(refuse("leaves the project folder (`..`)")),
            Component::RootDir | Component::Prefix(_) => return Err(refuse("is absolute")),
        }
    }
    if parts.is_empty() {
        return Ok(".".into());
    }
    Ok(parts.join("/"))
}

/// The file under `root` (canonical) for a relative `path` from the window; links leaving the
/// folder are refused. A path that does not exist yet is checked through its nearest existing
/// parent.
pub fn resolve(root: &Path, path: &str) -> Result<PathBuf, RpcError> {
    let rel = check_relative(path)?;
    let full = if rel == "." {
        root.to_path_buf()
    } else {
        root.join(&rel)
    };
    // The nearest existing ancestor, resolved through links, must stay inside the root.
    let mut probe = full.as_path();
    let mut rest = Vec::new();
    loop {
        match std::fs::canonicalize(probe) {
            Ok(real) => {
                if !real.starts_with(root) {
                    return Err(RpcError::new(
                        ErrorCode::Invalid,
                        format!("path {path:?} is a link that leaves the project folder"),
                    ));
                }
                let mut out = real;
                for r in rest.iter().rev() {
                    out.push(r);
                }
                return Ok(out);
            }
            Err(_) => {
                let Some(name) = probe.file_name() else {
                    return Err(RpcError::new(
                        ErrorCode::Invalid,
                        "path outside the project",
                    ));
                };
                rest.push(name.to_os_string());
                probe = match probe.parent() {
                    Some(p) => p,
                    None => return Err(RpcError::new(ErrorCode::Invalid, "path outside")),
                };
            }
        }
    }
}

/// The relative wire form of `full` under `root`.
pub fn relative(root: &Path, full: &Path) -> Option<String> {
    let rel = full.strip_prefix(root).ok()?;
    let s = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Some(if s.is_empty() { ".".into() } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_leaving_the_folder_are_refused() {
        assert_eq!(check_relative("src/./lib.rs").unwrap(), "src/lib.rs");
        assert_eq!(check_relative(".").unwrap(), ".");
        for bad in ["../x", "a/../../x", "a/..", "/etc/passwd", "", "a\0b"] {
            let e = check_relative(bad).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid, "{bad}");
        }
        let dir = std::env::temp_dir().join(format!("ferris-idea-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("p/src")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        let root = std::fs::canonicalize(dir.join("p")).unwrap();
        assert_eq!(
            resolve(&root, "src/new/file.rs").unwrap(),
            root.join("src/new/file.rs")
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("outside"), root.join("out")).unwrap();
            std::os::unix::fs::symlink(root.join("src"), root.join("in")).unwrap();
            assert!(resolve(&root, "out/secret").is_err());
            assert!(resolve(&root, "out").is_err());
            assert_eq!(resolve(&root, "in/a.rs").unwrap(), root.join("src/a.rs"));
        }
        assert_eq!(relative(&root, &root.join("src/a.rs")).unwrap(), "src/a.rs");
        assert_eq!(relative(&root, &root).unwrap(), ".");
        assert!(relative(&root, &dir).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}

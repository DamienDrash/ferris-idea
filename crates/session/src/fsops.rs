//! The `fs` family on the backend (`ADR-0026` §6): listing, reading and changing files of the
//! project, every path relative to its root and checked to stay inside it.

use std::path::Path;

use serde_json::{Value, json};

use crate::paths::{relative, resolve};
use crate::rpc::{ErrorCode, RpcError};

/// Files up to this size are sent inline in answers; larger ones go through a stream.
pub const INLINE_LIMIT: usize = 512 << 10;

fn io_error(path: &str, e: &std::io::Error) -> RpcError {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::Denied,
        std::io::ErrorKind::AlreadyExists => ErrorCode::Conflict,
        _ => ErrorCode::Internal,
    };
    RpcError::new(code, format!("{path}: {e}"))
}

fn kind(meta: &std::fs::Metadata) -> &'static str {
    if meta.file_type().is_symlink() {
        "link"
    } else if meta.is_dir() {
        "dir"
    } else {
        "file"
    }
}

fn mtime(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn str_param<'a>(params: &'a Value, name: &str) -> Result<&'a str, RpcError> {
    params[name]
        .as_str()
        .ok_or_else(|| RpcError::new(ErrorCode::Invalid, format!("`{name}` is missing")))
}

/// `fs.list {path}`: the entries of a folder, folders first, then by name.
pub fn list(root: &Path, params: &Value) -> Result<Value, RpcError> {
    let path = params["path"].as_str().unwrap_or(".");
    let dir = resolve(root, path)?;
    let mut entries = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| io_error(path, &e))? {
        let e = e.map_err(|e| io_error(path, &e))?;
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        let Some(rel) = relative(root, &e.path()) else {
            continue;
        };
        entries.push(json!({
            "name": e.file_name().to_string_lossy(),
            "path": rel,
            "kind": kind(&meta),
            "size": meta.len(),
        }));
    }
    entries.sort_by(|a, b| {
        (a["kind"] != "dir", a["name"].as_str()).cmp(&(b["kind"] != "dir", b["name"].as_str()))
    });
    Ok(json!({ "path": relative(root, &dir), "entries": entries }))
}

/// `fs.stat {path}`.
pub fn stat(root: &Path, params: &Value) -> Result<Value, RpcError> {
    let path = str_param(params, "path")?;
    let full = resolve(root, path)?;
    let meta = std::fs::symlink_metadata(&full).map_err(|e| io_error(path, &e))?;
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o7777;
    #[cfg(not(unix))]
    let mode = 0u32;
    Ok(json!({
        "path": relative(root, &full), "kind": kind(&meta), "size": meta.len(),
        "modified": mtime(&meta), "mode": format!("{mode:o}"),
    }))
}

/// `fs.read {path}`: the bytes of a file; text up to [`INLINE_LIMIT`] inline, else the caller
/// streams `bytes`.
pub fn read(root: &Path, params: &Value) -> Result<(Value, Option<Vec<u8>>), RpcError> {
    let path = str_param(params, "path")?;
    let full = resolve(root, path)?;
    let bytes = std::fs::read(&full).map_err(|e| io_error(path, &e))?;
    let rel = relative(root, &full);
    if bytes.len() <= INLINE_LIMIT
        && let Ok(text) = std::str::from_utf8(&bytes)
    {
        return Ok((json!({ "path": rel, "text": text }), None));
    }
    Ok((json!({ "path": rel, "bytes": bytes.len() }), Some(bytes)))
}

/// `fs.write {path, text}`: writes through a temporary file renamed into place, keeping the mode.
pub fn write(root: &Path, params: &Value) -> Result<Value, RpcError> {
    let path = str_param(params, "path")?;
    let text = str_param(params, "text")?;
    let full = resolve(root, path)?;
    let mode = std::fs::metadata(&full).ok().map(|m| m.permissions());
    let mut name = full.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".ferris-idea-write-{}", std::process::id()));
    let tmp = full.with_file_name(name);
    let done = std::fs::write(&tmp, text)
        .and_then(|()| match mode {
            Some(m) => std::fs::set_permissions(&tmp, m),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&tmp, &full));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(path, &e));
    }
    Ok(json!({ "path": relative(root, &full), "bytes": text.len() }))
}

/// `fs.create {path, kind}`: a new empty file or folder; an existing one is a `conflict`.
pub fn create(root: &Path, params: &Value) -> Result<Value, RpcError> {
    let path = str_param(params, "path")?;
    let full = resolve(root, path)?;
    if full.exists() {
        return Err(RpcError::new(ErrorCode::Conflict, format!("{path} exists")));
    }
    match params["kind"].as_str().unwrap_or("file") {
        "dir" => std::fs::create_dir_all(&full),
        _ => {
            if let Some(p) = full.parent() {
                std::fs::create_dir_all(p).map_err(|e| io_error(path, &e))?;
            }
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&full)
                .map(drop)
        }
    }
    .map_err(|e| io_error(path, &e))?;
    Ok(json!({ "path": relative(root, &full) }))
}

/// `fs.rename {from, to}`; an existing target is a `conflict`.
pub fn rename(root: &Path, params: &Value) -> Result<Value, RpcError> {
    let from = str_param(params, "from")?;
    let to = str_param(params, "to")?;
    let a = resolve(root, from)?;
    let b = resolve(root, to)?;
    if a == root {
        return Err(RpcError::new(
            ErrorCode::Invalid,
            "the project folder itself",
        ));
    }
    if b.exists() {
        return Err(RpcError::new(ErrorCode::Conflict, format!("{to} exists")));
    }
    std::fs::rename(&a, &b).map_err(|e| io_error(from, &e))?;
    Ok(json!({ "from": relative(root, &a), "to": relative(root, &b) }))
}

/// `fs.delete {path, recursive}`; a folder needs `recursive: true`.
pub fn delete(root: &Path, params: &Value) -> Result<Value, RpcError> {
    let path = str_param(params, "path")?;
    let full = resolve(root, path)?;
    if full == root {
        return Err(RpcError::new(
            ErrorCode::Invalid,
            "the project folder itself",
        ));
    }
    let meta = std::fs::symlink_metadata(&full).map_err(|e| io_error(path, &e))?;
    if meta.is_dir() {
        if params["recursive"].as_bool() != Some(true) {
            return Err(RpcError::new(
                ErrorCode::Invalid,
                format!("{path} is a folder; pass recursive: true"),
            ));
        }
        std::fs::remove_dir_all(&full)
    } else {
        std::fs::remove_file(&full)
    }
    .map_err(|e| io_error(path, &e))?;
    Ok(json!({ "path": path }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_operations_stay_in_the_project() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-fsops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("p/src")).unwrap();
        let root = std::fs::canonicalize(dir.join("p")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn a() {}\n").unwrap();
        let listed = list(&root, &json!({})).unwrap();
        assert_eq!(listed["entries"][0]["path"], "src");
        assert_eq!(listed["entries"][0]["kind"], "dir");
        let (read_back, stream) = read(&root, &json!({"path": "src/a.rs"})).unwrap();
        assert_eq!(read_back["text"], "fn a() {}\n");
        assert!(stream.is_none());
        create(&root, &json!({"path": "src/b.rs"})).unwrap();
        assert_eq!(
            create(&root, &json!({"path": "src/b.rs"}))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        write(&root, &json!({"path": "src/b.rs", "text": "b"})).unwrap();
        assert_eq!(
            stat(&root, &json!({"path": "src/b.rs"})).unwrap()["size"],
            1
        );
        rename(&root, &json!({"from": "src/b.rs", "to": "src/c.rs"})).unwrap();
        assert!(root.join("src/c.rs").exists());
        assert_eq!(
            delete(&root, &json!({"path": "src"})).unwrap_err().code,
            ErrorCode::Invalid
        );
        delete(&root, &json!({"path": "src/c.rs"})).unwrap();
        // Big or binary files are streamed.
        std::fs::write(root.join("big.bin"), vec![0u8; INLINE_LIMIT + 1]).unwrap();
        let (big, bytes) = read(&root, &json!({"path": "big.bin"})).unwrap();
        assert_eq!(big["bytes"], INLINE_LIMIT + 1);
        assert_eq!(bytes.unwrap().len(), INLINE_LIMIT + 1);
        // Paths that leave the project are refused before anything happens.
        for bad in [
            json!({"path": "../x", "text": ""}),
            json!({"path": "/tmp/x", "text": ""}),
        ] {
            assert_eq!(write(&root, &bad).unwrap_err().code, ErrorCode::Invalid);
            assert_eq!(create(&root, &bad).unwrap_err().code, ErrorCode::Invalid);
        }
        assert_eq!(
            rename(&root, &json!({"from": "big.bin", "to": "../big.bin"}))
                .unwrap_err()
                .code,
            ErrorCode::Invalid
        );
        assert!(!dir.join("big.bin").exists());
        assert_eq!(
            delete(&root, &json!({"path": "."})).unwrap_err().code,
            ErrorCode::Invalid
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

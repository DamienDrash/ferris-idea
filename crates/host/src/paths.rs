//! Paths and URIs between the window and the host (`SPEC-0318` behaviour 3, `ADR-0025` T5,
//! SEC-14): the window names files by their path relative to the project folder (or a `file:`
//! URI inside it); everything else is refused. Host paths in answers become relative paths, or a
//! display name when they lie outside the project, so no host path ever reaches the window.

use std::path::{Path, PathBuf};

use idecraft_session::{Backend, ErrorCode, RpcError};
use serde_json::{Value, json};

use crate::invalid;

/// Decodes `%XX` escapes of a URI path.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The relative path a request names, from `doc` (or `path`) or a `file:` `uri`, checked to stay
/// inside `root` (canonical). A URI of another scheme, with a host, or leaving the project is
/// `invalid`.
pub fn doc_param(root: &Path, params: &Value) -> Result<String, RpcError> {
    if let Some(uri) = params["uri"].as_str() {
        let rest = uri
            .strip_prefix("file://")
            .ok_or_else(|| invalid(format!("{uri:?} is not a file: URI of the project")))?;
        if !rest.starts_with('/') {
            return Err(invalid(format!(
                "{uri:?} names another machine; paths stay in the project"
            )));
        }
        let path = percent_decode(rest).ok_or_else(|| invalid("a malformed URI"))?;
        let rel = Path::new(&path)
            .strip_prefix(root)
            .map_err(|_| invalid(format!("{uri:?} is outside the project folder")))?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        let checked =
            idecraft_session::paths::check_relative(if rel.is_empty() { "." } else { &rel })?;
        crate::resolve(root, &checked)?;
        return Ok(checked);
    }
    let rel = params["doc"]
        .as_str()
        .or(params["path"].as_str())
        .ok_or_else(|| invalid("`doc` is missing"))?;
    let checked = idecraft_session::paths::check_relative(rel)?;
    crate::resolve(root, &checked)?;
    Ok(checked)
}

/// The wire form of a host path: `{"doc": rel}` inside the project, else `{"outside": name}`
/// with the file name only.
pub fn wire(root: &Path, path: &Path) -> Value {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match idecraft_session::paths::relative(root, &real) {
        Some(rel) if rel != "." => json!({ "doc": rel }),
        _ => json!({ "outside": outside_name(path) }),
    }
}

/// The relative path of `path`, if it lies in the project.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    idecraft_session::paths::relative(root, &real).filter(|r| r != ".")
}

/// What the window sees of a place outside the project: its file name and the folder above it
/// (`Vec.java (java.util)` style names come from the servers themselves).
pub fn outside_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{name} (outside the project)")
}

/// A document of the request as the language functions need it.
pub struct DocCtx {
    /// Project root (canonical).
    pub root: PathBuf,
    /// Relative path.
    pub doc: String,
    /// The host file.
    pub file: PathBuf,
    /// The backend's text when the document is open (unsaved edits included).
    pub text: Option<String>,
    /// Its revision when open.
    pub revision: Option<u64>,
}

/// The document a request names; with `need_revision`, the request's `revision` must be the
/// document's current one (`stale_revision` otherwise, behaviour 4).
pub fn doc_ctx(backend: &Backend, params: &Value, need_revision: bool) -> Result<DocCtx, RpcError> {
    let root = backend
        .root()
        .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no workspace"))?;
    let doc = doc_param(&root, params)?;
    let file = crate::resolve(&root, &doc)?;
    let open = backend.document(&doc);
    if need_revision {
        let Some(d) = &open else {
            return Err(
                RpcError::new(ErrorCode::NotFound, format!("{doc} is not open"))
                    .hint("open it with doc.open first"),
            );
        };
        if let Some(r) = params["revision"].as_u64()
            && r != d.revision
        {
            return Err(RpcError::new(
                ErrorCode::StaleRevision,
                format!("{doc} is at revision {}, not {r}", d.revision),
            )
            .hint("ask again on the current revision"));
        }
    }
    Ok(DocCtx {
        root,
        doc,
        file,
        revision: open.as_ref().map(|d| d.revision),
        text: open.map(|d| d.text),
    })
}

/// The character offset of 0-based `line` and `character` (Unicode scalar values) in `text`.
pub fn char_offset(text: &str, line: u64, character: u64) -> Option<usize> {
    let mut chars = 0usize;
    let mut current = 0u64;
    let mut rest = text;
    while current < line {
        let i = rest.find('\n')?;
        chars += rest[..=i].chars().count();
        rest = &rest[i + 1..];
        current += 1;
    }
    let line_text = rest.split('\n').next().unwrap_or("");
    let len = line_text.chars().count() as u64;
    if character > len {
        return None;
    }
    Some(chars + character as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_paths_leaving_the_project_are_refused() {
        let dir = std::env::temp_dir().join(format!("idecraft-host-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("p/src")).unwrap();
        std::fs::write(dir.join("p/src/a b.c"), "x").unwrap();
        let root = std::fs::canonicalize(dir.join("p")).unwrap();
        let uri = format!("file://{}/src/a%20b.c", root.display());
        assert_eq!(
            doc_param(&root, &json!({ "uri": uri })).unwrap(),
            "src/a b.c"
        );
        assert_eq!(
            doc_param(&root, &json!({ "doc": "src/./a b.c" })).unwrap(),
            "src/a b.c"
        );
        for bad in [
            json!({ "uri": "file:///etc/passwd" }),
            json!({ "uri": format!("file://{}/../x", root.display()) }),
            json!({ "uri": "file://other-host/etc/passwd" }),
            json!({ "uri": "https://example.org/a.c" }),
            json!({ "doc": "../outside.c" }),
            json!({ "doc": "/etc/passwd" }),
            json!({}),
        ] {
            let e = doc_param(&root, &bad).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid, "{bad}");
        }
        assert_eq!(
            wire(&root, &root.join("src/a b.c")),
            json!({ "doc": "src/a b.c" })
        );
        let out = wire(&root, Path::new("/usr/include/stdio.h"));
        assert_eq!(out, json!({ "outside": "stdio.h (outside the project)" }));
        assert!(!out.to_string().contains("/usr"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn positions_become_character_offsets() {
        let t = "ab\nçdé\n\nx";
        assert_eq!(char_offset(t, 0, 0), Some(0));
        assert_eq!(char_offset(t, 1, 2), Some(5));
        assert_eq!(char_offset(t, 1, 3), Some(6));
        assert_eq!(char_offset(t, 2, 0), Some(7));
        assert_eq!(char_offset(t, 3, 1), Some(9));
        assert_eq!(char_offset(t, 1, 4), None);
        assert_eq!(char_offset(t, 9, 0), None);
    }
}

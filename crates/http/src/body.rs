//! The body as it is sent (`SPEC-0046` behaviours 2 and 11): bodies from files under the path rules,
//! form bodies written over several lines, CRLF line ends in multipart bodies.

use std::io::Read;
use std::path::{Component, Path};

use crate::env::PRIVATE_ENV_FILE;
use crate::file::ResolvedRequest;

/// Largest file a `< path` body line may name, in bytes.
pub const MAX_BODY_FILE: u64 = 16 * 1024 * 1024;

/// The path of a `< path` body line.
fn file_reference(line: &str) -> Option<&str> {
    let path = line.trim_start().strip_prefix("< ")?.trim();
    (!path.is_empty()).then_some(path)
}

/// The paths the body of `request` takes content from, as written.
pub fn body_files(request: &ResolvedRequest) -> Vec<String> {
    request
        .body
        .split('\n')
        .filter_map(file_reference)
        .map(str::to_owned)
        .collect()
}

/// Reads the body file `written` of a request file in `dir` of the project `root` (behaviour 11):
/// relative to `dir`, inside `root` after resolving links, a regular file of UTF-8 text of at most
/// [`MAX_BODY_FILE`] bytes, not the private environment file and nothing under `.git` or
/// `.idecraft`. The error names the path and the rule.
fn read_body_file(written: &str, dir: &Path, root: &Path) -> Result<String, String> {
    let path = Path::new(written);
    if written.starts_with('~') || path.is_absolute() || written.starts_with(['/', '\\']) {
        return Err(format!(
            "body file {written}: only paths relative to the request file are read"
        ));
    }
    let file =
        std::fs::canonicalize(dir.join(path)).map_err(|e| format!("body file {written}: {e}"))?;
    let root = std::fs::canonicalize(root).map_err(|e| format!("project folder: {e}"))?;
    let Ok(inside) = file.strip_prefix(&root) else {
        return Err(format!(
            "body file {written} is outside the project; only files inside it are sent"
        ));
    };
    let kept = inside
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n == ".git" || n == ".idecraft"))
        || inside.file_name().is_some_and(|n| n == PRIVATE_ENV_FILE);
    if kept {
        return Err(format!(
            "body file {written}: the private environment file and files under .git and .idecraft are never sent"
        ));
    }
    let opened = std::fs::File::open(&file).map_err(|e| format!("body file {written}: {e}"))?;
    let meta = opened
        .metadata()
        .map_err(|e| format!("body file {written}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("body file {written} is not a regular file"));
    }
    if meta.len() > MAX_BODY_FILE {
        return Err(format!(
            "body file {written} is larger than {} MiB",
            MAX_BODY_FILE / (1024 * 1024)
        ));
    }
    let mut bytes = Vec::new();
    opened
        .take(MAX_BODY_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("body file {written}: {e}"))?;
    if bytes.len() as u64 > MAX_BODY_FILE {
        return Err(format!(
            "body file {written} is larger than {} MiB",
            MAX_BODY_FILE / (1024 * 1024)
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        format!("body file {written} is not UTF-8 text; binary body files are not supported")
    })
}

/// Turns the body of `request` into what is sent, once, before sending: `< path` lines become the
/// content of those files (behaviour 11), a form body written over several lines is joined
/// (behaviour 2), a multipart body gets CRLF line ends. `dir` is the folder of the request file,
/// `root` the project folder. On an error nothing may be sent.
pub fn finish_body(request: &mut ResolvedRequest, dir: &Path, root: &Path) -> Result<(), String> {
    if request.body.is_empty() {
        return Ok(());
    }
    let content_type = request
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let multipart = content_type.starts_with("multipart/");
    let form = content_type.starts_with("application/x-www-form-urlencoded");
    let lines: Vec<&str> = request
        .body
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let has_files = lines.iter().any(|l| file_reference(l).is_some());
    if let [only] = lines.as_slice()
        && let Some(path) = file_reference(only)
    {
        request.body = read_body_file(path, dir, root)?;
        return Ok(());
    }
    if form && !has_files {
        request.body = lines.iter().map(|l| l.trim()).collect();
        return Ok(());
    }
    if !multipart && !has_files {
        return Ok(());
    }
    let eol = if multipart { "\r\n" } else { "\n" };
    let mut parts = Vec::with_capacity(lines.len());
    for line in &lines {
        match file_reference(line) {
            Some(path) => {
                let text = read_body_file(path, dir, root)?;
                let text = text
                    .strip_suffix("\r\n")
                    .or_else(|| text.strip_suffix('\n'))
                    .unwrap_or(&text)
                    .to_owned();
                parts.push(text);
            }
            None => parts.push((*line).to_owned()),
        }
    }
    let mut body = parts.join(eol);
    if multipart {
        body.push_str(eol);
    }
    request.body = body;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(content_type: &str, body: &str) -> ResolvedRequest {
        ResolvedRequest {
            name: None,
            line: 1,
            method: "POST".into(),
            url: "http://127.0.0.1:1/".into(),
            headers: vec![("Content-Type".into(), content_type.into())],
            body: body.into(),
            unresolved: Vec::new(),
        }
    }

    fn project(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("idecraft-httpbody-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project/api/data")).unwrap();
        std::fs::create_dir_all(root.join("project/.git")).unwrap();
        std::fs::create_dir_all(root.join("project/.idecraft")).unwrap();
        root
    }

    #[test]
    fn body_files_stay_inside_the_project() {
        let base = project("files");
        let root = base.join("project");
        let dir = root.join("api");
        std::fs::write(dir.join("payload.json"), "{\"a\": 1}\n").unwrap();
        std::fs::write(dir.join("data/nested.txt"), "nested").unwrap();
        std::fs::write(root.join("top.txt"), "top").unwrap();
        std::fs::write(base.join("outside.txt"), "outside the project").unwrap();
        std::fs::write(dir.join(PRIVATE_ENV_FILE), "{\"dev\": {\"token\": \"t\"}}").unwrap();
        std::fs::write(root.join(".git/config"), "[remote]").unwrap();
        std::fs::write(root.join(".idecraft/agent-log.jsonl"), "{}").unwrap();
        std::fs::write(dir.join("binary.bin"), [0xff, 0xfe, 0x00]).unwrap();

        let send = |body: &str| {
            let mut r = request("application/json", body);
            finish_body(&mut r, &dir, &root).map(|()| r.body)
        };
        // A body that is only the line is the file exactly, last line break included.
        assert_eq!(send("< ./payload.json").unwrap(), "{\"a\": 1}\n");
        assert_eq!(send("< data/nested.txt").unwrap(), "nested");
        // `..` is fine while the file stays inside the project.
        assert_eq!(send("< ../top.txt").unwrap(), "top");
        // Inside a larger body the content replaces the line.
        assert_eq!(
            send("before\n< payload.json\nafter").unwrap(),
            "before\n{\"a\": 1}\nafter"
        );
        assert_eq!(
            body_files(&request(
                "text/plain",
                "a\n< payload.json\n<tag>\n<  x.txt "
            )),
            ["payload.json", "x.txt"]
        );

        let refused = |body: &str, part: &str| {
            let e = send(body).unwrap_err();
            assert!(e.contains(part), "{body}: {e}");
        };
        refused("< ../../outside.txt", "outside the project");
        refused("< ../../../../../../../../etc/hosts", "outside the project");
        refused("< /etc/hosts", "relative to the request file");
        refused("< ~/.ssh/id_ed25519", "relative to the request file");
        refused("< ./missing.txt", "missing.txt");
        refused("< .", "not a regular file");
        refused(&format!("< {PRIVATE_ENV_FILE}"), "never sent");
        refused("< ../.git/config", "never sent");
        refused("< ../.idecraft/agent-log.jsonl", "never sent");
        refused("< binary.bin", "not UTF-8");
        // One refused file refuses the whole body.
        refused("< payload.json\n< ../../outside.txt", "outside the project");

        // A link that points outside the project is followed before the check.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside.txt"), dir.join("link.txt")).unwrap();
            refused("< link.txt", "outside the project");
            std::os::unix::fs::symlink(dir.join("payload.json"), dir.join("inner.json")).unwrap();
            assert_eq!(send("< inner.json").unwrap(), "{\"a\": 1}\n");
        }

        let big = dir.join("big.txt");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_BODY_FILE + 1).unwrap();
        refused("< big.txt", "larger than 16 MiB");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn multipart_bodies_get_crlf() {
        let base = project("multipart");
        let root = base.join("project");
        let dir = root.join("api");
        std::fs::write(dir.join("note.txt"), "line one\nline two\n").unwrap();
        let mut r = request(
            "multipart/form-data; boundary=b",
            "--b\nContent-Disposition: form-data; name=\"f\"; filename=\"note.txt\"\n\n< note.txt\n--b--",
        );
        finish_body(&mut r, &dir, &root).unwrap();
        assert_eq!(
            r.body,
            "--b\r\nContent-Disposition: form-data; name=\"f\"; filename=\"note.txt\"\r\n\r\nline one\nline two\r\n--b--\r\n",
            "written lines end with CRLF, the file content is unchanged"
        );
        // A body that already has CRLF line ends is not doubled.
        let mut r = request("multipart/mixed; boundary=b", "--b\r\n\r\nx\r\n--b--");
        finish_body(&mut r, &dir, &root).unwrap();
        assert_eq!(r.body, "--b\r\n\r\nx\r\n--b--\r\n");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn form_bodies_are_joined() {
        let root = std::env::temp_dir();
        let mut r = request(
            "application/x-www-form-urlencoded; charset=utf-8",
            "name=Ada\n  &lang=en\n&note=a%26b",
        );
        finish_body(&mut r, &root, &root).unwrap();
        assert_eq!(r.body, "name=Ada&lang=en&note=a%26b");
        // Other bodies keep their lines.
        let mut r = request("text/plain", "a\n b\n");
        finish_body(&mut r, &root, &root).unwrap();
        assert_eq!(r.body, "a\n b\n");
        let mut r = request("application/json", "");
        finish_body(&mut r, &root, &root).unwrap();
        assert_eq!(r.body, "");
    }
}

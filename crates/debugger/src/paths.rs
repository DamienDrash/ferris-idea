//! Source paths on the wire of a debug adapter running on another machine (`SPEC-0322` behaviour
//! 14): the window names files below the project's stand-in folder, the adapter below the host
//! project folder.

use std::path::Path;

use serde_json::Value;

/// Rewrites `path` values between the window's stand-in folder and the host project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    local: String,
    host: String,
}

impl PathMap {
    /// A map between `local` (the stand-in folder) and `host` (the host project folder).
    pub fn new(local: &Path, host: &str) -> Self {
        Self {
            local: local.to_string_lossy().trim_end_matches('/').to_owned(),
            host: host.trim_end_matches('/').to_owned(),
        }
    }

    /// The host path of `path`, unchanged outside the stand-in folder.
    pub fn host_path(&self, path: &Path) -> String {
        let text = path.to_string_lossy();
        swap(&text, &self.local, &self.host).unwrap_or_else(|| text.into_owned())
    }

    /// Rewrites every `path` in `message` for the adapter.
    pub fn to_host(&self, message: &mut Value) {
        walk(message, &self.local, &self.host);
    }

    /// Rewrites every `path` in `message` for the window.
    pub fn to_local(&self, message: &mut Value) {
        walk(message, &self.host, &self.local);
    }
}

fn swap(text: &str, from: &str, to: &str) -> Option<String> {
    let rest = text.strip_prefix(from)?;
    (rest.is_empty() || rest.starts_with('/')).then(|| format!("{to}{rest}"))
}

fn walk(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key == "path"
                    && let Some(new) = v.as_str().and_then(|t| swap(t, from, to))
                {
                    *v = Value::String(new);
                } else {
                    walk(v, from, to);
                }
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|v| walk(v, from, to)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn paths_are_rewritten_both_ways_only_inside_the_folders() {
        let map = PathMap::new(Path::new("/cache/stand"), "/srv/app/");
        let mut out = json!({
            "source": { "path": "/cache/stand/src/a.py", "name": "a.py" },
            "breakpoints": [{ "line": 3 }],
            "other": "/cache/stand/src/a.py",
            "sibling": { "path": "/cache/standing/b.py" },
        });
        map.to_host(&mut out);
        assert_eq!(out["source"]["path"], "/srv/app/src/a.py");
        assert_eq!(out["other"], "/cache/stand/src/a.py", "only `path` keys");
        assert_eq!(out["sibling"]["path"], "/cache/standing/b.py");
        let mut back = json!({ "stackFrames": [
            { "source": { "path": "/srv/app/src/a.py" } },
            { "source": { "path": "/usr/lib/python3/os.py" } },
        ]});
        map.to_local(&mut back);
        assert_eq!(
            back["stackFrames"][0]["source"]["path"],
            "/cache/stand/src/a.py"
        );
        assert_eq!(
            back["stackFrames"][1]["source"]["path"], "/usr/lib/python3/os.py",
            "a frame outside the project keeps its host path"
        );
        assert_eq!(
            map.host_path(Path::new("/cache/stand/x")),
            "/srv/app/x".to_owned()
        );
    }
}

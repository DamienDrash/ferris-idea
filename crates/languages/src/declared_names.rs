//! The names a file declares, from its syntax tree, in any language with a grammar: what the
//! grammar's tags query names a definition, and the `name` of every node whose kind declares or
//! defines something (`function_definition`, `struct_item`, `type_spec`, …). Find Usages takes the
//! searched symbol's own declaration from here when the language server names none (`SPEC-0293`
//! v12 behaviour 29); completion ranks the names declared in the file and in the project before
//! the others the server ranks alike (`SPEC-0072` v21 behaviour 56).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use crate::LanguageId;
use crate::syntax::{MAX_SYNTAX_BYTES, SyntaxDocument};

/// A declared name and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Declared {
    /// The name as written, without PHP's `$`.
    pub name: String,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column (chars) of the name.
    pub character: usize,
}

/// Whether a node of this kind declares or defines the node of its `name` field.
fn declares(kind: &str) -> bool {
    kind.contains("declaration")
        || kind.contains("definition")
        || kind.ends_with("_item")
        || kind.ends_with("_spec")
        || kind.ends_with("_signature")
}

/// The names `text`, the content of `path`, declares, in document order, each place once; empty
/// for a file without a grammar or too large for a tree.
pub fn declarations(path: &Path, text: &str) -> Vec<Declared> {
    if text.len() > MAX_SYNTAX_BYTES {
        return Vec::new();
    }
    let Some(language) = LanguageId::detect(path) else {
        return Vec::new();
    };
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    let Ok(doc) = SyntaxDocument::new(language, tsx, text) else {
        return Vec::new();
    };
    let mut out: Vec<Declared> = doc
        .definitions()
        .into_iter()
        .map(|d| Declared {
            name: d.name,
            line: d.line,
            character: d.character,
        })
        .collect();
    let mut stack = vec![doc.tree().root_node()];
    while let Some(node) = stack.pop() {
        if declares(node.kind())
            && let Some(name) = node.child_by_field_name("name")
            && let Some(written) = text.get(name.byte_range())
        {
            let p = name.start_position();
            let line_start = name.start_byte() - p.column;
            let mut character = text[line_start..name.start_byte()].chars().count();
            let mut written = written;
            if let Some(rest) = written.strip_prefix('$') {
                written = rest;
                character += 1;
            }
            if !written.is_empty() && written.chars().all(crate::completion_filter::is_ident) {
                out.push(Declared {
                    name: written.to_owned(),
                    line: p.row,
                    character,
                });
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    out.sort_by_key(|d| (d.line, d.character));
    out.dedup_by(|a, b| a.line == b.line && a.character == b.character);
    out
}

/// The declaration of `symbol` whose name covers the 0-based `line` and `character` of `text`, the
/// content of `path`.
pub fn declaration_at(
    path: &Path,
    text: &str,
    symbol: &str,
    line: u64,
    character: u64,
) -> Option<Declared> {
    let length = symbol.chars().count();
    declarations(path, text).into_iter().find(|d| {
        d.name == symbol
            && d.line as u64 == line
            && (d.character..=d.character + length).contains(&(character as usize))
    })
}

/// The names `text`, the content of `path`, declares.
pub fn names_in(path: &Path, text: &str) -> HashSet<String> {
    declarations(path, text)
        .into_iter()
        .map(|d| d.name)
        .collect()
}

/// Most project files read for [`project_names`].
pub const MAX_PROJECT_FILES: usize = 2000;

/// Longest time [`project_names`] reads files; what it has read by then is the answer.
pub const PROJECT_BUDGET: Duration = Duration::from_millis(300);

/// The names a file declared when it had this modification time and size.
type Cached = (SystemTime, u64, Arc<HashSet<String>>);

fn cache() -> &'static Mutex<HashMap<PathBuf, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The names declared in the project's files with the extension of `file`, `file` itself left
/// out: at most [`MAX_PROJECT_FILES`] files, read for at most [`PROJECT_BUDGET`]; each file's
/// names are kept in memory by its modification time and size, so a second question reads only
/// the files changed since.
pub fn project_names(root: &Path, file: &Path) -> HashSet<String> {
    let started = Instant::now();
    let mut out = HashSet::new();
    let Ok(workspace) = ferris_idea_core::Workspace::open(root) else {
        return out;
    };
    let extension = file.extension().map(|e| e.to_os_string());
    if extension.is_none() {
        return out;
    }
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let own = canonical(file);
    let files = workspace.files().unwrap_or_default();
    for relative in files
        .into_iter()
        .filter(|r| r.extension().map(|e| e.to_os_string()) == extension)
        .take(MAX_PROJECT_FILES)
    {
        if started.elapsed() > PROJECT_BUDGET {
            break;
        }
        let path = canonical(&workspace.root().join(&relative));
        if path == own {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let (modified, size) = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        if size as usize > MAX_SYNTAX_BYTES {
            continue;
        }
        let known = cache()
            .lock()
            .ok()
            .and_then(|c| c.get(&path).cloned())
            .filter(|(m, s, _)| *m == modified && *s == size)
            .map(|(_, _, names)| names);
        let names = match known {
            Some(names) => names,
            None => {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let names = Arc::new(names_in(&path, &text));
                if let Ok(mut c) = cache().lock() {
                    c.insert(path.clone(), (modified, size, names.clone()));
                }
                names
            }
        };
        out.extend(names.iter().cloned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: &[Declared], name: &str) -> Vec<(usize, usize)> {
        d.iter()
            .filter(|d| d.name == name)
            .map(|d| (d.line, d.character))
            .collect()
    }

    /// SPEC-0293 v12 AK-18 and SPEC-0072 v21 AK-42: declarations from the tags query and from the
    /// `name` of declaring nodes, in Rust, Go and PHP (which has no tags query here).
    #[test]
    fn declarations_in_three_languages() {
        let rust = "#[derive(Debug)]\nstruct Rect {\n    w: f64,\n}\n\nimpl Rect {\n    fn area(&self) -> f64 { self.w }\n}\n";
        let d = declarations(Path::new("main.rs"), rust);
        assert_eq!(at(&d, "Rect"), [(1, 7)]);
        assert_eq!(at(&d, "area"), [(6, 7)]);
        assert_eq!(
            declaration_at(Path::new("main.rs"), rust, "Rect", 1, 9).map(|d| (d.line, d.character)),
            Some((1, 7))
        );
        // The use in `impl Rect` declares nothing.
        assert_eq!(
            declaration_at(Path::new("main.rs"), rust, "Rect", 5, 5),
            None
        );
        let go = "package main\n\n// Rect is a rectangle.\ntype Rect struct{ W, H float64 }\n\nfunc (r Rect) Area() float64 { return r.W * r.H }\n";
        let d = declarations(Path::new("main.go"), go);
        assert_eq!(at(&d, "Rect"), [(3, 5)]);
        assert_eq!(at(&d, "Area"), [(5, 14)]);
        let php = "<?php\n\nfunction greet(string $name): string\n{\n    return $name;\n}\n\nclass Greeter {\n    public function hello(): void {}\n}\n";
        let names = names_in(Path::new("index.php"), php);
        for name in ["greet", "Greeter", "hello"] {
            assert!(names.contains(name), "{name} in {names:?}");
        }
        assert!(!names.contains("string"));
        assert!(names_in(Path::new("notes.txt"), "greet").is_empty());
    }

    /// SPEC-0072 v21 AK-42: the project's names come from the other files of the same extension,
    /// read again when a file changes.
    #[test]
    fn project_names_from_the_other_files() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-declared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("index.php"), "<?php\nfunction here() {}\n").unwrap();
        std::fs::write(
            root.join("lib/util.php"),
            "<?php\nfunction greet_all() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("lib/other.go"), "package lib\nfunc Gone() {}\n").unwrap();
        let names = project_names(&root, &root.join("index.php"));
        assert!(names.contains("greet_all"), "{names:?}");
        assert!(!names.contains("here"), "the file itself is left out");
        assert!(!names.contains("Gone"), "other extensions are left out");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(
            root.join("lib/util.php"),
            "<?php\nfunction greet_some() {}\n",
        )
        .unwrap();
        let names = project_names(&root, &root.join("index.php"));
        assert!(
            names.contains("greet_some") && !names.contains("greet_all"),
            "{names:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

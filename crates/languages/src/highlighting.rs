//! The highlighting level of a file (`SPEC-0070` v6 behaviour 11): none, syntax, essential or
//! all, kept per project in `.idecraft/highlighting.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file the levels are kept in, below the project root.
pub const LEVELS_FILE: &str = ".idecraft/highlighting.json";

/// How much of a file's problems the editor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HighlightLevel {
    /// No problem is shown.
    None,
    /// Errors only.
    Syntax,
    /// Errors and warnings.
    Essential,
    /// Every problem.
    #[default]
    All,
}

/// How serious a problem is, as far as the level is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seriousness {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// A weak warning, information, or unused code.
    Weak,
}

impl HighlightLevel {
    /// Every level, lowest first, as the popup lists them.
    pub const ALL: [HighlightLevel; 4] = [
        HighlightLevel::None,
        HighlightLevel::Syntax,
        HighlightLevel::Essential,
        HighlightLevel::All,
    ];

    /// The level named `none`, `syntax`, `essential` or `all` (any case).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "syntax" => Self::Syntax,
            "essential" => Self::Essential,
            "all" => Self::All,
            _ => return None,
        })
    }

    /// The name used in files and tools: `none`, `syntax`, `essential`, `all`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Syntax => "syntax",
            Self::Essential => "essential",
            Self::All => "all",
        }
    }

    /// The name the popup shows: `None`, `Syntax`, `Essential`, `All`.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Syntax => "Syntax",
            Self::Essential => "Essential",
            Self::All => "All",
        }
    }

    /// What the level shows, in words (for tools).
    pub fn shows(self) -> &'static str {
        match self {
            Self::None => "no problems",
            Self::Syntax => "errors",
            Self::Essential => "errors and warnings",
            Self::All => "all problems",
        }
    }

    /// Whether a problem of `s` is shown at this level.
    pub fn shows_problem(self, s: Seriousness) -> bool {
        match self {
            Self::None => false,
            Self::Syntax => s == Seriousness::Error,
            Self::Essential => s != Seriousness::Weak,
            Self::All => true,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    files: BTreeMap<String, HighlightLevel>,
}

fn key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn read(root: &Path) -> Saved {
    std::fs::read(root.join(LEVELS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The level of `path` (absolute, or relative to `root`) in the project at `root`; All when it has
/// none or the file cannot be read.
pub fn level(root: &Path, path: &Path) -> HighlightLevel {
    read(root)
        .files
        .get(&key(root, path))
        .copied()
        .unwrap_or_default()
}

/// Sets the level of `path` in the project at `root`; All removes its entry, and the file is
/// removed when no entry is left. Written atomically.
pub fn set_level(root: &Path, path: &Path, level: HighlightLevel) -> std::io::Result<()> {
    let mut saved = read(root);
    let k = key(root, path);
    if level == HighlightLevel::All {
        saved.files.remove(&k);
    } else {
        saved.files.insert(k, level);
    }
    let file = root.join(LEVELS_FILE);
    if saved.files.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let dir = file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".highlighting.json.{}.tmp", std::process::id()));
    let json = serde_json::to_string_pretty(&saved).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json + "\n")?;
    std::fs::rename(&tmp, &file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0070/AK-6 (the stored levels)
    #[test]
    fn levels_are_kept_per_file() {
        let dir = std::env::temp_dir().join(format!("idecraft-hl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("src/A.java");
        assert_eq!(level(&dir, &file), HighlightLevel::All);
        set_level(&dir, &file, HighlightLevel::Syntax).unwrap();
        assert_eq!(level(&dir, &file), HighlightLevel::Syntax);
        assert_eq!(level(&dir, Path::new("src/A.java")), HighlightLevel::Syntax);
        assert_eq!(level(&dir, &dir.join("src/B.java")), HighlightLevel::All);
        let json = std::fs::read_to_string(dir.join(LEVELS_FILE)).unwrap();
        assert!(json.contains("\"src/A.java\": \"syntax\""), "{json}");
        set_level(&dir, &file, HighlightLevel::All).unwrap();
        assert!(!dir.join(LEVELS_FILE).exists(), "nothing left, no file");
        assert_eq!(
            HighlightLevel::parse("Essential"),
            Some(HighlightLevel::Essential)
        );
        assert_eq!(HighlightLevel::parse("loud"), None);
        assert!(!HighlightLevel::None.shows_problem(Seriousness::Error));
        assert!(HighlightLevel::Syntax.shows_problem(Seriousness::Error));
        assert!(!HighlightLevel::Syntax.shows_problem(Seriousness::Warning));
        assert!(HighlightLevel::Essential.shows_problem(Seriousness::Warning));
        assert!(!HighlightLevel::Essential.shows_problem(Seriousness::Weak));
        assert!(HighlightLevel::All.shows_problem(Seriousness::Weak));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

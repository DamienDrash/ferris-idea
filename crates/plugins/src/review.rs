//! Reviewing a plugin file before it is installed or updated (`SPEC-0211`).

use std::cmp::Ordering;
use std::path::Path;

use serde::Serialize;

use crate::{Manifest, Permission, Plugin, PluginError, contributions, granted, installed, label};

/// Compares two versions by their dot-separated parts (behaviour 3): numerically where both parts
/// are numbers, else as text; a missing part counts as `0`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<String> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split('.')
            .map(str::to_owned)
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).map_or("0", String::as_str);
        let y = b.get(i).map_or("0", String::as_str);
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// What installing a file would do (behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum ReviewKind {
    /// No plugin of this file name is installed.
    Install,
    /// A newer version replaces `from`.
    Update {
        /// The installed version.
        from: String,
    },
    /// The same version again.
    Reinstall,
    /// An older version replaces `from` (a warning).
    Downgrade {
        /// The installed version.
        from: String,
    },
}

/// A plugin file shown for review before installing (behaviour 2).
#[derive(Debug, Clone, Serialize)]
pub struct Review {
    /// File name without `.wasm`.
    pub file: String,
    /// The new manifest.
    pub manifest: Manifest,
    /// What installing does.
    #[serde(flatten)]
    pub kind: ReviewKind,
    /// What it contributes (`SPEC-0211` behaviour 1).
    pub contributions: Vec<String>,
    /// The permissions it asks for and whether they are granted now (behaviour 4: never by
    /// updating).
    pub permissions: Vec<(Permission, bool)>,
}

impl Review {
    /// One line for the Plugins page, e.g. `Update Upper 1.0 → 1.1`.
    pub fn headline(&self) -> String {
        let (name, version) = (&self.manifest.name, &self.manifest.version);
        match &self.kind {
            ReviewKind::Install => format!("Install {name} {version}"),
            ReviewKind::Update { from } => format!("Update {name} {from} → {version}"),
            ReviewKind::Reinstall => format!("Reinstall {name} {version}"),
            ReviewKind::Downgrade { from } => format!("Downgrade {name} {from} → {version}"),
        }
        .trim_end()
        .to_owned()
    }
}

/// Loads `source` and says what installing it into `dir` would do (behaviours 2–4).
pub fn review(dir: &Path, source: &Path) -> Result<Review, PluginError> {
    let name = label(source);
    let file = name
        .strip_suffix(".wasm")
        .ok_or_else(|| PluginError::Invalid(name.clone(), "the file must end in .wasm".into()))?
        .to_owned();
    let plugin = Plugin::load(source)?;
    let manifest = plugin.manifest;
    let old = installed(dir)
        .into_iter()
        .find(|i| i.file == file)
        .and_then(|i| i.manifest);
    let kind = match old {
        None => ReviewKind::Install,
        Some(old) => match compare_versions(&manifest.version, &old.version) {
            Ordering::Greater => ReviewKind::Update { from: old.version },
            Ordering::Equal => ReviewKind::Reinstall,
            Ordering::Less => ReviewKind::Downgrade { from: old.version },
        },
    };
    let permissions = manifest
        .permissions
        .iter()
        .map(|p| (*p, granted(dir, &file, *p)))
        .collect();
    Ok(Review {
        contributions: contributions(&manifest),
        file,
        manifest,
        kind,
        permissions,
    })
}

/// Keeps the grants of plugin `file` only for the permissions it still `asks` for (behaviour 4).
pub fn update_grants(dir: &Path, file: &str, asks: &[Permission]) -> Result<(), PluginError> {
    for p in Permission::ALL {
        if !asks.contains(&p) && granted(dir, file, p) {
            crate::set_granted(dir, file, p, false)?;
        }
    }
    Ok(())
}

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
    /// A different plugin (another manifest `name`) replaces the one installed under this file
    /// name (a warning, `SPEC-0211` v3 behaviour 8): nothing granted to the old one carries over.
    Replace {
        /// The installed plugin's name.
        from_name: String,
        /// The installed plugin's version.
        from: String,
    },
}

impl ReviewKind {
    /// Whether the new file is the plugin already installed under its file name (same manifest
    /// `name`), so what the user granted may carry over (`SPEC-0211` v3 behaviour 8).
    pub fn same_plugin(&self) -> bool {
        matches!(
            self,
            Self::Update { .. } | Self::Reinstall | Self::Downgrade { .. }
        )
    }
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
    /// updating; v3 behaviour 8: never to a new or a different plugin).
    pub permissions: Vec<(Permission, bool)>,
    /// The module's bytes as they were checked: installing writes exactly these (`SPEC-0211` v3
    /// behaviour 7), never the file again, which may have changed since.
    #[serde(skip)]
    pub(crate) bytes: Vec<u8>,
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
            ReviewKind::Replace { from_name, from } => {
                format!("Replace {from_name} {from} with {name} {version}")
            }
        }
        .trim_end()
        .to_owned()
    }
}

/// Loads `source` and says what installing it into `dir` would do (behaviours 2–4, v3 behaviours
/// 7 and 8). The file is read once; the review keeps the bytes it checked.
pub fn review(dir: &Path, source: &Path) -> Result<Review, PluginError> {
    let name = label(source);
    let file = name
        .strip_suffix(".wasm")
        .ok_or_else(|| PluginError::Invalid(name.clone(), "the file must end in .wasm".into()))?
        .to_owned();
    let bytes = crate::read_module(source)?;
    review_bytes(dir, source, file, bytes)
}

/// [`review`] of a module's `bytes` checked as plugin file `file` of `dir`.
fn review_bytes(
    dir: &Path,
    source: &Path,
    file: String,
    bytes: Vec<u8>,
) -> Result<Review, PluginError> {
    let manifest = Plugin::from_bytes(source, &bytes)?.manifest;
    let old = installed(dir)
        .into_iter()
        .find(|i| i.file == file)
        .and_then(|i| i.manifest);
    let kind = match old {
        None => ReviewKind::Install,
        // Another plugin under the same file name is no update of it (v3 behaviour 8).
        Some(old) if old.name != manifest.name => ReviewKind::Replace {
            from_name: old.name,
            from: old.version,
        },
        Some(old) => match compare_versions(&manifest.version, &old.version) {
            Ordering::Greater => ReviewKind::Update { from: old.version },
            Ordering::Equal => ReviewKind::Reinstall,
            Ordering::Less => ReviewKind::Downgrade { from: old.version },
        },
    };
    let carries = kind.same_plugin();
    let permissions = manifest
        .permissions
        .iter()
        .map(|p| (*p, carries && granted(dir, &file, *p)))
        .collect();
    Ok(Review {
        contributions: contributions(&manifest),
        file,
        manifest,
        kind,
        permissions,
        bytes,
    })
}

/// Installs exactly the module `review` checked into `dir` (`SPEC-0211` v3 behaviour 7). What
/// installing does is decided again against `dir` as it is now; grants carry over only to the
/// same plugin and only for what it still asks for (behaviour 4, v3 behaviour 8). The manifest.
pub fn install_reviewed(dir: &Path, review: &Review) -> Result<Manifest, PluginError> {
    let name = format!("{}.wasm", review.file);
    let now = review_bytes(
        dir,
        Path::new(&name),
        review.file.clone(),
        review.bytes.clone(),
    )?;
    let io = |e: std::io::Error| PluginError::Io(name.clone(), e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    if !now.kind.same_plugin() {
        // A new or a different plugin starts with nothing granted, also when a grant of a
        // plugin of that file name was left behind (removed by hand).
        for p in Permission::ALL {
            if granted(dir, &now.file, p) {
                crate::set_granted(dir, &now.file, p, false)?;
            }
        }
        crate::revoke_network(dir, &now.file, None, None)?;
    }
    let _ = std::fs::remove_file(dir.join(format!("{}{}", now.file, crate::DISABLED)));
    crate::write_new_module(dir, &name, &now.bytes)?;
    update_grants(dir, &now.file, &now.manifest.permissions)?;
    Ok(now.manifest)
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

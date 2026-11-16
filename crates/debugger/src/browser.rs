//! Chrome's profile folder for debugging pages (`SPEC-0133` v3 behaviour 2): one per project, in
//! IDECraft's user cache folder with mode 0700, never inside the project, and used by one browser
//! at a time.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Where an earlier version kept the profile, relative to the project.
pub const OLD_PROFILE: &str = ".idecraft/browser-profile";

/// The profile folder of a project's page sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProfile {
    /// The profile folder Chrome gets as `userDataDir`.
    pub folder: PathBuf,
    /// The project it belongs to (for a profile an earlier version kept there).
    pub project: PathBuf,
}

/// What happened to a profile an earlier version kept in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OldProfile {
    /// There is none.
    None,
    /// It was moved to the new folder (on the same disk, nothing copied or deleted).
    Moved,
    /// It stays in the project; the text says why.
    Kept(String),
}

/// `<folder name>-<16 hex digits of the SHA-256 of the project's full path>`: readable and
/// different for two projects of the same name.
pub fn profile_name(project: &Path) -> String {
    let name: String = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    let digest = Sha256::digest(project.as_os_str().as_encoded_bytes());
    let hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    if name.is_empty() || name.starts_with('.') {
        format!("project-{hash}")
    } else {
        format!("{name}-{hash}")
    }
}

impl BrowserProfile {
    /// The profile of the project at `project` below `cache` (`browser-profiles/<name>`).
    pub fn in_cache(cache: &Path, project: &Path) -> Self {
        let project = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
        Self {
            folder: cache.join("browser-profiles").join(profile_name(&project)),
            project,
        }
    }

    /// The profile of the project at `project` in IDECraft's user cache folder; `None` without a
    /// home folder.
    pub fn for_project(project: &Path) -> Option<Self> {
        Some(Self::in_cache(&idecraft_core::user_cache_dir()?, project))
    }

    /// Creates the folder (and `browser-profiles` above it) with mode 0700, moving a profile an
    /// earlier version kept in the project when it can; refuses a symbolic link or a folder of
    /// another user.
    pub fn prepare(&self) -> Result<OldProfile, String> {
        let parent = self
            .folder
            .parent()
            .ok_or_else(|| format!("{}: no parent folder", self.folder.display()))?;
        if let Some(cache) = parent.parent() {
            std::fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
        }
        private_folder(parent)?;
        let old = self.adopt_old();
        private_folder(&self.folder)?;
        Ok(old)
    }

    /// Moves `<project>/.idecraft/browser-profile` to the new folder when the new one does not
    /// exist yet, the old one is a real folder and no browser holds it; otherwise leaves it.
    fn adopt_old(&self) -> OldProfile {
        let old = self.project.join(OLD_PROFILE);
        let Ok(meta) = std::fs::symlink_metadata(&old) else {
            return OldProfile::None;
        };
        if !meta.is_dir() {
            return OldProfile::Kept("it is not a folder".into());
        }
        if std::fs::symlink_metadata(&self.folder).is_ok() {
            return OldProfile::Kept("the new profile folder exists already".into());
        }
        if held(&old) {
            return OldProfile::Kept("a browser may still be using it".into());
        }
        match std::fs::rename(&old, &self.folder) {
            Ok(()) => OldProfile::Moved,
            Err(e) => OldProfile::Kept(format!("it could not be moved: {e}")),
        }
    }

    /// The line for the debug console about an old profile; `None` when there is nothing to say.
    pub fn note(&self, old: &OldProfile) -> Option<String> {
        match old {
            OldProfile::None => None,
            OldProfile::Moved => Some(format!(
                "[IDECraft] The browser profile of earlier debug sessions was moved out of the project from {OLD_PROFILE} to {}\n",
                self.folder.display()
            )),
            OldProfile::Kept(why) => Some(format!(
                "[IDECraft] {OLD_PROFILE} still holds the browser profile of earlier debug sessions (cookies, logins, cache) and is not ignored by Git; IDECraft no longer uses it ({why}). Delete it or move it out of the project.\n"
            )),
        }
    }
}

/// Whether a browser may hold `profile`: Chrome's `SingletonLock` link names `<host>-<pid>`; a lock
/// whose process no longer runs is left from a browser that was killed. Anything unreadable counts
/// as held.
fn held(profile: &Path) -> bool {
    let lock = profile.join("SingletonLock");
    if std::fs::symlink_metadata(&lock).is_err() {
        return false;
    }
    let Ok(target) = std::fs::read_link(&lock) else {
        return true;
    };
    let target = target.to_string_lossy().into_owned();
    match target
        .rsplit('-')
        .next()
        .and_then(|p| p.parse::<u32>().ok())
    {
        Some(pid) => crate::lifetime::command_of(pid).is_some(),
        None => true,
    }
}

/// Creates `path` with mode 0700 or makes an existing folder of this user 0700; refuses links,
/// files and folders of other users.
fn private_folder(path: &Path) -> Result<(), String> {
    let shown = path.display();
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(format!("{shown} is a symbolic link; not used"))
        }
        Ok(meta) if !meta.is_dir() => Err(format!("{shown} is not a folder")),
        Ok(meta) => tighten(path, &meta),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(path) {
                Ok(()) => Ok(()),
                // Made at the same moment by another session: check what is there.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let meta =
                        std::fs::symlink_metadata(path).map_err(|e| format!("{shown}: {e}"))?;
                    if meta.file_type().is_symlink() || !meta.is_dir() {
                        return Err(format!("{shown} is not a folder"));
                    }
                    tighten(path, &meta)
                }
                Err(e) => Err(format!("{shown}: {e}")),
            }
        }
        Err(e) => Err(format!("{shown}: {e}")),
    }
}

#[cfg(unix)]
fn tighten(path: &Path, meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if meta.uid() != nix::unistd::geteuid().as_raw() {
        return Err(format!("{} belongs to another user", path.display()));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten(_: &Path, _: &std::fs::Metadata) -> Result<(), String> {
    Ok(())
}

static IN_USE: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
static RELEASED: Condvar = Condvar::new();

/// A profile folder in use by one page session of this process; released when dropped.
#[derive(Debug)]
pub struct ProfileLock {
    folder: PathBuf,
}

impl ProfileLock {
    /// Takes `folder`, waiting up to `timeout` for the session still using it (Rerun ends the old
    /// session while the new one starts; Chrome takes one profile only once). `None` after the
    /// timeout.
    pub fn acquire(folder: &Path, timeout: Duration) -> Option<Self> {
        let until = Instant::now() + timeout;
        let mut used = IN_USE.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let set = used.get_or_insert_with(HashSet::new);
            if !set.contains(folder) {
                set.insert(folder.to_path_buf());
                return Some(Self {
                    folder: folder.to_path_buf(),
                });
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            used = RELEASED
                .wait_timeout(used, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl Drop for ProfileLock {
    fn drop(&mut self) {
        let mut used = IN_USE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = used.as_mut() {
            set.remove(&self.folder);
        }
        RELEASED.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn one_folder_per_project_outside_it() {
        let a = BrowserProfile::in_cache(Path::new("/c"), Path::new("/w/one/web"));
        let b = BrowserProfile::in_cache(Path::new("/c"), Path::new("/w/two/web"));
        assert_ne!(a.folder, b.folder);
        assert!(a.folder.starts_with("/c/browser-profiles"));
        assert!(
            a.folder
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("web-")
        );
        assert_eq!(
            profile_name(Path::new("/x/my app")).len(),
            "my_app-".len() + 16
        );
        assert!(profile_name(Path::new("/")).starts_with("project-"));
    }

    #[cfg(unix)]
    #[test]
    fn folders_are_private_and_an_old_profile_moves_once() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("move");
        let project = dir.join("web");
        let old = project.join(OLD_PROFILE);
        std::fs::create_dir_all(old.join("Default")).unwrap();
        std::fs::write(old.join("Default/Cookies"), "c").unwrap();
        let profile = BrowserProfile::in_cache(&dir.join("cache"), &project);
        assert_eq!(profile.prepare().unwrap(), OldProfile::Moved);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(profile.folder.join("Default/Cookies")).unwrap(),
            "c"
        );
        for folder in [
            &profile.folder,
            &profile.folder.parent().unwrap().to_path_buf(),
        ] {
            let mode = std::fs::metadata(folder).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{folder:?}");
        }
        assert!(
            profile
                .note(&OldProfile::Moved)
                .unwrap()
                .contains("moved out")
        );
        // A second old profile is left alone and named, never deleted.
        std::fs::create_dir_all(&old).unwrap();
        let kept = profile.prepare().unwrap();
        assert!(matches!(kept, OldProfile::Kept(_)), "{kept:?}");
        assert!(old.exists());
        assert!(profile.note(&kept).unwrap().contains("not ignored by Git"));
        // A wider mode is tightened.
        std::fs::set_permissions(&profile.folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        profile.prepare().unwrap();
        let mode = std::fs::metadata(&profile.folder)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_folder_is_refused() {
        let dir = scratch("link");
        let project = dir.join("web");
        std::fs::create_dir_all(&project).unwrap();
        let profile = BrowserProfile::in_cache(&dir.join("cache"), &project);
        std::fs::create_dir_all(profile.folder.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&dir, &profile.folder).unwrap();
        let e = profile.prepare().unwrap_err();
        assert!(e.contains("symbolic link"), "{e}");
        // A browser still holding an old profile keeps it in place.
        std::fs::remove_file(&profile.folder).unwrap();
        let old = project.join(OLD_PROFILE);
        std::fs::create_dir_all(&old).unwrap();
        std::os::unix::fs::symlink("host-1", old.join("SingletonLock")).unwrap();
        assert_eq!(
            profile.prepare().unwrap(),
            OldProfile::Kept("a browser may still be using it".into())
        );
        // A lock left by a browser that no longer runs does not keep it.
        std::fs::remove_dir(&profile.folder).unwrap();
        let mut ended = std::process::Command::new("true").spawn().unwrap();
        let gone = ended.id();
        ended.wait().unwrap();
        std::fs::remove_file(old.join("SingletonLock")).unwrap();
        std::os::unix::fs::symlink(format!("host-{gone}"), old.join("SingletonLock")).unwrap();
        assert_eq!(profile.prepare().unwrap(), OldProfile::Moved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_profile_is_used_by_one_session_at_a_time() {
        let folder = std::env::temp_dir().join(format!("idecraft-lock-{}", std::process::id()));
        let first = ProfileLock::acquire(&folder, Duration::ZERO).unwrap();
        assert!(ProfileLock::acquire(&folder, Duration::from_millis(50)).is_none());
        let waiter = {
            let folder = folder.clone();
            std::thread::spawn(move || {
                ProfileLock::acquire(&folder, Duration::from_secs(5)).is_some()
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        drop(first);
        assert!(waiter.join().unwrap());
    }
}

//! The reattach socket of a stdio backend (`ADR-0025` §5, SEC-11): one Unix domain socket per
//! session in a folder of mode 0700 owned by the user; a peer with another user id is refused
//! before a byte is read; the first message must be `session.resume` with the token.

use std::path::{Path, PathBuf};

/// Why a private folder or a peer was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrivateError {
    /// The folder belongs to another user.
    #[error("{0} belongs to user {1}, not to this user ({2}); refusing to use it")]
    Owner(PathBuf, u32, u32),
    /// The folder could not be made private.
    #[error("{0}: {1}")]
    Io(PathBuf, String),
    /// The peer of a socket is another user.
    #[error("peer user {0} is not this user ({1}); refused before reading")]
    Peer(u32, u32),
}

/// Refuses a peer whose user id is not ours.
pub fn check_peer(peer_uid: u32, own_uid: u32) -> Result<(), PrivateError> {
    if peer_uid == own_uid {
        Ok(())
    } else {
        Err(PrivateError::Peer(peer_uid, own_uid))
    }
}

/// The user id of this process.
#[cfg(unix)]
pub fn own_uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

/// Creates `dir` (and parents) as needed and makes sure it is ours with mode 0700: a folder of
/// another user is refused, our own one with wider rights is narrowed.
#[cfg(unix)]
pub fn private_dir(dir: &Path) -> Result<PathBuf, PrivateError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let io = |e: std::io::Error| PrivateError::Io(dir.to_path_buf(), e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let meta = std::fs::symlink_metadata(dir).map_err(io)?;
    if !meta.is_dir() {
        return Err(PrivateError::Io(dir.to_path_buf(), "not a folder".into()));
    }
    if meta.uid() != own_uid() {
        return Err(PrivateError::Owner(
            dir.to_path_buf(),
            meta.uid(),
            own_uid(),
        ));
    }
    if meta.mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    Ok(dir.to_path_buf())
}

/// Creates `dir` as needed (no POSIX modes on this system).
#[cfg(not(unix))]
pub fn private_dir(dir: &Path) -> Result<PathBuf, PrivateError> {
    std::fs::create_dir_all(dir).map_err(|e| PrivateError::Io(dir.to_path_buf(), e.to_string()))?;
    Ok(dir.to_path_buf())
}

/// Appends one line to `path`.
#[cfg(not(unix))]
pub fn append_private(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")
}

/// Writes `bytes` to `path` with mode 0600 through a temporary file and a rename (`SPEC-0262`).
#[cfg(unix)]
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    let tmp = path.with_file_name(name);
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Appends one line to `path`, creating it with mode 0600.
#[cfg(unix)]
pub fn append_private(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")
}

/// The default folder of reattach sockets: `$XDG_RUNTIME_DIR/idecraft`, else
/// `~/.cache/idecraft/run`.
pub fn default_run_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join("idecraft"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/idecraft/run"))
}

/// The socket of session `id` in `dir`.
pub fn socket_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.sock"))
}

/// The peer user id of a connected socket.
#[cfg(unix)]
pub fn peer_uid(stream: &std::os::unix::net::UnixStream) -> std::io::Result<u32> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let cred = nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)
            .map_err(std::io::Error::from)?;
        Ok(cred.uid())
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let (uid, _) = nix::unistd::getpeereid(stream).map_err(std::io::Error::from)?;
        Ok(uid.as_raw())
    }
}

/// Connects standard input and output to the socket of session `id` (`idecraft serve --attach`);
/// returns when either side ends. Nothing but the bytes passes; the token travels inside.
#[cfg(unix)]
pub fn bridge_stdio(dir: &Path, id: &str) -> std::io::Result<()> {
    use std::io::{Read, Write};
    if !id.chars().all(|c| c.is_ascii_hexdigit()) || id.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a session id",
        ));
    }
    let stream = std::os::unix::net::UnixStream::connect(socket_path(dir, id))?;
    let mut from_socket = stream.try_clone()?;
    let mut to_socket = stream.try_clone()?;
    let out = std::thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        let mut buf = [0u8; 64 << 10];
        loop {
            match from_socket.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if stdout
                        .write_all(&buf[..n])
                        .and_then(|()| stdout.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 64 << 10];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if to_socket.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = out.join();
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn peers_of_other_users_and_open_folders_are_refused() {
        assert!(check_peer(501, 501).is_ok());
        let e = check_peer(0, 501).unwrap_err();
        assert!(e.to_string().contains("refused before reading"), "{e}");

        let dir = std::env::temp_dir().join(format!("idecraft-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        let file = dir.join("state.json");
        write_private(&file, b"{}").unwrap();
        append_private(&dir.join("audit.jsonl"), "{}").unwrap();
        for f in [&file, &dir.join("audit.jsonl")] {
            assert_eq!(std::fs::metadata(f).unwrap().mode() & 0o777, 0o600);
        }
        // A folder of another user (the root folder is root's) is refused.
        if own_uid() != 0 {
            assert!(matches!(
                private_dir(Path::new("/")),
                Err(PrivateError::Owner(..))
            ));
        }
        // A real socket reports our own user id as its peer.
        let sock = socket_path(&dir, "abc");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let client = std::os::unix::net::UnixStream::connect(&sock).unwrap();
        let (server, _) = listener.accept().unwrap();
        assert_eq!(peer_uid(&server).unwrap(), own_uid());
        assert_eq!(peer_uid(&client).unwrap(), own_uid());
        let _ = std::fs::remove_dir_all(dir);
    }
}

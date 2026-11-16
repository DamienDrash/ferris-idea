//! The small step before the backend runs (`SPEC-0321` behaviours 3–6): find the host's system
//! and processor, pick the backend build for it, bring it there, check it by SHA-256.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use sha2::{Digest, Sha256};

use crate::route::Route;
use crate::ssh::RemoteError;

/// Whether `folder` is an absolute path on some host: `/…`, or `C:\…` / `C:/…` on Windows.
pub fn absolute_folder(folder: &str) -> bool {
    let b = folder.as_bytes();
    folder.starts_with('/')
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// The family of a host's shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFamily {
    /// A POSIX shell.
    Posix,
    /// Windows, through PowerShell.
    Windows,
}

/// The host's system and processor as the bootstrap found them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPlatform {
    /// Family of the shell.
    pub family: HostFamily,
    /// `uname -sm` form, e.g. `Linux x86_64` or `Windows amd64`.
    pub platform: String,
}

/// The Rust target of a `uname -sm` answer, if IDECraft builds for it.
pub fn target_triple(platform: &str) -> Option<&'static str> {
    let norm = platform.trim().to_lowercase().replace("aarch64", "arm64");
    Some(match norm.as_str() {
        "linux x86_64" | "linux amd64" => "x86_64-unknown-linux-gnu",
        "linux arm64" => "aarch64-unknown-linux-gnu",
        "darwin x86_64" => "x86_64-apple-darwin",
        "darwin arm64" => "aarch64-apple-darwin",
        "windows amd64" | "windows x86_64" => "x86_64-pc-windows-gnu",
        "windows arm64" => "aarch64-pc-windows-gnullvm",
        _ => return None,
    })
}

/// The target this program was built for.
pub fn local_target() -> Option<&'static str> {
    target_triple(&crate::local_platform())
}

/// The folders searched for backend builds, in order.
fn backend_folders(local_binary: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = std::env::var_os("IDECRAFT_BACKENDS_DIR") {
        out.push(PathBuf::from(d));
    }
    if let Some(dir) = local_binary.parent() {
        out.push(dir.join("backends"));
        out.push(dir.join("../Resources/backends"));
    }
    out
}

/// The name of the backend program for a target.
pub fn backend_program(target: &str) -> &'static str {
    if target.contains("windows") {
        "idecraft-serve.exe"
    } else {
        "idecraft-serve"
    }
}

/// Picks the build for `host`: a dedicated build for its target, else the running program when
/// the platforms match. A platform without a build is refused and says what to do.
pub fn select_build(host: &HostPlatform, local_binary: &Path) -> Result<PathBuf, RemoteError> {
    let target = target_triple(&host.platform);
    if let Some(t) = target {
        for folder in backend_folders(local_binary) {
            let path = folder.join(t).join(backend_program(t));
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    if crate::same_platform(&host.platform, &crate::local_platform()) {
        return Ok(local_binary.to_path_buf());
    }
    Err(RemoteError::Failed(match target {
        Some(t) => format!(
            "no backend build for {} ({t}) in {}; run scripts/build-backends.sh to build it, or \
             install idecraft-serve on the host and start it with --pair",
            host.platform,
            backend_folders(local_binary)
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => format!(
            "IDECraft has no backend build for {}; install idecraft-serve on the host yourself \
             and start it with --pair",
            host.platform
        ),
    }))
}

/// SHA-256 of a local file as hex.
pub fn sha256_hex(path: &Path) -> Result<String, RemoteError> {
    let bytes =
        std::fs::read(path).map_err(|e| RemoteError::Failed(format!("{}: {e}", path.display())))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The PowerShell command line that runs `script` (Base64 as `-EncodedCommand` wants it).
pub fn powershell_line(script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64(&utf16)
    )
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(char::from(ALPHABET[(n >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

/// The PowerShell text that prints the processor (`AMD64`, `ARM64`).
pub const POWERSHELL_PROBE: &str = "Write-Output $env:PROCESSOR_ARCHITECTURE";

/// The platform of a Windows host from what [`POWERSHELL_PROBE`] printed.
pub fn windows_platform(answer: &str) -> Option<String> {
    match answer.trim().to_lowercase().as_str() {
        "amd64" => Some("Windows amd64".into()),
        "arm64" => Some("Windows arm64".into()),
        _ => None,
    }
}

/// Finds the host's system and processor (`SPEC-0321` behaviour 4).
pub fn detect(route: &Route) -> Result<HostPlatform, RemoteError> {
    let out = route.sh("uname -sm", &[], None);
    if let Ok(out) = &out
        && out.success()
    {
        let platform = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if platform.is_empty() {
            return Err(RemoteError::Failed(format!(
                "{}: the host did not say what it is",
                route.label()
            )));
        }
        return Ok(HostPlatform {
            family: HostFamily::Posix,
            platform,
        });
    }
    let mut command = route.command()?;
    let probe = command
        .arg(powershell_line(POWERSHELL_PROBE))
        .stdin(Stdio::null())
        .output();
    if let Ok(p) = probe
        && p.status.success()
        && let Some(platform) = windows_platform(&String::from_utf8_lossy(&p.stdout))
    {
        return Ok(HostPlatform {
            family: HostFamily::Windows,
            platform,
        });
    }
    match out {
        Ok(o) => Err(route.refusal(&o.stderr)),
        Err(e) => Err(e),
    }
}

/// The name on a Windows host: `%LOCALAPPDATA%\idecraft\bin\idecraft-serve-<version>-<sha12>.exe`.
pub fn windows_backend_path(version: &str, sha: &str) -> String {
    format!(
        "$env:LOCALAPPDATA\\idecraft\\bin\\idecraft-serve-{version}-{}.exe",
        &sha[..12.min(sha.len())]
    )
}

/// The PowerShell script that receives the build on standard input and checks it.
pub fn windows_upload_script(path: &str, sha: &str) -> String {
    format!(
        "$p = \"{path}\"; \
         New-Item -ItemType Directory -Force -Path (Split-Path $p) | Out-Null; \
         $t = \"$p.part\"; \
         $s = [Console]::OpenStandardInput(); $f = [IO.File]::Create($t); $s.CopyTo($f); $f.Close(); \
         $h = (Get-FileHash -Algorithm SHA256 $t).Hash.ToLower(); \
         if ($h -ne '{sha}') {{ Remove-Item -Force $t; Write-Error 'the backend upload was damaged'; exit 3 }}; \
         Move-Item -Force $t $p"
    )
}

/// The PowerShell script that says whether the installed build matches.
pub fn windows_check_script(path: &str, sha: &str) -> String {
    format!(
        "$p = \"{path}\"; if (-not (Test-Path $p)) {{ exit 4 }}; \
         $h = (Get-FileHash -Algorithm SHA256 $p).Hash.ToLower(); \
         if ($h -ne '{sha}') {{ Remove-Item -Force $p; exit 3 }}"
    )
}

/// The PowerShell command line that starts the installed backend with `arguments`.
pub fn windows_serve_line(path: &str, arguments: &str) -> String {
    powershell_line(&format!("& \"{path}\" {arguments}"))
}

/// Installs the build on a Windows host, checked by SHA-256 before it is moved into place, and
/// returns its path as PowerShell expands it. Later starts check it again.
pub fn install_windows(
    route: &Route,
    build: &Path,
    version: &str,
) -> Result<(String, String), RemoteError> {
    let sha = sha256_hex(build)?;
    let path = windows_backend_path(version, &sha);
    let check = windows_check_script(&path, &sha);
    let present = route
        .command()?
        .arg(powershell_line(&check))
        .stdin(Stdio::null())
        .status()
        .map_err(|e| RemoteError::Failed(e.to_string()))?
        .success();
    if !present {
        let file = std::fs::File::open(build)
            .map_err(|e| RemoteError::Failed(format!("{}: {e}", build.display())))?;
        let out = route
            .command()?
            .arg(powershell_line(&windows_upload_script(&path, &sha)))
            .stdin(Stdio::from(file))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| RemoteError::Failed(e.to_string()))?;
        if !out.status.success() {
            return Err(route.refusal(&String::from_utf8_lossy(&out.stderr)));
        }
    }
    Ok((path, sha))
}

/// Verifies an installed Windows backend before a start.
pub fn verify_windows(route: &Route, path: &str, sha: &str) -> Result<(), RemoteError> {
    let ok = route
        .command()?
        .arg(powershell_line(&windows_check_script(path, sha)))
        .stdin(Stdio::null())
        .status()
        .map_err(|e| RemoteError::Failed(e.to_string()))?
        .success();
    if ok {
        Ok(())
    } else {
        Err(RemoteError::Failed(format!(
            "{}: the backend on the host does not match this build (SHA-256 differs); the copy \
             was deleted and is uploaded again on the next open",
            route.label()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_of_every_host_are_absolute() {
        for ok in ["/srv/p", "C:\\work", "d:/x"] {
            assert!(absolute_folder(ok), "{ok}");
        }
        for bad in ["", "work", "C:work", "1:/x"] {
            assert!(!absolute_folder(bad), "{bad}");
        }
    }

    #[test]
    fn ak2_platforms_map_to_targets() {
        assert_eq!(
            target_triple("Linux x86_64"),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            target_triple("Linux aarch64"),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(target_triple("Darwin arm64"), Some("aarch64-apple-darwin"));
        assert_eq!(
            target_triple("Windows amd64"),
            Some("x86_64-pc-windows-gnu")
        );
        assert_eq!(
            target_triple("Windows arm64"),
            Some("aarch64-pc-windows-gnullvm")
        );
        assert_eq!(target_triple("FreeBSD amd64"), None);
    }

    #[test]
    fn ak2_windows_answers_are_read() {
        assert_eq!(
            windows_platform("AMD64\r\n"),
            Some("Windows amd64".to_owned())
        );
        assert_eq!(windows_platform("ARM64"), Some("Windows arm64".to_owned()));
        assert_eq!(windows_platform("x86"), None);
    }

    #[test]
    fn ak2_a_missing_build_is_refused_with_what_to_do() {
        let binary = PathBuf::from("/nonexistent/idecraft");
        let e = select_build(
            &HostPlatform {
                family: HostFamily::Posix,
                platform: "FreeBSD amd64".into(),
            },
            &binary,
        )
        .unwrap_err()
        .to_string();
        assert!(
            e.contains("no backend build") && e.contains("--pair"),
            "{e}"
        );
        // A known target whose build is not there names the script and the folders.
        let other = if crate::local_platform().starts_with("Linux") {
            "Darwin arm64"
        } else {
            "Linux x86_64"
        };
        let e = select_build(
            &HostPlatform {
                family: HostFamily::Posix,
                platform: other.into(),
            },
            &binary,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("build-backends.sh"), "{e}");
    }

    #[test]
    fn ak2_same_platform_falls_back_to_the_running_program() {
        let binary = std::env::current_exe().unwrap();
        let picked = select_build(
            &HostPlatform {
                family: HostFamily::Posix,
                platform: crate::local_platform(),
            },
            &binary,
        )
        .unwrap();
        assert_eq!(picked, binary);
    }

    #[test]
    fn ak2_a_dedicated_build_wins() {
        let dir = tempfile::tempdir().unwrap();
        let t = "x86_64-unknown-linux-gnu";
        let folder = dir.path().join("backends").join(t);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("idecraft-serve"), b"x").unwrap();
        let binary = dir.path().join("idecraft");
        let picked = select_build(
            &HostPlatform {
                family: HostFamily::Posix,
                platform: "Linux x86_64".into(),
            },
            &binary,
        )
        .unwrap();
        assert_eq!(picked, folder.join("idecraft-serve"));
    }

    #[test]
    fn ak2_powershell_lines_are_base64_of_utf16() {
        let line = powershell_line("Write-Output 1");
        assert!(line.starts_with("powershell -NoProfile -NonInteractive -EncodedCommand "));
        let b64 = line.rsplit(' ').next().unwrap();
        assert_eq!(b64, "VwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAxAA==");
    }

    #[test]
    fn windows_scripts_check_the_hash_before_moving() {
        let sha = "ab".repeat(32);
        let path = windows_backend_path("0.11.0", &sha);
        assert!(path.ends_with("idecraft-serve-0.11.0-abababababab.exe"));
        let s = windows_upload_script(&path, &sha);
        let hash = s.find("Get-FileHash").unwrap();
        let mv = s.find("Move-Item").unwrap();
        assert!(hash < mv, "the hash check comes first");
        assert!(s.contains(&sha));
    }
}

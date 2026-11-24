//! Starting this test program as the sandboxed child (`super::sandbox_child`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;

use ferris_idea_sandbox::{Policy, SandboxCommand, SandboxError, ValidPolicy};

/// The folder of this test program, which the child needs to read.
pub fn exe_dir() -> PathBuf {
    std::fs::canonicalize(std::env::current_exe().unwrap())
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A policy with this program's folder as a read root.
pub fn policy(write: &[PathBuf], hosts: &[String]) -> ValidPolicy {
    Policy {
        read: vec![exe_dir()],
        write: write.to_vec(),
        hosts: hosts.to_vec(),
    }
    .validate(std::env::var_os("HOME").map(PathBuf::from).as_deref())
    .unwrap()
}

/// Runs `actions` in the sandbox; the outcome of each (`ok` or `denied …`), or the refusal.
pub fn run(
    policy: ValidPolicy,
    actions: &[String],
) -> Result<BTreeMap<String, String>, SandboxError> {
    let mut cmd = SandboxCommand::new(policy, std::env::current_exe().unwrap())
        .args([
            "sandbox_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FERRIS_IDEA_SANDBOX_CHILD", actions.join(";"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(l) = option_env!("CARGO_BIN_EXE_ferris-idea-sandbox-launch") {
        cmd = cmd.launcher(l);
    }
    let child = cmd.spawn()?;
    let out = child.child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let results: BTreeMap<String, String> = text
        .lines()
        .filter_map(|l| l.find("RESULT ").map(|i| &l[i + 7..]))
        .filter_map(|l| l.split_once(' '))
        .map(|(a, r)| (a.to_string(), r.to_string()))
        .collect();
    assert_eq!(
        results.len(),
        actions.len(),
        "status {}, stdout {text}, stderr {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(results)
}

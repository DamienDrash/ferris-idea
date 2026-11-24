//! The Linux sandbox the launcher enters before it executes the program (`SPEC-0347` behaviour
//! 8): a new user and network namespace, then Landlock with the rights of ABI 9 as hard
//! requirements. Every failure is an error; nothing here falls back to less.

use std::path::Path;

use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, Scope, path_beneath_rules,
};
use nix::sched::{CloneFlags, unshare};

use crate::policy::ValidPolicy;

/// The system locations every sandboxed process may read and execute below on Linux
/// (behaviour 5); missing ones are skipped.
pub const SYSTEM_READ: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/etc"];

/// Devices that may be read; `/dev/null` may also be written.
pub const DEVICES: &[&str] = &["/dev/null", "/dev/zero", "/dev/random", "/dev/urandom"];

/// The Landlock version whose rights are required.
const ABI_REQUIRED: ABI = ABI::V9;

/// Whether the running kernel can enforce the Landlock rights of behaviour 8. Creating a
/// ruleset restricts nothing; the user namespace is checked only at launch.
pub fn landlock_available() -> Result<(), String> {
    ruleset().map(|_| ())
}

fn ruleset() -> Result<landlock::RulesetCreated, String> {
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI_REQUIRED))
        .and_then(|r| r.scope(Scope::from_all(ABI_REQUIRED)))
        .and_then(|r| r.create())
        .map_err(|e| format!("Landlock ABI 9 is not available on this kernel: {e}"))
}

/// Enters the sandbox: namespaces first, then Landlock. After `Ok` this process and every
/// program it executes are confined to `policy`.
pub fn enter(policy: &ValidPolicy) -> Result<(), String> {
    if !policy.hosts().is_empty() {
        return Err("a policy with hosts cannot be enforced on Linux in this version".into());
    }
    let (uid, gid) = (nix::unistd::getuid(), nix::unistd::getgid());
    unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET)
        .map_err(|e| format!("cannot create a user and network namespace: {e}"))?;
    let write = |file: &str, text: String| {
        std::fs::write(file, text).map_err(|e| format!("cannot write {file}: {e}"))
    };
    write("/proc/self/uid_map", format!("{uid} {uid} 1\n"))?;
    write("/proc/self/setgroups", "deny".into())?;
    write("/proc/self/gid_map", format!("{gid} {gid} 1\n"))?;

    let read = AccessFs::from_read(ABI_REQUIRED);
    let all = AccessFs::from_all(ABI_REQUIRED);
    let mut created = ruleset()?;
    let rule_err = |e: landlock::RulesetError| format!("cannot add a Landlock rule: {e}");
    created = created
        .add_rules(path_beneath_rules(SYSTEM_READ, read))
        .map_err(rule_err)?
        .add_rules(path_beneath_rules(DEVICES, AccessFs::ReadFile))
        .map_err(rule_err)?
        .add_rules(path_beneath_rules(["/dev/null"], AccessFs::WriteFile))
        .map_err(rule_err)?;
    for (roots, access) in [(policy.read(), read), (policy.write(), all)] {
        for root in roots {
            created = created.add_rule(beneath(root, access)?).map_err(rule_err)?;
        }
    }
    let status = created
        .restrict_self()
        .map_err(|e| format!("cannot restrict with Landlock: {e}"))?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err(format!(
            "Landlock is not fully enforced ({:?}, no_new_privs {})",
            status.ruleset, status.no_new_privs
        ));
    }
    Ok(())
}

/// A rule for one root; a root that cannot be opened is an error, not skipped.
fn beneath(
    root: &Path,
    access: landlock::BitFlags<AccessFs>,
) -> Result<PathBeneath<PathFd>, String> {
    let fd = PathFd::new(root).map_err(|e| format!("cannot open {}: {e}", root.display()))?;
    let access = if root.is_dir() {
        access
    } else {
        // Rights that only folders have are refused on a file.
        access
            & (AccessFs::ReadFile
                | AccessFs::WriteFile
                | AccessFs::Execute
                | AccessFs::Truncate
                | AccessFs::IoctlDev
                | AccessFs::ResolveUnix)
    };
    Ok(PathBeneath::new(fd, access))
}

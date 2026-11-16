//! Post-quantum key exchange on every SSH path (ADR-0025 v5 section 1a, PO-STATUS questions 29 and
//! 30): against real `sshd`s started for the test, a scan of the workspace for places that start
//! `ssh` (only the one start point `SshCommand` may), the Git stand-in of the Azure DevOps
//! exception and its (ignored) re-check against Azure DevOps.

use std::path::{Path, PathBuf};
use std::process::Command;

use idecraft_remote::testing::TestHost;
use idecraft_remote::testing::shared_options as ssh_options;
use idecraft_remote::{PQ_KEX, Remote, RemoteError};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("idc-pq-{name}-{}", std::process::id()))
}

/// The method `ssh -v` reports as negotiated.
fn negotiated(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .find_map(|l| l.split("kex: algorithm: ").nth(1))
        .map(|s| s.trim().to_owned())
}

/// SEC test (ADR-0025 section 9, SSH channel): a host whose `sshd` offers only classical key
/// exchanges is refused with the post-quantum message; nothing runs there.
#[test]
fn sec_classical_host_is_refused() {
    let Some(h) = TestHost::start_classical(&scratch("classic")) else {
        eprintln!("skipped: no sshd/ssh-keygen");
        return;
    };
    let marker = h.dir.join("ran");
    let r = Remote::new(h.target.clone());
    let e = r
        .sh(
            "touch \"$1\"",
            &[&marker.display().to_string()],
            None,
            idecraft_remote::TIMEOUT,
        )
        .unwrap_err();
    assert_eq!(e, RemoteError::NoPqKex(h.target.host.clone()), "{e:?}");
    let text = e.to_string();
    assert!(text.contains("no post-quantum key exchange"), "{text}");
    assert!(text.contains("OpenSSH 9.0"), "{text}");
    assert!(text.contains("ADR-0025"), "{text}");
    assert!(!marker.exists(), "nothing may run on a refused host");
    assert!(matches!(r.home(), Err(RemoteError::NoPqKex(_))));
    // The same host is reachable with OpenSSH's own defaults, which would fall back to a classical
    // exchange: the refusal comes from IDECraft's options, not from a broken test server.
    let classic = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ControlMaster=no", "-v"])
        .args(&h.target.extra)
        .args(["--", &h.target.host, "true"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&classic.stderr);
    assert!(classic.status.success(), "{stderr}");
    let kex = negotiated(&stderr).unwrap_or_default();
    assert!(!PQ_KEX.contains(&kex.as_str()), "{kex}");
}

/// SEC test: a host with a current `sshd` is accepted, and the exchange negotiated with the shared
/// options is one of the hybrid post-quantum methods.
#[test]
fn sec_post_quantum_host_is_accepted() {
    let Some(h) = TestHost::start(&scratch("pq")) else {
        eprintln!("skipped: no sshd/ssh-keygen");
        return;
    };
    let r = Remote::new(h.target.clone());
    assert!(r.home().unwrap().starts_with('/'));
    let out = idecraft_remote::SshCommand::new(idecraft_remote::SshPurpose::Host)
        .option("ControlMaster=no")
        .args(["-v"])
        .extra(&h.target.extra)
        .remote(["true"])
        .build(&h.target.host)
        .unwrap()
        .command()
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let kex = negotiated(&stderr).expect("ssh -v names the key exchange");
    assert!(PQ_KEX.contains(&kex.as_str()), "negotiated {kex}");
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if !matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some("target" | "tests" | "benches" | "examples" | ".git")
            ) {
                rust_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The text of a call's argument list starting after its `(`, up to the matching `)`.
fn argument(text: &str) -> &str {
    let mut depth = 1usize;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        match c {
            '"' if prev != '\\' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return &text[..i];
                }
            }
            _ => {}
        }
        prev = c;
    }
    text
}

/// Whether `text` names the `ssh` client (not the server `sshd` or the key tools).
fn mentions_ssh(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower
        .replace("sshd", "")
        .replace("ssh-keygen", "")
        .replace("ssh-add", "")
        .contains("ssh")
}

/// The doc comment and attributes right above the line of byte `at`.
fn doc_above(code: &str, at: usize) -> String {
    let line_start = code[..at].rfind('\n').map_or(0, |n| n + 1);
    code[..line_start]
        .lines()
        .rev()
        .take_while(|l| {
            let t = l.trim_start();
            t.starts_with("///") || t.starts_with("#[") || t.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the variable or field a spawn starts (`program`, `&self.program`, `&argv[0]`) was
/// given an `ssh` value in the enclosing function: a `let` binding mentioning ssh, a parameter of a
/// function whose signature or doc mentions ssh, or a field whose doc mentions ssh.
fn origin_mentions_ssh(code: &str, at: usize, arg: &str) -> bool {
    let path = arg.trim().trim_start_matches(['&', '*']).trim();
    if path.contains('(') || path.starts_with('"') {
        return false;
    }
    let last = path.rsplit('.').next().unwrap_or(path);
    let ident: String = last
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() {
        return false;
    }
    let fn_start = code[..at].rfind("fn ").unwrap_or(0);
    let body = &code[fn_start..at];
    for pat in [format!("let {ident}"), format!("let mut {ident}")] {
        for (i, _) in body.match_indices(&pat) {
            let rest = &body[i + pat.len()..];
            if !rest.starts_with([' ', ':', '=']) {
                continue;
            }
            let stmt = rest.split(';').next().unwrap_or(rest);
            if mentions_ssh(stmt) {
                return true;
            }
        }
    }
    let signature = body.split('{').next().unwrap_or(body);
    if signature.contains(&format!("{ident}:"))
        && (mentions_ssh(signature) || mentions_ssh(&doc_above(code, fn_start)))
    {
        return true;
    }
    if path.starts_with("self.") {
        for (i, _) in code.match_indices(&format!("{ident}:")) {
            let line_start = code[..i].rfind('\n').map_or(0, |n| n + 1);
            let line = code[line_start..].lines().next().unwrap_or("").trim();
            let field = line.starts_with(&format!("{ident}:"))
                || line.starts_with(&format!("pub {ident}:"))
                || line.starts_with(&format!("pub(crate) {ident}:"));
            if field && line.ends_with(',') && mentions_ssh(&doc_above(code, line_start)) {
                return true;
            }
        }
    }
    false
}

/// What starts `ssh` in `code` (product code without its unit tests), by line: (rule, line, text).
/// Rules: `spawn` — a process started with an argument or origin that mentions ssh; `argv` — an
/// `ssh` program name put into a command line by hand; `git` — `GIT_SSH_COMMAND`/`GIT_SSH` not
/// from the start point; `script` — shell text that runs `ssh`; `options` — the shared options
/// for tests used in the product.
fn ssh_starts(code: &str) -> Vec<(&'static str, usize, String)> {
    let line = |i: usize| code[..i].matches('\n').count() + 1;
    let mut out = Vec::new();
    for pat in ["Command::new(", "CommandBuilder::new("] {
        for (i, _) in code.match_indices(pat) {
            // `SshCommand::new(` and the like are not a process.
            if code[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let arg = argument(&code[i + pat.len()..]);
            if mentions_ssh(arg) || origin_mentions_ssh(code, i, arg) {
                out.push(("spawn", line(i), format!("{pat}{arg})")));
            }
        }
    }
    for pat in [
        "\"ssh\".into()",
        "\"ssh\".to_owned()",
        "\"ssh\".to_string()",
        "String::from(\"ssh\")",
        "PathBuf::from(\"ssh\")",
        "OsString::from(\"ssh\")",
        "vec![\"ssh\"",
    ] {
        for (i, _) in code.match_indices(pat) {
            out.push(("argv", line(i), pat.to_owned()));
        }
    }
    for pat in ["\"GIT_SSH_COMMAND\"", ".env(\"GIT_SSH\","] {
        for (i, _) in code.match_indices(pat) {
            let window = &code[i..(i + 240).min(code.len())];
            if !window.contains("SshCommand::git_ssh_command()") {
                out.push(("git", line(i), pat.to_owned()));
            }
        }
    }
    for (n, l) in code.lines().enumerate() {
        if l.contains('"') && l.contains("exec ") && mentions_ssh(l) {
            out.push(("script", n + 1, l.trim().to_owned()));
        }
    }
    for pat in ["testing::shared_options(", "ssh_options("] {
        for (i, _) in code.match_indices(pat) {
            out.push(("options", line(i), pat.to_owned()));
        }
    }
    out.sort_by_key(|(rule, line, _)| (*line, *rule));
    out.dedup_by_key(|(rule, line, _)| (*line, *rule));
    out
}

/// The scan finds every way of starting `ssh`, also through variables, and leaves other programs
/// alone.
#[test]
fn the_scan_finds_ssh_through_variables_too() {
    let found = |code: &str| {
        ssh_starts(code)
            .into_iter()
            .map(|(r, ..)| r)
            .collect::<Vec<_>>()
    };
    for code in [
        "fn a() { std::process::Command::new(\"ssh\").arg(\"x\"); }",
        "fn a() { let p = \"ssh\"; Command::new(p).spawn(); }",
        "fn a() { let mut program = which(\"ssh\"); Command::new(&program); }",
        "/// Opens an SSH tunnel.\nfn open(program: &str) { Command::new(program).spawn(); }",
        "fn a(&self) { Command::new(&self.ssh_program()); }",
        "struct S {\n    /// The ssh program.\n    program: String,\n}\nfn a(&self) { Command::new(&self.program); }",
        "fn a(cmd: Vec<String>) { let argv = remote_ssh(); CommandBuilder::new(&argv[0]); }",
    ] {
        assert_eq!(found(code), ["spawn"], "{code}");
    }
    assert_eq!(
        found("fn a() { let v = vec![\"ssh\".to_owned()]; }"),
        ["argv"]
    );
    assert_eq!(
        found("fn a() { cmd.env(\"GIT_SSH_COMMAND\", \"ssh -o X\"); }"),
        ["git"]
    );
    assert_eq!(
        found("const S: &str = \"#!/bin/sh\\nexec /usr/bin/ssh \\\"$@\\\"\";"),
        ["script"]
    );
    for code in [
        "fn a() { Command::new(\"git\").arg(\"fetch\"); }",
        "fn a() { Command::new(sshd).arg(\"-D\"); Command::new(\"ssh-keygen\"); }",
        "fn a() { SshCommand::new(SshPurpose::Host).build(\"h\"); }",
        "struct S {\n    /// The docker CLI.\n    program: String,\n}\nfn a(&self) { Command::new(&self.program); }",
        "/// Runs a build.\nfn run(program: &str) { Command::new(program); }",
        "fn a() { let p = \"cargo\"; Command::new(p); }",
        "fn a() { cmd.env(\"GIT_SSH_COMMAND\", idecraft_remote::SshCommand::git_ssh_command()); }",
    ] {
        assert!(found(code).is_empty(), "{code}: {:?}", ssh_starts(code));
    }
}

/// ADR-0025 v5 section 1a: there is one start point of `ssh` in the product. Only
/// `crates/remote/src/launch.rs` (`SshCommand`) starts the program; only the stand-in writers of
/// the remote crate contain shell text that runs it; every known SSH path goes through the start
/// point.
#[test]
fn ssh_starts_only_at_the_one_start_point() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_files(crates, &mut files);
    let scripts = [
        "remote/src/pq.rs",
        "remote/src/pq_git.rs",
        "remote/src/jump.rs",
    ];
    let mut bad = Vec::new();
    let mut start_point = 0;
    let mut users = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        // Unit tests at the end of a file may start `ssh` on purpose.
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        let rel = f.strip_prefix(crates).unwrap().display().to_string();
        for (rule, line, what) in ssh_starts(code) {
            let allowed = match rule {
                "spawn" => rel == "remote/src/launch.rs",
                "script" | "argv" => scripts.contains(&rel.as_str()),
                "options" => rel.starts_with("remote/src/"),
                _ => false,
            };
            if rel == "remote/src/launch.rs" && rule == "spawn" {
                start_point += 1;
            }
            if !allowed {
                bad.push(format!("{rel}:{line} ({rule}) {what}"));
            }
        }
        if code.contains("SshCommand::") || code.contains(".shell_command(") {
            users.push(rel);
        }
    }
    assert!(
        bad.is_empty(),
        "ssh started outside the start point: {bad:#?}"
    );
    assert!(start_point >= 1, "the start point itself was not found");
    for known in [
        "remote/src/ssh.rs",
        "remote/src/config.rs",
        "containers/src/docker.rs",
        "containers/src/compose.rs",
        "database/src/tunnel.rs",
        "vcs/src/remote.rs",
        "workbench/src/workbench_view.rs",
    ] {
        assert!(
            users.iter().any(|c| c == known),
            "{known} does not use the start point: {users:?}"
        );
    }
}

/// A stand-in for the system `ssh`: `-G` prints `hostname $FAKE_RESOLVED` (what `~/.ssh/config`
/// would resolve), any other call writes its arguments to `$FAKE_ARGS`, puts a negotiated method
/// and an error into the `-E` log and exits 7.
fn fake_ssh(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-ssh");
    std::fs::write(
        &p,
        "#!/bin/sh\n\
         for a in \"$@\"; do [ \"$a\" = -G ] && { echo \"user git\"; echo \"hostname $FAKE_RESOLVED\"; exit 0; }; done\n\
         printf '%s\\n' \"$@\" > \"$FAKE_ARGS\"\n\
         log=; prev=\n\
         for a in \"$@\"; do [ \"$prev\" = -E ] && log=$a; prev=$a; done\n\
         if [ -n \"$log\" ]; then echo 'debug1: kex: algorithm: diffie-hellman-group18-sha512' >> \"$log\"; echo 'Transferred: sent 1, received 2 bytes' >> \"$log\"; echo 'Permission denied (publickey).' >> \"$log\"; fi\n\
         exit 7\n",
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}

/// ADR-0025 section 1a, PO-STATUS question 30: the Git stand-in allows the classical methods only
/// when the host name *resolved* by `ssh -G` is an Azure DevOps host — an alias resolving to Azure
/// gets them, a host argument that merely reads like Azure but resolves elsewhere does not — and
/// it reports the negotiated method and replays `ssh`'s own messages.
#[test]
fn azure_exception_follows_the_resolved_host_name() {
    let dir = scratch("azure-shim");
    let _ = std::fs::remove_dir_all(&dir);
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let ssh = fake_ssh(&dir);
    let strict = ssh_options("ssh");
    let azure_kex = idecraft_remote::git_kex(
        idecraft_remote::KexPolicy::Hybrid,
        true,
        "ssh.dev.azure.com",
        &[
            "mlkem768x25519-sha256".to_owned(),
            "diffie-hellman-group18-sha512".to_owned(),
            "diffie-hellman-group16-sha512".to_owned(),
        ],
    );
    let azure: Vec<String> = [
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=yes".to_owned(),
        "-o".to_owned(),
        format!("KexAlgorithms={}", azure_kex.join(",")),
    ]
    .to_vec();
    let script = dir.join("git-ssh");
    std::fs::write(
        &script,
        idecraft_remote::git_shim_script(
            &ssh.display().to_string(),
            &strict,
            Some(&azure),
            &tmp,
            "/no/hop",
            false,
        ),
    )
    .unwrap();
    let run = |host_arg: &str, resolved: &str| {
        let args_file = dir.join("args");
        let _ = std::fs::remove_file(&args_file);
        let out = Command::new("sh")
            .arg(&script)
            .args(["-o", "SendEnv=GIT_PROTOCOL", "-p", "22", host_arg])
            .arg("git-upload-pack 'v3/org/project/repo'")
            .env("FAKE_RESOLVED", resolved)
            .env("FAKE_ARGS", &args_file)
            .output()
            .unwrap();
        let args = std::fs::read_to_string(&args_file).unwrap_or_default();
        (out, args)
    };
    // An alias of ~/.ssh/config whose Hostname is Azure DevOps: the exception applies.
    for resolved in idecraft_remote::AZURE_DEVOPS_HOSTS {
        let (out, args) = run("my-azure", resolved);
        assert_eq!(out.status.code(), Some(7), "ssh's status is kept");
        assert!(
            args.contains("diffie-hellman-group18-sha512"),
            "{resolved}: {args}"
        );
        assert!(args.contains("StrictHostKeyChecking=yes"), "{args}");
        assert!(args.lines().any(|l| l == "my-azure"), "{args}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Permission denied (publickey)."),
            "{stderr}"
        );
        assert!(
            !stderr.contains("debug1:") && !stderr.contains("Transferred:"),
            "{stderr}"
        );
        let (reports, _) = idecraft_remote::take_kex_reports(&stderr);
        assert_eq!(reports.len(), 1, "{stderr}");
        assert_eq!(reports[0].host, resolved);
        assert_eq!(reports[0].pq(), Some(false));
        assert!(
            std::fs::read_dir(&tmp).unwrap().next().is_none(),
            "the debug log is removed"
        );
    }
    // Looks like Azure, resolves elsewhere (a Hostname or alias trick): strict, no report.
    for (host_arg, resolved) in [
        ("ssh.dev.azure.com", "evil.example"),
        ("ssh.dev.azure.com", "ssh.dev.azure.com.evil.example"),
        ("azure", "SSH.DEV.AZURE.COM"),
        ("github.com", "github.com"),
        ("x", ""),
    ] {
        let (out, args) = run(host_arg, resolved);
        assert_eq!(out.status.code(), Some(7));
        assert!(
            !args.contains("diffie-hellman"),
            "{host_arg}/{resolved}: {args}"
        );
        assert!(!args.lines().any(|l| l == "-E"), "{args}");
        assert!(args.contains("KexAlgorithms="), "{args}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(idecraft_remote::take_kex_reports(&stderr).0.is_empty());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// No other SSH path gets the exception: the shared options and a remote host's command line to an
/// Azure DevOps host name carry only post-quantum methods.
#[test]
fn non_git_paths_to_azure_get_no_exception() {
    let remote = Remote::new(idecraft_remote::SshTarget::parse("ssh.dev.azure.com", &[]));
    let Ok(line) = remote.ssh_args() else {
        eprintln!("skipped: no ssh with a post-quantum key exchange");
        return;
    };
    for args in [ssh_options("ssh"), line] {
        let kex = args
            .iter()
            .find_map(|a| a.strip_prefix("KexAlgorithms="))
            .expect("KexAlgorithms is set");
        for a in kex.split(',') {
            assert!(PQ_KEX.contains(&a), "{a} on a non-Git path");
        }
    }
}

/// Re-check of the Azure DevOps exception (ADR-0025 section 1a, PO-STATUS question 30); ignored by
/// default, run by hand or in the nightly job:
/// `cargo test -p idecraft-remote --test it -- --ignored pq_kex::azure_devops`. It connects without a
/// key or login (the connection ends at the key exchange or at host key verification) and fails
/// when Azure DevOps offers a post-quantum key exchange — then the exception is to be removed — or
/// no longer offers an allowed classical one.
#[test]
#[ignore = "network: re-check of the Azure DevOps exception"]
fn azure_devops_still_lacks_post_quantum() {
    for host in idecraft_remote::AZURE_DEVOPS_HOSTS {
        let probe = |kex: &str| {
            Command::new("ssh")
                .args(["-F", "/dev/null", "-o", "BatchMode=yes"])
                .args(["-o", "PubkeyAuthentication=no"])
                .args(["-o", "PasswordAuthentication=no"])
                .args(["-o", "KbdInteractiveAuthentication=no"])
                .args(["-o", "IdentitiesOnly=yes"])
                .args(["-o", "UserKnownHostsFile=/dev/null"])
                .args(["-o", "GlobalKnownHostsFile=/dev/null"])
                .args(["-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=10"])
                .args(["-o", &format!("KexAlgorithms={kex}")])
                .args(["--", host, "true"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stderr).to_lowercase())
                .unwrap_or_default()
        };
        let pq = probe(&PQ_KEX.join(","));
        if !pq.contains("no matching key exchange method")
            && !pq.contains("host key verification failed")
        {
            eprintln!("skipped {host}: not reachable ({})", pq.trim());
            continue;
        }
        assert!(
            pq.contains("no matching key exchange method"),
            "{host} now offers a post-quantum key exchange: remove the Azure DevOps exception \
             (ADR-0025 section 1a, AZURE_CLASSIC_KEX in crates/remote/src/pq_git.rs, setting \
             git.ssh.allowClassicKexForAzureDevOps)"
        );
        let classic = probe(&idecraft_remote::AZURE_CLASSIC_KEX.join(","));
        assert!(
            classic.contains("host key verification failed"),
            "{host} no longer offers an allowed classical key exchange; check its offer and \
             AZURE_CLASSIC_KEX: {}",
            classic.trim()
        );
    }
}

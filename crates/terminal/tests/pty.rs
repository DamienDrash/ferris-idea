//! SPEC-0038 AK-5 with real programs in a pseudo terminal.
#![cfg(unix)]

use std::time::Duration;

use ferris_idea_terminal::{Color, Session};

const WAIT: Duration = Duration::from_secs(10);

fn sh(script: &str, rows: u16, cols: u16) -> Session {
    let cwd = std::env::temp_dir();
    let cmd = ["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()];
    Session::spawn(&cmd, &cwd, rows, cols).unwrap()
}

#[test]
fn coloured_output_and_exit_status() {
    let mut s = sh("printf '\\033[31mred\\033[0m plain'; exit 3", 5, 40);
    assert_eq!(s.wait_exit(WAIT).unwrap(), Some(3));
    assert!(s.wait_for(WAIT, |t| t == "red plain"), "{:?}", s.text());
    assert!(s.generation() > 0, "output is counted for redraws");
    s.with_terminal(|t| {
        assert_eq!(t.screen.cell(0, 0).unwrap().style.fg, Color::Indexed(1));
        assert_eq!(t.screen.cell(0, 4).unwrap().style.fg, Color::Default);
    });
}

#[test]
fn size_resize_cwd_and_input() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-pty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cmd = [
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "stty size; pwd; read line; echo \"got $line\"; stty size".to_owned(),
    ];
    let mut s = Session::spawn(&cmd, &dir, 24, 80).unwrap();
    assert!(s.wait_for(WAIT, |t| t.contains("24 80")), "{:?}", s.text());
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(s.wait_for(WAIT, |t| t.contains(&name)), "{:?}", s.text());
    s.resize(30, 100).unwrap();
    s.write(b"hello\r").unwrap();
    assert!(
        s.wait_for(WAIT, |t| t.contains("got hello")),
        "{:?}",
        s.text()
    );
    assert!(s.wait_for(WAIT, |t| t.contains("30 100")), "{:?}", s.text());
    assert_eq!(s.wait_exit(WAIT).unwrap(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kill_ends_a_running_program() {
    let mut s = sh("echo started; sleep 30", 5, 20);
    assert!(s.wait_for(WAIT, |t| t.contains("started")));
    s.kill().unwrap();
    assert!(s.wait_exit(WAIT).unwrap().is_some());
}

#[test]
fn cursor_position_report_is_answered() {
    // The program asks for the cursor position and prints what it got back.
    let script = "stty raw -echo; printf 'ab\\033[6n'; dd bs=1 count=6 2>/dev/null | od -An -c | tr -d ' \\n'; stty sane";
    let s = sh(script, 5, 40);
    assert!(
        s.wait_for(WAIT, |t| t.contains("033[1;3R")),
        "{:?}",
        s.text()
    );
}

#[test]
fn wide_characters_from_a_program() {
    let s = sh("printf '日本語 ok'", 3, 20);
    assert!(s.wait_for(WAIT, |t| t == "日本語 ok"), "{:?}", s.text());
    s.with_terminal(|t| assert_eq!(t.screen.cursor(), (0, 9)));
}

/// SPEC-0161: the shell's current folder follows `cd`, read from the process without OSC 7.
#[test]
fn current_folder_follows_cd() {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = base.join(format!("ferris-idea-cd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cmd = [
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        format!("cd '{}' && echo moved && sleep 5", dir.display()),
    ];
    let mut s = Session::spawn(&cmd, &base, 5, 40).unwrap();
    assert!(s.wait_for(WAIT, |t| t.contains("moved")), "{:?}", s.text());
    let here = s.current_dir().map(|d| std::fs::canonicalize(d).unwrap());
    assert_eq!(here.as_deref(), Some(dir.as_path()));
    let _ = s.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0161 v2 AK-5: a shell that reported its folder with OSC 7 starts `sh`, which sends no
/// OSC 7 and runs `cd ..`: the folder is the nested shell's; after `exit` the OSC 7 one again.
#[test]
fn nested_shell_without_osc7_is_followed() {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = base.join(format!("ferris-idea-nested-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let canonical = |d: std::path::PathBuf| std::fs::canonicalize(d).unwrap();
    let mut s = Session::spawn(&["/bin/sh".to_owned()], &dir, 10, 80).unwrap();
    let prompt = |t: &str| t.trim_end().ends_with('$') || t.trim_end().ends_with('#');
    assert!(s.wait_for(WAIT, prompt), "{:?}", s.text());
    // The outer shell reports its folder as zsh's integration does; the output marker is written
    // apart so that the echoed command line does not count.
    s.write(b"printf '\\033]7;file://host%s\\007' \"$PWD\"; echo \"rep\"\"orted\"\r")
        .unwrap();
    assert!(
        s.wait_for(WAIT, |t| t.contains("reported")),
        "{:?}",
        s.text()
    );
    assert!(s.with_terminal(|t| t.screen.reported_dir().is_some()));
    let shell = s.foreground_group();
    assert!(shell.is_some());
    assert_eq!(
        s.current_dir().map(canonical).as_deref(),
        Some(dir.as_path())
    );
    s.write(b"sh\r").unwrap();
    s.write(b"cd ..; echo \"nested\"\"-moved\"\r").unwrap();
    assert!(
        s.wait_for(WAIT, |t| t.contains("nested-moved")),
        "{:?}",
        s.text()
    );
    assert_ne!(
        s.foreground_group(),
        shell,
        "the nested shell leads its own group"
    );
    assert_eq!(
        s.current_dir().map(canonical).as_deref(),
        Some(base.as_path())
    );
    s.write(b"exit\r").unwrap();
    let start = std::time::Instant::now();
    while s.foreground_group() != shell && start.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(s.foreground_group(), shell, "{:?}", s.text());
    assert_eq!(
        s.current_dir().map(canonical).as_deref(),
        Some(dir.as_path())
    );
    let _ = s.kill_group();
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0205 AK-2: a shell that runs `exit` ends its session; saved lines come first.
#[test]
fn exit_ends_the_shell_and_history_comes_first() {
    let cwd = std::env::temp_dir();
    let options = ferris_idea_terminal::SpawnOptions {
        history: vec!["saved line".into()],
        integration: None,
        env: Vec::new(),
    };
    let mut s = Session::spawn_with(&["/bin/sh".to_owned()], &cwd, 10, 40, &options).unwrap();
    assert!(s.exited().is_none());
    // Typed once the shell asks, as a person would: input that comes before the first prompt
    // is echoed at once, and the shell's prompt then lands before the output (`$ after`).
    assert!(
        s.wait_for(WAIT, |t| t.trim_end().ends_with('$')
            || t.trim_end().ends_with('#')),
        "{:?}",
        s.text()
    );
    s.write(b"echo after\r").unwrap();
    assert!(
        s.wait_for(WAIT, |t| t.contains("\nafter")),
        "{:?}",
        s.text()
    );
    assert!(s.text().starts_with("saved line\n"), "{:?}", s.text());
    s.write(b"exit 4\r").unwrap();
    let start = std::time::Instant::now();
    while s.exited().is_none() && start.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(s.exited(), Some(4));
}

/// A test home whose start-up files set `FROM_USER`, a fixed prompt and hooks of their own, and
/// the options that start a shell in it as the terminal view does (integration files in `dir`).
fn test_shell(dir: &std::path::Path) -> (std::path::PathBuf, ferris_idea_terminal::SpawnOptions) {
    let home = dir.join("home");
    std::fs::create_dir_all(home.join("sub folder")).unwrap();
    // The user's own files are loaded first and keep their effect, hooks included.
    std::fs::write(
        home.join(".zshrc"),
        "export FROM_USER=zsh\nPS1='%% '\nprecmd() { USER_PRECMD=1 }\npreexec() { USER_PREEXEC=1 }\n",
    )
    .unwrap();
    std::fs::write(home.join(".bashrc"), "export FROM_USER=bash\nPS1='$ '\n").unwrap();
    let options = ferris_idea_terminal::SpawnOptions {
        history: Vec::new(),
        integration: Some(dir.join("files")),
        env: vec![
            ("HOME".into(), home.display().to_string()),
            (
                "FERRIS_IDEA_USER_ZDOTDIR".into(),
                home.display().to_string(),
            ),
        ],
    };
    (home, options)
}

/// The prompts marked once there are `n`, or after the wait.
fn wait_prompts(s: &Session, n: usize) -> Vec<ferris_idea_terminal::Prompt> {
    let prompts = |s: &Session| s.with_terminal(|t| t.screen.prompts().to_vec());
    let start = std::time::Instant::now();
    while prompts(s).len() < n && start.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    prompts(s)
}

/// SPEC-0206 AK-2: zsh and bash with the generated start-up files mark prompts, commands and
/// statuses and report the folder after `cd`; the user's own files and hooks still run.
#[test]
fn shell_integration_marks_prompts_in_zsh_and_bash() {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-pty-integration-{}",
        std::process::id()
    ));
    let (home, options) = test_shell(&dir);
    let sub = home.join("sub folder");
    for shell in ["/bin/zsh", "/bin/bash"] {
        if !std::path::Path::new(shell).exists() {
            continue;
        }
        let mut s = Session::spawn_with(&[shell.to_owned()], &home, 20, 80, &options).unwrap();
        wait_prompts(&s, 1);
        s.write(b"echo $FROM_USER; false\r").unwrap();
        s.write(b"cd 'sub folder'\r").unwrap();
        let p = wait_prompts(&s, 3);
        assert!(p.len() >= 3, "{shell}: {p:?}\n{}", s.text());
        assert_eq!(
            p[0].command.as_deref(),
            Some("echo $FROM_USER; false"),
            "{shell}"
        );
        assert_eq!(p[0].exit, Some(1), "{shell}");
        assert_eq!(p[1].command.as_deref(), Some("cd 'sub folder'"), "{shell}");
        assert_eq!(p[1].exit, Some(0), "{shell}");
        assert!(s.text().contains(&shell[5..]), "{shell}: {}", s.text());
        let reported = s.with_terminal(|t| t.screen.reported_dir().map(str::to_owned));
        assert_eq!(
            reported.map(|d| std::fs::canonicalize(d).unwrap()),
            Some(std::fs::canonicalize(&sub).unwrap()),
            "{shell}"
        );
        if shell.ends_with("zsh") {
            s.write(b"echo hooks-$USER_PRECMD-$USER_PREEXEC\r").unwrap();
            assert!(
                s.wait_for(WAIT, |t| t.contains("hooks-1-1")),
                "the user's precmd and preexec still run: {}",
                s.text()
            );
            // A history file the system's zshrc names lies in the user's folder, not ours.
            s.write(b"echo \"hist<$HISTFILE>\"\r").unwrap();
            assert!(
                s.wait_for(WAIT, |t| t.lines().any(|l| l.starts_with("hist<"))),
                "{}",
                s.text()
            );
            let shown = s.text().replace('\n', "");
            assert!(!shown.contains("files/zsh/.zsh_history"), "{shown}");
        }
        s.kill().unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window self-test's case (SPEC-0313 AK-6): a command sent at once after the first prompt
/// mark gets its command line and exit code, also where the screen cannot show the line whole.
/// zsh in a terminal of fewer than three rows scrolls one line sideways (it reports the line
/// itself), and a line that wraps right after a space keeps that space.
#[test]
fn the_command_line_survives_short_and_narrow_terminals() {
    const LINE: &str = "echo GITHUB_TOKEN=ghp_FAKEFAKEFAKEFAKEFAKEFAKE0000 ';%'";
    let dir = std::env::temp_dir().join(format!("ferris-idea-pty-short-{}", std::process::id()));
    let (home, options) = test_shell(&dir);
    for shell in ["/bin/zsh", "/bin/bash"] {
        if !std::path::Path::new(shell).exists() {
            continue;
        }
        // The prompt `% ` or `$ ` takes two columns: at 7 the space after `echo` fills the last.
        for (rows, cols) in [(2, 40), (1, 60), (5, 7), (24, 80)] {
            // Only zsh's line editor switches to one sideways-scrolled line in short terminals.
            if shell.ends_with("bash") && rows < 3 {
                continue;
            }
            let mut s =
                Session::spawn_with(&[shell.to_owned()], &home, rows, cols, &options).unwrap();
            assert_eq!(wait_prompts(&s, 1).len(), 1, "{shell} {rows}x{cols}");
            // zsh reports the bytes it ran, whatever the locale; bash's screen needs one for them.
            let line = if shell.ends_with("zsh") {
                format!("{LINE} 'ä 日'")
            } else {
                LINE.to_owned()
            };
            s.write(format!("{line}\r").as_bytes()).unwrap();
            let p = wait_prompts(&s, 2);
            assert_eq!(
                (p[0].command.as_deref(), p[0].exit),
                (Some(line.as_str()), Some(0)),
                "{shell} {rows}x{cols}: {}",
                s.history_text()
            );
            s.kill().unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window self-test's fallback case (SPEC-0313 behaviour 2 without marks): `/bin/sh` gets no
/// integration and marks nothing; in a terminal of one row the command's output has left the
/// screen for the history, where Last n Lines finds it.
#[test]
fn a_shell_without_marks_in_one_row_keeps_its_output_in_the_history() {
    let dir = std::env::temp_dir().join(format!("ferris-idea-pty-unmarked-{}", std::process::id()));
    let (home, options) = test_shell(&dir);
    let mut s = Session::spawn_with(&["/bin/sh".to_owned()], &home, 1, 60, &options).unwrap();
    assert!(s.wait_for(WAIT, |t| !t.trim().is_empty()), "no prompt");
    s.write(b"echo unmarked-$((40+2))\r").unwrap();
    let ran = |s: &Session| s.history_text().lines().any(|l| l.trim() == "unmarked-42");
    let start = std::time::Instant::now();
    while !ran(&s) && start.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ran(&s), "{}", s.history_text());
    // The next prompt follows the output; the one row shows only it.
    std::thread::sleep(Duration::from_millis(200));
    assert!(!s.text().contains("unmarked-42"), "one row: {}", s.text());
    assert!(s.with_terminal(|t| t.screen.prompts().is_empty()));
    s.kill().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0207 AK-1: a command's output as a terminal shows it, and a timeout ends its group.
#[test]
fn shell_command_output_and_group_kill() {
    let cwd = std::env::temp_dir();
    let cmd = [
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf 'progress 10%%\\rprogress 100%%\\n\\033[32mgreen\\033[0m\\n'; exit 2".to_owned(),
    ];
    let mut s = Session::spawn(&cmd, &cwd, 10, 40).unwrap();
    assert_eq!(s.wait_exit(WAIT).unwrap(), Some(2));
    assert!(s.wait_for(WAIT, |t| t.contains("green")));
    assert_eq!(s.history_text(), "progress 100%\ngreen");
    let cmd = [
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30 & echo started; wait".to_owned(),
    ];
    let mut s = Session::spawn(&cmd, &cwd, 10, 40).unwrap();
    assert!(s.wait_for(WAIT, |t| t.contains("started")));
    s.kill_group().unwrap();
    assert!(s.wait_exit(WAIT).unwrap().is_some());
}

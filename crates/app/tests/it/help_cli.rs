//! `SPEC-0321` v6 behaviour 1a, AK-23: `--help` and `-h` print the usage to standard output and
//! exit with 0, for the program and after each command; a wrong argument still exits with 2.

use std::process::{Command, Output};

fn ferris_idea(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferris-idea"))
        .args(args)
        .output()
        .unwrap()
}

fn helped(args: &[&str], expect: &str) {
    let out = ferris_idea(args);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{args:?}: {out:?}");
    assert!(said.starts_with("usage: "), "{args:?}: {said}");
    assert!(said.contains(expect), "{args:?} lacks {expect}: {said}");
}

#[test]
fn help_for_the_program_and_every_command() {
    helped(&["--help"], "ferris-idea serve --devices");
    helped(&["-h"], "ferris-idea open <directory>");
    helped(&["serve", "--help"], "--wss-origin <origin>");
    helped(&["serve", "-h"], "--reattach-minutes <n>");
    helped(&["serve", "--pair", "--help"], "--allow-network");
    helped(&["device", "--help"], "ferris-idea device pair");
    helped(&["trust", "--help"], "ferris-idea trust list");
    helped(&["quit", "-h"], "ferris-idea quit");
    helped(&["drive", "--help"], "verbs:");
    for command in [
        "open",
        "mcp",
        "call",
        "launch",
        "ui",
        "edit",
        "container-exec",
    ] {
        helped(&[command, "--help"], &format!("ferris-idea {command}"));
    }
}

#[test]
fn wrong_arguments_still_exit_with_2() {
    let out = ferris_idea(&["serve", "--no-such-option"]);
    assert_eq!(out.status.code(), Some(2));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("unknown option --no-such-option"), "{said}");
    assert!(said.contains("usage: ferris-idea serve"), "{said}");
    let out = ferris_idea(&["no-such-command"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("usage: ferris-idea open"));
}

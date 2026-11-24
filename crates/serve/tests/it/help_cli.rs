//! `SPEC-0321` v6 behaviour 1a, AK-23: `ferris-idea-serve --help`, `serve --help`, `device -h` and
//! `mcp --help` print the usage to standard output and exit with 0.

use std::process::Command;

#[test]
fn help_of_the_backend_program() {
    for (args, expect) in [
        (&["--help"][..], "ferris-idea-serve serve"),
        (&["-h"][..], "ferris-idea-serve mcp <folder>"),
        (&["serve", "--help"][..], "--wss-cert <file>"),
        (&["serve", "--stdio", "-h"][..], "--state-dir <dir>"),
        (&["device", "-h"][..], "ferris-idea device pair"),
        (&["mcp", "--help"][..], "ferris-idea-serve mcp <folder>"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_ferris-idea-serve"))
            .args(args)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(said.starts_with("usage: "), "{args:?}: {said}");
        assert!(said.contains(expect), "{args:?} lacks {expect}: {said}");
    }
}

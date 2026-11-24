//! `ferris-idea --version` prints the version from the workspace `Cargo.toml` and the maturity
//! stage (CONTENT_STYLE_GUIDE §5), e.g. `ferris-idea 0.8.0 (alpha)`.

use std::process::Command;

#[test]
fn version_flag_prints_version_and_stage() {
    let out = Command::new(env!("CARGO_BIN_EXE_ferris-idea"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let said = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        said.trim(),
        format!(
            "ferris-idea {} ({})",
            env!("CARGO_PKG_VERSION"),
            ferris_idea_core::release::STAGE
        )
    );
}

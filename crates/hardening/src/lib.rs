//! Hardening for M8.2: fuzz targets for every parser and protocol decoder that reads untrusted
//! input, a small mutation driver that runs them on the stable toolchain, and the long-session
//! soak test that drives a workspace through MCP.
//!
//! Each target is a plain `fn(&[u8])` in [`targets`]; it must not panic, hang or grow without
//! bound for any input. The same functions back the coverage-guided `cargo fuzz` targets in
//! `fuzz/` (nightly, libFuzzer) and the quick runs of [`driver`] in `cargo test` and
//! `scripts/ci-local.sh`. Commands: `docs/testing/HARDENING.md`.

pub mod driver;
pub mod soak;
pub mod targets;

pub use driver::{Budget, Crash, Report, run};
pub use targets::{TARGETS, Target, target};

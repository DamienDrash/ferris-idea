//! The integration tests of `idecraft-host` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.
//!
//! `trust.rs`, `lsp.rs` stay programs of their own: safe mode (`trust::enforce`) and the language
//! server pool hold for the whole process, and the other tests here run with neither.

mod git_ops;
mod host;

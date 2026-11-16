//! The integration tests of `idecraft-vcs` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod branch_ops;
mod commit_window;
mod conflicts;
mod git;
mod hooks;
mod log_actions;
mod merge3;
mod ops;
mod remote;
mod smart;

//! The integration tests of `idecraft` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod call_cli;
mod f084_handtests;
mod helper_processes;
mod no_display;
mod open_file;
mod remote_backend;
mod remote_project;
mod shell_env;
mod version_cli;

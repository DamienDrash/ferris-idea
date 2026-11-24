//! The integration tests of `ferris-idea` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod call_cli;
mod f083_handtests;
mod f084_handtests;
mod help_cli;
mod helper_processes;
mod no_display;
mod open_file;
mod remote_backend;
mod remote_project;
mod shell_env;
mod ui_driver_background;
mod version_cli;

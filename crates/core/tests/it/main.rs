//! The integration tests of `ferris-idea-core` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.
//!
//! `run_quit.rs` stays a program of its own: it ends every run of its process and checks that
//! none is left, which would end the runs of the other tests here.
//!
//! `trust.rs` stays a program of its own: it switches safe mode on for the whole process
//! (`trust::enforce`), and the other tests here run outside it.

mod agent_watch;
mod deptree;
mod history;
mod ideaproject;
mod java;
mod java_build;
mod jvm_resolve;
mod memory;
mod node;
mod permissions;
mod project;
mod python;
mod python_index;
mod python_launch;
mod quit_deadline;
mod recent;
mod ruby;
mod run;
mod run_context;
mod run_eof;
mod run_http;
mod run_stop;
mod run_tasks;
mod scheduler;
mod search;
mod session_history;
mod shell_env;
mod test_runners;
mod testreport;
mod tickets;
mod watch;

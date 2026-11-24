//! The integration tests of `ferris-idea-remote` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.
//!
//! `jump.rs` stays a program of its own: it sets `ssh.allowOwnProxyCommand` for the whole process.

mod host_debug;
mod host_debug_real;
mod host_functions;
mod live;
mod own_hosts;
mod paired_sshd;
mod pq_kex;
mod ssh;

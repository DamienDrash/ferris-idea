//! The integration tests of `ferris-idea-mcp` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod agent_bench;
mod agent_config;
mod agent_control;
mod agent_host;
mod agent_watch;
mod change_sets;
mod check_gates;
mod claude_link;
mod cockpit;
mod context_for;
mod effects;
mod http;
mod memory;
mod models;
mod open_files;
mod own_ssh_hosts;
mod protocol;
mod room_comment;
mod room_history;
mod sandbox;
mod scheduler;
mod session_history;
mod tickets;
mod ui_driver;
mod vault;
mod welcome;

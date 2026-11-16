//! The integration tests of `idecraft-mcp` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod effects;
mod http;
mod open_files;
mod protocol;
mod ui_driver;

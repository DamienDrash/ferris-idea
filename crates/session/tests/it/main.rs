//! The integration tests of `ferris-idea-session` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod room;
mod room_comments;
mod session;
mod ui_family;

//! The integration tests of `idecraft-serve` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod every_host;
mod room_stdio;
mod wss_serve;

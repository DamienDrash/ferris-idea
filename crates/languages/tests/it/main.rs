//! The integration tests of `ferris-idea-languages` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.
//!
//! `java_completion.rs`, `java_navigation.rs`, `java_signature.rs`, `java_smart.rs`,
//! `java_watched_files.rs`, `pool.rs`, `pool_extract.rs`, `pool_java.rs`, `py_completion.rs`,
//! `py_usages_pool.rs`, `python_server.rs`, `ts_completion.rs`, `vue_pool.rs` and `java_live/`
//! stay programs of their own: each switches the language server pool on for the whole process (`pool::enable`) and
//! checks its counters or shuts it down (`pool::running`, `pool::forwarded_events`,
//! `pool::shutdown`); the other tests here run without the pool, and two of these in one process
//! would count each other's servers.

mod clangd;
mod colour_roles;
mod frameworks;
mod html_css;
mod java_generate;
mod java_quick_fixes;
mod java_source_roots;
mod js_imports;
mod js_tools;
mod kotlin_structure;
mod move_file;
mod python_extract;
mod python_quick_fixes;
mod python_tools;
mod ruby;
mod rust_analyzer;
mod server_case;
mod servers;
mod side_by_side_f038;
mod spring;
mod ts_extract;
mod ts_structure;
mod ts_usages;
mod vue;

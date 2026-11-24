//! The integration tests of `ferris-idea-debugger` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod attach;
mod breakpoint_binding;
mod breakpoint_rules;
mod client_conditions;
mod condition_errors;
mod delve;
mod java_special;
mod jsdebug;
mod jvm_values;
mod kotlin;
mod kotlin_class_sources;
mod lifetime;
mod lldb;
mod method_exit;
mod native_memory;
mod python_more;

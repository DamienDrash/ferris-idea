//! CPU profiling for Ferris IDEA (`SPEC-0120`).
//!
//! A program is run once under a profiler and the result becomes one call tree with total and
//! self cost per function: native programs on macOS through `/usr/bin/sample` attached right after
//! the start (one sample per millisecond of run time), Python programs through the standard
//! library's `cProfile`, Java programs through JDK Flight Recorder (`SPEC-0136`). Nothing is
//! installed. Memory profiles (`SPEC-0250`), attaching to running processes (`SPEC-0251`) and
//! Linux `perf` plus Gradle `run` (`SPEC-0252`) use the same model.

mod attach;
mod cprofile;
mod jfr;
mod memory;
mod model;
mod perf;
mod project;
mod run;
mod sample;

pub use attach::{
    HEAP, MAX_ATTACH_SECONDS, ProcessInfo, attach, attach_plan, attachable, parse_heap, parse_ps,
    processes, reference_refusal,
};
pub use cprofile::{DUMP_SCRIPT, parse_cprofile};
pub use jfr::{is_java, jfr_tool, parse_jfr};
pub use memory::{
    ALL_ALLOCATIONS, TRACEMALLOC_RUNNER, parse_malloc_history, parse_massif, parse_tracemalloc,
    size_label,
};
pub use model::{HotSpot, LineCost, Profile, ProfileNode, StackFrame, StackTree, Unit, hot_spots};
pub use perf::{
    gradle_init_script, is_gradle_run, parse_perf_script, perf_blocked, perf_blocked_by,
};
pub use project::{SourceIndex, VIEW_FILE, ViewSettings, prepare};
pub use run::{
    Engine, Kind, LAST, LEAKS, MALLOC_HISTORY, PROFILES_DIR, ProfilePlan, ProfileRun,
    ProfileTarget, SAMPLE, SAMPLE_REFUSED, WINDOWS_REFUSED, can_profile, find_source, last_profile,
    plan, plan_kind, profile, profile_config, profile_config_kind, profile_kind,
};
pub use sample::parse_sample;

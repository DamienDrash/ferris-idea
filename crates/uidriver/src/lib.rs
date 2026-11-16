//! IDECraft's UI driver (`ADR-0027`, `SPEC-0319`), toolkit-free.
//!
//! - [`tree`]: the semantic tree in AccessKit's schema, the annotation description
//!   ([`tree::Semantics`]) and the bridge producer's builder for GPUI 0.2.2.
//! - [`locator`]: role and accessible name, test id, text and label locators, strict and lazy.
//! - [`action`]: actions on three input levels, planned as input events for the window.
//! - [`expect`]: assertions that retry until their deadline.
//! - [`engine`]: waits on presented frames, never on a timer; checks rights before input.
//! - [`security`]: who acts, effects and reserved elements, the emergency stop.
//! - [`trace`]: traces and audit entries without content.
//! - [`driver`]: the async Rust API for the self-test.
//!
//! The window (`crates/app`) feeds the engine with the tree of each presented frame and delivers
//! the input it plans; the MCP server (`crates/mcp`) and `idecraft drive` reach it only through the
//! window's loopback MCP server, so the driver opens no listener of its own.

pub mod action;
pub mod driver;
pub mod engine;
pub mod error;
pub mod expect;
pub mod locator;
pub mod security;
pub mod trace;
pub mod tree;

pub use accesskit::{Action as AxAction, Role};
pub use action::{Action, Button, Check, Input, KeyPress, Level, Modifier, parse_keys};
pub use driver::{Driver, UiLocator, expect};
pub use engine::{
    CancelHandle, Command, Done, Engine, FrameOutput, Keys, NoKeys, Op, Reply, Request, Target,
};
pub use error::{ErrorCode, UiError};
pub use expect::Matcher;
pub use locator::Locator;
pub use security::{Actor, Grants, StopSource};
pub use tree::{
    Effect, ElementId, ElementInfo, Rect, Reserved, Semantics, TextClass, Tree, TreeBuilder, View,
};

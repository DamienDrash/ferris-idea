//! User interface layer for Ferris IDEA. Toolkit decision: `docs/adr/0003-ui-toolkit.md`.
//!
//! Implements `SPEC-0028` (keymaps) and `SPEC-0060` (themes and editor colour schemes) and `SPEC-0167`
//! (tool window layout) and `SPEC-0294` (tooltips: registry, texts and the shared view), and embeds
//! the app icon (`brand`, ADR-0018). The views themselves live in `ferris-idea-workbench`.

pub mod brand;
pub mod icons;
pub mod keymap;
pub mod menus;
pub mod metrics;
pub mod theme;
pub mod tool_windows;
pub mod tooltip;
pub mod window_layouts;

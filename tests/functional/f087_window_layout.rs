//! F-087 · Window structure (toolbars, tool windows, status bar) and metrics (SPEC-0061, SPEC-0167).
//!
//! self-test: tool_window_steps, status_bar_steps, compact_mode_steps

use crate::harness::*;
use serde_json::json;

#[test]
fn tool_windows_have_sides_and_can_be_moved() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("tool_windows", json!({}));
    let project = r["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == "project")
        .unwrap();
    assert_eq!(project["side"], "left");
    let moved = ide.ok("tool_windows", json!({"move": "project", "side": "right"}));
    let project = moved["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == "project")
        .unwrap();
    assert_eq!(project["side"], "right", "{moved}");
    assert_eq!(project["default_side"], "left");
}

#[test]
fn the_chosen_theme_has_the_metrics_of_trees_tabs_and_menus() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let m = &ide.ok("themes", json!({}))["metrics"];
    for key in ["tree_row_height", "tab_height", "menu_item_height"] {
        assert!(m[key].as_f64().is_some_and(|v| v > 0.0), "{key}: {m}");
    }
}

//! F-002 · IDE and project settings: global and project settings, the project wins (SPEC-0002,
//! SPEC-0001 AK-3/AK-4, SPEC-0129).

use crate::harness::*;
use serde_json::json;

#[test]
fn project_settings_beat_the_defaults() {
    let p = Project::reference("python-basic");
    let out = p.cli(&["open", p.root.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("editor.tab_size = 4"));
    p.write(".ferris-idea/settings.toml", "[editor]\ntab_size = 2\n");
    let out = p.cli(&["open", p.root.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("workspace file loaded, editor.tab_size = 2"),
        "{text}"
    );
}

#[test]
fn invalid_project_settings_are_an_error_naming_the_problem() {
    let p = Project::reference("python-basic");
    p.write(".ferris-idea/settings.toml", "[editor\ntab_size = 2\n");
    let out = p.cli(&["open", p.root.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).is_empty());
}

#[test]
fn global_settings_live_in_the_users_folder_and_are_changed_there() {
    let p = Project::reference("python-basic");
    // The user's own settings are changed only with the `settings` right (SPEC-0053 v8).
    p.grant(&["settings"]);
    let mut ide = Ide::stdio(&p);
    let before = ide.ok("ide_settings", json!({}));
    assert!(
        text(&before["file"]).starts_with(&p.config.display().to_string()),
        "{before}"
    );
    ide.ok(
        "set_ide_settings",
        json!({"font_size": 17, "dry_run": false}),
    );
    let after = ide.ok("ide_settings", json!({}));
    assert_eq!(after["settings"]["editor"]["font_size"], json!(17.0));
    // Never inside the project.
    assert!(!p.path(".ferris-idea/settings.toml").exists());
    // Out of range is clamped as the dialog does (8–40).
    let clamped = ide.ok(
        "set_ide_settings",
        json!({"font_size": 99, "dry_run": false}),
    );
    assert_eq!(clamped["settings"]["editor"]["font_size"], json!(40.0));
}

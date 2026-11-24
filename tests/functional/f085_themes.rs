//! F-085 · Themes Darcula, Light, New UI, Islands: colours and surfaces as the reference
//! (SPEC-0060 AK-9, AK-10, ADR-0009, ADR-0018).
//!
//! self-test: theme_steps (popup ⌃`, switch to Light and back in a window)

use crate::harness::*;
use serde_json::json;

#[test]
fn bundled_themes_are_listed_chosen_and_unknown_names_refused() {
    let p = Project::empty();
    // The user's own settings are changed only with the `settings` right (SPEC-0053 v8).
    p.grant(&["settings"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("themes", json!({}));
    let names: Vec<String> = r["themes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| text(&t["name"]))
        .collect();
    for n in [
        "Ferris IDEA Dark",
        "Ferris IDEA Light",
        "Islands Dark",
        "Islands Light",
        "Dark",
        "Light",
    ] {
        assert!(names.contains(&n.to_owned()), "{r}");
    }
    // A new install draws with the brand theme (SPEC-0060 v6).
    assert_eq!(r["chosen"], "Ferris IDEA Dark", "{r}");
    let set = ide.ok("themes", json!({"set": "Light"}));
    assert_eq!(set["chosen"], "Light", "{set}");
    assert_eq!(ide.ok("themes", json!({}))["chosen"], "Light");
    ide.refused("themes", json!({"set": "Solarized From Mars"}));
}

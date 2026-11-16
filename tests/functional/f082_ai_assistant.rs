//! F-082 · AI assistance: optional, own choice of provider (SPEC-0138 AK-2).
//!
//! self-test: ai_steps (settings page, Explain Code against a fake endpoint)

use crate::harness::*;
use serde_json::json;

#[test]
fn off_and_without_provider_by_default_and_no_key_in_the_settings() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let ai = ide.ok("ai_assistant", json!({}));
    assert_eq!(ai["enabled"], false);
    assert_eq!(ai["endpoint"], "");
    assert_eq!(ai["ready"], false);
    let settings = ide.ok("ide_settings", json!({}));
    assert!(settings["settings"]["ai"].is_object(), "{settings}");
    assert!(
        !text(&settings).to_lowercase().contains("api_key"),
        "{settings}"
    );
}

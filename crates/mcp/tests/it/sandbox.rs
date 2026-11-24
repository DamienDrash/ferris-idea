//! SPEC-0347 AK-7: `sandbox_policy` shows the default policy of an agent, refuses a bad entry
//! and, on macOS, includes the Seatbelt profile. It needs no right.

use serde_json::json;

use super::protocol::{connect_exact, json_text};

#[tokio::test]
async fn sandbox_policy_shows_the_default_and_refuses_bad_entries() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let root = std::fs::canonicalize(&root).unwrap();
    let shown = json_text(&client.call("sandbox_policy", json!({})).await);
    assert_eq!(shown["valid"], true, "{shown}");
    assert_eq!(shown["default"], true);
    assert_eq!(shown["policy"]["write"], json!([root]));
    assert_eq!(shown["policy"]["read"], json!([]));
    assert_eq!(shown["policy"]["hosts"], json!([]));
    assert!(shown["fixed_locations"].as_array().is_some(), "{shown}");
    if cfg!(target_os = "macos") {
        assert_eq!(shown["mechanism"], "macos-seatbelt");
        let profile = shown["profile"].as_str().unwrap();
        assert!(profile.contains("(deny default)"), "{profile}");
        assert!(profile.contains(&format!("(subpath \"{}\")", root.display())));
        assert!(!profile.contains("(allow network"), "{profile}");
    }

    std::fs::create_dir_all(root.join("src")).unwrap();
    let bad = json_text(
        &client
            .call(
                "sandbox_policy",
                json!({"read": ["src"], "write": ["missing"], "hosts": ["*", "api.example.com"]}),
            )
            .await,
    );
    assert_eq!(bad["valid"], false, "{bad}");
    let entries: Vec<&str> = bad["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["entry"].as_str().unwrap())
        .collect();
    assert_eq!(entries.len(), 2, "{bad}");
    assert!(entries.contains(&"*"), "{bad}");
    assert!(entries.iter().any(|e| e.ends_with("missing")), "{bad}");
}

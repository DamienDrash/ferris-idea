//! The model tools (SPEC-0350 AK-5) against a fake provider on the loopback address.

use ferris_idea_core::Grant;
use serde_json::{Value, json};

use super::protocol::{connect_exact, error_message, is_error, json_text};

const SECRET: &str = "sk-ant-abcdefghijklmnopqrstuvwxyz0123";

fn no_secret(answer: &Value) {
    let text = answer.to_string();
    assert!(!text.contains(SECRET), "{text}");
}

/// SPEC-0350 AK-5.
#[tokio::test]
async fn model_tools() {
    let server = ferris_idea_ai::fake::start("unused", 200).unwrap();

    // Without rights: reading works, changing is refused, nothing is written.
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let empty = json_text(&client.call("model_providers", json!({})).await);
    assert_eq!(empty["providers"], json!([]));
    assert_eq!(empty["problems"], json!([]));
    let register_file = std::path::PathBuf::from(empty["register_file"].as_str().unwrap());
    assert!(
        !register_file.starts_with(&root),
        "{}",
        register_file.display()
    );
    let add = json!({"id": "srv", "kind": "openai", "endpoint": server.endpoint});
    let dry = json_text(&client.call("model_provider_set", add.clone()).await);
    assert_eq!(dry["dry_run"], json!(true));
    assert_eq!(dry["provider"]["key"], "none");
    let mut real = add.clone();
    real["dry_run"] = json!(false);
    let refused = client.call("model_provider_set", real.clone()).await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("settings"), "{refused}");
    assert!(!register_file.exists());
    // A key is never an argument; secret-looking values are refused without being repeated.
    let mut with_key = add.clone();
    with_key["key"] = json!(SECRET);
    let answer = client.call("model_provider_set", with_key).await;
    assert!(is_error(&answer));
    no_secret(&answer);
    let mut secret_model = add.clone();
    secret_model["models"] = json!([SECRET]);
    let answer = client.call("model_provider_set", secret_model).await;
    assert!(is_error(&answer));
    no_secret(&answer);
    let mut vault = add.clone();
    vault["key"] = json!("vault:main");
    assert!(error_message(&client.call("model_provider_set", vault).await).contains("M19.15"));

    // With `settings`: the register is written beside the grant file, mode 0600.
    let (mut client, root) = connect_exact(&[Grant::Settings], |s, _| s).await;
    let written = json_text(&client.call("model_provider_set", real.clone()).await);
    assert_eq!(written["dry_run"], json!(false));
    let register_file = std::path::PathBuf::from(written["register_file"].as_str().unwrap());
    assert!(register_file.exists() && !register_file.starts_with(&root));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&register_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let cloud = json!({"id": "cloud", "kind": "anthropic", "endpoint": "https://api.anthropic.com/v1", "key": "keychain", "dry_run": false});
    json_text(&client.call("model_provider_set", cloud).await);

    // `model_list` on loopback needs no right; on another machine it needs `http`.
    let listed = json_text(&client.call("model_list", json!({"provider": "srv"})).await);
    assert_eq!(listed["models"], json!(["model-a", "model-b"]));
    assert_eq!(listed["key_sent"], json!(false));
    assert_eq!(server.seen.lock().unwrap()[0].authorization, None);
    let remote = client
        .call("model_list", json!({"provider": "cloud"}))
        .await;
    assert!(is_error(&remote));
    assert!(error_message(&remote).contains("http"), "{remote}");

    // Detection asks loopback ports only and changes nothing.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let found = json_text(
        &client
            .call("model_detect_local", json!({"ports": [closed]}))
            .await,
    );
    assert_eq!(found["servers"], json!([]));

    // Price, assignment, booking and summary.
    json_text(
        &client
            .call(
                "model_price_set",
                json!({"provider": "srv", "model": "*", "input_per_mtok": "1", "output_per_mtok": "2", "dry_run": false}),
            )
            .await,
    );
    let assigned = json_text(
        &client
            .call(
                "model_assign",
                json!({"task": "default", "provider": "srv", "model": "model-a", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(assigned["now_applies"]["model"], "model-a");
    let changed = json_text(
        &client
            .call(
                "model_assign",
                json!({"task": "default", "provider": "srv", "model": "model-b", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(changed["previous"]["model"], "model-a");
    let removing = client
        .call("model_provider_set", json!({"id": "srv", "remove": true}))
        .await;
    assert!(error_message(&removing).contains("default"), "{removing}");
    let dry_booking = json_text(
        &client
            .call(
                "model_usage_record",
                json!({"provider": "srv", "model": "model-b", "input_tokens": 1000000, "output_tokens": 500000}),
            )
            .await,
    );
    assert_eq!(dry_booking["cost"], "2.000000");
    let booked = json_text(
        &client
            .call(
                "model_usage_record",
                json!({"provider": "cloud", "model": "x", "input_tokens": 10, "output_tokens": 5, "dry_run": false}),
            )
            .await,
    );
    assert_eq!(booked["record"]["cost_micro"], Value::Null);
    assert!(
        booked["record"]["key"]
            .as_str()
            .unwrap()
            .starts_with("keychain:")
    );
    json_text(
        &client
            .call(
                "model_usage_record",
                json!({"provider": "srv", "model": "model-b", "input_tokens": 1000000, "output_tokens": 500000, "dry_run": false}),
            )
            .await,
    );
    let usage = json_text(&client.call("model_usage", json!({})).await);
    assert_eq!(usage["records"], json!(2));
    let groups = usage["groups"].as_array().unwrap();
    assert_eq!(groups[0]["group"], "cloud");
    assert_eq!(groups[0]["unpriced_calls"], json!(1));
    assert_eq!(groups[1]["cost"]["USD"], "2.000000");
    let all = json_text(&client.call("model_providers", json!({})).await);
    assert_eq!(all["assignments"][0]["previous"][0]["model"], "model-a");
    no_secret(&all);
    let _ = std::fs::remove_file(&register_file);
    let _ = std::fs::remove_file(usage["ledger_file"].as_str().unwrap());
}

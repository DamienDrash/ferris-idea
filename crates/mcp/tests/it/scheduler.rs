//! SPEC-0342 AK-8: `schedule_list`, `schedule_add`, `schedule_remove` and `schedule_configure`
//! over MCP — dry runs by default, writing needs the `settings` right, a task never gets a right
//! the grant file does not hold, and bounds, cycles and dependents are refused.

use ferris_idea_core::Grant;
use ferris_idea_core::scheduler::{load_schedule, schedule_files};
use serde_json::json;

use super::protocol::{connect_exact, error_message, is_error, json_text};

fn config_file(root: &std::path::Path) -> std::path::PathBuf {
    schedule_files(&root.with_extension("agent-permissions.toml")).0
}

fn nightly() -> serde_json::Value {
    json!({
        "id": "nightly",
        "prompt": "Review yesterday's changes and list problems.",
        "daily": "02:30",
        "budget_cents": 200,
        "max_minutes": 60,
        "rights": ["run"],
    })
}

fn with(mut v: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

#[tokio::test]
async fn schedule_list_shows_the_empty_schedule_and_its_bounds() {
    let (mut client, _root) = connect_exact(&[], |s, _| s).await;
    let list = json_text(&client.call("schedule_list", json!({})).await);
    assert_eq!(list["enabled"], false, "{list}");
    assert_eq!(list["tasks"], json!([]));
    assert_eq!(list["queue"], json!([]));
    assert_eq!(list["problems"], json!([]));
    assert_eq!(list["config"]["max_parallel"], 1);
    assert_eq!(list["bounds"]["budget_cents"], json!([1, 10_000]));
    assert!(list["host"].as_str().unwrap().contains("M19.5"));
    assert!(
        list["config_file"]
            .as_str()
            .unwrap()
            .ends_with(".agent-schedule.toml"),
        "{list}"
    );
}

#[tokio::test]
async fn schedule_add_needs_the_settings_right_and_rights_the_creator_holds() {
    // Without `settings`: the dry run answers, writing is refused, nothing is written.
    let (mut client, root) = connect_exact(&[Grant::Run], |s, _| s).await;
    let dry = json_text(&client.call("schedule_add", nightly()).await);
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["task"]["created_by"], "mcp");
    assert_eq!(dry["rights_not_granted"], json!([]));
    let refused = client
        .call("schedule_add", with(nightly(), json!({"dry_run": false})))
        .await;
    assert!(is_error(&refused));
    assert!(
        error_message(&refused).starts_with("not granted:"),
        "{refused}"
    );
    assert!(!config_file(&root).exists());

    // With `settings` but not `run`: a task with `run` is refused.
    let (mut client, root) = connect_exact(&[Grant::Settings], |s, _| s).await;
    let dry = json_text(&client.call("schedule_add", nightly()).await);
    assert_eq!(dry["rights_not_granted"], json!(["run"]));
    let refused = client
        .call("schedule_add", with(nightly(), json!({"dry_run": false})))
        .await;
    assert!(
        error_message(&refused).contains("does not grant `run`"),
        "{refused}"
    );
    assert!(!config_file(&root).exists());

    // With both: written to the user's settings folder.
    let (mut client, root) = connect_exact(&[Grant::Settings, Grant::Run], |s, _| s).await;
    let added = json_text(
        &client
            .call("schedule_add", with(nightly(), json!({"dry_run": false})))
            .await,
    );
    assert_eq!(added["dry_run"], false, "{added}");
    let loaded = load_schedule(&config_file(&root), &root);
    assert!(loaded.problem.is_none(), "{:?}", loaded.problem);
    let t = &loaded.schedule.task[0];
    assert_eq!(t.id, "nightly");
    assert_eq!(t.rights, [Grant::Run].into());
    // The same id again needs `replace`.
    let again = client
        .call("schedule_add", with(nightly(), json!({"dry_run": false})))
        .await;
    assert!(error_message(&again).contains("replace"), "{again}");
    // Bounds, triggers, unknown dependencies and unknown rights are refused.
    for (extra, says) in [
        (json!({"budget_cents": 0}), "budget_cents"),
        (json!({"max_minutes": 5000}), "max_minutes"),
        (json!({"daily": null, "every": 60}), "every"),
        (json!({"at": 5}), "exactly one trigger"),
        (json!({"daily": null, "event": "bell"}), "no event kind"),
        (json!({"id": "x", "after": ["ghost"]}), "ghost"),
        (json!({"id": "x", "rights": ["root"]}), "no right"),
        (json!({"id": "x", "prompt": ""}), "prompt"),
        (json!({"id": "x", "wake": true}), "no argument `wake`"),
    ] {
        let r = client
            .call(
                "schedule_add",
                with(with(nightly(), json!({"replace": true})), extra.clone()),
            )
            .await;
        assert!(is_error(&r), "{extra}: {r}");
        assert!(error_message(&r).contains(says), "{extra}: {r}");
    }
    // A dependent task, then removing its dependency is refused, the dependent can go.
    let on_ci = json!({
        "id": "on-ci", "prompt": "Look at the failed CI run.", "event": "ci_failed",
        "filter": "main", "after": ["nightly"], "budget_cents": 50, "max_minutes": 20,
        "resources": ["machine", "simulator"], "dry_run": false,
    });
    json_text(&client.call("schedule_add", on_ci).await);
    let refused = client
        .call(
            "schedule_remove",
            json!({"id": "nightly", "dry_run": false}),
        )
        .await;
    assert!(error_message(&refused).contains("`on-ci`"), "{refused}");
    let dry = json_text(&client.call("schedule_remove", json!({"id": "on-ci"})).await);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(
        load_schedule(&config_file(&root), &root)
            .schedule
            .task
            .len(),
        2
    );
    json_text(
        &client
            .call("schedule_remove", json!({"id": "on-ci", "dry_run": false}))
            .await,
    );
    let list = json_text(&client.call("schedule_list", json!({})).await);
    assert_eq!(list["tasks"].as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list["tasks"][0]["prompt"], nightly()["prompt"]);
    let missing = client.call("schedule_remove", json!({"id": "ghost"})).await;
    assert!(error_message(&missing).contains("no task"), "{missing}");
}

#[tokio::test]
async fn schedule_configure_sets_quiet_hours_and_hard_caps() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    let args = json!({
        "enabled": true, "quiet_from": "22:00", "quiet_to": "07:00", "max_parallel": 2,
        "limits": {"simulator": 2, "database:pg": 1},
    });
    let dry = json_text(&client.call("schedule_configure", args.clone()).await);
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["changed"], true);
    assert_eq!(dry["config"]["quiet_hours"]["from"], "22:00");
    let refused = client
        .call(
            "schedule_configure",
            with(args.clone(), json!({"dry_run": false})),
        )
        .await;
    assert!(
        error_message(&refused).starts_with("not granted:"),
        "{refused}"
    );
    for (extra, says) in [
        (json!({"max_parallel": 9}), "max_parallel"),
        (json!({"quiet_to": null}), "both"),
        (json!({"quiet_to": "22:00"}), "same time"),
        (json!({"limits": {"simulator": 0}}), "limits.simulator"),
        (json!({"no_quiet_hours": true}), "contradict"),
    ] {
        let r = client
            .call("schedule_configure", with(args.clone(), extra.clone()))
            .await;
        assert!(error_message(&r).contains(says), "{extra}: {r}");
    }
    // With the right (a server of its own on a workspace of its own).
    let (mut client2, root2) = connect_exact(&[Grant::Settings], |s, _| s).await;
    json_text(
        &client2
            .call("schedule_configure", with(args, json!({"dry_run": false})))
            .await,
    );
    let s = load_schedule(&config_file(&root2), &root2).schedule;
    assert!(s.enabled);
    assert_eq!(s.max_parallel, 2);
    assert_eq!(s.limits["simulator"], 2);
    assert!(!config_file(&root).exists());
    let list = json_text(&client2.call("schedule_list", json!({})).await);
    assert_eq!(list["enabled"], true, "{list}");
    assert_eq!(list["config"]["quiet_hours"]["to"], "07:00");
}

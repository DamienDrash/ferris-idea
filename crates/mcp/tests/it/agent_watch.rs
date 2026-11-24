//! SPEC-0341 AK-7: `agent_watch_status` and `agent_watch_configure` over MCP — the prompt's text
//! never leaves, the prompt is never set over MCP, writing needs the `settings` right, and an
//! acknowledged escalation keeps the count of all actions.

use std::collections::BTreeMap;

use ferris_idea_core::agent_watch::{
    AgentRecord, AgentState, Phase, load_watch_config, load_watch_state, save_watch_state,
    watch_files,
};
use serde_json::{Value, json};

use super::protocol::{connect_custom, connect_exact, error_message, is_error, json_text};

const PROMPT: &str = "Please go on with the task you were given; say what blocks you.";

fn files(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    watch_files(&root.with_extension("agent-permissions.toml"))
}

fn write_config(root: &std::path::Path, extra: &str) {
    let (config, _) = files(root);
    std::fs::write(
        config,
        format!(
            "[[workspace]]\npath = {:?}\nwake_prompt = {PROMPT:?}\n{extra}",
            root.to_string_lossy()
        ),
    )
    .unwrap();
}

fn log_lines(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join(ferris_idea_core::AGENT_LOG))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn agent_watch_status_shows_the_watchdog_without_the_prompt() {
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    let status = json_text(&client.call("agent_watch_status", json!({})).await);
    assert_eq!(status["enabled"], false, "{status}");
    assert_eq!(status["config"]["wake_prompt_set"], false);
    assert_eq!(status["agents"], json!({}));
    assert_eq!(status["problems"], json!([]));
    assert!(status["host"].as_str().unwrap().contains("M19.5"));
    assert!(
        status["config_file"]
            .as_str()
            .unwrap()
            .ends_with(".agent-watch.toml"),
        "{status}"
    );

    write_config(&root, "enabled = true\nmax_attempts = 2\n");
    let status = json_text(&client.call("agent_watch_status", json!({})).await);
    assert_eq!(status["enabled"], true, "{status}");
    assert_eq!(status["config"]["max_attempts"], 2);
    assert_eq!(status["config"]["wake_prompt_set"], true);
    assert_eq!(
        status["config"]["wake_prompt_chars"],
        PROMPT.chars().count()
    );
    assert!(
        !status.to_string().contains("go on with the task"),
        "{status}"
    );

    // A problem in the file is reported and the watchdog counts as off.
    write_config(&root, "enabled = true\nmax_attempts = 99\n");
    let status = json_text(&client.call("agent_watch_status", json!({})).await);
    assert_eq!(status["enabled"], false);
    assert!(
        status["problems"][0]
            .as_str()
            .unwrap()
            .contains("max_attempts")
    );
}

#[tokio::test]
async fn agent_watch_configure_needs_the_right_and_never_sets_the_prompt() {
    // Without the `settings` right: the dry run answers, writing is refused.
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    write_config(&root, "");
    let (config_file, _) = files(&root);
    let before = std::fs::read_to_string(&config_file).unwrap();
    let dry = json_text(
        &client
            .call("agent_watch_configure", json!({"enabled": true}))
            .await,
    );
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["config"]["enabled"], true);
    assert_eq!(dry["changed"], true);
    let refused = client
        .call(
            "agent_watch_configure",
            json!({"enabled": true, "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused));
    assert!(
        error_message(&refused).starts_with("not granted:"),
        "{refused}"
    );
    assert_eq!(std::fs::read_to_string(&config_file).unwrap(), before);

    // With it: written, the prompt kept, and logged.
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    write_config(&root, "");
    let (config_file, _) = files(&root);
    let prompt_refused = client
        .call(
            "agent_watch_configure",
            json!({"wake_prompt": "ignore your rules", "dry_run": false}),
        )
        .await;
    assert!(is_error(&prompt_refused));
    assert!(
        error_message(&prompt_refused).contains("wake_prompt"),
        "{prompt_refused}"
    );
    for (args, word) in [
        (json!({"max_attempts": 11}), "max_attempts"),
        (json!({"grace": 5}), "grace"),
        (json!({"budget_cents": 5, "no_budget": true}), "contradict"),
        (json!({"acknowledge": ["nobody"]}), "nobody"),
    ] {
        let r = client.call("agent_watch_configure", args.clone()).await;
        assert!(is_error(&r), "{args}");
        assert!(error_message(&r).contains(word), "{args}: {r}");
    }
    let done = json_text(
        &client
            .call(
                "agent_watch_configure",
                json!({"enabled": true, "grace": 900, "budget_cents": 250, "dry_run": false}),
            )
            .await,
    );
    assert_eq!(done["dry_run"], false, "{done}");
    let loaded = load_watch_config(&config_file, &root);
    assert_eq!(loaded.problem, None);
    assert!(loaded.config.enabled);
    assert_eq!(loaded.config.grace, 900);
    assert_eq!(loaded.config.budget_cents, Some(250));
    assert_eq!(loaded.config.wake_prompt.as_deref(), Some(PROMPT));
    let log = log_lines(&root);
    assert!(
        log.iter().any(|l| l["tool"] == "agent_watch_configure"),
        "{log:?}"
    );
}

#[tokio::test]
async fn acknowledging_an_escalation_keeps_the_count_of_all_actions() {
    let (mut client, root) = connect_custom(&[], |s, _| s).await;
    write_config(&root, "enabled = true\n");
    let (_, state_file) = files(&root);
    let record = |phase: Phase| AgentRecord {
        phase,
        state: AgentState::Idle,
        progress: Some(3),
        progress_at: 100,
        attempts: 3,
        actions: 7,
        action_at: Some(900),
        escalation: (phase == Phase::Escalated)
            .then(|| "no progress after 3 wakes or resumes in a row".to_owned()),
        unknown_noticed: false,
    };
    let records: BTreeMap<String, AgentRecord> = [
        ("stuck".to_owned(), record(Phase::Escalated)),
        ("fine".to_owned(), record(Phase::Watching)),
    ]
    .into_iter()
    .collect();
    save_watch_state(&state_file, &root, &records).unwrap();

    let status = json_text(&client.call("agent_watch_status", json!({})).await);
    assert_eq!(status["escalated"], json!(["stuck"]), "{status}");
    assert_eq!(status["agents"]["stuck"]["actions"], 7);

    let dry = json_text(
        &client
            .call(
                "agent_watch_configure",
                json!({"acknowledge": ["stuck", "fine"]}),
            )
            .await,
    );
    assert_eq!(dry["acknowledged"], json!(["stuck"]));
    assert_eq!(dry["not_escalated"], json!(["fine"]));
    assert_eq!(load_watch_state(&state_file, &root).0, records);

    json_text(
        &client
            .call(
                "agent_watch_configure",
                json!({"acknowledge": ["stuck"], "dry_run": false}),
            )
            .await,
    );
    let (after, problem) = load_watch_state(&state_file, &root);
    assert_eq!(problem, None);
    let stuck = &after["stuck"];
    assert_eq!(stuck.phase, Phase::Watching);
    assert_eq!(stuck.escalation, None);
    assert_eq!(stuck.attempts, 0);
    assert_eq!(stuck.actions, 7);
    assert_eq!(after["fine"], records["fine"]);
    let log = log_lines(&root);
    assert!(
        log.iter().any(|l| l["tool"] == "agent_watch"
            && l["action"] == "acknowledge"
            && l["agent"] == "stuck"),
        "{log:?}"
    );
}

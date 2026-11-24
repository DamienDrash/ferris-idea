//! SPEC-0341 AK-1 to AK-6: the agent watchdog with a fake host and a fake clock — no sleeping,
//! every decision deterministic.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use ferris_idea_core::Grant;
use ferris_idea_core::agent_watch::{
    AgentHost, AgentState, Clock, Liveness, Notice, Observation, Phase, SavedSession, WatchConfig,
    WatchLog, Watchdog, load_watch_config, load_watch_state, save_watch_config, save_watch_state,
    watch_files,
};
use serde_json::Value;

const PROMPT: &str = "Continue with your task. If you are stuck, say what you need.";

struct FakeClock(Cell<u64>);

impl FakeClock {
    fn at(t: u64) -> Self {
        Self(Cell::new(t))
    }
    fn advance(&self, secs: u64) {
        self.0.set(self.0.get() + secs);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> u64 {
        self.0.get()
    }
}

#[derive(Default)]
struct FakeAgent {
    obs: Option<Observation>,
    session: Option<SavedSession>,
    wake_fails: bool,
}

#[derive(Default)]
struct FakeHost {
    agents: BTreeMap<String, FakeAgent>,
    wakes: Vec<(String, String)>,
    resumes: Vec<(String, String, BTreeSet<Grant>)>,
    notices: Vec<Notice>,
}

impl FakeHost {
    fn with(agents: &[(&str, AgentState)]) -> Self {
        let mut h = Self::default();
        for (id, state) in agents {
            h.set(id, *state, Liveness::Alive, Some(0));
        }
        h
    }
    fn set(&mut self, id: &str, state: AgentState, process: Liveness, progress: Option<u64>) {
        let a = self.agents.entry(id.to_owned()).or_default();
        a.obs = Some(Observation {
            state,
            process,
            progress,
            spent_cents: Some(0),
        });
    }
    fn obs(&mut self, id: &str) -> &mut Observation {
        self.agents.get_mut(id).unwrap().obs.as_mut().unwrap()
    }
}

impl AgentHost for FakeHost {
    fn agents(&self) -> Vec<String> {
        self.agents.keys().cloned().collect()
    }
    fn observe(&mut self, agent: &str) -> Observation {
        self.agents[agent].obs.clone().unwrap()
    }
    fn saved_session(&self, agent: &str) -> Option<SavedSession> {
        self.agents[agent].session.clone()
    }
    fn wake(&mut self, agent: &str, prompt: &str) -> Result<(), String> {
        if self.agents[agent].wake_fails {
            return Err("the agent did not take the prompt".into());
        }
        self.wakes.push((agent.to_owned(), prompt.to_owned()));
        Ok(())
    }
    fn resume(
        &mut self,
        agent: &str,
        session: &str,
        rights: &BTreeSet<Grant>,
    ) -> Result<(), String> {
        self.resumes
            .push((agent.to_owned(), session.to_owned(), rights.clone()));
        Ok(())
    }
    fn notify(&mut self, notice: &Notice) {
        self.notices.push(notice.clone());
    }
}

#[derive(Default)]
struct FakeLog {
    lines: Vec<Value>,
    broken: bool,
}

impl WatchLog for FakeLog {
    fn record(&mut self, line: &Value) -> Result<(), String> {
        if self.broken {
            return Err("disk full".into());
        }
        self.lines.push(line.clone());
        Ok(())
    }
}

fn config() -> WatchConfig {
    WatchConfig {
        enabled: true,
        wake_prompt: Some(PROMPT.into()),
        stall_after: 600,
        idle_after: 300,
        grace: 120,
        unknown_after: 900,
        max_attempts: 3,
        max_actions: 10,
        budget_cents: None,
        resume: true,
    }
}

fn dog(config: WatchConfig) -> Watchdog {
    Watchdog::new(config, BTreeSet::new(), BTreeMap::new())
}

fn actions(log: &FakeLog) -> Vec<String> {
    log.lines
        .iter()
        .map(|l| {
            format!(
                "{}:{}",
                l["agent"].as_str().unwrap(),
                l["action"].as_str().unwrap()
            )
        })
        .collect()
}

/// AK-1: hung and asleep agents are woken once, judged again only after the grace time, and
/// progress sets the attempts back.
#[test]
fn hung_and_sleeping_agents_are_woken_with_the_prompt() {
    let clock = FakeClock::at(1_000);
    let mut host = FakeHost::with(&[("busy", AgentState::Working), ("nap", AgentState::Idle)]);
    let mut log = FakeLog::default();
    let mut w = dog(config());
    assert!(w.tick(&mut host, &clock, &mut log).is_empty());

    // 299 s: nobody yet. 300 s: the idle one is asleep.
    clock.advance(299);
    assert!(w.tick(&mut host, &clock, &mut log).is_empty());
    clock.advance(1);
    let ev = w.tick(&mut host, &clock, &mut log);
    assert_eq!(ev.len(), 1);
    assert_eq!((ev[0].agent.as_str(), ev[0].action), ("nap", "wake"));
    assert_eq!(host.wakes, vec![("nap".to_owned(), PROMPT.to_owned())]);
    assert_eq!(w.records["nap"].phase, Phase::Woken);

    // Within the grace time nothing more for it; at 600 s the working one is hung.
    clock.advance(119);
    assert!(w.tick(&mut host, &clock, &mut log).is_empty());
    clock.advance(181);
    let ev = w.tick(&mut host, &clock, &mut log);
    let woken: Vec<_> = ev.iter().map(|e| e.agent.as_str()).collect();
    assert_eq!(woken, ["busy", "nap"], "{ev:?}");
    assert_eq!(w.records["nap"].attempts, 2);

    // Progress sets the attempts back and starts the clock again.
    host.obs("nap").progress = Some(5);
    host.obs("busy").progress = Some(1);
    clock.advance(60);
    assert!(w.tick(&mut host, &clock, &mut log).is_empty());
    assert_eq!(w.records["nap"].attempts, 0);
    assert_eq!(w.records["nap"].actions, 2);
    assert_eq!(w.records["nap"].phase, Phase::Woken);
    clock.advance(240);
    assert!(w.tick(&mut host, &clock, &mut log).is_empty());
    clock.advance(60);
    let ev = w.tick(&mut host, &clock, &mut log);
    assert!(ev.iter().any(|e| e.agent == "nap" && e.action == "wake"));
}

/// AK-2: agents waiting for the person, unknown, done or failed are never woken; unknown gets one
/// notice.
#[test]
fn waiting_unknown_and_ended_agents_are_never_woken() {
    let clock = FakeClock::at(0);
    let mut host = FakeHost::with(&[
        ("approval", AgentState::WaitingForApproval),
        ("input", AgentState::WaitingForInput),
        ("unknown", AgentState::Unknown),
        ("done", AgentState::Done),
        ("failed", AgentState::Failed),
    ]);
    let mut log = FakeLog::default();
    let mut w = dog(config());
    w.tick(&mut host, &clock, &mut log);
    let mut notices = 0;
    for _ in 0..50 {
        clock.advance(600);
        notices += w
            .tick(&mut host, &clock, &mut log)
            .iter()
            .filter(|e| e.action == "notice")
            .count();
    }
    assert!(host.wakes.is_empty(), "{:?}", host.wakes);
    assert!(host.resumes.is_empty());
    assert_eq!(notices, 1);
    assert_eq!(host.notices.len(), 1);
    assert_eq!(host.notices[0].agent, "unknown");
    assert_eq!(host.notices[0].kind, "unknown_state");
    assert_eq!(actions(&log), ["unknown:notice"]);
    assert_eq!(w.records["approval"].phase, Phase::NeedsPerson);
    assert_eq!(w.records["input"].phase, Phase::NeedsPerson);
    assert_eq!(w.records["unknown"].phase, Phase::Unknown);
    assert_eq!(w.records["done"].phase, Phase::Ended);
    assert_eq!(w.records["failed"].phase, Phase::Ended);

    // A change of state allows one more notice later.
    host.obs("unknown").state = AgentState::Working;
    w.tick(&mut host, &clock, &mut log);
    host.obs("unknown").state = AgentState::Unknown;
    w.tick(&mut host, &clock, &mut log);
    clock.advance(900);
    w.tick(&mut host, &clock, &mut log);
    assert_eq!(host.notices.len(), 2);

    // Switched off, nothing happens at all.
    let mut off = dog(WatchConfig {
        enabled: false,
        ..config()
    });
    let mut idle = FakeHost::with(&[("a", AgentState::Idle)]);
    for _ in 0..5 {
        clock.advance(10_000);
        assert!(off.tick(&mut idle, &clock, &mut log).is_empty());
    }
    assert!(off.records.is_empty());
}

/// AK-3: a dead agent is resumed with no more rights than it had and the workspace grants now;
/// otherwise it is escalated. Unknown liveness is not gone.
#[test]
fn dead_agents_are_resumed_without_wider_rights() {
    let clock = FakeClock::at(0);
    let mut host = FakeHost::with(&[
        ("dead", AgentState::Working),
        ("nosession", AgentState::Working),
        ("maybe", AgentState::Working),
    ]);
    host.agents.get_mut("dead").unwrap().session = Some(SavedSession {
        session: "s-1".into(),
        rights: [Grant::Write, Grant::Run, Grant::Http]
            .into_iter()
            .collect(),
        resumable: true,
    });
    host.agents.get_mut("nosession").unwrap().session = Some(SavedSession {
        session: "s-2".into(),
        rights: BTreeSet::new(),
        resumable: false,
    });
    let mut log = FakeLog::default();
    let grants: BTreeSet<Grant> = [Grant::Write, Grant::Run, Grant::Ssh].into_iter().collect();
    let mut w = Watchdog::new(config(), grants, BTreeMap::new());
    w.tick(&mut host, &clock, &mut log);
    host.obs("dead").process = Liveness::Gone;
    host.obs("nosession").process = Liveness::Gone;
    host.obs("maybe").process = Liveness::Unknown;
    clock.advance(10);
    let ev = w.tick(&mut host, &clock, &mut log);
    assert_eq!(
        ev.iter()
            .map(|e| format!("{}:{}", e.agent, e.action))
            .collect::<Vec<_>>(),
        ["dead:resume", "nosession:escalate"]
    );
    let expected: BTreeSet<Grant> = [Grant::Run, Grant::Write].into_iter().collect();
    assert_eq!(host.resumes, vec![("dead".into(), "s-1".into(), expected)]);
    let line = &log.lines[0];
    assert_eq!(line["action"], "resume");
    assert_eq!(line["rights"], serde_json::json!(["run", "write"]));
    assert_eq!(w.records["maybe"].phase, Phase::Watching);

    // Still gone within the grace time: no second resume; after it, one more.
    clock.advance(60);
    assert!(w.tick(&mut host, &clock, &mut log).is_empty());
    clock.advance(60);
    let ev = w.tick(&mut host, &clock, &mut log);
    assert_eq!(ev.len(), 1);
    assert_eq!(host.resumes.len(), 2);

    // Resuming off: escalated.
    let mut off = Watchdog::new(
        WatchConfig {
            resume: false,
            ..config()
        },
        BTreeSet::new(),
        BTreeMap::new(),
    );
    let mut h = FakeHost::with(&[("x", AgentState::Idle)]);
    h.obs("x").process = Liveness::Gone;
    let ev = off.tick(&mut h, &clock, &mut log);
    assert_eq!(ev[0].action, "escalate");
    assert!(ev[0].reason.contains("resuming is off"), "{ev:?}");
}

/// AK-4: limits escalate once and then stop; acknowledging starts the attempts again but not
/// the count of all actions; budget and a missing prompt escalate.
#[test]
fn limits_escalate_once_and_stay_bounded() {
    let clock = FakeClock::at(0);
    let mut host = FakeHost::with(&[("stuck", AgentState::Idle)]);
    let mut log = FakeLog::default();
    let mut w = dog(WatchConfig {
        max_actions: 5,
        ..config()
    });
    let mut all = Vec::new();
    for _ in 0..40 {
        clock.advance(300);
        all.extend(w.tick(&mut host, &clock, &mut log));
    }
    let kinds: Vec<_> = all.iter().map(|e| e.action).collect();
    assert_eq!(kinds, ["wake", "wake", "wake", "escalate"]);
    assert_eq!(host.notices.len(), 1);
    assert_eq!(host.notices[0].kind, "escalated");
    assert!(
        all[3].reason.contains("3 wakes or resumes in a row"),
        "{all:?}"
    );
    assert_eq!(w.records["stuck"].phase, Phase::Escalated);

    // Acknowledged: two more wakes reach max_actions = 5, then escalated again for good.
    assert!(w.acknowledge("stuck", clock.now(), &mut log).unwrap());
    assert!(!w.acknowledge("stuck", clock.now(), &mut log).unwrap());
    let mut more = Vec::new();
    for _ in 0..40 {
        clock.advance(300);
        more.extend(w.tick(&mut host, &clock, &mut log));
    }
    let kinds: Vec<_> = more.iter().map(|e| e.action).collect();
    assert_eq!(kinds, ["wake", "wake", "escalate"]);
    assert!(
        more[2].reason.contains("5 wakes or resumes in all"),
        "{more:?}"
    );
    assert_eq!(host.wakes.len(), 5);
    assert_eq!(w.records["stuck"].actions, 5);

    // Budget spent, spending not reported, no prompt: each escalates instead of waking.
    for (cfg, spent, why) in [
        (
            WatchConfig {
                budget_cents: Some(100),
                ..config()
            },
            Some(100),
            "budget of 100 cents is spent",
        ),
        (
            WatchConfig {
                budget_cents: Some(100),
                ..config()
            },
            None,
            "spending is not reported",
        ),
        (
            WatchConfig {
                wake_prompt: None,
                ..config()
            },
            Some(0),
            "no wake prompt",
        ),
    ] {
        let mut h = FakeHost::with(&[("a", AgentState::Working)]);
        h.obs("a").spent_cents = spent;
        let mut w = dog(cfg);
        w.tick(&mut h, &clock, &mut log);
        clock.advance(600);
        let ev = w.tick(&mut h, &clock, &mut log);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].action, "escalate");
        assert!(ev[0].reason.contains(why), "{ev:?}");
        assert!(h.wakes.is_empty());
    }

    // A wake the host refuses escalates with its reason.
    let mut h = FakeHost::with(&[("a", AgentState::Idle)]);
    h.agents.get_mut("a").unwrap().wake_fails = true;
    let mut w = dog(config());
    w.tick(&mut h, &clock, &mut log);
    clock.advance(300);
    let ev = w.tick(&mut h, &clock, &mut log);
    assert_eq!(ev[0].action, "escalate");
    assert!(ev[0].reason.contains("did not take the prompt"), "{ev:?}");
}

/// AK-5: nothing is sent unlogged; the lines carry the prompt's length, never its text.
#[test]
fn every_action_is_logged_first_and_without_the_prompt() {
    let clock = FakeClock::at(0);
    let mut host = FakeHost::with(&[("a", AgentState::Idle)]);
    let mut log = FakeLog {
        broken: true,
        ..FakeLog::default()
    };
    let mut w = dog(config());
    w.tick(&mut host, &clock, &mut log);
    clock.advance(300);
    let ev = w.tick(&mut host, &clock, &mut log);
    assert!(host.wakes.is_empty());
    assert_eq!(ev[0].action, "escalate");
    assert!(
        ev[0].reason.contains("agent log cannot be written"),
        "{ev:?}"
    );

    let mut host = FakeHost::with(&[("a", AgentState::Idle)]);
    let mut log = FakeLog::default();
    let mut w = dog(WatchConfig {
        max_attempts: 1,
        ..config()
    });
    w.tick(&mut host, &clock, &mut log);
    clock.advance(300);
    w.tick(&mut host, &clock, &mut log);
    clock.advance(300);
    w.tick(&mut host, &clock, &mut log);
    w.acknowledge("a", clock.now(), &mut log).unwrap();
    assert_eq!(actions(&log), ["a:wake", "a:escalate", "a:acknowledge"]);
    let wake = &log.lines[0];
    assert_eq!(wake["tool"], "agent_watch");
    assert_eq!(wake["prompt_chars"], PROMPT.chars().count());
    assert_eq!(wake["time"], 600);
    let text = serde_json::to_string(&log.lines).unwrap();
    assert!(!text.contains("Continue with your task"), "{text}");
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-agent-watch-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ws")).unwrap();
    std::fs::create_dir_all(dir.join("other")).unwrap();
    dir
}

/// AK-6: the configuration file, its bounds and its place; the state survives a restart.
#[test]
fn configuration_and_state_survive_and_are_checked() {
    let dir = temp("config");
    let ws = dir.join("ws");
    let (file, state) = watch_files(&dir.join("agent-permissions.toml"));
    assert_eq!(file, dir.join("agent-watch.toml"));
    assert_eq!(state, dir.join("agent-watch-state.json"));
    assert_eq!(
        watch_files(&dir.join("t-1.agent-permissions.toml")).0,
        dir.join("t-1.agent-watch.toml")
    );

    // Nothing configured: off, no problem.
    let loaded = load_watch_config(&file, &ws);
    assert_eq!(loaded.config, WatchConfig::default());
    assert!(!loaded.config.enabled);
    assert_eq!(loaded.problem, None);

    std::fs::write(
        &file,
        format!(
            "[[workspace]]\npath = {:?}\nenabled = true\nwake_prompt = \"Go on.\"\nmax_attempts = 2\n\n[[workspace]]\npath = {:?}\nenabled = false\n",
            ws.to_string_lossy(),
            dir.join("other").to_string_lossy()
        ),
    )
    .unwrap();
    let loaded = load_watch_config(&file, &ws);
    assert_eq!(loaded.problem, None);
    assert!(loaded.config.enabled);
    assert_eq!(loaded.config.wake_prompt.as_deref(), Some("Go on."));
    assert_eq!(loaded.config.max_attempts, 2);
    assert_eq!(loaded.config.stall_after, 600);

    // Saving keeps the other workspace's table.
    let mut changed = loaded.config.clone();
    changed.grace = 900;
    save_watch_config(&file, &ws, &changed).unwrap();
    assert_eq!(load_watch_config(&file, &ws).config.grace, 900);
    assert!(!load_watch_config(&file, &dir.join("other")).config.enabled);
    assert!(
        save_watch_config(
            &file,
            &ws,
            &WatchConfig {
                max_actions: 51,
                ..changed.clone()
            }
        )
        .unwrap_err()
        .contains("max_actions")
    );

    // Out of bounds, unknown keys, a bad prompt: a problem, and the watchdog stays off.
    for bad in [
        "stall_after = 10",
        "max_attempts = 11",
        "colour = \"red\"",
        "wake_prompt = \"\"",
        "wake_prompt = \"a\\u0007b\"",
    ] {
        std::fs::write(
            &file,
            format!(
                "[[workspace]]\npath = {:?}\nenabled = true\n{bad}\n",
                ws.to_string_lossy()
            ),
        )
        .unwrap();
        let loaded = load_watch_config(&file, &ws);
        assert!(loaded.problem.is_some(), "{bad}");
        assert!(!loaded.config.enabled, "{bad}");
    }

    // A file inside the workspace is ignored.
    let inside = ws.join("agent-watch.toml");
    std::fs::write(
        &inside,
        format!(
            "[[workspace]]\npath = {:?}\nenabled = true\n",
            ws.to_string_lossy()
        ),
    )
    .unwrap();
    let loaded = load_watch_config(&inside, &ws);
    assert!(!loaded.config.enabled);
    assert!(loaded.problem.unwrap().contains("inside the workspace"));
    assert!(save_watch_config(&inside, &ws, &config()).is_err());

    // The state survives a restart: escalation and counts are kept.
    let clock = FakeClock::at(0);
    let mut host = FakeHost::with(&[("a", AgentState::Idle)]);
    let mut log = FakeLog::default();
    let mut w = dog(WatchConfig {
        max_attempts: 1,
        ..config()
    });
    for _ in 0..3 {
        w.tick(&mut host, &clock, &mut log);
        clock.advance(300);
    }
    assert_eq!(w.records["a"].phase, Phase::Escalated);
    save_watch_state(&state, &ws, &w.records).unwrap();
    save_watch_state(&state, &dir.join("other"), &BTreeMap::new()).unwrap();
    let (records, problem) = load_watch_state(&state, &ws);
    assert_eq!(problem, None);
    assert_eq!(records, w.records);
    let mut again = Watchdog::new(config(), BTreeSet::new(), records);
    for _ in 0..5 {
        clock.advance(300);
        assert!(again.tick(&mut host, &clock, &mut log).is_empty());
    }
    assert_eq!(host.wakes.len(), 1);

    // A broken state file starts empty and says so.
    std::fs::write(&state, "{not json").unwrap();
    let (records, problem) = load_watch_state(&state, &ws);
    assert!(records.is_empty());
    assert!(problem.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

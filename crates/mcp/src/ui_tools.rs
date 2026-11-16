//! The UI driver's MCP tools (`SPEC-0319` behaviours 29 and 32-39): arguments, and the checks every
//! call passes before the window sees it — a window, the user's setting, the right `ui` — plus the
//! rights of an action's effects, asked like any other right, and the audit line.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;
use rmcp::schemars;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use idecraft_uidriver::engine::{DEFAULT_MAX_NODES, DEFAULT_TIMEOUT, Op, Request, Target};
use idecraft_uidriver::trace::audit_entry;
use idecraft_uidriver::{Actor, ErrorCode, Grants, Locator, Reply, StopSource, UiError};

use crate::IdeServer;
use crate::window::{UiCall, UiCallKind};

/// Arguments of `ui_snapshot`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiSnapshotArgs {
    /// Only the subtree of this element: a locator in the text form (`id=project-tree`,
    /// `role=dialog[name="Commit"]`) or the JSON form.
    pub locator: Option<Value>,
    /// Levels to include (default 30).
    pub depth: Option<usize>,
    /// Most nodes to include (default 2000); `truncated` says how many were left out.
    pub max_nodes: Option<usize>,
    /// `json` (default) or `text`: an indented outline `role "name" #test-id [states]`.
    pub format: Option<String>,
    /// A window of this project by id (default: the project's window).
    pub window: Option<u64>,
}

/// Arguments of `ui_find`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiFindArgs {
    /// What to find: `role=button[name="Commit"]`, `id=tab-0`, `text="No problems"`,
    /// `name~="…"` for a substring, `name=/…/` for a regular expression, `>>` for "inside",
    /// filters `[enabled]`, `[checked=false]`, `[nth=1]`; or the JSON form.
    pub locator: Value,
    /// Wait for the first match (default false).
    pub wait: Option<bool>,
    /// How long `wait` waits, in milliseconds (default 10000, at most 180000).
    pub timeout_ms: Option<u64>,
    /// A window of this project by id.
    pub window: Option<u64>,
}

/// Arguments of `ui_act`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiActArgs {
    /// The element: a locator (text or JSON form). Resolved again until it is actionable; more
    /// than one match fails with `ambiguous`.
    pub locator: Option<Value>,
    /// Or an element id from `ui_find` (fails with `stale` once the element is gone).
    pub element: Option<u64>,
    /// `click`, `context_click`, `hover`, `drag`, `scroll`, `scroll_into_view`, `press`, `type`,
    /// `focus`, `select`, `expand`, `collapse`, `invoke`, `show_context_menu`, `increment`,
    /// `decrement`, `set_value`.
    pub action: String,
    /// `click`: `left` (default), `right` or `middle`.
    pub button: Option<String>,
    /// `click`: 1 to 3 (default 1).
    pub count: Option<u8>,
    /// `click`: held modifiers, as the keymap writes them (`cmd`, `ctrl`, `alt`, `shift`).
    pub modifiers: Option<Vec<String>>,
    /// Pointer actions: the point relative to the element's top left (default: its centre).
    pub position: Option<Value>,
    /// `drag`: the target element (locator).
    pub to: Option<Value>,
    /// `drag`: an offset `{x, y}` instead of a target.
    pub offset: Option<Value>,
    /// `scroll`: horizontal delta.
    pub dx: Option<f32>,
    /// `scroll`: vertical delta.
    pub dy: Option<f32>,
    /// `scroll`: `lines` (default) or `pixels`.
    pub unit: Option<String>,
    /// `press`: keys as the keymap writes them (`cmd-shift-a`, `alt-f12`, `cmd-k cmd-s`); without
    /// a locator they go to the window.
    pub keys: Option<String>,
    /// `type`: Unicode text, committed through the window's input handler (logged only as its
    /// length).
    pub text: Option<String>,
    /// `type`: select and delete the existing value first.
    pub clear: Option<bool>,
    /// `set_value`: the value.
    pub value: Option<String>,
    /// `window` (default for pointer, key and text actions) or `semantic`; `native` is only for
    /// the self-test.
    pub level: Option<String>,
    /// How long to wait for the element to become actionable, in milliseconds (default 10000).
    pub timeout_ms: Option<u64>,
    /// Skip the checks except "attached" (tests of disabled states).
    pub force: Option<bool>,
    /// A window of this project by id.
    pub window: Option<u64>,
}

/// Arguments of `ui_expect`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiExpectArgs {
    /// The element(s), as for `ui_find`.
    pub locator: Value,
    /// `visible`, `hidden`, `enabled`, `disabled`, `focused`, `checked`, `selected`, `expanded`,
    /// `name`, `value`, `text`, `count`, `absent`.
    pub matcher: String,
    /// `name`, `value`, `text`: the expected text.
    pub text: Option<String>,
    /// `name`, `value`, `text`: `exact` (default), `contains` or `regex`.
    pub compare: Option<String>,
    /// `count`: exactly.
    pub exactly: Option<usize>,
    /// `count`: at least.
    pub at_least: Option<usize>,
    /// `count`: at most.
    pub at_most: Option<usize>,
    /// Inverts the matcher.
    pub not: Option<bool>,
    /// How long it retries, in milliseconds (default 10000).
    pub timeout_ms: Option<u64>,
    /// A window of this project by id.
    pub window: Option<u64>,
}

/// Arguments of `ui_screenshot`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiScreenshotArgs {
    /// Only this element (locator).
    pub locator: Option<Value>,
    /// A window of this project by id.
    pub window: Option<u64>,
}

/// Arguments of `ui_trace`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiTraceArgs {
    /// `start`, `stop` or `status`.
    pub command: String,
    /// `stop`: a folder inside the workspace to write `ui-traces/<time>/` into (default: the
    /// user's settings folder, never the project).
    pub path: Option<String>,
}

/// The MCP error of a driver error: `code: message (hint)`, with the code in `data`.
pub(crate) fn mcp_error(e: &UiError) -> McpError {
    McpError::invalid_params(
        e.line(),
        Some(json!({"code": e.code.as_str(), "right": e.right, "elements": e.elements})),
    )
}

fn timeout(ms: Option<u64>) -> Duration {
    ms.map_or(DEFAULT_TIMEOUT, Duration::from_millis)
}

fn locator(v: &Value) -> Result<Locator, McpError> {
    Locator::from_json(v).map_err(|e| mcp_error(&e))
}

/// The action of `ui_act` from its flat arguments.
fn action_of(args: &UiActArgs) -> Result<idecraft_uidriver::Action, McpError> {
    let mut o = Map::new();
    o.insert("action".into(), json!(args.action));
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            o.insert(k.into(), v);
        }
    };
    put("button", args.button.clone().map(Value::from));
    put("count", args.count.map(Value::from));
    put("modifiers", args.modifiers.clone().map(|m| json!(m)));
    put("position", args.position.clone());
    put("to", args.to.clone());
    put("offset", args.offset.clone());
    put("dx", args.dx.map(Value::from));
    put("dy", args.dy.map(Value::from));
    put("unit", args.unit.clone().map(Value::from));
    put("keys", args.keys.clone().map(Value::from));
    put("text", args.text.clone().map(Value::from));
    put("clear", args.clear.map(Value::from));
    put("value", args.value.clone().map(Value::from));
    // The error names the field, never a typed text.
    serde_json::from_value(Value::Object(o)).map_err(|e| {
        let why = e.to_string();
        let why = why.split(" at line").next().unwrap_or_default();
        mcp_error(&UiError::invalid(format!(
            "invalid: {}: {why}",
            args.action
        )))
    })
}

fn level_of(s: Option<&str>) -> Result<Option<idecraft_uidriver::Level>, McpError> {
    match s {
        None => Ok(None),
        Some(l) => serde_json::from_value(json!(l)).map(Some).map_err(|_| {
            mcp_error(&UiError::invalid(format!(
                "invalid: level is semantic, window or native, not `{l}`"
            )))
        }),
    }
}

/// The matcher of `ui_expect` from its flat arguments.
fn matcher_of(args: &UiExpectArgs) -> Result<idecraft_uidriver::Matcher, McpError> {
    let mut o = Map::new();
    o.insert("matcher".into(), json!(args.matcher));
    if let Some(t) = &args.text {
        o.insert("text".into(), json!(t));
    }
    if let Some(c) = &args.compare {
        o.insert("compare".into(), json!(c));
    }
    for (k, v) in [
        ("exactly", args.exactly),
        ("at_least", args.at_least),
        ("at_most", args.at_most),
    ] {
        if let Some(v) = v {
            o.insert(k.into(), json!(v));
        }
    }
    serde_json::from_value(Value::Object(o)).map_err(|e| {
        let why = e.to_string();
        let why = why.split(" at line").next().unwrap_or_default();
        mcp_error(&UiError::invalid(format!(
            "invalid: {}: {why}",
            args.matcher
        )))
    })
}

impl IdeServer {
    /// The settings file the setting `ui.driver.external` is read from.
    fn ui_settings_file(&self) -> Option<PathBuf> {
        self.ide_settings_file
            .clone()
            .or_else(idecraft_core::ide_settings_file)
    }

    /// Whether the user switched external driving on (behaviour 32).
    pub(crate) fn ui_external_on(&self) -> bool {
        self.ui_settings_file()
            .is_some_and(|f| idecraft_core::load_ide_settings(&f).ui.driver.external)
    }

    /// The client's name and this MCP session's id.
    fn ui_actor(&self) -> Actor {
        let client = crate::CALL
            .try_with(|c| c.client.clone())
            .unwrap_or_else(|_| "an unnamed client".into());
        let session = format!("{:x}", std::sync::Arc::as_ptr(&self.session) as usize);
        Actor::External { client, session }
    }

    /// The checks before the window sees a call: a window, the setting, the right `ui`
    /// (behaviours 29, 32, 34). Returns the rights of the call.
    pub(crate) async fn ui_prelude(&self) -> Result<Grants, McpError> {
        if self.window.is_none() {
            return Err(mcp_error(&UiError::new(
                ErrorCode::Unavailable,
                "unavailable: this MCP server runs without the IDECraft window (stdio, `idecraft mcp`)",
                "the UI driver is served by the window's MCP server over HTTP; its address and token are in mcp-http/<pid>.json of the settings folder",
            )));
        }
        if !self.ui_external_on() {
            return Err(mcp_error(&UiError::driving_off()));
        }
        // An emergency stop withdraws `ui` given for this session only (behaviour 37).
        {
            let generation = idecraft_uidriver::security::stop_generation();
            let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            if s.ui_stop_generation != generation {
                s.allowed.remove("ui");
                s.ui_stop_generation = generation;
            }
        }
        self.require(idecraft_core::Grant::Ui).await.map_err(|e| {
            let text = e.message.to_string();
            let line = if text.starts_with("denied") {
                text
            } else {
                format!("not_granted: {text}")
            };
            McpError::invalid_params(line, Some(json!({"code": "not_granted", "right": "ui"})))
        })?;
        Ok(self.ui_grants())
    }

    /// The rights this call has: the grant file's, this session's answers, and `ui`.
    fn ui_grants(&self) -> Grants {
        let mut names: std::collections::BTreeSet<String> = self
            .permissions
            .allow
            .iter()
            .map(|g| g.name().to_owned())
            .collect();
        {
            let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            names.extend(s.allowed.iter().cloned());
        }
        names.insert("ui".into());
        Grants(names)
    }

    /// Sends `kind` to the window and waits for its answer.
    async fn ui_send(&self, window: Option<u64>, kind: UiCallKind, wait: Duration) -> Reply {
        let unavailable = |m: &str| Reply {
            result: Err(UiError::new(
                ErrorCode::Unavailable,
                m,
                "is the IDECraft window still open?",
            )),
            facts: Default::default(),
        };
        let Some(w) = self.window.clone() else {
            return unavailable("unavailable: no window");
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        w.ui(
            UiCall {
                workspace: self.workspace.root().to_path_buf(),
                window,
                kind,
            },
            reply,
        );
        match tokio::time::timeout(wait + Duration::from_secs(5), answer).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => unavailable("unavailable: the window could not serve the UI driver"),
            Err(_) => unavailable("unavailable: the window did not answer in time"),
        }
    }

    /// Runs a driver call: asks for a missing effect right once, as the corresponding tool would,
    /// and logs actions and assertions.
    async fn ui_driver(
        &self,
        tool: &str,
        op: Op,
        window: Option<u64>,
        wait: Duration,
        typed_len: Option<usize>,
    ) -> Result<CallToolResult, McpError> {
        let started = Instant::now();
        let actor = self.ui_actor();
        let locator_text = op.locator_text();
        let audited = matches!(op, Op::Act { .. } | Op::Expect { .. });
        let mut grants = match self.ui_prelude().await {
            Ok(g) => g,
            Err(e) => {
                // A refused action is logged too (UISEC-3), once the user switched driving on.
                if audited
                    && self.window.is_some()
                    && self.ui_external_on()
                    && let Actor::External { client, session } = &actor
                {
                    let outcome = e.message.split(':').next().unwrap_or("refused").to_owned();
                    self.log(
                        self.workspace.root(),
                        audit_entry(
                            client,
                            session,
                            tool,
                            locator_text.as_deref(),
                            &Default::default(),
                            &outcome,
                            started.elapsed(),
                            typed_len,
                        ),
                    )?;
                }
                return Err(e);
            }
        };
        let mut asked = false;
        let reply = loop {
            let request = Request {
                op: op.clone(),
                actor: actor.clone(),
                grants: grants.clone(),
                timeout: wait,
                cancel: None,
            };
            let reply = self
                .ui_send(window, UiCallKind::Driver(request), wait)
                .await;
            // A missing effect right is asked once in the window, like a tool call's would be.
            if let Err(e) = &reply.result
                && e.code == ErrorCode::NotGranted
                && !asked
                && let Some(right) = e.right.as_deref()
                && right != "ui"
                && let Ok(grant) = serde_json::from_value::<idecraft_core::Grant>(json!(right))
            {
                asked = true;
                if self.require(grant).await.is_ok() {
                    grants.0.insert(right.to_owned());
                    continue;
                }
            }
            break reply;
        };
        if audited {
            let (client, session) = match &actor {
                Actor::External { client, session } => (client.clone(), session.clone()),
                Actor::SelfTest => ("self-test".into(), String::new()),
            };
            let outcome = match &reply.result {
                Ok(_) => "ok",
                Err(e) => e.code.as_str(),
            };
            self.log(
                self.workspace.root(),
                audit_entry(
                    &client,
                    &session,
                    tool,
                    locator_text.as_deref(),
                    &reply.facts,
                    outcome,
                    started.elapsed(),
                    typed_len,
                ),
            )?;
        }
        match reply.result {
            Ok(v) => crate::json_result(v),
            Err(e) => Err(mcp_error(&e)),
        }
    }

    pub(crate) async fn ui_snapshot_impl(
        &self,
        args: UiSnapshotArgs,
    ) -> Result<CallToolResult, McpError> {
        let text = match args.format.as_deref() {
            None | Some("json") => false,
            Some("text") => true,
            Some(other) => {
                return Err(mcp_error(&UiError::invalid(format!(
                    "invalid: format is json or text, not `{other}`"
                ))));
            }
        };
        let op = Op::Snapshot {
            locator: args.locator.as_ref().map(locator).transpose()?,
            depth: args.depth.unwrap_or(30),
            max_nodes: args.max_nodes.unwrap_or(DEFAULT_MAX_NODES),
            text,
        };
        self.ui_driver("ui_snapshot", op, args.window, DEFAULT_TIMEOUT, None)
            .await
    }

    pub(crate) async fn ui_find_impl(&self, args: UiFindArgs) -> Result<CallToolResult, McpError> {
        let op = Op::Find {
            locator: locator(&args.locator)?,
            wait: args.wait.unwrap_or(false),
        };
        self.ui_driver("ui_find", op, args.window, timeout(args.timeout_ms), None)
            .await
    }

    pub(crate) async fn ui_act_impl(&self, args: UiActArgs) -> Result<CallToolResult, McpError> {
        let action = action_of(&args)?;
        let target = match (&args.locator, args.element) {
            (Some(_), Some(_)) => {
                return Err(mcp_error(&UiError::invalid(
                    "invalid: give `locator` or `element`, not both",
                )));
            }
            (Some(l), None) => Target::Locator(locator(l)?),
            (None, Some(id)) => Target::Element(id),
            (None, None) => Target::Window,
        };
        let typed_len = action.typed_len();
        let op = Op::Act {
            target,
            action,
            level: level_of(args.level.as_deref())?,
            force: args.force.unwrap_or(false),
        };
        self.ui_driver(
            "ui_act",
            op,
            args.window,
            timeout(args.timeout_ms),
            typed_len,
        )
        .await
    }

    pub(crate) async fn ui_expect_impl(
        &self,
        args: UiExpectArgs,
    ) -> Result<CallToolResult, McpError> {
        let op = Op::Expect {
            locator: locator(&args.locator)?,
            matcher: matcher_of(&args)?,
            not: args.not.unwrap_or(false),
        };
        self.ui_driver("ui_expect", op, args.window, timeout(args.timeout_ms), None)
            .await
    }

    pub(crate) async fn ui_screenshot_impl(
        &self,
        args: UiScreenshotArgs,
    ) -> Result<CallToolResult, McpError> {
        let started = Instant::now();
        self.ui_prelude().await?;
        if let Some(l) = &args.locator {
            locator(l)?;
        }
        let reply = self
            .ui_send(args.window, UiCallKind::Screenshot, DEFAULT_TIMEOUT)
            .await;
        let actor = self.ui_actor();
        if let Actor::External { client, session } = &actor {
            let outcome = match &reply.result {
                Ok(_) => "ok",
                Err(e) => e.code.as_str(),
            };
            self.log(
                self.workspace.root(),
                audit_entry(
                    client,
                    session,
                    "ui_screenshot",
                    None,
                    &reply.facts,
                    outcome,
                    started.elapsed(),
                    None,
                ),
            )?;
        }
        match reply.result {
            Ok(v) => crate::json_result(v),
            Err(e) => Err(mcp_error(&e)),
        }
    }

    pub(crate) async fn ui_trace_impl(
        &self,
        args: UiTraceArgs,
    ) -> Result<CallToolResult, McpError> {
        let started = Instant::now();
        self.ui_prelude().await?;
        let kind = match args.command.as_str() {
            "start" => UiCallKind::TraceStart,
            "status" => UiCallKind::TraceStatus,
            "stop" => UiCallKind::TraceStop {
                dir: match &args.path {
                    None => None,
                    Some(p) => {
                        // A trace written into the workspace is a file of the project
                        // (SPEC-0053 v8 behaviour 13).
                        self.require(idecraft_core::Grant::Write).await?;
                        Some(self.trace_dir_in_workspace(p)?)
                    }
                },
            },
            other => {
                return Err(mcp_error(&UiError::invalid(format!(
                    "invalid: command is start, stop or status, not `{other}`"
                ))));
            }
        };
        let reply = self.ui_send(None, kind, DEFAULT_TIMEOUT).await;
        if args.command != "status"
            && let Actor::External { client, session } = self.ui_actor()
        {
            let outcome = match &reply.result {
                Ok(_) => "ok",
                Err(e) => e.code.as_str(),
            };
            self.log(
                self.workspace.root(),
                audit_entry(
                    &client,
                    &session,
                    &format!("ui_trace {}", args.command),
                    None,
                    &reply.facts,
                    outcome,
                    started.elapsed(),
                    None,
                ),
            )?;
        }
        match reply.result {
            Ok(v) => crate::json_result(v),
            Err(e) => Err(mcp_error(&e)),
        }
    }

    /// A folder for a trace inside the workspace (behaviour 26: never outside it).
    fn trace_dir_in_workspace(&self, path: &str) -> Result<PathBuf, McpError> {
        let rel = Path::new(path);
        let outside = || {
            mcp_error(&UiError::invalid(format!(
                "invalid: {path} is not a folder inside the workspace"
            )))
        };
        if rel.is_absolute()
            || rel.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(outside());
        }
        Ok(self.workspace.root().join(rel))
    }

    pub(crate) async fn ui_driver_status_impl(&self) -> Result<CallToolResult, McpError> {
        if self.window.is_none() {
            return crate::json_result(json!({
                "external": self.ui_external_on(),
                "window": false,
                "drivers": [],
                "hint": "the UI driver is served by the window's MCP server over HTTP",
            }));
        }
        let reply = self
            .ui_send(None, UiCallKind::Status, DEFAULT_TIMEOUT)
            .await;
        match reply.result {
            Ok(mut v) => {
                v["window"] = json!(true);
                v["external"] = json!(self.ui_external_on());
                crate::json_result(v)
            }
            Err(e) => Err(mcp_error(&e)),
        }
    }

    pub(crate) async fn ui_driver_stop_impl(
        &self,
        source: StopSource,
    ) -> Result<CallToolResult, McpError> {
        // The setting goes off even without a window to tell (behaviour 37).
        if let Some(file) = self.ui_settings_file() {
            let mut s = idecraft_core::load_ide_settings(&file);
            if s.ui.driver.external {
                s.ui.driver.external = false;
                idecraft_core::save_ide_settings(&file, &s).map_err(|e| {
                    McpError::internal_error(
                        format!("the setting could not be switched off: {e}"),
                        None,
                    )
                })?;
            }
        }
        let reply = match &self.window {
            Some(_) => {
                self.ui_send(None, UiCallKind::Stop(source), DEFAULT_TIMEOUT)
                    .await
            }
            None => Reply {
                result: Ok(json!({"stopped": true, "sessions_ended": 0})),
                facts: Default::default(),
            },
        };
        {
            let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            s.allowed.remove("ui");
        }
        if let Actor::External { client, session } = self.ui_actor() {
            let _ = idecraft_core::log_agent_action(
                self.workspace.root(),
                &audit_entry(
                    &client,
                    &session,
                    "ui_driver_stop",
                    None,
                    &Default::default(),
                    if reply.result.is_ok() { "ok" } else { "error" },
                    Duration::ZERO,
                    None,
                ),
            );
        }
        match reply.result {
            Ok(mut v) => {
                v["external"] = json!(false);
                crate::json_result(v)
            }
            Err(e) => Err(mcp_error(&e)),
        }
    }
}

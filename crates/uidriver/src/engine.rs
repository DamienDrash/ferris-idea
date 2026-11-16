//! The driver's engine: calls wait on presented frames, never on a timer (`SPEC-0319` behaviours
//! 12-15). The window calls [`Engine::frame`] with the tree of each frame it presents; the engine
//! resolves locators again, runs the actionability checks, checks rights, and answers with the
//! input to deliver and the calls that are done. It never sleeps and has no thread.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::action::{
    Action, Button, Check, Input, KeyPress, Level, parse_keys, pointer_inputs, primary,
};
use crate::error::{ErrorCode, UiError};
use crate::expect::{Matcher, Outcome, evaluate};
use crate::locator::Locator;
use crate::security::{
    self, Actor, Driving, Grants, HIGHLIGHT, StopSource, Supervisor, authorize, effects_of_action,
    reserved_action,
};
use crate::trace::{AuditFacts, CheckWait, Trace, TraceEntry};
use crate::tree::{Effect, ElementId, ElementInfo, Rect, Reserved, Tree, UiNode, View};

/// Id of a call.
pub type OpId = u64;

/// How long a call waits unless it says otherwise (behaviour 14).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest wait a call may ask for (the self-test's long steps).
pub const MAX_TIMEOUT: Duration = Duration::from_secs(180);

/// Default limit of a snapshot (behaviour 24).
pub const DEFAULT_MAX_NODES: usize = 2000;

/// Cancels one call (behaviour 15).
#[derive(Debug, Clone, Default)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    /// A fresh handle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels the call; it answers `cancelled` at the next frame.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether it was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// What an action acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// The element a locator resolves to, resolved again on every frame.
    Locator(Locator),
    /// A node by id; `stale` once it is gone.
    Element(ElementId),
    /// The window (`press` without an element).
    Window,
}

/// One call.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The tree, or the subtree of a locator.
    Snapshot {
        /// Subtree root.
        locator: Option<Locator>,
        /// Levels.
        depth: usize,
        /// Node limit.
        max_nodes: usize,
        /// The indented text outline instead of JSON.
        text: bool,
    },
    /// The current matches.
    Find {
        /// What to find.
        locator: Locator,
        /// Wait for the first match.
        wait: bool,
    },
    /// An action.
    Act {
        /// On what.
        target: Target,
        /// What.
        action: Action,
        /// Delivery level (default: the action's).
        level: Option<Level>,
        /// Skip the checks except `attached`.
        force: bool,
    },
    /// An assertion.
    Expect {
        /// Of what.
        locator: Locator,
        /// What it expects.
        matcher: Matcher,
        /// Inverted.
        not: bool,
    },
}

impl Op {
    /// The tool name in traces.
    pub fn tool(&self) -> &'static str {
        match self {
            Self::Snapshot { .. } => "snapshot",
            Self::Find { .. } => "find",
            Self::Act { .. } => "act",
            Self::Expect { .. } => "expect",
        }
    }

    /// The locator in the text form, if any.
    pub fn locator_text(&self) -> Option<String> {
        match self {
            Self::Snapshot { locator, .. } => locator.as_ref().map(Locator::to_text),
            Self::Find { locator, .. } | Self::Expect { locator, .. } => Some(locator.to_text()),
            Self::Act { target, .. } => match target {
                Target::Locator(l) => Some(l.to_text()),
                Target::Element(id) => Some(format!("element={id}")),
                Target::Window => None,
            },
        }
    }
}

/// A call with who makes it and what it may do.
#[derive(Debug, Clone)]
pub struct Request {
    /// The call.
    pub op: Op,
    /// Who.
    pub actor: Actor,
    /// The rights of the call (external clients).
    pub grants: Grants,
    /// How long it may wait.
    pub timeout: Duration,
    /// Its cancellation handle.
    pub cancel: Option<CancelHandle>,
}

impl Request {
    /// A call of the in-process self-test.
    pub fn selftest(op: Op) -> Self {
        Self {
            op,
            actor: Actor::SelfTest,
            grants: Grants::default(),
            timeout: DEFAULT_TIMEOUT,
            cancel: None,
        }
    }

    /// The same call with `timeout` (at most [`MAX_TIMEOUT`]).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.min(MAX_TIMEOUT);
        self
    }

    fn view(&self) -> View {
        if self.actor.is_external() {
            View {
                external: true,
                terminal_read: self.grants.has("terminal-read"),
            }
        } else {
            View::in_process()
        }
    }
}

/// What the keymap binds a key to.
pub trait Keys {
    /// The action ids bound to `key` in the keymap in use.
    fn actions_for(&self, key: &KeyPress) -> Vec<String>;
}

/// A keymap without bindings (tests).
pub struct NoKeys;

impl Keys for NoKeys {
    fn actions_for(&self, _: &KeyPress) -> Vec<String> {
        Vec::new()
    }
}

/// Something the window does for the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Deliver `inputs` on `level` into `window`, marked with `origin`.
    Deliver {
        /// The call.
        op: OpId,
        /// The window.
        window: u64,
        /// The events.
        inputs: Vec<Input>,
        /// The level.
        level: Level,
        /// `selftest` or `driver:<client>`.
        origin: String,
    },
    /// Highlight the target until `until` before its input arrives (behaviour 36).
    Highlight {
        /// The window.
        window: u64,
        /// The target's bounds.
        bounds: Option<Rect>,
        /// The pointer's action point.
        point: Option<(f32, f32)>,
        /// Until when.
        until: Instant,
    },
}

/// The answer to a call.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The result.
    pub result: Result<Value, UiError>,
    /// What the audit log records.
    pub facts: AuditFacts,
}

/// A call that is done.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// The call.
    pub op: OpId,
    /// Its answer.
    pub reply: Reply,
}

/// What one frame produced.
#[derive(Debug, Default)]
pub struct FrameOutput {
    /// Things to do, in order.
    pub commands: Vec<Command>,
    /// Calls answered.
    pub done: Vec<Done>,
}

enum Phase {
    Waiting,
    Highlight {
        until: Instant,
        inputs: Vec<Input>,
        level: Level,
    },
    Delivered {
        frame: u64,
    },
}

struct Pending {
    id: OpId,
    window: u64,
    request: Request,
    started: Instant,
    deadline: Instant,
    last_eval: Option<Instant>,
    waits: BTreeMap<Check, Duration>,
    seen: Option<(u64, ElementId, Rect)>,
    phase: Phase,
    before: Option<Tree>,
    element: Option<ElementInfo>,
    effects: Vec<Effect>,
    inputs: Vec<Input>,
}

enum Progress {
    Wait,
    Done(Result<Value, UiError>),
}

/// The engine of one window process.
pub struct Engine {
    next: OpId,
    pending: Vec<Pending>,
    /// External driving: sessions, the user's input, the emergency stop.
    pub supervisor: Supervisor,
    trace: Option<Trace>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// An engine with nothing to do.
    pub fn new() -> Self {
        Self {
            next: 1,
            pending: Vec::new(),
            supervisor: Supervisor::new(),
            trace: None,
        }
    }

    /// Accepts a call for `window`; it is answered from a later [`Engine::frame`].
    pub fn submit(&mut self, window: u64, request: Request, now: Instant) -> Result<OpId, UiError> {
        match &request.op {
            Op::Act {
                target,
                action,
                level,
                ..
            } => {
                action.validate()?;
                if action.needs_element() && *target == Target::Window {
                    return Err(UiError::invalid(format!(
                        "invalid: `{}` needs a locator or an element",
                        action.verb()
                    )));
                }
                let level = level.unwrap_or_else(|| action.default_level());
                if level == Level::Native && request.actor.is_external() {
                    return Err(UiError::new(
                        ErrorCode::Unsupported,
                        "unsupported: the native level is only for the in-process self-test",
                        "native events cannot be told from the user's own, so the user could not take over; use the window level",
                    ));
                }
            }
            Op::Expect { matcher, .. } => matcher.validate()?,
            _ => {}
        }
        let id = self.next;
        self.next += 1;
        self.supervisor.driving(&request.actor, window, now);
        let timeout = request.timeout.min(MAX_TIMEOUT);
        self.pending.push(Pending {
            id,
            window,
            started: now,
            deadline: now + timeout,
            last_eval: None,
            waits: BTreeMap::new(),
            seen: None,
            phase: Phase::Waiting,
            before: None,
            element: None,
            effects: Vec::new(),
            inputs: Vec::new(),
            request,
        });
        Ok(id)
    }

    /// Calls not answered yet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Whether calls for `window` wait (the window then asks for another frame).
    pub fn pending_for(&self, window: u64) -> bool {
        self.pending.iter().any(|p| p.window == window)
    }

    /// The windows with waiting calls.
    pub fn waiting_windows(&self) -> Vec<u64> {
        let mut w: Vec<u64> = self.pending.iter().map(|p| p.window).collect();
        w.sort_unstable();
        w.dedup();
        w
    }

    /// Answers a call whose input the window could not deliver (a level it does not have).
    pub fn fail(&mut self, op: OpId, error: UiError) -> Option<Done> {
        let i = self.pending.iter().position(|p| p.id == op)?;
        let p = self.pending.remove(i);
        Some(Done {
            op,
            reply: Reply {
                result: Err(error),
                facts: AuditFacts {
                    element: p.element,
                    effects: p.effects,
                },
            },
        })
    }

    /// Notes the user's own input: pending external calls answer `interrupted` (behaviour 37).
    pub fn user_input(&mut self, now: Instant) {
        self.supervisor.user_input(now);
    }

    /// The emergency stop (behaviour 37): every pending external call answers `cancelled` now and
    /// every external session ends. Self-test calls go on.
    pub fn stop(&mut self, source: StopSource) -> (Vec<Done>, Vec<Driving>) {
        let ended = self.supervisor.stop(source);
        let mut done = Vec::new();
        let mut keep = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            if p.request.actor.is_external() {
                done.push(Done {
                    op: p.id,
                    reply: Reply {
                        result: Err(security::stopped()),
                        facts: AuditFacts {
                            element: p.element.clone(),
                            effects: p.effects.clone(),
                        },
                    },
                });
            } else {
                keep.push(p);
            }
        }
        self.pending = keep;
        (done, ended)
    }

    /// Ends the sessions of clients that went away (behaviour 37b) and returns them.
    pub fn expire_idle(&mut self, now: Instant) -> Vec<Driving> {
        let busy = self
            .pending
            .iter()
            .filter_map(|p| match &p.request.actor {
                Actor::External { session, .. } => Some(session.clone()),
                Actor::SelfTest => None,
            })
            .collect();
        self.supervisor.expire(now, &busy)
    }

    /// Starts a trace; `false` when one runs already.
    pub fn trace_start(&mut self, now: Instant) -> bool {
        if self.trace.is_some() {
            return false;
        }
        self.trace = Some(Trace::start(now));
        true
    }

    /// Stops the trace and hands it over.
    pub fn trace_stop(&mut self) -> Option<Trace> {
        self.trace.take()
    }

    /// Entries of the running trace.
    pub fn trace_entries(&self) -> Option<usize> {
        self.trace.as_ref().map(|t| t.entries().len())
    }

    /// The state for `ui_driver_status` (the window adds whether the setting is on).
    pub fn status(&self, now: Instant) -> Value {
        json!({
            "drivers": self.supervisor.drivers(),
            "paused": self.supervisor.paused(now),
            "pending": self.pending.iter().filter(|p| p.request.actor.is_external()).count(),
            "trace": self.trace.is_some(),
            "last_stop": self.supervisor.last_stop(),
        })
    }

    /// Advances the calls of `window` with the tree of the frame it just presented.
    pub fn frame(
        &mut self,
        window: u64,
        tree: &Tree,
        now: Instant,
        keys: &dyn Keys,
    ) -> FrameOutput {
        let mut out = FrameOutput::default();
        let paused = self.supervisor.paused(now);
        let mut keep = Vec::new();
        for mut p in std::mem::take(&mut self.pending) {
            if p.window != window {
                keep.push(p);
                continue;
            }
            let progress = if p
                .request
                .cancel
                .as_ref()
                .is_some_and(CancelHandle::is_cancelled)
            {
                Progress::Done(Err(UiError::new(
                    ErrorCode::Cancelled,
                    "cancelled: the call was cancelled",
                    "",
                )))
            } else if p.request.actor.is_external() && !p.request.grants.has(security::UI_RIGHT) {
                // Reading the tree needs `ui` as much as acting does (behaviour 34).
                Progress::Done(Err(UiError::not_granted(
                    security::UI_RIGHT,
                    security::UI_MEANING,
                )))
            } else if paused
                && p.request.actor.is_external()
                && !matches!(p.phase, Phase::Delivered { .. })
            {
                Progress::Done(Err(security::interrupted()))
            } else {
                advance(&mut p, tree, now, keys, &mut out.commands)
            };
            p.last_eval = Some(now);
            match progress {
                Progress::Wait => keep.push(p),
                Progress::Done(result) => {
                    if let Some(trace) = &mut self.trace {
                        trace.record(entry(&p, &result, tree, trace.elapsed_ms(now)));
                    }
                    out.done.push(Done {
                        op: p.id,
                        reply: Reply {
                            result,
                            facts: AuditFacts {
                                element: p.element.clone(),
                                effects: p.effects.clone(),
                            },
                        },
                    });
                }
            }
        }
        // Calls submitted for other windows meanwhile stay.
        keep.append(&mut self.pending);
        self.pending = keep;
        out
    }
}

fn entry(p: &Pending, result: &Result<Value, UiError>, tree: &Tree, time_ms: u64) -> TraceEntry {
    let view = p.request.view();
    let action = match &p.request.op {
        Op::Act { action, .. } => Some(action.summary()),
        Op::Expect { matcher, not, .. } => Some(format!(
            "expect {}{}",
            if *not { "not " } else { "" },
            matcher.name()
        )),
        _ => None,
    };
    TraceEntry {
        time_ms,
        actor: p.request.actor.origin(),
        tool: p.request.op.tool().into(),
        locator: p.request.op.locator_text(),
        action,
        element: p.element.clone(),
        checks: p
            .waits
            .iter()
            .map(|(c, d)| CheckWait {
                check: c.name().into(),
                waited_ms: ms(*d),
            })
            .collect(),
        inputs: p.inputs.iter().map(Input::summary).collect(),
        diff: p.before.as_ref().map(|b| tree.diff(b, &view)),
        outcome: match result {
            Ok(_) => "ok".into(),
            Err(e) => e.code.as_str().into(),
        },
        message: result.as_ref().err().map(|e| e.message.clone()),
        stderr: Vec::new(),
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn waits_json(p: &Pending) -> Value {
    Value::Object(
        p.waits
            .iter()
            .map(|(c, d)| (c.name().to_owned(), json!(ms(*d))))
            .collect(),
    )
}

fn advance(
    p: &mut Pending,
    tree: &Tree,
    now: Instant,
    keys: &dyn Keys,
    commands: &mut Vec<Command>,
) -> Progress {
    let view = p.request.view();
    match p.request.op.clone() {
        Op::Snapshot {
            locator,
            depth,
            max_nodes,
            text,
        } => {
            let from = match &locator {
                None => None,
                Some(l) => match one_now(l, tree, &view) {
                    Ok(id) => Some(id),
                    Err(e) => return Progress::Done(Err(e)),
                },
            };
            let depth = depth.max(1);
            let max_nodes = max_nodes.max(1);
            let result = if text {
                let (outline, left) = tree.outline(from, depth, max_nodes, &view);
                json!({"frame": tree.frame, "outline": outline, "truncated": left})
            } else {
                let (nodes, left) = tree.to_json(from, depth, max_nodes, &view);
                json!({"frame": tree.frame, "nodes": nodes, "truncated": left})
            };
            Progress::Done(Ok(result))
        }
        Op::Find { locator, wait } => match locator.resolve(tree, &view) {
            Err(e) => Progress::Done(Err(e)),
            Ok(ids) if ids.is_empty() && wait && now < p.deadline => Progress::Wait,
            Ok(ids) => {
                let matches: Vec<ElementInfo> = ids
                    .iter()
                    .filter_map(|id| tree.get(*id))
                    .take(50)
                    .map(|n| ElementInfo::of(tree, n, &view))
                    .collect();
                Progress::Done(Ok(
                    json!({"count": ids.len(), "matches": matches, "frame": tree.frame}),
                ))
            }
        },
        Op::Expect {
            locator,
            matcher,
            not,
        } => {
            let ids = match locator.resolve(tree, &view) {
                Ok(ids) => ids,
                Err(e) => return Progress::Done(Err(e)),
            };
            if let [one] = ids.as_slice()
                && let Some(n) = tree.get(*one)
            {
                p.element = Some(ElementInfo::of(tree, n, &view));
            }
            match evaluate(&matcher, not, tree, &ids, &view) {
                Outcome::Pass => Progress::Done(Ok(json!({
                    "matcher": matcher.name(), "not": not, "passed": true,
                    "waited_ms": ms(now.saturating_duration_since(p.started)),
                }))),
                Outcome::Error(e) => Progress::Done(Err(e)),
                Outcome::Fail(_) if now < p.deadline => Progress::Wait,
                Outcome::Fail(shows) => {
                    // The expected text of a value matcher is not repeated: it may be a secret.
                    let expected = match &matcher {
                        Matcher::Name { text, .. } | Matcher::Text { text, .. } => {
                            format!(" {text:?}")
                        }
                        _ => String::new(),
                    };
                    Progress::Done(Err(UiError::new(
                        ErrorCode::DeadlineExceeded,
                        format!(
                            "deadline_exceeded: expected {}{}{} of `{}` within {} ms; at the deadline: {shows}",
                            if not { "not " } else { "" },
                            matcher.name(),
                            expected,
                            locator.to_text(),
                            ms(p.deadline.saturating_duration_since(p.started)),
                        ),
                        "the window may still be busy: allow more time with timeout_ms, or check the locator with ui_find",
                    )))
                }
            }
        }
        Op::Act {
            target,
            action,
            level,
            force,
        } => act(p, tree, now, keys, commands, &target, &action, level, force),
    }
}

fn one_now(l: &Locator, tree: &Tree, view: &View) -> Result<ElementId, UiError> {
    let ids = l.resolve(tree, view)?;
    match ids.as_slice() {
        [one] => Ok(*one),
        [] => Err(UiError::new(
            ErrorCode::NotFound,
            format!("not_found: nothing matches `{}`", l.to_text()),
            "see the closest elements, or take a snapshot",
        )
        .with_elements(l.closest(tree, view))),
        many => Err(ambiguous(l, many, tree, view)),
    }
}

fn ambiguous(l: &Locator, ids: &[ElementId], tree: &Tree, view: &View) -> UiError {
    UiError::new(
        ErrorCode::Ambiguous,
        format!(
            "ambiguous: `{}` matches {} elements; an action needs one",
            l.to_text(),
            ids.len()
        ),
        "make the locator more precise (exact name, test id, within) or add nth",
    )
    .with_elements(
        ids.iter()
            .filter_map(|id| tree.get(*id))
            .map(|n| ElementInfo::of(tree, n, view))
            .collect(),
    )
}

/// The effects and reservations of each key of a `press` (behaviour 35).
type KeyFacts = Vec<security::KeyFact>;

fn key_facts(keys_text: &str, keys: &dyn Keys, tree: &Tree) -> Result<KeyFacts, UiError> {
    let mut out = Vec::new();
    for k in parse_keys(keys_text)? {
        let actions = keys.actions_for(&k);
        let (effects, reserved) = if actions.is_empty() {
            // An unbound key types into the focused element.
            match tree.focused() {
                Some(f) if f.secret => (None, Some(Reserved::Secret)),
                Some(f) if f.editable() => (f.effects.clone(), f.reserved),
                _ => (None, None),
            }
        } else {
            let reserved = actions.iter().find_map(|a| reserved_action(a));
            let mut all: Option<Vec<Effect>> = Some(Vec::new());
            for a in &actions {
                match (effects_of_action(a), &mut all) {
                    (Some(e), Some(list)) => list.extend(e),
                    _ => all = None,
                }
            }
            (all, reserved)
        };
        out.push((k, effects, reserved));
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn act(
    p: &mut Pending,
    tree: &Tree,
    now: Instant,
    keys: &dyn Keys,
    commands: &mut Vec<Command>,
    target: &Target,
    action: &Action,
    level: Option<Level>,
    force: bool,
) -> Progress {
    let view = p.request.view();
    match p.phase {
        Phase::Delivered { frame } => {
            if tree.frame <= frame {
                return Progress::Wait;
            }
            let mut result = json!({
                "element": p.element,
                "level": level.unwrap_or_else(|| action.default_level()),
                "inputs": p.inputs.len(),
                "waited_ms": waits_json(p),
                "effects": p.effects,
                "focused": tree.focused().map(|n| ElementInfo::of(tree, n, &view)),
            });
            if p.element.is_none() {
                result.as_object_mut().map(|o| o.remove("element"));
            }
            return Progress::Done(Ok(result));
        }
        Phase::Highlight { until, .. } => {
            if now < until {
                return Progress::Wait;
            }
            // A question reserved for the user that opened during the highlight takes no input of
            // the client (SPEC-0319 v6 behaviour 35).
            if let Err(e) = security::refuse_while_reserved_modal(&p.request.actor, tree) {
                return Progress::Done(Err(e));
            }
            let Phase::Highlight { inputs, level, .. } =
                std::mem::replace(&mut p.phase, Phase::Delivered { frame: tree.frame })
            else {
                unreachable!("checked above");
            };
            p.before = Some(tree.clone());
            commands.push(Command::Deliver {
                op: p.id,
                window: p.window,
                inputs,
                level,
                origin: p.request.actor.origin(),
            });
            return Progress::Wait;
        }
        Phase::Waiting => {}
    }
    let dt = p
        .last_eval
        .map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
    let fail = |p: &mut Pending, check: Check, err: UiError| -> Progress {
        *p.waits.entry(check).or_default() += dt;
        if now >= p.deadline {
            let waited: Vec<String> = p
                .waits
                .iter()
                .map(|(c, d)| format!("{} {} ms", c.name(), ms(*d)))
                .collect();
            let mut err = err;
            err.message = format!(
                "{} (deadline of {} ms passed; waited: {})",
                err.message,
                ms(p.deadline.saturating_duration_since(p.started)),
                waited.join(", ")
            );
            Progress::Done(Err(err))
        } else {
            Progress::Wait
        }
    };
    // Resolve the target again on every frame.
    let node: Option<&UiNode> = match target {
        Target::Window => None,
        Target::Element(id) => match tree.get(*id) {
            Some(n) => Some(n),
            None => {
                return Progress::Done(Err(UiError::new(
                    ErrorCode::Stale,
                    format!("stale: element {id} is no longer in the window"),
                    "use a locator: locators are resolved again for every action",
                )));
            }
        },
        Target::Locator(l) => {
            let ids = match l.resolve(tree, &view) {
                Ok(ids) => ids,
                Err(e) => return Progress::Done(Err(e)),
            };
            match ids.as_slice() {
                [one] => tree.get(*one),
                [] => {
                    let err = UiError::new(
                        ErrorCode::NotFound,
                        format!("not_found: nothing matches `{}`", l.to_text()),
                        "see the closest elements, or take a snapshot",
                    )
                    .with_elements(l.closest(tree, &view));
                    return fail(p, Check::Attached, err);
                }
                many => return Progress::Done(Err(ambiguous(l, many, tree, &view))),
            }
        }
    };
    if let Some(n) = node {
        p.element = Some(ElementInfo::of(tree, n, &view));
    }
    // Rights before anything else: a refused call delivers nothing and waits for nothing. While a
    // question reserved for the user is open, nothing of an external client reaches the window.
    if let Err(e) = security::refuse_while_reserved_modal(&p.request.actor, tree) {
        return Progress::Done(Err(e));
    }
    let facts = match action {
        Action::Press { keys: text } => match key_facts(text, keys, tree) {
            Ok(f) => f,
            Err(e) => return Progress::Done(Err(e)),
        },
        _ => Vec::new(),
    };
    match authorize(&p.request.actor, action, node, &facts, &p.request.grants) {
        Ok(effects) => p.effects = effects,
        Err(e) => return Progress::Done(Err(e)),
    }
    let level = level.unwrap_or_else(|| action.default_level());
    let point = node.map(|n| action_point(n, action));
    for check in action.checks(force) {
        let Some(n) = node else { break };
        let info = || vec![ElementInfo::of(tree, n, &view)];
        let failed = match check {
            Check::Attached => None,
            Check::Visible => (!n.visible()).then(|| {
                UiError::new(
                    ErrorCode::NotVisible,
                    format!("not_visible: {} shows nothing", describe(n)),
                    "scroll it into view or open what contains it",
                )
            }),
            Check::Stable => {
                // Stable: the same bounds in an earlier frame than this one.
                let b = n.bounds();
                let same = matches!(p.seen, Some((_, id, ob)) if id == n.id && ob.same(&b));
                let stable = same && p.seen.is_some_and(|(f, _, _)| f < tree.frame);
                if !same {
                    p.seen = Some((tree.frame, n.id, b));
                }
                (!stable).then(|| {
                    UiError::new(
                        ErrorCode::DeadlineExceeded,
                        format!("deadline_exceeded: {} did not stop moving", describe(n)),
                        "it changed its bounds in every frame",
                    )
                })
            }
            Check::Enabled => (!n.enabled()).then(|| {
                UiError::new(
                    ErrorCode::NotEnabled,
                    format!("not_enabled: {} is disabled", describe(n)),
                    "use force: true only to test that a disabled element does nothing",
                )
            }),
            Check::Editable => (!n.editable()).then(|| {
                UiError::new(
                    ErrorCode::NotEditable,
                    format!("not_editable: {} takes no text", describe(n)),
                    "type into a text field",
                )
            }),
            Check::ReceivesEvents => {
                let (x, y) = point.unwrap_or_default();
                tree.obscuring(n.id, x, y).map(|o| {
                    UiError::new(
                        ErrorCode::Obscured,
                        format!(
                            "obscured: {} lies over {} at the action point",
                            describe(o),
                            describe(n)
                        ),
                        "close what covers it first",
                    )
                    .with_elements(vec![ElementInfo::of(tree, o, &view)])
                })
            }
        };
        if let Some(err) = failed {
            let err = if err.elements.is_empty() {
                err.with_elements(info())
            } else {
                err
            };
            return fail(p, check, err);
        }
        p.waits.entry(check).or_default();
    }
    let inputs = match plan(action, level, node, point, tree, &view) {
        Ok(i) => i,
        Err(e) => return Progress::Done(Err(e)),
    };
    p.inputs = inputs.clone();
    if p.request.actor.is_external() {
        commands.push(Command::Highlight {
            window: p.window,
            bounds: node.map(UiNode::bounds),
            point: point.filter(|_| action.is_pointer()),
            until: now + HIGHLIGHT,
        });
        p.phase = Phase::Highlight {
            until: now + HIGHLIGHT,
            inputs,
            level,
        };
        return Progress::Wait;
    }
    p.before = Some(tree.clone());
    commands.push(Command::Deliver {
        op: p.id,
        window: p.window,
        inputs,
        level,
        origin: p.request.actor.origin(),
    });
    p.phase = Phase::Delivered { frame: tree.frame };
    Progress::Wait
}

fn describe(n: &UiNode) -> String {
    let mut s = crate::tree::role_name(n.role());
    if let Some(name) = n.name() {
        s.push_str(&format!(" {name:?}"));
    }
    if let Some(t) = n.test_id() {
        s.push_str(&format!(" #{t}"));
    }
    s
}

/// The centre of the visible part, or `position` relative to the top left.
fn action_point(n: &UiNode, action: &Action) -> (f32, f32) {
    let position = match action {
        Action::Click { position, .. }
        | Action::ContextClick { position, .. }
        | Action::Hover { position } => *position,
        _ => None,
    };
    let b = n.bounds();
    match position {
        Some(p) => (b.x + p.x, b.y + p.y),
        None => n.visible_bounds.unwrap_or(b).centre(),
    }
}

fn plan(
    action: &Action,
    level: Level,
    node: Option<&UiNode>,
    point: Option<(f32, f32)>,
    tree: &Tree,
    view: &View,
) -> Result<Vec<Input>, UiError> {
    let unsupported = |what: &str| {
        Err(UiError::new(
            ErrorCode::Unsupported,
            format!("unsupported: `{}` at the {what} level", action.verb()),
            "use the action's default level",
        ))
    };
    let semantic = |name: &str, value: Option<String>| -> Result<Vec<Input>, UiError> {
        let n = node.expect("semantic actions have an element");
        Ok(vec![Input::Semantic {
            element: n.id,
            action: name.to_owned(),
            value,
        }])
    };
    let (x, y) = point.unwrap_or_default();
    // Moves the focus to the element first, unless it has it.
    let focus = |out: &mut Vec<Input>| {
        if let Some(n) = node
            && !tree.is_focused(n.id)
        {
            if level == Level::Semantic {
                out.push(Input::Semantic {
                    element: n.id,
                    action: "focus".into(),
                    value: None,
                });
            } else {
                out.extend(pointer_inputs(
                    &Action::Click {
                        button: Button::Left,
                        count: 1,
                        modifiers: Vec::new(),
                        position: None,
                    },
                    x,
                    y,
                    None,
                ));
            }
        }
    };
    match (action, level) {
        (Action::Click { .. }, Level::Semantic) => semantic("click", None),
        (Action::ContextClick { .. }, Level::Semantic) => semantic("show-context-menu", None),
        (_, Level::Semantic) if action.is_pointer() => unsupported("semantic"),
        (Action::Drag { to, offset }, _) => {
            let end = match (to, offset) {
                (Some(l), _) => {
                    let l = Locator::from_json(l)?;
                    let id = one_now(&l, tree, view)?;
                    let n = tree.get(id).expect("resolved");
                    n.visible_bounds.unwrap_or_else(|| n.bounds()).centre()
                }
                (None, Some(o)) => (x + o.x, y + o.y),
                (None, None) => {
                    return Err(UiError::invalid("invalid: `drag` needs `to` or `offset`"));
                }
            };
            if level == Level::Native {
                return unsupported("native");
            }
            Ok(pointer_inputs(action, x, y, Some(end)))
        }
        (Action::Scroll { .. }, Level::Native) => unsupported("native"),
        (a, _) if a.is_pointer() => Ok(pointer_inputs(a, x, y, None)),
        (Action::Press { keys }, l) => {
            if l == Level::Native {
                return unsupported("native");
            }
            let mut out = Vec::new();
            focus(&mut out);
            out.extend(parse_keys(keys)?.into_iter().map(Input::Key));
            Ok(out)
        }
        (Action::Type { text, clear }, l) => {
            if l == Level::Native {
                return unsupported("native");
            }
            let mut out = Vec::new();
            focus(&mut out);
            if *clear {
                out.push(Input::Key(KeyPress {
                    modifiers: vec![primary()],
                    key: "a".into(),
                }));
                out.push(Input::Key(KeyPress {
                    modifiers: Vec::new(),
                    key: "backspace".into(),
                }));
            }
            out.push(Input::Text { text: text.clone() });
            Ok(out)
        }
        (Action::ScrollIntoView, _) => {
            // Shown already: nothing to do. Scrolling a hidden node needs its view's handler,
            // which GPUI 0.2.2 has no way to reach (phase 3).
            if node.is_some_and(UiNode::visible) {
                Ok(Vec::new())
            } else {
                unsupported("semantic")
            }
        }
        (_, Level::Window | Level::Native) => unsupported(if level == Level::Native {
            "native"
        } else {
            "window"
        }),
        (Action::Focus, Level::Semantic) => semantic("focus", None),
        (Action::Select, Level::Semantic) => semantic("click", None),
        (Action::Expand, Level::Semantic) => semantic("expand", None),
        (Action::Collapse, Level::Semantic) => semantic("collapse", None),
        (Action::Invoke, Level::Semantic) => semantic("click", None),
        (Action::ShowContextMenu, Level::Semantic) => semantic("show-context-menu", None),
        (Action::Increment, Level::Semantic) => semantic("increment", None),
        (Action::Decrement, Level::Semantic) => semantic("decrement", None),
        (Action::SetValue { value }, Level::Semantic) => semantic("set-value", Some(value.clone())),
        (_, Level::Semantic) => unsupported("semantic"),
    }
}

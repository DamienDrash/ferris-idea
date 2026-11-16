//! The Rust API for the self-test and in-process tests (`SPEC-0319` behaviour 28):
//! `driver.role(Role::Button).name("Commit").click().await?`,
//! `expect(driver.id("project-tree")).to_be_visible().await?`.
//!
//! The futures resolve when the window presents frames: the host calls [`Driver::frame`] with the
//! tree of each presented frame and delivers the [`Command`]s it returns. Nothing sleeps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use accesskit::Role;
use serde_json::Value;

use crate::action::{Action, Button, Level, Modifier};
use crate::engine::{Command, Engine, Keys, Op, OpId, Request, Target};
use crate::error::UiError;
use crate::expect::{Compare, Matcher};
use crate::locator::{Locator, TextMatch};
use crate::tree::Tree;

struct Shared {
    engine: Engine,
    window: u64,
    clock: Box<dyn Fn() -> Instant>,
    results: HashMap<OpId, Result<Value, UiError>>,
    wakers: HashMap<OpId, Waker>,
}

/// The driver of one window, for in-process callers (the self-test).
#[derive(Clone)]
pub struct Driver(Rc<RefCell<Shared>>);

impl Driver {
    /// A driver of `window` that reads the time from `clock`.
    pub fn new(window: u64, clock: impl Fn() -> Instant + 'static) -> Self {
        Self(Rc::new(RefCell::new(Shared {
            engine: Engine::new(),
            window,
            clock: Box::new(clock),
            results: HashMap::new(),
            wakers: HashMap::new(),
        })))
    }

    /// Elements of `role`.
    pub fn role(&self, role: Role) -> UiLocator {
        self.locator(Locator::role(role))
    }

    /// The element with test id `id`.
    pub fn id(&self, id: &str) -> UiLocator {
        self.locator(Locator::id(id))
    }

    /// Elements showing `text`.
    pub fn text(&self, text: &str) -> UiLocator {
        self.locator(Locator::text(text))
    }

    /// The input labelled `label`.
    pub fn label(&self, label: &str) -> UiLocator {
        self.locator(Locator::label(label))
    }

    /// A locator in the text or JSON form.
    pub fn locator(&self, locator: Locator) -> UiLocator {
        UiLocator {
            driver: self.clone(),
            locator,
            timeout: None,
        }
    }

    /// Presses keys in the window (no element).
    pub async fn press(&self, keys: &str) -> Result<Value, UiError> {
        self.run(
            Op::Act {
                target: Target::Window,
                action: Action::Press { keys: keys.into() },
                level: None,
                force: false,
            },
            None,
        )
        .await
    }

    /// Advances the waiting calls with the tree of a presented frame; returns what to deliver.
    pub fn frame(&self, tree: &Tree, keys: &dyn Keys) -> Vec<Command> {
        let mut s = self.0.borrow_mut();
        let now = (s.clock)();
        let window = s.window;
        let out = s.engine.frame(window, tree, now, keys);
        for done in out.done {
            s.results.insert(done.op, done.reply.result);
            if let Some(w) = s.wakers.remove(&done.op) {
                w.wake();
            }
        }
        out.commands
    }

    /// Whether calls wait for frames.
    pub fn waiting(&self) -> bool {
        self.0.borrow().engine.pending() > 0
    }

    fn run(&self, op: Op, timeout: Option<Duration>) -> OpFuture {
        let mut s = self.0.borrow_mut();
        let now = (s.clock)();
        let window = s.window;
        let mut request = Request::selftest(op);
        if let Some(t) = timeout {
            request = request.with_timeout(t);
        }
        match s.engine.submit(window, request, now) {
            Ok(id) => OpFuture {
                driver: self.clone(),
                id: Some(id),
                error: None,
            },
            Err(e) => OpFuture {
                driver: self.clone(),
                id: None,
                error: Some(e),
            },
        }
    }
}

/// A call waiting for frames.
pub struct OpFuture {
    driver: Driver,
    id: Option<OpId>,
    error: Option<UiError>,
}

impl Future for OpFuture {
    type Output = Result<Value, UiError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(e) = self.error.take() {
            return Poll::Ready(Err(e));
        }
        let Some(id) = self.id else {
            return Poll::Ready(Err(UiError::new(
                crate::error::ErrorCode::Internal,
                "internal: polled after completion",
                "",
            )));
        };
        let mut s = self.driver.0.borrow_mut();
        match s.results.remove(&id) {
            Some(r) => {
                drop(s);
                self.id = None;
                Poll::Ready(r)
            }
            None => {
                s.wakers.insert(id, cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// A lazy locator bound to a driver.
#[derive(Clone)]
pub struct UiLocator {
    driver: Driver,
    locator: Locator,
    timeout: Option<Duration>,
}

impl UiLocator {
    /// With the exact accessible name `name`.
    pub fn name(mut self, name: &str) -> Self {
        self.locator = self.locator.name(name);
        self
    }

    /// With an accessible name containing `part`.
    pub fn name_contains(mut self, part: &str) -> Self {
        self.locator = self.locator.name_matching(TextMatch::contains(part));
        self
    }

    /// The `n`th match.
    pub fn nth(mut self, n: usize) -> Self {
        self.locator = self.locator.nth(n);
        self
    }

    /// `inner` inside this one.
    pub fn locate(mut self, inner: &UiLocator) -> Self {
        self.locator = self.locator.locate(inner.locator.clone());
        self
    }

    /// Waits at most `timeout` (default 10 s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The locator.
    pub fn locator(&self) -> &Locator {
        &self.locator
    }

    /// Runs `action` on the element at `level` (default: the action's).
    pub async fn act(&self, action: Action, level: Option<Level>) -> Result<Value, UiError> {
        self.driver
            .run(
                Op::Act {
                    target: Target::Locator(self.locator.clone()),
                    action,
                    level,
                    force: false,
                },
                self.timeout,
            )
            .await
    }

    /// A primary click.
    pub async fn click(&self) -> Result<Value, UiError> {
        self.act(
            Action::Click {
                button: Button::Left,
                count: 1,
                modifiers: Vec::new(),
                position: None,
            },
            None,
        )
        .await
    }

    /// A click with `modifiers`.
    pub async fn click_with(&self, modifiers: &[Modifier]) -> Result<Value, UiError> {
        self.act(
            Action::Click {
                button: Button::Left,
                count: 1,
                modifiers: modifiers.to_vec(),
                position: None,
            },
            None,
        )
        .await
    }

    /// A secondary click.
    pub async fn context_click(&self) -> Result<Value, UiError> {
        self.act(
            Action::ContextClick {
                position: None,
                control: false,
            },
            None,
        )
        .await
    }

    /// The pointer rests on it.
    pub async fn hover(&self) -> Result<Value, UiError> {
        self.act(Action::Hover { position: None }, None).await
    }

    /// Keys, after moving the focus to it.
    pub async fn press(&self, keys: &str) -> Result<Value, UiError> {
        self.act(Action::Press { keys: keys.into() }, None).await
    }

    /// Text, through the input handler.
    pub async fn type_text(&self, text: &str) -> Result<Value, UiError> {
        self.act(
            Action::Type {
                text: text.into(),
                clear: false,
            },
            None,
        )
        .await
    }

    /// The current matches, without waiting.
    pub async fn find(&self) -> Result<Value, UiError> {
        self.driver
            .run(
                Op::Find {
                    locator: self.locator.clone(),
                    wait: false,
                },
                self.timeout,
            )
            .await
    }
}

/// Assertions on a locator that retry until their deadline.
pub fn expect(locator: UiLocator) -> Expectation {
    Expectation {
        locator,
        not: false,
    }
}

/// The assertions of [`expect`].
pub struct Expectation {
    locator: UiLocator,
    not: bool,
}

impl Expectation {
    /// Inverts the next assertion.
    pub fn negated(mut self) -> Self {
        self.not = !self.not;
        self
    }

    /// Checks `matcher`.
    pub async fn to(self, matcher: Matcher) -> Result<Value, UiError> {
        self.locator
            .driver
            .run(
                Op::Expect {
                    locator: self.locator.locator.clone(),
                    matcher,
                    not: self.not,
                },
                self.locator.timeout,
            )
            .await
    }

    /// Visible.
    pub async fn to_be_visible(self) -> Result<Value, UiError> {
        self.to(Matcher::Visible).await
    }

    /// Hidden or absent.
    pub async fn to_be_hidden(self) -> Result<Value, UiError> {
        self.to(Matcher::Hidden).await
    }

    /// Enabled.
    pub async fn to_be_enabled(self) -> Result<Value, UiError> {
        self.to(Matcher::Enabled).await
    }

    /// Disabled.
    pub async fn to_be_disabled(self) -> Result<Value, UiError> {
        self.to(Matcher::Disabled).await
    }

    /// The accessible name `name`.
    pub async fn to_have_name(self, name: &str) -> Result<Value, UiError> {
        self.to(Matcher::Name {
            text: name.into(),
            compare: Compare::Exact,
        })
        .await
    }

    /// Exactly `n` matches.
    pub async fn to_have_count(self, n: usize) -> Result<Value, UiError> {
        self.to(Matcher::Count {
            exactly: Some(n),
            at_least: None,
            at_most: None,
        })
        .await
    }
}

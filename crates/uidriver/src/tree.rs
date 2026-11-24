//! The semantic tree of one window (`SPEC-0319` behaviours 1-7): AccessKit nodes with the bounds
//! they were painted at, plus what the driver needs for its security rules (declared effects,
//! reserved elements, secret and terminal text) — kept beside the node, never added to the schema.
//!
//! On GPUI 0.2.2 the window builds the tree itself (the **bridge producer**, ADR-0027 section 6):
//! each annotated element reports its [`Semantics`] and painted bounds to a [`TreeBuilder`] in
//! paint order, and [`TreeBuilder::finish`] derives the hierarchy from the bounds (`SPEC-0319` v2
//! behaviour 1a).

use std::collections::{BTreeMap, BTreeSet};

use accesskit::{Action, Node, Role, Toggled};
use serde::{Deserialize, Serialize};

/// A node id, stable for the node's lifetime (behaviour 4).
pub type ElementId = u64;

/// A rectangle in window coordinates (logical pixels, origin at the top left of the content).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle.
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Whether nothing of it is there.
    pub fn is_empty(&self) -> bool {
        self.w <= 0. || self.h <= 0.
    }

    /// The part inside `clip`, if any.
    pub fn intersect(&self, clip: &Rect) -> Option<Rect> {
        let x = self.x.max(clip.x);
        let y = self.y.max(clip.y);
        let right = (self.x + self.w).min(clip.x + clip.w);
        let bottom = (self.y + self.h).min(clip.y + clip.h);
        (right > x && bottom > y).then_some(Rect::new(x, y, right - x, bottom - y))
    }

    /// Whether `other` lies inside this one (half a pixel of tolerance).
    pub fn contains_rect(&self, other: &Rect) -> bool {
        const E: f32 = 0.5;
        other.x >= self.x - E
            && other.y >= self.y - E
            && other.x + other.w <= self.x + self.w + E
            && other.y + other.h <= self.y + self.h + E
    }

    /// Whether the point lies inside.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    /// Its centre.
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }

    /// Whether both are the same within half a pixel.
    pub fn same(&self, other: &Rect) -> bool {
        const E: f32 = 0.5;
        (self.x - other.x).abs() <= E
            && (self.y - other.y).abs() <= E
            && (self.w - other.w).abs() <= E
            && (self.h - other.h).abs() <= E
    }

    fn area(&self) -> f32 {
        self.w.max(0.) * self.h.max(0.)
    }
}

/// What an element's action does, derived from the shared action ids (behaviour 35). Each needs
/// the agent right of the same name, except `none`; `trust` is reserved for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// Changes nothing outside the window (navigation, showing, selecting).
    None,
    /// Changes files of the workspace (also editing a document that can be saved).
    Write,
    /// Changes the repository or the working tree through version control.
    VcsWrite,
    /// Starts programs.
    Run,
    /// Starts debug sessions.
    Debug,
    /// Fetches, pulls or pushes Git remotes.
    Remote,
    /// Reaches SSH hosts.
    Ssh,
    /// Sends HTTP requests.
    Http,
    /// Trusts the project (reserved for the user).
    Trust,
    /// Quits while work is pending.
    Quit,
    /// Forwards ports of a remote host.
    Ports,
}

impl Effect {
    /// The name, as in the grant file.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Write => "write",
            Self::VcsWrite => "vcs-write",
            Self::Run => "run",
            Self::Debug => "debug",
            Self::Remote => "remote",
            Self::Ssh => "ssh",
            Self::Http => "http",
            Self::Trust => "trust",
            Self::Quit => "quit",
            Self::Ports => "ports",
        }
    }

    /// The agent right the effect needs (`None` for `none`).
    pub fn right(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            other => Some(other.name()),
        }
    }

    /// What it allows, in words for refusals.
    pub fn meaning(self) -> &'static str {
        match self {
            Self::None => "nothing outside the window",
            Self::Write => "create, change, move or delete files of the workspace",
            Self::VcsWrite => "change the repository through version control",
            Self::Run => "start programs",
            Self::Debug => "start debug sessions",
            Self::Remote => "fetch, pull and push Git remotes",
            Self::Ssh => "reach SSH hosts and their files",
            Self::Http => "send HTTP requests",
            Self::Trust => "trust the project",
            Self::Quit => "quit Ferris IDEA while work is pending",
            Self::Ports => "forward ports of a remote project's host",
        }
    }
}

/// Why an element is reserved for the user (behaviour 35).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reserved {
    /// An answer SPEC-0281 reserves: an agent's request for a right, plugin installation and its
    /// rights, "Always Allow" for plugin commands.
    Decision,
    /// The trust question and "Trust Project…".
    Trust,
    /// The Settings pages for agent rights, MCP and UI automation.
    Settings,
    /// A keychain prompt.
    Keychain,
    /// Typing into a secret field.
    Secret,
    /// Reading the system clipboard (Paste and its variants).
    Clipboard,
}

impl Reserved {
    /// The name in errors.
    pub fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Trust => "trust",
            Self::Settings => "settings",
            Self::Keychain => "keychain",
            Self::Secret => "secret",
            Self::Clipboard => "clipboard",
        }
    }
}

/// What kind of text a node's value holds, for masking (behaviour 33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextClass {
    /// Ordinary text.
    #[default]
    Plain,
    /// Terminal output: only for clients with `terminal-read`, masked by the producer (SPEC-0313).
    Terminal,
    /// A document outside the workspace root: no value for external clients.
    OutsideWorkspace,
}

/// How an element describes itself: the annotation a view gives once (behaviour 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Semantics {
    /// AccessKit role.
    pub role: Role,
    /// Accessible name.
    pub name: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Value (dropped for secret nodes).
    pub value: Option<String>,
    /// Stable test id (AccessKit author id).
    pub test_id: Option<String>,
    /// Disabled.
    pub disabled: bool,
    /// Focused.
    pub focused: bool,
    /// Selected.
    pub selected: Option<bool>,
    /// Expanded.
    pub expanded: Option<bool>,
    /// Checked.
    pub checked: Option<bool>,
    /// Read-only.
    pub read_only: bool,
    /// Holds a secret.
    pub secret: bool,
    /// Busy.
    pub busy: bool,
    /// The AccessKit actions it answers.
    pub actions: Vec<Action>,
    /// The effects of its actions; `None`: undeclared.
    pub effects: Option<Vec<Effect>>,
    /// Reserved for the user.
    pub reserved: Option<Reserved>,
    /// Kind of text of its value.
    pub text_class: TextClass,
    /// Painted on top of what was painted before it (menus, dialogs, popups): never the parent of
    /// an element painted earlier.
    pub overlay: bool,
    /// A dialog that keeps the focus and the pointer while it is shown (AccessKit `modal`).
    pub modal: bool,
    /// The keys that give its action without the focus on it, in AccessKit's form (`Enter`,
    /// `Escape`, several separated by spaces).
    pub keyboard_shortcut: Option<String>,
    /// Keys its view handles itself before the keymap (`delete` on the Containers list, `cmd-n`
    /// in the Remote Host window), as the keymap writes them: pressed while it or an element in
    /// it has the focus, they take the effects declared on it and around it instead of their
    /// keymap actions' (`SPEC-0319` v7 behaviour 56).
    pub own_keys: Vec<String>,
}

impl Semantics {
    /// An element with `role`.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            name: None,
            description: None,
            value: None,
            test_id: None,
            disabled: false,
            focused: false,
            selected: None,
            expanded: None,
            checked: None,
            read_only: false,
            secret: role == Role::PasswordInput,
            busy: false,
            actions: Vec::new(),
            effects: None,
            reserved: None,
            text_class: TextClass::Plain,
            overlay: matches!(
                role,
                Role::Menu | Role::Dialog | Role::AlertDialog | Role::Tooltip | Role::Window
            ),
            modal: false,
            keyboard_shortcut: None,
            own_keys: Vec::new(),
        }
    }

    /// An element known only by its test id (a `hit_targets::mark` site, migration step 1): no
    /// role, no name, no actions, so external clients may read it but not operate it.
    pub fn marked(test_id: impl Into<String>) -> Self {
        Self::new(Role::GenericContainer).test_id(test_id)
    }

    /// Sets the accessible name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the description.
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }

    /// Sets the value.
    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = Some(v.into());
        self
    }

    /// Sets the test id.
    pub fn test_id(mut self, id: impl Into<String>) -> Self {
        self.test_id = Some(id.into());
        self
    }

    /// Enabled or disabled.
    pub fn enabled(mut self, on: bool) -> Self {
        self.disabled = !on;
        self
    }

    /// Focused.
    pub fn focused(mut self, on: bool) -> Self {
        self.focused = on;
        self
    }

    /// Selected.
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = Some(on);
        self
    }

    /// Expanded or collapsed.
    pub fn expanded(mut self, on: bool) -> Self {
        self.expanded = Some(on);
        self
    }

    /// Checked.
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }

    /// Read-only.
    pub fn read_only(mut self, on: bool) -> Self {
        self.read_only = on;
        self
    }

    /// Holds a secret: its value never leaves (behaviour 33).
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    /// Busy.
    pub fn busy(mut self, on: bool) -> Self {
        self.busy = on;
        self
    }

    /// Answers `action`.
    pub fn action(mut self, action: Action) -> Self {
        if !self.actions.contains(&action) {
            self.actions.push(action);
        }
        self
    }

    /// Declares the effects of its actions (`&[Effect::None]` for none).
    pub fn effects(mut self, effects: &[Effect]) -> Self {
        let mut list: Vec<Effect> = effects.to_vec();
        list.sort();
        list.dedup();
        self.effects = Some(list);
        self
    }

    /// Reserved for the user.
    pub fn reserved(mut self, why: Reserved) -> Self {
        self.reserved = Some(why);
        self
    }

    /// The kind of its text.
    pub fn text_class(mut self, class: TextClass) -> Self {
        self.text_class = class;
        self
    }

    /// Drawn on top of what came before (menus, popups, dialogs).
    pub fn overlay(mut self, on: bool) -> Self {
        self.overlay = on;
        self
    }

    /// A modal dialog: it keeps the focus and the pointer while it is shown.
    pub fn modal(mut self) -> Self {
        self.modal = true;
        self
    }

    /// The keys that give its action (`Enter`, `Escape`).
    pub fn keyboard_shortcut(mut self, keys: impl Into<String>) -> Self {
        self.keyboard_shortcut = Some(keys.into());
        self
    }

    /// Keys its view handles itself before the keymap, as the keymap writes them (`delete`,
    /// `cmd-backspace`); they take its declared effects (see [`Semantics::own_keys`]).
    pub fn handles_keys(mut self, keys: &[&str]) -> Self {
        self.own_keys = keys.iter().map(|k| (*k).to_owned()).collect();
        self
    }
}

/// One node of a window's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct UiNode {
    /// Its id.
    pub id: ElementId,
    /// The AccessKit node: role, name (label), description, value, author id, bounds, states and
    /// actions.
    pub node: Node,
    /// Its parent (`None` for top-level nodes).
    pub parent: Option<ElementId>,
    /// Its children in paint order.
    pub children: Vec<ElementId>,
    /// Bounds clipped by the scroll areas and windows around it; `None` when nothing is shown.
    pub visible_bounds: Option<Rect>,
    /// Holds a secret.
    pub secret: bool,
    /// Whether its value was empty (secret nodes say only this).
    pub empty: bool,
    /// Declared effects; `None`: undeclared.
    pub effects: Option<Vec<Effect>>,
    /// Reserved for the user.
    pub reserved: Option<Reserved>,
    /// Kind of text of its value.
    pub text_class: TextClass,
    /// Drawn on top of what came before.
    pub overlay: bool,
    /// Keys its view handles itself before the keymap (see [`Semantics::own_keys`]).
    pub own_keys: Vec<String>,
    /// Position in paint order.
    pub order: usize,
}

impl UiNode {
    /// AccessKit role.
    pub fn role(&self) -> Role {
        self.node.role()
    }

    /// Accessible name.
    pub fn name(&self) -> Option<&str> {
        self.node.label()
    }

    /// Test id (AccessKit author id).
    pub fn test_id(&self) -> Option<&str> {
        self.node.author_id()
    }

    /// Value as the producer gave it (never set for secret nodes).
    pub fn raw_value(&self) -> Option<&str> {
        self.node.value()
    }

    /// Painted bounds, unclipped.
    pub fn bounds(&self) -> Rect {
        self.node
            .bounds()
            .map(|r| {
                Rect::new(
                    r.x0 as f32,
                    r.y0 as f32,
                    (r.x1 - r.x0) as f32,
                    (r.y1 - r.y0) as f32,
                )
            })
            .unwrap_or_default()
    }

    /// Whether anything of it is shown.
    pub fn visible(&self) -> bool {
        !self.node.is_hidden() && self.visible_bounds.is_some_and(|b| !b.is_empty())
    }

    /// Whether it is enabled.
    pub fn enabled(&self) -> bool {
        !self.node.is_disabled()
    }

    /// Checked state, if it has one.
    pub fn checked(&self) -> Option<bool> {
        self.node.toggled().map(|t| t == Toggled::True)
    }

    /// Selected state, if it has one.
    pub fn selected(&self) -> Option<bool> {
        self.node.is_selected()
    }

    /// Expanded state, if it has one.
    pub fn expanded(&self) -> Option<bool> {
        self.node.is_expanded()
    }

    /// Whether it takes text.
    pub fn editable(&self) -> bool {
        is_text_input(self.role()) && !self.node.is_read_only() && self.enabled()
    }

    /// Whether it declares an action for pointers or a semantic request.
    pub fn actionable(&self) -> bool {
        self.effects.is_some()
            || [
                Action::Click,
                Action::Expand,
                Action::Collapse,
                Action::SetValue,
                Action::Increment,
                Action::Decrement,
                Action::ShowContextMenu,
            ]
            .iter()
            .any(|a| self.node.supports_action(*a))
    }

    /// The AccessKit actions it answers, by name.
    pub fn action_names(&self) -> Vec<&'static str> {
        ACTION_NAMES
            .iter()
            .filter(|(a, _)| self.node.supports_action(*a))
            .map(|(_, n)| *n)
            .collect()
    }
}

/// AccessKit actions with the names the driver uses.
pub const ACTION_NAMES: [(Action, &str); 10] = [
    (Action::Click, "click"),
    (Action::Focus, "focus"),
    (Action::Expand, "expand"),
    (Action::Collapse, "collapse"),
    (Action::SetValue, "set-value"),
    (Action::Increment, "increment"),
    (Action::Decrement, "decrement"),
    (Action::ShowContextMenu, "show-context-menu"),
    (Action::ScrollIntoView, "scroll-into-view"),
    (Action::ReplaceSelectedText, "replace-selected-text"),
];

/// Whether `role` takes text.
pub fn is_text_input(role: Role) -> bool {
    matches!(
        role,
        Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::PasswordInput
            | Role::EmailInput
            | Role::NumberInput
            | Role::UrlInput
            | Role::PhoneNumberInput
            | Role::DateInput
            | Role::DateTimeInput
            | Role::TimeInput
            | Role::Terminal
    )
}

/// The lower-case name of `role` (`button`, `treeitem`, `passwordinput`).
pub fn role_name(role: Role) -> String {
    format!("{role:?}").to_ascii_lowercase()
}

/// The role of a name as locators write it (`button`, `treeitem`, `tree-item`, `textbox`).
pub fn parse_role(name: &str) -> Option<Role> {
    let wanted: String = name
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .collect::<String>()
        .to_ascii_lowercase();
    match wanted.as_str() {
        "textbox" | "textfield" => return Some(Role::TextInput),
        "password" => return Some(Role::PasswordInput),
        "combobox" => return Some(Role::ComboBox),
        _ => {}
    }
    (0..=u8::MAX)
        .filter_map(Role::n)
        .find(|r| role_name(*r) == wanted)
}

/// The tree of one window, as the last presented frame painted it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tree {
    nodes: BTreeMap<ElementId, UiNode>,
    /// Ids in paint order.
    order: Vec<ElementId>,
    /// The focused node: of the nodes annotated as focused the last painted (the innermost).
    focused_id: Option<ElementId>,
    /// The frame that painted it.
    pub frame: u64,
}

impl Tree {
    /// An empty tree of `frame`.
    pub fn empty(frame: u64) -> Self {
        Self {
            frame,
            ..Self::default()
        }
    }

    /// The node `id`.
    pub fn get(&self, id: ElementId) -> Option<&UiNode> {
        self.nodes.get(&id)
    }

    /// Every node in paint order.
    pub fn iter(&self) -> impl Iterator<Item = &UiNode> {
        self.order.iter().filter_map(|id| self.nodes.get(id))
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether it has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The top-level nodes in paint order.
    pub fn roots(&self) -> Vec<ElementId> {
        self.iter()
            .filter(|n| n.parent.is_none())
            .map(|n| n.id)
            .collect()
    }

    /// The focused node. AccessKit keeps the focus on the tree, not on a node, so the tree keeps
    /// it here too.
    pub fn focused(&self) -> Option<&UiNode> {
        self.focused_id.and_then(|id| self.nodes.get(&id))
    }

    /// The shown modal dialog that is reserved for the user, if any — the question the toolkit
    /// draws where the platform has no alert (`SPEC-0281` v4 behaviour 9): while it is shown, no
    /// external client acts in the window (behaviour 35, v6).
    pub fn reserved_modal(&self) -> Option<&UiNode> {
        self.iter()
            .filter(|n| n.reserved.is_some() && n.node.is_modal() && n.visible())
            .last()
    }

    /// Whether `id` has the focus.
    pub fn is_focused(&self, id: ElementId) -> bool {
        self.focused_id == Some(id)
    }

    /// Whether `ancestor` is `id` or one of its ancestors.
    pub fn is_ancestor_or_self(&self, ancestor: ElementId, mut id: ElementId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            match self.nodes.get(&id).and_then(|n| n.parent) {
                Some(p) => id = p,
                None => return false,
            }
        }
    }

    /// The descendants of `id` (not itself), in paint order.
    pub fn descendants(&self, id: ElementId) -> Vec<ElementId> {
        self.iter()
            .filter(|n| n.id != id && self.is_ancestor_or_self(id, n.id))
            .map(|n| n.id)
            .collect()
    }

    /// The first visible node painted after `target` at the point that is neither an ancestor
    /// nor a descendant of `target` (behaviour 12, "receives the event").
    pub fn obscuring(&self, target: ElementId, x: f32, y: f32) -> Option<&UiNode> {
        let t = self.nodes.get(&target)?;
        self.iter()
            .filter(|n| n.order > t.order)
            .filter(|n| n.visible_bounds.is_some_and(|b| b.contains(x, y)))
            .filter(|n| {
                !self.is_ancestor_or_self(n.id, target) && !self.is_ancestor_or_self(target, n.id)
            })
            .last()
    }

    /// Text of the node and its descendants, values of secret nodes left out.
    pub fn text_of(&self, id: ElementId, view: &View) -> String {
        let mut parts = Vec::new();
        for n in std::iter::once(id).chain(self.descendants(id)) {
            if let Some(node) = self.nodes.get(&n) {
                if let Some(name) = node.name() {
                    parts.push(name.to_owned());
                }
                if let Some(v) = view.value_of(node) {
                    parts.push(v.to_owned());
                }
            }
        }
        parts.join(" ")
    }
}

/// Who reads the tree, for masking (behaviour 33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct View {
    /// An external client (not the in-process self-test).
    pub external: bool,
    /// The client has `terminal-read`.
    pub terminal_read: bool,
}

impl View {
    /// The in-process view (the self-test).
    pub fn in_process() -> Self {
        Self {
            external: false,
            terminal_read: true,
        }
    }

    /// The value `node` shows this reader, if any.
    pub fn value_of<'a>(&self, node: &'a UiNode) -> Option<&'a str> {
        if node.secret {
            return None;
        }
        match node.text_class {
            TextClass::Terminal if !self.terminal_read => None,
            TextClass::OutsideWorkspace if self.external => None,
            _ => node.raw_value(),
        }
    }
}

/// An element as queries, errors and traces show it (behaviour 23), masked for its reader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementInfo {
    /// Node id.
    pub id: ElementId,
    /// Role name.
    pub role: String,
    /// Accessible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Test id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    /// Value, masked (never for secret nodes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Secret node.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
    /// For secret nodes: whether the value is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<bool>,
    /// States that hold (`disabled`, `focused`, `selected`, `expanded`, `collapsed`, `checked`,
    /// `unchecked`, `read-only`, `busy`, `hidden`).
    pub states: Vec<String>,
    /// Painted bounds.
    pub bounds: Rect,
    /// Visible part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_bounds: Option<Rect>,
    /// AccessKit actions by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    /// Declared effects (`None`: undeclared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<Vec<Effect>>,
    /// Reserved for the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved: Option<Reserved>,
}

impl ElementInfo {
    /// `node` of `tree` as `view` may see it.
    pub fn of(tree: &Tree, node: &UiNode, view: &View) -> Self {
        let mut states = Vec::new();
        let n = &node.node;
        if n.is_disabled() {
            states.push("disabled".into());
        }
        if tree.is_focused(node.id) {
            states.push("focused".into());
        }
        if n.is_selected() == Some(true) {
            states.push("selected".into());
        }
        match n.is_expanded() {
            Some(true) => states.push("expanded".into()),
            Some(false) => states.push("collapsed".into()),
            None => {}
        }
        match node.checked() {
            Some(true) => states.push("checked".into()),
            Some(false) => states.push("unchecked".into()),
            None => {}
        }
        if n.is_read_only() {
            states.push("read-only".into());
        }
        if n.is_busy() {
            states.push("busy".into());
        }
        if !node.visible() {
            states.push("hidden".into());
        }
        Self {
            id: node.id,
            role: role_name(node.role()),
            name: node.name().map(str::to_owned),
            test_id: node.test_id().map(str::to_owned),
            value: view.value_of(node).map(str::to_owned),
            description: n.description().map(str::to_owned),
            secret: node.secret,
            empty: node.secret.then_some(node.empty),
            states,
            bounds: node.bounds(),
            visible_bounds: node.visible_bounds,
            actions: node.action_names().into_iter().map(str::to_owned).collect(),
            effects: node.effects.clone(),
            reserved: node.reserved,
        }
    }

    /// One line of the text outline: `role "name" #id [states]`.
    pub fn outline(&self) -> String {
        let mut line = self.role.clone();
        if let Some(n) = &self.name {
            line.push_str(&format!(" {n:?}"));
        }
        if let Some(id) = &self.test_id {
            line.push_str(&format!(" #{id}"));
        }
        if let Some(v) = &self.value {
            let short: String = v.chars().take(60).collect();
            line.push_str(&format!(" value={short:?}"));
        }
        if self.secret {
            line.push_str(if self.empty == Some(true) {
                " (secret, empty)"
            } else {
                " (secret)"
            });
        }
        if !self.states.is_empty() {
            line.push_str(&format!(" [{}]", self.states.join(", ")));
        }
        line
    }
}

/// Collects the nodes one frame paints (the bridge producer's toolkit-free part).
#[derive(Debug, Default)]
pub struct TreeBuilder {
    entries: Vec<(Semantics, Rect, Option<Rect>)>,
}

impl TreeBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an element painted at `bounds` whose visible part (clipped by the scroll areas
    /// and windows around it) is `visible`.
    pub fn push(&mut self, semantics: Semantics, bounds: Rect, visible: Option<Rect>) {
        self.entries.push((semantics, bounds, visible));
    }

    /// Number of elements recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The tree of frame `frame`. Ids come from test ids (else role, name and the position among
    /// equal siblings), so a node keeps its id from frame to frame. A later element with the same
    /// test id as an earlier one replaces it (an element painted twice in one frame).
    pub fn finish(self, frame: u64) -> Tree {
        let mut tree = Tree::empty(frame);
        let mut seen_ids: BTreeMap<ElementId, usize> = BTreeMap::new();
        let mut ordinal: BTreeMap<String, usize> = BTreeMap::new();
        let mut list: Vec<UiNode> = Vec::new();
        let mut focus: BTreeSet<ElementId> = BTreeSet::new();
        for (sem, bounds, visible) in self.entries {
            let key = match &sem.test_id {
                Some(t) => format!("id:{t}"),
                None => {
                    let base = format!(
                        "anon:{}:{}",
                        role_name(sem.role),
                        sem.name.as_deref().unwrap_or("")
                    );
                    let n = ordinal.entry(base.clone()).or_insert(0);
                    *n += 1;
                    format!("{base}:{n}")
                }
            };
            let id = fnv(&key);
            let mut node = Node::new(sem.role);
            if let Some(n) = &sem.name {
                node.set_label(n.as_str());
            }
            if let Some(d) = &sem.description {
                node.set_description(d.as_str());
            }
            let empty = sem.value.as_deref().is_none_or(str::is_empty);
            if !sem.secret
                && let Some(v) = &sem.value
            {
                node.set_value(v.as_str());
            }
            if let Some(t) = &sem.test_id {
                node.set_author_id(t.as_str());
            }
            node.set_bounds(accesskit::Rect {
                x0: f64::from(bounds.x),
                y0: f64::from(bounds.y),
                x1: f64::from(bounds.x + bounds.w),
                y1: f64::from(bounds.y + bounds.h),
            });
            if sem.disabled {
                node.set_disabled();
            }
            if sem.read_only {
                node.set_read_only();
            }
            if sem.busy {
                node.set_busy();
            }
            if sem.modal {
                node.set_modal();
            }
            if let Some(k) = &sem.keyboard_shortcut {
                node.set_keyboard_shortcut(k.as_str());
            }
            if let Some(s) = sem.selected {
                node.set_selected(s);
            }
            if let Some(e) = sem.expanded {
                node.set_expanded(e);
            }
            if let Some(c) = sem.checked {
                node.set_toggled(if c { Toggled::True } else { Toggled::False });
            }
            if visible.is_none() {
                node.set_hidden();
            }
            for a in &sem.actions {
                node.add_action(*a);
            }
            if sem.focused {
                focus.insert(id);
            } else {
                focus.remove(&id);
            }
            let ui = UiNode {
                id,
                node,
                parent: None,
                children: Vec::new(),
                visible_bounds: visible,
                secret: sem.secret,
                empty,
                effects: sem.effects,
                reserved: sem.reserved,
                text_class: sem.text_class,
                overlay: sem.overlay,
                own_keys: sem.own_keys,
                order: 0,
            };
            match seen_ids.get(&id) {
                Some(&i) => list[i] = ui,
                None => {
                    seen_ids.insert(id, list.len());
                    list.push(ui);
                }
            }
        }
        for (i, n) in list.iter_mut().enumerate() {
            n.order = i;
        }
        // Parents from the bounds: the smallest element that contains a node; of equal ones the
        // earlier. An overlay is never the parent of an element painted before it.
        let parents: Vec<Option<ElementId>> = (0..list.len())
            .map(|i| {
                let b = list[i].bounds();
                list.iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .filter(|(j, p)| {
                        let pb = p.bounds();
                        pb.contains_rect(&b) && (!pb.same(&b) || *j < i) && !(p.overlay && *j > i)
                    })
                    .min_by(|(ja, a), (jb, b2)| {
                        a.bounds()
                            .area()
                            .total_cmp(&b2.bounds().area())
                            .then(jb.cmp(ja))
                    })
                    .map(|(_, p)| p.id)
            })
            .collect();
        // Equal bounds could make two nodes each other's parent; the earlier one wins.
        for (i, parent) in parents.into_iter().enumerate() {
            list[i].parent = parent;
        }
        let ids: Vec<ElementId> = list.iter().map(|n| n.id).collect();
        tree.focused_id = ids.iter().rev().find(|id| focus.contains(id)).copied();
        tree.order = ids;
        for n in list {
            tree.nodes.insert(n.id, n);
        }
        let links: Vec<(ElementId, ElementId)> = tree
            .order
            .iter()
            .filter_map(|id| tree.nodes[id].parent.map(|p| (p, *id)))
            .collect();
        for (p, c) in links {
            if let Some(parent) = tree.nodes.get_mut(&p) {
                parent.children.push(c);
            }
        }
        tree
    }
}

/// FNV-1a of `key`, as the node id.
fn fnv(key: &str) -> ElementId {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // AccessKit reserves no ids, but 0 reads like "none" in logs.
    h.max(1)
}

impl Tree {
    /// The tree in the text outline of behaviour 24: `role "name" #id [states]`, indented by
    /// depth, limited to `max_nodes`; the second value says how many were left out.
    pub fn outline(
        &self,
        from: Option<ElementId>,
        depth: usize,
        max_nodes: usize,
        view: &View,
    ) -> (String, usize) {
        let mut out = String::new();
        let mut count = 0usize;
        let mut left = 0usize;
        let starts = match from {
            Some(id) => vec![id],
            None => self.roots(),
        };
        let mut stack: Vec<(ElementId, usize)> = starts.into_iter().rev().map(|i| (i, 0)).collect();
        while let Some((id, d)) = stack.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if count >= max_nodes {
                left += 1;
                continue;
            }
            count += 1;
            out.push_str(&"  ".repeat(d));
            out.push_str(&ElementInfo::of(self, node, view).outline());
            out.push('\n');
            if d + 1 < depth {
                for c in node.children.iter().rev() {
                    stack.push((*c, d + 1));
                }
            } else {
                left += self.descendants(id).len();
            }
        }
        (out, left)
    }

    /// The tree as JSON: nested elements, limited by `depth` and `max_nodes`.
    pub fn to_json(
        &self,
        from: Option<ElementId>,
        depth: usize,
        max_nodes: usize,
        view: &View,
    ) -> (serde_json::Value, usize) {
        let mut budget = max_nodes;
        let mut left = 0usize;
        let starts = match from {
            Some(id) => vec![id],
            None => self.roots(),
        };
        let nodes: Vec<serde_json::Value> = starts
            .into_iter()
            .filter_map(|id| self.json_node(id, 0, depth, &mut budget, &mut left, view))
            .collect();
        (serde_json::Value::Array(nodes), left)
    }

    fn json_node(
        &self,
        id: ElementId,
        d: usize,
        depth: usize,
        budget: &mut usize,
        left: &mut usize,
        view: &View,
    ) -> Option<serde_json::Value> {
        let node = self.nodes.get(&id)?;
        if *budget == 0 {
            *left += 1 + self.descendants(id).len();
            return None;
        }
        *budget -= 1;
        let mut value = serde_json::to_value(ElementInfo::of(self, node, view)).ok()?;
        if d + 1 < depth {
            let children: Vec<serde_json::Value> = node
                .children
                .iter()
                .filter_map(|c| self.json_node(*c, d + 1, depth, budget, left, view))
                .collect();
            if !children.is_empty() {
                value["children"] = serde_json::Value::Array(children);
            }
        } else {
            *left += self.descendants(id).len();
        }
        Some(value)
    }

    /// What changed from `before` to this tree: added, removed and changed nodes with the changed
    /// properties, masked for `view` (behaviour 27).
    pub fn diff(&self, before: &Tree, view: &View) -> TreeDiff {
        let old: BTreeSet<ElementId> = before.nodes.keys().copied().collect();
        let new: BTreeSet<ElementId> = self.nodes.keys().copied().collect();
        let info = |t: &Tree, id: &ElementId| ElementInfo::of(t, &t.nodes[id], view);
        let mut changed = Vec::new();
        for id in old.intersection(&new) {
            let a = info(before, id);
            let b = info(self, id);
            if a == b {
                continue;
            }
            let mut props = Vec::new();
            if a.name != b.name {
                props.push("name");
            }
            if a.value != b.value || a.empty != b.empty {
                props.push("value");
            }
            if a.states != b.states {
                props.push("states");
            }
            if a.bounds != b.bounds || a.visible_bounds != b.visible_bounds {
                props.push("bounds");
            }
            if a.actions != b.actions || a.effects != b.effects {
                props.push("actions");
            }
            if props.is_empty() {
                props.push("other");
            }
            changed.push(ChangedNode {
                element: b,
                properties: props.into_iter().map(str::to_owned).collect(),
            });
        }
        TreeDiff {
            added: new.difference(&old).map(|id| info(self, id)).collect(),
            removed: old.difference(&new).copied().collect(),
            changed,
        }
    }
}

/// Tree changes between two frames.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TreeDiff {
    /// New nodes.
    pub added: Vec<ElementInfo>,
    /// Ids of nodes that are gone.
    pub removed: Vec<ElementId>,
    /// Nodes whose properties changed.
    pub changed: Vec<ChangedNode>,
}

impl TreeDiff {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// A node and the properties of it that changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangedNode {
    /// The node now.
    pub element: ElementInfo,
    /// `name`, `value`, `states`, `bounds`, `actions`.
    pub properties: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn roles_parse_as_locators_write_them() {
        assert_eq!(parse_role("button"), Some(Role::Button));
        assert_eq!(parse_role("treeitem"), Some(Role::TreeItem));
        assert_eq!(parse_role("tree-item"), Some(Role::TreeItem));
        assert_eq!(parse_role("menuitem"), Some(Role::MenuItem));
        assert_eq!(parse_role("textbox"), Some(Role::TextInput));
        assert_eq!(parse_role("PasswordInput"), Some(Role::PasswordInput));
        assert_eq!(parse_role("nonsense"), None);
        assert_eq!(role_name(Role::MultilineTextInput), "multilinetextinput");
    }

    #[test]
    fn parents_come_from_bounds_and_overlays_cover() {
        let mut b = TreeBuilder::new();
        let window = r(0., 0., 800., 600.);
        b.push(
            Semantics::new(Role::Tree).test_id("project-tree"),
            r(0., 40., 200., 500.),
            Some(r(0., 40., 200., 500.)),
        );
        b.push(
            Semantics::new(Role::TreeItem)
                .name("main.rs")
                .test_id("tree-main.rs"),
            r(0., 60., 200., 20.),
            Some(r(0., 60., 200., 20.)),
        );
        // The same row marked twice (row id and file name): the second is the first's child.
        b.push(
            Semantics::marked("tree-row-3"),
            r(0., 60., 200., 20.),
            Some(r(0., 60., 200., 20.)),
        );
        // A menu painted later over the tree.
        b.push(
            Semantics::new(Role::Menu).test_id("context-menu"),
            r(10., 50., 300., 200.),
            Some(r(10., 50., 300., 200.)),
        );
        b.push(
            Semantics::new(Role::MenuItem)
                .name("Copy")
                .test_id("menu-Copy"),
            r(10., 60., 300., 20.),
            Some(r(10., 60., 300., 20.)),
        );
        let _ = window;
        let t = b.finish(7);
        let id = |s: &str| t.iter().find(|n| n.test_id() == Some(s)).unwrap().id;
        assert_eq!(
            t.get(id("tree-main.rs")).unwrap().parent,
            Some(id("project-tree"))
        );
        assert_eq!(
            t.get(id("tree-row-3")).unwrap().parent,
            Some(id("tree-main.rs"))
        );
        assert_eq!(
            t.get(id("menu-Copy")).unwrap().parent,
            Some(id("context-menu"))
        );
        // The menu is not the tree's parent though it was painted over it.
        assert_eq!(t.get(id("project-tree")).unwrap().parent, None);
        // At the row's centre the menu item covers the row.
        let (x, y) = t.get(id("tree-main.rs")).unwrap().bounds().centre();
        assert_eq!(
            t.obscuring(id("tree-main.rs"), x, y).map(|n| n.id),
            Some(id("menu-Copy"))
        );
        // The row's own second mark does not count as covering it.
        assert_eq!(t.obscuring(id("menu-Copy"), 150., 70.), None);
        assert_eq!(t.frame, 7);
    }

    #[test]
    fn secret_values_never_enter_the_tree() {
        let mut b = TreeBuilder::new();
        b.push(
            Semantics::new(Role::PasswordInput)
                .name("Password")
                .value("hunter2-known-secret"),
            r(0., 0., 100., 20.),
            Some(r(0., 0., 100., 20.)),
        );
        b.push(
            Semantics::new(Role::TextInput)
                .name("Token")
                .value("tok-known-secret")
                .secret(),
            r(0., 30., 100., 20.),
            Some(r(0., 30., 100., 20.)),
        );
        let t = b.finish(1);
        let all = format!("{t:?}");
        assert!(!all.contains("known-secret"), "{all}");
        let view = View::in_process();
        let (json, _) = t.to_json(None, 10, 100, &view);
        assert!(!json.to_string().contains("known-secret"));
        assert_eq!(json[0]["secret"], true);
        assert_eq!(json[0]["empty"], false);
    }

    #[test]
    fn hidden_and_clipped_nodes() {
        let mut b = TreeBuilder::new();
        b.push(Semantics::marked("gone"), r(0., 300., 10., 10.), None);
        b.push(
            Semantics::marked("cut"),
            r(0., 0., 10., 30.),
            Some(r(0., 0., 10., 16.)),
        );
        let t = b.finish(1);
        let gone = t.iter().find(|n| n.test_id() == Some("gone")).unwrap();
        assert!(!gone.visible());
        let cut = t.iter().find(|n| n.test_id() == Some("cut")).unwrap();
        assert!(cut.visible());
        assert_eq!(cut.visible_bounds, Some(r(0., 0., 10., 16.)));
    }

    #[test]
    fn ids_are_stable_across_frames() {
        let build = |frame| {
            let mut b = TreeBuilder::new();
            b.push(
                Semantics::new(Role::Button).name("Commit"),
                r(0., 0., 10., 10.),
                Some(r(0., 0., 10., 10.)),
            );
            b.push(
                Semantics::marked("x"),
                r(20., 0., 10., 10.),
                Some(r(20., 0., 10., 10.)),
            );
            b.finish(frame)
        };
        let (a, b) = (build(1), build(2));
        let ids = |t: &Tree| t.iter().map(|n| n.id).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
        assert!(b.diff(&a, &View::in_process()).is_empty());
    }
}

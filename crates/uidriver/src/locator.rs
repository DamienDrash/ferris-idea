//! Locators (`SPEC-0319` behaviours 8-11): what a user perceives — role and accessible name, test
//! id, visible text, label — with state filters, `has`, `within` and `nth`. Lazy: a locator is
//! resolved again for every action and every retry, so it never goes stale.
//!
//! Text form: `role=button[name="Commit"]`, `id=project-tree >> role=treeitem[name="main.rs"]`,
//! `text="No problems"`, `name~="…"` (substring), `name=/…/` (regular expression, `/…/i` ignores
//! case), `[ignore_case]`, `[enabled]`, `[checked=false]`, `[nth=2]`, `[has="role=button"]`. The
//! JSON form is the same structure as an object; both parse into [`Locator`].

use accesskit::Role;
use serde_json::{Map, Value, json};

use crate::error::{ErrorCode, UiError};
use crate::tree::{ElementId, ElementInfo, Tree, View, parse_role, role_name};

/// How a name or text is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// The whole text.
    Exact,
    /// A part of it.
    Contains,
    /// A regular expression.
    Regex,
}

/// A text to compare with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatch {
    /// How.
    pub mode: MatchMode,
    /// What.
    pub text: String,
    /// Ignore case.
    pub ignore_case: bool,
}

impl TextMatch {
    /// The exact text.
    pub fn exact(text: impl Into<String>) -> Self {
        Self {
            mode: MatchMode::Exact,
            text: text.into(),
            ignore_case: false,
        }
    }

    /// A substring.
    pub fn contains(text: impl Into<String>) -> Self {
        Self {
            mode: MatchMode::Contains,
            text: text.into(),
            ignore_case: false,
        }
    }

    /// A regular expression.
    pub fn regex(text: impl Into<String>) -> Self {
        Self {
            mode: MatchMode::Regex,
            text: text.into(),
            ignore_case: false,
        }
    }

    /// Checks the regular expression.
    pub fn validate(&self) -> Result<(), UiError> {
        if self.mode == MatchMode::Regex {
            self.compiled()?;
        }
        Ok(())
    }

    fn compiled(&self) -> Result<regex::Regex, UiError> {
        regex::RegexBuilder::new(&self.text)
            .case_insensitive(self.ignore_case)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| {
                let why = e
                    .to_string()
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                UiError::invalid(format!("invalid: not a regular expression ({why})"))
            })
    }

    /// Whether `value` matches.
    pub fn matches(&self, value: &str) -> bool {
        match self.mode {
            MatchMode::Exact if self.ignore_case => {
                value.to_lowercase() == self.text.to_lowercase()
            }
            MatchMode::Exact => value == self.text,
            MatchMode::Contains if self.ignore_case => {
                value.to_lowercase().contains(&self.text.to_lowercase())
            }
            MatchMode::Contains => value.contains(&self.text),
            MatchMode::Regex => self.compiled().is_ok_and(|r| r.is_match(value)),
        }
    }

    fn to_text(&self, attr: &str) -> String {
        match self.mode {
            MatchMode::Exact => format!("{attr}={}", quote(&self.text)),
            MatchMode::Contains => format!("{attr}~={}", quote(&self.text)),
            MatchMode::Regex => format!(
                "{attr}=/{}/{}",
                self.text.replace('/', "\\/"),
                if self.ignore_case { "i" } else { "" }
            ),
        }
    }

    fn mode_name(&self) -> &'static str {
        match self.mode {
            MatchMode::Exact => "exact",
            MatchMode::Contains => "contains",
            MatchMode::Regex => "regex",
        }
    }
}

/// What a selector looks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// An element of `role`, optionally with an accessible name.
    Role {
        /// The role.
        role: Role,
        /// Its accessible name.
        name: Option<TextMatch>,
    },
    /// The element with this test id.
    Id(String),
    /// An element whose own name or value shows this text.
    Text(TextMatch),
    /// The input a label names.
    Label(TextMatch),
}

/// State filters (behaviour 8).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filters {
    /// Enabled (true) or disabled (false).
    pub enabled: Option<bool>,
    /// Checked.
    pub checked: Option<bool>,
    /// Selected.
    pub selected: Option<bool>,
    /// Expanded.
    pub expanded: Option<bool>,
    /// Focused.
    pub focused: Option<bool>,
    /// Has a descendant matching this locator.
    pub has: Option<Box<Locator>>,
    /// The nth match, 0-based, only when asked for.
    pub nth: Option<usize>,
}

/// One step of a locator.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// What it looks for.
    pub kind: Kind,
    /// Its filters.
    pub filters: Filters,
}

/// A locator: steps, each scoped to the matches of the one before (`>>`, `within`).
#[derive(Debug, Clone, PartialEq)]
pub struct Locator {
    /// The steps, outermost first.
    pub steps: Vec<Selector>,
}

impl Locator {
    /// Elements of `role` (any name).
    pub fn role(role: Role) -> Self {
        Self::of(Kind::Role { role, name: None })
    }

    /// The element with test id `id`.
    pub fn id(id: impl Into<String>) -> Self {
        Self::of(Kind::Id(id.into()))
    }

    /// Elements showing exactly `text`.
    pub fn text(text: impl Into<String>) -> Self {
        Self::of(Kind::Text(TextMatch::exact(text)))
    }

    /// The input labelled exactly `label`.
    pub fn label(label: impl Into<String>) -> Self {
        Self::of(Kind::Label(TextMatch::exact(label)))
    }

    fn of(kind: Kind) -> Self {
        Self {
            steps: vec![Selector {
                kind,
                filters: Filters::default(),
            }],
        }
    }

    fn last(&mut self) -> &mut Selector {
        self.steps.last_mut().expect("a locator has a step")
    }

    /// With the exact accessible name `name` (role locators).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        if let Kind::Role { name: n, .. } = &mut self.last().kind {
            *n = Some(TextMatch::exact(name));
        }
        self
    }

    /// With an accessible name matching `m` (role locators).
    pub fn name_matching(mut self, m: TextMatch) -> Self {
        if let Kind::Role { name: n, .. } = &mut self.last().kind {
            *n = Some(m);
        }
        self
    }

    /// Only enabled (true) or disabled (false) elements.
    pub fn enabled(mut self, on: bool) -> Self {
        self.last().filters.enabled = Some(on);
        self
    }

    /// Only checked or unchecked elements.
    pub fn checked(mut self, on: bool) -> Self {
        self.last().filters.checked = Some(on);
        self
    }

    /// Only elements with a descendant matching `inner`.
    pub fn has(mut self, inner: Locator) -> Self {
        self.last().filters.has = Some(Box::new(inner));
        self
    }

    /// The `n`th match (0-based).
    pub fn nth(mut self, n: usize) -> Self {
        self.last().filters.nth = Some(n);
        self
    }

    /// `inner` inside the matches of this locator (`>>`).
    pub fn locate(mut self, inner: Locator) -> Self {
        self.steps.extend(inner.steps);
        self
    }

    /// Parses the text form (behaviour 9); an error names the position.
    pub fn parse(text: &str) -> Result<Self, UiError> {
        let mut p = Parser {
            s: text,
            chars: text.char_indices().collect(),
            i: 0,
        };
        let loc = p.locator()?;
        p.ws();
        if p.i < p.chars.len() {
            return Err(p.error("unexpected text after the locator"));
        }
        Ok(loc)
    }

    /// Parses the JSON form, or a string in the text form.
    pub fn from_json(value: &Value) -> Result<Self, UiError> {
        match value {
            Value::String(s) => Self::parse(s),
            Value::Object(o) => {
                let mut steps = match o.get("within") {
                    Some(w) => Self::from_json(w)?.steps,
                    None => Vec::new(),
                };
                steps.push(selector_from_json(o)?);
                Ok(Self { steps })
            }
            _ => Err(UiError::invalid(
                "invalid: a locator is a string in the text form or an object",
            )),
        }
    }

    /// The JSON form.
    pub fn to_json(&self) -> Value {
        let mut out: Option<Value> = None;
        for step in &self.steps {
            let mut v = selector_to_json(step);
            if let Some(w) = out.take() {
                v["within"] = w;
            }
            out = Some(v);
        }
        out.unwrap_or(Value::Null)
    }

    /// The canonical text form.
    pub fn to_text(&self) -> String {
        self.steps
            .iter()
            .map(selector_to_text)
            .collect::<Vec<_>>()
            .join(" >> ")
    }

    /// The matches in `tree`, in paint order, as `view` sees the tree.
    pub fn resolve(&self, tree: &Tree, view: &View) -> Result<Vec<ElementId>, UiError> {
        let mut scope: Option<Vec<ElementId>> = None;
        for step in &self.steps {
            let candidates: Vec<ElementId> = match &scope {
                None => tree.iter().map(|n| n.id).collect(),
                Some(s) => {
                    let mut ids: Vec<ElementId> = Vec::new();
                    for id in s {
                        for d in tree.descendants(*id) {
                            if !ids.contains(&d) {
                                ids.push(d);
                            }
                        }
                    }
                    ids.sort_by_key(|id| tree.get(*id).map_or(usize::MAX, |n| n.order));
                    ids
                }
            };
            let mut matched = Vec::new();
            for id in candidates {
                if step_matches(step, tree, id, view)? {
                    matched.push(id);
                }
            }
            if let Some(n) = step.filters.nth {
                matched = matched.get(n).copied().into_iter().collect();
            }
            scope = Some(matched);
        }
        Ok(scope.unwrap_or_default())
    }

    /// Up to five elements close to what was asked for, for `not_found` (behaviour 10).
    pub fn closest(&self, tree: &Tree, view: &View) -> Vec<ElementInfo> {
        let Some(last) = self.steps.last() else {
            return Vec::new();
        };
        let wanted = |n: &crate::tree::UiNode| -> bool {
            match &last.kind {
                Kind::Role { role, name } => {
                    n.role() == *role
                        || name.as_ref().is_some_and(|m| {
                            n.name()
                                .is_some_and(|x| x.to_lowercase().contains(&m.text.to_lowercase()))
                        })
                }
                Kind::Id(id) => n
                    .test_id()
                    .is_some_and(|t| t.contains(id.as_str()) || id.contains(t) && !t.is_empty()),
                Kind::Text(m) | Kind::Label(m) => n
                    .name()
                    .is_some_and(|x| x.to_lowercase().contains(&m.text.to_lowercase())),
            }
        };
        tree.iter()
            .filter(|n| wanted(n))
            .take(5)
            .map(|n| ElementInfo::of(tree, n, view))
            .collect()
    }
}

fn step_matches(step: &Selector, tree: &Tree, id: ElementId, view: &View) -> Result<bool, UiError> {
    let Some(n) = tree.get(id) else {
        return Ok(false);
    };
    let kind_ok = match &step.kind {
        Kind::Role { role, name } => {
            n.role() == *role
                && match name {
                    None => true,
                    Some(m) => {
                        m.validate()?;
                        n.name().is_some_and(|x| m.matches(x))
                    }
                }
        }
        Kind::Id(t) => n.test_id() == Some(t.as_str()),
        Kind::Text(m) => {
            m.validate()?;
            n.name().is_some_and(|x| m.matches(x)) || view.value_of(n).is_some_and(|v| m.matches(v))
        }
        Kind::Label(m) => {
            m.validate()?;
            crate::tree::is_text_input(n.role()) && n.name().is_some_and(|x| m.matches(x))
        }
    };
    if !kind_ok {
        return Ok(false);
    }
    let f = &step.filters;
    let state = |want: Option<bool>, have: bool| want.is_none_or(|w| w == have);
    if !state(f.enabled, n.enabled())
        || !state(f.checked, n.checked() == Some(true))
        || !state(f.selected, n.selected() == Some(true))
        || !state(f.expanded, n.expanded() == Some(true))
        || !state(f.focused, tree.is_focused(id))
    {
        return Ok(false);
    }
    if let Some(inner) = &f.has {
        // A match of `inner` anywhere that lies inside this node.
        let inside = inner.resolve(tree, view)?;
        if !inside
            .iter()
            .any(|d| *d != id && tree.is_ancestor_or_self(id, *d))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn selector_to_text(s: &Selector) -> String {
    let mut out = match &s.kind {
        Kind::Role { role, name } => {
            let mut t = format!("role={}", role_name(*role));
            if let Some(m) = name {
                t.push_str(&format!("[{}]", m.to_text("name")));
                if m.ignore_case && m.mode != MatchMode::Regex {
                    t.push_str("[ignore_case]");
                }
            }
            t
        }
        Kind::Id(id) => {
            if id
                .chars()
                .all(|c| c.is_alphanumeric() || "-_.:".contains(c))
                && !id.is_empty()
            {
                format!("id={id}")
            } else {
                format!("id={}", quote(id))
            }
        }
        Kind::Text(m) => {
            let mut t = m.to_text("text");
            if m.ignore_case && m.mode != MatchMode::Regex {
                t.push_str("[ignore_case]");
            }
            t
        }
        Kind::Label(m) => {
            let mut t = m.to_text("label");
            if m.ignore_case && m.mode != MatchMode::Regex {
                t.push_str("[ignore_case]");
            }
            t
        }
    };
    let f = &s.filters;
    for (attr, v) in [
        ("enabled", f.enabled),
        ("checked", f.checked),
        ("selected", f.selected),
        ("expanded", f.expanded),
        ("focused", f.focused),
    ] {
        match v {
            Some(true) => out.push_str(&format!("[{attr}]")),
            Some(false) => out.push_str(&format!("[{attr}=false]")),
            None => {}
        }
    }
    if let Some(h) = &f.has {
        out.push_str(&format!("[has={}]", quote(&h.to_text())));
    }
    if let Some(n) = f.nth {
        out.push_str(&format!("[nth={n}]"));
    }
    out
}

fn selector_to_json(s: &Selector) -> Value {
    let mut o = Map::new();
    let put_match = |o: &mut Map<String, Value>, m: &TextMatch| {
        if m.mode != MatchMode::Exact {
            o.insert("match".into(), json!(m.mode_name()));
        }
        if m.ignore_case {
            o.insert("ignore_case".into(), json!(true));
        }
    };
    match &s.kind {
        Kind::Role { role, name } => {
            o.insert("role".into(), json!(role_name(*role)));
            if let Some(m) = name {
                o.insert("name".into(), json!(m.text));
                put_match(&mut o, m);
            }
        }
        Kind::Id(id) => {
            o.insert("id".into(), json!(id));
        }
        Kind::Text(m) => {
            o.insert("text".into(), json!(m.text));
            put_match(&mut o, m);
        }
        Kind::Label(m) => {
            o.insert("label".into(), json!(m.text));
            put_match(&mut o, m);
        }
    }
    let f = &s.filters;
    for (attr, v) in [
        ("enabled", f.enabled),
        ("checked", f.checked),
        ("selected", f.selected),
        ("expanded", f.expanded),
        ("focused", f.focused),
    ] {
        if let Some(v) = v {
            o.insert(attr.into(), json!(v));
        }
    }
    if let Some(h) = &f.has {
        o.insert("has".into(), h.to_json());
    }
    if let Some(n) = f.nth {
        o.insert("nth".into(), json!(n));
    }
    Value::Object(o)
}

fn selector_from_json(o: &Map<String, Value>) -> Result<Selector, UiError> {
    const KNOWN: [&str; 14] = [
        "role",
        "name",
        "id",
        "text",
        "label",
        "match",
        "ignore_case",
        "enabled",
        "checked",
        "selected",
        "expanded",
        "focused",
        "has",
        "nth",
    ];
    if let Some(k) = o
        .keys()
        .find(|k| !KNOWN.contains(&k.as_str()) && *k != "within")
    {
        return Err(UiError::invalid(format!(
            "invalid: unknown locator field `{k}`"
        )));
    }
    let mode = match o.get("match").and_then(Value::as_str) {
        None | Some("exact") => MatchMode::Exact,
        Some("contains") => MatchMode::Contains,
        Some("regex") => MatchMode::Regex,
        Some(other) => {
            return Err(UiError::invalid(format!(
                "invalid: `match` is exact, contains or regex, not `{other}`"
            )));
        }
    };
    let ignore_case = o
        .get("ignore_case")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text_match = |field: &str| -> Result<Option<TextMatch>, UiError> {
        match o.get(field) {
            None => Ok(None),
            Some(Value::String(s)) => {
                let m = TextMatch {
                    mode,
                    text: s.clone(),
                    ignore_case,
                };
                m.validate()?;
                Ok(Some(m))
            }
            Some(_) => Err(UiError::invalid(format!("invalid: `{field}` is a string"))),
        }
    };
    let kinds = ["role", "id", "text", "label"]
        .iter()
        .filter(|k| o.contains_key(**k))
        .count();
    if kinds != 1 {
        return Err(UiError::invalid(
            "invalid: a locator step has exactly one of `role`, `id`, `text` and `label`",
        ));
    }
    let kind = if let Some(r) = o.get("role") {
        let name = r.as_str().unwrap_or_default();
        let role = parse_role(name)
            .ok_or_else(|| UiError::invalid(format!("invalid: unknown role `{name}`")))?;
        Kind::Role {
            role,
            name: text_match("name")?,
        }
    } else if let Some(id) = o.get("id") {
        Kind::Id(
            id.as_str()
                .ok_or_else(|| UiError::invalid("invalid: `id` is a string"))?
                .to_owned(),
        )
    } else if o.contains_key("text") {
        Kind::Text(text_match("text")?.expect("present"))
    } else {
        Kind::Label(text_match("label")?.expect("present"))
    };
    let flag = |field: &str| -> Result<Option<bool>, UiError> {
        match o.get(field) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(UiError::invalid(format!(
                "invalid: `{field}` is true or false"
            ))),
        }
    };
    let filters = Filters {
        enabled: flag("enabled")?,
        checked: flag("checked")?,
        selected: flag("selected")?,
        expanded: flag("expanded")?,
        focused: flag("focused")?,
        has: match o.get("has") {
            None => None,
            Some(h) => Some(Box::new(Locator::from_json(h)?)),
        },
        nth: match o.get("nth") {
            None => None,
            Some(v) => Some(
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| UiError::invalid("invalid: `nth` is a number from 0"))?,
            ),
        },
    };
    Ok(Selector { kind, filters })
}

struct Parser<'a> {
    s: &'a str,
    chars: Vec<(usize, char)>,
    i: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> UiError {
        let pos = self.chars.get(self.i).map_or(self.s.len(), |(b, _)| *b);
        UiError::new(
            ErrorCode::Invalid,
            format!("invalid: {what} at position {pos} of the locator"),
            "forms: role=button[name=\"Commit\"], id=project-tree >> role=treeitem[name~=\"main\"], text=\"No problems\"",
        )
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).map(|(_, c)| *c)
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, text: &str) -> bool {
        let n = text.chars().count();
        let here: String = self.chars[self.i.min(self.chars.len())..]
            .iter()
            .take(n)
            .map(|(_, c)| *c)
            .collect();
        if here == text {
            self.i += n;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || "-_.:".contains(c) {
                out.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        out
    }

    fn string(&mut self) -> Result<String, UiError> {
        if !self.eat("\"") {
            return Err(self.error("expected a quoted text"));
        }
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("the quoted text does not end")),
                Some('"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some('n') => out.push('\n'),
                        Some(c) => out.push(c),
                        None => return Err(self.error("the quoted text does not end")),
                    }
                    self.i += 1;
                }
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    /// `="…"`, `~="…"` or `=/…/flags` after an attribute name.
    fn text_match(&mut self) -> Result<TextMatch, UiError> {
        let contains = self.eat("~=");
        if !contains && !self.eat("=") {
            return Err(self.error("expected `=` or `~=`"));
        }
        if !contains && self.peek() == Some('/') {
            let re_start = self.i;
            self.i += 1;
            let mut re = String::new();
            loop {
                match self.peek() {
                    None => return Err(self.error("the regular expression does not end")),
                    Some('/') => {
                        self.i += 1;
                        break;
                    }
                    Some('\\') if self.chars.get(self.i + 1).map(|(_, c)| *c) == Some('/') => {
                        re.push('/');
                        self.i += 2;
                    }
                    Some(c) => {
                        re.push(c);
                        self.i += 1;
                    }
                }
            }
            let ignore_case = self.eat("i");
            let m = TextMatch {
                mode: MatchMode::Regex,
                text: re,
                ignore_case,
            };
            if let Err(e) = m.validate() {
                self.i = re_start;
                return Err(self.error(&e.message.replace("invalid: ", "")));
            }
            return Ok(m);
        }
        let text = self.string()?;
        Ok(TextMatch {
            mode: if contains {
                MatchMode::Contains
            } else {
                MatchMode::Exact
            },
            text,
            ignore_case: false,
        })
    }

    fn locator(&mut self) -> Result<Locator, UiError> {
        let mut steps = vec![self.selector()?];
        loop {
            self.ws();
            if self.eat(">>") {
                self.ws();
                steps.push(self.selector()?);
            } else {
                return Ok(Locator { steps });
            }
        }
    }

    fn selector(&mut self) -> Result<Selector, UiError> {
        self.ws();
        let start = self.i;
        let key = self.ident();
        let mut kind = match key.as_str() {
            "role" => {
                if !self.eat("=") {
                    return Err(self.error("expected `=` after `role`"));
                }
                let name = self.ident();
                let role = parse_role(&name).ok_or_else(|| {
                    self.i = start;
                    self.error(&format!("unknown role `{name}`"))
                })?;
                Kind::Role { role, name: None }
            }
            "id" => {
                if !self.eat("=") {
                    return Err(self.error("expected `=` after `id`"));
                }
                let id = if self.peek() == Some('"') {
                    self.string()?
                } else {
                    self.ident()
                };
                if id.is_empty() {
                    return Err(self.error("expected a test id"));
                }
                Kind::Id(id)
            }
            "text" => Kind::Text(self.text_match()?),
            "label" => Kind::Label(self.text_match()?),
            _ => {
                self.i = start;
                return Err(self.error("expected role=, id=, text= or label="));
            }
        };
        let mut filters = Filters::default();
        let mut ignore_case = false;
        while self.peek() == Some('[') {
            self.i += 1;
            loop {
                self.ws();
                let attr_start = self.i;
                let attr = self.ident();
                match attr.as_str() {
                    "name" => {
                        let m = self.text_match()?;
                        match &mut kind {
                            Kind::Role { name, .. } => *name = Some(m),
                            _ => return Err(self.error("`name` belongs to role=")),
                        }
                    }
                    "ignore_case" => ignore_case = true,
                    "enabled" | "checked" | "selected" | "expanded" | "focused" => {
                        let value = if self.eat("=") {
                            match self.ident().as_str() {
                                "true" => true,
                                "false" => false,
                                _ => return Err(self.error("expected true or false")),
                            }
                        } else {
                            true
                        };
                        let slot = match attr.as_str() {
                            "enabled" => &mut filters.enabled,
                            "checked" => &mut filters.checked,
                            "selected" => &mut filters.selected,
                            "expanded" => &mut filters.expanded,
                            _ => &mut filters.focused,
                        };
                        *slot = Some(value);
                    }
                    "nth" => {
                        if !self.eat("=") {
                            return Err(self.error("expected `=` after `nth`"));
                        }
                        let n = self.ident();
                        filters.nth = Some(
                            n.parse()
                                .map_err(|_| self.error("expected a number from 0"))?,
                        );
                    }
                    "has" => {
                        if !self.eat("=") {
                            return Err(self.error("expected `=` after `has`"));
                        }
                        let inner = self.string()?;
                        filters.has = Some(Box::new(Locator::parse(&inner)?));
                    }
                    "" => return Err(self.error("expected a filter")),
                    other => {
                        self.i = attr_start;
                        return Err(self.error(&format!("unknown filter `{other}`")));
                    }
                }
                self.ws();
                if self.eat(",") {
                    continue;
                }
                if self.eat("]") {
                    break;
                }
                return Err(self.error("expected `]`"));
            }
        }
        if ignore_case {
            match &mut kind {
                Kind::Role { name: Some(m), .. } | Kind::Text(m) | Kind::Label(m) => {
                    m.ignore_case = true;
                }
                _ => {}
            }
        }
        Ok(Selector { kind, filters })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_json_forms_parse_to_the_same_locator() {
        let cases = [
            (
                r#"role=button[name="Commit"]"#,
                json!({"role": "button", "name": "Commit"}),
            ),
            (
                r#"id=project-tree >> role=treeitem[name="main.rs"]"#,
                json!({"role": "treeitem", "name": "main.rs", "within": {"id": "project-tree"}}),
            ),
            (r#"text="No problems""#, json!({"text": "No problems"})),
            (
                r#"role=button[name~="Commit"][enabled]"#,
                json!({"role": "button", "name": "Commit", "match": "contains", "enabled": true}),
            ),
            (
                r#"role=treeitem[name=/^main\.rs$/i][checked=false][nth=2]"#,
                json!({"role": "treeitem", "name": "^main\\.rs$", "match": "regex", "ignore_case": true, "checked": false, "nth": 2}),
            ),
            (
                r#"role=row[has="role=button[name=\"Stop\"]"]"#,
                json!({"role": "row", "has": {"role": "button", "name": "Stop"}}),
            ),
            (
                r#"label="Password"[ignore_case]"#,
                json!({"label": "Password", "ignore_case": true}),
            ),
        ];
        for (text, json) in cases {
            let a = Locator::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            let b = Locator::from_json(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(a, b, "{text}");
            // Both forms round-trip.
            assert_eq!(Locator::parse(&a.to_text()).unwrap(), a, "{}", a.to_text());
            assert_eq!(Locator::from_json(&a.to_json()).unwrap(), a);
            // A string in JSON is the text form.
            assert_eq!(Locator::from_json(&json!(text)).unwrap(), a);
        }
    }

    #[test]
    fn invalid_locators_name_the_position() {
        for (text, pos) in [
            ("role=buton", 0),
            ("role=button[name=\"Commit\"", 25),
            ("rol=button", 0),
            ("role=button[nme=\"x\"]", 12),
            ("id=a >>", 7),
            ("text=/(/", 5),
        ] {
            let e = Locator::parse(text).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid, "{text}");
            assert!(
                e.message.contains(&format!("at position {pos} ")),
                "{text}: {}",
                e.message
            );
        }
        assert!(Locator::from_json(&json!({"role": "button", "id": "x"})).is_err());
        assert!(Locator::from_json(&json!({"role": "button", "colour": "x"})).is_err());
        assert!(Locator::from_json(&json!(3)).is_err());
    }
}

//! Reassigned Java locals and parameters (`SPEC-0027` v11): which variables get a new value after
//! their declaration, and every place that names them, found on the syntax tree alone.
//!
//! Each class member that overlaps the asked range is resolved as a whole: declarations open
//! scopes (method, constructor, lambda, block, loop, `catch`, `switch` group, resources), a name
//! refers to the innermost declaration before it, and a class body inside a member starts afresh,
//! because the outer locals it can see are effectively final.

use std::ops::Range;

use tree_sitter::Node;

/// How a variable was written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Write {
    /// `x = …`
    Plain,
    /// `x += …`, `x++`, `--x`.
    Compound,
}

struct Decl<'t> {
    name: Range<usize>,
    parameter: bool,
    /// Whether the variable has a value from its declaration on.
    has_value: bool,
    /// The node whose scope declares it.
    scope: Node<'t>,
    uses: Vec<Range<usize>>,
    writes: Vec<(Node<'t>, Write)>,
}

struct Resolver<'t, 'a> {
    text: &'a str,
    decls: Vec<Decl<'t>>,
    /// Innermost scope last: names and indices into `decls`.
    scopes: Vec<Vec<(&'a str, usize)>>,
}

/// The byte ranges inside `lo..hi` that name a reassigned local (`false`) or parameter (`true`):
/// the declaration and every use.
pub fn reassigned(root: Node, text: &str, lo: usize, hi: usize) -> Vec<(Range<usize>, bool)> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.end_byte() < lo || n.start_byte() > hi {
            continue;
        }
        let member = n.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "class_body" | "enum_body_declarations" | "interface_body" | "enum_body"
            )
        });
        if member
            && matches!(
                n.kind(),
                "method_declaration"
                    | "constructor_declaration"
                    | "compact_constructor_declaration"
                    | "static_initializer"
                    | "block"
                    | "field_declaration"
                    | "constant_declaration"
            )
        {
            let mut r = Resolver {
                text,
                decls: Vec::new(),
                scopes: Vec::new(),
            };
            r.visit(n);
            for d in r.decls {
                if is_reassigned(&d) {
                    let parameter = d.parameter;
                    out.extend(
                        std::iter::once(d.name)
                            .chain(d.uses)
                            .filter(|u| u.end > lo && u.start < hi)
                            .map(|u| (u, parameter)),
                    );
                }
            }
            continue;
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    out.sort_by_key(|(r, _)| r.start);
    out.dedup();
    out
}

fn is_loop(n: Node) -> bool {
    matches!(
        n.kind(),
        "for_statement" | "enhanced_for_statement" | "while_statement" | "do_statement"
    )
}

/// Whether two writes lie in different branches of the same `if`/`else`, `switch` or `? :`.
fn exclusive(a: Node, b: Node) -> bool {
    fn chain(n: Node) -> Vec<Node> {
        let mut v = vec![n];
        let mut at = n;
        while let Some(p) = at.parent() {
            v.push(p);
            at = p;
        }
        v.reverse();
        v
    }
    let (ca, cb) = (chain(a), chain(b));
    let mut i = 0;
    while i < ca.len() && i < cb.len() && ca[i] == cb[i] {
        i += 1;
    }
    if i == 0 || i >= ca.len() || i >= cb.len() {
        return false;
    }
    let lca = ca[i - 1];
    let (ba, bb) = (ca[i], cb[i]);
    match lca.kind() {
        "if_statement" | "ternary_expression" => {
            let branch = |n: Node| {
                ["consequence", "alternative"]
                    .iter()
                    .position(|f| lca.child_by_field_name(f) == Some(n))
            };
            matches!((branch(ba), branch(bb)), (Some(x), Some(y)) if x != y)
        }
        "switch_block" => ba != bb,
        _ => false,
    }
}

fn is_reassigned(d: &Decl) -> bool {
    if d.writes.is_empty() {
        return false;
    }
    if d.parameter || d.has_value {
        return true;
    }
    if d.writes.iter().any(|(_, w)| *w == Write::Compound) {
        return true;
    }
    // Written in a loop that does not hold the declaration: again on every turn.
    let in_loop = d.writes.iter().any(|(w, _)| {
        let mut at = *w;
        while let Some(p) = at.parent() {
            if p == d.scope {
                break;
            }
            if is_loop(p) {
                return true;
            }
            at = p;
        }
        false
    });
    if in_loop {
        return true;
    }
    d.writes
        .iter()
        .enumerate()
        .any(|(i, (a, _))| d.writes[i + 1..].iter().any(|(b, _)| !exclusive(*a, *b)))
}

impl<'t, 'a> Resolver<'t, 'a>
where
    't: 'a,
{
    fn name(&self, n: Node) -> &'a str {
        &self.text[n.byte_range()]
    }

    fn declare(&mut self, name: Node<'t>, parameter: bool, has_value: bool, scope: Node<'t>) {
        if name.kind() != "identifier" {
            return;
        }
        let i = self.decls.len();
        self.decls.push(Decl {
            name: name.byte_range(),
            parameter,
            has_value,
            scope,
            uses: Vec::new(),
            writes: Vec::new(),
        });
        let text = self.name(name);
        if let Some(s) = self.scopes.last_mut() {
            s.push((text, i));
        }
    }

    fn resolve(&self, n: Node) -> Option<usize> {
        let name = self.name(n);
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|(x, _)| *x == name)
            .map(|(_, i)| *i)
    }

    fn use_of(&mut self, n: Node<'t>, write: Option<Write>, at: Node<'t>) {
        if let Some(i) = self.resolve(n) {
            self.decls[i].uses.push(n.byte_range());
            if let Some(w) = write {
                self.decls[i].writes.push((at, w));
            }
        }
    }

    fn children(&mut self, n: Node<'t>) {
        let mut c = n.walk();
        let kids: Vec<Node<'t>> = n.children(&mut c).collect();
        for k in kids {
            self.visit(k);
        }
    }

    fn field(&mut self, n: Node<'t>, name: &str) {
        if let Some(k) = n.child_by_field_name(name) {
            self.visit(k);
        }
    }

    /// Declares the parameters of a method, constructor or lambda.
    fn parameters(&mut self, n: Node<'t>) {
        let Some(params) = n.child_by_field_name("parameters") else {
            return;
        };
        if params.kind() == "identifier" {
            self.declare(params, true, true, n);
            return;
        }
        let mut c = params.walk();
        let list: Vec<Node<'t>> = params.named_children(&mut c).collect();
        for p in list {
            match p.kind() {
                "identifier" => self.declare(p, true, true, n),
                "formal_parameter" => {
                    if let Some(name) = p.child_by_field_name("name") {
                        self.declare(name, true, true, n);
                    }
                }
                "spread_parameter" => {
                    let mut vc = p.walk();
                    let name = p
                        .named_children(&mut vc)
                        .find(|v| v.kind() == "variable_declarator")
                        .and_then(|v| v.child_by_field_name("name"));
                    if let Some(name) = name {
                        self.declare(name, true, true, n);
                    }
                }
                _ => {}
            }
        }
    }

    fn visit(&mut self, n: Node<'t>) {
        match n.kind() {
            "class_body" | "enum_body" | "interface_body" => {
                let saved = std::mem::take(&mut self.scopes);
                self.children(n);
                self.scopes = saved;
            }
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration" => self.field(n, "body"),
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration"
            | "lambda_expression" => {
                self.scopes.push(Vec::new());
                self.parameters(n);
                self.field(n, "body");
                self.scopes.pop();
            }
            "enhanced_for_statement" => {
                self.scopes.push(Vec::new());
                self.field(n, "value");
                if let Some(name) = n.child_by_field_name("name") {
                    self.declare(name, false, true, n);
                }
                self.field(n, "body");
                self.scopes.pop();
            }
            "catch_clause" => {
                self.scopes.push(Vec::new());
                let mut c = n.walk();
                let kids: Vec<Node<'t>> = n.named_children(&mut c).collect();
                for k in kids {
                    if k.kind() == "catch_formal_parameter" {
                        if let Some(name) = k.child_by_field_name("name") {
                            self.declare(name, false, true, n);
                        }
                    } else {
                        self.visit(k);
                    }
                }
                self.scopes.pop();
            }
            "block"
            | "constructor_body"
            | "switch_block_statement_group"
            | "switch_rule"
            | "for_statement"
            | "try_with_resources_statement" => {
                self.scopes.push(Vec::new());
                self.children(n);
                self.scopes.pop();
            }
            "local_variable_declaration" => {
                let scope = n.parent().unwrap_or(n);
                let mut c = n.walk();
                let declarators: Vec<Node<'t>> =
                    n.children_by_field_name("declarator", &mut c).collect();
                for d in declarators {
                    let value = d.child_by_field_name("value");
                    if let Some(v) = value {
                        self.visit(v);
                    }
                    if let Some(name) = d.child_by_field_name("name") {
                        self.declare(name, false, value.is_some(), scope);
                    }
                }
            }
            "resource" => {
                self.field(n, "value");
                if let Some(name) = n.child_by_field_name("name") {
                    let scope = n.parent().unwrap_or(n);
                    self.declare(name, false, true, scope);
                }
            }
            "assignment_expression" => {
                let plain = n
                    .child_by_field_name("operator")
                    .is_some_and(|o| self.name(o) == "=");
                match n.child_by_field_name("left") {
                    Some(l) if l.kind() == "identifier" => {
                        let w = if plain { Write::Plain } else { Write::Compound };
                        self.use_of(l, Some(w), n);
                    }
                    Some(l) => self.visit(l),
                    None => {}
                }
                self.field(n, "right");
            }
            "update_expression" => {
                let mut c = n.walk();
                let operand = n.named_children(&mut c).next();
                match operand {
                    Some(o) if o.kind() == "identifier" => self.use_of(o, Some(Write::Compound), n),
                    Some(o) => self.visit(o),
                    None => {}
                }
            }
            "identifier" => self.use_of(n, None, n),
            "field_access" => self.field(n, "object"),
            "method_invocation" => {
                self.field(n, "object");
                self.field(n, "arguments");
            }
            "method_reference" => {
                if let Some(first) = n.named_child(0) {
                    self.visit(first);
                }
            }
            "object_creation_expression" => {
                self.field(n, "object");
                self.field(n, "arguments");
                let mut c = n.walk();
                let body = n.named_children(&mut c).find(|k| k.kind() == "class_body");
                if let Some(b) = body {
                    self.visit(b);
                }
            }
            "labeled_statement" => {
                let mut c = n.walk();
                let kids: Vec<Node<'t>> = n
                    .named_children(&mut c)
                    .filter(|k| k.kind() != "identifier")
                    .collect();
                for k in kids {
                    self.visit(k);
                }
            }
            "break_statement" | "continue_statement" | "scoped_identifier" | "type_identifier"
            | "marker_annotation" | "annotation" | "modifiers" | "line_comment"
            | "block_comment" => {}
            // A field or constant declaration of a class: only the values, no local names.
            "field_declaration" | "constant_declaration" => {
                let mut c = n.walk();
                let values: Vec<Node<'t>> = n
                    .children_by_field_name("declarator", &mut c)
                    .filter_map(|d| d.child_by_field_name("value"))
                    .collect();
                for v in values {
                    self.visit(v);
                }
            }
            _ => self.children(n),
        }
    }
}

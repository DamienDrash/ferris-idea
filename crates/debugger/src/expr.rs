//! Simple breakpoint conditions evaluated by Ferris IDEA when the adapter cannot (`SPEC-0178`
//! behaviour 4a): literals, names and field paths, `! -`, `* / %`, `+ -`, comparisons, `&& ||`.

/// A value during evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A whole number.
    Int(i64),
    /// A decimal number.
    Float(f64),
    /// `true` or `false`.
    Bool(bool),
    /// Text (also characters and values shown in no other form).
    Text(String),
    /// `null`.
    Null,
}

impl Value {
    /// Reads a value as an adapter shows it: numbers, `true`/`false`, `null`, quoted text; anything
    /// else is taken as text.
    pub fn shown(text: &str) -> Self {
        let t = text.trim();
        match t {
            "true" => return Self::Bool(true),
            "false" => return Self::Bool(false),
            "null" => return Self::Null,
            _ => {}
        }
        if let Ok(n) = t.parse::<i64>() {
            return Self::Int(n);
        }
        if let Ok(n) = t.parse::<f64>() {
            return Self::Float(n);
        }
        for q in ['"', '\''] {
            if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
                return Self::Text(t[1..t.len() - 1].to_owned());
            }
        }
        Self::Text(t.to_owned())
    }

    /// The value as text, for log messages.
    pub fn text(&self) -> String {
        text(self)
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(n) => Some(*n),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i64),
    Float(f64),
    Text(String),
    Name(String),
    Op(&'static str),
}

const OPS: [&str; 20] = [
    "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "<", ">", "!", "+", "-", "*", "/", "%", "(",
    ")", ".", "?",
];

fn tokens(source: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let decimal = i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit();
            if decimal {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            // Kotlin and Java suffixes: `10L`, `1.5f`, `2.0d`.
            let digits = text.trim_end_matches(['L', 'l', 'f', 'F', 'd', 'D']);
            out.push(
                if decimal || digits.len() < text.len() && !text.ends_with(['L', 'l']) {
                    Token::Float(digits.parse().map_err(|_| format!("bad number `{text}`"))?)
                } else {
                    Token::Int(digits.parse().map_err(|_| format!("bad number `{text}`"))?)
                },
            );
        } else if c == '"' || c == '\'' {
            let mut text = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("text without its closing quote".into()),
                    Some(&q) if q == c => break,
                    Some('\\') => {
                        let escaped = chars.get(i + 1).ok_or("text ends with `\\`")?;
                        text.push(match escaped {
                            'n' => '\n',
                            't' => '\t',
                            other => *other,
                        });
                        i += 2;
                    }
                    Some(other) => {
                        text.push(*other);
                        i += 1;
                    }
                }
            }
            i += 1;
            out.push(Token::Text(text));
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..].iter().take(3).collect();
            let op = OPS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| format!("`{c}` is not supported"))?;
            out.push(Token::Op(op));
            i += op.len();
        }
    }
    Ok(out)
}

struct Parser<'a, F> {
    tokens: Vec<Token>,
    at: usize,
    resolve: &'a mut F,
}

impl<F: FnMut(&str) -> Result<Value, String>> Parser<'_, F> {
    fn peek_op(&self) -> Option<&'static str> {
        match self.tokens.get(self.at) {
            Some(Token::Op(op)) => Some(op),
            _ => None,
        }
    }

    fn eat(&mut self, op: &str) -> bool {
        let found = self.peek_op() == Some(op);
        if found {
            self.at += 1;
        }
        found
    }

    /// `skip`: parse only, without resolving names (the side `&&`/`||` do not need).
    fn or(&mut self, skip: bool) -> Result<Value, String> {
        let mut left = self.and(skip)?;
        while self.eat("||") {
            let done = skip || left == Value::Bool(true);
            let right = self.and(done)?;
            if !done {
                left = Value::Bool(boolean(&left)? || boolean(&right)?);
            }
        }
        Ok(left)
    }

    fn and(&mut self, skip: bool) -> Result<Value, String> {
        let mut left = self.compare(skip)?;
        while self.eat("&&") {
            let done = skip || left == Value::Bool(false);
            let right = self.compare(done)?;
            if !done {
                left = Value::Bool(boolean(&left)? && boolean(&right)?);
            }
        }
        Ok(left)
    }

    fn compare(&mut self, skip: bool) -> Result<Value, String> {
        let left = self.sum(skip)?;
        let Some(op) = self
            .peek_op()
            .filter(|op| ["==", "!=", "===", "!==", "<", "<=", ">", ">="].contains(op))
        else {
            return Ok(left);
        };
        self.at += 1;
        let right = self.sum(skip)?;
        if skip {
            return Ok(Value::Null);
        }
        let equal = || match (left.number(), right.number()) {
            (Some(a), Some(b)) => a == b,
            _ => left == right,
        };
        let order = || match (left.number(), right.number(), &left, &right) {
            (Some(a), Some(b), ..) => a.partial_cmp(&b).ok_or("cannot compare NaN".to_owned()),
            (_, _, Value::Text(a), Value::Text(b)) => Ok(a.cmp(b)),
            _ => Err(format!("cannot order {left:?} and {right:?}")),
        };
        Ok(Value::Bool(match op {
            "==" | "===" => equal(),
            "!=" | "!==" => !equal(),
            "<" => order()?.is_lt(),
            "<=" => order()?.is_le(),
            ">" => order()?.is_gt(),
            _ => order()?.is_ge(),
        }))
    }

    fn sum(&mut self, skip: bool) -> Result<Value, String> {
        let mut left = self.product(skip)?;
        while let Some(op) = self.peek_op().filter(|op| *op == "+" || *op == "-") {
            self.at += 1;
            let right = self.product(skip)?;
            if skip {
                continue;
            }
            left = match (op, &left, &right) {
                ("+", Value::Text(a), b) => Value::Text(format!("{a}{}", text(b))),
                _ => arithmetic(op, &left, &right)?,
            };
        }
        Ok(left)
    }

    fn product(&mut self, skip: bool) -> Result<Value, String> {
        let mut left = self.unary(skip)?;
        while let Some(op) = self.peek_op().filter(|op| ["*", "/", "%"].contains(op)) {
            self.at += 1;
            let right = self.unary(skip)?;
            if !skip {
                left = arithmetic(op, &left, &right)?;
            }
        }
        Ok(left)
    }

    fn unary(&mut self, skip: bool) -> Result<Value, String> {
        if self.eat("!") {
            let v = self.unary(skip)?;
            return if skip {
                Ok(v)
            } else {
                Ok(Value::Bool(!boolean(&v)?))
            };
        }
        if self.eat("-") {
            let v = self.unary(skip)?;
            return match v {
                _ if skip => Ok(v),
                Value::Int(n) => Ok(Value::Int(-n)),
                Value::Float(n) => Ok(Value::Float(-n)),
                other => Err(format!("cannot negate {other:?}")),
            };
        }
        self.primary(skip)
    }

    fn primary(&mut self, skip: bool) -> Result<Value, String> {
        let token = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or("the expression ends early")?;
        self.at += 1;
        match token {
            Token::Int(n) => Ok(Value::Int(n)),
            Token::Float(n) => Ok(Value::Float(n)),
            Token::Text(t) => Ok(Value::Text(t)),
            Token::Op("(") => {
                let v = self.or(skip)?;
                if !self.eat(")") {
                    return Err("`(` without `)`".into());
                }
                Ok(v)
            }
            Token::Name(name) => {
                match name.as_str() {
                    "true" => return Ok(Value::Bool(true)),
                    "false" => return Ok(Value::Bool(false)),
                    "null" => return Ok(Value::Null),
                    _ => {}
                }
                let mut path = name;
                while self.eat(".") {
                    match self.tokens.get(self.at) {
                        Some(Token::Name(field)) => {
                            path.push('.');
                            path.push_str(field);
                            self.at += 1;
                        }
                        _ => return Err("`.` must be followed by a name".into()),
                    }
                }
                if self.peek_op() == Some("(") {
                    return Err(format!("calls (`{path}(…)`) are not supported"));
                }
                if skip {
                    Ok(Value::Null)
                } else {
                    (self.resolve)(&path)
                }
            }
            Token::Op(op) => Err(format!("`{op}` is not supported here")),
        }
    }
}

fn boolean(v: &Value) -> Result<bool, String> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(format!("{other:?} is not true or false")),
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Text(t) => t.clone(),
        Value::Null => "null".into(),
    }
}

fn arithmetic(op: &str, a: &Value, b: &Value) -> Result<Value, String> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        let (x, y) = (*x, *y);
        if (op == "/" || op == "%") && y == 0 {
            return Err("division by zero".into());
        }
        let r = match op {
            "+" => x.checked_add(y),
            "-" => x.checked_sub(y),
            "*" => x.checked_mul(y),
            "/" => x.checked_div(y),
            _ => x.checked_rem(y),
        };
        return r.map(Value::Int).ok_or_else(|| "overflow".into());
    }
    let (Some(x), Some(y)) = (a.number(), b.number()) else {
        return Err(format!("`{op}` needs numbers, not {a:?} and {b:?}"));
    };
    Ok(Value::Float(match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => x / y,
        _ => x % y,
    }))
}

/// Evaluates `source`; `resolve` gives the value of a name or field path (`a`, `a.b`).
pub fn evaluate(
    source: &str,
    mut resolve: impl FnMut(&str) -> Result<Value, String>,
) -> Result<Value, String> {
    let mut parser = Parser {
        tokens: tokens(source)?,
        at: 0,
        resolve: &mut resolve,
    };
    let value = parser.or(false)?;
    if parser.at < parser.tokens.len() {
        return Err(format!("unexpected {:?}", parser.tokens[parser.at]));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Result<Value, String> {
        evaluate(source, |path| match path {
            "i" => Ok(Value::Int(4)),
            "name" => Ok(Value::shown("\"Ada\"")),
            "order.total" => Ok(Value::shown("12.5")),
            "order.customer" => Ok(Value::Null),
            "flag" => Ok(Value::Bool(true)),
            other => Err(format!("unknown {other}")),
        })
    }

    #[test]
    fn evaluates_simple_conditions() {
        let yes = [
            "i == 4",
            "i % 2 == 0 && i > 2",
            "i == 1 || i == 4",
            "!(i < 3)",
            "i * 2 + 1 == 9",
            "7 / 2 == 3",
            "7.0 / 2 == 3.5",
            "-i == -4",
            "name == \"Ada\"",
            "name != 'Bob'",
            "name + \"!\" == \"Ada!\"",
            "\"Ada\" < \"Bob\"",
            "order.total >= 12.5",
            "order.customer == null",
            "flag",
            "i == 4L",
            "1_000 > i",
            // Short-circuit: the unknown name is never asked for.
            "i == 4 || missing > 1",
            "i == 5 && missing > 1 || true",
        ];
        for source in yes {
            assert_eq!(run(source), Ok(Value::Bool(true)), "{source}");
        }
        assert_eq!(run("i > 4"), Ok(Value::Bool(false)));
        assert_eq!(run("i == 5 && missing"), Ok(Value::Bool(false)));
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        for (source, why) in [
            ("missing > 1", "unknown missing"),
            ("list[0] == 1", "`[` is not supported"),
            ("name.length() == 3", "calls"),
            ("i / 0 == 1", "division by zero"),
            ("(i == 4", "without `)`"),
            ("i == ", "ends early"),
            ("i i", "unexpected"),
            ("\"open", "closing quote"),
            ("i && true", "not true or false"),
            ("name > 3", "cannot order"),
        ] {
            let err = run(source).unwrap_err();
            assert!(err.contains(why), "{source}: {err}");
        }
    }

    #[test]
    fn reads_values_as_adapters_show_them() {
        assert_eq!(Value::shown("12"), Value::Int(12));
        assert_eq!(Value::shown("1.5"), Value::Float(1.5));
        assert_eq!(Value::shown("\"x\""), Value::Text("x".into()));
        assert_eq!(Value::shown("'c'"), Value::Text("c".into()));
        assert_eq!(Value::shown("null"), Value::Null);
        assert_eq!(Value::shown("Point(x=1)"), Value::Text("Point(x=1)".into()));
    }
}

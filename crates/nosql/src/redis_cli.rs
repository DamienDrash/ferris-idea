//! The Redis console (`SPEC-0312` behaviours 1, 2, 5): `redis-cli` quoting, a command table for
//! classification and completion, and reply rendering.

use crate::{Class, ShellError};

/// `KEYS` is refused above this many keys in the database (behaviour 5).
pub const KEYS_LIMIT: u64 = 10_000;

/// One known command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    /// Upper-case name.
    pub name: &'static str,
    /// Class.
    pub class: Class,
    /// Argument summary shown by completion.
    pub args: &'static str,
    /// Index (1-based, after the name) of the first key argument; 0 = none. Every following
    /// argument is a key too when `all_keys`.
    pub key_at: u8,
    /// Every argument from `key_at` is a key (`DEL a b c`).
    pub all_keys: bool,
}

const fn c(name: &'static str, class: Class, args: &'static str, key_at: u8) -> CommandInfo {
    CommandInfo {
        name,
        class,
        args,
        key_at,
        all_keys: false,
    }
}

const fn k(name: &'static str, class: Class, args: &'static str) -> CommandInfo {
    CommandInfo {
        name,
        class,
        args,
        key_at: 1,
        all_keys: true,
    }
}

use Class::{ModifyData as W, ModifySchema as A, ReadData as R};

/// Known commands, alphabetical.
pub const COMMANDS: &[CommandInfo] = &[
    c("ACL", A, "subcommand …", 0),
    c("APPEND", W, "key value", 1),
    c("BGREWRITEAOF", A, "", 0),
    c("BGSAVE", A, "", 0),
    c("BITCOUNT", R, "key [start end]", 1),
    c("CLIENT", A, "subcommand …", 0),
    c("CLUSTER", A, "subcommand …", 0),
    c("CONFIG", A, "GET|SET …", 0),
    c("COPY", W, "source destination [DB n] [REPLACE]", 1),
    c("DBSIZE", R, "", 0),
    c("DEBUG", A, "subcommand …", 0),
    c("DECR", W, "key", 1),
    c("DECRBY", W, "key decrement", 1),
    k("DEL", W, "key [key …]"),
    c("ECHO", R, "message", 0),
    c("EVAL", A, "script numkeys [key …] [arg …]", 0),
    c("EVALSHA", A, "sha1 numkeys [key …] [arg …]", 0),
    k("EXISTS", R, "key [key …]"),
    c("EXPIRE", W, "key seconds", 1),
    c("EXPIREAT", W, "key unix-time", 1),
    c("FAILOVER", A, "…", 0),
    c("FCALL", A, "function numkeys …", 0),
    c("FLUSHALL", A, "[ASYNC|SYNC]", 0),
    c("FLUSHDB", A, "[ASYNC|SYNC]", 0),
    c("FUNCTION", A, "subcommand …", 0),
    c("GET", R, "key", 1),
    c("GETDEL", W, "key", 1),
    c("GETEX", W, "key [EX seconds|PERSIST]", 1),
    c("GETRANGE", R, "key start end", 1),
    c("HDEL", W, "key field [field …]", 1),
    c("HEXISTS", R, "key field", 1),
    c("HGET", R, "key field", 1),
    c("HGETALL", R, "key", 1),
    c("HINCRBY", W, "key field increment", 1),
    c("HKEYS", R, "key", 1),
    c("HLEN", R, "key", 1),
    c("HMGET", R, "key field [field …]", 1),
    c("HSCAN", R, "key cursor [MATCH pattern] [COUNT n]", 1),
    c("HSET", W, "key field value [field value …]", 1),
    c("HSETNX", W, "key field value", 1),
    c("HVALS", R, "key", 1),
    c("INCR", W, "key", 1),
    c("INCRBY", W, "key increment", 1),
    c("INFO", R, "[section]", 0),
    c("KEYS", R, "pattern", 0),
    c("LINDEX", R, "key index", 1),
    c("LINSERT", W, "key BEFORE|AFTER pivot element", 1),
    c("LLEN", R, "key", 1),
    c("LPOP", W, "key [count]", 1),
    c("LPUSH", W, "key element [element …]", 1),
    c("LRANGE", R, "key start stop", 1),
    c("LREM", W, "key count element", 1),
    c("LSET", W, "key index element", 1),
    c("LTRIM", W, "key start stop", 1),
    c("MEMORY", R, "USAGE key", 0),
    k("MGET", R, "key [key …]"),
    c("MIGRATE", A, "…", 0),
    c("MODULE", A, "subcommand …", 0),
    c("MSET", W, "key value [key value …]", 0),
    c("OBJECT", R, "ENCODING|FREQ|IDLETIME|REFCOUNT key", 0),
    c("PERSIST", W, "key", 1),
    c("PEXPIRE", W, "key milliseconds", 1),
    c("PING", R, "[message]", 0),
    c("PTTL", R, "key", 1),
    c("RANDOMKEY", R, "", 0),
    c("RENAME", W, "key newkey", 1),
    c("RENAMENX", W, "key newkey", 1),
    c("REPLICAOF", A, "host port", 0),
    c("RPOP", W, "key [count]", 1),
    c("RPUSH", W, "key element [element …]", 1),
    c("SADD", W, "key member [member …]", 1),
    c("SAVE", A, "", 0),
    c("SCAN", R, "cursor [MATCH pattern] [COUNT n] [TYPE type]", 0),
    c("SCARD", R, "key", 1),
    c("SCRIPT", A, "subcommand …", 0),
    k("SDIFF", R, "key [key …]"),
    c("SELECT", R, "index", 0),
    c("SET", W, "key value [EX seconds] [NX|XX] [KEEPTTL]", 1),
    c("SETEX", W, "key seconds value", 1),
    c("SETNX", W, "key value", 1),
    c("SHUTDOWN", A, "[NOSAVE|SAVE]", 0),
    k("SINTER", R, "key [key …]"),
    c("SISMEMBER", R, "key member", 1),
    c("SLAVEOF", A, "host port", 0),
    c("SMEMBERS", R, "key", 1),
    c("SPOP", W, "key [count]", 1),
    c("SREM", W, "key member [member …]", 1),
    c("SSCAN", R, "key cursor [MATCH pattern] [COUNT n]", 1),
    c("STRLEN", R, "key", 1),
    k("SUNION", R, "key [key …]"),
    c("TIME", R, "", 0),
    k("TOUCH", R, "key [key …]"),
    c("TTL", R, "key", 1),
    c("TYPE", R, "key", 1),
    k("UNLINK", W, "key [key …]"),
    c("XADD", W, "key id field value [field value …]", 1),
    c("XDEL", W, "key id [id …]", 1),
    c("XLEN", R, "key", 1),
    c("XRANGE", R, "key start end [COUNT n]", 1),
    c("XREVRANGE", R, "key end start [COUNT n]", 1),
    c("ZADD", W, "key score member [score member …]", 1),
    c("ZCARD", R, "key", 1),
    c("ZCOUNT", R, "key min max", 1),
    c("ZINCRBY", W, "key increment member", 1),
    c("ZRANGE", R, "key start stop [WITHSCORES]", 1),
    c("ZRANK", R, "key member", 1),
    c("ZREM", W, "key member [member …]", 1),
    c("ZSCAN", R, "key cursor [MATCH pattern] [COUNT n]", 1),
    c("ZSCORE", R, "key member", 1),
];

/// The table entry of a command name, case-insensitive.
pub fn command_info(name: &str) -> Option<&'static CommandInfo> {
    COMMANDS.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

/// Class of a command line (behaviour 5); commands not in the table are administration.
pub fn classify_redis(args: &[Vec<u8>]) -> Class {
    let Some(name) = args.first() else {
        return Class::ReadData;
    };
    let name = String::from_utf8_lossy(name);
    match command_info(&name) {
        Some(info) => {
            // `CONFIG GET` reads, but still exposes server settings: keep it administration.
            info.class
        }
        None => Class::ModifySchema,
    }
}

/// Splits one console line into arguments as `redis-cli` does (behaviour 1). Offsets in errors are
/// character offsets into `line`.
pub fn tokenize(line: &str) -> Result<Vec<Vec<u8>>, ShellError> {
    let chars: Vec<char> = line.chars().collect();
    let mut args = Vec::new();
    let mut i = 0;
    let err = |offset: usize, message: &str| ShellError {
        offset,
        message: message.to_owned(),
    };
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let mut arg: Vec<u8> = Vec::new();
        let push = |arg: &mut Vec<u8>, c: char| {
            let mut buf = [0u8; 4];
            arg.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        };
        let mut quoted_end = false;
        while i < chars.len() && !chars[i].is_whitespace() {
            let ch = chars[i];
            if ch == '"' {
                let start = i;
                i += 1;
                loop {
                    let Some(&c) = chars.get(i) else {
                        return Err(err(start, "unclosed double quote"));
                    };
                    i += 1;
                    match c {
                        '"' => break,
                        '\\' => {
                            let Some(&e) = chars.get(i) else {
                                return Err(err(start, "unclosed double quote"));
                            };
                            i += 1;
                            match e {
                                'n' => arg.push(b'\n'),
                                'r' => arg.push(b'\r'),
                                't' => arg.push(b'\t'),
                                'b' => arg.push(8),
                                'a' => arg.push(7),
                                'x' => {
                                    let hex: String = chars
                                        .get(i..i + 2)
                                        .map(|s| s.iter().collect())
                                        .unwrap_or_default();
                                    match u8::from_str_radix(&hex, 16) {
                                        Ok(b) if hex.len() == 2 => {
                                            arg.push(b);
                                            i += 2;
                                        }
                                        _ => push(&mut arg, 'x'),
                                    }
                                }
                                other => push(&mut arg, other),
                            }
                        }
                        other => push(&mut arg, other),
                    }
                }
                quoted_end = true;
            } else if ch == '\'' {
                let start = i;
                i += 1;
                loop {
                    let Some(&c) = chars.get(i) else {
                        return Err(err(start, "unclosed single quote"));
                    };
                    i += 1;
                    match c {
                        '\'' => break,
                        '\\' if chars.get(i) == Some(&'\'') => {
                            arg.push(b'\'');
                            i += 1;
                        }
                        other => push(&mut arg, other),
                    }
                }
                quoted_end = true;
            } else {
                if quoted_end {
                    return Err(err(i, "a closing quote must be followed by a space"));
                }
                push(&mut arg, ch);
                i += 1;
            }
            if quoted_end && i < chars.len() && !chars[i].is_whitespace() {
                return Err(err(i, "a closing quote must be followed by a space"));
            }
        }
        args.push(arg);
    }
    Ok(args)
}

/// One command line of a console text: line index, character offset of the line, arguments.
pub type ScriptLine = (usize, usize, Vec<Vec<u8>>);

/// Lines of a console text that hold a command: (line index, character offset of the line,
/// arguments). Empty lines and `#` comments are skipped; the first bad line stops with its error
/// (offset into the whole text).
pub fn tokenize_script(text: &str) -> Result<Vec<ScriptLine>, ShellError> {
    let mut out = Vec::new();
    let mut offset = 0;
    for (n, line) in text.split('\n').enumerate() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            let args = tokenize(line).map_err(|e| ShellError {
                offset: offset + e.offset,
                message: e.message,
            })?;
            out.push((n, offset, args));
        }
        offset += line.chars().count() + 1;
    }
    Ok(out)
}

/// A server reply (behaviour 2).
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `(nil)`
    Nil,
    /// `(integer) n`
    Int(i64),
    /// Status reply such as `OK`.
    Simple(String),
    /// Bulk string.
    Bulk(Vec<u8>),
    /// Array.
    Array(Vec<Reply>),
    /// RESP3 map.
    Map(Vec<(Reply, Reply)>),
    /// RESP3 double.
    Double(f64),
    /// RESP3 boolean.
    Bool(bool),
    /// Error reply (`WRONGTYPE …`).
    Error(String),
}

impl Reply {
    /// Text of a bulk or simple string (lossy).
    pub fn text(&self) -> Option<String> {
        match self {
            Reply::Bulk(b) => Some(String::from_utf8_lossy(b).into_owned()),
            Reply::Simple(s) => Some(s.clone()),
            Reply::Int(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// As JSON for MCP and export: bulk strings as text or `{"base64": …}`.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Reply::Nil => serde_json::Value::Null,
            Reply::Int(n) => json!(n),
            Reply::Simple(s) => json!(s),
            Reply::Bulk(b) => bytes_json(b),
            Reply::Array(items) => {
                serde_json::Value::Array(items.iter().map(Reply::to_json).collect())
            }
            Reply::Map(pairs) => serde_json::Value::Array(
                pairs
                    .iter()
                    .map(|(k, v)| json!([k.to_json(), v.to_json()]))
                    .collect(),
            ),
            Reply::Double(d) => json!(d),
            Reply::Bool(b) => json!(b),
            Reply::Error(e) => json!({ "error": e }),
        }
    }
}

/// Bytes as JSON: a string when UTF-8, else `{"base64": …}`.
pub fn bytes_json(b: &[u8]) -> serde_json::Value {
    use base64::Engine;
    match std::str::from_utf8(b) {
        Ok(s) => serde_json::json!(s),
        Err(_) => {
            serde_json::json!({ "base64": base64::engine::general_purpose::STANDARD.encode(b) })
        }
    }
}

/// Bytes for display: text when UTF-8, else hex with the byte count (behaviour 2).
pub fn bytes_display(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("0x{hex} ({} bytes)", b.len())
        }
    }
}

fn quoted(b: &[u8]) -> String {
    let mut s = String::from("\"");
    for &x in b {
        match x {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(x as char),
            _ => s.push_str(&format!("\\x{x:02x}")),
        }
    }
    s.push('"');
    s
}

/// The raw form of a reply as `redis-cli` prints it.
pub fn render_reply(reply: &Reply) -> String {
    render(reply, 0)
}

fn render(reply: &Reply, indent: usize) -> String {
    match reply {
        Reply::Nil => "(nil)".into(),
        Reply::Int(n) => format!("(integer) {n}"),
        Reply::Simple(s) => s.clone(),
        Reply::Bulk(b) => quoted(b),
        Reply::Double(d) => format!("(double) {d}"),
        Reply::Bool(b) => format!("({})", if *b { "true" } else { "false" }),
        Reply::Error(e) => format!("(error) {e}"),
        Reply::Array(items) if items.is_empty() => "(empty array)".into(),
        Reply::Array(items) => {
            let width = items.len().to_string().len();
            items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let prefix = format!("{:>width$}) ", i + 1);
                    let pad = indent + prefix.len();
                    let body = render(item, pad);
                    let lead = if i == 0 {
                        String::new()
                    } else {
                        " ".repeat(indent)
                    };
                    format!("{lead}{prefix}{body}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Reply::Map(pairs) => {
            let flat: Vec<Reply> = pairs
                .iter()
                .flat_map(|(k, v)| [k.clone(), v.clone()])
                .collect();
            render(&Reply::Array(flat), indent)
        }
    }
}

impl From<redis::Value> for Reply {
    fn from(v: redis::Value) -> Self {
        use redis::Value as V;
        match v {
            V::Nil => Reply::Nil,
            V::Int(n) => Reply::Int(n),
            V::BulkString(b) => Reply::Bulk(b),
            V::Array(items) | V::Set(items) => {
                Reply::Array(items.into_iter().map(Reply::from).collect())
            }
            V::SimpleString(s) => Reply::Simple(s),
            V::Okay => Reply::Simple("OK".into()),
            V::Map(pairs) => Reply::Map(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            ),
            V::Attribute { data, .. } => (*data).into(),
            V::Double(d) => Reply::Double(d),
            V::Boolean(b) => Reply::Bool(b),
            V::VerbatimString { text, .. } => Reply::Bulk(text.into_bytes()),
            V::BigNumber(n) => Reply::Simple(String::from_utf8_lossy(&n).into_owned()),
            V::Push { data, .. } => Reply::Array(data.into_iter().map(Reply::from).collect()),
            V::ServerError(e) => Reply::Error(
                [e.code(), e.details().unwrap_or("")]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            #[allow(unreachable_patterns)]
            _ => Reply::Simple("(unsupported reply)".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(line: &str) -> Vec<String> {
        tokenize(line)
            .unwrap()
            .into_iter()
            .map(|a| String::from_utf8_lossy(&a).into_owned())
            .collect()
    }

    #[test]
    fn quoting_like_redis_cli() {
        assert_eq!(t("SET  greeting   hello"), ["SET", "greeting", "hello"]);
        assert_eq!(
            t(r#"SET k "a b\n\t\"q\" \\ \x41""#),
            ["SET", "k", "a b\n\t\"q\" \\ A"]
        );
        assert_eq!(t(r"SET k 'it\'s \n'"), ["SET", "k", "it's \\n"]);
        assert_eq!(t("SET k ''"), ["SET", "k", ""]);
        assert_eq!(t("GET café"), ["GET", "café"]);
        assert_eq!(tokenize("SET k \"\\xff\"").unwrap()[2], vec![0xff]);
        let e = tokenize("SET k \"open").unwrap_err();
        assert_eq!(e.offset, 6);
        assert!(e.message.contains("unclosed double quote"));
        assert_eq!(tokenize("GET 'x").unwrap_err().offset, 4);
        assert!(tokenize("GET \"a\"b").is_err());
    }

    #[test]
    fn scripts_skip_empty_lines_and_comments() {
        let lines = tokenize_script("# setup\nSET a 1\n\n  GET a  \n").unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].0, lines[0].1), (1, 8));
        assert_eq!(lines[1].0, 3);
        let e = tokenize_script("GET a\nGET \"b").unwrap_err();
        assert_eq!(e.offset, 10);
    }

    #[test]
    fn classification_by_table_unknown_is_administration() {
        let class = |l: &str| classify_redis(&tokenize(l).unwrap());
        assert_eq!(class("get a"), Class::ReadData);
        assert_eq!(class("HGETALL h"), Class::ReadData);
        assert_eq!(class("SET a 1"), Class::ModifyData);
        assert_eq!(class("unlink a b"), Class::ModifyData);
        assert_eq!(class("FLUSHDB"), Class::ModifySchema);
        assert_eq!(class("CONFIG SET maxmemory 1"), Class::ModifySchema);
        assert_eq!(class("EVAL 'return 1' 0"), Class::ModifySchema);
        assert_eq!(class("FOO.BAR x"), Class::ModifySchema);
        // The table is sorted and has no duplicates.
        for w in COMMANDS.windows(2) {
            assert!(w[0].name < w[1].name, "{} {}", w[0].name, w[1].name);
        }
    }

    #[test]
    fn replies_render_like_redis_cli() {
        assert_eq!(render_reply(&Reply::Nil), "(nil)");
        assert_eq!(render_reply(&Reply::Int(3)), "(integer) 3");
        assert_eq!(render_reply(&Reply::Simple("OK".into())), "OK");
        assert_eq!(render_reply(&Reply::Bulk(b"a\"b".to_vec())), r#""a\"b""#);
        assert_eq!(
            render_reply(&Reply::Bulk(vec![0xff, 0x00])),
            r#""\xff\x00""#
        );
        assert_eq!(bytes_display(&[0xff, 0x00]), "0xff00 (2 bytes)");
        assert_eq!(
            render_reply(&Reply::Error("WRONGTYPE no".into())),
            "(error) WRONGTYPE no"
        );
        assert_eq!(render_reply(&Reply::Array(vec![])), "(empty array)");
        let nested = Reply::Array(vec![
            Reply::Bulk(b"a".to_vec()),
            Reply::Array(vec![Reply::Int(1), Reply::Int(2)]),
        ]);
        assert_eq!(
            render_reply(&nested),
            "1) \"a\"\n2) 1) (integer) 1\n   2) (integer) 2"
        );
        assert_eq!(
            Reply::Bulk(vec![0xff]).to_json(),
            serde_json::json!({"base64": "/w=="})
        );
        assert_eq!(Reply::from(redis::Value::Okay), Reply::Simple("OK".into()));
    }
}

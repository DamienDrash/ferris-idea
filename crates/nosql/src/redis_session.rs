//! Redis sessions (`SPEC-0310` behaviours 4–5, `SPEC-0312` behaviours 1–5) on the `redis` crate.

use std::collections::BTreeSet;

use idecraft_database::Secret;
use redis::aio::MultiplexedConnection;
use redis::{
    AsyncConnectionConfig, ConnectionAddr, IntoConnectionInfo, RedisConnectionInfo, Value,
};

use crate::completion::scan_pattern;
use crate::edit::RedisEdit;
use crate::redis_cli::{KEYS_LIMIT, Reply, classify_redis, tokenize_script};
use crate::{CONNECT_TIMEOUT, Class, Permission, StoreError, StoreProfile, Tls, check_class};

/// A key as listed in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// Key name (binary safe).
    pub name: Vec<u8>,
    /// `string`, `hash`, `list`, `set`, `zset`, `stream`, or `none` when it vanished.
    pub kind: String,
    /// Remaining time to live in seconds; `None` without TTL.
    pub ttl: Option<i64>,
}

/// A key's value as read for the value view (behaviour 3).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyValue {
    /// Key.
    pub key: Vec<u8>,
    /// Type.
    pub kind: String,
    /// Remaining TTL in seconds.
    pub ttl: Option<i64>,
    /// Total length (`STRLEN`, `HLEN`, `LLEN`, `SCARD`, `ZCARD`, `XLEN`).
    pub total: u64,
    /// What was read (bounded).
    pub data: ValueData,
}

impl KeyValue {
    /// How many elements (or bytes for strings) are shown.
    pub fn shown(&self) -> u64 {
        match &self.data {
            ValueData::String(b) => b.len() as u64,
            ValueData::Pairs(p) => p.len() as u64,
            ValueData::Items(i) => i.len() as u64,
            ValueData::Scored(s) => s.len() as u64,
            ValueData::Stream(s) => s.len() as u64,
            ValueData::None => 0,
        }
    }
}

/// A stream entry: id and field/value pairs.
pub type StreamEntry = (String, Vec<(Vec<u8>, Vec<u8>)>);

/// Value data by type.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueData {
    /// The key does not exist.
    None,
    /// String bytes (at most 1 MiB).
    String(Vec<u8>),
    /// Hash fields and values.
    Pairs(Vec<(Vec<u8>, Vec<u8>)>),
    /// List elements (index = position) or set members.
    Items(Vec<Vec<u8>>),
    /// Sorted set members and scores.
    Scored(Vec<(Vec<u8>, f64)>),
    /// Stream entries: id and fields.
    Stream(Vec<StreamEntry>),
}

const STRING_LIMIT: u64 = 1 << 20;
const ELEMENT_LIMIT: usize = 500;
const STREAM_LIMIT: usize = 100;

/// An open Redis session.
pub struct RedisSession {
    conn: MultiplexedConnection,
    profile: StoreProfile,
    db: u8,
}

fn io_error(e: redis::RedisError) -> StoreError {
    StoreError::Server {
        code: e.code().map(str::to_owned),
        message: e.to_string(),
    }
}

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::BulkString(b) => b.clone(),
        Value::SimpleString(s) => s.clone().into_bytes(),
        Value::Int(n) => n.to_string().into_bytes(),
        Value::VerbatimString { text, .. } => text.clone().into_bytes(),
        Value::Okay => b"OK".to_vec(),
        Value::Double(d) => d.to_string().into_bytes(),
        _ => Vec::new(),
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        other => String::from_utf8_lossy(&bytes(other)).parse().unwrap_or(0),
    }
}

fn items(v: Value) -> Vec<Value> {
    match v {
        Value::Array(a) | Value::Set(a) => a,
        Value::Map(m) => m.into_iter().flat_map(|(k, v)| [k, v]).collect(),
        _ => Vec::new(),
    }
}

fn pairs(v: Value) -> Vec<(Vec<u8>, Vec<u8>)> {
    items(v)
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (bytes(&c[0]), bytes(&c[1])))
        .collect()
}

impl RedisSession {
    /// Connects, authenticates and selects the profile's database within [`CONNECT_TIMEOUT`].
    pub async fn connect(
        profile: &StoreProfile,
        password: Option<Secret>,
    ) -> Result<Self, StoreError> {
        let endpoint = profile.endpoint()?;
        let fail = |message: String| StoreError::Connect {
            profile: profile.name.clone(),
            address: endpoint.address(),
            message,
        };
        let (host, port) = endpoint.hosts[0].clone();
        let port = port.unwrap_or(6379);
        let addr = match endpoint.tls {
            Tls::Off => ConnectionAddr::Tcp(host, port),
            Tls::Verified | Tls::Unverified => ConnectionAddr::TcpTls {
                host,
                port,
                insecure: endpoint.tls == Tls::Unverified,
                tls_params: None,
            },
        };
        let mut redis_info = RedisConnectionInfo::default().set_db(i64::from(endpoint.db_index));
        if let Some(user) = endpoint
            .user
            .clone()
            .filter(|u| u != "default" || password.is_some())
        {
            redis_info = redis_info.set_username(user);
        }
        if let Some(p) = &password {
            redis_info = redis_info.set_password(p.expose());
        }
        let info = addr
            .into_connection_info()
            .map_err(|e| fail(e.to_string()))?
            .set_redis_settings(redis_info);
        let client = redis::Client::open(info).map_err(|e| fail(e.to_string()))?;
        // redis 1.x defaults to a 1 s connect and a 500 ms response timeout; keep the profile's
        // connect timeout and no response timeout, so slow or blocking console commands (`BLPOP`,
        // `DEBUG SLEEP`) are not cut off.
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(Some(CONNECT_TIMEOUT))
            .set_response_timeout(None);
        let conn = tokio::time::timeout(
            CONNECT_TIMEOUT,
            client.get_multiplexed_async_connection_with_config(&config),
        )
        .await
        .map_err(|_| fail("timed out".into()))?
        .map_err(|e| fail(e.to_string()))?;
        let mut session = Self {
            conn,
            profile: profile.clone(),
            db: endpoint.db_index,
        };
        tokio::time::timeout(CONNECT_TIMEOUT, session.raw(&[b"PING".to_vec()]))
            .await
            .map_err(|_| fail("timed out".into()))?
            .map_err(|e| fail(e.to_string()))?;
        Ok(session)
    }

    /// The profile.
    pub fn profile(&self) -> &StoreProfile {
        &self.profile
    }

    /// Server version (`INFO server`).
    pub async fn server_version(&mut self) -> Result<String, StoreError> {
        let info = self.query(&[b"INFO", b"server"]).await?;
        let text = String::from_utf8_lossy(&bytes(&info)).into_owned();
        Ok(text
            .lines()
            .find_map(|l| l.trim().strip_prefix("redis_version:"))
            .unwrap_or("?")
            .to_owned())
    }

    /// Database the console works in.
    pub fn db(&self) -> u8 {
        self.db
    }

    async fn raw(&mut self, args: &[Vec<u8>]) -> Result<Value, redis::RedisError> {
        let mut cmd = redis::Cmd::new();
        for a in args {
            cmd.arg(a.as_slice());
        }
        cmd.query_async::<Value>(&mut self.conn).await
    }

    async fn query(&mut self, args: &[&[u8]]) -> Result<Value, StoreError> {
        let owned: Vec<Vec<u8>> = args.iter().map(|a| a.to_vec()).collect();
        self.raw(&owned).await.map_err(io_error)
    }

    /// Databases holding keys plus the session's own, with key counts (tree level 1).
    pub async fn databases(&mut self) -> Result<Vec<(u8, u64)>, StoreError> {
        let info = self.query(&[b"INFO", b"keyspace"]).await?;
        let text = String::from_utf8_lossy(&bytes(&info)).into_owned();
        let mut out = parse_keyspace(&text);
        if !out.iter().any(|(d, _)| *d == self.db) {
            out.push((self.db, 0));
            out.sort();
        }
        Ok(out)
    }

    /// One `SCAN` page of keys matching `pattern` with type and TTL (behaviour 5); the returned
    /// cursor is 0 when the scan is complete.
    pub async fn scan(
        &mut self,
        pattern: &str,
        cursor: u64,
        count: usize,
    ) -> Result<(u64, Vec<KeyInfo>), StoreError> {
        let count = count.clamp(1, ELEMENT_LIMIT).to_string();
        let cursor_text = cursor.to_string();
        let reply = self
            .query(&[
                b"SCAN",
                cursor_text.as_bytes(),
                b"MATCH",
                pattern.as_bytes(),
                b"COUNT",
                count.as_bytes(),
            ])
            .await?;
        let mut parts = items(reply).into_iter();
        let next = parts.next().map(|v| int(&v) as u64).unwrap_or(0);
        let names: Vec<Vec<u8>> = parts
            .next()
            .map(items)
            .unwrap_or_default()
            .iter()
            .map(bytes)
            .collect();
        let mut keys = Vec::with_capacity(names.len());
        if !names.is_empty() {
            let mut pipe = redis::pipe();
            for n in &names {
                pipe.cmd("TYPE")
                    .arg(n.as_slice())
                    .cmd("PTTL")
                    .arg(n.as_slice());
            }
            let replies: Vec<Value> = pipe.query_async(&mut self.conn).await.map_err(io_error)?;
            for (i, n) in names.into_iter().enumerate() {
                let kind = replies.get(2 * i).map(bytes).unwrap_or_default();
                let pttl = replies.get(2 * i + 1).map(int).unwrap_or(-1);
                keys.push(KeyInfo {
                    name: n,
                    kind: String::from_utf8_lossy(&kind).into_owned(),
                    ttl: (pttl >= 0).then(|| (pttl + 999) / 1000),
                });
            }
        }
        keys.sort_by(|a, b| a.name.cmp(&b.name));
        Ok((next, keys))
    }

    /// Key names starting with `prefix` for completion: one bounded `SCAN` round, at most 50.
    pub async fn key_names(&mut self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let pattern = scan_pattern(prefix);
        let reply = self
            .query(&[
                b"SCAN",
                b"0",
                b"MATCH",
                pattern.as_bytes(),
                b"COUNT",
                b"200",
            ])
            .await?;
        let mut names: Vec<String> = items(reply)
            .into_iter()
            .nth(1)
            .map(items)
            .unwrap_or_default()
            .iter()
            .map(|v| String::from_utf8_lossy(&bytes(v)).into_owned())
            .collect();
        names.sort();
        names.truncate(50);
        Ok(names)
    }

    /// A key's value by type, bounded (behaviour 3).
    pub async fn read(&mut self, key: &[u8]) -> Result<KeyValue, StoreError> {
        let kind =
            String::from_utf8_lossy(&bytes(&self.query(&[b"TYPE", key]).await?)).into_owned();
        let pttl = int(&self.query(&[b"PTTL", key]).await?);
        let ttl = (pttl >= 0).then(|| (pttl + 999) / 1000);
        let limit = (ELEMENT_LIMIT - 1).to_string();
        let (total, data) = match kind.as_str() {
            "string" => {
                let len = int(&self.query(&[b"STRLEN", key]).await?) as u64;
                let v = if len <= STRING_LIMIT {
                    self.query(&[b"GET", key]).await?
                } else {
                    let end = (STRING_LIMIT - 1).to_string();
                    self.query(&[b"GETRANGE", key, b"0", end.as_bytes()])
                        .await?
                };
                (len, ValueData::String(bytes(&v)))
            }
            "hash" => {
                let len = int(&self.query(&[b"HLEN", key]).await?) as u64;
                let mut out = Vec::new();
                let mut cursor = "0".to_owned();
                loop {
                    let r = self
                        .query(&[b"HSCAN", key, cursor.as_bytes(), b"COUNT", b"200"])
                        .await?;
                    let mut parts = items(r).into_iter();
                    cursor = String::from_utf8_lossy(
                        &parts.next().map(|v| bytes(&v)).unwrap_or_default(),
                    )
                    .into_owned();
                    out.extend(parts.next().map(pairs).unwrap_or_default());
                    if cursor == "0" || cursor.is_empty() || out.len() >= ELEMENT_LIMIT {
                        break;
                    }
                }
                out.truncate(ELEMENT_LIMIT);
                (len, ValueData::Pairs(out))
            }
            "list" => {
                let len = int(&self.query(&[b"LLEN", key]).await?) as u64;
                let v = self
                    .query(&[b"LRANGE", key, b"0", limit.as_bytes()])
                    .await?;
                (len, ValueData::Items(items(v).iter().map(bytes).collect()))
            }
            "set" => {
                let len = int(&self.query(&[b"SCARD", key]).await?) as u64;
                let mut out: Vec<Vec<u8>> = Vec::new();
                let mut cursor = "0".to_owned();
                loop {
                    let r = self
                        .query(&[b"SSCAN", key, cursor.as_bytes(), b"COUNT", b"200"])
                        .await?;
                    let mut parts = items(r).into_iter();
                    cursor = String::from_utf8_lossy(
                        &parts.next().map(|v| bytes(&v)).unwrap_or_default(),
                    )
                    .into_owned();
                    out.extend(
                        parts
                            .next()
                            .map(items)
                            .unwrap_or_default()
                            .iter()
                            .map(bytes),
                    );
                    if cursor == "0" || cursor.is_empty() || out.len() >= ELEMENT_LIMIT {
                        break;
                    }
                }
                out.sort();
                out.dedup();
                out.truncate(ELEMENT_LIMIT);
                (len, ValueData::Items(out))
            }
            "zset" => {
                let len = int(&self.query(&[b"ZCARD", key]).await?) as u64;
                let v = self
                    .query(&[b"ZRANGE", key, b"0", limit.as_bytes(), b"WITHSCORES"])
                    .await?;
                let scored = pairs(v)
                    .into_iter()
                    .map(|(m, s)| (m, String::from_utf8_lossy(&s).parse().unwrap_or(f64::NAN)))
                    .collect();
                (len, ValueData::Scored(scored))
            }
            "stream" => {
                let len = int(&self.query(&[b"XLEN", key]).await?) as u64;
                let count = STREAM_LIMIT.to_string();
                let v = self
                    .query(&[b"XRANGE", key, b"-", b"+", b"COUNT", count.as_bytes()])
                    .await?;
                let entries = items(v)
                    .into_iter()
                    .map(|e| {
                        let mut parts = items(e).into_iter();
                        let id = String::from_utf8_lossy(
                            &parts.next().map(|v| bytes(&v)).unwrap_or_default(),
                        )
                        .into_owned();
                        (id, parts.next().map(pairs).unwrap_or_default())
                    })
                    .collect();
                (len, ValueData::Stream(entries))
            }
            _ => (0, ValueData::None),
        };
        Ok(KeyValue {
            key: key.to_vec(),
            kind,
            ttl,
            total,
            data,
        })
    }

    /// Checks one command line (behaviour 5) before it is sent.
    async fn check(
        &mut self,
        args: &[Vec<u8>],
        rights: Option<&BTreeSet<Permission>>,
        confirmed: bool,
    ) -> Result<(), StoreError> {
        let name = String::from_utf8_lossy(&args[0]).to_uppercase();
        let class = classify_redis(args);
        check_class(&self.profile, class, rights, &name)?;
        match name.as_str() {
            "KEYS" => {
                let n = int(&self.query(&[b"DBSIZE"]).await?) as u64;
                if n > KEYS_LIMIT {
                    return Err(StoreError::Confirm(format!(
                        "KEYS would block the server: database {} holds {n} keys (more than {KEYS_LIMIT}); use SCAN 0 MATCH <pattern> instead",
                        self.db
                    )));
                }
            }
            "FLUSHDB" | "FLUSHALL" if !confirmed => {
                let n = int(&self.query(&[b"DBSIZE"]).await?);
                let scope = if name == "FLUSHDB" {
                    format!("database {} with {n} keys", self.db)
                } else {
                    "every database of the server".to_owned()
                };
                return Err(StoreError::Confirm(format!(
                    "{name} deletes {scope}; confirm to run it"
                )));
            }
            _ => {}
        }
        Ok(())
    }

    /// Runs console text line by line (behaviour 1); stops at the first error reply. Returns the
    /// line index and reply of each command that ran.
    pub async fn run(
        &mut self,
        text: &str,
        rights: Option<&BTreeSet<Permission>>,
        confirmed: bool,
    ) -> Result<Vec<(usize, Reply)>, StoreError> {
        let lines = tokenize_script(text)?;
        let mut out = Vec::new();
        for (line, _, args) in lines {
            self.check(&args, rights, confirmed).await?;
            let reply = match self.raw(&args).await {
                Ok(v) => Reply::from(v),
                Err(e) if e.is_io_error() || e.is_timeout() || e.is_connection_dropped() => {
                    return Err(io_error(e));
                }
                Err(e) => Reply::Error(match (e.code(), e.detail()) {
                    (Some(c), Some(d)) => format!("{c} {d}"),
                    _ => e.to_string(),
                }),
            };
            let failed = matches!(reply, Reply::Error(_));
            if !failed
                && String::from_utf8_lossy(&args[0]).eq_ignore_ascii_case("SELECT")
                && let Some(n) = args
                    .get(1)
                    .and_then(|a| std::str::from_utf8(a).ok()?.parse().ok())
            {
                self.db = n;
            }
            out.push((line, reply));
            if failed {
                break;
            }
        }
        Ok(out)
    }

    /// Applies an edit (behaviour 4); a rename onto an existing key is refused.
    pub async fn apply(
        &mut self,
        edit: &RedisEdit,
        rights: Option<&BTreeSet<Permission>>,
    ) -> Result<Reply, StoreError> {
        check_class(&self.profile, Class::ModifyData, rights, &edit.preview())?;
        let reply: Reply = self.raw(&edit.args()).await.map_err(io_error)?.into();
        if let RedisEdit::Rename { to, .. } = edit
            && reply == Reply::Int(0)
        {
            return Err(StoreError::Edit(format!(
                "`{}` exists; nothing was renamed",
                String::from_utf8_lossy(to)
            )));
        }
        Ok(reply)
    }
}

/// `INFO keyspace` → (database, keys).
fn parse_keyspace(text: &str) -> Vec<(u8, u64)> {
    let mut out: Vec<(u8, u64)> = text
        .lines()
        .filter_map(|l| {
            let (db, rest) = l.trim().split_once(':')?;
            let n: u8 = db.strip_prefix("db")?.parse().ok()?;
            let keys = rest
                .split(',')
                .find_map(|kv| kv.strip_prefix("keys="))?
                .parse()
                .ok()?;
            Some((n, keys))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyspace_lines() {
        let text =
            "# Keyspace\r\ndb0:keys=3,expires=1,avg_ttl=0\r\ndb5:keys=12,expires=0,avg_ttl=0\r\n";
        assert_eq!(parse_keyspace(text), vec![(0, 3), (5, 12)]);
        assert!(parse_keyspace("# Keyspace\r\n").is_empty());
    }
}

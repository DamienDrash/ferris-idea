# SPEC-0312: Redis console, completion, value views and editing

- **Version:** 1 · **Date:** 2026-11-05
- **Sources:** GEN (Redis tools offer a command console and show a key's value by its type), STD
  (RESP2/RESP3; `redis-cli` argument quoting), DOC (Redis command reference: data types, `SCAN`,
  `TYPE`, `TTL`, `OBJECT`, `INFO keyspace`, `HGETALL`, `LRANGE`, `SMEMBERS`, `ZRANGE … WITHSCORES`,
  `XRANGE` — read, not quoted), ADR-0022, SPEC-0012, SPEC-0310
- **Feature matrix rows:** F-053, F-054, F-055, F-060

## Purpose
The user runs Redis commands in a console with completion, opens a key to see its value in a view
that fits its type, edits keys and values with a preview of the commands, and exports them.

## Behaviour
1. **Console.** One command per line; the line at the caret runs (a selection runs line by line and
   stops at the first error). Arguments split as in `redis-cli`: whitespace separates; double quotes
   allow `\n \r \t \" \\ \xHH`; single quotes allow only `\'`; an unclosed quote is an error at its
   offset. Empty lines and lines starting with `#` are skipped. `SELECT n` changes the console's
   database for following commands. [STD]
2. **Replies** are shown by kind: simple string, error (with its prefix such as `WRONGTYPE`),
   integer, bulk string (text when valid UTF-8, else hex with the byte count), nil, array (indexed,
   nested), map (field/value). Every reply also has its raw form as `redis-cli` would print it. [STD]
3. **Value view.** Opening a key reads `TYPE` and `TTL` and then its value by type, bounded: string
   (up to 1 MiB; text, pretty JSON when it parses as JSON, else hex), hash (`HSCAN` field/value
   grid), list (`LRANGE` index/value, first 500), set (`SSCAN` members), sorted set (`ZRANGE … WITHSCORES`
   member/score, first 500), stream (`XRANGE` id/fields, first 100). The view says how many elements
   exist (`STRLEN`/`HLEN`/`LLEN`/`SCARD`/`ZCARD`/`XLEN`) and how many are shown. [DOC, IMP-0290]
4. **Editing** builds commands and shows them before they run: set a string (`SET key value KEEPTTL`),
   set/remove TTL (`EXPIRE`/`PERSIST`), rename (`RENAME` refused when the new name exists — `RENAMENX`
   semantics), delete (`UNLINK`), hash field set/remove (`HSET`/`HDEL`), list element set/append/remove
   (`LSET`/`RPUSH`/`LREM key 1 value`), set member add/remove (`SADD`/`SREM`), sorted set member
   add/score/remove (`ZADD`/`ZREM`), new key of any of these types. Values are written as binary-safe
   arguments, never interpolated into a command line. [GEN]
5. **Classification for rights and read-only** (SPEC-0012) by a command table: read-data (e.g.
   `GET MGET HGET HGETALL HSCAN LRANGE LLEN SMEMBERS SSCAN SCARD ZRANGE ZSCORE ZCARD XRANGE XLEN TYPE
   TTL PTTL EXISTS STRLEN SCAN DBSIZE PING ECHO INFO TIME OBJECT MEMORY SELECT`), modify-data (`SET
   MSET DEL UNLINK EXPIRE PERSIST RENAME HSET HDEL LPUSH RPUSH LSET LREM SADD SREM ZADD ZREM XADD
   INCR DECR APPEND …`), modify-schema for administration (`FLUSHDB FLUSHALL CONFIG SHUTDOWN DEBUG
   SAVE BGSAVE BGREWRITEAOF REPLICAOF SLAVEOF CLIENT ACL MODULE SCRIPT FUNCTION EVAL EVALSHA
   FCALL MIGRATE CLUSTER FAILOVER`) and for any command not in the table. `KEYS` is refused in the
   console with a hint to `SCAN` unless the database holds at most 10 000 keys (`DBSIZE`). `FLUSHDB`
   and `FLUSHALL` ask for confirmation naming the database and its key count. [SPEC-0012, IMP-0290]
6. **Completion**: at the start of a line, command names (upper case, filtered by prefix,
   case-insensitive) with their argument summary; at a key argument position, key names that start
   with the typed prefix, read with one `SCAN … MATCH <prefix>* COUNT 200` round (at most 50
   candidates, glob characters in the prefix escaped). [GEN]
7. **Export** of a key or a key page: JSON (one object per key with key, type, ttl, value — strings
   as text or `{"base64": …}` when not UTF-8), JSON Lines, CSV (key, type, ttl, value as JSON).
   [SPEC-0310]

## Acceptance criteria
- **AK-1** Tokenizer: quoting and escapes of behaviour 1, unclosed quote offset, comments, empty
  lines (unit tests `redis_cli::tests`).
- **AK-2** Classification incl. unknown command → modify-schema, `KEYS` hint, `FLUSHDB` confirmation
  (unit tests).
- **AK-3** Reply rendering of behaviour 2 incl. binary bulk strings and nesting (unit tests).
- **AK-4** Edit commands of behaviour 4 as argument lists, rename refused on clash (unit tests).
- **AK-5** Completion of commands and of keys by prefix with glob escaping (unit tests with a fake
  key source).
- **AK-6** Live (Redis container with password): every type of behaviour 3 is written and read back
  with counts, TTL set and removed, rename refused on clash, a binary value shown as hex, `SCAN`
  pages, read-only refusal, `FLUSHDB` needs confirmation (`crates/nosql/tests/live.rs`).

## Out of scope
Pub/Sub, `MONITOR`, Lua script editing, Redis modules' own types (JSON, search, time series) beyond
showing their raw replies, cluster slot routing.

## Known deviations from the reference
Browsing never runs `KEYS` and every value view is bounded, so a large production key space cannot
block the server from the IDE (IMP-0290). Administration commands need an explicit right for agents
and a confirmation for people (IMP-0293).

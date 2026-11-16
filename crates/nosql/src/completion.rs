//! Console completion (`SPEC-0311` behaviour 8, `SPEC-0312` behaviour 6).

use std::collections::HashMap;

use crate::redis_cli::{COMMANDS, command_info, tokenize};

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreCandidate {
    /// Text inserted.
    pub label: String,
    /// `collection`, `method`, `operator`, `stage`, `field`, `command`, `key`.
    pub kind: &'static str,
    /// Extra text (argument summary, types).
    pub detail: String,
}

/// Collection methods offered after `db.<coll>.`.
pub const MONGO_METHODS: &[&str] = &[
    "aggregate",
    "countDocuments",
    "createIndex",
    "deleteMany",
    "deleteOne",
    "distinct",
    "drop",
    "dropIndex",
    "estimatedDocumentCount",
    "find",
    "findOne",
    "insertMany",
    "insertOne",
    "replaceOne",
    "updateMany",
    "updateOne",
];

const QUERY_OPS: &[&str] = &[
    "$eq",
    "$ne",
    "$gt",
    "$gte",
    "$lt",
    "$lte",
    "$in",
    "$nin",
    "$exists",
    "$type",
    "$regex",
    "$and",
    "$or",
    "$nor",
    "$not",
    "$elemMatch",
    "$size",
    "$all",
];
const UPDATE_OPS: &[&str] = &[
    "$set",
    "$unset",
    "$inc",
    "$push",
    "$pull",
    "$addToSet",
    "$rename",
    "$mul",
    "$min",
    "$max",
    "$currentDate",
];
const STAGES: &[&str] = &[
    "$match",
    "$project",
    "$group",
    "$sort",
    "$limit",
    "$skip",
    "$unwind",
    "$lookup",
    "$count",
    "$addFields",
    "$set",
    "$unset",
    "$facet",
    "$replaceRoot",
    "$sample",
    "$out",
    "$merge",
];

/// What the MongoDB console knows for completion.
#[derive(Debug, Clone, Default)]
pub struct MongoContext {
    /// Collections of the current database.
    pub collections: Vec<String>,
    /// Sampled field paths per collection.
    pub fields: HashMap<String, Vec<String>>,
}

fn filter(
    prefix: &str,
    items: impl IntoIterator<Item = String>,
    kind: &'static str,
) -> Vec<StoreCandidate> {
    let lower = prefix.to_lowercase();
    items
        .into_iter()
        .filter(|i| i.to_lowercase().starts_with(&lower))
        .map(|label| StoreCandidate {
            label,
            kind,
            detail: String::new(),
        })
        .collect()
}

/// Candidates at character offset `caret` of `text`, and the character offset where the typed
/// prefix starts (the text replaced on accept).
pub fn complete_mongo(
    text: &str,
    caret: usize,
    ctx: &MongoContext,
) -> (usize, Vec<StoreCandidate>) {
    let chars: Vec<char> = text.chars().take(caret).collect();
    let mut start = chars.len();
    while start > 0
        && (chars[start - 1].is_alphanumeric() || matches!(chars[start - 1], '_' | '$' | '.'))
    {
        start -= 1;
    }
    let word: String = chars[start..].iter().collect();
    let before: String = chars[..start].iter().collect();

    // `db.` / `db.coll.` / `db.coll.met`
    if !before.contains('(') && word.starts_with("db.") {
        let rest = &word[3..];
        return match rest.rsplit_once('.') {
            None => {
                let mut c = filter(rest, ctx.collections.iter().cloned(), "collection");
                c.extend(filter(rest, ["getCollection".to_owned()], "method"));
                (start + 3, c)
            }
            Some((coll, method)) => {
                let at = start + 3 + coll.chars().count() + 1;
                (
                    at,
                    filter(
                        method,
                        MONGO_METHODS.iter().map(|m| (*m).to_owned()),
                        "method",
                    ),
                )
            }
        };
    }

    // Inside a call: find the method and the argument index.
    let Some((collection, method, arg_index, depth, innermost)) = call_context(&before) else {
        return (start, Vec::new());
    };
    let key_position = innermost == '{'
        && matches!(before.trim_end().chars().last(), Some('{' | ','))
        && depth > 0;
    if !key_position {
        return (start, Vec::new());
    }
    if word.starts_with('$') {
        let ops: &[&str] = if method == "aggregate" && depth == 1 {
            STAGES
        } else if matches!(method.as_str(), "updateOne" | "updateMany")
            && arg_index == 1
            && depth == 1
        {
            UPDATE_OPS
        } else {
            QUERY_OPS
        };
        let kind = if ops == STAGES { "stage" } else { "operator" };
        return (
            start,
            filter(&word, ops.iter().map(|o| (*o).to_owned()), kind),
        );
    }
    let fields = ctx.fields.get(&collection).cloned().unwrap_or_default();
    (start, filter(&word, fields, "field"))
}

/// (collection, method, argument index, object depth inside the call, innermost open bracket)
/// for text ending inside `db.<coll>.<method>(`; `None` inside a string.
fn call_context(before: &str) -> Option<(String, String, usize, usize, char)> {
    let chars: Vec<char> = before.chars().collect();
    let mut stack: Vec<(char, usize, usize)> = Vec::new();
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    for (j, &c) in chars.iter().enumerate() {
        if let Some(q) = in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                in_string = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => in_string = Some(c),
            '(' | '{' | '[' => stack.push((c, j, 0)),
            ')' | '}' | ']' => {
                stack.pop();
            }
            ',' => {
                if let Some(top) = stack.last_mut() {
                    top.2 += 1;
                }
            }
            _ => {}
        }
    }
    if in_string.is_some() {
        return None;
    }
    let call = stack.iter().rposition(|(c, _, _)| *c == '(')?;
    let (_, open, args) = stack[call];
    let depth = stack[call + 1..]
        .iter()
        .filter(|(c, _, _)| *c == '{')
        .count();
    let innermost = stack.last().map(|(c, _, _)| *c).unwrap_or('(');
    let head: String = chars[..open].iter().collect();
    let head = head.trim_end();
    let (target, method) = head.rsplit_once('.')?;
    let collection = if let Some(rest) = target.strip_prefix("db.getCollection(") {
        rest.trim_end_matches(')')
            .trim_matches(['"', '\''])
            .to_owned()
    } else {
        let t = target
            .rsplit(|c: char| c.is_whitespace() || c == ';')
            .next()?;
        t.strip_prefix("db.")?.to_owned()
    };
    Some((collection, method.to_owned(), args, depth, innermost))
}

/// A `SCAN MATCH` pattern for keys starting with `prefix`: glob characters escaped (behaviour 6).
pub fn scan_pattern(prefix: &str) -> String {
    let mut s = String::new();
    for c in prefix.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('*');
    s
}

/// Candidates for a Redis console line at `caret`; `keys(prefix)` returns key names starting with
/// the prefix (bounded by the caller). Returns the start offset of the typed word.
pub fn complete_redis(
    line: &str,
    caret: usize,
    keys: &mut dyn FnMut(&str) -> Vec<String>,
) -> (usize, Vec<StoreCandidate>) {
    let chars: Vec<char> = line.chars().take(caret).collect();
    let mut start = chars.len();
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let word: String = chars[start..].iter().collect();
    let before: String = chars[..start].iter().collect();
    let Ok(args) = tokenize(&before) else {
        return (start, Vec::new());
    };
    if args.is_empty() {
        let upper = word.to_uppercase();
        let c = COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(&upper))
            .map(|c| StoreCandidate {
                label: c.name.to_owned(),
                kind: "command",
                detail: c.args.to_owned(),
            })
            .collect();
        return (start, c);
    }
    let name = String::from_utf8_lossy(&args[0]).into_owned();
    let Some(info) = command_info(&name) else {
        return (start, Vec::new());
    };
    let position = args.len();
    let is_key = info.key_at > 0
        && (position == usize::from(info.key_at)
            || (info.all_keys && position >= usize::from(info.key_at)));
    if !is_key {
        return (start, Vec::new());
    }
    let mut names = keys(&word);
    names.retain(|k| k.starts_with(&word));
    names.sort();
    names.dedup();
    names.truncate(50);
    (
        start,
        names
            .into_iter()
            .map(|label| StoreCandidate {
                label,
                kind: "key",
                detail: String::new(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MongoContext {
        MongoContext {
            collections: vec!["items".into(), "orders".into(), "inventory".into()],
            fields: [(
                "items".to_owned(),
                vec!["name".to_owned(), "price".to_owned(), "dim.w".to_owned()],
            )]
            .into_iter()
            .collect(),
        }
    }

    fn labels(text: &str) -> Vec<String> {
        complete_mongo(text, text.chars().count(), &ctx())
            .1
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn mongo_contexts() {
        assert_eq!(labels("db.i"), ["items", "inventory"]);
        assert!(labels("db.").contains(&"getCollection".to_owned()));
        assert_eq!(complete_mongo("db.ite", 6, &ctx()).0, 3);
        assert_eq!(labels("db.items.fin"), ["find", "findOne"]);
        assert_eq!(complete_mongo("db.items.fin", 12, &ctx()).0, 9);
        assert_eq!(labels("db.items.find({p"), ["price"]);
        assert_eq!(
            labels("db.items.find({name: 'a', "),
            ["name", "price", "dim.w"]
        );
        assert_eq!(labels("db.items.find({price: {$g"), ["$gt", "$gte"]);
        assert_eq!(labels("db.items.updateOne({a: 1}, {$s"), ["$set"]);
        assert_eq!(labels("db.items.updateOne({$a"), ["$and", "$all"]);
        assert_eq!(labels("db.items.aggregate([{$m"), ["$match", "$merge"]);
        assert_eq!(labels("db.items.aggregate([{$match: {$o"), ["$or"]);
        assert_eq!(labels("db.getCollection('items').find({na"), ["name"]);
        assert!(
            labels("db.items.find({name: 'x").is_empty(),
            "inside a string value"
        );
        assert!(
            labels("db.items.find({name: pr").is_empty(),
            "value position"
        );
        assert!(labels("show c").is_empty());
    }

    #[test]
    fn redis_commands_and_keys() {
        let mut keys = |prefix: &str| -> Vec<String> {
            ["user:1", "user:2", "session"]
                .iter()
                .filter(|k| k.starts_with(prefix))
                .map(|k| (*k).to_owned())
                .collect()
        };
        let (at, c) = complete_redis("hg", 2, &mut keys);
        assert_eq!(at, 0);
        assert_eq!(
            c.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["HGET", "HGETALL"]
        );
        assert_eq!(c[0].detail, "key field");
        let (at, c) = complete_redis("GET us", 6, &mut keys);
        assert_eq!(at, 4);
        assert_eq!(
            c.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["user:1", "user:2"]
        );
        assert!(
            complete_redis("HSET user:1 na", 14, &mut keys).1.is_empty(),
            "field, not a key"
        );
        assert_eq!(
            complete_redis("DEL a s", 7, &mut keys).1.len(),
            1,
            "every DEL argument is a key"
        );
        assert!(complete_redis("NOPE x", 6, &mut keys).1.is_empty());
    }

    #[test]
    fn scan_patterns_escape_glob_characters() {
        assert_eq!(scan_pattern("user:"), "user:*");
        assert_eq!(scan_pattern("a*b?[c]\\"), "a\\*b\\?\\[c\\]\\\\*");
    }
}

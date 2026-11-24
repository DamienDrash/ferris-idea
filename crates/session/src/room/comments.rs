//! Comments of a room (`SPEC-0323` behaviour 22, v8 behaviours 53–60): threads on a document
//! position with replies, resolving and reopening, deleting one's own, and a notification to every
//! participant when the comments of a document changed, so their windows show them at once.

use serde_json::{Value, json};

use super::super::{Comment, Room, invalid, not_granted};
use super::{Backend, Call, ErrorCode, Handle, MAX_COMMENT, MAX_COMMENTS, RpcError, str_of};
use crate::room::access::Role;

/// The longest document path a comment names, as for presence (behaviour 21).
const MAX_DOC: usize = 4096;

/// The notification sent when the comments of a document changed (v8 behaviour 58).
pub(super) const CHANGED: &str = "room.comments_changed";

/// Whether `text` is a comment's text: 1 to 16 384 bytes of plain text, no control character but
/// line breaks and tabs (v8 behaviour 57).
fn check_text(text: &str) -> Result<(), RpcError> {
    if text.trim().is_empty() || text.len() > MAX_COMMENT {
        return Err(invalid("a comment has 1 to 16384 bytes"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(invalid(
            "a comment is plain text: no control characters but line breaks and tabs",
        ));
    }
    Ok(())
}

/// Tells everyone in the room that the comments of `doc` changed.
fn announce(room: &mut Room, doc: &str, except: &str) {
    let params = json!({ "doc": doc });
    room.announce(CHANGED, &params, &params, Some(except));
}

/// The thread a comment belongs to: its own id for a thread's first comment, else the first's.
fn root_of(room: &Room, id: u64) -> Result<u64, RpcError> {
    let c = room
        .meta
        .comments
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no comment {id}")))?;
    Ok(c.reply_to.unwrap_or(c.id))
}

/// `room.comment`: a new thread at `doc` and `at`, or with `reply_to` a reply in that thread.
pub(super) fn comment(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let text = str_of(&call.params, "text")?.to_owned();
    check_text(&text)?;
    let reply_to = match &call.params["reply_to"] {
        Value::Null => None,
        v => Some(
            v.as_u64()
                .ok_or_else(|| invalid("`reply_to` is a comment id"))?,
        ),
    };
    let place = match reply_to {
        Some(_) => None,
        None => {
            let doc = str_of(&call.params, "doc")?;
            if doc.len() > MAX_DOC {
                return Err(invalid("`doc` is longer than 4096 bytes"));
            }
            let doc = crate::paths::check_relative(doc)?;
            let at = match &call.params["at"] {
                Value::Null => 0,
                v => v
                    .as_u64()
                    .ok_or_else(|| invalid("`at` is a position: a whole number from 0"))?
                    as usize,
            };
            Some((doc, at))
        }
    };
    let now = rooms.now();
    let sid = call.session.id();
    rooms.with(|room| {
        let p = room.need(sid, Role::Commenter)?;
        let (author, name) = (p.person.id.clone(), p.person.name.clone());
        if room.meta.comments.len() >= MAX_COMMENTS {
            return Err(invalid("too many comments"));
        }
        let (doc, at, reply_to) = match (place, reply_to) {
            (Some((doc, at)), _) => (doc, at, None),
            (None, Some(id)) => {
                let root = root_of(room, id)?;
                let r = room
                    .meta
                    .comments
                    .iter()
                    .find(|c| c.id == root)
                    .ok_or_else(|| {
                        RpcError::new(ErrorCode::NotFound, format!("no comment {id}"))
                    })?;
                (r.doc.clone(), r.at, Some(root))
            }
            (None, None) => return Err(invalid("`doc` is missing")),
        };
        room.meta.next_comment += 1;
        let id = room.meta.next_comment;
        room.meta.comments.push(Comment {
            id,
            doc: doc.clone(),
            at,
            author,
            name,
            text,
            time: now,
            resolved: false,
            reply_to,
        });
        room.save_meta();
        announce(room, &doc, sid);
        Ok(json!({ "id": id, "doc": doc, "reply_to": reply_to }))
    })
}

/// `room.comments`: the comments of the room, or of `doc`, as the caller may see them.
pub(super) fn comments(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let doc = call.params["doc"].as_str();
    rooms.with(|room| {
        room.participant(call.session.id())?;
        Ok(room.comments_for(call.session.id(), doc))
    })
}

/// `room.resolve_comment`: resolves the thread of `id`, or reopens it with `resolved: false`.
pub(super) fn resolve_comment(
    rooms: &Handle,
    _b: &Backend,
    call: &Call,
) -> Result<Value, RpcError> {
    let id = call.params["id"]
        .as_u64()
        .ok_or_else(|| invalid("`id` is missing"))?;
    let resolved = match &call.params["resolved"] {
        Value::Null => true,
        v => v
            .as_bool()
            .ok_or_else(|| invalid("`resolved` is true or false"))?,
    };
    let sid = call.session.id();
    rooms.with(|room| {
        room.need(sid, Role::Commenter)?;
        let root = root_of(room, id)?;
        let c = room
            .meta
            .comments
            .iter_mut()
            .find(|c| c.id == root)
            .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no comment {id}")))?;
        c.resolved = resolved;
        let doc = c.doc.clone();
        room.save_meta();
        announce(room, &doc, sid);
        Ok(json!({ "id": root, "resolved": resolved }))
    })
}

/// `room.delete_comment`: deletes one's own comment; a thread's first comment takes its replies
/// along, and is refused while others replied (v8 behaviour 56).
pub(super) fn delete_comment(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let id = call.params["id"]
        .as_u64()
        .ok_or_else(|| invalid("`id` is missing"))?;
    let sid = call.session.id();
    rooms.with(|room| {
        let p = room.need(sid, Role::Commenter)?;
        let me = p.person.id.clone();
        let c = room
            .meta
            .comments
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| RpcError::new(ErrorCode::NotFound, format!("no comment {id}")))?;
        if c.author != me {
            return Err(not_granted("only its author deletes a comment"));
        }
        let (doc, root) = (c.doc.clone(), c.reply_to.is_none());
        if root
            && room
                .meta
                .comments
                .iter()
                .any(|r| r.reply_to == Some(id) && r.author != me)
        {
            return Err(RpcError::new(
                ErrorCode::Conflict,
                "others replied to this comment: resolve the thread instead",
            ));
        }
        let before = room.meta.comments.len();
        room.meta
            .comments
            .retain(|c| c.id != id && !(root && c.reply_to == Some(id)));
        let deleted = before - room.meta.comments.len();
        room.save_meta();
        announce(room, &doc, sid);
        Ok(json!({ "deleted": deleted }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_plain_and_bounded() {
        assert!(check_text("looks good\n\tsecond line").is_ok());
        assert!(check_text("").is_err());
        assert!(check_text("  \n ").is_err());
        assert!(check_text(&"x".repeat(MAX_COMMENT)).is_ok());
        assert!(check_text(&"x".repeat(MAX_COMMENT + 1)).is_err());
        assert!(check_text("bell \u{7}").is_err());
        assert!(check_text("escape \u{1b}[31m").is_err());
        assert!(check_text("return\r\n").is_err());
    }
}

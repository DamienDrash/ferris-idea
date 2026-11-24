//! Protocol tests of the Claude Code link (`SPEC-0358`) against a stand-in client and a stand-in
//! window. Every lock file goes to a temporary folder; nothing touches `~/.claude`.

mod protocol;
mod standin;

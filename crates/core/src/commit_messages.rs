//! The commit messages the user committed with in a project (`SPEC-0063` v10 behaviour 22): the
//! last [`MAX_MESSAGES`], newest first, each once, kept in the workspace's Ferris IDEA settings
//! (`.ferris-idea/commit-messages.toml`), never read from the Git history.
//!
//! A message is whatever the user typed: it is stored and returned as it is and never
//! interpreted, a message or a file over its size limit is left out, and no message is ever part
//! of an error text or a log line.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where the messages of a workspace are kept, relative to its root.
pub const COMMIT_MESSAGES_FILE: &str = ".ferris-idea/commit-messages.toml";

/// Messages kept; the oldest goes when one more is added.
pub const MAX_MESSAGES: usize = 25;

/// The largest message that is kept, in bytes of UTF-8; a larger one is not remembered.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// The largest file that is read, in bytes. A file written by [`CommitMessages::save`] stays
/// below it: [`MAX_MESSAGES`] messages of [`MAX_MESSAGE_BYTES`], each byte escaped as six.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// The format this build writes and reads.
pub const FORMAT_VERSION: u32 = 1;

/// Why a history file was not used. The text never quotes the file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// Not a regular file (a link or a folder), or it cannot be read.
    NotAFile,
    /// Larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// Not the expected TOML.
    Broken,
    /// Written by a newer format.
    Version(u32),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::NotAFile => f.write_str("it is not a regular file or cannot be read"),
            Unreadable::TooLarge => write!(f, "it is larger than {} MiB", MAX_FILE_BYTES >> 20),
            Unreadable::Broken => f.write_str("its content is not a commit message history"),
            Unreadable::Version(v) => write!(
                f,
                "it has format version {v}, this Ferris IDEA reads version {FORMAT_VERSION}"
            ),
        }
    }
}

/// The file's content.
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    /// Newest first.
    #[serde(default)]
    message: Vec<String>,
}

/// The remembered commit messages of one workspace, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitMessages {
    messages: Vec<String>,
}

impl CommitMessages {
    /// Reads the history of the workspace at `root`. A missing file is an empty history; a file
    /// that cannot be used is an empty history and the reason.
    pub fn load(root: &Path) -> (Self, Option<Unreadable>) {
        let file = root.join(COMMIT_MESSAGES_FILE);
        // A link is not followed: the file is Ferris IDEA's own or it is not used.
        let meta = match std::fs::symlink_metadata(&file) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(_) => return (Self::default(), Some(Unreadable::NotAFile)),
        };
        if !meta.is_file() {
            return (Self::default(), Some(Unreadable::NotAFile));
        }
        if meta.len() > MAX_FILE_BYTES {
            return (Self::default(), Some(Unreadable::TooLarge));
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            return (Self::default(), Some(Unreadable::NotAFile));
        };
        match Self::parse(&text) {
            Ok(history) => (history, None),
            Err(why) => (Self::default(), Some(why)),
        }
    }

    /// The history in `text`, the content of a history file.
    fn parse(text: &str) -> Result<Self, Unreadable> {
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(Unreadable::TooLarge);
        }
        let stored: Stored = toml::from_str(text).map_err(|_| Unreadable::Broken)?;
        if stored.version != FORMAT_VERSION {
            return Err(Unreadable::Version(stored.version));
        }
        // What a hand-edited file holds beyond the rules is left out, not trusted.
        let mut messages: Vec<String> = Vec::new();
        for m in stored.message {
            if messages.len() == MAX_MESSAGES {
                break;
            }
            if keepable(&m) && !messages.contains(&m) {
                messages.push(m);
            }
        }
        Ok(Self { messages })
    }

    /// The messages, newest first.
    pub fn newest_first(&self) -> &[String] {
        &self.messages
    }

    /// Whether no message is remembered.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Remembers `message` as the newest. An equal message kept before moves to the front
    /// instead of being listed twice; beyond [`MAX_MESSAGES`] the oldest goes. A blank message
    /// and one over [`MAX_MESSAGE_BYTES`] are not remembered: `false`.
    pub fn remember(&mut self, message: &str) -> bool {
        if !keepable(message) {
            return false;
        }
        self.messages.retain(|m| m != message);
        self.messages.insert(0, message.to_owned());
        self.messages.truncate(MAX_MESSAGES);
        true
    }

    /// Writes the history to the workspace at `root`, replacing the file as a whole.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let stored = Stored {
            version: FORMAT_VERSION,
            message: self.messages.clone(),
        };
        let mut text =
            String::from("# Commit messages used in this workspace, newest first (Ferris IDEA)\n");
        text.push_str(&toml::to_string(&stored).map_err(std::io::Error::other)?);
        let file = root.join(COMMIT_MESSAGES_FILE);
        // Something else under the name (a link, a folder) is not written through.
        if std::fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) {
            return Err(std::io::Error::other(format!(
                "{COMMIT_MESSAGES_FILE} is not a regular file"
            )));
        }
        crate::write_atomic(&file, text)
    }
}

/// Whether a message is kept at all.
fn keepable(message: &str) -> bool {
    !message.trim().is_empty() && message.len() <= MAX_MESSAGE_BYTES
}

/// Loads the history of the workspace at `root`, remembers `message` and saves; `Ok(false)` when
/// the message is not one to remember (nothing is written then). A history file that cannot be
/// used is replaced by one holding this message.
pub fn remember_commit_message(root: &Path, message: &str) -> std::io::Result<bool> {
    if !keepable(message) {
        return Ok(false);
    }
    let (mut history, _) = CommitMessages::load(root);
    history.remember(message);
    history.save(root)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferris-idea-commit-messages-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// SPEC-0063 AK-12 (v10): newest first, an equal message moves to the front, blank ones are
    /// not kept.
    #[test]
    fn newest_first_and_each_message_once() {
        let mut h = CommitMessages::default();
        assert!(h.is_empty());
        assert!(h.remember("first"));
        assert!(h.remember("second\n\nwith a body"));
        assert!(h.remember("third"));
        assert_eq!(
            h.newest_first(),
            ["third", "second\n\nwith a body", "first"]
        );
        // Used again: once in the list, now the newest.
        assert!(h.remember("first"));
        assert_eq!(
            h.newest_first(),
            ["first", "third", "second\n\nwith a body"]
        );
        // Equal means equal to the character.
        assert!(h.remember("First"));
        assert!(h.remember("first "));
        assert_eq!(h.newest_first().len(), 5);
        for blank in ["", " ", "\n\t \n"] {
            assert!(!h.remember(blank), "{blank:?}");
        }
        assert_eq!(h.newest_first().len(), 5);
    }

    /// SPEC-0063 AK-12 (v10): 25 messages, the oldest goes; a message over the size limit is
    /// not kept.
    #[test]
    fn the_number_and_the_size_are_limited() {
        let mut h = CommitMessages::default();
        for i in 0..MAX_MESSAGES + 5 {
            assert!(h.remember(&format!("message {i}")));
        }
        assert_eq!(h.newest_first().len(), MAX_MESSAGES);
        assert_eq!(h.newest_first()[0], "message 29");
        assert_eq!(h.newest_first()[MAX_MESSAGES - 1], "message 5");
        let largest = "x".repeat(MAX_MESSAGE_BYTES);
        assert!(h.remember(&largest));
        assert!(!h.remember(&format!("{largest}y")));
        // The limit counts bytes, not characters.
        assert!(!h.remember(&"ä".repeat(MAX_MESSAGE_BYTES / 2 + 1)));
        assert_eq!(h.newest_first()[0], largest);
        assert_eq!(h.newest_first().len(), MAX_MESSAGES);
    }

    /// SPEC-0063 AK-12 (v10): what was typed comes back unchanged, whatever it is.
    #[test]
    fn messages_are_data_and_survive_the_file() {
        let dir = scratch("data");
        let odd = [
            "plain",
            "two\nlines",
            "quotes \" ' and \\ backslash",
            "\"\"\"\nversion = 2\nmessage = [\"injected\"]\n\"\"\"",
            "control \u{1b}[31m red \u{7} bell \u{0} nul",
            "  leading and trailing  \n",
            "ünïcödé \u{202e} reversed 🙂",
            "$(rm -rf ~) `id` ${HOME} %s {0}",
        ];
        let mut h = CommitMessages::default();
        for m in odd {
            assert!(h.remember(m));
        }
        h.save(&dir).unwrap();
        let (read, why) = CommitMessages::load(&dir);
        assert_eq!(why, None);
        assert_eq!(read, h);
        let newest_first: Vec<&str> = odd.iter().rev().copied().collect();
        assert_eq!(read.newest_first(), newest_first);
        // A history of the largest messages of the worst kind stays a readable file.
        let mut worst = CommitMessages::default();
        for i in 0..MAX_MESSAGES {
            let mut m = "\u{1}".repeat(MAX_MESSAGE_BYTES - 2);
            m.push_str(&format!("{i:02}"));
            assert!(worst.remember(&m));
        }
        worst.save(&dir).unwrap();
        let size = std::fs::metadata(dir.join(COMMIT_MESSAGES_FILE))
            .unwrap()
            .len();
        assert!(size <= MAX_FILE_BYTES, "{size}");
        assert_eq!(CommitMessages::load(&dir), (worst, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0063 AK-12 (v10): a file that cannot be used is an empty history and a reason that
    /// does not quote it; the next commit writes a good file.
    #[test]
    fn an_unusable_file_is_an_empty_history_with_the_reason() {
        let dir = scratch("corrupt");
        assert_eq!(
            CommitMessages::load(&dir),
            (CommitMessages::default(), None),
            "no file is no problem"
        );
        let file = dir.join(COMMIT_MESSAGES_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let secret = "s3cr3t-token";
        for (content, why) in [
            (
                format!("version = 1\nmessage = [\"{secret}\"\n"),
                Unreadable::Broken,
            ),
            (
                format!("version = 1\nmessage = \"{secret}\"\n"),
                Unreadable::Broken,
            ),
            (format!("message = [\"{secret}\"]\n"), Unreadable::Broken),
            (
                format!("version = 7\nmessage = [\"{secret}\"]\n"),
                Unreadable::Version(7),
            ),
            ("\u{0}\u{1}not toml".to_owned(), Unreadable::Broken),
        ] {
            std::fs::write(&file, &content).unwrap();
            let (read, said) = CommitMessages::load(&dir);
            assert!(read.is_empty(), "{content:?}");
            assert_eq!(said, Some(why), "{content:?}");
            assert!(!why.to_string().contains(secret));
        }
        // Too large: not read at all.
        std::fs::write(&file, vec![b'#'; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert_eq!(
            CommitMessages::load(&dir),
            (CommitMessages::default(), Some(Unreadable::TooLarge))
        );
        // Entries beyond the rules are left out of a file that is otherwise good.
        let mut text = String::from("version = 1\nmessage = [\"kept\", \"\", \"kept\", ");
        text.push_str(&format!("\"{}\", ", "x".repeat(MAX_MESSAGE_BYTES + 1)));
        for i in 0..40 {
            text.push_str(&format!("\"m{i}\", "));
        }
        text.push_str("]\n");
        std::fs::write(&file, text).unwrap();
        let (read, said) = CommitMessages::load(&dir);
        assert_eq!(said, None);
        assert_eq!(read.newest_first().len(), MAX_MESSAGES);
        assert_eq!(&read.newest_first()[..2], ["kept", "m0"]);
        // The next commit replaces a broken file.
        std::fs::write(&file, "broken [").unwrap();
        assert!(remember_commit_message(&dir, "after the break").unwrap());
        assert!(!remember_commit_message(&dir, "  ").unwrap());
        let (read, said) = CommitMessages::load(&dir);
        assert_eq!(said, None);
        assert_eq!(read.newest_first(), ["after the break"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0063 AK-12 (v10): a link under the file's name is neither read nor written through.
    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed() {
        let dir = scratch("link");
        let elsewhere = dir.join("elsewhere.toml");
        std::fs::write(&elsewhere, "version = 1\nmessage = [\"from elsewhere\"]\n").unwrap();
        let file = dir.join(COMMIT_MESSAGES_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        assert_eq!(
            CommitMessages::load(&dir),
            (CommitMessages::default(), Some(Unreadable::NotAFile))
        );
        assert!(remember_commit_message(&dir, "typed").is_err());
        assert_eq!(
            std::fs::read_to_string(&elsewhere).unwrap(),
            "version = 1\nmessage = [\"from elsewhere\"]\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

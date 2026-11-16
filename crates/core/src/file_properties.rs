//! File properties (`SPEC-0336`): the encoding a file's bytes are read and written in, its line
//! separator and its read-only state — what they are, how one is converted into another and how
//! a file of the project is rewritten for it. Toolkit-free, so the window and the MCP server
//! share it. Nothing here writes outside the project folder (behaviour 2), every write replaces
//! the file through a temporary file and a rename (behaviour 5), and no conversion loses a
//! character unless the caller says so explicitly (behaviour 14).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Largest file whose separators or encoding are changed (behaviour 3).
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// A line separator (behaviour 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineSeparator {
    /// `\n`
    Lf,
    /// `\r\n`
    CrLf,
    /// `\r`
    Cr,
}

impl LineSeparator {
    /// The three in the order of the menu (behaviour 19).
    pub const ALL: [LineSeparator; 3] = [LineSeparator::CrLf, LineSeparator::Lf, LineSeparator::Cr];

    /// The characters.
    pub fn as_str(self) -> &'static str {
        match self {
            LineSeparator::Lf => "\n",
            LineSeparator::CrLf => "\r\n",
            LineSeparator::Cr => "\r",
        }
    }

    /// `LF`, `CRLF` or `CR`, as the status bar shows it.
    pub fn name(self) -> &'static str {
        match self {
            LineSeparator::Lf => "LF",
            LineSeparator::CrLf => "CRLF",
            LineSeparator::Cr => "CR",
        }
    }

    /// The title of the action that converts to it (behaviour 7).
    pub fn title(self) -> &'static str {
        match self {
            LineSeparator::Lf => "LF - Unix and macOS (\\n)",
            LineSeparator::CrLf => "CRLF - Windows (\\r\\n)",
            LineSeparator::Cr => "CR - Classic Mac OS (\\r)",
        }
    }

    /// The id of the action that converts to it (behaviour 7).
    pub fn action_id(self) -> &'static str {
        match self {
            LineSeparator::Lf => "ConvertToUnixLineSeparators",
            LineSeparator::CrLf => "ConvertToWindowsLineSeparators",
            LineSeparator::Cr => "ConvertToMacLineSeparators",
        }
    }

    /// The separator an action id converts to.
    pub fn of_action(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.action_id() == id)
    }

    /// `lf`, `crlf` or `cr`, in any case.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// How many line breaks of each kind a text has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeparatorCounts {
    /// `\n` not after `\r`.
    pub lf: usize,
    /// `\r\n`.
    pub crlf: usize,
    /// `\r` not before `\n`.
    pub cr: usize,
}

impl SeparatorCounts {
    /// Counts the line breaks of `text`.
    pub fn of(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut counts = Self::default();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                    counts.crlf += 1;
                    i += 1;
                }
                b'\r' => counts.cr += 1,
                b'\n' => counts.lf += 1,
                _ => {}
            }
            i += 1;
        }
        counts
    }

    /// The count of one kind.
    pub fn count(&self, separator: LineSeparator) -> usize {
        match separator {
            LineSeparator::Lf => self.lf,
            LineSeparator::CrLf => self.crlf,
            LineSeparator::Cr => self.cr,
        }
    }

    /// All line breaks.
    pub fn total(&self) -> usize {
        self.lf + self.crlf + self.cr
    }

    /// The line breaks that are not `separator`: what a conversion to it changes.
    pub fn other_than(&self, separator: LineSeparator) -> usize {
        self.total() - self.count(separator)
    }

    /// The separator most line breaks use; with equal counts LF before CRLF before CR; `None`
    /// without a line break (behaviour 6).
    pub fn dominant(&self) -> Option<LineSeparator> {
        if self.total() == 0 {
            None
        } else if self.cr > self.lf && self.cr > self.crlf {
            Some(LineSeparator::Cr)
        } else if self.crlf > self.lf {
            Some(LineSeparator::CrLf)
        } else {
            Some(LineSeparator::Lf)
        }
    }
}

/// `text` with every line break replaced by `target`, and how many line breaks changed
/// (behaviour 7). Nothing else changes; a last line without a line break stays without one.
pub fn convert_separators(text: &str, target: LineSeparator) -> (String, usize) {
    let changed = SeparatorCounts::of(text).other_than(target);
    if changed == 0 {
        return (text.to_owned(), 0);
    }
    let mut out = String::with_capacity(text.len() + changed);
    let mut rest = text;
    while let Some(at) = rest.find(['\r', '\n']) {
        out.push_str(&rest[..at]);
        out.push_str(target.as_str());
        let len = if rest[at..].starts_with("\r\n") { 2 } else { 1 };
        rest = &rest[at + len..];
    }
    out.push_str(rest);
    (out, changed)
}

/// An encoding IDECraft reads and writes (behaviour 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Charset {
    /// UTF-8.
    Utf8,
    /// UTF-16, little endian, with its byte order mark.
    Utf16Le,
    /// UTF-16, big endian, with its byte order mark.
    Utf16Be,
    /// ISO-8859-1 (Latin-1): the first 256 code points.
    Iso8859_1,
    /// windows-1252 as the WHATWG Encoding Standard defines it.
    Windows1252,
    /// US-ASCII: the first 128 code points.
    UsAscii,
}

/// The characters of the bytes 0x80 to 0x9F in windows-1252; the five unassigned bytes are the C1
/// controls of the same value (WHATWG Encoding Standard).
const WINDOWS_1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
];

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const UTF16LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16BE_BOM: &[u8] = &[0xFE, 0xFF];

impl Charset {
    /// Every encoding, in the order they are listed (behaviour 12).
    pub const ALL: [Charset; 6] = [
        Charset::Utf8,
        Charset::Utf16Le,
        Charset::Utf16Be,
        Charset::Iso8859_1,
        Charset::Windows1252,
        Charset::UsAscii,
    ];

    /// The name shown and accepted.
    pub fn name(self) -> &'static str {
        match self {
            Charset::Utf8 => "UTF-8",
            Charset::Utf16Le => "UTF-16LE",
            Charset::Utf16Be => "UTF-16BE",
            Charset::Iso8859_1 => "ISO-8859-1",
            Charset::Windows1252 => "windows-1252",
            Charset::UsAscii => "US-ASCII",
        }
    }

    /// The encoding of a name, in any case; a few common other spellings are understood.
    pub fn from_name(name: &str) -> Option<Self> {
        let wanted: String = name
            .trim()
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Some(match wanted.as_str() {
            "utf8" => Charset::Utf8,
            "utf16le" => Charset::Utf16Le,
            "utf16be" => Charset::Utf16Be,
            "iso88591" | "latin1" => Charset::Iso8859_1,
            "windows1252" | "cp1252" => Charset::Windows1252,
            "usascii" | "ascii" => Charset::UsAscii,
            _ => return None,
        })
    }

    /// The byte order mark the encoding may have (behaviour 15).
    pub fn bom(self) -> Option<&'static [u8]> {
        match self {
            Charset::Utf8 => Some(UTF8_BOM),
            Charset::Utf16Le => Some(UTF16LE_BOM),
            Charset::Utf16Be => Some(UTF16BE_BOM),
            _ => None,
        }
    }

    /// Whether the encoding is always written with its byte order mark.
    pub fn bom_mandatory(self) -> bool {
        matches!(self, Charset::Utf16Le | Charset::Utf16Be)
    }

    /// The byte of `c` in a one-byte encoding; `None` when it has none, and for the others.
    fn byte_of(self, c: char) -> Option<u8> {
        let code = c as u32;
        match self {
            Charset::UsAscii => (code < 0x80).then_some(code as u8),
            Charset::Iso8859_1 => (code < 0x100).then_some(code as u8),
            Charset::Windows1252 => {
                if code < 0x80 || (0xA0..0x100).contains(&code) {
                    Some(code as u8)
                } else {
                    WINDOWS_1252_HIGH
                        .iter()
                        .position(|h| *h == c)
                        .map(|i| 0x80 + i as u8)
                }
            }
            _ => None,
        }
    }

    /// Whether `c` can be written in the encoding.
    pub fn can_hold(self, c: char) -> bool {
        match self {
            Charset::Utf8 | Charset::Utf16Le | Charset::Utf16Be => true,
            _ => self.byte_of(c).is_some(),
        }
    }
}

impl std::fmt::Display for Charset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// An encoding and whether the file starts with its byte order mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileEncoding {
    /// The encoding.
    pub charset: Charset,
    /// Whether the bytes start with the byte order mark.
    pub bom: bool,
}

impl Default for FileEncoding {
    fn default() -> Self {
        Self::UTF8
    }
}

impl FileEncoding {
    /// UTF-8 without a byte order mark.
    pub const UTF8: FileEncoding = FileEncoding {
        charset: Charset::Utf8,
        bom: false,
    };

    /// `charset` with a byte order mark where it requires one, else without.
    pub fn of(charset: Charset) -> Self {
        Self {
            charset,
            bom: charset.bom_mandatory(),
        }
    }

    /// `charset` with the byte order mark as asked, where the encoding leaves a choice
    /// (behaviour 15): UTF-16 always has it, the one-byte encodings never.
    pub fn with_bom(charset: Charset, bom: bool) -> Self {
        Self {
            charset,
            bom: charset.bom_mandatory() || (bom && charset.bom().is_some()),
        }
    }

    /// What the status bar shows: the name, with ` BOM` for UTF-8 with a mark.
    pub fn label(self) -> String {
        if self.bom && !self.charset.bom_mandatory() {
            format!("{} BOM", self.charset.name())
        } else {
            self.charset.name().to_owned()
        }
    }
}

/// Characters an encoding cannot hold (behaviour 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unrepresentable {
    /// How many characters of the text.
    pub count: usize,
    /// The first of them.
    pub first: char,
}

impl Unrepresentable {
    /// "3 characters cannot be written in US-ASCII (the first is 'é', U+00E9)".
    pub fn message(&self, charset: Charset) -> String {
        format!(
            "{} {} cannot be written in {charset} (the first is '{}', U+{:04X})",
            self.count,
            if self.count == 1 {
                "character"
            } else {
                "characters"
            },
            self.first,
            self.first as u32
        )
    }
}

/// The characters of `text` that `charset` cannot hold.
pub fn unrepresentable(text: &str, charset: Charset) -> Option<Unrepresentable> {
    let mut found: Option<Unrepresentable> = None;
    for c in text.chars().filter(|c| !charset.can_hold(*c)) {
        match &mut found {
            Some(u) => u.count += 1,
            None => found = Some(Unrepresentable { count: 1, first: c }),
        }
    }
    found
}

/// `text` as the bytes of `encoding`, with its byte order mark when it has one. Fails when a
/// character cannot be written; nothing is replaced (behaviour 14).
pub fn encode(text: &str, encoding: FileEncoding) -> Result<Vec<u8>, Unrepresentable> {
    if let Some(u) = unrepresentable(text, encoding.charset) {
        return Err(u);
    }
    let mut out = Vec::with_capacity(text.len() + 3);
    if encoding.bom
        && let Some(bom) = encoding.charset.bom()
    {
        out.extend_from_slice(bom);
    }
    match encoding.charset {
        Charset::Utf8 => out.extend_from_slice(text.as_bytes()),
        Charset::Utf16Le => text
            .encode_utf16()
            .for_each(|u| out.extend_from_slice(&u.to_le_bytes())),
        Charset::Utf16Be => text
            .encode_utf16()
            .for_each(|u| out.extend_from_slice(&u.to_be_bytes())),
        one_byte => out.extend(text.chars().filter_map(|c| one_byte.byte_of(c))),
    }
    Ok(out)
}

/// The explicit lossy form (behaviour 14): every character `encoding` cannot hold becomes `?`.
/// Returns the bytes, the text as written and how many characters were replaced.
pub fn encode_lossy(text: &str, encoding: FileEncoding) -> (Vec<u8>, String, usize) {
    let mut replaced = 0;
    let written: String = text
        .chars()
        .map(|c| {
            if encoding.charset.can_hold(c) {
                c
            } else {
                replaced += 1;
                '?'
            }
        })
        .collect();
    let bytes = encode(&written, encoding).unwrap_or_default();
    (bytes, written, replaced)
}

/// The text of `bytes` (without a byte order mark) in `charset`; `None` when a byte sequence is
/// not valid in it. Nothing is replaced.
fn decode_body(bytes: &[u8], charset: Charset) -> Option<String> {
    let utf16 = |unit: fn([u8; 2]) -> u16| {
        let (pairs, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            return None;
        }
        char::decode_utf16(pairs.iter().map(|pair| unit(*pair)))
            .collect::<Result<String, _>>()
            .ok()
    };
    match charset {
        Charset::Utf8 => std::str::from_utf8(bytes).ok().map(str::to_owned),
        Charset::Utf16Le => utf16(u16::from_le_bytes),
        Charset::Utf16Be => utf16(u16::from_be_bytes),
        Charset::UsAscii => bytes
            .is_ascii()
            .then(|| bytes.iter().map(|b| *b as char).collect()),
        Charset::Iso8859_1 => Some(bytes.iter().map(|b| *b as char).collect()),
        Charset::Windows1252 => Some(
            bytes
                .iter()
                .map(|b| match *b {
                    0x80..=0x9F => WINDOWS_1252_HIGH[(*b - 0x80) as usize],
                    b => b as char,
                })
                .collect(),
        ),
    }
}

/// The text of `bytes` in `encoding`: the byte order mark, when the encoding has one, must be
/// there and is not part of the text. `None` when the bytes are not valid; nothing is replaced.
pub fn decode(bytes: &[u8], encoding: FileEncoding) -> Option<String> {
    let body = if encoding.bom {
        bytes.strip_prefix(encoding.charset.bom()?)?
    } else {
        bytes
    };
    decode_body(body, encoding.charset)
}

/// The encoding whose byte order mark `bytes` start with.
pub fn bom_of(bytes: &[u8]) -> Option<Charset> {
    [Charset::Utf8, Charset::Utf16Le, Charset::Utf16Be]
        .into_iter()
        .find(|c| c.bom().is_some_and(|bom| bytes.starts_with(bom)))
}

/// Why a file's property is not read or changed (behaviours 2 to 4, 12 to 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The resolved path is not below the resolved project folder.
    OutsideProject,
    /// The file does not exist or cannot be looked at.
    Missing(String),
    /// A folder.
    Directory,
    /// Larger than [`MAX_BYTES`].
    TooLarge,
    /// The bytes are not text.
    Binary,
    /// The file is read-only.
    ReadOnly,
    /// Characters the encoding cannot hold, without the explicit confirmation.
    Lossy(Unrepresentable, Charset),
    /// The bytes cannot be read in the encoding; the text says why.
    CannotReload(String),
    /// The byte order mark cannot be added or removed; the text says why.
    Bom(String),
    /// Reading or writing failed.
    Io(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::OutsideProject => f.write_str("it is outside the project"),
            Refusal::Missing(why) => write!(f, "it cannot be read ({why})"),
            Refusal::Directory => f.write_str("it is a folder"),
            Refusal::TooLarge => write!(f, "it is larger than {} MiB", MAX_BYTES >> 20),
            Refusal::Binary => f.write_str("it is not a text file"),
            Refusal::ReadOnly => f.write_str("it is read-only"),
            Refusal::Lossy(u, charset) => f.write_str(&u.message(*charset)),
            Refusal::CannotReload(why) | Refusal::Bom(why) | Refusal::Io(why) => f.write_str(why),
        }
    }
}

/// What the bytes of a file are read as (behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// The encoding.
    pub encoding: FileEncoding,
    /// The text, without the byte order mark.
    pub text: String,
    /// The encoding is the fallback for bytes that are not UTF-8.
    pub fallback: bool,
}

/// Reads `bytes` as text (behaviour 11): a byte order mark decides; else `chosen` when the bytes
/// are valid in it; else UTF-8; else windows-1252. Bytes with a NUL and no UTF-16 mark, a text
/// with a NUL and bytes that contradict their mark are not text.
pub fn detect(bytes: &[u8], chosen: Option<Charset>) -> Result<Detected, Refusal> {
    let found = |encoding, text: String, fallback| {
        if text.contains('\0') {
            Err(Refusal::Binary)
        } else {
            Ok(Detected {
                encoding,
                text,
                fallback,
            })
        }
    };
    if let Some(charset) = bom_of(bytes) {
        let encoding = FileEncoding { charset, bom: true };
        return match decode(bytes, encoding) {
            Some(text) => found(encoding, text, false),
            None => Err(Refusal::Binary),
        };
    }
    if bytes.contains(&0) {
        return Err(Refusal::Binary);
    }
    if let Some(charset) = chosen.filter(|c| !c.bom_mandatory())
        && let Some(text) = decode_body(bytes, charset)
    {
        return found(
            FileEncoding {
                charset,
                bom: false,
            },
            text,
            false,
        );
    }
    match decode_body(bytes, Charset::Utf8) {
        Some(text) => found(FileEncoding::UTF8, text, false),
        None => found(
            FileEncoding::of(Charset::Windows1252),
            decode_body(bytes, Charset::Windows1252).unwrap_or_default(),
            true,
        ),
    }
}

/// How much an operation on the encoding changes (behaviour 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    /// Nothing: the text (reload) or the bytes (convert) come out the same.
    Same,
    /// Something, and the way back exists.
    Reversible,
    /// Characters are lost.
    Lossy,
}

impl Safety {
    /// `same`, `reversible` or `lossy`.
    pub fn name(self) -> &'static str {
        match self {
            Safety::Same => "same",
            Safety::Reversible => "reversible",
            Safety::Lossy => "lossy",
        }
    }
}

/// What reading a file again in another encoding gives (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reload {
    /// [`Safety::Same`] when the text does not change, else [`Safety::Reversible`].
    pub safety: Safety,
    /// The encoding the bytes are then read in.
    pub encoding: FileEncoding,
    /// The text they give.
    pub text: String,
}

/// Judges reading `bytes` in `to` instead, against the text `now` shown (behaviour 12). Refused
/// when the bytes are not valid in `to`, carry a byte order mark `to` cannot have, or lack the
/// one it needs: IDECraft never reads a file into replacement characters (IMP-0404).
pub fn reload_safety(bytes: &[u8], now: &str, to: Charset) -> Result<Reload, Refusal> {
    let cannot = |why: String| Err(Refusal::CannotReload(why));
    let encoding = match bom_of(bytes) {
        Some(marked) if marked != to => {
            return cannot(format!(
                "its bytes start with the byte order mark of {marked}"
            ));
        }
        Some(_) => FileEncoding {
            charset: to,
            bom: true,
        },
        None if to.bom_mandatory() => {
            return cannot(format!(
                "{to} needs its byte order mark, which the file does not have"
            ));
        }
        None => FileEncoding {
            charset: to,
            bom: false,
        },
    };
    let Some(text) = decode(bytes, encoding).filter(|t| !t.contains('\0')) else {
        return cannot(format!("its bytes are not valid {to}"));
    };
    Ok(Reload {
        safety: if text == now {
            Safety::Same
        } else {
            Safety::Reversible
        },
        encoding,
        text,
    })
}

/// Judges writing `text` in `to` over a file that holds `on_disk` (behaviour 12), with the
/// characters `to` cannot hold when it is lossy.
pub fn convert_safety(
    text: &str,
    on_disk: &[u8],
    to: FileEncoding,
) -> (Safety, Option<Unrepresentable>) {
    match encode(text, to) {
        Err(u) => (Safety::Lossy, Some(u)),
        Ok(bytes) if bytes == on_disk => (Safety::Same, None),
        Ok(_) => (Safety::Reversible, None),
    }
}

/// The encodings chosen for files in this run of IDECraft (behaviour 11), by resolved path.
fn choices() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Charset>> {
    static CHOICES: OnceLock<Mutex<HashMap<PathBuf, Charset>>> = OnceLock::new();
    CHOICES
        .get_or_init(Default::default)
        .lock()
        // A panic while holding the lock leaves plain data behind; go on with it.
        .unwrap_or_else(|e| e.into_inner())
}

static CHOICE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The encoding chosen for `path` in this run, if any.
pub fn chosen(path: &Path) -> Option<Charset> {
    choices().get(&key(path)).copied()
}

/// Remembers `charset` as the encoding of `path` until IDECraft quits (behaviours 13, 14).
pub fn choose(path: &Path, charset: Charset) {
    if choices().insert(key(path), charset) != Some(charset) {
        CHOICE_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
}

/// Changes whenever a choice changes, so an editor knows when to read its file again.
pub fn choice_generation() -> u64 {
    CHOICE_GENERATION.load(Ordering::SeqCst)
}

/// A file read as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    /// The text, without the byte order mark.
    pub text: String,
    /// The encoding it was read in.
    pub encoding: FileEncoding,
    /// The bytes read.
    pub bytes: Vec<u8>,
}

/// Reads the file at `path` as text in its encoding (behaviour 11); `limit` refuses a larger
/// file. A file read in the fallback encoding keeps it as its choice.
pub fn read_text(path: &Path, limit: Option<u64>) -> Result<TextFile, Refusal> {
    let meta = std::fs::metadata(path).map_err(|e| Refusal::Missing(e.to_string()))?;
    if meta.is_dir() {
        return Err(Refusal::Directory);
    }
    if limit.is_some_and(|l| meta.len() > l) {
        return Err(Refusal::TooLarge);
    }
    let bytes = std::fs::read(path).map_err(|e| Refusal::Missing(e.to_string()))?;
    let detected = detect(&bytes, chosen(path))?;
    if detected.fallback {
        choose(path, detected.encoding.charset);
    }
    Ok(TextFile {
        text: detected.text,
        encoding: detected.encoding,
        bytes,
    })
}

/// The file `path` (absolute, or relative to `root`) resolved through links, when it lies below
/// the resolved project folder `root` (behaviour 2).
pub fn resolve(root: &Path, path: &Path) -> Result<PathBuf, Refusal> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let root = root
        .canonicalize()
        .map_err(|e| Refusal::Missing(e.to_string()))?;
    let resolved = joined
        .canonicalize()
        .map_err(|e| Refusal::Missing(e.to_string()))?;
    if resolved.starts_with(&root) && resolved != root {
        Ok(resolved)
    } else {
        Err(Refusal::OutsideProject)
    }
}

/// Whether the file is read-only (behaviour 17): on Unix its owner has no write permission, on
/// Windows it has the read-only attribute.
pub fn is_read_only(path: &Path) -> std::io::Result<bool> {
    let permissions = std::fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(permissions.mode() & 0o200 == 0)
    }
    #[cfg(not(unix))]
    {
        Ok(permissions.readonly())
    }
}

/// Replaces the file at `path` by `bytes` (behaviour 5): a new file in the same folder gets the
/// bytes and the permission bits of the old one and is renamed over it. On failure the old file
/// stays and the new one is removed. `path` is a resolved path, so no link is replaced.
pub fn write_atomic_keeping_mode(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let permissions = std::fs::metadata(path)?.permissions();
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(
        ".idecraft-tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let tmp = path.with_file_name(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Private until it has the old file's bits.
        options.mode(0o600);
    }
    let written = options.open(&tmp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// A project file read for a change: resolved, a text file within the limit.
struct Loaded {
    path: PathBuf,
    file: TextFile,
    read_only: bool,
}

fn load(root: &Path, path: &Path) -> Result<Loaded, Refusal> {
    let path = resolve(root, path)?;
    let file = read_text(&path, Some(MAX_BYTES))?;
    let read_only = is_read_only(&path).map_err(|e| Refusal::Io(e.to_string()))?;
    Ok(Loaded {
        path,
        file,
        read_only,
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    write_atomic_keeping_mode(path, bytes).map_err(|e| Refusal::Io(e.to_string()))
}

/// The properties of a project file (behaviour 23, `get`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    /// The resolved path.
    pub path: PathBuf,
    /// Its encoding.
    pub encoding: FileEncoding,
    /// Its line breaks by kind.
    pub separators: SeparatorCounts,
    /// Whether it is read-only.
    pub read_only: bool,
    /// Its size in bytes.
    pub size: u64,
}

/// Reads the properties of the project file `path`.
pub fn properties(root: &Path, path: &Path) -> Result<Properties, Refusal> {
    let loaded = load(root, path)?;
    Ok(Properties {
        separators: SeparatorCounts::of(&loaded.file.text),
        encoding: loaded.file.encoding,
        read_only: loaded.read_only,
        size: loaded.file.bytes.len() as u64,
        path: loaded.path,
    })
}

/// What converting a file's line separators did or would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeparatorChange {
    /// The resolved path.
    pub path: PathBuf,
    /// The line breaks before.
    pub before: SeparatorCounts,
    /// How many line breaks change.
    pub changed: usize,
    /// Whether the file was written.
    pub written: bool,
}

/// Converts every line break of the project file `path` to `target` on disk (behaviours 7, 8);
/// with `dry_run` only reports. The file keeps its encoding and byte order mark.
pub fn set_line_separator(
    root: &Path,
    path: &Path,
    target: LineSeparator,
    dry_run: bool,
) -> Result<SeparatorChange, Refusal> {
    let loaded = load(root, path)?;
    if loaded.read_only {
        return Err(Refusal::ReadOnly);
    }
    let before = SeparatorCounts::of(&loaded.file.text);
    let (text, changed) = convert_separators(&loaded.file.text, target);
    let written = changed > 0 && !dry_run;
    if written {
        let bytes = encode(&text, loaded.file.encoding)
            .map_err(|u| Refusal::Lossy(u, loaded.file.encoding.charset))?;
        write(&loaded.path, &bytes)?;
    }
    Ok(SeparatorChange {
        path: loaded.path,
        before,
        changed,
        written,
    })
}

/// What converting a file's encoding did or would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodingChange {
    /// The resolved path.
    pub path: PathBuf,
    /// The encoding before.
    pub from: FileEncoding,
    /// The encoding after.
    pub to: FileEncoding,
    /// How much the conversion changes.
    pub safety: Safety,
    /// The characters `to` cannot hold, when it is lossy.
    pub unrepresentable: Option<Unrepresentable>,
    /// Whether the file was written.
    pub written: bool,
}

/// Writes the text of the project file `path` in the encoding `to` (behaviour 14); with `dry_run`
/// only reports. A conversion that loses characters is refused unless `lossy` confirms it; then
/// each such character is written as `?`.
pub fn convert_encoding(
    root: &Path,
    path: &Path,
    to: FileEncoding,
    lossy: bool,
    dry_run: bool,
) -> Result<EncodingChange, Refusal> {
    let to = FileEncoding::with_bom(to.charset, to.bom);
    let loaded = load(root, path)?;
    if loaded.read_only {
        return Err(Refusal::ReadOnly);
    }
    let (safety, unrepresentable) = convert_safety(&loaded.file.text, &loaded.file.bytes, to);
    let mut written = false;
    if !dry_run {
        match (safety, unrepresentable) {
            (Safety::Lossy, Some(u)) if !lossy => return Err(Refusal::Lossy(u, to.charset)),
            (Safety::Same, _) => {}
            _ => {
                let (bytes, _, _) = encode_lossy(&loaded.file.text, to);
                write(&loaded.path, &bytes)?;
                written = true;
            }
        }
        choose(&loaded.path, to.charset);
    }
    Ok(EncodingChange {
        path: loaded.path,
        from: loaded.file.encoding,
        to,
        safety,
        unrepresentable,
        written,
    })
}

/// Adds (`on`) or removes the byte order mark of the project file `path` (behaviour 15).
pub fn set_bom(
    root: &Path,
    path: &Path,
    on: bool,
    dry_run: bool,
) -> Result<EncodingChange, Refusal> {
    let from = load(root, path)?.file.encoding;
    bom_change(from, on)?;
    convert_encoding(
        root,
        path,
        FileEncoding {
            charset: from.charset,
            bom: on,
        },
        false,
        dry_run,
    )
}

/// Whether the byte order mark of a file in `encoding` can be added (`on`) or removed
/// (behaviour 15); the refusal says why not.
pub fn bom_change(encoding: FileEncoding, on: bool) -> Result<(), Refusal> {
    let charset = encoding.charset;
    let why = if charset.bom().is_none() {
        format!("{charset} has no byte order mark")
    } else if charset.bom_mandatory() {
        format!("{charset} always has its byte order mark")
    } else if encoding.bom == on {
        if on {
            "the file already has a byte order mark".to_owned()
        } else {
            "the file has no byte order mark".to_owned()
        }
    } else {
        return Ok(());
    };
    Err(Refusal::Bom(why))
}

/// What reading a file in another encoding did or would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadChange {
    /// The resolved path.
    pub path: PathBuf,
    /// The encoding before.
    pub from: FileEncoding,
    /// The reload.
    pub reload: Reload,
    /// Whether the choice was remembered.
    pub chosen: bool,
}

/// Reads the project file `path` in `to` from now on (behaviour 13): remembers the choice unless
/// `dry_run`. Nothing is written. Refused when the bytes cannot be read in `to`.
pub fn reload_encoding(
    root: &Path,
    path: &Path,
    to: Charset,
    dry_run: bool,
) -> Result<ReloadChange, Refusal> {
    let loaded = load(root, path)?;
    let reload = reload_safety(&loaded.file.bytes, &loaded.file.text, to)?;
    if !dry_run {
        choose(&loaded.path, to);
    }
    Ok(ReloadChange {
        path: loaded.path,
        from: loaded.file.encoding,
        reload,
        chosen: !dry_run,
    })
}

/// What changing a file's read-only state did or would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnlyChange {
    /// The resolved path.
    pub path: PathBuf,
    /// Read-only before.
    pub was: bool,
    /// Read-only as asked.
    pub now: bool,
    /// Whether the permissions were changed.
    pub written: bool,
}

/// Makes the project file `path` read-only (`on`) or writable (behaviours 16, 17); with `dry_run`
/// only reports. Only the write bits change (Unix) or the read-only attribute (Windows).
pub fn set_read_only(
    root: &Path,
    path: &Path,
    on: bool,
    dry_run: bool,
) -> Result<ReadOnlyChange, Refusal> {
    let io = |e: std::io::Error| Refusal::Io(e.to_string());
    let path = resolve(root, path)?;
    let meta = std::fs::metadata(&path).map_err(|e| Refusal::Missing(e.to_string()))?;
    if meta.is_dir() {
        return Err(Refusal::Directory);
    }
    let was = is_read_only(&path).map_err(io)?;
    let written = was != on && !dry_run;
    if written {
        let mut permissions = meta.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = permissions.mode();
            permissions.set_mode(if on { mode & !0o222 } else { mode | 0o200 });
        }
        #[cfg(not(unix))]
        permissions.set_readonly(on);
        std::fs::set_permissions(&path, permissions).map_err(io)?;
    }
    Ok(ReadOnlyChange {
        path,
        was,
        now: on,
        written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use LineSeparator::{Cr, CrLf, Lf};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "idecraft-file-properties-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// SPEC-0336 AK-1: each separator converts into each other one.
    #[test]
    fn converts_between_the_three_separators() {
        let lines = |s: LineSeparator| format!("a{0}bb{0}{0}c{0}", s.as_str());
        for from in LineSeparator::ALL {
            for to in LineSeparator::ALL {
                let (text, changed) = convert_separators(&lines(from), to);
                assert_eq!(text, lines(to), "{from:?} to {to:?}");
                assert_eq!(changed, if from == to { 0 } else { 4 });
            }
        }
        assert_eq!(convert_separators("", CrLf), (String::new(), 0));
        assert_eq!(convert_separators("no break", Cr), ("no break".into(), 0));
        assert_eq!(convert_separators("ä\nö", CrLf), ("ä\r\nö".into(), 1));
        for s in LineSeparator::ALL {
            assert_eq!(LineSeparator::of_action(s.action_id()), Some(s));
            assert_eq!(LineSeparator::from_name(&s.name().to_lowercase()), Some(s));
            assert!(s.title().starts_with(s.name()));
        }
        assert_eq!(LineSeparator::from_name("nl"), None);
    }

    /// SPEC-0336 AK-1: a mixed text is normalised; the count is the line breaks changed.
    #[test]
    fn mixed_separators_are_normalised_and_counted() {
        let mixed = "one\r\ntwo\nthree\rfour\r\n\r\nfive\n";
        let counts = SeparatorCounts::of(mixed);
        assert_eq!(
            counts,
            SeparatorCounts {
                lf: 2,
                crlf: 3,
                cr: 1
            }
        );
        assert_eq!(
            convert_separators(mixed, Lf),
            ("one\ntwo\nthree\nfour\n\nfive\n".into(), 4)
        );
        assert_eq!(
            convert_separators(mixed, CrLf),
            ("one\r\ntwo\r\nthree\r\nfour\r\n\r\nfive\r\n".into(), 3)
        );
        assert_eq!(
            convert_separators(mixed, Cr),
            ("one\rtwo\rthree\rfour\r\rfive\r".into(), 5)
        );
        // `\r\r\n` is a CR and a CRLF, `\n\r` an LF and a CR.
        assert_eq!(convert_separators("a\r\r\nb\n\rc", Lf).0, "a\n\nb\n\nc");
    }

    /// SPEC-0336 AK-1: no line break is added at the end.
    #[test]
    fn last_line_without_a_line_break_stays() {
        assert_eq!(convert_separators("a\nb", CrLf), ("a\r\nb".into(), 1));
        assert_eq!(convert_separators("a\r\nb", Lf), ("a\nb".into(), 1));
        assert_eq!(convert_separators("a\rb", Lf), ("a\nb".into(), 1));
        assert_eq!(convert_separators("a\r", CrLf), ("a\r\n".into(), 1));
    }

    /// SPEC-0336 AK-1: CRLF → LF → CRLF is the first text again.
    #[test]
    fn crlf_lf_crlf_round_trip() {
        let crlf = "fn main() {\r\n    println!(\"é\");\r\n}\r\n\r\nlast";
        let (lf, n) = convert_separators(crlf, Lf);
        assert_eq!(n, 4);
        assert!(!lf.contains('\r'));
        assert_eq!(convert_separators(&lf, CrLf), (crlf.to_owned(), 4));
    }

    /// SPEC-0336 AK-1: the majority decides; LF before CRLF before CR on a tie.
    #[test]
    fn dominant_separator_and_ties() {
        let dominant = |t: &str| SeparatorCounts::of(t).dominant();
        assert_eq!(dominant(""), None);
        assert_eq!(dominant("abc"), None);
        assert_eq!(dominant("a\nb\r\nc\r\n"), Some(CrLf));
        assert_eq!(dominant("a\rb\rc\n"), Some(Cr));
        assert_eq!(dominant("a\nb\r\n"), Some(Lf));
        assert_eq!(dominant("a\nb\r"), Some(Lf));
        assert_eq!(dominant("a\r\nb\r"), Some(CrLf));
        assert_eq!(dominant("a\rb"), Some(Cr));
    }

    /// SPEC-0336 AK-2: what an encoding wrote it reads again.
    #[test]
    fn encodings_round_trip() {
        let ascii = "plain text\r\nwith lines\n";
        let latin = "Grüße, señor: ½ ÿ";
        let wide = "Grüße – “quoted” € 😀 日本";
        for charset in Charset::ALL {
            assert_eq!(Charset::from_name(charset.name()), Some(charset));
            for bom in [false, true] {
                let encoding = FileEncoding::with_bom(charset, bom);
                let texts: &[&str] = match charset {
                    Charset::UsAscii => &[ascii],
                    Charset::Iso8859_1 => &[ascii, latin],
                    Charset::Windows1252 => &[ascii, latin, "– “quoted” €"],
                    _ => &[ascii, latin, wide],
                };
                for text in texts {
                    let bytes = encode(text, encoding).unwrap();
                    assert_eq!(
                        decode(&bytes, encoding).as_deref(),
                        Some(*text),
                        "{encoding:?}"
                    );
                }
            }
        }
        assert_eq!(encode("é", FileEncoding::UTF8).unwrap(), [0xC3, 0xA9]);
        assert_eq!(
            encode("é", FileEncoding::of(Charset::Iso8859_1)).unwrap(),
            [0xE9]
        );
        assert_eq!(
            encode("a€", FileEncoding::of(Charset::Windows1252)).unwrap(),
            [b'a', 0x80]
        );
        assert_eq!(
            encode("a😀", FileEncoding::of(Charset::Utf16Le)).unwrap(),
            [0xFF, 0xFE, b'a', 0, 0x3D, 0xD8, 0x00, 0xDE]
        );
        assert_eq!(
            encode("a", FileEncoding::of(Charset::Utf16Be)).unwrap(),
            [0xFE, 0xFF, 0, b'a']
        );
        assert_eq!(Charset::from_name("latin1"), Some(Charset::Iso8859_1));
        assert_eq!(Charset::from_name("utf8"), Some(Charset::Utf8));
        assert_eq!(Charset::from_name("Shift_JIS"), None);
        assert_eq!(
            FileEncoding::with_bom(Charset::Utf8, true).label(),
            "UTF-8 BOM"
        );
        assert_eq!(FileEncoding::of(Charset::Utf16Le).label(), "UTF-16LE");
        assert!(!FileEncoding::with_bom(Charset::UsAscii, true).bom);
    }

    /// SPEC-0336 AK-2: windows-1252 reads every byte and writes it back.
    #[test]
    fn windows_1252_maps_every_byte() {
        let all: Vec<u8> = (0..=255u8).collect();
        for charset in [Charset::Windows1252, Charset::Iso8859_1] {
            let encoding = FileEncoding::of(charset);
            let text = decode(&all, encoding).unwrap();
            assert_eq!(text.chars().count(), 256);
            assert_eq!(encode(&text, encoding).unwrap(), all, "{charset}");
        }
        let text = decode(
            &[0x80, 0x81, 0x93, 0x9F],
            FileEncoding::of(Charset::Windows1252),
        );
        assert_eq!(text.as_deref(), Some("€\u{81}“Ÿ"));
    }

    /// SPEC-0336 AK-2, IMP-0404: invalid bytes are an error, never a replacement character.
    #[test]
    fn invalid_bytes_are_refused_not_replaced() {
        let utf8 = FileEncoding::UTF8;
        assert_eq!(decode(&[b'a', 0xE9, b'b'], utf8), None);
        assert_eq!(decode(&[0xC3], utf8), None);
        assert_eq!(
            decode(&[b'a', 0x80], FileEncoding::of(Charset::UsAscii)),
            None
        );
        let le = FileEncoding::of(Charset::Utf16Le);
        // Odd length, a lone surrogate, a missing mark.
        assert_eq!(decode(&[0xFF, 0xFE, b'a'], le), None);
        assert_eq!(decode(&[0xFF, 0xFE, 0x00, 0xD8], le), None);
        assert_eq!(decode(&[b'a', 0], le), None);
        assert_eq!(decode(&[0xFF, 0xFE, b'a', 0], le).as_deref(), Some("a"));
    }

    /// SPEC-0336 AK-2: a character without a place is an error with count and first character.
    #[test]
    fn non_representable_characters_are_refused() {
        let text = "naïve café €5";
        assert_eq!(
            encode(text, FileEncoding::of(Charset::UsAscii)),
            Err(Unrepresentable {
                count: 3,
                first: 'ï'
            })
        );
        assert_eq!(
            encode(text, FileEncoding::of(Charset::Iso8859_1)),
            Err(Unrepresentable {
                count: 1,
                first: '€'
            })
        );
        assert!(encode(text, FileEncoding::of(Charset::Windows1252)).is_ok());
        assert_eq!(
            encode("日", FileEncoding::of(Charset::Windows1252)),
            Err(Unrepresentable {
                count: 1,
                first: '日'
            })
        );
        let said = Unrepresentable {
            count: 3,
            first: 'ï',
        }
        .message(Charset::UsAscii);
        assert_eq!(
            said,
            "3 characters cannot be written in US-ASCII (the first is 'ï', U+00EF)"
        );
    }

    /// SPEC-0336 AK-2: only the explicit lossy form replaces, and it returns what it wrote.
    #[test]
    fn lossy_encoding_is_explicit_and_says_what_it_wrote() {
        let (bytes, written, replaced) =
            encode_lossy("naïve café €5", FileEncoding::of(Charset::UsAscii));
        assert_eq!(written, "na?ve caf? ?5");
        assert_eq!(bytes, written.as_bytes());
        assert_eq!(replaced, 3);
        let (bytes, written, replaced) = encode_lossy("ok", FileEncoding::UTF8);
        assert_eq!(
            (bytes.as_slice(), written.as_str(), replaced),
            (&b"ok"[..], "ok", 0)
        );
    }

    /// SPEC-0336 AK-3: mark, choice, UTF-8, fallback; NUL bytes are not text.
    #[test]
    fn detection_order() {
        let read =
            |bytes: &[u8], chosen| detect(bytes, chosen).map(|d| (d.encoding, d.text, d.fallback));
        let utf8 = FileEncoding::UTF8;
        assert_eq!(read(b"plain", None), Ok((utf8, "plain".into(), false)));
        assert_eq!(read("é".as_bytes(), None), Ok((utf8, "é".into(), false)));
        // A mark decides, whatever was chosen.
        assert_eq!(
            read(&[0xEF, 0xBB, 0xBF, b'a'], Some(Charset::Iso8859_1)),
            Ok((
                FileEncoding::with_bom(Charset::Utf8, true),
                "a".into(),
                false
            ))
        );
        assert_eq!(
            read(&[0xFE, 0xFF, 0, b'a'], None),
            Ok((FileEncoding::of(Charset::Utf16Be), "a".into(), false))
        );
        // A choice decides when the bytes are valid in it.
        assert_eq!(
            read("é".as_bytes(), Some(Charset::Iso8859_1)),
            Ok((FileEncoding::of(Charset::Iso8859_1), "Ã©".into(), false))
        );
        assert_eq!(
            read("é".as_bytes(), Some(Charset::UsAscii)),
            Ok((utf8, "é".into(), false))
        );
        // Not UTF-8: windows-1252.
        assert_eq!(
            read(&[b'a', 0xE9, 0x80], None),
            Ok((FileEncoding::of(Charset::Windows1252), "aé€".into(), true))
        );
        // NUL without a UTF-16 mark, a mark with broken content, a NUL character in UTF-16.
        assert_eq!(read(&[b'a', 0, b'b'], None), Err(Refusal::Binary));
        assert_eq!(read(&[0xFF, 0xFE, b'a'], None), Err(Refusal::Binary));
        assert_eq!(read(&[0xFF, 0xFE, 0, 0], None), Err(Refusal::Binary));
        assert_eq!(read(b"", None), Ok((utf8, String::new(), false)));
    }

    /// SPEC-0336 AK-3: the mark is not in the text and is there again after a rewrite.
    #[test]
    fn a_byte_order_mark_is_kept_out_of_the_text_and_written_back() {
        let dir = scratch("bom-kept");
        let file = dir.join("marked.txt");
        std::fs::write(&file, b"\xEF\xBB\xBFone\r\ntwo\r\n").unwrap();
        let read = read_text(&file, None).unwrap();
        assert_eq!(read.text, "one\r\ntwo\r\n");
        assert_eq!(read.encoding.label(), "UTF-8 BOM");
        let change = set_line_separator(&dir, &file, Lf, false).unwrap();
        assert_eq!((change.changed, change.written), (2, true));
        assert_eq!(std::fs::read(&file).unwrap(), b"\xEF\xBB\xBFone\ntwo\n");
        // UTF-16 keeps its mark and its two-byte line breaks.
        let wide = dir.join("wide.txt");
        std::fs::write(
            &wide,
            encode("a\nb", FileEncoding::of(Charset::Utf16Le)).unwrap(),
        )
        .unwrap();
        set_line_separator(&dir, &wide, CrLf, false).unwrap();
        assert_eq!(
            std::fs::read(&wide).unwrap(),
            [0xFF, 0xFE, b'a', 0, b'\r', 0, b'\n', 0, b'b', 0]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0336 AK-3, AK-5: folders, binary and oversized files are refused with a reason and
    /// stay as they are.
    #[test]
    fn binary_and_oversized_files_are_refused() {
        let dir = scratch("refused");
        let binary = dir.join("image.bin");
        std::fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 1, 2, b'\r', b'\n']).unwrap();
        let large = dir.join("large.txt");
        let file = std::fs::File::create(&large).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        drop(file);
        std::fs::create_dir(dir.join("folder")).unwrap();
        for (path, why) in [
            (binary.clone(), Refusal::Binary),
            (large.clone(), Refusal::TooLarge),
            (dir.join("folder"), Refusal::Directory),
        ] {
            assert_eq!(set_line_separator(&dir, &path, Lf, false), Err(why.clone()));
            assert_eq!(
                convert_encoding(&dir, &path, FileEncoding::UTF8, true, false).map(|_| ()),
                Err(why.clone())
            );
            assert_eq!(
                reload_encoding(&dir, &path, Charset::Iso8859_1, false).map(|_| ()),
                Err(why.clone())
            );
            assert_eq!(properties(&dir, &path).map(|_| ()), Err(why));
        }
        assert_eq!(
            std::fs::read(&binary).unwrap(),
            [0x89, b'P', b'N', b'G', 0, 1, 2, b'\r', b'\n']
        );
        assert_eq!(std::fs::metadata(&large).unwrap().len(), MAX_BYTES + 1);
        assert_eq!(Refusal::Binary.to_string(), "it is not a text file");
        assert_eq!(Refusal::TooLarge.to_string(), "it is larger than 20 MiB");
        assert!(matches!(
            set_line_separator(&dir, &dir.join("missing.txt"), Lf, false),
            Err(Refusal::Missing(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0336 AK-4: reload changes nothing, something, or is impossible.
    #[test]
    fn reload_is_judged_against_the_bytes() {
        let judged =
            |bytes: &[u8], now: &str, to| reload_safety(bytes, now, to).map(|r| (r.safety, r.text));
        // ASCII reads the same in every one-byte encoding and UTF-8.
        assert_eq!(
            judged(b"abc", "abc", Charset::Iso8859_1),
            Ok((Safety::Same, "abc".into()))
        );
        // UTF-8 bytes read as Latin-1: another text, and the way back exists.
        assert_eq!(
            judged("é".as_bytes(), "é", Charset::Iso8859_1),
            Ok((Safety::Reversible, "Ã©".into()))
        );
        // Latin-1 bytes shown as windows-1252, read as UTF-8: not valid.
        assert_eq!(
            judged(&[0xE9], "é", Charset::Utf8),
            Err(Refusal::CannotReload(
                "its bytes are not valid UTF-8".into()
            ))
        );
        assert_eq!(
            judged("é".as_bytes(), "é", Charset::UsAscii),
            Err(Refusal::CannotReload(
                "its bytes are not valid US-ASCII".into()
            ))
        );
        // A mark the encoding cannot have; a mark it needs.
        assert_eq!(
            judged(&[0xEF, 0xBB, 0xBF, b'a'], "a", Charset::Iso8859_1),
            Err(Refusal::CannotReload(
                "its bytes start with the byte order mark of UTF-8".into()
            ))
        );
        assert_eq!(
            judged(b"ab", "ab", Charset::Utf16Le),
            Err(Refusal::CannotReload(
                "UTF-16LE needs its byte order mark, which the file does not have".into()
            ))
        );
        let marked = reload_safety(&[0xEF, 0xBB, 0xBF, b'a'], "a", Charset::Utf8).unwrap();
        assert_eq!((marked.safety, marked.encoding.bom), (Safety::Same, true));
    }

    /// SPEC-0336 AK-4: convert changes nothing, something, or loses characters.
    #[test]
    fn convert_is_judged_against_the_text() {
        let latin1 = FileEncoding::of(Charset::Iso8859_1);
        assert_eq!(convert_safety("abc", b"abc", latin1), (Safety::Same, None));
        assert_eq!(
            convert_safety("é", "é".as_bytes(), latin1),
            (Safety::Reversible, None)
        );
        assert_eq!(
            convert_safety("é€", "é€".as_bytes(), latin1),
            (
                Safety::Lossy,
                Some(Unrepresentable {
                    count: 1,
                    first: '€'
                })
            )
        );
        // Unsaved text counts: the bytes on disk differ from what would be written.
        assert_eq!(
            convert_safety("abcd", b"abc", FileEncoding::UTF8),
            (Safety::Reversible, None)
        );
        assert_eq!(
            convert_safety("a", b"a", FileEncoding::with_bom(Charset::Utf8, true)),
            (Safety::Reversible, None)
        );
    }

    /// SPEC-0336 AK-5: the file is replaced in one step with its permission bits; nothing else
    /// appears in the folder; a read-only file is refused.
    #[test]
    fn separators_are_rewritten_on_disk_atomically() {
        let dir = scratch("separators");
        let file = dir.join("script.sh");
        std::fs::write(&file, "#!/bin/sh\r\necho ä\r\nexit\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o750)).unwrap();
        }
        let change = set_line_separator(&dir, Path::new("script.sh"), Lf, false).unwrap();
        assert_eq!(change.path, file);
        assert_eq!(
            (
                change.before.crlf,
                change.before.lf,
                change.changed,
                change.written
            ),
            (2, 1, 2, true)
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "#!/bin/sh\necho ä\nexit\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o750);
        }
        assert_eq!(names(&dir), ["script.sh"], "no temporary file is left");
        // Already LF: nothing is written.
        let again = set_line_separator(&dir, &file, Lf, false).unwrap();
        assert_eq!((again.changed, again.written), (0, false));
        // A file in windows-1252 keeps its bytes but for the line breaks.
        let legacy = dir.join("legacy.txt");
        std::fs::write(&legacy, [b'a', 0xE9, b'\n', b'b']).unwrap();
        set_line_separator(&dir, &legacy, CrLf, false).unwrap();
        assert_eq!(
            std::fs::read(&legacy).unwrap(),
            [b'a', 0xE9, b'\r', b'\n', b'b']
        );
        // Read-only: refused, unchanged.
        set_read_only(&dir, &file, true, false).unwrap();
        assert_eq!(
            set_line_separator(&dir, &file, CrLf, false),
            Err(Refusal::ReadOnly)
        );
        assert_eq!(
            convert_encoding(
                &dir,
                &file,
                FileEncoding::of(Charset::Utf16Le),
                false,
                false
            )
            .map(|_| ()),
            Err(Refusal::ReadOnly)
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "#!/bin/sh\necho ä\nexit\n"
        );
        set_read_only(&dir, &file, false, false).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0336 AK-5: a conversion rewrites the bytes; a lossy one needs the confirmation.
    #[test]
    fn encoding_is_converted_on_disk_and_refused_when_lossy() {
        let dir = scratch("encoding");
        let file = dir.join("text.txt");
        std::fs::write(&file, "Grüße €\n").unwrap();
        let latin1 = FileEncoding::of(Charset::Iso8859_1);
        let refused = convert_encoding(&dir, &file, latin1, false, false);
        assert_eq!(
            refused,
            Err(Refusal::Lossy(
                Unrepresentable {
                    count: 1,
                    first: '€'
                },
                Charset::Iso8859_1
            ))
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Grüße €\n");
        assert_eq!(
            chosen(&file),
            None,
            "a refused conversion remembers nothing"
        );
        // windows-1252 holds every character: reversible, written.
        let change = convert_encoding(
            &dir,
            &file,
            FileEncoding::of(Charset::Windows1252),
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            (change.from, change.safety, change.written),
            (FileEncoding::UTF8, Safety::Reversible, true)
        );
        assert_eq!(
            std::fs::read(&file).unwrap(),
            [b'G', b'r', 0xFC, 0xDF, b'e', b' ', 0x80, b'\n']
        );
        assert_eq!(chosen(&file), Some(Charset::Windows1252));
        assert_eq!(read_text(&file, None).unwrap().text, "Grüße €\n");
        // Back to UTF-8, then lossy to US-ASCII with the confirmation.
        convert_encoding(&dir, &file, FileEncoding::UTF8, false, false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Grüße €\n");
        let lossy =
            convert_encoding(&dir, &file, FileEncoding::of(Charset::UsAscii), true, false).unwrap();
        assert_eq!(lossy.safety, Safety::Lossy);
        assert_eq!(lossy.unrepresentable.map(|u| u.count), Some(3));
        assert_eq!(std::fs::read(&file).unwrap(), b"Gr??e ?\n");
        // The same encoding again: nothing to write.
        let same = convert_encoding(
            &dir,
            &file,
            FileEncoding::of(Charset::UsAscii),
            false,
            false,
        )
        .unwrap();
        assert_eq!((same.safety, same.written), (Safety::Same, false));
        // Reload: remembered only when possible.
        std::fs::write(&file, [b'a', 0xE9]).unwrap();
        assert!(matches!(
            reload_encoding(&dir, &file, Charset::Utf8, false),
            Err(Refusal::CannotReload(_))
        ));
        let reload = reload_encoding(&dir, &file, Charset::Iso8859_1, false).unwrap();
        assert_eq!((reload.reload.text.as_str(), reload.chosen), ("aé", true));
        assert_eq!(chosen(&file), Some(Charset::Iso8859_1));
        assert_eq!(names(&dir), ["text.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0336 AK-5: the mark of UTF-8 is added and removed; other encodings say why not.
    #[test]
    fn add_and_remove_the_byte_order_mark() {
        let dir = scratch("bom");
        let file = dir.join("a.txt");
        std::fs::write(&file, "é\n").unwrap();
        assert_eq!(
            set_bom(&dir, &file, false, false).map(|_| ()),
            Err(Refusal::Bom("the file has no byte order mark".into()))
        );
        assert!(set_bom(&dir, &file, true, false).unwrap().written);
        assert_eq!(std::fs::read(&file).unwrap(), b"\xEF\xBB\xBF\xC3\xA9\n");
        assert_eq!(
            set_bom(&dir, &file, true, false).map(|_| ()),
            Err(Refusal::Bom(
                "the file already has a byte order mark".into()
            ))
        );
        assert!(set_bom(&dir, &file, false, false).unwrap().written);
        assert_eq!(std::fs::read(&file).unwrap(), b"\xC3\xA9\n");
        assert_eq!(
            bom_change(FileEncoding::of(Charset::Utf16Be), false),
            Err(Refusal::Bom(
                "UTF-16BE always has its byte order mark".into()
            ))
        );
        assert_eq!(
            bom_change(FileEncoding::of(Charset::Iso8859_1), true),
            Err(Refusal::Bom("ISO-8859-1 has no byte order mark".into()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0336 AK-5: a dry run reports and writes nothing.
    #[test]
    fn dry_runs_write_nothing() {
        let dir = scratch("dry");
        let file = dir.join("a.txt");
        std::fs::write(&file, "é\r\n").unwrap();
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        let s = set_line_separator(&dir, &file, Lf, true).unwrap();
        assert_eq!((s.changed, s.written), (1, false));
        let e =
            convert_encoding(&dir, &file, FileEncoding::of(Charset::UsAscii), false, true).unwrap();
        assert_eq!((e.safety, e.written), (Safety::Lossy, false));
        assert_eq!(e.unrepresentable.map(|u| u.first), Some('é'));
        let r = reload_encoding(&dir, &file, Charset::Windows1252, true).unwrap();
        assert_eq!((r.reload.safety, r.chosen), (Safety::Reversible, false));
        let ro = set_read_only(&dir, &file, true, true).unwrap();
        assert_eq!((ro.was, ro.now, ro.written), (false, true, false));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "é\r\n");
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            before
        );
        assert!(!is_read_only(&file).unwrap());
        assert_eq!(chosen(&file), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0336 AK-6: nothing outside the project is read or changed, also not through a link.
    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_project_is_refused() {
        let base = scratch("escape");
        let (project, outside) = (base.join("project"), base.join("outside"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "a\r\nb\r\n").unwrap();
        std::os::unix::fs::symlink(&secret, project.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, project.join("dir")).unwrap();
        for path in [
            project.join("link.txt"),
            project.join("dir/secret.txt"),
            PathBuf::from("link.txt"),
            PathBuf::from("../outside/secret.txt"),
            secret.clone(),
            project.clone(),
        ] {
            let outside = Err(Refusal::OutsideProject);
            assert_eq!(properties(&project, &path).map(|_| ()), outside, "{path:?}");
            assert_eq!(
                set_line_separator(&project, &path, Lf, false).map(|_| ()),
                outside
            );
            assert_eq!(
                convert_encoding(
                    &project,
                    &path,
                    FileEncoding::of(Charset::Utf16Le),
                    true,
                    false
                )
                .map(|_| ()),
                outside
            );
            assert_eq!(
                reload_encoding(&project, &path, Charset::Iso8859_1, false).map(|_| ()),
                outside
            );
            assert_eq!(set_bom(&project, &path, true, false).map(|_| ()), outside);
            assert_eq!(
                set_read_only(&project, &path, true, false).map(|_| ()),
                outside
            );
        }
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "a\r\nb\r\n");
        assert!(!is_read_only(&secret).unwrap());
        assert_eq!(chosen(&secret), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// SPEC-0336 AK-6: a link that stays inside changes its target and stays a link.
    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_project_changes_its_target() {
        let project = scratch("inside");
        let target = project.join("real.txt");
        std::fs::write(&target, "a\r\nb").unwrap();
        let link = project.join("link.txt");
        std::os::unix::fs::symlink("real.txt", &link).unwrap();
        let change = set_line_separator(&project, &link, Lf, false).unwrap();
        assert_eq!(change.path, target);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "a\nb");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(names(&project), ["link.txt", "real.txt"]);
        let _ = std::fs::remove_dir_all(&project);
    }

    /// SPEC-0336 AK-7: the toggle changes the write bits as behaviour 17 says, nothing else.
    #[cfg(unix)]
    #[test]
    fn read_only_toggle_changes_only_the_write_bits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("read-only");
        let file = dir.join("a.sh");
        std::fs::write(&file, "content").unwrap();
        let mode = |m: u32| std::fs::set_permissions(&file, std::fs::Permissions::from_mode(m));
        let now = || std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
        mode(0o775).unwrap();
        let change = set_read_only(&dir, &file, true, false).unwrap();
        assert_eq!(
            (change.was, change.now, change.written),
            (false, true, true)
        );
        assert_eq!(now(), 0o555, "every write bit is cleared, the others stay");
        assert!(is_read_only(&file).unwrap());
        // Already read-only: nothing to do.
        assert!(!set_read_only(&dir, &file, true, false).unwrap().written);
        let change = set_read_only(&dir, &file, false, false).unwrap();
        assert_eq!(
            (change.was, change.now, change.written),
            (true, false, true)
        );
        assert_eq!(now(), 0o755, "only the owner's write bit comes back");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "content");
        assert!(set_read_only(&dir, &dir.join("missing"), true, false).is_err());
        std::fs::create_dir(dir.join("sub")).unwrap();
        assert_eq!(
            set_read_only(&dir, &dir.join("sub"), true, false),
            Err(Refusal::Directory)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

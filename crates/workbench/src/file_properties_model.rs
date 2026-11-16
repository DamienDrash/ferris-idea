//! What the file property actions offer, ask and say (`SPEC-0336`), without a window: the rows
//! of the File Encoding list, the questions before a reload or a conversion, and the status bar
//! lines. The properties themselves are in [`idecraft_core::file_properties`].

use idecraft_core::file_properties::{
    Charset, FileEncoding, LineSeparator, Refusal, Reload, Safety, Unrepresentable,
};

use crate::dialogs::DrawnQuestion;

/// Title of the list of encodings (behaviour 12).
pub const ENCODING_TITLE: &str = "File Encoding";
/// Title of the list of line separators the status bar opens (behaviour 21).
pub const SEPARATORS_TITLE: &str = "Line Separators";
/// Said when an action has no file to act on (behaviour 1).
pub const NO_FILE: &str = "No file: open a file or select one in the Project tree";
/// Said for a file of a remote project (behaviour 4).
pub const REMOTE: &str = "File properties are not available for remote project files";

/// A row of the File Encoding list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingRow {
    /// Add the byte order mark (`AddBom`).
    AddBom,
    /// Remove the byte order mark (`RemoveBom`).
    RemoveBom,
    /// Change to this encoding.
    Charset(Charset),
}

impl EncodingRow {
    /// The row's text.
    pub fn label(self) -> String {
        match self {
            EncodingRow::AddBom => "Add BOM".into(),
            EncodingRow::RemoveBom => "Remove BOM".into(),
            EncodingRow::Charset(c) => c.name().into(),
        }
    }
}

/// The rows for a file in `current` (behaviour 12): the byte order mark action that applies,
/// then every encoding; and the row of the current encoding.
pub fn encoding_rows(current: FileEncoding) -> (Vec<EncodingRow>, usize) {
    let mut rows = Vec::new();
    if current.charset == Charset::Utf8 {
        rows.push(if current.bom {
            EncodingRow::RemoveBom
        } else {
            EncodingRow::AddBom
        });
    }
    rows.extend(Charset::ALL.map(EncodingRow::Charset));
    let selected = rows
        .iter()
        .position(|r| *r == EncodingRow::Charset(current.charset))
        .unwrap_or(0);
    (rows, selected)
}

/// An answer to the question of behaviour 12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingAnswer {
    /// Read the bytes again in the encoding.
    Reload,
    /// Write the text in the encoding.
    Convert,
    /// Do nothing.
    Cancel,
}

/// What choosing another encoding leads to (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodingStep {
    /// Reload and convert both change nothing: the encoding is set at once.
    Set(FileEncoding),
    /// The question, with what each answer does.
    Ask(DrawnQuestion, Vec<EncodingAnswer>),
}

/// Judges the change of `name` to `to` from what reloading and converting would do
/// (behaviour 12). Reload is offered only when it is possible; Cancel is the default answer.
pub fn encoding_step(
    name: &str,
    to: Charset,
    reload: &Result<Reload, Refusal>,
    convert: (Safety, Option<Unrepresentable>),
) -> EncodingStep {
    if let (Ok(r), Safety::Same) = (reload, convert.0)
        && r.safety == Safety::Same
    {
        return EncodingStep::Set(r.encoding);
    }
    let mut detail = Vec::new();
    let mut buttons = Vec::new();
    let mut answers = Vec::new();
    match reload {
        Ok(r) => {
            detail.push(match r.safety {
                Safety::Same => format!("Reload reads the file again as {to}; its text stays the same."),
                _ => format!(
                    "Reload reads the bytes of the file again as {to}: the text in the editor changes, the file does not."
                ),
            });
            buttons.push("Reload".to_owned());
            answers.push(EncodingAnswer::Reload);
        }
        Err(why) => detail.push(format!("Reload is not possible: {why}.")),
    }
    detail.push(match convert {
        (Safety::Lossy, Some(u)) => format!(
            "Convert writes the text in {to} and loses characters: {}.",
            u.message(to)
        ),
        (Safety::Same, _) => "Convert keeps the text; the file's bytes stay the same.".to_owned(),
        _ => format!("Convert writes the text of the editor to the file in {to}."),
    });
    buttons.push("Convert".to_owned());
    answers.push(EncodingAnswer::Convert);
    buttons.push("Cancel".to_owned());
    answers.push(EncodingAnswer::Cancel);
    let cancel = buttons.len() - 1;
    EncodingStep::Ask(
        DrawnQuestion {
            message: format!("Change the encoding of {name} to {to}?"),
            detail: detail.join("\n"),
            buttons,
            default: cancel,
            cancel: Some(cancel),
        },
        answers,
    )
}

/// The question before a reload discards unsaved changes (behaviour 13): answer 0 reloads,
/// Cancel is the default.
pub fn reload_unsaved_question(name: &str, to: Charset) -> DrawnQuestion {
    DrawnQuestion {
        message: format!("Reload {name} in {to} and lose its unsaved changes?"),
        detail: "The editor's text is replaced by the file on disk. The file is not changed."
            .into(),
        buttons: vec!["Reload and Discard Changes".into(), "Cancel".into()],
        default: 1,
        cancel: Some(1),
    }
}

/// The question before a conversion that loses characters (behaviour 14): answer 0 converts,
/// Cancel is the default.
pub fn lossy_question(name: &str, to: Charset, lost: Unrepresentable) -> DrawnQuestion {
    DrawnQuestion {
        message: format!("Convert {name} to {to} and lose characters?"),
        detail: format!(
            "{}. Each of them is written as ? and becomes ? in the editor.",
            lost.message(to)
        ),
        buttons: vec!["Convert Anyway".into(), "Cancel".into()],
        default: 1,
        cancel: Some(1),
    }
}

/// What happened to one file: its name and the line breaks changed, or why it was refused.
pub type SeparatorResult = (String, Result<usize, String>);

/// The status bar line after converting line separators (behaviour 9); `folders` names the
/// selected folders, which are never converted.
pub fn separator_summary(
    target: LineSeparator,
    results: &[SeparatorResult],
    folders: &[String],
) -> String {
    let name = target.name();
    let breaks = |n: usize| if n == 1 { "line break" } else { "line breaks" };
    let mut said = match results {
        [] => String::new(),
        [(file, Ok(0))] => format!("{file} already has {name} line separators"),
        [(file, Ok(n))] => {
            format!(
                "Line separators changed to {name}: {n} {} in {file}",
                breaks(*n)
            )
        }
        [(file, Err(why))] => format!("Line separators of {file} were not changed: {why}"),
        many => {
            let changed = many
                .iter()
                .filter(|(_, r)| matches!(r, Ok(n) if *n > 0))
                .count();
            let already = many.iter().filter(|(_, r)| *r == Ok(0)).count();
            let refused: Vec<&SeparatorResult> = many.iter().filter(|(_, r)| r.is_err()).collect();
            let mut said = format!(
                "Line separators changed to {name} in {changed} of {} files",
                many.len()
            );
            if already > 0 {
                said.push_str(&format!("; {already} already had them"));
            }
            if let Some((file, Err(why))) = refused.first() {
                said.push_str(&format!("; {} refused ({file}: {why})", refused.len()));
            }
            said
        }
    };
    if let Some(first) = folders.first() {
        if !said.is_empty() {
            said.push_str("; ");
        }
        said.push_str(&match folders.len() {
            1 => format!("the folder {first} was left out: select files"),
            n => format!("{n} folders were left out: select files"),
        });
    }
    said
}

/// What happened to one file's read-only state: its name and the new state, or why not.
pub type ReadOnlyResult = (String, Result<bool, String>);

/// The status bar line after toggling the read-only state (behaviour 16).
pub fn read_only_summary(results: &[ReadOnlyResult], folders: &[String]) -> String {
    let state = |on: bool| if on { "read-only" } else { "writable" };
    let mut said = match results {
        [] => String::new(),
        [(file, Ok(on))] => format!("{file} is now {}", state(*on)),
        [(file, Err(why))] => format!("{file} was not changed: {why}"),
        many => {
            let now = |on: bool| many.iter().filter(|(_, r)| *r == Ok(on)).count();
            let refused: Vec<&ReadOnlyResult> = many.iter().filter(|(_, r)| r.is_err()).collect();
            let mut parts = Vec::new();
            for on in [true, false] {
                if now(on) > 0 {
                    parts.push(format!("{} now {}", now(on), state(on)));
                }
            }
            if let Some((file, Err(why))) = refused.first() {
                parts.push(format!("{} refused ({file}: {why})", refused.len()));
            }
            format!("{} files: {}", many.len(), parts.join(", "))
        }
    };
    if let Some(first) = folders.first() {
        if !said.is_empty() {
            said.push_str("; ");
        }
        said.push_str(&match folders.len() {
            1 => format!("the folder {first} was left out: select files"),
            n => format!("{n} folders were left out: select files"),
        });
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_core::file_properties::{convert_safety, reload_safety};

    /// SPEC-0336 AK-8: the list has the byte order mark action that applies and every encoding,
    /// the current one selected.
    #[test]
    fn encoding_rows_and_the_current_one() {
        let (rows, selected) = encoding_rows(FileEncoding::UTF8);
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0].label(), "Add BOM");
        assert_eq!(rows[selected], EncodingRow::Charset(Charset::Utf8));
        let (rows, _) = encoding_rows(FileEncoding::with_bom(Charset::Utf8, true));
        assert_eq!(rows[0], EncodingRow::RemoveBom);
        let (rows, selected) = encoding_rows(FileEncoding::of(Charset::Windows1252));
        let labels: Vec<String> = rows.iter().map(|r| r.label()).collect();
        assert_eq!(
            labels,
            [
                "UTF-8",
                "UTF-16LE",
                "UTF-16BE",
                "ISO-8859-1",
                "windows-1252",
                "US-ASCII"
            ]
        );
        assert_eq!(labels[selected], "windows-1252");
    }

    fn step(text: &str, bytes: &[u8], to: Charset) -> EncodingStep {
        encoding_step(
            "a.txt",
            to,
            &reload_safety(bytes, text, to),
            convert_safety(text, bytes, FileEncoding::of(to)),
        )
    }

    /// SPEC-0336 AK-8, behaviour 12: set at once, or a question whose default is Cancel and
    /// which offers Reload only when it is possible.
    #[test]
    fn the_encoding_question() {
        // ASCII reads and writes the same in ISO-8859-1.
        assert_eq!(
            step("abc", b"abc", Charset::Iso8859_1),
            EncodingStep::Set(FileEncoding::of(Charset::Iso8859_1))
        );
        // UTF-8 "é" to ISO-8859-1: both possible, both change something.
        let EncodingStep::Ask(q, answers) = step("é", "é".as_bytes(), Charset::Iso8859_1) else {
            panic!("a question");
        };
        assert_eq!(q.message, "Change the encoding of a.txt to ISO-8859-1?");
        assert_eq!(q.buttons, ["Reload", "Convert", "Cancel"]);
        assert_eq!(
            answers,
            [
                EncodingAnswer::Reload,
                EncodingAnswer::Convert,
                EncodingAnswer::Cancel
            ]
        );
        assert_eq!((q.default, q.cancel), (2, Some(2)));
        assert_eq!(q.answer_for_key("enter"), Some(2), "Return cancels");
        // To US-ASCII: the bytes are not ASCII, so no Reload; Convert would lose the é.
        let EncodingStep::Ask(q, answers) = step("é", "é".as_bytes(), Charset::UsAscii) else {
            panic!("a question");
        };
        assert_eq!(q.buttons, ["Convert", "Cancel"]);
        assert_eq!(answers, [EncodingAnswer::Convert, EncodingAnswer::Cancel]);
        assert_eq!(q.default, 1);
        assert!(
            q.detail
                .contains("Reload is not possible: its bytes are not valid US-ASCII.")
        );
        assert!(
            q.detail
                .contains("1 character cannot be written in US-ASCII")
        );
        // Unsaved text: converting writes it, so it is asked even for ASCII.
        assert!(matches!(
            step("abcd", b"abc", Charset::Iso8859_1),
            EncodingStep::Ask(..)
        ));
        for q in [
            reload_unsaved_question("a.txt", Charset::Utf8),
            lossy_question(
                "a.txt",
                Charset::UsAscii,
                Unrepresentable {
                    count: 2,
                    first: 'é',
                },
            ),
        ] {
            assert_eq!((q.buttons.len(), q.default, q.cancel), (2, 1, Some(1)));
            assert_eq!(q.answer_for_key("enter"), Some(1));
        }
    }

    /// SPEC-0336 behaviours 9 and 16: what the status bar says.
    #[test]
    fn summaries() {
        let one = |r| vec![("a.txt".to_owned(), r)];
        assert_eq!(
            separator_summary(LineSeparator::Lf, &one(Ok(12)), &[]),
            "Line separators changed to LF: 12 line breaks in a.txt"
        );
        assert_eq!(
            separator_summary(LineSeparator::CrLf, &one(Ok(1)), &[]),
            "Line separators changed to CRLF: 1 line break in a.txt"
        );
        assert_eq!(
            separator_summary(LineSeparator::Cr, &one(Ok(0)), &[]),
            "a.txt already has CR line separators"
        );
        assert_eq!(
            separator_summary(LineSeparator::Lf, &one(Err("it is read-only".into())), &[]),
            "Line separators of a.txt were not changed: it is read-only"
        );
        let many = [
            ("a".to_owned(), Ok(3)),
            ("b".to_owned(), Ok(0)),
            ("c".to_owned(), Err("it is not a text file".to_owned())),
            ("d".to_owned(), Ok(1)),
        ];
        assert_eq!(
            separator_summary(LineSeparator::Lf, &many, &["src".to_owned()]),
            "Line separators changed to LF in 2 of 4 files; 1 already had them; 1 refused (c: it is not a text file); the folder src was left out: select files"
        );
        assert_eq!(
            separator_summary(LineSeparator::Lf, &[], &["a".to_owned(), "b".to_owned()]),
            "2 folders were left out: select files"
        );
        assert_eq!(
            read_only_summary(&[("a.txt".to_owned(), Ok(true))], &[]),
            "a.txt is now read-only"
        );
        assert_eq!(
            read_only_summary(&[("a.txt".to_owned(), Ok(false))], &[]),
            "a.txt is now writable"
        );
        assert_eq!(
            read_only_summary(
                &[
                    ("a".to_owned(), Ok(true)),
                    ("b".to_owned(), Ok(false)),
                    ("c".to_owned(), Err("it is outside the project".to_owned())),
                ],
                &[]
            ),
            "3 files: 1 now read-only, 1 now writable, 1 refused (c: it is outside the project)"
        );
    }
}

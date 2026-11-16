//! Which classes hold the code of a Kotlin source line, said to kotlin-debug-adapter through the
//! paths of `setBreakpoints` (`SPEC-0132` v5 behaviour 4b).
//!
//! The adapter finds the classes of a breakpoint from the path alone: the folders after the last
//! one called `kotlin` or `java` are the package, the file name is the class (`zoo/Animals.kt` →
//! `zoo.Animals` and `zoo.AnimalsKt`). A class with another name than its file (`zoo.Animal`) is
//! never looked at, and classes inside a class that load later (companion objects, anonymous
//! objects) are missed too. IDECraft therefore sends the lines again under paths that name these
//! classes; the paths exist only as names.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// What to send for the breakpoints of one Kotlin file besides the file itself.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ClassSources {
    /// Paths that name a class to the adapter, each with the lines to set there.
    pub sources: Vec<(PathBuf, Vec<u64>)>,
    /// Lines whose class cannot be named to the adapter.
    pub unbindable: Vec<u64>,
}

/// A piece of Kotlin source, as far as finding declarations needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    /// A name or keyword.
    Word(&'a str),
    /// The inside of a string literal.
    Text(&'a str),
    /// Any other character.
    Sym(u8),
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// After the block comment that starts at `i`; comments nest in Kotlin.
fn skip_comment(b: &[u8], mut i: usize, line: &mut u64) -> usize {
    let mut depth = 0;
    while i < b.len() {
        if b[i..].starts_with(b"/*") {
            depth += 1;
            i += 2;
        } else if b[i..].starts_with(b"*/") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                break;
            }
        } else {
            if b[i] == b'\n' {
                *line += 1;
            }
            i += 1;
        }
    }
    i.min(b.len())
}

/// After the character literal that starts at `i`, or after the quote alone.
fn skip_char(b: &[u8], i: usize) -> usize {
    let from = if b.get(i + 1) == Some(&b'\\') {
        i + 3
    } else {
        i + 2
    };
    (from..(from + 8).min(b.len()))
        .take_while(|&j| b[j] != b'\n')
        .find(|&j| b[j] == b'\'')
        .map_or(i + 1, |j| j + 1)
}

/// After the `}` that closes the template whose code starts at `i`.
fn skip_template(b: &[u8], mut i: usize, line: &mut u64) -> usize {
    let mut depth = 1;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                *line += 1;
                i += 1;
            }
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            b'"' => i = skip_string(b, i, line).0,
            b'\'' => i = skip_char(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_comment(b, i, line),
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    i.min(b.len())
}

/// The end of the string literal that starts at `i` and the range of its inside.
fn skip_string(b: &[u8], i: usize, line: &mut u64) -> (usize, Range<usize>) {
    let raw = b[i..].starts_with(b"\"\"\"");
    let start = (i + if raw { 3 } else { 1 }).min(b.len());
    let mut j = start;
    while j < b.len() {
        match b[j] {
            b'\n' if raw => {
                *line += 1;
                j += 1;
            }
            // Not closed on its line.
            b'\n' => return (j, start..j),
            b'\\' if !raw => j += 2,
            b'$' if b.get(j + 1) == Some(&b'{') => j = skip_template(b, j + 2, line),
            b'"' if !raw => return (j + 1, start..j),
            b'"' if b[j..].starts_with(b"\"\"\"") => {
                let mut end = j + 3;
                while b.get(end) == Some(&b'"') {
                    end += 1;
                }
                return (end, start..j);
            }
            _ => j += 1,
        }
    }
    (b.len(), start..start)
}

/// The words, strings and other characters of `text` outside comments, each with its 1-based line.
fn tokens(text: &str) -> Vec<(u64, Tok<'_>)> {
    let b = text.as_bytes();
    let (mut i, mut line) = (0, 1u64);
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_comment(b, i, &mut line),
            b'"' => {
                let at = line;
                let (end, inside) = skip_string(b, i, &mut line);
                out.push((at, Tok::Text(text.get(inside).unwrap_or_default())));
                i = end;
            }
            b'\'' => i = skip_char(b, i),
            b'`' => {
                let end = (i + 1..b.len())
                    .take_while(|&j| b[j] != b'\n')
                    .find(|&j| b[j] == b'`');
                match end {
                    Some(end) => {
                        out.push((line, Tok::Word(&text[i + 1..end])));
                        i = end + 1;
                    }
                    None => i += 1,
                }
            }
            c if is_word(c) => {
                let start = i;
                while i < b.len() && is_word(b[i]) {
                    i += 1;
                }
                out.push((line, Tok::Word(&text[start..i])));
            }
            c if c.is_ascii_whitespace() => i += 1,
            c => {
                out.push((line, Tok::Sym(c)));
                i += 1;
            }
        }
    }
    out
}

/// The top-level declarations of a Kotlin file.
#[derive(Debug, PartialEq, Eq)]
struct Outline {
    /// The segments of the package.
    package: Vec<String>,
    /// The class of the file's top-level functions and properties (`AnimalsKt`).
    facade: String,
    /// Line of each top-level declaration with the name of its class; `None` for a function or
    /// property.
    starts: Vec<(u64, Option<String>)>,
}

/// The name Kotlin gives the class of the top-level code of the file called `stem`.
fn facade_name(stem: &str) -> String {
    let mut name: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.chars().next().is_none_or(|c| c.is_numeric()) {
        name.insert(0, '_');
    }
    let first = name.remove(0);
    format!("{}{name}Kt", first.to_ascii_uppercase())
}

fn outline(path: &Path, text: &str) -> Outline {
    let toks = tokens(text);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let mut out = Outline {
        package: Vec::new(),
        facade: facade_name(&stem),
        starts: Vec::new(),
    };
    let (mut braces, mut parens) = (0u32, 0u32);
    let mut packaged = false;
    for (k, &(line, tok)) in toks.iter().enumerate() {
        let next = toks.get(k + 1).map(|t| t.1);
        let word = match tok {
            Tok::Sym(b'{') => {
                braces += 1;
                continue;
            }
            Tok::Sym(b'}') => {
                braces = braces.saturating_sub(1);
                continue;
            }
            Tok::Sym(b'(' | b'[') => {
                parens += 1;
                continue;
            }
            Tok::Sym(b')' | b']') => {
                parens = parens.saturating_sub(1);
                continue;
            }
            Tok::Word(word) if braces == 0 && parens == 0 => word,
            _ => continue,
        };
        match word {
            "package" if !packaged && out.starts.is_empty() => {
                packaged = true;
                out.package = toks[k + 1..]
                    .iter()
                    .take_while(|t| t.0 == line && matches!(t.1, Tok::Word(_) | Tok::Sym(b'.')))
                    .filter_map(|t| match t.1 {
                        Tok::Word(w) => Some(w.to_owned()),
                        _ => None,
                    })
                    .collect();
            }
            // `@file:JvmName("Zoo")` renames the class of the top-level code.
            "JvmName"
                if k >= 3
                    && toks[k - 3].1 == Tok::Sym(b'@')
                    && toks[k - 2].1 == Tok::Word("file")
                    && toks[k - 1].1 == Tok::Sym(b':')
                    && next == Some(Tok::Sym(b'(')) =>
            {
                if let Some(Tok::Text(name)) = toks.get(k + 2).map(|t| t.1) {
                    out.facade = name.to_owned();
                }
            }
            "class" | "interface" | "object" => {
                // `Animal::class` is no declaration, `object : Runnable { … }` has no name.
                let reference =
                    k >= 2 && toks[k - 1].1 == Tok::Sym(b':') && toks[k - 2].1 == Tok::Sym(b':');
                if let (false, Some(Tok::Word(name))) = (reference, next) {
                    out.starts.push((line, Some(name.to_owned())));
                }
            }
            "fun" if next != Some(Tok::Word("interface")) => out.starts.push((line, None)),
            "val" | "var" => out.starts.push((line, None)),
            _ => {}
        }
    }
    out
}

impl Outline {
    /// The classes that may hold the code of 1-based `line`: the declaration the line is in,
    /// and every other one starting on the same line as it.
    fn classes(&self, line: u64) -> Vec<&str> {
        let Some(start) = self
            .starts
            .iter()
            .rev()
            .map(|s| s.0)
            .find(|&start| start <= line)
        else {
            // Before the first declaration there is no code.
            return vec![&self.facade];
        };
        let mut names: Vec<&str> = self
            .starts
            .iter()
            .filter(|s| s.0 == start)
            .map(|s| s.1.as_deref().unwrap_or(&self.facade))
            .collect();
        names.dedup();
        names
    }
}

/// The two class names kotlin-debug-adapter 0.4.4 derives from the path of a `.kt` file: the
/// path after the last folder called `kotlin` or `java`, with dots, and the same with `Kt`.
fn adapter_names(path: &Path) -> Vec<String> {
    let whole = path.to_string_lossy();
    let mut tail: Vec<&str> = whole
        .rsplit(['/', '\\'])
        .take_while(|s| *s != "kotlin" && *s != "java")
        .collect();
    tail.reverse();
    let joined = tail.join(".");
    let Some(class) = joined.strip_suffix(".kt") else {
        return Vec::new();
    };
    let at = class.rfind('.').map_or(0, |i| i + 1);
    let mut rest = class[at..].chars();
    let Some(first) = rest.next() else {
        return vec![class.to_owned()];
    };
    let kt = format!(
        "{}{}{}Kt",
        &class[..at],
        first.to_uppercase(),
        rest.as_str()
    );
    vec![class.to_owned(), kt]
}

/// Whether the Kotlin file `path` with the content `text` declares the class `class` (with its
/// package): as a top-level class or as the class of its top-level code.
pub(crate) fn declares(path: &Path, text: &str, class: &str) -> bool {
    let outline = outline(path, text);
    let Some(name) = class.rsplit('.').next() else {
        return false;
    };
    let package = &class[..class.len() - name.len()];
    package.trim_end_matches('.') == outline.package.join(".")
        && (name == outline.facade || outline.starts.iter().any(|s| s.1.as_deref() == Some(name)))
}

/// What to send besides `path` itself so that the 1-based `lines` of the Kotlin file `path` with
/// the content `text` are set in the classes that hold their code.
pub(crate) fn class_sources(path: &Path, text: &str, lines: &[u64]) -> ClassSources {
    let mut out = ClassSources::default();
    if lines.is_empty() {
        return out;
    }
    let outline = outline(path, text);
    let known = adapter_names(path);
    let mut per_class: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for &line in lines {
        for class in outline.classes(line) {
            let list = per_class.entry(class).or_default();
            if !list.contains(&line) {
                list.push(line);
            }
        }
    }
    // A package segment called `kotlin` or `java` cuts the adapter's class name short, whatever
    // the path.
    let nameable = outline.package.iter().all(|s| s != "kotlin" && s != "java");
    let named = |class: &str| {
        let mut alias = path.join("kotlin");
        alias.extend(&outline.package);
        alias.join(format!("{class}.kt"))
    };
    for (class, lines) in per_class {
        let full = outline
            .package
            .iter()
            .map(String::as_str)
            .chain([class])
            .collect::<Vec<_>>()
            .join(".");
        if !nameable || class.contains(['/', '\\', '.']) {
            if !known.contains(&full) {
                out.unbindable.extend(lines);
            }
            continue;
        }
        if !known.contains(&full) {
            out.sources.push((named(class), lines.clone()));
        }
        // The classes inside it, also those that load later: the adapter passes the name on as
        // a class filter, where a `*` at the end stands for any rest.
        out.sources.push((named(&format!("{class}$*")), lines));
    }
    out.unbindable.sort_unstable();
    out.unbindable.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOO: &str = r#"package zoo

/** Base type { of all animals. */
abstract class Animal(val name: String) {
    abstract fun sound(): String

    open fun describe(): String = "$name says ${sound() + "}"}"

    companion object {
        fun count() = 2
    }
}

class Cat(name: String) : Animal(name) {
    override fun sound() = "meow"
}

fun helper(): Int {
    val kind = Animal::class
    return 1
}

val shared = object : Runnable {
    override fun run() {}
}
"#;

    fn sources(path: &str, text: &str, lines: &[u64]) -> Vec<(String, Vec<u64>)> {
        class_sources(Path::new(path), text, lines)
            .sources
            .into_iter()
            .map(|(p, l)| (p.to_string_lossy().into_owned(), l))
            .collect()
    }

    #[test]
    fn declarations_are_found_outside_comments_strings_and_bodies() {
        let o = outline(Path::new("/p/src/main/kotlin/zoo/Animals.kt"), ZOO);
        assert_eq!(o.package, ["zoo"]);
        assert_eq!(o.facade, "AnimalsKt");
        assert_eq!(
            o.starts,
            [
                (4, Some("Animal".to_owned())),
                (14, Some("Cat".to_owned())),
                (18, None),
                (23, None),
            ]
        );
        assert_eq!(o.classes(7), ["Animal"]);
        assert_eq!(o.classes(10), ["Animal"]);
        assert_eq!(o.classes(15), ["Cat"]);
        assert_eq!(o.classes(20), ["AnimalsKt"]);
        assert_eq!(o.classes(24), ["AnimalsKt"]);
        assert_eq!(o.classes(1), ["AnimalsKt"]);
    }

    #[test]
    fn a_class_named_unlike_its_file_gets_a_source_of_its_own() {
        let file = "/p/src/main/kotlin/zoo/Animals.kt";
        assert_eq!(
            sources(file, ZOO, &[7, 15, 10]),
            [
                (format!("{file}/kotlin/zoo/Animal.kt"), vec![7, 10]),
                (format!("{file}/kotlin/zoo/Animal$*.kt"), vec![7, 10]),
                (format!("{file}/kotlin/zoo/Cat.kt"), vec![15]),
                (format!("{file}/kotlin/zoo/Cat$*.kt"), vec![15]),
            ]
        );
        // Top-level code is in `zoo.AnimalsKt`, which the adapter derives itself; only the
        // classes inside it are added.
        assert_eq!(
            sources(file, ZOO, &[19]),
            [(format!("{file}/kotlin/zoo/AnimalsKt$*.kt"), vec![19])]
        );
        assert!(sources(file, ZOO, &[]).is_empty());
    }

    #[test]
    fn a_class_named_like_its_file_needs_only_its_inner_classes() {
        let file = "/p/src/main/kotlin/zoo/Cat.kt";
        let text = "package zoo\n\nclass Cat {\n    fun sound() = \"meow\"\n}\n";
        assert_eq!(
            sources(file, text, &[4]),
            [(format!("{file}/kotlin/zoo/Cat$*.kt"), vec![4])]
        );
    }

    #[test]
    fn the_package_counts_not_the_folder() {
        // No folder called `kotlin`, and the package is not the folder.
        let file = "/p/src/app/Main.kt";
        let text = "package demo.app\n\nfun main() {\n    println(1)\n}\n";
        assert_eq!(
            sources(file, text, &[4]),
            [
                (format!("{file}/kotlin/demo/app/MainKt.kt"), vec![4]),
                (format!("{file}/kotlin/demo/app/MainKt$*.kt"), vec![4]),
            ]
        );
        // No package at all.
        let text = "fun main() {\n    println(1)\n}\n";
        assert_eq!(
            sources("/p/src/main/kotlin/Main.kt", text, &[2]),
            [(
                "/p/src/main/kotlin/Main.kt/kotlin/MainKt$*.kt".to_owned(),
                vec![2]
            )]
        );
    }

    #[test]
    fn a_renamed_file_class_and_odd_file_names() {
        let file = "/p/src/main/kotlin/zoo/my-tools.kt";
        let text = "@file:JvmName(\"Tools\")\npackage zoo\n\nfun tool() = 1\n";
        assert_eq!(
            sources(file, text, &[4]),
            [
                (format!("{file}/kotlin/zoo/Tools.kt"), vec![4]),
                (format!("{file}/kotlin/zoo/Tools$*.kt"), vec![4]),
            ]
        );
        assert_eq!(facade_name("my-tools"), "My_toolsKt");
        assert_eq!(facade_name("2d"), "_2dKt");
        assert_eq!(
            adapter_names(Path::new("/p/src/main/kotlin/zoo/animals.kt")),
            ["zoo.animals", "zoo.AnimalsKt"]
        );
        assert_eq!(
            adapter_names(Path::new("C:\\p\\src\\main\\java\\Main.kt")),
            ["Main", "MainKt"]
        );
    }

    #[test]
    fn the_file_of_a_main_class_is_known() {
        let file = Path::new("/p/src/main/kotlin/zoo/Animals.kt");
        assert!(declares(file, ZOO, "zoo.AnimalsKt"));
        assert!(declares(file, ZOO, "zoo.Cat"));
        assert!(!declares(file, ZOO, "zoo.MainKt"));
        assert!(!declares(file, ZOO, "AnimalsKt"));
        assert!(!declares(file, ZOO, "other.Cat"));
        let main = Path::new("/p/Main.kt");
        assert!(declares(main, "fun main() {}\n", "MainKt"));
        assert!(!declares(main, "fun main() {}\n", ""));
    }

    #[test]
    fn a_package_segment_called_kotlin_or_java_cannot_be_named() {
        let file = "/p/src/main/kotlin/org/kotlin/tools/Tool.kt";
        let text = "package org.kotlin.tools\n\nclass Tool {\n    fun run() = 1\n}\n";
        let plan = class_sources(Path::new(file), text, &[4, 4]);
        assert!(plan.sources.is_empty());
        assert_eq!(plan.unbindable, [4]);
    }

    #[test]
    fn strings_templates_and_comments_do_not_open_blocks() {
        let text = "package a\n\nval s = \"\"\"\n{ class Fake\n${ \"}\" }\n\"\"\"\nval c = '{'\n/* class Gone { /* nested */ */\nfun interface Shape { fun area(): Int }\nclass `my class`\n";
        let o = outline(Path::new("/x/A.kt"), text);
        assert_eq!(
            o.starts,
            [
                (3, None),
                (7, None),
                (9, Some("Shape".to_owned())),
                (10, Some("my class".to_owned())),
            ]
        );
    }
}

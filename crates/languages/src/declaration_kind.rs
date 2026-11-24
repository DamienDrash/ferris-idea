//! The kind of a Java or Kotlin file's primary declaration, for its icon (`SPEC-0131` v3
//! behaviour 7): a scan of the top level of the text, without a syntax tree, so the project tree
//! can ask for every visible file.

use std::path::Path;

/// What a Java or Kotlin file declares first of all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    /// A class (also a Kotlin `data`, `sealed` or `value` class).
    Class,
    /// An interface (also a Kotlin `fun interface`).
    Interface,
    /// An enum (Kotlin: `enum class`).
    Enum,
    /// A Java record.
    Record,
    /// An annotation type (Java `@interface`, Kotlin `annotation class`).
    Annotation,
    /// A Kotlin object.
    Object,
}

impl DeclarationKind {
    /// The bundled icon of the kind.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Class => "nodes/class",
            Self::Interface => "nodes/interface",
            Self::Enum => "nodes/enum",
            Self::Record => "ferris-idea/record",
            Self::Annotation => "ferris-idea/annotation",
            Self::Object => "ferris-idea/object",
        }
    }
}

/// The most text of a file read for its kind: the primary declaration starts near the top; a
/// longer licence header or import list leaves the plain file icon.
pub const SCAN_BYTES: usize = 64 * 1024;

/// A word or mark of the top level.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    At,
    DoubleColon,
    Other,
}

/// The tokens outside braces, parentheses, brackets, comments and literals.
fn top_level(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut depth) = (0usize, 0i32);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '/' if next == Some('/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if next == Some('*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                continue;
            }
            '"' => {
                // A text block or raw string runs to the next three quotes, another string to
                // the closing quote on its line.
                if next == Some('"') && chars.get(i + 2) == Some(&'"') {
                    i += 3;
                    while i < chars.len()
                        && !(chars[i] == '"'
                            && chars.get(i + 1) == Some(&'"')
                            && chars.get(i + 2) == Some(&'"'))
                    {
                        i += 1;
                    }
                    i += 3;
                } else {
                    i += 1;
                    while i < chars.len() && chars[i] != '"' && chars[i] != '\n' {
                        i += if chars[i] == '\\' { 2 } else { 1 };
                    }
                    i += 1;
                }
                if depth == 0 {
                    out.push(Token::Other);
                }
                continue;
            }
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' && chars[i] != '\n' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                i += 1;
                continue;
            }
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 || c.is_whitespace() => {}
            '@' => out.push(Token::At),
            ':' if next == Some(':') => {
                out.push(Token::DoubleColon);
                i += 1;
            }
            _ if c.is_alphabetic() || c == '_' || c == '$' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
                {
                    i += 1;
                }
                out.push(Token::Word(chars[start..i].iter().collect()));
                continue;
            }
            _ => out.push(Token::Other),
        }
        i += 1;
    }
    out
}

fn word(tokens: &[Token], i: usize) -> Option<&str> {
    match tokens.get(i) {
        Some(Token::Word(w)) => Some(w),
        _ => None,
    }
}

/// The top-level types of a Java text with their names, in order.
fn java_types(tokens: &[Token]) -> Vec<(DeclarationKind, &str)> {
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        let (Some(keyword), Some(name)) = (word(tokens, i), word(tokens, i + 1)) else {
            continue;
        };
        let annotation = i > 0 && tokens[i - 1] == Token::At;
        let kind = match keyword {
            "interface" if annotation => DeclarationKind::Annotation,
            _ if annotation => continue,
            "class" => DeclarationKind::Class,
            "interface" => DeclarationKind::Interface,
            "enum" => DeclarationKind::Enum,
            // `record` is a word like any other unless a name and a header follow.
            "record" if !matches!(name, "extends" | "implements") => DeclarationKind::Record,
            _ => continue,
        };
        // `Outer.class` in an annotation's arguments is inside parentheses and never gets here.
        out.push((kind, name));
    }
    out
}

/// The only top-level declaration of a Kotlin text when it is a type; `None` when the file
/// declares a function, a property or a type alias at the top level, or several types.
fn kotlin_type(tokens: &[Token]) -> Option<DeclarationKind> {
    let mut found = None;
    for i in 0..tokens.len() {
        let Some(keyword) = word(tokens, i) else {
            continue;
        };
        let before = i.checked_sub(1).and_then(|p| word(tokens, p));
        let reference = i > 0 && tokens[i - 1] == Token::DoubleColon;
        let kind = match keyword {
            // `fun interface` declares an interface.
            "fun" if word(tokens, i + 1) == Some("interface") => continue,
            "fun" | "val" | "var" | "typealias" => return None,
            "class" if reference => continue,
            "class" => match before {
                Some("enum") => DeclarationKind::Enum,
                Some("annotation") => DeclarationKind::Annotation,
                _ => DeclarationKind::Class,
            },
            "interface" => DeclarationKind::Interface,
            "object" => DeclarationKind::Object,
            _ => continue,
        };
        if found.replace(kind).is_some() {
            return None;
        }
    }
    found
}

/// The kind of the primary declaration of the Java or Kotlin file `path` with the content `text`
/// (`SPEC-0131` v3 behaviour 7). Java: the top-level type named like the file, else the first
/// top-level type. Kotlin: the file's only top-level declaration when it is a type. `None` for
/// other files, `package-info.java`, `module-info.java`, scripts and files without such a type.
pub fn primary_declaration(path: &Path, text: &str) -> Option<DeclarationKind> {
    let stem = path.file_stem()?.to_str()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mut end = text.len().min(SCAN_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = end < text.len();
    let tokens = top_level(&text[..end]);
    match ext.as_str() {
        "java" => {
            let types = java_types(&tokens);
            types
                .iter()
                .find(|(_, name)| *name == stem)
                .or(types.first())
                .map(|(kind, _)| *kind)
        }
        // A Kotlin file's kind needs the whole top level: more may follow in a cut text.
        "kt" if !cut => kotlin_type(&tokens),
        _ => None,
    }
}

/// [`primary_declaration`] of a file on disk, reading at most [`SCAN_BYTES`] of it.
pub fn primary_declaration_of(path: &Path) -> Option<DeclarationKind> {
    use std::io::Read;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "java" && ext != "kt" {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(SCAN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    primary_declaration(path, &String::from_utf8_lossy(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str, text: &str) -> Option<DeclarationKind> {
        primary_declaration(Path::new(name), text)
    }

    /// SPEC-0131 AK-5 (v3): Java kinds, the type named like the file first.
    #[test]
    fn java_primary_declarations() {
        use DeclarationKind::*;
        for (name, text, expected) in [
            (
                "Dog.java",
                "package shop;\n\npublic class Dog extends Animal {}\n",
                Some(Class),
            ),
            (
                "Pet.java",
                "public interface Pet { default String owner() { return \"\"; } }",
                Some(Interface),
            ),
            (
                "Shape.java",
                "public sealed interface Shape permits Circle {}",
                Some(Interface),
            ),
            (
                "Colour.java",
                "/** class Colour */\nenum Colour { RED }",
                Some(Enum),
            ),
            (
                "Circle.java",
                "public record Circle(double radius) implements Shape {}",
                Some(Record),
            ),
            (
                "Marker.java",
                "@Retention(RetentionPolicy.RUNTIME)\npublic @interface Marker {}",
                Some(Annotation),
            ),
            // An annotation on the class, also with a class literal and a string naming a keyword.
            (
                "A.java",
                "@Uses(value = B.class, note = \"interface\")\n@Deprecated final class A {}",
                Some(Class),
            ),
            // The type named like the file wins over an earlier helper.
            (
                "Main.java",
                "interface Helper {}\nenum Mode { A }\npublic class Main {}",
                Some(Class),
            ),
            (
                "Other.java",
                "interface Helper {}\nclass Main {}",
                Some(Interface),
            ),
            // Nested types and members do not count.
            (
                "Box.java",
                "class Box { interface Inner {} record R(int x) {} }",
                Some(Class),
            ),
            (
                "Text.java",
                "class Text { String s = \"\"\"\n  interface X {\n  \"\"\"; char c = '{'; }",
                Some(Class),
            ),
            (
                "package-info.java",
                "/** Docs. */\n@Marker\npackage shop;\n",
                None,
            ),
            (
                "module-info.java",
                "module shop { requires java.sql; }",
                None,
            ),
            ("Empty.java", "// nothing yet\n", None),
        ] {
            assert_eq!(kind(name, text), expected, "{name}");
        }
        // A variable named `record` in a top-level-looking position is no record.
        assert_eq!(kind("R.java", "class R { int record = 1; }"), Some(Class));
    }

    /// SPEC-0131 AK-5 (v3): Kotlin shows a type's kind only when the type is all the file declares.
    #[test]
    fn kotlin_primary_declarations() {
        use DeclarationKind::*;
        for (name, text, expected) in [
            (
                "Person.kt",
                "package zoo\n\nclass Person(val name: String, var age: Int) {\n}\n",
                Some(Class),
            ),
            (
                "Point.kt",
                "data class Point(val x: Int = 0, val y: Int = 0)",
                Some(Class),
            ),
            (
                "Pet.kt",
                "interface Pet {\n    val owner: String\n    fun friendly(): Boolean = true\n}",
                Some(Interface),
            ),
            (
                "Action.kt",
                "fun interface Action { fun run() }",
                Some(Interface),
            ),
            ("Colour.kt", "enum class Colour { RED, GREEN }", Some(Enum)),
            (
                "Marker.kt",
                "@Target(AnnotationTarget.CLASS)\nannotation class Marker",
                Some(Annotation),
            ),
            (
                "Registry.kt",
                "@file:JvmName(\"R\")\npackage zoo\nimport a.b\ninternal object Registry { val all = listOf<String>() }",
                Some(Object),
            ),
            (
                "Shape.kt",
                "sealed class Shape {\n    data class Circle(val radius: Double) : Shape()\n    object Empty : Shape()\n}",
                Some(Class),
            ),
            // Several declarations, or any function or property: the file.
            (
                "Animals.kt",
                "abstract class Animal\ninterface Pet\nclass Dog : Animal(), Pet",
                None,
            ),
            (
                "Main.kt",
                "fun total(a: Int, b: Int): Int = a + b\n\nfun main() {}\n",
                None,
            ),
            ("Util.kt", "class Util\n\nfun Util.twice() = 2", None),
            ("Consts.kt", "const val LIMIT = 3\nobject Holder", None),
            (
                "Types.kt",
                "typealias Names = List<String>\nclass Types",
                None,
            ),
            ("Ref.kt", "val kind = String::class\n", None),
            (
                "build.gradle.kts",
                "plugins { kotlin(\"jvm\") }\nclass X",
                None,
            ),
            ("Empty.kt", "package zoo\n", None),
        ] {
            assert_eq!(kind(name, text), expected, "{name}");
        }
        assert_eq!(kind("notes.txt", "class Notes"), None);
        assert_eq!(kind("a.ts", "class A {}"), None);
    }

    /// Every kind's icon is bundled in both variants, and a long file is read only at its start.
    #[test]
    fn icons_are_bundled_and_long_files_are_cut() {
        use DeclarationKind::*;
        for k in [Class, Interface, Enum, Record, Annotation, Object] {
            assert!(
                ferris_idea_ui::icons::svg(k.icon(), true).is_some(),
                "{k:?}"
            );
            assert!(
                ferris_idea_ui::icons::svg(k.icon(), false).is_some(),
                "{k:?}"
            );
        }
        let long = format!("class Big {{\n{}}}\n", "    // filler line\n".repeat(8000));
        assert!(long.len() > SCAN_BYTES);
        assert_eq!(kind("Big.java", &long), Some(Class));
        // Kotlin needs the whole top level, so a cut text gives the file icon.
        assert_eq!(kind("Big.kt", &long), None);
        let dir = std::env::temp_dir().join(format!("ferris-idea-declkind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Pet.java");
        std::fs::write(&file, "public interface Pet {}\n").unwrap();
        assert_eq!(primary_declaration_of(&file), Some(Interface));
        assert_eq!(primary_declaration_of(&dir.join("Missing.java")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

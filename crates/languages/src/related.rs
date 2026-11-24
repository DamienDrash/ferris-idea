//! Go to Test and Go to Related (`SPEC-0071` v10 behaviours 15 and 17): the test of a source file
//! and the subject of a test by their file names, and the files Ferris IDEA's own model relates to a
//! file — an HTML file and the stylesheets it links; and the header of a C or C++ source file and
//! the source of a header (`SPEC-0071` v12 behaviour 19). Nothing here reads a language server.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The extensions that match each other as JavaScript and TypeScript sources.
const JS: [&str; 8] = ["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx"];

/// The extensions that match each other as JVM sources.
const JVM: [&str; 4] = ["java", "kt", "groovy", "scala"];

/// Other languages whose tests are named `XTest` or `XTests`; a test keeps its subject's
/// extension.
const CAMEL: [&str; 3] = ["cs", "php", "swift"];

/// The files whose names are compared with each other.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Family {
    Python,
    Go,
    Js,
    Jvm,
    /// One of [`CAMEL`], by its extension.
    Same(String),
}

/// What a file's name says about tests (behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestName {
    /// Whether the file is named like a test.
    pub is_test: bool,
    /// The name without the test affix: `Cart` of `CartTest.java`, `cart` of `test_cart.py`.
    pub subject: String,
    family: Family,
}

/// `stem` without `suffix` when something is left.
fn without_suffix<'a>(stem: &'a str, suffix: &str) -> Option<&'a str> {
    stem.strip_suffix(suffix).filter(|rest| !rest.is_empty())
}

/// The test name of `file` by the names the run markers use (`SPEC-0045`): `test_x.py` and
/// `x_test.py`, `x_test.go`, `x.test.*` and `x.spec.*` of JavaScript and TypeScript, `XTest` and
/// `XTests` of the JVM languages, C#, PHP and Swift. `None` for a file of a language without a
/// naming rule.
pub fn test_name(file: &Path) -> Option<TestName> {
    let stem = file.file_stem()?.to_str()?;
    let extension = file.extension()?.to_str()?.to_ascii_lowercase();
    let (family, subject) = match extension.as_str() {
        "py" => (
            Family::Python,
            stem.strip_prefix("test_")
                .filter(|rest| !rest.is_empty())
                .or_else(|| without_suffix(stem, "_test")),
        ),
        "go" => (Family::Go, without_suffix(stem, "_test")),
        e if JS.contains(&e) => (
            Family::Js,
            without_suffix(stem, ".test").or_else(|| without_suffix(stem, ".spec")),
        ),
        e if JVM.contains(&e) || CAMEL.contains(&e) => (
            if JVM.contains(&e) {
                Family::Jvm
            } else {
                Family::Same(extension.clone())
            },
            without_suffix(stem, "Tests").or_else(|| without_suffix(stem, "Test")),
        ),
        _ => return None,
    };
    Some(TestName {
        is_test: subject.is_some(),
        subject: subject.unwrap_or(stem).to_owned(),
        family,
    })
}

/// How many folder names `a` and `b` share, counted from their own folders upwards.
fn shared_folders(a: &Path, b: &Path) -> usize {
    let folders = |p: &Path| -> Vec<std::ffi::OsString> {
        p.parent()
            .map(|d| d.iter().rev().map(|c| c.to_owned()).collect())
            .unwrap_or_default()
    };
    folders(a)
        .iter()
        .zip(folders(b).iter())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The tests of the source file `file`, or the subjects of the test `file`, among `files` (all
/// relative to the project folder): the files of the same family with the same subject name and
/// the other role, ordered by the folder names they share with `file` from its folder upwards,
/// then by path. `None` when `file`'s language has no naming rule.
pub fn counterparts(file: &Path, files: &[PathBuf]) -> Option<Vec<PathBuf>> {
    let me = test_name(file)?;
    let mut found: Vec<(usize, &PathBuf)> = files
        .iter()
        .filter(|other| other.as_path() != file)
        .filter(|other| {
            test_name(other).is_some_and(|t| {
                t.family == me.family && t.is_test != me.is_test && t.subject == me.subject
            })
        })
        .map(|other| (shared_folders(file, other), other))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    Some(found.into_iter().map(|(_, p)| p.clone()).collect())
}

/// The extensions of C, C++ and Objective-C headers (`SPEC-0071` v12 behaviour 19).
const HEADERS: [&str; 6] = ["h", "hh", "hpp", "hxx", "h++", "inl"];

/// The extensions of C, C++ and Objective-C source files.
const SOURCES: [&str; 7] = ["c", "cc", "cpp", "cxx", "c++", "m", "mm"];

/// Whether `file` is a header (`Some(true)`) or a source file (`Some(false)`) of C, C++ or
/// Objective-C; `None` for another file.
pub fn is_header(file: &Path) -> Option<bool> {
    let extension = file.extension()?.to_str()?.to_ascii_lowercase();
    if HEADERS.contains(&extension.as_str()) {
        Some(true)
    } else if SOURCES.contains(&extension.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// The headers of the source file `file`, or the sources of the header `file`, among `files`
/// (all relative to the project folder): the files of the other role with the same name
/// without the extension, ordered by the folder names they share with `file` from its folder
/// upwards, then by path. `None` when `file` is neither.
pub fn header_source(file: &Path, files: &[PathBuf]) -> Option<Vec<PathBuf>> {
    let header = is_header(file)?;
    let stem = file.file_stem()?;
    let mut found: Vec<(usize, &PathBuf)> = files
        .iter()
        .filter(|other| other.as_path() != file)
        .filter(|other| is_header(other) == Some(!header) && other.file_stem() == Some(stem))
        .map(|other| (shared_folders(file, other), other))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    Some(found.into_iter().map(|(_, p)| p.clone()).collect())
}

/// How a related file relates to the file asked about (behaviour 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// A stylesheet the HTML file links.
    Stylesheet,
    /// An HTML file that links the stylesheet.
    LinkedFrom,
}

/// The project's own files related to `file` (absolute, below `root`), relative to `root`: the
/// stylesheets an HTML file links, in the order of the links, and the HTML files that link a
/// stylesheet, by path. `text` is the file's content when it is open in an editor. `None` for a
/// file of a kind the model knows no relation for.
pub fn related_files(
    root: &Path,
    file: &Path,
    text: Option<&str>,
) -> Option<Vec<(PathBuf, Relation)>> {
    if crate::html_css::is_html(file) {
        let read;
        let text = match text {
            Some(text) => text,
            None => {
                read = std::fs::read_to_string(file).unwrap_or_default();
                &read
            }
        };
        return Some(
            crate::html_css::linked_stylesheets(root, file, text)
                .into_iter()
                .map(|p| (p, Relation::Stylesheet))
                .collect(),
        );
    }
    if crate::html_css::is_stylesheet(file) {
        return Some(
            crate::html_css::pages_linking(root, file)
                .into_iter()
                .map(|p| (p, Relation::LinkedFrom))
                .collect(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(file: &str) -> Option<(bool, String)> {
        test_name(Path::new(file)).map(|t| (t.is_test, t.subject))
    }

    /// SPEC-0071 AK-11 (v10): the test names of each language and what is left of them.
    #[test]
    fn test_names_and_their_subjects() {
        for (file, is_test, subject) in [
            ("shop/test_cart.py", true, "cart"),
            ("shop/cart_test.py", true, "cart"),
            ("shop/cart.py", false, "cart"),
            ("shop/cart_test.go", true, "cart"),
            ("shop/cart.go", false, "cart"),
            ("src/cart.test.ts", true, "cart"),
            ("src/cart.spec.jsx", true, "cart"),
            ("src/cart.ts", false, "cart"),
            ("src/CartTest.java", true, "Cart"),
            ("src/CartTests.kt", true, "Cart"),
            ("src/Cart.java", false, "Cart"),
            ("Tests/CartTests.cs", true, "Cart"),
            ("tests/CartTest.php", true, "Cart"),
            ("Tests/CartTests.swift", true, "Cart"),
            // The affix alone names no subject.
            ("src/Test.java", false, "Test"),
            ("test_.py", false, "test_"),
            // Go and Python do not use the camel-case names, nor Java the others.
            ("shop/CartTest.go", false, "CartTest"),
            ("src/cart_test.java", false, "cart_test"),
        ] {
            assert_eq!(name(file), Some((is_test, subject.to_owned())), "{file}");
        }
        // Rust keeps its tests in the file or under tests/ without a naming rule.
        for file in ["src/cart.rs", "README.md", "Makefile", "style.css"] {
            assert_eq!(name(file), None, "{file}");
        }
    }

    /// SPEC-0071 AK-11 (v10): tests of a source and the subject of a test, across the JavaScript
    /// and the JVM extensions and not across others, the nearest folders first.
    #[test]
    fn counterparts_in_both_directions_ordered_by_shared_folders() {
        let files: Vec<PathBuf> = [
            "src/main/java/shop/Cart.java",
            "src/main/java/other/Cart.java",
            "src/test/java/shop/CartTest.java",
            "src/test/java/shop/CartTests.kt",
            "src/test/java/other/CartTest.java",
            "src/test/java/shop/CartItemTest.java",
            "web/cart.ts",
            "web/cart.test.tsx",
            "web/e2e/cart.spec.js",
            "web/cart.go",
            "py/cart.py",
            "py/tests/test_cart.py",
            "py/cart_test.go",
            "php/Cart.php",
            "php/CartTest.php",
            "php/CartTest.cs",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let of = |file: &str| -> Vec<String> {
            counterparts(Path::new(file), &files)
                .unwrap()
                .iter()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect()
        };
        // The tests in the folder of the same name first, then the other one.
        assert_eq!(
            of("src/main/java/shop/Cart.java"),
            [
                "src/test/java/shop/CartTest.java",
                "src/test/java/shop/CartTests.kt",
                "src/test/java/other/CartTest.java",
            ]
        );
        assert_eq!(
            of("src/test/java/shop/CartTest.java"),
            [
                "src/main/java/shop/Cart.java",
                "src/main/java/other/Cart.java"
            ]
        );
        assert_eq!(
            of("web/cart.ts"),
            ["web/cart.test.tsx", "web/e2e/cart.spec.js"]
        );
        assert_eq!(of("web/e2e/cart.spec.js"), ["web/cart.ts"]);
        assert_eq!(of("py/cart.py"), ["py/tests/test_cart.py"]);
        assert_eq!(of("py/tests/test_cart.py"), ["py/cart.py"]);
        // Go: only Go files; the PHP test is not the C# file's.
        assert_eq!(of("web/cart.go"), ["py/cart_test.go"]);
        assert_eq!(of("php/Cart.php"), ["php/CartTest.php"]);
        assert_eq!(of("php/CartTest.cs"), Vec::<String>::new());
        // A file that is not in the list is still asked about; no rule gives no answer.
        assert_eq!(of("src/main/java/shop/CartItem.java").len(), 1);
        assert_eq!(counterparts(Path::new("src/cart.rs"), &files), None);
    }

    /// SPEC-0071 AK-13 (v10): the stylesheets of an HTML file, the pages of a stylesheet, and no
    /// relation for other files.
    #[test]
    fn related_files_of_html_and_stylesheets() {
        let root = std::env::temp_dir().join(format!("ferris-idea-related-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("css")).unwrap();
        let root = root.canonicalize().unwrap();
        let page = "<html><head><link rel=\"stylesheet\" href=\"css/site.css\"></head></html>\n";
        std::fs::write(root.join("index.html"), page).unwrap();
        std::fs::write(root.join("css/site.css"), ".a {}\n").unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        assert_eq!(
            related_files(&root, &root.join("index.html"), None),
            Some(vec![(PathBuf::from("css/site.css"), Relation::Stylesheet)])
        );
        // The editor's text counts: the link is gone there.
        assert_eq!(
            related_files(&root, &root.join("index.html"), Some("<html></html>")),
            Some(Vec::new())
        );
        assert_eq!(
            related_files(&root, &root.join("css/site.css"), None),
            Some(vec![(PathBuf::from("index.html"), Relation::LinkedFrom)])
        );
        assert_eq!(related_files(&root, &root.join("main.rs"), None), None);
        let _ = std::fs::remove_dir_all(&root);
    }
    /// SPEC-0071 AK-14 (v12): headers and sources in both directions by their names, the order
    /// by shared folders, other files unknown.
    #[test]
    fn headers_and_sources_by_name_ordered_by_shared_folders() {
        let files: Vec<PathBuf> = [
            "src/shop/cart.cpp",
            "src/shop/cart.mm",
            "include/shop/cart.h",
            "include/cart.hpp",
            "include/shop/carts.h",
            "src/shop/Cart.h",
            "docs/cart.md",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(is_header(&p("a/b.HPP")), Some(true));
        assert_eq!(is_header(&p("a/b.c++")), Some(false));
        assert_eq!(is_header(&p("a/b.rs")), None);
        assert_eq!(
            header_source(&p("src/shop/cart.cpp"), &files),
            Some(vec![p("include/shop/cart.h"), p("include/cart.hpp")])
        );
        assert_eq!(
            header_source(&p("include/shop/cart.h"), &files),
            Some(vec![p("src/shop/cart.cpp"), p("src/shop/cart.mm")])
        );
        assert_eq!(header_source(&p("src/shop/Cart.h"), &files), Some(vec![]));
        assert_eq!(header_source(&p("docs/cart.md"), &files), None);
    }
}

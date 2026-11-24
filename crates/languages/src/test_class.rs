//! Test… of Generate (`SPEC-0122` v10 behaviour 3e): the test class of a Java or Kotlin class —
//! the one the project already has, found by its name as Go to Test finds it, or where a new one
//! goes in the test root and the text it starts with. Nothing here reads a language server.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where the test class of a source file is, and what a new one holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestClassPlan {
    /// Workspace-relative path of the test class, `/`-separated.
    pub path: String,
    /// Whether the project has it already (it is opened, not written).
    pub exists: bool,
    /// The class's name (`PersonTest`).
    pub class: String,
    /// The text of a new test class; empty when it exists.
    pub text: String,
}

/// The test folder of `relative` (a source file): `src/test/<language>/<package>` beside
/// `src/main/<language>/<package>` of Maven and Gradle (modules included), `test/<package>`
/// beside a top-level `src/<package>`, otherwise the source's own folder.
fn test_folder(relative: &Path) -> PathBuf {
    let parts: Vec<String> = relative
        .parent()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let mut out = parts.clone();
    if let Some(i) = parts
        .windows(2)
        .position(|w| w[0] == "src" && w[1] == "main")
    {
        out[i + 1] = "test".into();
    } else if parts.first().is_some_and(|p| p == "src") {
        out[0] = "test".into();
    }
    out.iter().collect()
}

/// The package a Java or Kotlin text declares (`shop.core`), if any.
fn package_of(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let rest = line.strip_prefix("package ")?;
        let name = rest.trim_end_matches(';').trim();
        (!name.is_empty()).then(|| name.to_owned())
    })
}

/// Whether the project's build files name JUnit 4 and not JUnit 5: its test classes are then
/// `public`, as JUnit 4 requires.
fn junit4(root: &Path) -> bool {
    let text: String = ["pom.xml", "build.gradle", "build.gradle.kts"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(root.join(f)).ok())
        .collect();
    let four = text.contains("junit:junit") || text.contains("<artifactId>junit</artifactId>");
    four && !text.contains("junit-jupiter")
}

/// The test class of `relative`, a Java or Kotlin file of the project at `root` holding `text`;
/// `files` are the project's own files (relative), among which an existing test is looked for
/// first. A file that is a test already, or of another language, is refused with the reason.
pub fn plan(
    root: &Path,
    relative: &Path,
    text: &str,
    files: &[PathBuf],
) -> Result<TestClassPlan, String> {
    const ONLY: &str = "Test… works in Java and Kotlin classes";
    let extension = relative
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let kotlin = match extension.as_deref() {
        Some("java") => false,
        Some("kt") => true,
        _ => return Err(ONLY.into()),
    };
    let name = crate::related::test_name(relative).ok_or_else(|| ONLY.to_owned())?;
    let stem = relative
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.is_test {
        return Err(format!("{stem} is a test class already"));
    }
    let class = format!("{}Test", name.subject);
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let existing = |path: &Path| TestClassPlan {
        path: slash(path),
        exists: true,
        class: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| class.clone()),
        text: String::new(),
    };
    if let Some(found) = crate::related::counterparts(relative, files)
        .unwrap_or_default()
        .first()
    {
        return Ok(existing(found));
    }
    let file =
        test_folder(relative).join(format!("{class}.{}", if kotlin { "kt" } else { "java" }));
    // A file of that name outside the project's own files (an ignored folder) is opened too.
    if root.join(&file).is_file() {
        return Ok(existing(&file));
    }
    let mut out = String::new();
    if let Some(package) = package_of(text) {
        let end = if kotlin { "" } else { ";" };
        out.push_str(&format!("package {package}{end}\n\n"));
    }
    let public = if !kotlin && junit4(root) {
        "public "
    } else {
        ""
    };
    out.push_str(&format!("{public}class {class} {{\n\n}}\n"));
    Ok(TestClassPlan {
        path: slash(&file),
        exists: false,
        class,
        text: out,
    })
}

/// The byte offset in a new test class's `text` where the caret goes: the empty line in its body.
pub fn caret_in(text: &str) -> usize {
    text.find("{\n\n").map_or(text.len(), |i| i + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ferris-idea-testclass-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// SPEC-0122 v10 AK-25: a Maven source gets its test in `src/test/java` with the same package,
    /// a top-level `src/` one in `test/`, other files beside themselves; Kotlin keeps its language.
    #[test]
    fn a_new_test_goes_into_the_test_root() {
        let root = temp("new");
        let person = "package shop;\n\npublic class Person {\n}\n";
        let p = plan(
            &root,
            Path::new("src/main/java/shop/Person.java"),
            person,
            &[],
        )
        .unwrap();
        assert_eq!(p.path, "src/test/java/shop/PersonTest.java");
        assert!(!p.exists);
        assert_eq!(p.class, "PersonTest");
        assert_eq!(p.text, "package shop;\n\nclass PersonTest {\n\n}\n");
        assert_eq!(&p.text[caret_in(&p.text)..], "\n}\n");
        let m = plan(
            &root,
            Path::new("core/src/main/kotlin/a/B.kt"),
            "package a\n",
            &[],
        )
        .unwrap();
        assert_eq!(m.path, "core/src/test/kotlin/a/BTest.kt");
        assert_eq!(m.text, "package a\n\nclass BTest {\n\n}\n");
        let s = plan(
            &root,
            Path::new("src/shapes/Shape.java"),
            "package shapes;",
            &[],
        )
        .unwrap();
        assert_eq!(s.path, "test/shapes/ShapeTest.java");
        let flat = plan(&root, Path::new("Main.java"), "class Main {}", &[]).unwrap();
        assert_eq!(flat.path, "MainTest.java");
        assert_eq!(flat.text, "class MainTest {\n\n}\n");
        // JUnit 4 alone in the build file: the class is public.
        std::fs::write(root.join("pom.xml"), "<artifactId>junit</artifactId>").unwrap();
        let four = plan(&root, Path::new("Main.java"), "", &[]).unwrap();
        assert_eq!(four.text, "public class MainTest {\n\n}\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0122 v10 AK-25: an existing test is opened, not written; a test or another language
    /// is refused with the reason.
    #[test]
    fn an_existing_test_is_found_and_tests_are_refused() {
        let root = temp("found");
        let files = vec![
            PathBuf::from("src/main/java/shop/Person.java"),
            PathBuf::from("src/test/java/shop/PersonTests.java"),
        ];
        let p = plan(&root, &files[0], "package shop;", &files).unwrap();
        assert!(p.exists);
        assert_eq!(p.path, "src/test/java/shop/PersonTests.java");
        assert_eq!(p.class, "PersonTests");
        assert!(p.text.is_empty());
        // On disk but not among the project's own files.
        std::fs::create_dir_all(root.join("test")).unwrap();
        std::fs::write(root.join("test/ATest.java"), "class ATest {}").unwrap();
        assert!(
            plan(&root, Path::new("src/A.java"), "", &[])
                .unwrap()
                .exists
        );
        assert_eq!(
            plan(&root, Path::new("src/ATest.java"), "", &[]).unwrap_err(),
            "ATest is a test class already"
        );
        assert!(plan(&root, Path::new("a.py"), "", &[]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}

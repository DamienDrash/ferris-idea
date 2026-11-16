//! Source folders recognised from the files themselves (`SPEC-0048` v6 behaviour 14): where no
//! Maven or Gradle build file says which folders hold sources, the folder that is left when the
//! package a Java or Kotlin file declares is taken off the file's folder is a source folder
//! (`src/main/java` for `src/main/java/lab/inventory/Inventory.java` with
//! `package lab.inventory;`). Only the project's own sources are looked at, and only a bounded
//! number of files is read.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::buildtools::{BuildOptions, FolderInfo, FolderRole};

/// At most this many files are read for one project.
pub const MAX_FILES_READ: usize = 1_000;

/// At most this many files of one folder are read: the first by name that declare a package
/// decide for the folder.
pub const MAX_FILES_PER_FOLDER: usize = 3;

/// Of a file at most this many bytes are read; the package declaration stands before the imports.
pub const MAX_HEAD_BYTES: u64 = 64 * 1024;

/// Build files that declare the source folders of their folder: there nothing is derived.
const JVM_BUILD_FILES: &[&str] = &[
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];

/// The package a Java or Kotlin source declares: the name after `package`, up to `;` or the end
/// of the line, after comments and annotations (`package-info.java`, Kotlin's `@file:`). `None`
/// without a declaration or when the name is not made of plain identifiers.
pub fn declared_package(text: &str) -> Option<String> {
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("//") {
            rest = r.split_once('\n').map_or("", |(_, after)| after);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, after)| after);
        } else if rest.starts_with('@') {
            // An annotation of the package or of the file: to the end of its line.
            rest = rest.split_once('\n').map_or("", |(_, after)| after);
        } else {
            break;
        }
    }
    let r = rest.strip_prefix("package")?;
    if !r.starts_with(|c: char| c.is_whitespace() || c == '/') {
        return None;
    }
    let end = r.find([';', '\n']).unwrap_or(r.len());
    let name: String = r[..end].split_whitespace().collect();
    (!name.is_empty()
        && name.split('.').all(|part| {
            part.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && part
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        }))
    .then_some(name)
}

/// The folder that is left when `package` is taken off `dir`, the folder of a file that declares
/// it: `None` when the folder's last names are not the package's.
pub fn root_for_package(dir: &Path, package: &str) -> Option<PathBuf> {
    let mut root = dir.to_path_buf();
    for part in package.split('.').rev() {
        if root.file_name()? != part {
            return None;
        }
        root.pop();
    }
    Some(root)
}

/// Whether `path` is a Java or Kotlin source file by its name.
pub fn is_jvm_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("java") || e.eq_ignore_ascii_case("kt"))
}

/// Whether `path` is one of the build files that declare source folders.
pub fn is_jvm_build_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| JVM_BUILD_FILES.contains(&n))
}

/// Sources or tests, by the conventional path: a folder at or below `src/test` holds tests.
fn role_of(root: &Path) -> FolderRole {
    let names: Vec<_> = root.components().map(Component::as_os_str).collect();
    if names.windows(2).any(|w| w[0] == "src" && w[1] == "test") {
        FolderRole::Tests
    } else {
        FolderRole::Sources
    }
}

/// The first bytes of `file` as text; `None` when it cannot be read.
fn head(file: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(MAX_HEAD_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The source folders the files `files` of the project at `root` give (relative paths, as
/// [`crate::OwnSources::files`] lists them), relative to `root`, with their role.
///
/// - A folder with a Maven or Gradle build file, and everything below it, belongs to that build:
///   nothing is derived there, unless the folder was unlinked by hand (`unlinked` of
///   `.idecraft/build-tools.toml`), and no derived folder holds such a folder.
/// - Of each folder the first files by name are read, at most [`MAX_FILES_PER_FOLDER`], until one
///   declares a package the folder's last names spell; a folder below a source folder already
///   found is not read. In all at most [`MAX_FILES_READ`] files are read, of each at most
///   [`MAX_HEAD_BYTES`].
/// - The project folder itself is never a source folder, and of two candidates of which one
///   holds the other the outer one counts.
pub fn derived_from(root: &Path, files: &[PathBuf]) -> BTreeMap<PathBuf, FolderRole> {
    let links = BuildOptions::load(root);
    let rel_dir = |dir: &Path| {
        let s = dir.to_string_lossy().replace('\\', "/");
        if s.is_empty() { ".".to_owned() } else { s }
    };
    // Folders a real build file speaks for.
    let declared: BTreeSet<PathBuf> = files
        .iter()
        .cloned()
        .chain(links.linked.iter().map(PathBuf::from))
        .filter(|f| is_jvm_build_file(f))
        .map(|f| f.parent().map(Path::to_path_buf).unwrap_or_default())
        .filter(|dir| !links.is_unlinked(&rel_dir(dir)))
        .collect();
    if declared.contains(Path::new("")) {
        return BTreeMap::new();
    }
    let in_declared = |dir: &Path| declared.iter().any(|d| dir.starts_with(d));
    // Java and Kotlin files by folder, in name order.
    let mut by_folder: BTreeMap<PathBuf, Vec<&PathBuf>> = BTreeMap::new();
    for file in files.iter().filter(|f| is_jvm_source(f)) {
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        if !in_declared(&dir) {
            by_folder.entry(dir).or_default().push(file);
        }
    }
    let mut candidates: BTreeSet<PathBuf> = BTreeSet::new();
    let mut read = 0usize;
    'folders: for (dir, mut names) in by_folder {
        if candidates.iter().any(|c| dir.starts_with(c)) {
            continue;
        }
        names.sort();
        for file in names.into_iter().take(MAX_FILES_PER_FOLDER) {
            if read >= MAX_FILES_READ {
                break 'folders;
            }
            read += 1;
            let found = head(&root.join(file))
                .and_then(|text| declared_package(&text))
                .and_then(|package| root_for_package(&dir, &package));
            if let Some(candidate) = found {
                if !candidate.as_os_str().is_empty() {
                    candidates.insert(candidate);
                }
                break;
            }
        }
    }
    // Outer folders first; one that lies in another, or holds a folder with a build file, is
    // left out.
    let mut ordered: Vec<PathBuf> = candidates.into_iter().collect();
    ordered.sort_by(|a, b| {
        a.components()
            .count()
            .cmp(&b.components().count())
            .then(a.cmp(b))
    });
    let mut out: BTreeMap<PathBuf, FolderRole> = BTreeMap::new();
    for candidate in ordered {
        if out.keys().any(|r| candidate.starts_with(r))
            || declared.iter().any(|d| d.starts_with(&candidate))
        {
            continue;
        }
        let role = role_of(&candidate);
        out.insert(candidate, role);
    }
    out
}

/// The folder roles of the project at `root` that the project folder's own build file does not
/// give, as the project tree shows them: absolute paths below `root` as it is given.
/// - The roles of the Maven and Gradle modules whose build file lies in a sub-folder while the
///   project folder has none (`SPEC-0048` v7 behaviour 10a), read from the project's own build
///   files ([`crate::buildtools::folder_roles`]).
/// - The source and test folders no build file declares (v6 behaviour 14, [`derived_from`]).
///
/// Reads the project's own sources ([`crate::OwnSources`]) — never a virtual environment,
/// installed dependencies, build output or what Git ignores — in one walk, within the bounds of
/// [`derived_from`]. Walks the project folder: call it off the UI thread where the project may be
/// large.
pub fn derived(root: &Path) -> BTreeMap<PathBuf, FolderInfo> {
    // The usual case of a JVM project costs nothing: its build file speaks for every folder.
    let links = BuildOptions::load(root);
    if !links.is_unlinked(".") && JVM_BUILD_FILES.iter().any(|f| root.join(f).is_file()) {
        return BTreeMap::new();
    }
    let files = crate::OwnSources::new(root).files();
    let mut out = nested_modules(root, &files, &links);
    for (rel, role) in derived_from(root, &files) {
        out.entry(root.join(rel)).or_insert(FolderInfo {
            role,
            module_name: None,
        });
    }
    out
}

/// The folder roles of the Maven and Gradle modules among `files` (the project's own, relative
/// to `root`) and the build files linked by hand; nothing without such a build file, so a
/// project without one reads no build file at all.
fn nested_modules(
    root: &Path,
    files: &[PathBuf],
    links: &BuildOptions,
) -> BTreeMap<PathBuf, FolderInfo> {
    let any = files.iter().any(|f| is_jvm_build_file(f))
        || links.linked.iter().any(|f| is_jvm_build_file(Path::new(f)));
    if !any {
        return BTreeMap::new();
    }
    // Only these build files are read: the Cargo, npm and other build files of a large project
    // and their lock files would cost seconds and give no folder roles.
    let builds: Vec<PathBuf> = files
        .iter()
        .filter(|f| is_jvm_build_file(f))
        .cloned()
        .collect();
    crate::buildtools::folder_roles(root, &crate::read_project(root, &builds))
}

/// Whether a folder with `role` is a source folder: sources, tests and their resources
/// (`SPEC-0332` v2 behaviour 13a).
pub fn is_source_role(role: FolderRole) -> bool {
    matches!(
        role,
        FolderRole::Sources | FolderRole::Tests | FolderRole::Resources | FolderRole::TestResources
    )
}

/// The source folders of the project at `root` (absolute paths, sorted): those its Maven and
/// Gradle build files declare and those derived from the files ([`derived`]). Reads the build
/// files and walks the project's own sources: call it off the UI thread.
pub fn folders(root: &Path) -> Vec<PathBuf> {
    let mut roles = derived(root);
    if JVM_BUILD_FILES.iter().any(|f| root.join(f).is_file()) {
        let files = crate::Workspace::open(root)
            .ok()
            .and_then(|w| w.files().ok())
            .unwrap_or_default();
        roles.extend(crate::buildtools::folder_roles(
            root,
            &crate::read_project(root, &files),
        ));
    }
    roles
        .into_iter()
        .filter(|(_, info)| is_source_role(info.role))
        .map(|(path, _)| path)
        .collect()
}

/// The innermost of the source folders `roots` that holds `path`; a source folder does not hold
/// itself (`SPEC-0332` v2 behaviour 13a).
pub fn containing<'a>(roots: &'a [PathBuf], path: &Path) -> Option<&'a Path> {
    roots
        .iter()
        .filter(|root| path != root.as_path() && path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .map(PathBuf::as_path)
}

/// The package of a new Java or Kotlin file in folder `dir` of the project at `root`, from the
/// files around it: the nearest folder from `dir` upwards with a file of the project's own that
/// declares a package the folder's last names spell gives the source folder, and the folders
/// from there down to `dir` the package. `None` when no such file is found below `root`, or when
/// `dir` is the source folder itself. Reads at most [`MAX_FILES_PER_FOLDER`] files of a folder.
pub fn package_from_neighbours(root: &Path, dir: &Path) -> Option<String> {
    let below = dir.strip_prefix(root).ok()?;
    let own = crate::OwnSources::new(root);
    let mut at = below.to_path_buf();
    loop {
        if at.as_os_str().is_empty() {
            return None;
        }
        let mut names: Vec<PathBuf> = std::fs::read_dir(root.join(&at))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| at.join(e.file_name()))
            .filter(|f| is_jvm_source(f))
            .collect();
        names.sort();
        for file in names.into_iter().take(MAX_FILES_PER_FOLDER) {
            if own.not_own(&file).is_some() {
                continue;
            }
            let found = head(&root.join(&file))
                .and_then(|text| declared_package(&text))
                .and_then(|package| root_for_package(&at, &package));
            if let Some(source_root) = found {
                let package: Vec<String> = below
                    .strip_prefix(&source_root)
                    .ok()?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                return (!package.is_empty()).then(|| package.join("."));
            }
        }
        at.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary project with `files`, removed on drop.
    struct Project(PathBuf);

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn project(tag: &str, files: &[(&str, &str)]) -> Project {
        let dir = std::env::temp_dir().join(format!(
            "idecraft-source-roots-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        // The project is its own repository, wherever the temporary folder lies.
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let p = Project(dir);
        for (path, text) in files {
            p.write(path, text);
        }
        p
    }

    impl Project {
        fn write(&self, path: &str, text: &str) {
            let file = self.0.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }

        /// The derived folders as `path role`.
        fn roots(&self) -> Vec<String> {
            derived(&self.0)
                .into_iter()
                .map(|(path, info)| {
                    assert_eq!(info.module_name, None);
                    format!(
                        "{} {:?}",
                        path.strip_prefix(&self.0)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                        info.role
                    )
                })
                .collect()
        }
    }

    // SPEC-0048/AK-14 (v6, behaviour 14).
    #[test]
    fn package_declarations() {
        assert_eq!(
            declared_package("package lab.inventory;\n\npublic class Inventory {}\n").as_deref(),
            Some("lab.inventory")
        );
        // A licence header, a line comment and an annotated package.
        assert_eq!(
            declared_package(
                "/*\n * package wrong.one;\n */\n// package wrong.two;\n@NonNullApi\npackage lab.api;\n"
            )
            .as_deref(),
            Some("lab.api")
        );
        // Kotlin: no semicolon, a file annotation before.
        assert_eq!(
            declared_package("@file:JvmName(\"Util\")\npackage lab.util\n\nfun f() = 1\n")
                .as_deref(),
            Some("lab.util")
        );
        assert_eq!(
            declared_package("\u{feff}package lab ;").as_deref(),
            Some("lab")
        );
        for none in [
            "",
            "import java.util.List;\nclass A {}\n",
            "packages lab;\n",
            "package ;\n",
            "package 1lab;\n",
            "package lab..x;\n",
            "class A { String s = \"package lab;\"; }\n",
        ] {
            assert_eq!(declared_package(none), None, "{none:?}");
        }
        assert_eq!(
            root_for_package(Path::new("src/main/java/lab/inventory"), "lab.inventory"),
            Some(PathBuf::from("src/main/java"))
        );
        assert_eq!(
            root_for_package(Path::new("lab/inventory"), "lab.inventory"),
            Some(PathBuf::new())
        );
        assert_eq!(
            root_for_package(Path::new("src/other/inventory"), "lab.inventory"),
            None
        );
    }

    // SPEC-0048/AK-14 (v6, behaviour 14).
    #[test]
    fn roots_from_the_declared_packages() {
        let p = project(
            "derive",
            &[
                (
                    "src/main/java/lab/inventory/Inventory.java",
                    "package lab.inventory;\npublic class Inventory {}\n",
                ),
                (
                    "src/main/java/lab/inventory/util/deep/Leaf.java",
                    "package lab.inventory.util.deep;\nclass Leaf {}\n",
                ),
                (
                    "src/test/java/lab/inventory/InventoryTest.java",
                    "package lab.inventory;\nclass InventoryTest {}\n",
                ),
                (
                    "tools/gen/src/main/kotlin/lab/gen/Main.kt",
                    "package lab.gen\n\nfun main() {}\n",
                ),
                // No package, a package that is not the folder's, a file that is no source.
                ("scripts/Hello.java", "class Hello {}\n"),
                (
                    "odd/place/Odd.java",
                    "package lab.elsewhere;\nclass Odd {}\n",
                ),
                ("docs/one/two/note.txt", "package docs.one.two;\n"),
                // The project folder itself is never a source folder.
                ("top/Top.java", "package top;\nclass Top {}\n"),
            ],
        );
        assert_eq!(
            p.roots(),
            [
                "src/main/java Sources",
                "src/test/java Tests",
                "tools/gen/src/main/kotlin Sources",
            ]
        );
    }

    // SPEC-0048/AK-14 (v6, behaviour 14): a real build file always wins; (v7, behaviour 10a) its
    // module's folders have the roles it gives.
    #[test]
    fn a_build_file_speaks_for_its_folder() {
        let class = "package lab;\nclass A {}\n";
        let p = project(
            "build",
            &[
                ("app/pom.xml", "<project/>"),
                ("app/src/main/java/lab/A.java", class),
                ("lib/build.gradle.kts", ""),
                ("lib/code/lab/A.java", class),
                ("plain/src/main/java/lab/A.java", class),
                // A candidate that would hold a folder with a build file is left out.
                ("outer/lab/A.java", class),
                ("outer/inner/pom.xml", "<project/>"),
            ],
        );
        assert_eq!(
            p.roots(),
            [
                "app Module",
                "app/src/main/java Sources",
                "lib Module",
                "outer/inner Module",
                "plain/src/main/java Sources"
            ]
        );
        // A module unlinked by hand is no module of the model: its sources are derived.
        p.write(".idecraft/build-tools.toml", "unlinked = [\"lib\"]\n");
        assert_eq!(
            p.roots(),
            [
                "app Module",
                "app/src/main/java Sources",
                "lib/code Sources",
                "outer/inner Module",
                "plain/src/main/java Sources"
            ]
        );
        // A build file in the project folder speaks for everything.
        p.write("settings.gradle", "");
        assert!(p.roots().is_empty());
        p.write(".idecraft/build-tools.toml", "unlinked = [\".\"]\n");
        assert_eq!(
            p.roots(),
            [
                "app/src/main/java Sources",
                "lib/code Sources",
                "outer Sources",
                "plain/src/main/java Sources"
            ],
            "everything unlinked: no folder has a module"
        );
    }

    // SPEC-0048/AK-14 (v6, behaviour 14): only the project's own sources, within the bounds.
    #[test]
    fn only_own_sources_and_a_bounded_number_of_files() {
        let class = "package lab;\nclass A {}\n";
        let p = project(
            "own",
            &[
                (".gitignore", "generated/\n"),
                ("generated/src/lab/A.java", class),
                ("node_modules/pkg/java/lab/A.java", class),
                (".venv/lib/python3.12/site-packages/x/lab/A.java", class),
                ("target/classes/lab/A.java", class),
                ("own/lab/A.java", class),
            ],
        );
        assert_eq!(p.roots(), ["own Sources"]);
        // Of a folder the first three files by name are read: a package declared only in the
        // fourth is not found.
        let q = project(
            "bound",
            &[
                ("late/lab/A.java", "class A {}\n"),
                ("late/lab/B.java", "class B {}\n"),
                ("late/lab/C.java", "class C {}\n"),
                ("late/lab/D.java", "package lab;\nclass D {}\n"),
                ("soon/lab/A.java", "class A {}\n"),
                ("soon/lab/B.java", "class B {}\n"),
                ("soon/lab/C.java", "package lab;\nclass C {}\n"),
            ],
        );
        assert_eq!(q.roots(), ["soon Sources"]);
        // In all at most MAX_FILES_READ files are read: of that many folders without a
        // declaration plus one with, in name order, the last is not reached.
        let mut files: Vec<PathBuf> = (0..MAX_FILES_READ)
            .map(|i| PathBuf::from(format!("many/f{i:04}/A.java")))
            .collect();
        files.push(PathBuf::from("zz/lab/A.java"));
        let r = project("many", &[("zz/lab/A.java", class)]);
        assert!(derived_from(&r.0, &files).is_empty());
        assert_eq!(
            derived_from(&r.0, &files[1..])
                .into_iter()
                .collect::<Vec<_>>(),
            [(PathBuf::from("zz"), FolderRole::Sources)]
        );
    }

    // SPEC-0275/AK-3 (v11): the package of a new file from the files around it.
    #[test]
    fn package_of_a_new_file_from_its_neighbours() {
        let p = project(
            "neighbours",
            &[
                (
                    "app/code/lab/inventory/Inventory.java",
                    "package lab.inventory;\nclass Inventory {}\n",
                ),
                ("app/code/lab/inventory/util/.keep", ""),
                ("docs/a/b/note.txt", ""),
            ],
        );
        let at = |dir: &str| package_from_neighbours(&p.0, &p.0.join(dir));
        assert_eq!(
            at("app/code/lab/inventory").as_deref(),
            Some("lab.inventory")
        );
        assert_eq!(
            at("app/code/lab/inventory/util").as_deref(),
            Some("lab.inventory.util"),
            "an empty folder below a package"
        );
        assert_eq!(
            at("app/code/lab/inventory/util/new/deeper").as_deref(),
            Some("lab.inventory.util.new.deeper"),
            "folders that do not exist yet"
        );
        assert_eq!(at("app/code"), None, "the source folder itself");
        assert_eq!(at("docs/a/b"), None);
        assert_eq!(at(""), None);
    }

    /// SPEC-0048 v7 AK-14: a Maven and a Gradle module in sub-folders of a project without a
    /// build file of its own get the roles of behaviour 10; sources outside them are still
    /// derived; a build file that is not the project's own (installed dependencies) gives nothing.
    #[test]
    fn modules_in_sub_folders_get_their_roles() {
        let p = project(
            "nested",
            &[
                (
                    "app/pom.xml",
                    "<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId>\
                     <artifactId>shop-app</artifactId><version>1</version></project>",
                ),
                (
                    "app/src/main/java/lab/App.java",
                    "package lab;\nclass App {}\n",
                ),
                (
                    "app/src/test/java/lab/AppTest.java",
                    "package lab;\nclass AppTest {}\n",
                ),
                ("app/src/main/resources/app.properties", "a=1\n"),
                ("lib/build.gradle.kts", "plugins { kotlin(\"jvm\") }\n"),
                ("lib/src/main/kotlin/lab/Lib.kt", "package lab\nclass Lib\n"),
                (
                    "tools/gen/lab/gen/Main.kt",
                    "package lab.gen\n\nfun main() {}\n",
                ),
                (
                    "node_modules/pkg/pom.xml",
                    "<project><artifactId>pkg</artifactId></project>",
                ),
                (
                    "node_modules/pkg/src/main/java/x/X.java",
                    "package x;\nclass X {}\n",
                ),
            ],
        );
        std::fs::create_dir_all(p.0.join("app/target/classes")).unwrap();
        let roles: Vec<String> = derived(&p.0)
            .into_iter()
            .map(|(path, info)| {
                let rel = path
                    .strip_prefix(&p.0)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                match info.module_name {
                    Some(name) => format!("{rel} {:?} [{name}]", info.role),
                    None => format!("{rel} {:?}", info.role),
                }
            })
            .collect();
        assert_eq!(
            roles,
            [
                "app Module [shop-app]",
                "app/src/main/java Sources",
                "app/src/main/resources Resources",
                "app/src/test/java Tests",
                "app/target Excluded",
                "lib Module",
                "lib/src/main/kotlin Sources",
                "tools/gen Sources",
            ]
        );
    }

    /// SPEC-0332 v2 AK-12: the source folders of a project and the innermost one of a file.
    #[test]
    fn source_folder_of_a_file() {
        let p = project(
            "containing",
            &[
                (
                    "src/lab/inventory/Inventory.java",
                    "package lab.inventory;\n",
                ),
                (
                    "test/lab/inventory/InventoryTest.java",
                    "package lab.inventory;\n",
                ),
                ("notes/readme.txt", "hello\n"),
            ],
        );
        let roots = folders(&p.0);
        assert_eq!(roots, [p.0.join("src"), p.0.join("test")]);
        let file = p.0.join("src/lab/inventory/Inventory.java");
        assert_eq!(containing(&roots, &file), Some(p.0.join("src").as_path()));
        assert_eq!(
            containing(&roots, &p.0.join("src/lab")),
            Some(p.0.join("src").as_path())
        );
        assert_eq!(containing(&roots, &p.0.join("src")), None, "not itself");
        assert_eq!(containing(&roots, &p.0.join("notes/readme.txt")), None);
        assert_eq!(containing(&roots, &p.0.join("srcs/a.java")), None);
        // Of two nested folders the inner one counts.
        let nested = [p.0.join("src"), p.0.join("src/gen")];
        assert_eq!(
            containing(&nested, &p.0.join("src/gen/a/B.java")),
            Some(p.0.join("src/gen").as_path())
        );
        assert!(is_source_role(FolderRole::TestResources));
        assert!(!is_source_role(FolderRole::Module));
        assert!(!is_source_role(FolderRole::Excluded));
    }
}

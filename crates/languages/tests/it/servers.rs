//! The language features against more servers (F-035, F-036, F-038): pyright for Python,
//! typescript-language-server for TypeScript and gopls for Go. Each case is a small program; the file
//! on disk stays a stub, so every answer comes from the editor's text.

use std::path::PathBuf;
use std::time::Duration;

use idecraft_languages::insight::{LocationKind, complete_in, hover_in, locations_in, symbols_in};
use idecraft_languages::rename::plan_rename_in;

use crate::server_case::{Case, WAIT, check, installed, lines, skip};

#[tokio::test]
async fn pyright_python() {
    check(&Case {
        name: "python",
        server: "pyright-langserver",
        files: &[],
        file: "shapes.py",
        text: r#""""Shapes."""


def add(a: int, b: int) -> int:
    """Add two numbers."""
    return a + b


class Square:
    def __init__(self, side: int) -> None:
        self.side = side

    def area(self) -> int:
        return self.side * self.side


def main() -> None:
    sq = Square(2)
    total = add(sq.area(), 1)
    print(total)
"#,
        call: (18, 12),
        definition: (3, 4),
        second_argument: (18, 27),
        parameter: "b: int",
        variable: (17, 4),
        type_line: Some(8),
        doc: "Add two numbers",
        function: "add",
        typing: ("total = add(sq.area(), 1)", "total = ad", 18, 14),
        top_symbols: &["add", "Square", "main"],
        calls: true,
        rename_to: Some("plus"),
    })
    .await;
}

#[tokio::test]
async fn typescript_language_server() {
    check(&Case {
        name: "typescript",
        server: "typescript-language-server",
        files: &[(
            "tsconfig.json",
            "{\"compilerOptions\": {\"strict\": true}}\n",
        )],
        file: "shapes.ts",
        text: "/** Adds two numbers. */
function add(a: number, b: number): number {
    return a + b;
}

class Square {
    constructor(public side: number) {}
    area(): number {
        return this.side * this.side;
    }
}

function main(): void {
    const sq = new Square(2);
    const total = add(sq.area(), 1);
    console.log(total);
}
",
        call: (14, 18),
        definition: (1, 9),
        second_argument: (14, 33),
        parameter: "b: number",
        variable: (13, 10),
        type_line: Some(5),
        doc: "Adds two numbers",
        function: "add",
        typing: (
            "const total = add(sq.area(), 1);",
            "const total = ad",
            14,
            20,
        ),
        top_symbols: &["add", "Square", "main"],
        calls: true,
        rename_to: Some("plus"),
    })
    .await;
}

#[tokio::test]
async fn gopls_go() {
    check(&Case {
        name: "go",
        server: "gopls",
        files: &[("go.mod", "module shapes\n\ngo 1.21\n")],
        file: "main.go",
        text: "package main

import \"fmt\"

// Add adds two numbers.
func Add(a int, b int) int {
\treturn a + b
}

type Square struct {
\tSide int
}

func (s Square) Area() int {
\treturn s.Side * s.Side
}

func main() {
\tsq := Square{Side: 2}
\ttotal := Add(sq.Area(), 1)
\tfmt.Println(total)
}
",
        call: (19, 10),
        definition: (5, 5),
        second_argument: (19, 25),
        parameter: "b int",
        variable: (18, 1),
        type_line: Some(9),
        doc: "Add adds two numbers",
        function: "Add",
        typing: ("total := Add(sq.Area(), 1)", "total := Ad", 19, 12),
        top_symbols: &["Add", "Square", "main"],
        calls: true,
        rename_to: Some("Plus"),
    })
    .await;
}

#[tokio::test]
async fn intelephense_php() {
    check(&Case {
        name: "php",
        server: "intelephense",
        files: &[],
        file: "shapes.php",
        text: "<?php

/** Adds two numbers. */
function add(int $a, int $b): int
{
    return $a + $b;
}

class Square
{
    public function __construct(public int $side) {}
    public function area(): int { return $this->side * $this->side; }
}

function main(): void
{
    $sq = new Square(2);
    $total = add($sq->area(), 1);
    echo $total;
}
",
        call: (17, 14),
        definition: (3, 10),
        second_argument: (17, 30),
        parameter: "int $b",
        variable: (16, 5),
        type_line: None,
        doc: "Adds two numbers",
        function: "add",
        typing: ("$total = add($sq->area(), 1);", "$total = ad", 17, 15),
        top_symbols: &["add", "Square", "main"],
        calls: false,
        rename_to: None,
    })
    .await;
}

/// HTML, CSS and JSON from vscode-langservers-extracted: completion, hover and the outline.
#[tokio::test]
async fn vscode_html_css_json() {
    if !installed("vscode-css-language-server") {
        skip("vscode-langservers-extracted not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-web-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();

    let css = root.join("site.css");
    std::fs::write(&css, "\n").unwrap();
    let text = ".card {\n    color: red;\n}\n\n#main {\n    margin: 0;\n}\n";
    let hover = hover_in(&root, &css, Some(text), 1, 6, WAIT)
        .await
        .unwrap()
        .expect("a hover for color");
    assert!(hover.text.contains("color"), "{}", hover.text);
    let symbols = symbols_in(&root, &css, Some(text), WAIT).await.unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, [".card", "#main"]);
    let typed = text.replace("color: red;", "backg");
    let items = complete_in(&root, &css, Some(&typed), 1, 9, 200, WAIT)
        .await
        .unwrap();
    assert!(
        items.iter().any(|i| i.label == "background-color"),
        "{items:?}"
    );

    let html = root.join("index.html");
    std::fs::write(&html, "\n").unwrap();
    let text = "<!DOCTYPE html>\n<html>\n<body>\n<di\n</body>\n</html>\n";
    let items = complete_in(&root, &html, Some(text), 3, 3, 200, WAIT)
        .await
        .unwrap();
    assert!(items.iter().any(|i| i.label == "div"), "{items:?}");

    let json = root.join("package.json");
    std::fs::write(&json, "\n").unwrap();
    let text = "{\n  \"name\": \"site\",\n  \"scripts\": {\"build\": \"make\"}\n}\n";
    let symbols = symbols_in(&root, &json, Some(text), WAIT).await.unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["name", "scripts", "build"]);

    let _ = std::fs::remove_dir_all(&root);
}

/// A smaller case for servers of data, markup and script languages: what each of them offers.
struct Light {
    name: &'static str,
    server: &'static str,
    /// Whether CI may lack the server (the Swift toolchain is too large for the CI runner).
    optional: bool,
    files: &'static [(&'static str, &'static str)],
    file: &'static str,
    text: &'static str,
    /// Names the outline must contain.
    symbols: &'static [&'static str],
    /// A position and text the hover must contain.
    hover: Option<(u64, u64, &'static str)>,
    /// A position whose definition is on the given line.
    definition: Option<((u64, u64), u64)>,
    /// Text to complete in (replacing `from` with `to`), the caret and a label the list must offer.
    complete: Option<(&'static str, &'static str, u64, u64, &'static str)>,
}

/// Runs the request `$ask` again (at most twice more) while it ends in a timeout: a server that
/// starts cold under a heavily loaded machine may miss one deadline (marksman on the Mac merge
/// queue, 2026-11-17); a server that keeps timing out still fails the test.
macro_rules! after_timeouts {
    ($ask:expr) => {{
        let mut answer = $ask;
        for _ in 0..2 {
            if !matches!(answer, Err(idecraft_languages::lsp::LspError::Timeout(..))) {
                break;
            }
            answer = $ask;
        }
        answer
    }};
}

async fn check_light(case: &Light) {
    if !installed(case.server) {
        if case.optional {
            eprintln!("skipped: {} not installed (optional)", case.server);
        } else {
            skip(&format!("{} not installed", case.server));
        }
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-{}-{}", case.name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, content) in case.files {
        std::fs::write(root.join(path), content).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let file = root.join(case.file);
    std::fs::write(&file, "\n").unwrap();
    let (name, text) = (case.name, Some(case.text));

    // A cold server under load may answer before it has read the file (kotlin-language-server);
    // a server that keeps answering nothing still fails the test.
    let mut symbols = after_timeouts!(symbols_in(&root, &file, text, WAIT).await).unwrap();
    for _ in 0..2 {
        if !symbols.is_empty() {
            break;
        }
        symbols = after_timeouts!(symbols_in(&root, &file, text, WAIT).await).unwrap();
    }
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    for want in case.symbols {
        assert!(
            names.iter().any(|n| n.contains(want)),
            "{name}: {want} in {names:?}"
        );
    }
    if let Some((line, column, want)) = case.hover {
        // A cold kotlin-language-server under load answers "Internal error" or an empty hover
        // while it still compiles the file; a server that keeps failing still fails the test.
        // Attempts back off so the server can finish compiling.
        let mut answer = hover_in(&root, &file, text, line, column, WAIT).await;
        for attempt in 1..5 {
            if matches!(answer, Ok(Some(_))) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2 * attempt)).await;
            answer = hover_in(&root, &file, text, line, column, WAIT).await;
        }
        let hover = answer.unwrap().unwrap_or_else(|| panic!("{name}: a hover"));
        assert!(hover.text.contains(want), "{name}: {}", hover.text);
    }
    if let Some(((line, column), target)) = case.definition {
        // A server that is still compiling answers with no location (kotlin-language-server under
        // load); attempts back off, and a server that keeps answering nothing still fails.
        let mut def = Vec::new();
        for attempt in 0..5 {
            def = after_timeouts!(
                locations_in(
                    &root,
                    &file,
                    text,
                    line,
                    column,
                    LocationKind::Definition,
                    WAIT,
                )
                .await
            )
            .unwrap();
            if !def.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2 * (attempt + 1))).await;
        }
        assert_eq!(lines(&def), [target], "{name}: definition {def:?}");
    }
    if let Some((from, to, line, column, label)) = case.complete {
        let typed = case.text.replace(from, to);
        let items = complete_in(&root, &file, Some(&typed), line, column, 200, WAIT)
            .await
            .unwrap();
        assert!(
            items.iter().any(|i| i.label.trim().starts_with(label)),
            "{name}: {label} in {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn bash_language_server_shell() {
    check_light(&Light {
        name: "shell",
        server: "bash-language-server",
        optional: false,
        files: &[],
        file: "run.sh",
        text: "#!/bin/bash\ngreet() {\n  echo \"hi $1\"\n}\nNAME=world\ngreet \"$NAME\"\n",
        symbols: &["greet", "NAME"],
        hover: None,
        definition: Some(((5, 1), 1)),
        complete: Some(("greet \"$NAME\"", "gre", 5, 3, "greet")),
    })
    .await;
}

#[tokio::test]
async fn lua_language_server_lua() {
    check_light(&Light {
        name: "lua",
        server: "lua-language-server",
        optional: false,
        files: &[],
        file: "main.lua",
        text: "--- Adds two numbers.\nlocal function add(a, b)\n  return a + b\nend\nlocal total = add(1, 2)\nprint(total)\n",
        symbols: &["add", "total"],
        hover: Some((4, 15, "Adds two numbers")),
        definition: Some(((4, 15), 1)),
        complete: Some(("local total = add(1, 2)", "local total = ad", 4, 16, "add")),
    })
    .await;
}

#[tokio::test]
async fn yaml_language_server_yaml() {
    check_light(&Light {
        name: "yaml",
        server: "yaml-language-server",
        optional: false,
        files: &[],
        file: "compose.yml",
        text: "name: app\nservices:\n  web:\n    image: nginx\n",
        symbols: &["name", "services", "web", "image"],
        hover: None,
        definition: None,
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn taplo_toml() {
    check_light(&Light {
        name: "toml",
        server: "taplo",
        optional: false,
        files: &[],
        file: "config.toml",
        text: "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n",
        symbols: &["package", "name", "dependencies"],
        hover: None,
        definition: None,
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn marksman_markdown() {
    check_light(&Light {
        name: "markdown",
        server: "marksman",
        optional: false,
        files: &[],
        file: "README.md",
        text: "# Title\n\nSee [usage](#usage).\n\n## Usage\n\nRun it.\n",
        symbols: &["Title", "Usage"],
        hover: None,
        definition: Some(((2, 6), 4)),
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn lemminx_xml() {
    check_light(&Light {
        name: "xml",
        server: "lemminx",
        optional: false,
        files: &[],
        file: "pom.xml",
        text: "<project>\n  <name>demo</name>\n  <modules>\n    <module>core</module>\n  </modules>\n</project>\n",
        symbols: &["project", "name", "modules"],
        hover: None,
        definition: None,
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn docker_langserver_dockerfile() {
    check_light(&Light {
        name: "dockerfile",
        server: "docker-langserver",
        optional: false,
        files: &[],
        file: "Dockerfile",
        text: "FROM alpine:3.20\nRUN apk add curl\nCMD [\"sh\"]\n",
        symbols: &["FROM", "RUN"],
        hover: Some((1, 1, "RUN")),
        definition: None,
        complete: Some(("CMD [\"sh\"]", "EXP", 2, 3, "EXPOSE")),
    })
    .await;
}

#[tokio::test]
async fn vscode_scss_and_less() {
    for (name, file, text, complete) in [
        (
            "scss",
            "site.scss",
            "$main: red;\n.card {\n    color: $main;\n    .title { margin: 0; }\n}\n",
            ("color: $main;", "color: r;", "red"),
        ),
        (
            "less",
            "site.less",
            "@main: red;\n.card {\n    color: @main;\n    .title { margin: 0; }\n}\n",
            ("color: @main;", "color: r;", "red"),
        ),
    ] {
        let (name, file, text, complete) = (name, file, text, complete);
        check_light(&Light {
            name,
            server: "vscode-css-language-server",
            optional: false,
            files: &[],
            file,
            text,
            symbols: &[".card", ".title"],
            hover: Some((2, 6, "color")),
            definition: Some(((2, 13), 0)),
            complete: Some((complete.0, complete.1, 2, 12, complete.2)),
        })
        .await;
    }
}

#[tokio::test]
async fn sourcekit_lsp_swift() {
    check_light(&Light {
        name: "swift",
        server: "sourcekit-lsp",
        optional: true,
        files: &[],
        file: "main.swift",
        text: "/// Adds two numbers.\nfunc add(_ a: Int, _ b: Int) -> Int { a + b }\nlet total = add(1, 2)\nprint(total)\n",
        symbols: &["add", "total"],
        hover: Some((2, 13, "Adds two numbers")),
        definition: Some(((2, 13), 1)),
        complete: Some(("let total = add(1, 2)", "let total = ad", 2, 14, "add(")),
    })
    .await;
}

#[tokio::test]
async fn r_languageserver() {
    check_light(&Light {
        name: "r",
        server: "R",
        optional: false,
        files: &[],
        file: "analysis.R",
        text: "add <- function(a, b) {\n  a + b\n}\ntotal <- add(1, 2)\nprint(total)\n",
        symbols: &["add", "total"],
        hover: Some((3, 10, "add")),
        definition: Some(((3, 10), 0)),
        complete: Some(("total <- add(1, 2)", "total <- ad", 3, 11, "add")),
    })
    .await;
}

#[tokio::test]
async fn groovy_language_server() {
    check_light(&Light {
        name: "groovy",
        server: "groovy-language-server",
        optional: false,
        files: &[],
        file: "Shapes.groovy",
        text: "class Shapes {\n    static int add(int a, int b) {\n        a + b\n    }\n\n    static void main(String[] args) {\n        int total = add(1, 2)\n        println total\n    }\n}\n",
        symbols: &["Shapes", "add", "main"],
        // The server does not resolve unqualified static calls; variables and declarations work.
        hover: Some((1, 16, "static int add(int a, int b)")),
        definition: Some(((7, 16), 6)),
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn metals_scala() {
    check_light(&Light {
        name: "scala",
        server: "metals",
        optional: false,
        files: &[],
        file: "Shapes.scala",
        text: "object Shapes {\n  /** Adds two numbers. */\n  def add(a: Int, b: Int): Int = a + b\n\n  def main(args: Array[String]): Unit = {\n    val total = add(1, 2)\n    println(total)\n  }\n}\n",
        symbols: &["Shapes", "add", "main"],
        hover: None,
        definition: Some(((5, 16), 2)),
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn kotlin_language_server() {
    check_light(&Light {
        name: "kotlin",
        server: "kotlin-language-server",
        optional: true,
        files: &[],
        file: "Shapes.kt",
        text: "/** Adds two numbers. */\nfun add(a: Int, b: Int): Int = a + b\n\nfun main() {\n    val total = add(1, 2)\n    println(total)\n}\n",
        symbols: &["add", "main"],
        hover: Some((4, 17, "add")),
        definition: Some(((4, 17), 1)),
        complete: None,
    })
    .await;
}

#[tokio::test]
async fn vue_language_server() {
    check_light(&Light {
        name: "vue",
        server: "vue-language-server",
        optional: false,
        files: &[("package.json", "{\"name\": \"app\"}\n")],
        file: "App.vue",
        text: "<script setup lang=\"ts\">\n/** Doubles a number. */\nfunction twice(n: number) { return n * 2 }\nconst total = twice(21)\n</script>\n\n<template>\n  <p>{{ total }}</p>\n</template>\n",
        symbols: &["template", "script setup"],
        hover: Some((3, 15, "Doubles a number")),
        definition: Some(((3, 15), 2)),
        complete: Some(("const total = twice(21)", "const total = twi", 3, 17, "twice")),
    })
    .await;
}

#[tokio::test]
async fn svelte_language_server() {
    check_light(&Light {
        name: "svelte",
        server: "svelteserver",
        optional: false,
        files: &[("package.json", "{\"name\": \"app\"}\n")],
        file: "App.svelte",
        text: "<script>\n  /** Doubles a number. */\n  function twice(n) { return n * 2 }\n  let total = twice(21)\n</script>\n\n<p>{total}</p>\n",
        symbols: &["twice", "total"],
        hover: Some((3, 16, "Doubles a number")),
        definition: Some(((3, 16), 2)),
        complete: None,
    })
    .await;
}

/// Angular templates go to ngserver in a workspace with `angular.json` (SPEC-0141). Needs the
/// reference project `angular-basic` with its `node_modules` (`js.sh install` of the reference environment)
/// or `IDECRAFT_TEST_ANGULAR` pointing at such a project.
#[tokio::test]
async fn angular_language_server() {
    let project = std::env::var_os("IDECRAFT_TEST_ANGULAR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join("ReferenceEnv/projects/angular-basic"))
        })
        .filter(|p| p.join("node_modules/@angular/core").is_dir());
    let (Some(root), true) = (project, installed("ngserver")) else {
        skip("ngserver or the angular-basic reference project is missing");
        return;
    };
    let root = root.canonicalize().unwrap();
    let resolved =
        idecraft_languages::servers::resolve_for(idecraft_languages::LanguageId::Html, &root)
            .unwrap();
    assert_eq!(resolved.candidate.program, "ngserver");
    let file = root.join("src/app/app.component.html");
    let mut answer = hover_in(&root, &file, None, 0, 8, WAIT).await;
    for attempt in 1..5 {
        if matches!(answer, Ok(Some(_))) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2 * attempt)).await;
        answer = hover_in(&root, &file, None, 0, 8, WAIT).await;
    }
    let hover = answer.unwrap().expect("a hover on 'title'");
    assert!(
        hover.text.contains("title") && hover.text.contains("string"),
        "{}",
        hover.text
    );
}

/// A Java file in a subfolder of a project without Maven or Gradle (hand tests 12 and 16 of
/// round 5, `SPEC-0042` v7): jdtls finds the declarations of a class, a local variable and a
/// method in the file.
#[tokio::test]
async fn jdtls_standalone_file_in_subfolder() {
    if !installed("jdtls") {
        skip("jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-java-single-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sprachen")).unwrap();
    std::fs::write(root.join("shapes.cpp"), "int main() { return 0; }\n").unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"scripts\": {\"hello\": \"echo hi\"}}\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("sprachen/test.java");
    let text = "class Person {
    private final String name;

    Person(String name) {
        this.name = name;
    }

    String greet() {
        return \"Hello, \" + name;
    }
}

public class Main {
    public static void main(String[] args) {
        Person person = new Person(\"Ada\");
        System.out.println(person.greet());
    }
}
";
    std::fs::write(&file, text).unwrap();
    // `Person` in `new Person(…)`, then `person` and `greet` in the println line.
    for (line, character, want) in [(14u64, 30u64, 3u64), (15, 31, 14), (15, 35, 7)] {
        let def = locations_in(
            &root,
            &file,
            Some(text),
            line,
            character,
            LocationKind::Definition,
            WAIT,
        )
        .await
        .unwrap();
        assert_eq!(
            lines(&def),
            [want],
            "definition at {line}:{character}: {def:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0291 AK-2e (v7, OBS-0033 L11): a class field without an annotation has its type behind its
/// name and an enum member without an initializer its value; an annotated field and a member with an
/// initializer have none. A `.js` class field has its type too.
#[tokio::test]
async fn typescript_field_and_enum_hints() {
    if !installed("typescript-language-server") {
        skip("typescript-language-server not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-ts-fields-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tsconfig.json"),
        "{\"compilerOptions\": {\"allowJs\": true}}",
    )
    .unwrap();
    let file = dir.join("a.ts");
    std::fs::write(
        &file,
        "class Counter {\n    count = 0;\n    typed: string = \"x\";\n}\nenum Color { Red, Green }\nenum Named { A = \"a\", B = 5, C }\n",
    )
    .unwrap();
    let script = dir.join("b.js");
    std::fs::write(&script, "class Box {\n    size = 1;\n}\n").unwrap();
    idecraft_core::trust::trust_for_process(&dir);
    let wait = std::time::Duration::from_secs(90);
    let hints = idecraft_languages::inlay::parameter_hints_in(&dir, &file, None, wait)
        .await
        .unwrap();
    let seen: Vec<(u64, u64, &str, bool, bool, bool)> = hints
        .iter()
        .map(|h| {
            (
                h.line,
                h.character,
                h.label.as_str(),
                h.is_type,
                h.is_value,
                h.padding_left,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            // `count` in line 1, `Red` and `Green` in line 4, `C` after `B = 5` in line 5.
            (1, 9, ": number", true, false, true),
            (4, 16, "= 0", false, true, true),
            (4, 23, "= 1", false, true, true),
            (5, 30, "= 6", false, true, true),
        ]
    );
    let hints = idecraft_languages::inlay::parameter_hints_in(&dir, &script, None, wait)
        .await
        .unwrap();
    assert!(
        hints
            .iter()
            .any(|h| h.is_type && h.line == 1 && h.character == 8 && h.label == ": number"),
        "{hints:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0291 AK-2b: the server gets its settings, so a `.ts` file has parameter names (also for a
/// single argument) and inferred types.
#[tokio::test]
async fn typescript_inlay_hints() {
    if !installed("typescript-language-server") {
        skip("typescript-language-server not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-ts-inlay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
    let file = dir.join("a.ts");
    std::fs::write(
        &file,
        "function one(size: number) { return size * 2; }\nconst twice = one(1) + 1;\nconst text = String(twice);\nconsole.log(text);\n",
    )
    .unwrap();
    idecraft_core::trust::trust_for_process(&dir);
    let hints = idecraft_languages::inlay::parameter_hints_in(
        &dir,
        &file,
        None,
        std::time::Duration::from_secs(90),
    )
    .await
    .unwrap();
    let labels: Vec<&str> = hints.iter().map(|h| h.label.as_str()).collect();
    assert!(labels.contains(&"size:"), "{labels:?}");
    assert!(
        hints.iter().any(|h| h.is_type && h.label == ": number"),
        "{labels:?}"
    );
    // SPEC-0042 v10 behaviour 19, AK-15: the first search after opening already sees the other files.
    std::fs::write(
        dir.join("b.ts"),
        "import { one } from \"./a\";\nconsole.log(one(2));\n",
    )
    .unwrap();
    std::fs::write(
        &file,
        "export function one(size: number) { return size * 2; }\n",
    )
    .unwrap();
    let found = idecraft_languages::find_usages::find_usages_in(
        &dir,
        &file,
        None,
        0,
        17,
        "one",
        std::time::Duration::from_secs(90),
        None,
    )
    .await
    .unwrap();
    assert!(
        found.usages.iter().any(|u| u.path.ends_with("b.ts")),
        "{:?}",
        found.usages
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0042 v12 AK-20: with TypeScript 7 installed (its own language server, no `tsserver.js`),
/// IDECraft starts `tsc --lsp --stdio` from that installation and answers from it.
#[tokio::test]
async fn typescript_7_brings_its_own_server() {
    let Ok(r) = idecraft_languages::servers::resolve(idecraft_languages::LanguageId::TypeScript)
    else {
        skip("no TypeScript language server");
        return;
    };
    if r.candidate.program != "tsc" {
        // Not missing: this machine has a TypeScript for typescript-language-server.
        eprintln!("skipped: TypeScript 7 is not what IDECraft uses here");
        return;
    }
    assert_eq!(r.args(), ["--lsp", "--stdio"]);
    let dir = std::env::temp_dir().join(format!("idecraft-ts7-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
    let file = dir.join("a.ts");
    std::fs::write(
        &file,
        "export function one(size: number) { return size * 2; }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("b.ts"),
        "import { one } from \"./a\";\nconsole.log(one(2));\n",
    )
    .unwrap();
    let found = idecraft_languages::find_usages::find_usages_in(
        &dir,
        &file,
        None,
        0,
        17,
        "one",
        std::time::Duration::from_secs(90),
        None,
    )
    .await
    .unwrap();
    assert!(
        found.usages.iter().any(|u| u.path.ends_with("b.ts")),
        "{:?}",
        found.usages
    );
    // v13: its diagnostics are asked for, so a type error is found.
    let bad = dir.join("bad.ts");
    std::fs::write(&bad, "const n: number = \"x\";\nexport {};\n").unwrap();
    let found =
        idecraft_languages::lsp::file_diagnostics(&dir, &bad, std::time::Duration::from_secs(90))
            .await
            .unwrap();
    assert!(
        found
            .iter()
            .any(|d| d.severity == "error" && d.start_line == 0),
        "{found:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The project of the pyright test of `SPEC-0042` v14 behaviour 22: a class with a method and a
/// function in a package, used from two more files.
fn python_usages_project(name: &str) -> Option<PathBuf> {
    let python = idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Python)
        .ok()
        .filter(|r| idecraft_languages::python_server::is_python_server(r.candidate.program));
    if python.is_none() {
        skip("no pyright or basedpyright installed");
        return None;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-py-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("app")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("app/__init__.py"), "").unwrap();
    std::fs::write(
        dir.join("app/prices.py"),
        "from dataclasses import dataclass\n\n\n@dataclass\nclass Item:\n    name: str\n    price: float\n    qty: int = 1\n\n    def total(self) -> float:\n        return self.price * self.qty\n\n\ndef order_total(items: list[Item]) -> float:\n    return sum(i.total() for i in items)\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.py"),
        "from app.prices import Item, order_total\n\nitems = [Item(\"pen\", 1.5, 4), Item(\"book\", 12.0)]\nprint(order_total(items))\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("test_prices.py"),
        "from app.prices import Item, order_total\n\n\ndef test_total():\n    assert order_total([Item(\"a\", 2.0, 3)]) == 6.0\n    assert Item(\"b\", 1.0).total() == 1.0\n",
    )
    .unwrap();
    Some(dir)
}

/// SPEC-0042/AK-22 (OBS-0034 L8): the first question to a freshly started pyright already covers
/// the project's other files: Find Usages on a class, a function and a method, the references and
/// the rename plan.
#[tokio::test]
async fn pyright_usages_in_other_files_on_the_first_question() {
    let Some(dir) = python_usages_project("usages") else {
        return;
    };
    let file = dir.join("app/prices.py");
    // (line, character, name, usages per file: prices.py, main.py, test_prices.py)
    for (line, character, name, expected) in [
        (4, 6, "Item", [1, 3, 3]),
        (13, 4, "order_total", [0, 2, 2]),
        (9, 8, "total", [1, 0, 1]),
    ] {
        let found = idecraft_languages::find_usages::find_usages_in(
            &dir,
            &file,
            None,
            line,
            character,
            name,
            Duration::from_secs(60),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            found.source,
            idecraft_languages::find_usages::UsageSource::LanguageServer
        );
        let count = |f: &str| {
            found
                .usages
                .iter()
                .filter(|u| u.path == dir.join(f))
                .count()
        };
        assert_eq!(
            [
                count("app/prices.py"),
                count("main.py"),
                count("test_prices.py")
            ],
            expected,
            "{name}: {:?}",
            found.usages
        );
    }
    let references = locations_in(&dir, &file, None, 4, 6, LocationKind::References, WAIT)
        .await
        .unwrap();
    for other in ["main.py", "test_prices.py"] {
        assert!(
            references.iter().any(|l| l.path == dir.join(other)),
            "{other}: {references:?}"
        );
    }
    let plan = plan_rename_in(&dir, &file, None, 4, 6, "Article", WAIT)
        .await
        .unwrap();
    let edits: Vec<usize> = plan.files.iter().map(|f| f.edits.len()).collect();
    assert_eq!(edits, [2, 3, 3], "{plan:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0079/AK-7 (OBS-0034 L11): the structure of a Python file has no parameters and local
/// variables, and each function carries its parameter list; unsaved text counts.
#[tokio::test]
async fn pyright_structure_without_parameters_and_locals() {
    let Some(dir) = python_usages_project("structure") else {
        return;
    };
    let file = dir.join("fmt.py");
    std::fs::write(&file, "x = 1\n").unwrap();
    let text = "import os, sys\ndef messy( a,b = 2,*args,**kw ):\n  x=a+b\n  y = [ 1,2 , 3 ]\n  return x\nclass Shape :\n  sides=4\n  def __init__(self, n):\n    self.n = n\n  def area( self ):\n    return 0\nlam = lambda q:q*2\n";
    let symbols = idecraft_languages::insight::symbols_in(&dir, &file, Some(text), WAIT)
        .await
        .unwrap();
    let rows: Vec<String> = symbols
        .iter()
        .map(|s| {
            format!(
                "{}{} {}{}",
                "  ".repeat(s.depth),
                s.kind,
                s.name,
                s.detail.as_deref().unwrap_or_default()
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            "function messy(a, b=2, *args, **kw)",
            "class Shape",
            "  variable sides",
            "  method __init__(self, n)",
            "  method area(self)",
            // pyright lists an attribute assigned through `self` below its class.
            "  variable n",
            "variable lam",
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0079/AK-8 (OBS-0028 T1): the structure of a TypeScript file has no local variables, an
/// arrow function bound to a constant is a function, functions carry their parameter list and
/// members their annotated type; unsaved text counts.
#[tokio::test]
async fn typescript_structure_without_locals() {
    if !installed("typescript-language-server") {
        skip("typescript-language-server not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("idecraft-ts-structure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
    let file = dir.join("src/fmt.ts");
    std::fs::write(&file, "export {}\n").unwrap();
    let text = "import {readFileSync,writeFileSync} from \"fs\"\nexport function  messy( a:number,b = 2 ,...rest:string[] ){\n  const x=a+b*2\n  const y = [ 1,2 ,3 ]\n  function inner(q: string) { const z = q; return z }\n  if(x>3) return x\n  return   y.length\n}\n\nexport class  Shape {\n    sides=0\n    label: string = ''\n    area(): number { return 0.0 }\n    name( ) {\n        return 'shape'\n    }\n}\nexport const lam = (q:number)=>q+1\nexport const limit: number = 3\n";
    idecraft_core::trust::trust_for_process(&dir);
    let symbols = idecraft_languages::insight::symbols_in(&dir, &file, Some(text), WAIT)
        .await
        .unwrap();
    let mut rows: Vec<String> = symbols
        .iter()
        .map(|s| {
            format!(
                "{}{} {}{}",
                "  ".repeat(s.depth),
                s.kind,
                s.name,
                s.detail.as_deref().unwrap_or_default()
            )
        })
        .collect();
    // The server's order (by name) is not what is checked here.
    rows.sort();
    assert_eq!(
        rows,
        [
            "  function inner(q: string)",
            "  method area(): number",
            "  method name()",
            "  property label: string",
            "  property sides",
            "class Shape",
            "constant limit: number",
            "function lam(q: number)",
            "function messy(a: number, b?, ...rest: string[])",
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0329/AK-5 (OBS-0034 L10): without ruff the unused imports pyright reports are removed
/// from unsaved text, a used name of the same statement stays.
#[tokio::test]
async fn pyright_unused_imports_are_removed() {
    use idecraft_languages::python_tools::language_server_imports_in;
    let Some(dir) = python_usages_project("imports") else {
        return;
    };
    let file = dir.join("fmt.py");
    std::fs::write(&file, "x = 1\n").unwrap();
    let text = "import os, sys\nimport json, re\nfrom app.prices import Item\n\n\ndef f(a, unused):\n    return re.compile(a)\n";
    let done = language_server_imports_in(&dir, &file, text, WAIT)
        .await
        .unwrap();
    assert_eq!(
        done.text,
        "import re\n\n\ndef f(a, unused):\n    return re.compile(a)\n"
    );
    assert_eq!(done.removed, ["os", "sys", "json", "Item"]);
    assert!(done.left.is_empty(), "{:?}", done.left);
    assert_eq!((done.by, done.sorted), ("language server", false));
    // A package's `__init__.py` is refused with the reason.
    let refused =
        language_server_imports_in(&dir, &dir.join("app/__init__.py"), "import os\n", WAIT)
            .await
            .unwrap_err();
    assert!(refused.contains("__init__.py"), "{refused}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0072/AK-28 (OBS-0034 L7): pyright names no origin with its completion items, so the class
/// a member comes from is worked out from the receiver's type definition and the classes'
/// sources; resolving within a budget fills in the declarations.
#[tokio::test]
async fn pyright_completion_names_the_origin() {
    use idecraft_languages::insight::resolve_details_within;
    use idecraft_languages::python_origin::{add_origins_in, member_origins_in};
    let Some(dir) = python_usages_project("origin") else {
        return;
    };
    let probe = dir.join("probe.py");
    let probe_text =
        "import os\n\nfrom app.prices import Item\n\n\nitem = Item(\"pen\", 1.5, 4)\nitem.\nos.\n";
    std::fs::write(&probe, probe_text).unwrap();
    let shapes = dir.join("shapes.py");
    let shapes_text = "from app.prices import Item\n\n\nclass Base:\n    kind = \"base\"\n\n    def area(self) -> float:\n        return 0.0\n\n\nclass Special(Item, Base):\n    def __init__(self) -> None:\n        super().__init__(\"x\", 1.0)\n        self.extra = 1\n\n\nclass Oops(ValueError):\n    pass\n\n\ndef use(s: Special, e: Oops, u: Special | Oops):\n    s.\n    e.\n    u.\n";
    std::fs::write(&shapes, shapes_text).unwrap();

    // A dataclass from another file: its own members, the ones of `object`, and none for the
    // methods the decorator generates.
    let mut items = complete_in(&dir, &probe, None, 6, 5, 5000, WAIT)
        .await
        .unwrap();
    add_origins_in(&dir, &probe, None, 6, 5, &mut items, WAIT)
        .await
        .unwrap();
    let origin = |items: &[idecraft_languages::insight::CompletionItem], label: &str| {
        items
            .iter()
            .find(|i| i.label == label)
            .unwrap_or_else(|| panic!("no {label}"))
            .origin
            .clone()
    };
    for own in ["name", "price", "qty", "total"] {
        assert_eq!(origin(&items, own).as_deref(), Some("Item"), "{own}");
    }
    for inherited in ["__class__", "__sizeof__", "__doc__"] {
        assert_eq!(
            origin(&items, inherited).as_deref(),
            Some("object"),
            "{inherited}"
        );
    }
    assert_eq!(origin(&items, "__eq__"), None);
    // IDECraft's own templates after the dot have no origin.
    assert!(
        items
            .iter()
            .filter(|i| i.kind.as_deref() == Some("snippet"))
            .all(|i| i.origin.is_none())
    );

    // Resolving within a budget: the declaration of what the time reached, in order.
    let members: Vec<_> = items
        .iter()
        .filter(|i| ["name", "total", "__eq__"].contains(&i.label.as_str()))
        .cloned()
        .collect();
    let resolved = resolve_details_within(
        &dir,
        &probe,
        None,
        &members,
        WAIT,
        Some(Duration::from_secs(30)),
    )
    .await
    .unwrap();
    let details: Vec<_> = resolved
        .iter()
        .map(|r| r.as_ref().and_then(|r| r.detail.clone()))
        .collect();
    assert_eq!(
        details,
        [
            Some("name: str".to_owned()),
            Some("def total() -> float".to_owned()),
            // pyright has no declaration for a method the dataclass generates.
            None
        ]
    );
    let none = resolve_details_within(&dir, &probe, None, &members, WAIT, Some(Duration::ZERO))
        .await
        .unwrap();
    assert_eq!(none, [None, None, None]);

    // Several bases, all readable: the class, its bases in order, then `object`.
    let h = member_origins_in(&dir, &shapes, None, 21, 6, WAIT)
        .await
        .unwrap()
        .expect("hierarchy of Special");
    assert_eq!(h.origin("extra"), Some("Special"));
    assert_eq!(h.origin("__init__"), Some("Special"));
    assert_eq!(h.origin("total"), Some("Item"));
    assert_eq!(h.origin("area"), Some("Base"));
    assert_eq!(h.origin("kind"), Some("Base"));
    assert_eq!(h.origin("__class__"), Some("object"));
    assert_eq!(h.origin("no_such_member"), None);

    // A chain into the standard library's stubs.
    let h = member_origins_in(&dir, &shapes, None, 22, 6, WAIT)
        .await
        .unwrap()
        .expect("hierarchy of Oops");
    assert_eq!(h.origin("args"), Some("BaseException"));
    assert_eq!(h.origin("with_traceback"), Some("BaseException"));
    assert_eq!(h.origin("__class__"), Some("object"));

    // A union and a module have no single class.
    assert_eq!(
        member_origins_in(&dir, &shapes, None, 23, 6, WAIT)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        member_origins_in(&dir, &probe, None, 7, 3, WAIT)
            .await
            .unwrap(),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0043/AK-7: the HTML, CSS and JSON servers of vscode-langservers-extracted format a file —
/// also SCSS and Less — and a range; before v4 they were never asked to and answered that they
/// cannot format.
#[tokio::test]
async fn vscode_servers_format_files_and_ranges() {
    use idecraft_languages::format::{FormatOptions, plan_format, plan_format_in};
    use idecraft_languages::insight::Range;
    if !installed("vscode-css-language-server") {
        skip("vscode-langservers-extracted not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-web-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let options = FormatOptions::default();
    for (name, ugly, formatted) in [
        (
            "data.json",
            "{\"name\":\"lab\",\"tags\":[\"a\",\n\"b\"]}\n",
            "{\n    \"name\": \"lab\",\n    \"tags\": [\n        \"a\",\n        \"b\"\n    ]\n}\n",
        ),
        (
            "site.css",
            ".a,.b{margin:0;\n      padding:0}\n",
            ".a,\n.b {\n    margin: 0;\n    padding: 0\n}\n",
        ),
        (
            "site.scss",
            "$gap:4px;\n.card{color:red;.title{padding:$gap}}\n",
            "$gap: 4px;\n\n.card {\n    color: red;\n\n    .title {\n        padding: $gap\n    }\n}\n",
        ),
        (
            "site.less",
            "@gap:4px;\n.card{color:red;.title{padding:@gap}}\n",
            "@gap: 4px;\n\n.card {\n    color: red;\n\n    .title {\n        padding: @gap\n    }\n}\n",
        ),
        (
            "page.html",
            "<html><body><div><p>one</p>\n      <p>two</p></div></body></html>\n",
            // v5: no blank lines around `body` (behaviour 14, OBS-0036 FM2).
            "<html>\n<body>\n    <div>\n        <p>one</p>\n        <p>two</p>\n    </div>\n</body>\n</html>\n",
        ),
    ] {
        let file = root.join(name);
        std::fs::write(&file, ugly).unwrap();
        let plan = plan_format(&root, &file, None, options, WAIT)
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(plan.formatted, formatted, "{name}");
        assert!(plan.whitespace_only, "{name}");
        // Formatting again changes nothing.
        let again = plan_format_in(&root, &file, Some(formatted), None, options, WAIT)
            .await
            .unwrap();
        assert!(again.edits.edits.is_empty(), "{name}: {again:?}");
    }

    // A range: only the second of two objects is laid out.
    let file = root.join("two.json");
    let ugly = "[\n{\"a\":1},\n{\"b\":2}\n]\n";
    std::fs::write(&file, ugly).unwrap();
    let second = Range {
        start_line: 2,
        start_character: 0,
        end_line: 2,
        end_character: 7,
    };
    let plan = plan_format(&root, &file, Some(&second), options, WAIT)
        .await
        .unwrap();
    assert!(plan.formatted.starts_with("[\n{\"a\":1},\n"), "{plan:?}");
    assert!(plan.formatted.contains("\"b\": 2"), "{plan:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0043/AK-8a, AK-12 and AK-13 with real servers (OBS-0036 FM1, FM2, FM3): in a project
/// without `.editorconfig` settings JSON, SCSS, Less and YAML are laid out with two spaces and CSS
/// with four; typescript-language-server formats with the indentation asked for, not with the
/// one the unformatted file happens to have; lemminx closes an empty element without a space.
#[tokio::test]
async fn formatting_with_the_defaults_per_language() {
    use idecraft_languages::format::{FormatOptions, plan_format, plan_format_in};
    let root =
        std::env::temp_dir().join(format!("idecraft-format-defaults-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    // Nothing above the folder decides.
    std::fs::write(root.join(".editorconfig"), "root = true\n").unwrap();
    let style = idecraft_core::CodeStyleSettings::default();
    let cases: [(&str, &str, &str, &str); 8] = [
        (
            "vscode-json-language-server",
            "data.json",
            "{\"name\":\"lab\",\"tags\":[\"a\"]}\n",
            "{\n  \"name\": \"lab\",\n  \"tags\": [\n    \"a\"\n  ]\n}\n",
        ),
        (
            "vscode-css-language-server",
            "theme.scss",
            "$gap:4px;\n.card{color:red;.title{padding:$gap}}\n",
            "$gap: 4px;\n\n.card {\n  color: red;\n\n  .title {\n    padding: $gap\n  }\n}\n",
        ),
        (
            "vscode-css-language-server",
            "theme.less",
            "@gap:4px;\n.card{color:red;.title{padding:@gap}}\n",
            "@gap: 4px;\n\n.card {\n  color: red;\n\n  .title {\n    padding: @gap\n  }\n}\n",
        ),
        (
            "vscode-css-language-server",
            "theme.css",
            ".a{margin:0}\n",
            ".a {\n    margin: 0\n}\n",
        ),
        (
            "yaml-language-server",
            "config.yaml",
            "name:   lab\nservers:\n- host: one\n  nested:\n        deep: yes\n",
            "name: lab\nservers:\n  - host: one\n    nested:\n      deep: yes\n",
        ),
        // The function's line is indented by six: that is what the server would format with
        // if it heard the file's own indentation (before v5 it sometimes did).
        (
            "typescript-language-server",
            "app.ts",
            "function area(w:number,h:number):number{\n      return w*h }\nconst list=[1,\n2]\n",
            "function area(w: number, h: number): number {\n    return w * h\n}\nconst list = [1,\n    2]\n",
        ),
        (
            "typescript-language-server",
            "app.js",
            "function area(w,h){\n      return w*h }\n",
            "function area(w, h) {\n    return w * h\n}\n",
        ),
        (
            "lemminx",
            "layout.xml",
            "<root><item id=\"1\"><nested a=\"b\"/></item></root>\n",
            "<root>\n    <item id=\"1\">\n        <nested a=\"b\"/>\n    </item>\n</root>\n",
        ),
    ];
    for (server, name, ugly, formatted) in cases {
        if !installed(server) {
            skip(&format!("{server} not installed"));
            continue;
        }
        let file = root.join(name);
        std::fs::write(&file, ugly).unwrap();
        let options = FormatOptions::for_file(&file, &style);
        // More than once: the TypeScript server's answer depended on which of two messages
        // reached it first.
        for round in 0..3 {
            let plan = plan_format(&root, &file, None, options, WAIT)
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(plan.formatted, formatted, "{name}, round {round}");
        }
        let again = plan_format_in(&root, &file, Some(formatted), None, options, WAIT)
            .await
            .unwrap();
        assert!(again.edits.edits.is_empty(), "{name}: {again:?}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0043/AK-5 with real servers: groovy-language-server has no formatter and the answer says
/// so; gopls formats files but no selection (v8: a selection through the whole file, as
/// kotlin-language-server, AK-15); bash-language-server without shfmt is not asked, and
/// the answer names shfmt instead of "already formatted". v5: Markdown needs no server.
#[tokio::test]
async fn servers_that_cannot_format_say_why() {
    use idecraft_languages::format::{FormatOptions, FormatPlan, plan_format};
    use idecraft_languages::insight::Range;
    use idecraft_languages::lsp::LspError;
    let root = std::env::temp_dir().join(format!("idecraft-no-format-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let options = FormatOptions::default();
    let refused = |answer: Result<FormatPlan, LspError>| match answer {
        Err(LspError::Refused(why)) => why,
        other => panic!("not refused: {other:?}"),
    };

    // No server is needed to know these.
    for (name, start) in [
        ("notes.txt", "No formatter for notes.txt: "),
        // The SQL formatter is not reached through a language server (v5 behaviour 11).
        (
            "query.sql",
            "SQL files are formatted by IDECraft's own SQL formatter",
        ),
    ] {
        let file = root.join(name);
        std::fs::write(&file, "select  1;\n").unwrap();
        let why = refused(plan_format(&root, &file, None, options, WAIT).await);
        assert!(why.starts_with(start), "{why}");
    }

    // v5 behaviour 12: Markdown is formatted by IDECraft, with or without marksman.
    let file = root.join("README.md");
    std::fs::write(&file, "# Title\n* one\n").unwrap();
    let plan = plan_format(&root, &file, None, options, WAIT)
        .await
        .unwrap();
    assert_eq!(plan.formatted, "# Title\n\n* one\n");

    if installed("groovy-language-server") {
        let file = root.join("Shapes.groovy");
        std::fs::write(&file, "class Shapes{int a\n}\n").unwrap();
        let why = refused(plan_format(&root, &file, None, options, WAIT).await);
        assert_eq!(
            why,
            "groovy-language-server has no formatter, so Groovy files are not reformatted."
        );
    } else {
        skip("groovy-language-server not installed");
    }

    // SPEC-0043 v8 behaviour 15: gopls formats no ranges, so a selection goes through the whole
    // file; the lines after it stay as they are.
    if installed("gopls") {
        std::fs::write(root.join("go.mod"), "module lab\n\ngo 1.22\n").unwrap();
        let file = root.join("main.go");
        let ugly = "package main\nfunc   a(){\nx:=1\n_ = x\n}\nfunc   b(){\n}\n";
        std::fs::write(&file, ugly).unwrap();
        let first = Range {
            start_line: 1,
            start_character: 0,
            end_line: 5,
            end_character: 0,
        };
        let plan = plan_format(&root, &file, Some(&first), options, WAIT)
            .await
            .unwrap();
        assert_eq!(
            plan.formatted,
            "package main\n\nfunc a() {\n\tx := 1\n\t_ = x\n}\nfunc   b(){\n}\n"
        );
        assert!(plan.whitespace_only);
        assert_eq!(plan.edits.edits.len(), 1);
        // The same lines again: nothing left to do.
        std::fs::write(&file, &plan.formatted).unwrap();
        let again = Range {
            start_line: 2,
            start_character: 0,
            end_line: 6,
            end_character: 0,
        };
        let plan = plan_format(&root, &file, Some(&again), options, WAIT)
            .await
            .unwrap();
        assert!(plan.edits.edits.is_empty(), "{:?}", plan.edits);
        let plan = plan_format(&root, &file, None, options, WAIT)
            .await
            .unwrap();
        assert_eq!(
            plan.formatted,
            "package main\n\nfunc a() {\n\tx := 1\n\t_ = x\n}\nfunc b() {\n}\n"
        );
    } else {
        skip("gopls not installed");
    }

    // SPEC-0043 v8 behaviour 15: kotlin-language-server's own range formatting split `Int` at the
    // end of a selection inside a line and lost the indentation of a member; through the whole
    // file the selected lines get the whole file's layout.
    if installed("kotlin-language-server") {
        let file = root.join("Shop.kt");
        let ugly = "package demo\nclass Shop {\nfun total(price:Int,qty:Int):Int{\nval sum=price*qty\n      if(sum>10){return sum}\nreturn 0}\nfun other(){}\n}\n";
        std::fs::write(&file, ugly).unwrap();
        let inside = Range {
            start_line: 2,
            start_character: 0,
            end_line: 2,
            end_character: 30,
        };
        let plan = plan_format(&root, &file, Some(&inside), options, WAIT)
            .await
            .unwrap();
        let lines: Vec<&str> = plan.formatted.lines().collect();
        assert_eq!(
            lines[2], "    fun total(price: Int, qty: Int): Int {",
            "{}",
            plan.formatted
        );
        assert_eq!(&lines[3..], &ugly.lines().collect::<Vec<_>>()[3..]);
        let block = Range {
            start_line: 3,
            start_character: 0,
            end_line: 5,
            end_character: 0,
        };
        let plan = plan_format(&root, &file, Some(&block), options, WAIT)
            .await
            .unwrap();
        assert!(plan.whitespace_only);
        assert!(
            plan.formatted.starts_with(
                "package demo\nclass Shop {\nfun total(price:Int,qty:Int):Int{\n        val sum = price * qty\n        if (sum > 10) {\n"
            ),
            "{}",
            plan.formatted
        );
        assert!(
            plan.formatted
                .ends_with("        }\nreturn 0}\nfun other(){}\n}\n"),
            "{}",
            plan.formatted
        );
    } else {
        skip("kotlin-language-server not installed");
    }

    if installed("bash-language-server") {
        let file = root.join("build.sh");
        let ugly = "#!/bin/bash\nif true;then\necho a\nfi\n";
        std::fs::write(&file, ugly).unwrap();
        let answer = plan_format(&root, &file, None, options, WAIT).await;
        if installed("shfmt") {
            let plan = answer.unwrap();
            assert_ne!(plan.formatted, ugly, "{plan:?}");
        } else {
            let why = refused(answer);
            assert!(why.contains("shfmt was not found on PATH"), "{why}");
        }
    } else {
        skip("bash-language-server not installed");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A project folder of its own for a live test.
fn live_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-live-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, content) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }
    root.canonicalize().unwrap()
}

/// SPEC-0042/AK-26 (v16): intelephense is asked after its indexing: a method declared in another file
/// has its documentation, and the diagnostics are those of the indexed project.
#[tokio::test]
async fn intelephense_is_asked_after_its_indexing() {
    if !installed("intelephense") {
        skip("intelephense not installed");
        return;
    }
    let root = live_project(
        "php",
        &[
            (
                "src/Invoice.php",
                "<?php\n\nnamespace App;\n\nclass Invoice\n{\n    /** Adds a line in cents. */\n    public function addLine(int $cents): void\n    {\n    }\n}\n",
            ),
            (
                "src/Report.php",
                "<?php\n\nnamespace App;\n\nfunction report(): void\n{\n    $invoice = new Invoice();\n    $invoice->addLine(1200);\n    $invoice->missingMethod();\n}\n",
            ),
        ],
    );
    let file = root.join("src/Report.php");
    let hover = hover_in(&root, &file, None, 7, 16, WAIT)
        .await
        .unwrap()
        .expect("a hover for a method of another file");
    assert!(hover.text.contains("addLine"), "{}", hover.text);
    let found = idecraft_languages::lsp::file_diagnostics(&root, &file, WAIT)
        .await
        .unwrap();
    assert!(
        found.iter().any(|d| d.message.contains("missingMethod")),
        "{found:?}"
    );
    // The free server cannot rename, and says so (a licensed one renames).
    if let Err(error) = plan_rename_in(&root, &file, None, 7, 16, "appendLine", WAIT).await {
        assert_eq!(
            error.to_string(),
            "intelephense does not offer renaming (textDocument/rename)"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0042/AK-25 (v16): clangd is asked after it has parsed the file, so the members of a class are
/// completed, not the words of the file.
#[tokio::test]
async fn clangd_completes_members_after_parsing() {
    if !installed("clangd") {
        skip("clangd not installed");
        return;
    }
    let text = "struct Box {\n    int width;\n    int height() const { return 2; }\n};\n\nint area(const Box &box) {\n    int unrelated = 1;\n    return unrelated + box.;\n}\n";
    let root = live_project("cpp", &[("box.cpp", text)]);
    let file = root.join("box.cpp");
    let items = complete_in(&root, &file, None, 7, 27, 50, WAIT)
        .await
        .unwrap();
    let labels: Vec<(&str, &str)> = items
        .iter()
        .map(|i| (i.label.as_str(), i.kind.as_deref().unwrap_or_default()))
        .collect();
    assert!(labels.contains(&("width", "field")), "{labels:?}");
    assert!(labels.contains(&("height", "method")), "{labels:?}");
    assert!(
        !labels.iter().any(|(label, _)| *label == "unrelated"),
        "{labels:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0042/AK-27 (v16): the CSS server gets its validation settings and reports problems.
#[tokio::test]
async fn css_server_reports_problems() {
    if !installed("vscode-css-language-server") {
        skip("vscode-langservers-extracted not installed");
        return;
    }
    let root = live_project(
        "css",
        &[
            ("site.css", ".card {\n  colr: red;\n}\n"),
            ("site.scss", "$gap: 2px;\n.card {\n  colr: red;\n}\n"),
        ],
    );
    for name in ["site.css", "site.scss"] {
        let found = idecraft_languages::lsp::file_diagnostics(&root, &root.join(name), WAIT)
            .await
            .unwrap();
        assert!(
            found.iter().any(|d| d.message.contains("colr")),
            "{name}: {found:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0291/AK-6: gopls is asked for parameter names and gives them before literal arguments.
#[tokio::test]
async fn gopls_parameter_hints() {
    if !installed("gopls") {
        skip("gopls not installed");
        return;
    }
    let root = live_project(
        "go",
        &[
            ("go.mod", "module hints\n\ngo 1.21\n"),
            (
                "main.go",
                "package main\n\nfunc scaled(value float64, factor float64) float64 {\n\treturn value * factor\n}\n\nfunc main() {\n\t_ = scaled(1.5, 2.5)\n}\n",
            ),
        ],
    );
    let hints =
        idecraft_languages::inlay::parameter_hints_in(&root, &root.join("main.go"), None, WAIT)
            .await
            .unwrap();
    let labels: Vec<&str> = hints.iter().map(|h| h.label.as_str()).collect();
    assert_eq!(labels, ["value:", "factor:"], "{hints:?}");
    assert!(hints.iter().all(|h| h.line == 7), "{hints:?}");
    let _ = std::fs::remove_dir_all(&root);
}

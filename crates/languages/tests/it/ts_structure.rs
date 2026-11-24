//! SPEC-0320 with a real typescript-language-server: type hierarchy, usage counts, gutter marks and
//! class refactorings over several files. Skipped without the server.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_languages::actions::{Refactoring, code_actions_in, plan_code_action_in};
use ferris_idea_languages::insight::CallNode;
use ferris_idea_languages::java_structure::TypeDirection;
use ferris_idea_languages::ts_refactor::{
    extract_interface_in, pull_members_up_in, pull_targets_in,
};
use ferris_idea_languages::ts_structure::{MarkKind, declaration_marks_in, type_hierarchy_in};
use ferris_idea_languages::usages::usage_counts_in;

const BASE: &str = "export interface Shape {\n  area(): number;\n}\n\nexport abstract class Base implements Shape {\n  abstract area(): number;\n\n  describe(): string {\n    return String(this.area());\n  }\n\n  private hidden(): void {}\n\n  static make(): void {}\n}\n";
const SHAPES: &str = "import { Base as Root } from \"./base\";\n\nexport class Circle extends Root {\n  constructor(readonly r: number) {\n    super();\n  }\n\n  area() {\n    return this.r * this.r * 3;\n  }\n\n  override describe(): string {\n    return \"circle\";\n  }\n\n  hidden2 = 1;\n}\n\nexport class Square extends Root {\n  area(): number {\n    return 4;\n  }\n}\n";
const USE: &str = "import { Circle } from \"./shapes\";\n\nexport function unused(): void {}\n\nexport const shown = new Circle(1).describe() + new Circle(2).describe();\n";

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(name: &str) -> Option<Project> {
    // Only typescript-language-server: TypeScript 7's own server is checked in servers.rs.
    if !ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::TypeScript)
        .is_ok_and(|r| r.candidate.program == "typescript-language-server")
    {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: typescript-language-server"
        );
        eprintln!("skipped: typescript-language-server not installed");
        return None;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-ts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        "{\"compilerOptions\": {\"strict\": true, \"target\": \"es2022\", \"module\": \"commonjs\"}, \"include\": [\"src\"]}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/base.ts"), BASE).unwrap();
    std::fs::write(root.join("src/shapes.ts"), SHAPES).unwrap();
    std::fs::write(root.join("src/use.ts"), USE).unwrap();
    Some(Project(root))
}

const WAIT: Duration = Duration::from_secs(90);

fn names(n: &CallNode) -> Vec<String> {
    let mut v: Vec<String> = n.children.iter().map(|c| c.name.clone()).collect();
    v.sort();
    v
}

fn line_of(text: &str, needle: &str) -> u64 {
    text.lines().position(|l| l.contains(needle)).unwrap() as u64
}

/// SPEC-0320 AK-1.
#[tokio::test]
async fn type_hierarchy_over_files() {
    let Some(p) = project("hier") else { return };
    let root = &p.0;
    let shapes = root.join("src/shapes.ts");
    // From inside Circle's body.
    let tree = type_hierarchy_in(root, &shapes, None, 8, 6, TypeDirection::Class, WAIT)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tree.name, "Base");
    assert_eq!(names(&tree), ["Circle"], "only the chain leading down");
    assert_eq!(tree.children[0].mark.as_deref(), Some("*"));
    let base = root.join("src/base.ts");
    let at = line_of(BASE, "abstract class Base");
    let sub = type_hierarchy_in(root, &base, None, at, 25, TypeDirection::Subtypes, WAIT)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        names(&sub),
        ["Circle", "Square"],
        "through the renamed import"
    );
    let up = type_hierarchy_in(root, &shapes, None, 2, 15, TypeDirection::Supertypes, WAIT)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(names(&up), ["Base"]);
    assert_eq!(names(&up.children[0]), ["Shape"]);
    let interface = type_hierarchy_in(root, &base, None, 0, 20, TypeDirection::Class, WAIT)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(interface.name, "Shape");
    assert_eq!(names(&interface), ["Base"]);
    assert_eq!(
        type_hierarchy_in(
            root,
            &root.join("src/use.ts"),
            None,
            2,
            3,
            TypeDirection::Class,
            WAIT
        )
        .await
        .unwrap(),
        None
    );
}

/// SPEC-0320 AK-2.
#[tokio::test]
async fn usage_counts_with_members_and_inheritors() {
    let Some(p) = project("uses") else { return };
    let root = &p.0;
    let shapes = root.join("src/shapes.ts");
    let counts = usage_counts_in(root, &root.join("src/base.ts"), None, WAIT)
        .await
        .unwrap();
    let get = |n: &str| counts.iter().find(|c| c.name == n).unwrap();
    assert_eq!(get("Base").inheritors, 2, "{counts:?}");
    assert!(
        get("Base").label().ends_with(" · 2 inheritors"),
        "{}",
        get("Base").label()
    );
    // Two calls; the method that overrides it is no usage (SPEC-0320, OBS-0028 T8).
    assert_eq!(get("describe").count, 2, "{counts:?}");
    assert_eq!(get("describe").inheritors, 1, "{counts:?}");
    // SPEC-0320 v5 AK-15: what is below a member. `area` is declared twice in this file: the
    // interface's is implemented by `Base`, `Circle` and `Square`, `Base`'s abstract one by the
    // two subclasses.
    let area_at = |needle: &str| {
        let line = line_of(BASE, needle);
        counts
            .iter()
            .find(|c| c.name == "area" && c.line == line)
            .unwrap()
            .label()
    };
    assert!(
        area_at("  area(): number;").ends_with(" · 3 implementations"),
        "{}",
        area_at("  area(): number;")
    );
    assert!(
        area_at("abstract area").ends_with(" · 2 implementations"),
        "{}",
        area_at("abstract area")
    );
    assert!(
        get("describe").label().ends_with(" · 1 override"),
        "{}",
        get("describe").label()
    );
    assert_eq!(get("hidden").inheritors, 0);
    assert_eq!(get("hidden").label(), "no usages");
    let use_counts = usage_counts_in(root, &root.join("src/use.ts"), None, WAIT)
        .await
        .unwrap();
    let found: Vec<_> = use_counts
        .iter()
        .map(|c| (c.name.as_str(), c.kind))
        .collect();
    assert!(found.contains(&("unused", "function")), "{found:?}");
    assert_eq!(
        use_counts
            .iter()
            .find(|c| c.name == "unused")
            .unwrap()
            .label(),
        "no usages"
    );
    let shape_counts = usage_counts_in(root, &shapes, None, WAIT).await.unwrap();
    assert!(
        !shape_counts.iter().any(|c| c.name == "constructor"),
        "{shape_counts:?}"
    );
    assert!(
        shape_counts
            .iter()
            .any(|c| c.name == "hidden2" && c.kind == "property"),
        "{shape_counts:?}"
    );
}

/// SPEC-0320 AK-3.
#[tokio::test]
async fn declaration_marks_over_files() {
    let Some(p) = project("marks") else { return };
    let root = &p.0;
    let shapes = root.join("src/shapes.ts");
    let base = root.join("src/base.ts");
    let marks = declaration_marks_in(root, &base, None, WAIT).await.unwrap();
    let on = |name: &str, kind: MarkKind| marks.iter().any(|m| m.name == name && m.kind == kind);
    assert!(on("Base", MarkKind::Inherited), "{marks:?}");
    assert!(on("area", MarkKind::Implemented), "{marks:?}");
    assert!(on("area", MarkKind::Implements), "{marks:?}");
    assert!(on("describe", MarkKind::Overridden), "{marks:?}");
    assert!(
        !marks.iter().any(|m| m.name == "hidden" || m.name == "make"),
        "{marks:?}"
    );
    let down = marks
        .iter()
        .find(|m| m.name == "area" && m.kind == MarkKind::Implemented && m.line == 5)
        .unwrap();
    assert_eq!(down.targets.len(), 2, "{down:?}");
    let sub = declaration_marks_in(root, &shapes, None, WAIT)
        .await
        .unwrap();
    assert!(
        sub.iter()
            .any(|m| m.name == "describe" && m.kind == MarkKind::Overrides),
        "{sub:?}"
    );
    assert!(
        sub.iter()
            .any(|m| m.name == "area" && m.kind == MarkKind::Implements),
        "{sub:?}"
    );
}

/// SPEC-0320 AK-4.
#[tokio::test]
async fn extract_field_with_the_server() {
    let Some(p) = project("field") else { return };
    let root = &p.0;
    let file = root.join("src/field.ts");
    let text = "export class F {\n  m(): number {\n    return 40 + 2;\n  }\n}\n";
    std::fs::write(&file, text).unwrap();
    let range = ferris_idea_languages::insight::Range {
        start_line: 2,
        start_character: 11,
        end_line: 2,
        end_character: 17,
    };
    let actions = code_actions_in(root, &file, Some(text), &range, WAIT)
        .await
        .unwrap();
    let field = actions
        .iter()
        .find(|a| Refactoring::Field.matches(a))
        .unwrap_or_else(|| panic!("no field action: {actions:?}"));
    let files = plan_code_action_in(root, &file, Some(text), &range, &field.title, WAIT)
        .await
        .unwrap();
    let out = ferris_idea_languages::rename::apply_edits(text, &files[0].edits).unwrap();
    assert!(out.contains("readonly"), "{out}");
    assert!(out.contains("this."), "{out}");
}

/// SPEC-0041 v3 AK-6: the request says that it was invoked, so typescript-language-server offers
/// Inline variable with the caret on a use of the variable (it reports it as disabled to a
/// request without a trigger kind), and the action applies. Refactorings that do not apply at
/// the place come with the server's reason.
#[tokio::test]
async fn inline_variable_is_applicable_on_a_use() {
    let Some(p) = project("invoked") else { return };
    let root = &p.0;
    let file = root.join("src/inline.ts");
    let text =
        "export function f(a: number): number {\n  const y = [1, 2, a];\n  return y.length;\n}\n";
    std::fs::write(&file, text).unwrap();
    let caret = ferris_idea_languages::insight::Range {
        start_line: 2,
        start_character: 9,
        end_line: 2,
        end_character: 9,
    };
    let actions = code_actions_in(root, &file, Some(text), &caret, WAIT)
        .await
        .unwrap();
    let inline = actions
        .iter()
        .find(|a| a.title == "Inline variable")
        .unwrap_or_else(|| panic!("no Inline variable: {actions:?}"));
    assert_eq!(inline.disabled, None, "{actions:?}");
    let export = actions
        .iter()
        .find(|a| a.title == "Convert default export to named export")
        .unwrap_or_else(|| panic!("{actions:?}"));
    assert!(export.disabled.as_deref().is_some_and(|r| !r.is_empty()));
    let sorted = ferris_idea_languages::actions::applicable_first(actions.clone(), false);
    let first_disabled = sorted.iter().position(|a| a.disabled.is_some()).unwrap();
    assert!(
        sorted[first_disabled..]
            .iter()
            .all(|a| a.disabled.is_some())
    );
    assert!(
        ferris_idea_languages::actions::applicable_first(actions, true)
            .iter()
            .all(|a| a.disabled.is_none())
    );
    let files = plan_code_action_in(root, &file, Some(text), &caret, "Inline variable", WAIT)
        .await
        .unwrap();
    let out = ferris_idea_languages::rename::apply_edits(text, &files[0].edits).unwrap();
    assert!(out.contains("return [1, 2, a].length;"), "{out}");
    assert!(!out.contains("const y"), "{out}");
}

/// SPEC-0320 AK-5 and AK-7: the results have no type errors.
#[tokio::test]
async fn extract_interface_and_pull_up_are_valid_typescript() {
    let Some(p) = project("refactor") else { return };
    let root = &p.0;
    let shapes = root.join("src/shapes.ts");
    // `area` of Circle has no written return type: the hover gives it.
    let files = extract_interface_in(
        root,
        &shapes,
        None,
        2,
        15,
        "HasArea",
        &["area".to_owned()],
        WAIT,
    )
    .await
    .unwrap();
    let out = ferris_idea_languages::rename::apply_edits(SHAPES, &files[0].edits).unwrap();
    assert!(
        out.contains("export interface HasArea {\n  area(): number;\n}"),
        "{out}"
    );
    assert!(
        out.contains("class Circle extends Root implements HasArea {"),
        "{out}"
    );
    ferris_idea_languages::rename::apply_files(&files).unwrap();
    assert_no_errors(root, &shapes).await;
    // Pull an annotated method up across files.
    let more = format!(
        "{}\nexport class T extends Root {{\n  area(): number {{ return 1; }}\n  extra(): number {{ return 2; }}\n}}\n",
        std::fs::read_to_string(&shapes).unwrap()
    );
    std::fs::write(&shapes, &more).unwrap();
    let line = line_of(&more, "class T") + 1;
    let targets = pull_targets_in(root, &shapes, Some(&more), line, 5, WAIT)
        .await
        .unwrap();
    assert_eq!(targets, ["Base"]);
    let files = pull_members_up_in(
        root,
        &shapes,
        Some(&more),
        line,
        5,
        None,
        &["extra".to_owned()],
        false,
        WAIT,
    )
    .await
    .unwrap();
    assert_eq!(files.len(), 2, "{files:?}");
    ferris_idea_languages::rename::apply_files(&files).unwrap();
    let base = std::fs::read_to_string(root.join("src/base.ts")).unwrap();
    assert!(
        base.contains("  extra(): number { return 2; }\n}"),
        "{base}"
    );
    assert_no_errors(root, &shapes).await;
}

async fn assert_no_errors(root: &Path, file: &Path) {
    let problems = ferris_idea_languages::lsp::file_diagnostics(root, file, WAIT)
        .await
        .unwrap();
    let errors: Vec<_> = problems.iter().filter(|p| p.severity == "error").collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// SPEC-0320 v5 AK-14, v6: through the entry points the window and MCP use, with or without a
/// language server, the hierarchy and the marks of a project with an aliased import. The project
/// has no `tsconfig.json`: a server would know only the open file, so the structure comes from
/// the syntax trees (v6).
#[tokio::test]
async fn structure_through_the_entry_points_with_or_without_a_server() {
    let root = std::env::temp_dir().join(format!("ferris-idea-ts-entry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    let _cleanup = Project(root.clone());
    std::fs::write(root.join("src/base.ts"), BASE).unwrap();
    std::fs::write(root.join("src/shapes.ts"), SHAPES).unwrap();
    let base = root.join("src/base.ts");
    let at = line_of(BASE, "abstract class Base");
    let tree = type_hierarchy_in(&root, &base, None, at, 25, TypeDirection::Subtypes, WAIT)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(names(&tree), ["Circle", "Square"]);
    let marks = declaration_marks_in(&root, &base, None, WAIT)
        .await
        .unwrap();
    // `area` of `Base` implements `Shape` and is implemented by both subclasses.
    let area: Vec<_> = marks
        .iter()
        .filter(|m| m.name == "area" && m.line == line_of(BASE, "abstract area"))
        .map(|m| (m.kind, m.targets.len()))
        .collect();
    assert_eq!(
        area,
        [(MarkKind::Implements, 1), (MarkKind::Implemented, 2)],
        "{marks:?}"
    );
    assert!(
        marks
            .iter()
            .any(|m| m.name == "Base" && m.kind == MarkKind::Inherited)
    );
}

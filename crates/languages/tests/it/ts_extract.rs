//! SPEC-0095 v6 and SPEC-0320 v7 with real TypeScript servers: Extract Variable, Extract Constant
//! and Extract Method in a `.ts` file (OBS-0028 T4). With typescript-language-server the server's
//! own actions are found and applied, the name it introduced is found for the name popup; with a
//! server that offers no extraction (TypeScript 7's `tsc --lsp`) Ferris IDEA's own Extract Variable
//! does it. Skipped without the server each test needs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_languages::LanguageId;
use ferris_idea_languages::actions::{
    Refactoring, introduced_name, plan_code_action_in, refactoring_targets,
};
use ferris_idea_languages::insight::Range;
use ferris_idea_languages::rename::apply_edits;

/// The function of OBS-0028 item 4, unformatted as there, with the further cases of the task.
const FMT: &str = "export function f(x: number, a: number, b: number) {\n  if(x>3) return x\n  const s = a+b*2\n  const l = [1,2,3]\n  console.log(s, l)\n  return 0\n}\n";

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn server() -> Option<String> {
    ferris_idea_languages::servers::resolve(LanguageId::TypeScript)
        .ok()
        .map(|r| r.candidate.program.to_owned())
}

fn project(name: &str) -> Project {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-ts-extract-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        "{\"compilerOptions\": {\"strict\": true, \"target\": \"es2022\"}, \"include\": [\"src\"]}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/fmt.ts"), FMT).unwrap();
    Project(root)
}

fn range(line: u64, from: u64, to: u64) -> Range {
    Range {
        start_line: line,
        start_character: from,
        end_line: line,
        end_character: to,
    }
}

const WAIT: Duration = Duration::from_secs(90);

/// Applies the action titled `title` for `range` and returns the new text.
async fn applied(root: &Path, file: &Path, range: &Range, title: &str) -> String {
    let plan = plan_code_action_in(root, file, Some(FMT), range, title, WAIT)
        .await
        .unwrap_or_else(|e| panic!("{title}: {e}"));
    let edits = &plan
        .iter()
        .find(|f| f.path == file)
        .expect("edits of the file")
        .edits;
    apply_edits(FMT, edits).unwrap()
}

/// With typescript-language-server: each extraction of the task is found under its shortcut and
/// applied, and the introduced name is found for the name popup.
#[tokio::test(flavor = "multi_thread")]
async fn typescript_language_server_extracts_variables_constants_and_functions() {
    if server().as_deref() != Some("typescript-language-server") {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: typescript-language-server"
        );
        eprintln!("skipped: Ferris IDEA does not choose typescript-language-server here");
        return;
    }
    let p = project("tsls");
    let file = p.0.join("src/fmt.ts");
    let titles = async |r: &Range, what: Refactoring| -> Vec<String> {
        let targets = refactoring_targets(&p.0, &file, Some(FMT), r, what, WAIT)
            .await
            .unwrap();
        assert_eq!(targets.len(), 1, "{what:?} at {r:?}: {targets:?}");
        targets[0].actions.iter().map(|a| a.title.clone()).collect()
    };

    // OBS-0028 T4: `x>3` in `if(x>3) return x`.
    let x = range(1, 5, 8);
    assert_eq!(
        titles(&x, Refactoring::Variable).await,
        ["Extract to constant in enclosing scope"]
    );
    let after = applied(&p.0, &file, &x, "Extract to constant in enclosing scope").await;
    let name = introduced_name(FMT, &after).expect("the new name is found");
    assert!(
        after.contains(&format!("const {} = x>3", name.name))
            || after.contains(&format!("const {} = x > 3", name.name)),
        "{after}"
    );
    assert!(after.contains(&format!("if({})", name.name)), "{after}");
    // SPEC-0095 v7 behaviour 3a (OBS-0028 T7): the file indents by two spaces, so does the new line.
    assert!(
        after
            .lines()
            .any(|l| l.starts_with(&format!("  const {}", name.name))),
        "{after}"
    );

    // `a+b*2` and `[1,2,3]`: a variable, and a constant in either scope.
    assert_eq!(
        titles(&range(2, 12, 17), Refactoring::Variable).await,
        ["Extract to constant in enclosing scope"]
    );
    let constants = titles(&range(3, 12, 19), Refactoring::Constant).await;
    assert!(
        constants.contains(&"Extract to constant in module scope".to_owned()),
        "{constants:?}"
    );

    // A whole statement: a function, no variable.
    let statement = range(4, 2, 19);
    let functions = titles(&statement, Refactoring::Method).await;
    assert!(
        functions.contains(&"Extract to function in module scope".to_owned()),
        "{functions:?}"
    );
    let after = applied(
        &p.0,
        &file,
        &statement,
        "Extract to function in module scope",
    )
    .await;
    let name = introduced_name(FMT, &after).expect("the new function is found");
    assert!(
        after.contains(&format!("function {}(", name.name)),
        "{after}"
    );
    // The new function's body is indented by two spaces too.
    let body = after
        .split(&format!("function {}(", name.name))
        .nth(1)
        .unwrap_or_default();
    assert!(body.contains("\n  console.log"), "{after}");
    assert!(!body.contains("\n    console.log"), "{after}");
}

/// With any TypeScript server or none: Extract Variable on `x>3` works, by the server's action or
/// Ferris IDEA's own (TypeScript 7's `tsc --lsp` offers no extraction).
#[tokio::test(flavor = "multi_thread")]
async fn extract_variable_works_with_every_typescript_server() {
    let p = project("any");
    let file = p.0.join("src/fmt.ts");
    let x = range(1, 5, 8);
    let targets = refactoring_targets(&p.0, &file, Some(FMT), &x, Refactoring::Variable, WAIT)
        .await
        .unwrap_or_default();
    assert_eq!(targets.len(), 1, "server {:?}: {targets:?}", server());
    let title = targets[0].actions[0].title.clone();
    let after = applied(&p.0, &file, &x, &title).await;
    let name = introduced_name(FMT, &after).expect("the new name is found");
    assert!(after.contains(&format!("if({})", name.name)), "{after}");
    eprintln!("server {:?}: {title}\n{after}", server());
}

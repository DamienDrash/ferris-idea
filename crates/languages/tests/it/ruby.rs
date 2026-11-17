//! Ruby language servers for real (`SPEC-0185` AK-7): ruby-lsp started directly and solargraph
//! through `bundle exec` in a project that locks it: outline, hover and completion. ruby-lsp indexes
//! saved files only (`SPEC-0042` v6 behaviour 13), so its file is saved; solargraph's stays a stub.
//! Skipped cleanly when the servers are missing (`IDECRAFT_REQUIRE_TOOLS` makes that a failure).

use std::path::{Path, PathBuf};
use std::time::Duration;

use idecraft_languages::LanguageId;
use idecraft_languages::insight::{complete_in, hover_in, symbols_in};
use idecraft_languages::servers::resolve_for;

const WAIT: Duration = Duration::from_secs(120);

const TEXT: &str = "class Greeter\n  # Says hello to someone.\n  def greet(name)\n    \"Hello #{name}\"\n  end\nend\n\ngreeter = Greeter.new\ngreeter.greet(\"you\")\n";

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-ruby-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

async fn check(name: &str, root: &Path, saved: bool) {
    let file = root.join("greeter.rb");
    std::fs::write(&file, if saved { TEXT } else { "\n" }).unwrap();
    let text = Some(TEXT);

    let symbols = symbols_in(root, &file, text, WAIT).await.unwrap();
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    for want in ["Greeter", "greet"] {
        assert!(
            names.iter().any(|n| n.contains(want)),
            "{name}: {want} in {names:?}"
        );
    }

    // The first request already answers: ruby-lsp is asked after its indexing (SPEC-0042 v6).
    let hover = hover_in(root, &file, text, 7, 12, WAIT)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{name}: a hover"));
    assert!(hover.text.contains("Greeter"), "{name}: {}", hover.text);

    // ⌘B on `Greeter` of `Greeter.new` finds the class, once (hand test 110, D61).
    let found = idecraft_languages::insight::locations_in(
        root,
        &file,
        text,
        7,
        12,
        idecraft_languages::insight::LocationKind::Definition,
        WAIT,
    )
    .await
    .unwrap();
    let lines: Vec<u64> = found.iter().map(|l| l.range.start_line).collect();
    assert_eq!(lines, [0], "{name}: {found:?}");

    let typed = TEXT.replace("greeter.greet(\"you\")", "Gre");
    let items = complete_in(root, &file, Some(&typed), 8, 3, 200, WAIT)
        .await
        .unwrap();
    assert!(
        items.iter().any(|i| i.label.trim().starts_with("Greeter")),
        "{name}: Greeter in {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn ruby_lsp_directly() {
    let root = project("lsp");
    match resolve_for(LanguageId::Ruby, &root) {
        Ok(r) if r.candidate.program == "ruby-lsp" => {
            assert!(r.prefix_args.is_empty(), "{}", r.command_line());
        }
        _ => {
            skip("ruby-lsp not installed");
            return;
        }
    }
    check("ruby-lsp", &root, true).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn solargraph_through_bundler() {
    let root = project("bundle");
    std::fs::write(
        root.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"solargraph\"\n",
    )
    .unwrap();
    // The lockfile from the gems installed with the Ruby the workspace selects.
    let env = idecraft_core::ruby_environment(&idecraft_core::RubySearch::from_env(&root));
    let path = std::env::join_paths(
        env.server_path.iter().map(PathBuf::from).chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths),
        ),
    )
    .unwrap();
    let locked = std::process::Command::new("bundle")
        .args(["lock", "--local"])
        .env("PATH", &path)
        .current_dir(&root)
        .output()
        .is_ok_and(|o| o.status.success());
    if !locked {
        skip("bundle or solargraph not installed");
        return;
    }
    let r = resolve_for(LanguageId::Ruby, &root).unwrap();
    assert_eq!(
        r.args(),
        ["exec", "solargraph", "stdio"],
        "{}",
        r.command_line()
    );
    assert!(r.command.ends_with("bundle"));
    check("solargraph", &root, false).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0042 v25 AK-48: ruby-lsp gives a file's problems only when asked for them
/// (`textDocument/diagnostic`) and publishes none; asked so, a syntax error is found at once, and
/// a file without problems has an empty list long before the wait is over (before v25 every Ruby
/// file waited the whole wait and got an empty list, also with a syntax error).
#[tokio::test]
async fn ruby_lsp_reports_problems_when_asked() {
    let root = project("diagnostics");
    match resolve_for(LanguageId::Ruby, &root) {
        Ok(r) if r.candidate.program == "ruby-lsp" => {}
        _ => {
            skip("ruby-lsp not installed");
            return;
        }
    }
    let broken = root.join("broken.rb");
    std::fs::write(&broken, "class Broken\n  def area(\n    1 +\n  end\n").unwrap();
    let good = root.join("greeter.rb");
    std::fs::write(&good, TEXT).unwrap();
    let wait = Duration::from_secs(60);
    let started = std::time::Instant::now();
    let found = idecraft_languages::lsp::file_diagnostics(&root, &broken, wait)
        .await
        .unwrap();
    assert!(found.iter().any(|d| d.severity == "error"), "{found:?}");
    let found = idecraft_languages::lsp::file_diagnostics(&root, &good, wait)
        .await
        .unwrap();
    assert!(found.is_empty(), "{found:?}");
    assert!(
        started.elapsed() < Duration::from_secs(45),
        "two files took {:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_dir_all(&root);
}

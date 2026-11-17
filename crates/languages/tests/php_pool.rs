//! SPEC-0042 v25 AK-47 (OBS-0081, F-038): a PHP file opened in the pooled intelephense after its
//! indexing has ended gets its diagnostics at once. intelephense publishes such a file's
//! diagnostics once, checked against the finished index; before v25 the question waited 30 s for
//! a second list that never came, in the window and over MCP alike. A test binary of its own,
//! because the pool is switched on for the whole process. Skipped without intelephense.

use std::time::{Duration, Instant};

use idecraft_languages::pool;

#[tokio::test]
async fn a_file_opened_after_the_indexing_is_answered_at_once() {
    let php = idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Php)
        .ok()
        .filter(|r| r.candidate.program == "intelephense");
    if php.is_none() {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: intelephense"
        );
        eprintln!("skipped: intelephense not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-php-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let good = root.join("greeter.php");
    let bad = root.join("bad.php");
    std::fs::write(
        &good,
        "<?php\n// Greets someone.\nfunction greet(string $name): string {\n    return \"Hello \" . $name;\n}\necho greet(\"you\");\n",
    )
    .unwrap();
    std::fs::write(&bad, "<?php\n$x = new Missing();\necho undefined_fn(1);\n").unwrap();
    pool::enable();
    let wait = Duration::from_secs(60);
    // The first question starts the server and waits for its indexing.
    let first = idecraft_languages::lsp::file_diagnostics(&root, &good, wait)
        .await
        .unwrap();
    assert!(first.is_empty(), "{first:?}");
    // The next file is opened in the indexed server: one list, the answer.
    let started = Instant::now();
    let found = idecraft_languages::lsp::file_diagnostics(&root, &bad, wait)
        .await
        .unwrap();
    let took = started.elapsed();
    assert!(
        found.iter().any(|d| d.message.contains("Missing")),
        "{found:?}"
    );
    assert!(took < Duration::from_secs(10), "took {took:?}");
    let _ = std::fs::remove_dir_all(&root);
}

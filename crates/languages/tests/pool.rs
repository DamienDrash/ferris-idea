//! SPEC-0090/AK-1: the language server pool with a real server. Skipped when `clangd` is not
//! installed. A test binary of its own, because the pool is switched on for the whole process.

use std::time::{Duration, Instant};

use idecraft_languages::{insight, lsp, pool};

/// Skips a test whose tool is missing; with `IDECRAFT_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

/// Waits until the pool has diagnostics for `file` that satisfy `want`.
async fn wait_for(
    file: &std::path::Path,
    since: &mut u64,
    want: impl Fn(&[lsp::Diagnostic]) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let (now, changed) = pool::diagnostics_since(*since);
        *since = now;
        if changed.iter().any(|(p, d)| p == file && want(d)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn one_server_follows_the_text_and_publishes_while_typing() {
    if std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_err()
    {
        skip("clangd not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("main.c");
    let good = "int twice(int x) { return 2 * x; }\nint main(void) { return twice(1); }\n";
    std::fs::write(&file, good).unwrap();
    pool::enable();
    let wait = Duration::from_secs(30);

    // Two questions, one server; the second does not start anything.
    let first = Instant::now();
    let hover = insight::hover_in(&root, &file, None, 1, 25, wait)
        .await
        .unwrap();
    let cold = first.elapsed();
    assert!(hover.is_some_and(|h| h.text.contains("twice")));
    let second = Instant::now();
    let hover = insight::hover_in(&root, &file, None, 1, 25, wait)
        .await
        .unwrap();
    assert!(hover.is_some());
    assert_eq!(pool::running(), 1);
    assert!(
        second.elapsed() < cold,
        "warm {:?} not faster than cold {cold:?}",
        second.elapsed()
    );

    // Unsaved text reaches the server; its diagnostics come without saving.
    let mut since = pool::diagnostics_since(0).0;
    let broken = good.replace("twice(1)", "twice(missing)");
    pool::sync_text(&root, &file, &broken).await.unwrap();
    assert!(
        wait_for(&file, &mut since, |d| d
            .iter()
            .any(|d| d.message.contains("missing")))
        .await,
        "no diagnostic for the typed error"
    );
    pool::sync_text(&root, &file, good).await.unwrap();
    assert!(
        wait_for(&file, &mut since, |d| d.is_empty()).await,
        "the fixed text kept its error"
    );

    // Questions on the unsaved text use the same document.
    let completion = insight::complete_in(&root, &file, Some(good), 1, 26, 50, wait)
        .await
        .unwrap();
    assert!(completion.iter().any(|c| c.label.contains("twice")));
    assert_eq!(pool::running(), 1);

    pool::close_text(&file).await;
    pool::shutdown();
    assert_eq!(pool::running(), 0);
    let _ = std::fs::remove_dir_all(&root);
}

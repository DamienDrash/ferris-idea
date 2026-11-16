//! The system keychain is never asked on the UI thread (OBS-0026 L26): a system prompt for
//! keychain access would otherwise stop the window from drawing. Every use of the system store in
//! the window's code sits in a background task, or in a function documented to run on one.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Whether the keychain use at byte `at` of `code` runs off the UI thread: a background task opens
/// within the 30 lines before it, or the enclosing function says it runs in the background.
fn off_ui_thread(code: &str, at: usize) -> bool {
    let before = &code[..at];
    let window_start = before.rmatch_indices('\n').nth(30).map_or(0, |(i, _)| i);
    let window = &before[window_start..];
    if [
        "background_executor().spawn(",
        "background_spawn(",
        "thread::spawn(",
        "thread::Builder::new()",
        "spawn_blocking(",
    ]
    .iter()
    .any(|p| window.contains(p))
    {
        return true;
    }
    let Some(fn_at) = before.rfind("fn ") else {
        return false;
    };
    let line_start = before[..fn_at].rfind('\n').map_or(0, |n| n + 1);
    let doc: String = before[..line_start]
        .lines()
        .rev()
        .take_while(|l| l.trim_start().starts_with("///") || l.trim_start().starts_with("#["))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    doc.contains("background")
}

#[test]
fn the_keychain_is_read_and_written_off_the_ui_thread() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for dir in ["workbench/src", "app/src", "ui/src"] {
        rust_files(&crates.join(dir), &mut files);
    }
    let mut bad = Vec::new();
    let mut seen = 0;
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        for pat in ["SystemStore", "KeychainStore", "stored_key("] {
            for (i, _) in code.match_indices(pat) {
                let line_start = code[..i].rfind('\n').map_or(0, |n| n + 1);
                let line = code[line_start..].lines().next().unwrap_or("").trim();
                // The statement it belongs to: an import names the store, it does not use it.
                let stmt_start = code[..i].rfind(';').map_or(0, |n| n + 1);
                let stmt = code[stmt_start..i].trim_start();
                if stmt.starts_with("use ")
                    || stmt.starts_with("pub use ")
                    || line.starts_with("//")
                {
                    continue;
                }
                seen += 1;
                if !off_ui_thread(code, i) {
                    let n = code[..i].matches('\n').count() + 1;
                    bad.push(format!("{}:{n}: {line}", f.display()));
                }
            }
        }
    }
    assert!(seen > 0, "no keychain use found: the scan is broken");
    assert!(bad.is_empty(), "keychain on the UI thread: {bad:#?}");
}

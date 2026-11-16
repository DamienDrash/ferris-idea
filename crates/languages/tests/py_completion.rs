//! SPEC-0072 v9 AK-26 (OBS-0021): Python completion through pyright: signature and type from the
//! resolved documentation, call parentheses, IDECraft's postfix and live templates and standard
//! library imports the server does not offer. A test binary of its own, because the pool is
//! switched on for the whole process. Skipped without a Python language server.

use std::time::Duration;

use idecraft_languages::insight::{CompletionItem, complete_in, resolve_details_in};
use idecraft_languages::pool;
use idecraft_languages::script_completion::detail_parts;

const MODEL: &str = "from dataclasses import dataclass\n\n\n@dataclass\nclass Item:\n    sku: str\n    qty: int\n\n    def total_cents(self) -> int:\n        return self.qty\n";

#[tokio::test]
async fn python_types_parentheses_templates_and_imports() {
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Python).is_err() {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: pyright-langserver"
        );
        eprintln!("skipped: no Python language server installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-py-cmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("model.py"), MODEL).unwrap();
    let file = root.join("report.py");
    pool::enable();
    let wait = Duration::from_secs(120);
    let head = "import os\n\nfrom model import Item\n\n\ndef report(item: Item):\n";
    let ask = |line: &'static str| {
        let (root, file) = (root.clone(), file.clone());
        async move {
            let text = format!("{head}{line}\n");
            std::fs::write(&file, &text).unwrap();
            pool::sync_text(&root, &file, &text).await.unwrap();
            let column = line.chars().count() as u64;
            let items = complete_in(&root, &file, Some(&text), 6, column, 5000, wait)
                .await
                .unwrap();
            (text, items)
        }
    };
    let find = |items: &[CompletionItem], label: &str| -> CompletionItem {
        items
            .iter()
            .find(|i| i.label == label)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "no {label} in {:?}",
                    items.iter().map(|i| &i.label).collect::<Vec<_>>()
                )
            })
    };

    // Members, types from the resolved documentation (behaviour 34), parentheses (37) and the
    // postfix templates after them (38).
    let (text, items) = ask("    item.").await;
    let qty = find(&items, "qty");
    let total = find(&items, "total_cents");
    assert_eq!(total.insert, "total_cents($0)");
    let resolved = resolve_details_in(
        &root,
        &file,
        Some(&text),
        &[qty.clone(), total.clone()],
        wait,
    )
    .await
    .unwrap();
    let d = resolved[0]
        .as_ref()
        .and_then(|r| r.detail.clone())
        .expect("qty detail");
    assert_eq!(detail_parts("qty", &d).1.as_deref(), Some("int"));
    let d = resolved[1]
        .as_ref()
        .and_then(|r| r.detail.clone())
        .expect("total detail");
    assert_eq!(
        detail_parts("total_cents", &d),
        (Some("()".into()), Some("int".into()))
    );
    let iff = find(&items, "if");
    assert_eq!(iff.insert, "if item:\n\t$0");

    // A standard library class the server does not offer, with its import (behaviours 40–41).
    let (_, items) = ask("    Path").await;
    let path = items
        .iter()
        .find(|i| i.label == "Path" && i.description.as_deref() == Some("pathlib"))
        .unwrap_or_else(|| panic!("no pathlib.Path in {items:#?}"));
    assert_eq!(path.insert, "Path");
    assert_eq!(path.additional_edits.len(), 1);
    let e = &path.additional_edits[0];
    assert_eq!((e.range.start_line, e.range.start_character), (0, 9));
    assert_eq!(e.text, "\nfrom pathlib import Path");

    // Only the names of the running platform (IMP-0315): no Windows-only function on macOS/Linux.
    let (_, items) = ask("    os.").await;
    find(&items, "getcwd");
    if !cfg!(windows) {
        assert!(!items.iter().any(|i| i.label == "add_dll_directory"));
    }

    // A live template at the start of a statement (behaviour 39).
    let (_, items) = ask("    iter").await;
    let iter = items
        .iter()
        .find(|i| i.label == "iter" && i.kind.as_deref() == Some("snippet"))
        .expect("iter template");
    assert!(iter.insert.starts_with("for ${1:"), "{}", iter.insert);
    let _ = std::fs::remove_dir_all(&root);
}

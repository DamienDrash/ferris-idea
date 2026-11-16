//! F-015 · Find/replace, regex, in files: search in a file and in the project, preview before writing
//! (SPEC-0029 AK-1…AK-5, SPEC-0066).

use crate::harness::*;
use serde_json::json;

#[test]
fn find_in_files_with_case_whole_words_and_mask() {
    let p = Project::reference("python-basic");
    p.write("notes.txt", "QTY qtys qty\n");
    let mut ide = Ide::stdio(&p);
    let all = ide.ok("search_in_files", json!({"pattern": "qty"}));
    let paths: Vec<String> = all["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| text(&m["path"]))
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "sorted by path");
    assert!(paths.contains(&"notes.txt".to_owned()));

    let case = ide.ok(
        "search_in_files",
        json!({"pattern": "QTY", "match_case": true, "file_mask": "*.txt"}),
    );
    assert_eq!(case["matches"].as_array().unwrap().len(), 1, "{case}");
    let words = ide.ok(
        "search_in_files",
        json!({"pattern": "qty", "whole_words": true, "file_mask": "*.txt"}),
    );
    assert_eq!(words["matches"].as_array().unwrap().len(), 2, "{words}");
    let py = ide.ok(
        "search_in_files",
        json!({"pattern": "qty", "file_mask": "*.py"}),
    );
    assert!(
        py["matches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| text(&m["path"]).ends_with(".py"))
    );
}

#[test]
fn regex_replace_preview_with_groups_writes_nothing() {
    let p = Project::reference("python-basic");
    let before = p.read("src/inventory/model.py");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "replace_in_files_preview",
        json!({"pattern": "def (\\w+)\\(", "replacement": "def new_$1(", "regex": true}),
    );
    let all = text(&r);
    assert!(all.contains("def new_total_cents("), "{all}");
    assert!(all.contains("def new_restock("), "{all}");
    assert_eq!(p.read("src/inventory/model.py"), before);
}

#[test]
fn an_invalid_regex_is_an_error() {
    let p = Project::reference("python-basic");
    let mut ide = Ide::stdio(&p);
    let reason = ide.refused(
        "search_in_files",
        json!({"pattern": "(unclosed", "regex": true}),
    );
    assert!(!reason.is_empty());
}

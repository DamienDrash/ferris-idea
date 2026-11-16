//! SPEC-0220 v5: agents' edits of files open in the window's editor. AK-4 (the edit lands in the
//! buffer as one undo step), AK-5 (tools that cannot do that yet refuse unsaved files), AK-6
//! (the ratchet over every tool that changes files), AK-7 to AK-9 (language servers and
//! refactorings through the buffers, all or nothing), AK-10 (other unsaved files reach the
//! language server; moves through the buffers), AK-11 (the window gets the refactoring's group).
//! AK-12 to AK-15 (v5: Git, Compare, bundles and line separators through the buffers; deleting
//! and renaming keep refusing).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use idecraft_editor::TextBuffer;
use idecraft_editor::documents::{
    DocumentChange, DocumentEdit, DocumentEdited, DocumentRegistry, apply_edit,
};
use idecraft_mcp::effects::{Change, TOOLS};
use idecraft_mcp::open_files::{OWN_FILES, REFUSES_UNSAVED, REFUSES_UNSAVED_MAX, THROUGH_BUFFER};
use idecraft_mcp::window::{PendingWork, PermissionAnswer, PermissionRequest, QuitMode, Window};
use serde_json::json;
use tokio::sync::oneshot;

use super::effects::{code_of, functions};
use super::protocol::{connect_custom, error_message, is_error, json_text};

/// Calls in a tool's code that refuse a file with unsaved changes, or write through the buffer
/// (which refuses what it cannot take).
const REFUSALS: [&str; 7] = [
    "refuse_unsaved(",
    "refuse_unsaved_under(",
    "unsaved_error(",
    "write_document(",
    "write_documents(",
    "write_file_edits(",
    "write_shown(",
];

/// SPEC-0220 v3 AK-6: every tool that changes files is in exactly one list, the lists say what
/// the code does, and the list of refusing tools does not grow.
#[test]
fn every_file_changing_tool_edits_open_files_through_the_buffer_or_refuses() {
    let functions = functions();
    let mut problems: Vec<String> = Vec::new();
    let changing: BTreeSet<&str> = TOOLS
        .iter()
        .filter(|t| t.changes.contains(&Change::Files))
        .map(|t| t.tool)
        .collect();
    let mut lists: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for t in THROUGH_BUFFER {
        lists.entry(t).or_default().push("THROUGH_BUFFER");
    }
    for t in REFUSES_UNSAVED {
        lists.entry(t).or_default().push("REFUSES_UNSAVED");
    }
    for (t, why) in OWN_FILES {
        lists.entry(t).or_default().push("OWN_FILES");
        if why.trim().is_empty() {
            problems.push(format!("{t}: OWN_FILES without a reason"));
        }
    }
    for (tool, in_lists) in &lists {
        if in_lists.len() > 1 {
            problems.push(format!("{tool}: in {in_lists:?}; one list only"));
        }
        if !changing.contains(tool) {
            problems.push(format!(
                "{tool}: listed in crates/mcp/src/open_files.rs, but its row in TOOLS does not change files"
            ));
        }
    }
    for tool in &changing {
        if !lists.contains_key(tool) {
            problems.push(format!(
                "{tool}: changes files but is in no list of crates/mcp/src/open_files.rs; write through the buffer (read_document + write_document), refuse unsaved files (refuse_unsaved) or name it as an own file"
            ));
        }
    }
    for tool in THROUGH_BUFFER {
        let code = code_of(tool, &functions);
        if !["write_document(", "write_documents(", "write_shown("]
            .iter()
            .any(|w| code.contains(w))
        {
            problems.push(format!(
                "{tool}: in THROUGH_BUFFER, but never calls write_document, write_documents or write_shown"
            ));
        }
        if !(code.contains("read_document(") || code.contains("documents.get(")) {
            problems.push(format!(
                "{tool}: in THROUGH_BUFFER, but reads its text from the disk instead of the editor"
            ));
        }
    }
    for tool in REFUSES_UNSAVED {
        let code = code_of(tool, &functions);
        if !REFUSALS.iter().any(|r| code.contains(r)) {
            problems.push(format!(
                "{tool}: in REFUSES_UNSAVED, but its code makes no refusal ({REFUSALS:?})"
            ));
        }
    }
    if REFUSES_UNSAVED.len() > REFUSES_UNSAVED_MAX {
        problems.push(format!(
            "REFUSES_UNSAVED has {} tools, at most {REFUSES_UNSAVED_MAX}: the list only shrinks",
            REFUSES_UNSAVED.len()
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The window's editor of one file, toolkit-free: applies edits as the window does
/// (`WorkbenchView::edit_document_in`), with the same checks of the registry.
struct EditorWindow {
    registry: DocumentRegistry,
    file: PathBuf,
    buffer: Arc<Mutex<TextBuffer>>,
    /// Whether the window answers edits at all.
    answers: bool,
}

impl EditorWindow {
    fn publish(&self) {
        let b = self.buffer.lock().unwrap();
        self.registry.publish(
            &DocumentRegistry::key(&self.file),
            b.snapshot(),
            b.is_modified(),
        );
    }
}

impl Window for EditorWindow {
    fn ask(&self, _: PermissionRequest, _: oneshot::Sender<PermissionAnswer>) {}
    fn pending_work(&self, _: oneshot::Sender<PendingWork>) {}
    fn quit(&self, _: QuitMode, _: oneshot::Sender<Result<(), String>>) {}
    fn edit_document(
        &self,
        edit: DocumentEdit,
        reply: oneshot::Sender<Result<DocumentEdited, String>>,
    ) {
        if !self.answers {
            return;
        }
        if let Err(e) = self.registry.check_edit(&edit) {
            let _ = reply.send(Err(e));
            return;
        }
        {
            let mut b = self.buffer.lock().unwrap();
            apply_edit(&mut b, &edit.text);
            if edit.save {
                std::fs::write(&self.file, b.text()).unwrap();
                b.mark_saved();
            }
        }
        self.publish();
        let doc = self.registry.get(&edit.path).unwrap();
        let _ = reply.send(Ok(DocumentEdited {
            revision: doc.revision,
            modified: doc.modified,
        }));
    }
}

/// A workspace with a copyright profile and `src/app.py` open in an editor with unsaved typing.
async fn open_with_unsaved_typing(
    answers: bool,
) -> (super::protocol::Client, PathBuf, Arc<EditorWindow>, String) {
    let registry = DocumentRegistry::default();
    let buffer = Arc::new(Mutex::new(TextBuffer::from_text("")));
    let window: Arc<Mutex<Option<Arc<EditorWindow>>>> = Arc::default();
    let (r, b, w) = (registry.clone(), buffer.clone(), window.clone());
    let (client, root) = connect_custom(&[], move |server, root| {
        let file = root.join("src/app.py");
        let editor = Arc::new(EditorWindow {
            registry: r.clone(),
            file,
            buffer: b,
            answers,
        });
        *w.lock().unwrap() = Some(editor.clone());
        let window: Arc<dyn Window> = editor;
        server.with_documents(r).with_window(window)
    })
    .await;
    let editor = window.lock().unwrap().clone().unwrap();
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    std::fs::write(
        root.join(".idecraft/copyright.txt"),
        "Copyright $today.year ACME",
    )
    .unwrap();
    let file = root.join("src/app.py");
    let disk = std::fs::read_to_string(&file).unwrap();
    *buffer.lock().unwrap() = TextBuffer::from_text(&disk);
    registry.open(
        &DocumentRegistry::key(&file),
        buffer.lock().unwrap().snapshot(),
        false,
    );
    // The user types without saving.
    let end = buffer.lock().unwrap().len_chars();
    buffer.lock().unwrap().insert(end, "d = 4\n").unwrap();
    editor.publish();
    let typed = buffer.lock().unwrap().text();
    (client, root, editor, typed)
}

/// SPEC-0220 v2 AK-4: an agent's edit of a buffer with unsaved changes lands in the buffer next
/// to the user's typing, leaves the disk alone, and one undo gives back exactly the user's text; a
/// saved buffer gets the edit saved.
#[tokio::test]
async fn an_agent_edit_of_an_unsaved_buffer_keeps_the_users_text_and_undoes_in_one_step() {
    let (mut client, root, editor, typed) = open_with_unsaved_typing(true).await;
    let file = root.join("src/app.py");
    let disk = std::fs::read_to_string(&file).unwrap();
    let key = DocumentRegistry::key(&file);
    let before = editor.registry.get(&key).unwrap();
    assert!(before.modified);

    let done = client
        .call(
            "update_copyright",
            json!({"path": "src/app.py", "dry_run": false, "year": 2026}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    let answer = json_text(&done);
    assert_eq!(answer["applied"], true, "{answer}");
    // The answer and the buffer hold the user's typing and the notice.
    let shown = editor.buffer.lock().unwrap().text();
    assert!(shown.starts_with("# Copyright 2026 ACME"), "{shown}");
    assert!(shown.ends_with(&typed), "{shown}");
    assert_eq!(answer["text"], json!(shown));
    // Nothing was written behind the editor; the entry moved on by one and stays unsaved.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);
    let after = editor.registry.get(&key).unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert!(after.modified);
    // read_file shows the edited buffer.
    let read = client
        .call("read_file", json!({"path": "src/app.py"}))
        .await;
    assert!(
        super::protocol::text(&read).contains("Copyright 2026"),
        "{read}"
    );
    // One undo is the user's text again.
    assert!(editor.buffer.lock().unwrap().undo());
    assert_eq!(editor.buffer.lock().unwrap().text(), typed);
    editor.publish();

    // A saved buffer: the edit is saved, the disk equals the buffer and nothing is modified.
    {
        let mut b = editor.buffer.lock().unwrap();
        std::fs::write(&file, b.text()).unwrap();
        b.mark_saved();
    }
    editor.publish();
    let done = client
        .call(
            "update_copyright",
            json!({"path": "src/app.py", "dry_run": false, "year": 2027}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    let shown = editor.buffer.lock().unwrap().text();
    assert!(shown.starts_with("# Copyright 2027 ACME"), "{shown}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), shown);
    assert!(!editor.registry.get(&key).unwrap().modified);
    assert!(editor.buffer.lock().unwrap().undo());
    assert_eq!(editor.buffer.lock().unwrap().text(), typed);
}

/// SPEC-0220 v2 AK-5: tools that compute from the disk refuse a file with unsaved changes and
/// change nothing; without the window's answer a buffer tool refuses it too.
#[tokio::test]
async fn tools_that_cannot_edit_the_buffer_refuse_unsaved_files() {
    let (mut client, root, editor, typed) = open_with_unsaved_typing(false).await;
    let file = root.join("src/app.py");
    let disk = std::fs::read_to_string(&file).unwrap();
    let key = DocumentRegistry::key(&file);
    let revision = editor.registry.get(&key).unwrap().revision;
    let calls = [
        (
            "delete_file",
            json!({"path": "src/app.py", "dry_run": false}),
        ),
        (
            "rename_file",
            json!({"path": "src/app.py", "new_name": "other.py"}),
        ),
        (
            "move_file",
            json!({"path": "src/app.py", "target_dir": ".", "dry_run": false,
                   "update_references": false}),
        ),
        // The window does not answer: the edit is not written behind it.
        (
            "update_copyright",
            json!({"path": "src/app.py", "dry_run": false, "year": 2026}),
        ),
    ];
    for (tool, args) in calls {
        let refused = client.call(tool, args).await;
        assert!(is_error(&refused), "{tool}: {refused}");
        let message = error_message(&refused);
        assert!(
            message.contains("src/app.py has unsaved changes in an editor: save it first"),
            "{tool}: {message}"
        );
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);
    assert!(!Path::new(&root.join("src/other.py")).exists());
    let doc = editor.registry.get(&key).unwrap();
    assert_eq!((doc.revision, doc.modified), (revision, true));
    assert_eq!(editor.buffer.lock().unwrap().text(), typed);

    // Saved, the same call without the window's answer writes the disk.
    {
        let mut b = editor.buffer.lock().unwrap();
        std::fs::write(&file, b.text()).unwrap();
        b.mark_saved();
    }
    editor.publish();
    let done = client
        .call(
            "update_copyright",
            json!({"path": "src/app.py", "dry_run": false, "year": 2026}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .starts_with("# Copyright 2026 ACME")
    );
}

/// The window's editors of several files, toolkit-free: takes a change of several files all or
/// nothing as the window does (`WorkbenchView::edit_documents_in`, SPEC-0220 v3 behaviour 14).
#[derive(Default)]
struct Editors {
    registry: DocumentRegistry,
    files: Mutex<Vec<(PathBuf, Arc<Mutex<TextBuffer>>)>>,
    /// Typing that lands in a file just before the window takes the next change (the user typed
    /// in between).
    type_first: Mutex<Option<(PathBuf, String)>>,
    /// Every change the window was handed.
    changes: Mutex<Vec<DocumentChange>>,
}

impl Editors {
    fn buffer(&self, file: &Path) -> Arc<Mutex<TextBuffer>> {
        let key = DocumentRegistry::key(file);
        let files = self.files.lock().unwrap();
        let (_, b) = files
            .iter()
            .find(|(p, _)| DocumentRegistry::key(p) == key)
            .expect("open");
        b.clone()
    }

    fn text(&self, file: &Path) -> String {
        self.buffer(file).lock().unwrap().text()
    }

    fn publish(&self, file: &Path) {
        let buffer = self.buffer(file);
        let b = buffer.lock().unwrap();
        self.registry
            .publish(&DocumentRegistry::key(file), b.snapshot(), b.is_modified());
    }

    /// Opens `file` as it is on disk.
    fn open(&self, file: &Path) {
        let b = TextBuffer::from_text(&std::fs::read_to_string(file).unwrap());
        self.registry
            .open(&DocumentRegistry::key(file), b.snapshot(), false);
        self.files
            .lock()
            .unwrap()
            .push((file.to_path_buf(), Arc::new(Mutex::new(b))));
    }

    /// The user types `text` at the end of `file` without saving.
    fn type_at_end(&self, file: &Path, text: &str) {
        {
            let buffer = self.buffer(file);
            let mut b = buffer.lock().unwrap();
            let end = b.len_chars();
            b.insert(end, text).unwrap();
        }
        self.publish(file);
    }

    fn undo(&self, file: &Path) -> bool {
        let undone = self.buffer(file).lock().unwrap().undo();
        self.publish(file);
        undone
    }

    fn revision(&self, file: &Path) -> u64 {
        self.registry.get(file).unwrap().revision
    }

    fn modified(&self, file: &Path) -> bool {
        self.registry.get(file).unwrap().modified
    }

    fn file_of(&self, key: &Path) -> PathBuf {
        let files = self.files.lock().unwrap();
        files
            .iter()
            .find(|(p, _)| DocumentRegistry::key(p) == key)
            .map(|(p, _)| p.clone())
            .unwrap()
    }
}

impl Window for Editors {
    fn ask(&self, _: PermissionRequest, _: oneshot::Sender<PermissionAnswer>) {}
    fn pending_work(&self, _: oneshot::Sender<PendingWork>) {}
    fn quit(&self, _: QuitMode, _: oneshot::Sender<Result<(), String>>) {}
    fn edit_documents(
        &self,
        change: DocumentChange,
        reply: oneshot::Sender<Result<Vec<DocumentEdited>, (PathBuf, String)>>,
    ) {
        let edits = change.edits.clone();
        self.changes.lock().unwrap().push(change);
        if let Some((file, text)) = self.type_first.lock().unwrap().take() {
            self.type_at_end(&file, &text);
        }
        for edit in &edits {
            if let Err(e) = self.registry.check_edit(edit) {
                let _ = reply.send(Err((edit.path.clone(), e)));
                return;
            }
        }
        let mut done = Vec::new();
        for edit in &edits {
            let file = self.file_of(&edit.path);
            {
                let buffer = self.buffer(&file);
                let mut b = buffer.lock().unwrap();
                apply_edit(&mut b, &edit.text);
                if edit.save {
                    std::fs::write(&file, b.text()).unwrap();
                    b.mark_saved();
                }
            }
            self.publish(&file);
            let doc = self.registry.get(&edit.path).unwrap();
            done.push(DocumentEdited {
                revision: doc.revision,
                modified: doc.modified,
            });
        }
        let _ = reply.send(Ok(done));
    }
}

/// A workspace whose window has the editors `editors`.
async fn with_editors() -> (super::protocol::Client, PathBuf, Arc<Editors>) {
    let editors = Arc::new(Editors::default());
    let e = editors.clone();
    let (client, root) = connect_custom(&[], move |server, _| {
        let window: Arc<dyn Window> = e.clone();
        server
            .with_documents(e.registry.clone())
            .with_window(window)
    })
    .await;
    (client, root, editors)
}

fn has_clangd() -> bool {
    std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_ok()
}

const CALC: &str =
    "int add(int a, int b) { return a + b; }\nint main(void) { return add(1, 2); }\n";

/// SPEC-0220 v3 AK-7: a rename of a file open with unsaved typing is computed against the shown
/// text (the typed usage is renamed too), lands in the buffer as one undo step and leaves the disk
/// alone; `undo_refactoring` goes back through the buffer; a change the window finds at another
/// revision is refused and changes nothing.
#[tokio::test]
async fn a_rename_of_an_unsaved_buffer_renames_the_typed_usage_and_undoes_in_one_step() {
    if !has_clangd() {
        return;
    }
    let (mut client, root, editors) = with_editors().await;
    let file = root.join("src/calc.c");
    std::fs::write(&file, CALC).unwrap();
    editors.open(&file);
    editors.type_at_end(&file, "int twice(void) { return add(2, 2); }\n");
    let typed = editors.text(&file);
    let revision = editors.revision(&file);
    let rename = json!({"path": "src/calc.c", "line": 1, "character": 25, "new_name": "sum",
                        "dry_run": false});

    let done = client.call("rename_symbol", rename.clone()).await;
    assert!(!is_error(&done), "{done}");
    assert_eq!(json_text(&done)["applied"], true, "{done}");
    let shown = editors.text(&file);
    assert_eq!(shown, typed.replace("add(", "sum("), "{shown}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), CALC);
    assert_eq!(editors.revision(&file), revision + 1);
    assert!(editors.modified(&file));
    // One undo in the editor is the user's text again.
    assert!(editors.undo(&file));
    assert_eq!(editors.text(&file), typed);

    // undo_refactoring takes a rename back through the buffer.
    let done = client.call("rename_symbol", rename.clone()).await;
    assert!(!is_error(&done), "{done}");
    assert!(editors.text(&file).contains("return sum(2, 2)"));
    let undone = client.call("undo_refactoring", json!({})).await;
    assert!(!is_error(&undone), "{undone}");
    assert_eq!(
        json_text(&undone)["undone"],
        "Rename add to sum",
        "{undone}"
    );
    assert_eq!(editors.text(&file), typed);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), CALC);
    assert!(editors.modified(&file));

    // The user types while the window takes the change: refused, nothing changed.
    *editors.type_first.lock().unwrap() = Some((file.clone(), "// more\n".into()));
    let refused = client.call("rename_symbol", rename).await;
    assert!(is_error(&refused), "{refused}");
    let message = error_message(&refused);
    assert!(message.contains("src/calc.c"), "{message}");
    assert!(
        message.contains("changed in the editor meanwhile"),
        "{message}"
    );
    assert_eq!(editors.text(&file), format!("{typed}// more\n"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), CALC);
}

/// SPEC-0220 v3 AK-8: a rename that changes two open files (the code, and a text occurrence in
/// a saved Markdown file) applies to both or to neither.
#[tokio::test]
async fn a_change_of_several_open_files_applies_to_all_or_none() {
    if !has_clangd() {
        return;
    }
    let (mut client, root, editors) = with_editors().await;
    let (code, notes) = (root.join("src/calc.c"), root.join("NOTES.md"));
    std::fs::write(&code, CALC).unwrap();
    std::fs::write(&notes, "Call add to add numbers.\n").unwrap();
    editors.open(&code);
    editors.open(&notes);
    editors.type_at_end(&code, "int twice(void) { return add(2, 2); }\n");
    let typed = editors.text(&code);
    let rename = json!({"path": "src/calc.c", "line": 1, "character": 25, "new_name": "sum",
                        "search_text_occurrences": true, "exclude": [1], "dry_run": false});

    // The user types in the notes in between: neither file changes.
    *editors.type_first.lock().unwrap() = Some((notes.clone(), "More.\n".into()));
    let refused = client.call("rename_symbol", rename.clone()).await;
    assert!(is_error(&refused), "{refused}");
    assert!(error_message(&refused).contains("NOTES.md"), "{refused}");
    assert_eq!(editors.text(&code), typed);
    assert_eq!(
        editors.text(&notes),
        "Call add to add numbers.\nMore.\n",
        "nothing but the typing"
    );
    assert_eq!(std::fs::read_to_string(&code).unwrap(), CALC);
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "Call add to add numbers.\n"
    );
    // Saved again, the same rename changes both: the unsaved code stays unsaved, the saved notes
    // are saved.
    assert!(editors.undo(&notes));
    {
        let buffer = editors.buffer(&notes);
        buffer.lock().unwrap().mark_saved();
    }
    editors.publish(&notes);
    let done = client.call("rename_symbol", rename).await;
    assert!(!is_error(&done), "{done}");
    assert_eq!(editors.text(&code), typed.replace("add(", "sum("));
    assert_eq!(editors.text(&notes), "Call sum to add numbers.\n");
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "Call sum to add numbers.\n"
    );
    assert!(editors.modified(&code) && !editors.modified(&notes));
    assert_eq!(std::fs::read_to_string(&code).unwrap(), CALC);
    // undo_refactoring takes back both.
    let undone = client.call("undo_refactoring", json!({})).await;
    assert!(!is_error(&undone), "{undone}");
    assert_eq!(editors.text(&code), typed);
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "Call add to add numbers.\n"
    );
}

/// SPEC-0220 v3 AK-9: format_file formats the shown text of a file with unsaved changes and
/// leaves the disk alone: IDECraft's own SQL formatter, and with clangd the language server.
#[tokio::test]
async fn format_file_formats_the_shown_text_of_an_unsaved_buffer() {
    let (mut client, root, editors) = with_editors().await;
    let sql = root.join("src/q.sql");
    std::fs::write(&sql, "select 1;\n").unwrap();
    editors.open(&sql);
    editors.type_at_end(&sql, "select a,b from t where a=1;\n");
    let done = client
        .call(
            "format_file",
            json!({"path": "src/q.sql", "dry_run": false}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    let shown = editors.text(&sql);
    assert!(shown.contains("a = 1"), "{shown}");
    assert!(editors.modified(&sql));
    assert_eq!(std::fs::read_to_string(&sql).unwrap(), "select 1;\n");
    if !has_clangd() {
        return;
    }
    let code = root.join("src/calc.c");
    std::fs::write(&code, CALC).unwrap();
    editors.open(&code);
    editors.type_at_end(&code, "int   twice(void){return add(2,2);}\n");
    let done = client
        .call(
            "format_file",
            json!({"path": "src/calc.c", "dry_run": false}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    let shown = editors.text(&code);
    assert!(shown.contains("int twice(void)"), "{shown}");
    assert!(editors.modified(&code));
    assert_eq!(std::fs::read_to_string(&code).unwrap(), CALC);
}

const CALC_H: &str = "int add(int a, int b);\n";
const CALC_C: &str = "#include \"calc.h\"\nint add(int a, int b) { return a + b; }\nint main(void) { return add(1, 2); }\n";

/// SPEC-0220 v4 AK-10: a rename in `calc.cpp` renames the usage typed without saving in the
/// open `calc.h` too: the server got the header's shown text before it planned (clangd renames
/// in another file only when it has that file open). Neither file is refused with "save it
/// first"; the header's disk is unchanged; one undo in each editor gives back the user's text.
#[tokio::test]
async fn a_rename_reaches_other_unsaved_files_through_the_server() {
    if !has_clangd() {
        return;
    }
    let (mut client, root, editors) = with_editors().await;
    let (header, code) = (root.join("src/calc.h"), root.join("src/calc.cpp"));
    std::fs::write(&header, CALC_H).unwrap();
    std::fs::write(&code, CALC_C).unwrap();
    editors.open(&header);
    editors.open(&code);
    editors.type_at_end(
        &header,
        "static inline int three(void) { return add(1, 2); }\n",
    );
    let typed = editors.text(&header);
    let rename = json!({"path": "src/calc.cpp", "line": 2, "character": 25, "new_name": "sum",
                        "dry_run": false});
    let done = client.call("rename_symbol", rename).await;
    assert!(!is_error(&done), "{done}");
    assert_eq!(editors.text(&header), typed.replace("add(", "sum("));
    assert_eq!(editors.text(&code), CALC_C.replace("add(", "sum("));
    assert_eq!(std::fs::read_to_string(&header).unwrap(), CALC_H);
    // The saved code was saved with the change; the header stays unsaved.
    assert_eq!(
        std::fs::read_to_string(&code).unwrap(),
        CALC_C.replace("add(", "sum(")
    );
    assert!(editors.modified(&header) && !editors.modified(&code));
    assert!(editors.undo(&header));
    assert_eq!(editors.text(&header), typed);
    assert!(editors.undo(&code));
    assert_eq!(editors.text(&code), CALC_C);
}

/// SPEC-0220 v4 AK-11: an agent's rename across an open file and a closed one (a text occurrence
/// in Markdown) hands the window the refactoring's name and the file written to disk, for the
/// window's undo group (SPEC-0193 v5 behaviour 10); `undo_refactoring` hands it "Undo <name>".
#[tokio::test]
async fn a_rename_across_files_hands_the_window_its_group() {
    if !has_clangd() {
        return;
    }
    let (mut client, root, editors) = with_editors().await;
    let (notes, code) = (root.join("NOTES.md"), root.join("src/calc.c"));
    std::fs::write(&notes, "Call add to add numbers.\n").unwrap();
    std::fs::write(&code, CALC).unwrap();
    editors.open(&code);
    editors.type_at_end(&code, "// typed\n");
    let rename = json!({"path": "src/calc.c", "line": 1, "character": 25, "new_name": "sum",
                        "search_text_occurrences": true, "exclude": [1], "dry_run": false});
    let done = client.call("rename_symbol", rename).await;
    assert!(!is_error(&done), "{done}");
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "Call sum to add numbers.\n"
    );
    {
        let changes = editors.changes.lock().unwrap();
        let change = changes.last().expect("the window got the change");
        assert_eq!(change.name.as_deref(), Some("Rename add to sum"));
        assert_eq!(change.edits.len(), 1);
        assert_eq!(change.on_disk.len(), 1, "{:?}", change.on_disk);
        assert_eq!(change.on_disk[0].path, DocumentRegistry::key(&notes));
        assert_eq!(change.on_disk[0].before, "Call add to add numbers.\n");
        assert_eq!(change.on_disk[0].after, "Call sum to add numbers.\n");
    }
    let undone = client.call("undo_refactoring", json!({})).await;
    assert!(!is_error(&undone), "{undone}");
    let changes = editors.changes.lock().unwrap();
    assert_eq!(
        changes.last().unwrap().name.as_deref(),
        Some("Undo Rename add to sum")
    );
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "Call add to add numbers.\n"
    );
}

/// SPEC-0220 v4 AK-10 (behaviour 16): `move_file` with reference updates rewrites the include
/// of a file open with unsaved typing in its buffer, leaves its disk alone and moves the file; a
/// moved file with unsaved changes is still refused.
#[tokio::test]
async fn move_file_rewrites_references_in_unsaved_buffers() {
    let (mut client, root, editors) = with_editors().await;
    let (util, main) = (root.join("src/util.h"), root.join("src/main.c"));
    std::fs::write(&util, "int twice(int x);\n").unwrap();
    let disk = "#include \"util.h\"\nint main(void) { return twice(2); }\n";
    std::fs::write(&main, disk).unwrap();
    editors.open(&main);
    editors.type_at_end(&main, "// typed\n");
    let typed = editors.text(&main);
    let revision = editors.revision(&main);
    let done = client
        .call(
            "move_file",
            json!({"path": "src/util.h", "target_dir": "include", "dry_run": false}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    assert_eq!(
        editors.text(&main),
        typed.replace("\"util.h\"", "\"../include/util.h\"")
    );
    assert_eq!(std::fs::read_to_string(&main).unwrap(), disk);
    assert_eq!(editors.revision(&main), revision + 1);
    assert!(editors.modified(&main));
    assert!(root.join("include/util.h").exists() && !util.exists());
    // One undo is the user's text again.
    assert!(editors.undo(&main));
    assert_eq!(editors.text(&main), typed);

    // The moved file itself with unsaved changes: refused, nothing moved.
    let moved = root.join("include/util.h");
    editors.open(&moved);
    editors.type_at_end(&moved, "int thrice(int x);\n");
    let refused = client
        .call(
            "move_file",
            json!({"path": "include/util.h", "target_dir": "src", "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(
        error_message(&refused)
            .contains("include/util.h has unsaved changes in an editor: save it first"),
        "{refused}"
    );
    assert!(moved.exists() && !util.exists());
}

/// Runs git in `root`; false when git is missing or the command failed.
fn git(root: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@t.invalid"])
        .args(args)
        .current_dir(root)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// SPEC-0220 v5 AK-12: a gutter change's rollback and Accept Left Side work on the shown text and
/// change unsaved buffers only.
#[tokio::test]
async fn a_line_change_and_a_compare_apply_work_on_the_shown_text() {
    let (mut client, root, editors) = with_editors().await;
    if !git(&root, &["init", "-q", "-b", "main"]) {
        eprintln!("skipped: no git");
        return;
    }
    let file = root.join("a.txt");
    std::fs::write(&file, "1\n2\n3\n4\n5\n").unwrap();
    assert!(git(&root, &["add", "a.txt"]) && git(&root, &["commit", "-q", "-m", "base"]));
    let disk = "1\ntwo\n3\n4\n5\n";
    std::fs::write(&file, disk).unwrap();
    editors.open(&file);
    editors.type_at_end(&file, "6\n");
    let typed = editors.text(&file);
    // The typed line is a change of the shown text.
    let listed = json_text(
        &client
            .call("git_line_change", json!({"path": "a.txt"}))
            .await,
    );
    let changes = listed["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2, "{listed}");
    let revision = editors.revision(&file);
    let done = client
        .call(
            "git_line_change",
            json!({"path": "a.txt", "line": 2, "action": "rollback", "dry_run": false}),
        )
        .await;
    assert!(!is_error(&done), "{done}");
    assert_eq!(editors.text(&file), "1\n2\n3\n4\n5\n6\n");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), disk);
    assert_eq!(editors.revision(&file), revision + 1);
    assert!(editors.modified(&file));
    assert!(editors.undo(&file));
    assert_eq!(editors.text(&file), typed);

    // Accept Left Side into an unsaved right file: both sides as shown, the right buffer changes.
    let (left, right) = (root.join("left.txt"), root.join("right.txt"));
    std::fs::write(&left, "a\nb\nx\n").unwrap();
    std::fs::write(&right, "a\nB\nx\n").unwrap();
    editors.open(&right);
    editors.type_at_end(&right, "c\n");
    let compared = json_text(
        &client
            .call(
                "compare_files",
                json!({"left": "left.txt", "right": "right.txt", "apply": 1, "take": "left",
                       "dry_run": false}),
            )
            .await,
    );
    assert_eq!(compared["text"], "a\nb\nx\nc\n", "{compared}");
    assert_eq!(editors.text(&right), "a\nb\nx\nc\n");
    assert_eq!(std::fs::read_to_string(&right).unwrap(), "a\nB\nx\n");
    assert!(editors.modified(&right));
}

/// SPEC-0220 v5 AK-13: conflict resolution and rollback put their text in the buffer and save it
/// before Git takes the file from the disk; Undo gives the typed text back.
#[tokio::test]
async fn rollback_and_conflict_resolution_save_the_buffer_before_git() {
    let (mut client, root, editors) = with_editors().await;
    if !git(&root, &["init", "-q", "-b", "main"]) {
        eprintln!("skipped: no git");
        return;
    }
    let g = |args: &[&str]| assert!(git(&root, args), "git {args:?}");
    // A stopped merge; the file with its markers is open with unsaved typing.
    let c = root.join("c.txt");
    std::fs::write(&c, "1\n2\n3\n").unwrap();
    g(&["add", "c.txt"]);
    g(&["commit", "-q", "-m", "base"]);
    g(&["checkout", "-q", "-b", "topic"]);
    std::fs::write(&c, "1\ntwo\n3\n").unwrap();
    g(&["commit", "-qam", "topic"]);
    g(&["checkout", "-q", "main"]);
    std::fs::write(&c, "1\nTWO\n3\n").unwrap();
    g(&["commit", "-qam", "main"]);
    assert!(!git(&root, &["merge", "-q", "topic"]), "the merge stops");
    editors.open(&c);
    editors.type_at_end(&c, "typed\n");
    let resolved = client
        .call(
            "git_resolve_conflict",
            json!({"path": "c.txt", "take": "theirs", "dry_run": false}),
        )
        .await;
    assert!(!is_error(&resolved), "{resolved}");
    assert_eq!(editors.text(&c), "1\ntwo\n3\n");
    assert_eq!(std::fs::read_to_string(&c).unwrap(), "1\ntwo\n3\n");
    assert!(!editors.modified(&c));
    let after = json_text(&client.call("git_conflicts", json!({})).await);
    assert_eq!(after, json!([]));
    g(&["commit", "-q", "--no-edit"]);

    // Rollback: HEAD's text in the buffer, saved, the index restored.
    let a = root.join("a.txt");
    std::fs::write(&a, "base\n").unwrap();
    g(&["add", "a.txt"]);
    g(&["commit", "-q", "-m", "a"]);
    std::fs::write(&a, "changed\n").unwrap();
    g(&["add", "a.txt"]);
    editors.open(&a);
    editors.type_at_end(&a, "typed\n");
    let typed = editors.text(&a);
    let rolled = client
        .call(
            "git_rollback",
            json!({"paths": ["a.txt"], "dry_run": false}),
        )
        .await;
    assert!(!is_error(&rolled), "{rolled}");
    assert_eq!(editors.text(&a), "base\n");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "base\n");
    assert!(!editors.modified(&a));
    assert!(
        git(&root, &["diff", "--quiet", "HEAD", "--", "a.txt"]),
        "index and tree are HEAD's"
    );
    assert!(editors.undo(&a));
    assert_eq!(editors.text(&a), typed);
    assert!(editors.modified(&a));

    // A rollback that deletes an added file with unsaved changes is a file operation: refused.
    let b = root.join("b.txt");
    std::fs::write(&b, "new\n").unwrap();
    g(&["add", "b.txt"]);
    editors.open(&b);
    editors.type_at_end(&b, "typed\n");
    let refused = client
        .call(
            "git_rollback",
            json!({"paths": ["b.txt"], "delete_added": true, "dry_run": false}),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    assert!(
        error_message(&refused).contains("b.txt has unsaved changes in an editor: save it first"),
        "{refused}"
    );
    assert!(b.exists());
}

/// SPEC-0220 v5 AK-14: a bundle edit and a line separator change work on unsaved buffers; a change
/// of two open bundle files is all or nothing; converting the encoding of an unsaved file refuses.
#[tokio::test]
async fn bundle_and_line_separator_change_unsaved_buffers_and_encodings_refuse() {
    let (mut client, root, editors) = with_editors().await;
    let (base, de) = (
        root.join("Messages.properties"),
        root.join("Messages_de.properties"),
    );
    std::fs::write(&base, "greeting=Hello\n").unwrap();
    let de_disk = "greeting=Hallo\n";
    std::fs::write(&de, de_disk).unwrap();
    editors.open(&base);
    editors.open(&de);
    editors.type_at_end(&de, "bye=Tschuess\n");
    let set = client
        .call(
            "resource_bundle",
            json!({"path": "Messages.properties", "dry_run": false,
                   "set": {"key": "greeting", "locale": "de", "value": "Guten Tag"}}),
        )
        .await;
    assert!(!is_error(&set), "{set}");
    let shown = editors.text(&de);
    assert!(
        shown.contains("greeting=Guten Tag") && shown.contains("bye=Tschuess"),
        "{shown}"
    );
    assert_eq!(std::fs::read_to_string(&de).unwrap(), de_disk);
    assert!(editors.modified(&de));
    assert_eq!(editors.text(&base), "greeting=Hello\n");

    // The user types into the base file just before the window takes a change of both files:
    // refused as a whole, neither changes.
    *editors.type_first.lock().unwrap() = Some((base.clone(), "x=1\n".into()));
    let refused = client
        .call(
            "resource_bundle",
            json!({"path": "Messages.properties", "dry_run": false, "remove": "greeting"}),
        )
        .await;
    assert!(is_error(&refused), "{refused}");
    assert_eq!(editors.text(&de), shown);
    assert_eq!(editors.text(&base), "greeting=Hello\nx=1\n");

    // Line separators of the shown text.
    let crlf = root.join("crlf.txt");
    std::fs::write(&crlf, "a\r\nb\r\n").unwrap();
    editors.open(&crlf);
    editors.type_at_end(&crlf, "c\r\n");
    let converted = json_text(
        &client
            .call(
                "file_properties",
                json!({"path": "crlf.txt", "action": "set_line_separator",
                       "line_separator": "lf", "dry_run": false}),
            )
            .await,
    );
    assert_eq!(converted["changed"], 3, "{converted}");
    assert_eq!(editors.text(&crlf), "a\nb\nc\n");
    assert_eq!(std::fs::read_to_string(&crlf).unwrap(), "a\r\nb\r\n");
    assert!(editors.modified(&crlf));
    // Another encoding is no change of the shown text: refused while unsaved.
    let encoding = client
        .call(
            "file_properties",
            json!({"path": "crlf.txt", "action": "convert_encoding",
                   "encoding": "ISO-8859-1", "dry_run": false}),
        )
        .await;
    assert!(is_error(&encoding), "{encoding}");
    assert!(
        error_message(&encoding)
            .contains("crlf.txt has unsaved changes in an editor: save it first"),
        "{encoding}"
    );
}

/// SPEC-0220 v5 AK-15: deleting and renaming keep refusing; the tools of behaviours 17–19 left the
/// refusing list for the buffer.
#[test]
fn deleting_and_renaming_keep_refusing_and_the_rest_moved_to_the_buffer() {
    assert_eq!(REFUSES_UNSAVED.len(), 13);
    assert_eq!(REFUSES_UNSAVED_MAX, 13);
    for kept in ["delete_file", "rename_file"] {
        assert!(REFUSES_UNSAVED.contains(&kept), "{kept}");
    }
    for moved in [
        "compare_files",
        "file_properties",
        "git_line_change",
        "git_resolve_conflict",
        "git_rollback",
        "rename_sql_object",
        "resource_bundle",
    ] {
        assert!(THROUGH_BUFFER.contains(&moved), "{moved}");
        assert!(!REFUSES_UNSAVED.contains(&moved), "{moved}");
    }
}

/// SPEC-0220 v5 AK-14, behaviour 19: a SQL rename with `apply` computes the edits of an open
/// `.sql` file from its shown text, the typed reference included, and changes only the buffer.
#[tokio::test]
async fn a_sql_rename_changes_the_shown_text_of_an_unsaved_sql_file() {
    use super::protocol::write_sqlite_profile;
    let (mut client, root, editors) = with_editors().await;
    write_sqlite_profile(
        &root,
        "ai = [\"read-schema\", \"read-data\", \"modify-data\", \"modify-schema\"]",
    );
    let created = client
        .call(
            "run_sql",
            json!({"apply": true, "connection": "lite", "sql": "create table w (id integer primary key, city text)"}),
        )
        .await;
    assert!(!is_error(&created), "{created}");
    let q = root.join("q.sql");
    let disk = "select city from w;\n";
    std::fs::write(&q, disk).unwrap();
    editors.open(&q);
    editors.type_at_end(&q, "select id, city from w;\n");
    let typed = editors.text(&q);
    let applied = json_text(
        &client
            .call(
                "rename_sql_object",
                json!({"connection": "lite", "table": "w", "column": "city", "new_name": "town",
                       "apply": true}),
            )
            .await,
    );
    assert_eq!(applied["applied"], true, "{applied}");
    assert_eq!(
        editors.text(&q),
        "select town from w;\nselect id, town from w;\n"
    );
    assert_eq!(std::fs::read_to_string(&q).unwrap(), disk);
    assert!(editors.modified(&q));
    assert!(editors.undo(&q));
    assert_eq!(editors.text(&q), typed);
}

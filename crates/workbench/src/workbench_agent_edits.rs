//! Agents' edits of open files (`SPEC-0220` v2 behaviours 7 and 8): the window's MCP server hands
//! an edit over, and the editor of the file applies it as one undo step, so the user's Undo covers
//! it and nothing typed is lost. A change of several files (a language server's rename, a
//! refactoring) is checked for every file before any is applied (`SPEC-0220` v3 behaviour 14) and
//! kept as an undo group of the window (`SPEC-0220` v4 behaviour 15, `SPEC-0193` v5 behaviour 10).

use std::path::PathBuf;

use gpui::Entity;
use idecraft_editor::documents::{
    DiskChange, DocumentChange, DocumentEdit, DocumentEdited, DocumentRegistry,
};

use super::WorkbenchView;
use crate::editor_view::EditorView;

/// An editor's part of an agent's change: its project window, the file's registry key, the text
/// before and after, and the editor's undo step.
type AgentStep = (Entity<WorkbenchView>, PathBuf, String, String, Option<u64>);

impl WorkbenchView {
    /// Applies `edit` to an editor of its file in any project window, after checking that the
    /// file is still at the revision the agent read; the other views of the file follow as for
    /// typing (`SPEC-0112`). Returns the file's state afterwards, or why it was refused.
    pub fn edit_document_in(
        edit: &DocumentEdit,
        cx: &mut gpui::App,
    ) -> Result<DocumentEdited, String> {
        let registry = DocumentRegistry::global();
        registry.check_edit(edit)?;
        let view = crate::project_windows::workbenches(cx)
            .into_iter()
            .find_map(|bench| bench.read(cx).editor_of_key(&edit.path))
            .ok_or("no editor of a project window shows the file")?;
        view.update(cx, |v, cx| v.apply_agent_edit(&edit.text, edit.save, cx))?;
        let doc = registry
            .get(&edit.path)
            .ok_or("the file is no longer open in an editor")?;
        Ok(DocumentEdited {
            revision: doc.revision,
            modified: doc.modified,
        })
    }

    /// Applies the edits of one change of several open files, all or nothing (`SPEC-0220` v3
    /// behaviour 14): every file is checked first — still at the revision the agent read, shown by
    /// an editor of a project window that takes agent edits — and only then is each applied as one
    /// undo step of its editor. With a name the change is kept as an undo group of the project
    /// window whose editors show its files, so ⌘Z in one of them undoes it everywhere (`SPEC-0220`
    /// v4 behaviour 15, `SPEC-0193` v5 behaviour 10). Returns each file's state afterwards, or the
    /// first refused file and why; a refusal changes nothing.
    pub fn edit_documents_in(
        change: &DocumentChange,
        cx: &mut gpui::App,
    ) -> Result<Vec<DocumentEdited>, (PathBuf, String)> {
        let registry = DocumentRegistry::global();
        let benches = crate::project_windows::workbenches(cx);
        let mut views = Vec::new();
        for edit in &change.edits {
            let refused = |why: String| (edit.path.clone(), why);
            registry.check_edit(edit).map_err(refused)?;
            let (bench, view) = benches
                .iter()
                .find_map(|bench| Some((bench.clone(), bench.read(cx).editor_of_key(&edit.path)?)))
                .ok_or_else(|| refused("no editor of a project window shows the file".into()))?;
            if let Some(why) = view.read(cx).agent_edit_refusal() {
                return Err(refused(why.into()));
            }
            views.push((bench, view));
        }
        let mut done = Vec::new();
        let mut steps: Vec<AgentStep> = Vec::new();
        for (edit, (bench, view)) in change.edits.iter().zip(views) {
            let refused = |why: String| (edit.path.clone(), why);
            let before = view.read(cx).state.buffer.text();
            view.update(cx, |v, cx| v.apply_agent_edit(&edit.text, edit.save, cx))
                .map_err(refused)?;
            let doc = registry
                .get(&edit.path)
                .ok_or_else(|| refused("the file is no longer open in an editor".into()))?;
            done.push(DocumentEdited {
                revision: doc.revision,
                modified: doc.modified,
            });
            let buffer = &view.read(cx).state.buffer;
            let after = buffer.text();
            if after != before {
                let step = buffer.top_step();
                steps.push((bench, edit.path.clone(), before, after, step));
            }
        }
        if let Some(name) = &change.name {
            Self::keep_agent_group(name, steps, &change.on_disk, cx);
        }
        Ok(done)
    }

    /// Keeps an agent's change as an undo group of the project window whose main editors show
    /// every changed file (`SPEC-0193` v5 behaviour 10): `steps` are the editors' parts, `on_disk`
    /// the files written to disk. Files in several project windows, or only in a split, keep no
    /// group: each editor's Undo takes back its own step.
    fn keep_agent_group(
        name: &str,
        steps: Vec<AgentStep>,
        on_disk: &[DiskChange],
        cx: &mut gpui::App,
    ) {
        use idecraft_languages::undo_group::{Group, Part};
        let Some(bench) = steps.first().map(|s| s.0.clone()) else {
            return;
        };
        let mut parts = Vec::new();
        for (other, key, before, after, step) in steps {
            if other != bench {
                return;
            }
            let Some(path) = bench.read(cx).main_editor_path(&key) else {
                return;
            };
            parts.push(Part {
                path,
                before,
                after,
                step,
            });
        }
        parts.extend(on_disk.iter().map(|d| Part {
            path: d.path.canonicalize().unwrap_or_else(|_| d.path.clone()),
            before: d.before.clone(),
            after: d.after.clone(),
            step: None,
        }));
        bench.update(cx, |b, cx| {
            b.keep_refactoring_group(Group::new(name, parts), cx)
        });
    }

    /// The path under which the main editor group holds the editor of registry key `key`.
    fn main_editor_path(&self, key: &std::path::Path) -> Option<PathBuf> {
        self.editors
            .keys()
            .find(|p| DocumentRegistry::key(p) == key)
            .cloned()
    }

    /// The editor view of the file with registry key `key`: the main group's first, then a
    /// split's.
    fn editor_of_key(&self, key: &std::path::Path) -> Option<Entity<EditorView>> {
        let same = |p: &PathBuf| DocumentRegistry::key(p) == key;
        self.editors
            .iter()
            .chain(self.groups.iter().flat_map(|s| s.editors.iter()))
            .find(|(p, _)| same(p))
            .map(|(_, e)| e.clone())
    }
}

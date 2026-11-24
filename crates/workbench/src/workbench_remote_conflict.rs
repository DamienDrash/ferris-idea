//! A file opened from an SSH host that changed there since it was read (`SPEC-0121` v7
//! behaviour 4a): the question after a save, and Merge, Compare, Overwrite and Cancel. Nothing is
//! sent before an answer that sends, and every upload is checked against the host version the
//! user was shown, so a third change on the host asks again.

use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Window};

use super::{DrawnFor, WorkbenchView};
use crate::remote_conflict::{ConflictAnswer, question};
use crate::{DiffView, MergeEvent, MergeView};

/// Headers of the copy's and the host's side in the diff and the merge view.
const SIDES: (&str, &str) = ("Your copy", "On the host");

fn name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

impl WorkbenchView {
    /// Asks what to do with the saved copy `key` whose host file changed.
    pub(super) fn ask_remote_conflict(&mut self, key: &Path, cx: &mut Context<Self>) {
        let Some(opened) = self.remote_files.get(key) else {
            return;
        };
        let Some((host, _)) = &opened.host else {
            return;
        };
        let ours = std::fs::read(key).unwrap_or_default();
        let label = format!("{}:{}", opened.remote.target.host, opened.path);
        let (q, answers) = question(
            &name_of(&opened.path),
            &label,
            &opened.base,
            &ours,
            host.as_deref(),
        );
        self.drawn_question = Some((q, DrawnFor::RemoteConflict(key.to_path_buf(), answers)));
        cx.notify();
    }

    /// Carries out `answer` for the copy `key`.
    pub(super) fn answer_remote_conflict(
        &mut self,
        key: PathBuf,
        answer: ConflictAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(opened) = self.remote_files.get(&key).cloned() else {
            return;
        };
        let Some((host, host_stamp)) = opened.host.clone() else {
            return;
        };
        let label = format!("{}:{}", opened.remote.target.host, opened.path);
        let name = name_of(&opened.path);
        let ours = std::fs::read(&key).unwrap_or_default();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        match answer {
            ConflictAnswer::Cancel => {
                self.status = Some(format!(
                    "Nothing was sent to {label}: your copy keeps its edits and the host keeps its version. Saving again asks again"
                ));
                self.focus_active(window, cx);
            }
            ConflictAnswer::Compare => {
                let host_text = host.as_deref().map(text).unwrap_or_default();
                let rows = ferris_idea_vcs::side_by_side(&text(&ours), &host_text);
                let view = cx.new(|cx| {
                    DiffView::new(format!("{name}: your copy and {label}"), rows, cx)
                        .with_sides(SIDES.0, SIDES.1)
                });
                self.present_diff(view, window, cx);
                self.status = Some(format!(
                    "Nothing was sent to {label}. Save again to merge or overwrite"
                ));
            }
            ConflictAnswer::Overwrite => {
                // The copy goes over the version the user was shown, and only over that one.
                if let Some(e) = self.remote_files.get_mut(&key) {
                    e.stamp = host_stamp;
                    e.base = host.unwrap_or_default();
                    e.host = None;
                }
                self.upload_remote(&key, cx);
            }
            ConflictAnswer::Merge => {
                let Some(host) = host else {
                    return;
                };
                let texts = [text(&opened.base), text(&ours), text(&host)];
                let sides = (SIDES.0.to_owned(), SIDES.1.to_owned());
                let target = key.clone();
                let view = cx.new(|cx| MergeView::for_texts(target, name, texts, sides, cx));
                self._merge_events = Some(cx.subscribe_in(
                    &view,
                    window,
                    move |this, _, event: &MergeEvent, window, cx| match event {
                        MergeEvent::Close | MergeEvent::Finished => {
                            this.merge = None;
                            this.focus_active(window, cx);
                            cx.notify();
                        }
                        MergeEvent::Resolved(path, Some(text)) => {
                            this.merged_remote(path, text, host_stamp.clone(), host.clone(), cx);
                        }
                        MergeEvent::Resolved(_, None) => {}
                    },
                ));
                window.focus(&view.read(cx).focus());
                self.merge = Some(view);
                self.status = Some(format!(
                    "Merging your copy with {label}: Apply uploads the result, closing sends nothing"
                ));
            }
        }
        cx.notify();
    }

    /// The merge of the copy `key` with the host version `host` (stamp `stamp`) was applied: the
    /// editor takes the result, which is saved and uploaded over that host version only.
    fn merged_remote(
        &mut self,
        key: &Path,
        text: &str,
        stamp: String,
        host: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        if let Some(e) = self.remote_files.get_mut(key) {
            e.stamp = stamp;
            e.base = host;
            e.host = None;
        }
        match self.editors.get(key).cloned() {
            Some(editor) => {
                // An undoable change in the editor; saving it uploads (behaviour 4).
                editor.update(cx, |e, cx| {
                    let len = e.state.buffer.len_chars();
                    e.replace_ranges(&[(0..len, text.to_owned())], cx);
                    e.run_action("SaveDocument", cx);
                });
            }
            None => {
                if std::fs::write(key, text).is_ok() {
                    self.upload_remote(key, cx);
                }
            }
        }
        cx.notify();
    }
}

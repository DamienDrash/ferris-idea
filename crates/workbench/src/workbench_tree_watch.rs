//! The Project tree follows the disk (`SPEC-0059` v9.11 behaviour 31): files and folders that
//! another program creates, deletes or renames in a folder the tree has read show within about a
//! second, while the window stays in front and without Reload All from Disk.

use std::time::Duration;

use gpui::{AppContext, Context};

use super::WorkbenchView;
use crate::project_tree::ProjectTree;

/// Time between two looks at the folders the tree has read.
pub const TREE_WATCH_INTERVAL: Duration = Duration::from_millis(500);

impl WorkbenchView {
    /// Starts looking at the folders the tree has read every [`TREE_WATCH_INTERVAL`], off the UI
    /// thread; when the disk differs, the tree is read again keeping its expanded folders and
    /// selection. A look that overlaps a reading of the tree by the window itself is dropped.
    /// Remote projects are not looked at (their tree has no folders of this machine).
    pub(crate) fn watch_tree_folders(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TREE_WATCH_INTERVAL).await;
                let Ok((read, generation)) = this.update(cx, |this, _| {
                    (this.tree.read_folders(), this.tree.generation())
                }) else {
                    break;
                };
                if read.is_empty() {
                    continue;
                }
                let differs = cx
                    .background_spawn(async move { ProjectTree::disk_differs(&read) })
                    .await;
                if !differs {
                    continue;
                }
                let alive = this.update(cx, |this, cx| {
                    if this.tree.generation() == generation {
                        this.tree = this.tree.reloaded();
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

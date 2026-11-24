//! The costs of the profile shown, next to the lines in the editor's gutter (`SPEC-0120` v5
//! behaviour 10): a column right of the Git annotations, there while the profile has costs for
//! this file and the text is the one profiled (its SHA-256 equals the digest kept with the
//! profile). A click asks the window to show the line's function in the profiler
//! (`EditorEvent::ProfileLine`); a tooltip and the accessible name give share, time and function.

use std::sync::Arc;

use gpui::{Context, Pixels, div, prelude::*};

use super::{EditorEvent, EditorView};
use crate::profile_lines_model::FileCosts;
use crate::semantics::{Annotated, Role, Semantics};

/// The test id of the cost on 0-based `line`: `profile-cost-` and the line counted from 1, as the
/// editor shows it.
pub fn profile_cost_id(line: usize) -> String {
    format!("profile-cost-{}", line + 1)
}

impl EditorView {
    /// Shows the costs of the profile for this file, or none.
    pub fn set_profile_costs(&mut self, costs: Option<Arc<FileCosts>>, cx: &mut Context<Self>) {
        let same = match (&self.profile_costs, &costs) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.profile_costs = costs;
            self.profile_match = None;
            cx.notify();
        }
    }

    /// The costs set for this file, shown or not.
    pub fn profile_costs(&self) -> Option<&Arc<FileCosts>> {
        self.profile_costs.as_ref()
    }

    /// Whether the text is the one profiled, checked once per revision.
    pub(super) fn profile_costs_match(&mut self) -> bool {
        let Some(costs) = &self.profile_costs else {
            return false;
        };
        match self.profile_match {
            Some((rev, matches)) if rev == self.revision => matches,
            _ => {
                let text = self.state.buffer.text();
                let matches = ferris_idea_core::sha256_hex(text.as_bytes()) == costs.digest;
                self.profile_match = Some((self.revision, matches));
                matches
            }
        }
    }

    /// The costs shown now, by 0-based line, with their labels: none once the text differs from
    /// the one profiled.
    pub fn profile_costs_shown(&mut self) -> Vec<(usize, String)> {
        if self.field || !self.profile_costs_match() {
            return Vec::new();
        }
        self.profile_costs
            .as_ref()
            .map(|c| c.lines.iter().map(|l| (l.line, l.label.clone())).collect())
            .unwrap_or_default()
    }

    /// Characters the column needs: the longest label.
    pub(super) fn profile_width_chars(&mut self) -> usize {
        if self.field || !self.profile_costs_match() {
            return 0;
        }
        self.profile_costs
            .as_ref()
            .and_then(|c| c.lines.iter().map(|l| l.label.chars().count()).max())
            .unwrap_or(0)
    }

    /// A click on a cost of the last frame: the window shows its function in the profiler.
    pub(super) fn profile_cost_click(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(line) = self
            .profile_hits
            .iter()
            .find(|(b, _)| b.contains(&position))
            .map(|(_, line)| *line)
        else {
            return false;
        };
        cx.emit(EditorEvent::ProfileLine(line));
        cx.stop_propagation();
        true
    }

    /// Invisible boxes over the costs of the last frame: their tooltip and their node in the
    /// semantic tree, named with share, time and function; they take no clicks.
    pub(super) fn profile_cost_overlays(&self) -> Vec<gpui::AnyElement> {
        use crate::tips::Tipped;
        let Some(costs) = &self.profile_costs else {
            return Vec::new();
        };
        let origin = self.last.origin;
        self.profile_hits
            .iter()
            .filter_map(|(b, line)| {
                let mark = costs.lines.iter().find(|l| l.line == *line)?;
                let id = profile_cost_id(*line);
                let tip = ferris_idea_ui::tooltip::Tip {
                    title: mark.detail.clone(),
                    shortcut: String::new(),
                    details: Vec::new(),
                };
                Some(
                    div()
                        .id(gpui::SharedString::from(format!("{id}-tip")))
                        .absolute()
                        .left(b.origin.x - origin.x)
                        .top(b.origin.y - origin.y)
                        .w(b.size.width)
                        .h(b.size.height)
                        .semantics(
                            Semantics::new(Role::Button)
                                .name(mark.detail.clone())
                                .test_id(id.clone()),
                        )
                        .tip_text(format!("{id}-tip"), tip)
                        .into_any_element(),
                )
            })
            .collect()
    }
}

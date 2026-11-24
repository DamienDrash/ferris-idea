//! Keyboard focus on the gutter icons (`EditorFocusGutter`, `SPEC-0058` v5 behaviour 13): the
//! icons drawn in the visible part of the editor, line by line and left to right; ↑ ↓ go to the
//! previous and next line with icons, ← → along the icons of a line, Enter and Space do what a
//! click does, Escape gives the keys back to the text.

use gpui::{Bounds, Context, Pixels, point};

use super::{EditorEvent, EditorView, GutterIcon};

/// The status text when the visible part of the editor has no gutter icon.
pub const NO_GUTTER_ICONS_SHOWN: &str = "No gutter icons in the visible part of the editor";

/// The gutter icons of a frame by line, each line's icons left to right, lines from top to bottom.
pub(super) fn icon_rows(drawn: &[(Bounds<Pixels>, GutterIcon)]) -> Vec<(usize, Vec<GutterIcon>)> {
    let mut sorted: Vec<&(Bounds<Pixels>, GutterIcon)> = drawn.iter().collect();
    sorted.sort_by(|a, b| {
        a.1.line().cmp(&b.1.line()).then(
            a.0.origin
                .x
                .partial_cmp(&b.0.origin.x)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let mut rows: Vec<(usize, Vec<GutterIcon>)> = Vec::new();
    for (_, icon) in sorted {
        match rows.last_mut() {
            Some((line, icons)) if *line == icon.line() => {
                if !icons.contains(icon) {
                    icons.push(*icon);
                }
            }
            _ => rows.push((icon.line(), vec![*icon])),
        }
    }
    rows
}

/// Where the focus starts: the first icon of the caret's line, else of the nearest line below
/// it, else of the nearest line above.
pub(super) fn start(
    rows: &[(usize, Vec<GutterIcon>)],
    caret_line: usize,
) -> Option<(usize, usize)> {
    rows.iter()
        .find(|(l, _)| *l >= caret_line)
        .or_else(|| rows.last())
        .map(|(l, _)| (*l, 0))
}

/// A move of the focus: ↑ ↓ to the previous and next line with icons (keeping the place in the
/// line where it can), ← → along the line's icons; it stops at the ends.
pub(super) fn step(
    rows: &[(usize, Vec<GutterIcon>)],
    at: (usize, usize),
    key: &str,
) -> (usize, usize) {
    let Some(row) = rows.iter().position(|(l, _)| *l == at.0) else {
        return start(rows, at.0).unwrap_or(at);
    };
    let len = rows[row].1.len();
    match key {
        "UP" if row > 0 => (rows[row - 1].0, at.1.min(rows[row - 1].1.len() - 1)),
        "DOWN" if row + 1 < rows.len() => (rows[row + 1].0, at.1.min(rows[row + 1].1.len() - 1)),
        "LEFT" => (at.0, at.1.saturating_sub(1)),
        "RIGHT" => (at.0, (at.1 + 1).min(len.saturating_sub(1))),
        _ => (at.0, at.1.min(len.saturating_sub(1))),
    }
}

impl EditorView {
    /// `EditorFocusGutter`: puts the keys on the first gutter icon of the caret's line or the
    /// nearest line with one; says so in the status line when none is drawn.
    pub(super) fn focus_gutter(&mut self, cx: &mut Context<Self>) -> bool {
        if self.field {
            return false;
        }
        let rows = icon_rows(&self.last.gutter_tips);
        let caret_line = self.line_of(self.state.carets.primary().head);
        match start(&rows, caret_line) {
            Some(at) => self.gutter_focus = Some(at),
            None => cx.emit(EditorEvent::Note(NO_GUTTER_ICONS_SHOWN.into())),
        }
        cx.notify();
        true
    }

    /// The line and place of the focused gutter icon, if the gutter has the keys.
    pub fn gutter_focus(&self) -> Option<(usize, usize)> {
        self.gutter_focus
    }

    /// The tooltip of the focused gutter icon (its name and what a click does).
    pub fn focused_gutter_tip(&self) -> Option<ferris_idea_ui::tooltip::Tip> {
        let icon = self.focused_icon()?;
        Some(self.gutter_icon_tip(icon).1)
    }

    fn focused_icon(&self) -> Option<GutterIcon> {
        let (line, i) = self.gutter_focus?;
        icon_rows(&self.last.gutter_tips)
            .into_iter()
            .find(|(l, _)| *l == line)
            .and_then(|(_, icons)| icons.get(i).copied())
    }

    /// Where the focused icon is drawn, for its focus ring.
    pub(super) fn focused_gutter_bounds(
        &self,
        drawn: &[(Bounds<Pixels>, GutterIcon)],
    ) -> Option<Bounds<Pixels>> {
        let (line, i) = self.gutter_focus?;
        let icon = icon_rows(drawn)
            .into_iter()
            .find(|(l, _)| *l == line)
            .and_then(|(_, icons)| icons.get(i).copied())?;
        drawn.iter().find(|(_, d)| *d == icon).map(|(b, _)| *b)
    }

    /// A key while the gutter has the keys: true when it was the gutter's. Another key gives
    /// the keys back to the text and is not taken.
    pub(super) fn gutter_key(&mut self, key: &str, plain: bool, cx: &mut Context<Self>) -> bool {
        let Some(at) = self.gutter_focus else {
            return false;
        };
        match key {
            "UP" | "DOWN" | "LEFT" | "RIGHT" if plain => {
                let rows = icon_rows(&self.last.gutter_tips);
                self.gutter_focus = Some(step(&rows, at, key));
            }
            "ENTER" | "SPACE" if plain => {
                if let Some(icon) = self.focused_icon() {
                    self.activate_gutter_icon(icon, cx);
                }
            }
            "ESCAPE" => self.gutter_focus = None,
            _ => {
                self.gutter_focus = None;
                cx.notify();
                return false;
            }
        }
        cx.notify();
        true
    }

    /// What a click on `icon` does: run what a ▶ marks, remove a breakpoint, fold or unfold, go
    /// to a declaration's or a bean's other side; a bookmark's tooltip is all it has.
    fn activate_gutter_icon(&mut self, icon: GutterIcon, cx: &mut Context<Self>) {
        match icon {
            GutterIcon::Run(line) => cx.emit(EditorEvent::RunMarker(line)),
            GutterIcon::Breakpoint(line) => {
                self.toggle_breakpoint(line, cx);
                self.gutter_focus = None;
            }
            GutterIcon::Fold(line, _) => {
                let at = self
                    .last
                    .gutter_tips
                    .iter()
                    .find(|(_, i)| *i == icon)
                    .map(|(b, _)| {
                        point(
                            b.origin.x + b.size.width / 2.0,
                            b.origin.y + b.size.height / 2.0,
                        )
                    });
                if let Some(at) = at
                    && self.fold_click(at)
                {
                    // The arrow now shows the other state: keep the focus on it.
                    self.gutter_focus = Some((line, self.gutter_focus.map_or(0, |f| f.1)));
                }
            }
            GutterIcon::Declaration(line) => cx.emit(EditorEvent::DeclarationMark(line)),
            GutterIcon::Framework(line) => cx.emit(EditorEvent::FrameworkMark(line)),
            // A swatch opens no colour picker yet (`SPEC-0042` v27 behaviour 48).
            GutterIcon::Bookmark(_) | GutterIcon::Color(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    fn at(x: f32, icon: GutterIcon) -> (Bounds<Pixels>, GutterIcon) {
        (
            Bounds::new(
                point(px(x), px(icon.line() as f32 * 20.0)),
                size(px(12.), px(20.)),
            ),
            icon,
        )
    }

    /// SPEC-0058 v5 AK-8: the icons by line, left to right; start and moves.
    #[test]
    fn the_gutter_icons_are_walked_by_line_and_place() {
        let drawn = vec![
            at(40., GutterIcon::Fold(3, false)),
            at(4., GutterIcon::Breakpoint(3)),
            at(28., GutterIcon::Run(3)),
            at(28., GutterIcon::Run(9)),
            at(16., GutterIcon::Declaration(1)),
        ];
        let rows = icon_rows(&drawn);
        assert_eq!(
            rows,
            vec![
                (1, vec![GutterIcon::Declaration(1)]),
                (
                    3,
                    vec![
                        GutterIcon::Breakpoint(3),
                        GutterIcon::Run(3),
                        GutterIcon::Fold(3, false)
                    ]
                ),
                (9, vec![GutterIcon::Run(9)]),
            ]
        );
        assert_eq!(start(&rows, 3), Some((3, 0)), "the caret's line");
        assert_eq!(start(&rows, 2), Some((3, 0)), "the next line below");
        assert_eq!(start(&rows, 12), Some((9, 0)), "else the last line above");
        assert_eq!(start(&[], 0), None);
        assert_eq!(step(&rows, (3, 0), "RIGHT"), (3, 1));
        assert_eq!(step(&rows, (3, 2), "RIGHT"), (3, 2), "stops at the end");
        assert_eq!(step(&rows, (3, 0), "LEFT"), (3, 0));
        assert_eq!(
            step(&rows, (3, 2), "DOWN"),
            (9, 0),
            "the place shrinks to the line"
        );
        assert_eq!(step(&rows, (3, 1), "UP"), (1, 0));
        assert_eq!(step(&rows, (1, 0), "UP"), (1, 0), "stops at the top");
    }
}

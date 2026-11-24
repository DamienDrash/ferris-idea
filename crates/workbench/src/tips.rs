//! Tooltips on the window's controls (`SPEC-0294`): `.tip(declared!(AREA, "name"))` gives an element
//! the tooltip of its declared control, `.tip_text(name, tip)` one made for this moment. Both draw
//! the shared view of `ferris_idea_ui::tooltip`, note the control for the inspection (MCP
//! `ui_tooltips`, window self-test) and mark it for the self-test's pointer.

use ferris_idea_ui::tooltip::{self, Control, Tip, view::TooltipView};
use gpui::{App, ParentElement, StatefulInteractiveElement};

pub use ferris_idea_ui::declared;

/// Whether shortcuts are written the macOS way.
pub fn macos() -> bool {
    cfg!(target_os = "macos")
}

/// The tooltip view saying `tip` in the current theme.
pub fn view(tip: Tip, cx: &mut App) -> gpui::AnyView {
    TooltipView::build(tip, crate::palette::Palette::current().tooltip, cx)
}

/// Hides shown tooltips on every key press (behaviour 6).
pub fn install(cx: &mut App) {
    cx.intercept_keystrokes(|_, window, _| {
        if tooltip::shown().is_some() {
            tooltip::dismiss_all();
            window.refresh();
        } else {
            tooltip::dismiss_all();
        }
    })
    .detach();
}

/// Hides the shown tooltip when its control is pressed: a button that opens a menu or popup left
/// its tooltip standing next to it (`SPEC-0294` v4 behaviour 6). Runs in the capture phase, so a
/// control that stops the event still hides it.
fn hide_on_press(_: &gpui::MouseDownEvent, window: &mut gpui::Window, _: &mut App) {
    if tooltip::shown().is_some() {
        tooltip::dismiss_all();
        window.refresh();
    }
}

/// The tooltip of a control, as an element builder.
pub trait Tipped: Sized {
    /// The tooltip of the declared `control`, its text made from the keymap in use when it appears.
    fn tip(self, control: &'static Control) -> Self;
    /// A tooltip made for this moment for the control `name`.
    fn tip_text(self, name: impl Into<String>, tip: Tip) -> Self;
    /// A tooltip for the control `name` made by `make` when it appears (its shortcut from the
    /// keymap in use at that moment).
    fn tip_with(self, name: impl Into<String>, make: impl Fn() -> Tip + 'static) -> Self;
}

impl<E: StatefulInteractiveElement + ParentElement> Tipped for E {
    fn tip(self, control: &'static Control) -> Self {
        tooltip::declare_control(control);
        self.capture_any_mouse_down(hide_on_press)
            .children(crate::hit_targets::mark(control.name))
            .tooltip(move |_, cx| view(Tip::of(control, &crate::keymaps::current(), macos()), cx))
    }

    fn tip_text(self, name: impl Into<String>, tip: Tip) -> Self {
        let name = name.into();
        // An empty tip marks the control without a tooltip (a tab without its path, SPEC-0059 v9
        // behaviour 17).
        let shown = !(tip.title.is_empty() && tip.details.is_empty());
        if shown {
            tooltip::declare(&name, &tip);
        }
        let marked = self
            .capture_any_mouse_down(hide_on_press)
            .children(crate::hit_targets::mark(name));
        if shown {
            marked.tooltip(move |_, cx| view(tip.clone(), cx))
        } else {
            marked
        }
    }

    fn tip_with(self, name: impl Into<String>, make: impl Fn() -> Tip + 'static) -> Self {
        let name = name.into();
        tooltip::declare(&name, &make());
        self.capture_any_mouse_down(hide_on_press)
            .children(crate::hit_targets::mark(name))
            .tooltip(move |_, cx| view(make(), cx))
    }
}

/// The tooltip of a breakpoint with `props` (`SPEC-0294` behaviour 4), and why the running
/// session did not bind it (`SPEC-0127` v5 behaviour 14).
pub fn breakpoint_tip(props: &crate::debug_model::BreakpointProps, not_bound: Option<&str>) -> Tip {
    tooltip::breakpoint(tooltip::BreakpointFacts {
        disabled: props.disabled,
        condition: props.condition.as_deref(),
        hit_condition: props.hit_condition.as_deref(),
        log_message: props.log_message.as_deref(),
        not_bound,
    })
}

/// The tooltips of the breakpoints of one file with properties or not bound, by 0-based line;
/// others say they are plain line breakpoints.
pub fn breakpoint_tips(
    props: Option<&std::collections::BTreeMap<usize, crate::debug_model::BreakpointProps>>,
    unbound: Option<&std::collections::BTreeMap<usize, String>>,
) -> std::collections::BTreeMap<usize, Tip> {
    let plain = crate::debug_model::BreakpointProps::default();
    let lines: std::collections::BTreeSet<usize> = props
        .into_iter()
        .flat_map(|m| m.keys())
        .chain(unbound.into_iter().flat_map(|m| m.keys()))
        .copied()
        .collect();
    lines
        .into_iter()
        .map(|l| {
            let p = props.and_then(|m| m.get(&l)).unwrap_or(&plain);
            let reason = unbound.and_then(|m| m.get(&l)).map(String::as_str);
            (l, breakpoint_tip(p, reason))
        })
        .collect()
}

/// The tooltips of a line's gutter icons as the hint below the caret shows them (`SPEC-0294`
/// v5 behaviour 12): one icon after the other, its title with the shortcut in brackets, then
/// its detail lines; an icon without any text is left out.
pub fn gutter_tips_text(tips: &[Tip]) -> String {
    tips.iter()
        .filter_map(|t| {
            let mut lines = Vec::new();
            let title = t.title.trim();
            match (title.is_empty(), t.shortcut.is_empty()) {
                (false, false) => lines.push(format!("{title} ({})", t.shortcut)),
                (false, true) => lines.push(title.to_owned()),
                (true, false) => lines.push(t.shortcut.clone()),
                (true, true) => {}
            }
            lines.extend(t.details.iter().filter(|d| !d.trim().is_empty()).cloned());
            (!lines.is_empty()).then(|| lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0294 AK-12 (v5): the tips of a line read as one text, icon after icon.
    #[test]
    fn gutter_tips_read_as_one_text() {
        let run = Tip {
            title: "Run 'main'".into(),
            shortcut: "⌃⇧R".into(),
            details: Vec::new(),
        };
        let mark = Tip {
            title: "Bookmark".into(),
            shortcut: String::new(),
            details: vec!["Mnemonic 3".into(), " ".into()],
        };
        assert_eq!(
            gutter_tips_text(std::slice::from_ref(&run)),
            "Run 'main' (⌃⇧R)"
        );
        assert_eq!(
            gutter_tips_text(&[run, Tip::plain(""), mark]),
            "Run 'main' (⌃⇧R)\n\nBookmark\nMnemonic 3"
        );
        assert_eq!(gutter_tips_text(&[]), "");
    }

    // SPEC-0294 AK-4: the breakpoint's tooltip comes from its saved properties.
    #[test]
    fn breakpoint_tips_come_from_the_properties() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            4,
            crate::debug_model::BreakpointProps {
                condition: Some("n == 2".into()),
                ..Default::default()
            },
        );
        props.insert(
            9,
            crate::debug_model::BreakpointProps {
                log_message: Some("n is {n}".into()),
                disabled: true,
                ..Default::default()
            },
        );
        let tips = breakpoint_tips(Some(&props), None);
        assert_eq!(
            tips[&4].text(),
            "Line breakpoint\nCondition: n == 2\nRight click to edit"
        );
        assert_eq!(
            tips[&9].text(),
            "Line breakpoint (disabled)\nLogs: n is {n}\nDoes not suspend\nRight click to edit"
        );
        assert!(breakpoint_tips(None, None).is_empty());
        // SPEC-0127 v5: a plain breakpoint the session did not bind gets a tip of its own.
        let unbound = std::collections::BTreeMap::from([(2, "no code".to_owned())]);
        let tips = breakpoint_tips(None, Some(&unbound));
        assert_eq!(
            tips[&2].text(),
            "Line breakpoint\nNot bound: no code\nRight click to edit"
        );
    }
}

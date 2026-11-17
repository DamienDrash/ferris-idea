//! Clone Caret Above/Below by the doubled multi-caret modifier (`SPEC-0025` v5 behaviour 9): the
//! modifier (⌥ on macOS, Ctrl elsewhere) pressed, released and pressed again, each step within
//! 300 ms of the one before, and held; while it stays held ↑ and ↓ clone the carets.

use std::time::{Duration, Instant};

/// The longest time between two steps of the gesture.
const STEP: Duration = Duration::from_millis(300);

/// Where the gesture stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    /// Nothing pressed yet, or the gesture ended.
    #[default]
    Idle,
    /// The modifier is down for the first time.
    FirstDown(Instant),
    /// It was released once.
    Released(Instant),
    /// It is down for the second time and held: the arrows clone carets.
    Armed,
}

/// The state of the gesture in one editor.
#[derive(Debug, Clone, Default)]
pub struct CaretGesture {
    stage: Stage,
}

impl CaretGesture {
    /// The modifiers changed at `now`: `alone` is true when the multi-caret modifier is held and
    /// no other one, `none` when no modifier is held.
    pub fn modifiers(&mut self, alone: bool, none: bool, now: Instant) {
        let within = |at: Instant| now.duration_since(at) <= STEP;
        self.stage = match (self.stage, alone, none) {
            (Stage::Armed, true, _) => Stage::Armed,
            (Stage::Released(at), true, _) if within(at) => Stage::Armed,
            (_, true, _) => Stage::FirstDown(now),
            (Stage::FirstDown(at), false, true) if within(at) => Stage::Released(now),
            _ => Stage::Idle,
        };
    }

    /// Whether ↑ and ↓ with the modifier clone carets now.
    pub fn armed(&self) -> bool {
        self.stage == Stage::Armed
    }

    /// A key that is no clone step was pressed: the gesture ends.
    pub fn key(&mut self) {
        self.stage = Stage::Idle;
    }

    /// The clone action for a key pressed while the gesture is armed: `up` or `down` with the
    /// multi-caret modifier alone (`alone`).
    pub fn action_for(&self, key: &str, alone: bool) -> Option<&'static str> {
        if !self.armed() || !alone {
            return None;
        }
        match key {
            "up" => Some("EditorCloneCaretAbove"),
            "down" => Some("EditorCloneCaretBelow"),
            _ => None,
        }
    }
}

/// Whether the multi-caret modifier is held alone: ⌥ on macOS, Ctrl elsewhere.
pub fn modifier_alone(m: &gpui::Modifiers) -> bool {
    let (wanted, others) = if cfg!(target_os = "macos") {
        (m.alt, m.control || m.shift || m.platform || m.function)
    } else {
        (m.control, m.alt || m.shift || m.platform || m.function)
    };
    wanted && !others
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(start: Instant, n: u64) -> Instant {
        start + Duration::from_millis(n)
    }

    /// SPEC-0025 AK-13 (v5): down, up, down within 300 ms arms the gesture; it stays armed while
    /// the modifier is held and for every arrow; release, a slow step, another modifier and
    /// another key end it.
    #[test]
    fn the_doubled_modifier_arms_the_gesture() {
        let t = Instant::now();
        let mut g = CaretGesture::default();
        g.modifiers(true, false, t);
        assert!(!g.armed(), "one press is no gesture");
        assert_eq!(g.action_for("down", true), None);
        g.modifiers(false, true, ms(t, 100));
        assert!(!g.armed());
        g.modifiers(true, false, ms(t, 200));
        assert!(g.armed());
        assert_eq!(g.action_for("down", true), Some("EditorCloneCaretBelow"));
        assert_eq!(g.action_for("up", true), Some("EditorCloneCaretAbove"));
        assert_eq!(g.action_for("left", true), None, "only ↑ and ↓");
        assert_eq!(g.action_for("down", false), None, "with ⇧ also held");
        // The arrows leave it armed, even long after.
        g.modifiers(true, false, ms(t, 5_000));
        assert!(g.armed(), "held on");
        // Releasing ends it.
        g.modifiers(false, true, ms(t, 5_100));
        assert!(!g.armed());

        // Too slow between the steps.
        let mut g = CaretGesture::default();
        g.modifiers(true, false, t);
        g.modifiers(false, true, ms(t, 400));
        g.modifiers(true, false, ms(t, 450));
        assert!(!g.armed(), "held too long the first time");
        let mut g = CaretGesture::default();
        g.modifiers(true, false, t);
        g.modifiers(false, true, ms(t, 100));
        g.modifiers(true, false, ms(t, 500));
        assert!(!g.armed(), "pressed again too late");
        // A slow second press counts as a new first one.
        g.modifiers(false, true, ms(t, 600));
        g.modifiers(true, false, ms(t, 700));
        assert!(g.armed());

        // Another modifier in between, or a key, ends it.
        let mut g = CaretGesture::default();
        g.modifiers(true, false, t);
        g.modifiers(false, false, ms(t, 50));
        g.modifiers(false, true, ms(t, 100));
        g.modifiers(true, false, ms(t, 150));
        assert!(!g.armed(), "⇧ joined the first press");
        let mut g = CaretGesture::default();
        g.modifiers(true, false, t);
        g.modifiers(false, true, ms(t, 50));
        g.key();
        g.modifiers(true, false, ms(t, 100));
        assert!(!g.armed(), "a key between the presses");
        g.modifiers(false, true, ms(t, 150));
        g.modifiers(true, false, ms(t, 200));
        assert!(g.armed());
        g.key();
        assert!(!g.armed(), "a key that is no clone step");
    }

    /// The modifier is ⌥ on macOS and Ctrl elsewhere, held alone.
    #[test]
    fn the_modifier_depends_on_the_platform() {
        let alt = gpui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let control = gpui::Modifiers {
            control: true,
            ..Default::default()
        };
        let both = gpui::Modifiers {
            alt: true,
            control: true,
            ..Default::default()
        };
        assert_eq!(modifier_alone(&alt), cfg!(target_os = "macos"));
        assert_eq!(modifier_alone(&control), !cfg!(target_os = "macos"));
        assert!(!modifier_alone(&both));
        assert!(!modifier_alone(&gpui::Modifiers::default()));
    }
}

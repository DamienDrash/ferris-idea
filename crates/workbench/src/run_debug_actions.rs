//! The decisions behind the run, debug and window actions that exist under their keymap id
//! because the bundled keymaps bind them (`docs/research/KEYMAP_COVERAGE.md`): which tabs Rerun
//! Tests runs again (`SPEC-0088` v4 behaviour 11) and which project window comes after or before
//! the current one (`SPEC-0094` v2 behaviour 5). No window is needed here; the temporary line
//! breakpoint is in `ferris_idea_debugger::saved` (`SPEC-0127` v8) and the chooser opened for
//! debugging in [`crate::run_list_model`] (`SPEC-0087` v8).

use ferris_idea_core::{RunConfig, RunKind};

/// One run Rerun Tests starts again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRerun {
    /// The configuration to run.
    pub config: String,
    /// The title of the tab it runs in: the tab's own, so a further instance stays in its tab.
    pub tab: String,
}

/// The runs Rerun Tests starts again, in tab order: one per tab (`(title, configuration)`) whose
/// configuration is a test configuration. A program's tab and a tab whose configuration no
/// longer exists are left as they are (`SPEC-0088` v4 behaviour 11).
pub fn test_reruns(tabs: &[(String, String)], configs: &[RunConfig]) -> Vec<TestRerun> {
    tabs.iter()
        .filter(|(_, config)| {
            configs
                .iter()
                .any(|c| &c.name == config && c.kind == RunKind::Test)
        })
        .map(|(title, config)| TestRerun {
            config: config.clone(),
            tab: title.clone(),
        })
        .collect()
}

/// The window Next (`forward`) or Previous Project Window brings to the front: the one opened
/// after or before `current` in `opened` (opening order, closed windows left out), wrapping at
/// both ends. `None` with a single window, or when `current` is not a project window
/// (`SPEC-0094` v2 behaviour 5).
pub fn step_window<T: Copy + PartialEq>(opened: &[T], current: T, forward: bool) -> Option<T> {
    let at = opened.iter().position(|w| *w == current)?;
    let n = opened.len();
    if n < 2 {
        return None;
    }
    let next = if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    };
    Some(opened[next])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str, kind: RunKind) -> RunConfig {
        RunConfig {
            name: name.into(),
            command: vec!["true".into()],
            cwd: None,
            env: Default::default(),
            kind,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
            http: None,
        }
    }

    fn tab(title: &str, config: &str) -> (String, String) {
        (title.into(), config.into())
    }

    /// SPEC-0088 AK-8 (v4): the tabs of test configurations in tab order, a further instance
    /// under its own title; a program's tab and a configuration that is gone are left out.
    #[test]
    fn rerun_tests_takes_the_tabs_of_test_configurations() {
        let configs = [
            config("serve", RunKind::Run),
            config("unit", RunKind::Test),
            config("Failed tests of unit", RunKind::Test),
        ];
        let tabs = [
            tab("serve", "serve"),
            tab("unit", "unit"),
            tab("gone", "gone"),
            tab("unit (2)", "unit"),
            tab("Failed tests of unit", "Failed tests of unit"),
        ];
        let runs: Vec<(String, String)> = test_reruns(&tabs, &configs)
            .into_iter()
            .map(|r| (r.config, r.tab))
            .collect();
        assert_eq!(
            runs,
            [
                tab("unit", "unit"),
                tab("unit", "unit (2)"),
                tab("Failed tests of unit", "Failed tests of unit"),
            ]
        );
        // Nothing to run again: no tab, or only programs.
        assert!(test_reruns(&[], &configs).is_empty());
        assert!(test_reruns(&tabs[..1], &configs).is_empty());
        assert!(test_reruns(&tabs, &configs[..1]).is_empty());
    }

    /// SPEC-0094 AK-4 (v2): after and before in opening order, the wrap at both ends, nothing
    /// with one window or for a window that is not a project window.
    #[test]
    fn project_windows_step_in_opening_order_and_wrap() {
        let opened = [10, 20, 30];
        assert_eq!(step_window(&opened, 10, true), Some(20));
        assert_eq!(step_window(&opened, 20, true), Some(30));
        assert_eq!(
            step_window(&opened, 30, true),
            Some(10),
            "wraps to the first"
        );
        assert_eq!(step_window(&opened, 20, false), Some(10));
        assert_eq!(
            step_window(&opened, 10, false),
            Some(30),
            "wraps to the last"
        );
        // Two windows: both directions reach the other one.
        assert_eq!(step_window(&[10, 20], 10, true), Some(20));
        assert_eq!(step_window(&[10, 20], 10, false), Some(20));
        assert_eq!(step_window(&[10], 10, true), None);
        assert_eq!(step_window(&[10], 10, false), None);
        assert_eq!(step_window(&opened, 40, true), None);
        assert_eq!(step_window::<u8>(&[], 1, true), None);
    }
}

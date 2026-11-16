//! When the status bar shows the Python interpreter widget (`SPEC-0117` v2 behaviour 1): it follows
//! the active editor, so a Java or TypeScript tab never shows Python.

use std::path::Path;

/// Whether `path` is a Python file or a Python declaration.
pub fn is_python_path(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "py" | "pyi"))
    {
        return true;
    }
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        matches!(n, "pyproject.toml" | "setup.py" | "setup.cfg" | "Pipfile")
            || (n.starts_with("requirements") && n.ends_with(".txt"))
    })
}

/// Whether the interpreter widget is shown: in a Python project, while the active tab is a Python
/// file or declaration, or while no editor is open.
pub fn shown(python_project: bool, active: Option<&Path>) -> bool {
    python_project && active.is_none_or(is_python_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_widget_follows_the_active_tab() {
        let shown_for = |p: &str| shown(true, Some(Path::new(p)));
        for python in [
            "/w/sprachen/test.py",
            "/w/stubs/x.pyi",
            "/w/pyproject.toml",
            "/w/requirements-dev.txt",
            "/w/setup.py",
            "/w/setup.cfg",
            "/w/Pipfile",
        ] {
            assert!(shown_for(python), "{python}");
        }
        // Hand test 2026-11-04: test.java was active and the bar still said Python.
        for other in [
            "/w/sprachen/test.java",
            "/w/sprachen/test.ts",
            "/w/README.md",
            "/w/requirements.md",
        ] {
            assert!(!shown_for(other), "{other}");
        }
        assert!(
            shown(true, None),
            "no editor open: the project's interpreter"
        );
        assert!(!shown(false, None));
        assert!(
            !shown(false, Some(Path::new("/w/a.py"))),
            "not a Python project"
        );
    }

    #[test]
    fn switching_tabs_switches_the_widget() {
        use crate::tabs::Tabs;
        let mut tabs = Tabs::default();
        for f in ["/w/test.py", "/w/test.ts", "/w/test.java"] {
            tabs.open(Path::new(f), |_| false);
        }
        let now = |t: &Tabs| shown(true, t.active().map(|i| t.paths()[i].as_path()));
        assert!(!now(&tabs), "test.java is active");
        tabs.activate(0);
        assert!(now(&tabs), "test.py is active");
        tabs.step(true);
        assert!(!now(&tabs), "test.ts is active");
    }
}

//! The trust question and the safe-mode banner (`SPEC-0278` behaviours 1–3, 7), without drawing.

use std::path::{Path, PathBuf};

use idecraft_core::trust::{self, TrustKind};

/// An answer to the trust question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Open in safe mode (the default, Return).
    SafeMode,
    /// Trust this project.
    TrustProject,
    /// Trust every project in this folder.
    TrustFolder(PathBuf),
    /// Close the window (Escape; only when opening).
    DontOpen,
}

/// The native alert's buttons for `answers`: *Preview in Safe Mode*, the first, is the default
/// (Return) and the question's named safe answer, which holds the keyboard focus — it grants no
/// trust and closes nothing (`SPEC-0281` v3 behaviour 8); *Don't Open* is the cancel button
/// (Escape); the others are plain answers.
pub fn prompt_buttons(answers: &[(String, Answer)]) -> Vec<gpui::PromptButton> {
    answers
        .iter()
        .map(|(label, answer)| match answer {
            Answer::SafeMode => gpui::PromptButton::safe(label.clone()),
            Answer::DontOpen => gpui::PromptButton::cancel(label.clone()),
            _ => gpui::PromptButton::new(label.clone()),
        })
        .collect()
}

/// The question's buttons in the order given to the native alert: the first is the default
/// (Return), the last the cancel button (Escape) — behaviour 2. `opening` adds *Don't Open*; asked
/// again from the banner, Escape keeps safe mode.
pub fn answers(root: &Path, opening: bool) -> Vec<(String, Answer)> {
    let mut list = vec![("Preview in Safe Mode".to_owned(), Answer::SafeMode)];
    if let Some(parent) = root.parent().filter(|p| p.parent().is_some()) {
        let name = parent
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| parent.display().to_string());
        list.push((
            format!("Trust Projects in \"{name}\""),
            Answer::TrustFolder(parent.to_path_buf()),
        ));
    }
    list.push(("Trust Project".to_owned(), Answer::TrustProject));
    if opening {
        list.push(("Don't Open".to_owned(), Answer::DontOpen));
    }
    list
}

/// Remembers the answer in `file`; returns the status line, or `None` when nothing was trusted.
pub fn apply(answer: &Answer, root: &Path, file: &Path) -> Result<Option<String>, String> {
    match answer {
        Answer::TrustProject => {
            trust::add_location(file, root, TrustKind::Project)?;
            Ok(Some(format!("Trusted project {}", root.display())))
        }
        Answer::TrustFolder(dir) => {
            trust::add_location(file, dir, TrustKind::Folder)?;
            Ok(Some(format!("Trusted all projects in {}", dir.display())))
        }
        Answer::SafeMode | Answer::DontOpen => Ok(None),
    }
}

/// The banner text of a project in safe mode (behaviour 7).
pub fn banner(root: &Path) -> String {
    let found: Vec<String> = trust::findings(root).into_iter().map(|f| f.file).collect();
    if found.is_empty() {
        "Safe mode: this project's run configurations, tests and tasks are off until you trust it."
            .to_owned()
    } else {
        format!(
            "Safe mode: nothing of this project runs until you trust it — no build scripts, build-tool sync, run configurations, tests or tasks. Found: {}.",
            found.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SPEC-0281` v3 behaviour 8, AK-7: the keyboard focus (and so Space) is on Preview in Safe
    /// Mode, also when Don't Open, the cancel answer, is offered; Return gives it too.
    #[test]
    fn the_trust_question_focuses_preview_in_safe_mode() {
        let dir = std::env::temp_dir().join(format!("idecraft-trust-f-{}", std::process::id()));
        let root = dir.join("projects/app");
        std::fs::create_dir_all(&root).unwrap();
        for opening in [true, false] {
            let answers = answers(&root, opening);
            let buttons = prompt_buttons(&answers);
            let focused = gpui::prompt_focused_answer(&buttons).unwrap();
            assert_eq!(answers[focused].1, Answer::SafeMode, "opening: {opening}");
            assert_eq!(focused, 0, "the safe answer is also the default");
            assert_eq!(
                buttons.iter().filter(|b| b.is_safe()).count(),
                1,
                "one named safe answer"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn question_answers_and_banner() {
        let dir = std::env::temp_dir().join(format!("idecraft-trust-q-{}", std::process::id()));
        let root = dir.join("projects/app");
        std::fs::create_dir_all(&root).unwrap();
        let opening = answers(&root, true);
        let labels: Vec<&str> = opening.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Preview in Safe Mode",
                "Trust Projects in \"projects\"",
                "Trust Project",
                "Don't Open"
            ]
        );
        assert_eq!(opening[0].1, Answer::SafeMode, "Return opens in safe mode");
        assert_eq!(opening.last().unwrap().1, Answer::DontOpen, "Escape");
        assert!(
            !answers(&root, false)
                .iter()
                .any(|(_, a)| *a == Answer::DontOpen),
            "the banner's question keeps the window"
        );
        assert!(banner(&root).contains("until you trust it"));
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        assert!(
            banner(&root).ends_with("Found: pom.xml."),
            "{}",
            banner(&root)
        );

        let file = dir.join("config/trusted-projects.toml");
        let settings = trust::TrustSettings::with_file(&file);
        assert_eq!(apply(&Answer::SafeMode, &root, &file).unwrap(), None);
        assert!(!settings.status(&root).trusted);
        let line = apply(&opening[1].1, &root, &file).unwrap().unwrap();
        assert!(line.starts_with("Trusted all projects in"), "{line}");
        std::fs::create_dir_all(dir.join("projects/other")).unwrap();
        assert!(settings.status(&root).trusted);
        assert!(settings.status(&dir.join("projects/other")).trusted);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! The indentation a file's own text uses (`SPEC-0150` v5 behaviour 7): a tab, or the number of
//! spaces one level adds, read from the first lines of the file.

/// How many lines are looked at.
const LINES: usize = 1000;

/// The indent unit `text` uses: `"\t"`, two to eight spaces, or `None` when the text shows no
/// indentation to go by. Blank lines and lines that continue a block comment (`*` after the
/// indentation) do not count.
pub fn detect(text: &str) -> Option<String> {
    let mut tabs = 0usize;
    let mut spaced = 0usize;
    // How often each step from 1 to 8 spaces was added over the line before.
    let mut steps = [0usize; 9];
    let mut previous: Option<usize> = None;
    // The step that led to the current level: lines that stay there support it too.
    let mut level_step: Option<usize> = None;
    for line in text.lines().take(LINES) {
        let body = line.trim_start_matches([' ', '\t']);
        if body.trim().is_empty() || body.starts_with('*') {
            continue;
        }
        let lead = &line[..line.len() - body.len()];
        if lead.starts_with('\t') {
            tabs += 1;
            previous = None;
            level_step = None;
            continue;
        }
        if lead.contains('\t') {
            previous = None;
            level_step = None;
            continue;
        }
        let width = lead.len();
        if width > 0 {
            spaced += 1;
        }
        match previous {
            Some(before) if width > before && width - before <= 8 => {
                steps[width - before] += 1;
                level_step = Some(width - before);
            }
            Some(before) if width == before && width > 0 => {
                if let Some(step) = level_step {
                    steps[step] += 1;
                }
            }
            _ => level_step = None,
        }
        previous = Some(width);
    }
    if tabs > spaced && tabs > 0 {
        return Some("\t".into());
    }
    // The best supported step of two or more spaces, with at least two lines for it; on a tie
    // the smaller one.
    let (best, count) = (2..=8)
        .map(|n| (n, steps[n]))
        .fold((0, 0), |acc, (n, c)| if c > acc.1 { (n, c) } else { acc });
    (count >= 2).then(|| " ".repeat(best))
}

#[cfg(test)]
mod tests {
    use super::detect;

    /// SPEC-0150 AK-7 (v5): the step a file's lines use.
    #[test]
    fn the_files_own_indentation() {
        let css = ".card {\n  display: flex;\n  color: red;\n}\n\n.box {\n  margin: 0;\n}\n";
        assert_eq!(detect(css).as_deref(), Some("  "));
        let java = "/**\n * Doc.\n */\nclass A {\n    /**\n     * More.\n     */\n    void f() {\n        g();\n    }\n}\n";
        assert_eq!(detect(java).as_deref(), Some("    "));
        let go = "func main() {\n\tif x {\n\t\ty()\n\t}\n}\n";
        assert_eq!(detect(go).as_deref(), Some("\t"));
        assert_eq!(detect("a\nb\n\nc\n"), None);
        assert_eq!(detect(""), None);
        // Two steps of two and two of four: the smaller one.
        let tie = "a\n  b\nc\n  d\ne\n    f\ng\n    h\n";
        assert_eq!(detect(tie).as_deref(), Some("  "));
        // A single badly indented line decides nothing.
        let flat = "class Flat {\nint a;\n        void f() {\nrun();\n}\n}\n";
        assert_eq!(detect(flat), None);
        // One-space continuation lines are no step.
        let one = "x = f(a,\n b)\ny {\n    z\n    w\n}\n";
        assert_eq!(detect(one).as_deref(), Some("    "));
    }
}

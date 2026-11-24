//! Reading the report of macOS `/usr/bin/sample` (behaviour 1): its "Call graph" section, one
//! tree per thread, with a count per frame and the depth given by the column of the count.

use crate::model::{Profile, ProfileNode, Unit};

/// One frame line: `  + ! :   690 outer  (in busy) + 56  [0x…]  busy.c:3`.
fn frame(line: &str) -> Option<(usize, f64, ProfileNode)> {
    let at = line.find(|c: char| c.is_ascii_digit())?;
    // Only tree markers and spaces may come before the count.
    if !line[..at].chars().all(|c| " +!:|".contains(c)) {
        return None;
    }
    let rest = &line[at..];
    let (count, rest) = rest.split_once(' ')?;
    let count: f64 = count.parse().ok()?;
    let rest = rest.trim();
    let (name, tail) = match rest.find("  (in ") {
        Some(i) => (&rest[..i], &rest[i + 2..]),
        None => match rest.find("  [") {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        },
    };
    let module = tail
        .strip_prefix("(in ")
        .and_then(|t| t.split_once(')'))
        .map(|(m, _)| m.to_owned());
    // A source location follows the addresses: `]  busy.c:3`.
    let location = tail.rsplit_once("]  ").and_then(|(_, loc)| {
        let (file, line) = loc.trim().rsplit_once(':')?;
        Some((file.to_owned(), line.trim().parse::<u32>().ok()?))
    });
    let mut node = ProfileNode::leaf(name.trim(), count);
    node.module = module;
    node.location = location;
    Some((at, count, node))
}

/// Parses a `sample` report into a profile (counts are samples).
pub fn parse_sample(text: &str) -> Profile {
    let mut roots: Vec<ProfileNode> = Vec::new();
    // (column of the count, node) from the root to the current frame.
    let mut stack: Vec<(usize, ProfileNode)> = Vec::new();
    let close = |stack: &mut Vec<(usize, ProfileNode)>, roots: &mut Vec<ProfileNode>| {
        if let Some((_, node)) = stack.pop() {
            match stack.last_mut() {
                Some((_, parent)) => parent.children.push(node),
                None => roots.push(node),
            }
        }
    };
    let mut in_graph = false;
    for line in text.lines() {
        if line.starts_with("Call graph:") {
            in_graph = true;
            continue;
        }
        if !in_graph {
            continue;
        }
        if line.trim().is_empty() {
            // The graph ends with an empty line before the "Total number in stack" part.
            if !stack.is_empty() || !roots.is_empty() {
                break;
            }
            continue;
        }
        let Some((column, _, node)) = frame(line) else {
            continue;
        };
        while stack.last().is_some_and(|(c, _)| *c >= column) {
            close(&mut stack, &mut roots);
        }
        stack.push((column, node));
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    for r in &mut roots {
        r.finish();
    }
    let total = roots.iter().map(|r| r.total).sum();
    let mut notes = Vec::new();
    if roots.is_empty() {
        notes.push(
            "the report has no call graph (the program ended before the first sample?)".into(),
        );
    }
    Profile {
        engine: "sample".into(),
        unit: Unit::Samples,
        total,
        roots,
        notes,
        lines: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "Analysis of sampling busy (pid 1) every 1 millisecond
Process:         busy [1]

Call graph:
    700 Thread_66166070: Main Thread
    + 690 start  (in dyld) + 6596  [0x19272be24]
    + ! 690 main  (in busy) + 60  [0x102fd8574]  busy.c:4
    + !   690 outer  (in busy) + 56  [0x102fd8504]  busy.c:3
    + !     688 inner  (in busy) + 28,72,...  [0x102fd847c,0x102fd84a8,...]  busy.c:2
    + 10 _dyld_start  (in dyld) + 0  [0x103178bf0]
    12 Thread_2
    + 12 __workq_kernreturn  (in libsystem_kernel.dylib) + 8  [0x1]

Total number in stack (recursive counted multiple, when >=5):

Sort by top of stack, same collapsed (when >= 5):
        inner  (in busy)        688
";

    #[test]
    fn threads_frames_locations_and_self_counts() {
        let p = parse_sample(REPORT);
        assert_eq!(p.total, 712.0);
        assert_eq!(p.roots.len(), 2);
        let main_thread = &p.roots[0];
        assert_eq!(main_thread.name, "Thread_66166070: Main Thread");
        let start = &main_thread.children[0];
        assert_eq!(
            (start.name.as_str(), start.module.as_deref()),
            ("start", Some("dyld"))
        );
        let main = &start.children[0];
        assert_eq!(main.location, Some(("busy.c".into(), 4)));
        let inner = &main.children[0].children[0];
        assert_eq!(inner.name, "inner");
        assert_eq!(inner.location, Some(("busy.c".into(), 2)));
        assert_eq!(inner.self_cost, 688.0);
        assert_eq!(main.children[0].self_cost, 2.0);
        assert_eq!(main_thread.children[1].name, "_dyld_start");
        assert_eq!(
            p.roots[1].children[0].module.as_deref(),
            Some("libsystem_kernel.dylib")
        );
        assert!(parse_sample("no graph here").notes[0].contains("no call graph"));
    }
}

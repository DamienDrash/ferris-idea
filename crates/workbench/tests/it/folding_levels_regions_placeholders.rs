//! Window checks of folding (`SPEC-0083` v8 AK-25 to AK-27): Collapse All leaves the outermost
//! class open, custom regions fold to their name, the levels, the documentation comments, the
//! placeholders as drawn, and the folding arrows that follow the pointer and their setting. No
//! language server takes part.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{EditorView, WorkbenchView};

/// The file of the comparison: a header comment 0–3, imports 4–6, a documentation comment
/// 8–11, the class body 12–50 with a custom region 13–16, line comments 18–20, a one-line
/// method 21–23, `add` 25–34 (`if` 26–29 with a `for` 27–29, `else` 30–32), an anonymous class
/// 36–43 (`run` 38–42, a lambda 39–41) and an inner class 45–49 (an argument list 46–48).
const FOLD: &str = "/*
 * Header comment
 * over three lines.
 */
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * A class to fold.
 * Second line of the doc.
 */
public class Fold {
    // region Fields
    private final List<String> names = new ArrayList<>();
    private int count;
    // endregion

    // one
    // two
    // three
    public int getCount() {
        return count;
    }

    public void add(String name) {
        if (name != null) {
            for (int i = 0; i < 2; i++) {
                names.add(name + i);
            }
        } else {
            count--;
        }
        count++;
    }

    Runnable task = new Runnable() {
        @Override
        public void run() {
            names.forEach(n -> {
                System.out.println(n);
            });
        }
    };

    static class Inner {
        Map<String, Integer> sizes = Map.of(
                \"a\", 1,
                \"b\", 2);
    }
}
";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-folding-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Fold.java"), FOLD).unwrap();
    // Markers without a partner.
    std::fs::write(
        root.join("src/Open.java"),
        "class Open {\n    // region Never closed\n    int a;\n    int b;\n}\n",
    )
    .unwrap();
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(1200.)));
    (bench, cx)
}

/// Waits until `done` holds, letting background work and timers run.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

/// Opens the file and waits until its imports and its one-line method are folded.
fn open_fold(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    root: &Path,
) -> Entity<EditorView> {
    let file = root.join("src/Fold.java");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    until(cx, "the file is folded on open", |cx| {
        editor.read_with(cx, |e, _| e.folded_regions() == [(4, 6), (21, 23)])
    });
    editor
}

fn run(editor: &Entity<EditorView>, cx: &mut VisualTestContext, id: &str) {
    assert!(editor.update(cx, |e, cx| e.run_action(id, cx)), "{id}");
    cx.run_until_parked();
}

fn folded(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
    editor.read_with(cx, |e, _| e.folded_regions())
}

/// The primary caret as 0-based line and column.
fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    editor.read_with(cx, |e, _| {
        let p = e
            .state
            .buffer
            .offset_to_position(e.state.carets.primary().head)
            .unwrap();
        (p.line, p.column)
    })
}

/// The folded lines as drawn: line, the text of the line as shown, the placeholder.
fn drawn(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, String, String)> {
    cx.run_until_parked();
    editor.read_with(cx, |e, _| e.painted_placeholders().to_vec())
}

/// Everything Collapse All folds in the file.
const ALL: [(usize, usize); 9] = [
    (0, 3),
    (4, 6),
    (8, 11),
    (13, 16),
    (18, 20),
    (21, 23),
    (25, 34),
    (36, 43),
    (45, 49),
];

/// AK-25: Collapse All leaves the body of the outermost class open; the regions show their
/// placeholders; levels 1 and 2; the documentation comments alone.
#[gpui::test]
fn collapse_all_levels_and_doc_comments(cx: &mut TestAppContext) {
    let root = workspace("levels");
    let (bench, cx) = window(&root, cx);
    let editor = open_fold(&bench, cx, &root);

    run(&editor, cx, "CollapseAllRegions");
    assert_eq!(folded(&editor, cx), ALL, "the class body 12–50 stays open");
    assert_eq!(caret(&editor, cx), (0, 0));
    let line = |text: &str, placeholder: &str| (text.to_owned(), placeholder.to_owned());
    let shown: Vec<(String, String)> = drawn(&editor, cx)
        .into_iter()
        .map(|(_, text, placeholder)| (text, placeholder))
        .collect();
    assert_eq!(
        shown,
        [
            line("/*", "...*/"),
            line("import ", "..."),
            line("/**", " A class to fold. ...*/"),
            line("    ", "Fields"),
            line("    //", "..."),
            line("    public int getCount() {", " return count; }"),
            line("    public void add(String name) {", "...}"),
            line("    Runnable task = new Runnable() {", "...}"),
            line("    static class Inner {", "...}"),
        ]
    );
    // 35 lines are hidden.
    assert_eq!(
        editor.read_with(cx, |e, _| (e.state.buffer.len_lines(), e.visible_rows())),
        (52, 17)
    );

    // All to level 1: the blocks directly in methods, `if` and `else` each; the method of the
    // anonymous class and the argument list in the inner class are one level down as well.
    run(&editor, cx, "ExpandAllToLevel1");
    assert_eq!(
        folded(&editor, cx),
        [(26, 29), (30, 32), (38, 42), (46, 48)]
    );
    let shown: Vec<(usize, String)> = drawn(&editor, cx)
        .into_iter()
        .map(|(l, _, placeholder)| (l, placeholder))
        .collect();
    assert_eq!(
        shown,
        [
            (26, "...".to_owned()),
            (30, "...}".to_owned()),
            (38, "...}".to_owned()),
            (46, "...".to_owned()),
        ],
        "the closing brace of the `if` stays on the line of `else`"
    );
    // All to level 2: the `for` in the `if` and the lambda in the anonymous class's method.
    run(&editor, cx, "ExpandAllToLevel2");
    assert_eq!(folded(&editor, cx), [(27, 29), (39, 41)]);
    run(&editor, cx, "ExpandAllToLevel3");
    assert!(folded(&editor, cx).is_empty());
    // To level 1 in `add`: its blocks fold, the rest stays.
    editor.update(cx, |e, cx| e.go_to(33, 8, cx));
    run(&editor, cx, "ExpandToLevel1");
    assert_eq!(folded(&editor, cx), [(26, 29), (30, 32)]);
    run(&editor, cx, "ExpandRegionRecursively");
    assert!(folded(&editor, cx).is_empty());

    // The documentation comments alone.
    run(&editor, cx, "CollapseDocComments");
    assert_eq!(folded(&editor, cx), [(8, 11)]);
    run(&editor, cx, "ExpandDocComments");
    assert!(folded(&editor, cx).is_empty());
    editor.update(cx, |e, cx| e.go_to(0, 0, cx));
    run(&editor, cx, "CollapseAllRegions");
    run(&editor, cx, "ExpandDocComments");
    assert_eq!(
        folded(&editor, cx),
        ALL.into_iter()
            .filter(|r| *r != (8, 11))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        editor.read_with(cx, |e, _| e.state.buffer.text()),
        FOLD,
        "folding changes no text"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-26: a custom region folds to its name; the caret stays in front of the name and opens
/// the region when it moves into the text the name stands for; a marker without a partner
/// folds nothing.
#[gpui::test]
fn custom_regions_and_the_caret(cx: &mut TestAppContext) {
    let root = workspace("regions");
    let (bench, cx) = window(&root, cx);
    let editor = open_fold(&bench, cx, &root);

    editor.update(cx, |e, cx| e.go_to(15, 10, cx));
    run(&editor, cx, "CollapseRegion");
    assert_eq!(folded(&editor, cx), [(4, 6), (13, 16), (21, 23)]);
    assert_eq!(caret(&editor, cx), (13, 4), "in front of the name");
    assert!(
        drawn(&editor, cx).contains(&(13, "    ".to_owned(), "Fields".to_owned())),
        "{:?}",
        drawn(&editor, cx)
    );
    // The caret goes into the hidden rest of the line: the region opens.
    editor.update(cx, |e, cx| e.go_to(13, 12, cx));
    cx.run_until_parked();
    assert_eq!(folded(&editor, cx), [(4, 6), (21, 23)]);
    // The imports: the caret at the end of the first import ends up behind the keyword.
    run(&editor, cx, "ExpandAllRegions");
    editor.update(cx, |e, cx| e.go_to(5, 3, cx));
    run(&editor, cx, "CollapseRegion");
    assert_eq!(folded(&editor, cx), [(4, 6)]);
    assert_eq!(caret(&editor, cx), (4, 7));
    assert!(drawn(&editor, cx).contains(&(4, "import ".to_owned(), "...".to_owned())));

    // A marker without a partner: only the class body is a region.
    let file = root.join("src/Open.java");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let open = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    open.update(cx, |e, cx| e.go_to(2, 4, cx));
    until(cx, "the syntax tree gives the class body", |cx| {
        open.update(cx, |e, cx| {
            e.run_action("CollapseRegion", cx);
            !e.folded_regions().is_empty()
        })
    });
    assert_eq!(folded(&open, cx), [(0, 4)]);
    run(&open, cx, "ExpandAllRegions");
    run(&open, cx, "CollapseAllRegions");
    assert!(folded(&open, cx).is_empty(), "the class body stays open");
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-27: the folding arrows are drawn at folded regions only, at the others while the pointer
/// is over the editor; with the setting on "always" at every region.
#[gpui::test]
fn folding_arrows_follow_the_pointer(cx: &mut TestAppContext) {
    let root = workspace("arrows");
    let (bench, cx) = window(&root, cx);
    let editor = open_fold(&bench, cx, &root);
    let arrows = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        editor.read_with(cx, |e, _| e.painted_fold_arrows())
    };
    editor.update(cx, |e, cx| e.set_pointer_inside(false, cx));
    assert_eq!(arrows(cx), [(4, true), (21, true)], "folded regions only");
    // The pointer comes over the editor: every region of the lines in view has its arrow.
    editor.update(cx, |e, cx| e.set_pointer_inside(true, cx));
    let all = arrows(cx);
    for expected in [
        (0, false),
        (4, true),
        (8, false),
        (12, false),
        (13, false),
        (18, false),
        (21, true),
        (25, false),
        (26, false),
        (30, false),
    ] {
        assert!(all.contains(&expected), "{expected:?} in {all:?}");
    }
    editor.update(cx, |e, cx| e.set_pointer_inside(false, cx));
    assert_eq!(arrows(cx), [(4, true), (21, true)]);
    // The setting: always.
    cx.update(|_, cx| {
        let mut settings = idecraft_workbench::appearance::ide_settings(cx);
        assert_eq!(
            settings.editor.folding_arrows,
            idecraft_core::FoldingArrows::Hover,
            "on hover by default"
        );
        settings.editor.folding_arrows = idecraft_core::FoldingArrows::Always;
        cx.set_global(idecraft_workbench::appearance::CurrentIdeSettings(settings));
    });
    editor.update(cx, |_, cx| cx.notify());
    assert_eq!(arrows(cx), all);
    let _ = std::fs::remove_dir_all(&root);
}

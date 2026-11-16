//! Window checks of the editor actions taken from the keymap: folding with everything inside and
//! down to a level (`SPEC-0083` v6 AK-15), scroll to centre (`SPEC-0057` v4 AK-17), previous and
//! next method, edit locations and Move Element (`SPEC-0071` v9 AK-10), Cyclic Expand Word
//! (`SPEC-0057` v5 AK-18), the small editing commands (`SPEC-0332` AK-9), Copy Reference and the
//! copy popup for an editor and for the project tree's selection (`SPEC-0332` v2 AK-15). No
//! language server takes part.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{EditorView, WorkbenchView};

/// Regions by line: the class body 2–17, `total` 3–11 with the loop 5–9 and the `if` 6–8 in it,
/// `clear` 13–16.
const CART: &str = "package shop;

public class Cart {
    int total(int[] xs) {
        int sum = 0;
        for (int x : xs) {
            if (x > 0) {
                sum += x;
            }
        }
        return sum;
    }

    void clear() {
        items = 0;
        count = 0;
    }
}
";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "idecraft-editor-actions-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join("src/shop/Cart.java"), CART).unwrap();
    // 60 methods of four lines each: method `i` is on the lines `3 + 4 * i` to `6 + 4 * i`.
    let mut many = String::from("package shop;\n\npublic class Many {\n");
    for i in 0..60 {
        many.push_str(&format!(
            "    int m{i}() {{\n        int a = {i};\n        return a;\n    }}\n"
        ));
    }
    many.push_str("}\n");
    std::fs::write(root.join("src/shop/Many.java"), many).unwrap();
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
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
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

/// Opens the class and waits for its syntax tree (Collapse All folds something once it is there).
fn open_cart(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    root: &Path,
) -> Entity<EditorView> {
    open_class(bench, cx, root, "Cart")
}

fn open_class(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    root: &Path,
    name: &str,
) -> Entity<EditorView> {
    let file = root.join(format!("src/shop/{name}.java"));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    until(cx, "the syntax tree gives folding regions", |cx| {
        editor.update(cx, |e, cx| {
            e.run_action("CollapseAllRegions", cx);
            !e.folded_regions().is_empty()
        })
    });
    run(&editor, cx, "ExpandAllRegions");
    editor.update(cx, |e, cx| e.go_to(0, 0, cx));
    editor
}

fn run(editor: &Entity<EditorView>, cx: &mut VisualTestContext, id: &str) {
    assert!(editor.update(cx, |e, cx| e.run_action(id, cx)), "{id}");
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

#[gpui::test]
fn folding_recursively_and_to_a_level(cx: &mut TestAppContext) {
    let root = workspace("folding");
    let (bench, cx) = window(&root, cx);
    let editor = open_cart(&bench, cx, &root);

    // Collapse recursively in `total`: the method folds, the caret goes to the end of its first
    // line, and the loop comes back folded when the method is expanded.
    editor.update(cx, |e, cx| e.go_to(4, 8, cx));
    run(&editor, cx, "CollapseRegionRecursively");
    assert_eq!(folded(&editor, cx), [(3, 11)]);
    assert_eq!(caret(&editor, cx), (3, 25));
    run(&editor, cx, "ExpandRegion");
    assert_eq!(folded(&editor, cx), [(5, 9)], "the loop was kept folded");
    run(&editor, cx, "ExpandRegion");
    assert_eq!(
        folded(&editor, cx),
        [(5, 9)],
        "the caret is not on the loop"
    );
    // Expand recursively around the caret opens the method with everything in it.
    run(&editor, cx, "ExpandRegionRecursively");
    assert!(folded(&editor, cx).is_empty());
    // On a folded line it opens that fold with what was kept in it.
    run(&editor, cx, "CollapseRegionRecursively");
    assert_eq!(folded(&editor, cx), [(3, 11)]);
    run(&editor, cx, "ExpandRegionRecursively");
    assert!(folded(&editor, cx).is_empty());

    // All to a level: the class body is the outermost container and no level (SPEC-0083 v8
    // behaviour 18); the methods are level 1, the loop 2, the `if` 3.
    editor.update(cx, |e, cx| e.go_to(0, 0, cx));
    run(&editor, cx, "ExpandAllToLevel1");
    assert_eq!(folded(&editor, cx), [(5, 9)]);
    run(&editor, cx, "ExpandAllToLevel2");
    assert_eq!(folded(&editor, cx), [(6, 8)]);
    run(&editor, cx, "ExpandAllToLevel3");
    assert!(folded(&editor, cx).is_empty());
    run(&editor, cx, "ExpandAllToLevel5");
    assert!(folded(&editor, cx).is_empty());

    // To a level with the caret in the class body between the methods: the class is level 1
    // there, and the methods fold with everything inside.
    editor.update(cx, |e, cx| e.go_to(12, 0, cx));
    run(&editor, cx, "ExpandToLevel1");
    assert_eq!(folded(&editor, cx), [(3, 11), (13, 16)]);
    // A jump into a method folded with everything inside opens it and leaves the loop folded.
    editor.update(cx, |e, cx| e.go_to(4, 8, cx));
    assert_eq!(folded(&editor, cx), [(5, 9), (13, 16)]);
    // To a level inside the method at the caret; `clear` stays as it is.
    run(&editor, cx, "ExpandToLevel2");
    assert_eq!(folded(&editor, cx), [(6, 8), (13, 16)]);
    run(&editor, cx, "ExpandToLevel1");
    assert_eq!(folded(&editor, cx), [(5, 9), (13, 16)]);
    run(&editor, cx, "ExpandToLevel3");
    assert_eq!(folded(&editor, cx), [(13, 16)]);
    // A caret the fold would hide moves to the end of the fold's first line.
    editor.update(cx, |e, cx| e.go_to(7, 16, cx));
    run(&editor, cx, "ExpandAllToLevel1");
    assert_eq!(folded(&editor, cx), [(5, 9)]);
    assert_eq!(caret(&editor, cx), (5, 26));
    assert_eq!(
        editor.read_with(cx, |e, _| e.state.buffer.text()),
        CART,
        "folding changes no text"
    );
}

#[gpui::test]
fn scroll_to_center_counts_rows(cx: &mut TestAppContext) {
    let root = workspace("scroll");
    let (bench, cx) = window(&root, cx);
    let editor = open_class(&bench, cx, &root, "Many");

    // The first method folded: three lines above the caret take no rows.
    editor.update(cx, |e, cx| e.go_to(4, 0, cx));
    run(&editor, cx, "CollapseRegion");
    assert_eq!(folded(&editor, cx), [(3, 6)]);
    editor.update(cx, |e, cx| e.go_to(150, 8, cx));
    cx.run_until_parked();
    let before = caret(&editor, cx);
    run(&editor, cx, "EditorScrollToCenter");
    let (scrolled, height) = editor.read_with(cx, |e, _| e.scrolled_rows());
    assert!(height > 3.0, "the view has a height: {height}");
    let want = 147.0 - (height - 1.0) / 2.0;
    assert!(
        (scrolled - want).abs() < 0.01,
        "row 147 in the middle of {height} rows: scrolled {scrolled}, want {want}"
    );
    assert_eq!(caret(&editor, cx), before, "the caret stays");
    assert_eq!(folded(&editor, cx), [(3, 6)], "and so do the folds");
    // Near the top the view cannot scroll before the first row.
    editor.update(cx, |e, cx| e.go_to(1, 0, cx));
    run(&editor, cx, "EditorScrollToCenter");
    assert_eq!(editor.read_with(cx, |e, _| e.scrolled_rows()).0, 0.0);
}

#[gpui::test]
fn methods_and_edit_locations(cx: &mut TestAppContext) {
    let root = workspace("methods");
    let (bench, cx) = window(&root, cx);
    let editor = open_cart(&bench, cx, &root);

    // Previous and next method from `sum += x;`.
    editor.update(cx, |e, cx| e.go_to(7, 16, cx));
    run(&editor, cx, "MethodUp");
    assert_eq!(caret(&editor, cx), (3, 8), "the name of `total`");
    run(&editor, cx, "MethodUp");
    assert_eq!(caret(&editor, cx), (3, 8), "no method before it");
    run(&editor, cx, "MethodDown");
    assert_eq!(caret(&editor, cx), (13, 9), "the name of `clear`");
    run(&editor, cx, "MethodDown");
    assert_eq!(caret(&editor, cx), (13, 9));
    // A method folded with everything inside is still a stop, and its line stays folded. From
    // the class body the class is level 1, so the methods fold (SPEC-0083 v8 behaviour 18).
    editor.update(cx, |e, cx| e.go_to(12, 0, cx));
    run(&editor, cx, "ExpandToLevel1");
    assert_eq!(folded(&editor, cx), [(3, 11), (13, 16)]);
    run(&editor, cx, "MethodUp");
    assert_eq!(caret(&editor, cx), (3, 8));
    assert_eq!(folded(&editor, cx), [(3, 11), (13, 16)]);
    run(&editor, cx, "ExpandAllRegions");

    // Move Element on the two arguments of a call added to `clear`, as one undo step.
    let at = editor.read_with(cx, |e, _| {
        let text = e.state.buffer.text();
        text[..text.find("        count = 0;").unwrap()]
            .chars()
            .count()
    });
    editor.update(cx, |e, cx| {
        e.replace_ranges(&[(at..at, "        put(first, second);\n".into())], cx)
    });
    editor.update(cx, |e, cx| e.go_to(15, 14, cx));
    run(&editor, cx, "MoveElementRight");
    let line = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.state.buffer.line(15).unwrap_or_default())
    };
    assert_eq!(line(cx), "        put(second, first);");
    assert_eq!(caret(&editor, cx), (15, 22), "the caret stays in `first`");
    run(&editor, cx, "MoveElementRight");
    assert_eq!(line(cx), "        put(second, first);", "the last element");
    run(&editor, cx, "MoveElementLeft");
    assert_eq!(line(cx), "        put(first, second);");
    run(&editor, cx, "$Undo");
    run(&editor, cx, "$Undo");
    assert_eq!(
        line(cx),
        "        put(first, second);",
        "each move is one step"
    );

    // Edit locations: the inserted call, then nothing older; forward does nothing at the newest.
    editor.update(cx, |e, cx| e.go_to(0, 0, cx));
    run(&editor, cx, "JumpToLastChange");
    assert_eq!(caret(&editor, cx), (16, 0), "the end of the inserted line");
    run(&editor, cx, "JumpToLastChange");
    assert_eq!(caret(&editor, cx), (16, 0));
    run(&editor, cx, "JumpToNextChange");
    assert_eq!(caret(&editor, cx), (16, 0));
    // A second edit above: the newest place first, the older one moved along with it.
    editor.update(cx, |e, cx| {
        e.replace_ranges(&[(0..0, "// cart\n".into())], cx)
    });
    editor.update(cx, |e, cx| e.go_to(5, 0, cx));
    run(&editor, cx, "JumpToLastChange");
    assert_eq!(caret(&editor, cx), (1, 0));
    run(&editor, cx, "JumpToLastChange");
    assert_eq!(caret(&editor, cx), (17, 0));
    run(&editor, cx, "JumpToNextChange");
    assert_eq!(caret(&editor, cx), (1, 0));
}

/// SPEC-0057 v5 AK-18: Cyclic Expand Word in the view takes the words of the other open file
/// after the text's own, and one undo puts the prefix back.
#[gpui::test]
fn expand_word_in_the_window(cx: &mut TestAppContext) {
    let root = workspace("expand");
    // A prefix no other test's file has: the registry of open documents is the process's.
    std::fs::write(root.join("src/shop/near.txt"), "zqvalpha here\nzqv\n").unwrap();
    std::fs::write(root.join("src/shop/other.txt"), "zqvbeta zqvalpha\n").unwrap();
    let (bench, cx) = window(&root, cx);
    for name in ["other.txt", "near.txt"] {
        let file = root.join("src/shop").join(name);
        bench.update_in(cx, |b, window, cx| {
            b.open(&file, window, cx);
            b.focus_editor(window, cx);
        });
        cx.run_until_parked();
    }
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    editor.update(cx, |e, cx| e.go_to(1, 3, cx));
    let line = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.state.buffer.line(1).unwrap_or_default())
    };
    run(&editor, cx, "HippieCompletion");
    assert_eq!(line(cx).trim_end(), "zqvalpha", "the text's own word first");
    run(&editor, cx, "HippieCompletion");
    assert_eq!(line(cx).trim_end(), "zqvbeta", "then the other open file's");
    assert_eq!(caret(&editor, cx), (1, 7));
    run(&editor, cx, "$Undo");
    assert_eq!(line(cx).trim_end(), "zqv", "one step for the cycle");
    run(&editor, cx, "HippieBackwardCompletion");
    assert_eq!(line(cx).trim_end(), "zqvalpha");
    run(&editor, cx, "HippieBackwardCompletion");
    run(&editor, cx, "HippieBackwardCompletion");
    assert_eq!(line(cx).trim_end(), "zqv", "after the last word the prefix");
}

/// SPEC-0332 AK-9: the small editing commands through the editor view and the menu — one undo step
/// each, Convert Indents with the file's tab width and its message, a read-only text left as it is
/// with its note, and the rows of Copy Path/Reference… with the clipboard after choosing one.
#[gpui::test]
fn small_editing_commands_in_the_window(cx: &mut TestAppContext) {
    let root = workspace("small");
    std::fs::write(root.join("names.txt"), "pear\nfig\napple\n").unwrap();
    std::fs::write(
        root.join(".editorconfig"),
        "root = true\n\n[deep.txt]\nindent_style = tab\ntab_width = 8\n",
    )
    .unwrap();
    std::fs::write(root.join("deep.txt"), "        a\n    b\n").unwrap();
    std::fs::write(root.join("library.txt"), "\tb\n    a\n").unwrap();
    let (bench, cx) = window(&root, cx);
    let open = |name: &str, cx: &mut VisualTestContext| {
        let file = root.join(name);
        bench.update_in(cx, |b, window, cx| {
            b.open(&file, window, cx);
            b.focus_editor(window, cx);
        });
        cx.run_until_parked();
        bench
            .read_with(cx, |b, _| b.active_editor_entity())
            .expect("an editor is open")
    };
    let text = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.state.buffer.text())
    };
    let note = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.status().note)
    };

    let names = open("names.txt", cx);
    run(&names, cx, "EditorSortLines");
    assert_eq!(text(&names, cx), "apple\nfig\npear\n");
    // From the menu bar, as Edit › Reverse Lines does.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("EditorReverseLines", window, cx)
    });
    assert_eq!(text(&names, cx), "pear\nfig\napple\n");
    run(&names, cx, "$Undo");
    assert_eq!(text(&names, cx), "apple\nfig\npear\n", "one undo step");
    run(&names, cx, "$Undo");
    assert_eq!(text(&names, cx), "pear\nfig\napple\n");
    names.update(cx, |e, cx| e.go_to(0, 1, cx));
    run(&names, cx, "EditorTranspose");
    assert_eq!(text(&names, cx), "epar\nfig\napple\n");

    // Eight columns are one tab where a tab is eight columns wide; four stay spaces.
    let deep = open("deep.txt", cx);
    assert_eq!(deep.read_with(cx, |e, _| e.tab_width()), 8);
    run(&deep, cx, "ConvertIndentsToTabs");
    assert_eq!(text(&deep, cx), "\ta\n    b\n");
    assert_eq!(
        note(&deep, cx).as_deref(),
        Some("Indentation converted in 1 line")
    );
    run(&deep, cx, "ConvertIndentsToTabs");
    assert_eq!(
        note(&deep, cx).as_deref(),
        Some("Indentation already uses tabs")
    );

    // A read-only text keeps its text and says why.
    let library = open("library.txt", cx);
    library.update(cx, |e, _| e.set_read_only(true));
    for id in ["ConvertIndentsToSpaces", "EditorReverseLines"] {
        run(&library, cx, id);
        assert_eq!(text(&library, cx), "\tb\n    a\n", "{id}");
        assert_eq!(
            note(&library, cx).as_deref(),
            Some("Read-only: a library source"),
            "{id}"
        );
    }
    library.update(cx, |e, cx| e.go_to(0, 1, cx));
    run(&library, cx, "EditorTranspose");
    assert_eq!(text(&library, cx), "\tb\n    a\n");
    // A caret per line changes no text and works there.
    run(&library, cx, "$SelectAll");
    run(&library, cx, "EditorAddCaretPerSelectedLine");
    let heads: Vec<usize> = library.read_with(cx, |e, _| {
        e.state.carets.carets().iter().map(|c| c.head).collect()
    });
    assert_eq!(heads, [2, 8]);

    // Copy Path/Reference… lists what it would copy for the active file: a text file has no
    // symbols to name, so Copy Reference shows file and line (SPEC-0332 v2 behaviours 13–13b).
    let deep = open("deep.txt", cx);
    run(&deep, cx, "EditorTextEnd");
    let line = deep.read_with(cx, |e, _| e.status().line);
    assert_eq!(
        line, 3,
        "the caret is on the line after the last line break"
    );
    let clipboard = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|i| i.text());
    let status = |cx: &mut VisualTestContext| bench.read_with(cx, |b, cx| b.status_message(cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyReferencePopupGroup", window, cx)
    });
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the copy list is open");
    assert_eq!(
        popup.read_with(cx, |p, _| p.shown_items()),
        [
            format!("Absolute Path    {}", root.join("deep.txt").display()),
            "File Name    deep.txt".to_owned(),
            "Path with Line Number    deep.txt:3".to_owned(),
            "Path from Content Root    deep.txt".to_owned(),
            "Copy Reference    deep.txt:3".to_owned(),
        ]
    );
    popup.update(cx, |p, cx| {
        p.select(3);
        cx.emit(p.accept());
    });
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_none());
    assert_eq!(clipboard(cx).as_deref(), Some("deep.txt"));
    assert_eq!(
        status(cx).as_deref(),
        Some("Path from Content Root copied: deep.txt")
    );
    // The Copy Reference row copies file and line and says so.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyReferencePopupGroup", window, cx)
    });
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the copy list is open again");
    popup.update(cx, |p, cx| {
        p.select(4);
        cx.emit(p.accept());
    });
    cx.run_until_parked();
    assert_eq!(clipboard(cx).as_deref(), Some("deep.txt:3"));
    assert_eq!(status(cx).as_deref(), Some("Reference copied: deep.txt:3"));
    // The new rows as actions of their own; the caret's line counts.
    run(&deep, cx, "EditorTextStart");
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyPathWithLineNumber", window, cx)
    });
    assert_eq!(clipboard(cx).as_deref(), Some("deep.txt:1"));
    assert_eq!(
        status(cx).as_deref(),
        Some("Path with Line Number copied: deep.txt:1")
    );
    run(&deep, cx, "EditorDown");
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyReference", window, cx)
    });
    assert_eq!(clipboard(cx).as_deref(), Some("deep.txt:2"));
    assert_eq!(status(cx).as_deref(), Some("Reference copied: deep.txt:2"));
    // A file in no source folder has no path from one: the clipboard stays, the status bar says
    // why.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopySourceRootPath", window, cx)
    });
    assert_eq!(clipboard(cx).as_deref(), Some("deep.txt:2"));
    assert_eq!(
        status(cx).as_deref(),
        Some("Path from Source Root: not in a source folder")
    );
    // Each row is an action of its own.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyAbsolutePath", window, cx)
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|i| i.text()),
        Some(root.join("deep.txt").display().to_string())
    );
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyFileName", window, cx)
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|i| i.text()).as_deref(),
        Some("deep.txt")
    );
}

/// SPEC-0332 v2 AK-15: with the keyboard focus in the project tree the copy actions act on its
/// selection, one line per item: Path from Source Root for two files of a source folder derived
/// from their package lines, and the rows of the popup, which offers neither a line nor a
/// reference there.
#[gpui::test]
fn copy_paths_of_the_tree_selection(cx: &mut TestAppContext) {
    let root = workspace("tree-copy");
    let (bench, cx) = window(&root, cx);
    let (cart, many) = (
        root.join("src/shop/Cart.java"),
        root.join("src/shop/Many.java"),
    );
    bench.update_in(cx, |b, window, cx| {
        assert!(b.reveal_in_tree(&cart, cx), "the class is in the tree");
        b.focus_tree(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("shift-down");
    cx.run_until_parked();
    let clipboard = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|i| i.text());
    // The source folders are derived off the UI thread: until then the files are in none.
    until(cx, "src is a source folder", |cx| {
        bench.update_in(cx, |b, window, cx| {
            b.menu_action("CopySourceRootPath", window, cx)
        });
        clipboard(cx).as_deref() == Some("shop/Cart.java\nshop/Many.java")
    });
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)).as_deref(),
        Some("Path from Source Root copied: shop/Cart.java (and 1 more line)")
    );

    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CopyReferencePopupGroup", window, cx)
    });
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the copy list is open");
    assert_eq!(
        popup.read_with(cx, |p, _| p.shown_items()),
        [
            format!("Absolute Path    {}  {}", cart.display(), many.display()),
            "File Name    Cart.java  Many.java".to_owned(),
            "Path from Content Root    src/shop/Cart.java  src/shop/Many.java".to_owned(),
            "Path from Source Root    shop/Cart.java  shop/Many.java".to_owned(),
        ]
    );
    popup.update(cx, |p, cx| {
        p.select(1);
        cx.emit(p.accept());
    });
    cx.run_until_parked();
    assert_eq!(clipboard(cx).as_deref(), Some("Cart.java\nMany.java"));
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)).as_deref(),
        Some("File Name copied: Cart.java (and 1 more line)")
    );
}

//! Window check of the editor in `Islands Light` (`SPEC-0060` v13 AK-19, `SPEC-0291` v9 AK-9,
//! `SPEC-0061` v19 AK-v19, OBS-0079 L1–L4): with the language server's tokens and hints of
//! `Inventory.java` (as jdtls sends them), method declarations are drawn like plain text, calls of
//! static methods in italics, the parameter name hints on grey pills, and the selected editor tab
//! takes the theme's colours of a focused or an unfocused tab group. The chosen theme is one value
//! for the whole process, so this test has a process of its own. No language server takes part.
//!
//! Window tests need gpui's test support, which the workspace enables on macOS only.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_languages::inlay::ParameterHint;
use ferris_idea_languages::semantic::SemanticToken;
use ferris_idea_ui::theme::{KeyFamily, semantic_key_in};
use ferris_idea_workbench::palette::{Palette, ThemeId, set_current_theme};
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

/// The test project's file of OBS-0079.
const INVENTORY: &str = "package lab.inventory;

import java.util.List;

public final class Inventory {
    public record Item(String sku, String name, int qty, int unitPriceCents) {
        int totalCents() {
            return qty * unitPriceCents;
        }
    }

    public static int totalValue(List<Item> items) {
        return items.stream().mapToInt(Item::totalCents).sum();
    }

    public static void main(String[] args) {
        System.out.println(totalValue(List.of(new Item(\"A-1\", \"Bolt\", 3, 25))));
    }
}
";

fn workspace() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-islands-light-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Inventory.java"), INVENTORY).unwrap();
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

/// The 0-based line of the first line containing `needle` and the column of `needle` there.
fn find(needle: &str) -> (u64, u64) {
    INVENTORY
        .lines()
        .enumerate()
        .find_map(|(i, l)| l.find(needle).map(|c| (i as u64, c as u64)))
        .unwrap_or_else(|| panic!("{needle} is in the file"))
}

/// A token as jdtls sends it for the name at the start of `at` (recorded from jdtls 1.64 for
/// this file): the name is `at` up to its first character that is no letter.
fn token(at: &str, ty: &str, modifiers: &[&str]) -> SemanticToken {
    let (line, start) = find(at);
    let name = at
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or(at);
    SemanticToken {
        line,
        start,
        end: start + name.len() as u64,
        token_type: ty.into(),
        modifiers: modifiers.iter().map(|m| (*m).to_owned()).collect(),
        key: semantic_key_in(ty, modifiers, KeyFamily::Java),
    }
}

fn hint(before: &str, label: &str) -> ParameterHint {
    let (line, character) = find(before);
    ParameterHint {
        line,
        character,
        label: label.into(),
        padding_left: false,
        padding_right: true,
        is_type: false,
        is_value: false,
    }
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..20 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn islands_light_draws_the_editor_like_the_reference(cx: &mut TestAppContext) {
    set_current_theme(ThemeId::IslandsLight);
    let root = workspace();
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("src/Inventory.java"), window, cx);
        b.focus_editor(window, cx);
    });
    settle(cx);
    let editor: Entity<EditorView> = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    // Everything unfolded, as in OBS-0079.
    editor.update(cx, |e, cx| assert!(e.run_action("ExpandAllRegions", cx)));
    let tokens = [
        token("totalCents()", "method", &["declaration"]),
        token(
            "totalValue(List",
            "method",
            &["static", "declaration", "public"],
        ),
        token("main(", "method", &["static", "declaration", "public"]),
        token("out.", "property", &["static", "readonly", "public"]),
        token("println(", "method", &["public"]),
        token("totalValue(List.of", "method", &["static", "public"]),
        token("of(new", "method", &["static", "public", "generic"]),
    ];
    let hints = [
        hint("\"A-1\"", "sku:"),
        hint("\"Bolt\"", "name:"),
        hint("3, 25", "qty:"),
        hint("25)", "unitPriceCents:"),
    ];
    editor.update(cx, |e, cx| {
        let revision = e.revision();
        assert!(e.set_semantic_tokens(revision, &tokens, cx));
        assert!(e.set_parameter_hints(revision, &hints, cx));
    });
    settle(cx);

    let (decl_line, _) = find("totalValue(List");
    let (call_line, _) = find("System.out");
    let italic = |line: u64, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.painted_italic_texts(line as usize))
    };
    // L2: the static field and both static calls are italic, nothing else on the line.
    assert_eq!(italic(call_line, cx), ["out", "totalValue", "of"]);
    // L1: a static method's declaration is upright and has the text colour.
    assert!(italic(decl_line, cx).is_empty());
    let p = Palette::current();
    let declaration = p.key_style("METHOD_DECLARATION_ATTRIBUTES");
    assert_eq!(declaration.foreground, p.key_style("TEXT").foreground);
    // L3: each of the four hints stands on a pill of the scheme's hint background.
    let pills = editor.read_with(cx, |e, _| e.painted_hint_pills().to_vec());
    let grey = p
        .key_style("INLINE_PARAMETER_HINT")
        .background
        .expect("a hint background");
    assert_eq!(pills, vec![(call_line as usize, grey); 4]);

    // L4: the selected tab is active while the keyboard focus is in its editor, not while it is
    // in the Project tree.
    cx.update(|window, _| window.activate_window());
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.update(|window, _| window.refresh());
    settle(cx);
    assert!(bench.read_with(cx, |b, _| b.editor_tabs_active()));
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.update(|window, _| window.refresh());
    settle(cx);
    assert!(!bench.read_with(cx, |b, _| b.editor_tabs_active()));
    // An inactive window draws no tab as active.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.deactivate_window();
    cx.update(|window, _| window.refresh());
    settle(cx);
    assert!(!bench.read_with(cx, |b, _| b.editor_tabs_active()));
    let (active, inactive) = (
        ferris_idea_workbench::metrics::pill_colours(p, true),
        ferris_idea_workbench::metrics::pill_colours(p, false),
    );
    assert_eq!((active.0.rgb, active.1.rgb), (0xe3ebfe, 0xa7c5ff));
    assert_eq!((inactive.0.rgb, inactive.1.rgb), (0xe9eaee, 0xd1d3d9));
}

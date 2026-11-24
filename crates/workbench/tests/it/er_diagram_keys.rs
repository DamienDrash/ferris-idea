//! Window check of `SPEC-0111` v5, headless on SQLite: the foreign-key line names its columns,
//! Tab and ⇧Tab walk the boxes, ⌥E and the arrow keys export, Escape closes the menu first. v8:
//! lines go around boxes, and the yEd GraphML and draw.io exports.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::WorkbenchView;
use gpui::{TestAppContext, VisualTestContext, px};

/// SPEC-0111/AK-10 (v5).
#[gpui::test]
fn boxes_and_exports_by_keyboard(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("ferris-idea-dbecp-er-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let db = root.join("shop.db");
    let made = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
             CREATE TABLE orders (id INTEGER PRIMARY KEY, \
             customer_id INTEGER NOT NULL REFERENCES customers(id));",
        )
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            db.display()
        ),
    )
    .unwrap();
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.ShowErDiagram", window, cx)
    });
    for _ in 0..400 {
        cx.run_until_parked();
        if bench.read_with(cx, |b, _| b.er_view().is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let er = bench
        .read_with(cx, |b, _| b.er_view())
        .expect("the diagram opens");
    let labels: Vec<String> = er.read_with(cx, |v, _| {
        v.layout().lines.iter().map(|l| l.label.clone()).collect()
    });
    assert_eq!(labels, ["customer_id:id"]);

    let selected =
        |cx: &mut VisualTestContext| er.read_with(cx, |v, _| v.selected().map(str::to_owned));
    cx.simulate_keystrokes("tab");
    assert_eq!(selected(cx).as_deref(), Some("customers"));
    cx.simulate_keystrokes("tab");
    assert_eq!(selected(cx).as_deref(), Some("orders"));
    cx.simulate_keystrokes("tab");
    assert_eq!(selected(cx).as_deref(), Some("customers"), "Tab wraps");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(selected(cx).as_deref(), Some("orders"));

    cx.simulate_keystrokes("alt-e");
    assert_eq!(
        er.read_with(cx, |v, _| v.marked_export()).as_deref(),
        Some("Mermaid")
    );
    cx.simulate_keystrokes("down down down");
    assert_eq!(
        er.read_with(cx, |v, _| v.marked_export()).as_deref(),
        Some("SVG")
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        !er.read_with(cx, |v, _| v.export_menu_open()),
        "Enter closes the menu"
    );
    let svg =
        std::fs::read_to_string(root.join(".ferris-idea/diagrams/shop.svg")).unwrap_or_default();
    assert!(svg.contains(">customer_id:id</text>"), "{svg}");

    cx.simulate_keystrokes("alt-e");
    cx.simulate_keystrokes("escape");
    assert!(!er.read_with(cx, |v, _| v.export_menu_open()));
    assert!(
        bench.read_with(cx, |b, _| b.er_view().is_some()),
        "Escape closes only the menu"
    );
    cx.simulate_keystrokes("escape");
    assert!(bench.read_with(cx, |b, _| b.er_view().is_none()));
    let _ = std::fs::remove_dir_all(&root);
}

/// Where `layout` breaks the rule of SPEC-0111 v8 behaviour 16: a slanted segment, or one that
/// crosses or touches a box other than its line's ends, or its own boxes away from its ends.
fn breaches(layout: &ferris_idea_database::er_layout::DiagramLayout) -> Vec<String> {
    let mut out = Vec::new();
    for l in &layout.lines {
        let index = |n: &str| layout.boxes.iter().position(|b| b.name == n).unwrap();
        let (start, end) = (index(&l.to), index(&l.from));
        let last = l.points.len() - 2;
        for (k, w) in l.points.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            if a.0 != b.0 && a.1 != b.1 {
                out.push(format!("{}-{}: slanted {a:?} {b:?}", l.from, l.to));
            }
            for (i, r) in layout.boxes.iter().enumerate() {
                if (i == start && k == 0) || (i == end && k == last) {
                    continue;
                }
                if a.0.min(b.0) <= r.x + r.width
                    && a.0.max(b.0) >= r.x
                    && a.1.min(b.1) <= r.y + r.height
                    && a.1.max(b.1) >= r.y
                {
                    out.push(format!(
                        "{}-{}: {a:?} {b:?} touches {}",
                        l.from, l.to, r.name
                    ));
                }
            }
        }
    }
    out
}

/// For each line, how near it comes to a box it must keep away from (every box but its ends, and
/// its own boxes away from its first and last segment), measured straight, with that box's name.
fn nearest(layout: &ferris_idea_database::er_layout::DiagramLayout) -> Vec<(String, f32, String)> {
    let mut out = Vec::new();
    for l in &layout.lines {
        let index = |n: &str| layout.boxes.iter().position(|b| b.name == n).unwrap();
        let (start, end) = (index(&l.to), index(&l.from));
        let last = l.points.len() - 2;
        let mut best = (f32::INFINITY, String::new());
        for (k, w) in l.points.windows(2).enumerate() {
            let (x0, x1) = (w[0].0.min(w[1].0), w[0].0.max(w[1].0));
            let (y0, y1) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
            for (i, r) in layout.boxes.iter().enumerate() {
                if (i == start && k == 0) || (i == end && k == last) {
                    continue;
                }
                let dx = (r.x - x1).max(x0 - (r.x + r.width)).max(0.);
                let dy = (r.y - y1).max(y0 - (r.y + r.height)).max(0.);
                if dx.hypot(dy) < best.0 {
                    best = (dx.hypot(dy), r.name.clone());
                }
            }
        }
        out.push((format!("{}-{}", l.from, l.to), best.0, best.1));
    }
    out
}

/// SPEC-0111/AK-16 (v8): lines go around boxes in the window, 8 px clear in the layered layout and
/// clear of every box after a move, and the editor exports are written and copied with names as
/// text.
#[gpui::test]
fn lines_go_around_boxes_and_editor_exports(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("ferris-idea-er-around-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let db = root.join("inv.db");
    let made = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg(
            "CREATE TABLE item (sku TEXT PRIMARY KEY, name TEXT NOT NULL, \
             unit_price_cents INTEGER NOT NULL); \
             CREATE TABLE warehouse (id INTEGER PRIMARY KEY, city TEXT NOT NULL); \
             CREATE TABLE movement (id INTEGER PRIMARY KEY, \
             warehouse_id INTEGER NOT NULL REFERENCES warehouse(id), \
             sku TEXT NOT NULL REFERENCES item(sku), delta INTEGER NOT NULL, \
             moved_at \"timestamp without time zone\" NOT NULL); \
             CREATE TABLE pd_note (id INTEGER PRIMARY KEY, \
             warehouse_id INTEGER NOT NULL REFERENCES warehouse(id), body TEXT, \
             amount NUMERIC, qty INTEGER); \
             CREATE TABLE stock (warehouse_id INTEGER NOT NULL REFERENCES warehouse(id), \
             sku TEXT NOT NULL REFERENCES item(sku), qty INTEGER NOT NULL, \
             PRIMARY KEY (warehouse_id, sku)); \
             CREATE TABLE pd_move (id INTEGER PRIMARY KEY, \
             movement_id INTEGER NOT NULL REFERENCES movement(id), \
             warehouse_id INTEGER NOT NULL REFERENCES warehouse(id), note TEXT); \
             CREATE VIEW stock_value AS SELECT s.warehouse_id, s.qty * i.unit_price_cents \
             AS value_cents FROM stock s JOIN item i ON i.sku = s.sku; \
             CREATE TABLE \"<html><img src=x>\" (\"</y:NodeLabel><y:Evil/>\" INTEGER);",
        )
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        eprintln!("skipped: no sqlite3 program");
        return;
    }
    std::fs::write(
        root.join(".ferris-idea/connections.toml"),
        format!(
            "[[connection]]\nid = \"inv\"\nname = \"inv\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            db.display()
        ),
    )
    .unwrap();
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Sql.ShowErDiagram", window, cx)
    });
    for _ in 0..400 {
        cx.run_until_parked();
        if bench.read_with(cx, |b, _| b.er_view().is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let er = bench
        .read_with(cx, |b, _| b.er_view())
        .expect("the diagram opens");

    // The drawing the window paints keeps the rule; some lines had to go around.
    let drawn = er.read_with(cx, |v, _| v.layout().clone());
    assert_eq!(drawn.boxes.len(), 8, "{:?}", drawn.boxes);
    assert_eq!(breaches(&drawn), Vec::<String>::new());
    for (line, d, near) in nearest(&drawn) {
        assert!(d >= 8. - 1e-3, "{line} runs {d} px from {near}");
    }
    assert!(
        drawn.lines.iter().any(|l| l.points.len() > 4),
        "a line led around a box: {:?}",
        drawn.lines
    );
    // Also after `item` is dragged to the far right, so its lines cross the whole diagram.
    let item = drawn.boxes.iter().position(|b| b.name == "item").unwrap();
    let (x, y) = (drawn.width + 40., 40.);
    er.update(cx, |v, cx| v.move_box(item, x, y, cx));
    let moved = er.read_with(cx, |v, _| v.layout().clone());
    assert_eq!((moved.boxes[item].x, moved.boxes[item].y), (x, y));
    assert_eq!(breaches(&moved), Vec::<String>::new());

    // ⌥E, down to yEd GraphML, Enter; then draw.io.
    cx.update(|window, cx| window.focus(&er.read(cx).focus()));
    let clipboard = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|i| i.text());
    for (downs, label, file, expected) in [
        (
            7,
            "yEd GraphML",
            "inv.graphml",
            ferris_idea_database::er_editors::to_graphml(&moved),
        ),
        (
            8,
            "draw.io",
            "inv.drawio",
            ferris_idea_database::er_editors::to_drawio(&moved),
        ),
    ] {
        cx.simulate_keystrokes("alt-e");
        for _ in 0..downs {
            cx.simulate_keystrokes("down");
        }
        assert_eq!(
            er.read_with(cx, |v, _| v.marked_export()).as_deref(),
            Some(label)
        );
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let written = std::fs::read_to_string(root.join(".ferris-idea/diagrams").join(file))
            .unwrap_or_default();
        assert_eq!(written, expected, "{file} holds the drawing as moved");
        assert_eq!(
            clipboard(cx).as_deref(),
            Some(expected.as_str()),
            "{file} copied"
        );
        assert!(written.starts_with("<?xml"));
        assert!(
            !written.contains("<y:Evil") && !written.contains("<html>"),
            "{written}"
        );
        assert!(
            written.contains("&lt;html&gt;&lt;img src=x&gt;"),
            "{written}"
        );
        assert!(
            written.contains("&lt;/y:NodeLabel&gt;&lt;y:Evil/&gt;"),
            "{written}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

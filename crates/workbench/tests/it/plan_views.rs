//! Window check of `SPEC-0110` v13 behaviours 19 and 20, headless on SQLite (AK-25): ⌥R goes
//! through Tree, Diagram, Flame and Raw, the selected step stays selected in every view with
//! steps, and the properties of the selected step list every field and its hint. v14 behaviour 21
//! (AK-26): the head with the switches stays while the view scrolls, several hints fold to one
//! line, and the diagram is fitted into the area below the head.

#![cfg(target_os = "macos")]

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_workbench::plan_view_model::{FlameMetric, PlanMode};
use idecraft_workbench::{WorkbenchView, semantics};

const QUERY: &str = "select c.name, o.id from customers c join orders o on o.customer_id = c.id;\n";

/// A plan five rows deep with three full scans: two unions over a join and two scans.
const DEEP: &str = "select name from customers union all select c.name from customers c join orders o \
    on o.customer_id = c.id union all select name from customers where name like 'a%' order by 1;\n";

fn sqlite(db: &std::path::Path, sql: &str) -> bool {
    std::process::Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .status()
        .is_ok_and(|s| s.success())
}

/// Two SQLite connections: `shop` with `customers` and `orders`, `copy` with `customers` and a
/// table `extra` that `shop` lacks. `None` without the `sqlite3` program.
fn workspace(tag: &str, query: &str) -> Option<std::path::PathBuf> {
    let root =
        std::env::temp_dir().join(format!("idecraft-planviews-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    let (shop, copy) = (root.join("shop.db"), root.join("copy.db"));
    let made = sqlite(
        &shop,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));",
    ) && sqlite(
        &copy,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE extra (id INTEGER);",
    );
    if !made {
        let _ = std::fs::remove_dir_all(&root);
        return None;
    }
    std::fs::write(
        root.join(".idecraft/connections.toml"),
        format!(
            "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n\n\
             [[connection]]\nid = \"copy\"\nname = \"copy\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
            shop.display(),
            copy.display()
        ),
    )
    .unwrap();
    std::fs::write(root.join("query.sql"), query).unwrap();
    Some(root)
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = root.to_path_buf();
    let file = root.join("query.sql");
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.toggle_database(window, cx);
    });
    cx.run_until_parked();
    select_shop(&bench, cx);
    (bench, cx)
}

fn select_shop(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        let view = b.database_view().expect("the Database window is open");
        view.update(cx, |v, cx| {
            let row = v
                .visible()
                .iter()
                .position(|(_, name, _)| name == "shop")
                .expect("shop is listed");
            v.select(row, cx);
        });
    });
}

/// Waits for the background work of the Services window.
fn settle(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    done: fn(&WorkbenchView, &gpui::App) -> bool,
) {
    for _ in 0..400 {
        cx.run_until_parked();
        if bench.read_with(cx, done) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("the background work did not finish");
}

/// SPEC-0110 v13 AK-25.
#[gpui::test]
fn diagram_flame_and_properties(cx: &mut TestAppContext) {
    let Some(root) = workspace("views", QUERY) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        b.menu_action("Sql.ExplainPlan", window, cx);
    });
    settle(&bench, cx, |b, cx| {
        b.services_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.running() && v.plan().is_some()
        })
    });
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    assert_eq!(services.read_with(cx, |v, _| v.plan_mode()), PlanMode::Tree);
    cx.update(|window, cx| window.focus(&services.read(cx).focus()));
    cx.simulate_keystrokes("down");
    assert_eq!(services.read_with(cx, |v, _| v.plan_step()), Some(0));
    let props = services.read_with(cx, |v, _| v.plan_step_properties());
    assert_eq!(
        props.first().map(|(n, _)| n.as_str()),
        Some("Operation"),
        "{props:?}"
    );
    assert!(
        props
            .iter()
            .any(|(n, v)| n == "⚠ Hint" && v.starts_with("full scan of `o`")),
        "the hint is at the step: {props:?}"
    );

    cx.simulate_keystrokes("alt-r");
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_mode()),
        PlanMode::Diagram
    );
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_step()),
        Some(0),
        "selection kept"
    );
    cx.simulate_keystrokes("down");
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_step()),
        Some(1),
        "↓ in the diagram"
    );

    cx.simulate_keystrokes("alt-r");
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_mode()),
        PlanMode::Flame
    );
    assert_eq!(
        services.read_with(cx, |v, _| v.flame_metric()),
        FlameMetric::Total
    );
    services.update(cx, |v, cx| v.set_flame_metric(FlameMetric::Time, cx));
    assert_eq!(
        services.read_with(cx, |v, _| v.flame_metric()),
        FlameMetric::Total,
        "no Time for a plan that was not measured"
    );
    services.update(cx, |v, cx| v.set_flame_metric(FlameMetric::Startup, cx));
    assert_eq!(
        services.read_with(cx, |v, _| v.flame_metric()),
        FlameMetric::Startup
    );
    cx.simulate_keystrokes("up");
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_step()),
        Some(0),
        "↑ in the flame view"
    );

    cx.simulate_keystrokes("alt-r");
    assert!(services.read_with(cx, |v, _| v.plan_raw()));
    cx.simulate_keystrokes("alt-r");
    assert_eq!(services.read_with(cx, |v, _| v.plan_mode()), PlanMode::Tree);
    assert_eq!(
        bench.read_with(cx, |b, cx| b.active_text(cx)).as_deref(),
        Some(QUERY),
        "no key reached the editor"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Draws a frame and the one it asks for (the fitted diagram is drawn in the frame after the one
/// that measured its area).
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
    }
}

/// Name, bounds and shown part of the node with test id `id` in the frame just drawn.
fn node(
    cx: &mut VisualTestContext,
    id: &str,
) -> Option<(String, semantics::Rect, Option<semantics::Rect>)> {
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        tree.iter().find(|n| n.test_id() == Some(id)).map(|n| {
            (
                n.name().unwrap_or_default().to_owned(),
                n.bounds(),
                n.visible_bounds,
            )
        })
    })
}

/// The middle of what is shown of the node `id`.
fn spot(cx: &mut VisualTestContext, id: &str) -> gpui::Point<gpui::Pixels> {
    let (_, _, shown) = node(cx, id).unwrap_or_else(|| panic!("{id} is drawn"));
    let r = shown.unwrap_or_else(|| panic!("{id} is in view"));
    point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
}

fn click(cx: &mut VisualTestContext, id: &str) {
    let at = spot(cx, id);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn same(a: semantics::Rect, b: semantics::Rect) -> bool {
    (a.x - b.x).abs() < 0.5
        && (a.y - b.y).abs() < 0.5
        && (a.w - b.w).abs() < 0.5
        && (a.h - b.h).abs() < 0.5
}

/// SPEC-0110 v14 AK-26.
#[gpui::test]
fn the_head_stays_and_the_diagram_fits(cx: &mut TestAppContext) {
    let Some(root) = workspace("head", DEEP) else {
        eprintln!("skipped: no sqlite3 program");
        return;
    };
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.focus_editor(window, cx);
        b.menu_action("Sql.ExplainPlan", window, cx);
    });
    settle(&bench, cx, |b, cx| {
        b.services_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.running() && v.plan().is_some()
        })
    });
    let services = bench.read_with(cx, |b, _| b.services_view()).unwrap();
    let hints = services.read_with(cx, |v, _| v.plan().map(|p| p.hints.clone()).unwrap());
    assert_eq!(hints.len(), 3, "three full scans: {hints:?}");
    draw(cx);

    // Several hints take one line: the first and how many more there are.
    let (name, line, _) = node(cx, "plan-hints").expect("the hint line is drawn");
    assert_eq!(name, format!("⚠ {} · 2 more hints ▾", hints[0]));
    assert!(line.h < 30., "one line: {line:?}");
    assert!(
        node(cx, "plan-hint-1").is_none(),
        "the other hints are folded"
    );

    // ⌥H shows every hint on its own line; a click on the first line folds them again.
    cx.update(|window, cx| window.focus(&services.read(cx).focus()));
    cx.simulate_keystrokes("alt-h");
    assert!(services.read_with(cx, |v, _| v.plan_hints_open()));
    draw(cx);
    let second = node(cx, "plan-hint-1").expect("the second hint is shown");
    let third = node(cx, "plan-hint-2").expect("the third hint is shown");
    assert_eq!(second.0, format!("⚠ {}", hints[1]));
    assert!(third.1.y > second.1.y, "each on its own line");
    assert!(node(cx, "plan-hints").unwrap().0.ends_with("fewer hints ▴"));
    click(cx, "plan-hints");
    assert!(!services.read_with(cx, |v, _| v.plan_hints_open()));
    assert!(node(cx, "plan-hint-1").is_none(), "folded again");

    // The diagram is fitted into the area below the head: below 100 %, every box whole in view.
    cx.update(|window, cx| window.focus(&services.read(cx).focus()));
    cx.simulate_keystrokes("alt-r");
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_mode()),
        PlanMode::Diagram
    );
    draw(cx);
    let fitted = |cx: &mut VisualTestContext| {
        services.read_with(cx, |v, _| {
            let (w, h) = v.plan_area();
            let (m, zoom) = v.plan_diagram_zoom().unwrap();
            let (w, h) = (w - 24., h - 8.);
            let fit = idecraft_database::er_layout::fit_zoom(m.width, m.height, w, h);
            let inside = m.width * zoom <= w + 0.5 && m.height * zoom <= h + 0.5;
            ((w, h), zoom, fit, inside)
        })
    };
    // As it opens (SPEC-0110 v15 behaviour 22): fitted, but not below the readable 60 %, and the
    // top step whole in view.
    let (area, zoom, fit, _) = fitted(cx);
    assert!(area.0 > 0. && area.1 > 0., "the area is measured");
    let readable = idecraft_workbench::plan_view_model::READABLE_ZOOM;
    assert!(
        (zoom - fit.clamp(readable, 1.)).abs() < 1e-3,
        "zoom {zoom}, fit {fit} in {area:?}"
    );
    let (_, bounds, shown) = node(cx, "plan-box-0").expect("the top step is drawn");
    assert!(
        shown.is_some_and(|s| same(s, bounds)),
        "the top step is whole in view: {bounds:?}, shown {shown:?}"
    );
    // Fit chosen: the whole diagram inside the area, below 60 % where it must be.
    click(cx, "plan-zoom-fit");
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_diagram_zoom_mode()),
        idecraft_workbench::plan_view_model::DiagramZoom::Fit
    );
    let (area, zoom, fit, inside) = fitted(cx);
    assert!(
        zoom < 1. && (zoom - fit).abs() < 1e-3,
        "zoom {zoom}, fit {fit} in {area:?}"
    );
    assert!(inside, "the diagram is inside {area:?} at {zoom}");
    let steps = services.read_with(cx, |v, _| {
        v.plan()
            .map_or(0, |p| idecraft_workbench::plan_view_model::steps(p).len())
    });
    assert!(steps >= 9, "{steps} steps");
    for i in 0..steps {
        let (_, bounds, shown) = node(cx, &format!("plan-box-{i}")).expect("every box is drawn");
        assert!(
            shown.is_some_and(|s| same(s, bounds)),
            "box {i} is whole in view: {bounds:?}, shown {shown:?}"
        );
    }

    // At 100 % the diagram is larger than the area; the wheel scrolls the view below the head,
    // and the switches and the hint line stay where they are.
    click(cx, "plan-zoom-100");
    assert!(!services.read_with(cx, |v, _| v.plan_diagram_fit()));
    assert_eq!(
        services.read_with(cx, |v, _| v.plan_diagram_zoom().unwrap().1),
        1.
    );
    let switch = node(cx, "plan-tree").unwrap().1;
    let hint_line = node(cx, "plan-hints").unwrap().1;
    let top_box = node(cx, "plan-box-0").unwrap().1;
    let at = spot(cx, "plan-box-0");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-120.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(cx);
    let moved = node(cx, "plan-box-0").unwrap().1;
    assert!(
        moved.y < top_box.y - 60.,
        "the view scrolled: {top_box:?} → {moved:?}"
    );
    assert!(
        (moved.x - top_box.x).abs() < 0.5,
        "a wheel turn down does not move the diagram sideways"
    );
    assert!(
        same(node(cx, "plan-tree").unwrap().1, switch),
        "the switches stay"
    );
    assert!(
        same(node(cx, "plan-hints").unwrap().1, hint_line),
        "the hint line stays"
    );

    // Fitted again, the diagram follows its area when the window changes size: larger in a
    // taller window, every box still whole in view.
    click(cx, "plan-zoom-fit");
    cx.simulate_resize(gpui::size(px(1372.), px(1300.)));
    draw(cx);
    let (taller, larger, fit, inside) = fitted(cx);
    assert!(taller.1 > area.1, "the area grew: {area:?} → {taller:?}");
    assert!(
        (larger - fit).abs() < 1e-3 && larger > zoom && inside,
        "zoom {larger} (was {zoom}), fit {fit} in {taller:?}"
    );
    for i in 0..steps {
        let (_, bounds, shown) = node(cx, &format!("plan-box-{i}")).expect("every box is drawn");
        assert!(
            shown.is_some_and(|s| same(s, bounds)),
            "box {i} is whole in view: {bounds:?}, shown {shown:?}"
        );
    }
    // In a low window it stops at 25 %, as the ER diagram does, and the view scrolls the rest.
    cx.simulate_resize(gpui::size(px(1100.), px(700.)));
    draw(cx);
    let (low, smallest, fit, inside) = fitted(cx);
    assert!(
        (smallest - fit).abs() < 1e-3 && smallest < zoom,
        "zoom {smallest}, fit {fit} in {low:?}"
    );
    assert!(inside || smallest == 0.25, "zoom {smallest} in {low:?}");
    assert_eq!(
        bench.read_with(cx, |b, cx| b.active_text(cx)).as_deref(),
        Some(DEEP),
        "no key reached the editor"
    );
    let _ = std::fs::remove_dir_all(&root);
}

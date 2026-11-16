//! ER diagram exports for diagram editors (`SPEC-0022` v4 behaviours 15–17): yEd GraphML and the
//! uncompressed `.drawio` file of draw.io, both from the drawing of `SPEC-0111` (boxes as moved,
//! lines with their routes), and the XML text every name goes through. Plain string building: the
//! exports make no network connection and name no host (`SPEC-0022` behaviour 13, IMP-0054).

use crate::er_layout::{BoxLayout, DiagramLayout, HEADER_HEIGHT, LINE_HEIGHT, LineLayout};

/// `text` as XML character data or attribute value (`SPEC-0022` v4 behaviour 17): `&`, `<`, `>`,
/// `"` and `'` as entities, line feed, carriage return and tab as character references, characters
/// XML 1.0 does not allow as U+FFFD.
pub fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}' => {
                out.push(c);
            }
            _ => out.push('\u{FFFD}'),
        }
    }
    out
}

/// `text` as the text of a label that an editor would read as HTML when it begins with `<` (yEd):
/// [`xml_text`] with a zero-width space in front of such a text, so it stays plain text.
pub fn label_text(text: &str) -> String {
    if text.trim_start().starts_with('<') {
        format!("\u{200B}{}", xml_text(text))
    } else {
        xml_text(text)
    }
}

fn num(v: f32) -> String {
    format!("{}", (v * 10.).round() / 10.)
}

/// The name shown in a box's header.
fn title(b: &BoxLayout) -> String {
    if b.view {
        format!("{} (view)", b.name)
    } else {
        b.name.clone()
    }
}

/// One column as a line of text: key marks, name, `:` and type, keys left out when there are none.
fn column_line((keys, name, ty, _): &(String, String, String, bool)) -> String {
    let mut line = if keys.is_empty() {
        name.clone()
    } else {
        format!("{keys} {name}")
    };
    if !ty.is_empty() {
        line.push_str(" : ");
        line.push_str(ty);
    }
    line
}

/// The boxes a line starts and ends at (indices into the drawing's boxes).
fn ends(layout: &DiagramLayout, l: &LineLayout) -> Option<(usize, usize)> {
    let index = |name: &str| layout.boxes.iter().position(|b| b.name == name);
    index(&l.to).zip(index(&l.from))
}

/// The crow's feet of yEd at the referenced and the referencing end of a line.
fn yed_arrows(l: &LineLayout) -> (&'static str, &'static str) {
    let referenced = match l.to_label.as_str() {
        "1" => "crows_foot_one_mandatory",
        "0..1" => "crows_foot_one_optional",
        _ => "none",
    };
    let referencing = match l.from_label.as_str() {
        "N" => "crows_foot_many_optional",
        "1" => "crows_foot_one_optional",
        _ => "none",
    };
    (referenced, referencing)
}

/// The drawing as GraphML with the yWorks extension that yEd reads (`SPEC-0022` v4 behaviour 15).
/// The namespace names are identifiers only; no schema location points a reader at a host.
pub fn to_graphml(layout: &DiagramLayout) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
         <graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\" \
         xmlns:y=\"http://www.yworks.com/xml/graphml\">\n\
         \x20 <key for=\"node\" id=\"d0\" yfiles.type=\"nodegraphics\"/>\n\
         \x20 <key for=\"edge\" id=\"d1\" yfiles.type=\"edgegraphics\"/>\n\
         \x20 <key for=\"node\" id=\"d2\" attr.name=\"name\" attr.type=\"string\"/>\n\
         \x20 <key for=\"edge\" id=\"d3\" attr.name=\"columns\" attr.type=\"string\"/>\n\
         \x20 <graph id=\"G\" edgedefault=\"directed\">\n",
    );
    for (i, b) in layout.boxes.iter().enumerate() {
        let columns: Vec<String> = b.lines.iter().map(column_line).collect();
        out.push_str(&format!(
            "    <node id=\"n{i}\">\n      <data key=\"d2\">{name}</data>\n      <data key=\"d0\">\n        \
             <y:GenericNode configuration=\"com.yworks.entityRelationship.big_entity\">\n          \
             <y:Geometry x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"/>\n          \
             <y:Fill color=\"#F9FAFB\" transparent=\"false\"/>\n          \
             <y:BorderStyle color=\"#9CA3AF\" type=\"{border}\" width=\"1.0\"/>\n          \
             <y:NodeLabel alignment=\"center\" autoSizePolicy=\"content\" backgroundColor=\"#E5E7EB\" \
             configuration=\"com.yworks.entityRelationship.label.name\" fontFamily=\"Dialog\" \
             fontSize=\"12\" fontStyle=\"bold\" hasLineColor=\"false\" modelName=\"internal\" \
             modelPosition=\"t\" textColor=\"#111827\" visible=\"true\">{title}</y:NodeLabel>\n          \
             <y:NodeLabel alignment=\"left\" autoSizePolicy=\"content\" \
             configuration=\"com.yworks.entityRelationship.label.attributes\" fontFamily=\"Monospaced\" \
             fontSize=\"12\" fontStyle=\"plain\" hasBackgroundColor=\"false\" hasLineColor=\"false\" \
             modelName=\"custom\" textColor=\"#111827\" visible=\"true\">{columns}<y:LabelModel>\
             <y:ErdAttributesNodeLabelModel/></y:LabelModel><y:ModelParameter>\
             <y:ErdAttributesNodeLabelModelParameter/></y:ModelParameter></y:NodeLabel>\n        \
             </y:GenericNode>\n      </data>\n    </node>\n",
            name = xml_text(&b.name),
            x = num(b.x),
            y = num(b.y),
            w = num(b.width),
            h = num(b.height),
            border = if b.view { "dashed" } else { "line" },
            title = label_text(&title(b)),
            columns = label_text(&columns.join("\n")),
        ));
    }
    for (i, l) in layout.lines.iter().enumerate() {
        let Some((source, target)) = ends(layout, l) else {
            continue;
        };
        let (Some(first), Some(last)) = (l.points.first(), l.points.last()) else {
            continue;
        };
        let centre = |b: &BoxLayout| (b.x + b.width / 2., b.y + b.height / 2.);
        let (sc, tc) = (centre(&layout.boxes[source]), centre(&layout.boxes[target]));
        let bends: String = l.points[1..l.points.len() - 1]
            .iter()
            .map(|(x, y)| {
                format!(
                    "            <y:Point x=\"{}\" y=\"{}\"/>\n",
                    num(*x),
                    num(*y)
                )
            })
            .collect();
        let (from_arrow, to_arrow) = if l.dependency {
            ("none", "none")
        } else {
            yed_arrows(l)
        };
        let label = if l.label.is_empty() {
            String::new()
        } else {
            format!(
                "          <y:EdgeLabel alignment=\"center\" fontFamily=\"Dialog\" fontSize=\"10\" \
                 fontStyle=\"plain\" textColor=\"#374151\" visible=\"true\">{}</y:EdgeLabel>\n",
                label_text(&l.label)
            )
        };
        out.push_str(&format!(
            "    <edge id=\"e{i}\" source=\"n{source}\" target=\"n{target}\">\n      \
             <data key=\"d3\">{columns}</data>\n      <data key=\"d1\">\n        <y:PolyLineEdge>\n          \
             <y:Path sx=\"{sx}\" sy=\"{sy}\" tx=\"{tx}\" ty=\"{ty}\">\n{bends}          </y:Path>\n          \
             <y:LineStyle color=\"#6B7280\" type=\"{style}\" width=\"1.0\"/>\n          \
             <y:Arrows source=\"{from_arrow}\" target=\"{to_arrow}\"/>\n{label}          \
             <y:BendStyle smoothed=\"false\"/>\n        </y:PolyLineEdge>\n      </data>\n    </edge>\n",
            columns = xml_text(&l.label),
            sx = num(first.0 - sc.0),
            sy = num(first.1 - sc.1),
            tx = num(last.0 - tc.0),
            ty = num(last.1 - tc.1),
            style = if l.dependency { "dashed" } else { "line" },
        ));
    }
    out.push_str("  </graph>\n</graphml>\n");
    out
}

/// The ER markers of draw.io at the referenced and the referencing end of a line.
fn drawio_markers(l: &LineLayout) -> (&'static str, &'static str) {
    let referenced = match l.to_label.as_str() {
        "1" => "ERmandOne",
        "0..1" => "ERzeroToOne",
        _ => "none",
    };
    let referencing = match l.from_label.as_str() {
        "N" => "ERzeroToMany",
        "1" => "ERzeroToOne",
        _ => "none",
    };
    (referenced, referencing)
}

/// Where `p` lies on box `b`, as fractions of its width and height (draw.io's exit and entry points).
fn fraction(b: &BoxLayout, p: (f32, f32)) -> (String, String) {
    let f = |v: f32| format!("{}", (v * 10_000.).round() / 10_000.);
    (
        f((p.0 - b.x) / b.width.max(1.)),
        f((p.1 - b.y) / b.height.max(1.)),
    )
}

/// The drawing as an uncompressed draw.io file (`SPEC-0022` v4 behaviour 16). Labels stay plain
/// text: no cell switches on HTML labels.
pub fn to_drawio(layout: &DiagramLayout) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile>\n  <diagram id=\"er\" name=\"ER diagram\">\n    \
         <mxGraphModel grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" \
         fold=\"1\" page=\"0\" pageScale=\"1\" math=\"0\" shadow=\"0\">\n      <root>\n        \
         <mxCell id=\"0\"/>\n        <mxCell id=\"1\" parent=\"0\"/>\n",
    );
    for (i, b) in layout.boxes.iter().enumerate() {
        out.push_str(&format!(
            "        <mxCell id=\"t{i}\" value=\"{value}\" style=\"swimlane;fontStyle=1;childLayout=stackLayout;\
             horizontal=1;startSize={header};horizontalStack=0;resizeParent=1;resizeParentMax=0;resizeLast=0;\
             collapsible=0;marginBottom=0;align=left;spacingLeft=8;fillColor=#e5e7eb;swimlaneFillColor=#f9fafb;\
             strokeColor=#9ca3af;fontColor=#111827;fontFamily=monospace;fontSize=12;html=0;{dashed}\" \
             vertex=\"1\" parent=\"1\">\n          \
             <mxGeometry x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" as=\"geometry\"/>\n        </mxCell>\n",
            value = xml_text(&title(b)),
            header = num(HEADER_HEIGHT),
            dashed = if b.view { "dashed=1;" } else { "" },
            x = num(b.x),
            y = num(b.y),
            w = num(b.width),
            h = num(b.height),
        ));
        for (c, line) in b.lines.iter().enumerate() {
            out.push_str(&format!(
                "        <mxCell id=\"t{i}c{c}\" value=\"{value}\" style=\"text;strokeColor=none;fillColor=none;\
                 align=left;verticalAlign=middle;spacingLeft=8;spacingRight=8;overflow=hidden;rotatable=0;\
                 points=[[0,0.5],[1,0.5]];portConstraint=eastwest;fontColor=#111827;fontFamily=monospace;\
                 fontSize=12;html=0;\" vertex=\"1\" parent=\"t{i}\">\n          \
                 <mxGeometry y=\"{y}\" width=\"{w}\" height=\"{h}\" as=\"geometry\"/>\n        </mxCell>\n",
                value = xml_text(&column_line(line)),
                y = num(HEADER_HEIGHT + LINE_HEIGHT * c as f32),
                w = num(b.width),
                h = num(LINE_HEIGHT),
            ));
        }
    }
    for (i, l) in layout.lines.iter().enumerate() {
        let Some((source, target)) = ends(layout, l) else {
            continue;
        };
        let (Some(&first), Some(&last)) = (l.points.first(), l.points.last()) else {
            continue;
        };
        let (exit_x, exit_y) = fraction(&layout.boxes[source], first);
        let (entry_x, entry_y) = fraction(&layout.boxes[target], last);
        let (start, end) = if l.dependency {
            ("none", "none")
        } else {
            drawio_markers(l)
        };
        let points: String = l.points[1..l.points.len() - 1]
            .iter()
            .map(|(x, y)| {
                format!(
                    "              <mxPoint x=\"{}\" y=\"{}\"/>\n",
                    num(*x),
                    num(*y)
                )
            })
            .collect();
        out.push_str(&format!(
            "        <mxCell id=\"r{i}\" value=\"{value}\" style=\"edgeStyle=none;rounded=0;html=0;\
             startArrow={start};endArrow={end};startFill=0;endFill=0;exitX={exit_x};exitY={exit_y};exitDx=0;\
             exitDy=0;entryX={entry_x};entryY={entry_y};entryDx=0;entryDy=0;strokeColor=#6b7280;\
             fontColor=#374151;fontFamily=monospace;fontSize=10;{dashed}\" edge=\"1\" parent=\"1\" \
             source=\"t{source}\" target=\"t{target}\">\n          <mxGeometry relative=\"1\" as=\"geometry\">\n            \
             <Array as=\"points\">\n{points}            </Array>\n          </mxGeometry>\n        </mxCell>\n",
            value = xml_text(&l.label),
            dashed = if l.dependency { "dashed=1;" } else { "" },
        ));
    }
    out.push_str("      </root>\n    </mxGraphModel>\n  </diagram>\n</mxfile>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::er_layout::{layout, layout_with_positions, to_svg};
    use crate::{Dependency, Entity, ErColumn, ErModel, Relationship};

    fn col(name: &str, ty: &str, keys: &[&str], nullable: bool) -> ErColumn {
        ErColumn {
            name: name.into(),
            data_type: ty.into(),
            nullable,
            keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        }
    }

    fn entity(name: &str, layer: usize, row: usize, columns: Vec<ErColumn>, view: bool) -> Entity {
        Entity {
            name: name.into(),
            columns,
            layer,
            row,
            view,
        }
    }

    /// `customers`, `orders` (many to exactly one) and `invoice` (one to zero or one), and the
    /// view `big_orders` on `orders`.
    fn model() -> ErModel {
        ErModel {
            entities: vec![
                entity(
                    "customers",
                    0,
                    0,
                    vec![
                        col("id", "integer", &["PK"], false),
                        col("name", "text", &[], true),
                    ],
                    false,
                ),
                entity(
                    "orders",
                    1,
                    0,
                    vec![
                        col("id", "integer", &["PK"], false),
                        col("customer_id", "integer", &["FK"], false),
                    ],
                    false,
                ),
                entity(
                    "invoice",
                    2,
                    0,
                    vec![col("order_id", "integer", &["PK", "FK"], true)],
                    false,
                ),
                entity("big_orders", 2, 1, vec![col("id", "", &[], true)], true),
            ],
            relationships: vec![
                Relationship {
                    from: "orders".into(),
                    from_columns: vec!["customer_id".into()],
                    to: "customers".into(),
                    to_columns: vec!["id".into()],
                    many: true,
                    optional: false,
                },
                Relationship {
                    from: "invoice".into(),
                    from_columns: vec!["order_id".into()],
                    to: "orders".into(),
                    to_columns: vec!["id".into()],
                    many: false,
                    optional: true,
                },
            ],
            outside: vec![],
            dependencies: vec![Dependency {
                view: "big_orders".into(),
                uses: "orders".into(),
            }],
        }
    }

    fn f(node: roxmltree::Node, name: &str) -> f32 {
        node.attribute(name)
            .unwrap_or_else(|| panic!("{name} on {node:?}"))
            .parse()
            .unwrap()
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.06
    }

    const Y: &str = "http://www.yworks.com/xml/graphml";
    const G: &str = "http://graphml.graphdrawing.org/xmlns";

    /// SPEC-0022/AK-12 (v4).
    #[test]
    fn graphml_holds_boxes_columns_and_crows_feet() {
        let l = layout_with_positions(&model(), &[("customers".to_owned(), (30., 400.))].into());
        let text = to_graphml(&l);
        let doc = roxmltree::Document::parse(&text).expect("well-formed");
        let root = doc.root_element();
        assert!(root.has_tag_name((G, "graphml")));
        assert!(!text.contains("schemaLocation"), "nothing to fetch");
        let nodes: Vec<_> = root
            .descendants()
            .filter(|n| n.has_tag_name((G, "node")))
            .collect();
        assert_eq!(nodes.len(), l.boxes.len());
        for (i, (node, b)) in nodes.iter().zip(&l.boxes).enumerate() {
            assert_eq!(node.attribute("id"), Some(format!("n{i}").as_str()));
            let geometry = node
                .descendants()
                .find(|n| n.has_tag_name((Y, "Geometry")))
                .unwrap();
            assert!(near(f(geometry, "x"), b.x) && near(f(geometry, "y"), b.y));
            assert!(near(f(geometry, "width"), b.width));
            assert!(near(f(geometry, "height"), b.height));
            let generic = node
                .descendants()
                .find(|n| n.has_tag_name((Y, "GenericNode")))
                .unwrap();
            assert_eq!(
                generic.attribute("configuration"),
                Some("com.yworks.entityRelationship.big_entity")
            );
            let labels: Vec<_> = node
                .descendants()
                .filter(|n| n.has_tag_name((Y, "NodeLabel")))
                .collect();
            assert_eq!(labels.len(), 2);
            let title = if b.view {
                format!("{} (view)", b.name)
            } else {
                b.name.clone()
            };
            assert_eq!(labels[0].text(), Some(title.as_str()));
            let columns: Vec<String> = b.lines.iter().map(column_line).collect();
            assert_eq!(labels[1].text(), Some(columns.join("\n").as_str()));
            let border = node
                .descendants()
                .find(|n| n.has_tag_name((Y, "BorderStyle")))
                .unwrap();
            assert_eq!(
                border.attribute("type"),
                Some(if b.view { "dashed" } else { "line" })
            );
        }
        let orders = &nodes[1];
        let attributes = orders
            .descendants()
            .filter(|n| n.has_tag_name((Y, "NodeLabel")))
            .nth(1)
            .unwrap();
        assert_eq!(
            attributes.text(),
            Some("PK id : integer\nFK customer_id : integer")
        );
        let edges: Vec<_> = root
            .descendants()
            .filter(|n| n.has_tag_name((G, "edge")))
            .collect();
        assert_eq!(edges.len(), l.lines.len());
        let expect = [
            (
                "n0",
                "n1",
                "crows_foot_one_mandatory",
                "crows_foot_many_optional",
                "line",
            ),
            (
                "n1",
                "n2",
                "crows_foot_one_optional",
                "crows_foot_one_optional",
                "line",
            ),
            ("n1", "n3", "none", "none", "dashed"),
        ];
        for ((edge, line), (source, target, a, b, style)) in edges.iter().zip(&l.lines).zip(expect)
        {
            assert_eq!(edge.attribute("source"), Some(source));
            assert_eq!(edge.attribute("target"), Some(target));
            let arrows = edge
                .descendants()
                .find(|n| n.has_tag_name((Y, "Arrows")))
                .unwrap();
            assert_eq!(
                (arrows.attribute("source"), arrows.attribute("target")),
                (Some(a), Some(b))
            );
            let ls = edge
                .descendants()
                .find(|n| n.has_tag_name((Y, "LineStyle")))
                .unwrap();
            assert_eq!(ls.attribute("type"), Some(style));
            let path = edge
                .descendants()
                .find(|n| n.has_tag_name((Y, "Path")))
                .unwrap();
            let bends: Vec<(f32, f32)> = path
                .children()
                .filter(|n| n.has_tag_name((Y, "Point")))
                .map(|p| (f(p, "x"), f(p, "y")))
                .collect();
            let inner = &line.points[1..line.points.len() - 1];
            assert_eq!(bends.len(), inner.len());
            for (b, p) in bends.iter().zip(inner) {
                assert!(near(b.0, p.0) && near(b.1, p.1), "{b:?} {p:?}");
            }
            // The ports put the line's end points on its boxes.
            let s = &l.boxes[source[1..].parse::<usize>().unwrap()];
            let t = &l.boxes[target[1..].parse::<usize>().unwrap()];
            let first = line.points[0];
            let last = *line.points.last().unwrap();
            assert!(near(s.x + s.width / 2. + f(path, "sx"), first.0));
            assert!(near(s.y + s.height / 2. + f(path, "sy"), first.1));
            assert!(near(t.x + t.width / 2. + f(path, "tx"), last.0));
            assert!(near(t.y + t.height / 2. + f(path, "ty"), last.1));
            let label = edge
                .descendants()
                .find(|n| n.has_tag_name((Y, "EdgeLabel")))
                .and_then(|n| n.text());
            assert_eq!(
                label,
                (!line.label.is_empty()).then_some(line.label.as_str())
            );
        }
        assert!(text.contains(">customer_id:id</y:EdgeLabel>"));
    }

    /// SPEC-0022/AK-13 (v4).
    #[test]
    fn drawio_holds_boxes_columns_and_er_markers() {
        let l = layout(&model());
        let text = to_drawio(&l);
        let doc = roxmltree::Document::parse(&text).expect("well-formed");
        let root = doc.root_element();
        assert!(root.has_tag_name("mxfile"));
        let model_node = root
            .children()
            .find(|n| n.has_tag_name("diagram"))
            .and_then(|d| d.children().find(|n| n.has_tag_name("mxGraphModel")))
            .expect("diagram → mxGraphModel");
        let cells: Vec<_> = model_node
            .children()
            .find(|n| n.has_tag_name("root"))
            .unwrap()
            .children()
            .filter(|n| n.has_tag_name("mxCell"))
            .collect();
        assert_eq!(cells[0].attribute("id"), Some("0"));
        assert_eq!(
            (cells[1].attribute("id"), cells[1].attribute("parent")),
            (Some("1"), Some("0"))
        );
        for cell in &cells {
            let style = cell.attribute("style").unwrap_or_default();
            assert!(!style.contains("html=1"), "{style}");
        }
        let cell = |id: &str| {
            cells
                .iter()
                .find(|c| c.attribute("id") == Some(id))
                .unwrap_or_else(|| panic!("cell {id}"))
        };
        fn geometry<'a, 'b>(c: &roxmltree::Node<'a, 'b>) -> roxmltree::Node<'a, 'b> {
            c.children().find(|n| n.has_tag_name("mxGeometry")).unwrap()
        }
        for (i, b) in l.boxes.iter().enumerate() {
            let t = cell(&format!("t{i}"));
            let style = t.attribute("style").unwrap();
            assert!(style.starts_with("swimlane;"));
            assert_eq!(style.contains("dashed=1"), b.view);
            let title = if b.view {
                format!("{} (view)", b.name)
            } else {
                b.name.clone()
            };
            assert_eq!(t.attribute("value"), Some(title.as_str()));
            let g = geometry(t);
            assert!(near(f(g, "x"), b.x) && near(f(g, "y"), b.y));
            assert!(near(f(g, "width"), b.width) && near(f(g, "height"), b.height));
            let rows: Vec<_> = cells
                .iter()
                .filter(|c| c.attribute("parent") == Some(format!("t{i}").as_str()))
                .collect();
            assert_eq!(rows.len(), b.lines.len());
            for (k, (row, line)) in rows.iter().zip(&b.lines).enumerate() {
                assert_eq!(row.attribute("value"), Some(column_line(line).as_str()));
                assert!(near(
                    f(geometry(row), "y"),
                    HEADER_HEIGHT + LINE_HEIGHT * k as f32
                ));
            }
        }
        assert_eq!(
            cell("t1c1").attribute("value"),
            Some("FK customer_id : integer")
        );
        let expect = [
            ("t0", "t1", "ERmandOne", "ERzeroToMany", false),
            ("t1", "t2", "ERzeroToOne", "ERzeroToOne", false),
            ("t1", "t3", "none", "none", true),
        ];
        for (i, (line, (source, target, start, end, dashed))) in
            l.lines.iter().zip(expect).enumerate()
        {
            let r = cell(&format!("r{i}"));
            assert_eq!(r.attribute("edge"), Some("1"));
            assert_eq!(r.attribute("source"), Some(source));
            assert_eq!(r.attribute("target"), Some(target));
            assert_eq!(r.attribute("value"), Some(line.label.as_str()));
            let style: std::collections::HashMap<&str, &str> = r
                .attribute("style")
                .unwrap()
                .split(';')
                .filter_map(|kv| kv.split_once('='))
                .collect();
            assert_eq!(style["startArrow"], start);
            assert_eq!(style["endArrow"], end);
            assert_eq!(style.contains_key("dashed"), dashed);
            // Exit and entry points reproduce the line's end points on its boxes.
            let (s, t) = (
                &l.boxes[source[1..].parse::<usize>().unwrap()],
                &l.boxes[target[1..].parse::<usize>().unwrap()],
            );
            let v = |k: &str| style[k].parse::<f32>().unwrap();
            let (first, last) = (line.points[0], *line.points.last().unwrap());
            assert!(near(s.x + v("exitX") * s.width, first.0));
            assert!(near(s.y + v("exitY") * s.height, first.1));
            assert!(near(t.x + v("entryX") * t.width, last.0));
            assert!(near(t.y + v("entryY") * t.height, last.1));
            let points: Vec<(f32, f32)> = geometry(r)
                .descendants()
                .filter(|n| n.has_tag_name("mxPoint"))
                .map(|p| (f(p, "x"), f(p, "y")))
                .collect();
            let inner = &line.points[1..line.points.len() - 1];
            assert_eq!(points.len(), inner.len());
            for (a, b) in points.iter().zip(inner) {
                assert!(near(a.0, b.0) && near(a.1, b.1));
            }
        }
    }

    /// SPEC-0022/AK-14 (v4): names with markup stay text in SVG, GraphML and draw.io.
    #[test]
    fn names_cannot_add_markup() {
        let table = "</y:NodeLabel><y:Evil/>";
        let other = "\"/><mxCell id=\"x";
        let view = "<html><img src=\"http://example.invalid/x.png\">";
        let column = "a&b'c\u{1}\nline";
        let mut m = model();
        m.entities[0].name = table.into();
        m.entities[0].columns[1].name = column.into();
        m.entities[0].columns[1].data_type = "<b>type</b>".into();
        m.entities[1].name = other.into();
        m.entities[3].name = view.into();
        m.relationships[0].to = table.into();
        m.relationships[0].from = other.into();
        m.relationships[0].from_columns = vec![column.into()];
        m.relationships[1].to = other.into();
        m.dependencies[0].view = view.into();
        m.dependencies[0].uses = other.into();
        let l = layout(&m);
        let shown = "a&b'c\u{FFFD}\nline";

        let graphml = to_graphml(&l);
        let doc = roxmltree::Document::parse(&graphml).expect("GraphML stays well-formed");
        let count = |doc: &roxmltree::Document, ns: Option<&str>, tag: &str| {
            doc.descendants()
                .filter(|n| match ns {
                    Some(ns) => n.has_tag_name((ns, tag)),
                    None => n.has_tag_name(tag),
                })
                .count()
        };
        assert_eq!(count(&doc, Some(G), "node"), 4);
        assert_eq!(count(&doc, Some(G), "edge"), 3);
        assert_eq!(count(&doc, Some(Y), "NodeLabel"), 8);
        assert_eq!(count(&doc, Some(Y), "Evil"), 0, "no element from a name");
        assert!(
            doc.descendants()
                .filter(|n| n.has_tag_name((Y, "NodeLabel")) || n.has_tag_name((Y, "EdgeLabel")))
                .filter_map(|n| n.text())
                .all(|t| !t.trim_start().starts_with('<')),
            "no label yEd would read as HTML"
        );
        let names: Vec<&str> = doc
            .descendants()
            .filter(|n| n.attribute("key") == Some("d2"))
            .filter_map(|n| n.text())
            .collect();
        assert_eq!(names, [table, other, "invoice", view]);
        let first_label = doc
            .descendants()
            .find(|n| n.has_tag_name((Y, "NodeLabel")))
            .and_then(|n| n.text())
            .unwrap();
        assert_eq!(first_label, format!("\u{200B}{table}"));
        let columns = doc
            .descendants()
            .filter(|n| n.has_tag_name((Y, "NodeLabel")))
            .nth(1)
            .and_then(|n| n.text())
            .unwrap();
        assert_eq!(columns, format!("PK id : integer\n{shown} : <b>type</b>"));
        let edge_columns = doc
            .descendants()
            .find(|n| n.attribute("key") == Some("d3"))
            .and_then(|n| n.text())
            .unwrap();
        assert_eq!(edge_columns, format!("{shown}:id"));

        let drawio = to_drawio(&l);
        let doc = roxmltree::Document::parse(&drawio).expect("draw.io stays well-formed");
        assert_eq!(count(&doc, None, "mxCell"), 2 + 4 + 6 + 3);
        let ids: Vec<&str> = doc
            .descendants()
            .filter(|n| n.has_tag_name("mxCell"))
            .filter_map(|n| n.attribute("id"))
            .collect();
        assert!(!ids.contains(&"x"), "no cell from a name: {ids:?}");
        let values: Vec<&str> = doc
            .descendants()
            .filter_map(|n| n.attribute("value"))
            .collect();
        assert!(values.contains(&table) && values.contains(&other));
        assert!(values.contains(&format!("{view} (view)").as_str()));
        assert!(values.contains(&format!("{shown} : <b>type</b>").as_str()));
        for n in doc.descendants() {
            if let Some(style) = n.attribute("style") {
                assert!(!style.contains("html=1"));
                for name in [table, other, view, "a&b"] {
                    assert!(!style.contains(name), "{name} in a style");
                }
            }
        }
        assert!(
            !drawio.contains("example.invalid\">"),
            "the image tag is text"
        );

        let svg = to_svg(&l);
        let doc = roxmltree::Document::parse(&svg).expect("SVG stays well-formed");
        assert_eq!(count(&doc, Some("http://www.w3.org/2000/svg"), "g"), 4);
        assert_eq!(count(&doc, Some("http://www.w3.org/2000/svg"), "img"), 0);
        assert!(doc.descendants().any(|n| n.text() == Some(table)));
        // The PNG renders from that SVG; a control character in a name no longer breaks it.
        assert!(crate::er_layout::to_png(&l, 1.).is_ok());
    }

    #[test]
    fn xml_text_masks_markup_and_forbidden_characters() {
        assert_eq!(
            xml_text("<a href=\"x\">&'\t\r\n\u{0}\u{1F}\u{FFFE}é"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&apos;&#9;&#13;&#10;\u{FFFD}\u{FFFD}\u{FFFD}é"
        );
        assert_eq!(label_text("<html>x"), "\u{200B}&lt;html&gt;x");
        assert_eq!(label_text("  <html>"), "\u{200B}  &lt;html&gt;");
        assert_eq!(label_text("a<b"), "a&lt;b");
    }
}

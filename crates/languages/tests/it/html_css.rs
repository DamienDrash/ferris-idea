//! HTML and stylesheets from IDECraft's own syntax trees through the entry points the editor and
//! the MCP server use (`SPEC-0042` v17 behaviours 28–33, OBS-0039 W1 to W3; v22 behaviours 38–40,
//! OBS-0069 G1 to G3), on the sample of the observation: `index.html` linking `css/site.css`.

use std::path::PathBuf;
use std::time::Duration;

use idecraft_languages::find_usages::find_usages_in;
use idecraft_languages::insight::{LocationKind, complete_in, found_in, symbols_in};
use idecraft_languages::rename::{apply_edits, plan_rename_in};

const INDEX: &str = include_str!("../fixtures/web/index.html");
const SITE: &str = include_str!("../fixtures/web/css/site.css");

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        matches!(
            idecraft_languages::servers::find_in(&paths, program),
            idecraft_languages::servers::Availability::Ready(_)
        )
    })
}

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("css")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("index.html"), INDEX).unwrap();
    std::fs::write(root.join("css/site.css"), SITE).unwrap();
    root
}

/// SPEC-0042/AK-33, SPEC-0071/AK-7: the three steps of OBS-0039 answer on the editor's unsaved
/// text and with no time for a language server.
#[tokio::test]
async fn html_answers_without_a_server() {
    let root = project("own");
    let file = root.join("index.html");
    // The editor's text has one more line at the top than the saved file.
    let unsaved = format!("<!-- draft -->\n{INDEX}");
    let none = Duration::ZERO;

    // Step 1: the caret in `card` of `class="card"` (line 13 of the saved file).
    let found = found_in(
        &root,
        &file,
        Some(&unsaved),
        13,
        28,
        LocationKind::Definition,
        none,
    )
    .await
    .unwrap();
    let places: Vec<(PathBuf, u64, u64)> = found
        .locations
        .iter()
        .map(|l| {
            (
                l.path.strip_prefix(&root).unwrap().to_path_buf(),
                l.range.start_line,
                l.range.start_character,
            )
        })
        .collect();
    // `.card` on line 20 of `css/site.css`, then `.card:hover`.
    assert_eq!(
        places,
        [
            (PathBuf::from("css/site.css"), 19, 1),
            (PathBuf::from("css/site.css"), 22, 1)
        ]
    );
    assert!(found.outside.is_empty());

    // Step 2: the caret in `#prices` of the jump link (line 11): `id="prices"` on line 13.
    let found = found_in(
        &root,
        &file,
        Some(&unsaved),
        11,
        15,
        LocationKind::Definition,
        none,
    )
    .await
    .unwrap();
    assert_eq!(found.locations.len(), 1);
    let target = &found.locations[0];
    assert_eq!(target.path, file);
    assert_eq!(
        (target.range.start_line, target.range.start_character),
        (13, 12)
    );

    // The saved file, without text.
    let saved = found_in(&root, &file, None, 12, 28, LocationKind::Definition, none)
        .await
        .unwrap();
    assert_eq!(saved.locations.len(), 2);

    // Step 3: the structure, nested, with id and classes.
    let symbols = symbols_in(&root, &file, Some(&unsaved), none)
        .await
        .unwrap();
    let rows: Vec<String> = symbols
        .iter()
        .map(|s| {
            format!(
                "{}{}{}",
                "  ".repeat(s.depth),
                s.name,
                s.detail
                    .as_ref()
                    .map(|d| format!(" [{d}]"))
                    .unwrap_or_default()
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            "html",
            "  head",
            "    meta",
            "    title",
            "    link [css/site.css]",
            "  body",
            "    header#masthead.banner",
            "      img [logo.png]",
            "      a [#prices]",
            "    main#prices.card",
            "      p",
            "      button.button.primary",
            "    script [app.js]",
        ]
    );
    assert!(symbols.iter().all(|s| s.kind == "element"));
    assert_eq!(
        symbols_in(&root, &file, None, none).await.unwrap().len(),
        13
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0042/AK-32, SPEC-0293/AK-10: Find Usages on `.card` of the stylesheet keeps what the CSS
/// server names and adds the use in `index.html`, marked as from IDECraft's own index.
#[tokio::test]
async fn css_selector_usages_include_html() {
    if !installed("vscode-css-language-server") {
        skip("vscode-langservers-extracted not installed");
        return;
    }
    let root = project("usages");
    let css = root.join("css/site.css");
    let found = find_usages_in(
        &root,
        &css,
        Some(SITE),
        19,
        2,
        "card",
        Duration::from_secs(60),
        None,
    )
    .await
    .unwrap();
    let rows: Vec<(PathBuf, u64, u64, bool)> = found
        .usages
        .iter()
        .zip(&found.details)
        .map(|(u, d)| {
            (
                u.path.strip_prefix(&root).unwrap().to_path_buf(),
                u.range.start_line,
                u.range.start_character,
                d.own_index,
            )
        })
        .collect();
    assert!(
        rows.contains(&(PathBuf::from("index.html"), 12, 27, true)),
        "{rows:?}"
    );
    // What the server names in the stylesheet stays, unmarked.
    assert!(
        rows.iter()
            .filter(|r| r.0.ends_with("css/site.css"))
            .all(|r| !r.3),
        "{rows:?}"
    );
    assert_eq!(rows.iter().filter(|r| r.3).count(), 1, "{rows:?}");
    assert!(
        found
            .note
            .as_deref()
            .is_some_and(|n| n.contains("1 usage in HTML files")),
        "{:?}",
        found.note
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0042/AK-39, AK-42, AK-44 (v22, behaviours 38–40; OBS-0069 G1, G2): rename and completion
/// through the entry points of the editor and the MCP server answer with no time for a language
/// server: `.card` becomes `.box` in the stylesheet and `class="box"` in the page, from either
/// side; `1box` is refused; an empty `class` value offers the stylesheet's classes.
#[tokio::test]
async fn rename_and_completion_without_a_server() {
    let root = project("own-rename");
    let css = root.join("css/site.css");
    let index = root.join("index.html");
    let none = Duration::ZERO;
    for (file, text, line, character) in [(&css, SITE, 19, 2), (&index, INDEX, 12, 28)] {
        let plan = plan_rename_in(&root, file, Some(text), line, character, "box", none)
            .await
            .unwrap();
        assert_eq!(plan.old_name, "card");
        let files: Vec<PathBuf> = plan
            .files
            .iter()
            .map(|f| f.path.strip_prefix(&root).unwrap().to_path_buf())
            .collect();
        assert_eq!(
            files,
            [PathBuf::from("css/site.css"), PathBuf::from("index.html")]
        );
        let site = apply_edits(SITE, &plan.files[0].edits).unwrap();
        assert!(
            site.contains("\n.box {\n") && site.contains("\n.box:hover {\n"),
            "{site}"
        );
        let page = apply_edits(INDEX, &plan.files[1].edits).unwrap();
        assert!(
            page.contains("<main id=\"prices\" class=\"box\">"),
            "{page}"
        );
    }
    let refused = plan_rename_in(&root, &css, Some(SITE), 19, 2, "1box", none)
        .await
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("is not a valid CSS class name"),
        "{refused}"
    );

    let typed = INDEX.replace("class=\"card\"", "class=\"\"");
    let items = complete_in(&root, &index, Some(&typed), 12, 27, 50, none)
        .await
        .unwrap();
    let offered: Vec<(&str, &str)> = items
        .iter()
        .map(|i| {
            (
                i.label.as_str(),
                i.description.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        offered,
        [
            ("banner", "css/site.css:5"),
            ("button", "css/site.css:13"),
            ("primary", "css/site.css:13"),
            ("card", "css/site.css:20"),
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

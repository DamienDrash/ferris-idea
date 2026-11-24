//! F-081 · Functions of open platform plugins: logic of Apache-2.0 plugins ported to Rust
//! (ADR-0006, `docs/legal/APACHE_PORTING.md`).
//!
//! The ported features are behaviours of other rows (keymaps F-018, themes F-085, inspections
//! F-032); this row checks the ported features an agent can name: properties files, resource
//! bundles and the project dictionary (SPEC-0155, SPEC-0163).

use crate::harness::*;
use serde_json::json;

#[test]
fn properties_files_and_resource_bundles() {
    let p = Project::empty();
    p.write(
        "i18n/Messages.properties",
        "greeting = Hello\\\n  World\nkey\\ with\\ space=1\n",
    )
    .write("i18n/Messages_de.properties", "greeting = Hallo Welt\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "properties_file",
        json!({"path": "i18n/Messages.properties"}),
    );
    let all = text(&r);
    // Continuation lines are joined without their leading blanks, as java.util.Properties does.
    assert!(all.contains("\"HelloWorld\""), "{r}");
    assert!(all.contains("key with space"), "escapes resolved: {r}");
    let bundle = ide.ok(
        "resource_bundle",
        json!({"path": "i18n/Messages.properties"}),
    );
    assert!(text(&bundle).contains("de"), "{bundle}");
}

#[test]
fn project_dictionary_lives_where_the_reference_keeps_it() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    ide.ok(
        "project_dictionary",
        json!({"add": ["ferris-idea"], "dry_run": false}),
    );
    assert!(
        p.read(".idea/dictionaries/project.xml")
            .contains("ferris-idea")
    );
}

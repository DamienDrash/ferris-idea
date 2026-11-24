//! Show DDL of the Database tool window (`SPEC-0089` v5 behaviour 11): the selected table's or
//! view's definition, generated from Ferris IDEA's own introspection, in a read-only editor tab
//! `<name> (DDL).sql`; since v13 also the definition of a routine, trigger or type as the database
//! states it (behaviour 18).

use std::path::PathBuf;

use gpui::{Context, Window};

use super::WorkbenchView;

/// Where the DDL of `table` of connection `id` is written: a file in Ferris IDEA's cache, named
/// `<table> (DDL).sql`, so the tab shows the name and the editor highlights SQL.
pub fn ddl_path(id: &str, table: &str) -> Option<PathBuf> {
    let clean = |s: &str| {
        s.chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '\0') {
                    '_'
                } else {
                    c
                }
            })
            .collect::<String>()
    };
    Some(
        ferris_idea_core::user_cache_dir()?
            .join("ddl")
            .join(clean(id))
            .join(format!("{} (DDL).sql", clean(table))),
    )
}

/// The name of the tab of an object's definition: a routine with its arguments, so overloads get
/// their own tabs (`SPEC-0089` v13 behaviour 25).
pub fn object_ddl_name(object: &ferris_idea_database::SchemaObject) -> String {
    match object.kind.as_str() {
        "function" | "procedure" | "aggregate" => {
            format!(
                "{}{}",
                object.name,
                object.detail.as_deref().unwrap_or_default()
            )
        }
        _ => object.name.clone(),
    }
}

impl WorkbenchView {
    /// Reads the definition of a routine, trigger or type of `profile`'s current schema in the
    /// background and opens it as a read-only tab (`SPEC-0089` v13 behaviour 25); failures, such as
    /// a missing privilege, go to the status line.
    pub fn open_object_ddl(
        &mut self,
        profile: ferris_idea_database::ConnectionProfile,
        object: ferris_idea_database::SchemaObject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = object_ddl_name(&object);
        self.status = Some(format!("Reading the definition of `{title}`…"));
        cx.notify();
        let name = title.clone();
        let task = cx.background_executor().spawn(async move {
            let mut p = profile;
            p.read_only = true;
            let (rt, session) = super::connect(&p)?;
            let ddl = rt.block_on(async {
                ferris_idea_database::object_definition(&session, None, &object)
                    .await
                    .map_err(|e| format!("{}: {e}", p.name))?
                    .ok_or_else(|| format!("{} has no {} `{name}`", p.name, object.kind))
            })?;
            let path = ddl_path(&p.id, &name).ok_or("no cache folder for the DDL")?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, format!("{ddl}\n")).map_err(|e| e.to_string())?;
            Ok::<_, String>(path)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(path) => {
                        this.status = Some(format!(
                            "DDL of `{title}`: as the database states it, read-only"
                        ));
                        this.open(&path, window, cx);
                        if let Some(editor) = this.editors.get(&path).cloned() {
                            editor.update(cx, |e, cx| {
                                e.reload_from_disk(cx);
                                e.set_read_only(true);
                            });
                        }
                    }
                    Err(e) => this.status = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the schema of `profile` in the background and opens the definition of `table` as a
    /// read-only tab; failures go to the status line.
    pub fn open_ddl(
        &mut self,
        profile: ferris_idea_database::ConnectionProfile,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = Some(format!("Reading the definition of `{table}`…"));
        cx.notify();
        let name = table.clone();
        let task = cx.background_executor().spawn(async move {
            let mut p = profile;
            p.read_only = true;
            let (rt, session) = super::connect(&p)?;
            let ddl = rt.block_on(async {
                ferris_idea_database::show_ddl(&session, &name)
                    .await
                    .map_err(|e| format!("{}: {e}", p.name))?
                    .map(|object| object.ddl)
                    .ok_or_else(|| format!("{} has no table or view `{name}`", p.name))
            })?;
            let path = ddl_path(&p.id, &name).ok_or("no cache folder for the DDL")?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, ddl).map_err(|e| e.to_string())?;
            Ok::<_, String>(path)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(path) => {
                        this.status = Some(format!(
                            "DDL of `{table}`: generated from the schema, read-only"
                        ));
                        this.open(&path, window, cx);
                        if let Some(editor) = this.editors.get(&path).cloned() {
                            editor.update(cx, |e, cx| {
                                // Shown again: the schema may have changed meanwhile.
                                e.reload_from_disk(cx);
                                e.set_read_only(true);
                            });
                        }
                    }
                    Err(e) => this.status = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

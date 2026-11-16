//! The application menu bar (`SPEC-0124`): the reference's main menus with the actions IDECraft
//! carries out, each showing its shortcut from the keymap in use.
//!
//! A menu item sends [`IdeAction`] with the action id; the window runs it the way Search
//! Everywhere does (editor first, then workbench). Shortcuts are registered with the toolkit only
//! for display, under a key context no element has, so a key press is still handled once, by the
//! IDE's own keymap.

use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType};
use idecraft_ui::keymap::{Keystroke, Modifier, Shortcut};
pub use idecraft_ui::menus::{Entry, MENUS, menu_actions};
use idecraft_ui::window_layouts::MenuItem as LayoutItem;

/// Runs a keymap action by id (sent by a menu item).
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = idecraft, no_json)]
pub struct IdeAction {
    /// Keymap action id, e.g. `SaveAll`.
    pub id: String,
}

/// Quits IDECraft.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = idecraft, no_json)]
pub struct QuitIde;

/// Key context of the display-only bindings; no element sets it.
const MENU_ONLY: &str = "IdeMenuOnly";

/// A keystroke in the toolkit's notation (`cmd-shift-s`), if it has one.
pub fn toolkit_keystroke(k: &Keystroke) -> Option<String> {
    let key = match k.key.as_str() {
        "UP" => "up".into(),
        "DOWN" => "down".into(),
        "LEFT" => "left".into(),
        "RIGHT" => "right".into(),
        "BACK_SPACE" => "backspace".into(),
        "DELETE" => "delete".into(),
        "ENTER" => "enter".into(),
        "ESCAPE" => "escape".into(),
        "TAB" => "tab".into(),
        "HOME" => "home".into(),
        "END" => "end".into(),
        "PAGE_UP" => "pageup".into(),
        "PAGE_DOWN" => "pagedown".into(),
        "INSERT" => "insert".into(),
        "SPACE" => "space".into(),
        "SLASH" => "/".into(),
        "BACK_SLASH" => "\\".into(),
        "COMMA" => ",".into(),
        "PERIOD" => ".".into(),
        "SEMICOLON" => ";".into(),
        "EQUALS" => "=".into(),
        "MINUS" => "-".into(),
        "OPEN_BRACKET" => "[".into(),
        "CLOSE_BRACKET" => "]".into(),
        "QUOTE" => "'".into(),
        "BACK_QUOTE" => "`".into(),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => k.to_lowercase(),
        k if k.starts_with('F') && k[1..].parse::<u8>().is_ok() => k.to_lowercase(),
        _ => return None,
    };
    let mut out = String::new();
    for (m, name) in [
        (Modifier::Ctrl, "ctrl-"),
        (Modifier::Alt, "alt-"),
        (Modifier::Shift, "shift-"),
        (
            Modifier::Meta,
            if cfg!(target_os = "macos") {
                "cmd-"
            } else {
                "super-"
            },
        ),
    ] {
        if k.modifiers.contains(&m) {
            out.push_str(name);
        }
    }
    if k.modifiers.contains(&Modifier::AltGraph) {
        return None;
    }
    out.push_str(&key);
    Some(out)
}

/// The status text when action `id` cannot run here: its title and its first shortcut in `keymap`,
/// so the status says which action a key reached ("Resume Program (⌥⌘R) is not available here";
/// hand test 8 of round 4 showed only the id).
pub fn unavailable(id: &str, keymap: &idecraft_ui::keymap::Keymap) -> String {
    let (title, _) = idecraft_languages::everywhere::action_title(id);
    match keymap.shortcuts(id).first() {
        Some(s) => format!(
            "{title} ({}) is not available here",
            s.display(cfg!(target_os = "macos"))
        ),
        None => format!("{title} is not available here"),
    }
}

/// The first single-keystroke shortcut of `id` in the keymap in use, in toolkit notation.
pub fn menu_shortcut(id: &str) -> Option<String> {
    // Without the chords the action yields to another one (SPEC-0071 v10 behaviour 16).
    crate::keymaps::current()
        .shown_shortcuts(id)
        .into_iter()
        .find_map(|s| match s {
            Shortcut::Keyboard {
                first,
                second: None,
            } => toolkit_keystroke(first),
            _ => None,
        })
}

/// The entries of Window › Layouts as they are now: Default, the saved layouts, the actions on
/// the current one (`SPEC-0333` behaviour 12). A window sets it after it changed the layouts;
/// without it the menu bar reads the user's file.
pub struct LayoutMenu(pub Vec<LayoutItem>);

impl gpui::Global for LayoutMenu {}

/// The entries of the Layouts submenu as the toolkit can show them: it can neither tick nor
/// grey out an item, so a ticked item gets a ✓ before its title and an unavailable one is left
/// out, with the line that would stand alone.
pub fn layout_entries(entries: &[LayoutItem]) -> Vec<LayoutItem> {
    let mut out: Vec<LayoutItem> = Vec::new();
    for e in entries {
        match e {
            LayoutItem::Action { enabled: false, .. } => {}
            LayoutItem::Action {
                label,
                id,
                checked,
                enabled,
            } => out.push(LayoutItem::Action {
                label: if *checked {
                    format!("✓ {label}")
                } else {
                    label.clone()
                },
                id: id.clone(),
                checked: *checked,
                enabled: *enabled,
            }),
            LayoutItem::Separator => {
                if !matches!(out.last(), None | Some(LayoutItem::Separator)) {
                    out.push(LayoutItem::Separator);
                }
            }
            LayoutItem::Sub { label, items } => out.push(LayoutItem::Sub {
                label: label.clone(),
                items: layout_entries(items),
            }),
        }
    }
    if out.last() == Some(&LayoutItem::Separator) {
        out.pop();
    }
    out
}

/// The titles of Edit › Undo and Edit › Redo while they mean a step of the project tree's file
/// operations (`SPEC-0275` v16 behaviour 42): the step's name, or `None` for the plain title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoTitles {
    /// The step `$Undo` would take back.
    pub undo: Option<String>,
    /// The step `$Redo` would make again.
    pub redo: Option<String>,
}

impl gpui::Global for UndoTitles {}

/// The title of the menu item of action `id`: Undo and Redo name the step they would take, every
/// other item keeps `label`.
pub fn edit_title(id: &str, label: &str, titles: &UndoTitles) -> String {
    let step = match id {
        "$Undo" => titles.undo.as_deref(),
        "$Redo" => titles.redo.as_deref(),
        _ => None,
    };
    match step {
        Some(step) => format!("{label} {step}"),
        None => label.to_owned(),
    }
}

/// Shows `titles` in the Edit menu; the menu bar is built again only when they changed.
pub fn set_undo_titles(titles: UndoTitles, cx: &mut App) {
    if cx.try_global::<UndoTitles>().cloned().unwrap_or_default() == titles {
        return;
    }
    cx.set_global(titles);
    set_menus(cx);
}

fn layout_items(entries: &[LayoutItem]) -> Vec<MenuItem> {
    layout_entries(entries)
        .iter()
        .map(|e| match e {
            LayoutItem::Action { label, id, .. } => {
                MenuItem::action(label.clone(), IdeAction { id: id.clone() })
            }
            LayoutItem::Separator => MenuItem::separator(),
            LayoutItem::Sub { label, items } => MenuItem::submenu(Menu {
                name: label.clone().into(),
                items: layout_items(items),
            }),
        })
        .collect()
}

fn items(entries: &[Entry], layouts: &[LayoutItem], titles: &UndoTitles) -> Vec<MenuItem> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Action(label, id) => MenuItem::action(
                edit_title(id, label, titles),
                IdeAction { id: id.to_string() },
            ),
            Entry::Separator => MenuItem::separator(),
            // The saved layouts are not in the static menus.
            Entry::Sub(name, _) if *name == idecraft_ui::menus::LAYOUTS => {
                MenuItem::submenu(Menu {
                    name: (*name).into(),
                    items: layout_items(layouts),
                })
            }
            Entry::Sub(name, sub) => MenuItem::submenu(Menu {
                name: (*name).into(),
                items: items(sub, layouts, titles),
            }),
        })
        .collect()
}

/// The application menu: About, Settings, Services, Quit.
fn app_menu() -> Menu {
    Menu {
        name: "IDECraft".into(),
        items: vec![
            // D62 (hand test 205): name and version, as every macOS application shows them.
            MenuItem::action("About IDECraft", IdeAction { id: "About".into() }),
            MenuItem::separator(),
            MenuItem::action(
                "Settings…",
                IdeAction {
                    id: "ShowSettings".into(),
                },
            ),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit IDECraft", QuitIde),
        ],
    }
}

/// Installs the menu bar and its shortcut labels; call again after the keymap changed.
pub fn install(cx: &mut App) {
    cx.clear_key_bindings();
    let mut bindings = Vec::new();
    for id in std::iter::once("ShowSettings").chain(menu_actions()) {
        if let Some(keys) = menu_shortcut(id) {
            bindings.push(KeyBinding::new(
                &keys,
                IdeAction { id: id.to_owned() },
                Some(MENU_ONLY),
            ));
        }
    }
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-q", QuitIde, Some(MENU_ONLY)));
    }
    cx.bind_keys(bindings);
    // Asks first when files are unsaved (SPEC-0261).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    cx.on_action(|_: &QuitIde, cx| crate::WorkbenchView::quit(cx));
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    cx.on_action(|_: &QuitIde, cx| cx.quit());
    set_menus(cx);
}

/// Builds the menus from the static entries, the saved layouts and the titles of Undo and Redo.
fn set_menus(cx: &mut App) {
    let titles = cx.try_global::<UndoTitles>().cloned().unwrap_or_default();
    let layouts = match cx.try_global::<LayoutMenu>() {
        Some(menu) => menu.0.clone(),
        None => idecraft_ui::window_layouts::layouts_file()
            .map(|f| idecraft_ui::window_layouts::Layouts::load(&f))
            .unwrap_or_default()
            .menu(),
    };
    let mut menus = vec![app_menu()];
    menus.extend(MENUS.iter().map(|(name, entries)| Menu {
        name: (*name).into(),
        items: items(entries, &layouts, &titles),
    }));
    cx.set_menus(menus);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_in_toolkit_notation() {
        let k = |t: &str| Keystroke::parse(t).unwrap();
        assert_eq!(
            toolkit_keystroke(&k("meta shift S")).as_deref(),
            Some(if cfg!(target_os = "macos") {
                "shift-cmd-s"
            } else {
                "shift-super-s"
            })
        );
        assert_eq!(
            toolkit_keystroke(&k("control alt O")).as_deref(),
            Some("ctrl-alt-o")
        );
        assert_eq!(
            toolkit_keystroke(&k("shift F6")).as_deref(),
            Some("shift-f6")
        );
        assert_eq!(
            toolkit_keystroke(&k("meta OPEN_BRACKET"))
                .as_deref()
                .map(|s| s.ends_with("-[")),
            Some(true)
        );
        assert_eq!(
            toolkit_keystroke(&k("alt ENTER")).as_deref(),
            Some("alt-enter")
        );
    }

    /// SPEC-0275 v16 AK-36 (behaviour 42): Undo and Redo of the Edit menu name the step of the
    /// project tree they would take; every other item, and both without a step, keep their title.
    #[test]
    fn the_edit_menu_names_the_step() {
        fn titles_of(built: &[MenuItem]) -> Vec<String> {
            built
                .iter()
                .filter_map(|i| match i {
                    MenuItem::Action { name, .. } => Some(name.to_string()),
                    _ => None,
                })
                .collect()
        }
        let edit = MENUS.iter().find(|(n, _)| *n == "Edit").unwrap().1;
        let plain = titles_of(&items(edit, &[], &UndoTitles::default()));
        assert_eq!(&plain[..3], ["Undo", "Redo", "Cut"]);
        let named = titles_of(&items(
            edit,
            &[],
            &UndoTitles {
                undo: Some("New File a.txt".into()),
                redo: Some("Delete 3 items".into()),
            },
        ));
        assert_eq!(
            &named[..3],
            ["Undo New File a.txt", "Redo Delete 3 items", "Cut"]
        );
        assert_eq!(named[3..], plain[3..]);
        let one = UndoTitles {
            undo: None,
            redo: Some("Rename a.txt to b.txt".into()),
        };
        assert_eq!(edit_title("$Undo", "Undo", &one), "Undo");
        assert_eq!(
            edit_title("$Redo", "Redo", &one),
            "Redo Rename a.txt to b.txt"
        );
        assert_eq!(edit_title("$Cut", "Cut", &one), "Cut");
    }

    /// SPEC-0333 AK-5: Window › Layouts lists the saved layouts between Default and the actions
    /// on the current one; the selected layout has a ✓ and no Delete….
    #[test]
    fn the_layouts_submenu_lists_the_saved_layouts() {
        use idecraft_ui::tool_windows::Layout;
        use idecraft_ui::window_layouts::{Arrangement, Layouts};
        fn shape(items: &[MenuItem]) -> Vec<String> {
            items
                .iter()
                .map(|i| match i {
                    MenuItem::Action { name, action, .. } => {
                        let id = action
                            .as_any()
                            .downcast_ref::<IdeAction>()
                            .map(|a| a.id.clone())
                            .unwrap_or_default();
                        format!("{name} = {id}")
                    }
                    MenuItem::Submenu(m) => format!("> {}", m.name),
                    _ => "-".into(),
                })
                .collect()
        }
        fn layouts_of(built: &[MenuItem]) -> &Menu {
            built
                .iter()
                .find_map(|i| match i {
                    MenuItem::Submenu(m) if m.name == "Layouts" => Some(m),
                    _ => None,
                })
                .expect("Window has Layouts")
        }
        let mut layouts = Layouts::default();
        let plain = || Arrangement::new(&Layout::default(), &[], None);
        layouts.save_new("Review", plain()).unwrap();
        layouts.save_new("Coding", plain()).unwrap();
        let window = MENUS.iter().find(|(n, _)| *n == "Window").unwrap().1;
        let built = items(window, &layouts.menu(), &UndoTitles::default());
        let sub = layouts_of(&built);
        assert_eq!(
            shape(&sub.items),
            [
                "Default = RestoreFactoryDefaultLayout",
                "-",
                "> ✓ Coding",
                "> Review",
                "-",
                "Restore Current Layout = RestoreDefaultLayout",
                "Save Changes in Current Layout = StoreDefaultLayout",
                "Save Current Layout as New… = StoreNewLayout",
            ]
        );
        let of = |title: &str| {
            sub.items
                .iter()
                .find_map(|i| match i {
                    MenuItem::Submenu(m) if m.name == title => Some(shape(&m.items)),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(
            of("✓ Coding"),
            [
                "Restore = RestoreDefaultLayout",
                "Save Changes = StoreDefaultLayout",
                "Rename… = IDECraft.Layout.Rename:Coding",
            ]
        );
        assert_eq!(
            of("Review"),
            [
                "Apply = IDECraft.Layout.Apply:Review",
                "Rename… = IDECraft.Layout.Rename:Review",
                "-",
                "Delete… = IDECraft.Layout.Delete:Review",
            ]
        );
        // Without saved layouts: Default has the ✓, Save Changes is left out.
        let none = items(window, &Layouts::default().menu(), &UndoTitles::default());
        assert_eq!(
            shape(&layouts_of(&none).items),
            [
                "✓ Default = RestoreFactoryDefaultLayout",
                "-",
                "Restore Current Layout = RestoreDefaultLayout",
                "Save Current Layout as New… = StoreNewLayout",
            ]
        );
    }

    /// D62 (hand test 205): the application menu starts with About IDECraft.
    #[test]
    fn app_menu_has_about() {
        let menu = app_menu();
        let first = menu.items.first();
        assert!(matches!(first, Some(MenuItem::Action { name, .. }) if name == "About IDECraft"));
        let (title, detail) = crate::workbench_view::about_text();
        assert_eq!(title, "IDECraft");
        assert!(detail.contains(env!("CARGO_PKG_VERSION")), "{detail}");
        assert!(detail.contains(idecraft_core::release::STAGE), "{detail}");
    }

    #[test]
    fn every_menu_action_is_carried_out_somewhere() {
        // Ids handled by the workbench, the editor view or the editor commands.
        let sources = [
            include_str!("workbench_view.rs"),
            include_str!("workbench_bookmarks.rs"),
            include_str!("workbench_keymap_eighth.rs"),
            include_str!("workbench_keymap_ninth.rs"),
            include_str!("workbench_keymap_tenth.rs"),
            include_str!("workbench_tabs.rs"),
            include_str!("tabs.rs"),
            include_str!("editor_view.rs"),
            include_str!("../../editor/src/commands.rs"),
        ];
        let actions = menu_actions();
        assert!(actions.len() > 100, "{}", actions.len());
        // Exporting result data is reachable from the menu bar (SPEC-0124 v2, IMP-0013).
        assert!(actions.contains(&"Sql.ExportData"));
        // Git › Current File › Annotate with Git Blame (SPEC-0277 behaviour 1).
        assert!(actions.contains(&"Annotate"));
        for id in actions {
            let quoted = format!("\"{id}\"");
            assert!(
                sources.iter().any(|s| s.contains(&quoted)),
                "{id} is in a menu but nothing runs it"
            );
        }
    }

    /// Hand test 8 of round 4: in the macOS keymap ⌃⌥R is "Run…" (the run popup) and ⌥⌘R is
    /// "Resume Program"; neither key has a second action, and the Run menu carries both with
    /// these keys.
    #[test]
    fn run_and_resume_keys_on_macos() {
        use idecraft_ui::keymap::Keymap;
        let mac = Keymap::bundled_macos();
        let k = |t: &str| Keystroke::parse(t).unwrap();
        assert_eq!(
            mac.actions_for(&k("control alt R"), None),
            ["ChooseRunConfiguration"]
        );
        assert_eq!(mac.actions_for(&k("meta alt R"), None), ["Resume"]);
        assert_eq!(
            mac.shortcuts("ChooseRunConfiguration")
                .first()
                .and_then(|s| match s {
                    Shortcut::Keyboard { first, .. } => toolkit_keystroke(first),
                    _ => None,
                })
                .as_deref(),
            Some("ctrl-alt-r")
        );
        let run_menu = MENUS
            .iter()
            .find(|(name, _)| *name == "Run")
            .map(|(_, items)| *items)
            .unwrap();
        assert!(run_menu.contains(&Entry::Action("Run…", "ChooseRunConfiguration")));
        assert!(run_menu.contains(&Entry::Action("Resume Program", "Resume")));
        // The status names the action and its key, so a wrong key is seen at once.
        let text = unavailable("Resume", &mac);
        assert!(text.starts_with("Resume Program ("), "{text}");
        assert!(text.ends_with(") is not available here"), "{text}");
        assert_eq!(
            unavailable("NoSuchAction", &mac),
            "No Such Action is not available here"
        );
    }

    /// Hand test 9 of round 4: ⌃G, ⌃⇧G and ⌃⌘G each reach exactly one action in the macOS keymap,
    /// the multiple-caret one; Go to Line is ⌘L there and nothing else takes ⌃G.
    #[test]
    fn occurrence_keys_on_macos() {
        use idecraft_ui::keymap::Keymap;
        let mac = Keymap::bundled_macos();
        let k = |t: &str| Keystroke::parse(t).unwrap();
        assert_eq!(
            mac.actions_for(&k("control G"), None),
            ["SelectNextOccurrence"]
        );
        assert_eq!(
            mac.actions_for(&k("control shift G"), None),
            ["UnselectPreviousOccurrence"]
        );
        assert_eq!(
            mac.actions_for(&k("meta control G"), None),
            ["SelectAllOccurrences"]
        );
        assert_eq!(mac.actions_for(&k("meta L"), None), ["GotoLine"]);
        // The editor carries them out (it runs editor actions before the workbench's).
        for id in [
            "SelectNextOccurrence",
            "UnselectPreviousOccurrence",
            "SelectAllOccurrences",
        ] {
            assert!(idecraft_editor::COMMANDS.contains(&id), "{id}");
        }
    }

    #[test]
    fn every_menu_action_is_found_by_search_everywhere() {
        // Search Everywhere offers the keymap's actions and the unbound ones (IMP-0013), with the
        // keymap of either platform.
        use idecraft_ui::keymap::Keymap;
        let keymaps = [Keymap::bundled_default(), Keymap::bundled_macos()];
        let missing: Vec<&str> = menu_actions()
            .into_iter()
            .filter(|id| {
                !idecraft_languages::everywhere::UNBOUND_ACTIONS.contains(id)
                    && keymaps.iter().any(|k| k.shortcuts(id).is_empty())
            })
            .collect();
        assert!(missing.is_empty(), "not in Search Everywhere: {missing:?}");
    }
}

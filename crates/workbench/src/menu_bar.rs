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

/// Menu items that leave a chord to another item that has it too (`SPEC-0124` v18 behaviour 20):
/// ⌘D (Ctrl+D) shows at View › Compare With…, as in the reference, not at Edit › Duplicate Line;
/// in the editor the key still duplicates (`keymap_actions`).
pub const CHORD_SHOWN_ELSEWHERE: &[(&str, &str)] = &[("EditorDuplicate", "CompareTwoFiles")];

/// The first single-keystroke shortcut of `id` in the keymap in use, in toolkit notation.
pub fn menu_shortcut(id: &str) -> Option<String> {
    menu_shortcut_in(&crate::keymaps::current(), id)
}

/// [`menu_shortcut`] in `keymap`.
pub fn menu_shortcut_in(keymap: &idecraft_ui::keymap::Keymap, id: &str) -> Option<String> {
    let owner = CHORD_SHOWN_ELSEWHERE
        .iter()
        .find(|(item, _)| *item == id)
        .map(|(_, owner)| keymap.shortcuts(owner).to_vec())
        .unwrap_or_default();
    // Without the chords the action yields to another one (SPEC-0071 v10 behaviour 16).
    keymap
        .shown_shortcuts(id)
        .into_iter()
        .filter(|s| !owner.contains(s))
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

/// The menu label of the shortcut `keys` (toolkit notation) on the active keyboard layout: the
/// key a US keyboard has there is replaced by the key the layout puts in its place, as macOS does
/// for the shortcuts it localizes (⌘[ shows as ⌘Ö on German layouts; `SPEC-0124` v19
/// behaviour 3a). `equivalents` maps US keys to the layout's keys as the platform reports them;
/// `None` on a US layout. Pressing the shown key reaches the same action through the workbench
/// (SPEC-0071 v4 behaviour 6).
pub fn localized_keys<S: std::hash::BuildHasher>(
    keys: &str,
    equivalents: Option<&std::collections::HashMap<char, char, S>>,
) -> String {
    let Some(equivalents) = equivalents else {
        return keys.to_owned();
    };
    // The key is the part after the last `-` that is no key of its own (`cmd--` is ⌘-).
    let (prefix, key) = match keys.strip_suffix("--") {
        Some(rest) => (format!("{rest}-"), "-"),
        None => match keys.rsplit_once('-') {
            Some((mods, key)) => (format!("{mods}-"), key),
            None => (String::new(), keys),
        },
    };
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => match equivalents.get(&c) {
            Some(layout) => format!("{prefix}{layout}"),
            None => keys.to_owned(),
        },
        _ => keys.to_owned(),
    }
}

/// Whether the menu bar follows changes of the keyboard layout already.
struct FollowsLayout;

impl gpui::Global for FollowsLayout {}

/// Installs the menu bar and its shortcut labels; call again after the keymap changed. The labels
/// follow the active keyboard layout and are set again when it changes (behaviour 3a).
pub fn install(cx: &mut App) {
    cx.clear_key_bindings();
    let mut bindings = Vec::new();
    let mapper = cx.keyboard_mapper().clone();
    for id in std::iter::once("ShowSettings").chain(menu_actions()) {
        if let Some(keys) = menu_shortcut(id) {
            let keys = localized_keys(&keys, mapper.get_key_equivalents());
            bindings.push(KeyBinding::new(
                &keys,
                IdeAction { id: id.to_owned() },
                Some(MENU_ONLY),
            ));
        }
    }
    if !cx.has_global::<FollowsLayout>() {
        cx.set_global(FollowsLayout);
        cx.on_keyboard_layout_change(install).detach();
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
            include_str!("workbench_keymap_eleventh.rs"),
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

    /// SPEC-0124 AK-19 (v18): the items of OBS-0055 show the macOS keymap's shortcuts; ⌘D shows
    /// at View › Compare With… and not at Edit › Duplicate Line, while the keymap still binds it
    /// to Duplicate too (the editor duplicates on ⌘D).
    // The labels are what macOS shows (⌘ is `cmd`); elsewhere the toolkit writes the same key as
    // `super`, and the menu bar is the macOS one.
    #[cfg(target_os = "macos")]
    #[test]
    fn obs_0055_items_show_their_macos_shortcuts() {
        use idecraft_ui::keymap::Keymap;
        let mac = Keymap::bundled_macos();
        for (id, shown) in [
            ("EditorContextInfo", Some("ctrl-shift-q")),
            ("EditSource", Some("cmd-down")),
            ("RecentLocations", Some("shift-cmd-e")),
            ("CompareTwoFiles", Some("cmd-d")),
            ("CompareClipboardWithSelection", None),
            ("ShowFilePath", Some("alt-cmd-f12")),
            ("PreviousOccurence", Some("alt-cmd-up")),
            ("NextOccurence", Some("alt-cmd-down")),
            ("EditorDuplicate", None),
        ] {
            assert_eq!(menu_shortcut_in(&mac, id).as_deref(), shown, "{id}");
        }
        assert!(menu_shortcut_in(&mac, "EditorIncreaseFontSizeGlobal").is_some());
        assert!(menu_shortcut_in(&mac, "EditorDecreaseFontSizeGlobal").is_some());
        let k = |t: &str| Keystroke::parse(t).unwrap();
        assert!(
            mac.actions_for(&k("meta D"), None)
                .contains(&"EditorDuplicate")
        );
    }

    /// SPEC-0124 AK-20 (v19, OBS-0055 M6): on a German layout the menu labels ⌘[ and ⌘] as ⌘Ö and
    /// ⌘Ä, as macOS localizes them; letters and named keys keep theirs. The keymap binds
    /// characters, not key codes: pressing ⌘Ö on that layout reaches Back through the layout's
    /// stand-in for `[`.
    // The labels are what macOS shows (⌘ is `cmd`); elsewhere the toolkit writes the same key as
    // `super`, and the menu bar is the macOS one.
    #[cfg(target_os = "macos")]
    #[test]
    fn menu_shortcuts_follow_the_keyboard_layout() {
        use idecraft_ui::keymap::{EventStrokes, Keymap};
        // Apple's localization of the German layout for the keys it moves (part of gpui's table).
        let german: std::collections::HashMap<char, char> =
            [('[', 'ö'), (']', 'ä'), ('/', 'ß'), (';', 'ü')]
                .into_iter()
                .collect();
        let mac = Keymap::bundled_macos();
        let shown =
            |id: &str| localized_keys(&menu_shortcut_in(&mac, id).expect(id), Some(&german));
        assert_eq!(menu_shortcut_in(&mac, "Back").as_deref(), Some("cmd-["));
        assert_eq!(shown("Back"), "cmd-ö");
        assert_eq!(shown("Forward"), "cmd-ä");
        assert_eq!(shown("CompareTwoFiles"), "cmd-d");
        assert_eq!(shown("EditSource"), "cmd-down");
        assert_eq!(localized_keys("shift-cmd-[", Some(&german)), "shift-cmd-ö");
        assert_eq!(localized_keys("cmd--", Some(&german)), "cmd--");
        let none: Option<&std::collections::HashMap<char, char>> = None;
        assert_eq!(
            localized_keys("cmd-[", none),
            "cmd-[",
            "a US layout keeps it"
        );
        // The localized label is a binding the toolkit takes.
        assert!(gpui::Keystroke::parse(&shown("Back")).is_ok());
        // ⌘Ö and ⌘Ä pressed on the German layout reach Back and Forward.
        for (key, id) in [("ö", "Back"), ("ä", "Forward")] {
            let strokes = EventStrokes::of(key, false, false, false, true, Some(&german));
            let stroke = strokes.resolve(&mac).expect("a shortcut");
            assert!(
                mac.actions_for(&stroke, None).contains(&id),
                "{key}: {stroke:?}"
            );
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

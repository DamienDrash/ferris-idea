//! SPEC-0303 AK-7: `idecraft open --print <file>` prints an exchange file's import preview and says
//! by its exit status whether it can be imported; `idecraft open <directory>` is unchanged.

use std::process::Command;

#[test]
fn open_print_shows_the_preview() {
    let dir = std::env::temp_dir().join(format!("idecraft-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let good = dir.join("light.idctheme");
    std::fs::write(
        &good,
        "[idecraft]\nkind = \"theme\"\nformat = 1\nname = \"Light\"\n\n[theme]\nname = \"IDECraft Light\"\n",
    )
    .unwrap();
    let newer = dir.join("newer.idcrun");
    std::fs::write(&newer, "[idecraft]\nkind = \"run\"\nformat = 2\n").unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_idecraft"))
            .args(args)
            .current_dir(&dir)
            .env("IDECRAFT_CONFIG_DIR", dir.join("config"))
            .output()
            .unwrap()
    };
    let out = run(&["open".as_ref(), "--print".as_ref(), good.as_os_str()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(
        text.starts_with("Import Theme: Light\nkind theme, format 1\n"),
        "{text}"
    );
    assert!(
        text.contains("  replace appearance.theme in IDE settings: IDECraft Dark → IDECraft Light"),
        "{text}"
    );
    assert!(
        !dir.join("config/settings.toml").exists(),
        "a preview writes nothing"
    );
    let out = run(&["open".as_ref(), "--print".as_ref(), newer.as_os_str()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("written by a newer IDECraft (format 2)"),
        "{text}"
    );
    let other = dir.join("notes.txt");
    std::fs::write(&other, "x").unwrap();
    let out = run(&["open".as_ref(), "--print".as_ref(), other.as_os_str()]);
    assert_eq!(out.status.code(), Some(2));
    // Other files are still refused, without a window (SPEC-0001).
    let out = run(&["open".as_ref(), other.as_os_str()]);
    assert!(!out.status.success());
    let out = run(&["open".as_ref(), dir.as_os_str()]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("workspace: "));
    let _ = std::fs::remove_dir_all(&dir);
}

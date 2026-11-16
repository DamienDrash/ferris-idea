//! Embeds `Info.plist` into the macOS binary, so the menu bar and the Dock name the application
//! "IDECraft" although it runs without an app bundle (`SPEC-0124` behaviour 1).

fn main() {
    println!("cargo:rerun-if-changed=Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
            .join("Info.plist");
        println!(
            "cargo:rustc-link-arg-bin=idecraft=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}

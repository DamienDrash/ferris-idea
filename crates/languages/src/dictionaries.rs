//! English word lists for the typo check (`SPEC-0159`): Hunspell dictionaries of the LibreOffice
//! project that the user downloads into IDECraft's settings folder, checked against the sizes and
//! SHA-256 sums of one fixed commit, so a changed or broken file is never used. English (United
//! States) of the same commit is built in (`SPEC-0159` v2 behaviour 6), so typos are checked from
//! the first start without a download.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Commit of `github.com/LibreOffice/dictionaries` the catalog was checked against (2026-10-27).
pub const COMMIT: &str = "4f1df64b2e8e14751aa7c2205d170ca016071315";

/// Where the files come from.
pub fn url(code: &str, ext: &str) -> String {
    format!("https://raw.githubusercontent.com/LibreOffice/dictionaries/{COMMIT}/en/{code}.{ext}")
}

/// The project page of a dictionary's licence text.
pub fn readme_url(code: &str) -> String {
    format!("https://github.com/LibreOffice/dictionaries/blob/{COMMIT}/en/README_{code}.txt")
}

/// One dictionary IDECraft can download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// Language code, e.g. `en_US`.
    pub code: &'static str,
    /// Name shown, e.g. `English (United States)`.
    pub name: &'static str,
    /// Licence of the word list.
    pub license: &'static str,
    /// Size and SHA-256 of the `.dic` file.
    pub dic: (u64, &'static str),
    /// Size and SHA-256 of the `.aff` file.
    pub aff: (u64, &'static str),
}

/// The dictionaries offered, in the order the typo check prefers them (`SPEC-0159` behaviour 1).
pub const CATALOG: &[Entry] = &[
    Entry {
        code: "en_US",
        name: "English (United States)",
        license: "SCOWL (BSD-style)",
        dic: (
            551_762,
            "f0b1a234bd178bdd01875b2a392a9647f888b8fe879f79c52aae62c2759b3647",
        ),
        aff: (
            3_205,
            "e746c882dd6f303c2c46e7452804b9201115a6942cfeb15f18f8edf774d2e24e",
        ),
    },
    Entry {
        code: "en_GB",
        name: "English (United Kingdom)",
        license: "LGPL",
        dic: (
            1_230_571,
            "04e90f34f5263bf26780e9c4a442e9ad16584e227af49ddd1b3b21b01df5b29c",
        ),
        aff: (
            44_158,
            "0fd6ed120ef28957847d98ba5149b117e27116cf81b5aa36208453f6755a36fd",
        ),
    },
    Entry {
        code: "en_CA",
        name: "English (Canada)",
        license: "SCOWL (BSD-style)",
        dic: (
            551_583,
            "53096689870246a78fa408b1e7a8c49bc140bd466a84a384cfb509445fff7173",
        ),
        aff: (
            3_205,
            "e746c882dd6f303c2c46e7452804b9201115a6942cfeb15f18f8edf774d2e24e",
        ),
    },
    Entry {
        code: "en_AU",
        name: "English (Australia)",
        license: "SCOWL (BSD-style)",
        dic: (
            554_336,
            "aa07c46571f306b79fc1bc534357ed357af15687381b26f891ba66e8a2caed89",
        ),
        aff: (
            3_205,
            "e746c882dd6f303c2c46e7452804b9201115a6942cfeb15f18f8edf774d2e24e",
        ),
    },
    Entry {
        code: "en_ZA",
        name: "English (South Africa)",
        license: "LGPL",
        dic: (
            998_279,
            "a438af6c6bfa9b25208d72c6ebf53507e178d6e6d285895e97870c36193433b1",
        ),
        aff: (
            43_842,
            "5ddaf2bd123df16cbb887adf7dd835593200d0454703c461714886833c9ddec9",
        ),
    },
];

/// The catalog entry of `code`.
pub fn entry(code: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.code == code)
}

/// The dictionary built into IDECraft (`SPEC-0159` v2 behaviour 6).
pub const BUILT_IN: &str = "en_US";

/// `en_US.dic` of [`COMMIT`], unchanged (licence: `dictionaries/README_en_US.txt`).
const BUILT_IN_DIC: &str = include_str!("../dictionaries/en_US.dic");
/// `en_US.aff` of [`COMMIT`], unchanged.
const BUILT_IN_AFF: &str = include_str!("../dictionaries/en_US.aff");

/// Whether `code` is built in.
pub fn is_built_in(code: &str) -> bool {
    code == BUILT_IN
}

/// The built-in `.dic` and `.aff` texts, once they match the catalog's size and SHA-256; `None`
/// when they do not (a changed file is never read).
pub fn built_in() -> Option<(&'static str, &'static str)> {
    static CHECKED: OnceLock<bool> = OnceLock::new();
    let ok = *CHECKED.get_or_init(|| {
        entry(BUILT_IN).is_some_and(|e| {
            verified(BUILT_IN_DIC.as_bytes(), e.dic) && verified(BUILT_IN_AFF.as_bytes(), e.aff)
        })
    });
    ok.then_some((BUILT_IN_DIC, BUILT_IN_AFF))
}

/// The word list the typo check reads (`SPEC-0159` v2 behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WordList {
    /// The `.dic` file of a downloaded dictionary (its `.aff` beside it).
    File(PathBuf),
    /// The dictionary built into IDECraft.
    BuiltIn,
}

/// IDECraft's folder for downloaded dictionaries: `dictionaries` in the user's settings folder.
pub fn dictionaries_dir() -> Option<PathBuf> {
    idecraft_core::user_config_dir().map(|d| d.join("dictionaries"))
}

/// Whether both files of `code` are in `dir`.
pub fn is_installed(dir: &Path, code: &str) -> bool {
    ["dic", "aff"]
        .iter()
        .all(|e| dir.join(format!("{code}.{e}")).is_file())
}

/// The `.dic` file of `code` in `dir`, if installed.
pub fn dic_file(dir: &Path, code: &str) -> Option<PathBuf> {
    is_installed(dir, code).then(|| dir.join(format!("{code}.dic")))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `bytes` are the file the catalog names: size and SHA-256.
pub fn verified(bytes: &[u8], expected: (u64, &str)) -> bool {
    bytes.len() as u64 == expected.0 && hex(&Sha256::digest(bytes)) == expected.1
}

/// Downloads `code` with `fetch` (URL to bytes), checks both files and writes them into `dir`
/// through temporary files (`SPEC-0159` behaviour 2). Nothing is kept when a check fails.
pub fn install(
    dir: &Path,
    code: &str,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let e = entry(code).ok_or_else(|| format!("no dictionary {code}"))?;
    let mut files = Vec::new();
    for (ext, expected) in [("dic", e.dic), ("aff", e.aff)] {
        let bytes = fetch(&url(code, ext))?;
        if !verified(&bytes, expected) {
            return Err(format!(
                "{code}.{ext} is not the expected file (size or SHA-256 differs); nothing was installed"
            ));
        }
        files.push((ext, bytes));
    }
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    for (ext, bytes) in &files {
        let part = dir.join(format!("{code}.{ext}.part"));
        std::fs::write(&part, bytes).map_err(|err| err.to_string())?;
    }
    for (ext, _) in &files {
        std::fs::rename(
            dir.join(format!("{code}.{ext}.part")),
            dir.join(format!("{code}.{ext}")),
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Removes the files of `code` from `dir`.
pub fn remove(dir: &Path, code: &str) -> Result<(), String> {
    for ext in ["dic", "aff"] {
        let f = dir.join(format!("{code}.{ext}"));
        if f.exists() {
            std::fs::remove_file(&f).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The dictionary the typo check uses (`SPEC-0159` v2 behaviour 3): `chosen` when it is downloaded
/// into `dir` or built in, else the first downloaded one of the catalog, else the built-in one.
pub fn active(dir: Option<&Path>, chosen: &str) -> &'static str {
    let installed = |code: &str| dir.is_some_and(|d| is_installed(d, code));
    entry(chosen)
        .filter(|e| is_built_in(e.code) || installed(e.code))
        .or_else(|| CATALOG.iter().find(|e| installed(e.code)))
        .map_or(BUILT_IN, |e| e.code)
}

/// The word list of the dictionary [`active`] picks: its downloaded file when there is one, else
/// the built-in list.
pub fn word_list(dir: Option<&Path>, chosen: &str) -> WordList {
    let code = active(dir, chosen);
    dir.and_then(|d| dic_file(d, code))
        .map_or(WordList::BuiltIn, WordList::File)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_only_verified_files_and_picks_the_active_one() {
        let dir = std::env::temp_dir().join(format!("idecraft-dicts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A tampered download is refused and leaves nothing behind.
        let bad = install(&dir, "en_US", &|_| Ok(b"1\nword\n".to_vec())).unwrap_err();
        assert!(bad.contains("not the expected file"), "{bad}");
        assert!(!is_installed(&dir, "en_US"));
        assert!(install(&dir, "xx_XX", &|_| Ok(Vec::new())).is_err());
        assert!(url("en_GB", "dic").contains(COMMIT));
        assert_eq!(active(Some(&dir), ""), BUILT_IN);
        // Installed files (written as if verified) are found; the choice wins when installed.
        std::fs::create_dir_all(&dir).unwrap();
        for code in ["en_GB", "en_CA"] {
            std::fs::write(dir.join(format!("{code}.dic")), "1\nword\n").unwrap();
            std::fs::write(dir.join(format!("{code}.aff")), "SET UTF-8\n").unwrap();
        }
        assert_eq!(active(Some(&dir), ""), "en_GB");
        assert_eq!(active(Some(&dir), "en_CA"), "en_CA");
        // A dictionary not downloaded is not used, unless it is the built-in one (v2).
        assert_eq!(active(Some(&dir), "en_AU"), "en_GB");
        assert_eq!(active(Some(&dir), "en_US"), "en_US");
        assert_eq!(word_list(Some(&dir), "en_US"), WordList::BuiltIn);
        remove(&dir, "en_GB").unwrap();
        assert_eq!(active(Some(&dir), ""), "en_CA");
        assert_eq!(
            word_list(Some(&dir), ""),
            WordList::File(dir.join("en_CA.dic"))
        );
        assert!(verified(
            b"",
            (
                0,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            )
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0159/AK-6 (v2): the built-in files are the catalog's; a fresh settings folder without
    /// any download uses them, and a download still comes first. Nothing is fetched.
    #[test]
    fn built_in_en_us_without_a_download() {
        let e = entry(BUILT_IN).unwrap();
        assert!(
            verified(BUILT_IN_DIC.as_bytes(), e.dic),
            "en_US.dic as in the catalog"
        );
        assert!(
            verified(BUILT_IN_AFF.as_bytes(), e.aff),
            "en_US.aff as in the catalog"
        );
        let (dic, aff) = built_in().expect("the built-in files pass the check");
        assert!(dic.starts_with("49568\n") && aff.contains("SFX S"));
        let dir = std::env::temp_dir().join(format!("idecraft-dicts-fresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Fresh: no settings folder, or no dictionaries folder in it yet.
        for d in [None, Some(dir.as_path())] {
            assert_eq!(active(d, ""), BUILT_IN);
            assert_eq!(word_list(d, ""), WordList::BuiltIn);
            assert_eq!(
                word_list(d, "en_GB"),
                WordList::BuiltIn,
                "en_GB is not downloaded"
            );
        }
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(word_list(Some(&dir), ""), WordList::BuiltIn);
        // A downloaded dictionary comes first, the choice before it.
        std::fs::write(dir.join("en_GB.dic"), "1\ncolour\n").unwrap();
        std::fs::write(dir.join("en_GB.aff"), "SET UTF-8\n").unwrap();
        assert_eq!(
            word_list(Some(&dir), ""),
            WordList::File(dir.join("en_GB.dic"))
        );
        assert_eq!(word_list(Some(&dir), "en_US"), WordList::BuiltIn);
        assert!(is_built_in("en_US") && !is_built_in("en_GB"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

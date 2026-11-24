//! English word lists for the typo check (`SPEC-0159`): Hunspell dictionaries of the LibreOffice
//! project that the user downloads into Ferris IDEA's settings folder, checked against the sizes and
//! SHA-256 sums of one fixed commit, so a changed or broken file is never used. English (United
//! States) of the same commit is built in (`SPEC-0159` v2 behaviour 6), so typos are checked from
//! the first start without a download; English (United Kingdom) of SCOWL's release 2020.12.07 is
//! built in too (v4), and several dictionaries can be used at once (v4 behaviour 3).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Commit of `github.com/LibreOffice/dictionaries` the catalog was checked against (2026-10-27).
pub const COMMIT: &str = "4f1df64b2e8e14751aa7c2205d170ca016071315";

/// SCOWL's zip the built-in English (United Kingdom) comes from (`SPEC-0159` v4).
pub const EN_GB_SOURCE: &str = "https://downloads.sourceforge.net/project/wordlist/speller/2020.12.07/hunspell-en_GB-ise-2020.12.07.zip";

/// Where the files come from: the LibreOffice project at [`COMMIT`], or for English (United
/// Kingdom) SCOWL's zip, which is never downloaded (it is built in).
pub fn url(code: &str, ext: &str) -> String {
    if code == "en_GB" {
        return EN_GB_SOURCE.to_owned();
    }
    format!("https://raw.githubusercontent.com/LibreOffice/dictionaries/{COMMIT}/en/{code}.{ext}")
}

/// The project page of a dictionary's licence text.
pub fn readme_url(code: &str) -> String {
    format!("https://github.com/LibreOffice/dictionaries/blob/{COMMIT}/en/README_{code}.txt")
}

/// One dictionary Ferris IDEA can download.
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
    // SCOWL's en_GB-ise of release 2020.12.07, built in (`SPEC-0159` v4).
    Entry {
        code: "en_GB",
        name: "English (United Kingdom)",
        license: "SCOWL (BSD-style)",
        dic: (
            552_106,
            "869fe17ba4ee4b5401c60a666ee2d6a3dcc237f460b7294435df1cc6a799aa57",
        ),
        aff: (
            3_090,
            "c7a8c4d08c29d237880844b1623099f59092602f189be38ce3912e457ff38bc1",
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

/// The dictionary used when nothing else is (`SPEC-0159` v2 behaviour 3): English (United States).
pub const BUILT_IN: &str = "en_US";

/// The dictionaries built into Ferris IDEA with their `.dic` and `.aff` texts, unchanged
/// (`SPEC-0159` v2/v4 behaviour 6): en_US of [`COMMIT`] (licence `dictionaries/README_en_US.txt`)
/// and SCOWL's en_GB-ise of release 2020.12.07 (licence `dictionaries/README_en_GB-ise.txt`).
const BUILT_INS: [(&str, &str, &str); 2] = [
    (
        "en_US",
        include_str!("../dictionaries/en_US.dic"),
        include_str!("../dictionaries/en_US.aff"),
    ),
    (
        "en_GB",
        include_str!("../dictionaries/en_GB-ise.dic"),
        include_str!("../dictionaries/en_GB-ise.aff"),
    ),
];

/// Whether `code` is built in.
pub fn is_built_in(code: &str) -> bool {
    BUILT_INS.iter().any(|(c, _, _)| *c == code)
}

/// The built-in `.dic` and `.aff` texts of `code`, once they match the catalog's size and
/// SHA-256; `None` when `code` is not built in or they do not match (a changed file is never
/// read). Each is checked once, when it is first used.
pub fn built_in(code: &str) -> Option<(&'static str, &'static str)> {
    static CHECKED: [OnceLock<bool>; 2] = [OnceLock::new(), OnceLock::new()];
    let at = BUILT_INS.iter().position(|(c, _, _)| *c == code)?;
    let (_, dic, aff) = BUILT_INS[at];
    let ok = *CHECKED[at].get_or_init(|| {
        entry(code)
            .is_some_and(|e| verified(dic.as_bytes(), e.dic) && verified(aff.as_bytes(), e.aff))
    });
    ok.then_some((dic, aff))
}

/// A word list the typo check reads (`SPEC-0159` v2/v4 behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WordList {
    /// The `.dic` file of a downloaded dictionary (its `.aff` beside it).
    File(PathBuf),
    /// A dictionary built into Ferris IDEA, by code.
    BuiltIn(&'static str),
}

/// Ferris IDEA's folder for downloaded dictionaries: `dictionaries` in the user's settings folder.
pub fn dictionaries_dir() -> Option<PathBuf> {
    ferris_idea_core::user_config_dir().map(|d| d.join("dictionaries"))
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
    if is_built_in(code) {
        return Err(format!("{code} is built in; nothing to download"));
    }
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

/// The catalog codes named in the setting `chosen` (`SPEC-0159` v4 behaviour 3): one code or
/// several separated by commas, in the order given; unknown codes and repeats are left out.
pub fn chosen_codes(chosen: &str) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = Vec::new();
    for part in chosen.split(',') {
        if let Some(e) = entry(part.trim())
            && !codes.contains(&e.code)
        {
            codes.push(e.code);
        }
    }
    codes
}

/// Whether `code` can be used: built in, or downloaded into `dir`.
pub fn usable(dir: Option<&Path>, code: &str) -> bool {
    is_built_in(code) || dir.is_some_and(|d| is_installed(d, code))
}

/// The dictionaries the typo check uses (`SPEC-0159` v4 behaviour 3), never empty: the chosen ones
/// that are built in or downloaded into `dir`, in the order given; when none is, the first
/// downloaded one of the catalog, else English (United States).
pub fn active_all(dir: Option<&Path>, chosen: &str) -> Vec<&'static str> {
    let used: Vec<&'static str> = chosen_codes(chosen)
        .into_iter()
        .filter(|c| usable(dir, c))
        .collect();
    if !used.is_empty() {
        return used;
    }
    let installed = |code: &str| dir.is_some_and(|d| is_installed(d, code));
    vec![
        CATALOG
            .iter()
            .find(|e| installed(e.code))
            .map_or(BUILT_IN, |e| e.code),
    ]
}

/// The first dictionary of [`active_all`] (`SPEC-0159` v2 behaviour 3).
pub fn active(dir: Option<&Path>, chosen: &str) -> &'static str {
    active_all(dir, chosen)[0]
}

/// The word list of `code`: built in when it is (a file of the same code in `dir` is not read,
/// `SPEC-0159` v4), else its downloaded file.
fn list_of(dir: Option<&Path>, code: &'static str) -> Option<WordList> {
    if is_built_in(code) {
        return Some(WordList::BuiltIn(code));
    }
    dir.and_then(|d| dic_file(d, code)).map(WordList::File)
}

/// The word lists of the dictionaries [`active_all`] picks, in the same order.
pub fn word_lists(dir: Option<&Path>, chosen: &str) -> Vec<WordList> {
    active_all(dir, chosen)
        .into_iter()
        .filter_map(|c| list_of(dir, c))
        .collect()
}

/// The word list of the first dictionary [`active_all`] picks.
pub fn word_list(dir: Option<&Path>, chosen: &str) -> WordList {
    word_lists(dir, chosen)
        .into_iter()
        .next()
        .unwrap_or(WordList::BuiltIn(BUILT_IN))
}

/// The setting after Space on `code` in Settings › Editor › Natural Languages (`SPEC-0159` v4
/// behaviour 3): `code` is added to the dictionaries used, or taken away when it is one of them;
/// the last one stays. The codes are written in the order they are used, separated by commas.
pub fn toggled(dir: Option<&Path>, chosen: &str, code: &str) -> String {
    let mut used = active_all(dir, chosen);
    if let Some(at) = used.iter().position(|c| *c == code) {
        if used.len() > 1 {
            used.remove(at);
        }
    } else if let Some(e) = entry(code).filter(|e| usable(dir, e.code)) {
        used.push(e.code);
    }
    used.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test under the system's temporary folder.
    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-dicts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Writes a fake download of `code` into `dir` (as if it had passed the check).
    fn fake_download(dir: &Path, code: &str, words: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{code}.dic")), words).unwrap();
        std::fs::write(dir.join(format!("{code}.aff")), "SET UTF-8\n").unwrap();
    }

    #[test]
    fn installs_only_verified_files_and_picks_the_active_one() {
        let dir = temp("install");
        // A tampered download is refused and leaves nothing behind.
        let bad = install(&dir, "en_CA", &|_| Ok(b"1\nword\n".to_vec())).unwrap_err();
        assert!(bad.contains("not the expected file"), "{bad}");
        assert!(!is_installed(&dir, "en_CA"));
        assert!(install(&dir, "xx_XX", &|_| Ok(Vec::new())).is_err());
        assert!(url("en_CA", "dic").contains(COMMIT));
        assert_eq!(active(Some(&dir), ""), BUILT_IN);
        // Installed files (written as if verified) are found; the choice wins when installed.
        for code in ["en_CA", "en_AU"] {
            fake_download(&dir, code, "1\nword\n");
        }
        assert_eq!(active(Some(&dir), ""), "en_CA");
        assert_eq!(active(Some(&dir), "en_AU"), "en_AU");
        // A dictionary not downloaded is not used, unless it is built in (v2, v4).
        assert_eq!(active(Some(&dir), "en_ZA"), "en_CA");
        assert_eq!(active(Some(&dir), "en_US"), "en_US");
        assert_eq!(active(Some(&dir), "en_GB"), "en_GB");
        assert_eq!(word_list(Some(&dir), "en_US"), WordList::BuiltIn("en_US"));
        remove(&dir, "en_CA").unwrap();
        assert_eq!(active(Some(&dir), ""), "en_AU");
        assert_eq!(
            word_list(Some(&dir), ""),
            WordList::File(dir.join("en_AU.dic"))
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
    /// any download uses them, and a downloaded dictionary still comes first. Nothing is fetched.
    #[test]
    fn built_in_en_us_without_a_download() {
        let (dic, aff) = built_in("en_US").expect("the built-in files pass the check");
        let e = entry("en_US").unwrap();
        assert!(
            verified(dic.as_bytes(), e.dic),
            "en_US.dic as in the catalog"
        );
        assert!(
            verified(aff.as_bytes(), e.aff),
            "en_US.aff as in the catalog"
        );
        assert!(dic.starts_with("49568\n") && aff.contains("SFX S"));
        assert!(built_in("en_CA").is_none(), "en_CA is not built in");
        let dir = temp("fresh");
        // Fresh: no settings folder, or no dictionaries folder in it yet.
        for d in [None, Some(dir.as_path())] {
            assert_eq!(active(d, ""), BUILT_IN);
            assert_eq!(word_list(d, ""), WordList::BuiltIn("en_US"));
            assert_eq!(
                word_list(d, "en_CA"),
                WordList::BuiltIn("en_US"),
                "en_CA is not downloaded"
            );
        }
        // A downloaded dictionary comes first, the choice before it.
        fake_download(&dir, "en_CA", "1\ncolour\n");
        assert_eq!(
            word_list(Some(&dir), ""),
            WordList::File(dir.join("en_CA.dic"))
        );
        assert_eq!(word_list(Some(&dir), "en_US"), WordList::BuiltIn("en_US"));
        assert!(is_built_in("en_US") && !is_built_in("en_CA"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0159/AK-8 (v4): the built-in en_GB is SCOWL's en_GB-ise as the catalog names it, is
    /// never downloaded, and is read from the program even when an earlier download of en_GB lies
    /// in the folder.
    #[test]
    fn built_in_en_gb() {
        let (dic, aff) = built_in("en_GB").expect("the built-in en_GB passes the check");
        let e = entry("en_GB").unwrap();
        assert!(verified(dic.as_bytes(), e.dic) && verified(aff.as_bytes(), e.aff));
        assert_eq!(e.license, "SCOWL (BSD-style)");
        assert!(dic.starts_with("49601\n") && dic.contains("\ncolour/"));
        assert!(is_built_in("en_GB"));
        assert_eq!(url("en_GB", "dic"), EN_GB_SOURCE);
        let dir = temp("gb");
        let refused = install(&dir, "en_GB", &|u| {
            panic!("no fetch for a built-in list: {u}")
        });
        assert!(refused.unwrap_err().contains("built in"));
        assert!(!dir.exists(), "nothing written");
        // Fresh settings: en_GB is used when chosen, without a download.
        assert_eq!(word_list(Some(&dir), "en_GB"), WordList::BuiltIn("en_GB"));
        // An earlier download of en_GB (the LibreOffice list) is not read; it still counts as the
        // user's earlier choice when nothing is chosen.
        fake_download(&dir, "en_GB", "1\ncolor\n");
        assert_eq!(word_list(Some(&dir), ""), WordList::BuiltIn("en_GB"));
        assert_eq!(word_list(Some(&dir), "en_GB"), WordList::BuiltIn("en_GB"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0159/AK-9 (v4): several dictionaries at once, in the order chosen; unknown codes and
    /// repeats left out; Space adds and takes away, the last one stays.
    #[test]
    fn several_dictionaries_at_once() {
        assert_eq!(
            chosen_codes(" en_GB , en_US,en_GB,xx_XX,"),
            ["en_GB", "en_US"]
        );
        assert!(chosen_codes("").is_empty());
        let dir = temp("several");
        let d = Some(dir.as_path());
        assert_eq!(active_all(d, "en_GB,en_US"), ["en_GB", "en_US"]);
        assert_eq!(
            word_lists(d, "en_GB,en_US"),
            [WordList::BuiltIn("en_GB"), WordList::BuiltIn("en_US")]
        );
        // Space with nothing chosen adds to the default.
        assert_eq!(toggled(d, "", "en_GB"), "en_US,en_GB");
        // A chosen dictionary that is not downloaded is left out; the others stay.
        assert_eq!(active_all(d, "en_CA,en_GB"), ["en_GB"]);
        fake_download(&dir, "en_CA", "1\ncolour\n");
        assert_eq!(
            word_lists(d, "en_CA,en_GB"),
            [
                WordList::File(dir.join("en_CA.dic")),
                WordList::BuiltIn("en_GB")
            ]
        );
        assert_eq!(active(d, "en_CA,en_GB"), "en_CA");
        // Space: add, take away, the last one stays; a dictionary not downloaded is not added.
        assert_eq!(toggled(d, "en_US", "en_GB"), "en_US,en_GB");
        assert_eq!(toggled(d, "en_US,en_GB", "en_US"), "en_GB");
        assert_eq!(toggled(d, "en_GB", "en_GB"), "en_GB");
        assert_eq!(toggled(d, "en_GB", "en_ZA"), "en_GB");
        assert_eq!(toggled(d, "en_GB", "en_CA"), "en_GB,en_CA");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

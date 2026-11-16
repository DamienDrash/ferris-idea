# Built-in spelling dictionary

English (United States), unchanged: `en_US.dic`, `en_US.aff` and their licence text
`README_en_US.txt`, from the folder `en` of `LibreOffice/dictionaries` at commit
`4f1df64b2e8e14751aa7c2205d170ca016071315`, the commit of the download catalog in
`crates/languages/src/dictionaries.rs` (`SPEC-0159` v2 behaviour 6).

| File | Bytes | SHA-256 |
|---|---|---|
| `en_US.dic` | 551762 | `f0b1a234bd178bdd01875b2a392a9647f888b8fe879f79c52aae62c2759b3647` |
| `en_US.aff` | 3205 | `e746c882dd6f303c2c46e7452804b9201115a6942cfeb15f18f8edf774d2e24e` |
| `README_en_US.txt` | 15732 | `168b4c01cb841f766a72f310b065c04d09cff46376c37a81d799593ce751c371` |

The word list comes from SCOWL (Kevin Atkinson and others) under the permissive terms that
`README_en_US.txt` gives in full; it must stay beside the files and in every copy. The files are
compiled into IDECraft and checked against the catalog's size and SHA-256 before they are read
(`dictionaries::built_in`). Do not edit them; to update, take the files of a newer commit, change
the catalog, this table, `NOTICE` and the specification together.

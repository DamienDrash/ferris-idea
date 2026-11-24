# Built-in spelling dictionaries

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

## English (United Kingdom)

`en_GB-ise.dic`, `en_GB-ise.aff` and their licence text `README_en_GB-ise.txt`, unchanged, from
SCOWL's Hunspell dictionary with "ise" spelling, release 2020.12.07
(`hunspell-en_GB-ise-2020.12.07.zip` from the folder `speller/2020.12.07` of the SCOWL project's
SourceForge downloads, MD5 `1c0d38766cd35c41e80b50f42ceecfba` as SourceForge publishes it,
SHA-256 `2ced4b2ec9d188c3b540dbb75b5143f65b990daf17437758726ff1fab5f01ad3`), `SPEC-0159` v4
behaviour 6. Its terms are word for word those of `README_en_US.txt` (only the name and build
lines differ); the zip of en_US of the same release holds an `en_US.dic` equal to the one above.
The files are checked against the catalog's size and SHA-256 before they are read
(`dictionaries::built_in("en_GB")`).

| File | Bytes | SHA-256 |
|---|---|---|
| `en_GB-ise.dic` | 552106 | `869fe17ba4ee4b5401c60a666ee2d6a3dcc237f460b7294435df1cc6a799aa57` |
| `en_GB-ise.aff` | 3090 | `c7a8c4d08c29d237880844b1623099f59092602f189be38ce3912e457ff38bc1` |
| `README_en_GB-ise.txt` | 15740 | `0bcf1616b29dee3fbcbef1428b0c3d4d77f6a36cc88b9e130ee65e5382899029` |

## Frequency classes

`en_frequency.txt` ranks the suggestions for a typo, the common word first (`SPEC-0159` v3
behaviour 7). It is made from SCOWL release 2020.12.07 (`scowl-2020.12.07.tar.gz` from the SCOWL
project's SourceForge downloads, SHA-256
`5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc`): the words of the size levels
10, 20 and 35 of `final/english-words.*`, `american-words.*` and `british-words.*` as classes 1, 2
and 3, each word in the lowest class it is listed in, sorted within its class, possessive forms
(`'s`) left out, re-encoded from Latin-1 to UTF-8. `SCOWL-Copyright.txt` is SCOWL's licence file
`Copyright`, unchanged; it must stay beside the list.

| File | Bytes | SHA-256 |
|---|---|---|
| `en_frequency.txt` | 360120 | `2889037b7a56a30f4c027025dc6b83465b35b598382888df74a61f335cd0a8b5` |
| `SCOWL-Copyright.txt` | 11758 | `283326a422e29c510e2ba6805518c418ce3c35ed1fadc3eec83d2da4f6c5a055` |

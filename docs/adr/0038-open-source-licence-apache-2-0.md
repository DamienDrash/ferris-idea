# ADR-0038: Open-source licence: Apache-2.0

- **Status:** accepted · **Version:** 1 · **Date:** 2026-11-25 · decided by the Project Owner on
  2026-11-25 · supersedes [ADR-0032](0032-source-available-licence.md) (PolyForm Shield 1.0.0) ·
  amends [ADR-0006](0006-apache-porting.md) (licence of crates with ported code)
- **Related:** [`LICENSE.md`](../../LICENSE.md), [`NOTICE`](../../NOTICE),
  [`LICENSING.md`](../../LICENSING.md), [`TRADEMARKS.md`](../../TRADEMARKS.md),
  [`LICENSES/`](../../LICENSES/README.md), [`docs/legal/CLA.md`](../legal/CLA.md),
  [`docs/legal/APACHE_PORTING.md`](../legal/APACHE_PORTING.md)
- This record is not legal advice.

## Context

Since 2026-11-14 (ADR-0032) the project's own code and documentation were under the PolyForm
Shield License 1.0.0: source-available, with one excluded purpose, providing a competing product.
That licence is not approved by the Open Source Initiative.

On 2026-11-25 the Project Owner decided to move the project to an OSI-approved licence, so that it
qualifies for programmes that support open-source projects and require one. His words:
„Ändere dazu ferris zu Apache-2.0“.

The licensor holds the rights in the project's own work. No outside contribution was merged:
[`CLA_SIGNATURES.md`](../legal/CLA_SIGNATURES.md) has no row, so no one else's consent is needed.
The CLA (from version 1.0) already lets the licensor license contributions under other terms.

## Decision

1. **Licence:** the **Apache License, Version 2.0, unmodified**, for the project's own code,
   documentation and own assets. `LICENSE.md` holds the text as published at
   <https://www.apache.org/licenses/LICENSE-2.0.txt> (byte for byte the same as
   `LICENSES/Apache-2.0.txt`, SHA-256 `cfc7749b…`). SPDX identifier `Apache-2.0`.
2. **NOTICE** names the licensor and the copyright line, states the licence and keeps every
   third-party attribution: the NOTICE text of `JetBrains/intellij-community`, JetBrains Mono
   (OFL-1.1), the Hunspell dictionaries and SCOWL, the vendored crates under `vendor/`. It is the
   NOTICE file of Apache-2.0 section 4(d). The `Required Notice:` line of PolyForm Shield is gone.
3. **Cargo manifests:** the workspace, `fuzz/` and `crates/ui` are `Apache-2.0`;
   `crates/workbench` (embeds JetBrains Mono) is `Apache-2.0 AND OFL-1.1`. A crate with ported
   Apache-2.0 code needs no `AND` any more (amends ADR-0006 and its ADR-0032 amendment).
   `publish = false` stays; `deny.toml` keeps skipping the unpublished workspace crates.
4. **File headers:** every `SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0` and every
   own-icon comment naming PolyForm Shield names Apache-2.0. The Java debug helper's manifest says
   `Bundle-License: Apache-2.0`; its jar was rebuilt with `scripts/build-java-debug-private.sh` and
   differs from the earlier one in that manifest line only.
5. **Third-party parts keep their licences**, as before: the upstream keymaps, themes and icons
   (Apache-2.0), JetBrains Mono (OFL-1.1), the dictionaries (SCOWL and its sources), every
   dependency under its own licence (`THIRD-PARTY-NOTICES.md`).
6. **Name and logo stay reserved.** Apache-2.0 section 6 grants no permission to use the
   licensor's trade names, trademarks, service marks or product names. `TRADEMARKS.md` stands
   unchanged in substance: the name, symbol, wordmark, logo and app icon are not under the
   licence.
7. **Contributions:** the Contributor License Agreement stays, as version 1.2, which only names
   where the licensor is stated now (NOTICE and LICENSING.md instead of LICENSE.md). Its grants are
   unchanged.
8. **Wording:** Ferris IDEA is "open source" (Apache-2.0). The brand and content guides, the
   README badge and the GitHub description and topics change with it (`open-source` replaces
   `source-available`).
9. **Earlier versions:** the versions exported to the public repository before this decision were
   published under PolyForm Shield 1.0.0; a copy taken under those terms keeps them. Every export
   from this decision on is Apache-2.0. History in the working repository is not rewritten.

## Alternatives

| Licence | Why not |
|---|---|
| Keep PolyForm Shield 1.0.0 | Not OSI-approved; the reason for the change. |
| MIT | OSI-approved and shorter, but no patent licence and no NOTICE mechanism; the bundled upstream material is Apache-2.0 already. |
| A choice of MIT or Apache-2.0 (ADR-0002) | Common in Rust; the Project Owner named Apache-2.0 alone. |
| MPL-2.0, GPL-3.0, AGPL-3.0 | Copyleft; not what was asked, and it would change the dependency policy. |

## Consequences

- **The non-compete limit is gone.** Anyone may sell Ferris IDEA, offer it as a service or build a
  competing product on it, keeping the licence's conditions (licence copy, change notices, NOTICE).
  Separate commercial licences have nothing left to grant beyond the trademark.
- **Open questions of ADR-0032** on the fit of a non-compete licence, its enforceability and selling
  licences (its questions 2, 4, 5 in part, 12 and 13) no longer apply. Still open: AI-generated code
  and copyright (6), the contributor agreement under German law (7), name and trademark clearance
  (11), and the native libraries per release (15).
- **Dependency policy unchanged.** Every licence in `deny.toml`'s allow list is compatible with
  distribution under Apache-2.0; MPL-2.0 packages stay under MPL-2.0 file by file.
- **No technical enforcement**, as before (ADR-0007).

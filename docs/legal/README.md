# Legal summary (English)

The detailed documents in this folder are in German because they analyse German and EU law.
This page summarises them. It is not legal advice.

- **Functionality is free, expression is not.** Ideas, functions, programming languages and file
  formats are not protected by copyright (§ 69a(2) UrhG; CJEU C-406/10 *SAS Institute*).
  Code, texts, icons and graphics are. → legal analysis §2
- **Black-box observation** of the reference IDE during a lawful licence (the official trial)
  is a mandatory right that contracts cannot exclude (§ 69d(3), § 69g(2) UrhG; Art. 5(3), 8
  Directive 2009/24/EC). Decompiling, unpacking, profiler attachment and any licence or trial
  circumvention are excluded. → [RESEARCH_BOUNDARIES.md](RESEARCH_BOUNDARIES.md)
- **Open-source platform code.** Much of the IntelliJ Platform is Apache-2.0 in
  `JetBrains/intellij-community`. It may be ported to Rust with attribution: SPDX header,
  [PROVENANCE.md](PROVENANCE.md) entry, upstream [NOTICE](../../NOTICE). The ported files and the
  copied assets stay under Apache-2.0, the licence of the whole project since 2026-11-25.
  → [APACHE_PORTING.md](APACHE_PORTING.md)
- **The project's own licence.** Ferris IDEA is open source under the Apache License 2.0
  ([LICENSE.md](../../LICENSE.md)) since 2026-11-25 ([ADR-0038](../adr/0038-open-source-licence-apache-2-0.md)).
  It replaces the PolyForm Shield License 1.0.0 of 2026-11-14 ([ADR-0032](../adr/0032-source-available-licence.md),
  superseded), which replaced the earlier choice `MIT OR Apache-2.0` (never used for a distribution).
  Notes: [LICENSING.md](../../LICENSING.md).
- **Proprietary features** (database tools, JavaScript/TypeScript, Spring and others) are
  rebuilt clean-room from observation, public facts and open standards only.
- **Names and logos.** No JetBrains or ArtCraft logos, icons or product names in the product;
  descriptive mentions only. → [../design/BRAND_GUIDELINES.md](../design/BRAND_GUIDELINES.md) §5
- **Legal consultation.** The project owner obtained legal advice on 2026-10-30 confirming the
  clean-room approach and the use of Apache-2.0 platform code (legal analysis §10).
- **Licences of test servers.** CockroachDB's licence is accepted for development and tests only,
  as a single node without a key, account or outbound network.
  → [COCKROACHDB_LICENCE.md](COCKROACHDB_LICENCE.md)

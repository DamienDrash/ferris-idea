# Übernahme aus `intellij-community` (Apache-2.0)

**Stand:** 2026-10-30 · Begründung: Rechtsanalyse, Abschnitt 10 · Entscheidung: ADR-0006

## Was darf übernommen werden

Alles aus dem Repository `JetBrains/intellij-community`, das unter dessen
Apache-2.0-Lizenz steht: Code (als Port nach Rust), Themes, Icons, Keymaps,
Farbschemata, Jewel-Metriken, Testdaten. **Nicht:** Logos, Produktnamen,
Produkt-Icons und Splash-Grafiken (Apache-2.0 Ziff. 6), Dateien unter
`license/` mit Fremdlizenzen ohne Einzelprüfung, Inhalte, die nur über
`getPlugins.sh` o. ä. nachgeladen werden (anderer Ursprung).

Referenz-Commit für alle Übernahmen, bis er bewusst angehoben wird:
`d4018fb57c367e7b5d895bd8a0c84c382efd0e17` (master, 2026-10-30).

## Pflichten je Übernahme

1. **Herkunft festhalten** — Zeile in [`PROVENANCE.md`](PROVENANCE.md):
   Zieldatei, Quellpfad, Commit, Art (Port / Asset / Logik-Vorlage).
2. **Dateikopf** in jeder Rust-Datei mit übertragenem Code:

   ```rust
   // SPDX-License-Identifier: Apache-2.0
   // Derived from JetBrains intellij-community, <quellpfad>
   // at commit <sha>. Copyright 2000-2026 JetBrains s.r.o. and contributors.
   // Modified by the IDECraft contributors: ported from <Java|Kotlin> to Rust.
   ```

   Assets (SVG, JSON, XML) behalten ihren Inhalt; die Herkunft steht in
   `PROVENANCE.md` und im Ordner-`README.md` des Ziels.
3. **Crate-Lizenz:** Ein Crate mit übertragenem Code oder unverändert
   übernommenen Assets trägt in `Cargo.toml`
   `license = "LicenseRef-PolyForm-Shield-1.0.0 AND Apache-2.0"` statt
   `license.workspace = true` (ADR-0032, 2026-11-14; vorher: nur `Apache-2.0`).
   Die übertragenen Dateien und die Assets selbst bleiben unter Apache-2.0;
   der Lizenztext liegt in `LICENSES/Apache-2.0.txt`.
4. **NOTICE:** Der NOTICE-Text von `intellij-community` steht in unserer
   `NOTICE` und wird mit jeder Verbreitung (auch Binärpaketen) mitgegeben.
5. **Logik-Vorlage ohne Code:** Wurde nur die Arbeitsweise eines Moduls
   übernommen (eigener Code), reicht der Eintrag in `PROVENANCE.md` mit Art
   „Logik-Vorlage“; das Crate bleibt unter der Lizenz des Projekts
   (PolyForm Shield 1.0.0, ADR-0032).

## Was weiterhin clean room bleibt

Funktionen, deren Quelltext **nicht** offen ist (siehe Tabelle in
Rechtsanalyse 10.3: Database Tools, JS/TS, Spring, HTTP-Client …), entstehen
nur aus Beobachtung, Standards und Doku — nach `RESEARCH_BOUNDARIES.md`.
Ein Port aus offenem Code und eine Clean-Room-Spezifikation dürfen
nebeneinander stehen; die Spezifikation nennt den Port dann als Quelle `OSS`.

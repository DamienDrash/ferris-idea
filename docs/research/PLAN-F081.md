# PLAN-F081: Funktionen offener Plugins neben der Referenz vergleichen

> Plan für den Vergleich, Stand 2026-11-15. Den Vergleich führt der Koordinator durch; die Ergebnisse
> kommen danach in eine Beobachtung (`docs/research/observations/OBS-nnnn.md`).
> Regeln: `docs/legal/RESEARCH_BOUNDARIES.md`, Abschnitte 3–5. Eigene Worte, keine Bilder im Repository.

- **Feature-Matrix:** F-081 (Stufe 3 → 4)
- **Grundlage:** SPEC-0137 (EditorConfig), SPEC-0155 (Copyright, Properties, Grammatik), SPEC-0159
  (Wörterbücher), SPEC-0163 (Projektwörterbuch, Bundles), SPEC-0212 (Resource Bundle Editor)
- **Ohne Bildschirm erledigt (Ferris IDEA-Seite):**
  - `crates/workbench/tests/it/editorconfig_handtest_63.rs` (Handtest 63)
  - `crates/workbench/tests/it/open_plugin_handtests.rs` (Handtests 81, 89, 152)
  - Handtest 85 lädt ein Wörterbuch aus dem Netz und bleibt einem Lauf mit Netz vorbehalten.

## Aufbau
Es gibt zwei Wegwerf-Projekte mit demselben Inhalt: `~/Projects/cmp-ec-ref` für die Referenz und
`~/Projects/cmp-ec-ide` für Ferris IDEA. Jeder Fall unten liegt in einem eigenen Ordner `ecNN/` mit
seiner `.editorconfig` und seinen Dateien. Vor jedem Fall werden die Ausgangsdateien neu geschrieben
(Skript `reset.sh` im Projekt, das die Dateien aus diesem Plan anlegt). Die Referenz bleibt auf ihren
Vorgaben, mit dem gebündelten EditorConfig-Plugin eingeschaltet. In Ferris IDEA sind die eigenen
Speicheroptionen aus (Settings › Editor › General), damit nur `.editorconfig` wirkt.

**Vergleich Zeichen für Zeichen:** Nach jedem Fall die gespeicherten Dateien beider Projekte
vergleichen: `cmp cmp-ec-ref/ecNN/<datei> cmp-ec-ide/ecNN/<datei>` und bei einem Unterschied
`xxd` beider Dateien. Notiert wird je Fall: gleich / verschieden, und die Bytes, die abweichen.
Tasten werden in beiden IDEs gleich gedrückt: die Datei öffnen, der Caret an die genannte Stelle
(per Mausklick oder ⌘L Zeile:Spalte), dann die Tasten, dann ⌘S.

## EditorConfig-Fälle

| Nr. | `.editorconfig` | Ausgangsdatei (`\n` = LF) | Tasten | Erwartet (Ferris IDEA, SPEC-0137) |
|---|---|---|---|---|
| ec01 | `root = true` · `[*.py]` `indent_style = space` `indent_size = 4` | `a.py`: `def f():\n` | Caret Zeile 2, Tab, `x = 1`, ⌘S | `def f():\n    x = 1` (ohne letzten Umbruch, da nicht verlangt) |
| ec02 | `[Makefile]` `indent_style = tab` `tab_width = 8` | `Makefile`: `all:\n` | Caret Zeile 2, Tab, `echo hi`, ⌘S | `all:\n\techo hi`; Tab-Breite im Editor 8 Spalten (Caret-Spalte nach Tab ansehen) |
| ec03 | `[*.bat]` `end_of_line = crlf` | `run.bat`: `echo a\necho b\r\necho c` (gemischt) | ein Zeichen am Ende tippen, ⌘S | alle Umbrüche `\r\n`: `echo a\r\necho b\r\necho c!` |
| ec04 | `[*]` `end_of_line = lf` | `w.txt`: `a\r\nb\r\n` | `x` am Ende, ⌘S | `a\nb\nx` — Ersetzen der vorhandenen CRLF beim Speichern? |
| ec05 | `[*]` `trim_trailing_whitespace = true` | `t.txt`: `a  \nb\t\nc  ` | Caret ans Ende von Zeile 3, ⌘S | Ferris IDEA: `a\nb\nc  ` (Caret-Zeile behält ihre Leerzeichen); Referenz prüfen, ob sie die Caret-Zeile auch kürzt |
| ec06 | `[*]` `insert_final_newline = true` | `n.txt`: `a` | `b` tippen, ⌘S | `ab\n` |
| ec07 | `[*]` `insert_final_newline = false` | `m.txt`: `a\n\n\n` | `b` am Anfang, ⌘S | Ferris IDEA entfernt die letzten Umbrüche: `ba`; Referenz: entfernt sie oder lässt sie? |
| ec08 | Wurzel: `root = true` · `[*]` `indent_size = 2`; Unterordner `sub/.editorconfig`: `[*]` `indent_size = 8` | `sub/x.js`: `f(\n` | Zeile 2, Tab, `y`, ⌘S | die nähere Datei gewinnt: 8 Leerzeichen |
| ec09 | Wurzel `[*]` `indent_size = 2`; `sub/.editorconfig`: `root = true` · `[*]` `indent_style = tab` | `sub/y.js`: `f(\n` | Zeile 2, Tab | Wurzel wird wegen `root = true` in `sub` nicht gelesen: ein Tab |
| ec10 | `[*]` `indent_size = 3` · `[*.md]` `indent_size = unset` | `r.md`: `- a\n` | Zeile 2, Tab | `unset` gibt die Vorgabe zurück (Ferris IDEA 4 Leerzeichen; Referenz: ihre Markdown-Vorgabe) |
| ec11 | `[{a,b}.txt]` `indent_style = tab` · `[[!c]*.cfg]` `indent_size = 6` · `[f{1..3}.txt]` `indent_size = 5` | `a.txt`, `c1.cfg`, `d1.cfg`, `f2.txt`, `f4.txt`, alle `x\n` | in jeder: Zeile 2, Tab, ⌘S | `a.txt` Tab; `c1.cfg` Vorgabe; `d1.cfg` 6 Leerzeichen; `f2.txt` 5; `f4.txt` Vorgabe |
| ec12 | `[**/gen/*.txt]` `indent_style = tab` | `deep/gen/g.txt` und `gen.txt`, je `x\n` | Zeile 2, Tab, ⌘S | nur `deep/gen/g.txt` mit Tab |
| ec13 | `[*]` `indent_style = space` `indent_size = tab` `tab_width = 3` | `i.txt`: `x\n` | Zeile 2, Tab, ⌘S | `indent_size = tab` heißt Einrückung = Tab-Breite: 3 Leerzeichen |
| ec14 | `[*]` `max_line_length = 40` | `l.txt`: eine Zeile mit 60 Zeichen | ansehen, Reformat (⌥⌘L) | Ferris IDEA meldet den Wert nur (SPEC-0137 Out of scope); Referenz: Randlinie bei 40, bricht Reformat um? → Abweichung festhalten |
| ec15 | `[*]` `charset = latin1` | `c.txt` in Latin-1 mit `ä` (Byte `E4`) | `x` tippen, ⌘S | Ferris IDEA liest und schreibt UTF-8 (Out of scope); Referenz schreibt Latin-1? Bytes vergleichen |
| ec16 | `[*]` `indent_style = tabs` (ungültig) | `bad.txt`: `x\n` | Zeile 2, Tab | Ferris IDEA meldet das Problem (MCP `editorconfig`) und nimmt die Vorgabe; Referenz: Hinweis in der Datei? |
| ec17 | `[*.py]` `indent_size = 4`, offene `a.py` | `a.py`: `def f():\n` | `.editorconfig` auf `indent_size = 2` ändern und speichern, zurück in `a.py`, Zeile 2, Tab, ⌘S | die Änderung wirkt sofort: 2 Leerzeichen |
| ec18 | `[*]` `trim_trailing_whitespace = false`; in Settings „Remove trailing spaces“ an | `k.txt`: `a  \nb` | Caret Zeile 2, ⌘S | `.editorconfig` hat Vorrang vor den IDE-Einstellungen: `a  \nb` bleibt |

Bei jedem Fall außerdem: das Einrückungslabel in der Statusleiste beider IDEs notieren (z. B.
„4 spaces“, „Tab“).

## Übrige Funktionen offener Plugins
1. **Copyright (Handtest 81):** In beiden IDEs dasselbe Profil anlegen. In der Referenz geht das
   über Settings › Editor › Copyright, in Ferris IDEA über `.ferris-idea/copyright.txt` oder dieselben
   `.idea/copyright`-Dateien. Dann Update Copyright in `Main.java`, `tool.py` und `notice.sh` (mit
   Shebang), zweimal. Die Dateien mit `cmp` vergleichen. Ein zweiter Lauf darf nichts ändern.
2. **Properties:** `messages.properties` mit einem doppelten Schlüssel, Leerzeichen am Wertende und
   `ü`. Welche Markierungen und welche ⌥↩-Angebote gibt es? Nach der Korrektur mit `cmp`
   vergleichen.
3. **Grammatik:** Eine README mit „the the“, „an user“, „a apple“ und einem Tippfehler. Was wird
   markiert, mit welchem Vorschlag? Das Ergebnis nach ⌥↩ vergleichen.
4. **Wörterbücher (Handtest 85, mit Netz):** en_GB in beiden laden. „colour“, „color“ und „recieve“
   prüfen, dann auf en_US umstellen.
5. **Projektwörterbuch (Handtest 89):** In Ferris IDEA ein Wort speichern, dann
   `.idea/dictionaries/project.xml` in der Referenz öffnen. Die Referenz muss das Wort kennen. Danach
   umgekehrt.
6. **Resource Bundle Editor (Handtest 152):** Dieselben drei Dateien wie im Fenstertest, dieselben
   Schritte. Danach `git diff` beider Projekte vergleichen (eine Zeile je Änderung, `ü` in einer
   ASCII-Datei).

## Ergebnis
— (nach dem Vergleich in einer Beobachtung)

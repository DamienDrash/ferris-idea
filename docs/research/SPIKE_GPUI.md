# Spike GPUI (M3.4, ADR-0003)

**Stand:** 2026-11-03 · alle fünf Kriterien erfüllt · GPUI 0.2.2 (crates.io, Apache-2.0) · Befehle: `idecraft spike-scroll [datei]`, `idecraft spike-input`

Der Spike prüft die fünf Abnahmekriterien aus [ADR-0003](../adr/0003-ui-toolkit.md), bevor die
Editor-Ansicht (M3.7) darauf aufbaut. Code: `crates/app/src/spike.rs`, `crates/app/src/spike_input.rs` (macOS und Linux).

## Einbindung

- Lizenzen: drei eng gefasste Ausnahmen in `deny.toml` (Freigaben des Projektinhabers, 2026-10-31): `hexf-parse`
  (CC0-1.0, nur Bauzeit), `libfuzzer-sys` (NCSA) und `tiny-keccak` (CC0-1.0, Linux-Backends). Die
  Positivliste selbst ist unverändert.
- Features: macOS `font-kit`, `runtime_shaders` (übersetzt die Metal-Shader beim Start, weil die
  Metal-Toolchain von Xcode auf diesem Mac fehlt); Linux `font-kit`, `x11`, `wayland`.
- Ein gesperrter Bildschirm liefert unter macOS keine Bildtakte; gemessen wird bei entsperrtem Mac.

## Kriterien

| # | Kriterium | Stand | Beleg |
|---|---|---|---|
| 1 | 100 000-Zeilen-Datei scrollt mit Bildwiederholrate (M1-Klasse) | erfüllt | 900 Bilder, 3 Zeilen je Bild: 119,8 fps; Bildabstand p50 8,33 ms, p95 9,21 ms, p99 9,43 ms, max 12,62 ms; internes 120-Hz-Display, Apple M1 Max, 1200×900-Fenster, erzeugte Datei mit Umlauten (2026-10-31) |
| 2 | Tipp-Latenz unter 16 ms in dieser Datei | erfüllt | Editor-Ansicht (`idecraft spike-editor`, SPEC-0058), 100 000 Zeilen Rust mit Syntaxfarben, 220 Tastendrücke in Zeile 50 000 (Text mit Umlauten, Backspace, Pfeil runter, Enter), je einer pro Bild, Zeit vom Tastendruck bis zum fertig gezeichneten Bild: p50 0,88 ms, p95 2,2 ms, p99 12,6 ms; über 16 ms nur der zweite Tastendruck mit 82 ms (Kaltstart: erste Glyphen, erster Hintergrund-Parse), in drei Läufen gleich (M1 Max, 2026-10-31). Hinzu kommt bis zu ein Bildtakt (8,3 ms bei 120 Hz) bis zur Anzeige. Befund: synchrones Neuparsen der ganzen Datei kostete ~400 ms je Taste; der Parse läuft jetzt im Hintergrund. Vorher im Eingabefeld (`spike-input`, mit IME): p50 2,99 ms, p95 13,86 ms |
| 3 | Umlaute, Dead Keys, IME (Japanisch) | erfüllt | Test durch den Projektinhaber am Mac (2026-10-31): `Grüße` mit deutscher Tastatur; `´`+`e` → `é` und `^`+`a` → `â` (Akzent erst als Vorschau, dann ersetzt); Japanisch-Romaji `nihongo` → `に`…`にほんg` → `日本語` umgewandelt und übernommen; Backspace. macOS meldet dabei einmal `error messaging the mach port for IMKCFRunLoopWakeUpReliable` (Systemmeldung des Eingabesystems, ohne Auswirkung) |
| 4 | Zwei Fenster, geteilte Ansichten, Baum, Tabelle mit 100 000 Zeilen | erfüllt | `idecraft spike-layout .` (2026-10-31): zwei Fenster gleichzeitig, je Dateibaum des Repos (bis 381 sichtbare Knoten, alle 40 Bilder ganz auf- und zugeklappt), Textbereich und Tabelle mit 100 000 Zeilen und 5 Spalten, beide Trenner bewegen sich jedes Bild. 600 Bilder je Fenster: links 119,8 fps, Bildabstand p50 8,33 / p95 12,88 / p99 13,59 / max 14,78 ms; rechts 119,6 fps, p50 8,33 / p95 13,46 / p99 14,53 / max 17,67 ms. Kein Bild über 16,7 ms (60-Hz-Grenze) außer einem mit 17,67 ms im rechten Fenster. Handtest durch den Projektinhaber mit `idecraft spike-layout --interactive .` (2026-10-31): Ordner anklicken, Trenner ziehen, Baum und Tabelle scrollen liefen ohne Auffälligkeiten |
| 5 | Baut auf macOS und Linux in CI | erfüllt | macOS: Job „fmt, clippy, test, docs, deny (own Mac runner)“ auf dem eigenen Runner `mac-idecraft` baut und prüft bei jedem Push und PR alle GPUI-Crates (`scripts/ci-local.sh`: clippy, alle Tests, Start des GPUI-Programms ohne Fenster); Fenster-Selbsttest und die Spike-Messungen 1, 2 und 4 mit Release-Build laufen nachts und auf Anforderung (`scripts/ci-window.sh`, überspringt sich tagsüber, bei Nutzung des Macs und bei gesperrtem Bildschirm; 2026-11-03). Linux: Job „fmt, clippy, test (ubuntu)“ baut die Oberfläche mit X11/Wayland, zuletzt grün am 2026-11-01 ([Lauf](https://github.com/DamienDrash/idecraft/actions/runs/41290393412/job/124800245907), Commit `34a55eb`); seit dem Ausgabenlimit nur mit `HOSTED_RUNNERS=true` |

Die Messung zeigt Bildtakt und den Aufbau der Elemente (`render_ms` p50 0,006 ms); die GPU-Arbeit
steckt im Bildabstand. Sie ist ein Spike-Ergebnis für die Wahl des Toolkits, kein Vergleich mit der
Referenz.

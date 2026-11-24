# Feature-Matrix

**Stand:** 2026-10-31 · Referenz: IntelliJ IDEA 2026.2.4 (Build 262.11879.100) inkl. Ultimate-Funktionen · Marken nur beschreibend.

Spalten wie im Projektauftrag. Funktionsbezeichnungen sind gängige Fachbegriffe,
das Verhalten ist in eigenen Worten und auf Fachebene beschrieben (Quelltyp
`GEN`/`DOC`, noch keine eigene Beobachtung).

**Methode (zulässige Untersuchung):** `GEN` Fachwissen · `STD` offener Standard · `OSS` offener Quelltext (Apache-2.0, Port erlaubt nach `docs/legal/APACHE_PORTING.md`) ·
`DOC` öffentliche Doku, nur Fakten · `OBS` Black-Box in der Testphase (nach GO-1) ·
`OBS-F` Black-Box im kostenlosen Modus, auch nach der Testphase.

**Rechtlicher Status der Analyse:** 🟢 gesichert zulässig · 🟡 vertretbar (§ 69d Abs. 3 UrhG, nach GO-1) ·
🟠 ungeklärt, nur Alternative · 🔴 ausgeschlossen. Begründung: Rechtsanalyse, Abschnitt 9.

**Rust-Status:** — offen · S spezifiziert · I implementiert (mit Tests) · T teilweise.

## Kern, Workspace, Editor

| ID | Referenzfunktion | Beobachtbares Verhalten | Methode | Rust | Tests | Abweichungen | Recht |
|---|---|---|---|---|---|---|---|
| F-001 | Ordner als Projekt öffnen | Verzeichnis wird Projekt, Dateibaum ohne Build-/VCS-Ordner | GEN, OBS-F | I (SPEC-0001) | 4 | keine `.gitignore`-Auswertung | 🟢 |
| F-002 | Einstellungen IDE/Projekt | globale und projektbezogene Einstellungen, Projekt hat Vorrang | GEN | I (SPEC-0002) | 5 | eigenes TOML-Format statt XML | 🟢 |
| F-003 | Projektmodell aus Build-Dateien (Maven, Gradle, npm, pip) | Module, Abhängigkeiten, SDK aus Build-Beschreibung | STD (Build-Tools), DOC, OBS-F, OSS | I (SPEC-0048 v3, SPEC-0100 v2, SPEC-0149 v2) | 6 | Seitenvergleich OBS-0024; statisch gelesen: Cargo, npm, Python, Maven (Parent-Kette, Properties, dependencyManagement), Go, Gradle (literale Abhängigkeiten, Versionskatalog); gesperrte Versionen aus Lockfiles; transitiv für Cargo/npm aus Lockfiles, für Maven/Gradle über das Build-Werkzeug (SPEC-0149) | 🟢 |
| F-004 | Projektdateien der Referenz-IDE lesen | Ordner mit IDE-Metadaten wird als Projekt erkannt | OSS (Plattform, Apache-2.0), OBS-F | L (SPEC-0049) | 5 | nur lesen; `.ipr`, Run-Konfigurationen, Facetten nicht; `workspace.xml` und Datenbank-Dateien bewusst nicht geöffnet | 🟢 |
| F-010 | Texteditor-Grundfunktionen | Einfügen, Löschen, Ersetzen, Unicode-sicher | GEN | I (SPEC-0003, SPEC-0056, SPEC-0057, SPEC-0058) | 12 | Bearbeitungsbefehle (Duplizieren, Zeilen löschen/verschieben/verbinden, Kommentar, Einrücken, Enter, Klammern) und 64 Editor-Befehle je Keymap-Aktion (Bewegung, Smart Home, Seiten, Auswahlstufen, Zwischenablage mit ganzen Zeilen, Undo); Editor-Ansicht auf GPUI (SPEC-0058, Handtest offen) | 🟢 |
| F-011 | Undo/Redo mit Gruppen | Refactoring wird als ein Schritt rückgängig gemacht | GEN | I (SPEC-0003) | 5 | — | 🟢 |
| F-012 | Mehrfach-Cursor, Spaltenauswahl | mehrere Carets editieren gleichzeitig | GEN, OBS-F, OSS | L (SPEC-0025) | 7 | — | 🟢 |
| F-013 | Syntaxhervorhebung | Färbung nach Sprache, inkrementell | GEN, STD (Tree-sitter), OSS | I (SPEC-0027, SPEC-0060) | 5 | Farben aus dem Editor-Schema der Referenz | 🟢 |
| F-014 | Code-Folding | Blöcke ein-/ausklappen | GEN, OSS | L (SPEC-0030), I (SPEC-0083: im Editor) | 4 | — | 🟢 |
| F-015 | Suchen/Ersetzen, Regex, in Dateien | Suche in Datei und Projekt | GEN, OSS | L (SPEC-0029), I (SPEC-0065: Such- und Ersetzleiste; SPEC-0066: Find in Files) | 6 | Regex ohne Look-around (lineare Laufzeit), Vorschau vor dem Schreiben | 🟢 |
| F-016 | „Überall suchen“ / Dateien, Klassen, Symbole, Aktionen per Name | ein Eingabefeld findet Dateien, Symbole und Befehle | GEN, DOC, OBS-F, OSS | L (SPEC-0031), I (SPEC-0067: Popup) | 4 | — | 🟢 |
| F-017 | Tastenkürzel-Schema | Standardbelegung | GEN | L (SPEC-0028) | 5 | Belegung der Referenz übernommen (F-018) | 🟢 |
| F-018 | Kompatible Tastenkürzel-Belegung der Referenz | identische Belegung, Standard unter macOS/Windows/Linux | OSS (`$default.xml`, `Mac OS X 10.5+.xml`, Apache-2.0) | L (SPEC-0028) | 5 | — | 🟢 (ADR-0009) |
| F-019 | Lokale Historie | Dateiversionen unabhängig von VCS wiederherstellen | GEN, DOC, OBS-F, OSS | I (SPEC-0044, SPEC-0085: Ansicht) | 3 | im Projekt statt im IDE-Cache, 30 Tage, Wiederherstellen rückgängig machbar (IMP-0021) | 🟢 |
| F-020 | Editor-Tabs, geteilte Ansichten | mehrere Dateien, Split horizontal/vertikal | GEN, OSS | I (SPEC-0059: Tabs, Projektbaum) | 5 | eigene Gestaltung; Split offen | 🟢 |

## Sprachen

| ID | Referenzfunktion | Beobachtbares Verhalten | Methode | Rust | Tests | Abweichungen | Recht |
|---|---|---|---|---|---|---|---|
| F-030 | Sprache einer Datei erkennen | Endung → Sprache | GEN, STD (LSP) | I (SPEC-0004) | 3 | — | 🟢 |
| F-031 | Codevervollständigung | kontextabhängige Vorschläge | STD (LSP) | L (SPEC-0024), I (SPEC-0072: Liste im Editor; SPEC-0080: Parameter-Info ⌘P) | 4 | Qualität je Server | 🟢 |
| F-032 | Diagnosen / Inspektionen | Fehler und Warnungen beim Tippen | STD (LSP), OBS, OSS | L (SPEC-0023), I (SPEC-0070: im Editor; SPEC-0078: F2/⇧F2) | 7 | Inspektionsumfang der Referenz nicht erreichbar über LSP allein | 🟢 |
| F-033 | Quick-Fixes | Korrekturvorschläge an Fehlern | STD (LSP code actions), OBS, OSS | L (SPEC-0041), I (SPEC-0076: ⌥↩) | 6 | Diagnosen des Servers als Kontext | 🟢 |
| F-034 | Gehe zu Definition / Verwendungen | Navigation über Symbole | STD (LSP) | L (SPEC-0024), I (SPEC-0071: ⌘B, Zurück/Vor; SPEC-0073: Verwendungen; SPEC-0077: Schnelldokumentation; SPEC-0079: Dateistruktur; SPEC-0081: Implementierung, Typdeklaration; SPEC-0082: Aufrufhierarchie; SPEC-0292 v8: Verwendungszahlen an Deklarationen, Klick öffnet Find Usages) | 4 | — | 🟢 |
| F-035 | Python-Unterstützung | Interpreter, venv, Tests, Debug | STD, DOC, OBS-F, OSS | L (SPEC-0050) | 6 | Interpreter, venv, Pakete, Anforderungen, Testkonfiguration statisch (IMP-0025); Sprachfunktionen mit pyright (SPEC-0042 v3); offen: Debug (debugpy), Pakete installieren | 🟢 |
| F-036 | JavaScript/TypeScript, Frameworks | Node, npm, Frameworks (React, Angular, Vue) | STD, DOC, OBS | L (SPEC-0051), I (SPEC-0091 v4, SPEC-0123, SPEC-0133, SPEC-0141 v2, SPEC-0168) | 6 | Node-Laufzeit, Paketmanager, Pakete, Testkonfiguration statisch (IMP-0026); Sprachfunktionen mit typescript-language-server (SPEC-0042 v3); Frameworks: Erkennung mit Dev-Server/Browser (SPEC-0133), Vue/Svelte/Angular-Server und Blockfarben (SPEC-0141 v2); Debug: Node-Skripte, npm-Skripte mit Kindprozessen, TypeScript ohne Build (SPEC-0168), Seiten in Chrome; offen: Vorlagen-Grammatik für Vue/Svelte, Handtests | 🟡 |
| F-037 | Java/Kotlin | Build, Run, Navigation, Inspektionen | STD (JDT LS), OSS (`java/`, `plugins/kotlin`) | L (SPEC-0052) | 5 | JDK-Erkennung und Java-Version des Projekts statisch (IMP-0027); Sprachfunktionen mit jdtls (SPEC-0042 v3); Build, Run, Debug (SPEC-0097, SPEC-0122), Inspektionen (SPEC-0125, SPEC-0139), Kotlin (SPEC-0132, SPEC-0140, Bedingungen beim Debuggen SPEC-0178); offen: Handtests | 🟢 |
| F-040 | Spring, Jakarta EE, Micronaut, Quarkus | Bean-Navigation, Konfigurationshilfe | DOC, OBS | — | — | proprietär → clean room | 🟡 |
| F-038 | PHP, Go, Ruby, C/C++, HTML, CSS, Rust | Sprachdienste über LSP | STD | L (SPEC-0042: Server-Register; geprüft mit clangd, rust-analyzer, gopls, intelephense, HTML/CSS/JSON-Servern) | 6 | fehlende Server mit Grund und Installationshinweis (IMP-0019); XML, YAML, Markdown, TOML, Shell, Dockerfile, Groovy, Scala, SCSS, Less, Swift, Lua, R (SPEC-0042 v4); Ruby mit Ruby-Versionsverwaltern und Bundler, ruby-lsp/solargraph geprüft (SPEC-0185, SPEC-0042 v6); Syntaxfarben für Ruby und PHP (SPEC-0027 v5) | 🟢 |
| F-039 | Code-Formatierung | Formatieren nach Stilregeln | STD (LSP, externe Formatter), OSS | L (SPEC-0043), I (SPEC-0075: ⌥⌘L) | 6 | eigene Regeln, keine Übernahme der Referenz-Codestile; Hinweis bei Änderungen über Leerraum hinaus (IMP-0020) | 🟢 |

## Datenbank (Umfang vergleichbar mit DataGrip)

| ID | Referenzfunktion | Beobachtbares Verhalten | Methode | Rust | Tests | Abweichungen | Recht |
|---|---|---|---|---|---|---|---|
| F-050 | SQL-Skript in Anweisungen zerlegen | aktuelle Anweisung oder ganzes Skript ausführen | GEN, STD | I (SPEC-0005) | 9 | kein `DELIMITER`, kein `GO` | 🟢 |
| F-051 | Verbindungen verwalten | Profile, Treiber, SSH-Tunnel, SSL | GEN, DOC, OBS | I (SPEC-0008) | 5 | Zugangsdaten nur im System-Schlüsselbund; SSH-Tunnel offen, TLS nach SPEC-0016 | 🟡 |
| F-052 | Schema-Introspektion | Baum: Schemas, Tabellen, Spalten, Indizes, Schlüssel | STD (information_schema, Kataloge) | I (SPEC-0013) | 5 | — | 🟢 |
| F-053 | SQL-Editor mit Schema-Vervollständigung | Tabellen-/Spaltennamen aus live Schema | GEN, OBS | L (SPEC-0014) | 5 | — | 🟡 |
| F-054 | Ergebnisraster | sortieren, filtern, paginieren, exportieren | GEN, OBS, OSS | L (SPEC-0007, SPEC-0021) | 15 | stabile Sortierung, Wertübersicht mit Umfang, Export unterscheidet NULL und Leerstring | 🟢 |
| F-055 | Daten bearbeiten | Zellen ändern, Zeilen hinzufügen/löschen, Übertragung als Transaktion | GEN, OBS | L (SPEC-0017) | 11 | Konfliktprüfung gegen geladene Werte, alles oder nichts (IMP-0014) | 🟡 |
| F-056 | ER-Diagramme | Tabellen und Fremdschlüssel als Diagramm | GEN, OBS | L (SPEC-0022) | 6 | eigene Darstellung; Textexport Mermaid/DOT, Verweise außerhalb der Auswahl gelistet | 🟡 |
| F-057 | Datenbank-Refactorings | Umbenennen von Tabelle/Spalte inkl. abhängiger SQL-Stellen | GEN, OBS | L (SPEC-0019) | 7 | mehrdeutige Stellen werden gemeldet statt geraten | 🟡 |
| F-058 | Schema-Vergleich, DDL-Erzeugung | Unterschiede zweier Schemas als DDL | GEN, OBS | L (SPEC-0018) | 11 | Indizes/Fremdschlüssel nach Inhalt verglichen; Datenverlust je Anweisung markiert | 🟡 |
| F-059 | Abfrageplan anzeigen | EXPLAIN grafisch | STD (EXPLAIN je DB) | L (SPEC-0020) | 7 | Hinweise (volle Scans, Fehlschätzungen); gemessene Änderungen werden zurückgerollt | 🟢 |
| F-060 | Weitere Dialekte (Oracle, SQL Server, …) | wie F-051–F-059 | STD | — | — | Treiberlage in Rust offen | 🟢 |

## Erweiterte Funktionen

| ID | Referenzfunktion | Beobachtbares Verhalten | Methode | Rust | Tests | Abweichungen | Recht |
|---|---|---|---|---|---|---|---|
| F-070 | Debugger: Haltepunkte, Schritte, Variablen | Programm anhalten, Zustand ansehen | STD (DAP), OSS | L (SPEC-0039, SPEC-0054, SPEC-0194: Pause, Run to Cursor, Set Value) | 8 | Oberfläche mit Attach to Process (SPEC-0147); per MCP mit Recht `debug` | 🟢 |
| F-071 | Bedingte Haltepunkte, Ausdrücke auswerten | — | STD (DAP), OSS | L (SPEC-0039 v2) | 6 | nicht unterstützte Funktionen werden abgelehnt statt ignoriert (IMP-0022) | 🟢 |
| F-072 | Umbenennen | alle Verwendungen anpassen | STD (LSP rename), OSS | L (SPEC-0040), I (SPEC-0074: ⇧F6) | 6 | Vorschau mit Prüfung auf zwischenzeitliche Änderungen (IMP-0017); Kommentare, Strings, Textdateien (SPEC-0148); ein Undo über alle Dateien (SPEC-0193) | 🟢 |
| F-073 | Methode/Variable extrahieren, inline | — | STD (LSP code actions), OBS, OSS | L (SPEC-0041 v2: Code Actions inkl. Quick Fixes und `codeAction/resolve`), I (SPEC-0076 v3: ⌥↩ mit Vorschau) | 6 | je Server; Befehls-Refactorings mit Vorschau (IMP-0018) | 🟢 |
| F-074 | Git: Status, Diff, Commit, Branches, Log, Blame | — | STD (Git), OSS | L (SPEC-0032: Status, Log, Diff; SPEC-0033: Blame, Branches; SPEC-0034: Commit, Branch anlegen; SPEC-0035: Branch wechseln; SPEC-0036: Merge; SPEC-0055: Remotes, Fetch, Pull, Push; SPEC-0063: Commit-Werkzeugfenster; SPEC-0064: Änderungsmarker im Gutter; SPEC-0068: Branch-Popup; SPEC-0084: Diff-Ansicht; SPEC-0086: Git-Log-Werkzeugfenster; SPEC-0106/0145: Stash, Cherry-Pick, Rebase; SPEC-0230: Commit-Hooks; SPEC-0231: Smart Checkout) | 6 | Konflikte über F-075; Smart Checkout über benannten Stash statt Shelf (IMP-0080); nie hängende Passwortabfragen, Force-Push nur mit Lease (IMP-0029) | 🟢 |
| F-075 | Merge-Konflikte lösen (Drei-Wege) | — | GEN, OSS | L (SPEC-0037: Chunks, Auflösung, Marker wie `git merge-file`; SPEC-0101: Merge-Ansicht) | 6 | Merge-Ansicht (SPEC-0101 v3) mit Konfliktzustand im Repository, einfachen Konflikten (SPEC-0037 v2) und Abschluss des Vorgangs | 🟢 |
| F-076 | Integriertes Terminal | Shell im Projektordner | GEN, OSS | I (SPEC-0038: PTY-Sitzung, Bildschirmmodell; SPEC-0062: Werkzeugfenster und Tabs; SPEC-0118: Maus; SPEC-0153/0161: Links; SPEC-0205: Sitzungen, Teilen, Wiederherstellen; SPEC-0206: Suche, Shell-Integration; SPEC-0207: MCP `terminal_command`) | 6 | Shell-Integration nur zsh/bash; MCP-Befehle laufen ohne Fenster; nicht neben der Referenz verglichen | 🟢 |
| F-077 | Run-Konfigurationen | Programme/Tests mit Parametern starten | GEN, OBS-F, OSS | L (SPEC-0047, SPEC-0087) | 6 | eigenes Format (TOML); Prüfung vor dem Start (IMP-0023); Start per MCP mit Recht `run` (SPEC-0053); Run-Widget und Run-Werkzeugfenster mit Live-Ausgabe (SPEC-0087) | 🟢 |
| F-078 | Test-Runner mit Ergebnisbaum | — | GEN, STD (Test-Reporter-Formate), OSS | L (SPEC-0045: Berichte JUnit/TAP/libtest → Baum; SPEC-0088: Ergebnisbaum im Run-Fenster) | 6 | Tests starten über Run-Konfigurationen (SPEC-0047); Baum erscheint am Ende des Laufs | 🟢 |
| F-079 | HTTP-Client (Anfragedateien) | Anfragen aus Textdateien senden | DOC, OBS, STD, OSS | L (SPEC-0046) | 6 | offenes `.http`-Format; ungelöste Variablen werden nie gesendet; Senden per MCP mit Recht `http` (SPEC-0053); Antwortansicht und Vergleich (Block 4) offen | 🟡 |
| F-080 | Plugin-System | Erweiterungen installieren, Sandbox | GEN, OSS | — | — | eigene API (WASM); keine Kompatibilität zu Plattform-Plugins geplant | 🟢 |
| F-081 | Funktionen offener Plattform-Plugins übernehmen | Logik offener Plugins (Apache-2.0) nach Rust portiert | OSS | — | — | JVM-Plugins laufen nicht direkt; Port nötig | 🟢 (ADR-0006) |
| F-082 | KI-Assistenz | optional, eigene Anbieterwahl | GEN | — | — | Referenz-KI nicht untersuchen (eigene Bedingungen) | 🟢 / 🔴 für Referenz-KI |
| F-083 | Profiler, Docker, Kubernetes, Remote-Entwicklung | — | DOC, OBS | — | — | spätere Phase | 🟡 |

## Plattform-Eigenschaften (Zusatzauftrag 2026-10-30)

| ID | Referenzfunktion | Beobachtbares Verhalten | Methode | Rust | Tests | Abweichungen | Recht |
|---|---|---|---|---|---|---|---|
| F-084 | MCP-Server für LLM-Agenten | Agenten nutzen IDE-Funktionen als Werkzeuge | STD (MCP), OSS (`plugins/mcp-server`, Logik-Vorlage) | I (SPEC-0006, SPEC-0053) | 4 + Stdio-Probe | 60+ Werkzeuge; Ausführen/HTTP nur mit Recht pro Arbeitsbereich außerhalb des Projekts, mit Protokoll (IMP-0028) | 🟢 |
| F-085 | Themes Darcula, Light, New UI, Islands | Farben und Flächen wie Referenz | OSS (`platform-resources/src/themes`) | I (SPEC-0060: New UI Dark/Light mit Editor-Schemata) | 5 | Islands und Umschalten offen | 🟢 (ADR-0009) |
| F-086 | Icon-Satz New UI | gleiche Icons in offenen Bereichen | OSS (`platform/icons`, `expui`) | — | — | proprietäre Bereiche: eigene Icons im Stil | 🟢 / 🔴 für proprietäre Icons |
| F-087 | Fensteraufbau (Werkzeugleisten, Tool-Fenster, Statusleiste) und Metriken | Anordnung, Abstände, Schriftgrößen | OSS (Jewel), OBS | I (SPEC-0061: Islands, Hauptwerkzeugleiste, Streifen, Statusleiste; SPEC-0294: Tooltips mit Tastenkürzel der aktiven Belegung) | 5 | eigenes App-Icon und Name; nur vorhandene Tool-Fenster im Streifen | 🟢 |
| F-088 | Keine Testphase, keine Sperren, keine Telemetrie | — | GEN | I (es gibt keinen Lizenzcode) | — | bewusste Abweichung | 🟢 (ADR-0007) |
| F-089 | Gemeinsames Arbeiten (Code With Me) | Gäste arbeiten live im Projekt des Gastgebers: Einladung, Anwesenheit, Folgen, Rechte je Gast, geteiltes Terminal, Ausführen und Debuggen | DOC, OBS (Beobachtung ausstehend) | — | — | geplant M6.15, baut auf Sitzungsprotokoll (ADR-0026) und Kopplung (ADR-0025); Anmeldung bei Diensten der Referenz nur durch den Projektinhaber | 🟡 |

## Ausgeschlossen

| ID | Was | Grund | Recht |
|---|---|---|---|
| X-001 | Logos, App-Icon, Splash, Produktnamen; Icons und Grafiken proprietärer Bereiche | Marke (Apache-2.0 Ziff. 6), Gestaltung geschützt (EuGH C-393/09) | 🔴 |
| X-002 | Hilfetexte, Dialogtexte, Fehlermeldungen proprietärer Teile im Wortlaut | Sprachwerk | 🔴 |
| X-003 | Interna aus der Installation (JARs, Bytecode, Ressourcen) | § 69e nicht erfüllt; Ziff. 3.5(b) | 🔴 |

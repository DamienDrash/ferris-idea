# Forschungsgrenzen · Ferris IDEA

**Stand:** 2026-10-30 (Nachtrag nach anwaltlicher Beratung) · verbindlich für alle Beitragenden und Agenten · Begründung: Rechtsanalyse

Dieses Dokument ist die operative Fassung der Rechtsanalyse: Was darf wer, wann,
womit. Bei Zweifel gilt: **nicht tun, Frage in die Statusdatei eintragen,
unabhängige Arbeit fortsetzen.**

## 1. Quelltypen

Jede Spezifikation nennt für jede Aussage ihren Quelltyp. Erlaubt sind nur diese:

| Code | Quelltyp | Beispiele | Freigabe |
|---|---|---|---|
| `GEN` | Allgemeines Fachwissen, eigene Erfahrung, Lehrbücher | „Ein Editor hat Undo/Redo“ | frei |
| `STD` | Offene Standards und Protokolle | LSP, DAP, SQL-Standard, Git-Format, EditorConfig, Unicode | frei |
| `OSS` | Quellcode/Doku anderer Open-Source-Projekte unter ihrer Lizenz | Tree-sitter-Grammatiken, `JetBrains/intellij-community` (Apache-2.0) | lesen und übernehmen/portieren **mit Lizenzpflichten**: [APACHE_PORTING.md](APACHE_PORTING.md), Eintrag in [PROVENANCE.md](PROVENANCE.md) |
| `DOC` | Öffentliche Herstellerdokumentation, Produktseiten, Videos | jetbrains.com/help | frei lesen; **nur Fakten in eigenen Worten**, keine Zitate über einen Satz, keine Bilder |
| `OBS` | Eigene Black-Box-Beobachtung der Referenzinstallation | Protokoll `OBS-0001` | nur nach **GO-1** und nur in den Formen aus Abschnitt 3 |

Verboten als Quelle: dekompilierter Code, Inhalte aus JAR-/App-Archiven der
Referenzinstallation, geleakte oder vertrauliche Unterlagen, Inhalte hinter
Anmeldung, die nicht für Nutzer bestimmt sind, Kopien fremder Screenshots.

## 2. Rollen und Trennung (Clean Room)

| Rolle | Darf sehen | Liefert | Ablage |
|---|---|---|---|
| **Beobachter** (Phase A) | Referenzinstallation, `DOC`, eigene Testprojekte | Beobachtungsprotokolle `OBS-*` | `docs/research/observations/` |
| **Spezifizierer** (Phase B) | Protokolle, `GEN`, `STD`, `DOC`, `OSS` | neutrale Spezifikation `SPEC-*` | `docs/specifications/` |
| **Implementierer** (Phase C) | **nur** Spezifikationen, `GEN`, `STD`, Rust-Doku | Code und Tests | `crates/`, `tests/` |
| **Prüfer** | alles im Repository | Review-Vermerk im Pull Request | PR-Beschreibung |

Ein einzelner KI-Agent darf mehrere Rollen übernehmen. Dann gilt:

1. Rollenwechsel wird im Commit sichtbar: Commits tragen das Präfix
   `obs:`, `spec:` oder `impl:` und berühren nur die Ablage ihrer Rolle.
   Ein Commit mischt nie Beobachtung und Implementierung.
2. Der Implementierer arbeitet ausschließlich gegen die Spezifikations-ID;
   jedes Modul nennt `SPEC-xxxx` im Modul-Kommentar, jedes Akzeptanzkriterium
   (`SPEC-xxxx/AK-n`) ist in `docs/specifications/TRACEABILITY.md` einem Test
   zugeordnet.
3. In Implementierungs-Prompts und -Sitzungen werden keine Protokolle mit
   Rohbeobachtungen, keine Screenshots und kein Code der Referenz eingefügt —
   nur die Spezifikation.
4. Auffällige Übereinstimmungen mit fremdem Code (gleiche interne Bezeichner,
   Kommentare, Struktur) werden im Review markiert und neu geschrieben.

## 3. Erlaubte Beobachtung (Phase A, nach GO-1)

Erlaubt, weil Nutzung im Rahmen der Lizenz (§ 69d Abs. 3 UrhG):

- Bedienen der IDE über Oberfläche, Menüs, Dialoge, Tastenkürzel.
- Fernsteuerung der Oberfläche mit dem offenen UI-Testwerkzeug Remote Robot
  (`JetBrains/intellij-ui-test-robot`, Apache-2.0; Entscheidung des Projektinhabers
  2026-11-05): Tastatur, Maus, Hover, Rechtsklick, Bildschirmfotos und die
  sichtbaren Eigenschaften der Oberfläche (Text, Tooltip, Größe, Lage,
  Zustand) für Handtests und den Seite-an-Seite-Vergleich. Nur Verhalten und
  Aufbau beobachten; kein Lesen oder Übernehmen von Code, Klassen oder Daten
  der Installation, kein Zugriff auf Lizenz- oder Testphasendaten.
  Werkzeuge: `robot.sh` der Referenzumgebung, `irobot.py`.
- **Ergänzung 2026-11-07:** Fernsteuerung mit JetBrains' Driver (offen in
  `JetBrains/intellij-community`, `platform/remote-driver`, Apache-2.0). Der Projektinhaber auf die
  Frage, ob er statt des Remote Robots genutzt werden soll: „ja wenn das robuster ist dann
  lieber das nutzen.“ Grenzen:
  - Keine neue Installation und kein eigener Konfigurationsordner (keine neue Testphase,
    keine Anmeldung). Die IDE-Seite ist das mitgelieferte Plugin „Performance Testing“;
    eingeschaltet wird nur der JMX-Zugang der JVM über Startoptionen in der bestehenden
    `idea.vmoptions` — nur `127.0.0.1`, mit Passwort, mit Deserialisierungsfilter. Das ist
    der eingebaute Verwaltungsdienst der JVM, kein Java-Agent mit Instrumentierung im Sinn von
    Abschnitt 4.
  - Erlaubt ist nur Bedienen und Lesen dessen, was die Oberfläche zeigt: Komponentenbaum mit
    sichtbaren Eigenschaften (Klassenname als Locator, Text, Name, Tooltip, Zustand), Aktionen
    per ID auslösen, Tasten, Maus, Text, Zustand von Editor (Text, Caret, Auswahl) und
    Fenstern, Bildschirmfotos.
  - Verboten: die freie Aufrufschnittstelle des Drivers für Daten, die der Nutzer nicht sieht
    (Dienste, PSI, Indizes, Modelle, Einstellungen, Lizenz- und Testphasendaten), und die
    Plattform-MBeans der JVM (Threads, Heap, Diagnose; fällt unter Abschnitt 4). Der Client
    `idriver` bietet deshalb keinen freien Aufruf; neue Befehle halten sich an diese Liste.
    Der Komponentenbaum, den die IDE liefert, enthält auch gespiegelte private Felder der
    Komponenten (Aktionsobjekte, Symbolnamen, Schlüssel der Textbündel); `idriver` verwirft
    sie vor jeder Suche und Ausgabe und behält nur eine Positivliste sichtbarer Eigenschaften.
  Werkzeuge: `driver.sh` der Referenzumgebung, `idriver`, `IDriver.java`; der Robot bleibt
  als Rückfall installiert.
- Eigene Testprojekte aus `tests/reference-projects/` öffnen, bearbeiten,
  ausführen, refaktorieren, debuggen.
- Eigene Datenbanken (lokal, Docker) verbinden und abfragen.
- Dateien ansehen, die die IDE in **eigenen** Projektordnern erzeugt.
- Messen mit Betriebssystem-Werkzeugen von außen: Uhrzeit, `ps`, `top`,
  Aktivitätsanzeige, `/usr/bin/time`.
- Netzwerkverkehr **nur** zwischen IDE und eigenen Diensten (eigene DB, eigener
  Git-Server) beobachten; Verkehr zu JetBrains-Servern nicht mitschneiden.

## 4. Verbotene Handlungen (immer)

- Dekompilieren, Disassemblieren, Bytecode-Viewer, Entpacken von JARs oder
  `.app`-Inhalten der Referenzinstallation, Lesen interner Ressourcen.
- Debugger, Profiler, Java-Agenten, JFR, `jstack`/`jmap` am IDE-Prozess.
- Testphase zurücksetzen, Uhr verstellen, zweites Konto, Lizenz-Emulation,
  Patchen, Mitschneiden der Lizenzkommunikation.
- Ultimate-Funktionen nach Ablauf der Testphase nutzen.
- Code, Icons, Grafiken, Farbschemata, Hilfetexte oder Fehlermeldungstexte
  aus **proprietären** Teilen oder aus der Installation ins Repository
  übernehmen. (Offene Teile aus `intellij-community` dürfen nach
  APACHE_PORTING.md übernommen werden; Schriften unter OFL-1.1 unverändert.)
- Logos, Produktnamen oder Produkt-Icons von JetBrains im eigenen Produkt.
- Screenshots oder Bildschirmaufnahmen ins Repository committen. (Lokal als
  Gedächtnisstütze unter `evidence/` erlaubt; der Ordner ist in `.gitignore`.)
- Zugang zur Referenzinstallation an Dritte geben.
- Feedback an JetBrains senden, das Projektinhalte enthält (Ziff. 4.3 User
  Agreement: unwiderrufliche Nutzungsrechte für JetBrains).
- KI-Funktionen der Referenz-IDE nutzen, solange deren eigene Bedingungen
  nicht geprüft sind.
- Plugins aus dem Marketplace installieren, ohne deren Lizenz zu prüfen.

## 5. Schreibregeln für Protokolle und Spezifikationen

- Eigene Worte. Keine Übernahme von Menünamen-Hierarchien als Ganzes, keine
  Dialogtexte, keine Fehlermeldungen im Wortlaut. Einzelne Funktionsnamen, die
  zugleich gängige Fachbegriffe sind („Rename“, „Extract Method“), sind erlaubt.
- Verhalten beschreiben, nicht Gestaltung: „Nach Umbenennen werden alle
  Verwendungen angepasst, auch in Kommentaren nur auf Nachfrage“ — nicht
  „Dialog mit drei Checkboxen oben links“.
- Jede Beobachtung nennt Version, Build, Betriebssystem, Datum, Testprojekt,
  Schritte, Ergebnis, Abweichungen.
- Marken (IntelliJ, DataGrip, JetBrains) nur beschreibend zur Benennung der
  Referenz.

## 5a. Look and Feel

- Offene Themes, Icons, Keymaps und Jewel-Metriken: übernehmen nach APACHE_PORTING.md.
- Proprietäre Bereiche: gleiche Bedienlogik und Anordnung erlaubt; Icons und
  Texte selbst gestalten, im Stil der offenen New-UI-Icons, nie nachzeichnen.
- Eigener Name, eigenes App-Icon, eigener Startbildschirm.

## 6. Freigaben

| ID | Freigabe | Wer | Wirkung |
|---|---|---|---|
| GO-1 | Beginn Phase A mit Ultimate-Funktionen | Projektinhaber | **erteilt 2026-10-30** nach anwaltlicher Beratung |
| GO-2 | Herunterladen/Installieren (= Vertragsschluss) und Start der Testphase mit eigenem Konto | Projektinhaber | **erteilt 2026-10-30**; Anmeldung selbst durch den Projektinhaber |
| GO-3 | Lizenz des Projekts | Projektinhaber | **erteilt 2026-10-30**, am 2026-11-14 ersetzt durch PolyForm Shield 1.0.0 ([ADR-0032](../adr/0032-source-available-licence.md)), am 2026-11-25 durch Apache-2.0 ([ADR-0038](../adr/0038-open-source-licence-apache-2-0.md), Entscheidung des Projektinhabers); Veröffentlichung braucht eine eigene Freigabe (GO-4) |
| GO-4 | Veröffentlichung des Repositorys | Projektinhaber | offen |

Der Stand der Freigaben steht ausschließlich in der Statusdatei.

## 7. Eskalation

Erscheint eine Handlung rechtlich unklar:

1. Handlung konkret benennen.
2. Einschlägige Norm oder Vertragsziffer nennen.
3. Unsicherheit beschreiben.
4. Unbedenklichere technische Alternative vorschlagen.
5. Als offene Frage in die Statusdatei eintragen und unabhängige Arbeit fortsetzen.

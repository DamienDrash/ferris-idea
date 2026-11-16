# SPEC-0336: File properties — encoding, line separators, read-only

- **Version:** 1 · **Date:** 2026-11-14
- **Sources:** OSS — `JetBrains/intellij-community` at the pinned commit `d4018fb` of
  APACHE_PORTING.md (PROVENANCE.md: logic template, no code):
  `platform/platform-impl/resources/idea/PlatformActions.xml` (the main-menu group of the file
  properties, its place in File and the order of its items),
  `platform/platform-impl/resources/intellij.platform.ide.impl.actions.xml` (the action ids),
  `platform/platform-resources-en/src/messages/ActionsBundle.properties` and
  `platform/ide-core/resources/messages/ApplicationBundle.properties` (action and group texts, the
  names of the three line separators),
  `platform/editor-ui-ex/src/com/intellij/codeStyle/AbstractConvertLineSeparatorsAction.java` with
  `platform/platform-impl/src/com/intellij/codeStyle/ConvertToUnixLineSeparatorsAction.java` (a
  separator action is titled with the separator's short name and its description; it converts the
  selected files; a single file that already has the separator is not converted; directories,
  read-only and binary files are skipped), `platform/util/src/com/intellij/util/LineSeparator.java`
  (LF, CRLF, CR), `platform/core-impl/src/com/intellij/openapi/fileEditor/impl/LoadTextUtil.java`
  (every separator of the text is replaced by the new one),
  `platform/platform-impl/src/com/intellij/openapi/vfs/encoding/ChangeFileEncodingAction.java`,
  `EncodingUtil.java` and `IncompatibleEncodingDialog.java` (the two operations: reading the
  bytes again in another encoding, and writing the text in another encoding; each is judged as
  changing nothing, changing something reversibly, or losing information; when both change nothing
  the encoding is set without a question; otherwise a question offers both with cancelling as the
  default, and a lossy conversion needs a second confirmation whose default is to cancel),
  `platform/util/src/com/intellij/openapi/vfs/CharsetToolkit.java` (UTF-8 may have a byte order
  mark, UTF-16 must have one, other encodings have none),
  `platform/platform-impl/src/com/intellij/openapi/editor/actions/AddBomAction.java` and
  `RemoveBomAction.java` (adding a mark to a file whose encoding may have one and has none;
  removing one unless the encoding requires it),
  `platform/platform-impl/src/com/intellij/ide/actions/ToggleReadOnlyAttributeAction.java` and
  `platform/ide-core/src/com/intellij/util/io/ReadOnlyAttributeUtil.java` (each selected local file
  gets the opposite of its current state; documents are saved first), and the bundled keymaps
  `crates/ui/keymaps/` (no shortcut for any of these actions). STD — Unicode (UTF-8, UTF-16, the
  byte order mark U+FEFF), ISO/IEC 8859-1, the WHATWG Encoding Standard (the table of
  windows-1252, with the five unassigned bytes mapped to the C1 controls of the same value), POSIX
  file permission bits, the read-only file attribute of Windows. GEN (a file is replaced through a
  temporary file and a rename, so a reader never sees half of it; a text with a NUL character is
  not text). SPEC-0058 (saving, changes on disk), SPEC-0061 (status bar), SPEC-0124 (menu bar),
  SPEC-0149 (read-only library sources), SPEC-0220 (registry of open documents), SPEC-0317
  (remote projects), SPEC-0053 (agent rights), SPEC-0281 (dialog decisions), SPEC-0319 (effects
  of actions), ADR-0008 (MCP), ADR-0025.
- **Feature matrix rows:** F-010, F-087

## Purpose
The user sees and changes three properties of a file: the encoding its bytes are read and written
in, the line separator, and whether the file is read-only. The status bar shows the first two for
the file in the editor. Every change that could lose text asks first and can be cancelled; nothing
is written outside the project.

## Behaviour

### Which files
1. **Targets.** While the Project tree has the focus the actions for line separators and the
   read-only state act on the files selected there; otherwise, and always for the encoding and the
   byte order mark, they act on the file of the active editor. Without a target the status bar
   says so and nothing happens. A selected folder is not a target: the status bar names it and the
   selected files are still handled. [OSS for the selection, ours for folders — see deviations]
2. **Inside the project only.** A target is resolved through symbolic links before anything is
   read or written. A file whose resolved path is not below the resolved project folder is refused
   with that reason — a library source, a file reached through a link that leaves the project. A
   link that stays inside the project is followed, and the file it points to is changed; the link
   itself stays a link. [ours, security]
3. **Refused files.** A folder, a file larger than 20 MiB, and a file that is not text — its bytes
   contain a NUL and do not start with a UTF-16 byte order mark, or they cannot be decoded — are
   refused for line separators and encoding, each with its reason. A read-only file is refused for
   both too; its read-only state can be changed (behaviour 16). [OSS: directories, read-only and
   binary files are skipped; ours: the limit and the reasons]
4. **Remote projects.** For a file of a remote project (SPEC-0317) none of the three is offered:
   the action says in the status bar that file properties are not available for remote project
   files. The MCP tool acts on the files of the workspace it serves. [ours]
5. **Atomic writes.** Every write of this specification puts the new bytes into a new file in the
   same folder, gives it the permission bits of the old file and renames it over the old one. When
   a step fails the old file stays as it was and the temporary file is removed. [GEN]

### Line separators
6. **The three separators** are LF (`\n`), CRLF (`\r\n`) and CR (`\r`). A file's separator is the
   one most of its line breaks use; with equal counts LF wins over CRLF and CRLF over CR; a text
   without a line break has none and is shown as LF. The status bar shows `LF`, `CRLF` or `CR` for
   the active editor from the editor's text. [OSS: the three; ours: the majority rule, which
   SPEC-0061 already used for two]
7. **Actions.** `ConvertToWindowsLineSeparators`, `ConvertToUnixLineSeparators` and
   `ConvertToMacLineSeparators`, titled "CRLF - Windows (\r\n)", "LF - Unix and macOS (\n)" and
   "CR - Classic Mac OS (\r)". Each replaces every line break of each target by its separator: a
   file with mixed separators is normalised. The text between the line breaks does not change; a
   last line without a line break stays without one. [OSS]
8. **In an open editor** the conversion is one change of the text and one undo step; the file is
   not written until it is saved. A read-only editor is refused. **A file that is not open** is
   rewritten on disk (behaviour 5) in its encoding, with its byte order mark kept. [ours for the
   undo step: the open platform saves the document and rewrites the file]
9. **What the status bar says afterwards:** for one file the separator and the number of line
   breaks changed ("Line separators changed to LF: 12 line breaks in main.rs"); when every line
   break already was the wanted one, that nothing changed; for several files how many were
   changed, how many already had the separator and how many were refused, with the first reason.
   [ours]

### Encoding
10. **Encodings.** The editor layer read and wrote UTF-8 only. With this specification it reads
    and writes these, without a library: UTF-8 (with or without a byte order mark), UTF-16LE and
    UTF-16BE (always with their byte order mark), ISO-8859-1, windows-1252 and US-ASCII. No other
    encoding is offered. [STD; ours: the list]
11. **Which encoding a file is read in.** In this order: a byte order mark decides (UTF-8,
    UTF-16LE, UTF-16BE); else the encoding chosen for this file earlier in this run of IDECraft
    (behaviours 13, 14), when the bytes are valid in it; else UTF-8 when the bytes are valid
    UTF-8; else windows-1252, which then counts as chosen. Bytes with a NUL and without a UTF-16
    mark are not text. A file that opened with an error before because it was not UTF-8 now opens
    in windows-1252 and is saved in it. The byte order mark is not part of the editor's text and
    is written back on saving. The status bar shows the encoding of the active editor ("UTF-8",
    "UTF-8 BOM", "UTF-16LE", …). [OSS: mark first, then the file's own setting; ours: the
    fallback and that a choice lasts until IDECraft quits]
12. **`ChangeFileEncodingAction`, "File Encoding",** lists the encodings for the file of the
    active editor, the current one selected, with "Add BOM" or "Remove BOM" first when that
    applies (behaviour 15). Choosing the current encoding changes nothing. For another encoding
    both operations are judged against the bytes on disk and the editor's text [OSS]:
    - *Reload* reads the bytes on disk again in the chosen encoding. It changes nothing when the
      text comes out the same, is reversible when the text differs, and is impossible when the
      bytes are not valid in the chosen encoding or carry a byte order mark the chosen encoding
      cannot have.
    - *Convert* writes the editor's text in the chosen encoding. It changes nothing when the
      bytes come out the same, is reversible when the bytes differ and every character can be
      written, and is lossy when a character has no place in the chosen encoding.
    When both change nothing the encoding is set at once (it is the encoding the file is saved
    in from now on). Otherwise a question names the file and the encoding and offers Reload (only
    when possible), Convert and Cancel; Cancel is the default answer and Escape's.
13. **Reload.** With unsaved changes in the editor a second question says they will be lost:
    Reload and Discard Changes, or Cancel (default). Reloading replaces the editor's text by the
    bytes on disk read in the chosen encoding, marks the editor unmodified and remembers the
    choice for the file. Nothing is written. [OSS: the user is asked about a modified document]
14. **Convert.** A lossy conversion asks a second time and says how many characters cannot be
    written and the first of them: Convert Anyway, or Cancel (default). Converting writes the
    editor's text — unsaved changes included — to the file in the chosen encoding (behaviour 5),
    marks the editor saved and remembers the choice. After Convert Anyway each character that
    cannot be written becomes `?` in the editor's text too, as one undo step, so the editor shows
    what the file holds. Without that answer nothing lossy is ever written: saving a text with
    characters its encoding cannot hold fails with a message that names the count and the
    encoding, and the file keeps its content (SPEC-0058 behaviour 9). [OSS for the questions and
    the default; ours: the `?`, the count, the failing save]
15. **Byte order mark.** `AddBom` ("Add BOM") applies to a UTF-8 file without a mark, `RemoveBom`
    ("Remove BOM") to a UTF-8 file with one; both write the file like a conversion (behaviour 14,
    never lossy). UTF-16 always has its mark and it cannot be removed; the other encodings have
    none. Where an action does not apply the status bar says why. [OSS]

### Read-only
16. **`ToggleReadOnlyAttribute`, "Toggle Read-Only Attribute":** each target that is writable
    becomes read-only and each read-only one writable. A modified editor of a file that becomes
    read-only is saved first; when that fails the file stays writable and the status bar says so.
    [OSS]
17. **What read-only means.** On Unix a file is read-only when its owner has no write permission.
    Making it read-only clears the write bit of owner, group and others; making it writable sets
    the owner's write bit and no other. On Windows the file's read-only attribute is set or
    cleared; access control lists are not looked at. Other permission bits and the file's content
    and times are untouched. [STD; ours]
18. **The editor follows.** An editor whose file is read-only does not take edits and does not
    save (as SPEC-0149 v4 behaviour 11 for library sources); it follows the file when the action
    or the MCP tool changes the state, when the file is opened and when the editor looks at the
    disk (SPEC-0058 behaviour 10). A library source stays read-only whatever its file says.
    [ours]

### Where the actions are
19. **Menu.** File has the submenu File Properties directly after Plugins…, where the open
    platform's File group has it, with File Encoding, Toggle Read-Only Attribute and the submenu
    Line Separators holding the three conversions in the order CRLF, LF, CR (SPEC-0124 v12
    behaviour 14). [OSS]
20. **Find Action** lists the five menu actions with these titles under File Properties and Line
    Separators, and Add BOM and Remove BOM under File Properties. The bundled keymaps bind none
    of the seven, so none has a shortcut. [OSS]
21. **Status bar.** A click on the line separator opens the three conversions as a list, a click
    on the encoding opens the list of behaviour 12. [OSS: the reference's widgets open popups;
    ours: their content]
22. **Effects.** UI automation (SPEC-0319) declares all seven actions as writing. [ours]

### MCP
23. **Tool `file_properties`** (ADR-0008) with `action`:
    - `get` (default): encoding, byte order mark, line separator with the counts of LF, CRLF and
      CR, read-only state, size, whether the file is open in an editor and modified there.
    - `set_line_separator` with `line_separator` (`lf`, `crlf`, `cr`).
    - `convert_encoding` with `encoding` and optionally `bom`; `lossy: true` is the explicit
      confirmation of behaviour 14 and without it a lossy conversion is refused with the count.
    - `reload_encoding` with `encoding`: remembers the choice (behaviour 11) when the reload is
      possible; reports whether the text changes.
    - `set_read_only` with `read_only`.
    The four changing actions take `dry_run`, which defaults to true and then only reports what
    would change; with `dry_run: false` they need the `write` right and are written to the agent
    log. Paths are relative to the workspace; a path that leaves it, also through a link, is
    refused. A file with unsaved changes in an editor is refused for the changing actions except
    `set_read_only` to false: the agent saves or the user decides first. The tool writes the file
    (behaviour 5); an open, unmodified editor takes the change when it looks at the disk. `get`
    needs no right. [ours]
24. **Dialog decisions** (SPEC-0281): the questions of behaviours 12 to 14 are listed with the
    tool calls that take their place. [ours]

## Acceptance criteria
- **AK-1** Line separators: LF, CRLF and CR texts convert into each other; a mixed text is
  normalised and the count is the number of line breaks changed; a last line without a line break
  stays so; CRLF → LF → CRLF gives the first text again; the majority rule with its ties
  (`file_properties::tests::{converts_between_the_three_separators,
  mixed_separators_are_normalised_and_counted, last_line_without_a_line_break_stays,
  crlf_lf_crlf_round_trip, dominant_separator_and_ties}`;
  `buffer::tests::line_ending_detection`).
- **AK-2** Encodings: every encoding decodes what it encoded; windows-1252 maps all 256 bytes
  and back; invalid bytes are an error, never a replacement character; a character an encoding
  cannot hold is an error with count and first character; the lossy form writes `?` and returns
  the text as written (`file_properties::tests::{encodings_round_trip,
  windows_1252_maps_every_byte, invalid_bytes_are_refused_not_replaced,
  non_representable_characters_are_refused, lossy_encoding_is_explicit_and_says_what_it_wrote}`).
- **AK-3** Detection: the byte order marks, a remembered choice, UTF-8, the windows-1252
  fallback, NUL bytes as not text; the mark is not in the text and is written back
  (`file_properties::tests::{detection_order, a_byte_order_mark_is_kept_out_of_the_text_and_written_back,
  binary_and_oversized_files_are_refused}`).
- **AK-4** Judging reload and convert: same, reversible, lossy, impossible
  (`file_properties::tests::{reload_is_judged_against_the_bytes, convert_is_judged_against_the_text}`).
- **AK-5** On disk: a conversion of separators or encoding replaces the file atomically with its
  permission bits, leaves no temporary file, refuses read-only, binary and oversized files and
  folders; a dry run writes nothing (`file_properties::tests::{separators_are_rewritten_on_disk_atomically,
  encoding_is_converted_on_disk_and_refused_when_lossy, add_and_remove_the_byte_order_mark,
  dry_runs_write_nothing, binary_and_oversized_files_are_refused}`).
- **AK-6** Inside the project: a path outside, a link leaving the project and `..` are refused for
  every operation; a link inside the project changes its target and stays a link
  (`file_properties::tests::{a_symlink_out_of_the_project_is_refused,
  a_symlink_inside_the_project_changes_its_target}`).
- **AK-7** Read-only: the toggle clears and sets the write bits as behaviour 17 says and touches
  nothing else (`file_properties::tests::read_only_toggle_changes_only_the_write_bits`).
- **AK-8** Editor: a windows-1252 file opens, shows its text and is saved byte for byte; a text
  its encoding cannot hold is not saved; a UTF-8 mark survives a save; converting separators is
  one undo step; a read-only file's editor takes no edit and follows the file
  (`editor_view::file_property_tests::{a_windows_1252_file_is_read_and_saved_in_its_encoding,
  a_text_the_encoding_cannot_hold_is_not_saved, the_byte_order_mark_survives_a_save}`;
  `workbench/tests/it/file_properties.rs`: `converting_separators_in_an_editor_is_one_undo_step`,
  `reload_with_unsaved_changes_asks_and_cancel_keeps_them`,
  `a_lossy_conversion_asks_twice_and_cancel_is_the_default`,
  `the_editor_follows_the_read_only_attribute`, `the_status_bar_shows_separator_and_encoding`).
- **AK-9** Menu and Find Action: File › File Properties after Plugins… with the items of
  behaviour 19 in order; the seven actions are found with their titles and have no shortcut; all
  seven are declared as writing (`menus::tests::file_properties_is_a_submenu_of_file`;
  `everywhere::tests::file_property_actions_have_titles`;
  `menu_bar::tests::{every_menu_action_is_carried_out_somewhere,
  every_menu_action_is_found_by_search_everywhere}`;
  `security::tests::file_property_actions_are_declared_as_writes`).
- **AK-10** MCP: `get` reports the properties; each changing action is a dry run by default,
  needs `write` otherwise, refuses a lossy conversion without `lossy`, refuses paths outside the
  workspace and files modified in an editor (`protocol.rs`: `file_properties_tool`,
  `file_properties_changes_need_the_write_right_and_stay_in_the_workspace`,
  `lists_all_tools_with_schemas`; `dialogs::tests::every_tool_and_test_named_exists`).
- **AK-11** In the window, next to the reference: the submenu, the three conversions, the
  encoding question and the read-only toggle by hand — open (hand test).

## Out of scope
- Associating a file with a file type, and overriding a file's type: SPEC-0004 v5 (not part of
  the File Properties group).
- Encodings beyond behaviour 10; guessing an encoding from the content beyond behaviour 11;
  project and IDE default encodings; an encoding per folder; keeping a file's chosen encoding
  after IDECraft quits.
- Converting the files of a selected folder recursively.
- Remote project files (behaviour 4).
- Typing a line break in a CRLF or CR file inserts `\n` as before; the file then has mixed
  separators until it is converted.
- Search, indexing, version control views and the MCP tools that read files as text still read
  UTF-8 only; a file in another encoding is edited correctly but not searched by them.

## Known deviations from the reference
- **Folders are not converted** (behaviour 1): the open action walks a selected folder and
  rewrites every text file below it without a preview. IDECraft converts selected files only.
- **Open editors are changed, not rewritten** (behaviour 8): the conversion of line separators is
  an undo step in the editor and reaches the disk with the next save, where the open platform
  saves and rewrites at once.
- **No forced reload** (behaviour 12, IMP-0404): the open dialog offers to reload "anyway" in an
  encoding the bytes are not valid in; the text then holds replacement characters, and the next
  save writes them over the original bytes. IDECraft offers Reload only when every byte is valid
  in the chosen encoding.
- **A lossy conversion shows its result** (behaviour 14): the characters that could not be written
  become `?` in the editor as an undo step, so text and file agree; the count and the first such
  character are named before.
- **The chosen encoding is not stored in the project** (behaviour 11); the open platform keeps it
  in the project's settings.
- **The read-only action's title does not change** with the selection (the open action reads
  "make read-only" or "make writable" for a uniform selection); the status bar says what was done.

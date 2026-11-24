# SPEC-0356: Taking over what the former name left behind

- **Version:** 1 · **Date:** 2026-11-23
- **Sources:** GEN (a renamed program moves its own data once and keeps reading what older
  versions wrote; `rename(2)` keeps a file's content, owner and mode); the project owner's decision
  of 2026-11-22 to rename the product and, for step 2, "everything, with takeover" (PO-STATUS
  question 52); ADR-0021 and SPEC-0303 (the envelope of project and exchange files); SPEC-0008 v8
  behaviours 11–18 (passwords in the system keychain); SPEC-0353 behaviours 6, 14 and 15 (the vault
  key and vault references); ADR-0025 §3 and SPEC-0321 behaviour 20 (remote keys in the keychain or
  Secret Service); SPEC-0069 behaviour 3 (the generated ignore file); SPEC-0156 behaviour 4 (the
  plugin host functions); CLAUDE.md rule 12 (security first)
- **Feature matrix rows:** none (product rename, step 2)

## Purpose
The product gets a new name, and with it new technical names: the settings folder, the project
folder, environment variables, keychain services, stored theme and keymap names, the envelope of
its files and the names its plugins link against. Someone who used an older version keeps all of
it: the folders move to their new names once, and everything older versions stored is still read.
Nothing is deleted, nothing that exists under the new name is replaced, and no password or key is
weakened, copied into a file or shown on the way.

The code is written before the mechanical rename (rename step 2, technical names): every former
name is a constant the rename leaves (`rebrand:keep`), every current name one it rewrites. Until
the rename both are the same and nothing in this specification changes anything (behaviour 13).

## Behaviour

### Folders
1. **Settings folder.** At the start of the program (the window and every command of `ferris-idea`,
   and `ferris-idea-serve`), before anything reads or writes settings, the settings folder of the
   former name (`<configuration home>/<former short name>`, the configuration home being
   `$XDG_CONFIG_HOME` or `~/.config`) is moved to today's name (behaviours 3–4). A settings folder
   named by the settings-folder variable is the user's choice and is not moved. A line on standard
   error says what was moved, or why the move failed. [GEN]
2. **Project folder.** When a project is opened (the workspace of the MCP server and of every
   command, the project window, and before the generated ignore file is written), its project
   folder of the former name (`<root>/.<former short name>`) is moved to today's name the same
   way. So is the product's own folder inside a Git repository's folder, where a smart checkout
   records its stash (SPEC-0231). [GEN]
3. **Never overwritten, once.** A folder is moved only when nothing of today's name exists (no
   folder, also no empty one, no file, no link, also no dangling one), the former name is a folder
   (a link to a folder moves as the link; its target is not touched), and the former folder holds
   no note (behaviour 4). Otherwise nothing happens. [GEN]
4. **A move, not a copy.** The folder is renamed, so every file keeps its content, owner and mode
   and no second copy of a key, password file or token is made. In its place a folder of the
   former name is created with the mode the former folder had, holding only the note `MOVED.txt`
   (mode 0600): the product's old and new name, the new path, that nothing was deleted, and that
   the folder may be deleted. While the note is there, the folder is never taken over again. A
   rename that fails changes nothing; there is no copy as a fallback. [GEN, CLAUDE.md rule 12]
5. **Keymap file.** Inside the settings folder the user keymap file of the former name
   (`keymaps/<former name>.toml`, and the XML file of older versions beside it) is renamed to
   today's name, each only when today's file does not exist. [GEN, SPEC-0303]

### Stored values
6. **Environment variables.** Every variable of the product (today's prefix) is also read under
   the former prefix while today's name is not set; when both are set, today's wins. This holds
   for the variables of the program, its self-test and its tests, and for the terminal's
   shell-integration switch and the login-shell switch. [GEN]
7. **Theme names.** A theme or editor scheme saved under a brand theme's former name (in the IDE
   settings' theme, dark theme, light theme and editor scheme, and in the theme file of older
   versions), in any case, is read as today's brand theme; saving writes today's name. [SPEC-0060,
   SPEC-0303]
8. **Keymap name.** A user keymap saved under the former name keeps its shortcuts and reads with
   today's name. [SPEC-0105]
9. **Generated ignore file.** A project folder's ignore file whose first line is the generated
   comment of the former name is still recognised as generated: its first line becomes today's
   comment and missing entries are added; the user's lines stay. [SPEC-0069]
10. **Envelope.** A file whose envelope table carries the former name is read like one with
    today's name, by every reader of project and exchange files; when a file has both, today's
    wins. Files are written with today's name; a file edited in place keeps the envelope it has.
    [ADR-0021]

### Keychain
11. **Passwords and keys.** The keychain services that carry the product's short name change with
    it: database passwords (the short name itself), API keys of AI endpoints (`-ai`), the vault key
    (`-vault-key`) and the remote keys (`-remote`); on Linux also the Secret Service items'
    application attribute. [SPEC-0008, SPEC-0353, ADR-0025]
    - An entry is read under today's service first. Only when there is none it is read under the
      former service; the value is returned, copied to today's service in the same keychain (on
      Linux the same default collection over an encrypted session), and the former entry is
      removed only after the copy reads back exactly the same value. A copy that fails or reads
      back differently leaves the former entry as it is.
    - Saving writes today's service and then removes a former entry once the write reads back;
      removing an entry removes it under both names. A keychain that cannot be read is an error,
      never "no password".
    - A profile saved by an older version names the former service: it is the same entry as
      today's (an agent's check that a profile names the product's own entry accepts it). A
      profile naming the vault key under either name is still refused as a password, and a vault
      reference under either name is still a vault reference.
    - No value is logged, printed, put into an error or written to a file; the copy is a new entry
      made the way any save makes one, so no access setting is weakened and no weaker store (a
      file, the kernel keyring) is tried. Tests use stores in memory or the test store of debug
      builds, never the user's keychain.

### Plugins
12. **Plugins built for older versions.** The host functions are offered under today's module name
    and the former one, and a plugin's exported functions (`alloc`, `manifest`, `run` with the
    product's prefix) are found under today's prefix or, when a plugin exports only those, the
    former one. [SPEC-0156]

### Before the rename
13. While the former and today's names are the same, every behaviour above finds nothing to do:
    no folder is moved, no note written, no variable or keychain entry read twice. [GEN]

## Acceptance criteria
- **AK-1** Given a former folder with a key file of mode 0600 and the folder itself 0750, when it
  is taken over, then today's folder holds the same files with the same modes, the former place
  is a 0750 folder with only the 0600 note naming the new path, and a later run changes nothing
  even when today's folder is gone again; a project's former folder is taken over at its root.
- **AK-2** Given today's folder exists (with files, empty, a file, a dangling link), when a
  takeover runs, then nothing is changed in either place; without a former folder, with a former
  file, or with equal names nothing happens.
- **AK-3** Given the former folder is a link to a folder, when it is taken over, then today's name
  is the link, the target holds no note, and the note is in a new folder of the former name.
- **AK-4** Given keymap files of the former name in the settings folder, when the takeover runs,
  then they carry today's names; a former file written later never replaces today's.
- **AK-5** Given only the former variable is set, the program reads it; given both, today's wins.
- **AK-6** Given IDE settings, a theme file or a user keymap saved with the former brand theme or
  keymap names, when they are read, then they carry today's names and the keymap its shortcuts;
  the brand themes are found by their former names in any case.
- **AK-7** Given a generated ignore file with the former first line, when it is refreshed, then
  its first line is today's, each entry appears once and the user's lines stay.
- **AK-8** Given a run file with the former envelope, when it is read, then its kind and format
  are found and its envelope is taken out like today's; today's envelope wins when both exist.
- **AK-9** Given a plugin importing a host function from the former module and exporting the
  former prefix, when it is loaded and run, then it works.
- **AK-10** Given a password under the former service, when it is read through today's or the
  former name, then it is returned, copied, and the former entry removed only after the copy reads
  back; today's entry wins; a failed or garbled copy keeps the former entry; saving and removing
  cover both names; a failing former store is an error naming no value; in the test store the copy
  is a 0600 file.
- **AK-11** The renamed services are exactly the product's own (passwords, AI keys, vault key) and
  map a former profile reference onto today's entry; the vault key under either name is never a
  password; a vault reference is recognised under either name.
- **AK-12** Given the remote key under the former service, when it is read, then it is copied and
  the former entry removed only after the copy reads back; a locked keychain is an error; writing
  and removing cover both names; a host keeps its identity across the rename.

## Out of scope
- Caches and runtime state named after the product (the user cache folder, the remote mirror
  cache, reattach sockets, log folders, temporary files): they are rebuilt, not taken over.
- The mechanical rename itself (PO-STATUS question 52, step 2), and the environment variables of
  the repository's own scripts.
- Moving a folder across file systems: the rename fails, nothing changes, the reason is shown.

## Known deviations from the reference
None: the reference has no such rename. The choice of a move over a copy (behaviour 4) is a
security decision of this product: a copy would leave a second copy of every key and password
file on disk.

# SPEC-0094: Open project and recent projects

- **Version:** 9 · **Date:** 2026-11-17 (v9: `recent_projects` needs the agent right
  `recent-projects`, behaviour 23, AK-20 — security review: an agent of one project read the
  folders of all the user's other projects); 2026-11-17 (v8: each recent project once, by its canonical path,
  behaviour 22, AK-19 — headless check of F-001, OBS-0077); 2026-11-15 (v7: a path to a file opens the file's project and the file in an editor, never a project with the file as its root, behaviours 18 to 21, AK-16 to AK-18 — window check of 2026-11-15, `idecraft ui <folder>/queries/file.sql` showed the file as the project folder and opened nothing; 2026-11-14 (v6: `NextWindow` and `PreviousWindow` step through every window of the application, behaviour 17, AK-15; sixth package of docs/research/KEYMAP_COVERAGE.md; v5: the popup's actions New Project…, Clone Repository… and its Open Projects section, behaviours 1 and 12 to 15, AK-10 to AK-14; the window that opens the popup or closes projects is read directly, behaviour 16 — crash of the window check of 2026-11-14 — comparison next to the reference of 2026-11-14, F-001; v4: the window's close button closes as Close Project,
  behaviour 11, AK-9; v3: closing project windows — the current one, all, all others —
  behaviours 6 to 10, AK-5 to AK-8; v2: next and previous project window, the keymap's `NextProjectWindow` and `PreviousProjectWindow`, behaviour 5, AK-4); 2026-11-01 (v1)
- **Sources:** GEN (the project widget at the left of the main toolbar opens a popup with Open… and the
  recently opened projects; choosing a project opens it in a window of its own, an open project's window
  comes to the front; File | Open… asks for a folder; F-001), OSS (action id `OpenFile`; platform icon
  `expui/nodes/folder`), SPEC-0001 (workspace), SPEC-0061 (main toolbar)
- **Feature matrix rows:** F-001

## Purpose
Switch between projects and open a new one without the command line.

## Behaviour
1. **Popup:** a click on the project widget opens a popup under it. (v5) Its first rows are "New
   Project…", "Open…", "Clone Repository…" and "Remote Development…" (SPEC-0317); under an "Open
   Projects" heading follow the projects open in windows (behaviour 12). Under a
   "Recent Projects" heading follow the recently opened projects, most recent first, without the current
   one and (v5) without the projects open in windows. Each has the project's initials, its name and its folder ("~/…" inside the home folder). Without
   other projects the heading says "No recent projects". ↑/↓ move (wrapping around), Enter or a click
   chooses, Escape closes. [GEN]
2. **Open:** choosing a project opens it in a new window. When the project is already open in a window,
   that window comes to the front instead. "Open…" and `OpenFile` (found through Search Everywhere) show
   the system's folder dialog; the chosen folder opens the same way. A folder that cannot be opened
   leaves a message in the status bar. [GEN, OSS]
3. **List:** every window that opens a project puts it at the front of `recent-projects` in the user's
   IDECraft settings folder (SPEC-0053: `$IDECRAFT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/idecraft`, else
   `~/.config/idecraft`), one folder per line, at most 25. Folders that no longer exist are left out when
   the list is read. [GEN]
4. Agents read the list through `recent_projects` (v9: only with the `recent-projects` right,
   behaviour 23). [ADR-0008]
5. **Next and previous project window (v2):** `NextProjectWindow` (⌥⌘` in the macOS keymap, Ctrl+Alt+] in
   the default one) brings the project window opened after the current one to the front,
   `PreviousProjectWindow` (⌥⇧⌘`, Ctrl+Alt+[) the one opened before it; after the last comes the first, and
   before the first the last. The order is the order in which the windows were opened; a window that was
   closed is left out. Only project windows count: a tool window in its own window (SPEC-0061 behaviour 39)
   is passed over. With a single project window the actions do nothing. Both are in the Window menu after
   Zoom and in Find Action. Agents name the project window they mean with the `window` argument of the
   `ui_*` tools and need no tool to bring one to the front. [GEN: stepping through the windows of an
   application; OSS: the action ids and their shortcuts in the bundled keymaps]

6. **Close Project (v3):** `CloseProject` closes the project window it is used in. `CloseAllProjects`
   closes every project window, `CloseOtherProjects` every one but the current. The last two are
   offered only while more than one project window is open; with a single window they stay in the
   menu (the menu bar does not enable items by context, SPEC-0124) and do nothing but say in the
   status bar that they are not available. The rule which windows close and when
   each action is offered follows the open platform's actions of the same ids (OSS, logic template,
   no code: `docs/legal/PROVENANCE.md`). [OSS]
7. **Nothing is lost silently (v3):** before any window closes, the unsaved files and the running
   work of the projects that close are asked about once, in the question of quitting (SPEC-0261 v6
   behaviour 19) with the words of closing: Save All and Close, Close Without Saving, Cancel; only
   running work: Stop and Close, Cancel. Saving that leaves a file unsaved closes nothing and says
   which file. Cancel closes nothing. The other project windows and their files are not touched and
   not named. Closing stops the run configurations, tests and debug sessions of the windows that
   close; a database statement that still runs is left to finish and its result is dropped. Files
   not saved are dropped with their window: nothing of them stays behind to be asked about at quit.
   [GEN, security first]
8. **Order and recent list (v3):** the other windows close first, the current one last, as in the
   open platform. Each closed project moves to the front of the recent list (behaviour 3), so the
   one closed last is opened first at the next start. A closed window's project is no longer listed
   as open, and the agent server stops serving that project (SPEC-0221).
9. **After the last window (v3):** when no project window is left, IDECraft offers the folder chooser
   it shows at a start without a folder (behaviour 2); choosing a folder opens it, cancelling quits
   IDECraft. IDECraft has no welcome screen of its own (see Out of scope).
10. **Agents (v3):** the MCP tool `close_project` closes this project's window (`scope` `this`),
    every other (`others`) or all (`all`). `dry_run` (default true) only names the windows that
    would close and their unsaved files and running work. A real close needs the user-granted
    `quit` right; with unsaved files or running work `pending` decides: `refuse` (default: nothing closes and the
    files are named), `save_all` (needs `write` too) or `discard`. External UI automation declares
    the three actions with the effect of quitting (`quit`), so a client without that right cannot
    press them. [ADR-0008, SPEC-0053, SPEC-0319]

11. **The window's close button (v4):** closing a local project window with its close button (the
    title bar's button, the system's close command) is Close Project of behaviour 6: the same
    question for its unsaved files and running work (behaviour 7), the same stopping, recent list
    and agent server (behaviour 8), the folder chooser after the last window (behaviour 9). Until
    v3 the button closed the window at once: unsaved changes were lost without a word, and the
    window's run configurations kept running with no window left to stop them until IDECraft quit.
    A remote project's window keeps its own question (SPEC-0317). ⌘W closes a tab, never the
    window (SPEC-0059). [GEN, security first]
12. **Open Projects (v5):** every project open in a window of this IDECraft is a row with its
    initials, name and folder, in the order the windows opened; the window the popup is used in is
    marked "current". Choosing a row brings that project's window to the front (behaviour 2); the
    popup closes. A remote project's window is not listed here (SPEC-0317 lists those under Remote
    Development). [GEN]
13. **Clone Repository (v5):** the row turns the popup into a form with the repository's address
    ("URL") and the folder the clone goes in ("Location", the folder of the current project at
    first); a line names the new folder, `Location`/<repository name>, or the rule the address
    breaks. Enter or "Clone" starts it, Escape or "Cancel" goes back to the list. Rules (security
    first, rule 12):
    - Only HTTPS (`https://host/path`) and SSH (`ssh://[user@]host[:port]/path`, `user@host:path`)
      addresses are cloned. An address with a password, a token or any user information in HTTPS,
      with a query or fragment, other transports (`http`, `git`, `file`, `ext::`, local paths) and
      an address or host starting with `-` are refused before anything runs, with the reason; the
      refusal never repeats the credentials. Git is told to use only HTTPS and SSH, also for
      redirects, and submodules are not cloned.
    - The folder must be new, in a folder that exists; nothing is written into a folder that is
      there already.
    - Git never asks: credentials come from the system's credential helper or the SSH agent, SSH
      runs in batch mode with strict host keys and the shared options of ADR-0025. An unknown host
      key fails the clone with a hint to add it to `known_hosts` first.
    - Progress: while git clones, the popup and the status bar show "Cloning <name>: <phase> <n>%"
      from git's progress lines, with "Cancel" in the popup (also after closing and reopening it).
      Cancelling, a failure or the time limit (one hour) ends git and removes the new folder again;
      the reason shows in the form and the status bar. One clone runs at a time.
    - After the clone the new folder opens in a window of its own, which asks whether to trust the
      project like any new folder (SPEC-0278); nothing of it runs before.
    [GEN, rule 12, ADR-0025]
14. **New Project (v5):** the row turns the popup into a form with the project's name, the folder
    it goes in ("Location", the folder of the current project at first) and "Create Git
    repository" (on at first). "Create" makes the empty folder — it must be new, in a folder that
    exists, its name one plain folder name — runs `git init` in it when asked, and opens it in a
    window of its own (behaviour 2); a problem shows in the form and nothing is made. No project
    type, template or build file is generated. [GEN]
15. **Agents (v5):** `clone_repository` (address, `location`, `name`, `dry_run` default true,
    `timeout_s`) and `new_project` (`name`, `location`, `git_init`, `dry_run` default true) follow
    behaviours 13 and 14. Their `location` is the workspace's parent folder (default) or a folder
    inside the workspace outside its hidden folders, so an agent never makes folders elsewhere
    (settings, start-up folders). The dry run checks everything and names the folder without
    network; a real clone needs the `remote` and `write` rights, a real new project `write`. Each
    real call is a line in the agent log with the address (which holds no credentials), the folder
    and whether it worked. Neither opens a window or trusts the folder: the user opens it. The
    projects open in windows are listed only in the popup. [ADR-0008, SPEC-0053]
16. **One process, any number of windows (v5):** opening the popup and every closing action
    (behaviours 6 to 11) read the project windows of the process — their folders, unsaved files and
    running work — without ending it, with one window as with several: the window the action is
    used in is read directly, the others through their windows. Until the window check of
    2026-11-14 the popup, Close Project, Close Other Projects, Close All Projects and the close
    button read the window they were used in a second time while it was handling the action, and
    the process ended with all its windows. [GEN, rule 12]
17. **Every window (v6):** `NextWindow` (Activate Next Window, ⌘` in the bundled macOS keymap;
    the default keymap binds no key) and `PreviousWindow` (Activate Previous
    Window, ⇧⌘`) bring the window after or before the current one to the front, among all windows
    of the application: the project windows in the order they were opened (behaviour 5), then
    every other window, such as a tool window in a window of its own, in the order the toolkit
    lists them; the step wraps at both ends, and with a single window the actions do nothing.
    Both stand in the Window menu below the project window entries (SPEC-0124 v13 behaviour 15).
    On macOS the system itself steps through an application's windows on ⌘`; where the key
    reaches IDECraft the result is the same kind of step. [OSS: ids, titles and keys of the
    ported keymaps and `ActionsBundle.properties`; GEN for the order]
18. **A file's project (v7):** wherever IDECraft is asked to open a path as a project and the
    path is a file (after following links), it opens the file's project, never a project whose
    root is the file. The project is the folder found first, walking from the file's folder up:
    1. the nearest folder with `.idecraft/` or `.idea/` (a folder that was opened as a project);
    2. else the nearest version control root (`.git` folder or file, `.hg/`, `.svn/`);
    3. else the nearest folder with a build file (`Cargo.toml`, `package.json`, `pom.xml`,
       `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts`, `build.sbt`,
       `go.mod`, `pyproject.toml`, `setup.py`, `Gemfile`, `composer.json`, `Package.swift`,
       `CMakeLists.txt`, `mix.exs`, `deno.json`);
    4. else the file's own folder.
    The user's home folder and the file system root are never chosen by steps 1 to 3 (they are
    passed over; the walk stops at them); step 4 may still give the home folder for a file that
    lies directly in it. Why tiers and not simply the nearest folder with any marker: a file in a
    crate of a Cargo workspace or a package of a monorepo would then open the crate or the
    package (its `Cargo.toml`, its `package.json`) instead of the repository the user works in;
    a folder opened as a project before is the clearest statement of what the user calls the
    project, the version control root the next clearest. Why not home: a dotfiles repository
    or a stray `.idea` there would make the whole home folder the project — indexed, watched and
    served to agents (rule 12). A folder path opens as before. [GEN, rule 12]
19. **The file opens at once (v7):** the file opens in a focused editor tab of that project's
    window. When a window of this IDECraft already shows the project, the file opens there and
    the window comes to the front; no second window. `idecraft ui <file>` and
    `idecraft launch <file>` (SPEC-0282 behaviours 6 and 8) first look for a running IDECraft that
    serves the project (the discovery files of SPEC-0221): when one does, the file is handed to
    it with `editor_tabs` `open` (behaviour 21) as client `idecraft-cli`, which that window asks
    the `ui` right for like any client (SPEC-0053), and the command ends without a window of its
    own; without one, or when the hand-off fails (refused, no answer), a new window opens as
    above. The same holds inside the window for every way a path reaches `open_project`
    (the popup's rows, the folder chooser, a recent project). [GEN]
20. **Commands that serve a project to agents (v7):** `idecraft mcp <path>`, `idecraft call <path> …`
    and `idecraft open <path>` (a path that is not an exchange file) take a folder. A file is
    refused, with the folder behaviour 18 finds named in the message so the user can pass it;
    `call` ends with exit code 2 as for other wrong arguments. They do not widen a file to its
    project by themselves: the agent server's scope is what the user named, and a file widened
    to its repository would give agents more than asked (rule 12). Dropping: IDECraft has no
    welcome screen (behaviour 9); files dropped on a project window's editor open in tabs of
    that window and dropped folders are passed over, as before (SPEC-0059 v5 behaviour 11).
    [rule 12]
21. **Agents (v7):** `editor_tabs` has the action `open`: it opens the file `path`
    (workspace-relative, or absolute inside the workspace) in a tab of `group` (default: the
    focused group), focuses the editor and brings the window to the front, and returns the tab
    state like `list`. A path outside the workspace (also through `..` or a link), a folder and
    a missing file are refused. Needs the `ui` right like the other actions that change tabs.
    [ADR-0008, SPEC-0053]
22. **One entry per folder (v8):** the recent list names each folder by its canonical path
    (links resolved, no trailing `/`, no `.` or `..`). Reading the list resolves every line and
    keeps the first of several lines that name the same folder, so the popup, `recent_projects`
    and the next start see it once, at the place of the most recent; remembering a project
    stores its canonical path. Until v8 the lines were compared as written: a list with
    `…/proj`, `…/proj/` and `…/proj/../proj` (a list edited by hand, kept from an older version
    or written by another tool through a link such as `/tmp` for `/private/tmp` on macOS) showed the same
    project three times, against SPEC-0001 behaviour 1 and AK-1. [GEN]
23. **The list is the user's, not the workspace's (v9):** the folders of the other recent
    projects lie outside the workspace an agent is connected to and tell where the user's other
    work is. `recent_projects` therefore checks the agent right `recent-projects` (SPEC-0053) before
    it reads the list; the right is not granted unless the user grants it for the workspace
    (in the grant file or by answering "Always for This Workspace" when the window asks a client
    over HTTP). Without it the call is refused with the stable "not granted" text that names the
    right `recent-projects`, its meaning and the grant entry to add, and returns no folder. With it
    the tool returns the list as before and writes one line to the workspace's agent log with the
    tool, the right and the number of folders, never their paths. The project popup, the next and
    previous project window and every other use of the list inside IDECraft's own windows are
    unaffected: the right governs agents only. Until v9 the tool needed no right, so every agent
    connected to any project could read the folders of all others. [GEN, SPEC-0053]
## Acceptance criteria
- **AK-1** Most recent first, no duplicates, missing folders left out, at most 25
  (`crates/core/tests/it/recent.rs`); the tool is listed (`crates/mcp/tests/it/protocol.rs`).
- **AK-2** In a real window (`idecraft ui --selftest`, with its own list): the popup lists "Open…" and
  the other recent project, Escape closes it, choosing the project opens a second window.
- **AK-3** By hand: popup look next to the reference, Open… with the folder dialog, an open project's
  window comes to the front.

- **AK-5 (v3, behaviours 6 and 8)** Unit tests `close_model::tests`: which windows each action closes
  and in which order (current last), when each is offered (one window, several), the action ids.
- **AK-6 (v3, behaviour 7)** Unit tests `close_model::tests`: the question with the words of closing
  for unsaved files, for running work alone, none when nothing is pending; only the files of the
  projects that close are named.
- **AK-7 (v3, behaviour 10)** MCP protocol tests with a stand-in window: dry run names windows and
  pending work; a real close without `quit` is refused; with unsaved files `refuse` closes nothing;
  `discard` closes; the effects of the three actions in UI automation (`security::tests`).
- **AK-9 (v4, behaviour 11)** `run_stop::a_dropped_run_handle_leaves_its_program_running` shows why
  the window has to stop its runs itself; `close_model::tests::the_close_button_is_close_project`;
  by hand in a window: the close button with an unsaved file (question, each answer), with a
  running program (stopped), with nothing pending (closes at once), the last window (folder
  chooser).
- **AK-10 (v5, behaviours 1 and 12)** `project_popup_model::tests::open_projects_come_before_the_recent_ones`:
  the four actions, the open projects in order with the current one marked, then the recent ones
  that are not open. In a real window (`idecraft ui --selftest`): the rows start with the four
  actions, the current project is marked, choosing the other recent project opens a second window.
- **AK-11 (v5, behaviours 13 and 14)** `clone::tests` (`idecraft-vcs`): HTTPS and SSH accepted,
  credentials, other transports and disguised options refused without repeating the credentials;
  new folders only; progress lines; the arguments that keep other transports and submodules out;
  a clone that fails or is cancelled leaves no folder; New Project with and without `git init`.
  `project_popup_model::tests::the_forms_name_the_new_folder_or_the_reason`.
- **AK-12 (v5, behaviour 15)** `protocol::clone_repository_and_new_project_check_first_and_need_their_rights`:
  dry run names transport, host and folder; credentials, `file:` and `ext::` refused; a location
  outside the workspace and its parent and a hidden folder refused; the real steps refused without
  `remote` / `write`; with them a failing clone leaves no folder, New Project with `git init`
  makes the folder, both logged.
- **AK-13 (v5)** By hand in a window: the popup next to the reference; Clone Repository of a public
  HTTPS repository with progress, Cancel in the middle (no folder left), a clone that opens with
  the trust question; an SSH address of an unknown host (the hint); New Project with and without
  Git; switching to another open project's window.
- **AK-14 (v5, behaviour 16)** Window tests of the test harness (`project_popup_windows`, macOS):
  one window with 25 recent projects opens the popup with its actions, the current project and
  the 25; two windows in one process — each popup lists both open projects with its own marked,
  choosing the other brings its window to the front, Close Other Projects in the first closes the
  second and the process goes on. By hand: the steps of the window check (one window; two
  windows; the popup; Close Project; the close button).
- **AK-8 (v3)** By hand in a window: each action with two and three windows, with an unsaved file
  and a running program; after the last window the folder chooser.
- **AK-4 (v2, behaviour 5)** The window after and before the current one in opening order, the wrap at both
  ends, nothing with one window or when the current one is unknown, closed windows left out
  (`run_debug_actions::tests::project_windows_step_in_opening_order_and_wrap`,
  `project_windows::tests::the_order_keeps_each_window_once`); the actions have their titles, their place
  in the Window menu and a shortcut in both keymaps
  (`everywhere::tests::run_debug_and_window_actions_from_the_keymap`,
  `menus::tests::run_debug_and_window_actions_are_in_their_menus`). With three windows of the test harness
  the actions bring the next and the previous one to the front and wrap
  (`run_debug_actions_keymap::next_and_previous_project_window_step_through_the_windows`). On screen: not
  checked.
- **AK-15 (v6, behaviour 17)** The cycle holds the project windows in opening order, then the
  other windows, each once, and leaves out closed ones
  (`keymap_actions::tests::every_window_is_in_the_cycle_once`); with three windows of the test
  harness the actions bring the next and the previous one to the front and wrap, and with one
  window they report nothing to do (`sixth_package_keymap.rs::next_and_previous_window_step`).
  On screen: not checked.

- **AK-16 (v7, behaviour 18)** Unit tests `project_root::tests` (`idecraft-core`): a folder is its
  own project; a file in a plain folder gives that folder; a file in a crate of a Cargo
  workspace inside a Git repository gives the repository; `.idea/` and `.idecraft/` win over a
  nearer Git root and a nearer build file; a build file wins when there is no version control;
  the home folder and the root are passed over (a `.git` in the home folder is ignored); a link
  to a file gives the project of the file it points to. `launch::tests` (crate `idecraft`, `crates/app`): the
  start folder of `idecraft launch <file>` is its project.
- **AK-17 (v7, behaviours 19 and 20)** Window test of the test harness (`open_file_path.rs`):
  `open_project` with a file inside a repository opens one window whose project is the
  repository root (not the file) and whose active, focused tab is the file; a second file of
  the same project opens in that window, no second window. Unit tests in `crates/app`: the
  refusal of `mcp`/`call`/`open` with a file names its project.
- **AK-18 (v7, behaviour 21)** MCP protocol test with a stand-in window
  (`protocol::editor_tabs_open_checks_the_path`): `open` with a file of the workspace reaches the
  window as an absolute path; a path outside the workspace, through `..`, a folder and a missing
  file are refused before the window is asked; the action is listed. By hand: not checked
  (`idecraft ui <file>` with and without a running window).

- **AK-19 (v8, behaviour 22)** `crates/core/tests/it/recent.rs`:
  `spellings_of_one_folder_are_one_entry`: a list with a folder with a trailing `/`, through
  `..` and by its canonical path, and another folder, reads as the two canonical folders in order;
  remembering a link to the second puts that folder's canonical path first and the file holds
  each folder once. By hand: not needed (the popup reads the same list).
- **AK-20 (v9, behaviour 23)** MCP protocol test
  `protocol::recent_projects_needs_its_right_and_logs_the_read`: without the right (also with the
  `write`, `vcs-write` and `settings` rights) the call is refused, the message starts with
  "not granted:", names `recent-projects` and no folder, and nothing is logged; with the right it
  lists the existing folder of the list (a missing one left out) and the agent log holds a line
  with `recent_projects`, the right and `count` 1, without the folder's path. The effects table
  names the check (`tests/it/effects.rs`). By hand: not needed (the popup does not use the tool).

## Out of scope
A welcome screen without a project (v3: after the last window the folder chooser stands in for it), removing entries from the list, opening a project in the current
window (the reference asks), project groups, opening a single file without a project (v7: a file opens with its project, behaviour 18); (v5) cloning other than with Git, cloning with credentials typed into IDECraft (they stay in the credential helper or the SSH agent), project templates and types in New Project, remote projects in the Open Projects section.

## Known deviations from the reference
The reference asks whether to open a project in this window or a new one; IDECraft always opens a new
window (or brings the open one to the front), so no work in the current window is closed by accident
(IMP-0033). v3: when several windows close, IDECraft asks once for all of them before any closes,
like quitting (SPEC-0261 behaviour 14); the open platform asks per project as each closes, so a
Cancel in the middle leaves some closed and some open.
v2: the order in which the reference steps through its project windows was not compared.
v5: New Project makes an empty folder only, without the reference's project types and templates. Clone Repository takes only HTTPS and SSH addresses without credentials and never asks for a password itself; the reference's handling of other address forms and of credentials was not compared.
Not compared side by side yet.

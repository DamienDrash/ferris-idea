# SPEC-0262: State that survives any exit

- **Version:** 3 · **Date:** 2026-11-23 (v3: the deadline after a signal runs from the signal itself, also when the window's thread is blocked, and ends the process within 5 s, behaviour 3, AK-5; OBS-0117, where `kill <pid>` did not end the window); 2026-11-10 (v2: a signal that the platform's quit does not carry out
  in time still ends the process, behaviour 3; coordinator's window run of 2026-11-10, where
  `SIGTERM` left a window with running work open, see SPEC-0261 v4)
- **Sources:** OBS-0002, OBS-0003, OBS-0004 (after `SIGTERM` the reference restores an older
  session and a connection lost its user name → IMP-0004), STD (POSIX `rename` replaces a file
  atomically within one file system; `SIGTERM`, `SIGINT`, `SIGHUP` ask a process to end),
  SPEC-0008, SPEC-0094, SPEC-0129, SPEC-0105, SPEC-0167, SPEC-0205
- **Feature matrix rows:** F-087

## Purpose
However Ferris IDEA ends, the next start finds the state it had last, never a half-written or older
one.

## Behaviour
1. **Written when it changes:** recent projects (SPEC-0094), IDE settings (SPEC-0129), the user's
   keymap (SPEC-0105), the tool window layout of a project (SPEC-0167) and connection profiles
   (SPEC-0008) are written at the moment they change, not at quit. [IMP-0004]
2. **Atomically:** each of these files, and the saved terminal tabs (SPEC-0205), is written to a
   temporary file in the same folder and then renamed over the old one, so a reader or the next
   start sees either the old or the new content, never a mix; a failed write leaves the old file
   and no temporary file behind. [STD, IMP-0004]
3. **Signals end normally:** `SIGTERM`, `SIGINT` and `SIGHUP` sent to the window process quit the
   application the normal way (within 0.25 s), so quit work runs: terminal tabs are saved, language
   servers are shut down (SPEC-0263), the MCP HTTP server removes its discovery file (SPEC-0221).
   No question is asked (nobody may be there to answer), also not with running work or an open
   question in a window; unsaved editor text is not saved. If the process has not ended 5 s after
   the signal, the programs of run configurations are ended (SPEC-0261 v4 behaviour 13) and the
   process exits (SPEC-0261 v4 behaviour 17), so a signal never leaves a window open. [STD,
   IMP-0004]
   **v3:** the deadline is set by the thread that receives the signal, at the signal, not by the
   window's thread: a window thread that is blocked or busy (a modal loop, a stalled step) can no
   longer keep the process alive. After a signal the process has ended within 5 s — the normal quit
   if it finishes in time, otherwise after 4 s the programs of run configurations end and the
   process exits with status 0. Unsaved editor text is dropped as with `ferris-idea quit --discard`;
   whatever recovery data exists stays on disk. Until v2 the deadline was set only once the window's
   thread saw the signal, so a blocked window thread ignored `SIGTERM`.

## Acceptance criteria
- **AK-1** Atomic write: replaces content, creates the folder, leaves no temporary file, keeps the
  old file when the write fails (`lifecycle::tests`).
- **AK-2** A process that caught the signals stays alive on `SIGTERM` and reports it
  (`crates/app/src/termination.rs`: `termination::tests`).
- **AK-4** The 5 s deadline ends a process whose quit does not finish (`lifecycle::tests`); hand
  check: with a debug session and a long-running configuration, `kill <pid>` ends the window
  within 5 s (SPEC-0261 v4 AK-12).
- **AK-5 (v3, behaviour 3)** Process test `termination::tests::a_signal_ends_a_process_whose_window_thread_is_blocked`
  (`crates/app/src/termination.rs`): a child process catches the signals and then blocks its main
  thread; after `SIGTERM` it has exited with status 0 within 5 s. `lifecycle::tests`: the signal
  deadline is 4 s and shares the once-only arming with the quit deadline.
- **AK-3** Hand test (round 4, no. 191): open a project, change the tool window layout, `kill <pid>` the window,
  start again → same layout and terminal tabs.

## Out of scope
Terminal tabs are saved at quit (SPEC-0205), so after `SIGKILL` or a crash the tabs of the previous
quit come back. Unsaved editor text after a signal (no autosave yet). Trust decisions are written when
they change, in their own file (SPEC-0278).

## Known deviations from the reference
Any end — quit, `SIGTERM`, crash — restores the last written state; `SIGTERM` takes the normal quit
path (IMP-0004).

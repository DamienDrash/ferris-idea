# SPEC-0133: Debugging pages in Chrome, web frameworks

- **Version:** 3 · **Date:** 2026-11-10 (v2: Run of a page names the Debug key; v3: profile
  outside the project, Stop closes the browser, Rerun restarts the page session — hand test 60
  window run)
- **Sources:** GEN (IDEs debug the JavaScript of a page in Chrome with breakpoints in the project's
  source files — a "JavaScript Debug" configuration with a URL, or Debug on an HTML file — and know
  the project's web framework with its development server; F-036), STD (Chrome DevTools Protocol
  through js-debug's `pwa-chrome` launch: `url`, `webRoot`, `runtimeExecutable`, `userDataDir`,
  `cleanUp`), OSS (js-debug, MIT: the launching session owns the browser it starts in a process
  session of its own),
  SPEC-0091 v4 (js-debug), SPEC-0047 (run configurations), SPEC-0051 (Node environment),
  SPEC-0194 (Rerun), SPEC-0263 v2 (helper process lifetime), ADR-0008
- **Feature matrix rows:** F-036

## Purpose
Debug the front end of a web project the way the back end is debugged.

## Behaviour
1. **Pages in run configurations:** a configuration may have `url` (`http://`, `https://` or
   `file://`); with a URL the command may be empty. Run opens the URL in the default browser
   (`open` / `xdg-open`); Debug opens it in Chrome under js-debug. After Run of a page without a
   command the status bar says so and names the Debug key (v2, hand test 60: "Debug in Browser"
   chosen with Enter opened the user's browser and looked like a failed debug start). [GEN]
2. **Debugging a page:** Chrome (or Chromium/Edge; `IDECRAFT_CHROME` chooses) starts with a
   profile folder of its own — never the user's profile, so no cookies, extensions or logins of
   the user are touched — and the configuration's folder as web root, so breakpoints in the
   project's scripts bind (also through source maps). Stops, locals, watches, stepping and the
   console work as in SPEC-0091. Without Chrome the status bar says so. [STD]
   - **Profile location (v3):** one folder per project, `browser-profiles/<project folder
     name>-<16 hex digits of the SHA-256 of the project's full path>` in IDECraft's user cache
     folder (`$XDG_CACHE_HOME/idecraft`, else `~/Library/Caches/idecraft` on macOS, else
     `~/.cache/idecraft`). `browser-profiles` and the profile folder have mode 0700; a wider mode
     of an existing folder is tightened; a symbolic link, a file or a folder of another user in
     their place is refused and the session does not start. The profile holds whatever the
     developer signs into while debugging, so it never lives inside the project, where it could
     be committed. [GEN]
   - **Profile of an earlier version (v3):** an existing `<project>/.idecraft/browser-profile`
     is moved (renamed, nothing copied or deleted) to the new folder when the new folder does not
     exist yet, it is a real folder and no browser holds it (no `SingletonLock` entry); the debug
     console says where it went. Otherwise it stays where it is and every page session says in the
     console that it still holds cookies and logins, is not ignored by Git and is no longer used,
     with the reason it stayed. [GEN]
   - **Stop (v3):** ending the session (Stop, the page session ending, Rerun) ends js-debug's
     launching session too, which closes the whole browser it started (`cleanUp: wholeBrowser`);
     IDECraft then ends js-debug and, should the browser still run, ends it itself (`SIGTERM`,
     `SIGKILL` after 3 s). A browser IDECraft started is never left running after its session.
     Keeping it open would leave a second, unrelated Chrome window holding the project's profile
     and block the next session's browser (windowed Chrome takes a profile only once). [STD, OSS, GEN]
   - **Rerun (v3):** a new page session waits up to 15 s until the previous session on the same
     profile has ended and its browser is gone, then starts; after the wait the console says the
     previous browser is still closing. [GEN]
3. **HTML files:** ⌃⇧R in an HTML file opens it in the default browser, ⌃⇧D debugs it in Chrome.
4. **Frameworks:** the Node environment (SPEC-0051) names the web frameworks of `package.json`
   (Next.js, Nuxt, Angular, SvelteKit, Svelte, Vue, React, Solid) and suggests a configuration
   that starts the development server (`dev`, `start` or `serve` script with the project's package
   manager) and "Debug in Browser" with the server's usual address (Next.js, Nuxt, Create React App
   3000; Angular 4200; Vite and SvelteKit 5173; Vue CLI 8080). [GEN]
5. **MCP:** `node_environment` returns `frameworks`, `dev_config` and `browser_config`;
   `run_configuration` runs configurations with a URL. [ADR-0008]

## Acceptance criteria
- **AK-1** Frameworks, dev server configuration and page address for React with Vite, Angular,
  Next.js and a project without a framework (`crates/core/tests/it/node.rs`:
  `web_frameworks_and_their_pages`); a URL without a scheme is a problem of the run file.
- **AK-2** A page in headless Chrome stops at a breakpoint in its script with the locals and logs
  after resuming (`crates/debugger/tests/it/jsdebug.rs`: `browser_page_stops_at_breakpoint`).
- **AK-3** Window self-test (`browser_debug_steps`): ⌃⇧D in `index.html` stops in `twice` of
  `page.js` with `doubled = 42`; resume logs `page result 42` (Chrome without a window). v3: Rerun
  stops in `twice` again; after Stop no process uses the project's profile folder.
- **AK-4** Hand test (round 4): a React (Vite) project: start the dev server, Debug in Browser,
  breakpoint in a component, next to the reference.
- **AK-5** (v3) Ending a page session in headless Chrome leaves no browser on its profile and no
  js-debug, and a second session on the same profile stops at the breakpoint right after
  (`crates/debugger/tests/it/jsdebug.rs`: `browser_page_stops_at_breakpoint`); a Rerun through the
  session controller does the same (`crates/workbench/tests/it/browser_debug.rs`:
  `rerun_restarts_the_page_session`).
- **AK-6** (v3) The profile folder is per project, outside it, mode 0700; an old profile in the
  project moves once and is otherwise kept and named; a link in place of the folder is refused
  (`crates/debugger/src/browser.rs` tests).

## Out of scope
Vue, Angular and Svelte language servers and template support (next step), attaching to a running
Chrome, Firefox, React/Vue component trees, framework project wizards, hot reload control.

## Known deviations from the reference
Chrome runs with a separate profile per project in IDECraft's cache folder, so the user's browser
profile is never used by a debug session and the debug profile is never part of the project
(IMP-0047). Where the reference keeps its debug profile was not observed. Not compared side by side.

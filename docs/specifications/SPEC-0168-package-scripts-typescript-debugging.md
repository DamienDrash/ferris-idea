# SPEC-0168: Debugging package scripts and TypeScript files

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (IDEs run and debug the scripts of `package.json` from markers next to them and
  debug Node.js programs started by a package manager; F-036), STD (Debug Adapter Protocol;
  js-debug's `pwa-node` launch with `runtimeExecutable`/`runtimeArgs`, which debugs the child
  processes of the started program, each in a session of its own announced with
  `startDebugging`, and `resolveSourceMapLocations`; Node.js runs `.ts`, `.mts` and `.cts` files
  by stripping their types from version 22.18 on (23.6 on the current line) without a flag; npm,
  pnpm, yarn and bun run a script with `run <name>`, npm also with `start` and `test`),
  SPEC-0039 v5 (js-debug), SPEC-0091 v4 (Node.js debugging), SPEC-0097 (run the file in the
  editor), SPEC-0122 (▶ markers), SPEC-0051 (package manager), ADR-0008
- **Feature matrix rows:** F-036, F-070

## Purpose
A JavaScript or TypeScript project is debugged the way it is started: through a script of its
`package.json` with its own package manager, or by running a TypeScript file directly, without a
separate build step.

## Behaviour
1. **Package scripts:** Debug on a run configuration whose command is `npm|pnpm|yarn|bun run
   <script> …` (also `npm start` and `npm test`) starts the package manager under js-debug with the
   configuration's folder and environment. The Node.js processes the script starts are debugged:
   breakpoints in the project's files bind in them, stops, locals, stepping, watches and the
   console work as in SPEC-0091. [STD]
2. **Child processes:** when js-debug announces a child process, it becomes the debugged session
   (its breakpoints are set first); when that process ends, its parent is the debugged session
   again, so a script that runs several programs one after the other is followed through. The
   debug session ends with the started program. Output of all of them reaches the console.
   Stopping the session ends the started program too. Source maps are read only from the
   configuration's folder outside `node_modules`, so the package manager's own files do not fill
   the console with warnings. [STD]
3. **TypeScript files:** a configuration `node <file>.ts|.mts|.cts` runs and debugs like a
   JavaScript file; ⌃⇧R/⌃⇧D on a TypeScript file (not a test file, not `.d.ts`, not `.tsx`) runs it
   with `node`. Breakpoints bind on the TypeScript lines. A Node.js older than 22.18 reports the
   unknown file type in the console. [STD]
4. **Markers in `package.json`:** every script in the top-level `scripts` object of an open
   `package.json` gets a ▶ in the gutter on the line of its name, kept current while editing. A
   click offers "Run '<pm> run <script>'" and "Debug '<pm> run <script>'"; the package manager is
   the manifest's `packageManager`, else the one of the lockfile in that folder, else npm; the
   command runs in the folder of the `package.json`. [GEN]
5. **MCP:** `package_scripts` lists the scripts of a `package.json` with their 1-based line,
   command and run configuration; `debug_start` takes `npm_script` (with `program` the folder of
   the `package.json`) and debugs TypeScript files like JavaScript files. [ADR-0008]

## Acceptance criteria
- **AK-1** Debug plan: `node app.ts` is a Node.js program; `npm run dev`, `pnpm run build -- -w`,
  `yarn run serve`, `bun run dev`, `npm start`, `npm test -- x` are package scripts with `run`
  arguments, `npm install` and `yarn start` are not; the launch request names the package manager
  as runtime with the `run` arguments (`debug_model` unit test
  `package_scripts_and_typescript`).
- **AK-2** `npm run start` of a project whose script runs `node app.js` stops at a breakpoint in
  `twice` with `doubled = 42`, runs to its end with the output in the console and without source
  map warnings (`crates/debugger/tests/it/jsdebug.rs`: `npm_script_stops_at_breakpoint`).
- **AK-3** A TypeScript file stops at a breakpoint the same way
  (`typescript_file_stops_at_breakpoint`, skipped below Node.js 22.18); ⌃⇧R on `math.ts` runs
  `node src/math.ts`, `App.tsx` and `env.d.ts` have no configuration
  (`crates/core/tests/it/run_context.rs`).
- **AK-4** Scripts with their lines, `scripts` keys nested elsewhere ignored, escaped quotes,
  package manager from `packageManager` and lockfile, configuration folder (unit test
  `package_scripts_with_lines`).
- **AK-5** Window self-test (`package_script_debug_steps`): an open `package.json` shows ▶ on its
  two scripts, the marker of `start` (`node app.ts`) offers Run and Debug with npm, Debug stops in
  `twice` of `app.ts` with `y = 42`, resume ends with "script result 42" in the console.
- **AK-6** MCP protocol test: `package_scripts` lists the scripts with their run configuration,
  `debug_start` with `npm_script` returns a launch with the package manager as runtime.
- **AK-7** Hand test (round 4): a Vite or Express project with TypeScript: ▶ next to `dev`, Debug,
  breakpoint in a server file, stepping and evaluating, next to the reference.

## Out of scope
`tsx`/`ts-node` loaders and TypeScript features that need a transform (enums, namespaces, JSX in
Node), Deno, debugging tests of Jest/Vitest from the result tree, attaching to a running dev
server's browser page (SPEC-0133), npm workspaces' script lists across packages in one view.

## Known deviations from the reference
TypeScript files run on Node's own type stripping instead of a bundled TypeScript loader, so
nothing is installed into the project. Not compared side by side.

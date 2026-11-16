# SPEC-0141: Vue, Svelte and Angular language servers

- **Version:** 4 · **Date:** 2026-11-10 (v1 2026-11-02; v2: script and style blocks in their own
  colours; v3: SCSS and Less coloured like CSS, hand test Nr. 95; v4: Vue diagnostics asked by
  pull, hover at the start of a template name, findings of the hand test Nr. 67 window run)
- **Sources:** GEN (IDEs understand Vue single-file components, Svelte components and Angular
  templates: outline, hover, go to definition, completion, diagnostics; F-036), STD (Language
  Server Protocol; `@vue/language-server` 2.x with `initializationOptions` `typescript.tsdk` and
  `vue.hybridMode = false`, the standalone mode; `svelte-language-server` (`svelteserver
  --stdio`); `@angular/language-server` (`ngserver --stdio --tsProbeLocations …
  --ngProbeLocations …`), which serves the templates of a workspace with `angular.json`),
  SPEC-0042 (server registry), SPEC-0133 (web frameworks), ADR-0005
- **Feature matrix rows:** F-036

## Purpose
The front-end frameworks SPEC-0133 recognises get their language intelligence in the editor.

## Behaviour
1. **Languages:** `.vue` is Vue, `.svelte` is Svelte (LSP ids `vue`, `svelte`); both are
   highlighted with the HTML grammar for now. [STD]
2. **Servers:** Vue → `vue-language-server --stdio` in standalone mode with the TypeScript
   installed next to the server; Svelte → `svelteserver --stdio`. Version 2.x of the Vue server
   is used because 3.x answers only together with a TypeScript plugin inside the editor's own
   TypeScript server. [STD]
3. **Angular:** in a workspace with `angular.json`, HTML files go to `ngserver` when it is
   installed (else to the HTML server as before); it gets the workspace and the server's own
   `node_modules` as probe locations, so the project's TypeScript and Angular language service win
   and the installed ones are the fallback. [STD]
4. **Installation:** `js.sh install-servers` of the reference environment installs the three servers
   (pinned versions, `--ignore-scripts`); svelte-language-server pulls one package from GitHub, so
   that command alone runs with `--allow-git=all` (approved by the project owner, 2026-11-02); the global npm
   setting `allow-git = none` stays. `js.sh install-angular` sets up the reference project
   `angular-basic`.
5. **MCP and windows:** everything that uses language servers (hover, definition, completion,
   symbols, diagnostics, `language_servers`) works for the new languages through SPEC-0042.
6. **Embedded blocks (v2):** in Vue, Svelte and HTML files the contents of `<script>` and `<style>`
   elements are highlighted with their own grammar: script as JavaScript, as TypeScript with
   `lang="ts"`/`"typescript"` (TSX with `"tsx"`) or a TypeScript `type`, as JSON with a JSON
   `type`; style as CSS, as SCSS or Less with `lang="scss"`/`"less"`. Other script types (templates)
   and other style languages stay plain. A change inside a block repaints the whole block. The tags
   around them keep the HTML colours. [GEN, STD]
7. **SCSS and Less like CSS (v3):** SCSS and Less files and blocks get CSS's colours for selectors,
   properties, colours, numbers and units, plus their own for variables, functions, mixins and
   control words; a declared variable (`$farbe: …`, `@farbe: …`) is a variable, not a property.
   Before, only functions, variables and plain values were coloured. [GEN, OSS]
8. **Standalone, not hybrid (v4):** the Vue server stays in standalone mode (behaviour 2): it loads
   the TypeScript next to it and serves script and template itself, so IDECraft runs no second
   TypeScript server for `.vue` files and needs no `tsserver/request` forwarding. Checked on
   2026-11-10 against a `npm create vue@latest` project (TypeScript 6 and `vue-tsc` 3 in the
   project, solution-style `tsconfig.json` with references): hover on `defineProps`, on a prop in
   `{{ }}` and on a component tag, and type errors in the template, all from the one server. The
   hybrid mode would add a second process and a request bridge for no gain while the standalone
   mode answers. Svelte (`svelteserver`) and Angular (`ngserver`) also serve their templates
   themselves (hover on `{{ title() }}` and `{@render children()}`, misspelt names reported in a
   SvelteKit and an `ng new` project); neither needs a TypeScript plugin. [STD]
9. **Vue diagnostics by pull (v4):** the Vue server publishes a file's diagnostics in steps — first
   the template and syntax ones (often an empty list), then the type errors. Diagnostics asked once
   (MCP `get_diagnostics`, the context of code actions, a question without the editor) therefore
   come from a `textDocument/diagnostic` request, which the server answers with the complete list;
   when the request fails, the first publication is taken as for other servers. The editor keeps
   showing every publication, the last one winning. Before v4 such a question got the first, empty
   step and missed type errors. [STD]
10. **Hover at the start of a template name (v4):** on the first character of a name in a template
   expression (`m` of `{{ msg }}`) the Vue server answers with its generated context variable
   (`const __VLS_ctx: …`) instead of the name. When a Vue hover names a `__VLS_` declaration and the
   next character still belongs to the same name, IDECraft asks once more one character further
   right and shows that answer (`(property) msg: string`). [STD]

## Acceptance criteria
- **AK-1** Vue: outline with `template` and `script setup`, hover with the JSDoc of a function,
  definition and completion in `<script setup lang="ts">` (`crates/languages/tests/it/servers.rs`:
  `vue_language_server`).
- **AK-2** Svelte: outline, hover, definition (`svelte_language_server`).
- **AK-3** Angular: `ngserver` chosen for a template of `angular-basic`; hover on `{{ title }}`
  names the component property and its type (`angular_language_server`).
- **AK-4** Detection of `.vue`/`.svelte` and HTML highlighting (unit tests in `lib.rs`/`syntax.rs`).
- **AK-5** Hand test (round 4): a Vue (Vite) and an Angular project next to the reference.
- **AK-6 (v2)** A Vue component with `<script setup lang="ts">` and `<style lang="scss">`, a
  Svelte script and an HTML file with a template script and a style: keywords, types and
  properties in their colours, the template plain, a partial range and an edit inside the block
  correct (unit test `embedded_script_and_style_blocks` in `syntax.rs`).
- **AK-7 (v3)** SCSS, Less and a Vue `lang="scss"` block colour the variable, colour value,
  selectors, properties, function and number (unit test `scss_and_less_are_coloured_like_css`).
- **AK-8 (v4)** A Vue file whose script holds a type error: one-time diagnostics name it, without
  and with the server pool (`crates/languages/tests/it/vue.rs`: `vue_diagnostics_are_complete`,
  `crates/languages/tests/vue_pool.rs`: `pooled_vue_diagnostics_are_complete`; real server,
  skipped when it is not installed); only the Vue server pulls (unit test
  `servers::tests::only_vue_and_typescript_7_pull_diagnostics`).
- **AK-9 (v4)** Hover on the first character of a template name never shows a `__VLS_` declaration
  (`tests/vue.rs`: `vue_template_hover_names_the_prop`, real server); in a Vue project with its `node_modules`
  (`IDECRAFT_TEST_VUE`, e.g. a `npm create vue@latest` project) hover on `defineProps`, on `msg` in
  `{{ msg }}` and on `<HelloWorld>` and a template type error (`vue_project_template_features`,
  skipped without such a project). Unit test `insight::tests::vue_internal_hovers_are_recognised`.

## Out of scope
Highlighting of the template syntax (`{{ }}`, `v-` directives, Svelte blocks) with its own
grammar, the Vue 3.x hybrid mode, Angular in `.ts` component files (the TypeScript server serves those),
framework-specific refactorings and project wizards.

## Known deviations from the reference
Template expressions are highlighted as HTML text until a template grammar is added (the
language server still serves them). Not compared side by side. Problems in templates are the Vue
compiler's: an attribute written right after a quoted value without a space (`class="green"x>`)
is a second attribute `x` to it, not an error, so no problem is shown there (v4; whether the
reference marks it is not observed — open question in PO-STATUS).

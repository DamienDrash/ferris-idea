# js-lint

ESLint 9 (flat config) and Prettier 3 for SPEC-0123. `src/app.js` has an unused variable
(`no-unused-vars`, error), a `let` that is never reassigned (`prefer-const`, error, fixable) and
`==` (`eqeqeq`, warning), and is not formatted. Installed copy for tests:
`js.sh install` of the reference environment (into `~/ReferenceEnv/projects/js-lint`).

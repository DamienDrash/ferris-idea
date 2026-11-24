# Tools of the server CI

**Status:** v1 · 2026-11-14

The server CI (the project's own build server, ADR-0030) runs the whole workspace on Linux. Many live tests
start a real language server, debug adapter or build tool. This page lists the tools the CI
provides and what happens to a test whose tool is missing.

## A missing tool fails the run

The CI sets `FERRIS_IDEA_REQUIRE_TOOLS=1`. A live test whose tool is missing then fails with
`required tool missing: …` instead of returning early as skipped. A gap shows as a red run, not as
a green run that tested less.

The tests whose tool the CI does not provide are left out by name in a skip list kept with the
build server's scripts, each with the tool it needs. Installing a tool means removing its lines
there. The CI log still names live tests that skipped for another reason (`skipped-live=N`, read
from the JUnit report that `.config/nextest.toml` writes).

## Provided

Installed without root privileges from each project's official source. Every archive was checked
against the checksum its source publishes, except where noted.

| Tool | Version | Used by |
|---|---|---|
| Temurin JDK | 21.0.13 (`JAVA_HOME`), 25.0.5 (jdtls only) | Java, Kotlin, Gradle, Maven |
| jdtls | 1.63.0 | Java |
| java-debug plugin | 0.53.2 | Java debugging |
| Maven | 3.9.16 | Maven projects |
| Gradle | 9.9.1 | Gradle projects |
| kotlin-language-server | 1.3.13 (the release has no checksum; the hash is recorded on the server) | Kotlin |
| kotlin-debug-adapter | 0.4.4 (the release has no checksum; the hash is recorded on the server) | Kotlin debugging |
| rust-analyzer | 1.99.1 (rustup component) | Rust |
| clangd | 23.1.0 | C and C++ |
| Go, gopls | 1.27.4, 0.24.0 | Go |
| uv | 0.13.8 | Python tools |
| ruff, black, python-lsp-server, pytest | 0.17.4, 25.9.0, 1.13.1, 8.4.2 | Python |
| pyright | 1.1.414 | Python |
| typescript, typescript-language-server | 5.6.3, 5.3.0 | JavaScript and TypeScript |
| js-debug | 1.140.1 | JavaScript debugging |
| intelephense | 1.12.6 | PHP |
| @vue/language-server | 2.2.0 | Vue |
| svelte-language-server | 0.17.8 | Svelte |
| vscode-langservers-extracted | 4.10.0 | HTML, CSS, JSON |
| yaml-language-server | 1.19.2 | YAML |
| bash-language-server | 5.4.3 | Shell |
| dockerfile-language-server-nodejs | 0.13.0 | Dockerfile |

The npm packages are installed with `--save-exact`.

## Not provided

These tests are left out of the CI, by name, in the build server's skip list. They run where the
tool is installed, for example on a developer machine or in the macOS window check.

| Tool | Why it is missing | Tests |
|---|---|---|
| Python `venv` (`ensurepip`) | The system package needs root | `python::real_virtual_environment`, `python::create_choose_and_install_in_a_real_environment` |
| debugpy | Not installed | Python debugging in `ferris-idea-debugger` and `ferris-idea-mcp` |
| lldb-dap | Not installed | Native debugging (`lldb::…`, `attach::attach_to_a_native_process_by_id`) |
| ruby-lsp, bundler, solargraph | Not installed | `ruby::…` |
| lemminx, lua-language-server, marksman, metals, taplo, groovy-language-server, R | Not installed | `servers::…` |
| Angular language server and its reference project | Not installed | `servers::angular_language_server` |
| A Vue project with `node_modules` (`FERRIS_IDEA_TEST_VUE`) | Not set up | `vue::vue_project_template_features` |
| dbus-daemon, gnome-keyring-daemon | The system packages need root | `secret_service::secret_service_on_a_private_bus` |

## Under load

The tests that start heavy servers (jdtls, kotlin-language-server, Gradle, Maven, intelephense,
rust-analyzer) form the test group `heavy-servers` in `.config/nextest.toml`. At most two of them
run at the same time, with a longer time limit. Before tools are switched on in the CI, these
tests run three times in a row under CPU load on every core. The remote-project test
`remote_project::a_folder_on_an_ssh_host_opens_survives_breaks_and_stays_private` is part of those
runs.

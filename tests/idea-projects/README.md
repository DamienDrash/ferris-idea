# Projects in the reference IDE's file formats

Fixtures for SPEC-0049 v3 (`crates/core/tests/it/ideaproject.rs`). They are written by hand from
the open project format of `JetBrains/intellij-community` (format knowledge, no code); no file
here was written by the reference IDE.

- `ipr-basic`: the file-based format: an `.ipr` with two modules, a project library, a version
  control mapping and a shared run configuration, its `.iws` with a personal one.
- `run-configs`: the folder format with a run configuration of every type Ferris IDEA translates,
  some it does not, facets, and a `workspace.xml` whose only used part is `RunManager`.

Nothing in them is run by the tests; `grd/gradlew` is a stand-in.

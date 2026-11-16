# SPEC-0052: Java development kits and the project's Java version

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (IDEs detect installed JDKs, pick one matching the project and warn when it cannot build
  it; F-037), STD (the `release` file of a Java runtime image with `JAVA_VERSION`, `IMPLEMENTOR`,
  `OS_ARCH`; Java version strings incl. the `1.x` scheme; the macOS bundle layout `Contents/Home`;
  `JAVA_HOME`; Maven compiler properties and plugin configuration; Gradle `JavaLanguageVersion.of(n)`
  toolchains and `sourceCompatibility`/`targetCompatibility`; jenv's `.java-version`; SDKMAN's
  `candidates/java` and `.sdkmanrc`), SPEC-0049 (`languageLevel` in `.idea/misc.xml`)
- **Feature matrix rows:** F-037

## Purpose
Know which JDKs are installed, which Java version a project needs and whether the chosen JDK can build it,
without starting a JVM.

## Behaviour
1. JDKs are found from a configured home, `JAVA_HOME`, the system and user JVM folders
   (`/Library/Java/JavaVirtualMachines`, `~/Library/Java/JavaVirtualMachines`, `/usr/lib/jvm`, `~/.jdks`),
   SDKMAN candidates (without the `current` link), Gradle-provisioned JDKs and `java` on `PATH` (resolved to
   its home; launcher stubs without a `release` file are skipped). A folder counts when it, or its
   `Contents/Home`, has `bin/java`. Duplicates (same resolved home) are dropped. [STD]
2. Each JDK reports home, version, feature release (`1.8.0_412` → 8), vendor, whether `javac` exists and
   the architecture, all from files. No JVM is started. [STD; same principle as IMP-0025]
3. The project's Java version is read from `pom.xml` (`maven.compiler.release`/`source`/`target`,
   `java.version`, compiler plugin `release`/`source`), Gradle build scripts (toolchain
   `JavaLanguageVersion.of(n)`, `sourceCompatibility`, `targetCompatibility` as literals),
   `.java-version`, `.sdkmanrc` and `languageLevel` of `.idea/misc.xml`. Requirements are listed strictest
   first; different versions across these files are a problem listing all of them. [STD, improvement
   IMP-0027]
4. Selection: configured; a JDK with a compiler and exactly the required feature release; else the closest
   newer one; else `JAVA_HOME`, `java` on `PATH`, the newest JDK. If no JDK can build the required
   version, that is a problem. The reason is reported. [GEN]
5. Notes for the selected JDK: a runtime without `javac`, an unknown version (no `release` file), an
   architecture different from the machine (emulated or not runnable). [GEN]
6. MCP tool `java_environment` (optional `jdk`). [ADR-0008]

## Acceptance criteria
- **AK-1** JDKs from `JAVA_HOME` (runtime only), macOS bundles, SDKMAN with `current` link, PATH symlink;
  versions incl. `1.8`, vendor, compiler flag.
- **AK-2** Selection: `JAVA_HOME` without requirement, exact match for Maven release 17, closest newer for a
  Gradle toolchain 20, problem for Java 25.
- **AK-3** Requirements from all listed files in order, with the contradiction problem.
- **AK-4** The installed JDK: the version from its `release` file equals `java.version` of a program
  compiled and run with that JDK.
- **AK-5** The MCP tool reports requirements and problems.

## Out of scope
Downloading JDKs, Kotlin compiler settings, Gradle toolchain resolution rules beyond the literal version,
Maven profiles and properties defined elsewhere, the Java language server (M4.5), running builds.

## Known deviations from the reference
Nothing is started for discovery; contradicting version statements are reported together (IMP-0027); not
compared side by side.

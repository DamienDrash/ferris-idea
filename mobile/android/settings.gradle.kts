// The Ferris IDEA Android app (ADR-0029, SPEC-0326). Build with `scripts/build-mobile.sh android-apk`,
// which builds the Rust core and the Kotlin bindings into generated/ first.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "FerrisIDEA"
include(":app")

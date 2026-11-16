// The IDECraft Android app (ADR-0029, SPEC-0326): Jetpack Compose over the Rust core.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The app's version is the workspace's, so the daily minor version reaches the APK too.
val workspaceVersion: String =
    Regex("""(?m)^version = "([^"]+)"""")
        .find(rootDir.resolve("../../Cargo.toml").readText())
        ?.groupValues?.get(1)
        ?: error("no version in the workspace Cargo.toml")

// A debug build is signed only with a throwaway key that scripts/build-mobile.sh makes in a temp
// folder, never with the shared ~/.android/debug.keystore. The release APK stays unsigned.
val throwawayStore: String? = providers.environmentVariable("IDECRAFT_ANDROID_STORE").orNull
val throwawayPassword: String? = providers.environmentVariable("IDECRAFT_ANDROID_STORE_PASSWORD").orNull

android {
    namespace = "dev.idecraft.mobile"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.idecraft.mobile"
        // 28: Keystore keys that need an unlocked device (KeystoreVault).
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = workspaceVersion
        ndk { abiFilters += "arm64-v8a" }
    }

    signingConfigs {
        getByName("debug") {
            if (throwawayStore != null && throwawayPassword != null) {
                storeFile = file(throwawayStore)
                storePassword = throwawayPassword
                keyAlias = "idecraft-throwaway"
                keyPassword = throwawayPassword
            } else {
                // Points nowhere, so a build outside the script fails instead of using a shared key.
                storeFile = file("missing-throwaway-key-use-scripts-build-mobile-sh.keystore")
            }
        }
    }

    buildTypes {
        debug {
            signingConfig = signingConfigs.getByName("debug")
        }
        release {
            signingConfig = null
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir("../generated/kotlin")
            jniLibs.srcDir("../generated/jniLibs")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    buildFeatures {
        compose = true
        buildConfig = false
    }

    packaging {
        jniLibs { useLegacyPackaging = false }
        resources { excludes += "/META-INF/{AL2.0,LGPL2.1}" }
    }

    lint {
        abortOnError = true
        checkReleaseBuilds = true
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.6")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("androidx.compose.ui:ui:1.7.5")
    implementation("androidx.compose.foundation:foundation:1.7.5")
    implementation("androidx.compose.material3:material3:1.3.1")
}

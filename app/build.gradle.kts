// Meetings — sideloaded Android app for Supernote (Android 11, arm64).
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.jraghavan.snmeeting"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.jraghavan.snmeeting"
        minSdk = 30 // Supernote runs Android 11
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            // The Supernote is arm64; the core is built for nothing else (F1-FR4).
            abiFilters += "arm64-v8a"
        }
    }

    // libmeeting.so is staged into src/main/jniLibs/ by buildApk.sh (cargo-ndk) before this runs.
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // Don't let Gradle recompress the prebuilt .so (it is already stripped).
    packaging {
        jniLibs.useLegacyPackaging = false
    }

    // F8-FR4: the spike shipped an API-31 call to the API-30 device; make that a build failure.
    lint {
        error += "NewApi"
        abortOnError = true
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    // android.jar's org.json is a stub in JVM unit tests ("Stub!"); the real library replaces it there.
    testImplementation("org.json:json:20240303")
}

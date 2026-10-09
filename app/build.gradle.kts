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
        // F14-FR1, as inkread: release.yml passes the tag through ORG_GRADLE_PROJECT_meetings*
        // env vars, which Gradle maps to these project properties (versionCode = the workflow run
        // number). Local builds fall back to a clearly marked -dev version with code 1.
        versionCode = (project.findProperty("meetingsVersionCode") as String?)?.toInt() ?: 1
        versionName = (project.findProperty("meetingsVersionName") as String?) ?: "0.1.0-dev"
        // The GitHub repo whose releases/latest the updater reads (ADR D13: a public
        // releases-only repo can stand in while the source repo is private).
        val updateRepo = (project.findProperty("meetingsUpdateRepo") as String?)?.takeIf { it.isNotBlank() }
            ?: "j-raghavan/sn-meeting"
        buildConfigField("String", "UPDATE_REPO", "\"$updateRepo\"")
        // SHA-256 of the release signing certificate (public: it is in every signed APK).
        // release.yml derives it from the keystore; the default is the key made on 2026-10-08
        // (ADR D12). Only used to tell the user a copy signed with another key (a local debug
        // build) can't update itself; installs are gated on the installed signers (F14-FR6).
        val certPin = ((project.findProperty("meetingsReleaseCertSha256") as String?)?.takeIf { it.isNotBlank() }
            ?: "C1:54:9C:C8:F8:DA:3B:4C:94:AF:64:0D:2D:AD:08:FB:4B:5A:BF:D9:DC:AB:21:5C:B9:F2:FD:D1:40:C3:B8:1B")
            .replace(":", "").lowercase()
        require(Regex("[0-9a-f]{64}").matches(certPin)) { "meetingsReleaseCertSha256 must be a SHA-256 fingerprint" }
        buildConfigField("String", "RELEASE_CERT_SHA256", "\"$certPin\"")
        ndk {
            // The Supernote is arm64; the core is built for nothing else (F1-FR4).
            abiFilters += "arm64-v8a"
        }
    }

    // libmeeting.so is staged into src/main/jniLibs/ by buildApk.sh (cargo-ndk) before this runs.
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    // Release signing (F14-FR1, as inkread). release.yml decodes the keystore secret to a file and
    // exports these; local builds leave them unset and fall back to the debug key. A stable
    // release key is what lets the in-app updater work: Android refuses an update whose signer
    // differs from the installed app's.
    val keystorePath = System.getenv("MEETINGS_KEYSTORE_FILE")
        ?: project.findProperty("meetingsKeystoreFile") as String?
    signingConfigs {
        create("release") {
            if (!keystorePath.isNullOrBlank()) {
                storeFile = file(keystorePath)
                storePassword = System.getenv("MEETINGS_KEYSTORE_PASSWORD")
                    ?: project.findProperty("meetingsKeystorePassword") as String?
                keyAlias = System.getenv("MEETINGS_KEY_ALIAS")
                    ?: project.findProperty("meetingsKeyAlias") as String?
                keyPassword = System.getenv("MEETINGS_KEY_PASSWORD")
                    ?: project.findProperty("meetingsKeyPassword") as String?
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // The release key when a keystore is configured (CI); else the debug key for local
            // sideloading. A debug-signed install never accepts an update (F14-FR6 fails closed).
            signingConfig = if (!keystorePath.isNullOrBlank()) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    buildFeatures {
        buildConfig = true // UPDATE_REPO, RELEASE_CERT_SHA256 (F14-FR1)
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

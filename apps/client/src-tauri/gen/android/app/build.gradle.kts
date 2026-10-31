import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// The release signing key, kept outside the repository: keystore.properties
// (git ignores it) names the keystore file, the alias and the password.
// Without it a release build comes out unsigned and cannot be installed.
val keystoreProperties = Properties().apply {
    val propFile = rootProject.file("keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 37
    namespace = "com.synaelle.ficelle.dev"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        // release builds install as the real app, debug ones next to it as .dev,
        // the same split as the desktop builds. The code's namespace stays.
        applicationId = "com.synaelle.ficelle"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
        // issue reports to a picked folder (DevReports.kt), off in releases below
        buildConfigField("boolean", "DEV_REPORTS", "true")
    }
    signingConfigs {
        if (keystoreProperties.containsKey("storeFile")) {
            create("release") {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("password")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("password")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfigs.findByName("release")?.let { signingConfig = it }
            resValue("string", "app_name", "Ficelle")
            resValue("string", "main_activity_title", "Ficelle")
            buildConfigField("boolean", "DEV_REPORTS", "false")
            optimization {
               enable = true
            }
            // the native library compressed in the APK, about half the
            // download for each architecture; Android unpacks it at install
            packaging {
                jniLibs.useLegacyPackaging = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
        // the app's name comes from the build type, see below
        resValues = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))

// A preview (gradle -Ppreview): the release build, under its own id and name
// and signed with the SDK's debug key, so it installs next to the real app
// with notes of its own, and the yellow knot of the previews (src/preview/res,
// from app-icon-preview.svg and app-icon-android-foreground-preview.svg). For
// trying main on a phone, never published.
if (project.hasProperty("preview")) {
    android.sourceSets.getByName("release").res.srcDirs("src/preview/res")
    android.buildTypes.getByName("release") {
        applicationIdSuffix = ".preview"
        resValue("string", "app_name", "Ficelle Preview")
        resValue("string", "main_activity_title", "Ficelle Preview")
        buildConfigField("boolean", "DEV_REPORTS", "true")
        signingConfig = android.signingConfigs.getByName("debug")
    }
}

// Debug builds install next to the real app, under their own id and name.
// Set here rather than in buildTypes above: the tauri cli rewrites the debug
// block at each android build.
android.buildTypes.getByName("debug") {
    applicationIdSuffix = ".dev"
    resValue("string", "app_name", "Ficelle Dev")
    resValue("string", "main_activity_title", "Ficelle Dev")
}

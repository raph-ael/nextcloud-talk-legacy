import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "org.sirplus.talk"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "org.sirplus.talk"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Until a release keystore exists: installable debug-signed release builds.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures { compose = true }
    sourceSets["main"].java.srcDir(layout.buildDirectory.dir("generated/uniffi"))
    packaging { jniLibs { useLegacyPackaging = false } }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.compose.icons)
    implementation(libs.profileinstaller)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.emojipicker)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    implementation("${libs.jna.get()}@aar")
}

// --- Rust core: cargo-ndk builds libtalk_ffi.so, uniffi-bindgen generates Kotlin -----

val coreDir = rootProject.file("../core")
val cargo = "${System.getProperty("user.home")}/.cargo/bin/cargo"
val jniLibsDir = file("src/main/jniLibs")
val bindingsDir = layout.buildDirectory.dir("generated/uniffi").get().asFile

val buildRustCore by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds talk-ffi for Android ABIs with cargo-ndk"
    workingDir = coreDir
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    commandLine(
        cargo, "ndk",
        "-t", "arm64-v8a", "-t", "x86_64",
        "--platform", "26",
        "-o", jniLibsDir.absolutePath,
        "build", "-p", "talk-ffi", "--release",
    )
    inputs.dir(File(coreDir, "talk-core/src"))
    inputs.dir(File(coreDir, "talk-ffi/src"))
    outputs.dir(jniLibsDir)
}

val generateBindings by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generates Kotlin bindings (from an unstripped host build of the same crate)"
    dependsOn(buildRustCore)
    workingDir = coreDir
    commandLine(
        "sh", "-c",
        "'$cargo' build -q -p talk-ffi && '$cargo' run -q -p talk-ffi --bin uniffi-bindgen -- " +
            "generate --library target/debug/libtalk_ffi.so --language kotlin --no-format " +
            "--out-dir '${bindingsDir.absolutePath}'",
    )
    inputs.dir(File(coreDir, "talk-ffi/src"))
    outputs.dir(bindingsDir)
}

tasks.named("preBuild") { dependsOn(generateBindings) }

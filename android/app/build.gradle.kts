import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

// Push (FCM): the Firebase config and the push proxy are deployment data and stay out of the
// public repo. google-services.json goes to app/ (gitignored); the proxy URL comes from the
// Gradle property or environment variable TALK_PUSH_PROXY_URL, or from local.properties
// (talk.pushProxyUrl). Without both, the app builds and runs without push. A config for
// another package name (e.g. left over from before the rename) is ignored with a warning.
val appId = "org.wolkenfunk.app"
val googleServices = file("google-services.json")
if (googleServices.exists()) {
    if (googleServices.readText().contains("\"$appId\"")) {
        apply(plugin = "com.google.gms.google-services")
    } else {
        logger.warn("google-services.json has no client for $appId: building without push")
    }
}
val localProps = Properties().apply {
    rootProject.file("local.properties").takeIf { it.exists() }?.inputStream()?.use { load(it) }
}
val pushProxyUrl = (findProperty("TALK_PUSH_PROXY_URL") as String?)
    ?: System.getenv("TALK_PUSH_PROXY_URL")
    ?: localProps.getProperty("talk.pushProxyUrl")
    ?: ""

// Release signing: the keystore and its passwords come from the environment (see
// scripts/release-android.sh), never from the repo. Without them the release build stays unsigned.
fun signingValue(name: String): String? =
    (findProperty(name) as String?)?.takeIf { it.isNotBlank() } ?: System.getenv(name)?.takeIf { it.isNotBlank() }
val releaseKeystore = signingValue("WOLKENFUNK_KEYSTORE")?.let { file(it) }?.takeIf { it.exists() }
val releaseSigning = listOf("WOLKENFUNK_KEYSTORE_PASSWORD", "WOLKENFUNK_KEY_ALIAS", "WOLKENFUNK_KEY_PASSWORD")
    .map { signingValue(it) }
    .takeIf { releaseKeystore != null && it.all { v -> v != null } }
    ?.map { it!! }

// -Pwolkenfunk.abiSplits=true (release script): one APK per phone ABI plus a universal APK.
// Splits and ndk.abiFilters exclude each other, so there packaging drops all other ABIs (the
// emulator's x86_64 and what libraries such as JNA still ship for x86, armeabi and mips).
val abiSplits = (findProperty("wolkenfunk.abiSplits") as String?) == "true"
val phoneAbis = listOf("arm64-v8a", "armeabi-v7a")

android {
    namespace = appId
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = appId
        minSdk = 26
        targetSdk = 36
        versionCode = 300
        versionName = "0.3.0"
        buildConfigField("String", "PUSH_PROXY_URL", "\"$pushProxyUrl\"")
        // armeabi-v7a: low-end devices with a 32-bit-only system (e.g. Galaxy A13 with Android 13).
        if (!abiSplits) ndk { abiFilters += phoneAbis + "x86_64" }
    }

    if (abiSplits) {
        splits {
            abi {
                isEnable = true
                reset()
                include(*phoneAbis.toTypedArray())
                isUniversalApk = true
            }
        }
    }

    signingConfigs {
        if (releaseSigning != null) {
            create("release") {
                storeFile = releaseKeystore
                storePassword = releaseSigning[0]
                keyAlias = releaseSigning[1]
                keyPassword = releaseSigning[2]
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    sourceSets["main"].java.srcDir(layout.buildDirectory.dir("generated/uniffi"))
    packaging {
        jniLibs {
            useLegacyPackaging = false
            if (abiSplits) excludes += listOf("x86_64", "x86", "armeabi", "mips", "mips64").map { "lib/$it/**" }
        }
    }
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
    implementation(libs.work.runtime)
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.session)
    implementation(libs.webrtc)
    implementation(platform(libs.firebase.bom))
    implementation(libs.firebase.messaging)
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
        "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86_64",
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

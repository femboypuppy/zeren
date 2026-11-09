plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.compose.compiler)
}

// The Rust mobile core (crates/mobile) — the library the iOS app links —
// built for Android with its generated Kotlin bindings. `-PzerenSkipCore`
// reuses the last build while iterating on Kotlin only.
val NDK_VERSION = "29.0.14206865"
val repoRoot = rootProject.projectDir.resolve("../..").canonicalFile
val coreOut = repoRoot.resolve("target/android-core")
val iconsOut = layout.buildDirectory.dir("generated/zeren-icons")
val skipCore = providers.gradleProperty("zerenSkipCore").isPresent

// The app ships the workspace's version (Cargo.toml), like the desktop apps;
// CI passes a build number that only grows (`-PzerenVersionCode=<n>`).
val workspaceVersion = Regex("(?m)^version = \"([^\"]+)\"")
    .find(repoRoot.resolve("Cargo.toml").readText())?.groupValues?.get(1) ?: "0.0.0"
val buildNumber = providers.gradleProperty("zerenVersionCode").orNull?.toInt() ?: 1

// Release signing: Zeren's upload key from the environment (CI decodes it from
// secrets), so each build installs over the last. Without it, the debug key.
val releaseKeystore = System.getenv("ZEREN_ANDROID_KEYSTORE")?.takeIf { file(it).exists() }

val buildCore by tasks.registering(Exec::class) {
    description = "Builds crates/mobile for Android and generates Kotlin bindings."
    val ndk = System.getenv("ANDROID_NDK_HOME")
        ?: androidComponents.sdkComponents.sdkDirectory.get().dir("ndk/$NDK_VERSION").asFile.path
    // Any ABI will do when skipping (`ZEREN_ANDROID_ABIS=x86_64` builds only that one).
    val libs = coreOut.resolve("jniLibs")
    val skip = skipCore
    workingDir = repoRoot
    commandLine("bash", "scripts/android/build-core.sh", coreOut.path)
    environment("ANDROID_NDK_HOME", ndk)
    onlyIf { !skip || libs.walkTopDown().none { it.name == "libzeren_mobile.so" } }
}

// Tool and file icons: the iOS asset catalog's SVGs, rasterized.
val genIcons by tasks.registering(Exec::class) {
    description = "Rasterizes the shared transcript icons."
    val out = iconsOut.get().asFile
    inputs.dir(repoRoot.resolve("apps/ios/Zeren/Assets.xcassets"))
    inputs.dir(repoRoot.resolve("crates/ui/assets/icons"))
    inputs.dir(repoRoot.resolve("apps/android/icons"))
    inputs.file(repoRoot.resolve("scripts/android/svg2vd.py"))
    outputs.dir(out)
    commandLine("bash", repoRoot.resolve("scripts/android/gen-icons.sh").path, out.resolve("assets/icons").path, out.resolve("res").path)
}

android {
    namespace = "sh.zeron.android"
    compileSdk = 37
    ndkVersion = NDK_VERSION

    defaultConfig {
        applicationId = "sh.zeron.android"
        minSdk = 29
        targetSdk = 37
        versionCode = buildNumber
        versionName = workspaceVersion
        // The ABIs the core was built for (`ZEREN_ANDROID_ABIS`): a dependency's
        // native libraries for others would only add weight.
        ndk { abiFilters += (System.getenv("ZEREN_ANDROID_ABIS") ?: "arm64-v8a x86_64").split(" ").filter { it.isNotBlank() } }
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("zeren") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("ZEREN_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ZEREN_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ZEREN_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("zeren") ?: signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    sourceSets["main"].apply {
        kotlin.directories.add(coreOut.resolve("kotlin").path)
        jniLibs.directories.add(coreOut.resolve("jniLibs").path)
        // The exact font bytes the Rust layout engine measures.
        assets.directories.add(repoRoot.resolve("apps/ios/Zeren/Fonts").path)
        assets.directories.add(iconsOut.get().asFile.resolve("assets").path)
        res.directories.add(iconsOut.get().asFile.resolve("res").path)
    }

    // Compressed: the APK is downloaded from zeron.sh, not delivered by Play
    // (which compresses for transfer itself); about a third of the size.
    packaging {
        jniLibs { useLegacyPackaging = true }
        dex { useLegacyPackaging = true }
    }
}

tasks.named("preBuild") { dependsOn(buildCore, genIcons) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.browser)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
    implementation(libs.webrtc)
    debugImplementation(libs.compose.ui.tooling)
}

kotlin {
    compilerOptions {
        optIn.addAll(
            "androidx.compose.material3.ExperimentalMaterial3Api",
            "androidx.compose.material3.ExperimentalMaterial3ExpressiveApi",
        )
    }
}

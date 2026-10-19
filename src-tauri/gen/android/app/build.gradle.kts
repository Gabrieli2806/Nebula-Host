import java.util.Properties

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

android {
    compileSdk = 36
    namespace = "net.g2806.nebulahost"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "net.g2806.nebulahost"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    packaging {
        // The launcher is run from the library folder, so libraries must be
        // unpacked there rather than mapped out of the APK.
        jniLibs {
            useLegacyPackaging = true
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

// -- Java launcher ----------------------------------------------------------
// The one executable the package carries: it starts the Java runtime that
// ships in the assets (see src-tauri/android/launcher/nebula_java.c). Built
// with the NDK's clang for each ABI and packaged as a native library, the only
// kind of file Android lets an app execute.
val launcherSource = file("../../../android/launcher/nebula_java.c")
val launcherAbis = mapOf("arm64-v8a" to "aarch64-linux-android", "x86_64" to "x86_64-linux-android")

val buildNebulaLauncher = tasks.register("buildNebulaLauncher") {
    inputs.file(launcherSource)
    val outDir = file("src/main/jniLibs")
    outputs.dir(outDir)
    doLast {
        val ndk = android.ndkDirectory
        val host = File(ndk, "toolchains/llvm/prebuilt").listFiles()!!.first()
        val windows = System.getProperty("os.name").lowercase().contains("windows")
        for ((abi, triple) in launcherAbis) {
            val clang = File(host, "bin/${triple}24-clang" + if (windows) ".cmd" else "")
            val target = File(outDir, "$abi/libnebula_java.so")
            target.parentFile.mkdirs()
            ProcessBuilder(clang.absolutePath, "-O2", "-fPIE", "-pie", "-o", target.absolutePath, launcherSource.absolutePath, "-ldl")
                .inheritIO().start().waitFor().let { code -> if (code != 0) throw GradleException("launcher build failed for $abi") }
        }
    }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(buildNebulaLauncher) }

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

apply(from = "tauri.build.gradle.kts")
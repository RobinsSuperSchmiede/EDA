import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Release signing: keystore.properties (local) or environment variables (CI). Without either,
// the release build is unsigned (fine for testing; Play needs a signed upload).
val keystore = Properties().apply {
    val f = rootProject.file("keystore.properties")
    if (f.exists()) f.inputStream().use { load(it) }
}
fun secret(key: String, env: String): String? = keystore.getProperty(key)?.trim() ?: System.getenv(env)

android {
    namespace = "dev.openshell.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.openshell.android"
        minSdk = 26
        // Google Play requires a recent target API level for new apps and updates
        targetSdk = 36
        versionCode = (System.getenv("VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("VERSION_NAME") ?: "0.3.0"
        ndk {
            // the Rust library is built for these ABIs (cargo ndk -t arm64-v8a -t x86_64)
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    signingConfigs {
        create("release") {
            val store = secret("storeFile", "ANDROID_KEYSTORE_PATH")
            if (store != null) {
                storeFile = file(store)
                storePassword = secret("storePassword", "ANDROID_KEYSTORE_PASSWORD")
                keyAlias = secret("keyAlias", "ANDROID_KEY_ALIAS")
                keyPassword = secret("keyPassword", "ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            val release = signingConfigs.getByName("release")
            if (release.storeFile != null) signingConfig = release
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            // native libraries are extracted to nativeLibraryDir: bundled runtimes (libnode.so,
            // libpython.so) must exist as files to be executed
            useLegacyPackaging = true
        }
    }

    bundle {
        abi { enableSplit = true }
        language { enableSplit = true }
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

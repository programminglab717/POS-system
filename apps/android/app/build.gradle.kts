// The register app (ADR-0027): Compose screens over the register's state, the device's keys in the
// Android Keystore, and the kernel.

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.keel.android"
    compileSdk = 37
    ndkVersion = "30.0.16248370"
    defaultConfig {
        applicationId = "dev.keel.register"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // Each test starts as a fresh install: the orchestrator clears the app's data before it.
        testInstrumentationRunnerArguments["clearPackageData"] = "true"
        // The ABIs the kernel is built for, as `:kernel` builds them: JNA brings others, where
        // the app would install and then fail to load the kernel.
        ndk {
            abiFilters += providers.gradleProperty("keelAbis").getOrElse("arm64-v8a,x86_64").split(",")
        }
    }
    buildTypes {
        release {
            // Shrinking comes with the release build's own checks; until then, none.
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    testOptions {
        execution = "ANDROIDX_TEST_ORCHESTRATOR"
    }
    buildFeatures {
        compose = true
    }
    packaging {
        // The kernel's libraries are stored uncompressed and page-aligned, so they load in place.
        jniLibs.useLegacyPackaging = false
    }
}

dependencies {
    implementation(project(":kernel"))
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.viewmodel.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.kotlinx.coroutines.core)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.kotlin.test.junit)
    androidTestUtil(libs.androidx.test.orchestrator)
    debugImplementation(libs.compose.ui.test.manifest)
}
